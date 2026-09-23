//! ADR-181: the per-attempt evidence a lost agent is diagnosed from. Nothing
//! here authorizes anything, promotes a status or releases custody: every
//! write is an observation beside the existing rules, bounded to fixed keys,
//! integers and control-stripped text (ADR-175), and a write that fails never
//! changes the attempt's outcome.
use super::{DomainRepository, serialize};
use crate::Error;
use hagency_core::{InvalidInput, JSON_SAFE_MAX, project::identifier, tasks::clock};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The phases an attempt can visit, in the order it can visit them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptPhase {
    Claimed,
    SpawnStarted,
    SpawnDone,
    Initialized,
    TurnStarted,
    ApprovalRequested,
    ApprovalDecided,
    Parked,
    Resumed,
    StopRequested,
    StopReported,
    Settled,
    Failed,
    Lost,
}
impl AttemptPhase {
    const ALL: [Self; 14] = [
        Self::Claimed,
        Self::SpawnStarted,
        Self::SpawnDone,
        Self::Initialized,
        Self::TurnStarted,
        Self::ApprovalRequested,
        Self::ApprovalDecided,
        Self::Parked,
        Self::Resumed,
        Self::StopRequested,
        Self::StopReported,
        Self::Settled,
        Self::Failed,
        Self::Lost,
    ];
    /// The stored word; the 037 CHECK constraint lists exactly these.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::SpawnStarted => "spawn_started",
            Self::SpawnDone => "spawn_done",
            Self::Initialized => "initialized",
            Self::TurnStarted => "turn_started",
            Self::ApprovalRequested => "approval_requested",
            Self::ApprovalDecided => "approval_decided",
            Self::Parked => "parked",
            Self::Resumed => "resumed",
            Self::StopRequested => "stop_requested",
            Self::StopReported => "stop_reported",
            Self::Settled => "settled",
            Self::Failed => "failed",
            Self::Lost => "lost",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|phase| phase.as_str() == value)
    }
}
/// One observation the host asks the store to keep. `detail` is bounded on
/// the way in: an object of identifier keys, control-stripped strings of at
/// most 4 KiB, at most two containers deep inside it, 8 KiB serialized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptEvent {
    pub dispatch_id: String,
    pub fence: u64,
    pub phase: AttemptPhase,
    pub detail: Value,
}
/// One row of the event log, as read back in `seq` order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptEventRow {
    pub seq: u64,
    pub at_ms: u64,
    pub phase: AttemptPhase,
    pub detail: Value,
}
/// The attempt row's clock columns (ADR-181 point 2). `Started`, `Parked` and
/// `Settled` are written once — the first observation stands, a repeat is a
/// no-op; `LastRenew` is the renewal's own mark and always moves forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptClock {
    Started,
    Parked,
    LastRenew,
    Settled,
}
impl AttemptClock {
    fn column(self) -> &'static str {
        match self {
            Self::Started => "started_at",
            Self::Parked => "parked_at",
            Self::LastRenew => "last_renew_at",
            Self::Settled => "settled_at",
        }
    }
}
/// The attempt row's clock and terminal reason, every field absent until its
/// writer observed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptClockRow {
    pub started_at: Option<u64>,
    pub parked_at: Option<u64>,
    pub last_renew_at: Option<u64>,
    pub settled_at: Option<u64>,
    pub terminal_reason: Option<String>,
}

/// At most this many events per (dispatch, fence): fourteen phases, a few
/// approval and park cycles, and the host's repeats after a lost reply.
const EVENT_LIMIT: u32 = 256;
/// The serialized `detail` bound.
const DETAIL_LIMIT: usize = 8192;
/// Every string inside `detail` is cut here, on a char boundary.
const STRING_LIMIT: usize = 4096;
/// Every key inside `detail` is an ASCII identifier of at most this length.
const KEY_LIMIT: usize = 64;
/// Containers nested inside the root object: `{"rows":[{"pid":1}]}` is two
/// deep (the guardian's live-row list, ADR-181 point 5b); a container inside
/// those rows would be three and is refused.
const DEPTH_LIMIT: usize = 2;
/// `terminal_reason` (ADR-181 point 2): failure, exit identity and the 512
/// byte stderr tail with their separators.
const TERMINAL_REASON_LIMIT: usize = 640;

