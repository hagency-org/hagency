//! Internal sessions share the executor but never acquire a Matrix room route.
use super::{DomainRepository, bounded_row, execution, serialize};
use crate::Error;
use hagency_core::{
    canonical,
    conversations::*,
    tasks::{RunnerCapability, TaskState, clock},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::json;

pub(super) fn project(db: &Connection, engagement: &str) -> Result<(String, String, u64), Error> {
    db.query_row("SELECT e.fleet_id,e.project_id,e.generation FROM engagements e JOIN registrations r ON r.fleet_id=e.fleet_id WHERE e.id=?1 AND e.state='active' AND e.generation=r.generation",[engagement],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(Error::RunnerAuthority)
}
pub(super) fn read(db: &Connection, id: &str) -> Result<Conversation, Error> {
    let value: String = db
        .query_row(
            "SELECT config FROM internal_conversations WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(&value)?)
}
pub(super) fn scoped(db: &Connection, session_id: &str, id: &str) -> Result<Conversation, Error> {
    let session = execution::session(db, session_id)?;
    with_session(db, &session, id)
}
pub(super) fn admission_scope(
    db: &Connection,
    session_id: &str,
    id: &str,
) -> Result<Conversation, Error> {
    let session = execution::admission_session(db, session_id)?;
    with_session(db, &session, id)
}
fn with_session(db: &Connection, session: &StoredSession, id: &str) -> Result<Conversation, Error> {
    let session_id = session.id();
    let (fleet, project_id, generation) = project(db, session.engagement_id())?;
    let creator:String=db.query_row("SELECT creator_session_id FROM internal_conversations WHERE id=?1 AND state='active' AND fleet_id=?2 AND project_id=?3 AND generation=?4",params![id,fleet,project_id,generation],|r|r.get(0)).optional()?.ok_or(Error::RunnerAuthority)?;
    let value = read(db, id)?;
    if value.id != id || value.creator_session_id != creator || value.state != "active" {
        return Err(Error::State);
    }
    if creator != session_id
        && !matches!(&session,StoredSession::Internal(b) if b.conversation_id==id)
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(value)
}
pub(super) fn recipient(
    db: &Connection,
    conversation: &Conversation,
    id: &str,
) -> Result<StoredSession, Error> {
    let session = execution::admission_session(db, id)?;
    let allowed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM internal_conversations c WHERE c.id=?1 AND c.state='active' AND (c.creator_session_id=?2 OR EXISTS(SELECT 1 FROM internal_participants p WHERE p.conversation_id=c.id AND p.session_id=?2)))",params![conversation.id,id],|r|r.get(0))?;
    if !allowed {
        return Err(Error::RunnerAuthority);
    }
    Ok(session)
}
impl DomainRepository {
    pub fn create_internal_conversation(
        &mut self,
        cap: &RunnerCapability,
        input: &ConversationRequest,
        now: u64,
    ) -> Result<ConversationResult, Error> {
        input.validate()?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let dispatch = execution::authorize_work(&tx, cap, now)?;
        if let Some(id) = &dispatch.task_id
            && execution::task(&tx, id)?.status == TaskState::Done
        {
            return Err(Error::State);
        }
        let creator = execution::session(&tx, &dispatch.session_id)?;
        let (fleet, project_id, generation) = project(&tx, creator.engagement_id())?;
        let mut participants: std::collections::BTreeSet<String> =
            input.participant_engagements.iter().cloned().collect();
        participants.insert(creator.engagement_id().into());
        if participants.len() > 64 {
            return Err(hagency_core::InvalidInput("too many conversation participants").into());
        }
        for id in &participants {
            if project(&tx, id)? != (fleet.clone(), project_id.clone(), generation) {
                return Err(Error::RunnerAuthority);
            }
        }
        let scope = format!("dispatch_{}", cap.dispatch_id);
        let digest = canonical::digest(&json!([
            scope,
            input.call_id,
            input.label,
            participants,
            dispatch.session_id
        ]))?;
        let prior:Option<(String,String)>=tx.query_row("SELECT id,digest FROM internal_conversations WHERE request_scope=?1 AND request_key=?2",params![scope,input.call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((id, old)) = prior {
            return if old == digest {
                Ok(ConversationResult {
                    conversation: scoped(&tx, &dispatch.session_id, &id)?,
                    replayed: true,
                })
            } else {
                Err(Error::Conflict)
            };
        }
        let id = format!(
            "conversation_{}",
            &canonical::digest(&json!([scope, input.call_id]))?[..40]
        );
        bounded_row(&tx, "internal_conversations", "id", &id, 5000)?;
        let bindings: Vec<_> = participants
            .into_iter()
            .map(|engagement| {
                Ok(InternalSessionBinding {
                    kind: InternalKind::Internal,
                    id: format!(
                        "session_{}",
                        &canonical::digest(&json!(["internal", engagement, id]))?[..32]
                    ),
                    engagement_id: engagement,
                    conversation_id: id.clone(),
                })
            })
            .collect::<Result<_, Error>>()?;
        let value = Conversation {
            id: id.clone(),
            label: input.label.clone(),
            creator_session_id: dispatch.session_id.clone(),
            participants: bindings,
            state: "active".into(),
            revision: 0,
        };
        tx.execute("INSERT INTO internal_conversations(id,fleet_id,project_id,generation,creator_session_id,request_scope,request_key,digest,config,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'active')",params![id,fleet,project_id,generation,dispatch.session_id,scope,input.call_id,digest,serialize(&value)?])?;
        for binding in &value.participants {
            binding.validate()?;
            bounded_row(&tx, "runner_sessions", "id", &binding.id, 10_000)?;
            tx.execute(
                "INSERT INTO runner_sessions(id,engagement_id,binding) VALUES(?1,?2,?3)",
                params![binding.id, binding.engagement_id, serialize(binding)?],
            )?;
            tx.execute("INSERT INTO internal_participants(conversation_id,engagement_id,session_id) VALUES(?1,?2,?3)",params![id,binding.engagement_id,binding.id])?;
            execution::session(&tx, &binding.id)?;
        }
        tx.commit()?;
        Ok(ConversationResult {
            conversation: value,
            replayed: false,
        })
    }
    pub fn runner_conversation(
        &self,
        cap: &RunnerCapability,
        id: &str,
        now: u64,
    ) -> Result<Conversation, Error> {
        let d = execution::authorize(&self.db, cap, now, &["started"])?;
        scoped(&self.db, &d.session_id, id)
    }
}
