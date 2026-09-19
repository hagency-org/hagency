//! Additional configurations retain the source's actual private account association.
use super::{
    DomainRepository, prepare_resource_write, read_resource, resource_publication::lock_error,
    write_resource_configuration,
};
use crate::{
    Error, ResourcePublicationAccess, ResourcePublicationCommand, ResourcePublicationRetirement,
    resource_publication_revision,
};
use hagency_core::{
    allocation::{Ceiling, Tokens},
    qualification,
};
use rusqlite::TransactionBehavior;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileChange {
    Preserve {},
    Select {
        model: String,
        reasoning: Option<String>,
    },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CeilingChange {
    Preserve {},
    Clear {},
    Monthly { tokens: Tokens },
}
/// Trusted-host constructor only. Never owned by a read-only/publication grant.
/// The inner access supplies the original finite gate, not publication authority.
pub struct ResourceConfigurationAccess(ResourcePublicationAccess);
impl ResourceConfigurationAccess {
    pub fn new(expires: Instant, retirement: ResourcePublicationRetirement) -> Self {
        Self(ResourcePublicationAccess::new(expires, retirement))
    }
    pub fn revoke(&self) -> Result<(), Error> {
        self.0.revoke()
    }
    pub fn prepare(
        &self,
        resource: String,
        revision: String,
        create: bool,
        profile: ProfileChange,
        ceiling: CeilingChange,
        deadline: Instant,
    ) -> Result<ResourceConfigurationCommand, Error> {
        if let ProfileChange::Select { model, reasoning } = &profile
            && (model.len() > 256 || reasoning.as_ref().is_some_and(|v| v.len() > 128))
        {
            return Err(hagency_core::InvalidInput("configuration field too long").into());
        }
        let gate = self.0.prepare(resource, revision, false, deadline)?;
        let preset = if create {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
            Some(format!(
                "preset_{}",
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            ))
        } else {
            None
        };
        Ok(ResourceConfigurationCommand {
            gate,
            preset,
            profile,
            ceiling,
        })
    }
}
/// One consuming command; data unions cannot construct or recreate this authority.
pub struct ResourceConfigurationCommand {
    gate: ResourcePublicationCommand,
    preset: Option<String>,
    profile: ProfileChange,
    ceiling: CeilingChange,
}
impl ResourceConfigurationCommand {
    pub(crate) fn weight(&self) -> u32 {
        2048
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceConfigurationResult {
    pub resource_id: String,
    pub revision: String,
    pub published: bool,
}
impl DomainRepository {
    pub fn configure_resource(
        &mut self,
        command: ResourceConfigurationCommand,
    ) -> Result<ResourceConfigurationResult, Error> {
        self.configure_resource_clock(command, Instant::now)
    }
    fn configure_resource_clock(
        &mut self,
        command: ResourceConfigurationCommand,
        mut clock: impl FnMut() -> Instant,
    ) -> Result<ResourceConfigurationResult, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let gate = &command.gate;
        let revoked = gate.access.revoked.try_lock().map_err(lock_error)?;
        let check = |now| gate.access.check_at(*revoked, gate.deadline, now);
        check(clock())?;
        let mut resource = read_resource(&tx, &gate.resource)?;
        self.accounts.check_resource(&tx, &resource)?;
        let source = resource.clone();
        if resource_publication_revision(&resource)? != gate.revision {
            return Err(Error::Conflict);
        }
        let create = command.preset.is_some();
        if let ProfileChange::Select { model, reasoning } = command.profile {
            if !qualification::configuration_choices(&resource.profile())?
                .iter()
                .any(|c| c.model == model && c.reasoning == reasoning)
            {
                return Err(hagency_core::InvalidInput(
                    "profile is not a selectable configuration",
                )
                .into());
            }
            resource.model = model;
            resource.reasoning = reasoning;
        }
        // A new configuration must use a supported framework and qualified pair.
        // Existing unsupported configurations can preserve their exact profile.
        if create
            && !qualification::configuration_choices(&resource.profile())?
                .iter()
                .any(|c| c.model == resource.model && c.reasoning == resource.reasoning)
        {
            return Err(
                hagency_core::InvalidInput("source profile cannot create a configuration").into(),
            );
        }
        match command.ceiling {
            CeilingChange::Preserve {} => {}
            CeilingChange::Clear {} => resource.ceiling = None,
            CeilingChange::Monthly { tokens } => {
                resource.ceiling = Some(serde_json::from_value::<Ceiling>(
                    serde_json::json!({"tokens":tokens,"period":"monthly"}),
                )?);
            }
        }
        if let Some(preset) = command.preset {
            resource.preset_id = preset;
        }
        if create {
            super::accounts::copy_association(&tx, &source, &resource)?;
        }
        let resource = prepare_resource_write(&tx, &resource, create.then_some(true), create)?;
        let result = ResourceConfigurationResult {
            resource_id: resource.id(),
            revision: resource_publication_revision(&resource)?,
            published: resource.published,
        };
        check(clock())?;
        self.accounts.check_resource(&tx, &resource)?;
        write_resource_configuration(&tx, &resource, create)?;
        check(clock())?;
        self.accounts.check_resource(&tx, &resource)?;
        tx.commit()?;
        self.accounts
            .check_resource(&self.db, &resource)
            .map_err(|_| Error::OutcomeUnknown)?;
        check(clock()).map_err(|_| Error::OutcomeUnknown)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainStore;
    use hagency_core::project::Resource;
    use std::{sync::TryLockError, time::Duration};
    #[tokio::test]
    async fn native_resource_configuration_current() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        let resource: Resource = serde_json::from_value(serde_json::json!({"presetId":"configuration_clock","seatId":"original_clock_account","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium"})).unwrap();
        db.put_resource(&resource).unwrap();
        let revision = resource_publication_revision(&resource).unwrap();
        let prepare = |access: &ResourceConfigurationAccess, deadline| {
            access
                .prepare(
                    resource.id(),
                    revision.clone(),
                    false,
                    ProfileChange::Preserve {},
                    CeilingChange::Monthly {
                        tokens: 1u64.try_into().unwrap(),
                    },
                    deadline,
                )
                .unwrap()
        };
        let inspect = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
        inspect.busy_timeout(Duration::ZERO).unwrap();
        for retire_at in [0, 2, 4] {
            let fence = ResourcePublicationRetirement::default();
            let access = ResourceConfigurationAccess::new(
                Instant::now() + Duration::from_secs(60),
                fence.clone(),
            );
            let command = prepare(&access, Instant::now() + Duration::from_secs(2));
            let original = command.gate.access.clone();
            let mut samples = 0;
            let result = db.configure_resource_clock(command, || {
                samples += 1;
                assert!(matches!(
                    original.revoked.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                assert!(matches!(access.revoke(), Err(Error::Busy)));
                let probe = inspect.execute_batch("BEGIN IMMEDIATE");
                if samples < 4 {
                    assert!(probe.is_err());
                } else {
                    probe.unwrap();
                    inspect.execute_batch("ROLLBACK").unwrap();
                }
                if samples == retire_at {
                    fence.retire();
                }
                Instant::now()
            });
            match retire_at {
                0 => assert!(result.is_ok()),
                2 => assert!(matches!(result, Err(Error::LocalAuthority))),
                _ => assert!(matches!(result, Err(Error::OutcomeUnknown))),
            }
            assert_eq!(
                db.resource_configuration(&resource.id())
                    .unwrap()
                    .ceiling
                    .is_some(),
                retire_at != 2
            );
            db.put_resource(&resource).unwrap();
        }
        // Creation collision refuses without replacing the existing resource.
        let access = ResourceConfigurationAccess::new(
            Instant::now() + Duration::from_secs(60),
            ResourcePublicationRetirement::default(),
        );
        let mut command = prepare(&access, Instant::now() + Duration::from_secs(2));
        command.preset = Some(resource.preset_id.clone());
        assert!(matches!(
            db.configure_resource(command),
            Err(Error::Conflict)
        ));
        assert!(
            db.resource_configuration(&resource.id())
                .unwrap()
                .ceiling
                .is_none()
        );
        let store = DomainStore::start(db, 8).unwrap();
        for mode in ["expiry", "deadline", "retirement", "caller_loss"] {
            let now = Instant::now();
            let fence = ResourcePublicationRetirement::default();
            let access = ResourceConfigurationAccess::new(
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
            let other = ResourceConfigurationAccess::new(
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
            let mut pending = Box::pin(store.configure_resource(command));
            assert!(
                std::future::poll_fn(|cx| std::task::Poll::Ready(pending.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            if mode == "retirement" {
                fence.retire();
            }
            if mode == "caller_loss" {
                drop(pending);
            } else {
                if mode != "retirement" {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                }
                inspect.execute_batch("COMMIT").unwrap();
                ahead.await.unwrap();
                let result = pending.await;
                assert!(if mode == "deadline" {
                    matches!(result, Err(Error::OutcomeUnknown))
                } else {
                    matches!(result, Err(Error::LocalAuthority))
                });
                assert!(
                    store
                        .resource_configuration(resource.id())
                        .await
                        .unwrap()
                        .ceiling
                        .is_none()
                );
                access.revoke().unwrap();
                continue;
            }
            inspect.execute_batch("COMMIT").unwrap();
            ahead.await.unwrap();
            assert!(
                store
                    .resource_configuration(resource.id())
                    .await
                    .unwrap()
                    .ceiling
                    .is_none()
            );
            access.revoke().unwrap();
        }
        // Per-session queue custody is finite and dropped preparation frees it.
        let access = ResourceConfigurationAccess::new(
            Instant::now() + Duration::from_secs(60),
            ResourcePublicationRetirement::default(),
        );
        let commands = (0..8)
            .map(|_| prepare(&access, Instant::now() + Duration::from_secs(2)))
            .collect::<Vec<_>>();
        assert!(matches!(
            access.prepare(
                resource.id(),
                revision,
                false,
                ProfileChange::Preserve {},
                CeilingChange::Preserve {},
                Instant::now() + Duration::from_secs(2)
            ),
            Err(Error::Busy)
        ));
        drop(commands);
        access.revoke().unwrap();
        store.shutdown().await.unwrap();
    }
}
