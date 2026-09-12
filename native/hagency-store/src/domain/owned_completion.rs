//! Held content has no execution or send authority. Positive publication is a
//! trusted host seam: only the coordinator retaining the same started scope and
//! actual owner may call it after all stop facts. SQLite does not inspect kernels.
use super::{
    DomainRepository, OwnedDispatchScope, bounded_row, conversation_lifecycle, execution, graphs,
    matrix_routes, owned_dispatch, replies, serialize,
};
use crate::Error;
use hagency_core::{canonical, completions::*, replies::ReplyRoute, tasks::*};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::json;

/// Non-deserializable writer observation. A clone is not kernel custody.
#[derive(Clone)]
pub struct OwnedCompletion {
    id: String,
}
impl OwnedCompletion {
    pub fn id(&self) -> &str {
        &self.id
    }
}
fn historical(db: &Connection, cap: &RunnerCapability) -> Result<(), Error> {
    let row: Option<(String, String)> = db.query_row(
        "SELECT runner_id,capability_hash FROM runner_attempts WHERE dispatch_id=?1 AND fence=?2",
        params![cap.dispatch_id, cap.fence], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    let (runner, hash) = row.ok_or(Error::RunnerAuthority)?;
    if runner != cap.runner_id || !execution::matches_secret(&hash, &cap.secret)? {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
fn receipt(db: &Connection, id: &str, replayed: bool) -> Result<CompletionReceipt, Error> {
    let (task_id, epoch, state): (String, u64, String) = db.query_row(
        "SELECT task_id,execution_epoch,state FROM owned_task_completions WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(CompletionReceipt {
        id: id.into(),
        task_id,
        execution_epoch: epoch,
        state: serde_json::from_value(json!(state))?,
        replayed,
    })
}
fn exact(
    db: &Connection,
    cap: &RunnerCapability,
    scope: &OwnedDispatchScope,
    reference: Option<&OwnedCompletion>,
) -> Result<Option<OwnedCompletion>, Error> {
    scope.check_started(cap)?;
    historical(db, cap)?;
    let row:Option<(String,String,String,u64)> = db.query_row(
        "SELECT id,fingerprint,task_id,execution_epoch FROM owned_task_completions WHERE dispatch_id=?1 AND fence=?2",
        params![cap.dispatch_id,cap.fence],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let Some((id, fp, task, epoch)) = row else {
        return if reference.is_some() {
            Err(Error::RunnerAuthority)
        } else {
            Ok(None)
        };
    };
    if fp != scope.fingerprint()
        || task != scope.task().id
        || epoch != scope.task().execution_epoch + 1
        || reference.is_some_and(|v| v.id != id)
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(Some(OwnedCompletion { id }))
}
impl DomainRepository {
    pub fn complete_task_with_reply(
        &mut self,
        cap: &RunnerCapability,
        input: &CompleteTaskWithReply,
        now: u64,
    ) -> Result<CompletionReceipt, Error> {
        self.finish_task_clock(cap, input, || Ok(now))
    }
    pub(crate) fn finish_task_clock(
        &mut self,
        cap: &RunnerCapability,
        input: &CompleteTaskWithReply,
        clock: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<CompletionReceipt, Error> {
        input.validate()?;
        let digest = canonical::digest(&json!(["complete_task_with_reply", input]))?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = clock()?;
        hagency_core::tasks::clock(now)?;
        historical(&tx, cap)?;
        // The sole post-fence replay is an identical completion receipt. It grants
        // no read, mutation, process, recovery, approval or send capability.
        let prior:Option<(String,String)> = tx.query_row(
            "SELECT digest,response FROM task_operation_receipts WHERE dispatch_id=?1 AND call_id=?2",
            params![cap.dispatch_id,input.call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((old, _)) = prior {
            if old != digest {
                return Err(Error::Conflict);
            }
            let id:Option<String>=tx.query_row("SELECT id FROM owned_task_completions WHERE dispatch_id=?1 AND fence=?2 AND call_id=?3 AND digest=?4",
                params![cap.dispatch_id,cap.fence,input.call_id,digest],|r|r.get(0)).optional()?;
            return receipt(&tx, &id.ok_or(Error::RunnerAuthority)?, true);
        }
        let before = owned_dispatch::scope(&tx, cap, now, &["started"])?;
        if before.task().id != input.id {
            return Err(Error::RunnerAuthority);
        }
        let route = matrix_routes::route(&tx, &before.input().session_id)?;
        let deadline: u64 = tx.query_row(
            "SELECT MIN(capability_until,?2) FROM runner_dispatches WHERE id=?1",
            params![cap.dispatch_id, now.saturating_add(30_000)],
            |r| r.get(0),
        )?;
        if deadline <= now {
            return Err(Error::RunnerAuthority);
        }
        let result = execution::mutate_in_transaction(
            &tx,
            cap,
            &input.id,
            &input.call_id,
            &TaskMutation::Transition {
                status: TaskState::Done,
                waiting_reason: None,
                waiting_until: None,
            },
            &digest,
            now,
        )?;
        // Graph result submission remains its own verified operation. Do not
        // pretend plain final prose supplies a missing graph result receipt.
        graphs::complete_guard(&tx, &execution::dispatch(&tx, &cap.dispatch_id)?)?;
        super::file_delivery::complete_guard(&tx, &cap.dispatch_id)?;
        let id = format!(
            "completion_{}",
            &canonical::digest(&json!([cap.dispatch_id, cap.fence]))?[..32]
        );
        bounded_row(&tx, "owned_task_completions", "id", &id, 30_000)?;
        let session_count:u64=tx.query_row("SELECT COUNT(*) FROM owned_task_completions c JOIN runner_dispatches d ON d.id=c.dispatch_id WHERE d.session_id=?1",[&before.input().session_id],|r|r.get(0))?;
        if session_count >= 128 {
            return Err(Error::Capacity);
        }
        tx.execute("INSERT INTO owned_task_completions(id,dispatch_id,fence,task_id,execution_epoch,fingerprint,call_id,digest,body,route,deadline,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'held',?12,?12)",
            params![id,cap.dispatch_id,cap.fence,result.task.id,result.task.execution_epoch,before.fingerprint(),input.call_id,digest,input.body,serialize(&route)?,deadline,now])?;
        conversation_lifecycle::fence_dispatch(&tx, &cap.dispatch_id, "owned_completion", now)?;
        let result = receipt(&tx, &id, false)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn observe_owned_completion(
        &self,
        cap: &RunnerCapability,
        scope: &OwnedDispatchScope,
    ) -> Result<Option<OwnedCompletion>, Error> {
        exact(&self.db, cap, scope, None)
    }

    /// Positive host-only trust seam. `scope` must be retained from this exact
    /// start together with its actual process owner. No StopReport parameter, no
    /// runner endpoint, and no reconstruction from a persisted completion row.
    pub fn publish_owned_completion(
        &mut self,
        cap: &RunnerCapability,
        scope: &OwnedDispatchScope,
        reference: &OwnedCompletion,
        now: u64,
    ) -> Result<CompletionReceipt, Error> {
        self.publish_completion_clock(cap, scope, reference, || Ok(now))
    }
    pub(crate) fn publish_completion_clock(
        &mut self,
        cap: &RunnerCapability,
        scope: &OwnedDispatchScope,
        reference: &OwnedCompletion,
        clock: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<CompletionReceipt, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = clock()?;
        hagency_core::tasks::clock(now)?;
        exact(&tx, cap, scope, Some(reference))?;
        let result = receipt(&tx, &reference.id, false)?;
        if result.state != CompletionState::Held {
            return Err(Error::State);
        }
        let (deadline, body, route): (u64, String, String) = tx.query_row(
            "SELECT deadline,body,route FROM owned_task_completions WHERE id=?1",
            [&reference.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if now >= deadline {
            return Err(Error::RunnerAuthority);
        }
        let d = execution::dispatch(&tx, &cap.dispatch_id)?;
        if d.state != "outcome_unknown" || d.fence != cap.fence {
            return Err(Error::RunnerAuthority);
        }
        let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM dispatch_stops WHERE dispatch_id=?1 AND fence=?2 AND reason='owned_completion' AND settled_at IS NULL)",params![cap.dispatch_id,cap.fence],|r|r.get(0))?;
        if !pending {
            return Err(Error::RunnerAuthority);
        }
        let current = owned_dispatch::projection(&tx, cap, &d, Some(scope.task().execution_epoch))?;
        if current.fingerprint() != scope.fingerprint()
            || current.task().status != TaskState::Done
            || current.task().execution_epoch != result.execution_epoch
        {
            return Err(Error::RunnerAuthority);
        }
        let frozen: ReplyRoute = serde_json::from_str(&route)?;
        if matrix_routes::route(&tx, &d.session_id)? != frozen {
            return Err(Error::RunnerAuthority);
        }
        // Do not transiently clear quarantine or accept another stop's cleanup.
        let unrelated:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM unresolved_dispatches u WHERE u.id<>?1 AND (u.session_id=?2 OR EXISTS(SELECT 1 FROM dispatch_resources a JOIN dispatch_resources b ON b.resource_id=a.resource_id WHERE a.dispatch_id=?1 AND b.dispatch_id=u.id))) OR EXISTS(SELECT 1 FROM resource_leases l JOIN dispatch_resources r ON r.resource_id=l.resource_id WHERE r.dispatch_id=?1 AND l.dispatch_id<>?1)",params![cap.dispatch_id,d.session_id],|r|r.get(0))?;
        if unrelated {
            return Err(Error::Quarantined);
        }
        graphs::complete_guard(&tx, &d)?;
        super::file_delivery::complete_guard(&tx, &cap.dispatch_id)?;
        let (reply, _) =
            replies::insert_intent(&tx, current.task(), &cap.dispatch_id, &frozen, &body, now)?;
        tx.execute(
            "UPDATE dispatch_stops SET evidence=?3,settled_at=?4 WHERE dispatch_id=?1 AND fence=?2",
            params![
                cap.dispatch_id,
                cap.fence,
                format!("retained_owner:{}", reference.id),
                now
            ],
        )?;
        tx.execute(
            "UPDATE runner_attempts SET outcome='completed' WHERE dispatch_id=?1 AND fence=?2",
            params![cap.dispatch_id, cap.fence],
        )?;
        tx.execute(
            "UPDATE runner_dispatches SET state='completed' WHERE id=?1 AND fence=?2",
            params![cap.dispatch_id, cap.fence],
        )?;
        super::messages::complete_inputs(&tx, &cap.dispatch_id, now)?;
        super::peers::complete_inputs(&tx, &cap.dispatch_id, now)?;
        tx.execute(
            "DELETE FROM resource_leases WHERE dispatch_id=?1",
            [&cap.dispatch_id],
        )?;
        tx.execute("UPDATE workspace_resources SET dirty=0 WHERE id IN (SELECT resource_id FROM dispatch_resources WHERE dispatch_id=?1 AND exclusive=1) AND NOT EXISTS(SELECT 1 FROM unresolved_dispatches u JOIN dispatch_resources r ON r.dispatch_id=u.id WHERE r.resource_id=workspace_resources.id)",[&cap.dispatch_id])?;
        tx.execute("UPDATE runner_sessions SET quarantined=0 WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM unresolved_dispatches u WHERE u.session_id=runner_sessions.id)",[&d.session_id])?;
        tx.execute(
            "UPDATE owned_task_completions SET state='ready',reply_id=?2,updated_at=?3 WHERE id=?1",
            params![reference.id, reply, now],
        )?;
        let result = receipt(&tx, &reference.id, false)?;
        tx.commit()?;
        Ok(result)
    }
}
