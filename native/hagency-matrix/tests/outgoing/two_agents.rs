//! MA-M8a (ADR-144): the in-process two-agent acceptance, over the real-TLS
//! fake peer and the two-engagement `PairFixture`. One shared DELIVERY room
//! that is never an approval room, one direct room per engagement, and the
//! retained oracle's properties plus its two refusals. Every HTTP loop runs
//! INSIDE the `common::scripted` future and ends on a quiet window, so the
//! exact preflight counts never couple to the assertions.
use super::*;
use crate::collector::observation::{Phase as ObservationPhase, Trace, observed};
use common::pair::{DM_A, DM_B, OWNER, PairFixture, SHARED_ROOM, state_for, who};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn pair_config(pair: &PairFixture, agent: &HostIdentity, dm: &str, endpoint: &str) -> HostConfig {
    pair.config(agent, dm, endpoint)
        .with_root_pem(include_bytes!("../fixtures/ca.pem"))
        .unwrap()
}
/// Answer bootstrap traffic for one agent — whoami as ITS OWN identity,
/// empty syncs, room state for any number of rooms — until the peer falls
/// quiet.
async fn serve_bootstrap(fake: &mut common::Fake, agent: &HostIdentity) {
    loop {
        let request = tokio::select! {
            request = fake.next() => request,
            _ = tokio::time::sleep(Duration::from_millis(300)) => return,
        };
        if request.target.contains("/account/whoami") {
            request.json(200, who(agent));
        } else if request.target.contains("/sync") {
            request.json(200, common::sync("boot"));
        } else if request.target.ends_with("/state") {
            request.json(200, state_for(agent));
        } else {
            panic!("unexpected bootstrap request: {}", request.target);
        }
    }
}
/// Bootstrap one agent's collector and resolve its session on ONE room (the
/// shared delivery room, or that agent's direct room) — the session's route
/// is what a final reply is addressed to.
async fn bootstrap_session(
    pair: &PairFixture,
    agent: &HostIdentity,
    dm: &str,
    endpoint: &str,
    session: &str,
    room: &str,
    fake: &mut common::Fake,
) -> Collector {
    let collector =
        Collector::new(pair_config(pair, agent, dm, endpoint), pair.store.clone()).unwrap();
    let cancel = CancellationToken::new();
    let trace = Trace::new("pair bootstrap", None, None);
    let operation = observed(trace, collector.collect(&cancel));
    let (result, ()) = common::scripted(operation, serve_bootstrap(fake, agent)).await;
    result.unwrap();
    pair.store
        .resolve_verified_matrix_session(SessionBinding {
            id: session.into(),
            engagement_id: agent.transport.engagement_id.clone(),
            room_id: room.into(),
            thread_root: None,
        })
        .await
        .unwrap();
    collector
}
/// Drive one agent's settled final reply through the store's public
/// dispatch lifecycle — the same shape `final_claim_named` pins for the
/// single-engagement fixture.
async fn final_claim_for(
    store: &hagency_store::DomainStore,
    session: &str,
    task: &str,
    body: &str,
) -> ReplyClaim {
    store
        .create_canonical_task(task.into(), session.into(), body.into(), now())
        .await
        .unwrap();
    store
        .enqueue_dispatch(DispatchInput {
            id: format!("run_{task}"),
            session_id: session.into(),
            task_id: Some(task.into()),
            resources: vec![],
            payload: json!({"instruction":"fixture"}),
        })
        .await
        .unwrap();
    let cap = store
        .claim_dispatch("runner".into(), now(), 60_000, 120_000, 1)
        .await
        .unwrap()
        .unwrap();
    store.start_dispatch(cap.clone(), now()).await.unwrap();
    store
        .mutate_task(
            cap.clone(),
            task.into(),
            "done".into(),
            TaskMutation::Transition {
                status: TaskState::Done,
                waiting_reason: None,
                waiting_until: None,
            },
            now(),
        )
        .await
        .unwrap();
    store
        .runner_command(
            cap.clone(),
            RunnerCommand::SubmitFinalReply(FinalReply {
                call_id: "final".into(),
                body: body.into(),
            }),
        )
        .await
        .unwrap();
    store
        .complete_dispatch(cap, json!({"observed":"fixture completed"}), now())
        .await
        .unwrap();
    store.claim_final_reply(60_000).await.unwrap().unwrap()
}
/// Send one final reply as one agent and capture the message PUTs the peer
/// received, answering every preflight as the agent's own identity.
async fn send_and_capture(
    collector: &Collector,
    fake: &mut common::Fake,
    agent: &HostIdentity,
    claim: ReplyClaim,
) -> (Vec<String>, Vec<Value>) {
    let cancel = CancellationToken::new();
    let trace = Trace::new("pair send", None, None);
    let operation = observed(trace.clone(), collector.send_final(claim, &cancel));
    let (result, puts) = common::scripted(operation, async {
        let mut targets = Vec::new();
        let mut bodies = Vec::new();
        loop {
            let request = tokio::select! {
                request = fake.next() => request,
                _ = tokio::time::sleep(Duration::from_millis(300)) => break,
            };
            if request.target.contains("/account/whoami") {
                request.json(200, who(agent));
            } else if request.target.contains("/sync") {
                request.json(200, common::sync("boot"));
            } else if request.target.ends_with("/state") {
                request.json(200, state_for(agent));
            } else if request.method == "PUT" && request.target.contains("/send/m.room.message/") {
                targets.push(request.target.clone());
                bodies.push(serde_json::from_slice(&request.body).unwrap());
                request.json(200, json!({"event_id":"$pair"}));
            } else {
                panic!("unexpected plain send request: {}", request.target);
            }
        }
        (targets, bodies)
    })
    .await;
    assert_eq!(result.unwrap().state, OutgoingState::Delivered);
    assert!(trace.has(ObservationPhase::OwnerReturned));
    puts
}
async fn shutdown_pair(pair: PairFixture, fake: common::Fake) {
    pair.store.shutdown().await.unwrap();
    fake.close().await;
}
fn sql(pair: &PairFixture) -> rusqlite::Connection {
    rusqlite::Connection::open(pair.root.path().join("domain/domain.sqlite3")).unwrap()
}

