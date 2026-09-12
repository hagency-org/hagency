//! Host adapter observations are distinct from runtime routing requests.
use super::{DomainRepository, bounded_row, conversation_lifecycle, execution, serialize};
use crate::Error;
use hagency_core::{
    authority::Registration,
    project::identifier,
    replies::*,
    tasks::{SessionBinding, clock, text},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::VecDeque;

struct Context {
    registration: Registration,
    project: String,
    project_room: String,
    owner: String,
    approval_room: String,
}
fn context(db: &Connection, engagement: &str) -> Result<Context, Error> {
    identifier(engagement, 128)?;
    let row:Option<(String,String,String,String,String)>=db.query_row("SELECT r.config,e.project_id,p.room_id,p.owner_mxid,p.owner_room_id FROM engagements e JOIN registrations r ON r.fleet_id=e.fleet_id AND r.generation=e.generation JOIN projects p ON p.fleet_id=e.fleet_id AND p.id=e.project_id AND p.generation=e.generation WHERE e.id=?1 AND e.state='active'",[engagement],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let (registration, project, project_room, owner, approval_room) =
        row.ok_or(Error::RunnerAuthority)?;
    Ok(Context {
        registration: serde_json::from_str(&registration)?,
        project,
        project_room,
        owner,
        approval_room,
    })
}
fn user(id: &str, server: &str) -> Result<(), Error> {
    matrix_user(id, server).map_err(|_| Error::RunnerAuthority)?;
    Ok(())
}
pub(super) fn check(db: &Connection, session: &str) -> Result<(), Error> {
    let valid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM current_matrix_routes WHERE session_id=?1)",
        [session],
        |r| r.get(0),
    )?;
    if !valid {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
pub(super) fn route(db: &Connection, session: &str) -> Result<ReplyRoute, Error> {
    check(db, session)?;
    let encoded: String = db.query_row(
        "SELECT config FROM matrix_session_routes WHERE session_id=?1",
        [session],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&encoded)?)
}
pub(super) fn reconcile(tx: &Transaction<'_>, now: u64) -> Result<(), Error> {
    let retired:Vec<String>=tx.prepare("SELECT session_id FROM matrix_session_routes r WHERE retired=0 AND NOT EXISTS(SELECT 1 FROM current_matrix_routes c WHERE c.session_id=r.session_id)")?.query_map([],|r|r.get(0))?.collect::<Result<_,_>>()?;
    for id in &retired {
        tx.execute(
            "UPDATE matrix_session_routes SET retired=1 WHERE session_id=?1",
            [id],
        )?;
    }
    if !retired.is_empty() {
        conversation_lifecycle::retire(tx, VecDeque::from(retired), "Matrix route retired", now)?;
    }
    // A send that might have crossed the external boundary remains uncertain.
    tx.execute("UPDATE final_replies SET state=CASE WHEN state='sending' THEN 'uncertain' ELSE 'cancelled' END,claim_hash=NULL,claim_until=NULL,updated_at=?1 WHERE state IN ('pending','claimed','sending') AND NOT EXISTS(SELECT 1 FROM current_final_replies c WHERE c.id=final_replies.id)",[now])?;
    super::notice_custody::retire(tx)?;
    Ok(())
}
fn membership(
    tx: &Transaction<'_>,
    input: &MatrixRoomObservation,
    server: &str,
    transport_generation: u64,
) -> Result<(), Error> {
    let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM matrix_room_memberships WHERE engagement_id=?1 AND server_name=?2 AND room_id=?3)",params![input.engagement_id,server,input.room_id],|r|r.get(0))?;
    if !exists {
        let count: u64 = tx.query_row("SELECT COUNT(*) FROM matrix_room_memberships", [], |r| {
            r.get(0)
        })?;
        let own: u64 = tx.query_row(
            "SELECT COUNT(*) FROM matrix_room_memberships WHERE engagement_id=?1",
            [&input.engagement_id],
            |r| r.get(0),
        )?;
        if count >= 100_000 || own >= 1000 {
            return Err(Error::Capacity);
        }
    }
    tx.execute("INSERT INTO matrix_room_memberships(engagement_id,server_name,room_id,room_generation,transport_generation) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(engagement_id,server_name,room_id) DO UPDATE SET room_generation=excluded.room_generation,transport_generation=excluded.transport_generation",params![input.engagement_id,server,input.room_id,input.generation,transport_generation])?;
    Ok(())
}

