//! ADR-181, the store side: the per-attempt event log, the attempt row's
//! clock and terminal reason, and the `lost` event a lease loss leaves
//! behind. Every scenario drives the store's own entry points: a dispatch is
//! claimed and started the way the host does it, and the loss is the writer
//! call's own `expire()`, not a planted row.
mod common;
use common::*;
use hagency_core::tasks::*;
use hagency_store::{
    AttemptClock, AttemptClockRow, AttemptEvent, AttemptPhase, DomainRepository, EffectOutcome,
    Error,
};
use serde_json::{Value, json};

/// The fourteen phases in the order an attempt can visit them.
const PHASES: [AttemptPhase; 14] = [
    AttemptPhase::Claimed,
    AttemptPhase::SpawnStarted,
    AttemptPhase::SpawnDone,
    AttemptPhase::Initialized,
    AttemptPhase::TurnStarted,
    AttemptPhase::ApprovalRequested,
    AttemptPhase::ApprovalDecided,
    AttemptPhase::Parked,
    AttemptPhase::Resumed,
    AttemptPhase::StopRequested,
    AttemptPhase::StopReported,
    AttemptPhase::Settled,
    AttemptPhase::Failed,
    AttemptPhase::Lost,
];

/// Three engagements with one plain session each, so three dispatches can be
/// live in turn: a lost session is quarantined and never claims again.
struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        for (id, name) in [("a", "小白"), ("b", "Edison"), ("c", "Other")] {
            let req = request(id, name, &pool, 100);
            let p = proof(&req);
            let e = db.admit(&p, 1000).unwrap();
            db.approve(&format!("approve_{id}"), &p, 1000).unwrap();
            let effect = db.claim_effect().unwrap().unwrap();
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: format!("fixture_{id}"),
                },
            )
            .unwrap();
            db.register_session(&SessionBinding {
                id: id.into(),
                engagement_id: e.id,
                room_id: req.target_room_id,
                thread_root: None,
            })
            .unwrap();
        }
        Self { root, db }
    }
    /// Enqueue on `session`, claim (which is also the writer call whose
    /// `expire()` settles every overdue lease) and start at `now`.
    fn start(&mut self, id: &str, session: &str, now: u64) -> RunnerCapability {
        self.db
            .enqueue_dispatch(&DispatchInput {
                id: id.into(),
                session_id: session.into(),
                task_id: None,
                resources: vec![],
                payload: json!({"instruction":"fixture"}),
            })
            .unwrap();
        let cap = self
            .db
            .claim_dispatch("runner", now, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
        assert_eq!(cap.dispatch_id, id);
        self.db.start_dispatch(&cap, now + 1).unwrap();
        cap
    }
    fn record(
        &mut self,
        cap: &RunnerCapability,
        phase: AttemptPhase,
        detail: Value,
        now: u64,
    ) -> Result<u64, Error> {
        self.db.record_attempt_event(
            &AttemptEvent {
                dispatch_id: cap.dispatch_id.clone(),
                fence: cap.fence,
                phase,
                detail,
            },
            now,
        )
    }
    fn state(&self, id: &str) -> String {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3"))
            .unwrap()
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
    }
}