#[tokio::test]
async fn native_two_agents_share_one_room_with_independent_delivery() {
    let pair = PairFixture::new_pair();
    let mut fake = common::Fake::start(true).await;
    let endpoint = fake.endpoint.clone();
    let ca = bootstrap_session(
        &pair,
        &pair.a,
        DM_A,
        &endpoint,
        "root-a",
        SHARED_ROOM,
        &mut fake,
    )
    .await;
    let cb = bootstrap_session(
        &pair,
        &pair.b,
        DM_B,
        &endpoint,
        "root-b",
        SHARED_ROOM,
        &mut fake,
    )
    .await;
    // P1+P2: both agents deliver into the ONE shared room, each charged to
    // its own engagement's session.
    let claim_a = final_claim_for(&pair.store, "root-a", "task-a", "Shared answer A 中文").await;
    let (targets_a, _) = send_and_capture(&ca, &mut fake, &pair.a, claim_a).await;
    let claim_b = final_claim_for(&pair.store, "root-b", "task-b", "Shared answer B 中文").await;
    let (targets_b, _) = send_and_capture(&cb, &mut fake, &pair.b, claim_b).await;
    for (targets, agent) in [(&targets_a, "A"), (&targets_b, "B")] {
        assert_eq!(
            targets.len(),
            1,
            "exactly one send per agent ({agent}), got {targets:?}"
        );
        assert!(
            targets[0].contains("shared"),
            "agent {agent} must deliver into the shared room: {}",
            targets[0]
        );
    }
    // Charged to each own engagement: the reply rows sit under each agent's
    // own session, never the other's.
    let rows: Vec<(String, String)> = sql(&pair)
        .prepare(
            "SELECT f.id,f.session_id FROM final_replies f \
             JOIN runner_sessions s ON s.id=f.session_id ORDER BY f.id",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|(_, s)| s == "root-a"));
    assert!(rows.iter().any(|(_, s)| s == "root-b"));
    // Refusal: engagement A cannot bind a session on B's private DM room —
    // that room was never observed for A — so the cross-engagement private
    // route does not exist to send on.
    assert!(
        pair.store
            .resolve_verified_matrix_session(SessionBinding {
                id: "cross".into(),
                engagement_id: pair.a.transport.engagement_id.clone(),
                room_id: DM_B.into(),
                thread_root: None,
            })
            .await
            .is_err(),
        "a cross-engagement private-room route must be refused"
    );
    fake.no_request().await;
    // Refusal: an ambiguous sender — the peer answering an identity other
    // than the configured agent's — is refused before any send. A fresh
    // collector is required: an already-open owner skips whoami entirely.
    let impostor = Collector::new(
        pair_config(&pair, &pair.a, DM_A, &fake.endpoint),
        pair.store.clone(),
    )
    .unwrap();
    let claim = final_claim_for(&pair.store, "root-a", "task-ambiguous", "Ambiguous").await;
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(impostor.send_final(claim, &cancel), async {
        let request = fake.next().await;
        assert!(request.target.contains("/account/whoami"));
        // B's identity answering A's collector: ambiguous, refused.
        request.json(200, who(&pair.b));
    })
    .await;
    assert_eq!(result.err(), Some(Error::Identity));
    fake.no_request().await;
    drop(impostor);
    ca.close().await.unwrap();
    cb.close().await.unwrap();
    shutdown_pair(pair, fake).await;
}

