//! Current host observations, copied input and canonical task activation share
//! the domain transaction. Nothing here is callable by a runner HTTP payload.
use super::{DomainRepository, bounded_row, execution, matrix_routes, serialize, task_intents};
use crate::Error;
use hagency_core::{
    canonical,
    ingress::*,
    messages::{InboundMessage, Message},
    project::identifier,
    replies::*,
    task_intents::*,
    tasks::*,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::json;

fn scoped(db: &Connection, scope: &MatrixIngressScope) -> Result<ReplyRoute, Error> {
    scope.validate()?;
    let route = matrix_routes::route(db, &scope.session_id)?;
    if !scope.matches(&route) {
        return Err(Error::RunnerAuthority);
    }
    let since: Option<u64> = db.query_row(
        "SELECT ingress_since FROM matrix_session_routes WHERE session_id=?1",
        [&scope.session_id],
        |r| r.get(0),
    )?;
    if since.is_none() {
        return Err(Error::RunnerAuthority);
    }
    Ok(route)
}
fn scope_digest(route: &ReplyRoute) -> Result<String, Error> {
    Ok(canonical::digest(&json!([
        route.engagement_id,
        route.fleet_id,
        route.project_id,
        route.registration_generation,
        route.server_name,
        route.room_id,
        route.room_generation,
        route.sender_mxid,
        route.device_id,
        route.transport_generation,
        route.owner_mxid,
        route.privacy,
        route.encrypted
    ]))?)
}
fn service_sender(db: &Connection, sender: &str) -> Result<bool, Error> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM matrix_transports WHERE sender_mxid=?1) OR EXISTS(SELECT 1 FROM registrations WHERE json_extract(config,'$.representativeMxid')=?1 OR json_extract(config,'$.approvalBotMxid')=?1)",[sender],|r|r.get(0))?)
}
fn record_message(
    tx: &Transaction<'_>,
    input: &InboundMessage,
    now: u64,
) -> Result<(Message, bool), Error> {
    let key = input.source_key()?;
    let digest = canonical::digest(&json!(input))?;
    let old: Option<(u64, String)> = tx
        .query_row(
            "SELECT sequence,digest FROM admitted_messages WHERE source_key=?1",
            [&key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (sequence, created) = if let Some((sequence, prior)) = old {
        if prior != digest {
            return Err(Error::Conflict);
        }
        (sequence, false)
    } else {
        bounded_row(tx, "admitted_messages", "source_key", &key, 100_000)?;
        tx.execute(
            "INSERT INTO admitted_messages(source_key,digest,config) VALUES(?1,?2,'{}')",
            params![key, digest],
        )?;
        (
            u64::try_from(tx.last_insert_rowid()).map_err(|_| Error::Capacity)?,
            true,
        )
    };
    let message = Message {
        sequence,
        source_key: key,
        server_name: input.server_name.clone(),
        room_id: input.room_id.clone(),
        event_id: input.event_id.clone(),
        sender_mxid: input.sender_mxid.clone(),
        thread_root: input.thread_root.clone(),
        body: input.body.clone(),
        kind: input.kind.clone(),
        origin_ts: input.origin_ts,
        received_at: now,
    };
    if created {
        tx.execute(
            "UPDATE admitted_messages SET config=?2 WHERE sequence=?1",
            params![sequence, serialize(&message)?],
        )?;
    }
    Ok((message, created))
}
pub(super) fn input_message(
    db: &Connection,
    session: &str,
    sequence: u64,
) -> Result<Message, Error> {
    let encoded:String=db.query_row("SELECT CASE WHEN s.matrix_generation>0 THEN i.config ELSE m.config END FROM session_inputs i JOIN admitted_messages m ON m.sequence=i.message_sequence JOIN runner_sessions s ON s.id=i.session_id WHERE i.session_id=?1 AND i.message_sequence=?2",params![session,sequence],|r|r.get(0)).optional()?.ok_or(Error::RunnerAuthority)?;
    Ok(serde_json::from_str(&encoded)?)
}
pub(super) fn task_message(db: &Connection, task: &str, sequence: u64) -> Result<Message, Error> {
    let encoded:String=db.query_row("SELECT CASE WHEN s.matrix_generation>0 THEN i.config ELSE m.config END FROM task_inputs i JOIN canonical_tasks t ON t.id=i.task_id JOIN runner_sessions s ON s.id=t.session_id JOIN admitted_messages m ON m.sequence=i.message_sequence WHERE i.task_id=?1 AND i.message_sequence=?2",params![task,sequence],|r|r.get(0)).optional()?.ok_or(Error::RunnerAuthority)?;
    Ok(serde_json::from_str(&encoded)?)
}
pub(super) fn provenance(
    db: &Connection,
    route: &ReplyRoute,
    message: &Message,
) -> Result<(), Error> {
    let exact:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM matrix_ingress_events WHERE engagement_id=?1 AND source_key=?2 AND scope_digest=?3 AND message_sequence=?4 AND config=?5 AND EXISTS(SELECT 1 FROM current_matrix_routes r WHERE r.session_id=matrix_ingress_events.source_session_id))",params![route.engagement_id,message.source_key,scope_digest(route)?,message.sequence,serialize(message)?],|r|r.get(0))?;
    if !exact {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
fn attach(tx: &Transaction<'_>, task: &str, message: &Message, wake: bool) -> Result<(), Error> {
    let prior: Option<(Option<String>, Option<bool>)> = tx
        .query_row(
            "SELECT config,wake FROM task_inputs WHERE task_id=?1 AND message_sequence=?2",
            params![task, message.sequence],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let encoded = serialize(message)?;
    if let Some((old, old_wake)) = prior {
        if old.as_deref() != Some(&encoded) || old_wake != Some(wake) {
            return Err(Error::Conflict);
        }
        return Ok(());
    }
    let count: u64 = tx.query_row(
        "SELECT COUNT(*) FROM task_inputs WHERE task_id=?1",
        [task],
        |r| r.get(0),
    )?;
    if count >= 10_000 {
        return Err(Error::Capacity);
    }
    tx.execute(
        "INSERT INTO task_inputs(task_id,message_sequence,config,wake) VALUES(?1,?2,?3,?4)",
        params![task, message.sequence, encoded, wake],
    )?;
    Ok(())
}
fn bound_intent(db: &Connection, session: &str) -> Result<Option<(String, String, u64)>, Error> {
    Ok(db
        .query_row(
            "SELECT task_id,state,root_sequence FROM task_intents WHERE session_id=?1",
            [session],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

impl DomainRepository {
    pub fn matrix_ingress_scope(&self, session: &str) -> Result<MatrixIngressScope, Error> {
        identifier(session, 128)?;
        let route = matrix_routes::route(&self.db, session)?;
        let scope = MatrixIngressScope::from(&route);
        scoped(&self.db, &scope)?;
        Ok(scope)
    }

    /// The current registration the provisioning ingress verifies against:
    /// `fleet_id`, `reception_room_id`, `representative_mxid` and
    /// `approval_bot_mxid` live only in `registrations.config`, never in the
    /// intake event or the host identity.
    pub fn provisioning_registration(
        &self,
        fleet_id: &str,
    ) -> Result<hagency_core::authority::Registration, Error> {
        identifier(fleet_id, 128)?;
        let encoded: String = self
            .db
            .query_row(
                "SELECT config FROM registrations WHERE fleet_id=?1",
                [fleet_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok(serde_json::from_str(&encoded)?)
    }

    /// The enrolled owner-DM room for a requesting owner, recorded by the
    /// approval collector at enrollment. An owner with no enrolled room is
    /// refused fail-closed before `admit`.
    pub fn provisioning_owner_room(
        &self,
        owner_mxid: &str,
        server_name: &str,
    ) -> Result<OwnerRoomFacts, Error> {
        matrix_user(owner_mxid, server_name)
            .map_err(|_| Error::Invalid(hagency_core::InvalidInput("invalid owner mxid")))?;
        let row: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT room_id,config FROM approval_rooms WHERE owner_mxid=?1 AND available=1 ORDER BY generation DESC LIMIT 1",
                [owner_mxid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (room_id, config) = row.ok_or(Error::Invalid(hagency_core::InvalidInput(
            "owner room not enrolled",
        )))?;
        let value: serde_json::Value = serde_json::from_str(&config)?;
        Ok(OwnerRoomFacts {
            room_id,
            joined: value
                .get("joined")
                .and_then(serde_json::Value::as_array)
                .ok_or(Error::Invalid(hagency_core::InvalidInput("invalid owner room")))?
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or(Error::Invalid(hagency_core::InvalidInput("invalid owner room")))?,
            invite_only: value
                .get("invite_only")
                .and_then(serde_json::Value::as_bool)
                .ok_or(Error::Invalid(hagency_core::InvalidInput("invalid owner room")))?,
            encrypted: value
                .get("encrypted")
                .and_then(serde_json::Value::as_bool)
                .ok_or(Error::Invalid(hagency_core::InvalidInput("invalid owner room")))?,
        })
    }
    /// Host-only snapshot for one current intake target. A returned route is
    /// copied into authenticated custody, then checked again by admission.
    pub fn matrix_intake_route(&self, session: &str) -> Result<ReplyRoute, Error> {
        let scope = self.matrix_ingress_scope(session)?;
        scoped(&self.db, &scope)
    }
    /// Historical receipt lookup never projects input or revives a retired route.
    /// It is solely the acknowledgement seam for an already-committed exact event.
    pub fn matrix_ingress_receipt(
        &self,
        input: &MatrixEventObservation,
    ) -> Result<Option<MatrixIngressReceipt>, Error> {
        input.validate()?;
        let encoded: Option<String> = self
            .db
            .query_row(
                "SELECT config FROM matrix_session_routes WHERE session_id=?1",
                [&input.scope.session_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(encoded) = encoded else {
            return Ok(None);
        };
        let route: ReplyRoute = serde_json::from_str(&encoded)?;
        if !input.scope.matches(&route) {
            return Err(Error::RunnerAuthority);
        }
        let source = input.event.source_key()?;
        let prior: Option<(String,String,String,String,u64)>=self.db.query_row(
            "SELECT scope_digest,digest,config,source_session_id,message_sequence FROM matrix_ingress_events WHERE engagement_id=?1 AND source_key=?2",
            params![route.engagement_id,source], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
        ).optional()?;
        // Live miss → the archive answers by the same identity (ADR-125 P8':
        // provenance moves with the message, so the receipt lookup outlives
        // the live row). `wake` is reconstructed from the archive row because
        // the `session_inputs` child was pruned with the message.
        let prior = match prior {
            Some(row) => Some(row),
            None => self
                .db
                .query_row(
                    "SELECT scope_digest,digest,config,source_session_id,sequence FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2",
                    params![route.engagement_id,source],
                    |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
                )
                .optional()?,
        };
        let Some((scope, digest, config, session, sequence)) = prior else {
            return Ok(None);
        };
        if session != route.session_id || scope != scope_digest(&route)? {
            return Err(Error::RunnerAuthority);
        }
        if digest != canonical::digest(&json!([input.event, input.mentions, input.encrypted]))? {
            return Err(Error::Conflict);
        }
        let (wake, stored): (bool, String) = match self
            .db
            .query_row(
                "SELECT wake,config FROM session_inputs WHERE session_id=?1 AND message_sequence=?2",
                params![session, sequence],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
        {
            // A pruned message's `session_inputs` child was deleted with it;
            // the archive row carries both halves (A4), so the guard keeps
            // its meaning against the archived config.
            Some(row) => row,
            None => self.db.query_row(
                "SELECT wake,config FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2",
                params![route.engagement_id, source],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?,
        };
        if stored != config {
            return Err(Error::Conflict);
        }
        Ok(Some(MatrixIngressReceipt {
            sequence,
            session_id: session,
            wake,
            created: false,
            projected: false,
        }))
    }
    pub fn admit_matrix_event(
        &mut self,
        input: &MatrixEventObservation,
        now: u64,
    ) -> Result<MatrixIngressReceipt, Error> {
        self.admit_matrix_input(input, None, now)
    }
    pub(super) fn admit_matrix_input(
        &mut self,
        input: &MatrixEventObservation,
        attachment: Option<&hagency_core::attachments::MatrixAttachmentObservation>,
        now: u64,
    ) -> Result<MatrixIngressReceipt, Error> {
        input.validate()?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        matrix_routes::reconcile(&tx, now)?;
        let route = scoped(&tx, &input.scope)?;
        let event = &input.event;
        matrix_user(&event.sender_mxid, &route.server_name)?;
        matrix_room(&event.room_id, &route.server_name)?;
        if event.server_name != route.server_name
            || event.room_id != route.room_id
            || (event.thread_root != route.thread_root
                && !(event.thread_root.is_none()
                    && route.thread_root.as_deref() == Some(&event.event_id)))
            || (route.encrypted && !input.encrypted)
        {
            return Err(Error::RunnerAuthority);
        }
        let joined:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM matrix_room_scopes r,json_each(r.joined) m WHERE r.server_name=?1 AND r.room_id=?2 AND m.value=?3)",params![route.server_name,route.room_id,event.sender_mxid],|r|r.get(0))?;
        let since: u64 = tx.query_row(
            "SELECT ingress_since FROM matrix_session_routes WHERE session_id=?1",
            [&route.session_id],
            |r| r.get(0),
        )?;
        if !joined || event.origin_ts < since || event.origin_ts > now {
            return Err(Error::RunnerAuthority);
        }
        let source = event.source_key()?;
        let scope_hash = scope_digest(&route)?;
        let digest = canonical::digest(&json!([event, input.mentions, input.encrypted]))?;
        // Matrix source content is global; per-Agent scope receipts are separate.
        let different: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM matrix_ingress_events WHERE source_key=?1 AND digest<>?2)",
            params![source, digest],
            |r| r.get(0),
        )?;
        // Read 3's archive half (A3): the live read carries no engagement
        // filter because it runs inside the engagement transaction; the
        // archive is global, so its divergence probe is scoped to the pair —
        // a stale archive row from another engagement must not shadow this
        // one.
        let different = different
            || tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2 AND digest<>?3)",
                params![route.engagement_id, source, digest],
                |r| r.get(0),
            )?;
        if different {
            return Err(Error::Conflict);
        }
        let prior:Option<(String,String,String,String)>=tx.query_row("SELECT scope_digest,digest,config,source_session_id FROM matrix_ingress_events WHERE engagement_id=?1 AND source_key=?2",params![route.engagement_id,source],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        // Read 4's archive half (P8'): the provenance row moved with the
        // message, so an exact redelivery of a pruned admission is answered
        // from the archive and a divergent one is refused above.
        let archived = prior.is_none()
            && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2)",
                params![route.engagement_id, source],
                |r| r.get(0),
            )?;
        let prior = match prior {
            Some(row) => Some(row),
            None => tx
                .query_row(
                    "SELECT scope_digest,digest,config,source_session_id FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2",
                    params![route.engagement_id, source],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?,
        };
        let had_receipt = prior.is_some();
        let (message, created) = if let Some((scope, old, encoded, original_session)) = prior {
            if original_session != route.session_id {
                return Err(Error::RunnerAuthority);
            }
            if scope != scope_hash {
                return Err(Error::RunnerAuthority);
            }
            if old != digest {
                return Err(Error::Conflict);
            }
            (serde_json::from_str::<Message>(&encoded)?, false)
        } else {
            let total: u64 =
                tx.query_row("SELECT COUNT(*) FROM matrix_ingress_events", [], |r| {
                    r.get(0)
                })?;
            if total >= 100_000 {
                return Err(Error::Capacity);
            }
            let (message, created) = record_message(&tx, event, now)?;
            tx.execute("INSERT INTO matrix_ingress_events(engagement_id,source_key,message_sequence,scope_digest,digest,config,source_session_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![route.engagement_id,source,message.sequence,scope_hash,digest,serialize(&message)?,route.session_id])?;
            (message, created)
        };
        super::attachments::record(&tx, &route, &message, attachment, had_receipt)?;
        let prior:Option<(bool,String)>=tx.query_row("SELECT wake,config FROM session_inputs WHERE session_id=?1 AND message_sequence=?2",params![route.session_id,message.sequence],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        // A4's admission-path duty: an exact redelivery of a PRUNED admission
        // is already durably recorded in the archive. The `session_inputs`
        // child was deleted with the message, so re-inserting it would hit
        // the RESTRICT FK — return the archived receipt instead, exactly the
        // shape the `session_inputs`-hit early return below produces.
        if prior.is_none() && archived {
            let wake: bool = tx.query_row(
                "SELECT wake FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2",
                params![route.engagement_id, source],
                |r| r.get(0),
            )?;
            let result = MatrixIngressReceipt {
                sequence: message.sequence,
                session_id: route.session_id.clone(),
                wake,
                created: false,
                projected: false,
            };
            tx.commit()?;
            return Ok(result);
        }
        if let Some((wake, encoded)) = prior {
            if encoded != serialize(&message)? {
                return Err(Error::Conflict);
            }
            let result = MatrixIngressReceipt {
                sequence: message.sequence,
                session_id: route.session_id,
                wake,
                created: false,
                projected: false,
            };
            tx.commit()?;
            return Ok(result);
        }
        let human = !service_sender(&tx, &event.sender_mxid)?;
        let kind = matches!(
            event.kind.as_str(),
            "m.text" | "m.file" | "m.image" | "m.audio" | "m.video"
        );
        let mut wake = human
            && kind
            && match &route.privacy {
                RoomPrivacy::Direct { human_mxid } => &event.sender_mxid == human_mxid,
                RoomPrivacy::Group {} => input.mentions.contains(&route.sender_mxid),
            };
        let task = bound_intent(&tx, &route.session_id)?;
        if let Some((id, state, root)) = &task {
            if state == "closed" {
                return Err(Error::RunnerAuthority);
            }
            let t = execution::task(&tx, id)?;
            if t.status == TaskState::Done {
                let root = task_message(&tx, id, *root)?;
                wake &= event.sender_mxid == root.sender_mxid
                    && t.completed_at
                        .is_some_and(|done| event.origin_ts > done && now > done);
            }
        }
        let pending: u64 = tx.query_row(
            "SELECT COUNT(*) FROM session_inputs WHERE session_id=?1 AND processed_at IS NULL",
            [&route.session_id],
            |r| r.get(0),
        )?;
        if pending >= 2000 {
            return Err(Error::Capacity);
        }
        tx.execute("INSERT INTO session_inputs(session_id,message_sequence,wake,config) VALUES(?1,?2,?3,?4)",params![route.session_id,message.sequence,wake,serialize(&message)?])?;
        if let Some((id, _, _)) = task {
            attach(&tx, &id, &message, wake)?;
        }
        super::attachments::project_one(&tx, &route, &message)?;
        let result = MatrixIngressReceipt {
            sequence: message.sequence,
            session_id: route.session_id,
            wake,
            created,
            projected: true,
        };
        tx.commit()?;
        Ok(result)
    }

    pub fn create_verified_task_intent(
        &mut self,
        input: &VerifiedTaskRequest,
        now: u64,
    ) -> Result<IntentResult, Error> {
        input.validate()?;
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        matrix_routes::reconcile(&tx, now)?;
        let source_route = scoped(&tx, &input.scope)?;
        let source = input_message(&tx, &source_route.session_id, input.source_sequence)?;
        provenance(&tx, &source_route, &source)?;
        let wake: bool = tx.query_row(
            "SELECT wake FROM session_inputs WHERE session_id=?1 AND message_sequence=?2",
            params![source_route.session_id, source.sequence],
            |r| r.get(0),
        )?;
        if !wake {
            return Err(Error::State);
        }
        let digest = canonical::digest(&json!([input, source_route, source]))?;
        let prior:Option<(String,String)>=tx.query_row("SELECT task_id,digest FROM verified_task_requests WHERE source_session_id=?1 AND request_key=?2",params![source_route.session_id,input.request_key],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((id, old)) = prior {
            if old != digest {
                return Err(Error::Conflict);
            }
            let result = task_intents::intent_result(&tx, &id, true)?;
            matrix_routes::check(&tx, &result.session_id)?;
            return Ok(result);
        }
        let (root, root_session) = if let Some(root) = &source.thread_root {
            let key = canonical::digest(&json!([source.server_name, source.room_id, root]))?;
            let found: Option<(String,String)>=tx.query_row("SELECT config,source_session_id FROM matrix_ingress_events WHERE engagement_id=?1 AND source_key=?2 AND scope_digest=?3 AND EXISTS(SELECT 1 FROM current_matrix_routes r WHERE r.session_id=matrix_ingress_events.source_session_id)",params![source_route.engagement_id,key,scope_digest(&source_route)?],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            // Read 6 (A2): the root may have been pruned while its session
            // route survives; the archive answers the same lookup by the
            // same identity.
            let (encoded, root_session, archived): (String, String, Option<(u64, String, String)>) =
                match found {
                    Some(row) => (row.0, row.1, None),
                    None => {
                        let row: (String, String, u64, String, String) = tx.query_row(
                        "SELECT config,source_session_id,sequence,source_key,digest FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2 AND scope_digest=?3",
                        params![source_route.engagement_id,key,scope_digest(&source_route)?],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                    ).optional()?.ok_or(Error::RunnerAuthority)?;
                        (row.0, row.1, Some((row.2, row.3, row.4)))
                    }
                };
            let root: Message = serde_json::from_str(&encoded)?;
            if root.thread_root.is_some() || root.event_id != *source.thread_root.as_ref().unwrap()
            {
                return Err(Error::RunnerAuthority);
            }
            // ADR-125 ("Provenance moves with the message") answers read 6
            // live first and from the archive on a live miss — but this
            // caller binds the resolved root as `task_intents.root_sequence`
            // and a `task_inputs.message_sequence`, both RESTRICT children
            // of `admitted_messages`, so the archived answer must be
            // materialised: the root is re-admitted live (its own pruned
            // sequence, provenance pair copied verbatim from the archive
            // row) in THIS transaction and the archive row is dropped, and
            // the intent then pins the live row. Unlike the sibling reads,
            // which reconstruct `wake`/`config` and skip the re-insert,
            // this restores ONLY `admitted_messages` and its provenance
            // row — no `session_inputs` child and no live `wake`, neither
            // of which read 6 needs. The guard that can actually abort is
            // `admitted_messages.source_key`'s global UNIQUE (migration
            // 004): a live row holding this key while its provenance row
            // is absent or its route no longer current reaches this branch
            // and refuses loudly; the sequence itself cannot collide — it
            // is the pruned row's own AUTOINCREMENT key, never reused.
            if let Some((sequence, source_key, digest)) = archived {
                tx.execute(
                    "INSERT INTO admitted_messages(sequence,source_key,digest,config) VALUES(?1,?2,?3,?4)",
                    params![sequence, source_key, digest, encoded],
                )?;
                tx.execute(
                    "INSERT INTO matrix_ingress_events(engagement_id,source_key,message_sequence,source_session_id,scope_digest,digest,config) \
                     SELECT engagement_id,source_key,sequence,source_session_id,scope_digest,digest,config FROM retained_message_archive WHERE sequence=?1",
                    [sequence],
                )?;
                tx.execute(
                    "DELETE FROM retained_message_archive WHERE sequence=?1",
                    [sequence],
                )?;
            }
            (root, root_session)
        } else {
            (source.clone(), source_route.session_id.clone())
        };
        if service_sender(&tx, &root.sender_mxid)? {
            return Err(Error::RunnerAuthority);
        }
        let desired_root = if matches!(source_route.privacy, RoomPrivacy::Direct { .. })
            && source_route.thread_root.is_none()
        {
            None
        } else {
            Some(root.event_id.clone())
        };
        let session = if desired_root == source_route.thread_root {
            source_route.session_id.clone()
        } else {
            let binding = SessionBinding {
                id: format!(
                    "session_{}",
                    &canonical::digest(&json!([
                        "verified_task",
                        source_route.session_id,
                        source_route.session_generation,
                        root.event_id
                    ]))?[..32]
                ),
                engagement_id: source_route.engagement_id.clone(),
                room_id: source_route.room_id.clone(),
                thread_root: desired_root,
            };
            let binding = matrix_routes::resolve(&tx, &binding, now)?;
            binding.id
        };
        if root_session != session {
            let parent: Option<String> = tx.query_row(
                "SELECT parent_session_id FROM matrix_session_routes WHERE session_id=?1",
                [&session],
                |r| r.get(0),
            )?;
            if parent
                .as_deref()
                .is_some_and(|parent| parent != root_session)
            {
                return Err(Error::RunnerAuthority);
            }
            // The root's original projection owns the visibility boundary, including
            // when a host resolved the explicit thread before creating its task.
            tx.execute("UPDATE matrix_session_routes SET parent_session_id=?2 WHERE session_id=?1 AND parent_session_id IS NULL",params![session,root_session])?;
        }
        matrix_routes::check(&tx, &session)?;
        if let Some(parent) = &input.definition.parent_id {
            let parent = execution::task(&tx, parent)?;
            if parent.session_id != source_route.session_id {
                return Err(Error::RunnerAuthority);
            }
        }
        let result = if let Some((id, state, _)) = bound_intent(&tx, &session)? {
            if state == "closed" {
                return Err(Error::RunnerAuthority);
            }
            attach(&tx, &id, &source, wake)?;
            if state == "active" {
                task_intents::project_inputs(&tx, &id)?;
            }
            task_intents::intent_result(&tx, &id, true)?
        } else {
            let ids: Vec<u64> = std::collections::BTreeSet::from([root.sequence, source.sequence])
                .into_iter()
                .collect();
            let intent = TaskIntent {
                request_scope: format!("verified_{}", source_route.session_id),
                request_key: input.request_key.clone(),
                assignee_engagement: source_route.engagement_id.clone(),
                root_sequence: root.sequence,
                input_sequences: ids.clone(),
                definition: input.definition.clone(),
            };
            let result = task_intents::persist_intent(
                &tx,
                &intent,
                Some(&source_route.session_id),
                now,
                task_intents::IntentProjection {
                    session_id: &session,
                    root: &root,
                    sequences: &ids,
                },
                &digest,
            )?;
            for message in [&root, &source] {
                let projected_wake = if message.sequence == source.sequence {
                    wake
                } else {
                    false
                };
                tx.execute("UPDATE task_inputs SET config=?3,wake=?4 WHERE task_id=?1 AND message_sequence=?2",params![result.task_id,message.sequence,serialize(message)?,projected_wake])?;
            }
            result
        };
        let count: u64 = tx.query_row("SELECT COUNT(*) FROM verified_task_requests", [], |r| {
            r.get(0)
        })?;
        if count >= 100_000 {
            return Err(Error::Capacity);
        }
        tx.execute("INSERT INTO verified_task_requests(source_session_id,request_key,digest,task_id) VALUES(?1,?2,?3,?4)",params![source_route.session_id,input.request_key,digest,result.task_id])?;
        tx.commit()?;
        Ok(result)
    }
}
