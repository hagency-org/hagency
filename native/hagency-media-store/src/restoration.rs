use crate::{Error, Kind, OperationId, Recovery, RestoredEncrypted, Store, SyncEvidence};

impl Store {
    /// The host must retain the ORIGINAL operation and receipt digest outside
    /// the journal. Nothing here discovers whether a prior HTTP write happened.
    pub fn restore_encrypted(
        &mut self,
        operation: &OperationId,
        expected_digest: &[u8; 32],
    ) -> Result<RestoredEncrypted, Error> {
        if self.recovery != Recovery::Clean {
            return Err(Error::OutcomeUnknown);
        }
        let entry = self.entries.get(&operation.0).ok_or(Error::NotFound)?;
        if entry.kind != Kind::Encrypted || &entry.digest != expected_digest {
            return Err(Error::Conflict);
        }
        if self.sync != SyncEvidence::FileAndDirectorySynced {
            return Err(Error::Durability);
        }
        // read() acquires the existing shared permit before copying and checks
        // the retained file, complete original frame, namespace and exact digest.
        let stored = self.read(operation)?;
        let descriptor = stored.descriptor.ok_or(Error::Corrupt)?;
        Ok(RestoredEncrypted {
            bytes: stored.bytes,
            descriptor,
            receipt: stored.receipt,
            namespace: self.namespace.clone(),
            _permit: stored._permit,
        })
    }
}
