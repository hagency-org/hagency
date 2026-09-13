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
//! Both are keyed by the **operation's dispatch id** (`RunnerCapability::
//! dispatch_id`): hosted CI runs tests in parallel inside one process, and a
//! process-wide slot interleaved records from different tests — the VM trace
//! once listed two different phase histories for the same entry id because
//! two tests each drove an `approval-1`. A test reads only its own
//! operation's records; [`reset`] still clears everything.
//!
//! Production builds compile none of this: no field, no string, no slot.
use std::sync::Mutex;

/// One journal observation: (dispatch id, request id, phase label).
type PhaseRecord = (String, String, &'static str);
/// One recorded cancellation: (dispatch id, primitive, request id, trace).
type CancellationRecord = (String, &'static str, String, Vec<&'static str>);

/// Phase records in arrival order, whole process.
static PHASES: Mutex<Vec<PhaseRecord>> = Mutex::new(Vec::new());
/// Cancellation records in arrival order, whole process.
static CANCELLED: Mutex<Vec<CancellationRecord>> = Mutex::new(Vec::new());

/// Append one phase observation to the journal, attributed to the operation
/// (dispatch) that drove it. Called from [`super::state::Pending::mark`] only.
pub fn phase(dispatch: &str, id: &str, label: &'static str) {
    if let Ok(mut journal) = PHASES.lock() {
        journal.push((dispatch.to_owned(), id.to_owned(), label));
    }
}

/// Record one cancellation under its dispatch id: which primitive fired, for
/// which entry, with the entry's complete phase trace (the cancellation label
/// included).
pub fn cancellation(dispatch: &str, primitive: &'static str, id: &str, trace: &[&'static str]) {
    if let Ok(mut slot) = CANCELLED.lock() {
        slot.push((
            dispatch.to_owned(),
            primitive,
            id.to_owned(),
            trace.to_vec(),
        ));
    }
}

/// The recorded cancellations of one operation (dispatch id), formatted for a
/// panic message: `primitive on id: phase, phase, …`. Empty when none fired
/// for that dispatch — records of other operations running in parallel are
/// deliberately invisible here.
pub fn last_cancellation_trace(dispatch: &str) -> String {
    let Ok(slot) = CANCELLED.lock() else {
        return String::new();
    };
    slot.iter()
        .filter(|(owner, _, _, _)| owner == dispatch)
        .map(|(_, primitive, id, trace)| format!("{primitive} on {id}: {}", trace.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The recorded phase labels of one entry under one dispatch, in arrival
/// order.
pub fn phases_of(dispatch: &str, id: &str) -> Vec<&'static str> {
    PHASES
        .lock()
        .map(|journal| {
            journal
                .iter()
                .filter(|(owner, entry, _)| owner == dispatch && entry == id)
                .map(|(_, _, label)| *label)
                .collect()
        })
        .unwrap_or_default()
}

/// Every entry of one dispatch with its complete ordered phase history,
/// formatted for a panic message: `id[label, label, …]`, entries in
/// first-appearance order. The labels carry the coordinator flags a failing
/// assertion needs: `admitted` (admission acknowledged), `in-flight` (the
/// frame is armed), `write-accepted` (the transport receipt is on the
/// entry), `recorded` (the durable row is written) — an absent label means
/// that flag was never set on that path. Cancellation primitives
/// (`resolved-before-write`, `turn-ended-unwritten`) are recorded separately
/// by [`last_cancellation_trace`].
pub fn dispatch_trace(dispatch: &str) -> String {
    let Ok(journal) = PHASES.lock() else {
        return String::new();
    };
    let mut entries: Vec<(String, Vec<&'static str>)> = Vec::new();
    for (owner, id, label) in journal.iter() {
        if owner != dispatch {
            continue;
        }
        match entries.iter_mut().find(|(entry, _)| entry == id) {
            Some((_, labels)) => labels.push(*label),
            None => entries.push((id.clone(), vec![*label])),
        }
    }
    entries
        .iter()
        .map(|(id, labels)| format!("{id}[{}]", labels.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Clear both structures, every dispatch. Tests call this before a
/// deterministic drive.
pub fn reset() {
    if let Ok(mut journal) = PHASES.lock() {
        journal.clear();
    }
    if let Ok(mut slot) = CANCELLED.lock() {
        slot.clear();
    }
}
