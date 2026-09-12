#[path = "../../hagency-store/tests/common/mod.rs"]
mod common;
#[path = "../../hagency-matrix/tests/common/stall.rs"]
mod stall;
#[path = "runner/verified_ingress.rs"]
mod verified_ingress;
use common::*;
use hagency::App;
use hagency_core::{messages::*, tasks::*};
use hagency_store::{DomainRepository, DomainStore, EffectOutcome, Repository, Store};
use salvo::{
    prelude::*,
    test::{RequestBuilder, ResponseExt, TestClient},
};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TOKEN: &str = "fixture_operator_token_32_bytes_minimum";
const BASE: &str = "http://127.0.0.1:13300/api/native/v1";
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
struct Fixture {
    _root: tempfile::TempDir,
    service: Service,
    domain: DomainStore,
    custody: Store,
    cap: RunnerCapability,
    engagement: String,
    source_sequence: u64,
}
impl Fixture {
    async fn new(start: bool) -> Self {
        Self::with_thread(start, Some("$thread")).await
    }
    async fn with_thread(start: bool, thread: Option<&str>) -> Self {
        Self::configured(start, thread, false).await
    }
    async fn configured(start: bool, thread: Option<&str>, verified: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("request", "小白", &pool, 100));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture_provisioned".into(),
            },
        )
        .unwrap();
        let binding = SessionBinding {
            id: "session".into(),
            engagement_id: e.id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: thread.map(str::to_owned),
        };
        if verified {
            use hagency_core::replies::*;
            db.observe_matrix_transport(
                &MatrixTransportObservation {
                    engagement_id: e.id.clone(),
                    registration_generation: 1,
                    generation: 1,
                    sender_mxid: "@worker:example.test".into(),
                    device_id: "DEVICE".into(),
                },
                now(),
            )
            .unwrap();
            db.observe_matrix_room(
                &MatrixRoomObservation {
                    engagement_id: e.id.clone(),
                    registration_generation: 1,
                    transport_generation: 1,
                    room_id: binding.room_id.clone(),
                    generation: 1,
                    privacy: RoomPrivacy::Group {},
                    joined: std::collections::BTreeSet::from([
                        "@worker:example.test".into(),
                        "@owner:example.test".into(),
                    ]),
                    invite_only: true,
                    encrypted: false,
                },
                now(),
            )
            .unwrap();
            db.resolve_verified_matrix_session(&binding, now()).unwrap();
        } else {
            db.register_session(&binding).unwrap();
        }
        let current = now();
        db.create_canonical_task("task", "session", "Verify the implementation", current)
            .unwrap();
        db.create_canonical_task(
            "private_other_task",
            "session",
            "Not in this dispatch",
            current,
        )
        .unwrap();
        let source = InboundMessage {
            server_name: "example.test".into(),
            room_id: "!project:example.test".into(),
            event_id: "$source".into(),
            sender_mxid: "@owner:example.test".into(),
            thread_root: thread.map(str::to_owned),
            body: "Please verify the code".into(),
            kind: "m.text".into(),
            origin_ts: current,
        };
        let source_sequence = if verified {
            let observation = hagency_core::ingress::MatrixEventObservation {
                scope: db.matrix_ingress_scope("session").unwrap(),
                event: source,
                mentions: std::collections::BTreeSet::from(["@worker:example.test".into()]),
                encrypted: false,
            };
            db.admit_matrix_event(&observation, current)
                .unwrap()
                .sequence
        } else {
            db.ingest_message(
                &source,
                &[MessageTarget {
                    session_id: "session".into(),
                    wake: true,
                }],
                current,
            )
            .unwrap()
            .sequence
        };
        db.enqueue_inbox_dispatch(
            &DispatchInput {
                id: "dispatch".into(),
                session_id: "session".into(),
                task_id: Some("task".into()),
                resources: vec![],
                payload: json!({"instruction":"verify"}),
            },
            &[source_sequence],
        )
        .unwrap();
        let cap = db
            .claim_dispatch("runner", current, 120_000, 120_000, 8)
            .unwrap()
            .unwrap();
        if start {
            db.start_dispatch(&cap, current).unwrap();
        }
        let domain = DomainStore::start(db, 16).unwrap();
        let service = Service::new(
            App::new(
                custody.clone(),
                TOKEN.as_bytes(),
                "127.0.0.1:13300".parse().unwrap(),
            )
            .unwrap()
            .with_domain(domain.clone())
            .router(),
        );
        Self {
            _root: root,
            service,
            domain,
            custody,
            cap,
            engagement: e.id,
            source_sequence,
        }
    }
    /// Diagnostic only. Sampled from the caller while the writer is provably
    /// still inside the SQLite close path (the caller has just timed out and
    /// `connection_drop_finished_us` is unpublished). Sizes only, no content.
    fn teardown_sample(&self) -> Vec<(&'static str, Option<(u64, u128)>)> {
        let state = self._root.path().join("state");
        [
            "domain.sqlite3",
            "domain.sqlite3-wal",
            "domain.sqlite3-shm",
            "domain.sqlite3-journal",
        ]
        .into_iter()
        .map(|name| {
            // Size and modification age in milliseconds: a main database
            // touched moments ago means the close-time checkpoint completed
            // and the wait is in the unlink or handle close; an old main
            // database with a large `-wal` means the checkpoint itself waits.
            let sample = std::fs::metadata(state.join(name)).ok().map(|m| {
                let age = m
                    .modified()
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map_or(u128::MAX, |d| d.as_millis());
                (m.len(), age)
            });
            (name, sample)
        })
        .collect()
    }
    async fn close(self) {
        let (result, snapshot) = self.domain.shutdown_observed().await;
        let outcome = format!("{:?}", snapshot.outcome);
        if let Err(error) = &result {
            if stall::timed_out(&outcome) {
                eprintln!("[shutdown-stall] {outcome}; snapshot {snapshot:?}");
                stall::record_if_timed_out(
                    &outcome,
                    "runner::Fixture::close domain",
                    format!("{snapshot:?}"),
                    None,
                );
            }
            // The writer is still stalled here; read the teardown artifacts
            // before unwinding. A present, non-zero `-wal`/`-shm` means the
            // unlink inside `sqlite3_close` is failing and retrying; absent
            // means the wait is elsewhere (for example a stalled sync).
            let first = self.teardown_sample();
            tokio::time::sleep(Duration::from_millis(500)).await;
            let second = self.teardown_sample();
            panic!(
                "domain shutdown failed ({outcome}): {error:?}; {snapshot:?}; teardown={first:?}; teardown_after_500ms={second:?}"
            );
        }
        let (result, snapshot) = self.custody.shutdown_observed().await;
        let outcome = format!("{:?}", snapshot.outcome);
        if let Err(error) = &result {
            if stall::timed_out(&outcome) {
                eprintln!("[shutdown-stall] {outcome}; snapshot {snapshot:?}");
                stall::record_if_timed_out(
                    &outcome,
                    "runner::Fixture::close custody",
                    format!("{snapshot:?}"),
                    None,
                );
            }
            panic!("custody shutdown failed ({outcome}): {error:?}; {snapshot:?}");
        }
    }
}
fn auth(builder: RequestBuilder, cap: &RunnerCapability) -> RequestBuilder {
    builder
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(&cap.secret)
        .add_header("x-hagency-dispatch", &cap.dispatch_id, true)
        .add_header("x-hagency-runner", &cap.runner_id, true)
        .add_header("x-hagency-fence", cap.fence.to_string(), true)
}
fn get(path: &str, cap: &RunnerCapability) -> RequestBuilder {
    auth(TestClient::get(format!("{BASE}/runner/{path}")), cap)
}
fn post(id: &str, cap: &RunnerCapability, body: &Value) -> RequestBuilder {
    auth(
        TestClient::post(format!("{BASE}/runner/tasks/{id}/operations")),
        cap,
    )
    .json(body)
}
async fn operation(f: &Fixture, call: &str, operation: Value) -> (StatusCode, Value) {
    let mut response = post(
        "task",
        &f.cap,
        &json!({"call_id":call,"operation":operation}),
    )
    .send(&f.service)
    .await;
    let code = response.status_code.unwrap();
    (code, response.take_json().await.unwrap())
}

