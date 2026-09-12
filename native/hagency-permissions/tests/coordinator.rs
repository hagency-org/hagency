mod common;
use common::*;
use hagency_core::{approvals::*, replies::*, tasks::*};
use hagency_permissions::{Binding, CodexApprovals, HostUpdate};
use hagency_runtime::codex::{
    session::{SessionDriver, Settings},
    transport::Limits,
};
use hagency_store::{DomainRepository, DomainStore, EffectOutcome};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::time::timeout;

struct WireGate {
    inner: DuplexStream,
    path: PathBuf,
    enabled: Arc<AtomicBool>,
    block: Arc<AtomicBool>,
    fail: Arc<AtomicBool>,
    seen: Arc<AtomicBool>,
}
impl AsyncWrite for WireGate {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.enabled.load(Ordering::SeqCst) {
            let sql = rusqlite::Connection::open(&self.path).unwrap();
            let count: u64 = sql
                .query_row(
                    "SELECT COUNT(*) FROM owner_approvals WHERE state='applying'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(
                count > 0,
                "no approval byte may be attempted before durable Applying"
            );
            self.seen.store(true, Ordering::SeqCst);
            if self.block.load(Ordering::SeqCst) {
                return Poll::Pending;
            }
            if self.fail.load(Ordering::SeqCst) {
                return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
            }
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
type Coordinator = CodexApprovals<DuplexStream, WireGate, DuplexStream>;
struct Fixture {
    root: tempfile::TempDir,
    store: DomainStore,
    cap: RunnerCapability,
    agent: String,
    coordinator: Coordinator,
    input: DuplexStream,
    output: DuplexStream,
    _stderr: DuplexStream,
    cwd: String,
    block: Arc<AtomicBool>,
    fail: Arc<AtomicBool>,
    seen: Arc<AtomicBool>,
}
struct Prepared {
    root: tempfile::TempDir,
    store: DomainStore,
    cap: RunnerCapability,
    agent: String,
    session: SessionDriver<DuplexStream, WireGate, DuplexStream>,
    input: DuplexStream,
    output: DuplexStream,
    _stderr: DuplexStream,
    cwd: String,
    block: Arc<AtomicBool>,
    fail: Arc<AtomicBool>,
    seen: Arc<AtomicBool>,
}
async fn read(input: &mut DuplexStream) -> Value {
    timeout(Duration::from_secs(2), async {
        let mut data = vec![];
        loop {
            let b = input.read_u8().await.unwrap();
            data.push(b);
            if b == b'\n' {
                return serde_json::from_slice(&data).unwrap();
            }
        }
    })
    .await
    .unwrap()
}
async fn send(output: &mut DuplexStream, value: Value) {
    let mut data = serde_json::to_vec(&value).unwrap();
    data.push(b'\n');
    output.write_all(&data).await.unwrap();
}
impl Prepared {
    fn new(write: bool) -> Pin<Box<dyn std::future::Future<Output = Self>>> {
        Box::pin(async move {
            let root = tempfile::tempdir().unwrap();
            let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
            db.register(&registration()).unwrap();
            let pool = resource();
            db.put_resource(&pool).unwrap();
            let p = proof(&pool);
            let e = db.admit(&p, 1000).unwrap();
            db.approve("approve", &p, 1000).unwrap();
            let effect = db.claim_effect().unwrap().unwrap();
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: "provision".into(),
                },
            )
            .unwrap();
            let n = now();
            db.observe_matrix_transport(
                &MatrixTransportObservation {
                    engagement_id: e.id.clone(),
                    registration_generation: 1,
                    generation: 1,
                    sender_mxid: "@agent:example.test".into(),
                    device_id: "DEVICE".into(),
                },
                n,
            )
            .unwrap();
            db.observe_matrix_room(
                &MatrixRoomObservation {
                    engagement_id: e.id.clone(),
                    registration_generation: 1,
                    transport_generation: 1,
                    generation: 1,
                    room_id: "!project:example.test".into(),
                    privacy: RoomPrivacy::Group {},
                    joined: BTreeSet::from([
                        "@agent:example.test".into(),
                        "@owner:example.test".into(),
                    ]),
                    invite_only: true,
                    encrypted: false,
                },
                n,
            )
            .unwrap();
            db.resolve_verified_matrix_session(
                &SessionBinding {
                    id: "sid".into(),
                    engagement_id: e.id.clone(),
                    room_id: "!project:example.test".into(),
                    thread_root: None,
                },
                n,
            )
            .unwrap();
            db.create_canonical_task("task", "sid", "Approval fixture", n)
                .unwrap();
            db.register_workspace("workspace").unwrap();
            db.enqueue_dispatch(&DispatchInput {
                id: "dispatch".into(),
                session_id: "sid".into(),
                task_id: Some("task".into()),
                resources: vec![ResourceLease {
                    id: "workspace".into(),
                    exclusive: write,
                }],
                payload: json!({"instruction":"fixture"}),
            })
            .unwrap();
            let cap = db
                .claim_dispatch("runner", n, 60_000, 120_000, 8)
                .unwrap()
                .unwrap();
            db.start_dispatch(&cap, n).unwrap();
            db.observe_approval_room(
                &ApprovalRoomObservation {
                    engagement_id: e.id.clone(),
                    registration_generation: 1,
                    generation: 1,
                    room_id: "!private:example.test".into(),
                    device_id: "BOT".into(),
                    joined: BTreeSet::from([
                        "@owner:example.test".into(),
                        "@approval:example.test".into(),
                    ]),
                    invite_only: true,
                    encrypted: true,
                    available: true,
                },
                n,
            )
            .unwrap();
            let store = DomainStore::start(db, 32).unwrap();
            let cwd = root.path().join("workspace").to_str().unwrap().to_owned();
            let (wire_in, mut input) = tokio::io::duplex(131072);
            let (stdout, mut output) = tokio::io::duplex(131072);
            let (stderr, peer_err) = tokio::io::duplex(1024);
            let enabled = Arc::new(AtomicBool::new(false));
            let block = Arc::new(AtomicBool::new(false));
            let fail = Arc::new(AtomicBool::new(false));
            let seen = Arc::new(AtomicBool::new(false));
            let stdin = WireGate {
                inner: wire_in,
                path: root.path().join("state/domain.sqlite3"),
                enabled: enabled.clone(),
                block: block.clone(),
                fail: fail.clone(),
                seen: seen.clone(),
            };
            let settings =
                Settings::new(cwd.clone().into(), "gpt-5.6-sol".into(), "medium".into()).unwrap();
            let settings = if write {
                settings
            } else {
                settings.read_only()
            };
            let mut session = SessionDriver::new(
                stdout,
                stdin,
                stderr,
                settings,
                Limits {
                    write_timeout_ms: 1000,
                    event_wait_ms: 2000,
                    lifetime_ms: 30_000,
                },
                2000,
            )
            .unwrap();
            let (a, ()) = tokio::join!(session.initialize(), async {
                let r = read(&mut input).await;
                send(&mut output,json!({"id":r["id"],"result":{"userAgent":"fixture/0.153.4","platformFamily":"fixture","platformOs":"fixture","codexHome":"/fixture"}})).await;
                read(&mut input).await;
            });
            a.unwrap();
            let (a, ()) = tokio::join!(session.start_thread(), async {
                let r = read(&mut input).await;
                send(&mut output,json!({"id":r["id"],"result":{"thread":{"id":"thread","cwd":cwd,"status":{"type":"idle"},"turns":[]},"cwd":cwd,"model":"gpt-5.6-sol","modelProvider":"fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":if write{"workspaceWrite"}else{"readOnly"},"networkAccess":false}}})).await;
            });
            a.unwrap();
            let (a, ()) = tokio::join!(session.start_turn("fixture".into()), async {
                let r = read(&mut input).await;
                send(&mut output,json!({"id":r["id"],"result":{"turn":{"id":"turn","items":[],"status":"inProgress"}}})).await;
            });
            a.unwrap();
            enabled.store(true, Ordering::SeqCst);
            Self {
                root,
                store,
                cap,
                agent: e.id,
                session,
                input,
                output,
                _stderr: peer_err,
                cwd,
                block,
                fail,
                seen,
            }
        })
    }
}
impl Fixture {
    fn new(write: bool) -> Pin<Box<dyn std::future::Future<Output = Self>>> {
        Box::pin(async move {
            let p = Prepared::new(write).await;
            let coordinator =
                CodexApprovals::attach(p.session, p.store.clone(), p.cap.clone(), binding())
                    .await
                    .unwrap();
            Self {
                root: p.root,
                store: p.store,
                cap: p.cap,
                agent: p.agent,
                coordinator,
                input: p.input,
                output: p.output,
                _stderr: p._stderr,
                cwd: p.cwd,
                block: p.block,
                fail: p.fail,
                seen: p.seen,
            }
        })
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn message(&self, id: Value, kind: &str) -> Value {
        let mut params = json!({"threadId":"thread","turnId":"turn","itemId":format!("item_{kind}_{}",id.as_str().map(str::to_owned).unwrap_or_else(||id.to_string())),"startedAtMs":1});
        let method = match kind {
            "file" => "item/fileChange/requestApproval",
            "permissions" => {
                params["cwd"] = json!(self.cwd);
                params["permissions"] = json!({"network":{"enabled":true}});
                "item/permissions/requestApproval"
            }
            "network" => {
                params["networkApprovalContext"] = json!({"host":"example.org","protocol":"https"});
                "item/commandExecution/requestApproval"
            }
            _ => {
                params["cwd"] = json!(self.cwd);
                params["command"] = json!("echo approved");
                "item/commandExecution/requestApproval"
            }
        };
        json!({"id":id,"method":method,"params":params})
    }
    async fn admit_message(
        &mut self,
        message: Value,
    ) -> Result<HostUpdate, hagency_permissions::Error> {
        send(&mut self.output, message).await;
        self.coordinator.next_update(now() + 30_000).await
    }
    async fn admit(&mut self, id: Value, kind: &str) -> ApprovalSummary {
        let message = self.message(id, kind);
        let HostUpdate::Approval(a) = self.admit_message(message).await.unwrap() else {
            panic!("approval")
        };
        a
    }
    async fn choose(&self, id: &str, choice: ApprovalChoice) {
        let c = self.store.private_approval(id.into()).await.unwrap();
        self.store
            .observe_owner_verdict(OwnerVerdictObservation {
                request_id: id.into(),
                request_digest: c.digest,
                binding_generation: c.binding_generation,
                server_name: "example.test".into(),
                room_id: c.room_id,
                sender_mxid: c.owner_mxid,
                event_id: format!("${id}"),
                encrypted: true,
                choice,
            })
            .await
            .unwrap();
    }
    async fn state(&self, id: &str) -> String {
        self.store.approval_summary(id.into()).await.unwrap().state
    }
    fn dispatch_state(&self) -> String {
        self.sql()
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
    async fn no_bytes(&mut self) {
        assert!(
            timeout(Duration::from_millis(15), self.input.read_u8())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_codex_approval_coordinator_typed_responses() {
    for (kind, allow) in [
        ("command", true),
        ("file", false),
        ("permissions", true),
        ("permissions", false),
    ] {
        let mut f = Fixture::new(true).await;
        let a = f.admit(json!(7), kind).await;
        assert_eq!(f.dispatch_state(), "parked");
        assert!(f.coordinator.apply(&a.id).await.is_err());
        f.no_bytes().await;
        f.choose(
            &a.id,
            if allow {
                ApprovalChoice::Once
            } else {
                ApprovalChoice::Deny
            },
        )
        .await;
        f.coordinator.apply(&a.id).await.unwrap();
        assert!(f.seen.load(Ordering::SeqCst));
        assert_eq!(f.state(&a.id).await, "applying");
        let response = read(&mut f.input).await;
        assert_eq!(response["id"], 7);
        assert_eq!(
            response["result"],
            if kind == "permissions" {
                json!({"permissions":if allow{json!({"network":{"enabled":true}})}else{json!({})},"scope":"turn"})
            } else {
                json!({"decision":if allow{"accept"}else{"decline"}})
            }
        );
        assert!(f.coordinator.apply(&a.id).await.is_err());
        assert_eq!(f.dispatch_state(), "parked");
        f.coordinator.close().await.unwrap();
        f.store.shutdown().await.unwrap();
    }
}
#[tokio::test]
async fn native_codex_approval_coordinator_scope_and_revoke() {
    for change in ["threadId", "turnId", "itemId", "environmentId", "negative"] {
        let mut f = Fixture::new(true).await;
        let mut message = f.message(json!(7), "command");
        match change {
            "itemId" => message["params"][change] = json!(""),
            "negative" => message["id"] = json!(-1),
            _ => message["params"][change] = json!("wrong"),
        };
        assert!(f.admit_message(message).await.is_err(), "{change}");
        assert!(!f.seen.load(Ordering::SeqCst));
        f.store.shutdown().await.unwrap();
    }
    let mut f = Fixture::new(true).await;
    let a = f.admit(json!(7), "command").await;
    f.choose(&a.id, ApprovalChoice::Always).await;
    let grants = f
        .store
        .approval_grants(f.agent.clone(), "".into(), 10)
        .await
        .unwrap();
    f.store
        .revoke_approval_grant(grants[0].id.clone())
        .await
        .unwrap();
    // Revocation consumes an exact deny instead of sending the stale allow.
    f.coordinator.apply(&a.id).await.unwrap();
    assert_eq!(
        read(&mut f.input).await["result"],
        json!({"decision":"decline"})
    );
    assert_eq!(f.state(&a.id).await, "applying");
    f.store.shutdown().await.unwrap();
    let mut f = Fixture::new(false).await;
    let a = f.admit(json!(7), "network").await;
    f.choose(&a.id, ApprovalChoice::Once).await;
    f.coordinator.apply(&a.id).await.unwrap();
    assert_eq!(
        read(&mut f.input).await["result"],
        json!({"decision":"accept"})
    );
    f.store.shutdown().await.unwrap();
    let mut f = Fixture::new(false).await;
    let message = f.message(json!(7), "command");
    assert!(f.admit_message(message).await.is_err());
    assert!(!f.seen.load(Ordering::SeqCst));
    f.store.shutdown().await.unwrap();
}
#[tokio::test]
async fn native_codex_approval_coordinator_database_atomicity() {
    let mut f = Fixture::new(true).await;
    let a = f.admit(json!(7), "command").await;
    f.choose(&a.id, ApprovalChoice::Once).await;
    f.sql().execute_batch("CREATE TRIGGER reject_application BEFORE UPDATE OF state ON owner_approvals WHEN NEW.state='applying' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(f.coordinator.apply(&a.id).await.is_err());
    assert_eq!(f.state(&a.id).await, "decided");
    f.no_bytes().await;
    assert!(!f.seen.load(Ordering::SeqCst));
    f.sql()
        .execute_batch("DROP TRIGGER reject_application;")
        .unwrap();
    f.coordinator.apply(&a.id).await.unwrap();
    assert_eq!(read(&mut f.input).await["result"]["decision"], "accept");
    f.store.shutdown().await.unwrap();
}
#[tokio::test]
async fn native_codex_approval_uncertainty_resolution_and_pending_barrier() {
    let mut f = Fixture::new(true).await;
    let a = f.admit(json!(7), "command").await;
    let b = f.admit(json!("7"), "file").await;
    assert_ne!(a.id, b.id);
    f.choose(&a.id, ApprovalChoice::Once).await;
    f.coordinator.apply(&a.id).await.unwrap();
    read(&mut f.input).await;
    send(
        &mut f.output,
        json!({"method":"serverRequest/resolved","params":{"threadId":"thread","requestId":7}}),
    )
    .await;
    assert!(
        matches!(f.coordinator.next_update(now()+30_000).await.unwrap(),HostUpdate::Resolution(Some(s))if s.state=="uncertain")
    );
    assert_eq!(f.state(&b.id).await, "pending");
    assert!(
        f.store
            .park_dispatch(f.cap.clone(), false, now())
            .await
            .is_err()
    );
    assert!(f.coordinator.apply(&a.id).await.is_err());
    f.store.shutdown().await.unwrap();
    let mut f = Fixture::new(true).await;
    let a = f.admit(json!(7), "command").await;
    send(
        &mut f.output,
        json!({"method":"serverRequest/resolved","params":{"threadId":"thread","requestId":7}}),
    )
    .await;
    f.coordinator.next_update(now() + 30_000).await.unwrap();
    f.choose(&a.id, ApprovalChoice::Once).await;
    assert!(f.coordinator.apply(&a.id).await.is_err());
    assert!(!f.seen.load(Ordering::SeqCst));
    f.store.shutdown().await.unwrap();
}
#[tokio::test]
async fn native_codex_approval_uncertainty_write_cancel_restart() {
    for mode in ["failure", "cancel", "resolved_drop"] {
        let mut f = Fixture::new(true).await;
        let a = f.admit(json!(7), "command").await;
        f.choose(&a.id, ApprovalChoice::Once).await;
        match mode {
            "failure" => {
                f.fail.store(true, Ordering::SeqCst);
                assert!(f.coordinator.apply(&a.id).await.is_err());
                assert_eq!(f.state(&a.id).await, "uncertain");
            }
            "cancel" => {
                f.block.store(true, Ordering::SeqCst);
                let seen = f.seen.clone();
                let mut apply = Box::pin(f.coordinator.apply(&a.id));
                // Applying commits before its reply reaches this caller. A timer
                // started before that reply can cancel without attempting bytes.
                // Poll the actual operation until the blocked writer witnesses
                // durable Applying; the existing lost-response test covers the
                // earlier phase separately.
                timeout(
                    Duration::from_secs(2),
                    std::future::poll_fn(|cx| {
                        assert!(
                            apply.as_mut().poll(cx).is_pending(),
                            "{mode}: apply completed before the blocked-write gate"
                        );
                        if seen.load(Ordering::SeqCst) {
                            Poll::Ready(())
                        } else {
                            Poll::Pending
                        }
                    }),
                )
                .await
                .expect("cancel: first blocked write was not observed");
                assert!(
                    timeout(Duration::from_millis(30), apply.as_mut())
                        .await
                        .is_err(),
                    "{mode}: blocked write unexpectedly completed"
                );
                // Drop exactly once after timeout; never poll a canceled future.
                drop(apply);
                assert!(f.coordinator.is_closed(), "{mode}: session stayed open");
                assert_eq!(f.state(&a.id).await, "applying", "{mode}");
            }
            _ => {
                f.coordinator.apply(&a.id).await.unwrap();
                read(&mut f.input).await;
                assert_eq!(f.state(&a.id).await, "applying");
            }
        }
        assert!(
            f.seen.load(Ordering::SeqCst),
            "{mode}: no write was attempted"
        );
        assert!(f.coordinator.apply(&a.id).await.is_err(), "{mode}: retried");
        f.store.shutdown().await.unwrap();
        let mut reopened = DomainRepository::open(&f.root.path().join("state")).unwrap();
        assert_eq!(reopened.approval_summary(&a.id).unwrap().state, "uncertain");
        assert!(
            reopened
                .consume_owner_approval(&f.cap, &a.id, now())
                .is_err()
        );
        assert!(reopened.park_dispatch(&f.cap, false, now()).is_err());
    }
}

#[tokio::test]
async fn native_codex_approval_coordinator_generation_and_capacity() {
    for changed in ["private", "device", "lease"] {
        let mut f = Fixture::new(true).await;
        let a = f.admit(json!(7), "command").await;
        f.choose(&a.id, ApprovalChoice::Once).await;
        match changed {
            "private" => f
                .store
                .observe_approval_room(ApprovalRoomObservation {
                    engagement_id: f.agent.clone(),
                    registration_generation: 1,
                    generation: 1,
                    room_id: "!private:example.test".into(),
                    device_id: "BOT".into(),
                    joined: BTreeSet::from([
                        "@owner:example.test".into(),
                        "@approval:example.test".into(),
                        "@third:example.test".into(),
                    ]),
                    invite_only: true,
                    encrypted: true,
                    available: true,
                })
                .await
                .unwrap(),
            "device" => f
                .store
                .observe_matrix_transport(MatrixTransportObservation {
                    engagement_id: f.agent.clone(),
                    registration_generation: 1,
                    generation: 2,
                    sender_mxid: "@agent:example.test".into(),
                    device_id: "ROTATED".into(),
                })
                .await
                .unwrap(),
            _ => f.store.reconcile_dispatches(now() + 120_001).await.unwrap(),
        }
        assert!(f.coordinator.apply(&a.id).await.is_err(), "{changed}");
        f.no_bytes().await;
        assert!(!f.seen.load(Ordering::SeqCst));
        f.store.shutdown().await.unwrap();
    }
    let mut f = Fixture::new(true).await;
    for id in 0..16 {
        f.admit(json!(id), "command").await;
    }
    let message = f.message(json!(16), "command");
    assert!(matches!(
        f.admit_message(message).await,
        Err(hagency_permissions::Error::Capacity)
    ));
    assert_eq!(
        f.sql()
            .query_row("SELECT COUNT(*) FROM owner_approvals", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        16
    );
    assert!(!f.seen.load(Ordering::SeqCst));
    f.store.shutdown().await.unwrap();
}
#[tokio::test]
async fn native_codex_approval_uncertainty_eof_and_timeout() {
    for mode in ["eof", "timeout"] {
        let mut f = Fixture::new(true).await;
        let a = f.admit(json!(7), "command").await;
        f.choose(&a.id, ApprovalChoice::Deny).await;
        f.coordinator.apply(&a.id).await.unwrap();
        assert_eq!(
            read(&mut f.input).await["result"],
            json!({"decision":"decline"})
        );
        if mode == "eof" {
            f.output.shutdown().await.unwrap();
        }
        assert!(f.coordinator.next_update(now() + 30_000).await.is_err());
        assert_eq!(f.state(&a.id).await, "uncertain");
        assert!(f.coordinator.is_closed());
        assert_eq!(f.dispatch_state(), "parked");
        f.store.shutdown().await.unwrap();
    }
}

fn binding() -> Binding {
    Binding {
        context_id: "context".into(),
        connection_id: "connection".into(),
        workspace_resource: "workspace".into(),
    }
}
fn poll_pending<F: std::future::Future>(future: Pin<&mut F>) {
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(future.poll(&mut cx).is_pending());
}
async fn wait_row(sql: &rusqlite::Connection, query: &str) {
    timeout(Duration::from_secs(2), async {
        loop {
            if sql.query_row(query, [], |r| r.get::<_, bool>(0)).unwrap() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn native_codex_approval_uncertainty_attach_and_consume_lost_response() {
    let mut p = Prepared::new(true).await;
    let sql = rusqlite::Connection::open(p.root.path().join("state/domain.sqlite3")).unwrap();
    sql.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let mut attach = Box::pin(CodexApprovals::attach(
        p.session,
        p.store.clone(),
        p.cap.clone(),
        binding(),
    ));
    poll_pending(attach.as_mut());
    sql.execute_batch("COMMIT;").unwrap();
    wait_row(
        &sql,
        "SELECT EXISTS(SELECT 1 FROM approval_contexts WHERE id='context')",
    )
    .await;
    // The writer has bound the context; the caller never receives the session.
    drop(attach);
    assert!(
        timeout(Duration::from_secs(1), p.input.read_u8())
            .await
            .unwrap()
            .is_err()
    );
    assert!(!p.seen.load(Ordering::SeqCst));
    p.store.shutdown().await.unwrap();

    let mut f = Fixture::new(true).await;
    let a = f.admit(json!(7), "command").await;
    f.choose(&a.id, ApprovalChoice::Once).await;
    let sql = f.sql();
    sql.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let mut apply = Box::pin(f.coordinator.apply(&a.id));
    poll_pending(apply.as_mut());
    sql.execute_batch("COMMIT;").unwrap();
    wait_row(
        &sql,
        "SELECT EXISTS(SELECT 1 FROM owner_approvals WHERE state='applying')",
    )
    .await;
    // No polling between durable consumption and dropping the future: no
    // descriptor reached the coordinator and no native response was attempted.
    drop(apply);
    assert!(f.coordinator.is_closed());
    assert!(!f.seen.load(Ordering::SeqCst));
    assert_eq!(f.state(&a.id).await, "applying");
    assert!(f.coordinator.apply(&a.id).await.is_err());
    assert!(
        timeout(Duration::from_secs(1), f.input.read_u8())
            .await
            .unwrap()
            .is_err()
    );
    f.store.shutdown().await.unwrap();
    let mut db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    assert_eq!(db.approval_summary(&a.id).unwrap().state, "uncertain");
    assert!(db.consume_owner_approval(&f.cap, &a.id, now()).is_err());
}