/// Control characters become U+FFFD, then the text is cut to `limit` bytes
/// on a char boundary. Never rejects: the tail of a runtime's stderr is the
/// one free text ADR-181 admits, and it is admitted bounded, not refused.
fn bounded_text(text: &str, limit: usize) -> String {
    let mut clean: String = text
        .chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect();
    clean.truncate(clean.floor_char_boundary(limit));
    clean
}
fn key(name: &str) -> Result<(), Error> {
    let head = name
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    if !head
        || name.len() > KEY_LIMIT
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(InvalidInput("attempt event detail key is not an identifier").into());
    }
    Ok(())
}
fn sanitize_object(fields: &Map<String, Value>, depth: usize) -> Result<Map<String, Value>, Error> {
    fields
        .iter()
        .map(|(name, value)| {
            key(name)?;
            Ok((name.clone(), sanitize_value(value, depth)?))
        })
        .collect()
}
fn sanitize_value(value: &Value, depth: usize) -> Result<Value, Error> {
    let nested = || {
        if depth >= DEPTH_LIMIT {
            Err(Error::Invalid(InvalidInput(
                "attempt event detail is nested too deeply",
            )))
        } else {
            Ok(depth + 1)
        }
    };
    Ok(match value {
        Value::String(text) => Value::String(bounded_text(text, STRING_LIMIT)),
        Value::Object(fields) => Value::Object(sanitize_object(fields, nested()?)?),
        Value::Array(items) => {
            let depth = nested()?;
            Value::Array(
                items
                    .iter()
                    .map(|item| sanitize_value(item, depth))
                    .collect::<Result<_, _>>()?,
            )
        }
        scalar => scalar.clone(),
    })
}
/// The bounded copy of `detail` the row stores, or why it cannot be one.
fn sanitize(detail: &Value) -> Result<String, Error> {
    let Value::Object(fields) = detail else {
        return Err(InvalidInput("attempt event detail must be an object").into());
    };
    let encoded = serialize(&Value::Object(sanitize_object(fields, 0)?))?;
    if encoded.len() > DETAIL_LIMIT {
        return Err(InvalidInput("attempt event detail exceeds 8 KiB").into());
    }
    Ok(encoded)
}
/// The one insert, inside the caller's transaction: `lose` records its
/// `lost` event through this in the settlement's own transaction, the host's
/// observations through `record_attempt_event`. Returns the new 1-based seq.
pub(super) fn insert(
    tx: &Transaction<'_>,
    dispatch_id: &str,
    fence: u64,
    phase: AttemptPhase,
    detail: &Value,
    now: u64,
) -> Result<u64, Error> {
    identifier(dispatch_id, 128)?;
    if fence > JSON_SAFE_MAX {
        return Err(InvalidInput("attempt fence exceeds integer contract").into());
    }
    clock(now)?;
    let encoded = sanitize(detail)?;
    let known: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM runner_dispatches WHERE id=?1)",
        [dispatch_id],
        |r| r.get(0),
    )?;
    if !known {
        return Err(Error::NotFound);
    }
    let (count, seq): (u32, u64) = tx.query_row(
        "SELECT COUNT(*),COALESCE(MAX(seq),0)+1 FROM runner_attempt_events WHERE dispatch_id=?1 AND fence=?2",
        params![dispatch_id, fence],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if count >= EVENT_LIMIT {
        return Err(Error::Capacity);
    }
    tx.execute(
        "INSERT INTO runner_attempt_events(dispatch_id,fence,seq,at_ms,phase,detail) VALUES(?1,?2,?3,?4,?5,?6)",
        params![dispatch_id, fence, seq, now, phase.as_str(), encoded],
    )?;
    Ok(seq)
}

