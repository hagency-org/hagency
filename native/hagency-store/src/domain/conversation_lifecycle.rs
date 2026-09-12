//! Membership retirement fences work; only host inspection can release custody.
use super::{DomainRepository, bounded_row, conversations, execution, serialize};
use crate::Error;
use hagency_core::{
    JSON_SAFE_MAX, canonical,
    conversations::*,
    project::identifier,
    tasks::{RunnerCapability, TaskState, clock, text},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::json;
use std::collections::{BTreeSet, VecDeque};

fn owned(db: &Connection, session_id: &str, id: &str) -> Result<Conversation, Error> {
    let session = execution::session(db, session_id)?;
    let (fleet, project, generation) = conversations::project(db, session.engagement_id())?;
    let row: Option<(String, u64)> = db.query_row(
        "SELECT state,revision FROM internal_conversations WHERE id=?1 AND creator_session_id=?2 AND fleet_id=?3 AND project_id=?4 AND generation=?5",
        params![id,session_id,fleet,project,generation], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    let (state, revision) = row.ok_or(Error::RunnerAuthority)?;
    let value = conversations::read(db, id)?;
    if value.id != id
        || value.creator_session_id != session_id
        || value.state != state
        || value.revision != revision
    {
        return Err(Error::State);
    }
    Ok(value)
}
fn save(tx: &Transaction<'_>, value: &mut Conversation) -> Result<(), Error> {
    value.revision = value
        .revision
        .checked_add(1)
        .filter(|v| *v <= JSON_SAFE_MAX)
        .ok_or(Error::Capacity)?;
    tx.execute(
        "UPDATE internal_conversations SET state=?2,revision=?3,config=?4 WHERE id=?1",
        params![value.id, value.state, value.revision, serialize(value)?],
    )?;
    Ok(())
}
fn release_inputs(tx: &Transaction<'_>, id: &str) -> Result<(), Error> {
    // Release assignments, never fabricate a processed/delivery receipt. Immutable
    // dispatch projections remain available for host inspection.
    tx.execute(
        "UPDATE session_inputs SET dispatch_id=NULL WHERE dispatch_id=?1 AND processed_at IS NULL",
        [id],
    )?;
    tx.execute("UPDATE peer_session_inputs SET dispatch_id=NULL WHERE dispatch_id=?1 AND processed_at IS NULL",[id])?;
    Ok(())
}
pub(super) fn fence_dispatch(
    tx: &Transaction<'_>,
    id: &str,
    reason: &str,
    now: u64,
) -> Result<(), Error> {
    let (state, fence, session): (String, u64, String) = tx.query_row(
        "SELECT state,fence,session_id FROM runner_dispatches WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if !["queued", "leased", "started", "parked", "outcome_unknown"].contains(&state.as_str()) {
        return Ok(());
    }
    if state == "outcome_unknown" {
        let unresolved: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM unresolved_dispatches WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !unresolved {
            return Ok(());
        }
    }
    let uncertain = ["started", "parked", "outcome_unknown"].contains(&state.as_str());
    if uncertain {
        // Keep the leases as well as dirty/quarantine state. A removed worker can
        // still be running after its control-plane capability has been revoked.
        tx.execute("INSERT OR IGNORE INTO dispatch_stops(dispatch_id,fence,reason,created_at) VALUES(?1,?2,?3,?4)",params![id,fence,reason,now])?;
        tx.execute(
            "UPDATE runner_sessions SET quarantined=1 WHERE id=?1",
            [&session],
        )?;
        tx.execute("UPDATE workspace_resources SET dirty=1 WHERE id IN (SELECT resource_id FROM dispatch_resources WHERE dispatch_id=?1 AND exclusive=1)",[id])?;
    } else {
        tx.execute("DELETE FROM resource_leases WHERE dispatch_id=?1", [id])?;
        release_inputs(tx, id)?;
    }
    let next = if uncertain {
        "outcome_unknown"
    } else {
        "superseded"
    };
    tx.execute(
        "UPDATE runner_attempts SET outcome=?3 WHERE dispatch_id=?1 AND fence=?2",
        params![id, fence, next],
    )?;
    tx.execute("UPDATE runner_dispatches SET state=?2,capability_hash=NULL,lease_until=NULL,capability_until=NULL WHERE id=?1",params![id,next])?;
    Ok(())
}
fn close(
    tx: &Transaction<'_>,
    value: &mut Conversation,
    queue: &mut VecDeque<String>,
    now: u64,
) -> Result<(), Error> {
    value.state = "closed".into();
    queue.extend(value.participants.iter().map(|p| p.id.clone()));
    tx.execute(
        "DELETE FROM internal_participants WHERE conversation_id=?1",
        [&value.id],
    )?;
    save(tx, value)?;
    // Creator sessions can also hold a frozen peer batch from this group. Fence
    // the whole batch; filtering its payload after start would rewrite history.
    let ids:Vec<String>=tx.prepare("SELECT DISTINCT d.id FROM runner_dispatches d JOIN peer_dispatch_inputs pi ON pi.dispatch_id=d.id JOIN peer_messages m ON m.sequence=pi.message_sequence WHERE m.conversation_id=?1 AND d.state IN ('queued','leased','started','parked','outcome_unknown')")?.query_map([&value.id],|r|r.get(0))?.collect::<Result<_,_>>()?;
    for id in ids {
        fence_dispatch(tx, &id, &value.id, now)?;
    }
    Ok(())
}
pub(super) fn retire(
    tx: &Transaction<'_>,
    mut queue: VecDeque<String>,
    reason: &str,
    now: u64,
) -> Result<(), Error> {
    let mut visited = BTreeSet::new();
    while let Some(session) = queue.pop_front() {
        if !visited.insert(session.clone()) {
            continue;
        }
        if visited.len() > 10_000 {
            return Err(Error::Capacity);
        }
        let ids:Vec<String>=tx.prepare("SELECT id FROM runner_dispatches WHERE session_id=?1 AND state IN ('queued','leased','started','parked','outcome_unknown')")?.query_map([&session],|r|r.get(0))?.collect::<Result<_,_>>()?;
        for id in ids {
            fence_dispatch(tx, &id, reason, now)?;
        }
        let children:Vec<String>=tx.prepare("SELECT id FROM internal_conversations WHERE creator_session_id=?1 AND state='active'")?.query_map([&session],|r|r.get(0))?.collect::<Result<_,_>>()?;
        for child in children {
            close(tx, &mut conversations::read(tx, &child)?, &mut queue, now)?;
        }
    }
    super::graphs::reconcile(tx, now)?;
    Ok(())
}
impl DomainRepository {
    pub fn change_internal_conversation(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        input: &ConversationChange,
        now: u64,
    ) -> Result<ConversationResult, Error> {
        identifier(id, 128)?;
        input.validate()?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let dispatch = execution::authorize_work(&tx, cap, now)?;
        if let Some(task) = &dispatch.task_id
            && execution::task(&tx, task)?.status == TaskState::Done
        {
            return Err(Error::State);
        }
        let mut value = owned(&tx, &dispatch.session_id, id)?;
        // Canonicalize membership as a set before comparing retry content.
        let mut normalized = input.clone();
        if let ConversationAction::Members {
            participant_engagements,
        } = &mut normalized.action
        {
            participant_engagements.sort();
        }
        let digest = canonical::digest(&json!([id, normalized]))?;
        let prior:Option<(String,String)>=tx.query_row("SELECT digest,response FROM conversation_operations WHERE dispatch_id=?1 AND call_id=?2",params![cap.dispatch_id,input.call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((old, response)) = prior {
            if old != digest {
                return Err(Error::Conflict);
            }
            return Ok(ConversationResult {
                conversation: serde_json::from_str(&response)?,
                replayed: true,
            });
        }
        if value.state != "active" || value.revision != input.expected_revision {
            return Err(Error::Conflict);
        }
        let count: u64 = tx.query_row(
            "SELECT COUNT(*) FROM conversation_operations WHERE dispatch_id=?1",
            [&cap.dispatch_id],
            |r| r.get(0),
        )?;
        if count >= 1024 {
            return Err(Error::Capacity);
        }
        let total: u64 = tx.query_row("SELECT COUNT(*) FROM conversation_operations", [], |r| {
            r.get(0)
        })?;
        if total >= 100_000 {
            return Err(Error::Capacity);
        }
        let mut retired = VecDeque::new();
        match &normalized.action {
            ConversationAction::Close {} => close(&tx, &mut value, &mut retired, now)?,
            ConversationAction::Members {
                participant_engagements,
            } => {
                let creator = execution::session(&tx, &dispatch.session_id)?;
                let project = conversations::project(&tx, creator.engagement_id())?;
                let mut participants: BTreeSet<_> =
                    participant_engagements.iter().cloned().collect();
                participants.insert(creator.engagement_id().into());
                if participants.len() > 64 {
                    return Err(Error::Capacity);
                }
                for engagement in &participants {
                    if conversations::project(&tx, engagement)? != project {
                        return Err(Error::RunnerAuthority);
                    }
                }
                for old in &value.participants {
                    if !participants.contains(&old.engagement_id) {
                        tx.execute("DELETE FROM internal_participants WHERE conversation_id=?1 AND session_id=?2",params![id,old.id])?;
                        retired.push_back(old.id.clone());
                    }
                }
                let mut bindings = Vec::new();
                for engagement in participants {
                    if let Some(old) = value
                        .participants
                        .iter()
                        .find(|p| p.engagement_id == engagement)
                    {
                        bindings.push(old.clone());
                        continue;
                    }
                    let binding = InternalSessionBinding {
                        kind: InternalKind::Internal,
                        id: format!(
                            "session_{}",
                            &canonical::digest(&json!([
                                "membership",
                                id,
                                value.revision + 1,
                                engagement
                            ]))?[..32]
                        ),
                        engagement_id: engagement,
                        conversation_id: id.into(),
                    };
                    binding.validate()?;
                    bounded_row(&tx, "runner_sessions", "id", &binding.id, 10_000)?;
                    tx.execute(
                        "INSERT INTO runner_sessions(id,engagement_id,binding) VALUES(?1,?2,?3)",
                        params![binding.id, binding.engagement_id, serialize(&binding)?],
                    )?;
                    tx.execute("INSERT INTO internal_participants(conversation_id,engagement_id,session_id) VALUES(?1,?2,?3)",params![id,binding.engagement_id,binding.id])?;
                    execution::session(&tx, &binding.id)?;
                    bindings.push(binding);
                }
                value.participants = bindings;
                save(&tx, &mut value)?;
            }
        }
        retire(&tx, retired, id, now)?;
        tx.execute("INSERT INTO conversation_operations(dispatch_id,call_id,digest,response) VALUES(?1,?2,?3,?4)",params![cap.dispatch_id,input.call_id,digest,serialize(&value)?])?;
        tx.commit()?;
        Ok(ConversationResult {
            conversation: value,
            replayed: false,
        })
    }
    /// Host-only durable cancellation queue. Not a runtime or operator JSON API.
    pub fn pending_conversation_stops(
        &self,
        after: &str,
        limit: usize,
    ) -> Result<Vec<(String, u64)>, Error> {
        if !(1..=100).contains(&limit) {
            return Err(hagency_core::InvalidInput("stop page must be 1..100").into());
        }
        Ok(self.db.prepare("SELECT dispatch_id,fence FROM dispatch_stops WHERE settled_at IS NULL AND dispatch_id>?1 ORDER BY dispatch_id LIMIT ?2")?.query_map(params![after,limit],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<Result<_,_>>()?)
    }
    /// The host must first prove the owned scope stopped and inspect workspace
    /// effects. This records that result; the evidence string is NOT the proof.
    /// No runtime-facing command may call this method.
    pub fn settle_conversation_stop(
        &mut self,
        id: &str,
        fence: u64,
        evidence: &str,
        now: u64,
    ) -> Result<(), Error> {
        identifier(id, 128)?;
        text(evidence, 4096)?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(u64, Option<String>)> = tx
            .query_row(
                "SELECT fence,evidence FROM dispatch_stops WHERE dispatch_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (current, prior) = row.ok_or(Error::NotFound)?;
        if current != fence {
            return Err(Error::RunnerAuthority);
        }
        if let Some(prior) = prior {
            return if prior == evidence {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM runner_dispatches WHERE id=?1 AND fence=?2 AND state='outcome_unknown')",params![id,fence],|r|r.get(0))?;
        if !valid {
            return Err(Error::State);
        }
        tx.execute(
            "UPDATE dispatch_stops SET evidence=?2,settled_at=?3 WHERE dispatch_id=?1",
            params![id, evidence, now],
        )?;
        tx.execute("DELETE FROM resource_leases WHERE dispatch_id=?1", [id])?;
        tx.execute("UPDATE workspace_resources SET dirty=0 WHERE id IN (SELECT resource_id FROM dispatch_resources WHERE dispatch_id=?1 AND exclusive=1) AND NOT EXISTS(SELECT 1 FROM unresolved_dispatches u JOIN dispatch_resources r ON r.dispatch_id=u.id WHERE r.resource_id=workspace_resources.id AND r.exclusive=1)",[id])?;
        tx.execute("UPDATE runner_sessions SET quarantined=0 WHERE id=(SELECT session_id FROM runner_dispatches WHERE id=?1) AND NOT EXISTS(SELECT 1 FROM unresolved_dispatches u WHERE u.session_id=runner_sessions.id)",[id])?;
        release_inputs(&tx, id)?;
        tx.commit()?;
        Ok(())
    }
}
