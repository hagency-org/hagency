//! Durable owner authorization. No native protocol or Matrix send is performed.
use super::{DomainRepository, bounded_row, execution, matrix_routes, serialize};
use crate::Error;
use hagency_core::{
    approvals::*,
    authority::Registration,
    canonical,
    execution::{self as policy, HostRequest, PathFlavor, ScopeKind},
    project::identifier,
    replies::{ReplyRoute, matrix_room, matrix_user},
    tasks::{RunnerCapability, TaskState, clock, text},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(super) mod card;
pub(super) mod owned;
pub(super) mod responses;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Binding {
    engagement: String,
    fleet: String,
    project: String,
    registration: u64,
    server: String,
    room: String,
    owner: String,
    bot: String,
    device: String,
    room_generation: u64,
    generation: u64,
}
#[derive(Serialize, Deserialize)]
struct Context {
    id: String,
    dispatch: String,
    fence: u64,
    task: String,
    epoch: u64,
    route: ReplyRoute,
    binding: Binding,
    connection: String,
    thread: String,
    turn: String,
    resource: String,
    workspace: String,
    windows: bool,
    environment: Option<String>,
    may_write: bool,
}
#[derive(Serialize, Deserialize)]
struct Request {
    upstream: ApprovalRpcId,
    item: String,
    method: String,
    params: Value,
}
struct EncodedLimit(usize);
impl std::io::Write for EncodedLimit {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("approval metadata capacity"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn binding(db: &Connection, engagement: &str) -> Result<Binding, Error> {
    db.query_row("SELECT b.engagement_id,room.fleet_id,room.project_id,room.registration_generation,room.server_name,room.room_id,room.owner_mxid,room.bot_mxid,room.device_id,room.generation,b.incarnation FROM approval_bindings b JOIN current_approval_bindings current ON current.engagement_id=b.engagement_id JOIN approval_rooms room ON room.server_name=b.server_name AND room.room_id=b.room_id WHERE b.engagement_id=?1",[engagement],|r|Ok(Binding{engagement:r.get(0)?,fleet:r.get(1)?,project:r.get(2)?,registration:r.get(3)?,server:r.get(4)?,room:r.get(5)?,owner:r.get(6)?,bot:r.get(7)?,device:r.get(8)?,room_generation:r.get(9)?,generation:r.get(10)?})).optional()?.ok_or(Error::RunnerAuthority)
}
fn context(db: &Connection, id: &str) -> Result<Context, Error> {
    let value: String = db
        .query_row(
            "SELECT config FROM approval_contexts WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::RunnerAuthority)?;
    Ok(serde_json::from_str(&value)?)
}
fn request(db: &Connection, id: &str) -> Result<(Context, Request), Error> {
    let (context_id, value): (String, String) = db
        .query_row(
            "SELECT context_id,config FROM owner_approvals WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok((context(db, &context_id)?, serde_json::from_str(&value)?))
}
fn live(db: &Connection, c: &Context, now: u64) -> Result<(), Error> {
    let current:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM runner_dispatches d JOIN resource_leases lease ON lease.dispatch_id=d.id AND lease.resource_id=?3 JOIN workspace_resources resource ON resource.id=lease.resource_id AND resource.dirty=0 JOIN dispatch_resources intended ON intended.dispatch_id=d.id AND intended.resource_id=lease.resource_id WHERE d.id=?1 AND d.fence=?2 AND d.state IN ('started','parked') AND d.lease_until>?4 AND d.capability_until>?4 AND (?5=0 OR (lease.exclusive=1 AND intended.exclusive=1)))",params![c.dispatch,c.fence,c.resource,now,c.may_write],|r|r.get(0))?;
    if !current
        || binding(db, &c.binding.engagement)? != c.binding
        || matrix_routes::route(db, &c.route.session_id)? != c.route
    {
        return Err(Error::RunnerAuthority);
    }
    let task = execution::task(db, &c.task)?;
    if task.session_id != c.route.session_id
        || task.execution_epoch != c.epoch
        || task.status == TaskState::Done
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
fn authorize(db: &Connection, cap: &RunnerCapability, c: &Context, now: u64) -> Result<(), Error> {
    let d = execution::authorize(db, cap, now, &["started", "parked"])?;
    if c.dispatch != cap.dispatch_id
        || c.fence != cap.fence
        || d.task_id.as_deref() != Some(&c.task)
        || d.report_task.is_some()
    {
        return Err(Error::RunnerAuthority);
    }
    live(db, c, now)
}
fn grant_context(c: &Context) -> Result<String, Error> {
    Ok(canonical::digest(&json!([
        c.binding,
        c.workspace,
        c.windows,
        c.environment,
        c.may_write,
        c.resource
    ]))?)
}
fn summary(db: &Connection, id: &str) -> Result<ApprovalSummary, Error> {
    let (state, scope, choice): (String, bool, Option<String>) = db
        .query_row(
            "SELECT state,scope_key IS NOT NULL,choice FROM owner_approvals WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(ApprovalSummary {
        id: id.into(),
        state,
        reusable_scope: scope,
        choice: choice.map(|s| serde_json::from_str(&s)).transpose()?,
    })
}
fn park(tx: &Transaction<'_>, c: &Context) -> Result<(), Error> {
    tx.execute(
        "UPDATE runner_dispatches SET state='parked' WHERE id=?1 AND fence=?2",
        params![c.dispatch, c.fence],
    )?;
    tx.execute(
        "UPDATE runner_attempts SET outcome='parked' WHERE dispatch_id=?1 AND fence=?2",
        params![c.dispatch, c.fence],
    )?;
    Ok(())
}
// Generic unpark cannot bypass the original one-shot response owner.
pub(super) fn check_resume(db: &Connection, dispatch: &str, fence: u64) -> Result<(), Error> {
    let blocked:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM owner_approvals a JOIN approval_contexts c ON c.id=a.context_id WHERE c.dispatch_id=?1 AND c.fence=?2)",params![dispatch,fence],|r|r.get(0))?;
    if blocked {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
pub(super) fn recover(tx: &Transaction<'_>) -> Result<(), Error> {
    tx.execute(
        "UPDATE owner_approvals SET state='uncertain' WHERE state='applying'",
        [],
    )?;
    tx.execute("UPDATE approval_grants SET revoked=1 WHERE revoked=0 AND (NOT EXISTS(SELECT 1 FROM current_approval_bindings b JOIN approval_bindings binding ON binding.engagement_id=b.engagement_id WHERE b.engagement_id=approval_grants.engagement_id AND binding.incarnation=approval_grants.binding_generation) OR (mode='task' AND NOT EXISTS(SELECT 1 FROM canonical_tasks t WHERE t.id=approval_grants.task_id AND json_extract(t.config,'$.status')<>'done' AND json_extract(t.config,'$.execution_epoch')=approval_grants.task_epoch)))",[])?;
    Ok(())
}
impl DomainRepository {
    pub fn observe_approval_room(
        &mut self,
        input: &ApprovalRoomObservation,
        now: u64,
    ) -> Result<(), Error> {
        clock(now)?;
        identifier(&input.engagement_id, 128)?;
        text(&input.device_id, 255)?;
        hagency_core::replies::generation(input.generation)?;
        if input.joined.len() > 1000
            || input.joined.iter().map(String::len).sum::<usize>() > 48 * 1024
        {
            return Err(Error::Capacity);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (encoded,project,owner,room,project_room):(String,String,String,String,String)=tx.query_row("SELECT r.config,e.project_id,p.owner_mxid,p.owner_room_id,p.room_id FROM engagements e JOIN registrations r ON r.fleet_id=e.fleet_id AND r.generation=e.generation JOIN projects p ON p.fleet_id=e.fleet_id AND p.id=e.project_id AND p.generation=e.generation WHERE e.id=?1 AND e.state='active'",[&input.engagement_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or(Error::RunnerAuthority)?;
        let reg: Registration = serde_json::from_str(&encoded)?;
        if reg.generation != input.registration_generation
            || room != input.room_id
            || room == project_room
            || room == reg.reception_room_id
        {
            return Err(Error::RunnerAuthority);
        }
        matrix_room(&room, &reg.server_name)?;
        let safe = input.available
            && input.encrypted
            && input.invite_only
            && input.joined.len() == 2
            && input.joined.contains(&owner)
            && input.joined.contains(&reg.approval_bot_mxid)
            && input
                .joined
                .iter()
                .all(|id| matrix_user(id, &reg.server_name).is_ok());
        let snapshot = json!({"joined":input.joined,"invite_only":input.invite_only,"encrypted":input.encrypted,"available":input.available});
        let digest = canonical::digest(&json!([
            reg.fleet_id,
            project,
            reg.generation,
            owner,
            reg.approval_bot_mxid,
            input.room_id,
            input.device_id,
            input.generation,
            input.joined,
            input.invite_only,
            input.encrypted,
            input.available
        ]))?;
        let old:Option<(u64,String,String,String)>=tx.query_row("SELECT generation,digest,fleet_id,project_id FROM approval_rooms WHERE server_name=?1 AND room_id=?2",params![reg.server_name,room],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let update = if let Some((generation, prior, fleet, old_project)) = old {
            if fleet != reg.fleet_id || old_project != project {
                return Err(Error::RunnerAuthority);
            }
            if generation == input.generation {
                // Current negative evidence retires authority immediately even if
                // the adapter has not advanced its observation generation yet.
                // Positive changes and restoration still need a new generation.
                if prior != digest && safe {
                    return Err(Error::Conflict);
                }
                prior != digest
            } else {
                if input.generation != generation.checked_add(1).ok_or(Error::Capacity)? {
                    return Err(Error::Generation);
                }
                true
            }
        } else {
            if input.generation != 1 {
                return Err(Error::Generation);
            }
            bounded_row(&tx, "approval_rooms", "room_id", &room, 10_000)?;
            true
        };
        if update {
            tx.execute("INSERT INTO approval_rooms(server_name,room_id,generation,fleet_id,project_id,registration_generation,owner_mxid,bot_mxid,device_id,available,digest,config) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(server_name,room_id) DO UPDATE SET generation=excluded.generation,registration_generation=excluded.registration_generation,owner_mxid=excluded.owner_mxid,bot_mxid=excluded.bot_mxid,device_id=excluded.device_id,available=excluded.available,digest=excluded.digest,config=excluded.config",params![reg.server_name,room,input.generation,reg.fleet_id,project,reg.generation,owner,reg.approval_bot_mxid,input.device_id,safe,digest,serialize(&snapshot)?])?;
        }
        if safe {
            bounded_row(
                &tx,
                "approval_bindings",
                "engagement_id",
                &input.engagement_id,
                10_000,
            )?;
            tx.execute("INSERT INTO approval_bindings(engagement_id,server_name,room_id,room_generation,incarnation) VALUES(?1,?2,?3,?4,1) ON CONFLICT(engagement_id) DO UPDATE SET server_name=excluded.server_name,room_id=excluded.room_id,room_generation=excluded.room_generation,incarnation=approval_bindings.incarnation+1 WHERE approval_bindings.room_id<>excluded.room_id OR approval_bindings.server_name<>excluded.server_name OR approval_bindings.room_generation<>excluded.room_generation",params![input.engagement_id,reg.server_name,room,input.generation])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn bind_approval_context(
        &mut self,
        cap: &RunnerCapability,
        input: &HostApprovalContext,
        now: u64,
    ) -> Result<(), Error> {
        self.bind_approval_context_clock(cap, input, || Ok(now))
    }

    pub(crate) fn bind_approval_context_clock(
        &mut self,
        cap: &RunnerCapability,
        input: &HostApprovalContext,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<(), Error> {
        let workspace = context_input(input)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?; // The original writer queue and SQLite lock waits have ended.
        clock(now)?;
        bind_context(&tx, cap, input, workspace, now)?;
        tx.commit()?;
        Ok(())
    }
    pub fn request_owner_approval(
        &mut self,
        cap: &RunnerCapability,
        input: &HostApprovalRequest,
        now: u64,
    ) -> Result<ApprovalSummary, Error> {
        self.request_owner_approval_clock(cap, input, || Ok(now))
    }

    pub(crate) fn request_owner_approval_clock(
        &mut self,
        cap: &RunnerCapability,
        input: &HostApprovalRequest,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<ApprovalSummary, Error> {
        identifier(&input.context_id, 256)?;
        identifier(&input.item_id, 256)?;
        text(&input.method, 256)?;
        input.upstream_id.validate()?;
        clock(input.expires_at)?;
        if !input.params.is_object()
            || serde_json::to_writer(EncodedLimit(48 * 1024), input).is_err()
        {
            return Err(Error::Capacity);
        }
        canonical::payload_digest(&input.params)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?; // The original writer queue and SQLite lock waits have ended.
        clock(now)?;
        let c = context(&tx, &input.context_id)?;
        authorize(&tx, cap, &c, now)?;
        if input
            .params
            .get("environmentId")
            .is_some_and(|value| !value.is_null() && !value.is_string())
        {
            return Err(Error::RunnerAuthority);
        }
        if input.params.get("threadId").and_then(Value::as_str) != Some(&c.thread)
            || input.params.get("turnId").and_then(Value::as_str) != Some(&c.turn)
            || input.params.get("itemId").and_then(Value::as_str) != Some(&input.item_id)
            || input
                .params
                .get("environmentId")
                .filter(|v| !v.is_null())
                .and_then(Value::as_str)
                != c.environment.as_deref()
        {
            return Err(Error::RunnerAuthority);
        }
        let scope = policy::derive(HostRequest {
            agent_id: &c.binding.engagement,
            workspace: &c.workspace,
            task_id: Some(&c.task),
            may_write: c.may_write,
            method: &input.method,
            params: &input.params,
            path_flavor: if c.windows {
                PathFlavor::Windows
            } else {
                PathFlavor::Posix
            },
        });
        if !c.may_write
            && scope
                .as_ref()
                .is_none_or(|s| s.scope.kind != ScopeKind::NetworkHost)
        {
            return Err(Error::RunnerAuthority);
        }
        let source = canonical::digest(&json!([c.connection, input.upstream_id]))?;
        let digest = canonical::digest(&json!([input, c]))?;
        let prior: Option<(String, String)> = tx
            .query_row(
                "SELECT id,digest FROM owner_approvals WHERE source_key=?1",
                [&source],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, old)) = prior {
            if old != digest {
                return Err(Error::Conflict);
            }
            return summary(&tx, &id);
        }
        if input.expires_at <= now || input.expires_at - now > 600_000 {
            return Err(Error::RunnerAuthority);
        }
        let pending:(u64,u64,u64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM(c.engagement_id=?1),0),COALESCE(SUM(c.dispatch_id=?2 AND c.fence=?3),0) FROM owner_approvals a JOIN approval_contexts c ON c.id=a.context_id WHERE a.state IN ('pending','decided','applying','uncertain')",params![c.binding.engagement,c.dispatch,c.fence],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if pending.0 >= 1024 || pending.1 >= 64 || pending.2 >= 16 {
            return Err(Error::Capacity);
        }
        let id = format!("approval_{}", &source[..40]);
        bounded_row(&tx, "owner_approvals", "id", &id, 100_000)?;
        let context_key = grant_context(&c)?;
        let grant: Option<(String, String)> = if let Some(scope) = &scope {
            tx.query_row("SELECT id,mode FROM approval_grants WHERE engagement_id=?1 AND binding_generation=?2 AND scope_key=?3 AND context_key=?4 AND revoked=0 AND (mode='always' OR (task_id=?5 AND task_epoch=?6)) ORDER BY id LIMIT 1",params![c.binding.engagement,c.binding.generation,scope.scope.key,context_key,c.task,c.epoch],|r|Ok((r.get(0)?,r.get(1)?))).optional()?
        } else {
            None
        };
        let choice = grant.as_ref().map(|(_, mode)| {
            if mode == "always" {
                ApprovalChoice::Always
            } else {
                ApprovalChoice::Task
            }
        });
        let r = Request {
            upstream: input.upstream_id.clone(),
            item: input.item_id.clone(),
            method: input.method.clone(),
            params: input.params.clone(),
        };
        tx.execute("INSERT INTO owner_approvals(id,source_key,context_id,digest,config,scope_key,scope_kind,description,state,choice,grant_id,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![id,source,c.id,digest,serialize(&r)?,scope.as_ref().map(|s|s.scope.key.as_str()),scope.as_ref().map(|s|serialize(&s.scope.kind)).transpose()?,scope.as_ref().map(|s|s.scope.description.as_str()),if grant.is_some(){"decided"}else{"pending"},choice.map(|v|serialize(&v)).transpose()?,grant.map(|g|g.0),input.expires_at])?;
        park(&tx, &c)?;
        let result = summary(&tx, &id)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn approval_summary(&self, id: &str) -> Result<ApprovalSummary, Error> {
        identifier(id, 128)?;
        summary(&self.db, id)
    }
    pub fn private_approval(&self, id: &str, now: u64) -> Result<PrivateApproval, Error> {
        clock(now)?;
        identifier(id, 128)?;
        let (c, r) = request(&self.db, id)?;
        live(&self.db, &c, now)?;
        let (digest, description, expires): (String, Option<String>, u64) = self.db.query_row(
            "SELECT digest,description,expires_at FROM owner_approvals WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(PrivateApproval {
            summary: summary(&self.db, id)?,
            digest,
            room_id: c.binding.room,
            owner_mxid: c.binding.owner,
            binding_generation: c.binding.generation,
            method: r.method,
            params: r.params,
            description,
            expires_at: expires,
        })
    }
    pub fn observe_owner_verdict(
        &mut self,
        input: &OwnerVerdictObservation,
        now: u64,
    ) -> Result<ApprovalSummary, Error> {
        self.observe_owner_verdict_clock(input, || Ok(now))
    }

    pub(crate) fn observe_owner_verdict_clock(
        &mut self,
        input: &OwnerVerdictObservation,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<ApprovalSummary, Error> {
        identifier(&input.request_id, 128)?;
        text(&input.request_digest, 64)?;
        matrix_user(&input.sender_mxid, &input.server_name)?;
        matrix_room(&input.room_id, &input.server_name)?;
        hagency_core::replies::matrix_event(&input.event_id)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?; // The original writer queue and SQLite lock waits have ended.
        clock(now)?;
        let result = decide_verdict(&tx, input, now, None)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn consume_owner_approval(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        now: u64,
    ) -> Result<ApprovalApplication, Error> {
        self.consume_owner_approval_clock(cap, id, || Ok(now))
    }

    pub(crate) fn consume_owner_approval_clock(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<ApprovalApplication, Error> {
        identifier(id, 128)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?; // The original writer queue and SQLite lock waits have ended.
        clock(now)?;
        let application = consume(&tx, cap, id, now)?;
        tx.commit()?;
        Ok(application)
    }
    pub fn observe_approval_application(
        &mut self,
        input: &ApprovalApplicationObservation,
        now: u64,
    ) -> Result<ApprovalSummary, Error> {
        self.observe_approval_application_clock(input, || Ok(now))
    }

    pub(crate) fn observe_approval_application_clock(
        &mut self,
        input: &ApprovalApplicationObservation,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<ApprovalSummary, Error> {
        input.validate()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?; // The original writer queue and SQLite lock waits have ended.
        clock(now)?;
        let id = &input.application.id;
        let (application, state, prior): (Option<String>, String, Option<String>) = tx
            .query_row(
                "SELECT application,state,observation FROM owner_approvals WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if application.as_deref() != Some(&serialize(&input.application)?) {
            return Err(Error::RunnerAuthority);
        }
        let observed = serialize(input)?;
        if prior.as_deref() == Some(&observed) {
            return summary(&tx, id);
        }
        if !["applying", "uncertain"].contains(&state.as_str()) {
            return Err(Error::RunnerAuthority);
        }
        let state = match input.outcome {
            ApplicationOutcome::Applied => "applied",
            ApplicationOutcome::NotApplied => "not_applied",
            ApplicationOutcome::Unknown => "uncertain",
        };
        tx.execute(
            "UPDATE owner_approvals SET state=?2,observation=?3 WHERE id=?1",
            params![id, state, observed],
        )?;
        // Native application observations never create router execution authority.
        let result = summary(&tx, id)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn revoke_approval_grant(&mut self, id: &str) -> Result<(), Error> {
        identifier(id, 128)?;
        if self
            .db
            .execute("UPDATE approval_grants SET revoked=1 WHERE id=?1", [id])?
            != 1
        {
            return Err(Error::NotFound);
        }
        Ok(())
    }
    pub fn approval_grants(
        &self,
        engagement: &str,
        after: &str,
        limit: u64,
    ) -> Result<Vec<GrantSummary>, Error> {
        identifier(engagement, 128)?;
        if limit == 0 || limit > 100 {
            return Err(Error::Capacity);
        }
        let mut query=self.db.prepare("SELECT id,scope_kind,mode,revoked FROM approval_grants WHERE engagement_id=?1 AND id>?2 ORDER BY id LIMIT ?3")?;
        query
            .query_map(params![engagement, after, limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            })?
            .map(|r| {
                let (id, kind, mode, revoked) = r?;
                let kind: String = serde_json::from_str(&kind)?;
                if !["exact_command", "network_host", "permission_profile"].contains(&kind.as_str())
                {
                    return Err(Error::State);
                }
                Ok(GrantSummary {
                    id,
                    kind,
                    mode: if mode == "always" {
                        ApprovalChoice::Always
                    } else {
                        ApprovalChoice::Task
                    },
                    revoked,
                })
            })
            .collect()
    }
}

fn decide_verdict(
    tx: &Transaction<'_>,
    input: &OwnerVerdictObservation,
    now: u64,
    source_digest: Option<&str>,
) -> Result<ApprovalSummary, Error> {
    let (c, _) = request(tx, &input.request_id)?;
    live(tx, &c, now)?;
    if !input.encrypted
        || input.server_name != c.binding.server
        || input.room_id != c.binding.room
        || input.sender_mxid != c.binding.owner
        || input.binding_generation != c.binding.generation
    {
        return Err(Error::RunnerAuthority);
    }
    let (digest, state, expires, scope, kind): (
        String,
        String,
        u64,
        Option<String>,
        Option<String>,
    ) = tx.query_row(
        "SELECT digest,state,expires_at,scope_key,scope_kind FROM owner_approvals WHERE id=?1",
        [&input.request_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    if digest != input.request_digest || expires <= now {
        return Err(Error::RunnerAuthority);
    }
    let source = canonical::digest(&json!([input.server_name, input.event_id]))?;
    let digest = source_digest
        .map(str::to_owned)
        .unwrap_or(canonical::digest(&json!(input))?);
    let old: Option<String> = tx
        .query_row(
            "SELECT digest FROM approval_verdict_receipts WHERE source_key=?1",
            [&source],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = old {
        if old != digest {
            return Err(Error::Conflict);
        }
        return summary(tx, &input.request_id);
    }
    if state != "pending" {
        return Err(Error::RunnerAuthority);
    }
    let grant = if matches!(input.choice, ApprovalChoice::Task | ApprovalChoice::Always) {
        let key = scope.ok_or(Error::RunnerAuthority)?;
        let kind = kind.ok_or(Error::RunnerAuthority)?;
        let id = format!(
            "grant_{}",
            &canonical::digest(&json!([input.request_id, input.choice]))?[..40]
        );
        bounded_row(tx, "approval_grants", "id", &id, 100_000)?;
        tx.execute("INSERT INTO approval_grants(id,engagement_id,binding_generation,scope_key,scope_kind,mode,task_id,task_epoch,context_key) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![id,c.binding.engagement,c.binding.generation,key,kind,if input.choice==ApprovalChoice::Always{"always"}else{"task"},if input.choice==ApprovalChoice::Task{Some(&c.task)}else{None},if input.choice==ApprovalChoice::Task{Some(c.epoch)}else{None},grant_context(&c)?])?;
        Some(id)
    } else {
        None
    };
    bounded_row(
        tx,
        "approval_verdict_receipts",
        "source_key",
        &source,
        100_000,
    )?;
    tx.execute(
        "INSERT INTO approval_verdict_receipts(source_key,digest,request_id) VALUES(?1,?2,?3)",
        params![source, digest, input.request_id],
    )?;
    tx.execute(
        "UPDATE owner_approvals SET state='decided',choice=?2,grant_id=?3 WHERE id=?1",
        params![input.request_id, serialize(&input.choice)?, grant],
    )?;
    let result = summary(tx, &input.request_id)?;
    Ok(result)
}

fn room_authority(db: &Connection, engagement: &str) -> Result<ApprovalRoomAuthority, Error> {
    identifier(engagement, 128)?;
    let (encoded, project, owner, room, project_room):(String,String,String,String,String)=db.query_row("SELECT r.config,e.project_id,p.owner_mxid,p.owner_room_id,p.room_id FROM engagements e JOIN registrations r ON r.fleet_id=e.fleet_id AND r.generation=e.generation JOIN projects p ON p.fleet_id=e.fleet_id AND p.id=e.project_id AND p.generation=e.generation WHERE e.id=?1 AND e.state='active'",[engagement],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or(Error::RunnerAuthority)?;
    let reg: Registration = serde_json::from_str(&encoded)?;
    if room == project_room || room == reg.reception_room_id {
        return Err(Error::RunnerAuthority);
    }
    Ok(ApprovalRoomAuthority {
        engagement_id: engagement.into(),
        fleet_id: reg.fleet_id,
        project_id: project,
        registration_generation: reg.generation,
        server_name: reg.server_name,
        room_id: room,
        project_room_id: project_room,
        owner_mxid: owner,
        bot_mxid: reg.approval_bot_mxid,
    })
}
fn intake_target(db: &Connection, id: &str, now: u64) -> Result<ApprovalIntakeTarget, Error> {
    identifier(id, 128)?;
    let (c, _) = request(db, id)?;
    live(db, &c, now)?;
    let (digest, expires, state, scope): (String, u64, String, bool) = db.query_row(
        "SELECT digest,expires_at,state,scope_key IS NOT NULL FROM owner_approvals WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    if state != "pending" || expires <= now {
        return Err(Error::RunnerAuthority);
    }
    Ok(ApprovalIntakeTarget {
        authority: room_authority(db, &c.binding.engagement)?,
        device_id: c.binding.device,
        room_generation: c.binding.room_generation,
        binding_generation: c.binding.generation,
        request_id: id.into(),
        request_digest: digest,
        expires_at: expires,
        reusable_scope: scope,
    })
}
fn intake_receipt_key(input: &ApprovalVerdictInput) -> Result<(String, String), Error> {
    let v = &input.verdict;
    let t = &input.target;
    if input.source_digest.len() != 64
        || !input
            .source_digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || v.request_id != t.request_id
        || v.request_digest != t.request_digest
        || v.binding_generation != t.binding_generation
        || v.server_name != t.authority.server_name
        || v.room_id != t.authority.room_id
        || v.sender_mxid != t.authority.owner_mxid
        || !v.encrypted
        || (!t.reusable_scope && matches!(v.choice, ApprovalChoice::Task | ApprovalChoice::Always))
    {
        return Err(Error::RunnerAuthority);
    }
    identifier(&v.request_id, 128)?;
    hagency_core::replies::matrix_event(&v.event_id)?;
    serde_json::to_writer(EncodedLimit(48 * 1024), input).map_err(|_| Error::Capacity)?;
    Ok((
        canonical::digest(&json!([v.server_name, v.event_id]))?,
        canonical::digest(&json!(input))?,
    ))
}
impl DomainRepository {
    pub fn approval_room_authority(
        &self,
        engagement: &str,
    ) -> Result<ApprovalRoomAuthority, Error> {
        room_authority(&self.db, engagement)
    }
    pub fn approval_room_capture(
        &self,
        authority: &ApprovalRoomAuthority,
    ) -> Result<Option<ApprovalRoomCapture>, Error> {
        if room_authority(&self.db, &authority.engagement_id)? != *authority {
            return Err(Error::RunnerAuthority);
        }
        self.db
            .query_row(
                "SELECT digest,available,device_id,generation FROM approval_rooms WHERE server_name=?1 AND room_id=?2",
                params![authority.server_name, authority.room_id],
                |r| {
                    Ok(ApprovalRoomCapture {
                        server_name: authority.server_name.clone(),
                        room_id: authority.room_id.clone(),
                        digest: r.get(0)?,
                        available:r.get(1)?,device_id:r.get(2)?,generation:r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Error::from)
    }
    /// Fence only captured shared state or the exact attempted new observation.
    /// This covers a committed positive update whose caller lost its response.
    pub fn fence_approval_room(
        &mut self,
        authority: &ApprovalRoomAuthority,
        device: &str,
        generation: u64,
        prior: Option<&ApprovalRoomCapture>,
    ) -> Result<(), Error> {
        text(device, 255)?;
        hagency_core::replies::generation(generation)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(prior) = prior {
            if prior.server_name != authority.server_name || prior.room_id != authority.room_id {
                return Err(Error::RunnerAuthority);
            }
            tx.execute("UPDATE approval_rooms SET available=0 WHERE server_name=?1 AND room_id=?2 AND digest=?3",params![prior.server_name,prior.room_id,prior.digest])?;
        }
        tx.execute("UPDATE approval_rooms SET available=0 WHERE server_name=?1 AND room_id=?2 AND fleet_id=?3 AND project_id=?4 AND registration_generation=?5 AND owner_mxid=?6 AND bot_mxid=?7 AND device_id=?8 AND generation=?9",params![authority.server_name,authority.room_id,authority.fleet_id,authority.project_id,authority.registration_generation,authority.owner_mxid,authority.bot_mxid,device,generation])?;
        tx.commit()?;
        Ok(())
    }
    pub fn approval_intake_target(
        &self,
        id: &str,
        now: u64,
    ) -> Result<ApprovalIntakeTarget, Error> {
        clock(now)?;
        intake_target(&self.db, id, now)
    }
    /// Historical exact acceptance only: never current authority or a new grant.
    pub fn approval_verdict_receipt(
        &self,
        input: &ApprovalVerdictInput,
    ) -> Result<Option<ApprovalSummary>, Error> {
        let (source, digest) = intake_receipt_key(input)?;
        let prior: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT digest,request_id FROM approval_verdict_receipts WHERE source_key=?1",
                [source],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match prior {
            None => Ok(None),
            Some((old, request)) if old == digest && request == input.verdict.request_id => {
                Ok(Some(summary(&self.db, &request)?))
            }
            Some(_) => Err(Error::Conflict),
        }
    }
    pub fn admit_approval_verdict(
        &mut self,
        input: &ApprovalVerdictInput,
        now: u64,
    ) -> Result<ApprovalSummary, Error> {
        self.admit_approval_verdict_clock(input, || Ok(now))
    }

    pub(crate) fn admit_approval_verdict_clock(
        &mut self,
        input: &ApprovalVerdictInput,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<ApprovalSummary, Error> {
        let (_, digest) = intake_receipt_key(input)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?; // The original writer queue and SQLite lock waits have ended.
        clock(now)?;
        if intake_target(&tx, &input.verdict.request_id, now)? != input.target {
            return Err(Error::RunnerAuthority);
        }
        let result = decide_verdict(&tx, &input.verdict, now, Some(&digest))?;
        tx.commit()?;
        Ok(result)
    }
}

fn consume(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    id: &str,
    now: u64,
) -> Result<ApprovalApplication, Error> {
    let (c, r) = request(tx, id)?;
    authorize(tx, cap, &c, now)?;
    let (state, choice, expires, grant): (String, Option<String>, u64, Option<String>) = tx
        .query_row(
            "SELECT state,choice,expires_at,grant_id FROM owner_approvals WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
    if !["pending", "decided"].contains(&state.as_str()) || (state == "pending" && expires > now) {
        return Err(Error::RunnerAuthority);
    }
    let choice: Option<ApprovalChoice> = choice.map(|s| serde_json::from_str(&s)).transpose()?;
    let valid_grant = if let Some(grant) = grant {
        tx.query_row("SELECT EXISTS(SELECT 1 FROM approval_grants WHERE id=?1 AND revoked=0 AND context_key=?2 AND (mode='always' OR (task_id=?3 AND task_epoch=?4)))",params![grant,grant_context(&c)?,c.task,c.epoch],|r|r.get::<_,bool>(0))?
    } else {
        true
    };
    let allow =
        expires > now && valid_grant && choice.is_some_and(|choice| choice != ApprovalChoice::Deny);
    let digest = canonical::digest(&json!([id, c, r, allow]))?;
    let application = ApprovalApplication {
        id: id.into(),
        digest,
        connection_id: c.connection,
        upstream_id: r.upstream,
        thread_id: c.thread,
        turn_id: c.turn,
        item_id: r.item,
        allow,
    };
    tx.execute(
        "UPDATE owner_approvals SET state='applying',application=?2 WHERE id=?1",
        params![id, serialize(&application)?],
    )?;
    Ok(application)
}

fn context_input(input: &HostApprovalContext) -> Result<String, Error> {
    input.validate()?;
    if input.yolo {
        return Err(Error::RunnerAuthority);
    }
    let workspace = if input.windows_paths {
        PathFlavor::Windows
    } else {
        PathFlavor::Posix
    }
    .normalize(&input.workspace)
    .ok_or(Error::RunnerAuthority)?;
    Ok(workspace)
}

fn bind_context(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    input: &HostApprovalContext,
    workspace: String,
    now: u64,
) -> Result<Context, Error> {
    let d = execution::authorize(tx, cap, now, &["started", "parked"])?;
    if d.report_task.is_some() {
        return Err(Error::RunnerAuthority);
    }
    let task = execution::task(tx, d.task_id.as_deref().ok_or(Error::RunnerAuthority)?)?;
    let route = matrix_routes::route(tx, &d.session_id)?;
    let binding = binding(tx, &route.engagement_id)?;
    let c = Context {
        id: input.id.clone(),
        dispatch: cap.dispatch_id.clone(),
        fence: cap.fence,
        task: task.id,
        epoch: task.execution_epoch,
        route,
        binding,
        connection: input.connection_id.clone(),
        thread: input.thread_id.clone(),
        turn: input.turn_id.clone(),
        resource: input.workspace_resource.clone(),
        workspace,
        windows: input.windows_paths,
        environment: input.environment_id.clone(),
        may_write: input.may_write,
    };
    live(tx, &c, now)?;
    let digest = canonical::digest(&json!(c))?;
    let old: Option<String> = tx
        .query_row(
            "SELECT digest FROM approval_contexts WHERE id=?1",
            [&c.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = old {
        if old != digest {
            return Err(Error::Conflict);
        }
        return Ok(c);
    }
    bounded_row(tx, "approval_contexts", "id", &c.id, 100_000)?;
    tx.execute("INSERT INTO approval_contexts(id,dispatch_id,fence,engagement_id,digest,config) VALUES(?1,?2,?3,?4,?5,?6)",params![c.id,c.dispatch,c.fence,c.binding.engagement,digest,serialize(&c)?])?;
    Ok(c)
}
