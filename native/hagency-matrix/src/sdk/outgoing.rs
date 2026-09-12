use super::Sdk;
use crate::{
    Error,
    outgoing::state::{self, Attempt, Command, Phase, View, Write},
};
use ruma::{OwnedUserId, api::client::to_device::send_event_to_device};
use serde_json::{Value, json};

impl Sdk {
    pub(super) async fn outgoing(&mut self, command: Command) -> Result<View, Error> {
        if self.approval {
            return Err(Error::Generation);
        }
        if self.outgoing_poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let command = match command {
            Command::StartFile(start) => Command::Start(Box::new(self.start_file(*start)?)),
            other => other,
        };
        match command {
            Command::StartFile(_) => unreachable!(),
            Command::Read => {}
            Command::Start(mut attempt) => {
                if self.journal.outgoing.is_some() {
                    return Err(Error::OutcomeUnknown);
                }
                if self.journal.outgoing_receipts.len() >= state::MAX_RECEIPTS {
                    return Err(Error::Capacity);
                }
                if self.journal.outgoing_receipts.iter().any(|r| {
                    r.kind == attempt.kind && r.id == attempt.id && r.fence == attempt.fence
                }) {
                    return Err(Error::Conflict);
                }
                attempt.identity = self.identity.clone();
                let guard = self.client.olm_machine().await;
                let machine = guard.as_ref().ok_or(Error::Storage)?;
                attempt.validate(
                    &self.identity,
                    machine.user_id().as_str(),
                    machine.device_id().as_str(),
                )?;
                super::file_publication::validate_attempt(
                    &attempt,
                    self.uploads.as_ref(),
                    &self.upload_context,
                )?;
                drop(guard);
                self.journal.outgoing = Some(*attempt);
                self.persist_outgoing().await?;
            }
            Command::Begun => {
                let attempt = self.journal.outgoing.as_mut().ok_or(Error::Storage)?;
                if attempt.phase != Phase::BeforeBegin {
                    return Err(Error::OutcomeUnknown);
                }
                if !attempt.route.encrypted {
                    attempt.writes.push(Write::new(
                        "m.room.message".into(),
                        attempt.transaction_id.clone(),
                        attempt.content.clone(),
                        true,
                    )?);
                }
                attempt.phase = Phase::Ready;
                self.persist_outgoing().await?;
            }
            Command::Query => {
                let attempt = self.journal.outgoing.as_ref().ok_or(Error::Storage)?;
                if attempt.phase != Phase::Ready
                    || !attempt.route.encrypted
                    || !attempt.writes.is_empty()
                {
                    return Err(Error::OutcomeUnknown);
                }
                let users = users(attempt)?;
                let guard = self.client.olm_machine().await;
                let machine = guard.as_ref().ok_or(Error::Storage)?;
                let (id, _) = machine.query_keys_for_users(users.iter().map(|u| u.as_ref()));
                let body = json!({"device_keys": users.iter().map(|u|(u.to_string(),json!([]))).collect::<serde_json::Map<_,_>>()});
                drop(guard);
                let attempt = self.journal.outgoing.as_mut().ok_or(Error::Storage)?;
                attempt.query_id = Some(id.to_string());
                attempt.query_body = Some(state::encode(&body, state::MAX_QUERY)?);
                attempt.phase = Phase::QueryPrepared;
                self.persist_outgoing().await?;
            }
            Command::Encrypt(response) => {
                state::encode(&response, state::MAX_QUERY)?;
                let attempt = self.journal.outgoing.as_mut().ok_or(Error::Storage)?;
                if attempt.phase != Phase::QueryPrepared {
                    return Err(Error::OutcomeUnknown);
                }
                attempt.query_response = Some(response);
                attempt.phase = Phase::CryptoApplying;
                self.persist_outgoing().await?;
                let result = self.encrypt_outgoing().await;
                if let Err(error) = result {
                    if error == Error::Unsupported
                        || error == Error::Capacity
                        || error == Error::Wire
                    {
                        self.journal.outgoing.as_mut().ok_or(Error::Storage)?.phase =
                            Phase::Quarantined;
                        self.persist_outgoing().await?;
                    }
                    return Err(error);
                }
                self.persist_outgoing().await?;
            }
            Command::Possible(index) => {
                let attempt = self.journal.outgoing.as_mut().ok_or(Error::Storage)?;
                if attempt.phase != Phase::Ready
                    || attempt.index != index
                    || index >= attempt.writes.len()
                {
                    return Err(Error::OutcomeUnknown);
                }
                attempt.phase = Phase::WritePossible;
                self.persist_outgoing().await?;
            }
            Command::Accept(index, response) => {
                state::encode(&response, 4096)?;
                let attempt = self.journal.outgoing.as_mut().ok_or(Error::Storage)?;
                if attempt.phase != Phase::WritePossible || attempt.index != index {
                    return Err(Error::OutcomeUnknown);
                }
                let write = attempt.writes.get_mut(index).ok_or(Error::Storage)?;
                if write.room {
                    let event = response
                        .get("event_id")
                        .and_then(Value::as_str)
                        .ok_or(Error::Wire)?;
                    hagency_core::replies::matrix_event(event).map_err(|_| Error::Wire)?;
                    if response.as_object().is_none_or(|o| o.len() != 1) {
                        return Err(Error::Wire);
                    }
                } else if response.as_object().is_none_or(|o| !o.is_empty()) {
                    return Err(Error::Wire);
                }
                write.response = Some(response);
                attempt.phase = if write.room {
                    Phase::Complete
                } else {
                    Phase::ResponseStored
                };
                self.persist_outgoing().await?;
                if self.journal.outgoing.as_ref().ok_or(Error::Storage)?.phase
                    == Phase::ResponseStored
                {
                    let attempt = self.journal.outgoing.as_ref().ok_or(Error::Storage)?;
                    let id = attempt.writes[index].transaction_id.as_str();
                    let guard = self.client.olm_machine().await;
                    let machine = guard.as_ref().ok_or(Error::Storage)?;
                    machine
                        .mark_request_as_sent(id.into(), &send_event_to_device::v3::Response::new())
                        .await
                        .map_err(|_| Error::OutcomeUnknown)?;
                    drop(guard);
                    let attempt = self.journal.outgoing.as_mut().ok_or(Error::Storage)?;
                    attempt.index += 1;
                    attempt.phase = Phase::Ready;
                    self.persist_outgoing().await?;
                }
            }
            Command::Settle => {
                let receipt = self
                    .journal
                    .outgoing
                    .as_ref()
                    .ok_or(Error::Storage)?
                    .receipt()?;
                if self.journal.outgoing_receipts.len() >= state::MAX_RECEIPTS {
                    return Err(Error::Capacity);
                }
                self.journal.outgoing_receipts.push(receipt);
                self.journal.outgoing = None;
                self.persist_outgoing().await?;
            }
        }
        Ok(View {
            attempt: self.journal.outgoing.clone(),
            receipts: self.journal.outgoing_receipts.clone(),
        })
    }
    async fn persist_outgoing(&mut self) -> Result<(), Error> {
        if let Some(attempt) = &self.journal.outgoing {
            super::file_publication::validate_attempt(
                attempt,
                self.uploads.as_ref(),
                &self.upload_context,
            )?;
            let guard = self.client.olm_machine().await;
            let machine = guard.as_ref().ok_or(Error::Storage)?;
            if let Err(error) = attempt.validate(
                &self.identity,
                machine.user_id().as_str(),
                machine.device_id().as_str(),
            ) {
                self.outgoing_poisoned = true;
                return Err(error);
            }
        }
        if let Err(error) = self.persist().await {
            self.outgoing_poisoned = true;
            return Err(error);
        }
        Ok(())
    }
    async fn encrypt_outgoing(&mut self) -> Result<(), Error> {
        let attempt = self.journal.outgoing.as_ref().ok_or(Error::Storage)?;
        let response = attempt.query_response.as_ref().ok_or(Error::Storage)?;
        let users = users(attempt)?;
        let guard = self.client.olm_machine().await;
        let machine = guard.as_ref().ok_or(Error::Storage)?;
        let room = attempt
            .route
            .room_id
            .as_str()
            .try_into()
            .map_err(|_| Error::Wire)?;
        let writes = super::encrypted_message::prepare(
            machine,
            super::encrypted_message::Input {
                users: &users,
                room,
                transaction_id: &attempt.transaction_id,
                content: &attempt.content,
                query_id: attempt.query_id.as_deref().ok_or(Error::Storage)?,
                response,
            },
        )
        .await?;
        let keys_digest = state::hash(state::encode(response, state::MAX_QUERY)?.as_bytes());
        drop(guard);
        let attempt = self.journal.outgoing.as_mut().ok_or(Error::Storage)?;
        attempt.keys_digest = Some(keys_digest);
        attempt.writes = writes;
        attempt.phase = Phase::Ready;
        Ok(())
    }
}
fn users(attempt: &Attempt) -> Result<Vec<OwnedUserId>, Error> {
    attempt
        .joined
        .iter()
        .map(|u| u.parse().map_err(|_| Error::Wire))
        .collect()
}
