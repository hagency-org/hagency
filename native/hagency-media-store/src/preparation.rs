use crate::{
    Error, Media, OperationId, Permit, PreparedEncrypted, Recovery, StageFailure, Store,
    SyncEvidence, frame,
};
use hagency_media::Encrypted;
use std::sync::Arc;

impl Store {
    pub fn prepare_encrypted(
        &mut self,
        operation: &OperationId,
        encrypted: Encrypted,
    ) -> Result<PreparedEncrypted, StageFailure> {
        let media = Media::Encrypted(encrypted);
        let result = (|| {
            let permit = Permit::acquire(&self.pool)?;
            let digest = self.preparation_identity(operation, &media)?;
            Ok((permit, digest))
        })();
        match result {
            Ok((permit, digest)) => Ok(PreparedEncrypted {
                media,
                operation: operation.clone(),
                namespace: self.namespace.clone(),
                digest,
                _permit: permit,
            }),
            Err(error) => Err(StageFailure::returned(error, media)),
        }
    }

    /// No automatic domain mutation occurs here. The host must already have
    /// acknowledged the exact original commitment before this call.
    pub fn stage_prepared(
        &mut self,
        prepared: PreparedEncrypted,
    ) -> Result<crate::Receipt, StageFailure> {
        if !Arc::ptr_eq(&self.pool, &prepared._permit.0) || self.namespace.0 != prepared.namespace.0
        {
            return Err(StageFailure::returned(Error::Identity, prepared.media));
        }
        match self.preparation_identity(&prepared.operation, &prepared.media) {
            Ok(digest) if digest == prepared.digest => {}
            Ok(_) => return Err(StageFailure::returned(Error::Conflict, prepared.media)),
            Err(error) => return Err(StageFailure::returned(error, prepared.media)),
        }
        // Retain the same slot through synchronous write/failure handling. The
        // existing stage implementation owns material once it may perform IO.
        let result = self.stage(&prepared.operation, prepared.media);
        drop(prepared._permit);
        result
    }

    fn preparation_identity(
        &mut self,
        operation: &OperationId,
        media: &Media,
    ) -> Result<[u8; 32], Error> {
        if let Err(error) = self.check() {
            self.recovery = Recovery::WriteOutcomeUnknown;
            return Err(error);
        }
        if self.recovery != Recovery::Clean {
            return Err(Error::OutcomeUnknown);
        }
        if self.sync != SyncEvidence::FileAndDirectorySynced {
            return Err(Error::Durability);
        }
        let prepared = frame::prepare(
            &self.namespace,
            operation,
            media,
            self.length,
            self.chain,
            self.limits,
        )?;
        if let Some(original) = self.entries.get(&operation.0).cloned() {
            if original.digest != prepared.entry.digest {
                return Err(Error::Conflict);
            }
            self.validate(&original, false)?;
        } else if self.entries.len() >= self.limits.records
            || self
                .length
                .checked_add(prepared.entry.length)
                .is_none_or(|size| size > self.limits.file_bytes)
        {
            return Err(Error::Capacity);
        }
        Ok(prepared.entry.digest)
    }
}
