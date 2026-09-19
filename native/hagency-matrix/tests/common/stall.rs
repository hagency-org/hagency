//! Test-only, process-wide shutdown-stall latch (ADR-106 harness work).
#![allow(dead_code)]
//!
//! One root cause, N witnesses: the first domain or custody shutdown whose
//! `ShutdownOutcome` is a timeout records itself here — fixture label, the
//! `ShutdownSnapshot` Debug, and an optional two-point teardown sample. Every
//! later failure path calls [`witness`] before its own panic, so a sibling
//! test that fails because the process stalled reports the stall as the
//! stall. Nothing is retried, widened or ignored: every affected test still
//! fails and the binary still exits 101.
//!
//! Placement contract: this file must be compiled exactly ONCE per test
//! binary (the `OnceLock` below is per module instantiation). It is included
//! as a submodule of `common/mod.rs`, and — in test binaries that use the
//! hagency-store common instead — directly from the crate root via
//! `#[path]`. Never include it twice in one binary.

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// A recorded stall: the first timed-out shutdown observed in this process.
pub struct Stall {
    /// Fixture label naming where the stall was observed.
    pub label: String,
    /// `ShutdownOutcome` name, e.g. `ReplyTimedOut`.
    pub outcome: String,
    /// `ShutdownSnapshot` Debug at the moment the timeout was reported.
    pub snapshot: String,
    /// Two-point database teardown sample, when the fixture knows its path.
    pub teardown: Option<String>,
}

static LATCH: OnceLock<Option<Stall>> = OnceLock::new();
static RECORD_GUARD: Mutex<()> = Mutex::new(());

fn latch() -> &'static Option<Stall> {
    LATCH.get_or_init(|| None)
}

/// Whether a `ShutdownOutcome` name is a stall (a timeout), per ADR-106:
/// `ReplyTimedOut` and `EnqueueTimedOut` are the two outcomes that mean the
/// close outlived its two-second budget.
#[must_use]
pub fn timed_out(outcome: &str) -> bool {
    outcome == "ReplyTimedOut" || outcome == "EnqueueTimedOut"
}

/// Record the first stall of this process. Later stalls change nothing: the
/// first record is the root cause every witness reports, and a second
/// shutdown timing out is a consequence, not a new cause.
pub fn record_if_timed_out(outcome: &str, label: &str, snapshot: String, teardown: Option<String>) {
    if !timed_out(outcome) {
        return;
    }
    let _guard = RECORD_GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // First writer wins: `set` succeeds only while the slot is unpopulated.
    let _ = LATCH.set(Some(Stall {
        label: label.to_owned(),
        outcome: outcome.to_owned(),
        snapshot,
        teardown,
    }));
}

/// Consult the latch on a failure path. When a stall was recorded in this
/// process, print it labelled `[shutdown-stall witness]` and return `true`:
/// the caller reports the recorded root cause as the reason it failed. The
/// caller still fails; this hides nothing.
#[must_use]
pub fn witness(context: &str) -> bool {
    let Some(stall) = latch() else {
        return false;
    };
    let teardown = stall
        .teardown
        .as_deref()
        .unwrap_or("no teardown sample available");
    eprintln!(
        "[shutdown-stall witness] {context} fails in a process with a recorded shutdown stall; \
         root cause recorded by {} ({}): {} | teardown: {}",
        stall.label, stall.outcome, stall.snapshot, teardown
    );
    true
}

/// One read of the database files' sizes and mtimes. Two reads ~500 ms apart
/// distinguish "the close finishes late" from "the close never finishes".
#[must_use]
pub fn sample_database(state_dir: &Path) -> String {
    let mut parts = Vec::new();
    for name in ["domain.sqlite3", "domain.sqlite3-wal", "domain.sqlite3-shm"] {
        let file = state_dir.join(name);
        match std::fs::metadata(&file) {
            Ok(meta) => {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|when| when.duration_since(UNIX_EPOCH).ok())
                    .map(|since| since.as_millis())
                    .unwrap_or(0);
                parts.push(format!("{name}: {} bytes, mtime {} ms", meta.len(), mtime));
            }
            Err(_) => parts.push(format!("{name}: absent")),
        }
    }
    parts.join("; ")
}

/// Two-point teardown sample: now and ~500 ms later. Blocking sleep is
/// acceptable here because this runs only on a failure path that is about to
/// panic the test binary anyway.
#[must_use]
pub fn two_point_sample(state_dir: &Path) -> String {
    let first = sample_database(state_dir);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let second = sample_database(state_dir);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or(0);
    format!("at-timeout[{first}] after-500ms[{second}] sampled-at {now} ms")
}
