//! Private host/runner boundary. No public HTTP route constructs these host commands.
use super::{DomainRepository, bounded_row, read_engagement, serialize};
use crate::Error;
use hagency_core::conversations::StoredSession;
use hagency_core::{JSON_SAFE_MAX, canonical, project::identifier, tasks::*};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};

fn load_session(db: &Connection, id: &str, allow_quarantine: bool) -> Result<StoredSession, Error> {
    let value: Option<(String, bool, String)> = db
        .query_row(
            "SELECT binding,quarantined,engagement_id FROM runner_sessions WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (value, quarantined, engagement_id) = value.ok_or(Error::NotFound)?;
    if quarantined && !allow_quarantine {
        return Err(Error::Quarantined);
    }
    let binding: StoredSession = serde_json::from_str(&value)?;
    binding.validate()?;
    if binding.id() != id || binding.engagement_id() != engagement_id {
        return Err(Error::RunnerAuthority);
    }
    let active:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM engagements e JOIN registrations r ON r.fleet_id=e.fleet_id WHERE e.id=?1 AND e.state='active' AND e.generation=r.generation)",[binding.engagement_id()],|r|r.get(0))?;
    if !active {
        return Err(Error::RunnerAuthority);
    }
    if let StoredSession::Internal(internal) = &binding {
        let member:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM internal_participants p JOIN internal_conversations c ON c.id=p.conversation_id JOIN engagements e ON e.id=p.engagement_id WHERE p.session_id=?1 AND p.engagement_id=?2 AND p.conversation_id=?3 AND c.state='active' AND c.fleet_id=e.fleet_id AND c.project_id=e.project_id AND c.generation=e.generation)",params![id,internal.engagement_id,internal.conversation_id],|r|r.get(0))?;
        if !member {
            return Err(Error::RunnerAuthority);
        }
    }
    let matrix_generation: u64 = db.query_row(
        "SELECT matrix_generation FROM runner_sessions WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    if matrix_generation > 0 {
        super::matrix_routes::check(db, id)?;
    }
    Ok(binding)
}
pub(super) fn session(db: &Connection, id: &str) -> Result<StoredSession, Error> {
    load_session(db, id, false)
}
pub(super) fn admission_session(db: &Connection, id: &str) -> Result<StoredSession, Error> {
    load_session(db, id, true)
}
pub(super) fn matrix_admission_session(db: &Connection, id: &str) -> Result<SessionBinding, Error> {
    admission_session(db, id)?
        .matrix()
        .cloned()
        .ok_or(Error::RunnerAuthority)
}
pub(super) fn task(db: &Connection, id: &str) -> Result<Task, Error> {
    let value: String = db
        .query_row(
            "SELECT config FROM canonical_tasks WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(&value)?)
}
pub(super) fn save_task(tx: &Transaction<'_>, value: &Task, kind: &str) -> Result<(), Error> {
    let value_json = serialize(value)?;
    tx.execute(
        "UPDATE canonical_tasks SET config=?2 WHERE id=?1",
        params![value.id, value_json],
    )?;
    tx.execute(
        "INSERT INTO task_outbox(task_id,kind,task) VALUES(?1,?2,?3)",
        params![value.id, kind, value_json],
    )?;
    Ok(())
}
pub(super) struct Dispatch {
    pub(super) session_id: String,
    pub(super) task_id: Option<String>,
    pub(super) report_task: Option<String>,
    pub(super) state: String,
    pub(super) fence: u64,
    runner: Option<String>,
    hash: Option<String>,
    lease: u64,
    expiry: u64,
    pub(super) input: String,
}
pub(super) fn dispatch(db: &Connection, id: &str) -> Result<Dispatch, Error> {
    db.query_row("SELECT session_id,task_id,state,fence,runner_id,capability_hash,COALESCE(lease_until,0),COALESCE(capability_until,0),input FROM runner_dispatches WHERE id=?1",[id],|r|Ok(Dispatch {
        session_id:r.get(0)?,task_id:r.get(1)?,report_task:None,state:r.get(2)?,fence:r.get(3)?,runner:r.get(4)?,hash:r.get(5)?,lease:r.get(6)?,expiry:r.get(7)?,input:r.get(8)?,
    })).optional()?.ok_or(Error::NotFound)
}
pub(super) fn matches_secret(hash: &str, secret: &str) -> Result<bool, Error> {
    if secret.len() != 64 || !secret.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(false);
    }
    let candidate = canonical::digest(&json!(secret))?;
    if hash.len() != candidate.len() {
        return Ok(false);
    }
    Ok(hash
        .bytes()
        .zip(candidate.bytes())
        .fold(0u8, |a, (b, c)| a | (b ^ c))
        == 0)
}
pub(super) fn authorize(
    db: &Connection,
    cap: &RunnerCapability,
    now: u64,
    states: &[&str],
) -> Result<Dispatch, Error> {
    let mut d = authorize_attempt(db, cap, now, states)?;
    session(db, &d.session_id)?;
    d.report_task = report_grant(db, &cap.dispatch_id)?.map(|g| g.task.id);
    super::task_intents::check_session_task(
        db,
        &d.session_id,
        d.task_id.as_deref().or(d.report_task.as_deref()),
    )?;
    super::graphs::check_dispatch(db, &cap.dispatch_id, &d)?;
    Ok(d)
}
/// Work creation must not inherit a completed-result report's read permission.
pub(super) fn authorize_work(
    db: &Connection,
    cap: &RunnerCapability,
    now: u64,
) -> Result<Dispatch, Error> {
    let d = authorize(db, cap, now, &["started"])?;
    if d.report_task.is_some() {
        return Err(Error::RunnerAuthority);
    }
    Ok(d)
}
// Constructed only from persisted host recovery state or from the inspected
// completed task inside recover_dispatch. It has no runtime JSON constructor.
struct ReportGrant {
    task: Task,
}
fn report_grant(db: &Connection, dispatch_id: &str) -> Result<Option<ReportGrant>, Error> {
    let row: Option<Option<String>> = db.query_row(
        "SELECT current.task_id FROM dispatch_recovery_reports report LEFT JOIN current_recovery_reports current ON current.dispatch_id=report.dispatch_id WHERE report.dispatch_id=?1",
        [dispatch_id], |r| r.get(0),
    ).optional()?;
    match row {
        None => Ok(None),
        Some(None) => Err(Error::RunnerAuthority),
        Some(Some(id)) => Ok(Some(ReportGrant {
            task: task(db, &id)?,
        })),
    }
}
// Host cleanup of an unstarted lease must remain possible after its task or
// allocation binding changes. No runtime command exposes this cleanup authority.
fn authorize_attempt(
    db: &Connection,
    cap: &RunnerCapability,
    now: u64,
    states: &[&str],
) -> Result<Dispatch, Error> {
    clock(now)?;
    let d = dispatch(db, &cap.dispatch_id)?;
    if !states.contains(&d.state.as_str())
        || d.fence != cap.fence
        || d.runner.as_deref() != Some(&cap.runner_id)
        || d.lease <= now
        || d.expiry <= now
        || !matches_secret(d.hash.as_deref().unwrap_or(""), &cap.secret)?
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(d)
}
fn lose(tx: &Transaction<'_>, id: &str) -> Result<(), Error> {
    let d = dispatch(tx, id)?;
    if !["leased", "started", "parked"].contains(&d.state.as_str()) {
        return Ok(());
    }
    let next = if d.state == "leased" {
        "queued"
    } else {
        "outcome_unknown"
    };
    tx.execute(
        "UPDATE runner_attempts SET outcome=?3 WHERE dispatch_id=?1 AND fence=?2",
        params![
            id,
            d.fence,
            if next == "queued" {
                "unstarted_requeued"
            } else {
                next
            }
        ],
    )?;
    if next == "outcome_unknown" {
        tx.execute(
            "UPDATE runner_sessions SET quarantined=1 WHERE id=?1",
            [&d.session_id],
        )?;
        tx.execute("UPDATE workspace_resources SET dirty=1 WHERE id IN (SELECT resource_id FROM dispatch_resources WHERE dispatch_id=?1 AND exclusive=1)",[id])?;
    } else {
        // Only an unstarted attempt can relinquish custody without inspection.
        // Unknown shared readers must still exclude a new exclusive writer.
        tx.execute("DELETE FROM resource_leases WHERE dispatch_id=?1", [id])?;
    }
    tx.execute("UPDATE runner_dispatches SET state=?2,capability_hash=NULL,lease_until=NULL,capability_until=NULL WHERE id=?1",params![id,next])?;
    Ok(())
}
pub(super) fn recover_all(tx: &Transaction<'_>) -> Result<(), Error> {
    let ids = tx
        .prepare("SELECT id FROM runner_dispatches WHERE state IN ('leased','started','parked')")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        lose(tx, &id)?;
    }
    Ok(())
}
fn expire(tx: &Transaction<'_>, now: u64) -> Result<(), Error> {
    let ids=tx.prepare("SELECT id FROM runner_dispatches WHERE state IN ('leased','started','parked') AND (lease_until<=?1 OR capability_until<=?1)")?.query_map([now],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
    for id in ids {
        lose(tx, &id)?;
    }
    Ok(())
}
pub(super) fn create_task(
    tx: &Transaction<'_>,
    id: &str,
    session_id: &str,
    creator: Option<&str>,
    title: &str,
    now: u64,
) -> Result<Task, Error> {
    identifier(id, 128)?;
    text(title, 4096)?;
    clock(now)?;
    session(tx, session_id)?;
    match task(tx, id) {
        Ok(old) => {
            return if old.session_id == session_id
                && old.creator_session_id.as_deref() == creator
                && old.title == title
            {
                Ok(old)
            } else {
                Err(Error::Conflict)
            };
        }
        Err(Error::NotFound) => {}
        Err(error) => return Err(error),
    }
    bounded_row(tx, "canonical_tasks", "id", id, 10_000)?;
    let t = Task {
        id: id.into(),
        session_id: session_id.into(),
        creator_session_id: creator.map(str::to_owned),
        title: title.into(),
        description: String::new(),
        priority: Default::default(),
        granularity: Default::default(),
        labels: Vec::new(),
        parent_id: None,
        status: TaskState::Created,
        execution_epoch: 0,
        created_at: now,
        updated_at: now,
        started_at: None,
        completed_at: None,
        heartbeat_at: None,
        waiting_reason: None,
        waiting_until: None,
    };
    tx.execute(
        "INSERT INTO canonical_tasks(id,session_id,creator_session_id,config) VALUES(?1,?2,?3,?4)",
        params![id, session_id, creator, serialize(&t)?],
    )?;
    save_task(tx, &t, "created")?;
    Ok(t)
}
pub(super) fn enqueue(tx: &Transaction<'_>, input: &DispatchInput) -> Result<(), Error> {
    if super::graphs::is_graph_task(tx, input.task_id.as_deref())? {
        return Err(Error::RunnerAuthority);
    }
    enqueue_scoped(tx, input, None)
}
pub(super) fn enqueue_peers(
    tx: &Transaction<'_>,
    input: &DispatchInput,
    sequences: &[u64],
) -> Result<(), Error> {
    super::graphs::admit_inputs(tx, input, sequences)?;
    enqueue_scoped(tx, input, None)
}
fn enqueue_scoped(
    tx: &Transaction<'_>,
    input: &DispatchInput,
    report: Option<&ReportGrant>,
) -> Result<(), Error> {
    input.validate()?;
    let encoded = canonical::encode_payload(&serde_json::to_value(input)?)?;
    let digest = hagency_core::project::hash(encoded.as_bytes());
    let old: Option<String> = tx
        .query_row(
            "SELECT digest FROM runner_dispatches WHERE id=?1",
            [&input.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = old {
        return if old == digest {
            Ok(())
        } else {
            Err(Error::Conflict)
        };
    }
    session(tx, &input.session_id)?;
    if let Some(grant) = report {
        if input.task_id.is_some()
            || grant.task.session_id != input.session_id
            || grant.task.status != TaskState::Done
        {
            return Err(Error::RunnerAuthority);
        }
        super::task_intents::check_session_task(tx, &input.session_id, Some(&grant.task.id))?;
    } else {
        super::task_intents::check_session_task(tx, &input.session_id, input.task_id.as_deref())?;
    }
    if let Some(id) = &input.task_id {
        let t = task(tx, id)?;
        if t.session_id != input.session_id {
            return Err(Error::RunnerAuthority);
        }
        super::task_intents::check_enqueue(tx, &t)?;
    }
    bounded_row(tx, "runner_dispatches", "id", &input.id, 30_000)?;
    tx.execute("INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state) VALUES(?1,?2,?3,?4,?5,'queued')",params![input.id,input.session_id,input.task_id,encoded,digest])?;
    super::attachments::freeze(tx, &input.id, &input.session_id)?;
    for r in &input.resources {
        let dirty: bool = tx
            .query_row(
                "SELECT dirty FROM workspace_resources WHERE id=?1",
                [&r.id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if dirty {
            return Err(Error::Quarantined);
        }
        tx.execute(
            "INSERT INTO dispatch_resources(dispatch_id,resource_id,exclusive) VALUES(?1,?2,?3)",
            params![input.id, r.id, r.exclusive],
        )?;
    }
    Ok(())
}
fn visible(t: &Task, d: &Dispatch) -> bool {
    if let Some(id) = &d.report_task {
        return &t.id == id;
    }
    d.task_id.as_deref() == Some(&t.id) || t.creator_session_id.as_deref() == Some(&d.session_id)
}
pub(super) fn start_in_transaction(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    now: u64,
) -> Result<Value, Error> {
    let d = authorize(tx, cap, now, &["leased"])?;
    super::peers::validate_dispatch(tx, &cap.dispatch_id, &d.session_id)?;
    if let Some(id) = &d.task_id {
        let mut t = task(tx, id)?;
        if t.session_id != d.session_id {
            return Err(Error::RunnerAuthority);
        }
        super::task_intents::start_task(tx, &mut t, &cap.dispatch_id, now)?;
        if matches!(t.status, TaskState::Created | TaskState::Accepted) {
            t.status = TaskState::InProgress;
            t.updated_at = now;
            t.started_at.get_or_insert(now);
            save_task(tx, &t, "started")?;
        }
    }
    tx.execute(
        "UPDATE runner_dispatches SET state='started' WHERE id=?1",
        [&cap.dispatch_id],
    )?;
    super::graphs::started(tx, &cap.dispatch_id)?;
    tx.execute(
        "UPDATE runner_attempts SET outcome='started' WHERE dispatch_id=?1 AND fence=?2",
        params![cap.dispatch_id, cap.fence],
    )?;
    let input: DispatchInput = serde_json::from_str(&d.input)?;
    Ok(input.payload)
}

pub(super) fn complete_in_transaction(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    output: &Value,
    now: u64,
) -> Result<(), Error> {
    if serialize(output)?.len() > 32 * 1024 {
        return Err(hagency_core::InvalidInput("output exceeds 32 KiB").into());
    }
    let d = authorize(tx, cap, now, &["started"])?;
    super::graphs::complete_guard(tx, &d)?;
    super::file_delivery::complete_guard(tx, &cap.dispatch_id)?;
    tx.execute(
        "INSERT INTO runner_outputs(dispatch_id,fence,output,accepted) VALUES(?1,?2,?3,1)",
        params![cap.dispatch_id, cap.fence, serialize(output)?],
    )?;
    tx.execute(
        "UPDATE runner_attempts SET outcome='completed' WHERE dispatch_id=?1 AND fence=?2",
        params![cap.dispatch_id, cap.fence],
    )?;
    tx.execute("UPDATE runner_dispatches SET state='completed',capability_hash=NULL,lease_until=NULL,capability_until=NULL WHERE id=?1",[&cap.dispatch_id])?;
    super::messages::complete_inputs(tx, &cap.dispatch_id, now)?;
    super::peers::complete_inputs(tx, &cap.dispatch_id, now)?;
    tx.execute(
        "DELETE FROM resource_leases WHERE dispatch_id=?1",
        [&cap.dispatch_id],
    )?;
    Ok(())
}

pub(super) fn mutate_in_transaction(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    id: &str,
    call_id: &str,
    mutation: &TaskMutation,
    digest: &str,
    now: u64,
) -> Result<MutationResult, Error> {
    let d = authorize(tx, cap, now, &["started"])?;
    let mut t = task(tx, id)?;
    if d.task_id.as_deref() != Some(id) || t.session_id != d.session_id {
        return Err(Error::RunnerAuthority);
    }
    let prior:Option<(String,String)>=tx.query_row("SELECT digest,response FROM task_operation_receipts WHERE dispatch_id=?1 AND call_id=?2",params![cap.dispatch_id,call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((old, response)) = prior {
        if old != digest {
            return Err(Error::Conflict);
        }
        let task = serde_json::from_str(&response)?;
        return Ok(MutationResult {
            task,
            replayed: true,
        });
    }
    if t.status == TaskState::Done {
        return Err(Error::State);
    }
    let kind = match mutation {
        TaskMutation::Accept => {
            if !t.status.permits(TaskState::Accepted) {
                return Err(Error::State);
            }
            t.status = TaskState::Accepted;
            "accepted"
        }
        TaskMutation::Transition {
            status,
            waiting_reason,
            waiting_until,
        } => {
            if !t.status.permits(*status) {
                return Err(Error::State);
            }
            if *status == TaskState::Blocked {
                text(waiting_reason.as_deref().unwrap_or(""), 1024)?;
                text(waiting_until.as_deref().unwrap_or(""), 64)?;
                t.waiting_reason = waiting_reason.clone();
                t.waiting_until = waiting_until.clone();
            } else {
                t.waiting_reason = None;
                t.waiting_until = None;
            }
            t.status = *status;
            if *status == TaskState::Done {
                t.execution_epoch = t
                    .execution_epoch
                    .checked_add(1)
                    .filter(|v| *v <= JSON_SAFE_MAX)
                    .ok_or(Error::Capacity)?;
                t.completed_at = Some(now);
            }
            "transition"
        }
        TaskMutation::Comment { text: body } => {
            text(body, 8192)?;
            let count: u32 = tx.query_row(
                "SELECT COUNT(*) FROM task_comments WHERE task_id=?1",
                [id],
                |r| r.get(0),
            )?;
            if count >= 1000 {
                return Err(Error::Capacity);
            }
            let binding = session(tx, &d.session_id)?;
            let agent = read_engagement(tx, binding.engagement_id())?;
            tx.execute(
                "INSERT INTO task_comments(task_id,author,body,created_at) VALUES(?1,?2,?3,?4)",
                params![id, agent.agent_name.as_str(), body, now],
            )?;
            "comment"
        }
        TaskMutation::Execution {
            heartbeat,
            waiting_reason,
            waiting_until,
        } => {
            if let TextPatch::Value(v) = waiting_reason {
                if let Some(v) = v {
                    text(v, 1024)?;
                }
                t.waiting_reason = v.clone();
            }
            if let TextPatch::Value(v) = waiting_until {
                if let Some(v) = v {
                    text(v, 64)?;
                }
                t.waiting_until = v.clone();
            }
            if *heartbeat {
                t.heartbeat_at = Some(now);
            }
            "execution"
        }
    };
    t.updated_at = now;
    let count: u32 = tx.query_row(
        "SELECT COUNT(*) FROM task_operation_receipts WHERE dispatch_id=?1",
        [&cap.dispatch_id],
        |r| r.get(0),
    )?;
    if count >= 4096 {
        return Err(Error::Capacity);
    }
    save_task(tx, &t, kind)?;
    tx.execute("INSERT INTO task_operation_receipts(dispatch_id,call_id,digest,response) VALUES(?1,?2,?3,?4)",params![cap.dispatch_id,call_id,digest,serialize(&t)?])?;
    Ok(MutationResult {
        task: t,
        replayed: false,
    })
}

impl DomainRepository {
    pub fn check_runner(&self, cap: &RunnerCapability, now: u64) -> Result<(), Error> {
        authorize(&self.db, cap, now, &["started"])?;
        Ok(())
    }
    /// Host-only route binding, after the transport verifies room membership and scope.
    /// No native ingress currently exposes this constructor.
    pub fn register_session(&mut self, binding: &SessionBinding) -> Result<(), Error> {
        binding.validate()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT binding FROM runner_sessions WHERE id=?1",
                [&binding.id],
                |r| r.get(0),
            )
            .optional()?;
        let encoded = serialize(binding)?;
        if let Some(old) = previous {
            if old != encoded {
                return Err(Error::Conflict);
            }
        } else {
            if super::messages::find_session(&tx, binding)?.is_some() {
                return Err(Error::Conflict);
            }
            bounded_row(&tx, "runner_sessions", "id", &binding.id, 10_000)?;
            tx.execute(
                "INSERT INTO runner_sessions(id,engagement_id,binding) VALUES(?1,?2,?3)",
                params![binding.id, binding.engagement_id, encoded],
            )?;
        }
        session(&tx, &binding.id)?;
        tx.commit()?;
        Ok(())
    }
    pub fn register_workspace(&mut self, id: &str) -> Result<(), Error> {
        identifier(id, 128)?;
        bounded_row(&self.db, "workspace_resources", "id", id, 2048)?;
        self.db.execute(
            "INSERT OR IGNORE INTO workspace_resources(id) VALUES(?1)",
            [id],
        )?;
        Ok(())
    }
    pub fn create_canonical_task(
        &mut self,
        id: &str,
        session_id: &str,
        title: &str,
        now: u64,
    ) -> Result<Task, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = create_task(&tx, id, session_id, None, title, now)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn create_coordinator_task(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        target_session: &str,
        title: &str,
        now: u64,
    ) -> Result<Task, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let creator = authorize_work(&tx, cap, now)?;
        let a = session(&tx, &creator.session_id)?;
        let b = session(&tx, target_session)?;
        let same_project: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM engagements a JOIN engagements b ON a.fleet_id=b.fleet_id AND a.project_id=b.project_id WHERE a.id=?1 AND b.id=?2)",params![a.engagement_id(),b.engagement_id()],|r|r.get(0))?;
        if !same_project {
            return Err(Error::RunnerAuthority);
        }
        let value = create_task(
            &tx,
            id,
            target_session,
            Some(&creator.session_id),
            title,
            now,
        )?;
        tx.commit()?;
        Ok(value)
    }
    pub fn enqueue_dispatch(&mut self, input: &DispatchInput) -> Result<(), Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        enqueue(&tx, input)?;
        tx.commit()?;
        Ok(())
    }
    pub fn claim_dispatch(
        &mut self,
        runner: &str,
        now: u64,
        lease_ms: u64,
        capability_ms: u64,
        max_live: u32,
    ) -> Result<Option<RunnerCapability>, Error> {
        // Preserve legacy pre-lock validation and error precedence. The new
        // host writer path below obtains its own clock only after the lock.
        identifier(runner, 128)?;
        clock(now)?;
        if !(1..=300_000).contains(&lease_ms)
            || !(1..=300_000).contains(&capability_ms)
            || !(1..=128).contains(&max_live)
            || now > JSON_SAFE_MAX - 300_000
        {
            return Err(hagency_core::InvalidInput("invalid dispatch lease limits").into());
        }
        self.claim_clock(runner, lease_ms, capability_ms, max_live, None, || Ok(now))
    }
    /// Host-only compatible admission. No runtime route supplies this profile.
    pub fn claim_owned_dispatch_for_host(
        &mut self,
        profile: &super::OwnedClaimProfile,
        runner: &str,
        now: u64,
        lease_ms: u64,
        capability_ms: u64,
        max_live: u32,
    ) -> Result<Option<RunnerCapability>, Error> {
        self.claim_owned_clock(profile, runner, lease_ms, capability_ms, max_live, || {
            Ok(now)
        })
    }
    pub(crate) fn claim_owned_clock(
        &mut self,
        profile: &super::OwnedClaimProfile,
        runner: &str,
        lease_ms: u64,
        capability_ms: u64,
        max_live: u32,
        time: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<Option<RunnerCapability>, Error> {
        self.claim_clock(
            runner,
            lease_ms,
            capability_ms,
            max_live,
            Some(profile.encoded()),
            time,
        )
    }
    fn claim_clock(
        &mut self,
        runner: &str,
        lease_ms: u64,
        capability_ms: u64,
        max_live: u32,
        profile: Option<&str>,
        time: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<Option<RunnerCapability>, Error> {
        identifier(runner, 128)?;
        if !(1..=300_000).contains(&lease_ms)
            || !(1..=300_000).contains(&capability_ms)
            || !(1..=128).contains(&max_live)
        {
            return Err(hagency_core::InvalidInput("invalid dispatch lease limits").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.accounts.check_profile(&tx, profile)?;
        let now = time()?;
        clock(now)?;
        if now > JSON_SAFE_MAX - 300_000 {
            return Err(hagency_core::InvalidInput("invalid dispatch lease limits").into());
        }
        super::graphs::reconcile(&tx, now)?;
        super::matrix_routes::reconcile(&tx, now)?;
        expire(&tx, now)?;
        let live: u32 = tx.query_row(
            "SELECT COUNT(*) FROM runner_dispatches d WHERE state IN ('leased','started','parked') OR EXISTS(SELECT 1 FROM unresolved_dispatches u WHERE u.id=d.id)",
            [],
            |r| r.get(0),
        )?;
        if live >= max_live {
            tx.commit()?;
            return Ok(None);
        }
        let id:Option<String>=tx.query_row("SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagements e ON e.id=s.engagement_id JOIN registrations g ON g.fleet_id=e.fleet_id WHERE d.state='queued' AND (?2 IS NULL OR EXISTS(SELECT 1 FROM effects ae WHERE ae.engagement_id=e.id AND ae.kind='provision' AND ae.state='complete' AND ((json_extract(?2,'$.account') IS NULL AND NOT EXISTS(SELECT 1 FROM resource_accounts ra WHERE ra.preset_id=json_extract(ae.payload,'$.resource.presetId'))) OR EXISTS(SELECT 1 FROM resource_accounts ra JOIN managed_accounts ma ON ma.id=ra.account_id WHERE ra.preset_id=json_extract(ae.payload,'$.resource.presetId') AND ma.state='active' AND ma.id=json_extract(?2,'$.account.id') AND ma.generation=ra.binding_generation AND ma.generation=json_extract(?2,'$.account.generation') AND ma.seat_id=json_extract(?2,'$.account.seat') AND ma.seat_id=json_extract(ae.payload,'$.resource.seatId'))))) AND d.not_before<=?1 AND (?2 IS NULL OR json_extract(?2,'$.dispatch_id') IS NULL OR d.id=json_extract(?2,'$.dispatch_id')) AND (d.task_id IS NULL OR EXISTS(SELECT 1 FROM canonical_tasks t WHERE t.id=d.task_id AND (json_extract(t.config,'$.status')<>'done' OR EXISTS(SELECT 1 FROM task_followup_ready followup WHERE followup.dispatch_id=d.id AND followup.task_id=t.id)) AND NOT EXISTS(SELECT 1 FROM task_intents ti WHERE ti.task_id=t.id AND (ti.state<>'active' OR NOT EXISTS(SELECT 1 FROM task_dispatch_input_ready ready WHERE ready.dispatch_id=d.id AND ready.task_id=t.id))))) AND NOT EXISTS(SELECT 1 FROM task_intents current_task WHERE current_task.session_id=d.session_id AND (current_task.state<>'active' OR (current_task.task_id IS NOT d.task_id AND NOT EXISTS(SELECT 1 FROM current_recovery_reports cr WHERE cr.dispatch_id=d.id AND cr.task_id=current_task.task_id)))) AND (json_extract(s.binding,'$.kind') IS NULL OR (json_extract(s.binding,'$.kind')='internal' AND EXISTS(SELECT 1 FROM internal_participants ip JOIN internal_conversations ic ON ic.id=ip.conversation_id WHERE ip.session_id=s.id AND ip.engagement_id=e.id AND ip.conversation_id=json_extract(s.binding,'$.conversation_id') AND ic.state='active' AND ic.fleet_id=e.fleet_id AND ic.project_id=e.project_id AND ic.generation=e.generation))) AND NOT EXISTS(SELECT 1 FROM peer_dispatch_inputs pi WHERE pi.dispatch_id=d.id AND NOT EXISTS(SELECT 1 FROM admissible_dispatch_peer_inputs li WHERE li.dispatch_id=d.id AND li.message_sequence=pi.message_sequence)) AND NOT EXISTS(SELECT 1 FROM graph_nodes gn WHERE gn.task_id=d.task_id AND NOT EXISTS(SELECT 1 FROM graph_dispatch_ready gr WHERE gr.dispatch_id=d.id)) AND NOT EXISTS(SELECT 1 FROM dispatch_recovery_reports rr JOIN graph_nodes gn ON gn.task_id=rr.task_id WHERE rr.dispatch_id=d.id AND NOT EXISTS(SELECT 1 FROM graph_dispatch_scope gs WHERE gs.dispatch_id=d.id)) AND NOT EXISTS(SELECT 1 FROM dispatch_recovery_reports rr WHERE rr.dispatch_id=d.id AND NOT EXISTS(SELECT 1 FROM current_recovery_reports cr WHERE cr.dispatch_id=d.id)) AND (s.matrix_generation=0 OR EXISTS(SELECT 1 FROM current_matrix_routes cm WHERE cm.session_id=s.id)) AND s.quarantined=0 AND e.state='active' AND e.generation=g.generation AND NOT EXISTS(SELECT 1 FROM runner_dispatches live WHERE live.session_id=d.session_id AND live.state IN ('leased','started','parked')) AND NOT EXISTS(SELECT 1 FROM dispatch_resources dr JOIN workspace_resources w ON w.id=dr.resource_id WHERE dr.dispatch_id=d.id AND (w.dirty=1 OR EXISTS(SELECT 1 FROM resource_leases l WHERE l.resource_id=dr.resource_id AND (l.exclusive=1 OR dr.exclusive=1)))) AND (?2 IS NULL OR (EXISTS(SELECT 1 FROM canonical_tasks host_task WHERE host_task.id=d.task_id AND json_extract(host_task.config,'$.status')<>'done') AND NOT EXISTS(SELECT 1 FROM dispatch_recovery_reports host_report WHERE host_report.dispatch_id=d.id) AND EXISTS(SELECT 1 FROM current_matrix_routes cm JOIN matrix_session_routes mr ON mr.session_id=cm.session_id WHERE cm.session_id=d.session_id AND json_extract(mr.config,'$.encrypted')=1 AND json_extract(mr.config,'$.engagement_id')=json_extract(?2,'$.transport.engagement_id') AND json_extract(mr.config,'$.registration_generation')=json_extract(?2,'$.transport.registration_generation') AND json_extract(mr.config,'$.transport_generation')=json_extract(?2,'$.transport.generation') AND json_extract(mr.config,'$.sender_mxid')=json_extract(?2,'$.transport.sender_mxid') AND json_extract(mr.config,'$.device_id')=json_extract(?2,'$.transport.device_id') AND EXISTS(SELECT 1 FROM json_each(?2,'$.rooms') room WHERE json_extract(room.value,'$.id')=json_extract(mr.config,'$.room_id') AND json_extract(room.value,'$.generation')=json_extract(mr.config,'$.room_generation') AND json_extract(room.value,'$.privacy.kind')=json_extract(mr.config,'$.privacy.kind') AND json_extract(room.value,'$.privacy.human_mxid') IS json_extract(mr.config,'$.privacy.human_mxid'))) AND (SELECT COUNT(*) FROM dispatch_resources dr WHERE dr.dispatch_id=d.id)=1 AND EXISTS(SELECT 1 FROM dispatch_resources dr JOIN json_each(?2,'$.workspaces') workspace ON workspace.value=dr.resource_id WHERE dr.dispatch_id=d.id AND dr.exclusive=1) AND EXISTS(SELECT 1 FROM effects effect WHERE effect.engagement_id=e.id AND effect.kind='provision' AND effect.state='complete' AND json_extract(effect.payload,'$.resource.framework')='codex' AND (json_extract(effect.payload,'$.resource.provider') IS NULL OR json_extract(effect.payload,'$.resource.provider')='openai') AND (json_extract(effect.payload,'$.resource.reasoning') IS NULL OR json_extract(effect.payload,'$.resource.reasoning') IN ('none','minimal','low','medium','high','xhigh'))))) ORDER BY d.rowid LIMIT 1",params![now,profile],|r|r.get(0)).optional()?;
        let Some(id) = id else {
            tx.commit()?;
            return Ok(None);
        };
        let d = dispatch(&tx, &id)?;
        let fence = d
            .fence
            .checked_add(1)
            .filter(|n| *n <= JSON_SAFE_MAX)
            .ok_or(Error::Capacity)?;
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
        let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let hash = canonical::digest(&json!(secret))?;
        tx.execute("UPDATE runner_dispatches SET state='leased',fence=?2,runner_id=?3,capability_hash=?4,lease_until=?5,capability_until=?6 WHERE id=?1",params![id,fence,runner,hash,now+lease_ms,now+capability_ms])?;
        tx.execute("INSERT INTO runner_attempts(dispatch_id,fence,runner_id,outcome,capability_hash,created_at) VALUES(?1,?2,?3,'leased',?4,?5)",params![id,fence,runner,hash,now])?;
        tx.execute("INSERT INTO resource_leases(resource_id,dispatch_id,exclusive) SELECT resource_id,dispatch_id,exclusive FROM dispatch_resources WHERE dispatch_id=?1",[&id])?;
        tx.commit()?;
        Ok(Some(RunnerCapability {
            dispatch_id: id,
            runner_id: runner.into(),
            fence,
            secret,
        }))
    }
    /// The response is intentionally one-shot: a lost start response is unknown work.
    pub fn start_dispatch(&mut self, cap: &RunnerCapability, now: u64) -> Result<Value, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let payload = start_in_transaction(&tx, cap, now)?;
        tx.commit()?;
        Ok(payload)
    }
    pub fn park_dispatch(
        &mut self,
        cap: &RunnerCapability,
        parked: bool,
        now: u64,
    ) -> Result<(), Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        authorize(
            &tx,
            cap,
            now,
            if parked { &["started"] } else { &["parked"] },
        )?;
        if !parked {
            super::approvals::check_resume(&tx, &cap.dispatch_id, cap.fence)?;
        }
        let state = if parked { "parked" } else { "started" };
        tx.execute(
            "UPDATE runner_dispatches SET state=?2 WHERE id=?1",
            params![cap.dispatch_id, state],
        )?;
        tx.execute(
            "UPDATE runner_attempts SET outcome=?3 WHERE dispatch_id=?1 AND fence=?2",
            params![cap.dispatch_id, cap.fence, state],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn renew_dispatch(
        &mut self,
        cap: &RunnerCapability,
        now: u64,
        lease_ms: u64,
    ) -> Result<(), Error> {
        if !(1..=300_000).contains(&lease_ms) || now > JSON_SAFE_MAX - lease_ms {
            return Err(hagency_core::InvalidInput("invalid lease renewal").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let d = authorize(&tx, cap, now, &["leased", "started", "parked"])?;
        tx.execute(
            "UPDATE runner_dispatches SET lease_until=?2 WHERE id=?1",
            params![cap.dispatch_id, (now + lease_ms).min(d.expiry)],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Host runner adapter must have definitively observed that no process started.
    pub fn fail_before_start(
        &mut self,
        cap: &RunnerCapability,
        now: u64,
        retry_ms: u64,
    ) -> Result<(), Error> {
        if !(1..=300_000).contains(&retry_ms) || now > JSON_SAFE_MAX - retry_ms {
            return Err(hagency_core::InvalidInput("invalid launch backoff").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        authorize_attempt(&tx, cap, now, &["leased"])?;
        lose(&tx, &cap.dispatch_id)?;
        tx.execute(
            "UPDATE runner_dispatches SET not_before=?2 WHERE id=?1",
            params![cap.dispatch_id, now + retry_ms],
        )?;
        tx.execute(
            "UPDATE runner_attempts SET outcome='spawn_failed' WHERE dispatch_id=?1 AND fence=?2",
            params![cap.dispatch_id, cap.fence],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn complete_dispatch(
        &mut self,
        cap: &RunnerCapability,
        output: &Value,
        now: u64,
    ) -> Result<(), Error> {
        // Preserve this API's existing validation before taking the writer lock.
        if serialize(output)?.len() > 32 * 1024 {
            return Err(hagency_core::InvalidInput("output exceeds 32 KiB").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        complete_in_transaction(&tx, cap, output, now)?;
        tx.commit()?;
        Ok(())
    }
    /// Preserve authenticated old-attempt output for inspection, never task mutation/delivery.
    pub fn record_late_output(
        &mut self,
        cap: &RunnerCapability,
        output: &Value,
    ) -> Result<(), Error> {
        let encoded = serialize(output)?;
        if encoded.len() > 32 * 1024 {
            return Err(Error::Capacity);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior:Option<(String,String)>=tx.query_row("SELECT runner_id,capability_hash FROM runner_attempts WHERE dispatch_id=?1 AND fence=?2",params![cap.dispatch_id,cap.fence],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let (runner, hash) = prior.ok_or(Error::RunnerAuthority)?;
        if runner != cap.runner_id || !matches_secret(&hash, &cap.secret)? {
            return Err(Error::RunnerAuthority);
        }
        let count: u32 = tx.query_row(
            "SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id=?1 AND fence=?2",
            params![cap.dispatch_id, cap.fence],
            |r| r.get(0),
        )?;
        if count >= 128 {
            return Err(Error::Capacity);
        }
        tx.execute(
            "INSERT INTO runner_outputs(dispatch_id,fence,output,accepted) VALUES(?1,?2,?3,0)",
            params![cap.dispatch_id, cap.fence, encoded],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn reconcile_dispatches(&mut self, now: u64) -> Result<(), Error> {
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        expire(&tx, now)?;
        tx.commit()?;
        Ok(())
    }
    /// Operator-only inspection command. The real process adapter must first prove
    /// the old process stopped and inspect every named workspace. Never runner-callable.
    pub fn recover_dispatch(
        &mut self,
        original: &str,
        replacement: &DispatchInput,
        evidence: &str,
        now: u64,
    ) -> Result<(), Error> {
        text(evidence, 4096)?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let d = dispatch(&tx, original)?;
        if d.state != "outcome_unknown"
            || original == replacement.id
            || d.session_id != replacement.session_id
        {
            return Err(Error::State);
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM dispatch_stops WHERE dispatch_id=?1)",
            [original],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::State);
        }
        let report = if let Some(id) = &d.task_id {
            let original_task = task(&tx, id)?;
            (original_task.status == TaskState::Done).then_some(ReportGrant {
                task: original_task,
            })
        } else {
            report_grant(&tx, original)?
        };
        if (report.is_some() && replacement.task_id.is_some())
            || (report.is_none() && d.task_id != replacement.task_id)
        {
            return Err(Error::State);
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runner_dispatches WHERE id=?1)",
            [&replacement.id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::Conflict);
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM dispatch_recoveries WHERE original_id=?1)",
            [original],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::Conflict);
        }
        let old: DispatchInput = serde_json::from_str(&d.input)?;
        if serialize(&old.resources)? != serialize(&replacement.resources)?
            || canonical::encode_payload(&old.payload)?
                == canonical::encode_payload(&replacement.payload)?
        {
            return Err(Error::State);
        }
        // Another unknown writer must be inspected separately before clearing shared state.
        let another:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM unresolved_dispatches d WHERE d.id<>?1 AND (d.session_id=?2 OR EXISTS(SELECT 1 FROM dispatch_resources a JOIN dispatch_resources b ON a.resource_id=b.resource_id WHERE a.dispatch_id=d.id AND b.dispatch_id=?1 AND a.exclusive=1)))",params![original,d.session_id],|r|r.get(0))?;
        if another {
            return Err(Error::Quarantined);
        }
        super::graphs::admit_recovery(
            &tx,
            original,
            report
                .as_ref()
                .map(|r| r.task.id.as_str())
                .or(d.task_id.as_deref()),
            &d.session_id,
        )?;
        // Host inspection closes this attempt's physical custody. The recovery
        // intent acquires its own leases when claimed, in the same way as other
        // queued work. Never release a different unresolved reader's lease.
        tx.execute(
            "DELETE FROM resource_leases WHERE dispatch_id=?1",
            [original],
        )?;
        tx.execute(
            "UPDATE runner_sessions SET quarantined=0 WHERE id=?1",
            [&d.session_id],
        )?;
        tx.execute("UPDATE workspace_resources SET dirty=0 WHERE id IN (SELECT resource_id FROM dispatch_resources WHERE dispatch_id=?1 AND exclusive=1)",[original])?;
        // Older queued instructions cannot outrun an explicit recovery instruction.
        tx.execute("UPDATE runner_dispatches SET state='superseded' WHERE session_id=?1 AND state='queued'",[&d.session_id])?;
        let replacement = super::messages::recovery_input(&tx, original, replacement)?;
        let replacement = super::peers::recovery_input(&tx, original, &replacement)?;
        enqueue_scoped(&tx, &replacement, report.as_ref())?;
        super::messages::transfer_inputs(&tx, original, &replacement.id)?;
        super::peers::transfer_inputs(&tx, original, &replacement.id)?;
        tx.execute("INSERT INTO dispatch_recoveries(original_id,replacement_id,evidence,created_at) VALUES(?1,?2,?3,?4)",params![original,replacement.id,evidence,now])?;
        if let Some(grant) = report {
            tx.execute("INSERT INTO dispatch_recovery_reports(dispatch_id,task_id,execution_epoch) VALUES(?1,?2,?3)",params![replacement.id,grant.task.id,grant.task.execution_epoch])?;
            report_grant(&tx, &replacement.id)?.ok_or(Error::RunnerAuthority)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn canonical_task(&self, id: &str) -> Result<Task, Error> {
        task(&self.db, id)
    }
    pub fn runner_task(&self, cap: &RunnerCapability, id: &str, now: u64) -> Result<Task, Error> {
        let d = authorize(&self.db, cap, now, &["started"])?;
        let t = task(&self.db, id)?;
        if !visible(&t, &d) {
            return Err(Error::RunnerAuthority);
        }
        Ok(t)
    }
    pub fn runner_tasks(
        &self,
        cap: &RunnerCapability,
        after: &str,
        limit: usize,
        now: u64,
    ) -> Result<Vec<Task>, Error> {
        let d = authorize(&self.db, cap, now, &["started"])?;
        if !(1..=100).contains(&limit) {
            return Err(hagency_core::InvalidInput("task page must be 1..100").into());
        }
        if let Some(id) = &d.report_task {
            return Ok(if id.as_str() > after {
                vec![task(&self.db, id)?]
            } else {
                vec![]
            });
        }
        self.db.prepare("SELECT config FROM canonical_tasks WHERE id>?1 AND (id=?2 OR creator_session_id=?3) ORDER BY id LIMIT ?4")?.query_map(params![after,d.task_id,d.session_id,limit],|r|r.get::<_,String>(0))?.map(|row|Ok(serde_json::from_str(&row?)?)).collect()
    }
    pub fn mutate_task(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        call_id: &str,
        mutation: &TaskMutation,
        now: u64,
    ) -> Result<MutationResult, Error> {
        identifier(call_id, 512)?;
        let digest = canonical::digest(&json!([id, mutation]))?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = mutate_in_transaction(&tx, cap, id, call_id, mutation, &digest, now)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn runner_comments(
        &self,
        cap: &RunnerCapability,
        id: &str,
        after: u64,
        limit: usize,
        now: u64,
    ) -> Result<Vec<TaskComment>, Error> {
        self.runner_task(cap, id, now)?;
        if !(1..=100).contains(&limit) {
            return Err(hagency_core::InvalidInput("comment page must be 1..100").into());
        }
        Ok(self.db.prepare("SELECT sequence,author,body,created_at FROM task_comments WHERE task_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3")?.query_map(params![id,after,limit],|r|Ok(TaskComment{sequence:r.get(0)?,author:r.get(1)?,text:r.get(2)?,created_at:r.get(3)?}))?.collect::<Result<Vec<_>,_>>()?)
    }
    /// Internal durable projection, scoped delivery is supplied by the transport adapter.
    pub fn task_events(&self, after: u64, limit: usize) -> Result<Vec<TaskEvent>, Error> {
        if !(1..=100).contains(&limit) {
            return Err(hagency_core::InvalidInput("event page must be 1..100").into());
        }
        self.db.prepare("SELECT sequence,task_id,kind,task FROM task_outbox WHERE delivered=0 AND sequence>?1 ORDER BY sequence LIMIT ?2")?.query_map(params![after,limit],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,String>(3)?)))?.map(|row|{let(sequence,task_id,kind,value)=row?;Ok(TaskEvent{sequence,task_id,kind,task:serde_json::from_str(&value)?})}).collect()
    }
}
