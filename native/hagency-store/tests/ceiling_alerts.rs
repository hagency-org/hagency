mod common;
use common::*;
use hagency_core::project::Resource;
use hagency_core::tasks::*;
use hagency_metering::{Framework, observation::UsageObservation};
use hagency_store::{
    AlertTransition, DomainRepository, EffectOutcome, Error, SweepOutcome, allowed_transitions,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use std::path::PathBuf;

/// JSON-safe generous ceiling: `Tokens` refuses anything above JSON_SAFE_MAX,
/// so an unrepresentable "infinite" figure would panic at deserialization.
const GENEROUS: u64 = 9_000_000_000_000_000;

/// Compact alarm fixture: one resource, optional approved engagement, and
/// the dispatch machinery needed to bind a usage source and measure it.
struct Alarm {
    root: tempfile::TempDir,
    db: DomainRepository,
}

fn open(framework: Framework, ceiling: u64) -> Alarm {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let mut pool = resource("alarm_pool", "alarm_seat", ceiling);
    if framework == Framework::Claude {
        pool.framework = "claude".into();
        pool.model = "claude-sonnet-5".into();
        pool.reasoning = None;
    }
    db.put_resource(&pool).unwrap();
    Alarm { root, db }
}

/// The ceiling the sweep must see; the engagement is made at a generous
/// ceiling first (the retained commit-then-lower flow) so approve itself
/// succeeds — an overrun exists precisely because a ceiling was lowered
/// under commitments that were admissible when made.
fn set_ceiling(alarm: &mut Alarm, framework: Framework, ceiling: u64) {
    let mut pool = resource("alarm_pool", "alarm_seat", ceiling);
    if framework == Framework::Claude {
        pool.framework = "claude".into();
        pool.model = "claude-sonnet-5".into();
        pool.reasoning = None;
    }
    alarm.db.put_resource(&pool).unwrap();
}

fn engaged(alarm: &mut Alarm, id: &str, tokens: u64, at: u64) -> String {
    let pool = resource("alarm_pool", "alarm_seat", GENEROUS);
    let ask = request(id, "Worker", &pool, tokens);
    let proof = proof(&ask);
    alarm.db.admit(&proof, at).unwrap();
    alarm
        .db
        .approve(&format!("approve_{id}"), &proof, at)
        .unwrap();
    // A usage source needs an active engagement: apply the approval effect
    // exactly as the usage fixture does (tests/usage.rs), otherwise session
    // registration is refused as runner authority.
    let effect = alarm.db.claim_effect().unwrap().unwrap();
    alarm
        .db
        .observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "offline fixture".into(),
            },
        )
        .unwrap();
    ask.engagement_id().unwrap()
}

fn record(alarm: &mut Alarm, engagement: &str, observation: &UsageObservation, at: u64) {
    let session = "alarm_session_1";
    alarm
        .db
        .register_session(&SessionBinding {
            id: session.into(),
            engagement_id: engagement.into(),
            room_id: "!project:example.test".into(),
            thread_root: Some("$alarm_thread".into()),
        })
        .unwrap();
    alarm
        .db
        .create_canonical_task(session, session, "Observe usage", at)
        .unwrap();
    alarm.db.register_workspace(session).unwrap();
    alarm
        .db
        .enqueue_dispatch(&DispatchInput {
            id: session.into(),
            session_id: session.into(),
            task_id: Some(session.into()),
            resources: vec![ResourceLease {
                id: session.into(),
                exclusive: true,
            }],
            payload: json!({"untrusted_agent_hint":"someone else"}),
        })
        .unwrap();
    let cap = alarm
        .db
        .claim_dispatch("alarm_runner", at, 60000, 120000, 128)
        .unwrap()
        .unwrap();
    let scope = alarm.db.owned_dispatch_scope(&cap, at + 1).unwrap();
    let started = alarm
        .db
        .start_owned_dispatch(&cap, scope.fingerprint(), at + 2)
        .unwrap();
    let source = alarm.db.bind_usage_source(&cap, &started, at + 3).unwrap();
    alarm
        .db
        .record_usage_observation(&source, "alarm_call", observation, at + 4)
        .unwrap();
}

