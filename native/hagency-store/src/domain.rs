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
mod ceiling_alerts;
pub use ceiling_alerts::{
    ALERT_STATUSES, AlertTransition, CeilingAlert, MAX_OPEN_CEILING_ALERTS, SweepOutcome,
    allowed_transitions,
};
mod conversation_lifecycle;
mod conversations;
mod execution;
mod graphs;
mod matrix_routes;
mod messages;
pub use messages::{CorpusSweepOutcome, MESSAGE_RETENTION_FLOOR, RetentionStatus};
mod notice_custody;
mod owned_completion;
mod owned_dispatch;
pub use owned_completion::OwnedCompletion;
pub use owned_dispatch::{
    OwnedClaimProfile, OwnedClaimRoom, OwnedDispatchScope, OwnedFailure, OwnedObservation,
};
pub use peers::{
    PEER_RECEIPT_CEILING, PEER_RETENTION_CEILING, PEER_RETENTION_FLOOR, PeerRetentionStatus,
    PeerSweepOutcome,
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

/// The seven named observation columns (ADR-138), shared by the list and
/// single reads so neither can drift from the other. No `SELECT *`, no
/// `description`/`config`/`application`/`observation`, no owner or room.
const APPROVAL_SELECT: &str = "SELECT a.id,a.state,a.choice,a.scope_key IS NOT NULL,a.expires_at,c.engagement_id,p.room_id \
             FROM owner_approvals a \
             JOIN approval_contexts c ON c.id=a.context_id \
             JOIN engagements e ON e.id=c.engagement_id \
             LEFT JOIN projects p ON p.fleet_id=e.fleet_id AND p.id=e.project_id \
             AND p.generation=e.generation";

type ApprovalTuple = (
    String,
    String,
    Option<String>,
    bool,
    i64,
    String,
    Option<String>,
);

fn approval_tuple(r: &rusqlite::Row<'_>) -> rusqlite::Result<ApprovalTuple> {
    Ok((
        r.get::<_, String>(0)?,
        r.get::<_, String>(1)?,
        r.get::<_, Option<String>>(2)?,
        r.get::<_, bool>(3)?,
        r.get::<_, i64>(4)?,
        r.get::<_, String>(5)?,
        r.get::<_, Option<String>>(6)?,
    ))
}

fn approval_row(
    (id, state, choice, reusable_scope, expires_at, engagement_id, project_room_id): ApprovalTuple,
) -> Result<Value, Error> {
    Ok(json!({
        "id": id,
        "state": state,
        "choice": choice
            .map(|c| serde_json::from_str::<String>(&c))
            .transpose()?,
        "reusableScope": reusable_scope,
        "expiresAt": expires_at,
        "engagementId": engagement_id,
        "projectRoomId": project_room_id,
    }))
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
/// One project of a side, in the project-sides projection (ADR-132):
/// exactly these two keys — `owner_mxid` and `owner_room_id` are
/// deliberately withheld (the owner's DM room is non-public, ADR-112, and
/// the retained `publicSide` serves neither).
#[derive(Debug, Clone, Serialize)]
pub struct SideProject {
    pub id: String,
    pub room_id: String,
}
/// One row of the read-only project-sides projection (ADR-132): the fleet
/// registration read as a side — the id IS the server name (ADR-016) —
/// joined to its projects. Exactly these six keys; no credential key
/// exists at all, and the read extracts only the named config paths
/// (`json_extract`), never a parse-and-strip of the whole config, so a
/// credential-shaped value seeded into the config cannot travel inside an
/// otherwise-allowed key. `registered` compares the row's generation
/// column against the config's own generation field — true by
/// construction today, but computed, so drift is visible rather than
/// assumed away.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectSide {
    pub id: String,
    pub representative: String,
    pub generation: u64,
    pub reception_room_id: String,
    pub registered: bool,
    pub projects: Vec<SideProject>,
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
/// One row of the read-only agent roster (ADR-126): the engagement
/// projection joined to the resource's framework and the newest
/// dispatch-attempt clock. Exactly these seven keys — the console route
/// serves them verbatim and the client validator refuses an eighth — so
/// no credential home, workspace path or tmux target can travel inside
/// one. `last_activity_ms` is "last dispatch activity", NOT last seen:
/// native has no heartbeat model, and a dispatch with no attempt row
/// reports `None`, never zero.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRosterRow {
    pub name: String,
    pub framework: String,
    pub role: String,
    pub state: EngagementState,
    pub engagement_id: String,
    pub requested_tokens: u64,
    pub last_activity_ms: Option<u64>,
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
/// The decision-receipt bound (ADR-095 amendment, retention Slice 3): the
/// newest `DECISION_RETENTION_LIMIT` verdicts by `rowid` replay idempotently;
/// older rows are trimmed in-write, inside the deciding command's own
/// transaction — never a phase, no period, no bootstrap constant.
const DECISION_RETENTION_LIMIT: u64 = 500;
/// Per-command catch-up bound: at most this many rows are removed by one
/// deciding command, so a deeply over-window corpus drains over later writes
/// rather than lengthening a single verdict's writer hold.
const DECISION_PRUNE_BATCH: u64 = 512;
/// The kind word whose decisions are excluded from the bound (ADR-095 §2):
/// `retry_cleanup` mutates before it records, so its replay lookup on a fresh
/// command id finds no row by design, and pruning it would refuse a legitimate
/// first retry. The exclusion is recomputed in Rust over each candidate's
/// stored result, never by a `LIKE` on a digest.
const DECISION_PRUNE_RETRY_KIND: &str = "retry_cleanup";
/// Receipt trim bound (tick contract §3.1), applied by the same writer that
/// inserts a `phase='decisions'` receipt row.
const RETENTION_RECEIPT_LIMIT: u64 = 100;

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
    // The in-write trim runs after the insert, in the same transaction, so a
    // rolled-back verdict carries neither the prune nor a receipt.
    trim_decisions(tx)?;
    Ok(())
}

/// Trim `decisions` to the newest `DECISION_RETENTION_LIMIT` rows by `rowid`,
/// oldest first, excluding every `retry_cleanup` decision (recomputed in Rust
/// over the stored result) and never the maximum-`rowid` row. Writes one
/// `retention_prune_receipts` row with `phase='decisions'` when the phase did
/// work (`pruned > 0`) or the corpus still stands over the window
/// (`remaining > 0`), and trims the receipt table to `RETENTION_RECEIPT_LIMIT`
/// in the same step.
fn trim_decisions(tx: &Transaction<'_>) -> Result<(), Error> {
    let max_rowid: Option<i64> = tx
        .query_row("SELECT MAX(rowid) FROM decisions", [], |r| r.get(0))
        .optional()?;
    let Some(max_rowid) = max_rowid else {
        return Ok(());
    };
    // Rows at or below (max - limit) are past the newest-LIMIT window; the
    // subquery keeps the window correct while the batch catches up.
    let threshold = max_rowid.saturating_sub(DECISION_RETENTION_LIMIT as i64);
    if threshold <= 0 {
        return Ok(());
    }
    let started = std::time::Instant::now();
    let candidates: Vec<(String, i64, String, String)> = {
        let mut statement = tx.prepare(
            "SELECT id,rowid,digest,result FROM decisions \
             WHERE rowid <= ?1 AND rowid < ?2 ORDER BY rowid",
        )?;
        let rows = statement
            .query_map(params![threshold, max_rowid], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        rows
    };
    let mut pruned = 0u64;
    let mut oldest: Option<i64> = None;
    let mut newest: Option<i64> = None;
    for (_id, rowid, digest, result) in candidates {
        // Exclude retry_cleanup by recomputing its digest over the stored
        // engagement — exact comparison, never a LIKE on a digest (ADR-095 §2).
        let is_retry = serde_json::from_str::<Engagement>(&result)
            .ok()
            .and_then(|e| decision_digest(DECISION_PRUNE_RETRY_KIND, &e.id).ok())
            .is_some_and(|d| d == digest);
        if is_retry {
            continue;
        }
        if pruned >= DECISION_PRUNE_BATCH {
            break;
        }
        tx.execute("DELETE FROM decisions WHERE rowid = ?1", [rowid])?;
        oldest = Some(oldest.map_or(rowid, |o| o.min(rowid)));
        newest = Some(newest.map_or(rowid, |n| n.max(rowid)));
        pruned += 1;
    }
    let total: i64 = tx.query_row("SELECT COUNT(*) FROM decisions", [], |r| r.get(0))?;
    let remaining = (total as u64).saturating_sub(DECISION_RETENTION_LIMIT);
    if pruned > 0 || remaining > 0 {
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX / 2);
        tx.execute(
            "INSERT INTO retention_prune_receipts\
             (phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms) \
             VALUES('decisions',?1,?2,?3,?4,?5,?6)",
            params![
                pruned,
                oldest.map(|v| v.to_string()).unwrap_or_default(),
                newest.map(|v| v.to_string()).unwrap_or_default(),
                remaining,
                elapsed_ms,
                graphs::now_ms()?,
            ],
        )?;
        tx.execute(
            "DELETE FROM retention_prune_receipts WHERE sequence NOT IN (\
             SELECT sequence FROM retention_prune_receipts \
             ORDER BY sequence DESC LIMIT ?1)",
            [RETENTION_RECEIPT_LIMIT],
        )?;
    }
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
            // G5 (ADR-108 amendment): three derived keys over the SAME set
            // `available` samples — qualifying(role) = published resources
            // passing Resource::qualifies (provisionable ∧ ceiling ∧ policy
            // tier). `qualification::resources_for_role` is deliberately
            // NOT used: it omits published and provisionable, so a resource
            // could count toward fillable and not toward available — two
            // answers for one question. `families` is the MODEL family
            // (model().1), never the framework, matching the retained
            // catalogue's cross-family count (derive.js:91,100); sorted for
            // a stable wire. A model matching no policy tier yields
            // (None,None) and cannot qualify at all, so it contributes
            // nothing — unknown tier never reads as stronger.
            let qualifying: Vec<&Resource> = resources.iter().filter(|r| r.qualifies(role)).collect();
            let families: std::collections::BTreeSet<&str> =
                qualifying.iter().filter_map(|r| qualification::model(&r.profile()).1).collect();
            let fillable = qualifying.len();
            let default = qualification::default_tier(role);
            let over_tier = default.map_or(0, |needed| {
                qualifying
                    .iter()
                    .filter(|r| {
                        qualification::model(&r.profile())
                            .0
                            .is_some_and(|tier| tier > needed)
                    })
                    .count()
            });
            Ok(json!({"role":role,"explicitPublication":explicit,"available":available,"crossFamily":qualification::cross_family(role),"defaultTier":qualification::default_tier(role),"families":families.into_iter().collect::<Vec<&str>>(),"fillable":fillable,"overTier":over_tier}))
        }).collect()
    }
    pub fn open(directory: &Path) -> Result<Self, Error> {
        let mut database = database::open(
            directory,
            database::Schema {
                name: "domain.sqlite3",
                lock: "domain.lock",
                application_id: 0x48414732,
                version: 29,
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
                    (24, include_str!("migrations/024-ceiling-alerts.sql")),
                    (25, include_str!("migrations/025-alert-transitions.sql")),
                    (26, include_str!("migrations/026-corpus-retention.sql")),
                    (27, include_str!("migrations/027-peer-corpus-retention.sql")),
                    (
                        28,
                        include_str!("migrations/028-account-login-readiness.sql"),
                    ),
                    (
                        29,
                        include_str!("migrations/029-account-logout-receipt.sql"),
                    ),
                ],
                sql: include_str!("domain.sql"),
                verify: &[
                    "SELECT sequence,engagement_id,source_key,scope_digest,digest,config,source_session_id,wake,pruned_at_ms FROM retained_message_archive LIMIT 0",
                    "SELECT source_key,digest,sequence,pruned_at_ms FROM retained_peer_index LIMIT 0",
                    "SELECT sequence,phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms FROM retention_prune_receipts LIMIT 0",
                    "SELECT id,account_id,account_generation,attempt,observed_at_ms,expires_at_ms,mode,provider_state,outcome FROM account_login_observations LIMIT 0",
                    "SELECT account_id,attempt,started_at_ms,deadline_ms,state,receipt_id FROM account_login_attempts LIMIT 0",
                    "SELECT id,account_id,retired_at_ms,readiness,logout_detail FROM account_logout_receipts LIMIT 0",
                    "SELECT dedupe_key,resource_id,summary,detail,runbook,impact,recovery_condition,occurrences,first_seen_ms,last_seen_ms,resolved_at_ms,resolved_by,status,note,transitioned_at_ms,transitioned_by FROM ceiling_alerts LIMIT 0",
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
        accounts::reconcile_login_attempts(&tx, graphs::now_ms()?)?;
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
    /// The bounded approval observation read (ADR-138, C2a): pages
    /// `owner_approvals` by the opaque id cursor with the same hard cap as
    /// `engagements`, and names its `SELECT` columns so the projection cannot
    /// widen silently — no `description`, no `config`/`application`/
    /// `observation` JSON, no owner or room column is ever read, so none can
    /// cross. The item is exactly the console's seven camelCase keys.
    pub fn approvals(&self, after: &str, limit: usize) -> Result<Vec<Value>, Error> {
        if limit == 0 || limit > 100 {
            return Err(InvalidInput("page limit must be 1..100").into());
        }
        let mut query = self.db.prepare(&format!(
            "{APPROVAL_SELECT} WHERE a.id>?1 ORDER BY a.id LIMIT ?2"
        ))?;
        let rows = query
            .query_map(params![after, limit as i64], approval_tuple)?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        rows.into_iter().map(approval_row).collect()
    }
    /// The single-row half of the observation read: the same seven named
    /// columns as the list, keyed by the approval's own opaque id. Read-only.
    pub fn approval(&self, id: &str) -> Result<Value, Error> {
        project::identifier(id, 128)?;
        let row = self
            .db
            .query_row(
                &format!("{APPROVAL_SELECT} WHERE a.id=?1"),
                [id],
                approval_tuple,
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        approval_row(row)
    }
    pub fn get(&self, id: &str) -> Result<Engagement, Error> {
        read_engagement(&self.db, id)
    }
    /// The read-only project-sides projection (ADR-132): one row per fleet
    /// registration — the id IS the server name (ADR-016) — LEFT JOINed to
    /// its projects. `SELECT`-named columns only, and the side fields are
    /// extracted by path from the config JSON (`json_extract`), never by
    /// parsing the whole config and stripping: a credential-shaped value
    /// seeded into the config has no path into this projection. Projects
    /// are capped at 64 per side; the fleet cap is `registrations`' own
    /// 1024-row bound. `registered` compares the row's generation column
    /// against the config's own generation field — true by construction
    /// today, computed rather than assumed.
    pub fn project_sides(&self) -> Result<Vec<ProjectSide>, Error> {
        // ONE statement, one snapshot (F3): the LEFT JOIN is evaluated
        // inside a single implicit transaction, so a project written
        // between two reads cannot tear the view — the way `agent_roster`
        // joins in one statement. The ordering guarantee is this
        // statement's ORDER BY. The per-side cap (F2) is INSIDE the
        // statement as a ROW_NUMBER window — never a truncation of an
        // unbounded fetch in Rust. Bound arithmetic: `register()` bounds
        // `registrations` at 1024 rows and the window bounds projects at
        // 64 per side, so the joined statement yields at most 65,536 rows.
        let mut query = self.db.prepare(
            "SELECT r.fleet_id, \
             json_extract(r.config,'$.serverName'), \
             json_extract(r.config,'$.representativeMxid'), \
             json_extract(r.config,'$.receptionRoomId'), \
             json_extract(r.config,'$.generation'), r.generation, \
             p.id, p.room_id \
             FROM registrations r \
             LEFT JOIN (SELECT fleet_id,id,room_id, \
             ROW_NUMBER() OVER (PARTITION BY fleet_id ORDER BY id) AS rn \
             FROM projects) p ON p.fleet_id=r.fleet_id AND p.rn<=64 \
             ORDER BY r.fleet_id,p.id",
        )?;
        let rows = query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, rusqlite::types::Value>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;
        let mut sides: Vec<ProjectSide> = Vec::new();
        let mut current_fleet: Option<String> = None;
        for row in rows {
            let (
                fleet,
                id,
                representative,
                reception,
                config_generation,
                row_generation,
                project,
                room,
            ) = row?;
            let generation = u64::try_from(row_generation).map_err(|_| Error::Schema)?;
            // F4: a config whose `$.generation` is not a JSON integer is
            // corrupt store state, not a value to coerce — the conversion
            // failure is mapped to Error::Schema AT THE READ (a named
            // corrupt-state failure) instead of surfacing as the raw
            // FromSqlConversionFailure/Sqlite variant.
            let config_generation = match config_generation {
                rusqlite::types::Value::Integer(value) => {
                    u64::try_from(value).map_err(|_| Error::Schema)?
                }
                _ => return Err(Error::Schema),
            };
            if current_fleet.as_deref() == Some(fleet.as_str()) {
                // LEFT JOIN NULL arm (a side with no project) contributes
                // nothing to the fold.
                if let (Some(project), Some(room)) = (project, room) {
                    sides
                        .last_mut()
                        .expect("the fold key implies a pushed side")
                        .projects
                        .push(SideProject {
                            id: project,
                            room_id: room,
                        });
                }
            } else {
                current_fleet = Some(fleet);
                sides.push(ProjectSide {
                    id,
                    representative,
                    generation,
                    reception_room_id: reception,
                    registered: generation == config_generation,
                    projects: match (project, room) {
                        (Some(project), Some(room)) => vec![SideProject {
                            id: project,
                            room_id: room,
                        }],
                        _ => Vec::new(),
                    },
                });
            }
        }
        Ok(sides)
    }
    /// The read-only agent roster (ADR-126): one row per engagement — the
    /// derivation is engagement-keyed, so an agent with no engagement row is
    /// invisible (named in the ADR's consequences). The framework comes from
    /// the engagement's resource config; `last_activity_ms` is the NEWEST
    /// `runner_attempts.created_at` among the engagement's sessions'
    /// dispatches — last dispatch activity, not last seen — and a dispatch
    /// with no attempt row contributes nothing, so an engagement with no
    /// attempt at all reports `None`, never zero. Bounded to one read of at
    /// most 100 rows, ordered by engagement id like every other list read.
    pub fn agent_roster(&self) -> Result<Vec<AgentRosterRow>, Error> {
        let mut query = self.db.prepare(
            "SELECT e.projection,r.config,(SELECT MAX(a.created_at) FROM runner_sessions s \
             JOIN runner_dispatches d ON d.session_id=s.id \
             JOIN runner_attempts a ON a.dispatch_id=d.id WHERE s.engagement_id=e.id) \
             FROM engagements e JOIN resources r ON r.id=e.resource_id ORDER BY e.id LIMIT 100",
        )?;
        query
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })?
            .map(|row| {
                let (projection, config, last) = row?;
                let engagement: Engagement = serde_json::from_str(&projection)?;
                let resource: Resource = serde_json::from_str(&config)?;
                Ok(AgentRosterRow {
                    name: engagement.agent_name.as_str().to_owned(),
                    framework: resource.framework,
                    role: engagement.role,
                    state: engagement.state,
                    engagement_id: engagement.id,
                    requested_tokens: u64::from(engagement.requested_tokens),
                    last_activity_ms: last.and_then(|v| u64::try_from(v).ok()),
                })
            })
            .collect()
    }
    pub fn resource_budget(&self, id: &str) -> Result<Budget, Error> {
        budget(&self.db, &read_resource(&self.db, id)?, None, false)
    }
    /// The budget the console publishes (brief 18): the commitments budget
    /// AND the ceiling draw in ONE transaction-consistent read, so a page
    /// can never render figures from two different writer jobs. The draw
    /// figures (`spent`/`consumed` unknown-when-unmeasured, `drawn =
    /// max(reserved, spent)`, `backend-v2.js:14052-14053`) are the same ones
    /// the alarm sweep and admission publish — never a second arithmetic
    /// path (ADR-121).
    pub fn resource_headroom(&self, id: &str, at: u64) -> Result<(Budget, CeilingReport), Error> {
        let budget = budget(&self.db, &read_resource(&self.db, id)?, None, false)?;
        let report = usage::ceiling_report(&self.db, id, at)?;
        Ok((budget, report))
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