#[tokio::test]
async fn native_two_agent_dm_reaches_only_its_own_engagement() {
    let pair = PairFixture::new_pair();
    let mut fake = common::Fake::start(true).await;
    let endpoint = fake.endpoint.clone();
    let c = bootstrap_session(&pair, &pair.a, DM_A, &endpoint, "root-a", DM_A, &mut fake).await;
    // P3: the DM is addressed to the direct room only — no request in the
    // whole leg carries the shared room id.
    let claim = final_claim_for(&pair.store, "root-a", "task-dm", "Private DM body").await;
    let (targets, _) = send_and_capture(&c, &mut fake, &pair.a, claim).await;
    assert_eq!(targets.len(), 1);
    assert!(
        targets[0].contains("dm-a"),
        "the DM must be addressed to the direct room: {}",
        targets[0]
    );
    assert!(
        !targets[0].contains("shared"),
        "no DM request may carry the shared room id"
    );
    // The other engagement's rows are unchanged: B has no session at all,
    // so its inbox read is refused rather than silently shared.
    let b_sessions: i64 = sql(&pair)
        .query_row(
            "SELECT COUNT(*) FROM runner_sessions s \
             JOIN engagements e ON e.id=s.engagement_id WHERE e.id=?1",
            [&pair.b.transport.engagement_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(b_sessions, 0);
    assert!(
        pair.store
            .inbox("root-b".into(), 0, 100, None)
            .await
            .is_err()
    );
    c.close().await.unwrap();
    shutdown_pair(pair, fake).await;
}

#[tokio::test]
async fn native_two_agent_dm_content_is_absent_from_the_room() {
    let pair = PairFixture::new_pair();
    let mut fake = common::Fake::start(true).await;
    let endpoint = fake.endpoint.clone();
    let c = bootstrap_session(&pair, &pair.a, DM_A, &endpoint, "root-a", DM_A, &mut fake).await;
    let claim = final_claim_for(&pair.store, "root-a", "task-crypto", "Enciphered DM 中文").await;
    let peer = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing_fixture(true)
        .await;
    let cancel = CancellationToken::new();
    let trace = Trace::new("pair crypto dm", None, None);
    let operation = observed(trace.clone(), c.send_final(claim, &cancel));
    // The whole enciphered DM round — keys/query, share, then the encrypted
    // PUT(s). Room ciphertexts are counted: none may exist, so none can
    // decrypt to the DM body.
    let (result, (room_puts, dm_plain)) = common::scripted(operation, async {
        let mut room_puts = 0usize;
        let mut dm_plain: Option<Value> = None;
        loop {
            let request = tokio::select! {
                request = fake.next() => request,
                _ = tokio::time::sleep(Duration::from_millis(300)) => break,
            };
            if request.target.contains("/account/whoami") {
                request.json(200, who(&pair.a));
            } else if request.target.contains("/sync") {
                request.json(200, common::sync("boot"));
            } else if request.target.ends_with("/state") {
                request.json(200, state_for(&pair.a));
            } else if request.target.contains("/keys/query") {
                request.json(200, peer.query.clone());
            } else if request.target.contains("/sendToDevice/m.room.encrypted/") {
                peer.share(serde_json::from_slice(&request.body).unwrap())
                    .await;
                request.json(200, json!({}));
            } else if request.method == "PUT" && request.target.contains("/send/m.room.encrypted/")
            {
                let value: Value = serde_json::from_slice(&request.body).unwrap();
                assert!(value.get("body").is_none(), "ciphertext carries no body");
                if request.target.contains("shared") {
                    room_puts += 1;
                }
                assert!(
                    request.target.contains("dm-a"),
                    "the ciphertext belongs to the DM room: {}",
                    request.target
                );
                dm_plain = Some(peer.decrypt(value).await);
                request.json(200, json!({"event_id":"$enc"}));
            } else if request.method == "PUT" {
                panic!(
                    "unexpected plain PUT on the encrypted DM leg: {}",
                    request.target
                );
            } else {
                panic!("unexpected crypto leg request: {}", request.target);
            }
        }
        (room_puts, dm_plain)
    })
    .await;
    assert_eq!(result.unwrap().state, OutgoingState::Delivered);
    assert!(trace.has(ObservationPhase::OwnerReturned));
    assert_eq!(room_puts, 0, "no ciphertext may target the shared room");
    let plain = dm_plain.expect("exactly one DM ciphertext was sent");
    assert_eq!(plain["content"]["body"], "Enciphered DM 中文");
    c.close().await.unwrap();
    shutdown_pair(pair, fake).await;
}

#[tokio::test]
async fn native_two_agent_message_never_crosses_engagements() {
    let pair = PairFixture::new_pair();
    let mut fake = common::Fake::start(true).await;
    let endpoint = fake.endpoint.clone();
    let c = bootstrap_session(
        &pair,
        &pair.a,
        DM_A,
        &endpoint,
        "root-a",
        SHARED_ROOM,
        &mut fake,
    )
    .await;
    // The owner's shared-room mention reaches A's engagement only (N1).
    // `intake` — not `collect` — is the admitting job for a message batch.
    let event = json!({
        "event_id":"$question","sender":OWNER,"type":"m.room.message","origin_server_ts":now(),
        "content":{"msgtype":"m.text","body":"Secret for A 中文",
                   "m.mentions":{"user_ids":[pair.a.transport.sender_mxid]}}
    });
    let mut packet = common::sync("pair1");
    packet["rooms"]["join"][SHARED_ROOM]["timeline"] = json!({"events":[event],"limited":false});
    packet["rooms"]["join"][SHARED_ROOM]["state"] = json!({"events":[]});
    let plan = HostIntakePlan::new(vec!["root-a".into()]).unwrap();
    let cancel = CancellationToken::new();
    let mut delivered = false;
    let trace = Trace::new("pair intake", None, None);
    let operation = observed(trace, c.intake(plan, &cancel));
    let (summary, ()) = common::scripted(operation, async {
        loop {
            let request = tokio::select! {
                request = fake.next() => request,
                _ = tokio::time::sleep(Duration::from_millis(300)) => return,
            };
            if request.target.contains("/account/whoami") {
                request.json(200, who(&pair.a));
            } else if request.target.contains("/sync") {
                if delivered {
                    request.json(200, common::sync("pair2"));
                } else {
                    delivered = true;
                    request.json(200, packet.clone());
                }
            } else if request.target.ends_with("/state") {
                request.json(200, state_for(&pair.a));
            } else {
                panic!("unexpected intake request: {}", request.target);
            }
        }
    })
    .await;
    assert!(
        summary.unwrap().admitted >= 1,
        "the shared-room batch is admitted"
    );
    let inbox = pair
        .store
        .inbox("root-a".into(), 0, 100, None)
        .await
        .unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].message.body, "Secret for A 中文");
    assert_eq!(inbox[0].message.event_id, "$question");
    // The other engagement's inbox does not exist to read: B has no session,
    // so its read is refused — the body and sequence are absent, not shared.
    assert!(
        pair.store
            .inbox("root-b".into(), 0, 100, None)
            .await
            .is_err()
    );
    c.close().await.unwrap();
    shutdown_pair(pair, fake).await;
}
