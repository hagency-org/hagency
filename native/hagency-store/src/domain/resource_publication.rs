//! Concrete host-only local publication scope. No wire constructor or callback.
use super::{DomainRepository, read_resource, serialize};
use crate::Error;
use hagency_core::project::{Resource, identifier};
use rusqlite::{TransactionBehavior, params};
use serde::Serialize;
use std::{
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};

/// One console incarnation's irreversible retirement fence.
#[derive(Clone, Default)]
pub struct ResourcePublicationRetirement(Arc<AtomicBool>);
impl ResourcePublicationRetirement {
    pub fn retire(&self) {
        self.0.store(true, Ordering::Release);
    }
}
pub(super) struct Access {
    pub(super) revoked: Mutex<bool>,
    pending: AtomicUsize,
    expires: Instant,
    retirement: ResourcePublicationRetirement,
}
/// Constructed by the trusted host only after explicit management-ticket exchange.
/// Read-only browser sessions must never own this object.
pub struct ResourcePublicationAccess(Arc<Access>);
impl ResourcePublicationAccess {
    pub fn new(expires: Instant, retirement: ResourcePublicationRetirement) -> Self {
        Self(Arc::new(Access {
            revoked: Mutex::new(false),
            pending: AtomicUsize::new(0),
            expires,
            retirement,
        }))
    }
    /// Synchronous and effect-free; command custody precedes queue admission.
    pub fn prepare(
        &self,
        resource: String,
        revision: String,
        published: bool,
        deadline: Instant,
    ) -> Result<ResourcePublicationCommand, Error> {
        identifier(&resource, 128)?;
        if revision.len() != 64
            || !revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(hagency_core::InvalidInput("invalid resource revision").into());
        }
        let revoked = self.0.revoked.try_lock().map_err(lock_error)?;
        self.0.check(*revoked, deadline)?;
        if self.0.pending.load(Ordering::Acquire) >= 8 {
            return Err(Error::Busy);
        }
        self.0.pending.fetch_add(1, Ordering::AcqRel);
        Ok(ResourcePublicationCommand {
            access: self.0.clone(),
            resource,
            revision,
            published,
            deadline,
        })
    }
    /// Never waits for a queued command or SQLite commit. Busy is not revocation.
    pub fn revoke(&self) -> Result<(), Error> {
        let mut revoked = self.0.revoked.try_lock().map_err(lock_error)?;
        if self.0.pending.load(Ordering::Acquire) != 0 {
            return Err(Error::Busy);
        }
        *revoked = true;
        Ok(())
    }
}
pub(super) fn lock_error<T>(error: TryLockError<T>) -> Error {
    match error {
        TryLockError::WouldBlock => Error::Busy,
        TryLockError::Poisoned(_) => Error::Unavailable,
    }
}
impl Access {
    fn check(&self, revoked: bool, deadline: Instant) -> Result<(), Error> {
        self.check_at(revoked, deadline, Instant::now())
    }
    pub(super) fn check_at(
        &self,
        revoked: bool,
        deadline: Instant,
        now: Instant,
    ) -> Result<(), Error> {
        if revoked || self.retirement.0.load(Ordering::Acquire) || now >= self.expires {
            return Err(Error::LocalAuthority);
        }
        if now >= deadline {
            return Err(Error::OutcomeUnknown);
        }
        Ok(())
    }
}
/// One non-cloneable command from one exact original session. Not deserializable.
pub struct ResourcePublicationCommand {
    pub(super) access: Arc<Access>,
    pub(super) resource: String,
    pub(super) revision: String,
    published: bool,
    pub(super) deadline: Instant,
}
impl Drop for ResourcePublicationCommand {
    fn drop(&mut self) {
        self.access.pending.fetch_sub(1, Ordering::AcqRel);
    }
}
impl ResourcePublicationCommand {
    pub(crate) fn weight(&self) -> u32 {
        (self.resource.len() + self.revision.len() + 128) as u32
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePublicationResult {
    pub resource_id: String,
    pub published: bool,
    pub revision: String,
}
/// Revision covers actual configuration, with its model-derived roles normalized.
pub fn resource_publication_revision(resource: &Resource) -> Result<String, Error> {
    let mut value = resource.clone();
    value.roles = value.eligible_roles();
    Ok(hagency_core::canonical::digest(&serde_json::to_value(
        &value,
    )?)?)
}
impl DomainRepository {
    pub fn publish_resource(
        &mut self,
        command: ResourcePublicationCommand,
    ) -> Result<ResourcePublicationResult, Error> {
        self.publish_resource_clock(command, Instant::now)
    }
    fn publish_resource_clock(
        &mut self,
        command: ResourcePublicationCommand,
        mut clock: impl FnMut() -> Instant,
    ) -> Result<ResourcePublicationResult, Error> {
        // No session gate is retained while waiting for the actual SQLite lock.
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revoked = command.access.revoked.try_lock().map_err(lock_error)?;
        command
            .access
            .check_at(*revoked, command.deadline, clock())?;
        let mut resource = read_resource(&tx, &command.resource)?;
        self.accounts.check_resource(&tx, &resource)?;
        if resource_publication_revision(&resource)? != command.revision {
            return Err(Error::Conflict);
        }
        resource.published = command.published;
        resource.roles = resource.eligible_roles();
        let result = ResourcePublicationResult {
            resource_id: command.resource.clone(),
            published: resource.published,
            revision: resource_publication_revision(&resource)?,
        };
        command
            .access
            .check_at(*revoked, command.deadline, clock())?;
        tx.execute(
            "UPDATE resources SET config=?1 WHERE id=?2",
            params![serialize(&resource)?, command.resource],
        )?;
        command
            .access
            .check_at(*revoked, command.deadline, clock())?;
        self.accounts.check_resource(&tx, &resource)?;
        tx.commit()?;
        self.accounts
            .check_resource(&self.db, &resource)
            .map_err(|_| Error::OutcomeUnknown)?;
        // An already committed write is never described as rolled back.
        command
            .access
            .check_at(*revoked, command.deadline, clock())
            .map_err(|_| Error::OutcomeUnknown)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainStore;
    use std::time::Duration;

    #[tokio::test]
    async fn native_resource_publication_current() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        let resource: Resource = serde_json::from_value(serde_json::json!({"presetId":"clock_pool","seatId":"clock_seat","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium"})).unwrap();
        db.put_resource(&resource).unwrap();
        let revision = resource_publication_revision(&resource).unwrap();
        let fence = ResourcePublicationRetirement::default();
        let access =
            ResourcePublicationAccess::new(Instant::now() + Duration::from_secs(60), fence.clone());
        let prepare = |access: &ResourcePublicationAccess, deadline| {
            access
                .prepare(resource.id(), revision.clone(), false, deadline)
                .unwrap()
        };
        let command = prepare(&access, Instant::now() + Duration::from_secs(2));
        let inspect = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
        inspect.busy_timeout(Duration::ZERO).unwrap();
        let mut samples = 0;
        let result = db
            .publish_resource_clock(command, || {
                samples += 1;
                assert!(matches!(
                    access.0.revoked.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                assert!(matches!(
                    access.prepare(
                        resource.id(),
                        revision.clone(),
                        false,
                        Instant::now() + Duration::from_secs(2)
                    ),
                    Err(Error::Busy)
                ));
                assert!(matches!(access.revoke(), Err(Error::Busy)));
                let probe = inspect.execute_batch("BEGIN IMMEDIATE");
                if samples < 4 {
                    assert!(probe.is_err());
                } else {
                    probe.unwrap();
                    inspect.execute_batch("ROLLBACK").unwrap();
                }
                Instant::now()
            })
            .unwrap();
        assert_eq!(samples, 4);
        assert!(!result.published);
        db.put_resource(&resource).unwrap();
        let command = prepare(&access, Instant::now() + Duration::from_secs(2));
        let mut samples = 0;
        assert!(matches!(
            db.publish_resource_clock(command, || {
                samples += 1;
                if samples == 4 {
                    fence.retire();
                }
                Instant::now()
            }),
            Err(Error::OutcomeUnknown)
        ));
        assert!(
            !db.resource_configurations("", 16).unwrap()[0]
                .config
                .published
        );
        db.put_resource(&resource).unwrap();
        let store = DomainStore::start(db, 8).unwrap();
        for mode in ["expiry", "deadline", "retirement", "caller_loss"] {
            let now = Instant::now();
            let fence = ResourcePublicationRetirement::default();
            let access = ResourcePublicationAccess::new(
                now + if mode == "expiry" {
                    Duration::from_millis(60)
                } else {
                    Duration::from_secs(60)
                },
                fence.clone(),
            );
            let deadline = now
                + if mode == "deadline" {
                    Duration::from_millis(60)
                } else {
                    Duration::from_secs(2)
                };
            let command = prepare(&access, deadline);
            assert!(matches!(access.revoke(), Err(Error::Busy)));
            let other = ResourcePublicationAccess::new(
                now + Duration::from_secs(60),
                ResourcePublicationRetirement::default(),
            );
            drop(prepare(&other, deadline));
            other.revoke().unwrap();
            inspect.execute_batch("BEGIN IMMEDIATE").unwrap();
            let mut ahead = Box::pin(store.put_resource(resource.clone()));
            assert!(
                std::future::poll_fn(|cx| std::task::Poll::Ready(ahead.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            let mut operation = Box::pin(store.publish_resource(command));
            assert!(
                std::future::poll_fn(|cx| std::task::Poll::Ready(operation.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            if mode == "retirement" {
                fence.retire();
            }
            if mode == "caller_loss" {
                drop(operation);
            } else {
                if mode != "retirement" {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                }
                inspect.execute_batch("COMMIT").unwrap();
                ahead.await.unwrap();
                let result = operation.await;
                assert!(if mode == "deadline" {
                    matches!(result, Err(Error::OutcomeUnknown))
                } else {
                    matches!(result, Err(Error::LocalAuthority))
                });
                assert!(
                    store
                        .resource_configurations(String::new(), 16)
                        .await
                        .unwrap()[0]
                        .config
                        .published
                );
                access.revoke().unwrap();
                continue;
            }
            inspect.execute_batch("COMMIT").unwrap();
            ahead.await.unwrap();
            assert!(
                store
                    .resource_configurations(String::new(), 16)
                    .await
                    .unwrap()[0]
                    .config
                    .published
            );
            access.revoke().unwrap();
        }
        store.shutdown().await.unwrap();
    }
}