/// Scenario "An attempt's phases are recorded in order with their clock",
/// the store half: fourteen phases read back in seq order with their clock;
/// the detail bound (control characters replaced, strings cut at 4 KiB,
/// the object refused past 8 KiB, keys and depth checked); the 256-event
/// cap; an unknown dispatch; and a refused write leaving the next one whole.
#[test]
fn native_attempt_events_store_bounds() {
    let mut f = Fixture::new();
    let cap = f.start("first", "a", 1001);
    for (i, phase) in PHASES.into_iter().enumerate() {
        let detail = match phase {
            AttemptPhase::Claimed => json!({"note":"a\u{0}b\nc","engagement":"a"}),
            AttemptPhase::StopReported => {
                json!({"stderr_tail":"x".repeat(5000),"rows":[{"pid":7,"ppid":1,"exe":"node"}]})
            }
            _ => json!({"phase":phase.as_str(),"elapsed_ms":i}),
        };
        let seq = f.record(&cap, phase, detail, 3000 + i as u64).unwrap();
        assert_eq!(seq, i as u64 + 1);
    }
    let rows = f.db.attempt_events("first", 1).unwrap();
    assert_eq!(rows.len(), 14);
    for (i, (row, phase)) in rows.iter().zip(PHASES).enumerate() {
        assert_eq!(row.seq, i as u64 + 1);
        assert_eq!(row.at_ms, 3000 + i as u64);
        assert_eq!(row.phase, phase);
        assert!(row.detail.is_object());
    }
    // The bound is applied on the way in, so the row carries the clean copy.
    assert_eq!(rows[0].detail["note"], "a\u{fffd}b\u{fffd}c");
    assert_eq!(rows[0].detail["engagement"], "a");
    assert_eq!(rows[10].detail["stderr_tail"].as_str().unwrap().len(), 4096);
    assert_eq!(rows[10].detail["rows"][0]["exe"], "node");
    assert!(f.db.attempt_events("first", 2).unwrap().is_empty());
    // Refusals: an object over 8 KiB after the string cut, a non-object, a
    // key that is not an identifier, a container three deep. Each is
    // refused before anything is written.
    let refused = [
        json!({"a":"x".repeat(3000),"b":"y".repeat(3000),"c":"z".repeat(3000)}),
        json!(["not", "an", "object"]),
        json!({"bad key":1}),
        json!({"k".repeat(65):1}),
        json!({"rows":[{"nested":{"too":"deep"}}]}),
    ];
    for detail in refused {
        assert!(
            matches!(
                f.record(&cap, AttemptPhase::Failed, detail, 4000),
                Err(Error::Invalid(_))
            ),
            "refused detail"
        );
    }
    assert!(matches!(
        f.db.record_attempt_event(
            &AttemptEvent {
                dispatch_id: "unknown".into(),
                fence: 1,
                phase: AttemptPhase::Claimed,
                detail: json!({}),
            },
            4000,
        ),
        Err(Error::NotFound)
    ));
    // A refused write is its own rolled-back savepoint: the next valid
    // observation on the same attempt lands, with the next seq.
    assert_eq!(
        f.record(
            &cap,
            AttemptPhase::Resumed,
            json!({"after":"refusal"}),
            4001
        )
        .unwrap(),
        15
    );
    assert_eq!(f.db.attempt_events("first", 1).unwrap().len(), 15);
    // The cap: 256 events per (dispatch, fence); the 257th is refused and
    // the log is unchanged.
    for n in 16..=256 {
        assert_eq!(
            f.record(&cap, AttemptPhase::Resumed, json!({"n":n}), 4000 + n)
                .unwrap(),
            n
        );
    }
    assert!(matches!(
        f.record(&cap, AttemptPhase::Settled, json!({}), 5000),
        Err(Error::Capacity)
    ));
    assert_eq!(f.db.attempt_events("first", 1).unwrap().len(), 256);
}

/// Scenario "A lease loss names the writer that settled it": three started
/// dispatches, each settled by a different writer call's `expire()` — the
/// claim path, `reconcile_dispatches`, and the recovery a reopen runs — and
/// each `lost` event names its writer, the `lease_until` it judged against,
/// the `now` it judged at and the `last_renew_at` the lease had.
#[test]
fn native_lease_loss_names_its_writer() {
    let mut f = Fixture::new();
    let lost = |f: &Fixture, id: &str| -> Value {
        let rows = f.db.attempt_events(id, 1).unwrap();
        assert_eq!(rows.len(), 1, "{id}: exactly the lost event");
        assert_eq!(rows[0].phase, AttemptPhase::Lost);
        rows[0].detail.clone()
    };
    // The claim path: a started dispatch renewed once, then a claim with an
    // empty queue whose expire() finds the lease overdue.
    let first = f.start("first", "a", 1001);
    f.db.renew_dispatch(&first, 5000, 60_000).unwrap();
    assert_eq!(
        f.db.attempt_clock("first", 1).unwrap().last_renew_at,
        Some(5000)
    );
    assert!(f.db.attempt_events("first", 1).unwrap().is_empty());
    assert!(
        f.db.claim_dispatch("runner", 70_000, 60_000, 120_000, 8)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.state("first"), "outcome_unknown");
    assert_eq!(
        lost(&f, "first"),
        json!({"writer":"claim","lease_until":65_000,"now":70_000,"last_renew_at":5000})
    );
    // The reconciliation tick, on a lease never renewed.
    let third = f.start("third", "c", 71_000);
    assert_eq!(third.fence, 1);
    f.db.reconcile_dispatches(140_000).unwrap();
    assert_eq!(f.state("third"), "outcome_unknown");
    assert_eq!(
        lost(&f, "third"),
        json!({"writer":"reconcile","lease_until":131_000,"now":140_000,"last_renew_at":null})
    );
    // The restart: a live lease at close is settled by the reopen's own
    // recovery, judged at the reopen's clock.
    f.start("second", "b", 141_000);
    let root = f.root;
    drop(f.db);
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    let f = Fixture { root, db };
    assert_eq!(f.state("second"), "outcome_unknown");
    let detail = lost(&f, "second");
    assert_eq!(detail["writer"], "restart");
    assert_eq!(detail["lease_until"], 201_000);
    assert_eq!(detail["last_renew_at"], Value::Null);
    assert!(detail["now"].as_u64().unwrap() > 201_000);
}

