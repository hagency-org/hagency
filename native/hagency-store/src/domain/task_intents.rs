//! Canonical task activation and delegation; transport commands remain host-only.
use super::{DomainRepository, bounded_row, execution, messages, serialize};
use crate::Error;
use hagency_core::{canonical, messages::Message, project::identifier, task_intents::*, tasks::*};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::json;

fn active_engagement(db: &Connection, id: &str) -> Result<(String, String, String), Error> {
    db.query_row("SELECT e.fleet_id,e.project_id,json_extract(r.config,'$.serverName') FROM engagements e JOIN registrations r ON r.fleet_id=e.fleet_id WHERE e.id=?1 AND e.state='active' AND e.generation=r.generation",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(Error::RunnerAuthority)
}
pub(super) fn notice(db: &Connection, id: &str) -> Result<TaskNotice, Error> {
    let data: String = db
        .query_row("SELECT config FROM task_notices WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(&data)?)
}
fn notice_id(task_id: &str, kind: &str) -> Result<String, Error> {
    Ok(format!(
        "notice_{}",
        canonical::digest(&json!([task_id, kind]))?
    ))
}
pub(super) fn add_notice(
    tx: &Transaction<'_>,
    task: &Task,
    root: &Message,
    kind: &str,
    body: String,
    now: u64,
) -> Result<TaskNotice, Error> {
    let session = execution::matrix_admission_session(tx, &task.session_id)?;
    let id = notice_id(&task.id, kind)?;
    let value = TaskNotice {
        id: id.clone(),
        task_id: task.id.clone(),
        session_id: task.session_id.clone(),
        sender_engagement: session.engagement_id.clone(),
        server_name: root.server_name.clone(),
        room_id: root.room_id.clone(),
        thread_root: if tx.query_row(
            "SELECT matrix_generation>0 FROM runner_sessions WHERE id=?1",
            [&task.session_id],
            |r| r.get::<_, bool>(0),
        )? {
            session.thread_root.clone()
        } else {
            Some(root.event_id.clone())
        },
        transaction_id: format!("hagency_{}", &canonical::digest(&json!(id))?[..40]),
        body,
        kind: kind.into(),
    };
    bounded_row(tx, "task_notices", "id", &id, 30_000)?;
    tx.execute("INSERT INTO task_notices(id,task_id,config,state,not_before) VALUES(?1,?2,?3,'pending',?4)",params![id,task.id,serialize(&value)?,now])?;
    if tx.query_row(
        "SELECT matrix_generation>0 FROM runner_sessions WHERE id=?1",
        [&task.session_id],
        |r| r.get::<_, bool>(0),
    )? {
        let route = super::matrix_routes::route(tx, &task.session_id)?;
        let digest = canonical::digest(&json!([&value, &route, root.event_id]))?;
        tx.execute(
            "UPDATE task_notices SET verified_route=?2,content_digest=?3,task_epoch=?4,source_event_id=?5 WHERE id=?1",
            params![id, serialize(&route)?, digest, task.execution_epoch, root.event_id],
        )?;
    }
    Ok(value)
}
pub(super) fn intent_result(
    db: &Connection,
    id: &str,
    replayed: bool,
) -> Result<IntentResult, Error> {
    let (session_id, activation): (String, String) = db.query_row(
        "SELECT session_id,state FROM task_intents WHERE task_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let command = notice(db, &notice_id(id, "ack")?)?;
    Ok(IntentResult {
        task_id: id.into(),
        session_id,
        command_id: command.id,
        transaction_id: command.transaction_id,
        activation,
        replayed,
    })
}
fn sequences(root: u64, values: &[u64]) -> Result<Vec<u64>, Error> {
    if values.len() > 100 {
        return Err(hagency_core::InvalidInput("too many task inputs").into());
    }
    let mut set = std::collections::BTreeSet::from([root]);
    set.extend(values.iter().copied());
    for sequence in &set {
        clock(*sequence)?;
        if *sequence == 0 {
            return Err(hagency_core::InvalidInput("source sequence must be positive").into());
        }
    }
    if set.len() > 100 {
        return Err(hagency_core::InvalidInput("too many task inputs").into());
    }
    Ok(set.into_iter().collect())
}
fn check_message_scope(root: &Message, input: &Message) -> Result<(), Error> {
    if input.server_name != root.server_name
        || input.room_id != root.room_id
        || (input.sequence != root.sequence && input.thread_root.as_deref() != Some(&root.event_id))
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
fn create_intent(
    tx: &Transaction<'_>,
    input: &TaskIntent,
    creator: Option<&str>,
    now: u64,
) -> Result<IntentResult, Error> {
    identifier(&input.request_scope, 256)?;
    identifier(&input.request_key, 512)?;
    input.definition.validate()?;
    clock(now)?;
    let ids = sequences(input.root_sequence, &input.input_sequences)?;
    let digest = canonical::digest(&json!([
        input.request_scope,
        input.request_key,
        input.assignee_engagement,
        input.root_sequence,
        ids,
        input.definition,
        creator
    ]))?;
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT task_id,digest FROM task_intents WHERE request_scope=?1 AND request_key=?2",
            params![input.request_scope, input.request_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, old)) = prior {
        return if old == digest {
            intent_result(tx, &id, true)
        } else {
            Err(Error::Conflict)
        };
    }
    let (fleet, project, server) = active_engagement(tx, &input.assignee_engagement)?;
    let root = messages::read_message(tx, input.root_sequence)?;
    if root.server_name != server || root.thread_root.is_some() {
        return Err(Error::RunnerAuthority);
    }
    for sequence in &ids {
        check_message_scope(&root, &messages::read_message(tx, *sequence)?)?;
    }
    if let Some(parent) = &input.definition.parent_id {
        let t = execution::task(tx, parent)?;
        let s = execution::admission_session(tx, &t.session_id)?;
        let (f, p, _) = active_engagement(tx, s.engagement_id())?;
        if f != fleet || p != project {
            return Err(Error::RunnerAuthority);
        }
    }
    let session_id = format!(
        "session_{}",
        &canonical::digest(&json!([
            input.assignee_engagement,
            root.room_id,
            root.event_id
        ]))?[..32]
    );
    let binding = SessionBinding {
        id: session_id,
        engagement_id: input.assignee_engagement.clone(),
        room_id: root.room_id.clone(),
        thread_root: Some(root.event_id.clone()),
    };
    let session_id = if let Some(id) = messages::find_session(tx, &binding)? {
        execution::admission_session(tx, &id)?;
        id
    } else {
        bounded_row(tx, "runner_sessions", "id", &binding.id, 10_000)?;
        tx.execute(
            "INSERT INTO runner_sessions(id,engagement_id,binding) VALUES(?1,?2,?3)",
            params![binding.id, binding.engagement_id, serialize(&binding)?],
        )?;
        binding.id
    };
    persist_intent(
        tx,
        input,
        creator,
        now,
        IntentProjection {
            session_id: &session_id,
            root: &root,
            sequences: &ids,
        },
        &digest,
    )
}
pub(super) struct IntentProjection<'a> {
    pub session_id: &'a str,
    pub root: &'a Message,
    pub sequences: &'a [u64],
}
pub(super) fn persist_intent(
    tx: &Transaction<'_>,
    input: &TaskIntent,
    creator: Option<&str>,
    now: u64,
    projection: IntentProjection<'_>,
    digest: &str,
) -> Result<IntentResult, Error> {
    let IntentProjection {
        session_id,
        root,
        sequences: ids,
    } = projection;
    let existing: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_intents WHERE session_id=?1)",
        [&session_id],
        |r| r.get(0),
    )?;
    if existing {
        return Err(Error::Conflict);
    }
    let id = format!(
        "task_{}",
        &canonical::digest(&json!([input.request_scope, input.request_key]))?[..40]
    );
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM canonical_tasks WHERE id=?1)",
        [&id],
        |r| r.get(0),
    )?;
    if exists {
        return Err(Error::Conflict);
    }
    let mut task =
        execution::create_task(tx, &id, session_id, creator, &input.definition.title, now)?;
    task.description = input.definition.description.clone();
    task.priority = input.definition.priority;
    task.granularity = input.definition.granularity;
    task.labels = input.definition.labels.clone();
    task.parent_id = input.definition.parent_id.clone();
    execution::save_task(tx, &task, "intent_pending")?;
    tx.execute("INSERT INTO task_intents(task_id,request_scope,request_key,digest,session_id,root_sequence,state) VALUES(?1,?2,?3,?4,?5,?6,'pending')",params![id,input.request_scope,input.request_key,digest,session_id,root.sequence])?;
    for seq in ids {
        tx.execute(
            "INSERT INTO task_inputs(task_id,message_sequence) VALUES(?1,?2)",
            params![id, seq],
        )?;
    }
    add_notice(
        tx,
        &task,
        root,
        "ack",
        format!("Task created: {}", task.title),
        now,
    )?;
    intent_result(tx, &id, false)
}

