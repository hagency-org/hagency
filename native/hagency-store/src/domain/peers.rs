//! Peer input custody shares dispatch transactions and never asserts Matrix origin.
use super::{DomainRepository, bounded_row, conversations, execution};
use crate::Error;
use hagency_core::{canonical, peers::*, tasks::*};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::json;

fn read(db: &Connection, sequence: u64) -> Result<PeerMessage, Error> {
    let (key, conversation, config): (String, String, String) = db
        .query_row(
            "SELECT source_key,conversation_id,config FROM peer_messages WHERE sequence=?1",
            [sequence],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    let value: PeerMessage = serde_json::from_str(&config)?;
    if value.sequence != sequence || value.id != key || value.conversation_id != conversation {
        return Err(Error::State);
    }
    Ok(value)
}
fn items(db: &Connection, dispatch: &str) -> Result<Vec<PeerInboxItem>, Error> {
    let rows:Vec<(u64,bool)>=db.prepare("SELECT p.message_sequence,i.wake FROM peer_dispatch_inputs p JOIN runner_dispatches d ON d.id=p.dispatch_id JOIN peer_session_inputs i ON i.session_id=d.session_id AND i.message_sequence=p.message_sequence WHERE p.dispatch_id=?1 ORDER BY p.message_sequence")?.query_map([dispatch],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<Result<_,_>>()?;
    rows.into_iter()
        .map(|(seq, wake)| {
            Ok(PeerInboxItem {
                message: read(db, seq)?,
                wake,
            })
        })
        .collect()
}
fn current_recipient(
    db: &Connection,
    message: &PeerMessage,
    session_id: &str,
) -> Result<(), Error> {
    // The conversation's actor check belongs to send. Delivery independently
    // checks the exact recorded destination against current membership/state.
    execution::admission_session(db, session_id)?;
    let allowed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM internal_conversations c JOIN runner_sessions s ON s.id=?2 JOIN engagements e ON e.id=s.engagement_id WHERE c.id=?1 AND c.state='active' AND c.fleet_id=e.fleet_id AND c.project_id=e.project_id AND c.generation=e.generation AND (c.creator_session_id=s.id OR EXISTS(SELECT 1 FROM internal_participants p WHERE p.conversation_id=c.id AND p.session_id=s.id)))",params![message.conversation_id,session_id],|r|r.get(0))?;
    if !allowed
        || !message
            .recipient_session_ids
            .iter()
            .any(|id| id == session_id)
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
pub(super) fn validate_dispatch(db: &Connection, id: &str, session: &str) -> Result<(), Error> {
    for item in items(db, id)? {
        current_recipient(db, &item.message, session)?;
    }
    Ok(())
}
pub(super) fn complete_inputs(tx: &Transaction<'_>, id: &str, now: u64) -> Result<(), Error> {
    tx.execute("UPDATE peer_session_inputs SET processed_at=?2 WHERE dispatch_id=?1 AND session_id=(SELECT session_id FROM runner_dispatches WHERE id=?1) AND message_sequence IN (SELECT message_sequence FROM peer_dispatch_inputs WHERE dispatch_id=?1) AND processed_at IS NULL",params![id,now])?;
    Ok(())
}
pub(super) fn recovery_input(
    tx: &Transaction<'_>,
    old: &str,
    replacement: &DispatchInput,
) -> Result<DispatchInput, Error> {
    let mut value = replacement.clone();
    let old_input = items(tx, old)?;
    if !old_input.is_empty() {
        let body = value
            .payload
            .as_object_mut()
            .ok_or(hagency_core::InvalidInput(
                "recovery payload must be an object",
            ))?;
        if body.contains_key("peerInbox") || body.contains_key("recoveryPeerInbox") {
            return Err(hagency_core::InvalidInput("peer recovery input is host-owned").into());
        }
        body.insert("recoveryPeerInbox".into(), serde_json::to_value(old_input)?);
    }
    Ok(value)
}
pub(super) fn transfer_inputs(tx: &Transaction<'_>, old: &str, next: &str) -> Result<(), Error> {
    tx.execute("INSERT INTO peer_dispatch_inputs(dispatch_id,message_sequence) SELECT ?2,message_sequence FROM peer_dispatch_inputs WHERE dispatch_id=?1",params![old,next])?;
    tx.execute("UPDATE peer_session_inputs SET dispatch_id=?2 WHERE dispatch_id=?1 AND processed_at IS NULL",params![old,next])?;
    tx.execute("UPDATE peer_session_inputs SET dispatch_id=NULL WHERE processed_at IS NULL AND dispatch_id IN (SELECT id FROM runner_dispatches WHERE state='superseded' AND session_id=(SELECT session_id FROM runner_dispatches WHERE id=?1))",[old])?;
    Ok(())
}
pub(super) struct PeerOrigin<'a> {
    pub session_id: &'a str,
    pub dispatch_id: &'a str,
    pub task_id: Option<&'a str>,
}
/// A count snapshot belongs to one writer transaction and admission batch.
/// Every newly admitted recipient increments it; replay consumes no new slot.
pub(super) struct PeerCapacity<'tx, 'db> {
    tx: &'tx Transaction<'db>,
    pending: std::collections::BTreeMap<String, u64>,
}
impl<'tx, 'db> PeerCapacity<'tx, 'db> {
    pub(super) fn new(tx: &'tx Transaction<'db>) -> Self {
        Self {
            tx,
            pending: Default::default(),
        }
    }
    fn reserve(&mut self, target: &str) -> Result<(), Error> {
        let pending = match self.pending.get_mut(target) {
            Some(pending) => pending,
            None => {
                // Retired input is immutable history, not pending work. Frozen
                // input counts until its unresolved process is inspected.
                let count: u64 = self.tx.query_row(
                    "SELECT COUNT(*) FROM peer_session_inputs i WHERE i.session_id=?1 AND i.processed_at IS NULL AND (EXISTS(SELECT 1 FROM live_peer_inputs live WHERE live.session_id=i.session_id AND live.message_sequence=i.message_sequence) OR EXISTS(SELECT 1 FROM runner_dispatches d WHERE d.id=i.dispatch_id AND (d.state IN ('queued','leased','started','parked') OR EXISTS(SELECT 1 FROM unresolved_dispatches u WHERE u.id=d.id))))",
                    [target], |r| r.get(0),
                )?;
                self.pending.entry(target.into()).or_insert(count)
            }
        };
        if *pending >= 2000 {
            return Err(Error::Capacity);
        }
        *pending += 1;
        Ok(())
    }
}
// Only current runtime admission or an authorized finite stored workflow calls
// this transactional helper. No runtime JSON can supply PeerOrigin or the key.
pub(super) fn admit(
    capacity: &mut PeerCapacity<'_, '_>,
    origin: PeerOrigin<'_>,
    input: &PeerSend,
    key: &str,
    now: u64,
) -> Result<PeerReceipt, Error> {
    let tx = capacity.tx;
    input.validate()?;
    clock(now)?;
    let id = key.to_owned();
    let group = conversations::admission_scope(tx, origin.session_id, &input.conversation_id)?;
    let recipients: Vec<_> = input
        .recipient_session_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    for target in &recipients {
        conversations::recipient(tx, &group, target)?;
    }
    let source = execution::admission_session(tx, origin.session_id)?;
    let digest = canonical::payload_digest(&json!([
        id,
        input.conversation_id,
        recipients,
        input.kind,
        input.priority,
        input.summary,
        input.body,
        input.data,
        origin.session_id.to_owned(),
        source.engagement_id(),
        origin.task_id.map(str::to_owned)
    ]))?;
    let previous: Option<(u64, String)> = tx
        .query_row(
            "SELECT sequence,digest FROM peer_messages WHERE source_key=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((sequence, old)) = previous {
        return if old == digest {
            Ok(PeerReceipt {
                id,
                sequence,
                replayed: true,
                recipients: recipients.len(),
            })
        } else {
            Err(Error::Conflict)
        };
    }
    bounded_row(tx, "peer_messages", "source_key", &id, 100_000)?;
    tx.execute(
        "INSERT INTO peer_messages(source_key,digest,conversation_id,config) VALUES(?1,?2,?3,'{}')",
        params![id, digest, input.conversation_id],
    )?;
    let sequence = u64::try_from(tx.last_insert_rowid()).map_err(|_| Error::Capacity)?;
    clock(sequence)?;
    let message = PeerMessage {
        sequence,
        id: id.clone(),
        conversation_id: group.id,
        source_session_id: origin.session_id.to_owned(),
        source_engagement_id: source.engagement_id().into(),
        source_dispatch_id: origin.dispatch_id.to_owned(),
        source_task_id: origin.task_id.map(str::to_owned),
        recipient_session_ids: recipients.clone(),
        kind: input.kind,
        priority: input.priority,
        summary: input.summary.clone(),
        body: input.body.clone(),
        data: input.data.clone(),
        received_at: now,
    };
    tx.execute(
        "UPDATE peer_messages SET config=?2 WHERE sequence=?1",
        params![
            sequence,
            canonical::encode_payload(&serde_json::to_value(&message)?)?
        ],
    )?;
    for target in &recipients {
        capacity.reserve(target)?;
        tx.execute(
            "INSERT INTO peer_session_inputs(session_id,message_sequence,wake) VALUES(?1,?2,?3)",
            params![target, sequence, input.kind.wakes()],
        )?;
    }
    Ok(PeerReceipt {
        id,
        sequence,
        replayed: false,
        recipients: recipients.len(),
    })
}
impl DomainRepository {
    pub fn send_peer(
        &mut self,
        cap: &RunnerCapability,
        input: &PeerSend,
        now: u64,
    ) -> Result<PeerReceipt, Error> {
        input.validate()?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let d = execution::authorize_work(&tx, cap, now)?;
        if let Some(id) = &d.task_id
            && execution::task(&tx, id)?.status == TaskState::Done
        {
            return Err(Error::State);
        }
        conversations::scoped(&tx, &d.session_id, &input.conversation_id)?;
        let key = format!(
            "peer_{}",
            canonical::digest(&json!([cap.dispatch_id, input.call_id]))?
        );
        let result = admit(
            &mut PeerCapacity::new(&tx),
            PeerOrigin {
                session_id: &d.session_id,
                dispatch_id: &cap.dispatch_id,
                task_id: d.task_id.as_deref(),
            },
            input,
            &key,
            now,
        )?;
        tx.commit()?;
        Ok(result)
    }
    pub fn peer_inbox(
        &self,
        session: &str,
        after: u64,
        limit: usize,
    ) -> Result<Vec<PeerInboxItem>, Error> {
        execution::admission_session(&self.db, session)?;
        clock(after)?;
        if !(1..=100).contains(&limit) {
            return Err(hagency_core::InvalidInput("peer page must be 1..100").into());
        }
        let rows:Vec<(u64,bool)>=self.db.prepare("SELECT i.message_sequence,i.wake FROM peer_session_inputs i JOIN live_peer_inputs l ON l.session_id=i.session_id AND l.message_sequence=i.message_sequence WHERE i.session_id=?1 AND i.message_sequence>?2 AND i.processed_at IS NULL AND i.dispatch_id IS NULL ORDER BY i.message_sequence LIMIT ?3")?.query_map(params![session,after,limit],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<Result<_,_>>()?;
        rows.into_iter()
            .map(|(seq, wake)| {
                Ok(PeerInboxItem {
                    message: read(&self.db, seq)?,
                    wake,
                })
            })
            .collect()
    }
    pub fn enqueue_peer_dispatch(
        &mut self,
        input: &DispatchInput,
        sequences: &[u64],
    ) -> Result<(), Error> {
        input.validate()?;
        if ["inbox", "recoveryInbox", "peerInbox", "recoveryPeerInbox"]
            .iter()
            .any(|k| input.payload.get(k).is_some())
        {
            return Err(hagency_core::InvalidInput("dispatch input is host-owned").into());
        }
        if sequences.is_empty() || sequences.len() > 100 {
            return Err(hagency_core::InvalidInput("peer dispatch requires 1..100 inputs").into());
        }
        let ordered: std::collections::BTreeSet<_> = sequences.iter().copied().collect();
        if ordered.len() != sequences.len() {
            return Err(hagency_core::InvalidInput("duplicate peer input").into());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        execution::session(&tx, &input.session_id)?;
        let mut selected = Vec::new();
        for sequence in ordered {
            clock(sequence)?;
            let (wake,assigned,processed):(bool,Option<String>,Option<u64>)=tx.query_row("SELECT wake,dispatch_id,processed_at FROM peer_session_inputs WHERE session_id=?1 AND message_sequence=?2",params![input.session_id,sequence],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(Error::RunnerAuthority)?;
            if (assigned.is_some() && assigned.as_deref() != Some(&input.id))
                || (processed.is_some() && assigned.as_deref() != Some(&input.id))
            {
                return Err(Error::State);
            }
            let message = read(&tx, sequence)?;
            current_recipient(&tx, &message, &input.session_id)?;
            selected.push(PeerInboxItem { message, wake });
        }
        if !selected.iter().any(|item| item.wake) {
            return Err(Error::State);
        }
        let mut frozen = input.clone();
        frozen
            .payload
            .as_object_mut()
            .ok_or(hagency_core::InvalidInput(
                "dispatch payload must be object",
            ))?
            .insert("peerInbox".into(), serde_json::to_value(&selected)?);
        execution::enqueue_peers(&tx, &frozen, sequences)?;
        for item in selected {
            tx.execute("INSERT OR IGNORE INTO peer_dispatch_inputs(dispatch_id,message_sequence) VALUES(?1,?2)",params![input.id,item.message.sequence])?;
            tx.execute("UPDATE peer_session_inputs SET dispatch_id=?3 WHERE session_id=?1 AND message_sequence=?2",params![input.session_id,item.message.sequence,input.id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn runner_peer_inbox(
        &self,
        cap: &RunnerCapability,
        after: u64,
        limit: usize,
        now: u64,
    ) -> Result<Vec<PeerInboxItem>, Error> {
        let d = execution::authorize(&self.db, cap, now, &["started"])?;
        validate_dispatch(&self.db, &cap.dispatch_id, &d.session_id)?;
        clock(after)?;
        if !(1..=100).contains(&limit) {
            return Err(hagency_core::InvalidInput("peer page must be 1..100").into());
        }
        Ok(items(&self.db, &cap.dispatch_id)?
            .into_iter()
            .filter(|item| item.message.sequence > after)
            .take(limit)
            .collect())
    }
}
