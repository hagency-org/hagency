use super::*;
use crate::test_common::*;
use hagency_core::tasks::*;
use hagency_runtime::codex::{
    session::{SessionDriver, Settings},
    transport,
};
use hagency_store::{DomainRepository, EffectOutcome};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
struct Fixture {
    root: tempfile::TempDir,
    domain: DomainStore,
    run: UsageRun,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("allocation", "Worker", &pool, 100));
        let engagement = db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "offline".into(),
            },
        )
        .unwrap();
        db.register_session(&SessionBinding {
            id: "session".into(),
            engagement_id: engagement.id,
            room_id: "!project:example.test".into(),
            thread_root: None,
        })
        .unwrap();
        db.register_workspace("work").unwrap();
        db.create_canonical_task("task", "session", "Usage custody", now())
            .unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "dispatch".into(),
            session_id: "session".into(),
            task_id: Some("task".into()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"offline"}),
        })
        .unwrap();
        let cap = db
            .claim_dispatch("host", now(), 60000, 60000, 1)
            .unwrap()
            .unwrap();
        let scope = db.owned_dispatch_scope(&cap, now()).unwrap();
        let started = db
            .start_owned_dispatch(&cap, scope.fingerprint(), now())
            .unwrap();
        let domain = DomainStore::start(db, 16).unwrap();
        let run = UsageRun::bind(domain.clone(), cap, started).await.unwrap();
        Self { root, domain, run }
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    async fn observations(&self) -> u64 {
        self.domain
            .usage_source(self.run.source.clone())
            .await
            .unwrap()
            .observations
    }
}

