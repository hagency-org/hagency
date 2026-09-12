//! Narrow host-only projections and negative observations. No HTTP command or
//! deserialized runtime value constructs these projections or releases custody.
use super::{DomainRepository, conversation_lifecycle, execution, read_engagement, serialize};
use crate::Error;
use hagency_core::{
    canonical,
    conversations::StoredSession,
    project::Resource,
    tasks::{DispatchInput, RunnerCapability, Task, TaskState, clock},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use serde_json::json;

/// Bounded host selection metadata, never a serialized execution grant.
pub struct OwnedClaimRoom {
    id: String,
    generation: u64,
    privacy: hagency_core::replies::RoomPrivacy,
}
impl OwnedClaimRoom {
    pub fn new(
        id: String,
        generation: u64,
        privacy: hagency_core::replies::RoomPrivacy,
    ) -> Result<Self, Error> {
        use hagency_core::replies::{RoomPrivacy, matrix_room, matrix_user};
        let server = id.split_once(':').ok_or(Error::RunnerAuthority)?.1;
        matrix_room(&id, server)?;
        hagency_core::replies::generation(generation)?;
        if let RoomPrivacy::Direct { human_mxid } = &privacy {
            matrix_user(human_mxid, server)?;
        }
        Ok(Self {
            id,
            generation,
            privacy,
        })
    }
}
pub struct OwnedClaimProfile(String);
impl OwnedClaimProfile {
    pub(super) fn from_account_binding(encoded: String) -> Self {
        Self(encoded)
    }
    pub fn new(
        transport: hagency_core::replies::MatrixTransportObservation,
        rooms: Vec<OwnedClaimRoom>,
        workspaces: Vec<String>,
    ) -> Result<Self, Error> {
        use hagency_core::{project::identifier, replies::*, tasks::text};
        identifier(&transport.engagement_id, 128)?;
        generation(transport.generation)?;
        generation(transport.registration_generation)?;
        text(&transport.device_id, 255)?;
        let server = transport
            .sender_mxid
            .split_once(':')
            .ok_or(Error::RunnerAuthority)?
            .1;
        matrix_user(&transport.sender_mxid, server)?;
        if rooms.is_empty() || rooms.len() > 16 || workspaces.is_empty() || workspaces.len() > 16 {
            return Err(Error::Capacity);
        }
        let mut ids = std::collections::BTreeSet::new();
        for room in &rooms {
            matrix_room(&room.id, server)?;
            if !ids.insert(&room.id) {
                return Err(Error::Conflict);
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for id in &workspaces {
            identifier(id, 128)?;
            if !ids.insert(id) {
                return Err(Error::Conflict);
            }
        }
        let encoded = serde_json::to_string(&json!({
            "transport": transport,
            "rooms": rooms.iter().map(|r|json!({"id":r.id,"generation":r.generation,"privacy":r.privacy})).collect::<Vec<_>>(),
            "workspaces": workspaces,
        }))?;
        if encoded.len() > 16 * 1024 {
            return Err(Error::Capacity);
        }
        Ok(Self(encoded))
    }
    /// Narrow the host profile without granting a new eligibility exception.
    pub fn restrict_dispatch(self, id: String) -> Result<Self, Error> {
        hagency_core::project::identifier(&id, 128)?;
        let mut value: serde_json::Value = serde_json::from_str(&self.0)?;
        if value
            .get("dispatch_id")
            .is_some_and(|old| old.as_str() != Some(id.as_str()))
        {
            return Err(Error::Conflict);
        }
        value["dispatch_id"] = id.into();
        let encoded = serde_json::to_string(&value)?;
        if encoded.len() > 16 * 1024 {
            return Err(Error::Capacity);
        }
        Ok(Self(encoded))
    }
    pub(crate) fn encoded(&self) -> &str {
        &self.0
    }
}

/// Constructed from the writer's exact authorized snapshot. This is logical
/// dispatch authority; it does not prove a path's physical directory custody.
#[derive(Clone)]
pub struct OwnedDispatchScope {
    input: DispatchInput,
    task: Task,
    resource: Resource,
    pub(super) account: Option<super::accounts::Association>,
    fingerprint: String,
    started: Option<(String, u64, String)>,
}
impl OwnedDispatchScope {
    /// Internal queue accounting includes every captured variable-size field,
    /// including the private marker. This is never a public scope projection.
    pub(crate) fn queue_value(&self) -> impl Serialize + '_ {
        (
            &self.input,
            &self.task,
            &self.resource,
            &self.account,
            &self.fingerprint,
            &self.started,
        )
    }

    pub(super) fn check_started(&self, cap: &RunnerCapability) -> Result<(), Error> {
        let hash = canonical::digest(&json!(cap.secret))?;
        if self.input.id != cap.dispatch_id
            || self.started.as_ref() != Some(&(cap.runner_id.clone(), cap.fence, hash))
        {
            return Err(Error::RunnerAuthority);
        }
        Ok(())
    }

    pub fn input(&self) -> &DispatchInput {
        &self.input
    }
    pub fn task(&self) -> &Task {
        &self.task
    }
    pub fn requires_managed_account(&self) -> bool {
        self.account.is_some()
    }
    pub fn resource(&self) -> &Resource {
        &self.resource
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedFailure {
    Admission,
    Cancelled,
    StartUnknown,
    SpawnFailed,
    LostAuthority,
    Protocol,
    UnsupportedApproval,
    Deadline,
    CleanupUnknown,
    SettlementUnknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedObservation {
    Unstarted,
    Fenced,
    Historical,
    AlreadySettled,
}

pub(super) fn scope(
    db: &Connection,
    cap: &RunnerCapability,
    now: u64,
    states: &[&str],
) -> Result<OwnedDispatchScope, Error> {
    let dispatch = execution::authorize(db, cap, now, states)?;
    projection(db, cap, &dispatch, None)
}
pub(super) fn projection(
    db: &Connection,
    cap: &RunnerCapability,
    dispatch: &execution::Dispatch,
    completed_epoch: Option<u64>,
) -> Result<OwnedDispatchScope, Error> {
    if dispatch.report_task.is_some() {
        return Err(Error::RunnerAuthority);
    }
    let input: DispatchInput = serde_json::from_str(&dispatch.input)?;
    input.validate()?;
    let task_id = dispatch.task_id.as_deref().ok_or(Error::RunnerAuthority)?;
    let task = execution::task(db, task_id)?;
    if input.id != cap.dispatch_id
        || input.session_id != dispatch.session_id
        || input.task_id.as_deref() != Some(task_id)
        || task.session_id != input.session_id
        || (dispatch.state == "leased" && task.status == TaskState::Done)
    {
        return Err(Error::RunnerAuthority);
    }
    let session: StoredSession = if completed_epoch.is_some() {
        execution::admission_session(db, &input.session_id)?
    } else {
        execution::session(db, &input.session_id)?
    };
    let engagement = read_engagement(db, session.engagement_id())?;
    let (effect, generation): (String, u64) = db.query_row(
        "SELECT f.payload,e.generation FROM effects f JOIN engagements e ON e.id=f.engagement_id WHERE f.engagement_id=?1 AND f.kind='provision' AND f.state='complete'",
        [session.engagement_id()], |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let effect: serde_json::Value = serde_json::from_str(&effect)?;
    let resource: Resource =
        serde_json::from_value(effect.get("resource").cloned().ok_or(Error::Schema)?)?;
    resource.validate()?;
    if resource.id() != engagement.resource_id
        || effect
            .get("registrationGeneration")
            .and_then(|v| v.as_u64())
            != Some(generation)
        || effect.get("runtimeName").and_then(|v| v.as_str()) != Some(&engagement.runtime_name)
    {
        return Err(Error::RunnerAuthority);
    }
    let count: usize = db.query_row(
        "SELECT COUNT(*) FROM resource_leases WHERE dispatch_id=?1",
        [&cap.dispatch_id],
        |r| r.get(0),
    )?;
    if count != input.resources.len() {
        return Err(Error::RunnerAuthority);
    }
    for expected in &input.resources {
        let actual: Option<(bool, bool, bool)> = db.query_row(
            "SELECT l.exclusive,d.exclusive,w.dirty FROM resource_leases l JOIN dispatch_resources d ON d.dispatch_id=l.dispatch_id AND d.resource_id=l.resource_id JOIN workspace_resources w ON w.id=l.resource_id WHERE l.dispatch_id=?1 AND l.resource_id=?2",
            params![cap.dispatch_id,expected.id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        if !actual.is_some_and(|(lease, frozen, dirty)| {
            lease == expected.exclusive
                && frozen == expected.exclusive
                && (!dirty || completed_epoch.is_some())
        }) {
            return Err(Error::Quarantined);
        }
    }
    // Task status/heartbeat may change through authorized canonical operations;
    // its identity and execution epoch must remain fixed for this attempt.
    let account = super::accounts::association(db, &resource)?;
    let fingerprint = canonical::payload_digest(&json!({
        "input":input,"task_id":task.id,"task_epoch":completed_epoch.unwrap_or(task.execution_epoch),
        "session":session,"engagement":engagement.id,"generation":generation,
        "resource":resource,"runtime_name":engagement.runtime_name,"account":account,
    }))?;
    Ok(OwnedDispatchScope {
        input,
        task,
        resource,
        account,
        fingerprint,
        started: None,
    })
}

impl DomainRepository {
    pub fn owned_dispatch_scope(
        &self,
        cap: &RunnerCapability,
        now: u64,
    ) -> Result<OwnedDispatchScope, Error> {
        let scope = scope(&self.db, cap, now, &["leased"])?;
        self.accounts.check_resource(&self.db, scope.resource())?;
        Ok(scope)
    }

    pub fn start_owned_dispatch(
        &mut self,
        cap: &RunnerCapability,
        expected: &str,
        now: u64,
    ) -> Result<OwnedDispatchScope, Error> {
        self.start_owned_clock(cap, expected, || Ok(now))
    }

    pub(crate) fn start_owned_clock(
        &mut self,
        cap: &RunnerCapability,
        expected: &str,
        clock: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<OwnedDispatchScope, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = clock()?; // After writer queue and any SQLite lock wait.
        let before = scope(&tx, cap, now, &["leased"])?;
        self.accounts.check_resource(&tx, before.resource())?;
        if before.fingerprint != expected {
            return Err(Error::RunnerAuthority);
        }
        execution::start_in_transaction(&tx, cap, now)?;
        let mut started = scope(&tx, cap, now, &["started"])?;
        if started.fingerprint != expected {
            return Err(Error::RunnerAuthority);
        }
        started.started = Some((
            cap.runner_id.clone(),
            cap.fence,
            canonical::digest(&json!(cap.secret))?,
        ));
        tx.commit()?;
        Ok(started)
    }

    pub fn check_owned_dispatch(
        &self,
        cap: &RunnerCapability,
        expected: &str,
        now: u64,
    ) -> Result<Task, Error> {
        let value = scope(&self.db, cap, now, &["started"])?;
        self.accounts.check_resource(&self.db, value.resource())?;
        if value.fingerprint != expected {
            return Err(Error::RunnerAuthority);
        }
        Ok(value.task)
    }

    pub(crate) fn check_owned_clock(
        &mut self,
        cap: &RunnerCapability,
        expected: &str,
        clock: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<Task, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = clock()?; // The original queue and SQLite lock waits have ended.
        let value = scope(&tx, cap, now, &["started"])?;
        self.accounts.check_resource(&tx, value.resource())?;
        if value.fingerprint != expected {
            return Err(Error::RunnerAuthority);
        }
        tx.commit()?;
        Ok(value.task)
    }

    pub(crate) fn complete_owned_clock(
        &mut self,
        cap: &RunnerCapability,
        expected: &str,
        output: &serde_json::Value,
        clock: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<Task, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = clock()?;
        let value = scope(&tx, cap, now, &["started"])?;
        self.accounts.check_resource(&tx, value.resource())?;
        if value.fingerprint != expected {
            return Err(Error::RunnerAuthority);
        }
        execution::complete_in_transaction(&tx, cap, output, now)?;
        tx.commit()?;
        Ok(value.task)
    }

    pub(crate) fn renew_owned_clock(
        &mut self,
        cap: &RunnerCapability,
        expected: &str,
        lease_ms: u64,
        clock: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<Task, Error> {
        if !(1..=300_000).contains(&lease_ms) {
            return Err(Error::Capacity);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = clock()?;
        let value = scope(&tx, cap, now, &["started"])?;
        self.accounts.check_resource(&tx, value.resource())?;
        if value.fingerprint != expected || now > hagency_core::JSON_SAFE_MAX - lease_ms {
            return Err(Error::RunnerAuthority);
        }
        tx.execute(
            "UPDATE runner_dispatches SET lease_until=MIN(?2,capability_until) WHERE id=?1",
            params![cap.dispatch_id, now + lease_ms],
        )?;
        tx.commit()?;
        Ok(value.task)
    }

    /// Authenticates the original attempt, even after expiry/revocation. It may
    /// fence that exact attempt or append negative evidence; never a successor.
    pub fn observe_owned_failure(
        &mut self,
        cap: &RunnerCapability,
        failure: OwnedFailure,
        now: u64,
    ) -> Result<OwnedObservation, Error> {
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<(String,String)> = tx.query_row(
            "SELECT runner_id,capability_hash FROM runner_attempts WHERE dispatch_id=?1 AND fence=?2",
            params![cap.dispatch_id,cap.fence], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional()?;
        let (runner, hash) = prior.ok_or(Error::RunnerAuthority)?;
        if runner != cap.runner_id || !execution::matches_secret(&hash, &cap.secret)? {
            return Err(Error::RunnerAuthority);
        }
        tx.execute("UPDATE owned_task_completions SET state='cancelled',updated_at=?3 WHERE dispatch_id=?1 AND fence=?2 AND state='held'", params![cap.dispatch_id,cap.fence,now])?;
        let d = execution::dispatch(&tx, &cap.dispatch_id)?;
        let count: usize = tx.query_row(
            "SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id=?1 AND fence=?2",
            params![cap.dispatch_id, cap.fence],
            |r| r.get(0),
        )?;
        if count >= 128 {
            return Err(Error::Capacity);
        }
        tx.execute(
            "INSERT INTO runner_outputs(dispatch_id,fence,output,accepted) VALUES(?1,?2,?3,0)",
            params![
                cap.dispatch_id,
                cap.fence,
                serialize(&json!({"host_owned_failure":failure}))?
            ],
        )?;
        let result = if d.fence != cap.fence {
            OwnedObservation::Historical
        } else if ["leased", "started", "parked"].contains(&d.state.as_str()) {
            conversation_lifecycle::fence_dispatch(
                &tx,
                &cap.dispatch_id,
                "owned_runner_failure",
                now,
            )?;
            if d.state == "leased" {
                OwnedObservation::Unstarted
            } else {
                OwnedObservation::Fenced
            }
        } else if d.state == "outcome_unknown" {
            let unresolved: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM unresolved_dispatches WHERE id=?1)",
                [&cap.dispatch_id],
                |r| r.get(0),
            )?;
            if unresolved {
                OwnedObservation::Fenced
            } else {
                OwnedObservation::Historical
            }
        } else {
            OwnedObservation::AlreadySettled
        };
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod claim_clock_tests {
    use super::*;
    use hagency_core::replies::{MatrixTransportObservation, RoomPrivacy};
    #[test]
    fn native_owned_claim_profile_clock_after_actual_lock() {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        let observer =
            rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        observer.busy_timeout(std::time::Duration::ZERO).unwrap();
        let profile = OwnedClaimProfile::new(
            MatrixTransportObservation {
                engagement_id: "engagement".into(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE".into(),
            },
            vec![
                OwnedClaimRoom::new("!project:example.test".into(), 1, RoomPrivacy::Group {})
                    .unwrap(),
            ],
            vec!["work".into()],
        )
        .unwrap();
        let result = db
            .claim_owned_clock(&profile, "host", 1000, 1000, 1, || {
                // This independent connection MUST encounter the writer's actual
                // IMMEDIATE lock when the clock is sampled, with no timing sleeps.
                let error = observer.execute_batch("BEGIN IMMEDIATE").unwrap_err();
                assert_eq!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseBusy)
                );
                Ok(2000)
            })
            .unwrap();
        assert!(result.is_none());
        observer
            .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
            .unwrap();
    }
}
