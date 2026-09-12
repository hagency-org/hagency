use super::*;
use crate::collector::fixtures as common;
use crate::collector::observation::{Trace, observed};
use crate::{HostConfig, HostIdentity, HostRoom};
use hagency_core::{replies::*, tasks::SessionBinding};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn config(
    f: &common::Fixture,
    endpoint: &str,
    identity: HostIdentity,
    generation: u64,
    direct: bool,
) -> HostConfig {
    HostConfig::new(
        identity,
        endpoint,
        common::TOKEN,
        f.root.path().join("sdk"),
        [42; 32],
        vec![HostRoom {
            room_id: "!project:example.test".into(),
            generation,
            privacy: if direct {
                RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                }
            } else {
                RoomPrivacy::Group {}
            },
        }],
        common::limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!("../fixtures/ca.pem"))
    .unwrap()
}
fn state(encrypted: bool) -> Value {
    let mut s = common::state();
    if !encrypted {
        s.as_array_mut()
            .unwrap()
            .retain(|e| e["type"] != "m.room.encryption");
    }
    s
}
fn event(id: &str, body: &str, mentions: &[&str], thread: Option<&str>) -> Value {
    let mut v = json!({"event_id":format!("${id}"),"sender":"@owner:example.test","type":"m.room.message","origin_server_ts":now(),"content":{"msgtype":"m.text","body":body,"m.mentions":{"user_ids":mentions}}});
    if let Some(root) = thread {
        v["content"]["m.relates_to"] = json!({"rel_type":"m.thread","event_id":root});
    }
    v
}
fn sync(token: &str, events: Vec<Value>) -> Value {
    json!({"next_batch":token,"rooms":{"join":{"!project:example.test":{"timeline":{"events":events,"limited":false},"state":{"events":[]}}}},"to_device":{"events":[]}})
}
fn plan() -> HostIntakePlan {
    HostIntakePlan::new(vec!["root".into()]).unwrap()
}
async fn prime(c: &Collector, f: &common::Fixture, fake: &mut common::Fake, encrypted: bool) {
    prime_named(c, f, fake, encrypted, "intake prime", None).await;
}
async fn prime_named(
    c: &Collector,
    f: &common::Fixture,
    fake: &mut common::Fake,
    encrypted: bool,
    callsite: &'static str,
    variant: Option<&'static str>,
) {
    let cancel = CancellationToken::new();
    let result = observed(Trace::new(callsite, variant, None), async {
        let (result, _) = common::scripted(c.collect(&cancel), async {
            fake.next_phase(Some("intake prime: whoami"))
                .await
                .json(200, common::who());
            fake.next_phase(Some("intake prime: sync"))
                .await
                .json(200, common::sync("bootstrap"));
            fake.next_phase(Some("intake prime: room state"))
                .await
                .json(200, state(encrypted));
        })
        .await;
        result
    })
    .await;
    result.unwrap();
    f.store
        .resolve_verified_matrix_session(SessionBinding {
            id: "root".into(),
            engagement_id: f.identity.transport.engagement_id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: None,
        })
        .await
        .unwrap();
}
#[derive(Clone, Copy)]
enum IntakeHttpPhase {
    Whoami,
    Sync,
    RoomState,
    ScriptComplete,
}
struct IntakeHttpObservation {
    batch: u8,
    last: std::cell::Cell<&'static str>,
}
impl IntakeHttpObservation {
    fn new(batch: u8) -> Self {
        assert!(batch < 2);
        Self {
            batch,
            last: std::cell::Cell::new("before intake polling"),
        }
    }
    fn mark(&self, phase: IntakeHttpPhase) -> &'static str {
        let label = match (self.batch, phase) {
            (0, IntakeHttpPhase::Whoami) => "manifest batch 0: whoami",
            (0, IntakeHttpPhase::Sync) => "manifest batch 0: sync",
            (0, IntakeHttpPhase::RoomState) => "manifest batch 0: room state",
            (0, IntakeHttpPhase::ScriptComplete) => "manifest batch 0: HTTP script complete",
            (1, IntakeHttpPhase::Whoami) => "manifest batch 1: whoami",
            (1, IntakeHttpPhase::Sync) => "manifest batch 1: sync",
            (1, IntakeHttpPhase::RoomState) => "manifest batch 1: room state",
            (1, IntakeHttpPhase::ScriptComplete) => "manifest batch 1: HTTP script complete",
            _ => unreachable!("bounded fixture batch"),
        };
        self.last.set(label);
        label
    }
}
async fn run(
    c: &Collector,
    fake: &mut common::Fake,
    value: Value,
    encrypted: bool,
) -> Result<IntakeSummary, Error> {
    run_observed(c, fake, value, encrypted, None).await
}
async fn run_observed(
    c: &Collector,
    fake: &mut common::Fake,
    value: Value,
    encrypted: bool,
    observation: Option<&IntakeHttpObservation>,
) -> Result<IntakeSummary, Error> {
    let cancel = CancellationToken::new();
    let intake = c.intake(plan(), &cancel);
    let script = async {
        fake.next_phase(observation.map(|o| o.mark(IntakeHttpPhase::Whoami)))
            .await
            .json(200, common::who());
        let request = fake
            .next_phase(observation.map(|o| o.mark(IntakeHttpPhase::Sync)))
            .await;
        assert!(request.target.contains("sync?"));
        assert!(request.target.contains("since=bootstrap") || request.target.contains("since="));
        request.json(200, value);
        fake.next_phase(observation.map(|o| o.mark(IntakeHttpPhase::RoomState)))
            .await
            .json(200, state(encrypted));
        if let Some(observation) = observation {
            observation.mark(IntakeHttpPhase::ScriptComplete);
        }
    };
    let (result, _) = if let Some(observation) = observation {
        let observed = async {
            let result = intake.await;
            if result.is_err() {
                eprintln!("observed intake HTTP phase: {}", observation.last.get());
            }
            result
        };
        let result = crate::collector::observation::observed(
            Trace::new("manifest intake", None, Some(observation.batch)),
            async {
                let (result, ()) = common::scripted(observed, script).await;
                result
            },
        )
        .await;
        (result, ())
    } else {
        tokio::join!(intake, script)
    };
    result
}
async fn resume(
    c: &Collector,
    fake: &mut common::Fake,
    encrypted: bool,
) -> Result<IntakeSummary, Error> {
    let cancel = CancellationToken::new();
    let (result, _) = tokio::join!(c.intake(plan(), &cancel), async {
        fake.next().await.json(200, common::who());
        let req = fake.next().await;
        assert!(req.target.ends_with("/state"));
        req.json(200, state(encrypted));
    });
    result
}
async fn status(c: &Collector, fake: &mut common::Fake) -> IntakeStatus {
    let cancel = CancellationToken::new();
    let (r, _) = tokio::join!(c.intake_status(&cancel), async {
        fake.next().await.json(200, common::who());
    });
    r.unwrap()
}
fn rows(f: &common::Fixture, table: &str) -> u64 {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn native_matrix_intake_authenticated_group_thread_and_exact_mentions() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(true).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    let value = sync(
        "messages",
        vec![
            event("one", "@worker body alone", &[], None),
            event("two", "Wrong full MXID", &["@worker:foreign.test"], None),
            event("three", "Actual mention", &["@worker:example.test"], None),
        ],
    );
    assert_eq!(
        run(&c, &mut fake, value.clone(), false)
            .await
            .unwrap()
            .admitted,
        3
    );
    let inbox = f.store.inbox("root".into(), 0, 10, None).await.unwrap();
    assert_eq!(inbox.len(), 3);
    assert!(!inbox[0].wake);
    assert!(!inbox[1].wake);
    assert!(inbox[2].wake);
    assert_eq!(rows(&f, "admitted_messages"), 3);
    assert_eq!(run(&c, &mut fake, value, false).await.unwrap().admitted, 0);
    assert_eq!(rows(&f, "session_inputs"), 3);
    f.store
        .resolve_verified_matrix_session(SessionBinding {
            id: "thread".into(),
            engagement_id: f.identity.transport.engagement_id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: Some("$three".into()),
        })
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let plan = HostIntakePlan::new(vec!["root".into(), "thread".into()]).unwrap();
    let (result, _) = tokio::join!(c.intake(plan, &cancel), async {
        fake.next().await.json(200, common::who());
        fake.next().await.json(
            200,
            sync(
                "thread",
                vec![event(
                    "reply",
                    "Thread input",
                    &["@worker:example.test"],
                    Some("$three"),
                )],
            ),
        );
        fake.next().await.json(200, state(false));
    });
    assert_eq!(result.unwrap().admitted, 1);
    let thread = f.store.inbox("thread".into(), 0, 10, None).await.unwrap();
    assert_eq!(thread.len(), 1);
    assert_eq!(thread[0].message.thread_root.as_deref(), Some("$three"));
    assert_eq!(
        f.store
            .inbox("root".into(), 0, 10, None)
            .await
            .unwrap()
            .len(),
        3
    );
    assert_eq!(status(&c, &mut fake).await.stage, "idle");
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_intake_handoff_busy_and_lost_response_recover_without_device_failure() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    for fault in [1, 2] {
        c.inner
            .handoff_fault
            .store(fault, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            run(
                &c,
                &mut fake,
                sync(
                    &format!("fault{fault}"),
                    vec![event(
                        &format!("input{fault}"),
                        "Frozen work",
                        &["@worker:example.test"],
                        None
                    )]
                ),
                false
            )
            .await,
            Err(if fault == 1 {
                Error::Busy
            } else {
                Error::OutcomeUnknown
            })
        );
        assert!(f.available().await);
        assert_eq!(status(&c, &mut fake).await.stage, "domain_handoff");
        let n = rows(&f, "session_inputs");
        let result = resume(&c, &mut fake, false).await.unwrap();
        assert_eq!(result.admitted, usize::from(fault == 1));
        assert_eq!(result.replayed, usize::from(fault == 2));
        assert_eq!(rows(&f, "session_inputs"), n + u64::from(fault == 1));
    }
    assert_eq!(rows(&f, "session_inputs"), 2);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_intake_rotation_historical_commit_settles_without_new_projection() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    c.inner
        .handoff_fault
        .store(2, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        run(
            &c,
            &mut fake,
            sync(
                "lost",
                vec![event(
                    "committed",
                    "Committed before loss",
                    &["@worker:example.test"],
                    None
                )]
            ),
            false
        )
        .await,
        Err(Error::OutcomeUnknown)
    );
    f.store
        .invalidate_matrix_transport(MatrixTransportInvalidation {
            expected: f.identity.transport.clone(),
            reason: "Real authenticated device replacement".into(),
        })
        .await
        .unwrap();
    assert!(!f.available().await);
    c.close().await.unwrap();
    let mut identity = f.identity.clone();
    identity.transport.generation = 2;
    let c = Collector::new(
        config(&f, &fake.endpoint, identity, 2, false),
        f.store.clone(),
    )
    .unwrap();
    let result = resume(&c, &mut fake, false).await.unwrap();
    assert_eq!(result.admitted, 0);
    assert_eq!(result.replayed, 1);
    assert_eq!(rows(&f, "session_inputs"), 1);
    assert_eq!(status(&c, &mut fake).await.stage, "idle");
    c.close().await.unwrap();
    common::shutdown_domain(&f.store, "historical rotation cleanup").await;
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_intake_crypto_verified_human_dm_no_mention_and_spoof_refusal() {
    for variant in [
        "verified",
        "unverified",
        "missing_key",
        "forged_sender",
        "plaintext",
    ] {
        let f = common::Fixture::new();
        let mut fake = common::Fake::start(true).await;
        let c = Collector::new(
            config(&f, &fake.endpoint, f.identity.clone(), 1, true),
            f.store.clone(),
        )
        .unwrap();
        prime_named(&c, &f, &mut fake, true, "crypto prime", Some(variant)).await;
        let mut value = if variant == "plaintext" {
            let mut v = sync(
                "spoof",
                vec![event(
                    "spoof",
                    "Wire trust flag cannot authenticate plaintext",
                    &[],
                    None,
                )],
            );
            v["rooms"]["join"]["!project:example.test"]["timeline"]["events"][0]["encryption_info"] = json!({"verification_state":"verified","sender":"@owner:example.test","sender_device":"HUMAN"});
            v
        } else {
            c.inner
                .owner
                .lock()
                .await
                .as_ref()
                .unwrap()
                .crypto_fixture(variant != "unverified")
                .await
        };
        if variant == "missing_key" {
            value["rooms"]["join"]["!project:example.test"]["timeline"]["events"][0]["content"]["session_id"] =
                json!("missing-session");
        }
        if variant == "forged_sender" {
            value["rooms"]["join"]["!project:example.test"]["timeline"]["events"][0]["sender"] =
                json!("@other:example.test");
        }
        let cancel = CancellationToken::new();
        let result = observed(Trace::new("crypto intake", Some(variant), None), async {
            let (result, ()) = common::scripted(c.intake(plan(), &cancel), async {
                fake.next().await.json(200, common::who());
                fake.next().await.json(200, value);
                fake.next().await.json(200, state(true));
            })
            .await;
            result
        })
        .await;
        if variant == "verified" {
            assert_eq!(result.unwrap().admitted, 1);
            let inbox = f.store.inbox("root".into(), 0, 10, None).await.unwrap();
            assert_eq!(inbox.len(), 1);
            assert!(inbox[0].wake);
            assert_eq!(inbox[0].message.sender_mxid, "@owner:example.test");
            assert_eq!(inbox[0].message.thread_root, None);
            assert_eq!(inbox[0].message.body, "小白：已验证的私聊，无需提及");
        } else {
            let result = result.unwrap();
            assert_eq!(result.admitted, 0, "{variant}");
            assert_eq!(result.rejected, 1, "{variant}");
            assert_eq!(rows(&f, "admitted_messages"), 0);
            assert_eq!(status(&c, &mut fake).await.stage, "idle");
            assert!(f.available().await);
        }
        observed(Trace::new("crypto close", Some(variant), None), c.close())
            .await
            .unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}

#[tokio::test]
async fn native_matrix_intake_sdk_custody_interrupted_apply_retains_exact_raw_and_targets() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    c.inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .apply_fault()
        .await;
    let raw = sync(
        "sdk_consumed",
        vec![event(
            "unknown",
            "private unknown canary",
            &["@worker:example.test"],
            None,
        )],
    );
    let expected = raw.clone();
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(c.intake(plan(), &cancel), async {
        fake.next().await.json(200, common::who());
        fake.next().await.json(200, raw);
    })
    .await;
    assert_eq!(result, Err(Error::OutcomeUnknown));
    assert_eq!(rows(&f, "admitted_messages"), 0);
    let identity = std::fs::read(f.root.path().join("sdk/identity")).unwrap();
    let frozen = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .batch()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(frozen.raw, expected);
    assert!(frozen.phase == Phase::Applying);
    assert_eq!(frozen.targets[0].session_id, "root");
    assert_eq!(
        c.inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .cursor()
            .await
            .unwrap()
            .as_deref(),
        Some("bootstrap")
    );
    c.close().await.unwrap();
    let mut replacement = f.identity.clone();
    replacement.transport.generation = 2;
    let c = Collector::new(
        config(&f, &fake.endpoint, replacement, 2, false),
        f.store.clone(),
    )
    .unwrap();
    let summary = status(&c, &mut fake).await;
    assert_eq!(summary.stage, "sdk_outcome_unknown");
    assert_eq!(summary.acknowledged, 0);
    assert_eq!(
        std::fs::read(f.root.path().join("sdk/identity")).unwrap(),
        identity
    );
    let pending = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .batch()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.raw, expected);
    assert!(pending.targets == frozen.targets);
    let (result, ()) = common::scripted(c.intake(plan(), &cancel), async {
        fake.next().await.json(200, common::who());
    })
    .await;
    assert_eq!(result, Err(Error::OutcomeUnknown));
    fake.no_request().await;
    assert_eq!(rows(&f, "admitted_messages"), 0);
    c.close().await.unwrap();
    for name in ["matrix-sdk-state.sqlite3", "matrix-sdk-crypto.sqlite3"] {
        assert!(
            !std::fs::read(f.root.path().join("sdk").join(name))
                .unwrap()
                .windows(22)
                .any(|v| v == b"private unknown canary")
        );
    }
    common::shutdown_domain(&f.store, "interrupted apply cleanup").await;
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_intake_handoff_concurrent_cancel_and_negative_room_cannot_admit() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime_named(&c, &f, &mut fake, false, "handoff cancellation prime", None).await;
    c.inner
        .handoff_fault
        .store(3, std::sync::atomic::Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let result = observed(
        Trace::new("handoff cancellation intake", None, None),
        async {
            let (result, ()) = common::scripted(c.intake(plan(), &cancel), async {
                fake.next().await.json(200, common::who());
                fake.next().await.json(
                    200,
                    sync(
                        "cancelled",
                        vec![event(
                            "cancelled",
                            "Frozen old room",
                            &["@worker:example.test"],
                            None,
                        )],
                    ),
                );
                fake.next().await.json(200, state(false));
                c.inner.handoff_reached.notified().await;
                f.store
                    .invalidate_matrix_room(MatrixRoomInvalidation {
                        engagement_id: f.identity.transport.engagement_id.clone(),
                        registration_generation: 1,
                        transport_generation: 1,
                        room_id: "!project:example.test".into(),
                        generation: 2,
                        reason: "Authenticated loss while event waits".into(),
                    })
                    .await
                    .unwrap();
                cancel.cancel();
                c.inner.handoff_continue.notify_one();
            })
            .await;
            result
        },
    )
    .await;
    assert_eq!(result, Err(Error::Generation));
    assert_eq!(rows(&f, "admitted_messages"), 0);
    assert!(f.available().await); // Room evidence changes; no invented device failure.
    assert_eq!(status(&c, &mut fake).await.stage, "quarantined");
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_intake_bounds_limited_duplicate_preserve_raw() {
    for variant in ["limited", "duplicate", "overflow"] {
        let f = common::Fixture::new();
        let mut fake = common::Fake::start(false).await;
        let c = Collector::new(
            config(&f, &fake.endpoint, f.identity.clone(), 1, false),
            f.store.clone(),
        )
        .unwrap();
        prime(&c, &f, &mut fake, false).await;
        let event = event("one", "Retained raw", &[], None);
        let mut raw = sync(variant, vec![event.clone()]);
        let timeline = &mut raw["rooms"]["join"]["!project:example.test"]["timeline"];
        match variant {
            "limited" => timeline["limited"] = json!(true),
            "duplicate" => timeline["events"] = json!([event.clone(), event]),
            _ => {
                timeline["events"] = json!(
                    (0..101)
                        .map(|i| {
                            let mut v = event.clone();
                            v["event_id"] = json!(format!("$event{i}"));
                            v
                        })
                        .collect::<Vec<_>>()
                )
            }
        }
        let expected = raw.clone();
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(c.intake(plan(), &cancel), async {
            fake.next().await.json(200, common::who());
            fake.next().await.json(200, raw);
        })
        .await;
        assert_eq!(
            result,
            Err(match variant {
                "duplicate" => Error::Conflict,
                "overflow" => Error::Capacity,
                _ => Error::Unsupported,
            }),
            "{variant}"
        );
        let batch = c
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .batch()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(batch.raw, expected);
        assert!(batch.phase == Phase::Quarantined);
        assert_eq!(rows(&f, "admitted_messages"), 0);
        assert_eq!(
            c.inner
                .owner
                .lock()
                .await
                .as_ref()
                .unwrap()
                .cursor()
                .await
                .unwrap()
                .as_deref(),
            Some("bootstrap")
        );
        c.close().await.unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
    assert!(HostIntakePlan::new(vec!["same".into(), "same".into()]).is_err());
    assert!(HostIntakePlan::new((0..65).map(|i| format!("session{i}")).collect()).is_err());
}

#[tokio::test]
async fn native_matrix_intake_bounds_receipts_never_evict_and_ack_rollback_preserves_handoff() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    let targets = vec![f.store.matrix_intake_route("root".into()).await.unwrap()];
    let guard = c.inner.owner.lock().await;
    let owner = guard.as_ref().unwrap();
    let raw = sync(
        "rollback",
        vec![event(
            "rollback",
            "Frozen ACK",
            &["@worker:example.test"],
            None,
        )],
    );
    owner.intake_start(raw, targets.clone()).await.unwrap();
    let batch = owner.batch().await.unwrap().unwrap();
    let receipt = f
        .store
        .admit_matrix_event(batch.events[0].observation())
        .await
        .unwrap();
    let ack = Acknowledgement::from(&receipt);
    let sql =
        rusqlite::Connection::open(f.root.path().join("sdk/matrix-sdk-state.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER intake_abort BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;").unwrap();
    assert_eq!(
        owner.intake_ack(batch.digest.clone(), 0, ack.clone()).await,
        Err(Error::OutcomeUnknown)
    );
    assert_eq!(
        owner.batch().await.unwrap().unwrap().acknowledgements.len(),
        0
    );
    sql.execute_batch("DROP TRIGGER intake_abort").unwrap();
    owner
        .intake_ack(batch.digest.clone(), 0, ack.clone())
        .await
        .unwrap();
    owner
        .intake_ack(batch.digest.clone(), 0, ack.clone())
        .await
        .unwrap();
    let mut changed = ack;
    changed.sequence += 1;
    assert_eq!(
        owner.intake_ack(batch.digest.clone(), 0, changed).await,
        Err(Error::Conflict)
    );
    sql.execute_batch("CREATE TRIGGER intake_abort BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;").unwrap();
    assert_eq!(
        owner.intake_finish(batch.digest.clone()).await,
        Err(Error::OutcomeUnknown)
    );
    assert_eq!(
        owner.batch().await.unwrap().unwrap().acknowledgements.len(),
        1
    );
    assert_eq!(owner.cursor().await.unwrap().as_deref(), Some("bootstrap"));
    sql.execute_batch("DROP TRIGGER intake_abort").unwrap();
    drop(sql);
    owner.intake_finish(batch.digest.clone()).await.unwrap();
    owner.intake_finish(batch.digest).await.unwrap();
    for i in 2..64 {
        let raw = sync(&format!("empty{i}"), vec![]);
        owner.intake_start(raw, targets.clone()).await.unwrap();
        let batch = owner.batch().await.unwrap().unwrap();
        owner.intake_finish(batch.digest).await.unwrap();
    }
    assert_eq!(
        owner
            .intake_start(sync("overflow", vec![]), targets.clone())
            .await,
        Err(Error::Capacity)
    );
    owner
        .intake_start(sync("empty2", vec![]), targets.clone())
        .await
        .unwrap();
    let mut changed = sync("empty2", vec![]);
    changed["opaque"] = json!(0.125);
    assert_eq!(
        owner.intake_start(changed, targets).await,
        Err(Error::Conflict)
    );
    assert!(owner.batch().await.unwrap().is_none());
    assert_eq!(owner.cursor().await.unwrap().as_deref(), Some("empty63"));
    assert_eq!(
        owner.sync(common::sync("skip_intake")).await,
        Err(Error::Busy)
    );
    drop(guard);
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(c.collect(&cancel), async {
        fake.next().await.json(200, common::who());
        let request = fake.next().await;
        assert!(request.target.ends_with("/state"));
        request.json(200, state(false));
    })
    .await;
    result.unwrap();
    assert_eq!(rows(&f, "admitted_messages"), 1);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_intake_crypto_verified_receipt_survives_restart_without_redecrypting() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(true).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, true),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, true).await;
    let raw = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .crypto_fixture(true)
        .await;
    c.inner
        .handoff_fault
        .store(2, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        run(&c, &mut fake, raw.clone(), true).await,
        Err(Error::OutcomeUnknown)
    );
    let pending = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .batch()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.raw, raw);
    assert!(pending.events[0].observation().encrypted);
    c.close().await.unwrap();
    let mut identity = f.identity.clone();
    identity.transport.generation = 2;
    let c = Collector::new(
        config(&f, &fake.endpoint, identity, 2, true),
        f.store.clone(),
    )
    .unwrap();
    let result = resume(&c, &mut fake, true).await.unwrap();
    assert_eq!(result.admitted, 0);
    assert_eq!(result.replayed, 1);
    assert_eq!(rows(&f, "session_inputs"), 1);
    assert_eq!(status(&c, &mut fake).await.stage, "idle");
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_intake_bounds_unchanged_observation_token_transfers_cursor_custody() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    assert_eq!(
        run(&c, &mut fake, common::sync("bootstrap"), false)
            .await
            .unwrap()
            .admitted,
        0
    );
    assert!(
        c.inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .intake_mode()
            .await
            .unwrap()
    );
    c.close().await.unwrap();
    let mut identity = f.identity.clone();
    identity.transport.generation = 2;
    let c = Collector::new(
        config(&f, &fake.endpoint, identity, 2, false),
        f.store.clone(),
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(c.collect(&cancel), async {
        fake.next().await.json(200, common::who());
        let req = fake.next().await;
        assert!(req.target.ends_with("/state"));
        req.json(200, state(false));
    })
    .await;
    result.unwrap();
    assert!(
        c.inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .intake_mode()
            .await
            .unwrap()
    );
    assert_eq!(
        c.inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .cursor()
            .await
            .unwrap()
            .as_deref(),
        Some("bootstrap")
    );
    assert_eq!(rows(&f, "admitted_messages"), 0);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_intake_rotation_cancel_negative_and_lost_commit_settle_only_old_receipt() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    c.inner
        .handoff_fault
        .store(5, std::sync::atomic::Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(c.intake(plan(), &cancel), async {
        fake.next().await.json(200, common::who());
        fake.next().await.json(
            200,
            sync(
                "lost_after_negative",
                vec![event(
                    "lost",
                    "Committed old scope",
                    &["@worker:example.test"],
                    None,
                )],
            ),
        );
        fake.next().await.json(200, state(false));
        c.inner.handoff_reached.notified().await;
        assert_eq!(rows(&f, "admitted_messages"), 1);
        f.store
            .invalidate_matrix_room(MatrixRoomInvalidation {
                engagement_id: f.identity.transport.engagement_id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 2,
                reason: "Authenticated loss after domain commit".into(),
            })
            .await
            .unwrap();
        cancel.cancel();
        c.inner.handoff_continue.notify_one();
    })
    .await;
    assert_eq!(result, Err(Error::OutcomeUnknown));
    assert!(f.available().await);
    assert_eq!(status(&c, &mut fake).await.acknowledged, 0);
    c.close().await.unwrap();
    let mut identity = f.identity.clone();
    identity.transport.generation = 2;
    let c = Collector::new(
        config(&f, &fake.endpoint, identity, 3, false),
        f.store.clone(),
    )
    .unwrap();
    let result = resume(&c, &mut fake, false).await.unwrap();
    assert_eq!(result.admitted, 0);
    assert_eq!(result.replayed, 1);
    assert_eq!(rows(&f, "session_inputs"), 1);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_intake_rotation_old_inspector_cannot_retire_new_device_incarnation() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    let mut replacement = f.identity.transport.clone();
    replacement.generation = 2;
    f.store
        .observe_matrix_transport(replacement.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(c.intake_status(&cancel), async {
        fake.next_phase(Some("old inspector: whoami")).await.json(
            200,
            json!({"user_id":"@wrong:example.test","device_id":"OTHER"}),
        );
    })
    .await;
    assert_eq!(result, Err(Error::Identity));
    let state = f
        .store
        .matrix_transport_state(replacement.engagement_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(state.available && state.observation == replacement);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_intake_sdk_custody_restore_checks_authenticated_journal_consistency() {
    use matrix_sdk_base::StateStore;
    use matrix_sdk_sqlite::{SqliteStateStore, SqliteStoreConfig};
    use matrix_sdk_store_encryption::StoreCipher;
    for corrupt in ["identity", "target", "cursor", "raw"] {
        let f = common::Fixture::new();
        let mut fake = common::Fake::start(false).await;
        let c = Collector::new(
            config(&f, &fake.endpoint, f.identity.clone(), 1, false),
            f.store.clone(),
        )
        .unwrap();
        prime(&c, &f, &mut fake, false).await;
        c.inner
            .handoff_fault
            .store(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            run(
                &c,
                &mut fake,
                sync(
                    "pending",
                    vec![event("pending", "Consistent custody", &[], None)]
                ),
                false
            )
            .await,
            Err(Error::Busy)
        );
        c.close().await.unwrap();
        let root = f.root.path().join("sdk");
        let key = [42u8; 32];
        let cipher =
            StoreCipher::import_with_key(&key, &std::fs::read(root.join("journal.key")).unwrap())
                .unwrap();
        let store = SqliteStateStore::open_with_config(
            &SqliteStoreConfig::new(&root)
                .key(Some(&key))
                .pool_max_size(2),
        )
        .await
        .unwrap();
        let name = b"hagency.observer.sync.v1";
        let bytes = store.get_custom_value(name).await.unwrap().unwrap();
        let mut journal: Value = cipher.decrypt_value(&bytes).unwrap();
        match corrupt {
            "identity" => journal["intake"]["sdk_identity"] = json!("another owned device"),
            "target" => {
                journal["intake"]["targets"][0]["sender_mxid"] = json!("@other:example.test")
            }
            "cursor" => {
                journal["intake"]["token"] = json!("other");
                journal["intake"]["raw"]["next_batch"] = json!("other");
                journal["intake"]["digest"] = json!(
                    hagency_core::canonical::transport_digest(&journal["intake"]["raw"]).unwrap()
                );
            }
            _ => journal["intake"]["raw"]["changed"] = json!(0.125),
        }
        store
            .set_custom_value(name, cipher.encrypt_value(&journal).unwrap())
            .await
            .unwrap();
        store.close().await.unwrap();
        drop(store);
        let mut identity = f.identity.clone();
        identity.transport.generation = 2;
        let c = Collector::new(
            config(&f, &fake.endpoint, identity, 2, false),
            f.store.clone(),
        )
        .unwrap();
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(c.intake_status(&cancel), async {
            fake.next().await.json(200, common::who());
        })
        .await;
        assert_eq!(result, Err(Error::Storage), "{corrupt}");
        assert_eq!(rows(&f, "admitted_messages"), 0);
        c.close().await.unwrap();
        common::shutdown_domain(&f.store, "corrupt SDK history cleanup").await;
        fake.close().await;
    }
}

#[tokio::test]
async fn native_matrix_intake_handoff_changed_event_cannot_reuse_old_receipt() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, false).await;
    run(
        &c,
        &mut fake,
        sync(
            "original",
            vec![event("identity", "Original content", &[], None)],
        ),
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        run(
            &c,
            &mut fake,
            sync(
                "changed",
                vec![event("identity", "Changed content", &[], None)]
            ),
            false
        )
        .await,
        Err(Error::Generation)
    );
    assert!(f.available().await);
    assert_eq!(rows(&f, "session_inputs"), 1);
    assert_eq!(status(&c, &mut fake).await.stage, "quarantined");
    observed(Trace::new("changed-event close", None, None), c.close())
        .await
        .unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

mod rejections;

mod attachments;
mod receive;

#[tokio::test]
#[should_panic(expected = "collector completed before its HTTP script: Err(Identity)")]
async fn native_matrix_intake_prime_reports_early_identity() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let mut identity = f.identity.clone();
    identity.transport.device_id = "OTHER".into();
    let c = Collector::new(
        config(&f, &fake.endpoint, identity, 1, false),
        f.store.clone(),
    )
    .unwrap();
    // The actual peer supplies DEVICE_1, so collection refuses before sync.
    // No artificial delay or clock makes an impossible next request time out.
    prime(&c, &f, &mut fake, false).await;
}

#[tokio::test]
#[should_panic(expected = "collector completed before its HTTP script: Err(Identity)")]
async fn native_matrix_intake_observed_reports_early_identity() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    let mut identity = f.identity.clone();
    identity.transport.device_id = "OTHER".into();
    let c = Collector::new(
        config(&f, &fake.endpoint, identity, 1, false),
        f.store.clone(),
    )
    .unwrap();
    let observation = IntakeHttpObservation::new(0);
    // The actual DEVICE_1 response refuses intake before targets or sync;
    // observed driving must expose Identity rather than wait for another request.
    let _ = run_observed(
        &c,
        &mut fake,
        sync("never_received", vec![]),
        false,
        Some(&observation),
    )
    .await;
}
