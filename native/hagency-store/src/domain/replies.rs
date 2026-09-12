//! Final intent admission and external send observation have separate custody.
use super::{DomainRepository, bounded_row, execution, matrix_routes, serialize};
use crate::Error;
use hagency_core::{
    JSON_SAFE_MAX, canonical,
    project::identifier,
    replies::*,
    tasks::{RunnerCapability, TaskState, clock, text},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::json;

fn receipt(db: &Connection, id: &str, replayed: bool) -> Result<ReplyReceipt, Error> {
    let row: Option<(String, u64, String)> = db
        .query_row(
            "SELECT task_id,execution_epoch,state FROM final_replies WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (task_id, execution_epoch, state) = row.ok_or(Error::NotFound)?;
    Ok(ReplyReceipt {
        id: id.into(),
        task_id,
        execution_epoch,
        state: serde_json::from_value(json!(state))?,
        replayed,
    })
}
fn current(db: &Connection, id: &str) -> Result<(), Error> {
    if !db.query_row(
        "SELECT EXISTS(SELECT 1 FROM current_final_replies WHERE id=?1)",
        [id],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
fn snapshot(db: &Connection, id: &str) -> Result<ReplySend, Error> {
    let (transaction_id, digest, route, body): (String, String, String, String) = db.query_row(
        "SELECT transaction_id,digest,route,body FROM final_replies WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    Ok(ReplySend {
        id: id.into(),
        transaction_id,
        digest,
        route: serde_json::from_str(&route)?,
        body,
    })
}
fn claim_state(db: &Connection, claim: &ReplyClaim, now: u64) -> Result<String, Error> {
    identifier(&claim.id, 128)?;
    clock(now)?;
    let row: Option<(u64, Option<String>, Option<u64>, String)> = db
        .query_row(
            "SELECT fence,claim_hash,claim_until,state FROM final_replies WHERE id=?1",
            [&claim.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let (fence, hash, until, state) = row.ok_or(Error::RunnerAuthority)?;
    if fence != claim.fence
        || !execution::matches_secret(hash.as_deref().unwrap_or(""), &claim.secret)?
        || (state != "delivered" && until.is_none_or(|until| until <= now))
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(state)
}
fn delivery(send: &ReplySend, observed: &ReplyDeliveryObservation) -> Result<String, Error> {
    observed.validate()?;
    if observed.transaction_id != send.transaction_id
        || observed.digest != send.digest
        || observed.server_name != send.route.server_name
        || observed.room_id != send.route.room_id
        || observed.sender_mxid != send.route.sender_mxid
        || observed.device_id != send.route.device_id
        || send.route.encrypted && !observed.encrypted
    {
        return Err(Error::RunnerAuthority);
    }
    serialize(observed)
}
pub(super) fn reconcile(tx: &Transaction<'_>, now: u64, restart: bool) -> Result<(), Error> {
    matrix_routes::reconcile(tx, now)?;
    tx.execute("UPDATE final_replies SET state=CASE WHEN state='sending' THEN 'uncertain' ELSE 'pending' END,claim_hash=NULL,claim_until=NULL,updated_at=?1 WHERE state IN ('claimed','sending') AND (?2 OR claim_until<=?1)",params![now,restart])?;
    Ok(())
}
fn bound_task(
    db: &Connection,
    cap: &RunnerCapability,
    now: u64,
) -> Result<(String, hagency_core::tasks::Task), Error> {
    let dispatch = execution::authorize(db, cap, now, &["started"])?;
    let task_id = dispatch
        .task_id
        .as_deref()
        .or(dispatch.report_task.as_deref())
        .ok_or(Error::RunnerAuthority)?;
    let task = execution::task(db, task_id)?;
    if task.session_id != dispatch.session_id
        || task.status != TaskState::Done
        || task.execution_epoch == 0
    {
        return Err(Error::State);
    }
    matrix_routes::check(db, &dispatch.session_id)?;
    Ok((dispatch.session_id, task))
}

pub(super) fn insert_intent(
    tx: &Transaction<'_>,
    task: &hagency_core::tasks::Task,
    dispatch_id: &str,
    route: &ReplyRoute,
    body: &str,
    now: u64,
) -> Result<(String, bool), Error> {
    let session = &task.session_id;
    let task_id = &task.id;
    let epoch = task.execution_epoch;
    let digest = canonical::payload_digest(&json!(["final_reply", task_id, epoch, route, body]))?;
    let existing: Option<(String, String)> = tx
        .query_row(
            "SELECT id,digest FROM final_replies WHERE task_id=?1 AND execution_epoch=?2",
            params![task_id, epoch],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (id, replayed) = if let Some((id, old)) = existing {
        if old != digest {
            return Err(Error::Conflict);
        }
        (id, true)
    } else {
        let id = format!(
            "reply_{}",
            &canonical::digest(&json!([task_id, epoch]))?[..32]
        );
        bounded_row(tx, "final_replies", "id", &id, 30_000)?;
        let pending:u64=tx.query_row("SELECT COUNT(*) FROM final_replies WHERE session_id=?1 AND state IN ('pending','claimed','sending','uncertain')",[&session],|r|r.get(0))?;
        if pending >= 128 {
            return Err(Error::Capacity);
        }
        let transaction = format!("hagency_{id}");
        tx.execute("INSERT INTO final_replies(id,session_id,task_id,execution_epoch,source_dispatch_id,transaction_id,digest,body,route,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10,?10)",params![id,session,task_id,epoch,dispatch_id,transaction,digest,body,serialize(&route)?,now])?;
        (id, false)
    };
    Ok((id, replayed))
}

impl DomainRepository {
    pub fn submit_final_reply(
        &mut self,
        cap: &RunnerCapability,
        input: &FinalReply,
        now: u64,
    ) -> Result<ReplyReceipt, Error> {
        input.validate()?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (session, task) = bound_task(&tx, cap, now)?;
        let route = matrix_routes::route(&tx, &session)?;
        let digest = canonical::payload_digest(&json!([
            "final_reply",
            task.id,
            task.execution_epoch,
            route,
            input.body
        ]))?;
        let prior: Option<(String, String)> = tx
            .query_row(
                "SELECT reply_id,digest FROM final_reply_calls WHERE dispatch_id=?1 AND call_id=?2",
                params![cap.dispatch_id, input.call_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, old)) = prior {
            if old != digest {
                return Err(Error::Conflict);
            }
            return receipt(&tx, &id, true);
        }
        let (id, replayed) = insert_intent(&tx, &task, &cap.dispatch_id, &route, &input.body, now)?;
        let own: u64 = tx.query_row(
            "SELECT COUNT(*) FROM final_reply_calls WHERE dispatch_id=?1",
            [&cap.dispatch_id],
            |r| r.get(0),
        )?;
        let total: u64 =
            tx.query_row("SELECT COUNT(*) FROM final_reply_calls", [], |r| r.get(0))?;
        if own >= 1024 || total >= 100_000 {
            return Err(Error::Capacity);
        }
        tx.execute("INSERT INTO final_reply_calls(dispatch_id,call_id,reply_id,digest) VALUES(?1,?2,?3,?4)",params![cap.dispatch_id,input.call_id,id,digest])?;
        let result = receipt(&tx, &id, replayed)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn runner_final_reply(
        &self,
        cap: &RunnerCapability,
        id: &str,
        now: u64,
    ) -> Result<ReplyReceipt, Error> {
        identifier(id, 128)?;
        let (session, task) = bound_task(&self.db, cap, now)?;
        let allowed:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM final_replies WHERE id=?1 AND session_id=?2 AND task_id=?3 AND execution_epoch=?4)",params![id,session,task.id,task.execution_epoch],|r|r.get(0))?;
        if !allowed {
            return Err(Error::RunnerAuthority);
        }
        receipt(&self.db, id, false)
    }
    pub fn claim_final_reply(
        &mut self,
        now: u64,
        lease_ms: u64,
    ) -> Result<Option<ReplyClaim>, Error> {
        clock(now)?;
        if !(1..=60_000).contains(&lease_ms) || now > JSON_SAFE_MAX - lease_ms {
            return Err(hagency_core::InvalidInput("invalid reply lease").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        reconcile(&tx, now, false)?;
        let id:Option<String>=tx.query_row("SELECT f.id FROM final_replies f JOIN current_final_replies c ON c.id=f.id WHERE f.state='pending' AND f.cancel_requested=0 ORDER BY f.rowid LIMIT 1",[],|r|r.get(0)).optional()?;
        let Some(id) = id else {
            tx.commit()?;
            return Ok(None);
        };
        let fence: u64 = tx.query_row(
            "SELECT fence+1 FROM final_replies WHERE id=?1",
            [&id],
            |r| r.get(0),
        )?;
        generation(fence)?;
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
        let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let hash = canonical::digest(&json!(secret))?;
        tx.execute("UPDATE final_replies SET state='claimed',fence=?2,claim_hash=?3,claim_until=?4,updated_at=?5 WHERE id=?1",params![id,fence,hash,now+lease_ms,now])?;
        tx.commit()?;
        Ok(Some(ReplyClaim { id, fence, secret }))
    }
    /// Host-only routing preview. This neither changes state nor authorizes IO;
    /// the caller must still commit begin-send and validate before each write.
    pub fn preview_final_reply(&self, claim: &ReplyClaim, now: u64) -> Result<ReplySend, Error> {
        if claim_state(&self.db, claim, now)? != "claimed" {
            return Err(Error::State);
        }
        current(&self.db, &claim.id)?;
        snapshot(&self.db, &claim.id)
    }
    /// Recheck the exact still-current Sending claim immediately before a host
    /// transport write. It is not an atomic fence on a remote homeserver.
    pub fn validate_final_reply_send(&self, claim: &ReplyClaim, now: u64) -> Result<(), Error> {
        if claim_state(&self.db, claim, now)? != "sending" {
            return Err(Error::State);
        }
        current(&self.db, &claim.id)
    }
    /// Commit send-start before touching Matrix. Losing this response is uncertain;
    /// the host must inspect rather than execute the same begin command twice.
    pub fn begin_final_reply_send(
        &mut self,
        claim: &ReplyClaim,
        now: u64,
    ) -> Result<ReplySend, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if claim_state(&tx, claim, now)? != "claimed" {
            return Err(Error::State);
        }
        current(&tx, &claim.id)?;
        let send = snapshot(&tx, &claim.id)?;
        tx.execute(
            "UPDATE final_replies SET state='sending',updated_at=?2 WHERE id=?1",
            params![claim.id, now],
        )?;
        tx.commit()?;
        Ok(send)
    }
    pub fn observe_final_reply(
        &mut self,
        claim: &ReplyClaim,
        input: &ReplyDeliveryObservation,
        now: u64,
    ) -> Result<ReplyReceipt, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = claim_state(&tx, claim, now)?;
        let observation = delivery(&snapshot(&tx, &claim.id)?, input)?;
        if state == "delivered" {
            let old: String = tx.query_row(
                "SELECT observation FROM final_replies WHERE id=?1",
                [&claim.id],
                |r| r.get(0),
            )?;
            if old != observation {
                return Err(Error::Conflict);
            }
            return receipt(&tx, &claim.id, true);
        }
        if state != "sending" {
            return Err(Error::State);
        }
        current(&tx, &claim.id)?;
        tx.execute("UPDATE final_replies SET state='delivered',event_id=?2,observation=?3,updated_at=?4 WHERE id=?1",params![claim.id,input.event_id,observation,now])?;
        let result = receipt(&tx, &claim.id, false)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn cancel_final_reply(&mut self, id: &str, now: u64) -> Result<ReplyReceipt, Error> {
        identifier(id, 128)?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old = receipt(&tx, id, false)?;
        if old.state == ReplyState::Delivered {
            return Err(Error::State);
        }
        tx.execute("UPDATE final_replies SET cancel_requested=1,state=CASE WHEN state IN ('sending','uncertain') THEN 'uncertain' ELSE 'cancelled' END,claim_hash=NULL,claim_until=NULL,updated_at=?2 WHERE id=?1",params![id,now])?;
        let result = receipt(&tx, id, false)?;
        tx.commit()?;
        Ok(result)
    }
    /// A transport inspector supplies evidence, never a runner or an expired
    /// claim. Even an old route may be inspected; it may never be resent.
    pub fn reconcile_final_reply(
        &mut self,
        id: &str,
        fence: u64,
        input: &ReplyReconciliation,
        now: u64,
    ) -> Result<ReplyReceipt, Error> {
        identifier(id, 128)?;
        clock(now)?;
        generation(fence)?;
        match input {
            ReplyReconciliation::Delivered(observed) => observed.validate()?,
            ReplyReconciliation::NotSent { evidence } => text(evidence, 4000)?,
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let digest = canonical::digest(&json!([id, fence, input]))?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT digest FROM final_reply_inspections WHERE reply_id=?1 AND fence=?2",
                params![id, fence],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = prior {
            if old != digest {
                return Err(Error::Conflict);
            }
            return receipt(&tx, id, true);
        }
        let (state, current_fence): (String, u64) = tx.query_row(
            "SELECT state,fence FROM final_replies WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        // An authenticated inspector may have journaled an accepted response
        // before losing the sender's claim secret. That is positive delivery
        // evidence, not evidence that a send never happened.
        let inspectable = state == "uncertain"
            || (state == "sending" && matches!(input, ReplyReconciliation::Delivered(_)));
        if !inspectable || current_fence != fence {
            return Err(Error::RunnerAuthority);
        }
        let own: u64 = tx.query_row(
            "SELECT COUNT(*) FROM final_reply_inspections WHERE reply_id=?1",
            [id],
            |r| r.get(0),
        )?;
        let total: u64 = tx.query_row("SELECT COUNT(*) FROM final_reply_inspections", [], |r| {
            r.get(0)
        })?;
        if own >= 32 || total >= 100_000 {
            return Err(Error::Capacity);
        }
        match input {
            ReplyReconciliation::Delivered(observed) => {
                let encoded = delivery(&snapshot(&tx, id)?, observed)?;
                tx.execute("UPDATE final_replies SET state='delivered',event_id=?2,observation=?3,updated_at=?4 WHERE id=?1",params![id,observed.event_id,encoded,now])?;
            }
            ReplyReconciliation::NotSent { evidence } => {
                text(evidence, 4000)?;
                let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM current_final_replies c JOIN final_replies f ON f.id=c.id WHERE c.id=?1 AND f.cancel_requested=0)",[id],|r|r.get(0))?;
                tx.execute(
                    "UPDATE final_replies SET state=?2,observation=?3,updated_at=?4 WHERE id=?1",
                    params![
                        id,
                        if valid { "pending" } else { "cancelled" },
                        serialize(input)?,
                        now
                    ],
                )?;
            }
        }
        tx.execute(
            "INSERT INTO final_reply_inspections(reply_id,fence,digest) VALUES(?1,?2,?3)",
            params![id, fence, digest],
        )?;
        let result = receipt(&tx, id, false)?;
        tx.commit()?;
        Ok(result)
    }
}
