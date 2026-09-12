//! Allocation and canonical task/dispatch state share this one database owner.
use crate::{Error, database};
use hagency_core::{
    InvalidInput, JSON_SAFE_MAX,
    allocation::{self, Budget},
    authority::{Registration, VerifiedRequest},
    canonical,
    ceiling::{self as ceiling_wording, SpendContext},
    project::{
        self, CatalogResource, CleanupState, ConfiguredResource, Engagement, EngagementState,
        Resource, Seat,
    },
    qualification,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs::File, path::Path};
pub(crate) mod accounts;
mod approvals;
pub use approvals::card::PrivateApprovalCard;
mod attachments;
mod catalog_publication;
pub use attachments::AttachmentTicket;
pub use catalog_publication::PublishedCatalog;
mod conversation_lifecycle;
mod conversations;
mod execution;
mod graphs;
mod matrix_routes;
mod messages;
mod notice_custody;
mod owned_completion;
mod owned_dispatch;
pub use owned_completion::OwnedCompletion;
pub use owned_dispatch::{
    OwnedClaimProfile, OwnedClaimRoom, OwnedDispatchScope, OwnedFailure, OwnedObservation,
};
pub(crate) mod file_delivery;
mod peers;
pub(crate) mod received_files;
mod replies;
pub(crate) mod resource_configuration;
pub(crate) mod resource_publication;
mod task_intents;
pub(crate) mod uploads;
mod usage;
pub use uploads::{UploadAdmission, UploadClaim, UploadIdentity, UploadPreparation, UploadSend};
mod verified_ingress;
pub use usage::{
    CeilingReport, KnownTokens, MAX_ENGAGEMENT_USAGE_PERIODS, MAX_ENGAGEMENT_USAGE_SOURCES,
    MAX_SOURCE_USAGE_RECEIPTS, MAX_USAGE_PERIODS, MAX_USAGE_RECEIPTS, MAX_USAGE_SOURCES,
    SourceUsage, UsageCeiling, UsageEvidence, UsagePeriod, UsagePeriodKind, UsageReceipt,
    UsageReport, UsageSource, UsageSummary,
};

