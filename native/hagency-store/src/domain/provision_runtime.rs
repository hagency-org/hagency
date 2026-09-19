//! Original warm custody, never Applied or dispatch authority.
use super::{
    DomainRepository, Effect, EffectState, accounts, read_effect, read_engagement, read_resource,
};
use crate::Error;
use hagency_core::{
    authority::{ProjectRequest, Registration},
    canonical,
    project::{EngagementState, Resource},
};
use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub struct OwnedProvisionScope {
    effect: Effect,
    registration: Registration,
    resource: Resource,
    pub(super) account: Option<accounts::Association>,
    owner: Arc<()>,
    claimed: Arc<AtomicBool>,
}
impl OwnedProvisionScope {
    pub fn resource(&self) -> &Resource {
        &self.resource
    }
    pub fn engagement_id(&self) -> &str {
        &self.effect.engagement_id
    }
    pub fn requires_managed_account(&self) -> bool {
        self.account.is_some()
    }
    pub fn claim_warm(&self) -> Result<(), Error> {
        if self.claimed.swap(true, Ordering::AcqRel) {
            return Err(Error::Busy);
        }
        Ok(())
    }
    pub(crate) fn queue_value(&self) -> impl Serialize + '_ {
        (
            &self.effect,
            &self.registration,
            &self.resource,
            &self.account,
        )
    }
    pub(crate) fn provision_digest(&self) -> Result<String, Error> {
        canonical::transport_digest(&json!([self.effect, self.registration])).map_err(Into::into)
    }
}
fn current(
    db: &Connection,
    scope: &OwnedProvisionScope,
    registry: &accounts::Registry,
    owner: &Arc<()>,
    active: bool,
) -> Result<(), Error> {
    if !Arc::ptr_eq(owner, &scope.owner) {
        return Err(Error::RunnerAuthority);
    }
    let actual = read_effect(db, &scope.effect.id)?;
    let engagement = read_engagement(db, scope.engagement_id())?;
    if scope.effect.kind != "provision"
        || scope.effect.state != EffectState::Started
        || actual.kind != scope.effect.kind
        || actual.engagement_id != scope.engagement_id()
        || actual.fence != scope.effect.fence
        || actual.fence == 0
        || canonical::transport_digest(&actual.payload)?
            != canonical::transport_digest(&scope.effect.payload)?
        || !((actual.state == EffectState::Started
            && engagement.state == EngagementState::Reserved)
            || (active
                && actual.state == EffectState::Complete
                && engagement.state == EngagementState::Active))
    {
        return Err(Error::State);
    }
    let encoded: String = db.query_row(
        "SELECT config FROM registrations WHERE fleet_id=?1",
        [&scope.registration.fleet_id],
        |r| r.get(0),
    )?;
    let registration: Registration = serde_json::from_str(&encoded)?;
    let generation: u64 = db.query_row(
        "SELECT generation FROM engagements WHERE id=?1",
        [scope.engagement_id()],
        |r| r.get(0),
    )?;
    if registration != scope.registration || generation != registration.generation {
        return Err(Error::Generation);
    }
    let resource = read_resource(db, &scope.resource.id())?;
    if canonical::transport_digest(&json!(resource))?
        != canonical::transport_digest(&json!(scope.resource))?
        || accounts::association(db, &scope.resource)? != scope.account
    {
        return Err(Error::Unqualified);
    }
    registry.check_resource(db, &scope.resource)?;
    if !accounts::warm_ready(db, scope.account.as_ref(), super::graphs::now_ms()?)? {
        return Err(Error::LocalAuthority);
    }
    Ok(())
}
impl DomainRepository {
    /// Original registry owner only; never reopen a writer or select an account
    /// from caller metadata. The scope's producing owner and current facts gate
    /// this read before the fixed Host can prepare a launch.
    pub fn provision_runtime_account(
        &mut self,
        scope: &OwnedProvisionScope,
    ) -> Result<Option<crate::ManagedAccount>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        current(&tx, scope, &self.accounts, &self.approval_owner, false)?;
        tx.commit()?;
        scope
            .account
            .as_ref()
            .map(|association| self.managed_account(accounts::provision_account_id(association)))
            .transpose()
    }
    /// Trusted Host admission after physical checks, not caller-supplied proof.
    /// The original acknowledged scope must still be Started/Reserved and warm
    /// consumption sticky. Both admission and the existing kernel share one tx.
    pub fn complete_original_provision(
        &mut self,
        scope: &OwnedProvisionScope,
    ) -> Result<hagency_core::project::Engagement, Error> {
        if !scope.claimed.load(Ordering::Acquire) {
            return Err(Error::State);
        }
        let receipt = format!("inline_factory_{}", scope.provision_digest()?);
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        current(&tx, scope, &self.accounts, &self.approval_owner, false)?;
        let value = super::observe_effect_transaction(
            &tx,
            &scope.effect.id,
            scope.effect.fence,
            &super::EffectOutcome::Applied { receipt },
        )?;
        tx.commit()?;
        Ok(value)
    }
    pub fn provision_runtime_scope(
        &mut self,
        effect: &Effect,
        registration: &Registration,
    ) -> Result<OwnedProvisionScope, Error> {
        self.validate_provision_account(effect, registration)?;
        let resource: Resource = serde_json::from_value(
            effect
                .payload
                .get("resource")
                .cloned()
                .ok_or(Error::Schema)?,
        )?;
        let request: ProjectRequest = serde_json::from_value(
            effect
                .payload
                .get("request")
                .cloned()
                .ok_or(Error::Schema)?,
        )?;
        request.validate(registration)?;
        resource.validate()?;
        if request.engagement_id()? != effect.engagement_id
            || request.agent_definition.resource_id != resource.id()
            || !resource.qualifies(&request.role)
        {
            return Err(Error::Unqualified);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = OwnedProvisionScope {
            effect: effect.clone(),
            registration: registration.clone(),
            account: accounts::association(&tx, &resource)?,
            resource,
            owner: self.approval_owner.clone(),
            claimed: Arc::new(AtomicBool::new(false)),
        };
        current(&tx, &scope, &self.accounts, &self.approval_owner, false)?;
        tx.commit()?;
        if let Some(known) = self.warm_scopes.get(&effect.id) {
            if canonical::transport_digest(&json!(known.queue_value()))?
                != canonical::transport_digest(&json!(scope.queue_value()))?
            {
                return Err(Error::Conflict);
            }
            return Ok(known.clone());
        }
        if self.warm_scopes.len() >= 16 {
            return Err(Error::Capacity);
        }
        self.warm_scopes.insert(effect.id.clone(), scope.clone());
        Ok(scope)
    }
    pub fn validate_warm_runtime_scope(
        &mut self,
        scope: &OwnedProvisionScope,
    ) -> Result<(), Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        current(&tx, scope, &self.accounts, &self.approval_owner, true)?;
        tx.commit()?;
        Ok(())
    }
}