impl DomainRepository {
    /// Host observation, best effort: its own savepoint inside its own
    /// transaction, released on success and rolled back on failure, so a
    /// refused detail leaves nothing behind and the next observation on the
    /// same attempt is unaffected. The caller counts a refusal; it never
    /// retries it and never lets it change the attempt's outcome.
    pub fn record_attempt_event(&mut self, event: &AttemptEvent, now: u64) -> Result<u64, Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch("SAVEPOINT attempt_event")?;
        let seq = match insert(
            &tx,
            &event.dispatch_id,
            event.fence,
            event.phase,
            &event.detail,
            now,
        ) {
            Ok(seq) => {
                tx.execute_batch("RELEASE attempt_event")?;
                seq
            }
            Err(error) => {
                tx.execute_batch("ROLLBACK TO attempt_event; RELEASE attempt_event")?;
                return Err(error);
            }
        };
        tx.commit()?;
        Ok(seq)
    }
    /// The attempt's event log in visit order. Operator-private evidence;
    /// no runtime route reads it.
    pub fn attempt_events(
        &self,
        dispatch_id: &str,
        fence: u64,
    ) -> Result<Vec<AttemptEventRow>, Error> {
        identifier(dispatch_id, 128)?;
        self.db
            .prepare("SELECT seq,at_ms,phase,detail FROM runner_attempt_events WHERE dispatch_id=?1 AND fence=?2 ORDER BY seq")?
            .query_map(params![dispatch_id, fence], |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .map(|row| {
                let (seq, at_ms, phase, detail) = row?;
                // The 037 CHECK admits only the fourteen words; a row outside
                // them is a schema fault, not a phase.
                let phase = AttemptPhase::parse(&phase).ok_or(Error::Schema)?;
                Ok(AttemptEventRow {
                    seq,
                    at_ms,
                    phase,
                    detail: serde_json::from_str(&detail)?,
                })
            })
            .collect()
    }
    /// One clock column on the attempt row. `Started`, `Parked` and `Settled`
    /// keep their first value (idempotent under the host's repeats);
    /// `LastRenew` always overwrites. `NotFound` when no attempt row exists
    /// for the fence: the clock never invents an attempt.
    pub fn set_attempt_clock(
        &mut self,
        dispatch_id: &str,
        fence: u64,
        clock: AttemptClock,
        at_ms: u64,
    ) -> Result<(), Error> {
        identifier(dispatch_id, 128)?;
        hagency_core::tasks::clock(at_ms)?;
        let column = clock.column();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        attempt_exists(&tx, dispatch_id, fence)?;
        let statement = if clock == AttemptClock::LastRenew {
            format!("UPDATE runner_attempts SET {column}=?3 WHERE dispatch_id=?1 AND fence=?2")
        } else {
            format!(
                "UPDATE runner_attempts SET {column}=?3 WHERE dispatch_id=?1 AND fence=?2 AND {column} IS NULL"
            )
        };
        tx.execute(&statement, params![dispatch_id, fence, at_ms])?;
        tx.commit()?;
        Ok(())
    }
    /// Written once, at settlement or failure: the first writer wins and a
    /// later reason is dropped, not merged. Control characters are replaced
    /// and the text is cut to 640 bytes — the retained product's shape, and
    /// the only free text the private store admits (ADR-181 point 2).
    pub fn set_attempt_terminal_reason(
        &mut self,
        dispatch_id: &str,
        fence: u64,
        reason: &str,
    ) -> Result<(), Error> {
        identifier(dispatch_id, 128)?;
        let reason = bounded_text(reason, TERMINAL_REASON_LIMIT);
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        attempt_exists(&tx, dispatch_id, fence)?;
        tx.execute(
            "UPDATE runner_attempts SET terminal_reason=?3 WHERE dispatch_id=?1 AND fence=?2 AND terminal_reason IS NULL",
            params![dispatch_id, fence, reason],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// The attempt row's clock and terminal reason; `NotFound` when the
    /// fence never had an attempt.
    pub fn attempt_clock(&self, dispatch_id: &str, fence: u64) -> Result<AttemptClockRow, Error> {
        identifier(dispatch_id, 128)?;
        self.db
            .query_row(
                "SELECT started_at,parked_at,last_renew_at,settled_at,terminal_reason FROM runner_attempts WHERE dispatch_id=?1 AND fence=?2",
                params![dispatch_id, fence],
                |r| {
                    Ok(AttemptClockRow {
                        started_at: r.get(0)?,
                        parked_at: r.get(1)?,
                        last_renew_at: r.get(2)?,
                        settled_at: r.get(3)?,
                        terminal_reason: r.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or(Error::NotFound)
    }
}
fn attempt_exists(tx: &Transaction<'_>, dispatch_id: &str, fence: u64) -> Result<(), Error> {
    let known: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM runner_attempts WHERE dispatch_id=?1 AND fence=?2)",
        params![dispatch_id, fence],
        |r| r.get(0),
    )?;
    if known { Ok(()) } else { Err(Error::NotFound) }
}