fn claude(input: u64, output: u64, write: u64, read: u64) -> UsageObservation {
    UsageObservation::parse(
        Framework::Claude,
        &json!({"uuid":"message","message":{"usage":{"input_tokens":input,"output_tokens":output,"cache_creation_input_tokens":write,"cache_read_input_tokens":read}}}).to_string(),
    )
    .unwrap()
}

/// Direct row read: the sweep's outcome counts come back typed, and the row
/// contents are inspected exactly the way retained tests read the API body.
/// (summary, detail, runbook, impact, occurrences, resolved_at_ms, resolved_by)
type AlertRow = (
    String,
    String,
    String,
    String,
    i64,
    Option<u64>,
    Option<String>,
);

fn row(alarm: &Alarm) -> Option<AlertRow> {
    let sql = Connection::open(state_path(alarm)).unwrap();
    sql.query_row(
        "SELECT summary,detail,runbook,impact,occurrences,resolved_at_ms,resolved_by FROM ceiling_alerts",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<u64>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        },
    )
    .optional()
    .unwrap()
}

fn state_path(alarm: &Alarm) -> PathBuf {
    alarm.root.path().join("state/domain.sqlite3")
}

/// THE RETAINED CONTRACT (`tests/api-ceiling-overrun-alarm.test.js:86`):
/// committed 1.5M then ceiling lowered to 1M → one alert whose four
/// actionable fields are all present — which is exactly what makes it a
/// WARNING rather than a note (`buildActionability`, alert-store.js:102-138);
/// a port missing any field would file an `info` nobody pages on.
#[test]
fn native_ceiling_alert_sweep_files_warning_with_actionable_fields() {
    let mut alarm = open(Framework::Codex, GENEROUS);
    engaged(&mut alarm, "overcommit", 1_500_000, 1000);
    set_ceiling(&mut alarm, Framework::Codex, 1_000_000);
    let outcome = alarm.db.sweep_ceiling_overruns(1_000_000).unwrap();
    assert_eq!(
        outcome,
        SweepOutcome {
            raised: 1,
            updated: 0,
            resolved: 0,
            pruned: 0
        }
    );
    let (summary, detail, runbook, impact, occurrences, resolved_at, resolved_by) =
        row(&alarm).expect("one open alert");
    assert!(summary.contains("has drawn 1500000 against a ceiling of 1000000"));
    assert!(summary.contains("500000 past it"));
    assert!(runbook.contains("raise the ceiling on preset alarm_pool"));
    assert!(runbook.contains("revoke engagements on resource_"));
    assert!(impact.contains("no new engagement can be approved"));
    assert!(impact.contains("cannot retract a commitment it already granted"));
    assert_eq!(occurrences, 1);
    assert!(resolved_at.is_none() && resolved_by.is_none());
    // detail is a JSON STRING (retained storage shape) with the raw numbers.
    let detail: serde_json::Value = serde_json::from_str(&detail).unwrap();
    assert_eq!(detail["committedTokens"], 1_500_000);
    assert_eq!(detail["measuredTokens"], serde_json::Value::Null);
    assert_eq!(detail["drawnTokens"], 1_500_000);
    assert_eq!(detail["overByTokens"], 500_000);
    assert_eq!(detail["ceilingTokens"], 1_000_000);
    // An alert is diagnostic, never enforcement (ADR-124): with the alert
    // open, admission still refuses by its own rule and nothing else moved.
    let pool = resource("alarm_pool", "alarm_seat", 1_000_000);
    // A distinct agent name: a live engagement named "Worker" already holds
    // this project, and admit refuses a name collision before any ceiling rule.
    let ask = request("post_alert", "PostAlert", &pool, 1_000_000);
    alarm.db.admit(&proof(&ask), 1000).unwrap();
    assert!(matches!(
        alarm.db.approve("approve_post_alert", &proof(&ask), 1000),
        Err(Error::OverCommit { .. })
    ));
}

