//! Coherent domain observations enter the existing immutable publication lane.
use super::*;
use hagency_store::DomainStore;

impl Adapter {
    pub async fn publish_resources_once(
        &self,
        domain: &DomainStore,
        cancel: &CancellationToken,
    ) -> Result<Step, Error> {
        let _guard = self.publication.try_lock().map_err(|_| Error::Busy)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        // Rotation is checked even for an already frozen historical catalog.
        // A changed catalog alone must not overwrite that original pending body.
        domain
            .check_publication_registration(self.registration.clone())
            .await?;
        let Reply::Publication(pending) = self
            .command(Command::PendingPublication(self.scope()))
            .await?
        else {
            return Err(Error::Custody);
        };
        if pending.is_none() {
            let snapshot = domain.published_catalog(self.registration.clone()).await?;
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let Reply::Publication(Some(_)) = self
                .command(Command::FreezePublication {
                    scope: self.scope(),
                    body: snapshot.into_update(),
                })
                .await?
            else {
                return Err(Error::Custody);
            };
        }
        self.publish_checked(Some(domain), cancel).await
    }
}
