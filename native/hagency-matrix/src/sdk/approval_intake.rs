use super::{Journal, Owner, Sdk};
use crate::{
    Error,
    approval_batch::{self as state, Command, Phase, Tombstone, View},
};
use ruma::{OwnedUserId, api::IncomingResponse};
use serde_json::json;
use std::collections::BTreeSet;
use tokio::sync::oneshot;
pub(super) fn validate_journal(
    journal: &Journal,
    approval: bool,
    identity: &str,
    user: &str,
    device: &str,
) -> Result<(), Error> {
    if !approval {
        return if journal.approval.is_some()
            || !journal.approval_receipts.is_empty()
            || !journal.approval_outcomes.is_empty()
        {
            Err(Error::Storage)
        } else {
            Ok(())
        };
    }
    if journal.pending.is_some()
        || !journal.receipts.is_empty()
        || journal.intake.is_some()
        || journal.intake_enabled
        || !journal.intake_receipts.is_empty()
        || journal.uploads.is_some()
        || !journal.attachments.is_empty()
        || journal.outgoing.is_some()
        || !journal.outgoing_receipts.is_empty()
        || journal.approval_receipts.len() > state::MAX_BATCHES
        || journal.approval_outcomes.len() > state::MAX_OUTCOMES
    {
        return Err(Error::Storage);
    }
    let mut seen = BTreeSet::new();
    for r in &journal.approval_receipts {
        if r.token.is_empty()
            || r.token.len() > 4096
            || !seen.insert(&r.token)
            || !crate::outgoing::state::digest(&r.digest)
            || !crate::outgoing::state::digest(&r.targets_digest)
        {
            return Err(Error::Storage);
        }
    }
    let mut seen = BTreeSet::new();
    for e in &journal.approval_outcomes {
        if !seen.insert(&e.source)
            || !crate::outgoing::state::digest(&e.source)
            || !crate::outgoing::state::digest(&e.digest)
            || matches!(&e.outcome,state::Outcome::Accepted{request_id,..} if request_id.strip_prefix("approval_").is_none_or(|s|s.len()!=40 || !s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))))
        {
            return Err(Error::Storage);
        }
    }
    if let Some(b) = &journal.approval {
        b.validate(identity, user, device)?;
        if journal.approval_receipts.iter().any(|r| r.token == b.token) {
            return Err(Error::Storage);
        }
        for e in &b.events {
            if let Some(outcome) = e.replay()
                && !journal.approval_outcomes.iter().any(|old| {
                    old.source == e.source && old.digest == e.digest && old.outcome == *outcome
                })
            {
                return Err(Error::Storage);
            }
        }
        for (index, ack) in b.acknowledgements.iter().enumerate() {
            let e = &b.events[index];
            if !journal
                .approval_outcomes
                .iter()
                .any(|r| r.source == e.source && r.digest == e.digest && r.outcome == *ack)
            {
                return Err(Error::Storage);
            }
        }
    }
    Ok(())
}
pub(super) fn validate_cursor(journal: &Journal, cursor: Option<&str>) -> Result<(), Error> {
    let prior = journal.approval_receipts.last().map(|r| r.token.as_str());
    let valid = match &journal.approval {
        None => cursor == prior,
        Some(b) => match b.phase {
            Phase::Prepared => cursor == prior,
            Phase::Applying | Phase::Quarantined => {
                cursor == prior || cursor == Some(b.token.as_str())
            }
            Phase::Derived => cursor == Some(b.token.as_str()),
        },
    };
    if valid { Ok(()) } else { Err(Error::Storage) }
}
impl Sdk {
    pub(super) async fn approval_query(&self, users: Vec<String>) -> Result<String, Error> {
        if !self.approval
            || self.approval_poisoned
            || self.delivery_poisoned
            || self.journal.approval_delivery.is_some()
            || self.enrollment_poisoned
            || self
                .enrollment
                .as_ref()
                .is_some_and(|r| r.phase != crate::enrollment::state::Phase::Complete)
            || self.journal.approval.is_some()
            || users.is_empty()
            || users.len() > 17
        {
            return Err(Error::OutcomeUnknown);
        }
        let users: Vec<OwnedUserId> = users
            .into_iter()
            .map(|u| u.parse().map_err(|_| Error::Wire))
            .collect::<Result<_, _>>()?;
        let guard = self.client.olm_machine().await;
        let machine = guard.as_ref().ok_or(Error::Storage)?;
        if !users.iter().any(|u| u == machine.user_id()) {
            return Err(Error::Identity);
        }
        Ok(machine
            .query_keys_for_users(users.iter().map(|u| u.as_ref()))
            .0
            .to_string())
    }
    pub(super) async fn approval(&mut self, command: Command) -> Result<View, Error> {
        if !self.approval {
            return Err(Error::Generation);
        }
        if self.approval_poisoned
            || self.delivery_poisoned
            || (self.journal.approval_delivery.is_some() && !matches!(command, Command::Read))
            || self.enrollment_poisoned
            || self
                .enrollment
                .as_ref()
                .is_some_and(|r| r.phase != crate::enrollment::state::Phase::Complete)
        {
            return Err(Error::OutcomeUnknown);
        }
        match command {
            Command::Read => {}
            Command::Start(mut b) => {
                if self.journal.approval.is_some() {
                    return Err(Error::OutcomeUnknown);
                }
                if let Some(r) = self
                    .journal
                    .approval_receipts
                    .iter()
                    .find(|r| r.token == b.token)
                {
                    if r.digest != b.digest {
                        return Err(Error::Conflict);
                    }
                    return self.approval_view();
                }
                if self.journal.approval_receipts.len() >= state::MAX_BATCHES {
                    return Err(Error::Capacity);
                }
                let count = b
                    .raw
                    .get("rooms")
                    .and_then(|r| r.get("join"))
                    .and_then(|v| v.as_object())
                    .into_iter()
                    .flat_map(|r| r.values())
                    .map(|r| r["timeline"]["events"].as_array().map_or(0, Vec::len))
                    .sum::<usize>();
                if count > state::MAX_EVENTS
                    || self.journal.approval_outcomes.len() + count > state::MAX_OUTCOMES
                {
                    return Err(Error::Capacity);
                }
                b.identity = self.identity.clone();
                self.journal.approval = Some(*b);
                self.persist_approval().await?;
            }
            Command::Apply => {
                let b = self.journal.approval.as_mut().ok_or(Error::Storage)?;
                if b.phase != Phase::Prepared {
                    return Err(Error::OutcomeUnknown);
                }
                b.phase = Phase::Applying;
                self.persist_approval().await?;
                let b = self.journal.approval.as_ref().ok_or(Error::Storage)?;
                let users: BTreeSet<String> = b
                    .rooms
                    .iter()
                    .flat_map(|r| [r.authority.owner_mxid.clone(), r.authority.bot_mxid.clone()])
                    .collect();
                let users: Vec<OwnedUserId> = users
                    .into_iter()
                    .map(|u| u.parse().map_err(|_| Error::Wire))
                    .collect::<Result<_, _>>()?;
                let guard = self.client.olm_machine().await;
                let machine = guard.as_ref().ok_or(Error::Storage)?;
                let devices = super::keys::accept(machine, &users, &b.query_id, &b.keys).await?;
                drop(guard);
                let response =
                    ruma::api::client::sync::sync_events::v3::Response::try_from_http_response(
                        http::Response::new(serde_json::to_vec(&b.raw).map_err(|_| Error::Wire)?),
                    )
                    .map_err(|_| Error::Wire)?;
                let processed = self
                    .client
                    .receive_sync_response(response)
                    .await
                    .map_err(|_| Error::OutcomeUnknown)?;
                #[cfg(test)]
                if std::mem::take(&mut self.apply_fault) {
                    return Err(Error::OutcomeUnknown);
                }
                if let Err(error) = self
                    .journal
                    .approval
                    .as_mut()
                    .ok_or(Error::Storage)?
                    .derive(processed, &devices, &self.journal.approval_outcomes)
                {
                    self.journal.approval.as_mut().ok_or(Error::Storage)?.phase =
                        Phase::Quarantined;
                    self.persist_approval().await?;
                    return Err(if matches!(error, Error::Wire) {
                        Error::Unsupported
                    } else {
                        error
                    });
                }
                self.persist_approval().await?;
            }
            Command::Ack(index, outcome) => {
                let b = self.journal.approval.as_mut().ok_or(Error::Storage)?;
                if b.phase != Phase::Derived || index != b.acknowledgements.len() {
                    return Err(Error::Conflict);
                }
                let e = b.events.get(index).ok_or(Error::Conflict)?;
                if let Some(old) = self
                    .journal
                    .approval_outcomes
                    .iter()
                    .find(|r| r.source == e.source)
                {
                    if old.digest != e.digest || old.outcome != outcome {
                        return Err(Error::Conflict);
                    }
                } else {
                    if !e.permits(&outcome) {
                        return Err(Error::Conflict);
                    }
                    if self.journal.approval_outcomes.len() >= state::MAX_OUTCOMES {
                        return Err(Error::Capacity);
                    }
                    self.journal.approval_outcomes.push(Tombstone {
                        source: e.source.clone(),
                        digest: e.digest.clone(),
                        outcome: outcome.clone(),
                    });
                }
                b.acknowledgements.push(outcome);
                self.persist_approval().await?;
            }
            Command::Finish => {
                let receipt = self
                    .journal
                    .approval
                    .as_ref()
                    .ok_or(Error::Storage)?
                    .receipt()?;
                if self.journal.approval_receipts.len() >= state::MAX_BATCHES {
                    return Err(Error::Capacity);
                }
                self.journal.approval_receipts.push(receipt);
                self.journal.approval = None;
                self.persist_approval().await?;
            }
            Command::Quarantine => {
                let b = self.journal.approval.as_mut().ok_or(Error::Storage)?;
                // Retain already derived proof/ack data for inspection; never delete it.
                b.phase = Phase::Quarantined;
                self.persist_approval().await?;
            }
        }
        self.approval_view()
    }
    fn approval_view(&self) -> Result<View, Error> {
        Ok(View {
            batch: self.journal.approval.clone(),
            receipts: self.journal.approval_receipts.clone(),
            outcomes: self.journal.approval_outcomes.clone(),
        })
    }
    async fn persist_approval(&mut self) -> Result<(), Error> {
        let guard = self.client.olm_machine().await;
        let machine = guard.as_ref().ok_or(Error::Storage)?;
        let result = validate_journal(
            &self.journal,
            true,
            &self.identity,
            machine.user_id().as_str(),
            machine.device_id().as_str(),
        );
        drop(guard);
        if let Err(e) = result {
            self.approval_poisoned = true;
            return Err(e);
        }
        if let Err(e) = self.persist().await {
            self.approval_poisoned = true;
            return Err(e);
        }
        Ok(())
    }
}
impl Owner {
    pub(crate) async fn approval(&self, command: Command) -> Result<View, Error> {
        if let Command::Start(b) = &command {
            crate::outgoing::state::encode(
                &serde_json::to_value(b).map_err(|_| Error::Storage)?,
                3 * 1024 * 1024,
            )?;
        }
        let (send, reply) = oneshot::channel();
        self.tx
            .try_send(super::Command::Approval(command, send))
            .map_err(|_| Error::Busy)?;
        tokio::time::timeout(self.timeout, reply)
            .await
            .map_err(|_| Error::OutcomeUnknown)?
            .map_err(|_| Error::OutcomeUnknown)?
    }
    pub(crate) async fn approval_query(
        &self,
        users: Vec<String>,
    ) -> Result<(String, String), Error> {
        if users.len() > 17 || users.iter().any(|u| u.len() > 512) {
            return Err(Error::Capacity);
        }
        let body = crate::outgoing::state::encode(
            &json!({"device_keys":users.iter().map(|u|(u.clone(),json!([]))).collect::<serde_json::Map<_,_>>()}),
            256 * 1024,
        )?;
        let (send, reply) = oneshot::channel();
        self.tx
            .try_send(super::Command::ApprovalQuery(users, send))
            .map_err(|_| Error::Busy)?;
        let id = tokio::time::timeout(self.timeout, reply)
            .await
            .map_err(|_| Error::OutcomeUnknown)?
            .map_err(|_| Error::OutcomeUnknown)??;
        Ok((id, body))
    }
}
