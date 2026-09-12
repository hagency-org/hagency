//! A finite graph authorizes only its pre-created canonical tasks. Progress,
//! assignment input and receipts commit in the domain transaction.
use super::{DomainRepository, bounded_row, conversations, execution, peers};
use crate::Error;
use hagency_core::{canonical, graphs::*, peers::*, project::identifier, tasks::*, workflows::*};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn graph_state(state: GraphStatus) -> &'static str {
    match state {
        GraphStatus::Active => "active",
        GraphStatus::Complete => "complete",
        GraphStatus::Failed => "failed",
        GraphStatus::Cancelled => "cancelled",
    }
}
fn node_state(state: NodeStatus) -> &'static str {
    match state {
        NodeStatus::Pending => "pending",
        NodeStatus::Dispatched => "dispatched",
        NodeStatus::Active => "active",
        NodeStatus::Complete => "complete",
        NodeStatus::Failed => "failed",
        NodeStatus::Skipped => "skipped",
        NodeStatus::Cancelled => "cancelled",
    }
}
fn encoded(value: &impl Serialize) -> Result<String, Error> {
    Ok(canonical::encode_payload(&serde_json::to_value(value)?)?)
}
fn read(db: &Connection, id: &str) -> Result<Workflow, Error> {
    let row: Option<(String, String, String, String)> = db
        .query_row(
            "SELECT config,state,conversation_id,creator_session_id FROM task_graphs WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let (config, state, conversation, creator) = row.ok_or(Error::NotFound)?;
    let value: Workflow = serde_json::from_str(&config)?;
    value.graph.validate()?;
    if value.graph.progress.values().any(|p| !p.result.is_null()) {
        return Err(Error::State);
    }
    if value.id != id
        || value.conversation_id != conversation
        || value.creator_session_id != creator
        || graph_state(value.graph.status) != state
        || value.nodes.len() != value.graph.definition.nodes.len()
    {
        return Err(Error::State);
    }
    let rows=db.prepare("SELECT node_id,session_id,task_id,message_sequence,completed_epoch,state FROM graph_nodes WHERE graph_id=?1")?.query_map([id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<u64>>(3)?,r.get::<_,Option<u64>>(4)?,r.get::<_,String>(5)?)))?.collect::<Result<Vec<_>,_>>()?;
    if rows.len() != value.nodes.len() {
        return Err(Error::State);
    }
    for (node, session, task, message, epoch, state) in rows {
        let bound = value.nodes.get(&node).ok_or(Error::State)?;
        let definition = value
            .graph
            .definition
            .nodes
            .iter()
            .find(|n| n.id == node)
            .ok_or(Error::State)?;
        if bound.session_id != session
            || bound.task_id != task
            || bound.message_sequence != message
            || bound.completed_epoch != epoch
            || definition.assignee != session
            || node_state(value.graph.progress[&node].status) != state
        {
            return Err(Error::State);
        }
        let task = execution::task(db, &task)?;
        if task.id != bound.task_id
            || task.session_id != session
            || task.creator_session_id.as_deref() != Some(&creator)
        {
            return Err(Error::State);
        }
        if let Some(epoch) = epoch
            && (task.status != TaskState::Done || task.execution_epoch != epoch)
        {
            return Err(Error::State);
        }
    }
    Ok(value)
}
fn read_result(db: &Connection, id: &str, node: &str) -> Result<(u64, String, Value), Error> {
    let row:Option<(String,u64,String,String)>=db.query_row("SELECT task_id,completed_epoch,result_digest,result_value FROM graph_nodes WHERE graph_id=?1 AND node_id=?2 AND completed_epoch IS NOT NULL AND result_value IS NOT NULL",params![id,node],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let (task_id, epoch, digest, body) = row.ok_or(Error::State)?;
    let result: Value = serde_json::from_str(&body)?;
    validate_result(&result)?;
    let task = execution::task(db, &task_id)?;
    let observed = canonical::payload_digest(&json!([
        "graph_result",
        id,
        node,
        task_id,
        epoch,
        WorkflowOutcome::Complete {
            result: result.clone()
        }
    ]))?;
    if observed != digest
        || task.id != task_id
        || task.status != TaskState::Done
        || task.execution_epoch != epoch
    {
        return Err(Error::State);
    }
    Ok((epoch, digest, result))
}
fn current(db: &Connection, value: &Workflow) -> Result<(), Error> {
    let valid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM current_graph_scopes WHERE id=?1)",
        [&value.id],
        |r| r.get(0),
    )?;
    if !valid {
        return Err(Error::RunnerAuthority);
    }
    conversations::admission_scope(db, &value.creator_session_id, &value.conversation_id)?;
    Ok(())
}
fn owner(db: &Connection, session: &str, value: &Workflow) -> Result<(), Error> {
    if value.creator_session_id != session {
        return Err(Error::RunnerAuthority);
    }
    current(db, value)
}
fn save(tx: &Transaction<'_>, value: &Workflow) -> Result<(), Error> {
    value.graph.validate()?;
    tx.execute(
        "UPDATE task_graphs SET state=?2,config=?3 WHERE id=?1",
        params![value.id, graph_state(value.graph.status), encoded(value)?],
    )?;
    for (id, node) in &value.nodes {
        tx.execute("UPDATE graph_nodes SET state=?3,message_sequence=?4,completed_epoch=?5 WHERE graph_id=?1 AND node_id=?2",params![value.id,id,node_state(value.graph.progress[id].status),node.message_sequence,node.completed_epoch])?;
    }
    Ok(())
}
fn replay<T: DeserializeOwned>(
    db: &Connection,
    cap: &RunnerCapability,
    key: &str,
    digest: &str,
) -> Result<Option<T>, Error> {
    let row: Option<(String, String)> = db
        .query_row(
            "SELECT digest,response FROM graph_commands WHERE dispatch_id=?1 AND call_id=?2",
            params![cap.dispatch_id, key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some((old, body)) if old == digest => Ok(Some(serde_json::from_str(&body)?)),
        _ => Err(Error::Conflict),
    }
}
fn receipt(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    key: &str,
    digest: &str,
    value: &impl Serialize,
) -> Result<(), Error> {
    let (own,total):(u64,u64)=tx.query_row("SELECT (SELECT COUNT(*) FROM graph_commands WHERE dispatch_id=?1),(SELECT COUNT(*) FROM graph_commands)",[&cap.dispatch_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if own >= 1024 || total >= 100_000 {
        return Err(Error::Capacity);
    }
    tx.execute(
        "INSERT INTO graph_commands(dispatch_id,call_id,digest,response) VALUES(?1,?2,?3,?4)",
        params![cap.dispatch_id, key, digest, encoded(value)?],
    )?;
    Ok(())
}
fn advance(tx: &Transaction<'_>, value: &mut Workflow, now: u64) -> Result<(), Error> {
    current(tx, value)?;
    for progress in value.graph.progress.values_mut() {
        progress.result = Value::Null;
    }
    let mut needed = BTreeSet::new();
    for node in &value.graph.definition.nodes {
        if value.graph.progress[&node.id].status != NodeStatus::Pending {
            continue;
        }
        if let Some(condition) = &node.condition {
            let dep = condition
                .0
                .get("dep")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| node.depends_on.first().map(String::as_str));
            if let Some(dep) = dep
                && value.graph.progress[dep].status == NodeStatus::Complete
            {
                needed.insert(dep.to_owned());
            }
        }
    }
    for dep in needed {
        value
            .graph
            .progress
            .get_mut(&dep)
            .ok_or(Error::State)?
            .result = read_result(tx, &value.id, &dep)?.2;
    }
    let transition = value.graph.advance_references()?;
    let source: String = tx.query_row(
        "SELECT creator_dispatch_id FROM task_graphs WHERE id=?1",
        [&value.id],
        |r| r.get(0),
    )?;
    let mut capacity = peers::PeerCapacity::new(tx);
    for assignment in &transition.assignments {
        let target = value.nodes.get(&assignment.node_id).ok_or(Error::State)?;
        if target.message_sequence.is_some() {
            return Err(Error::State);
        }
        let key = format!(
            "peer_{}",
            canonical::digest(&json!(["graph_assignment", value.id, assignment.node_id]))?
        );
        // Full dependency values stay immutable in the graph store. A bounded
        // page API exposes only this assignment's explicitly pinned dependencies.
        let input = PeerSend {
            call_id: target.task_id.clone(),
            conversation_id: value.conversation_id.clone(),
            recipient_session_ids: vec![target.session_id.clone()],
            kind: PeerKind::Request,
            priority: PeerPriority::High,
            summary: "Canonical graph task assignment".into(),
            body: assignment.description.clone(),
            data: json!({"graph_id":value.id,"node_id":assignment.node_id,"task_id":target.task_id,"dependency_count":assignment.dependency_results.len()}),
        };
        let admitted = peers::admit(
            &mut capacity,
            peers::PeerOrigin {
                session_id: &value.creator_session_id,
                dispatch_id: &source,
                task_id: value.parent_task_id.as_deref(),
            },
            &input,
            &key,
            now,
        )?;
        value
            .nodes
            .get_mut(&assignment.node_id)
            .ok_or(Error::State)?
            .message_sequence = Some(admitted.sequence);
        for (index, dependency) in assignment.dependency_results.iter().enumerate() {
            let node = value.nodes.get(&dependency.node_id).ok_or(Error::State)?;
            let epoch = node.completed_epoch.ok_or(Error::State)?;
            let digest: String = tx.query_row(
                "SELECT result_digest FROM graph_nodes WHERE graph_id=?1 AND node_id=?2",
                params![value.id, dependency.node_id],
                |r| r.get(0),
            )?;
            tx.execute("INSERT INTO graph_dependencies(graph_id,node_id,sequence,dependency_id,task_id,execution_epoch,digest) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![value.id,assignment.node_id,index+1,dependency.node_id,node.task_id,epoch,digest])?;
        }
    }
    value.graph = transition.graph;
    for progress in value.graph.progress.values_mut() {
        progress.result = Value::Null;
    }
    save(tx, value)
}
fn fence_tasks(tx: &Transaction<'_>, id: &str, node: Option<&str>, now: u64) -> Result<(), Error> {
    let ids:Vec<String>=tx.prepare("SELECT d.id FROM runner_dispatches d LEFT JOIN dispatch_recovery_reports r ON r.dispatch_id=d.id JOIN graph_nodes n ON n.task_id=COALESCE(d.task_id,r.task_id) WHERE n.graph_id=?1 AND (?2 IS NULL OR n.node_id=?2) AND d.state IN ('queued','leased','started','parked','outcome_unknown')")?.query_map(params![id,node],|r|r.get(0))?.collect::<Result<_,_>>()?;
    for dispatch in ids {
        super::conversation_lifecycle::fence_dispatch(tx, &dispatch, id, now)?;
    }
    Ok(())
}
fn cancel(tx: &Transaction<'_>, value: &mut Workflow, now: u64) -> Result<(), Error> {
    if value.graph.status != GraphStatus::Cancelled {
        value.graph = value.graph.cancel()?;
        save(tx, value)?;
    }
    fence_tasks(tx, &value.id, None, now)
}
pub(super) fn reconcile(tx: &Transaction<'_>, now: u64) -> Result<(), Error> {
    let ids:Vec<String>=tx.prepare("SELECT id FROM task_graphs g WHERE NOT EXISTS(SELECT 1 FROM current_graph_scopes s WHERE s.id=g.id) AND (state='active' OR EXISTS(SELECT 1 FROM graph_nodes n JOIN runner_dispatches d LEFT JOIN dispatch_recovery_reports r ON r.dispatch_id=d.id WHERE n.graph_id=g.id AND n.task_id=COALESCE(d.task_id,r.task_id) AND (d.state IN ('queued','leased','started','parked') OR EXISTS(SELECT 1 FROM unresolved_dispatches u WHERE u.id=d.id))))")?.query_map([],|r|r.get(0))?.collect::<Result<_,_>>()?;
    for id in ids {
        let mut value = read(tx, &id)?;
        if value.graph.status == GraphStatus::Active {
            cancel(tx, &mut value, now)?;
        } else {
            fence_tasks(tx, &id, None, now)?;
        }
    }
    Ok(())
}
pub(super) fn now_ms() -> Result<u64, Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or(Error::Unavailable)?;
    clock(now)?;
    Ok(now)
}
pub(super) fn is_graph_task(db: &Connection, id: Option<&str>) -> Result<bool, Error> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM graph_nodes WHERE task_id=?1)",
        [id],
        |r| r.get(0),
    )?)
}
/// Generic dispatch entrypoints cannot omit the canonical graph assignment.
pub(super) fn admit_inputs(
    db: &Connection,
    input: &DispatchInput,
    sequences: &[u64],
) -> Result<(), Error> {
    for sequence in sequences {
        let bound: Option<String> = db
            .query_row(
                "SELECT task_id FROM graph_nodes WHERE message_sequence=?1",
                [sequence],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(task) = bound
            && input.task_id.as_deref() != Some(&task)
        {
            return Err(Error::RunnerAuthority);
        }
    }
    if !is_graph_task(db, input.task_id.as_deref())? {
        return Ok(());
    }
    let row:Option<u64>=db.query_row("SELECT n.message_sequence FROM graph_nodes n JOIN task_graphs g ON g.id=n.graph_id JOIN current_graph_scopes s ON s.id=g.id WHERE n.task_id=?1 AND n.session_id=?2 AND n.state IN ('dispatched','active') AND g.state='active'",params![input.task_id,input.session_id],|r|r.get(0)).optional()?;
    if !row.is_some_and(|sequence| sequences.contains(&sequence)) {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
pub(super) fn check_dispatch(
    db: &Connection,
    id: &str,
    d: &execution::Dispatch,
) -> Result<(), Error> {
    if !is_graph_task(db, d.task_id.as_deref().or(d.report_task.as_deref()))? {
        return Ok(());
    }
    let valid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM graph_dispatch_scope WHERE dispatch_id=?1)",
        [id],
        |r| r.get(0),
    )?;
    if !valid {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
pub(super) fn admit_recovery(
    db: &Connection,
    original: &str,
    task: Option<&str>,
    session: &str,
) -> Result<(), Error> {
    if !is_graph_task(db, task)? {
        return Ok(());
    }
    let valid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM graph_nodes n JOIN task_graphs g ON g.id=n.graph_id JOIN current_graph_scopes s ON s.id=g.id JOIN peer_dispatch_inputs pi ON pi.dispatch_id=?1 AND pi.message_sequence=n.message_sequence JOIN peer_session_inputs si ON si.session_id=n.session_id AND si.message_sequence=pi.message_sequence WHERE n.task_id=?2 AND n.session_id=?3 AND si.dispatch_id=?1 AND si.processed_at IS NULL AND ((g.state='active' AND n.state IN ('dispatched','active')) OR n.state='complete'))",params![original,task,session],|r|r.get(0))?;
    if !valid {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
pub(super) fn started(tx: &Transaction<'_>, id: &str) -> Result<(), Error> {
    let row:Option<(String,String)>=tx.query_row("SELECT n.graph_id,n.node_id FROM runner_dispatches d JOIN graph_nodes n ON n.task_id=d.task_id WHERE d.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((graph, node)) = row {
        let mut value = read(tx, &graph)?;
        value.graph = value.graph.observe(&node, &NodeObservation::Active)?;
        save(tx, &value)?;
    }
    Ok(())
}
pub(super) fn complete_guard(db: &Connection, d: &execution::Dispatch) -> Result<(), Error> {
    let task = d.task_id.as_deref().or(d.report_task.as_deref());
    if !is_graph_task(db, task)? {
        return Ok(());
    }
    let done:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM graph_nodes n JOIN canonical_tasks t ON t.id=n.task_id WHERE n.task_id=?1 AND n.state='complete' AND n.completed_epoch=json_extract(t.config,'$.execution_epoch') AND json_extract(t.config,'$.status')='done' AND n.result_receipt IS NOT NULL)",[task],|r|r.get(0))?;
    if !done {
        return Err(Error::State);
    }
    Ok(())
}
fn node_for_runner(
    db: &Connection,
    id: &str,
    d: &execution::Dispatch,
) -> Result<(Workflow, String), Error> {
    let task = d
        .task_id
        .as_deref()
        .or(d.report_task.as_deref())
        .ok_or(Error::RunnerAuthority)?;
    let node: String = db
        .query_row(
            "SELECT node_id FROM graph_nodes WHERE graph_id=?1 AND task_id=?2 AND session_id=?3",
            params![id, task, d.session_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::RunnerAuthority)?;
    let value = read(db, id)?;
    current(db, &value)?;
    Ok((value, node))
}

impl DomainRepository {
    pub fn create_workflow(
        &mut self,
        cap: &RunnerCapability,
        input: &WorkflowRequest,
        now: u64,
    ) -> Result<WorkflowReceipt, Error> {
        input.validate()?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let d = execution::authorize_work(&tx, cap, now)?;
        if is_graph_task(&tx, d.task_id.as_deref())? {
            return Err(Error::RunnerAuthority);
        }
        if let Some(id) = &d.task_id
            && execution::task(&tx, id)?.status == TaskState::Done
        {
            return Err(Error::State);
        }
        let conversation = conversations::scoped(&tx, &d.session_id, &input.conversation_id)?;
        let digest = canonical::payload_digest(&json!(["create_graph", input]))?;
        if let Some(mut old) = replay::<WorkflowReceipt>(&tx, cap, &input.call_id, &digest)? {
            owner(&tx, &d.session_id, &read(&tx, &old.workflow.id)?)?;
            old.replayed = true;
            return Ok(old);
        }
        for node in &input.definition.nodes {
            if !conversation
                .participants
                .iter()
                .any(|p| p.id == node.assignee)
            {
                return Err(Error::RunnerAuthority);
            }
            conversations::recipient(&tx, &conversation, &node.assignee)?;
        }
        let id = format!(
            "graph_{}",
            &canonical::digest(&json!([cap.dispatch_id, input.call_id]))?[..40]
        );
        bounded_row(&tx, "task_graphs", "id", &id, 1000)?;
        let source = execution::session(&tx, &d.session_id)?;
        let (fleet, project, generation) = conversations::project(&tx, source.engagement_id())?;
        let mut value = Workflow {
            id: id.clone(),
            conversation_id: input.conversation_id.clone(),
            creator_session_id: d.session_id.clone(),
            parent_task_id: d.task_id.clone(),
            created_at: now,
            graph: Graph::new(input.definition.clone())?,
            nodes: BTreeMap::new(),
        };
        tx.execute("INSERT INTO task_graphs(id,conversation_id,creator_session_id,creator_dispatch_id,parent_task_id,fleet_id,project_id,generation,state,config,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'active','{}',?9)",params![id,input.conversation_id,d.session_id,cap.dispatch_id,d.task_id,fleet,project,generation,now])?;
        for node in &input.definition.nodes {
            let task_id = format!(
                "task_{}",
                &canonical::digest(&json!(["graph_node", id, node.id]))?[..48]
            );
            let mut task = execution::create_task(
                &tx,
                &task_id,
                &node.assignee,
                Some(&d.session_id),
                &node.description,
                now,
            )?;
            task.parent_id = d.task_id.clone();
            execution::save_task(&tx, &task, "graph_bound")?;
            tx.execute("INSERT INTO graph_nodes(graph_id,node_id,session_id,task_id,state) VALUES(?1,?2,?3,?4,'pending')",params![id,node.id,node.assignee,task_id])?;
            value.nodes.insert(
                node.id.clone(),
                WorkflowNode {
                    task_id,
                    session_id: node.assignee.clone(),
                    message_sequence: None,
                    completed_epoch: None,
                },
            );
        }
        advance(&tx, &mut value, now)?;
        let result = WorkflowReceipt {
            workflow: value.view(),
            replayed: false,
        };
        receipt(&tx, cap, &input.call_id, &digest, &result)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn runner_workflow(
        &self,
        cap: &RunnerCapability,
        id: &str,
        now: u64,
    ) -> Result<WorkflowView, Error> {
        let d = execution::authorize(&self.db, cap, now, &["started"])?;
        if d.report_task.is_some() {
            return Err(Error::RunnerAuthority);
        }
        let value = read(&self.db, id)?;
        owner(&self.db, &d.session_id, &value)?;
        Ok(value.view())
    }
    pub fn runner_workflows(
        &self,
        cap: &RunnerCapability,
        after: &str,
        limit: usize,
        now: u64,
    ) -> Result<Vec<WorkflowSummary>, Error> {
        let d = execution::authorize(&self.db, cap, now, &["started"])?;
        if d.report_task.is_some() {
            return Err(Error::RunnerAuthority);
        }
        if !(1..=100).contains(&limit) || after.len() > 128 {
            return Err(hagency_core::InvalidInput("invalid graph page").into());
        }
        let rows=self.db.prepare("SELECT g.id,g.conversation_id,json_extract(g.config,'$.graph.definition.label'),g.state,g.created_at,(SELECT COUNT(*) FROM graph_nodes n WHERE n.graph_id=g.id) FROM task_graphs g JOIN current_graph_scopes s ON s.id=g.id WHERE g.creator_session_id=?1 AND g.id>?2 ORDER BY g.id LIMIT ?3")?.query_map(params![d.session_id,after,limit],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,u64>(4)?,r.get::<_,u64>(5)?)))?.collect::<Result<Vec<_>,_>>()?;
        rows.into_iter()
            .map(
                |(id, conversation_id, label, state, created_at, node_count)| {
                    Ok(WorkflowSummary {
                        id,
                        conversation_id,
                        label,
                        state: serde_json::from_value(json!(state))?,
                        created_at,
                        node_count,
                    })
                },
            )
            .collect()
    }
    pub fn cancel_workflow(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        input: &WorkflowCancel,
        now: u64,
    ) -> Result<WorkflowReceipt, Error> {
        identifier(id, 128)?;
        identifier(&input.call_id, 512)?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let d = execution::authorize_work(&tx, cap, now)?;
        let mut value = read(&tx, id)?;
        owner(&tx, &d.session_id, &value)?;
        let digest = canonical::digest(&json!(["cancel_graph", id]))?;
        if let Some(mut old) = replay::<WorkflowReceipt>(&tx, cap, &input.call_id, &digest)? {
            old.replayed = true;
            return Ok(old);
        }
        cancel(&tx, &mut value, now)?;
        let result = WorkflowReceipt {
            workflow: value.view(),
            replayed: false,
        };
        receipt(&tx, cap, &input.call_id, &digest, &result)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn report_workflow_result(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        input: &WorkflowResultRequest,
        now: u64,
    ) -> Result<WorkflowResultReceipt, Error> {
        identifier(id, 128)?;
        identifier(&input.call_id, 512)?;
        text(&input.node_id, 255)?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let d = execution::authorize(&tx, cap, now, &["started"])?;
        let (mut value, node) = node_for_runner(&tx, id, &d)?;
        if node != input.node_id {
            return Err(Error::RunnerAuthority);
        }
        let bound = value.nodes.get(&node).ok_or(Error::State)?.clone();
        let task = execution::task(&tx, &bound.task_id)?;
        let observation = match &input.outcome {
            WorkflowOutcome::Complete { result } => {
                if task.status != TaskState::Done {
                    return Err(Error::State);
                }
                validate_result(result)?;
                NodeObservation::Complete {
                    result: result.clone(),
                }
            }
            WorkflowOutcome::Failed { error } => {
                if d.report_task.is_some() || task.status != TaskState::Blocked {
                    return Err(Error::RunnerAuthority);
                }
                text(error, 4000)?;
                NodeObservation::Failed {
                    error: error.clone(),
                }
            }
        };
        let digest = canonical::payload_digest(&json!([
            "graph_result",
            id,
            node,
            bound.task_id,
            task.execution_epoch,
            input.outcome
        ]))?;
        if let Some(mut old) = replay::<WorkflowResultReceipt>(&tx, cap, &input.call_id, &digest)? {
            old.replayed = true;
            return Ok(old);
        }
        let prior:Option<(String,String)>=tx.query_row("SELECT result_digest,result_receipt FROM graph_nodes WHERE graph_id=?1 AND node_id=?2 AND result_digest IS NOT NULL",params![id,node],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((old, body)) = prior {
            if old != digest {
                return Err(Error::Conflict);
            }
            let mut result: WorkflowResultReceipt = serde_json::from_str(&body)?;
            result.replayed = true;
            receipt(&tx, cap, &input.call_id, &digest, &result)?;
            tx.commit()?;
            return Ok(result);
        }
        value.graph = value.graph.observe(&node, &observation)?;
        if matches!(observation, NodeObservation::Complete { .. }) {
            value
                .nodes
                .get_mut(&node)
                .ok_or(Error::State)?
                .completed_epoch = Some(task.execution_epoch);
        }
        let mut result = WorkflowResultReceipt {
            graph_id: id.into(),
            node_id: node.clone(),
            task_id: bound.task_id.clone(),
            state: value.graph.progress[&node].status,
            graph_state: value.graph.status,
            execution_epoch: task.execution_epoch,
            replayed: false,
        };
        let stored_value = match &input.outcome {
            WorkflowOutcome::Complete { result } => Some(canonical::encode_payload(result)?),
            WorkflowOutcome::Failed { .. } => None,
        };
        tx.execute("UPDATE graph_nodes SET result_digest=?3,result_receipt=?4,completed_epoch=?5,result_value=?6 WHERE graph_id=?1 AND node_id=?2",params![id,node,digest,encoded(&result)?,value.nodes[&node].completed_epoch,stored_value])?;
        advance(&tx, &mut value, now)?;
        result.graph_state = value.graph.status;
        tx.execute(
            "UPDATE graph_nodes SET result_receipt=?3 WHERE graph_id=?1 AND node_id=?2",
            params![id, node, encoded(&result)?],
        )?;
        if matches!(observation, NodeObservation::Failed { .. }) {
            fence_tasks(&tx, id, Some(&node), now)?;
        }
        receipt(&tx, cap, &input.call_id, &digest, &result)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn workflow_dependencies(
        &self,
        cap: &RunnerCapability,
        id: &str,
        after: u64,
        limit: usize,
        now: u64,
    ) -> Result<Vec<DependencyRef>, Error> {
        clock(after)?;
        if !(1..=32).contains(&limit) {
            return Err(hagency_core::InvalidInput("dependency page must be 1..32").into());
        }
        let d = execution::authorize(&self.db, cap, now, &["started"])?;
        let (_, node) = node_for_runner(&self.db, id, &d)?;
        Ok(self.db.prepare("SELECT sequence,dependency_id,task_id,execution_epoch,digest FROM graph_dependencies WHERE graph_id=?1 AND node_id=?2 AND sequence>?3 ORDER BY sequence LIMIT ?4")?.query_map(params![id,node,after,limit],|r|Ok(DependencyRef{sequence:r.get(0)?,node_id:r.get(1)?,task_id:r.get(2)?,execution_epoch:r.get(3)?,digest:r.get(4)?}))?.collect::<Result<_,_>>()?)
    }
    pub fn workflow_dependency(
        &self,
        cap: &RunnerCapability,
        id: &str,
        dependency: &str,
        now: u64,
    ) -> Result<DependencyValue, Error> {
        text(dependency, 255)?;
        let d = execution::authorize(&self.db, cap, now, &["started"])?;
        let value = read(&self.db, id)?;
        current(&self.db, &value)?;
        let reference = if d.report_task.is_none() && d.session_id == value.creator_session_id {
            let index = value
                .graph
                .definition
                .nodes
                .iter()
                .position(|n| n.id == dependency)
                .ok_or(Error::NotFound)?;
            if value.graph.progress[dependency].status != NodeStatus::Complete {
                return Err(Error::State);
            }
            let (epoch, digest, _) = read_result(&self.db, id, dependency)?;
            DependencyRef {
                sequence: (index + 1) as u64,
                node_id: dependency.into(),
                task_id: value.nodes[dependency].task_id.clone(),
                execution_epoch: epoch,
                digest,
            }
        } else {
            let (_, node) = node_for_runner(&self.db, id, &d)?;
            self.db.query_row("SELECT sequence,dependency_id,task_id,execution_epoch,digest FROM graph_dependencies WHERE graph_id=?1 AND node_id=?2 AND dependency_id=?3",params![id,node,dependency],|r|Ok(DependencyRef{sequence:r.get(0)?,node_id:r.get(1)?,task_id:r.get(2)?,execution_epoch:r.get(3)?,digest:r.get(4)?})).optional()?.ok_or(Error::RunnerAuthority)?
        };
        let (epoch, digest, result) = read_result(&self.db, id, dependency)?;
        let bound = value.nodes.get(dependency).ok_or(Error::State)?;
        if reference.digest != digest
            || epoch != reference.execution_epoch
            || bound.completed_epoch != Some(epoch)
            || bound.task_id != reference.task_id
            || value.graph.progress[dependency].status != NodeStatus::Complete
        {
            return Err(Error::State);
        }
        Ok(DependencyValue {
            dependency: reference,
            result,
        })
    }
}
