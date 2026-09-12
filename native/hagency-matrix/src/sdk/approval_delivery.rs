use super::{Owner, Sdk};
use crate::{
    Error,
    approval_delivery::state::{self, Attempt, Frozen, Phase, View},
    outgoing::state as wire,
};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
pub(crate) enum Command {
    Read,
    Start(Box<Frozen>),
    Query,
    Encrypt(Value),
    Possible(usize),
    Accept(usize, Value),
    Settle,
    #[cfg(test)]
    Hold(ReplyHold),
    #[cfg(test)]
    Corrupt(u8),
}
#[cfg(test)]
pub(crate) struct ReplyHold {
    pub phase: u8,
    pub reached: oneshot::Sender<()>,
    pub release: oneshot::Receiver<()>,
    pub lose: bool,
}
pub(crate) struct Handle {
    tx: mpsc::Sender<super::Command>,
    timeout: std::time::Duration,
}
pub(crate) struct Acceptance {
    permit: mpsc::OwnedPermit<super::Command>,
    timeout: std::time::Duration,
}
impl Owner {
    pub(crate) fn approval_delivery_handle(&self) -> Handle {
        Handle {
            tx: self.tx.clone(),
            timeout: self.timeout,
        }
    }
}
impl Handle {
    pub(crate) async fn command(&self, command: Command) -> Result<View, Error> {
        match &command {
            Command::Start(c) => state::size(c, state::MAX_START)?,
            Command::Encrypt(v) => {
                wire::encode(v, wire::MAX_QUERY)?;
            }
            Command::Accept(_, v) => {
                wire::encode(v, 4096)?;
            }
            _ => {}
        }
        let (send, receive) = oneshot::channel();
        self.tx
            .try_send(super::Command::ApprovalDelivery(command, send))
            .map_err(|_| Error::Busy)?;
        reply(self.timeout, receive).await
    }
    pub(crate) fn acceptance(&self) -> Result<Acceptance, Error> {
        Ok(Acceptance {
            permit: self
                .tx
                .clone()
                .try_reserve_owned()
                .map_err(|_| Error::Busy)?,
            timeout: self.timeout,
        })
    }
}
impl Acceptance {
    pub(crate) async fn accept(self, index: usize, value: Value) -> Result<View, Error> {
        wire::encode(&value, 4096)?;
        let (send, receive) = oneshot::channel();
        self.permit.send(super::Command::ApprovalDelivery(
            Command::Accept(index, value),
            send,
        ));
        reply(self.timeout, receive).await
    }
}
async fn reply(
    timeout: std::time::Duration,
    receive: oneshot::Receiver<Result<View, Error>>,
) -> Result<View, Error> {
    tokio::time::timeout(timeout, receive)
        .await
        .map_err(|_| Error::OutcomeUnknown)?
        .map_err(|_| Error::OutcomeUnknown)?
}
pub(super) fn validate(
    journal: &super::Journal,
    approval: bool,
    identity: &str,
    user: &str,
    device: &str,
) -> Result<(), Error> {
    if !approval
        && (journal.approval_delivery.is_some() || !journal.approval_delivery_receipts.is_empty())
    {
        return Err(Error::Storage);
    }
    if journal.approval_delivery_receipts.len() > state::MAX_RECEIPTS {
        return Err(Error::Storage);
    }
    let mut ids = std::collections::BTreeSet::new();
    for receipt in &journal.approval_delivery_receipts {
        receipt.validate()?;
        if !ids.insert(&receipt.request_id) {
            return Err(Error::Storage);
        }
    }
    if let Some(attempt) = &journal.approval_delivery {
        attempt.validate(identity, user, device)?;
        if ids.contains(&attempt.card.target.0.request_id) {
            return Err(Error::Storage);
        }
    }
    Ok(())
}
impl Sdk {
    pub(super) async fn approval_delivery(&mut self, command: Command) -> Result<View, Error> {
        if !self.approval {
            return Err(Error::Generation);
        }
        #[cfg(test)]
        if let Command::Hold(hold) = command {
            if self.delivery_hold.is_some() {
                return Err(Error::Busy);
            }
            self.delivery_hold = Some(hold);
            return self.delivery_view();
        }
        #[cfg(test)]
        if let Command::Corrupt(variant) = command {
            self.corrupt_card(variant).await?;
            return self.delivery_view();
        }
        if self.delivery_poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if !matches!(command, Command::Read | Command::Settle) {
            if self.approval_poisoned || self.enrollment_poisoned || self.journal.approval.is_some()
            {
                return Err(Error::OutcomeUnknown);
            }
            if self.enrollment_profile.is_none() {
                return Err(Error::Config);
            }
            let enrollment = self
                .enrollment
                .as_ref()
                .filter(|r| r.phase == crate::enrollment::state::Phase::Complete)
                .ok_or(Error::OutcomeUnknown)?;
            if Some(&enrollment.context.profile) != self.enrollment_profile.as_ref() {
                return Err(Error::Conflict);
            }
            let guard = self.client.olm_machine().await;
            let machine = guard.as_ref().ok_or(Error::Storage)?;
            super::enrollment::completed(machine, enrollment).await?;
        }
        match command {
            Command::Read => {}
            Command::Start(card) => {
                if self.journal.approval_delivery.is_some() {
                    return Err(Error::OutcomeUnknown);
                }
                if self.journal.approval_delivery_receipts.len() >= state::MAX_RECEIPTS {
                    return Err(Error::Capacity);
                }
                if self
                    .journal
                    .approval_delivery_receipts
                    .iter()
                    .any(|r| r.request_id == card.target.0.request_id)
                {
                    return Err(Error::Conflict);
                }
                self.journal.approval_delivery = Some(Attempt::new(*card, self.identity.clone()));
                self.persist_delivery().await?;
            }
            Command::Query => {
                let a = self
                    .journal
                    .approval_delivery
                    .as_ref()
                    .filter(|a| a.phase == Phase::Prepared)
                    .ok_or(Error::OutcomeUnknown)?;
                let users = a
                    .card
                    .users()
                    .into_iter()
                    .map(|u| u.try_into().map_err(|_| Error::Storage))
                    .collect::<Result<Vec<ruma::OwnedUserId>, _>>()?;
                let guard = self.client.olm_machine().await;
                let machine = guard.as_ref().ok_or(Error::Storage)?;
                let (id, _) = machine.query_keys_for_users(users.iter().map(|u| u.as_ref()));
                drop(guard);
                let a = self
                    .journal
                    .approval_delivery
                    .as_mut()
                    .ok_or(Error::Storage)?;
                a.query_id = Some(id.to_string());
                a.query_body = Some(wire::encode(&a.query(), wire::MAX_QUERY)?);
                a.phase = Phase::QueryPrepared;
                self.persist_delivery().await?;
            }
            Command::Encrypt(value) => {
                wire::encode(&value, wire::MAX_QUERY)?;
                let a = self
                    .journal
                    .approval_delivery
                    .as_mut()
                    .filter(|a| a.phase == Phase::QueryPrepared)
                    .ok_or(Error::OutcomeUnknown)?;
                a.query_response = Some(value);
                a.phase = Phase::CryptoApplying;
                self.persist_delivery().await?;
                if let Err(error) = self.encrypt_card().await {
                    self.journal
                        .approval_delivery
                        .as_mut()
                        .ok_or(Error::Storage)?
                        .phase = Phase::Quarantined;
                    self.persist_delivery().await?;
                    return Err(error);
                }
                self.persist_delivery().await?;
            }
            Command::Possible(index) => {
                let a = self
                    .journal
                    .approval_delivery
                    .as_mut()
                    .filter(|a| {
                        a.phase == Phase::Ready && a.index == index && index < a.writes.len()
                    })
                    .ok_or(Error::OutcomeUnknown)?;
                a.phase = Phase::WritePossible;
                self.persist_delivery().await?;
            }
            Command::Accept(index, value) => {
                let a = self
                    .journal
                    .approval_delivery
                    .as_mut()
                    .filter(|a| a.phase == Phase::WritePossible && a.index == index)
                    .ok_or(Error::OutcomeUnknown)?;
                let w = a.writes.get_mut(index).ok_or(Error::Storage)?;
                state::validate_response(w.room, &value)?;
                w.response = Some(value);
                a.phase = if w.room {
                    Phase::Complete
                } else {
                    Phase::ResponseStored
                };
                self.persist_delivery().await?;
                if self
                    .journal
                    .approval_delivery
                    .as_ref()
                    .is_some_and(|a| a.phase == Phase::ResponseStored)
                {
                    let a = self
                        .journal
                        .approval_delivery
                        .as_ref()
                        .ok_or(Error::Storage)?;
                    let guard = self.client.olm_machine().await;
                    let machine = guard.as_ref().ok_or(Error::Storage)?;
                    machine
                        .mark_request_as_sent(
                            a.writes[index].transaction_id.as_str().into(),
                            &ruma::api::client::to_device::send_event_to_device::v3::Response::new(
                            ),
                        )
                        .await
                        .map_err(|_| Error::OutcomeUnknown)?;
                    drop(guard);
                    let a = self
                        .journal
                        .approval_delivery
                        .as_mut()
                        .ok_or(Error::Storage)?;
                    a.index += 1;
                    a.phase = Phase::Ready;
                    self.persist_delivery().await?;
                }
            }
            Command::Settle => {
                let a = self
                    .journal
                    .approval_delivery
                    .as_ref()
                    .ok_or(Error::Storage)?;
                let receipt = a.receipt()?;
                if self.journal.approval_delivery_receipts.len() >= state::MAX_RECEIPTS {
                    return Err(Error::Capacity);
                }
                let original = self.journal.approval_delivery.take();
                self.journal.approval_delivery_receipts.push(receipt);
                if let Err(error) = self.persist_delivery().await {
                    self.journal.approval_delivery_receipts.pop();
                    self.journal.approval_delivery = original;
                    return Err(error);
                }
            }
            #[cfg(test)]
            Command::Hold(_) | Command::Corrupt(_) => unreachable!(),
        }
        self.delivery_view()
    }
    fn delivery_view(&self) -> Result<View, Error> {
        Ok(View {
            attempt: self.journal.approval_delivery.clone(),
            receipts: self.journal.approval_delivery_receipts.clone(),
        })
    }
    async fn persist_delivery(&mut self) -> Result<(), Error> {
        let result = async {
            let guard = self.client.olm_machine().await;
            let machine = guard.as_ref().ok_or(Error::Storage)?;
            validate(
                &self.journal,
                self.approval,
                &self.identity,
                machine.user_id().as_str(),
                machine.device_id().as_str(),
            )?;
            drop(guard);
            self.persist().await
        }
        .await;
        if result.is_err() {
            self.delivery_poisoned = true;
        }
        result
    }
    async fn encrypt_card(&mut self) -> Result<(), Error> {
        let a = self
            .journal
            .approval_delivery
            .as_ref()
            .ok_or(Error::Storage)?;
        let users = a
            .card
            .users()
            .into_iter()
            .map(|u| u.try_into().map_err(|_| Error::Storage))
            .collect::<Result<Vec<ruma::OwnedUserId>, _>>()?;
        let guard = self.client.olm_machine().await;
        let machine = guard.as_ref().ok_or(Error::Storage)?;
        let response = a.query_response.as_ref().ok_or(Error::Storage)?;
        let writes = super::encrypted_message::prepare(
            machine,
            super::encrypted_message::Input {
                users: &users,
                room: a
                    .card
                    .target
                    .0
                    .authority
                    .room_id
                    .as_str()
                    .try_into()
                    .map_err(|_| Error::Storage)?,
                transaction_id: &a.card.transaction(),
                content: &a.card.content,
                query_id: a.query_id.as_deref().ok_or(Error::Storage)?,
                response,
            },
        )
        .await?;
        let keys = wire::hash(wire::encode(response, wire::MAX_QUERY)?.as_bytes());
        drop(guard);
        let a = self
            .journal
            .approval_delivery
            .as_mut()
            .ok_or(Error::Storage)?;
        a.writes = writes;
        a.keys_digest = Some(keys);
        a.phase = Phase::Ready;
        Ok(())
    }
    #[cfg(test)]
    async fn corrupt_card(&mut self, variant: u8) -> Result<(), Error> {
        if variant == 100 {
            // Capacity fixture only: keep the one actual receipt, then fill the
            // remaining validated metadata slots. These are not network sends.
            let first = self
                .journal
                .approval_delivery_receipts
                .first()
                .ok_or(Error::Storage)?
                .clone();
            for index in 1..state::MAX_RECEIPTS {
                let mut r = first.clone();
                r.request_id = format!("approval_{index:040x}");
                self.journal.approval_delivery_receipts.push(r);
            }
            return self.persist_delivery().await;
        }
        let mut journal = serde_json::to_value(&self.journal).map_err(|_| Error::Storage)?;
        let a = &mut journal["approval_delivery"];
        match variant {
            1 => a["identity"] = serde_json::json!("foreign SDK"),
            2 => {
                a["card"]["target"]["authority"]["owner_mxid"] =
                    serde_json::json!("@other:example.test")
            }
            3 => a["card"]["cutoff"] = serde_json::json!(hagency_core::JSON_SAFE_MAX),
            4 => a["writes"][0]["response"] = Value::Null,
            5 => a["writes"][1]["transaction_id"] = serde_json::json!("replacement-transaction"),
            6 => {
                let mut body: Value =
                    serde_json::from_str(a["writes"][0]["body"].as_str().ok_or(Error::Storage)?)
                        .map_err(|_| Error::Storage)?;
                let devices = body["messages"]
                    .as_object_mut()
                    .ok_or(Error::Storage)?
                    .values_mut()
                    .next()
                    .ok_or(Error::Storage)?
                    .as_object_mut()
                    .ok_or(Error::Storage)?;
                let old = devices.remove("HUMAN").ok_or(Error::Storage)?;
                devices.insert("WRONG_DEVICE".into(), old);
                let bytes = wire::encode(&body, wire::MAX_QUERY)?;
                a["writes"][0]["digest"] = serde_json::json!(wire::hash(bytes.as_bytes()));
                a["writes"][0]["body"] = serde_json::json!(bytes);
            }
            7 => {
                a["phase"] = serde_json::json!("Complete");
                a["index"] = serde_json::json!(0);
            }
            8 => {
                journal["outgoing_receipts"] = serde_json::json!([{"kind":"Final","id":"foreign","fence":1,"attempt_digest":"a".repeat(64)}])
            }
            _ => return Err(Error::Config),
        }
        let bytes = self
            .cipher
            .encrypt_value(&journal)
            .map_err(|_| Error::Storage)?;
        self.client
            .state_store()
            .set_custom_value(super::JOURNAL, bytes)
            .await
            .map_err(|_| Error::Storage)?;
        Ok(())
    }
}

#[cfg(test)]
impl Owner {
    pub(crate) async fn approval_close_fault(&self) {
        let (send, reply) = oneshot::channel();
        self.tx.try_send(super::Command::CloseFault(send)).unwrap();
        reply.await.unwrap();
    }
}
