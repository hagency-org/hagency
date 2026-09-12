//! One notice row owns intent, send attempt and inspection; transport stays host-only.
use super::{DomainRepository, execution, matrix_routes, serialize, task_intents};
use crate::Error;
use hagency_core::{
    JSON_SAFE_MAX, canonical,
    ingress::{VerifiedNoticeClaim, VerifiedNoticeReceipt, VerifiedNoticeSend},
    project::identifier,
    replies::{ReplyDeliveryObservation, ReplyReconciliation, ReplyRoute, generation},
    task_intents::{IntentResult, NoticeClaim, TaskNotice},
    tasks::{clock, text},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::json;

fn receipt(db: &Connection, id: &str, replayed: bool) -> Result<VerifiedNoticeReceipt, Error> {
    db.query_row("SELECT task_id,state,send_fence,cancel_requested FROM task_notices WHERE id=?1 AND verified_route IS NOT NULL",[id],|r|Ok(VerifiedNoticeReceipt{id:id.into(),task_id:r.get(0)?,state:r.get(1)?,fence:r.get(2)?,cancel_requested:r.get(3)?,replayed})).optional()?.ok_or(Error::NotFound)
}
fn frozen(db: &Connection, id: &str) -> Result<(TaskNotice, ReplyRoute, String), Error> {
    let notice = task_intents::notice(db, id)?;
    let (route, digest): (String, String) = db.query_row(
        "SELECT verified_route,content_digest FROM task_notices WHERE id=?1 AND verified_route IS NOT NULL",
        [id], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?.ok_or(Error::RunnerAuthority)?;
    Ok((notice, serde_json::from_str(&route)?, digest))
}
fn current(db: &Connection, id: &str) -> Result<bool, Error> {
    let (notice, route, _) = frozen(db, id)?;
    let (epoch, cancelled, active): (Option<u64>, bool, bool) = db.query_row(
        "SELECT n.task_epoch,n.cancel_requested,i.state<>'closed' FROM task_notices n JOIN task_intents i ON i.task_id=n.task_id WHERE n.id=?1",
        [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    )?;
    if cancelled || !active || epoch != Some(execution::task(db, &notice.task_id)?.execution_epoch)
    {
        return Ok(false);
    }
    match matrix_routes::route(db, &notice.session_id) {
        Ok(value) => Ok(value == route),
        Err(Error::RunnerAuthority) => Ok(false),
        Err(error) => Err(error),
    }
}
fn check_claim(db: &Connection, id: &str, token: &str, now: u64) -> Result<String, Error> {
    identifier(id, 128)?;
    text(token, 128)?;
    clock(now)?;
    let (state, hash, until): (String, Option<String>, Option<u64>) = db.query_row(
        "SELECT state,claim_hash,claim_until FROM task_notices WHERE id=?1 AND verified_route IS NOT NULL",
        [id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).optional()?.ok_or(Error::RunnerAuthority)?;
    if !execution::matches_secret(hash.as_deref().unwrap_or(""), token)?
        || (state != "delivered" && until.is_none_or(|until| until <= now))
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(state)
}
fn observed(db: &Connection, id: &str, input: &ReplyDeliveryObservation) -> Result<String, Error> {
    input.validate()?;
    let (notice, route, digest) = frozen(db, id)?;
    if input.transaction_id != notice.transaction_id
        || input.digest != digest
        || input.server_name != route.server_name
        || input.room_id != route.room_id
        || input.sender_mxid != route.sender_mxid
        || input.device_id != route.device_id
        || (route.encrypted && !input.encrypted)
    {
        return Err(Error::RunnerAuthority);
    }
    serialize(input)
}
fn activate(tx: &Transaction<'_>, id: &str, input: &ReplyDeliveryObservation) -> Result<(), Error> {
    let notice = task_intents::notice(tx, id)?;
    if notice.kind == "ack" {
        if tx.execute("UPDATE task_intents SET state='active',anchor_event_id=?2 WHERE task_id=?1 AND state='pending'",params![notice.task_id,input.event_id])? != 1 {
            return Err(Error::State);
        }
        task_intents::project_inputs(tx, &notice.task_id)?;
        let task = execution::task(tx, &notice.task_id)?;
        execution::save_task(tx, &task, "thread_active")?;
    }
    Ok(())
}
/// Called during route reconciliation as well as before custody operations.
/// Possible sends remain uncertain and keep their attempt fence for inspection.
pub(super) fn retire(tx: &Transaction<'_>) -> Result<(), Error> {
    tx.execute("UPDATE task_notices SET cancel_requested=1,state=CASE WHEN state IN ('sending','uncertain') THEN 'uncertain' ELSE 'cancelled' END,claim_hash=NULL,claim_until=NULL,error_code='scope_retired' WHERE verified_route IS NOT NULL AND state IN ('pending','claimed','sending','uncertain','failed') AND NOT EXISTS(SELECT 1 FROM current_matrix_routes r JOIN canonical_tasks t ON t.session_id=r.session_id JOIN task_intents i ON i.task_id=t.id WHERE t.id=task_notices.task_id AND json_extract(t.config,'$.execution_epoch')=task_notices.task_epoch AND i.state<>'closed')",[])?;
    Ok(())
}
pub(super) fn reconcile(tx: &Transaction<'_>, now: u64, restart: bool) -> Result<(), Error> {
    matrix_routes::reconcile(tx, now)?;
    retire(tx)?;
    tx.execute("UPDATE task_notices SET state=CASE WHEN state='sending' THEN 'uncertain' ELSE 'pending' END,claim_hash=NULL,claim_until=NULL WHERE verified_route IS NOT NULL AND state IN ('claimed','sending') AND (?2 OR claim_until<=?1)",params![now,restart])?;
    Ok(())
}

impl DomainRepository {
    /// A claim is scheduling only. Begin must commit before external IO.
    pub fn claim_verified_task_notice(
        &mut self,
        now: u64,
        lease_ms: u64,
    ) -> Result<Option<VerifiedNoticeClaim>, Error> {
        clock(now)?;
        if !(1..=60_000).contains(&lease_ms) || now > JSON_SAFE_MAX - lease_ms {
            return Err(hagency_core::InvalidInput("invalid verified notice lease").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        reconcile(&tx, now, false)?;
        let id: Option<String> = tx.query_row("SELECT id FROM task_notices WHERE verified_route IS NOT NULL AND state='pending' AND cancel_requested=0 AND not_before<=?1 ORDER BY rowid LIMIT 1",[now],|r|r.get(0)).optional()?;
        let Some(id) = id else {
            tx.commit()?;
            return Ok(None);
        };
        if !current(&tx, &id)? {
            return Err(Error::RunnerAuthority);
        }
        let (notice, route, digest) = frozen(&tx, &id)?;
        let (source, fence): (String, u64) = tx.query_row(
            "SELECT source_event_id,send_fence+1 FROM task_notices WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        generation(fence)?;
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|_| Error::Unavailable)?;
        let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
        tx.execute("UPDATE task_notices SET state='claimed',send_fence=?2,claim_hash=?3,claim_until=?4 WHERE id=?1",params![id,fence,canonical::digest(&json!(token))?,now+lease_ms])?;
        tx.commit()?;
        Ok(Some(VerifiedNoticeClaim {
            claim: NoticeClaim {
                notice,
                token,
                deadline: now + lease_ms,
            },
            route,
            source_event_id: source,
            digest,
        }))
    }
    /// This transition is deliberately non-replayable. A lost response requires
    /// inspection, since the caller might already have started external IO.
    pub fn begin_verified_task_notice_send(
        &mut self,
        id: &str,
        token: &str,
        now: u64,
    ) -> Result<VerifiedNoticeSend, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if check_claim(&tx, id, token, now)? != "claimed" || !current(&tx, id)? {
            return Err(Error::RunnerAuthority);
        }
        let (notice, route, digest) = frozen(&tx, id)?;
        let (source_event_id, fence): (String, u64) = tx.query_row(
            "SELECT source_event_id,send_fence FROM task_notices WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute("UPDATE task_notices SET state='sending' WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(VerifiedNoticeSend {
            notice,
            route,
            digest,
            source_event_id,
            fence,
        })
    }
    /// Current live claim check immediately before each adapter write. This is
    /// not a replay of begin and does not create new send authority.
    pub fn validate_verified_task_notice_send(
        &self,
        id: &str,
        token: &str,
        fence: u64,
        now: u64,
    ) -> Result<(), Error> {
        if check_claim(&self.db, id, token, now)? != "sending"
            || receipt(&self.db, id, false)?.fence != fence
            || !current(&self.db, id)?
        {
            return Err(Error::RunnerAuthority);
        }
        Ok(())
    }
    pub fn deliver_verified_task_notice(
        &mut self,
        id: &str,
        token: &str,
        input: &ReplyDeliveryObservation,
        now: u64,
    ) -> Result<IntentResult, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = check_claim(&tx, id, token, now)?;
        let observation = observed(&tx, id, input)?;
        if !current(&tx, id)? {
            return Err(Error::RunnerAuthority);
        }
        let notice = task_intents::notice(&tx, id)?;
        if state == "delivered" {
            let old: String =
                tx.query_row("SELECT delivery FROM task_notices WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
            if old != observation {
                return Err(Error::Conflict);
            }
            return task_intents::intent_result(&tx, &notice.task_id, true);
        }
        if state != "sending" || !current(&tx, id)? {
            return Err(Error::RunnerAuthority);
        }
        tx.execute(
            "UPDATE task_notices SET state='delivered',delivery=?2 WHERE id=?1",
            params![id, observation],
        )?;
        activate(&tx, id, input)?;
        let result = task_intents::intent_result(&tx, &notice.task_id, false)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn verified_notice_receipt(&self, id: &str) -> Result<VerifiedNoticeReceipt, Error> {
        identifier(id, 128)?;
        receipt(&self.db, id, false)
    }
    pub fn cancel_verified_task_notice(
        &mut self,
        id: &str,
        now: u64,
    ) -> Result<VerifiedNoticeReceipt, Error> {
        identifier(id, 128)?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if receipt(&tx, id, false)?.state == "delivered" {
            return Err(Error::State);
        }
        tx.execute("UPDATE task_notices SET cancel_requested=1,state=CASE WHEN state IN ('sending','uncertain') THEN 'uncertain' ELSE 'cancelled' END,claim_hash=NULL,claim_until=NULL,error_code='host_cancelled' WHERE id=?1",[id])?;
        let result = receipt(&tx, id, false)?;
        tx.commit()?;
        Ok(result)
    }
    /// A host transport inspector can record an old accepted event, but only a
    /// current uncancelled task receives activation authority from that record.
    pub fn reconcile_verified_task_notice(
        &mut self,
        id: &str,
        fence: u64,
        input: &ReplyReconciliation,
        now: u64,
    ) -> Result<VerifiedNoticeReceipt, Error> {
        identifier(id, 128)?;
        generation(fence)?;
        clock(now)?;
        match input {
            ReplyReconciliation::Delivered(v) => v.validate()?,
            ReplyReconciliation::NotSent { evidence } => text(evidence, 4000)?,
        };
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        reconcile(&tx, now, false)?;
        let digest = canonical::digest(&json!([id, fence, input]))?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT digest FROM notice_send_inspections WHERE notice_id=?1 AND fence=?2",
                params![id, fence],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prior) = prior {
            if prior != digest {
                return Err(Error::Conflict);
            }
            return receipt(&tx, id, true);
        }
        let before = receipt(&tx, id, false)?;
        if (before.state != "uncertain"
            && !(before.state == "sending" && matches!(input, ReplyReconciliation::Delivered(_))))
            || before.fence != fence
        {
            return Err(Error::RunnerAuthority);
        }
        let count: u64 = tx.query_row("SELECT COUNT(*) FROM notice_send_inspections", [], |r| {
            r.get(0)
        })?;
        let own: u64 = tx.query_row(
            "SELECT COUNT(*) FROM notice_send_inspections WHERE notice_id=?1",
            [id],
            |r| r.get(0),
        )?;
        if count >= 100_000 || own >= 32 {
            return Err(Error::Capacity);
        }
        let eligible = current(&tx, id)?;
        match input {
            ReplyReconciliation::Delivered(input) => {
                let observation = observed(&tx, id, input)?;
                tx.execute("UPDATE task_notices SET state='delivered',delivery=?2,claim_hash=NULL,claim_until=NULL WHERE id=?1",params![id,observation])?;
                if eligible {
                    activate(&tx, id, input)?;
                }
            }
            ReplyReconciliation::NotSent { .. } => {
                tx.execute("UPDATE task_notices SET state=?2,claim_hash=NULL,claim_until=NULL,not_before=?3 WHERE id=?1",params![id,if eligible{"pending"}else{"cancelled"},now])?;
            }
        }
        tx.execute("INSERT INTO notice_send_inspections(notice_id,fence,digest,observation) VALUES(?1,?2,?3,?4)",params![id,fence,digest,serialize(input)?])?;
        let result = receipt(&tx, id, false)?;
        tx.commit()?;
        Ok(result)
    }
}