/// The attempt row's clock and terminal reason: `Started`, `Parked` and
/// `Settled` keep their first value, `LastRenew` follows every renewal (the
/// store's own renewal path included), `terminal_reason` is first-writer-
/// wins, control-stripped and cut at 640 bytes on a char boundary, and no
/// clock invents an attempt.
#[test]
fn native_attempt_clock_and_terminal_reason() {
    let mut f = Fixture::new();
    let cap = f.start("first", "a", 1001);
    assert_eq!(
        f.db.attempt_clock("first", 1).unwrap(),
        AttemptClockRow {
            started_at: None,
            parked_at: None,
            last_renew_at: None,
            settled_at: None,
            terminal_reason: None,
        }
    );
    for (clock, first, again) in [
        (AttemptClock::Started, 2000, 2500),
        (AttemptClock::Parked, 3000, 3500),
        (AttemptClock::Settled, 9000, 9500),
    ] {
        f.db.set_attempt_clock("first", 1, clock, first).unwrap();
        f.db.set_attempt_clock("first", 1, clock, again).unwrap();
    }
    f.db.set_attempt_clock("first", 1, AttemptClock::LastRenew, 4000)
        .unwrap();
    f.db.set_attempt_clock("first", 1, AttemptClock::LastRenew, 4500)
        .unwrap();
    let row = f.db.attempt_clock("first", 1).unwrap();
    assert_eq!(
        (
            row.started_at,
            row.parked_at,
            row.settled_at,
            row.last_renew_at
        ),
        (Some(2000), Some(3000), Some(9000), Some(4500))
    );
    f.db.renew_dispatch(&cap, 5000, 60_000).unwrap();
    assert_eq!(
        f.db.attempt_clock("first", 1).unwrap().last_renew_at,
        Some(5000)
    );
    f.db.set_attempt_terminal_reason("first", 1, "lost_authority:signal:9:tail\u{0}x\r\n")
        .unwrap();
    f.db.set_attempt_terminal_reason("first", 1, "completed:exit:0:")
        .unwrap();
    assert_eq!(
        f.db.attempt_clock("first", 1)
            .unwrap()
            .terminal_reason
            .as_deref(),
        Some("lost_authority:signal:9:tail\u{fffd}x\u{fffd}\u{fffd}")
    );
    // The cut lands on a char boundary: 300 three-byte chars are 900 bytes,
    // and 640 falls one byte into the 214th.
    f.start("second", "b", 1010);
    f.db.set_attempt_terminal_reason("second", 1, &"€".repeat(300))
        .unwrap();
    let reason =
        f.db.attempt_clock("second", 1)
            .unwrap()
            .terminal_reason
            .unwrap();
    assert_eq!(reason.len(), 639);
    assert!(reason.chars().all(|c| c == '€'));
    // No attempt row for the fence: nothing is invented.
    assert!(matches!(
        f.db.set_attempt_clock("first", 7, AttemptClock::Started, 1),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        f.db.set_attempt_terminal_reason("nowhere", 1, "x"),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        f.db.attempt_clock("first", 7),
        Err(Error::NotFound)
    ));
}
