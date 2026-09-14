//! MA-M8a (ADR-144): the in-process two-agent acceptance, over the real-TLS
//! fake peer and the two-engagement `PairFixture`. One shared DELIVERY room
//! that is never an approval room, one direct room per engagement, and the
//! retained oracle's properties plus its two refusals. Every HTTP loop runs
//! INSIDE the `common::scripted` future and ends on a quiet window. Both
//! engagements are bootstrapped in EVERY test, and each negative is asserted
//! against an engagement that carries real traffic of its own (review F2/F3):
//! a vacuous "the other side never existed" check is not isolation proof.
use super::*;
use crate::collector::observation::{Phase as ObservationPhase, Trace, observed};
use common::pair::{DM_A, DM_B, OWNER, PairFixture, SHARED_ROOM, shared_state, state_for, who};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

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
/// Answer bootstrap traffic for one agent — a FIXED script, mirroring the
/// single-agent `success` helper (common/mod.rs): whoami as ITS OWN
/// identity, one sync, then BOTH rooms' state (the shared Group room and
/// this agent's direct room), returning immediately after the second
/// answer. Never a quiet timer: `common::scripted`'s biased select panics
/// whenever the script settles after the collector (~136 ms) on a timer,
/// so the script's end must be the last bootstrap request it serves. The
/// room is keyed on the target; a DM always answers the invite-only +
/// encrypted shape.
async fn serve_bootstrap(fake: &mut common::Fake, pair: &PairFixture, agent: &HostIdentity) {
    let mut states_served = 0usize;
    loop {
        let request = fake.next().await;
        if request.target.contains("/account/whoami") {
            request.json(200, who(agent));
        } else if request.target.contains("/sync") {
            request.json(200, common::sync("boot"));
        } else if request.target.ends_with("/state") {
            // Room-keyed answers: a DIRECT room is always invite-only AND
            // encrypted (ADR-144; the store's invalid_direct clause), and the
            // SHARED room answers its one room-wide snapshot — both agents
            // and the owner joined — so the second agent's publish is
            // idempotent at the same generation.
            let is_dm = request.target.contains("dm-");
            request.json(
                200,
                if is_dm {
                    state_for(agent)
                } else {
                    shared_state(&pair.a, &pair.b)
                },
            );
            states_served += 1;
            // Both rooms answered — the observation pass is complete; the
            // script ends here, deterministically.
            if states_served == 2 {
                return;
            }
        } else {
            panic!("unexpected bootstrap request: {}", request.target);
        }
    }
}
/// The agent's own direct room, derived from its identity.
fn dm_of(pair: &PairFixture, agent: &HostIdentity) -> &'static str {
    if agent.transport.sender_mxid == pair.a.transport.sender_mxid {
        DM_A
    } else {
        DM_B
    }
}
/// Bootstrap one agent's collector and resolve its session on ONE room (the
/// shared delivery room, or that agent's direct room).
async fn bootstrap_session(
    pair: &PairFixture,
    agent: &HostIdentity,
    endpoint: &str,
    session: &str,
    room: &str,
    fake: &mut common::Fake,
) -> Collector {
    let collector = Collector::new(
        pair_config(pair, agent, dm_of(pair, agent), endpoint),
        pair.store.clone(),
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let trace = Trace::new("pair bootstrap", None, None);
    let operation = observed(trace, collector.collect(&cancel));
    let (result, ()) = common::scripted(operation, serve_bootstrap(fake, pair, agent)).await;
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
/// Drive one agent's settled final reply through the store's public dispatch
/// lifecycle — the same shape `final_claim_named` pins for the
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
/// Send one final reply as one agent (PLAIN leg) and capture the message
/// PUTs the peer received, answering every preflight as the agent's own
/// identity over plain room state.
async fn send_and_capture(
    collector: &Collector,
    fake: &mut common::Fake,
    pair: &PairFixture,
    agent: &HostIdentity,
    claim: ReplyClaim,
) -> (Vec<String>, Vec<Value>) {
    // A DM room is always encrypted (ADR-144), so a DM send runs the megolm
    // round; the peer crypto fixture is acquired BEFORE the send operation,
    // exactly as the single-agent crypto test does.
    let peer = collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing_fixture(true)
        .await;
    let cancel = CancellationToken::new();
    let trace = Trace::new("pair send", None, None);
    let operation = observed(trace.clone(), collector.send_final(claim, &cancel));
    let (result, puts) = common::scripted(operation, async {
        let mut targets = Vec::new();
        let mut bodies = Vec::new();
        // A FIXED script, never a quiet timer: preflights; for a DM leg the
        // megolm round (keys/query, sendToDevice share); then the one
        // message PUT (plain or encrypted) — the script ends on the PUT it
        // serves.
        loop {
            let request = fake.next().await;
            if request.target.contains("/account/whoami") {
                request.json(200, who(agent));
            } else if request.target.contains("/sync") {
                request.json(200, common::sync("boot"));
            } else if request.target.ends_with("/state") {
                let is_dm = request.target.contains("dm-");
                request.json(
                    200,
                    if is_dm {
                        state_for(agent)
                    } else {
                        shared_state(&pair.a, &pair.b)
                    },
                );
            } else if request.target.contains("/keys/query") {
                request.json(200, peer.query.clone());
            } else if request.target.contains("/sendToDevice/m.room.encrypted/") {
                peer.share(serde_json::from_slice(&request.body).unwrap())
                    .await;
                request.json(200, json!({}));
            } else if request.method == "PUT"
                && (request.target.contains("/send/m.room.message/")
                    || request.target.contains("/send/m.room.encrypted/"))
            {
                targets.push(request.target.clone());
                bodies.push(serde_json::from_slice(&request.body).unwrap());
                request.json(200, json!({"event_id":"$pair"}));
                return (targets, bodies);
            } else {
                panic!("unexpected send request: {}", request.target);
            }
        }
    })
    .await;
    assert_eq!(result.unwrap().state, OutgoingState::Delivered);
    assert!(trace.has(ObservationPhase::OwnerReturned));
    puts
}
/// The owner→agent direction (review F1): deliver the owner's DM event for
/// one agent into that agent's room and assert it is admitted.
async fn owner_dm_intake(
    collector: &Collector,
    fake: &mut common::Fake,
    pair: &PairFixture,
    agent: &HostIdentity,
    room: &str,
    body: &str,
    session: &str,
) {
    // A DM room is always encrypted (ADR-144), and the store refuses a
    // plaintext event in an encrypted scope (`verified_ingress.rs:303`,
    // route.encrypted && !input.encrypted), so the owner's DM must arrive
    // as REAL megolm ciphertext — built on the send leg's SAME trusted
    // human (a fresh verified_pair would be an untrusted identity change
    // to the receiver, which already pinned this human). The shared room
    // is a plain Group delivery room and keeps the plaintext shape.
    let packet = if room.contains("dm-") {
        let peer = collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing_fixture(true)
            .await;
        peer.owner_dm_packet(
            room,
            &format!("$own-{session}"),
            body,
            &agent.transport.sender_mxid,
        )
        .await
    } else {
        let event = json!({
            "event_id": format!("$own-{session}"),
            "sender": OWNER,
            "type": "m.room.message",
            "origin_server_ts": now(),
            "content": {
                "msgtype": "m.text",
                "body": body,
                "m.mentions": {"user_ids": [agent.transport.sender_mxid]}
            }
        });
        let mut packet = common::sync(&format!("own-{session}"));
        packet["rooms"]["join"][room]["timeline"] = json!({"events":[event],"limited":false});
        packet["rooms"]["join"][room]["state"] = json!({"events":[]});
        packet
    };
    let plan = HostIntakePlan::new(vec![session.into()]).unwrap();
    let cancel = CancellationToken::new();
    let mut delivered = false;
    let trace = Trace::new("pair owner dm intake", None, None);
    let operation = observed(trace, collector.intake(plan, &cancel));
    let (summary, ()) = common::scripted(operation, async {
        // A FIXED script (the notice-intake shape): whoami, the one sync
        // packet carrying the owner's DM event, then BOTH rooms' state —
        // the pair collector's config carries two rooms and the intake
        // re-checks each; answering only one starves the operation into
        // its internal Timeout. The script ends on the second answer,
        // never a quiet timer.
        let mut states = 0usize;
        loop {
            let request = fake.next().await;
            if request.target.contains("/account/whoami") {
                request.json(200, who(agent));
            } else if request.target.contains("/sync") {
                if delivered {
                    request.json(200, common::sync("after"));
                } else {
                    delivered = true;
                    request.json(200, packet.clone());
                }
            } else if request.target.ends_with("/state") {
                let is_dm = request.target.contains("dm-");
                request.json(
                    200,
                    if is_dm {
                        state_for(agent)
                    } else {
                        shared_state(&pair.a, &pair.b)
                    },
                );
                states += 1;
                if states == 2 {
                    return;
                }
            } else {
                panic!("unexpected intake request: {}", request.target);
            }
        }
    })
    .await;
    assert!(
        summary.unwrap().admitted >= 1,
        "the owner's DM for {session} must be admitted into its own room"
    );
}
async fn shutdown_pair(pair: PairFixture, fake: common::Fake) {
    pair.store.shutdown().await.unwrap();
    fake.close().await;
}
fn sql(pair: &PairFixture) -> rusqlite::Connection {
    rusqlite::Connection::open(pair.root.path().join("domain/domain.sqlite3")).unwrap()
}
/// Both engagements' reply rows, per session — the "rows unchanged"
/// evidence for the negatives (review F2/F3).
fn reply_rows(pair: &PairFixture, session: &str) -> Vec<String> {
    sql(pair)
        .prepare("SELECT body FROM final_replies WHERE session_id=?1 ORDER BY id")
        .unwrap()
        .query_map([session], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[tokio::test]
async fn native_two_agents_share_one_room_with_independent_delivery() {
    let pair = PairFixture::new_pair();
    let mut fake = common::Fake::start(true).await;
    let endpoint = fake.endpoint.clone();
    let ca = bootstrap_session(&pair, &pair.a, &endpoint, "root-a", SHARED_ROOM, &mut fake).await;
    let cb = bootstrap_session(&pair, &pair.b, &endpoint, "root-b", SHARED_ROOM, &mut fake).await;
    // P1+P2: both agents deliver into the ONE shared room, each charged to
    // its own engagement's session.
    let claim_a = final_claim_for(&pair.store, "root-a", "task-a", "Shared answer A 中文").await;
    let (targets_a, _) = send_and_capture(&ca, &mut fake, &pair, &pair.a, claim_a).await;
    let claim_b = final_claim_for(&pair.store, "root-b", "task-b", "Shared answer B 中文").await;
    let (targets_b, _) = send_and_capture(&cb, &mut fake, &pair, &pair.b, claim_b).await;
    for (targets, agent) in [(&targets_a, "A"), (&targets_b, "B")] {
        assert_eq!(
            targets.len(),
            1,
            "exactly one send per agent ({agent}), got {targets:?}"
        );
        assert!(
            targets[0].contains(SHARED_ROOM),
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
    let ca = bootstrap_session(&pair, &pair.a, &endpoint, "root-a", DM_A, &mut fake).await;
    let cb = bootstrap_session(&pair, &pair.b, &endpoint, "root-b", DM_B, &mut fake).await;
    // P3: A's DM is addressed to the direct room only. B sends its own DM
    // first so the later negative is asserted against real B traffic.
    let claim_b = final_claim_for(&pair.store, "root-b", "task-dm-b", "DM from B 中文").await;
    let (targets_b, _) = send_and_capture(&cb, &mut fake, &pair, &pair.b, claim_b).await;
    assert_eq!(targets_b.len(), 1);
    assert!(targets_b[0].contains("dm-b"), "B's DM targets B's room");
    let claim_a = final_claim_for(&pair.store, "root-a", "task-dm-a", "Private DM body").await;
    let (targets_a, _) = send_and_capture(&ca, &mut fake, &pair, &pair.a, claim_a).await;
    assert_eq!(targets_a.len(), 1);
    assert!(
        targets_a[0].contains("dm-a"),
        "the DM must be addressed to the direct room: {}",
        targets_a[0]
    );
    // No DM request in either direction carries the shared room id.
    for (targets, agent) in [(&targets_a, "A"), (&targets_b, "B")] {
        assert!(
            !targets.iter().any(|t| t.contains("shared")),
            "a DM send by {agent} referenced the shared room: {targets:?}"
        );
    }
    // The owner's DM back to A reaches A's engagement's inbox only, and the
    // owner's DM to B never appears in A's rows or in the shared room
    // (review F1: both owner→agent clauses asserted).
    owner_dm_intake(
        &ca,
        &mut fake,
        &pair,
        &pair.a,
        DM_A,
        "Owner to A private 中文",
        "root-a",
    )
    .await;
    owner_dm_intake(
        &cb,
        &mut fake,
        &pair,
        &pair.b,
        DM_B,
        "Owner to B private 中文",
        "root-b",
    )
    .await;
    let inbox_a = pair
        .store
        .inbox("root-a".into(), 0, 100, None)
        .await
        .unwrap();
    assert_eq!(inbox_a.len(), 1, "A's inbox carries only its own DM");
    assert_eq!(inbox_a[0].message.body, "Owner to A private 中文");
    assert_eq!(inbox_a[0].message.event_id, "$own-root-a");
    let inbox_b = pair
        .store
        .inbox("root-b".into(), 0, 100, None)
        .await
        .unwrap();
    assert_eq!(inbox_b.len(), 1, "B's inbox carries only its own DM");
    assert_eq!(inbox_b[0].message.body, "Owner to B private 中文");
    assert_eq!(inbox_b[0].message.event_id, "$own-root-b");
    // B's rows are unchanged by all of A's activity: still exactly its own
    // DM reply, never A's body (review F2: real traffic, not a zero count).
    assert_eq!(
        reply_rows(&pair, "root-b"),
        vec!["DM from B 中文".to_string()],
        "B's reply rows must remain exactly its own"
    );
    assert_eq!(
        reply_rows(&pair, "root-a"),
        vec!["Private DM body".to_string()],
        "A's reply rows must remain exactly its own"
    );
    ca.close().await.unwrap();
    cb.close().await.unwrap();
    shutdown_pair(pair, fake).await;
}

#[tokio::test]
async fn native_two_agent_dm_content_is_absent_from_the_room() {
    let pair = PairFixture::new_pair();
    let mut fake = common::Fake::start(true).await;
    let endpoint = fake.endpoint.clone();
    // A's DM room is the encrypted leg; B is bootstrapped plain and sends
    // its own DM first, so the negative is asserted against real B traffic.
    let ca = bootstrap_session(&pair, &pair.a, &endpoint, "root-a", DM_A, &mut fake).await;
    let cb = bootstrap_session(&pair, &pair.b, &endpoint, "root-b", DM_B, &mut fake).await;
    let claim_b = final_claim_for(&pair.store, "root-b", "task-plain-b", "Plain DM from B").await;
    let (targets_b, _) = send_and_capture(&cb, &mut fake, &pair, &pair.b, claim_b).await;
    assert_eq!(targets_b.len(), 1);
    assert!(targets_b[0].contains("dm-b"));
    let claim = final_claim_for(&pair.store, "root-a", "task-crypto", "Enciphered DM 中文").await;
    let peer = ca
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
    let operation = observed(trace.clone(), ca.send_final(claim, &cancel));
    // The whole enciphered DM round — keys/query, share, then the encrypted
    // PUT(s). Room ciphertexts are counted: none may exist, so none can
    // decrypt to the DM body. The script ENDS on the encrypted PUT it
    // serves — a quiet timer here starves the collector's trailing request
    // into its internal Timeout (the :617 failure).
    let (result, (room_puts, dm_plain)) = common::scripted(operation, async {
        let mut room_puts = 0usize;
        loop {
            let request = fake.next().await;
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
                let decrypted = peer
                    .decrypt_in(value, &ruma::RoomId::parse(DM_A).unwrap())
                    .await;
                request.json(200, json!({"event_id":"$enc"}));
                return (room_puts, Some(decrypted));
            } else if request.method == "PUT" {
                panic!(
                    "unexpected plain PUT on the encrypted DM leg: {}",
                    request.target
                );
            } else {
                panic!("unexpected crypto leg request: {}", request.target);
            }
        }
    })
    .await;
    assert_eq!(result.unwrap().state, OutgoingState::Delivered);
    assert!(trace.has(ObservationPhase::OwnerReturned));
    assert_eq!(room_puts, 0, "no ciphertext may target the shared room");
    let plain = dm_plain.expect("exactly one DM ciphertext was sent");
    assert_eq!(plain["content"]["body"], "Enciphered DM 中文");
    // B's rows are unchanged by A's enciphered DM: still exactly its own
    // plain reply (review F2/F3 form).
    assert_eq!(
        reply_rows(&pair, "root-b"),
        vec!["Plain DM from B".to_string()]
    );
    ca.close().await.unwrap();
    cb.close().await.unwrap();
    shutdown_pair(pair, fake).await;
}

#[tokio::test]
async fn native_two_agent_message_never_crosses_engagements() {
    let pair = PairFixture::new_pair();
    let mut fake = common::Fake::start(true).await;
    let endpoint = fake.endpoint.clone();
    // Two engagements with an active conversation EACH (review F2): both are
    // bootstrapped on the shared room and each receives its own owner
    // mention, so the negative is asserted against an engagement that
    // carries real traffic of its own.
    let ca = bootstrap_session(&pair, &pair.a, &endpoint, "root-a", SHARED_ROOM, &mut fake).await;
    let cb = bootstrap_session(&pair, &pair.b, &endpoint, "root-b", SHARED_ROOM, &mut fake).await;
    owner_dm_intake(
        &ca,
        &mut fake,
        &pair,
        &pair.a,
        SHARED_ROOM,
        "Secret for A 中文",
        "root-a",
    )
    .await;
    owner_dm_intake(
        &cb,
        &mut fake,
        &pair,
        &pair.b,
        SHARED_ROOM,
        "Secret for B 中文",
        "root-b",
    )
    .await;
    // A's inbox: only its own message — its own sequence and body; B's body
    // and event are absent even though B's conversation is real and live.
    let inbox_a = pair
        .store
        .inbox("root-a".into(), 0, 100, None)
        .await
        .unwrap();
    assert_eq!(inbox_a.len(), 1, "A sees exactly its own message");
    assert_eq!(inbox_a[0].message.body, "Secret for A 中文");
    assert_eq!(inbox_a[0].message.event_id, "$own-root-a");
    let inbox_b = pair
        .store
        .inbox("root-b".into(), 0, 100, None)
        .await
        .unwrap();
    assert_eq!(inbox_b.len(), 1, "B sees exactly its own message");
    assert_eq!(inbox_b[0].message.body, "Secret for B 中文");
    assert_eq!(inbox_b[0].message.event_id, "$own-root-b");
    // The cross-bodies are absent by name, not by count.
    let bodies_a: Vec<&str> = inbox_a
        .iter()
        .map(|item| item.message.body.as_str())
        .collect();
    assert!(
        !bodies_a.contains(&"Secret for B 中文"),
        "B's body leaked into A's inbox"
    );
    let bodies_b: Vec<&str> = inbox_b
        .iter()
        .map(|item| item.message.body.as_str())
        .collect();
    assert!(
        !bodies_b.contains(&"Secret for A 中文"),
        "A's body leaked into B's inbox"
    );
    ca.close().await.unwrap();
    cb.close().await.unwrap();
    shutdown_pair(pair, fake).await;
}