pub struct DomainRepository {
    db: Connection,
    accounts: accounts::Registry,
    _ownership: File,
    approval_owner: std::sync::Arc<()>,
}
impl DomainRepository {
    pub(super) fn drop_observed(self, probe: &std::sync::Arc<crate::shutdown::Probe>) {
        use crate::shutdown::{Phase, SqliteCloseScope};
        // Match the declared field drop order. Ownership is still a local
        // guard, so unwinding from connection destruction also releases it.
        let Self {
            db,
            _ownership,
            accounts,
            ..
        } = self;
        drop(accounts);
        let close_scope = SqliteCloseScope::install(&db, probe);
        probe.observe_domain_writer();
        probe.mark(Phase::ConnectionDropStarted);
        drop(db);
        probe.mark(Phase::ConnectionDropFinished);
        drop(close_scope);
        probe.mark(Phase::OwnershipDropStarted);
        drop(_ownership);
        probe.mark(Phase::OwnershipDropFinished);
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectState {
    Pending,
    Started,
    Uncertain,
    Complete,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    pub id: String,
    pub engagement_id: String,
    pub kind: String,
    pub state: EffectState,
    pub fence: u64,
    /// Adapter-only. Never project this into the operator console or catalog.
    pub payload: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EffectOutcome {
    /// The adapter observed the exact intended effect and supplies a stable receipt.
    Applied {
        receipt: String,
    },
    /// Definitive observation that nothing was applied. A timeout is not this case.
    NotApplied {
        receipt: String,
    },
    Unknown,
}
fn serialize<T: Serialize>(value: &T) -> Result<String, Error> {
    Ok(serde_json::to_string(value)?)
}
fn state_name(state: &EngagementState) -> &'static str {
    match state {
        EngagementState::Pending => "pending",
        EngagementState::Reserved => "reserved",
        EngagementState::Active => "active",
        EngagementState::Rejected => "rejected",
        EngagementState::Revoked => "revoked",
        EngagementState::Failed => "failed",
    }
}
fn read_engagement(db: &Connection, id: &str) -> Result<Engagement, Error> {
    let value: String = db
        .query_row(
            "SELECT projection FROM engagements WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(&value)?)
}
fn write_engagement(tx: &Transaction<'_>, value: &Engagement) -> Result<(), Error> {
    tx.execute(
        "UPDATE engagements SET state=?2,projection=?3 WHERE id=?1",
        params![value.id, state_name(&value.state), serialize(value)?],
    )?;
    Ok(())
}
fn read_resource(db: &Connection, id: &str) -> Result<Resource, Error> {
    let value: String = db
        .query_row("SELECT config FROM resources WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(&value)?)
}
fn role_available(db: &Connection, role: &str, fleet: Option<&str>) -> Result<bool, Error> {
    qualification::check_role(role)?;
    let publication: Option<bool> = db
        .query_row(
            "SELECT published FROM role_publications WHERE role=?1",
            [role],
            |r| r.get(0),
        )
        .optional()?;
    if publication == Some(false) {
        return Ok(false);
    }
    if !qualification::cross_family(role) {
        return Ok(true);
    }
    let mut query=db.prepare("SELECT json_extract(f.payload,'$.resource') FROM engagements e JOIN registrations r ON r.fleet_id=e.fleet_id JOIN effects f ON f.engagement_id=e.id AND f.kind='provision' WHERE e.state='active' AND e.generation=r.generation AND (?1 IS NULL OR e.fleet_id=?1)")?;
    let rows = query.query_map([fleet], |r| r.get::<_, String>(0))?;
    let mut families = std::collections::BTreeSet::new();
    let need =
        qualification::default_tier(role).ok_or(hagency_core::InvalidInput("unknown role"))?;
    for row in rows {
        let resource: Resource = serde_json::from_str(&row?)?;
        let (tier, family) = qualification::model(&resource.profile());
        if tier.is_some_and(|got| got >= need)
            && let Some(family) = family
        {
            families.insert(family);
        }
        if families.len() >= 2 {
            return Ok(true);
        }
    }
    Ok(false)
}
fn authority(db: &Connection, proof: &VerifiedRequest, now: u64) -> Result<(), Error> {
    proof.check_fresh(now)?;
    let value: String = db
        .query_row(
            "SELECT config FROM registrations WHERE fleet_id=?1",
            [&proof.request().fleet_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    if serde_json::from_str::<Registration>(&value)? != *proof.registration() {
        return Err(Error::Generation);
    }
    Ok(())
}

fn project_authority(db: &Connection, proof: &VerifiedRequest) -> Result<(), Error> {
    let request = proof.request();
    let existing: Option<(u64,String,String,String)> = db.query_row("SELECT generation,room_id,owner_mxid,owner_room_id FROM projects WHERE fleet_id=?1 AND id=?2",
        params![request.fleet_id,request.target_project_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    if let Some((generation, room, owner, owner_room)) = existing
        && (generation != proof.registration().generation
            || room != request.target_room_id
            || owner != request.owner_mxid
            || owner_room != request.owner_dm_room_id)
    {
        return Err(Error::Generation);
    }
    Ok(())
}

fn bounded_row(
    db: &Connection,
    table: &'static str,
    key: &str,
    value: &str,
    limit: i64,
) -> Result<(), Error> {
    // Callers supply only literal table/column names. Values remain SQL parameters.
    let exists: bool = db.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {key}=?1)"),
        [value],
        |r| r.get(0),
    )?;
    if !exists {
        let count: i64 =
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        if count >= limit {
            return Err(Error::Capacity);
        }
    }
    Ok(())
}
fn decision_digest(kind: &str, engagement: &str) -> Result<String, Error> {
    Ok(canonical::digest(&json!([kind, engagement]))?)
}
fn replay_decision(db: &Connection, id: &str, digest: &str) -> Result<Option<Engagement>, Error> {
    project::identifier(id, 128)?;
    let prior: Option<(String, String)> = db
        .query_row(
            "SELECT digest,result FROM decisions WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    prior
        .map(|(old, result)| {
            if old != digest {
                Err(Error::Conflict)
            } else {
                Ok(serde_json::from_str(&result)?)
            }
        })
        .transpose()
}
fn record_decision(
    tx: &Transaction<'_>,
    id: &str,
    digest: &str,
    value: &Engagement,
) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO decisions(id,digest,result) VALUES(?1,?2,?3)",
        params![id, digest, serialize(value)?],
    )?;
    Ok(())
}
fn budget(
    db: &Connection,
    resource: &Resource,
    exclude_engagement_id: Option<&str>,
    for_auto_join: bool,
) -> Result<Budget, Error> {
    let declaration: Option<String> = db
        .query_row(
            "SELECT config FROM seats WHERE id=?1",
            [&resource.seat_id],
            |r| r.get(0),
        )
        .optional()?;
    let declaration = declaration
        .map(|s| serde_json::from_str::<Seat>(&s))
        .transpose()?
        .and_then(|s| s.declaration);
    // Aggregate in SQLite, not by cloning or scanning the whole lifetime store in Rust.
    // SQLite SUM fails rather than wrapping; Tokens also enforces JSON-safe precision.
    let mut statement = db.prepare("SELECT preset_id,seat_id,SUM(tokens) FROM engagements WHERE state IN ('reserved','active') AND (preset_id=?1 OR seat_id=?2) GROUP BY preset_id,seat_id")?;
    let rows = statement.query_map(params![resource.preset_id, resource.seat_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u64>(2)?,
        ))
    })?;
    let mut commitments = Vec::new();
    for row in rows {
        let (preset, seat, tokens) = row?;
        commitments.push(allocation::Commitment {
            id: format!("{}", commitments.len()),
            preset_id: Some(preset),
            seat_id: Some(seat),
            allocated_tokens: Some(tokens.try_into()?),
            state: "active".into(),
            fulfillment: None,
        });
    }
    Ok(allocation::resource_budget(&allocation::Input {
        preset: allocation::Preset {
            id: resource.preset_id.clone(),
            ceiling: resource.ceiling.clone(),
        },
        seat_id: resource.seat_id.clone(),
        declaration,
        commitments,
        exclude_engagement_id: exclude_engagement_id.map(str::to_owned),
        for_auto_join,
    })?)
}

impl DomainRepository {
    pub fn set_role_publication(&mut self, role: &str, published: bool) -> Result<(), Error> {
        qualification::check_role(role)?;
        self.db.execute("INSERT INTO role_publications(role,published) VALUES(?1,?2) ON CONFLICT(role) DO UPDATE SET published=excluded.published",params![role,published])?;
        Ok(())
    }
    pub fn role_publications(&self) -> Result<Vec<Value>, Error> {
        let mut query = self
            .db
            .prepare("SELECT config FROM resources WHERE json_extract(config,'$.published')=1")?;
        let resources: Vec<Resource> = query
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<Result<_, Error>>()?;
        qualification::roles().map(|role| {
            let explicit:Option<bool>=self.db.query_row("SELECT published FROM role_publications WHERE role=?1",[role],|r|r.get(0)).optional()?;
            let available=role_available(&self.db,role,None)? && resources.iter().any(|r|r.qualifies(role));
            Ok(json!({"role":role,"explicitPublication":explicit,"available":available,"crossFamily":qualification::cross_family(role),"defaultTier":qualification::default_tier(role)}))
        }).collect()
    }
    pub fn open(directory: &Path) -> Result<Self, Error> {
        let mut database = database::open(
            directory,
            database::Schema {
                name: "domain.sqlite3",
                lock: "domain.lock",
                application_id: 0x48414732,
                version: 23,
                migrations: &[
                    (2, include_str!("migrations/002-role-publication.sql")),
                    (3, include_str!("migrations/003-task-dispatch.sql")),
                    (4, include_str!("migrations/004-message-inputs.sql")),
                    (5, include_str!("migrations/005-task-intents.sql")),
                    (6, include_str!("migrations/006-internal-conversations.sql")),
                    (7, include_str!("migrations/007-peer-inputs.sql")),
                    (8, include_str!("migrations/008-recovery-reports.sql")),
                    (9, include_str!("migrations/009-conversation-lifecycle.sql")),
                    (10, include_str!("migrations/010-task-graphs.sql")),
                    (11, include_str!("migrations/011-final-replies.sql")),
                    (12, include_str!("migrations/012-verified-ingress.sql")),
                    (13, include_str!("migrations/013-owner-approvals.sql")),
                    (14, include_str!("migrations/014-notice-custody.sql")),
                    (15, include_str!("migrations/015-matrix-transport.sql")),
                    (
                        16,
                        include_str!("migrations/016-owned-task-completions.sql"),
                    ),
                    (17, include_str!("migrations/017-usage-ledger.sql")),
                    (18, include_str!("migrations/018-attachment-visibility.sql")),
                    (19, include_str!("migrations/019-file-uploads.sql")),
                    (20, include_str!("migrations/020-file-deliveries.sql")),
                    (21, include_str!("migrations/021-received-files.sql")),
                    (22, include_str!("migrations/022-approval-responses.sql")),
                    (23, include_str!("migrations/023-managed-accounts.sql")),
                ],
                sql: include_str!("domain.sql"),
                verify: &[
                    "SELECT k.secret,k.deployment,k.root_identity,a.id,a.ordinal,a.generation,a.state,a.namespace_identity,a.identity_tuple,a.seat_id,r.preset_id,r.account_id,r.binding_generation FROM account_identity_key k CROSS JOIN managed_accounts a CROSS JOIN resource_accounts r LIMIT 0",
                    "SELECT request_id,context_id,capability_digest,decision_digest,state,write_accepted,authorized_at,response_started_at FROM approval_responses LIMIT 0",
                    "SELECT id,capability_digest,event_id,workspace_id,binding,binding_digest,byte_limit,facts,state,failure FROM received_files LIMIT 0",
                    "SELECT id,upload_id,dispatch_id,call_id,request,request_hash,captured,event_state,claim_fence,claim_hash,claim_until,transaction_id,publication,cancel_requested,failure,acceptance,created_at,updated_at FROM file_deliveries LIMIT 0",
                    "SELECT id,dispatch_id,call_id,request_digest,capability_digest,scope_fingerprint,route,preparation_hash,stage,stage_state,upload_state,claim_fence,claim_hash,claim_until,cancel_requested,outcome_unknown,acceptance,created_at,updated_at FROM file_uploads LIMIT 0",
                    "SELECT a.digest,a.content_digest,a.metadata,a.sdk_identity,a.manifest_id,v.projection_sequence,w.source_cutoff,w.projection_cutoff FROM matrix_attachments a CROSS JOIN session_attachment_visibility v CROSS JOIN dispatch_attachment_windows w LIMIT 0",
                    "SELECT id,dispatch_id,fence,engagement_id,identity_digest,framework,attribution,high_water,latest_counts,latest_observation,latest_incomplete,latest_regressed,historical_incomplete,regressions,observations,observed_at FROM usage_sources LIMIT 0",
                    "SELECT source_id,call_id,digest,observation,response FROM usage_receipts LIMIT 0",
                    "SELECT engagement_id,granularity,period_key,observed_growth,known_growth,incomplete,observations FROM usage_periods LIMIT 0",
                    "SELECT singleton,observed_at FROM usage_clock LIMIT 0",
                    "SELECT id,fingerprint,deadline,reply_id FROM owned_task_completions LIMIT 0",
                    "SELECT available,invalidation FROM matrix_transports LIMIT 0",
                    "SELECT n.send_fence,n.cancel_requested,n.task_epoch,n.source_event_id,i.digest,i.observation FROM task_notices n CROSS JOIN notice_send_inspections i LIMIT 0",
                    "SELECT r.available,r.config,b.incarnation,c.digest,a.state,g.context_key,v.digest FROM approval_rooms r CROSS JOIN approval_bindings b CROSS JOIN approval_contexts c CROSS JOIN owner_approvals a CROSS JOIN approval_grants g CROSS JOIN approval_verdict_receipts v LIMIT 0",
                    "SELECT engagement_id FROM current_approval_bindings LIMIT 0",
                    "SELECT e.id,e.context,e.evidence,e.projection,f.payload,p.owner_mxid,r.config,s.config,d.result,g.config,rp.role,rt.config,rd.capability_hash,ro.task,mi.digest,si.wake,di.message_sequence,ti.anchor_event_id,tn.delivery,tf.dispatch_id,tin.message_sequence,tir.digest,ic.digest,ip.session_id,pm.digest,psi.wake,pdi.message_sequence,lpi.session_id,tdpr.dispatch_id,drr.task_id,crr.execution_epoch,co.digest,ds.fence,ud.id,ic.revision FROM engagements e LEFT JOIN effects f ON f.engagement_id=e.id CROSS JOIN projects p CROSS JOIN resources r CROSS JOIN seats s CROSS JOIN decisions d CROSS JOIN registrations g CROSS JOIN role_publications rp CROSS JOIN canonical_tasks rt CROSS JOIN runner_dispatches rd CROSS JOIN task_outbox ro CROSS JOIN admitted_messages mi CROSS JOIN session_inputs si CROSS JOIN dispatch_inputs di CROSS JOIN task_intents ti CROSS JOIN task_notices tn CROSS JOIN task_followup_ready tf CROSS JOIN task_inputs tin CROSS JOIN task_input_receipts tir CROSS JOIN internal_conversations ic CROSS JOIN internal_participants ip CROSS JOIN peer_messages pm CROSS JOIN peer_session_inputs psi CROSS JOIN peer_dispatch_inputs pdi CROSS JOIN live_peer_inputs lpi CROSS JOIN task_dispatch_input_ready tdpr CROSS JOIN dispatch_recovery_reports drr CROSS JOIN current_recovery_reports crr CROSS JOIN conversation_operations co CROSS JOIN dispatch_stops ds CROSS JOIN unresolved_dispatches ud CROSS JOIN runner_sessions sc INDEXED BY canonical_runner_session LIMIT 0",
                    "SELECT tg.config,gn.state,gn.result_value,gc.digest,gd.digest FROM task_graphs tg CROSS JOIN graph_nodes gn CROSS JOIN graph_commands gc CROSS JOIN graph_dependencies gd LIMIT 0",
                    "SELECT id FROM current_graph_scopes LIMIT 0",
                    "SELECT dispatch_id FROM graph_dispatch_ready LIMIT 0",
                    "SELECT dispatch_id FROM graph_dispatch_scope LIMIT 0",
                    "SELECT message_sequence FROM admissible_dispatch_peer_inputs LIMIT 0",
                    "SELECT t.device_id,s.privacy,r.retired,f.digest,c.reply_id FROM matrix_transports t CROSS JOIN matrix_room_scopes s CROSS JOIN matrix_session_routes r CROSS JOIN final_replies f CROSS JOIN final_reply_calls c LIMIT 0",
                    "SELECT session_id FROM current_matrix_routes LIMIT 0",
                    "SELECT s.joined,s.invite_only,s.available,s.invalidation,m.transport_generation,f.cancel_requested,i.digest FROM matrix_room_scopes s CROSS JOIN matrix_room_memberships m CROSS JOIN final_replies f CROSS JOIN final_reply_inspections i LIMIT 0",
                    "SELECT id FROM current_final_replies LIMIT 0",
                    "SELECT e.scope_digest,e.config,r.digest,s.ingress_since,s.parent_session_id,t.observed_at,room.visibility_since,si.config,ti.config,ti.wake,n.verified_route,n.content_digest FROM matrix_ingress_events e CROSS JOIN verified_task_requests r CROSS JOIN matrix_session_routes s CROSS JOIN matrix_transports t CROSS JOIN matrix_room_scopes room CROSS JOIN session_inputs si CROSS JOIN task_inputs ti CROSS JOIN task_notices n LIMIT 0",
                ],
            },
        )?;
        let transport_trigger: bool = database.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='matrix_transport_retire_approvals' AND tbl_name='matrix_transports')", [], |r|r.get(0)).map_err(|_|Error::Schema)?;
        if !transport_trigger {
            return Err(Error::Schema);
        }
        // A previous owner died after an intent became externally executable. Inspection,
        // not automatically repeating that effect, is the only safe default.
        let tx = database
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE engagements SET projection=json_set(projection,'$.cleanup','uncertain') WHERE id IN (SELECT engagement_id FROM effects WHERE kind='retire' AND state='started')",[])?;
        tx.execute(
            "UPDATE effects SET state='uncertain' WHERE state='started'",
            [],
        )?;
        graphs::reconcile(&tx, graphs::now_ms()?)?;
        replies::reconcile(&tx, graphs::now_ms()?, true)?;
        notice_custody::reconcile(&tx, graphs::now_ms()?, true)?;
        execution::recover_all(&tx)?;
        approvals::recover(&tx)?;
        tx.execute("UPDATE approval_responses SET state='outcome_unknown' WHERE state IN ('authorized','response_may_send')", [])?;
        tx.execute("UPDATE received_files SET state='outcome_unknown',failure='outcome_unknown' WHERE state IN ('reserved','write_possible')", [])?;
        tx.execute(
            "UPDATE managed_accounts SET state='uncertain' WHERE state='preparing'",
            [],
        )?;
        tx.commit()?;
        let accounts = accounts::Registry::open(&database.connection, directory)?;
        Ok(Self {
            accounts,
            db: database.connection,
            _ownership: database.ownership,
            approval_owner: std::sync::Arc::new(()),
        })
    }
    pub fn register(&mut self, registration: &Registration) -> Result<(), Error> {
        registration.validate()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        bounded_row(
            &tx,
            "registrations",
            "fleet_id",
            &registration.fleet_id,
            1024,
        )?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT config FROM registrations WHERE fleet_id=?1",
                [&registration.fleet_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            let old: Registration = serde_json::from_str(&previous)?;
            if old == *registration {
                return Ok(());
            }
            if registration.generation <= old.generation {
                return Err(Error::Generation);
            }
            // Rotation fences execution immediately. Existing allocations stay observable
            // and reserved until explicit revoke/reconciliation; rotation cannot erase spend.
        }
        tx.execute("INSERT INTO registrations(fleet_id,generation,config) VALUES(?1,?2,?3) ON CONFLICT(fleet_id) DO UPDATE SET generation=excluded.generation,config=excluded.config",
            params![registration.fleet_id,registration.generation,serialize(registration)?])?;
        graphs::reconcile(&tx, graphs::now_ms()?)?;
        matrix_routes::reconcile(&tx, graphs::now_ms()?)?;
        tx.commit()?;
        Ok(())
    }
    pub fn put_resource(&mut self, resource: &Resource) -> Result<CatalogResource, Error> {
        self.edit_resource(resource, Some(resource.published))
    }
    pub fn edit_resource(
        &mut self,
        resource: &Resource,
        publication: Option<bool>,
    ) -> Result<CatalogResource, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let resource = prepare_resource_write(&tx, resource, publication, false)?;
        self.accounts.check_resource(&tx, &resource)?;
        write_resource_configuration(&tx, &resource, false)?;
        tx.commit()?;
        Ok(resource.catalog())
    }
    pub fn put_seat(&mut self, seat: &Seat) -> Result<(), Error> {
        seat.validate()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if seat.id.starts_with("seat_native_") {
            return Err(Error::Unqualified);
        }
        bounded_row(&tx, "seats", "id", &seat.id, 2048)?;
        tx.execute("INSERT INTO seats(id,config) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET config=excluded.config", params![seat.id,serialize(seat)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn catalog(&self, after: &str, limit: usize) -> Result<Vec<CatalogResource>, Error> {
        self.catalog_for(None, after, limit)
    }
    pub fn catalog_for(
        &self,
        fleet: Option<&str>,
        after: &str,
        limit: usize,
    ) -> Result<Vec<CatalogResource>, Error> {
        if limit == 0 || limit > 100 {
            return Err(InvalidInput("page limit must be 1..100").into());
        }
        // At most 2,048 configurations exist. Iterate until enough *qualified*
        // rows are found; filtering a short SQL page would incorrectly hide later IDs.
        let mut query = self.db.prepare("SELECT config FROM resources WHERE id>?1 AND json_extract(config,'$.published')=1 ORDER BY id")?;
        let rows = query.query_map([after], |r| r.get::<_, String>(0))?;
        let allowed: std::collections::BTreeSet<_> = qualification::roles()
            .filter_map(|role| match role_available(&self.db, role, fleet) {
                Ok(true) => Some(Ok(role)),
                Ok(false) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<_, _>>()?;
        let mut output = Vec::new();
        for row in rows {
            let resource: Resource = serde_json::from_str(&row?)?;
            match self.accounts.check_resource(&self.db, &resource) {
                Ok(()) => {}
                Err(Error::LocalAuthority | Error::Unqualified) => continue,
                Err(error) => return Err(error),
            }
            let mut public = resource.catalog();
            public.roles.retain(|role| allowed.contains(role.as_str()));
            if !public.roles.is_empty() {
                output.push(public);
            }
            if output.len() == limit {
                break;
            }
        }
        Ok(output)
    }
    pub fn resource_configuration(&self, id: &str) -> Result<Resource, Error> {
        read_resource(&self.db, id)
    }
    pub fn resource_configurations(
        &self,
        after: &str,
        limit: usize,
    ) -> Result<Vec<ConfiguredResource>, Error> {
        if limit == 0 || limit > 100 {
            return Err(InvalidInput("page limit must be 1..100").into());
        }
        let mut query = self
            .db
            .prepare("SELECT id,config FROM resources WHERE id>?1 ORDER BY id LIMIT ?2")?;
        query
            .query_map(params![after, limit as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .map(|row| {
                let (id, config) = row?;
                let mut config: Resource = serde_json::from_str(&config)?;
                config.roles = config.eligible_roles();
                Ok(ConfiguredResource { id, config })
            })
            .collect()
    }
    pub fn seats(&self, after: &str, limit: usize) -> Result<Vec<Seat>, Error> {
        if limit == 0 || limit > 100 {
            return Err(InvalidInput("page limit must be 1..100").into());
        }
        let mut query = self
            .db
            .prepare("SELECT config FROM seats WHERE id>?1 ORDER BY id LIMIT ?2")?;
        query
            .query_map(params![after, limit as i64], |r| r.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect()
    }
    pub fn engagements(&self, after: &str, limit: usize) -> Result<Vec<Engagement>, Error> {
        if limit == 0 || limit > 100 {
            return Err(InvalidInput("page limit must be 1..100").into());
        }
        let mut query = self
            .db
            .prepare("SELECT projection FROM engagements WHERE id>?1 ORDER BY id LIMIT ?2")?;
        query
            .query_map(params![after, limit as i64], |r| r.get::<_, String>(0))?
            .map(|s| Ok(serde_json::from_str(&s?)?))
            .collect()
    }
    pub fn get(&self, id: &str) -> Result<Engagement, Error> {
        read_engagement(&self.db, id)
    }
    pub fn resource_budget(&self, id: &str) -> Result<Budget, Error> {
        budget(&self.db, &read_resource(&self.db, id)?, None, false)
    }

    pub fn admit(&mut self, proof: &VerifiedRequest, now: u64) -> Result<Engagement, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        authority(&tx, proof, now)?;
        project_authority(&tx, proof)?;
        let request = proof.request();
        let id = request.engagement_id()?;
        let digest = request.digest()?;
        let previous: Option<String> = tx
            .query_row("SELECT digest FROM engagements WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(old) = previous {
            if old != digest {
                return Err(Error::Conflict);
            }
            return read_engagement(&tx, &id); // Exact replay survives withdrawal.
        }
        bounded_row(&tx, "engagements", "id", &id, 10_000)?;
        let resource = read_resource(&tx, &request.agent_definition.resource_id)?;
        self.accounts.check_resource(&tx, &resource)?;
        if !resource.qualifies(&request.role)
            || !role_available(&tx, &request.role, Some(&request.fleet_id))?
        {
            return Err(Error::Unqualified);
        }
        let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM engagements WHERE fleet_id=?1 AND project_id=?2 AND name=?3 AND state IN ('pending','reserved','active'))",
            params![request.fleet_id,request.target_project_id,request.agent_definition.name.as_str()], |r| r.get(0))?;
        if collision {
            return Err(Error::Conflict);
        }
        let value = Engagement {
            id,
            request_id: request.request_id.clone(),
            project_id: request.target_project_id.clone(),
            project_room_id: request.target_room_id.clone(),
            project_name: proof.project_name().map(str::to_owned),
            agent_name: request.agent_definition.name.clone(),
            runtime_name: request.agent_definition.runtime_name(
                &request.fleet_id,
                &request.target_project_id,
                &request.request_id,
            )?,
            resource_id: resource.id(),
            role: request.role.clone(),
            requested_tokens: request.requested_tokens,
            state: EngagementState::Pending,
            cleanup: CleanupState::NotRequired,
        };
        tx.execute("INSERT INTO projects(fleet_id,id,generation,room_id,owner_mxid,owner_room_id) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(fleet_id,id) DO NOTHING",
            params![request.fleet_id,request.target_project_id,proof.registration().generation,request.target_room_id,request.owner_mxid,request.owner_dm_room_id])?;
        // The request row itself is the domain inbox marker: same transaction and unique key.
        tx.execute("INSERT INTO engagements(id,fleet_id,generation,request_id,digest,context,evidence,project_id,name,resource_id,tokens,state,projection) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'pending',?12)",
            params![value.id,request.fleet_id,proof.registration().generation,request.request_id,digest,serialize(request)?,serialize(proof.audit())?,request.target_project_id,request.agent_definition.name.as_str(),resource.id(),u64::from(request.requested_tokens),serialize(&value)?])?;
        tx.commit()?;
        Ok(value)
    }
    /// Only an authenticated operator command calls this, after the Matrix adapter
    /// re-verifies current owner/room authority. No production HTTP route exists yet.
    pub fn approve(
        &mut self,
        command_id: &str,
        proof: &VerifiedRequest,
        now: u64,
    ) -> Result<Engagement, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        authority(&tx, proof, now)?;
        project_authority(&tx, proof)?;
        let request = proof.request();
        let id = request.engagement_id()?;
        let (stored_digest, generation): (String, u64) = tx
            .query_row(
                "SELECT digest,generation FROM engagements WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if stored_digest != request.digest()? {
            return Err(Error::Conflict);
        }
        if generation != proof.registration().generation {
            return Err(Error::Generation);
        }
        let digest = decision_digest("approve", &id)?;
        if let Some(value) = replay_decision(&tx, command_id, &digest)? {
            return Ok(value);
        }
        let mut value = read_engagement(&tx, &id)?;
        if value.state != EngagementState::Pending {
            return Err(Error::State);
        }
        let resource = read_resource(&tx, &value.resource_id)?;
        self.accounts.check_resource(&tx, &resource)?;
        if !resource.qualifies(&value.role)
            || !role_available(&tx, &value.role, Some(&request.fleet_id))?
        {
            return Err(Error::Unqualified);
        }
        // Admission uses the drawn ceiling (backend-v2.js:14036-14060):
        // `drawn = max(reserved, spent)` with unknown spend falling back to
        // the commitment figure, never to zero; `by_ceiling` saturates at
        // zero; the admitted figure is the minimum of the non-null limits.
        // The engagement being decided is excluded exactly like the retained
        // JavaScript decide() call (`excludeEngagementId: id`), and approve is
        // the operator verdict path, so `for_auto_join` is false — auto-join
        // is the other remainingFor caller, not this one.
        let report = usage::ceiling_report(&tx, &resource.id(), now)?;
        let spent_budget = budget(&tx, &resource, Some(id.as_str()), false)?;
        // backend-v2.js:14057: a seat declaration whose period mismatches the
        // pool's nulls the whole figure rather than falling back to the pool.
        if spent_budget.seat.status == allocation::SeatStatus::PeriodMismatch {
            return Err(Error::NoCeiling);
        }
        let requested = u64::from(value.requested_tokens);
        let by_ceiling = report
            .ceiling_tokens
            .map(|c| c.saturating_sub(report.drawn));
        let remaining = [
            by_ceiling,
            spent_budget.seat.remaining.map(u64::from),
            spent_budget.pool.remaining.map(u64::from),
        ]
        .into_iter()
        .flatten()
        .min()
        .ok_or(Error::NoCeiling)?;
        if remaining < requested {
            if by_ceiling.is_some_and(|b| b < requested) {
                // The ceiling side is binding: the refusal names both draws,
                // the binding one, the measurement's period key and the
                // cache-read discrepancy, exactly as the JavaScript does.
                let period_name = match report.period {
                    UsagePeriodKind::Daily => "daily",
                    UsagePeriodKind::Monthly => "monthly",
                };
                let message = ceiling_wording::over_commit_message(
                    value.agent_name.as_str(),
                    requested,
                    remaining,
                    Some(&SpendContext {
                        period: Some(period_name.into()),
                        reserved: Some(report.reserved),
                        spent: report.spent,
                        consumed: report.consumed,
                        ceiling_tokens: report.ceiling_tokens,
                        preset_name: Some(report.preset_name),
                        spend_period_key: report.spend_period_key,
                    }),
                );
                return Err(Error::OverCommit { message });
            }
            // The declared shared seat is the binding side: the resource-pool
            // refusal keeps its pre-existing identity and shape.
            return Err(Error::InsufficientCapacity);
        }
        value.state = EngagementState::Reserved;
        value.project_name = proof.project_name().map(str::to_owned);
        write_engagement(&tx, &value)?;
        tx.execute(
            "UPDATE engagements SET preset_id=?2,seat_id=?3 WHERE id=?1",
            params![id, resource.preset_id, resource.seat_id],
        )?;
        let payload = json!({"request":request,"registrationGeneration":generation,"runtimeName":value.runtime_name,"resource":resource,"approvalEvidence":proof.audit()});
        tx.execute("INSERT INTO effects(id,engagement_id,kind,state,payload) VALUES(?1,?2,'provision','pending',?3)", params![format!("provision_{id}"),id,serialize(&payload)?])?;
        record_decision(&tx, command_id, &digest, &value)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn reject(&mut self, command_id: &str, id: &str) -> Result<Engagement, Error> {
        self.end(command_id, id, false)
    }
    pub fn revoke(&mut self, command_id: &str, id: &str) -> Result<Engagement, Error> {
        self.end(command_id, id, true)
    }
    /// A definitive retirement failure can be retried explicitly. An uncertain
    /// retirement must first be inspected via observe_effect, never blindly replayed.
    pub fn retry_cleanup(&mut self, command_id: &str, id: &str) -> Result<Engagement, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let digest = decision_digest("retry_cleanup", id)?;
        if let Some(value) = replay_decision(&tx, command_id, &digest)? {
            return Ok(value);
        }
        let value = read_engagement(&tx, id)?;
        if value.state != EngagementState::Revoked {
            return Err(Error::State);
        }
        let changed=tx.execute("UPDATE effects SET state='pending',outcome_digest=NULL WHERE engagement_id=?1 AND kind='retire' AND state='failed'",[id])?;
        if changed != 1 {
            return Err(Error::State);
        }
        record_decision(&tx, command_id, &digest, &value)?;
        tx.commit()?;
        Ok(value)
    }
    fn end(&mut self, command_id: &str, id: &str, revoke: bool) -> Result<Engagement, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let digest = decision_digest(if revoke { "revoke" } else { "reject" }, id)?;
        if let Some(value) = replay_decision(&tx, command_id, &digest)? {
            return Ok(value);
        }
        let mut value = read_engagement(&tx, id)?;
        if !matches!(
            value.state,
            EngagementState::Pending | EngagementState::Reserved | EngagementState::Active
        ) || !revoke && value.state != EngagementState::Pending
        {
            return Err(Error::State);
        }
        let effect: Option<(String, String)> = tx
            .query_row(
                "SELECT state,payload FROM effects WHERE engagement_id=?1 AND kind='provision'",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((state, payload)) = effect {
            tx.execute("UPDATE effects SET state='cancelled',fence=fence+1 WHERE engagement_id=?1 AND kind='provision'", [id])?;
            if state != "pending" {
                value.cleanup = CleanupState::Pending;
                tx.execute("INSERT INTO effects(id,engagement_id,kind,state,payload) VALUES(?1,?2,'retire','pending',?3)", params![format!("retire_{id}"),id,payload])?;
            }
        }
        value.state = if revoke {
            EngagementState::Revoked
        } else {
            EngagementState::Rejected
        };
        write_engagement(&tx, &value)?;
        graphs::reconcile(&tx, graphs::now_ms()?)?;
        matrix_routes::reconcile(&tx, graphs::now_ms()?)?;
        record_decision(&tx, command_id, &digest, &value)?;
        tx.commit()?;
        Ok(value)
    }
    pub fn effect(&self, id: &str) -> Result<Effect, Error> {
        read_effect(&self.db, id)
    }
    pub fn claim_effect(&mut self) -> Result<Option<Effect>, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id: Option<String> = tx.query_row("SELECT f.id FROM effects f JOIN engagements e ON e.id=f.engagement_id JOIN registrations r ON r.fleet_id=e.fleet_id WHERE f.state='pending' AND e.generation=r.generation AND ((f.kind='provision' AND e.state='reserved') OR (f.kind='retire' AND e.state='revoked')) ORDER BY f.id LIMIT 1", [], |r| r.get(0)).optional()?;
        let Some(id) = id else {
            return Ok(None);
        };
        tx.execute(
            "UPDATE effects SET state='started',fence=fence+1 WHERE id=?1 AND fence<?2",
            params![id, JSON_SAFE_MAX],
        )?;
        let effect = read_effect(&tx, &id)?;
        if effect.state != EffectState::Started {
            return Err(Error::State);
        }
        tx.commit()?;
        Ok(Some(effect))
    }
    pub fn observe_effect(
        &mut self,
        id: &str,
        fence: u64,
        outcome: &EffectOutcome,
    ) -> Result<Engagement, Error> {
        match outcome {
            EffectOutcome::Applied { receipt } | EffectOutcome::NotApplied { receipt }
                if receipt.is_empty()
                    || receipt.len() > 2048
                    || receipt.chars().any(char::is_control) =>
            {
                return Err(InvalidInput("observed effect receipt required").into());
            }
            _ => {}
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let effect = read_effect(&tx, id)?;
        let digest = canonical::digest(&serde_json::to_value(outcome)?)?;
        if effect.fence != fence {
            return Err(Error::Generation);
        }
        let old: Option<String> = tx.query_row(
            "SELECT outcome_digest FROM effects WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        if old.as_ref() == Some(&digest) {
            return read_engagement(&tx, &effect.engagement_id);
        }
        if !matches!(effect.state, EffectState::Started | EffectState::Uncertain) {
            return Err(Error::State);
        }
        let mut value = read_engagement(&tx, &effect.engagement_id)?;
        let current: bool = tx.query_row("SELECT e.generation=r.generation FROM engagements e JOIN registrations r ON r.fleet_id=e.fleet_id WHERE e.id=?1", [&value.id], |r| r.get(0))?;
        if !current {
            return Err(Error::Generation);
        }
        let state = match outcome {
            EffectOutcome::Applied { .. } => {
                if effect.kind == "provision" {
                    value.state = EngagementState::Active;
                } else {
                    value.cleanup = CleanupState::Complete;
                }
                "complete"
            }
            EffectOutcome::NotApplied { .. } => {
                if effect.kind == "provision" {
                    value.state = EngagementState::Failed;
                    "failed"
                } else {
                    value.cleanup = CleanupState::Pending;
                    "failed"
                }
            }
            EffectOutcome::Unknown => {
                if effect.kind == "retire" {
                    value.cleanup = CleanupState::Uncertain;
                }
                "uncertain"
            }
        };
        tx.execute(
            "UPDATE effects SET state=?2,outcome_digest=?3 WHERE id=?1",
            params![id, state, digest],
        )?;
        write_engagement(&tx, &value)?;
        tx.commit()?;
        Ok(value)
    }
}
fn read_effect(db: &Connection, id: &str) -> Result<Effect, Error> {
    let row: (String, String, String, u64, String) = db
        .query_row(
            "SELECT engagement_id,kind,state,fence,payload FROM effects WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(Effect {
        id: id.into(),
        engagement_id: row.0,
        kind: row.1,
        state: serde_json::from_value(Value::String(row.2))?,
        fence: row.3,
        payload: serde_json::from_str(&row.4)?,
    })
}

pub use approvals::responses::{
    ApprovalResponseGrant, ApprovalResponseObservation, ApprovalResponseState,
    ApprovalResponseSummary,
};

pub use approvals::owned::{OwnedApprovalScope, OwnedApprovalStatus};

/// Validation shared by operator edits and concrete console commands inside their
/// original transaction. No write occurs until the caller rechecks its authority.
fn prepare_resource_write(
    tx: &Transaction<'_>,
    resource: &Resource,
    publication: Option<bool>,
    create_only: bool,
) -> Result<Resource, Error> {
    resource.validate()?;
    accounts::association(tx, resource)?;
    let mut resource = resource.clone();
    bounded_row(tx, "resources", "id", &resource.id(), 2048)?;
    let previous = match read_resource(tx, &resource.id()) {
        Ok(old) => Some(old),
        Err(Error::NotFound) => None,
        Err(error) => return Err(error),
    };
    if create_only && previous.is_some() {
        return Err(Error::Conflict);
    }
    resource.published = publication
        .or_else(|| previous.as_ref().map(|r| r.published))
        .unwrap_or(true);
    if let Some(old) = previous
        && (old.seat_id != resource.seat_id
            || old.framework != resource.framework
            || old.model != resource.model
            || old.provider != resource.provider
            || old.reasoning != resource.reasoning)
    {
        let count:i64=tx.query_row("SELECT COUNT(*) FROM engagements WHERE resource_id=?1 AND state IN ('reserved','active')",[resource.id()],|r|r.get(0))?;
        if count != 0 {
            return Err(Error::State);
        }
    }
    resource.roles = resource.eligible_roles();
    Ok(resource)
}
fn write_resource_configuration(
    tx: &Transaction<'_>,
    resource: &Resource,
    create_only: bool,
) -> Result<(), Error> {
    if create_only {
        tx.execute(
            "INSERT INTO resources(id,preset_id,config) VALUES(?1,?2,?3)",
            params![resource.id(), resource.preset_id, serialize(resource)?],
        )?;
    } else {
        tx.execute("INSERT INTO resources(id,preset_id,config) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET config=excluded.config", params![resource.id(),resource.preset_id,serialize(resource)?])?;
    }
    Ok(())
}