fn observed_transport(
    db: &Connection,
    c: &Context,
    engagement: &str,
    registration: u64,
    transport: u64,
) -> Result<String, Error> {
    generation(registration)?;
    generation(transport)?;
    if registration != c.registration.generation {
        return Err(Error::Generation);
    }
    db.query_row("SELECT sender_mxid FROM matrix_transports WHERE engagement_id=?1 AND registration_generation=?2 AND generation=?3 AND available=1",params![engagement,registration,transport],|r|r.get(0)).optional()?.ok_or(Error::RunnerAuthority)
}
fn invalidate(
    tx: &Transaction<'_>,
    c: &Context,
    room: &str,
    next: u64,
    reason: &str,
    now: u64,
) -> Result<(), Error> {
    generation(next)?;
    text(reason, 4000)?;
    matrix_room(room, &c.registration.server_name)?;
    let prior:Option<(u64,bool,Option<String>)>=tx.query_row("SELECT generation,available,invalidation FROM matrix_room_scopes WHERE server_name=?1 AND room_id=?2 AND fleet_id=?3 AND project_id=?4",params![c.registration.server_name,room,c.registration.fleet_id,c.project],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let (old, available, evidence) = prior.ok_or(Error::RunnerAuthority)?;
    if next == old && !available && evidence.as_deref() == Some(reason) {
        return Ok(());
    }
    if next != old.checked_add(1).ok_or(Error::Capacity)? {
        return Err(Error::Generation);
    }
    tx.execute("UPDATE matrix_room_scopes SET generation=?3,available=0,invalidation=?4 WHERE server_name=?1 AND room_id=?2",params![c.registration.server_name,room,next,reason])?;
    reconcile(tx, now)
}

impl DomainRepository {
    pub fn matrix_room_state(
        &self,
        engagement: &str,
        room: &str,
    ) -> Result<Option<MatrixRoomState>, Error> {
        let c = context(&self.db, engagement)?;
        matrix_room(room, &c.registration.server_name)?;
        Ok(self.db.query_row("SELECT generation,available FROM matrix_room_scopes WHERE server_name=?1 AND room_id=?2 AND fleet_id=?3 AND project_id=?4 AND registration_generation=?5",params![c.registration.server_name,room,c.registration.fleet_id,c.project,c.registration.generation],|r|Ok(MatrixRoomState{generation:r.get(0)?,available:r.get(1)?})).optional()?)
    }
    pub fn matrix_transport_state(
        &self,
        engagement: &str,
    ) -> Result<Option<MatrixTransportState>, Error> {
        let c = context(&self.db, engagement)?;
        let row: Option<(u64,u64,String,String,bool)> = self.db.query_row(
            "SELECT registration_generation,generation,sender_mxid,device_id,available FROM matrix_transports WHERE engagement_id=?1 AND server_name=?2",
            params![engagement,c.registration.server_name],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))
        ).optional()?;
        Ok(row.map(
            |(registration_generation, generation, sender_mxid, device_id, available)| {
                MatrixTransportState {
                    observation: MatrixTransportObservation {
                        engagement_id: engagement.into(),
                        registration_generation,
                        generation,
                        sender_mxid,
                        device_id,
                    },
                    available,
                }
            },
        ))
    }

    /// A failed authenticated bootstrap retires all old route-dependent powers.
    /// Exact captured identity prevents a delayed failure from retiring a newer
    /// owner. Negative evidence does not become a positive account observation.
    pub fn invalidate_matrix_transport(
        &mut self,
        input: &MatrixTransportInvalidation,
        now: u64,
    ) -> Result<(), Error> {
        clock(now)?;
        let expected = &input.expected;
        generation(expected.registration_generation)?;
        generation(expected.generation)?;
        text(&expected.device_id, 255)?;
        text(&input.reason, 256)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let c = context(&tx, &expected.engagement_id)?;
        if c.registration.generation != expected.registration_generation {
            return Err(Error::Generation);
        }
        user(&expected.sender_mxid, &c.registration.server_name)?;
        if [
            &c.owner,
            &c.registration.representative_mxid,
            &c.registration.approval_bot_mxid,
        ]
        .contains(&&expected.sender_mxid)
        {
            return Err(Error::RunnerAuthority);
        }
        let digest = hagency_core::canonical::digest(&serde_json::json!(input))?;
        let prior:Option<(u64,u64,String,String,bool,Option<String>)> = tx.query_row(
            "SELECT registration_generation,generation,sender_mxid,device_id,available,invalidation FROM matrix_transports WHERE engagement_id=?1 AND server_name=?2",
            params![expected.engagement_id,c.registration.server_name],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))
        ).optional()?;
        if let Some((reg, incarnation, sender, device, available, prior_digest)) = prior {
            if reg != expected.registration_generation
                || incarnation != expected.generation
                || sender != expected.sender_mxid
                || device != expected.device_id
            {
                return Err(Error::Generation);
            }
            if !available {
                return if prior_digest.as_deref() == Some(&digest) {
                    tx.commit()?;
                    Ok(())
                } else {
                    Err(Error::Conflict)
                };
            }
            tx.execute(
                "UPDATE matrix_transports SET available=0,invalidation=?2 WHERE engagement_id=?1",
                params![expected.engagement_id, digest],
            )?;
        } else {
            if expected.generation != 1 {
                return Err(Error::Generation);
            }
            tx.execute("INSERT INTO matrix_transports(engagement_id,registration_generation,generation,server_name,sender_mxid,device_id,available,invalidation) VALUES(?1,?2,?3,?4,?5,?6,0,?7)",params![expected.engagement_id,expected.registration_generation,expected.generation,c.registration.server_name,expected.sender_mxid,expected.device_id,digest])?;
        }
        reconcile(&tx, now)?;
        tx.commit()?;
        Ok(())
    }

    /// Called after authenticating the exact Matrix account/device. No endpoint
    /// accepts this observation and an applied provisioning string is not enough.
    pub fn observe_matrix_transport(
        &mut self,
        input: &MatrixTransportObservation,
        now: u64,
    ) -> Result<(), Error> {
        clock(now)?;
        generation(input.generation)?;
        generation(input.registration_generation)?;
        text(&input.device_id, 255)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let c = context(&tx, &input.engagement_id)?;
        if input.registration_generation != c.registration.generation {
            return Err(Error::Generation);
        }
        user(&input.sender_mxid, &c.registration.server_name)?;
        if [
            &c.owner,
            &c.registration.representative_mxid,
            &c.registration.approval_bot_mxid,
        ]
        .contains(&&input.sender_mxid)
        {
            return Err(Error::RunnerAuthority);
        }
        let old:Option<(u64,u64,String,String,String,bool)>=tx.query_row("SELECT generation,registration_generation,server_name,sender_mxid,device_id,available FROM matrix_transports WHERE engagement_id=?1",[&input.engagement_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
        if let Some((old_gen, reg, server, sender, device, available)) = old {
            if old_gen == input.generation {
                if reg != input.registration_generation
                    || server != c.registration.server_name
                    || sender != input.sender_mxid
                    || device != input.device_id
                {
                    return Err(Error::Conflict);
                }
                if !available {
                    return Err(Error::State);
                }
                tx.commit()?;
                return Ok(());
            }
            if input.generation != old_gen.checked_add(1).ok_or(Error::Capacity)? {
                return Err(Error::Generation);
            }
        } else if input.generation != 1 {
            return Err(Error::Generation);
        }
        tx.execute("INSERT INTO matrix_transports(engagement_id,registration_generation,generation,server_name,sender_mxid,device_id) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(engagement_id) DO UPDATE SET registration_generation=excluded.registration_generation,generation=excluded.generation,server_name=excluded.server_name,sender_mxid=excluded.sender_mxid,device_id=excluded.device_id,available=1,invalidation=NULL",params![input.engagement_id,input.registration_generation,input.generation,c.registration.server_name,input.sender_mxid,input.device_id])?;
        tx.execute(
            "UPDATE matrix_transports SET observed_at=?2 WHERE engagement_id=?1",
            params![input.engagement_id, now],
        )?;
        reconcile(&tx, now)?;
        tx.commit()?;
        Ok(())
    }

    /// A bounded, authenticated full room snapshot. Unsafe membership/privacy
    /// durably invalidates an existing scope; Ok acknowledges the observation,
    /// not a promise that the room admits a new session. Changed snapshots require
    /// a new room generation, including changes observed through another agent.
    pub fn observe_matrix_room(
        &mut self,
        input: &MatrixRoomObservation,
        now: u64,
    ) -> Result<(), Error> {
        clock(now)?;
        generation(input.generation)?;
        if input.joined.len() > 1000
            || input.joined.iter().map(String::len).sum::<usize>() > 48 * 1024
            || matches!(&input.privacy,RoomPrivacy::Direct{human_mxid} if human_mxid.len()>255)
        {
            return Err(Error::Capacity);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let c = context(&tx, &input.engagement_id)?;
        matrix_room(&input.room_id, &c.registration.server_name)?;
        if input.room_id == c.approval_room || input.room_id == c.registration.reception_room_id {
            return Err(Error::RunnerAuthority);
        }
        let sender = observed_transport(
            &tx,
            &c,
            &input.engagement_id,
            input.registration_generation,
            input.transport_generation,
        )?;
        let invalid_members = input
            .joined
            .iter()
            .any(|id| matrix_user(id, &c.registration.server_name).is_err())
            || !input.joined.contains(&sender)
            || !input.joined.contains(&c.owner);
        let invalid_direct = matches!(&input.privacy,RoomPrivacy::Direct{human_mxid} if human_mxid!=&c.owner || input.joined.len()!=2 || !input.invite_only || !input.encrypted);
        if invalid_members || invalid_direct {
            // Bind replay to the exact observation, not only a generic reason.
            let reason = format!(
                "unsafe snapshot {}",
                hagency_core::canonical::digest(&serde_json::json!(input))?
            );
            invalidate(&tx, &c, &input.room_id, input.generation, &reason, now)?;
            tx.commit()?;
            return Ok(());
        }
        let privacy = serialize(&input.privacy)?;
        let joined = serialize(&input.joined)?;
        let old:Option<(u64,String,String,String,bool,Option<String>)>=tx.query_row("SELECT generation,fleet_id,project_id,privacy,encrypted,direct_sender FROM matrix_room_scopes WHERE server_name=?1 AND room_id=?2",params![c.registration.server_name,input.room_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
        if let Some((old_gen, fleet, project, old_privacy, encrypted, direct_sender)) = old {
            if fleet != c.registration.fleet_id || project != c.project {
                return Err(Error::RunnerAuthority);
            }
            let prior: RoomPrivacy = serde_json::from_str(&old_privacy)?;
            if matches!(input.privacy, RoomPrivacy::Direct { .. })
                && (matches!(prior, RoomPrivacy::Group {})
                    || direct_sender.as_deref() != Some(&sender))
            {
                invalidate(
                    &tx,
                    &c,
                    &input.room_id,
                    input.generation,
                    "incompatible privacy classification",
                    now,
                )?;
                tx.commit()?;
                return Ok(());
            }
            if old_gen == input.generation {
                let exact:bool=tx.query_row("SELECT available=1 AND registration_generation=?3 AND owner_mxid=?4 AND joined=?5 AND invite_only=?6 FROM matrix_room_scopes WHERE server_name=?1 AND room_id=?2",params![c.registration.server_name,input.room_id,c.registration.generation,c.owner,joined,input.invite_only],|r|r.get(0))?;
                if !exact || old_privacy != privacy || encrypted != input.encrypted {
                    return Err(Error::Conflict);
                }
                membership(
                    &tx,
                    input,
                    &c.registration.server_name,
                    input.transport_generation,
                )?;
                reconcile(&tx, now)?;
                tx.commit()?;
                return Ok(());
            }
            if input.generation != old_gen.checked_add(1).ok_or(Error::Capacity)? {
                return Err(Error::Generation);
            }
        } else {
            let count: u64 =
                tx.query_row("SELECT COUNT(*) FROM matrix_room_scopes", [], |r| r.get(0))?;
            if count >= 10_000 {
                return Err(Error::Capacity);
            }
            if input.generation != 1 {
                return Err(Error::Generation);
            }
            if matches!(input.privacy, RoomPrivacy::Group {}) && input.room_id != c.project_room {
                return Err(Error::RunnerAuthority);
            }
        }
        let direct_sender = matches!(input.privacy, RoomPrivacy::Direct { .. }).then_some(sender);
        tx.execute("INSERT INTO matrix_room_scopes(server_name,room_id,generation,fleet_id,project_id,registration_generation,owner_mxid,privacy,encrypted,direct_sender,joined,invite_only) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(server_name,room_id) DO UPDATE SET generation=excluded.generation,registration_generation=excluded.registration_generation,owner_mxid=excluded.owner_mxid,privacy=excluded.privacy,encrypted=excluded.encrypted,direct_sender=excluded.direct_sender,joined=excluded.joined,invite_only=excluded.invite_only,available=1,invalidation=NULL",params![c.registration.server_name,input.room_id,input.generation,c.registration.fleet_id,c.project,c.registration.generation,c.owner,privacy,input.encrypted,direct_sender,joined,input.invite_only])?;
        tx.execute(
            "UPDATE matrix_room_scopes SET visibility_since=?3 WHERE server_name=?1 AND room_id=?2",
            params![c.registration.server_name, input.room_id, now],
        )?;
        membership(
            &tx,
            input,
            &c.registration.server_name,
            input.transport_generation,
        )?;
        reconcile(&tx, now)?;
        tx.commit()?;
        Ok(())
    }

    /// Missing/unknown room state is authoritative negative evidence. Timeouts
    /// must invalidate before any further send; restoring needs fresh evidence.
    pub fn invalidate_matrix_room(
        &mut self,
        input: &MatrixRoomInvalidation,
        now: u64,
    ) -> Result<(), Error> {
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let c = context(&tx, &input.engagement_id)?;
        observed_transport(
            &tx,
            &c,
            &input.engagement_id,
            input.registration_generation,
            input.transport_generation,
        )?;
        invalidate(
            &tx,
            &c,
            &input.room_id,
            input.generation,
            &input.reason,
            now,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Create or resolve only a current fresh session. Existing legacy IDs are
    /// never upgraded because their past private context cannot be reconstructed.
    pub fn resolve_verified_matrix_session(
        &mut self,
        binding: &SessionBinding,
        now: u64,
    ) -> Result<SessionBinding, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = resolve(&tx, binding, now)?;
        tx.commit()?;
        Ok(value)
    }
}

pub(super) fn resolve(
    tx: &Transaction<'_>,
    binding: &SessionBinding,
    now: u64,
) -> Result<SessionBinding, Error> {
    binding.validate()?;
    clock(now)?;
    let c = context(tx, &binding.engagement_id)?;
    reconcile(tx, now)?;
    let current:Option<String>=tx.query_row("SELECT s.id FROM runner_sessions s JOIN current_matrix_routes r ON r.session_id=s.id WHERE s.engagement_id=?1 AND json_extract(s.binding,'$.room_id')=?2 AND json_extract(s.binding,'$.thread_root') IS ?3",params![binding.engagement_id,binding.room_id,binding.thread_root],|r|r.get(0)).optional()?;
    if let Some(id) = current {
        let existing = execution::matrix_admission_session(tx, &id)?;
        return Ok(existing);
    }
    if tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM runner_sessions WHERE id=?1)",
        [&binding.id],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(Error::Conflict);
    }
    let transport:Option<(u64,String,String)>=tx.query_row("SELECT generation,sender_mxid,device_id FROM matrix_transports WHERE engagement_id=?1 AND registration_generation=?2 AND available=1",params![binding.engagement_id,c.registration.generation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let (transport_generation, sender_mxid, device_id) = transport.ok_or(Error::RunnerAuthority)?;
    let room:Option<(u64,String,bool)>=tx.query_row("SELECT generation,privacy,encrypted FROM matrix_room_scopes WHERE server_name=?1 AND room_id=?2 AND fleet_id=?3 AND project_id=?4 AND registration_generation=?5 AND owner_mxid=?6",params![c.registration.server_name,binding.room_id,c.registration.fleet_id,c.project,c.registration.generation,c.owner],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let (room_generation, privacy, encrypted) = room.ok_or(Error::RunnerAuthority)?;
    let session_generation:u64=tx.query_row("SELECT COALESCE(MAX(matrix_generation),0)+1 FROM runner_sessions WHERE engagement_id=?1 AND json_extract(binding,'$.room_id')=?2 AND json_extract(binding,'$.thread_root') IS ?3",params![binding.engagement_id,binding.room_id,binding.thread_root],|r|r.get(0))?;
    generation(session_generation)?;
    let route = ReplyRoute {
        session_id: binding.id.clone(),
        session_generation,
        engagement_id: binding.engagement_id.clone(),
        fleet_id: c.registration.fleet_id,
        project_id: c.project,
        registration_generation: c.registration.generation,
        server_name: c.registration.server_name,
        room_id: binding.room_id.clone(),
        room_generation,
        sender_mxid,
        device_id,
        transport_generation,
        owner_mxid: c.owner,
        privacy: serde_json::from_str(&privacy)?,
        encrypted,
        thread_root: binding.thread_root.clone(),
    };
    bounded_row(tx, "runner_sessions", "id", &binding.id, 10_000)?;
    tx.execute("INSERT INTO runner_sessions(id,engagement_id,binding,matrix_generation) VALUES(?1,?2,?3,?4)",params![binding.id,binding.engagement_id,serialize(binding)?,session_generation])?;
    tx.execute("INSERT INTO matrix_session_routes(session_id,server_name,room_id,room_generation,transport_generation,registration_generation,config) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![binding.id,route.server_name,route.room_id,route.room_generation,route.transport_generation,route.registration_generation,serialize(&route)?])?;
    tx.execute("UPDATE matrix_session_routes SET ingress_since=(SELECT MAX(t.observed_at,r.visibility_since) FROM matrix_transports t JOIN matrix_room_scopes r ON r.server_name=t.server_name AND r.room_id=?2 WHERE t.engagement_id=?3) WHERE session_id=?1",params![binding.id,binding.room_id,binding.engagement_id])?;
    check(tx, &binding.id)?;
    Ok(binding.clone())
}
