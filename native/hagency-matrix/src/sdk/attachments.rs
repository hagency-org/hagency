use super::{Command, Error, Owner, Phase, Sdk};
use crate::attachments::{AttachmentHandle, MAX_MANIFESTS, Manifest};
use hagency_store::AttachmentTicket;
use std::collections::BTreeMap;
use tokio::sync::{OwnedSemaphorePermit, oneshot};

pub(super) fn validate(
    sdk: &str,
    user: &str,
    device: &str,
    manifests: &BTreeMap<String, Manifest>,
) -> Result<(), Error> {
    if manifests.len() > MAX_MANIFESTS {
        return Err(Error::Storage);
    }
    for (key, value) in manifests {
        if key != &value.id {
            return Err(Error::Storage);
        }
        value.validate(sdk, user, device)?;
    }
    Ok(())
}
impl Sdk {
    pub(super) fn retain_attachments(&mut self) -> Result<(), Error> {
        let batch = self.journal.intake.as_ref().ok_or(Error::Storage)?;
        let mut added = BTreeMap::new();
        for manifest in batch
            .events
            .iter()
            .filter_map(|event| event.attachment.as_ref())
        {
            if let Some(old) = self.journal.attachments.get(&manifest.id) {
                if serde_json::to_value(old).map_err(|_| Error::Storage)?
                    != serde_json::to_value(manifest).map_err(|_| Error::Storage)?
                {
                    return Err(Error::Conflict);
                }
            } else {
                added.insert(manifest.id.clone(), manifest.clone());
            }
        }
        if self
            .journal
            .attachments
            .len()
            .checked_add(added.len())
            .is_none_or(|n| n > MAX_MANIFESTS)
        {
            return Err(Error::Capacity);
        }
        self.journal.attachments.extend(added);
        Ok(())
    }
    pub(super) fn attachment(
        &self,
        ticket: AttachmentTicket,
        permit: OwnedSemaphorePermit,
    ) -> Result<AttachmentHandle, Error> {
        if self.attachments_poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let manifest = self
            .journal
            .attachments
            .get(ticket.manifest_id())
            .ok_or(Error::Storage)?;
        if !manifest.matches(&ticket) {
            return Err(Error::Generation);
        }
        manifest.handle_with_clone(permit)
    }
    pub(super) fn poison_attachments(&mut self) {
        self.attachments_poisoned = true;
        if let Some(batch) = self.journal.intake.as_mut() {
            batch.phase = Phase::Applying;
            batch.events.clear();
            batch.acknowledgements.clear();
            batch.dispositions = Some(vec![]);
            batch.filtered = 0;
        }
    }
}
impl Manifest {
    fn handle_with_clone(&self, permit: OwnedSemaphorePermit) -> Result<AttachmentHandle, Error> {
        self.clone().handle(permit)
    }
}
impl Owner {
    pub(crate) async fn attachment(
        &self,
        ticket: AttachmentTicket,
        permit: OwnedSemaphorePermit,
    ) -> Result<AttachmentHandle, Error> {
        let (send, reply) = oneshot::channel();
        self.tx
            .try_send(Command::Attachment(Box::new(ticket), permit, send))
            .map_err(|_| Error::Busy)?;
        tokio::time::timeout(self.timeout, reply)
            .await
            .map_err(|_| Error::OutcomeUnknown)?
            .map_err(|_| Error::OutcomeUnknown)?
    }
}
