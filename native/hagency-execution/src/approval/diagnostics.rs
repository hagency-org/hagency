//! Test-only approval phase diagnostics (ADR-046 stage-1, no behavior change).
//!
//! Two structures, both compiled only under `cfg(test)` or the default-off
//! `test-diagnostics` feature:
//!
//! * a process-wide **phase journal** — every phase label appended to a
//!   pending entry's trace, in arrival order, so a test can read the exact
//!   sequence one entry drove through the coordinator;
//! * a **cancellation slot** — the offending entry ids and their traces at
//!   the moment a cancellation primitive fired, so a failing assertion can
//!   name the primitive (resolved-before-write vs turn-ended-unwritten), the
//!   entry, and every phase that entry had reached.
//!
//! Production builds compile none of this: no field, no string, no slot.
use std::sync::Mutex;

/// (request id, phase label) in arrival order across the whole process.
static PHASES: Mutex<Vec<(String, &'static str)>> = Mutex::new(Vec::new());
/// (primitive, request id, entry trace at cancellation) per cancelled entry.
static CANCELLED: Mutex<Vec<(&'static str, String, Vec<&'static str>)>> = Mutex::new(Vec::new());

/// Append one phase observation to the journal. Called from
/// [`super::state::Pending::mark`] only.
pub fn phase(id: &str, label: &'static str) {
    if let Ok(mut journal) = PHASES.lock() {
        journal.push((id.to_owned(), label));
    }
}

/// Record one cancellation: which primitive fired, for which entry, with the
/// entry's complete phase trace (the cancellation label included).
pub fn cancellation(primitive: &'static str, id: &str, trace: &[&'static str]) {
    if let Ok(mut slot) = CANCELLED.lock() {
        slot.push((primitive, id.to_owned(), trace.to_vec()));
    }
}

/// Every recorded cancellation formatted for a panic message:
/// `primitive on id: phase, phase, …`. Empty when none fired.
pub fn last_cancellation_trace() -> String {
    let Ok(slot) = CANCELLED.lock() else {
        return String::new();
    };
    slot.iter()
        .map(|(primitive, id, trace)| format!("{primitive} on {id}: {}", trace.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The recorded phase labels of one entry, in arrival order.
pub fn phases_of(id: &str) -> Vec<&'static str> {
    PHASES
        .lock()
        .map(|journal| {
            journal
                .iter()
                .filter(|(entry, _)| entry == id)
                .map(|(_, label)| *label)
                .collect()
        })
        .unwrap_or_default()
}

/// Clear both structures. Tests call this before a deterministic drive.
pub fn reset() {
    if let Ok(mut journal) = PHASES.lock() {
        journal.clear();
    }
    if let Ok(mut slot) = CANCELLED.lock() {
        slot.clear();
    }
}
