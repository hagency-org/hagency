use super::{Owner, Sdk, upload_state as state};
use crate::{Error, UploadResponse};
use hagency_store::UploadSend;
use matrix_sdk_base::BaseClient;
use matrix_sdk_store_encryption::StoreCipher;
use state::{Context, Inspection, Ledger, LivePermit, Marker, Phase, Record, Reference, Response};
use std::sync::Arc;
use tokio::sync::oneshot;

pub(super) enum Command {
    Reserve(Box<UploadSend>),
    Possible(LivePermit),
    Accept(Reference, Response),
    Inspect(Reference),
    Restore(String),
    #[cfg(test)]
    Fixture(u8),
}
pub(super) enum Reply {
    Live(LivePermit),
    Inspection(Inspection),
    Reference(Reference),
    #[cfg(test)]
    Fixture,
}

pub(super) async fn load(
    client: &BaseClient,
    cipher: &StoreCipher,
    marker: Option<&Marker>,
    context: &Context,
) -> Result<Option<Ledger>, Error> {
    let bytes = client
        .state_store()
        .get_custom_value(state::KEY)
        .await
        .map_err(|_| Error::Storage)?;
    match (marker, bytes) {
        (None, None) => Ok(None),
        (Some(marker), Some(bytes)) if marker == &Marker::new(context) => {
            if bytes.len() > state::MAX_ENVELOPE {
                return Err(Error::Capacity);
            }
            let ledger: Ledger = cipher.decrypt_value(&bytes).map_err(|_| Error::Storage)?;
            ledger.validate(context)?;
            Ok(Some(ledger))
        }
        _ => Err(Error::Storage),
    }
}
impl Sdk {
    pub(super) async fn upload(&mut self, command: Command) -> Result<Reply, Error> {
        #[cfg(test)]
        if let Command::Fixture(variant) = command {
            super::upload_fixture::fixture(self, variant).await;
            return Ok(Reply::Fixture);
        }
        if self.approval {
            return Err(Error::Generation);
        }
        if self.upload_poisoned {
            return Err(Error::OutcomeUnknown);
        }
        match command {
            Command::Reserve(send) => {
                let reference = self.upload_context.reference(&send)?;
                let record = Record::reserved(&reference)?;
                if let Some(ledger) = &self.uploads {
                    if ledger.records.contains_key(&reference.0.upload_id) {
                        return Err(Error::Conflict);
                    }
                    if ledger.records.len() >= state::MAX_RECORDS {
                        return Err(Error::Capacity);
                    }
                }
                if self.uploads.is_none() {
                    self.journal.uploads = Some(Marker::new(&self.upload_context));
                    if let Err(error) = self.persist().await {
                        self.upload_poisoned = true;
                        return Err(error);
                    }
                    self.uploads = Some(Ledger::new(&self.upload_context));
                    self.persist_uploads().await?;
                }
                self.uploads
                    .as_mut()
                    .ok_or(Error::Storage)?
                    .records
                    .insert(reference.0.upload_id.clone(), record);
                self.persist_uploads().await?;
                Ok(Reply::Live(LivePermit {
                    reference,
                    owner: self.upload_epoch.clone(),
                }))
            }
            Command::Possible(permit) => {
                if !Arc::ptr_eq(&permit.owner, &self.upload_epoch) {
                    return Err(Error::Conflict);
                }
                let ledger = self.uploads.as_mut().ok_or(Error::OutcomeUnknown)?;
                if ledger.exact(&permit.reference)?.phase != Phase::Reserved {
                    return Err(Error::OutcomeUnknown);
                }
                ledger
                    .records
                    .get_mut(&permit.reference.0.upload_id)
                    .ok_or(Error::Storage)?
                    .phase = Phase::WritePossible;
                self.persist_uploads().await?;
                self.inspect_upload(&permit.reference)
            }
            Command::Accept(reference, response) => {
                let ledger = self.uploads.as_mut().ok_or(Error::OutcomeUnknown)?;
                let record = ledger.exact(&reference)?;
                if let Some(previous) = &record.response {
                    if !previous.same(&response) {
                        return Err(Error::Conflict);
                    }
                    return self.inspect_upload(&reference);
                }
                if record.phase != Phase::WritePossible {
                    return Err(Error::OutcomeUnknown);
                }
                let record = ledger
                    .records
                    .get_mut(&reference.0.upload_id)
                    .ok_or(Error::Storage)?;
                record.response = Some(response);
                record.phase = Phase::Accepted;
                // Failure retains this actual copied response and memory permit in
                // the poisoned owner. Explicit owner close can discard uncommitted bytes.
                self.persist_uploads().await?;
                self.uploads
                    .as_mut()
                    .ok_or(Error::Storage)?
                    .records
                    .get_mut(&reference.0.upload_id)
                    .and_then(|r| r.response.as_mut())
                    .ok_or(Error::Storage)?
                    .custody
                    .take();
                self.inspect_upload(&reference)
            }
            Command::Inspect(reference) => self.inspect_upload(&reference),
            Command::Restore(id) => Ok(Reply::Reference(
                self.uploads
                    .as_ref()
                    .ok_or(Error::OutcomeUnknown)?
                    .restore(&id, &self.upload_context)?,
            )),
            #[cfg(test)]
            Command::Fixture(_) => unreachable!(),
        }
    }
    fn inspect_upload(&self, reference: &Reference) -> Result<Reply, Error> {
        Ok(Reply::Inspection(
            self.uploads
                .as_ref()
                .ok_or(Error::OutcomeUnknown)?
                .exact(reference)?
                .inspection(),
        ))
    }
    async fn persist_uploads(&mut self) -> Result<(), Error> {
        let result = async {
            let ledger = self.uploads.as_ref().ok_or(Error::Storage)?;
            ledger.validate(&self.upload_context)?;
            super::files(&self.root)?;
            let bytes = self
                .cipher
                .encrypt_value(ledger)
                .map_err(|_| Error::Storage)?;
            if bytes.len() > state::MAX_ENVELOPE {
                return Err(Error::Capacity);
            }
            self.client
                .state_store()
                .set_custom_value(state::KEY, bytes)
                .await
                .map_err(|_| Error::OutcomeUnknown)?;
            super::files(&self.root)?;
            Ok(())
        }
        .await;
        if result.is_err() {
            self.upload_poisoned = true;
        }
        result
    }
}
impl Owner {
    pub(crate) fn upload_reference(&self, send: &UploadSend) -> Result<Reference, Error> {
        self.upload_context.reference(send)
    }
    pub(crate) async fn reserve_upload(&self, send: UploadSend) -> Result<LivePermit, Error> {
        match self
            .upload_command(Command::Reserve(Box::new(send)))
            .await?
        {
            Reply::Live(permit) => Ok(permit),
            _ => Err(Error::Storage),
        }
    }
    pub(crate) async fn possible_upload(&self, permit: LivePermit) -> Result<Inspection, Error> {
        match self.upload_command(Command::Possible(permit)).await? {
            Reply::Inspection(view) => Ok(view),
            _ => Err(Error::Storage),
        }
    }
    pub(crate) async fn inspect_upload(&self, reference: &Reference) -> Result<Inspection, Error> {
        match self
            .upload_command(Command::Inspect(reference.clone()))
            .await?
        {
            Reply::Inspection(view) => Ok(view),
            _ => Err(Error::Storage),
        }
    }
    pub(crate) async fn accept_upload(
        &self,
        reference: &Reference,
        sealed: &UploadResponse,
    ) -> Result<Inspection, Error> {
        // try_reserve does not wait or allocate a response. At most one queued
        // acceptance per owner and64 queued/uncommitted copies across all owners.
        // Committed/restored bodies remain bounded separately by each SDK ledger.
        let queue = self.tx.try_reserve().map_err(|_| Error::Busy)?;
        let permit = state::memory()
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Capacity)?;
        let response = Response::copy(reference, sealed, permit)?;
        let (send, reply) = oneshot::channel();
        queue.send(super::Command::Upload(
            Command::Accept(reference.clone(), response),
            send,
        ));
        match self.upload_reply(reply).await? {
            Reply::Inspection(view) => Ok(view),
            _ => Err(Error::Storage),
        }
    }
    pub(crate) async fn restore_upload_reference(&self, id: &str) -> Result<Reference, Error> {
        // A bounded exact selector is lookup data, never a source/send capability.
        if id.len() != 39
            || !id.starts_with("upload_")
            || !id[7..]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::Config);
        }
        match self.upload_command(Command::Restore(id.into())).await? {
            Reply::Reference(reference) => Ok(reference),
            _ => Err(Error::Storage),
        }
    }
    async fn upload_command(&self, command: Command) -> Result<Reply, Error> {
        let (send, reply) = oneshot::channel();
        self.tx
            .try_send(super::Command::Upload(command, send))
            .map_err(|_| Error::Busy)?;
        self.upload_reply(reply).await
    }
    async fn upload_reply(
        &self,
        reply: oneshot::Receiver<Result<Reply, Error>>,
    ) -> Result<Reply, Error> {
        tokio::time::timeout(self.timeout, reply)
            .await
            .map_err(|_| Error::OutcomeUnknown)?
            .map_err(|_| Error::OutcomeUnknown)?
    }
}
