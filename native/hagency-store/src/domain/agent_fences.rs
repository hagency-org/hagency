//! ADR-182 decision 3: an unproven cleanup fences the agent, durably. The
//! fence is a row, not an in-memory owner: it survives a restart and a
//! re-attach; while it is open the host claim and both selectors return no
//! work for the engagement; and only the operator's resolution of the fenced
//! dispatch (`recover_dispatch`, `resolve_stopped_dispatch`,
//! `continue_stopped_dispatch`) clears it. Nothing here stops a process,
//! resolves a dispatch or lifts a quarantine.
use super::DomainRepository;
use crate::Error;
use hagency_core::{InvalidInput, JSON_SAFE_MAX, project::identifier, tasks::clock};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

/// Why the agent is fenced: the report's custody was `Observed` without all
/// three facts, or `Unknown`. The 038 CHECK constraint lists exactly these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FenceReason {
    CleanupUnproven,
    CleanupUnknown,
}
impl FenceReason {
    /// The stored word.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CleanupUnproven => "cleanup_unproven",
            Self::CleanupUnknown => "cleanup_unknown",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        [Self::CleanupUnproven, Self::CleanupUnknown]
            .into_iter()
            .find(|reason| reason.as_str() == value)
    }
}
/// One fence row. `cleared_at` and `cleared_by` are written together by the
/// resolution that cleared it and are both absent while the fence stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFence {
    pub id: u64,
    pub engagement_id: String,
    pub dispatch_id: String,
    pub fence: u64,
    pub reason: FenceReason,
    pub created_at: u64,
    pub cleared_at: Option<u64>,
    pub cleared_by: Option<String>,
}

/// The read bound: an engagement's fences, newest first.
const FENCE_LIMIT: u32 = 64;

type Raw = (
    u64,
    String,
    String,
    u64,
    String,
    u64,
    Option<u64>,
    Option<String>,
);
fn raw(r: &rusqlite::Row<'_>) -> rusqlite::Result<Raw> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
    ))
}
fn fence_row(raw: Raw) -> Result<AgentFence, Error> {
    let (id, engagement_id, dispatch_id, fence, reason, created_at, cleared_at, cleared_by) = raw;
    // The 038 CHECK admits only the two words; a row outside them is a
    // schema fault, not a reason.
    let reason = FenceReason::parse(&reason).ok_or(Error::Schema)?;
    Ok(AgentFence {
        id,
        engagement_id,
        dispatch_id,
        fence,
        reason,
        created_at,
        cleared_at,
        cleared_by,
    })
}

/// Whether the session's engagement has an open fence. The selectors mint
/// nothing for it and say nothing in the thread: the fence is the operator's
/// matter, not the room's, and the request stays unread for the selection
/// that follows the clearing.
pub(super) fn session_fenced(db: &Connection, session: &str) -> Result<bool, Error> {
    let fenced: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_fences f JOIN runner_sessions s ON s.engagement_id=f.engagement_id WHERE s.id=?1 AND f.cleared_at IS NULL)",
        [session],
        |r| r.get(0),
    )?;
    Ok(fenced)
}
/// Inside the resolution's own transaction, after its writes: every open
/// fence naming the dispatch is cleared with the word of the route that
/// resolved it. Returns how many; none is the common case, since most
/// resolutions are of dispatches that never fenced anything.
pub(super) fn clear_fences_for_dispatch(
    tx: &Transaction<'_>,
    dispatch_id: &str,
    cleared_by: &str,
    now: u64,
) -> Result<u64, Error> {
    let cleared = tx.execute(
        "UPDATE agent_fences SET cleared_at=?2,cleared_by=?3 WHERE dispatch_id=?1 AND cleared_at IS NULL",
        params![dispatch_id, now, cleared_by],
    )?;
    Ok(u64::try_from(cleared).unwrap_or(u64::MAX))
}