type Session = SessionDriver<DuplexStream, DuplexStream, DuplexStream>;
struct Peer {
    input: DuplexStream,
    output: DuplexStream,
    _stderr: DuplexStream,
}
async fn read(peer: &mut Peer) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        loop {
            let byte = peer.input.read_u8().await.unwrap();
            bytes.push(byte);
            assert!(bytes.len() < 100000);
            if byte == b'\n' {
                return serde_json::from_slice(&bytes).unwrap();
            }
        }
    })
    .await
    .unwrap()
}
async fn write(peer: &mut Peer, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    tokio::time::timeout(Duration::from_secs(3), peer.output.write_all(&bytes))
        .await
        .unwrap()
        .unwrap();
}
async fn running() -> (Session, Peer) {
    let (stdin, input) = tokio::io::duplex(131072);
    let (stdout, output) = tokio::io::duplex(131072);
    let (stderr, peer_stderr) = tokio::io::duplex(1024);
    let cwd = std::env::temp_dir().join("usage-capture-fixture");
    let settings = Settings::new(cwd.clone(), "fixture-model".into(), "medium".into()).unwrap();
    let mut s = Session::new(
        stdout,
        stdin,
        stderr,
        settings,
        transport::Limits {
            write_timeout_ms: 1000,
            event_wait_ms: 1000,
            lifetime_ms: 30000,
        },
        1000,
    )
    .unwrap();
    let mut p = Peer {
        input,
        output,
        _stderr: peer_stderr,
    };
    let (result, ()) = tokio::join!(s.initialize(), async {
        let r = read(&mut p).await;
        assert_eq!(r["method"], "initialize");
        write(&mut p,json!({"id":r["id"],"result":{"userAgent":"offline/0.153.4","platformFamily":"unix","platformOs":"fixture","codexHome":"/fixture"}})).await;
        assert_eq!(read(&mut p).await["method"], "initialized");
    });
    result.unwrap();
    let (result, ()) = tokio::join!(s.start_thread(), async {
        let r = read(&mut p).await;
        assert_eq!(r["method"], "thread/start");
        write(&mut p,json!({"id":r["id"],"result":{"thread":{"id":"same-thread","cwd":cwd,"status":{"type":"idle"},"turns":[]},"cwd":cwd,"model":"fixture-model","modelProvider":"fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"workspaceWrite"}}})).await;
    });
    result.unwrap();
    let (result, ()) = tokio::join!(s.start_turn("usage fixture".into()), async {
        let r = read(&mut p).await;
        assert_eq!(r["method"], "turn/start");
        write(&mut p,json!({"id":r["id"],"result":{"turn":{"id":"same-turn","status":"inProgress","items":[]}}})).await;
    });
    result.unwrap();
    (s, p)
}
fn usage() -> Value {
    json!({"method":"thread/tokenUsage/updated","params":{"threadId":"same-thread","turnId":"same-turn","tokenUsage":{
        "total":{"totalTokens":110,"inputTokens":100,"cachedInputTokens":40,"cacheWriteInputTokens":60,"outputTokens":10,"reasoningOutputTokens":2},
        "last":{"totalTokens":1,"inputTokens":0,"cachedInputTokens":0,"cacheWriteInputTokens":0,"outputTokens":1,"reasoningOutputTokens":0},
        "modelContextWindow":200000
    }}})
}
async fn observation(s: &mut Session, p: &mut Peer) -> Observation {
    let (result, ()) = tokio::join!(s.next_observed_update(), write(p, usage()));
    result.unwrap().1
}

#[tokio::test]
async fn native_owned_usage_normalization_refusal_retains_evidence() {
    let mut f = Fixture::new().await;
    let (mut s, mut p) = running().await;
    f.run.attach_source(s.observation_source().unwrap());
    let mut bad = usage();
    bad["params"]["tokenUsage"]["total"] = json!({"totalTokens":null,"inputTokens":9_007_199_254_740_991u64,"cachedInputTokens":0,"cacheWriteInputTokens":0,"outputTokens":1,"reasoningOutputTokens":0});
    let (result, ()) = tokio::join!(s.next_observed_update(), write(&mut p, bad));
    let event = result.unwrap().1;
    assert!(!f.run.observe(&event));
    assert!(f.run.status().rejected && f.run.status().closed);
    assert_eq!(f.run.status().failure, Some(UsageFailure::Normalization));
    let evidence = f.run.rejected.as_ref().unwrap();
    assert_eq!(evidence.total().input_tokens(), Some(9_007_199_254_740_991));
    assert_eq!(evidence.total().output_tokens(), Some(1));
    assert_eq!(evidence.total().total_tokens(), None);
    assert_eq!(f.observations().await, 0);
    f.run.record_pending().await.unwrap();
    assert_eq!(f.observations().await, 0);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_usage_source_guards() {
    for mode in ["foreign", "skipped", "retired", "replay", "reattach"] {
        let mut f = Fixture::new().await;
        let (mut s, mut p) = running().await;
        f.run.attach_source(s.observation_source().unwrap());
        let event = observation(&mut s, &mut p).await;
        let expected = match mode {
            "foreign" => {
                let (mut other, mut peer) = running().await;
                let foreign = observation(&mut other, &mut peer).await;
                assert!(event.source() != foreign.source());
                assert!(!f.run.observe(&foreign));
                UsageFailure::Source
            }
            "skipped" => {
                // Even the default update reader consumes a source receipt.
                let (r, ()) = tokio::join!(s.next_update(), write(&mut p, usage()));
                r.unwrap();
                let next = observation(&mut s, &mut p).await;
                assert_eq!(next.sequence(), 3);
                assert!(!f.run.observe(&next));
                UsageFailure::Sequence
            }
            "retired" => {
                s.close();
                assert!(event.source().is_retired());
                assert!(!f.run.observe(&event));
                UsageFailure::Retired
            }
            "replay" => {
                assert!(f.run.observe(&event));
                f.run.record_pending().await.unwrap();
                assert!(!f.run.observe(&event));
                UsageFailure::Sequence
            }
            "reattach" => {
                f.run.attach_source(event.source().clone());
                UsageFailure::Source
            }
            _ => unreachable!(),
        };
        assert_eq!(f.run.status().failure, Some(expected));
        assert!(f.run.status().closed);
        assert!(!f.run.observe(&event));
        assert_eq!(f.observations().await, u64::from(mode == "replay"));
        f.domain.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_owned_usage_pending_retry() {
    for committed in [false, true] {
        let mut f = Fixture::new().await;
        let (mut s, mut p) = running().await;
        f.run.attach_source(s.observation_source().unwrap());
        assert!(f.run.observe(&observation(&mut s, &mut p).await));
        let (entered, ready) = tokio::sync::oneshot::channel();
        let domain = f.domain.clone();
        // Gate either before actual writer invocation or after its real receipt;
        // cancellation drops only the waiter, never the retained pending tuple.
        let mut attempt = Box::pin(f.run.record_with(
            move |source, call, observation| async move {
                if committed {
                    let receipt = domain
                        .record_usage_observation(source, call, observation)
                        .await?;
                    let _ = entered.send(());
                    std::future::pending::<()>().await;
                    Ok(receipt)
                } else {
                    let _ = entered.send(());
                    std::future::pending::<()>().await;
                    domain
                        .record_usage_observation(source, call, observation)
                        .await
                }
            },
        ));
        tokio::select! {
            result=&mut attempt=>panic!("response gate unexpectedly returned: {result:?}"),
            result=ready=>result.unwrap(),
        }
        drop(attempt);
        assert!(f.run.status().pending);
        assert_eq!(f.observations().await, u64::from(committed));
        let before: u64 = f
            .sql()
            .query_row(
                "SELECT COALESCE(SUM(observations),0) FROM usage_periods",
                [],
                |r| r.get(0),
            )
            .unwrap();
        f.run.close(); // End of host capture; retries must never reopen it.
        f.run.record_pending().await.unwrap();
        assert_eq!(f.observations().await, 1);
        let view = f.domain.usage_source(f.run.source.clone()).await.unwrap();
        assert_eq!(view.high_water.input, Some(0));
        assert_eq!(view.high_water.cache_write, Some(60));
        assert_eq!(view.high_water.output, Some(10));
        let after: u64 = f
            .sql()
            .query_row("SELECT SUM(observations) FROM usage_periods", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(after, 2);
        if committed {
            assert_eq!(before, after);
        }
        assert!(f.run.status().closed && !f.run.status().pending);
        f.run.record_pending().await.unwrap(); // No second store call when empty.
        assert_eq!(f.observations().await, 1);
        f.domain.shutdown().await.unwrap();
    }
    // An explicit lost-response result after the actual commit also retains
    // identical retry custody and fences all subsequent source admission.
    let mut f = Fixture::new().await;
    let (mut s, mut p) = running().await;
    f.run.attach_source(s.observation_source().unwrap());
    let event = observation(&mut s, &mut p).await;
    assert!(f.run.observe(&event));
    let domain = f.domain.clone();
    assert_eq!(
        f.run
            .record_with(move |source, call, observation| async move {
                domain
                    .record_usage_observation(source, call, observation)
                    .await?;
                Err(hagency_store::Error::OutcomeUnknown)
            })
            .await,
        Err(UsageFailure::Storage)
    );
    assert!(f.run.status().pending && f.run.status().closed);
    assert!(!f.run.observe(&event));
    f.run.record_pending().await.unwrap();
    assert_eq!(f.observations().await, 1);
    assert!(f.run.status().closed);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_usage_restart_and_capacity() {
    let mut f = Fixture::new().await;
    let (mut s, mut p) = running().await;
    f.run.attach_source(s.observation_source().unwrap());
    assert!(f.run.observe(&observation(&mut s, &mut p).await));
    f.run.record_pending().await.unwrap();
    let mut sql = f.sql();
    let tx = sql.transaction().unwrap();
    for n in 1..hagency_store::MAX_SOURCE_USAGE_RECEIPTS {
        tx.execute("INSERT INTO usage_receipts SELECT source_id,?1,digest,observation,response FROM usage_receipts WHERE call_id='runtime_v1_1'",[format!("historical_{n}")]).unwrap();
    }
    tx.commit().unwrap();
    assert!(f.run.observe(&observation(&mut s, &mut p).await));
    assert_eq!(f.run.record_pending().await, Err(UsageFailure::Storage));
    assert!(f.run.status().closed && f.run.status().pending);
    assert_eq!(f.observations().await, 1);
    assert_eq!(f.run.record_pending().await, Err(UsageFailure::Storage));
    s.close();
    let source_id = f.run.source.id().to_owned();
    let call = f.run.pending.as_ref().unwrap().call_id.clone();
    let evidence = f.run.pending.as_ref().unwrap().observation.clone();
    f.domain.shutdown().await.unwrap();
    let mut db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    let source = db.restore_usage_source(&source_id).unwrap();
    let history = db.usage_source(&source).unwrap();
    assert_eq!(history.observations, 1);
    assert_eq!(history.high_water.cache_write, Some(60));
    assert!(history.historical_incomplete);
    assert!(matches!(
        db.record_usage_observation(&source, &call, &evidence, now()),
        Err(hagency_store::Error::Capacity)
    ));
    // Original identical call still replays despite receipt exhaustion and an
    // earlier clock. Observation 2 has identical typed counters, different ID.
    let replay = db
        .record_usage_observation(&source, "runtime_v1_1", &evidence, 0)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(db.usage_source(&source).unwrap().observations, 1);
}