/// The mutant-killer (`:122`): 1.2M FRESH tokens against a 1M ceiling,
/// cacheRead deliberately huge and excluded. `drawn` must be the fresh
/// figure, never the committed-only figure and never the four-kind total.
/// The retained seed commits zero; a native usage source requires a holding
/// engagement, so committed is the fixture's own 1 and the assertion pins
/// drawn/measured/over — the properties that kill both mutants.
#[test]
fn native_ceiling_alert_measured_over_raises_with_nothing_committed() {
    let mut alarm = open(Framework::Claude, GENEROUS);
    let engagement = engaged(&mut alarm, "measured", 1, 1000);
    record(
        &mut alarm,
        &engagement,
        &claude(900_000, 250_000, 50_000, 9_000_000),
        2000,
    );
    set_ceiling(&mut alarm, Framework::Claude, 1_000_000);
    let outcome = alarm.db.sweep_ceiling_overruns(1_000_000).unwrap();
    assert_eq!(outcome.raised, 1);
    let (_, detail, _, _, _, _, _) = row(&alarm).unwrap();
    let detail: serde_json::Value = serde_json::from_str(&detail).unwrap();
    assert_eq!(detail["measuredTokens"], 1_200_000);
    assert_eq!(detail["drawnTokens"], 1_200_000);
    assert_eq!(detail["overByTokens"], 200_000);
    assert_eq!(detail["committedTokens"], 1);
}

/// Inside (`:179`) and exactly ON (`:187`) raise nothing: `>=` would page an
/// operator whose configuration is exactly right.
#[test]
fn native_ceiling_alert_inside_and_on_boundary_raise_nothing() {
    let mut inside = open(Framework::Codex, GENEROUS);
    engaged(&mut inside, "inside", 500_000, 1000);
    set_ceiling(&mut inside, Framework::Codex, 2_000_000);
    let outcome = inside.db.sweep_ceiling_overruns(1_000_000).unwrap();
    assert_eq!(
        outcome,
        SweepOutcome {
            raised: 0,
            updated: 0,
            resolved: 0,
            pruned: 0
        }
    );
    assert!(row(&inside).is_none());
    let mut exact = open(Framework::Codex, GENEROUS);
    engaged(&mut exact, "exact", 1_000_000, 1000);
    set_ceiling(&mut exact, Framework::Codex, 1_000_000);
    let outcome = exact.db.sweep_ceiling_overruns(1_000_000).unwrap();
    assert_eq!(outcome.raised, 0);
    assert!(row(&exact).is_none());
}

/// Raising the ceiling back resolves without an operator closing anything
/// (`:199`): `resolved_by = 'system'`, same-row transition.
#[test]
fn native_ceiling_alert_resolves_when_draw_falls_back_under() {
    let mut alarm = open(Framework::Codex, GENEROUS);
    engaged(&mut alarm, "resolve_me", 1_500_000, 1000);
    set_ceiling(&mut alarm, Framework::Codex, 1_000_000);
    alarm.db.sweep_ceiling_overruns(1_000_000).unwrap();
    set_ceiling(&mut alarm, Framework::Codex, 2_000_000);
    let outcome = alarm.db.sweep_ceiling_overruns(4_600_000).unwrap();
    assert_eq!(outcome.resolved, 1);
    let (_, _, _, _, _, resolved_at, resolved_by) = row(&alarm).unwrap();
    assert_eq!(resolved_at, Some(4_600_000));
    assert_eq!(resolved_by.as_deref(), Some("system"));
}

