//! Explicit operator decisions over original stopped-owner evidence.
use super::{DomainRepository, execution, serialize};
use crate::Error;
use hagency_core::{
    JSON_SAFE_MAX, canonical,
    project::identifier,
    tasks::{DispatchInput, TaskState, clock, text},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeAction {
    Continue,
    AcceptCompleted,
    KeepBlocked,
}

// Do not derive Debug: the request contains a one-use inspection secret.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OutcomeResolution {
    pub original: String,
    pub request_id: String,
    pub inspection_id: String,
    pub inspection_token: String,
    pub action: OutcomeAction,
    pub operator_note: String,
    pub replacement: Option<DispatchInput>,
}
impl OutcomeResolution {
    fn validate(&self) -> Result<(), Error> {
        for id in [&self.original, &self.request_id, &self.inspection_id] {
            identifier(id, 128)?;
        }
        text(&self.operator_note, 2000)?;
        if self.inspection_token.len() != 64
            || !self.inspection_token.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(hagency_core::InvalidInput("invalid inspection credential").into());
        }
        match (&self.action, &self.replacement) {
            (OutcomeAction::Continue, Some(next)) => next.validate()?,
            (OutcomeAction::AcceptCompleted | OutcomeAction::KeepBlocked, None) => {}
            _ => {
                return Err(hagency_core::InvalidInput(
                    "replacement is required only for continuation",
                )
                .into());
            }
        }
        Ok(())
    }
    fn digest(&self) -> Result<String, Error> {
        let mut value = serde_json::to_value(self)?;
        value["inspectionToken"] = json!(canonical::digest(&json!(self.inspection_token))?);
        Ok(canonical::payload_digest(&value)?)
    }
}
fn secret() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn bound(db: &Connection, engagement: &str, id: &str) -> Result<execution::Dispatch, Error> {
    identifier(engagement, 128)?;
    identifier(id, 128)?;
    let d = execution::dispatch(db, id)?;
    let matches: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM runner_sessions WHERE id=?1 AND engagement_id=?2)",
        params![d.session_id, engagement],
        |r| r.get(0),
    )?;
    if !matches {
        return Err(Error::NotFound);
    }
    Ok(d)
}
struct Snapshot {
    fence: u64,
    receipt: String,
    digest: String,
    value: Value,
}
fn snapshot(db: &Connection, id: &str, d: &execution::Dispatch) -> Result<Snapshot, Error> {
    if d.state != "outcome_unknown" {
        return Err(Error::State);
    }
    let receipt:Option<(String,String)>=db.query_row(
        "SELECT i.digest,i.config FROM owned_stop_inspections i JOIN dispatch_stops s ON s.dispatch_id=i.dispatch_id AND s.fence=i.fence WHERE i.dispatch_id=?1 AND i.fence=?2 AND s.reason='owned_runner_failure' AND s.settled_at IS NULL",
        params![id,d.fence],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    // ADR-182 decision 3 (the console route, chosen 2026-09-23): an unproven
    // stop has no host receipt by ADR-162's own rule. When an open agent
    // fence names this attempt, the attempt's recorded stop evidence
    // (ADR-181, `stop_reported`) is the inspection material — what the
    // guardian reported and why it could not prove the tree gone — and its
    // digest is the receipt the resolution is bound to. Such an inspection
    // settles only: `continue` still needs the host receipt (the proof that
    // the workspace is free), which the fenced attempt lacks.
    let (receipt, observation, fenced) = match receipt {
        Some((receipt, observation)) => (receipt, observation, false),
        None => {
            let evidence: Option<String> = db
                .query_row(
                    "SELECT e.detail FROM runner_attempt_events e WHERE e.dispatch_id=?1 AND e.fence=?2 AND e.phase='stop_reported' AND EXISTS(SELECT 1 FROM agent_fences af WHERE af.dispatch_id=?1 AND af.fence=?2 AND af.cleared_at IS NULL) ORDER BY e.seq DESC LIMIT 1",
                    params![id, d.fence],
                    |r| r.get(0),
                )
                .optional()?;
            let evidence = evidence.ok_or(Error::State)?;
            let receipt = canonical::digest(&json!(["agent_fence", &evidence]))?;
            (receipt, evidence, true)
        }
    };
    let observation: Value = serde_json::from_str(&observation)?;
    if !fenced {
        let scope = super::owned_dispatch::inspection_projection(db, id, d)?;
        if observation.get("scope").and_then(Value::as_str) != Some(scope.fingerprint()) {
            return Err(Error::Conflict);
        }
    }
    let task = execution::task(db, d.task_id.as_deref().ok_or(Error::State)?)?;
    if task.status == TaskState::Done {
        return Err(Error::State);
    }
    let held:bool=db.query_row(
        "SELECT EXISTS(SELECT 1 FROM runner_sessions WHERE id=?2 AND quarantined=1) AND EXISTS(SELECT 1 FROM dispatch_resources r JOIN workspace_resources w ON w.id=r.resource_id WHERE r.dispatch_id=?1 AND r.exclusive=1 AND w.dirty=1) AND NOT EXISTS(SELECT 1 FROM dispatch_recoveries WHERE original_id=?1)",
        params![id,d.session_id],|r|r.get(0))?;
    if !held {
        return Err(Error::State);
    }
    let occupied:bool=db.query_row(
        "SELECT EXISTS(SELECT 1 FROM runner_dispatches WHERE id<>?1 AND session_id=?2 AND state IN ('leased','started','parked')) OR EXISTS(SELECT 1 FROM resource_leases l JOIN dispatch_resources r ON r.resource_id=l.resource_id WHERE r.dispatch_id=?1 AND l.dispatch_id<>?1 AND (l.exclusive=1 OR r.exclusive=1)) OR EXISTS(SELECT 1 FROM unresolved_dispatches d WHERE d.id<>?1 AND (d.session_id=?2 OR EXISTS(SELECT 1 FROM dispatch_resources a JOIN dispatch_resources b ON a.resource_id=b.resource_id WHERE a.dispatch_id=d.id AND b.dispatch_id=?1 AND (a.exclusive=1 OR b.exclusive=1))))",
        params![id,d.session_id],|r|r.get(0))?;
    if occupied {
        return Err(Error::Quarantined);
    }
    super::file_delivery::complete_guard(db, id)?;
    let media:bool=db.query_row(
        "SELECT EXISTS(SELECT 1 FROM received_files f JOIN dispatch_resources r ON r.resource_id=f.workspace_id WHERE r.dispatch_id=?1 AND f.state IN ('reserved','write_possible','outcome_unknown')) OR EXISTS(SELECT 1 FROM file_uploads WHERE dispatch_id=?1 AND (outcome_unknown=1 OR stage_state='unknown' OR upload_state IN ('claimed','write_possible')))",
        [id],|r|r.get(0))?;
    if media {
        return Err(Error::State);
    }
    let route: Option<String> = db
        .query_row(
            "SELECT config FROM matrix_session_routes WHERE session_id=?1",
            [&d.session_id],
            |r| r.get(0),
        )
        .optional()?;
    let value = json!({"dispatchId":id,"fence":d.fence,"receiptDigest":receipt,"observation":observation,"fenced":fenced,"task":task,"route":route});
    let digest = canonical::payload_digest(&value)?;
    Ok(Snapshot {
        fence: d.fence,
        receipt,
        digest,
        value,
    })
}

impl DomainRepository {
    /// Private lifecycle operation. The original host receipt is prerequisite,
    /// not evidence reconstructed from this operator's request.
    pub fn begin_outcome_inspection(
        &mut self,
        engagement: &str,
        id: &str,
        ttl_ms: u64,
        now: u64,
    ) -> Result<Value, Error> {
        self.begin_outcome_inspection_clock(engagement, id, ttl_ms, || Ok(now))
    }
    pub(crate) fn begin_outcome_inspection_clock(
        &mut self,
        engagement: &str,
        id: &str,
        ttl_ms: u64,
        time: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<Value, Error> {
        if !(60_000..=3_600_000).contains(&ttl_ms) {
            return Err(
                hagency_core::InvalidInput("inspection lifetime must be 1..60 minutes").into(),
            );
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = time()?;
        clock(now)?;
        let expires = now
            .checked_add(ttl_ms)
            .filter(|n| *n <= JSON_SAFE_MAX)
            .ok_or(Error::Capacity)?;
        let d = bound(&tx, engagement, id)?;
        let snapshot = snapshot(&tx, id, &d)?;
        tx.execute(
            "DELETE FROM outcome_inspections WHERE consumed_at IS NULL AND expires_at<=?1",
            [now],
        )?;
        let (total, per_dispatch): (u64, u64) = tx.query_row(
            "SELECT COUNT(*),COALESCE(SUM(dispatch_id=?1),0) FROM outcome_inspections",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if total >= 40_000 || per_dispatch >= 16 {
            return Err(Error::Capacity);
        }
        let inspection = secret()?;
        let token = secret()?;
        tx.execute("INSERT INTO outcome_inspections(id,dispatch_id,fence,receipt_digest,snapshot_digest,token_hash,created_at,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![inspection,id,snapshot.fence,snapshot.receipt,snapshot.digest,canonical::digest(&json!(token))?,now,expires])?;
        tx.commit()?;
        Ok(
            json!({"inspectionId":inspection,"inspectionToken":token,"expiresAt":expires,"snapshot":snapshot.value}),
        )
    }

    pub fn resolve_stopped_dispatch(
        &mut self,
        engagement: &str,
        input: &OutcomeResolution,
        now: u64,
    ) -> Result<Value, Error> {
        self.resolve_stopped_dispatch_clock(engagement, input, || Ok(now))
    }
    pub(crate) fn resolve_stopped_dispatch_clock(
        &mut self,
        engagement: &str,
        input: &OutcomeResolution,
        time: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<Value, Error> {
        input.validate()?;
        let request_digest = input.digest()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = time()?;
        clock(now)?;
        let d = bound(&tx, engagement, &input.original)?;
        let prior: Option<(String, String)> = tx
            .query_row(
                "SELECT request_digest,response FROM outcome_resolutions WHERE request_id=?1",
                [&input.request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((digest, response)) = prior {
            return if digest == request_digest {
                Ok(serde_json::from_str(&response)?)
            } else {
                Err(Error::Conflict)
            };
        }
        let resolved: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM outcome_resolutions WHERE dispatch_id=?1)",
            [&input.original],
            |r| r.get(0),
        )?;
        if resolved {
            return Err(Error::Conflict);
        }
        let row:Option<(u64,String,String,String,u64,Option<u64>)>=tx.query_row(
            "SELECT fence,receipt_digest,snapshot_digest,token_hash,expires_at,consumed_at FROM outcome_inspections WHERE id=?1 AND dispatch_id=?2",
            params![input.inspection_id,input.original],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
        let (fence, receipt, digest, hash, expires, consumed) = row.ok_or(Error::State)?;
        if consumed.is_some()
            || now >= expires
            || !execution::matches_secret(&hash, &input.inspection_token)?
        {
            return Err(Error::State);
        }
        let current = snapshot(&tx, &input.original, &d)?;
        if current.fence != fence || current.receipt != receipt || current.digest != digest {
            return Err(Error::Conflict);
        }
        let action = serde_json::to_value(input.action)?
            .as_str()
            .ok_or(Error::State)?
            .to_owned();
        let evidence = serialize(
            &json!({"profile":"operator-outcome-resolution-v1","requestId":input.request_id,"inspectionId":input.inspection_id,"receiptDigest":receipt,"action":action,"operatorNote":input.operator_note}),
        )?;
        if let Some(next) = &input.replacement {
            execution::recover_dispatch_in_transaction(
                &tx,
                Some(engagement),
                &input.original,
                next,
                &evidence,
                now,
                Some((fence, &receipt)),
                "resolve_stopped_dispatch",
            )?;
            let mut task = execution::task(&tx, d.task_id.as_deref().ok_or(Error::State)?)?;
            task.status = TaskState::InProgress;
            task.waiting_reason = None;
            task.waiting_until = None;
            task.updated_at = now;
            execution::save_task(&tx, &task, "operator_continue")?;
        } else {
            if super::graphs::is_graph_task(&tx, d.task_id.as_deref())? {
                return Err(Error::State);
            }
            super::conversation_lifecycle::settle_stop_in_transaction(
                &tx,
                &input.original,
                fence,
                &evidence,
                now,
                false,
            )?;
            tx.execute("UPDATE runner_dispatches SET state='superseded' WHERE session_id=?1 AND state='queued'",[&d.session_id])?;
            let mut task = execution::task(&tx, d.task_id.as_deref().ok_or(Error::State)?)?;
            task.updated_at = now;
            task.waiting_until = None;
            let kind = match input.action {
                OutcomeAction::AcceptCompleted => {
                    task.status = TaskState::Done;
                    task.completed_at = Some(now);
                    task.waiting_reason = None;
                    task.execution_epoch = task
                        .execution_epoch
                        .checked_add(1)
                        .filter(|n| *n <= JSON_SAFE_MAX)
                        .ok_or(Error::Capacity)?;
                    "operator_accept_completed"
                }
                OutcomeAction::KeepBlocked => {
                    task.status = TaskState::Blocked;
                    task.waiting_reason =
                        Some("operator inspected runner outcome; task remains blocked".into());
                    "operator_keep_blocked"
                }
                OutcomeAction::Continue => return Err(Error::State),
            };
            execution::save_task(&tx, &task, kind)?;
            // ADR-182 decision 3: the settlement is the operator's resolution
            // of this dispatch; the continuation branch clears through the
            // recovery kernel above.
            super::agent_fences::clear_fences_for_dispatch(
                &tx,
                &input.original,
                "resolve_stopped_dispatch",
                now,
            )?;
        }
        let response = json!({"requestId":input.request_id,"original":input.original,"action":action,
            "replacement":input.replacement.as_ref().map(|r|r.id.as_str()),"resolvedAt":now,
            "task":execution::task(&tx,d.task_id.as_deref().ok_or(Error::State)?)?});
        tx.execute(
            "UPDATE outcome_inspections SET consumed_at=?2 WHERE id=?1 AND consumed_at IS NULL",
            params![input.inspection_id, now],
        )?;
        tx.execute("INSERT INTO outcome_resolutions(request_id,dispatch_id,inspection_id,request_digest,action,response,resolved_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![input.request_id,input.original,input.inspection_id,request_digest,action,serialize(&response)?,now])?;
        tx.commit()?;
        Ok(response)
    }
}