pub(super) fn project_inputs(tx: &Transaction<'_>, task_id: &str) -> Result<(), Error> {
    let session: String = tx.query_row(
        "SELECT session_id FROM task_intents WHERE task_id=?1",
        [task_id],
        |r| r.get(0),
    )?;
    let pending: u64 = tx.query_row(
        "SELECT COUNT(*) FROM session_inputs WHERE session_id=?1 AND processed_at IS NULL",
        [&session],
        |r| r.get(0),
    )?;
    let added:u64=tx.query_row("SELECT COUNT(*) FROM task_inputs ti WHERE ti.task_id=?1 AND NOT EXISTS(SELECT 1 FROM session_inputs si WHERE si.session_id=?2 AND si.message_sequence=ti.message_sequence)",params![task_id,session],|r|r.get(0))?;
    if pending + added > 2000 {
        return Err(Error::Capacity);
    }
    tx.execute("INSERT OR IGNORE INTO session_inputs(session_id,message_sequence,wake,config) SELECT ?2,message_sequence,COALESCE(wake,1),config FROM task_inputs WHERE task_id=?1",params![task_id,session])?;
    super::attachments::project_task_inputs(tx, task_id, &session)?;
    Ok(())
}
fn binding(db: &Connection, id: &str) -> Result<Option<(String, u64)>, Error> {
    Ok(db
        .query_row(
            "SELECT state,root_sequence FROM task_intents WHERE task_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}
pub(super) fn check_enqueue(db: &Connection, task: &Task) -> Result<(), Error> {
    match binding(db, &task.id)? {
        Some((state, _)) if state != "active" => Err(Error::State),
        Some(_) => Ok(()), // Done requires a fresh attached input at claim/start below.
        None if task.status == TaskState::Done => Err(Error::State),
        None => Ok(()), // Explicit host-created kernel/local task, no Matrix activation.
    }
}
pub(super) fn check_session_task(
    db: &Connection,
    session: &str,
    task: Option<&str>,
) -> Result<(), Error> {
    let bound: Option<(String, String)> = db
        .query_row(
            "SELECT task_id,state FROM task_intents WHERE session_id=?1",
            [session],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, state)) = bound
        && (Some(id.as_str()) != task || state != "active")
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
pub(super) fn check_input(db: &Connection, task: Option<&str>, sequence: u64) -> Result<(), Error> {
    if let Some(id) = task
        && binding(db, id)?.is_some()
    {
        let attached: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_inputs WHERE task_id=?1 AND message_sequence=?2)",
            params![id, sequence],
            |r| r.get(0),
        )?;
        if !attached {
            return Err(Error::RunnerAuthority);
        }
    }
    Ok(())
}
pub(super) fn fresh_followup(
    db: &Connection,
    task: &Task,
    dispatch_id: &str,
) -> Result<bool, Error> {
    let Some((state, _)) = binding(db, &task.id)? else {
        return Ok(false);
    };
    if state != "active" || task.status != TaskState::Done {
        return Ok(false);
    }
    let valid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_followup_ready WHERE task_id=?1 AND dispatch_id=?2)",
        params![task.id, dispatch_id],
        |r| r.get(0),
    )?;
    Ok(valid)
}
pub(super) fn start_task(
    tx: &Transaction<'_>,
    task: &mut Task,
    dispatch_id: &str,
    now: u64,
) -> Result<(), Error> {
    check_enqueue(tx, task)?;
    if binding(tx, &task.id)?.is_some() {
        let attached:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM task_dispatch_input_ready WHERE dispatch_id=?1 AND task_id=?2)",params![dispatch_id,task.id],|r|r.get(0))?;
        if !attached {
            return Err(Error::RunnerAuthority);
        }
    }
    if task.status == TaskState::Done {
        if !fresh_followup(tx, task, dispatch_id)? {
            return Err(Error::RunnerAuthority);
        }
        task.status = TaskState::InProgress;
        task.completed_at = None;
        task.updated_at = now;
        task.waiting_reason = None;
        task.waiting_until = None;
        task.execution_epoch = task
            .execution_epoch
            .checked_add(1)
            .filter(|v| *v <= hagency_core::JSON_SAFE_MAX)
            .ok_or(Error::Capacity)?;
        execution::save_task(tx, task, "human_followup")?;
        let (_, root) = binding(tx, &task.id)?.ok_or(Error::State)?;
        add_notice(
            tx,
            task,
            &super::verified_ingress::task_message(tx, &task.id, root)?,
            &format!("followup_{}", task.execution_epoch),
            format!("Continuing task: {}", task.title),
            now,
        )?;
    }
    Ok(())
}
impl DomainRepository {
    pub fn create_task_intent(
        &mut self,
        input: &TaskIntent,
        now: u64,
    ) -> Result<IntentResult, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = create_intent(&tx, input, None, now)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn delegate_task(
        &mut self,
        cap: &RunnerCapability,
        input: &Delegation,
        now: u64,
    ) -> Result<IntentResult, Error> {
        input.definition.validate()?;
        identifier(&input.call_id, 512)?;
        identifier(&input.assignee_engagement, 128)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let d = execution::authorize_work(&tx, cap, now)?;
        if let Some(id) = &d.task_id
            && execution::task(&tx, id)?.status == TaskState::Done
        {
            return Err(Error::State);
        }
        if input.definition.parent_id.is_some() && input.definition.parent_id != d.task_id {
            return Err(Error::RunnerAuthority);
        }
        let s = execution::admission_session(&tx, &d.session_id)?;
        let (f, p, _) = active_engagement(&tx, s.engagement_id())?;
        let (tf, tp, _) = active_engagement(&tx, &input.assignee_engagement)?;
        if f != tf || p != tp {
            return Err(Error::RunnerAuthority);
        }
        let root = if let Some(root) = input.root_sequence {
            root
        } else if let Some(id) = &d.task_id {
            binding(&tx, id)?
                .map(|(_, seq)| seq)
                .ok_or(Error::RunnerAuthority)?
        } else {
            tx.query_row("SELECT m.sequence FROM admitted_messages m JOIN session_inputs si ON si.message_sequence=m.sequence WHERE si.session_id=?1 AND (si.processed_at IS NOT NULL OR si.dispatch_id=?2) AND json_extract(m.config,'$.thread_root') IS NULL ORDER BY m.sequence DESC LIMIT 1",params![d.session_id,cap.dispatch_id],|r|r.get(0)).optional()?.ok_or(Error::RunnerAuthority)?
        };
        let ids = sequences(root, &input.input_sequences)?;
        for seq in &ids {
            let visible:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM session_inputs WHERE session_id=?1 AND message_sequence=?2 AND (processed_at IS NOT NULL OR dispatch_id=?3))",params![d.session_id,seq,cap.dispatch_id],|r|r.get(0))?;
            if !visible {
                return Err(Error::RunnerAuthority);
            }
        }
        let intent = TaskIntent {
            request_scope: format!("dispatch_{}", cap.dispatch_id),
            request_key: input.call_id.clone(),
            assignee_engagement: input.assignee_engagement.clone(),
            root_sequence: root,
            input_sequences: ids,
            definition: input.definition.clone(),
        };
        let result = create_intent(&tx, &intent, Some(&d.session_id), now)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn attach_task_inputs(
        &mut self,
        id: &str,
        scope: &str,
        key: &str,
        values: &[u64],
    ) -> Result<(), Error> {
        identifier(scope, 256)?;
        identifier(key, 512)?;
        if values.is_empty() || values.len() > 100 {
            return Err(hagency_core::InvalidInput("attachment requires 1..100 inputs").into());
        }
        let ids: Vec<_> = values
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for sequence in &ids {
            clock(*sequence)?;
        }
        let digest = canonical::digest(&json!([id, ids]))?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT digest FROM task_input_receipts WHERE scope=?1 AND request_key=?2",
                params![scope, key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = prior {
            return if old == digest {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        let task = execution::task(&tx, id)?;
        if tx.query_row(
            "SELECT matrix_generation>0 FROM runner_sessions WHERE id=?1",
            [&task.session_id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::RunnerAuthority);
        }
        execution::admission_session(&tx, &task.session_id)?;
        let (state, root_seq) = binding(&tx, id)?.ok_or(Error::NotFound)?;
        if state == "closed" {
            return Err(Error::State);
        }
        let root = messages::read_message(&tx, root_seq)?;
        for seq in ids {
            let input = messages::read_message(&tx, seq)?;
            check_message_scope(&root, &input)?;
            // Only a currently admitted projection (or original pending root) may
            // attach. The host cannot smuggle a different conversation's event.
            let visible:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM session_inputs WHERE session_id=?1 AND message_sequence=?2)",params![task.session_id,seq],|r|r.get(0))?;
            if !visible && seq != root_seq {
                return Err(Error::RunnerAuthority);
            }
            tx.execute(
                "INSERT OR IGNORE INTO task_inputs(task_id,message_sequence) VALUES(?1,?2)",
                params![id, seq],
            )?;
        }
        let receipts: u64 =
            tx.query_row("SELECT COUNT(*) FROM task_input_receipts", [], |r| r.get(0))?;
        if receipts >= 100_000 {
            return Err(Error::Capacity);
        }
        tx.execute(
            "INSERT INTO task_input_receipts(scope,request_key,digest,task_id) VALUES(?1,?2,?3,?4)",
            params![scope, key, digest, id],
        )?;
        if state == "active" {
            project_inputs(&tx, id)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn claim_task_notice(
        &mut self,
        now: u64,
        lease_ms: u64,
    ) -> Result<Option<NoticeClaim>, Error> {
        clock(now)?;
        if !(1..=300_000).contains(&lease_ms) || now > hagency_core::JSON_SAFE_MAX - lease_ms {
            return Err(hagency_core::InvalidInput("invalid notice lease").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids=tx.prepare("SELECT id FROM task_notices WHERE verified_route IS NULL AND not_before<=?1 AND (state='pending' OR (state='claimed' AND claim_until<=?1)) ORDER BY rowid LIMIT 128")?.query_map([now],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        for id in ids {
            let value = notice(&tx, &id)?;
            match execution::matrix_admission_session(&tx, &value.session_id) {
                Ok(_) => {}
                Err(Error::RunnerAuthority) => {
                    tx.execute("UPDATE task_notices SET state='cancelled',claim_hash=NULL,claim_until=NULL,error_code='allocation_inactive' WHERE id=?1",[&id])?;
                    continue;
                }
                Err(error) => return Err(error),
            }
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
            let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            let hash = canonical::digest(&json!(token))?;
            tx.execute(
                "UPDATE task_notices SET state='claimed',claim_hash=?2,claim_until=?3 WHERE id=?1",
                params![id, hash, now + lease_ms],
            )?;
            tx.commit()?;
            return Ok(Some(NoticeClaim {
                notice: value,
                token,
                deadline: now + lease_ms,
            }));
        }
        tx.commit()?;
        Ok(None)
    }
    pub fn deliver_task_notice(
        &mut self,
        id: &str,
        token: &str,
        delivery: &NoticeDelivery,
        now: u64,
    ) -> Result<IntentResult, Error> {
        clock(now)?;
        delivery.validate()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.query_row(
            "SELECT verified_route IS NOT NULL FROM task_notices WHERE id=?1",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::RunnerAuthority);
        }
        let command = notice(&tx, id)?;
        execution::matrix_admission_session(&tx, &command.session_id)?;
        if command.server_name != delivery.server_name
            || command.room_id != delivery.room_id
            || command.transaction_id != delivery.transaction_id
        {
            return Err(Error::RunnerAuthority);
        }
        let (state, hash, deadline, old): (String, Option<String>, Option<u64>, Option<String>) =
            tx.query_row(
                "SELECT state,claim_hash,claim_until,delivery FROM task_notices WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        let encoded = serialize(delivery)?;
        if state == "delivered" {
            return if old.as_deref() == Some(&encoded) {
                intent_result(&tx, &command.task_id, true)
            } else {
                Err(Error::Conflict)
            };
        }
        if state != "claimed"
            || deadline.is_none_or(|d| d <= now)
            || !execution::matches_secret(hash.as_deref().unwrap_or(""), token)?
        {
            return Err(Error::RunnerAuthority);
        }
        tx.execute("UPDATE task_notices SET state='delivered',delivery=?2,claim_hash=NULL,claim_until=NULL WHERE id=?1",params![id,encoded])?;
        if command.kind == "ack" {
            if tx.execute("UPDATE task_intents SET state='active',anchor_event_id=?2 WHERE task_id=?1 AND state='pending'",params![command.task_id,delivery.event_id])?!=1 {return Err(Error::State);}
            project_inputs(&tx, &command.task_id)?;
            let task = execution::task(&tx, &command.task_id)?;
            execution::save_task(&tx, &task, "thread_active")?;
        }
        let result = intent_result(&tx, &command.task_id, false)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn fail_task_notice(
        &mut self,
        id: &str,
        token: &str,
        code: &str,
        permanent: bool,
        now: u64,
    ) -> Result<(), Error> {
        identifier(code, 128)?;
        clock(now)?;
        if now > hagency_core::JSON_SAFE_MAX - 1000 {
            return Err(hagency_core::InvalidInput("invalid notice retry clock").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.query_row(
            "SELECT verified_route IS NOT NULL FROM task_notices WHERE id=?1",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::RunnerAuthority);
        }
        let (state, hash, deadline): (String, Option<String>, Option<u64>) = tx
            .query_row(
                "SELECT state,claim_hash,claim_until FROM task_notices WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if state != "claimed"
            || deadline.is_none_or(|d| d <= now)
            || !execution::matches_secret(hash.as_deref().unwrap_or(""), token)?
        {
            return Err(Error::RunnerAuthority);
        }
        tx.execute("UPDATE task_notices SET state=?2,claim_hash=NULL,claim_until=NULL,error_code=?3,not_before=?4 WHERE id=?1",params![id,if permanent{"failed"}else{"pending"},code,now.saturating_add(1000)])?;
        tx.commit()?;
        Ok(())
    }
    pub fn retry_task_notice(&mut self, id: &str, now: u64) -> Result<(), Error> {
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.query_row(
            "SELECT verified_route IS NOT NULL FROM task_notices WHERE id=?1",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::RunnerAuthority);
        }
        let n = notice(&tx, id)?;
        execution::matrix_admission_session(&tx, &n.session_id)?;
        if tx.execute("UPDATE task_notices SET state='pending',error_code=NULL,not_before=?2 WHERE id=?1 AND state='failed'",params![id,now])?!=1 {return Err(Error::State);}
        tx.commit()?;
        Ok(())
    }
}