#[tokio::test]
async fn native_runner_receive_current_authority() {
    let f = Fixture::new(true).await;
    let receive = |cap: &RunnerCapability| {
        auth(
            TestClient::post(format!("{BASE}/runner/received-files")),
            cap,
        )
    };
    let valid = json!({"event_id":"$original"});
    // Neither a presentation marker nor current task credentials invent a service.
    assert_eq!(
        get("received-files", &f.cap)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    assert_eq!(
        receive(&f.cap)
            .json(&valid)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    for query in [
        "limit=0",
        "limit=17",
        "limit=1&limit=2",
        "after=-1",
        "after=1.5",
        "after=9007199254740992",
        "after=1&after=2",
        "path=private_canary",
        "after=%31",
        "after=",
    ] {
        assert_eq!(
            get(&format!("received-files?{query}"), &f.cap)
                .send(&f.service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST),
            "{query}"
        );
    }
    for payload in [
        r#"{"event_id":"$original","event_id":"$other"}"#,
        r#"{"event_id":"$original","path":"private_canary"}"#,
        r#"{"event_id":"missing_prefix"}"#,
    ] {
        let mut response = receive(&f.cap)
            .add_header("content-type", "application/json", true)
            .body(payload)
            .send(&f.service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::BAD_REQUEST));
        assert!(
            !response
                .take_string()
                .await
                .unwrap()
                .contains("private_canary")
        );
    }
    let missing = TestClient::get(format!("{BASE}/runner/received-files"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&f.service)
        .await;
    assert!(matches!(
        missing.status_code,
        Some(StatusCode::UNAUTHORIZED)
    ));
    let mut wrong = f.cap.clone();
    wrong.secret = "f".repeat(64);
    let mut successor = f.cap.clone();
    successor.fence += 1;
    for cap in [&wrong, &successor] {
        assert_eq!(
            get("received-files", cap)
                .send(&f.service)
                .await
                .status_code,
            Some(StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            receive(cap).json(&valid).send(&f.service).await.status_code,
            Some(StatusCode::UNAUTHORIZED)
        );
    }
    assert_eq!(
        operation(&f, "done", json!({"action":"transition","status":"done"}))
            .await
            .0,
        StatusCode::OK
    );
    // Task-only Done still permits original final-reply bookkeeping. Complete
    // the actual dispatch too before asserting that its credential is retired.
    f.domain
        .complete_dispatch(f.cap.clone(), json!({"text":"reported"}), now())
        .await
        .unwrap();
    // Old task-operation replay exists elsewhere; both receive paths stay current.
    assert_eq!(
        get("received-files", &f.cap)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        receive(&f.cap)
            .json(&valid)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.close().await;
}

#[tokio::test]
async fn native_runner_http_replies() {
    let f = Fixture::configured(true, None, true).await;
    let send = |body: Value| {
        auth(
            TestClient::post(format!("{BASE}/runner/final-replies")),
            &f.cap,
        )
        .json(&body)
    };
    let input = json!({"call_id":"final","body":"Verified output"});
    assert_eq!(
        send(input.clone()).send(&f.service).await.status_code,
        Some(StatusCode::CONFLICT)
    );
    let (code, _) = operation(&f, "done", json!({"action":"transition","status":"done"})).await;
    assert_eq!(code, StatusCode::OK);
    for field in [
        "room_id",
        "owner_mxid",
        "task_id",
        "execution_epoch",
        "route",
        "device_id",
        "delivered",
        "fence",
    ] {
        let mut forged = input.clone();
        forged[field] = json!("forged");
        assert_eq!(
            send(forged).send(&f.service).await.status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    let mut response = send(input.clone()).send(&f.service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let result: Value = response.take_json().await.unwrap();
    let id = result["id"].as_str().unwrap();
    assert_eq!(result["state"], "pending");
    let mut replay = send(input).send(&f.service).await;
    assert!(
        replay.take_json::<Value>().await.unwrap()["replayed"]
            .as_bool()
            .unwrap()
    );
    let mut response = get(&format!("final-replies/{id}"), &f.cap)
        .send(&f.service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let receipt: Value = response.take_json().await.unwrap();
    for field in [
        "body",
        "route",
        "owner_mxid",
        "room_id",
        "sender_mxid",
        "device_id",
        "transaction_id",
    ] {
        assert!(receipt.get(field).is_none())
    }
    let mut foreign = f.cap.clone();
    foreign.fence += 1;
    assert_eq!(
        get(&format!("final-replies/{id}"), &foreign)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    for path in [
        "final-replies/claim",
        &format!("final-replies/{id}/observe"),
        "matrix/rooms",
        "matrix/transports",
        "matrix/sessions",
    ] {
        assert_eq!(
            auth(TestClient::post(format!("{BASE}/runner/{path}")), &f.cap)
                .json(&json!({}))
                .send(&f.service)
                .await
                .status_code,
            Some(if path == "final-replies/claim" {
                StatusCode::METHOD_NOT_ALLOWED
            } else {
                StatusCode::NOT_FOUND
            })
        );
    }
    let claim = f.domain.claim_final_reply(60_000).await.unwrap().unwrap();
    let outbound = f
        .domain
        .begin_final_reply_send(claim.clone())
        .await
        .unwrap();
    assert_eq!(outbound.route.room_id, "!project:example.test");
    assert_eq!(outbound.route.thread_root, None);
    assert_eq!(
        send(json!({"call_id":"changed","body":"Other"}))
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::CONFLICT)
    );
    f.close().await;
    let legacy = Fixture::new(true).await;
    let (code, _) = operation(
        &legacy,
        "done",
        json!({"action":"transition","status":"done"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        auth(
            TestClient::post(format!("{BASE}/runner/final-replies")),
            &legacy.cap
        )
        .json(&json!({"call_id":"final","body":"Legacy unverified output"}))
        .send(&legacy.service)
        .await
        .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    legacy.close().await;
}

#[tokio::test]
async fn native_runner_http_graphs() {
    use hagency_core::workflows::WorkflowReceipt;
    let f = Fixture::new(true).await;
    let send = |path: &str, cap: &RunnerCapability, body: Value| {
        auth(TestClient::post(format!("{BASE}/runner/{path}")), cap).json(&body)
    };
    let mut response = send(
        "conversations",
        &f.cap,
        json!({"call_id":"group","label":"Graph work","participant_engagements":[f.engagement]}),
    )
    .send(&f.service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let group: Value = response.take_json().await.unwrap();
    let group = &group["conversation"];
    let session = group["participants"][0]["id"].as_str().unwrap();
    let body = json!({"call_id":"graph","conversation_id":group["id"],"definition":{"label":"Implement and verify","nodes":[{"id":"implement","assignee":session,"description":"Implement scoped work"},{"id":"verify","assignee":session,"description":"Verify scoped work","depends_on":["implement"]}]}});
    for key in [
        "creator_session_id",
        "parent_task_id",
        "owner",
        "workspace",
        "inspection",
    ] {
        let mut forged = body.clone();
        forged[key] = json!("forged");
        assert_eq!(
            send("graphs", &f.cap, forged)
                .send(&f.service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    let mut forged = body.clone();
    forged["definition"]["nodes"][0]["task_id"] = json!("task");
    assert_eq!(
        send("graphs", &f.cap, forged)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    let mut foreign = body.clone();
    foreign["definition"]["nodes"][0]["assignee"] = json!("session");
    assert_eq!(
        send("graphs", &f.cap, foreign)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    let mut response = send("graphs", &f.cap, body.clone()).send(&f.service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let receipt: WorkflowReceipt = response.take_json().await.unwrap();
    let w = receipt.workflow;
    let path = format!("graphs/{}", w.id);
    assert_eq!(
        get(&path, &f.cap).send(&f.service).await.status_code,
        Some(StatusCode::OK)
    );
    let mut response = send("graphs", &f.cap, body).send(&f.service).await;
    assert!(
        response
            .take_json::<WorkflowReceipt>()
            .await
            .unwrap()
            .replayed
    );
    let mut response = get("graphs?limit=1", &f.cap).send(&f.service).await;
    let list: Vec<Value> = response.take_json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["node_count"], 2);
    assert!(list[0].get("definition").is_none());
    let n = &w.nodes[0].binding;
    f.domain
        .enqueue_peer_dispatch(
            DispatchInput {
                id: "node_dispatch".into(),
                session_id: n.session_id.clone(),
                task_id: Some(n.task_id.clone()),
                resources: vec![],
                payload: json!({"instruction":"Execute admitted assignment"}),
            },
            vec![n.message_sequence.unwrap()],
        )
        .await
        .unwrap();
    let worker = f
        .domain
        .claim_dispatch("node_runner".into(), now(), 60_000, 60_000, 8)
        .await
        .unwrap()
        .unwrap();
    f.domain
        .start_dispatch(worker.clone(), now())
        .await
        .unwrap();
    assert_eq!(
        get(&path, &worker).send(&f.service).await.status_code,
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(
        send(
            &format!("{path}/cancel"),
            &worker,
            json!({"call_id":"cancel"})
        )
        .send(&f.service)
        .await
        .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    let results = format!("{path}/results");
    let result = json!({"call_id":"result","node_id":"implement","outcome":{"kind":"complete","result":{"score":0.25}}});
    assert_eq!(
        send(&results, &f.cap, result.clone())
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(
        send(&results, &worker, result.clone())
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::CONFLICT)
    );
    let mut forged = result.clone();
    forged["outcome"]["execution_epoch"] = json!(1);
    assert_eq!(
        send(&results, &worker, forged)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    assert_eq!(
        post(
            &n.task_id,
            &worker,
            &json!({"call_id":"done","operation":{"action":"transition","status":"done"}})
        )
        .send(&f.service)
        .await
        .status_code,
        Some(StatusCode::OK)
    );
    assert_eq!(
        send(&results, &worker, result.clone())
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::OK)
    );
    let mut response = send(&results, &worker, result).send(&f.service).await;
    assert!(
        response.take_json::<Value>().await.unwrap()["replayed"]
            .as_bool()
            .unwrap()
    );
    let dependencies = format!("{path}/dependencies");
    let mut response = get(&dependencies, &worker).send(&f.service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert!(response.take_json::<Vec<Value>>().await.unwrap().is_empty());
    assert_eq!(
        get(&format!("{dependencies}?limit=33"), &worker)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    assert_eq!(
        send(&dependencies, &worker, json!({"node_id":"implement"}))
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    let mut response = send(&dependencies, &f.cap, json!({"node_id":"implement"}))
        .send(&f.service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["result"],
        json!({"score":0.25})
    );
    assert_eq!(
        send(
            &format!("{path}/inspect"),
            &worker,
            json!({"evidence":"forged"})
        )
        .send(&f.service)
        .await
        .status_code,
        Some(StatusCode::NOT_FOUND)
    );
    f.domain
        .complete_dispatch(worker, json!({"reported":true}), now())
        .await
        .unwrap();
    let mut response = send(
        &format!("{path}/cancel"),
        &f.cap,
        json!({"call_id":"cancel"}),
    )
    .send(&f.service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["workflow"]["state"],
        "cancelled"
    );
    f.close().await;
}

#[tokio::test]
async fn native_runner_http_conversation_lifecycle() {
    let f = Fixture::new(true).await;
    let mut response = auth(
        TestClient::post(format!("{BASE}/runner/conversations")),
        &f.cap,
    )
    .json(
        &json!({"call_id":"group","label":"coordination","participant_engagements":[f.engagement]}),
    )
    .send(&f.service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let result: Value = response.take_json().await.unwrap();
    let id = result["conversation"]["id"].as_str().unwrap();
    let send = |body: Value| {
        auth(
            TestClient::post(format!("{BASE}/runner/conversations/{id}/operations")),
            &f.cap,
        )
        .json(&body)
    };
    let members = json!({"call_id":"members","expected_revision":0,"action":{"kind":"members","participant_engagements":[f.engagement]}});
    for key in ["creator_session_id", "owner", "fence", "stop_evidence"] {
        let mut forged = members.clone();
        forged[key] = json!("forged");
        assert_eq!(
            send(forged).send(&f.service).await.status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    let mut foreign = members.clone();
    foreign["action"]["participant_engagements"] = json!(["foreign"]);
    assert_eq!(
        send(foreign).send(&f.service).await.status_code,
        Some(StatusCode::FORBIDDEN)
    );
    let mut response = send(members.clone()).send(&f.service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["conversation"]["revision"],
        1
    );
    let mut response = send(members).send(&f.service).await;
    assert!(
        response.take_json::<Value>().await.unwrap()["replayed"]
            .as_bool()
            .unwrap()
    );
    let closing = json!({"call_id":"close","expected_revision":1,"action":{"kind":"close"}});
    let mut forged = closing.clone();
    forged["action"]["participant_engagements"] = json!([]);
    assert_eq!(
        send(forged).send(&f.service).await.status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    let mut response = send(closing.clone()).send(&f.service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["conversation"]["state"],
        "closed"
    );
    assert_eq!(
        send(closing).send(&f.service).await.status_code,
        Some(StatusCode::OK)
    );
    assert_eq!(
        send(json!({"call_id":"again","expected_revision":2,"action":{"kind":"close"}}))
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::CONFLICT)
    );
    let host = auth(
        TestClient::post(format!("{BASE}/runner/conversation-stops/dispatch/settle")),
        &f.cap,
    )
    .json(&json!({"evidence":"pretend stopped"}))
    .send(&f.service)
    .await;
    assert_eq!(host.status_code, Some(StatusCode::NOT_FOUND));
    f.close().await;
}

#[tokio::test]
async fn native_runner_http_conversations() {
    let f = Fixture::new(true).await;
    let body = json!({"call_id":"conversation","label":"内部协作","participant_engagements":[f.engagement]});
    let send = |body: Value| {
        auth(
            TestClient::post(format!("{BASE}/runner/conversations")),
            &f.cap,
        )
        .json(&body)
    };
    let mut forged = body.clone();
    forged["creator_session_id"] = json!("operator");
    assert_eq!(
        send(forged).send(&f.service).await.status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    let mut foreign = body.clone();
    foreign["participant_engagements"] = json!(["missing"]);
    assert_eq!(
        send(foreign).send(&f.service).await.status_code,
        Some(StatusCode::FORBIDDEN)
    );
    let mut res = send(body.clone()).send(&f.service).await;
    assert_eq!(res.status_code, Some(StatusCode::OK));
    let result: Value = res.take_json().await.unwrap();
    let group = &result["conversation"];
    assert_eq!(group["creator_session_id"], "session");
    assert_eq!(group["participants"][0]["kind"], "internal");
    assert!(group["participants"][0].get("room_id").is_none());
    let path = format!("conversations/{}", group["id"].as_str().unwrap());
    assert_eq!(
        get(&path, &f.cap).send(&f.service).await.status_code,
        Some(StatusCode::OK)
    );
    let mut res = send(body.clone()).send(&f.service).await;
    assert_eq!(res.take_json::<Value>().await.unwrap()["replayed"], true);
    let (status, _) = operation(&f, "done", json!({"action":"transition","status":"done"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        send(body).send(&f.service).await.status_code,
        Some(StatusCode::CONFLICT)
    );
    f.close().await;
}

#[tokio::test]
async fn native_runner_http_peer_mailbox() {
    let f = Fixture::new(true).await;
    let mut res = auth(
        TestClient::post(format!("{BASE}/runner/conversations")),
        &f.cap,
    )
    .json(&json!({"call_id":"group","label":"协作","participant_engagements":[f.engagement]}))
    .send(&f.service)
    .await;
    assert_eq!(res.status_code, Some(StatusCode::OK));
    let result: Value = res.take_json().await.unwrap();
    let group = &result["conversation"];
    let target = group["participants"][0]["id"].as_str().unwrap();
    let body = json!({"call_id":"send","conversation_id":group["id"],"recipient_session_ids":[target],"kind":"request","summary":"处理任务","data":{"score":0.25}});
    let send = |body: Value| {
        auth(
            TestClient::post(format!("{BASE}/runner/peer-messages")),
            &f.cap,
        )
        .json(&body)
    };
    for key in [
        "source_session_id",
        "source_engagement_id",
        "source_task_id",
        "source_dispatch_id",
        "received_at",
    ] {
        let mut forged = body.clone();
        forged[key] = json!("forged");
        assert_eq!(
            send(forged).send(&f.service).await.status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    let mut foreign = body.clone();
    foreign["recipient_session_ids"] = json!(["missing"]);
    assert_eq!(
        send(foreign).send(&f.service).await.status_code,
        Some(StatusCode::FORBIDDEN)
    );
    let before = now();
    let mut res = send(body.clone()).send(&f.service).await;
    assert_eq!(res.status_code, Some(StatusCode::OK));
    let receipt: Value = res.take_json().await.unwrap();
    let mut replay = send(body.clone()).send(&f.service).await;
    assert_eq!(replay.take_json::<Value>().await.unwrap()["replayed"], true);
    let inbox = f.domain.peer_inbox(target.into(), 0, 100).await.unwrap();
    assert_eq!(inbox.len(), 1);
    assert!(inbox[0].message.received_at >= before && inbox[0].message.received_at <= now());
    assert_eq!(inbox[0].message.source_task_id.as_deref(), Some("task"));
    f.domain
        .enqueue_peer_dispatch(
            DispatchInput {
                id: "peer_worker".into(),
                session_id: target.into(),
                task_id: None,
                resources: vec![],
                payload: json!({"instruction":"Handle request"}),
            },
            vec![receipt["sequence"].as_u64().unwrap()],
        )
        .await
        .unwrap();
    let cap = f
        .domain
        .claim_dispatch("peer_runner".into(), now(), 60_000, 120_000, 8)
        .await
        .unwrap()
        .unwrap();
    f.domain.start_dispatch(cap.clone(), now()).await.unwrap();
    let mut next = body.clone();
    next["call_id"] = json!("next");
    assert_eq!(
        send(next).send(&f.service).await.status_code,
        Some(StatusCode::OK)
    );
    let mut res = get("peer-inbox", &cap).send(&f.service).await;
    assert_eq!(res.status_code, Some(StatusCode::OK));
    let page: Value = res.take_json().await.unwrap();
    assert_eq!(page.as_array().unwrap().len(), 1);
    assert_eq!(page[0]["message"]["sequence"], receipt["sequence"]);
    let mut res = get("peer-inbox", &f.cap).send(&f.service).await;
    assert!(
        res.take_json::<Value>()
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        get("peer-inbox?limit=101", &cap)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    f.domain
        .park_dispatch(cap.clone(), true, now())
        .await
        .unwrap();
    assert_eq!(
        get("peer-inbox", &cap).send(&f.service).await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.close().await;
}

#[tokio::test]
async fn native_runner_http_delegation() {
    let f = Fixture::with_thread(true, None).await;
    let body = json!({"call_id":"delegate","assignee_engagement":f.engagement,"root_sequence":f.source_sequence,"definition":{"title":"编写测试","parent_id":"task"}});
    let send = |value: Value| {
        auth(
            TestClient::post(format!("{BASE}/runner/delegations")),
            &f.cap,
        )
        .json(&value)
    };
    for field in ["owner_mxid", "sender", "delivery", "activation"] {
        let mut forged = body.clone();
        forged[field] = json!("forged");
        let res = send(forged).send(&f.service).await;
        assert_eq!(res.status_code, Some(StatusCode::BAD_REQUEST), "{field}");
    }
    let mut foreign = body.clone();
    foreign["assignee_engagement"] = json!("missing");
    assert_eq!(
        send(foreign).send(&f.service).await.status_code,
        Some(StatusCode::FORBIDDEN)
    );
    let mut res = send(body.clone()).send(&f.service).await;
    assert_eq!(res.status_code, Some(StatusCode::OK));
    let task: Value = res.take_json().await.unwrap();
    assert_eq!(task["activation"], "pending");
    assert_eq!(task["replayed"], false);
    let mut res = send(body.clone()).send(&f.service).await;
    assert_eq!(res.status_code, Some(StatusCode::OK));
    assert_eq!(res.take_json::<Value>().await.unwrap()["replayed"], true);
    let mut changed = body.clone();
    changed["definition"]["title"] = json!("different");
    assert_eq!(
        send(changed).send(&f.service).await.status_code,
        Some(StatusCode::CONFLICT)
    );
    let mut res = get(
        &format!("tasks/{}", task["task_id"].as_str().unwrap()),
        &f.cap,
    )
    .send(&f.service)
    .await;
    assert_eq!(res.status_code, Some(StatusCode::OK));
    let child: Value = res.take_json().await.unwrap();
    assert_eq!(child["title"], "编写测试");
    assert_eq!(child["creator_session_id"], "session");
    assert!(
        f.domain
            .inbox(task["session_id"].as_str().unwrap().into(), 0, 100, None)
            .await
            .unwrap()
            .is_empty()
    );
    f.domain
        .park_dispatch(f.cap.clone(), true, now())
        .await
        .unwrap();
    assert_eq!(
        send(body).send(&f.service).await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.close().await;
}

#[tokio::test]
async fn native_runner_http_authority() {
    let f = Fixture::new(true).await;
    let mut valid = get("tasks", &f.cap).send(&f.service).await;
    assert_eq!(valid.status_code, Some(StatusCode::OK));
    assert_eq!(valid.headers()["cache-control"], "no-store");
    let text = valid.take_string().await.unwrap();
    assert!(!text.contains(&f.cap.secret));
    assert!(!text.contains(TOKEN));
    assert!(!text.contains("private_other_task"));
    for builder in [
        TestClient::get(format!("{BASE}/runner/tasks")),
        TestClient::get(format!("{BASE}/runner/tasks")).bearer_auth(TOKEN),
        TestClient::get(format!("{BASE}/runner/tasks?capability={}", f.cap.secret)),
    ] {
        let response = builder
            .add_header("host", "127.0.0.1:13300", true)
            .send(&f.service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::UNAUTHORIZED));
    }
    for (name, value) in [
        ("origin", "https://attacker.test"),
        ("sec-fetch-site", "same-origin"),
        ("forwarded", "for=127.0.0.1"),
        ("x-forwarded-for", "127.0.0.1"),
        ("host", "attacker.test:13300"),
    ] {
        assert_eq!(
            get("tasks", &f.cap)
                .add_header(name, value, true)
                .send(&f.service)
                .await
                .status_code,
            Some(StatusCode::FORBIDDEN)
        );
    }
    for name in [
        "authorization",
        "x-hagency-dispatch",
        "x-hagency-runner",
        "x-hagency-fence",
    ] {
        assert_eq!(
            get("tasks", &f.cap)
                .add_header(name, "duplicate", false)
                .send(&f.service)
                .await
                .status_code,
            Some(StatusCode::UNAUTHORIZED),
            "{name}"
        );
    }
    assert_eq!(
        get("tasks", &f.cap)
            .add_header("host", "127.0.0.1:13300", false)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(
        get("tasks?secret=discard_me", &f.cap)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    let bad = RunnerCapability {
        fence: f.cap.fence + 1,
        ..f.cap.clone()
    };
    assert_eq!(
        get("tasks", &bad).send(&f.service).await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        auth(TestClient::get(format!("{BASE}/resources")), &f.cap)
            .send(&f.service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    for path in ["claim", "start", "recover", "sessions", "resources"] {
        let response = auth(TestClient::post(format!("{BASE}/runner/{path}")), &f.cap)
            .json(&json!({}))
            .send(&f.service)
            .await;
        assert!(response.status_code.unwrap().is_client_error(), "{path}");
    }
    f.domain
        .park_dispatch(f.cap.clone(), true, now())
        .await
        .unwrap();
    assert_eq!(
        get("tasks", &f.cap).send(&f.service).await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.domain
        .park_dispatch(f.cap.clone(), false, now())
        .await
        .unwrap();
    f.domain
        .revoke("revoke".into(), f.engagement.clone())
        .await
        .unwrap();
    assert_eq!(
        get("tasks", &f.cap).send(&f.service).await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.close().await;
    let early = Fixture::new(false).await;
    assert_eq!(
        get("tasks", &early.cap)
            .send(&early.service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    early.close().await;
}

#[tokio::test]
async fn native_runner_http_task_lifecycle() {
    let f = Fixture::new(true).await;
    let mut task = get("tasks/task", &f.cap).send(&f.service).await;
    assert_eq!(
        task.take_json::<Value>().await.unwrap()["status"],
        "in_progress"
    );
    for id in ["private_other_task", "not_found"] {
        let mut response = get(&format!("tasks/{id}"), &f.cap).send(&f.service).await;
        assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
        assert_eq!(
            response.take_json::<Value>().await.unwrap(),
            json!({"ok":false,"code":"task_scope_required"})
        );
        assert_eq!(
            post(
                id,
                &f.cap,
                &json!({"call_id":"no","operation":{"action":"transition","status":"done"}})
            )
            .send(&f.service)
            .await
            .status_code,
            Some(StatusCode::FORBIDDEN)
        );
    }
    let before = now();
    let (code, beat) = operation(&f, "beat", json!({"action":"execution","heartbeat":true})).await;
    assert_eq!(code, StatusCode::OK);
    assert!(beat["task"]["heartbeat_at"].as_u64().unwrap() >= before);
    let (code, comment) = operation(
        &f,
        "comment",
        json!({"action":"comment","text":"Acceptance checks passed"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(comment["replayed"], false);
    let (_, replayed) = operation(
        &f,
        "comment",
        json!({"action":"comment","text":"Acceptance checks passed"}),
    )
    .await;
    assert_eq!(replayed["replayed"], true);
    assert_eq!(
        operation(&f, "comment", json!({"action":"comment","text":"changed"}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let mut comments = get("tasks/task/comments?limit=1", &f.cap)
        .send(&f.service)
        .await;
    let comments: Value = comments.take_json().await.unwrap();
    assert_eq!(comments[0]["author"], "小白");
    assert_eq!(comments.as_array().unwrap().len(), 1);
    assert_eq!(operation(&f,"blocked",json!({"action":"transition","status":"blocked","waiting_reason":"dependency","waiting_until":"2026-10-01T00:00:00Z"})).await.0,StatusCode::OK);
    assert_eq!(
        operation(
            &f,
            "resume",
            json!({"action":"transition","status":"in_progress"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (code, done) = operation(&f, "done", json!({"action":"transition","status":"done"})).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(done["task"]["execution_epoch"], 1);
    assert_eq!(done["task"]["status"], "done");
    assert_eq!(
        operation(&f, "done", json!({"action":"transition","status":"done"}))
            .await
            .1["replayed"],
        true
    );
    f.domain
        .complete_dispatch(f.cap.clone(), json!({"text":"reported"}), now())
        .await
        .unwrap();
    assert_eq!(
        get("tasks/task", &f.cap).send(&f.service).await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.close().await;
}

#[tokio::test]
async fn native_runner_http_inbox_and_limits() {
    let f = Fixture::new(true).await;
    let mut inbox = get("inbox?limit=1", &f.cap).send(&f.service).await;
    let first: Value = inbox.take_json().await.unwrap();
    assert_eq!(first.as_array().unwrap().len(), 1);
    let seq = first[0]["message"]["sequence"].as_u64().unwrap();
    let source = InboundMessage {
        server_name: "example.test".into(),
        room_id: "!project:example.test".into(),
        event_id: "$later".into(),
        sender_mxid: "@owner:example.test".into(),
        thread_root: Some("$thread".into()),
        body: "Later instruction".into(),
        kind: "m.text".into(),
        origin_ts: now(),
    };
    f.domain
        .ingest_message(
            source,
            vec![MessageTarget {
                session_id: "session".into(),
                wake: true,
            }],
            now(),
        )
        .await
        .unwrap();
    let mut later = get(&format!("inbox?after={seq}"), &f.cap)
        .send(&f.service)
        .await;
    assert_eq!(later.take_json::<Value>().await.unwrap(), json!([]));
    for path in [
        "tasks?limit=101",
        "tasks?limit=invalid",
        "inbox?after=not_an_integer",
        "tasks/task/comments?limit=0",
    ] {
        assert_eq!(
            get(path, &f.cap).send(&f.service).await.status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    for body in [
        json!({"call_id":"bad","operation":{"action":"comment","text":"spoof","author":"operator"}}),
        json!({"call_id":"bad","operation":{"action":"execution","heartbeat":true,"status":"done"}}),
        json!({"call_id":"bad","operation":{"action":"approve_all"}}),
        json!({"call_id":"bad","now":0,"operation":{"action":"transition","status":"done"}}),
    ] {
        assert_eq!(
            post("task", &f.cap, &body)
                .send(&f.service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    assert_eq!(
        auth(
            TestClient::post(format!("{BASE}/runner/tasks/task/operations")),
            &f.cap
        )
        .body("plain text")
        .send(&f.service)
        .await
        .status_code,
        Some(StatusCode::UNSUPPORTED_MEDIA_TYPE)
    );
    assert_eq!(
        post(
            "task",
            &f.cap,
            &json!({"call_id":"large","operation":{"action":"comment","text":"x".repeat(65*1024)}})
        )
        .send(&f.service)
        .await
        .status_code,
        Some(StatusCode::PAYLOAD_TOO_LARGE)
    );
    let mut comments = get("tasks/task/comments", &f.cap).send(&f.service).await;
    assert_eq!(comments.take_json::<Value>().await.unwrap(), json!([]));
    f.domain
        .complete_dispatch(f.cap.clone(), json!({"text":"All done!"}), now())
        .await
        .unwrap();
    let inspect = rusqlite::Connection::open(f._root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        inspect
            .query_row(
                "SELECT json_extract(config,'$.status') FROM canonical_tasks WHERE id='task'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "in_progress"
    );
    drop(inspect);
    f.close().await;
}