/// One alert per resource however many times the sweep runs (`:233`): the
/// repeat count rides ON the row (occurrences), never a second row.
#[test]
fn native_ceiling_alert_dedupes_across_repeated_sweeps() {
    let mut alarm = open(Framework::Codex, GENEROUS);
    engaged(&mut alarm, "dedupe_me", 1_500_000, 1000);
    set_ceiling(&mut alarm, Framework::Codex, 1_000_000);
    let mut total = SweepOutcome::default();
    for at in [1_000_000u64, 4_600_000, 8_200_000] {
        let outcome = alarm.db.sweep_ceiling_overruns(at).unwrap();
        total.raised += outcome.raised;
        total.updated += outcome.updated;
    }
    assert_eq!(total.raised, 1);
    assert_eq!(total.updated, 2);
    let sql = Connection::open(state_path(&alarm)).unwrap();
    let (count, occurrences): (i64, i64) = sql
        .query_row(
            "SELECT COUNT(*), MAX(occurrences) FROM ceiling_alerts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(occurrences, 3);
    // B3 sentinel: the retained store never rewrites the four text fields on
    // dedupe/reopen (`alert-store.js:231-249,254-271` update summary,
    // lastPayload, occurrences — not runbook/impact/recoveryCondition). Plant
    // a sentinel runbook, sweep twice more, and assert it survived: an UPDATE
    // that rewrote runbook would restore the composed wording and fail here.
    drop(sql);
    let sql = Connection::open(state_path(&alarm)).unwrap();
    sql.execute(
        "UPDATE ceiling_alerts SET runbook='sentinel_runbook_unmodified'",
        [],
    )
    .unwrap();
    drop(sql);
    alarm.db.sweep_ceiling_overruns(11_800_000).unwrap();
    let sql = Connection::open(state_path(&alarm)).unwrap();
    let (runbook, occurrences): (String, i64) = sql
        .query_row("SELECT runbook, occurrences FROM ceiling_alerts", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    drop(sql);
    assert_eq!(runbook, "sentinel_runbook_unmodified");
    assert_eq!(occurrences, 4);
}

/// No declared ceiling is unknown, not zero (`:220`): such a resource is
/// skipped entirely rather than reported as past a limit nobody chose.
#[test]
fn native_ceiling_alert_no_ceiling_resource_is_skipped() {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let ceilingless: Resource = serde_json::from_value(
        json!({"presetId":"no_ceiling","seatId":"seat_nc","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium"}),
    )
    .unwrap();
    db.put_resource(&ceilingless).unwrap();
    let outcome = db.sweep_ceiling_overruns(1_000_000).unwrap();
    assert_eq!(outcome, SweepOutcome::default());
    let sql = Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    let count: i64 = sql
        .query_row("SELECT COUNT(*) FROM ceiling_alerts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

/// Oracle replay: the fixture's sweep vectors (computed by the retained
/// JavaScript pinned by sha256) against the native store, plus the
/// reopen-after-resolution transition. Commit-then-lower is the flow every
/// commitment-based vector uses; the month-rollover vector pins
/// unknown-not-zero — a new unmeasured period falls back to reserved and the
/// closed period's overrun resolves.
#[test]
fn native_ceiling_alerts_match_javascript() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/ceiling-vectors.json")).unwrap();
    for vector in fixture["sweeps"].as_array().unwrap() {
        let name = vector["name"].as_str().unwrap();
        if name == "no-ceiling" {
            // No declared ceiling: skipped, nothing materialized.
            let root = tempfile::tempdir().unwrap();
            let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
            db.register(&registration()).unwrap();
            let ceilingless: Resource = serde_json::from_value(
                json!({"presetId":"no_ceiling","seatId":"seat_nc","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium"}),
            )
            .unwrap();
            db.put_resource(&ceilingless).unwrap();
            assert_eq!(db.sweep_ceiling_overruns(1_000_000).unwrap().raised, 0);
            continue;
        }
        let ceiling = vector["ceilingTokens"].as_u64().unwrap();
        let reserved = vector["reserved"].as_u64().unwrap();
        let measured = vector["spent"].as_u64();
        let framework = if measured.is_some() {
            Framework::Claude
        } else {
            Framework::Codex
        };
        let mut alarm = open(framework, GENEROUS);
        // The retained seed may commit zero, but a native usage source needs a
        // holding engagement and a scoped request never asks for zero tokens;
        // one committed token leaves the drawn rule (max of reserved and
        // measured) and every expected sweep state unchanged.
        let engagement = engaged(&mut alarm, &format!("oracle_{name}"), reserved.max(1), 1000);
        if let Some(spent) = measured {
            record(
                &mut alarm,
                &engagement,
                &claude(spent, 0, 0, 9_000_000),
                2000,
            );
        }
        set_ceiling(&mut alarm, framework, ceiling);
        for state in vector["expected"]["states"].as_array().unwrap() {
            let at = state["at"].as_u64().unwrap();
            let outcome = alarm.db.sweep_ceiling_overruns(at).unwrap();
            assert_eq!(
                outcome.raised,
                state["raised"].as_u64().unwrap(),
                "{name}@{at}"
            );
            assert_eq!(
                outcome.updated,
                state["updated"].as_u64().unwrap(),
                "{name}@{at}"
            );
            assert_eq!(
                outcome.resolved,
                state["resolved"].as_u64().unwrap(),
                "{name}@{at}"
            );
        }
        let final_row = &vector["expected"]["finalRow"];
        match row(&alarm) {
            None => assert!(final_row.is_null(), "{name}"),
            Some((_, _, _, _, occurrences, resolved_at, resolved_by)) => {
                assert_eq!(
                    u64::try_from(occurrences).unwrap(),
                    final_row["occurrences"].as_u64().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    resolved_at.is_some(),
                    !final_row["resolvedAt"].is_null(),
                    "{name}"
                );
                if final_row["resolvedBy"].is_string() {
                    assert_eq!(resolved_by.as_deref(), Some("system"), "{name}");
                }
            }
        }
    }
    // Reopen after resolution (the retained store's :254-271 window, here as
    // the same-row reopen the ADR records): over → resolve → over again.
    let mut alarm = open(Framework::Codex, GENEROUS);
    engaged(&mut alarm, "reopen", 1_500_000, 1000);
    set_ceiling(&mut alarm, Framework::Codex, 1_000_000);
    alarm.db.sweep_ceiling_overruns(1_000_000).unwrap();
    set_ceiling(&mut alarm, Framework::Codex, 2_000_000);
    alarm.db.sweep_ceiling_overruns(2_000_000).unwrap();
    set_ceiling(&mut alarm, Framework::Codex, 1_000_000);
    let outcome = alarm.db.sweep_ceiling_overruns(3_000_000).unwrap();
    assert_eq!(outcome.raised, 1, "re-over reopens the same row");
    let sql = Connection::open(state_path(&alarm)).unwrap();
    let (count, occurrences, resolved): (i64, i64, i64) = sql
        .query_row(
            "SELECT COUNT(*), occurrences, resolved_at_ms IS NULL FROM ceiling_alerts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((count, occurrences, resolved), (1, 2, 1));
}

/// The display-state columns (migration 025) for the transition tests.
fn dedupe_key_of(alarm: &Alarm) -> String {
    let sql = Connection::open(state_path(alarm)).unwrap();
    sql.query_row("SELECT dedupe_key FROM ceiling_alerts", [], |r| r.get(0))
        .unwrap()
}

fn display_row(alarm: &Alarm) -> (String, Option<u64>, Option<u64>, Option<String>) {
    let sql = Connection::open(state_path(alarm)).unwrap();
    sql.query_row(
        "SELECT status,transitioned_at_ms,resolved_at_ms,transitioned_by FROM ceiling_alerts",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<u64>>(1)?,
                r.get::<_, Option<u64>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        },
    )
    .unwrap()
}

fn transition(
    alarm: &mut Alarm,
    to: &'static str,
    note: Option<&str>,
) -> Result<hagency_store::CeilingAlert, Error> {
    alarm.db.transition_ceiling_alert(AlertTransition {
        key: dedupe_key_of(alarm),
        to,
        actor: "operator".into(),
        note: note.map(str::to_owned),
        now: 2_000_000,
    })
}

/// One seeded overrun row (the sweep is the only honest way to open one).
fn seeded_overrun() -> Alarm {
    let mut alarm = open(Framework::Codex, GENEROUS);
    engaged(&mut alarm, "transition_pool", 1_500_000, 1000);
    set_ceiling(&mut alarm, Framework::Codex, 1_000_000);
    let outcome = alarm.db.sweep_ceiling_overruns(1_000_000).unwrap();
    assert_eq!(outcome.raised, 1);
    alarm
}

/// The one map's full table: every (from,to) pair over the four states.
/// Legal pairs apply (each display column asserted, `resolved_by` the actor,
/// `resolved_at` exactly on resolved); illegal pairs refuse `Invalid` and
/// write nothing. `resolved` is terminal.
#[test]
fn native_ceiling_alert_transitions_follow_one_legal_map() {
    let legal = [
        ("open", "acknowledged"),
        ("open", "resolved"),
        ("open", "suppressed"),
        ("acknowledged", "resolved"),
        ("acknowledged", "suppressed"),
        ("suppressed", "open"),
        ("suppressed", "resolved"),
    ];
    let states = ["open", "acknowledged", "resolved", "suppressed"];
    for from in states {
        for to in states {
            let mut alarm = seeded_overrun();
            let sql = Connection::open(state_path(&alarm)).unwrap();
            if from != "open" {
                // Seeding display state directly is fixture-only: the
                // transition under test still goes through the store.
                sql.execute("UPDATE ceiling_alerts SET status=?1", [from])
                    .unwrap();
            }
            let result = transition(&mut alarm, to, Some("operator note"));
            if legal.contains(&(from, to)) {
                let alert = result.unwrap_or_else(|e| panic!("{from}->{to} must be legal: {e}"));
                assert_eq!(alert.status, to);
                assert_eq!(alert.note.as_deref(), Some("operator note"));
                assert_eq!(alert.transitioned_by.as_deref(), Some("operator"));
                assert_eq!(alert.transitioned_at_ms, Some(2_000_000));
                let (status, _, resolved_at, by) = display_row(&alarm);
                assert_eq!(status, to);
                assert_eq!(by.as_deref(), Some("operator"));
                if to == "resolved" {
                    assert!(resolved_at.is_some(), "resolved sets resolved_at");
                } else {
                    assert!(resolved_at.is_none(), "only resolved sets resolved_at");
                }
            } else {
                assert!(
                    matches!(result, Err(Error::Invalid(_))),
                    "{from}->{to} must refuse bad_transition"
                );
                let (status, transitioned_at, _, by) = display_row(&alarm);
                assert_eq!(status, from, "a refused transition writes nothing");
                assert!(transitioned_at.is_none() && by.is_none());
            }
        }
    }
    // Unknown key and the bounds: the store's own refusals.
    let mut alarm = seeded_overrun();
    assert!(matches!(
        alarm.db.transition_ceiling_alert(AlertTransition {
            key: "agent_ceiling_overrun:missing".into(),
            to: "acknowledged",
            actor: "operator".into(),
            note: None,
            now: 1,
        }),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        alarm.db.transition_ceiling_alert(AlertTransition {
            key: dedupe_key_of(&alarm),
            to: "acknowledged",
            actor: "a".repeat(129),
            note: None,
            now: 1,
        }),
        Err(Error::Invalid(_))
    ));
}

/// The sweep versus operator status: a suppressed row rides occurrences and
/// is NOT reopened (no window natively — operator release only); any
/// non-resolved row auto-resolves when the figure recovers, `resolved_by`
/// becoming 'system' with the note preserved; a resolved row re-raised
/// reopens as a fresh episode (display state reset).
#[test]
fn native_ceiling_sweep_respects_operator_status() {
    // Suppressed stays suppressed across a re-over.
    let mut alarm = seeded_overrun();
    transition(&mut alarm, "suppressed", Some("known overrun")).unwrap();
    let outcome = alarm.db.sweep_ceiling_overruns(2_000_000).unwrap();
    assert_eq!(outcome.updated, 1, "occurrences still ride");
    let alert = alarm.db.open_ceiling_alerts(100).unwrap().remove(0);
    assert_eq!(alert.status, "suppressed", "the sweep never reopens it");
    assert_eq!(alert.note.as_deref(), Some("known overrun"));
    assert_eq!(alert.occurrences, 2);
    // Acknowledged auto-resolves on recovery like an open row. The map
    // releases a suppression only through `open` (`suppressed` offers `open`
    // and `resolved`), so the operator reopens before acknowledging.
    transition(&mut alarm, "open", None).unwrap();
    transition(&mut alarm, "acknowledged", None).unwrap();
    set_ceiling(&mut alarm, Framework::Codex, GENEROUS);
    let outcome = alarm.db.sweep_ceiling_overruns(3_000_000).unwrap();
    assert_eq!(outcome.resolved, 1);
    let sql = Connection::open(state_path(&alarm)).unwrap();
    let (status, resolved_by, resolved_at): (String, Option<String>, Option<u64>) = sql
        .query_row(
            "SELECT status,resolved_by,resolved_at_ms FROM ceiling_alerts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "resolved");
    assert_eq!(resolved_by.as_deref(), Some("system"));
    assert!(resolved_at.is_some());
    // A re-over after resolution reopens as a fresh episode.
    set_ceiling(&mut alarm, Framework::Codex, 1_000_000);
    let outcome = alarm.db.sweep_ceiling_overruns(4_000_000).unwrap();
    assert_eq!(outcome.raised, 1);
    let (status, resolved_at, note, by) = display_row(&alarm);
    assert_eq!(status, "open", "the episode resets to open");
    assert!(resolved_at.is_none());
    assert!(
        note.is_none(),
        "the operator note does not leak into the new episode"
    );
    assert!(by.is_none());
}

/// Oracle replay for the operator transitions: the fixture's transition
/// vectors (computed by EXECUTING the retained `lib/alert-store.js`,
/// `createAlertStore` with a fake clock, pinned by sha256) against the
/// native store over the four-state subset. Every shared-legal pair drives
/// the native store through the same walk (sweep to raise, transition to
/// `from`, then to `to`), and the terminal refusal is the shared
/// `bad_transition`. Native's additional pairs (acknowledged→suppressed,
/// suppressed→resolved) are pinned by the map test above, not the oracle.
#[test]
fn native_ceiling_alert_transitions_match_javascript() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/ceiling-vectors.json")).unwrap();
    let stat = |word: &str| -> &'static str {
        hagency_store::ALERT_STATUSES
            .into_iter()
            .find(|state| *state == word)
            .unwrap_or_else(|| panic!("unknown state {word}"))
    };
    for vector in fixture["transitions"].as_array().unwrap() {
        let from = vector["from"].as_str().unwrap();
        let to = vector["to"].as_str().unwrap();
        let expected = &vector["expected"];
        let mut alarm = seeded_overrun();
        if from != "open" {
            transition(&mut alarm, stat(from), None)
                .unwrap_or_else(|e| panic!("seeding {from} failed: {e}"));
        }
        let result = transition(&mut alarm, stat(to), None);
        if let Some(refusal) = expected["refusal"].as_str() {
            assert!(
                matches!(result, Err(Error::Invalid(_))),
                "{from}->{to} must refuse like the retained store ({refusal})"
            );
        } else {
            let alert = result.unwrap_or_else(|e| panic!("{from}->{to} must be legal: {e}"));
            assert_eq!(
                alert.status,
                expected["status"].as_str().unwrap(),
                "{from}->{to}"
            );
            match expected["resolvedBy"].as_str() {
                Some(actor) => {
                    assert!(alert.resolved, "{from}->{to} resolves");
                    let sql = Connection::open(state_path(&alarm)).unwrap();
                    let resolved_by: Option<String> = sql
                        .query_row("SELECT resolved_by FROM ceiling_alerts", [], |r| r.get(0))
                        .unwrap();
                    assert_eq!(resolved_by.as_deref(), Some(actor), "{from}->{to}");
                }
                None => assert!(!alert.resolved, "{from}->{to} does not resolve"),
            }
            assert_eq!(
                alert.occurrences,
                expected["occurrences"].as_u64().unwrap_or(1),
                "a transition never touches occurrences"
            );
        }
    }
}

/// Migration 025 over populated rows, in `native_usage_migration`'s shape
/// (tests/usage.rs): rewind a live head-25 database to the 024 table shape
/// — rebuilt by executing migration 024 verbatim — carrying one open and
/// one resolved row in 024 columns only, then reopen and let the store
/// replay 025. The ADD COLUMN defaults and the resolved backfill must land,
/// the real read must serve the open row, and the sweep must still ride
/// occurrences on the upgraded row. NOT an idempotency fixture: the second
/// open runs at head 25 and is a no-op — no ADD COLUMN migration in this
/// store replays over an already-upgraded table (see 025's own comment).
#[test]
fn native_ceiling_alert_schema_upgrade() {
    let alarm = seeded_overrun();
    let (key, resource_id, summary, detail, runbook, impact, recovery, first_seen, last_seen): (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        u64,
        u64,
    ) = {
        let sql = Connection::open(state_path(&alarm)).unwrap();
        sql.query_row(
            "SELECT dedupe_key,resource_id,summary,detail,runbook,impact,recovery_condition,first_seen_ms,last_seen_ms FROM ceiling_alerts",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .unwrap()
    };
    let Alarm { root, db } = alarm;
    drop(db);
    let state = root.path().join("state");
    let sql = Connection::open(state.join("domain.sqlite3")).unwrap();
    sql.execute_batch("DROP TABLE ceiling_alerts;").unwrap();
    sql.execute_batch(include_str!("../src/migrations/024-ceiling-alerts.sql"))
        .unwrap();
    // One open row (the captured overrun, verbatim) and one resolved row,
    // both in 024 columns only: status/note/transitioned_* do not exist.
    sql.execute(
        "INSERT INTO ceiling_alerts(dedupe_key,resource_id,summary,detail,runbook,impact,recovery_condition,occurrences,first_seen_ms,last_seen_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,1,?8,?9)",
        params![
            key,
            resource_id,
            summary,
            detail,
            runbook,
            impact,
            recovery,
            first_seen,
            last_seen
        ],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO ceiling_alerts(dedupe_key,resource_id,summary,detail,runbook,impact,recovery_condition,occurrences,first_seen_ms,last_seen_ms,resolved_at_ms,resolved_by) VALUES(?1,?2,?3,?4,?5,?6,?7,1,?8,?9,500_000,'system')",
        params![
            format!("{key}:resolved"),
            resource_id,
            summary,
            detail,
            runbook,
            impact,
            recovery,
            first_seen,
            last_seen
        ],
    )
    .unwrap();
    sql.pragma_update(None, "user_version", 24).unwrap();
    drop(sql);
    for _ in 0..2 {
        let db = DomainRepository::open(&state).unwrap();
        assert_eq!(db.open_ceiling_alerts(100).unwrap().len(), 1);
    }
    let sql = Connection::open(state.join("domain.sqlite3")).unwrap();
    let head: u64 = sql
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(head, 28);
    // The backfill: a resolved row serves 'resolved' with an empty map.
    // The open-alerts read deliberately excludes resolved rows, so this
    // half is verified on the table the read is served from.
    let (resolved_status, resolved_note): (String, Option<String>) = sql
        .query_row(
            "SELECT status,note FROM ceiling_alerts WHERE resolved_at_ms IS NOT NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(resolved_status, "resolved");
    assert_eq!(resolved_note, None);
    assert!(allowed_transitions(&resolved_status).is_empty());
    // The real read serves the open row over the upgraded columns: the
    // default landed, the note is absent, the three-way next is served.
    let mut db = DomainRepository::open(&state).unwrap();
    let open = &db.open_ceiling_alerts(100).unwrap()[0];
    assert_eq!(open.status, "open");
    assert_eq!(open.note, None);
    assert_eq!(open.occurrences, 1);
    assert_eq!(
        allowed_transitions(&open.status),
        ["acknowledged", "resolved", "suppressed"].as_slice()
    );
    // The sweep still updates the upgraded open row (occurrences ride) and
    // leaves the resolved row alone.
    let outcome = db.sweep_ceiling_overruns(2_000_000).unwrap();
    assert_eq!(outcome.updated, 1);
    let open = &db.open_ceiling_alerts(100).unwrap()[0];
    assert_eq!(open.status, "open");
    assert_eq!(open.occurrences, 2);
    let resolved_status: String = sql
        .query_row(
            "SELECT status FROM ceiling_alerts WHERE resolved_at_ms IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(resolved_status, "resolved");
}