impl DomainRepository {
    /// The driver's fence, written before it drops the owner. The dispatch
    /// must exist and its session must be this engagement's: a fence names
    /// the agent's own attempt, never another agent's. Idempotent under the
    /// driver's repeats and a re-attach: an open fence for the same
    /// (engagement, dispatch, fence) is returned unchanged.
    pub fn write_agent_fence(
        &mut self,
        engagement_id: &str,
        dispatch_id: &str,
        fence: u64,
        reason: FenceReason,
        now: u64,
    ) -> Result<AgentFence, Error> {
        identifier(engagement_id, 128)?;
        identifier(dispatch_id, 128)?;
        if fence > JSON_SAFE_MAX {
            return Err(InvalidInput("agent fence exceeds integer contract").into());
        }
        clock(now)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT s.engagement_id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id WHERE d.id=?1",
                [dispatch_id],
                |r| r.get(0),
            )
            .optional()?;
        match owner.as_deref() {
            None => return Err(Error::NotFound),
            Some(owner) if owner != engagement_id => return Err(Error::RunnerAuthority),
            Some(_) => {}
        }
        let open: Option<Raw> = tx
            .query_row(
                "SELECT id,engagement_id,dispatch_id,fence,reason,created_at,cleared_at,cleared_by FROM agent_fences WHERE engagement_id=?1 AND dispatch_id=?2 AND fence=?3 AND cleared_at IS NULL ORDER BY id LIMIT 1",
                params![engagement_id, dispatch_id, fence],
                raw,
            )
            .optional()?;
        if let Some(open) = open {
            tx.commit()?;
            return fence_row(open);
        }
        tx.execute(
            "INSERT INTO agent_fences(engagement_id,dispatch_id,fence,reason,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![engagement_id, dispatch_id, fence, reason.as_str(), now],
        )?;
        let id = u64::try_from(tx.last_insert_rowid()).map_err(|_| Error::Schema)?;
        tx.commit()?;
        Ok(AgentFence {
            id,
            engagement_id: engagement_id.into(),
            dispatch_id: dispatch_id.into(),
            fence,
            reason,
            created_at: now,
            cleared_at: None,
            cleared_by: None,
        })
    }
    /// The oldest open fence of the engagement, if any: the one the fleet
    /// status names and re-attach honours.
    pub fn open_agent_fence(&self, engagement_id: &str) -> Result<Option<AgentFence>, Error> {
        identifier(engagement_id, 128)?;
        self.db
            .query_row(
                "SELECT id,engagement_id,dispatch_id,fence,reason,created_at,cleared_at,cleared_by FROM agent_fences WHERE engagement_id=?1 AND cleared_at IS NULL ORDER BY id LIMIT 1",
                [engagement_id],
                raw,
            )
            .optional()?
            .map(fence_row)
            .transpose()
    }
    /// The engagement's fences, open and cleared, newest first. Operator
    /// evidence; no runtime route reads it.
    pub fn agent_fences(&self, engagement_id: &str) -> Result<Vec<AgentFence>, Error> {
        identifier(engagement_id, 128)?;
        self.db
            .prepare(
                "SELECT id,engagement_id,dispatch_id,fence,reason,created_at,cleared_at,cleared_by FROM agent_fences WHERE engagement_id=?1 ORDER BY id DESC LIMIT ?2",
            )?
            .query_map(params![engagement_id, FENCE_LIMIT], raw)?
            .map(|row| fence_row(row?))
            .collect()
    }
    /// How many of the engagement's dispatches await an operator (ADR-182
    /// decision 6: `awaiting_operator` becomes this count, informational
    /// only). The `unresolved_dispatches` view's own rule, by session.
    pub fn unresolved_dispatches_for_engagement(&self, engagement_id: &str) -> Result<u64, Error> {
        identifier(engagement_id, 128)?;
        let count: u64 = self.db.query_row(
            "SELECT COUNT(*) FROM unresolved_dispatches u JOIN runner_sessions s ON s.id=u.session_id WHERE s.engagement_id=?1",
            [engagement_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }
}
