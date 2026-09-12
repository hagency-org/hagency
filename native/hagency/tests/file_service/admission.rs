//! Actual file owner with a real pre-launch Started handoff. These bounded
//! custody tests do not qualify the separate executable/recipient scenario.
use super::*;
use crate::bootstrap::workspace::WorkspaceAccess;
use hagency_core::{replies::*, tasks::*};
use hagency_execution::{Host, LaunchAck, Limits, Operation};
use hagency_store::{OwnedClaimProfile, OwnedClaimRoom, private};
use std::{
    collections::BTreeMap,
    sync::{Condvar, atomic::AtomicU8},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub(super) struct Hooks {
    source: Mutex<Option<Arc<SourceGate>>>,
    pub close_mode: AtomicU8,
    source_unwind: AtomicBool,
}
impl Hooks {
    pub fn block_source(&self) {
        let gate = self.source.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.entered.store(true, Ordering::Release);
            gate.notice.notify_one();
            let opened = gate.opened.lock().unwrap();
            let (opened, _) = gate
                .condition
                .wait_timeout_while(opened, Duration::from_secs(10), |open| !*open)
                .unwrap();
            assert!(
                *opened,
                "fixture did not release actual blocked file worker"
            );
        }
        assert!(
            !self.source_unwind.swap(false, Ordering::AcqRel),
            "fixture original file job unwind before capture"
        );
    }
}
pub(super) struct SourceGate {
    entered: AtomicBool,
    notice: tokio::sync::Notify,
    opened: Mutex<bool>,
    condition: Condvar,
}
pub(super) struct Gate(Arc<SourceGate>);
impl Gate {
    pub async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let notice = self.0.notice.notified();
                if self.0.entered.load(Ordering::Acquire) {
                    return;
                }
                notice.await;
            }
        })
        .await
        .expect("source worker did not reach controlled checkpoint");
    }
    pub fn release(&self) {
        *self.0.opened.lock().unwrap() = true;
        self.0.condition.notify_all();
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.release();
    }
}
pub(super) struct Harness {
    pub f: test_common::Fixture,
    pub fake: test_common::Fake,
    pub shared: Shared,
    pub owner: FileOwner,
    pub cap: RunnerCapability,
    operation: Operation,
    launch_ack: Option<LaunchAck>,
}
impl Harness {
    pub async fn new(limit: usize) -> Self {
        let f = test_common::Fixture::new();
        let mut fake = test_common::Fake::start(true).await;
        let config = f
            .config_under_load(&fake.endpoint)
            .with_root_pem(include_bytes!(
                "../../../hagency-matrix/tests/fixtures/ca.pem"
            ))
            .unwrap();
        let shared = Shared {
            domain: f.store.clone(),
            collector: Arc::new(hagency_matrix::Collector::new(config, f.store.clone()).unwrap()),
            workspace: WorkspaceAccess::new(),
        };
        let cancel = CancellationToken::new();
        let (collected, ()) = test_common::scripted(
            shared.collector.collect(&cancel),
            test_common::success(&mut fake, "file_service"),
        )
        .await;
        collected.unwrap();
        shared
            .collector
            .resume_outgoing_custody(&cancel)
            .await
            .unwrap();
        f.store
            .resolve_verified_matrix_session(SessionBinding {
                id: "file_service".into(),
                engagement_id: f.identity.transport.engagement_id.clone(),
                room_id: "!direct:example.test".into(),
                thread_root: None,
            })
            .await
            .unwrap();
        f.store
            .create_canonical_task(
                "task".into(),
                "file_service".into(),
                "File custody".into(),
                now(),
            )
            .await
            .unwrap();
        f.store.register_workspace("work".into()).await.unwrap();
        f.store
            .enqueue_dispatch(DispatchInput {
                id: "dispatch".into(),
                session_id: "file_service".into(),
                task_id: Some("task".into()),
                resources: vec![ResourceLease {
                    id: "work".into(),
                    exclusive: true,
                }],
                payload: serde_json::json!({"instruction":"bounded local fixture"}),
            })
            .await
            .unwrap();
        let root = f.root.path().join("work");
        private::directory(&root).unwrap();
        let executable = std::env::current_exe().unwrap();
        let host = Host::new(
            executable.clone(),
            executable,
            BTreeMap::new(),
            BTreeMap::from([("work".into(), root.canonicalize().unwrap())]),
        )
        .unwrap()
        .with_file_limit(limit)
        .unwrap();
        let profile = OwnedClaimProfile::new(
            f.identity.transport.clone(),
            vec![
                OwnedClaimRoom::new(
                    "!direct:example.test".into(),
                    1,
                    RoomPrivacy::Direct {
                        human_mxid: "@owner:example.test".into(),
                    },
                )
                .unwrap(),
            ],
            vec!["work".into()],
        )
        .unwrap();
        let cap = f
            .store
            .claim_owned_dispatch_for_host(profile, "file_host".into(), 60_000, 60_000, 1)
            .await
            .unwrap()
            .unwrap();
        let mut operation = Operation::start_requiring_workspace(
            f.store.clone(),
            cap.clone(),
            host,
            Limits {
                operation_ms: 30_000,
                response_ms: 1000,
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let (binding, ack) = loop {
            if let Some(value) = operation.take_workspace_registration() {
                break value.into_parts();
            }
            assert!(
                Instant::now() < deadline && !operation.is_finished(),
                "real Started handoff absent"
            );
            tokio::task::yield_now().await;
        };
        shared
            .workspace
            .register_for_service_test(cap.clone(), binding)
            .await
            .unwrap();
        private::directory(&f.root.path().join("file_state")).unwrap();
        let owner = FileOwner::start(
            shared.clone(),
            Setup {
                directory: f.root.path().join("file_state/media"),
                namespace: "file_service_custody".into(),
                limit,
            },
        )
        .unwrap();
        Self {
            f,
            fake,
            shared,
            owner,
            cap,
            operation,
            launch_ack: Some(ack),
        }
    }
    pub async fn initialize(&self) -> bool {
        match self.owner.handle().initialize().await {
            Ok(()) => true,
            Err(FileError::Unavailable) if cfg!(windows) => {
                // Windows directory sync is an explicit unqualified profile;
                // this branch proves only refusal, never positive capture/upload.
                assert!(!self.owner.handle.registry.ready.load(Ordering::Acquire));
                assert!(matches!(
                    self.owner
                        .handle()
                        .submit(self.cap.clone(), input("unqualified")),
                    Err(FileError::Unavailable)
                ));
                assert_eq!(self.count(), 0);
                eprintln!(
                    "file-service qualification: Windows directory durability unavailable; no source admission or POST"
                );
                false
            }
            Err(error) => panic!("file owner initialization: {error:?}"),
        }
    }
    pub fn gate(&self) -> Gate {
        let gate = Arc::new(SourceGate {
            entered: AtomicBool::new(false),
            notice: tokio::sync::Notify::new(),
            opened: Mutex::new(false),
            condition: Condvar::new(),
        });
        *self.owner.handle.registry.tests.source.lock().unwrap() = Some(gate.clone());
        Gate(gate)
    }
    pub fn count(&self) -> u64 {
        let sql =
            rusqlite::Connection::open(self.f.root.path().join("domain/domain.sqlite3")).unwrap();
        sql.query_row("SELECT COUNT(*) FROM file_deliveries", [], |r| r.get(0))
            .unwrap()
    }
    pub async fn empty(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.owner.handle.registry.empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("known-safe original jobs were not joined/released");
    }
    pub async fn close(self) {
        self.close_with(Ok(())).await;
    }
    pub async fn close_with(mut self, expected: Result<(), FileError>) {
        self.owner.quiesce();
        self.shared.workspace.retire();
        assert_eq!(self.owner.close().await, expected);
        assert_eq!(self.owner.close().await, expected);
        if let Some(thread) = self.owner.thread.take() {
            // Fault fixtures know the exact worker has returned or unwound.
            // Joining it here releases the test owner, without converting the
            // original unknown service receipt into success.
            let _ = thread.join();
            assert_eq!(self.owner.close().await, expected);
        }
        self.operation.cancel();
        drop(self.launch_ack.take());
        let report = self.operation.wait_boxed().await.unwrap();
        assert_eq!(report.protocol, hagency_execution::Protocol::NotStarted);
        assert!(!report.retains_process_custody());
        drop(report);
        self.shared.collector.close().await.unwrap();
        test_common::shutdown_domain(&self.f.store, "file service custody").await;
        self.fake.close().await;
    }
}
pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub(super) fn input(id: &str) -> SendFile {
    SendFile {
        call_id: id.into(),
        path: "missing.bin".into(),
        filename: None,
        caption: None,
    }
}

#[tokio::test]
async fn native_file_service_admission_blocked_worker_caller_loss() {
    let mut h = Harness::new(128).await;
    if !h.initialize().await {
        h.fake.no_request().await;
        h.close().await;
        return;
    }
    let gate = h.gate();
    let handle = h.owner.handle();
    let first = handle
        .submit(h.cap.clone(), input("first"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(first.status, FileStatus::Queued);
    gate.entered().await;
    let replay = handle
        .submit(h.cap.clone(), input("first"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(replay.delivery_id, first.delivery_id);
    assert!(replay.replayed);
    // Real synchronous source thread is now held; second command is admitted in
    // finite memory but not durably acknowledged. Losing its receiver changes
    // neither the command nor the original two-job bound.
    drop(handle.submit(h.cap.clone(), input("second")).unwrap());
    assert_eq!(h.count(), 1);
    assert!(matches!(
        handle.submit(h.cap.clone(), input("third")),
        Err(FileError::Busy)
    ));
    assert_eq!(
        handle
            .submit(h.cap.clone(), input("second"))
            .unwrap()
            .wait()
            .await,
        Err(FileError::Unknown)
    );
    gate.release();
    h.empty().await;
    assert_eq!(h.count(), 2);
    for request in [input("first"), input("second")] {
        let id =
            h.f.store
                .restore_file_delivery(h.cap.clone(), request.request().unwrap())
                .await
                .unwrap()
                .unwrap();
        let receipt =
            h.f.store
                .inspect_file_delivery(h.cap.clone(), id.id().into())
                .await
                .unwrap();
        assert_eq!(
            receipt.error_code,
            Some(hagency_core::file_delivery::FileDeliveryFailure::SourceRefused)
        );
        assert!(receipt.captured.is_none());
    }
    h.fake.no_request().await;
    h.close().await;
}

#[tokio::test]
async fn native_file_service_replay_bounds_changed_source_after_release() {
    let mut h = Harness::new(128).await;
    if !h.initialize().await {
        h.fake.no_request().await;
        h.close().await;
        return;
    }
    let handle = h.owner.handle();
    let original = input("original");
    let first = handle
        .submit(h.cap.clone(), original.clone())
        .unwrap()
        .wait()
        .await
        .unwrap();
    h.empty().await;
    std::fs::write(
        h.f.root.path().join("work/missing.bin"),
        b"new private bytes must never replace original source refusal",
    )
    .unwrap();
    let replay = handle
        .submit(h.cap.clone(), original.clone())
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(replay.delivery_id, first.delivery_id);
    assert_eq!(replay.status, FileStatus::Failed);
    assert_eq!(replay.error_code.as_deref(), Some("source_refused"));
    assert!(replay.replayed);
    h.empty().await;
    let mut conflict = original;
    conflict.caption = Some("changed selection commitment".into());
    assert_eq!(
        handle.submit(h.cap.clone(), conflict).unwrap().wait().await,
        Err(FileError::Conflict)
    );
    h.empty().await;
    assert_eq!(h.count(), 1);
    let observed = handle
        .inspect(h.cap.clone(), first.delivery_id)
        .await
        .unwrap();
    let json = serde_json::to_string(&observed).unwrap();
    assert!(
        !json.contains("private bytes")
            && !json.contains("caption")
            && !json.contains("mxc:")
            && !json.contains("work/")
    );
    h.fake.no_request().await;
    h.close().await;
}

/// Unknown network custody deliberately outlives this test's local caller. A
/// separate real process contains that lifetime; its exit is NOT settlement.
#[tokio::test]
async fn native_file_service_admission_network_wait_process() {
    const CHILD: &str = "HAGENCY_FILE_SERVICE_NETWORK_CHILD";
    const MARKER: &str = "file-service child: held encrypted POST; second durable; third busy; original unknown retained";
    const REFUSED: &str = "file-service child: Windows staging unavailable; no POST qualified";
    if std::env::var(CHILD).ok().as_deref() != Some("isolated_fixture_v1") {
        use tokio::io::AsyncReadExt;
        let temporary = tempfile::tempdir().unwrap();
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "file_service::admission_tests::native_file_service_admission_network_wait_process",
                "--nocapture",
            ])
            .env(CHILD, "isolated_fixture_v1")
            .env("TMPDIR", temporary.path())
            .env("TMP", temporary.path())
            .env("TEMP", temporary.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let read = async move {
            let mut output = Vec::new();
            stdout.take(8193).read_to_end(&mut output).await.unwrap();
            output
        };
        let observed = tokio::time::timeout(Duration::from_secs(25), async {
            tokio::join!(child.wait(), read)
        })
        .await;
        let (status, output) = match observed {
            Ok(value) => value,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                panic!("isolated file network custody fixture did not finish");
            }
        };
        assert!(
            status.unwrap().success(),
            "isolated original-unknown fixture failed"
        );
        assert!(output.len() <= 8192, "bounded child output exceeded");
        let output = std::str::from_utf8(&output).unwrap();
        if cfg!(windows) && output.contains(REFUSED) {
            assert!(!output.contains(MARKER));
            eprintln!("{REFUSED}");
        } else {
            assert!(
                output.contains(MARKER),
                "actual network custody evidence absent"
            );
        }
        // Reaped process death releases OS objects; it does not upgrade its
        // unknown domain/journal facts or prove that the POST was unsent.
        return;
    }
    use sha2::{Digest, Sha256};
    let mut h = Harness::new(128).await;
    if !h.initialize().await {
        h.fake.no_request().await;
        h.close().await;
        println!("{REFUSED}");
        return;
    }
    const SOURCE: &[u8] = b"original private file bytes before caller disappears";
    std::fs::write(h.f.root.path().join("work/sample.bin"), SOURCE).unwrap();
    let handle = h.owner.handle();
    let mut request = input("network_original");
    request.path = "sample.bin".into();
    // Drop the first admission receiver. Actual command/preparation/source
    // custody remains in the same original fixed owner.
    drop(handle.submit(h.cap.clone(), request.clone()).unwrap());
    let who = h.fake.next().await;
    assert_eq!(who.target, "/_matrix/client/v3/account/whoami");
    who.json(200, test_common::who());
    let post = h.fake.next().await;
    assert_eq!(post.method, "POST");
    assert_eq!(post.target, "/_matrix/media/v3/upload");
    assert_eq!(
        post.headers["authorization"],
        format!("Bearer {}", test_common::TOKEN)
    );
    assert_eq!(post.body.len(), SOURCE.len());
    assert_ne!(post.body, SOURCE);
    let identity =
        h.f.store
            .restore_file_delivery(h.cap.clone(), request.request().unwrap())
            .await
            .unwrap()
            .unwrap();
    let original =
        h.f.store
            .inspect_file_delivery(h.cap.clone(), identity.id().into())
            .await
            .unwrap();
    assert_eq!(
        original.captured.as_ref().unwrap().sha256,
        format!("{:x}", Sha256::digest(SOURCE))
    );
    assert_eq!(
        original.upload,
        hagency_core::uploads::UploadState::WritePossible
    );
    let second = handle
        .submit(h.cap.clone(), input("network_second"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(second.status, FileStatus::Queued);
    assert_eq!(h.count(), 2);
    assert!(matches!(
        handle.submit(h.cap.clone(), input("network_third")),
        Err(FileError::Busy)
    ));
    std::fs::write(
        h.f.root.path().join("work/sample.bin"),
        b"replacement must never be recaptured",
    )
    .unwrap();
    let replay = handle
        .submit(h.cap.clone(), request.clone())
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(replay.delivery_id, identity.id());
    assert!(replay.replayed);
    let mut conflict = request;
    conflict.caption = Some("different body commitment".into());
    assert!(matches!(
        handle.submit(h.cap.clone(), conflict),
        Err(FileError::Conflict)
    ));
    // Claim a longer body than actually supplied and omit clean TLS EOF. No
    // validated UploadResponse can exist, and neither HTTP nor SDK is retried.
    post.unclean(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 80\r\nConnection: close\r\n\r\n{\"content_uri\":".to_vec());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let complete = handle
                .registry
                .jobs
                .lock()
                .unwrap()
                .values()
                .all(|job| !job.info.lock().unwrap().live);
            if complete {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let held = handle
        .registry
        .jobs
        .lock()
        .unwrap()
        .values()
        .find(|job| job.info.lock().unwrap().id.as_deref() == Some(identity.id()))
        .unwrap()
        .clone();
    {
        let original = held.original.lock().await;
        assert!(
            original
                .upload
                .as_ref()
                .unwrap()
                .outcome()
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert!(original.preparation.is_some());
    }
    let result = handle
        .inspect(h.cap.clone(), identity.id().into())
        .await
        .unwrap();
    assert_eq!(result.status, FileStatus::OutcomeUnknown);
    assert_eq!(h.owner.close().await, Err(FileError::Unknown));
    assert_eq!(h.owner.close().await, Err(FileError::Unknown));
    assert!(
        h.owner.thread.is_some(),
        "unknown owner must remain retained"
    );
    assert!(matches!(
        recovery::open(&Setup {
            directory: h.f.root.path().join("file_state/media"),
            namespace: "file_service_custody".into(),
            limit: 128
        }),
        Err(FileError::Unknown)
    ));
    h.fake.no_request().await;
    println!("{MARKER}");
    // Deliberately retain exact source/SDK/media/process wrappers until the
    // enclosing test child exits. No fabricated release or status is written.
    std::mem::forget(h);
}

/// A real task panic must retire its live projection without discarding the
/// original preparation or converting physical owner loss into a close ACK.
#[tokio::test]
async fn native_file_service_shutdown_original_job_unwind() {
    const CHILD: &str = "HAGENCY_FILE_SERVICE_UNWIND_CHILD";
    const MARKER: &str =
        "file-service child: original task unwound; replay and inspect unknown; custody retained";
    const REFUSED: &str =
        "file-service child: Windows staging unavailable; no unwind admission qualified";
    if std::env::var(CHILD).ok().as_deref() != Some("isolated_fixture_v1") {
        use tokio::io::AsyncReadExt;
        let temporary = tempfile::tempdir().unwrap();
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "file_service::admission_tests::native_file_service_shutdown_original_job_unwind",
                "--nocapture",
            ])
            .env(CHILD, "isolated_fixture_v1")
            .env("TMPDIR", temporary.path())
            .env("TMP", temporary.path())
            .env("TEMP", temporary.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let read = async move {
            let mut output = Vec::new();
            stdout.take(8193).read_to_end(&mut output).await.unwrap();
            output
        };
        let observed = tokio::time::timeout(Duration::from_secs(25), async {
            tokio::join!(child.wait(), read)
        })
        .await;
        let (status, output) = match observed {
            Ok(value) => value,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                panic!("isolated file unwind custody fixture did not finish");
            }
        };
        assert!(
            status.unwrap().success(),
            "isolated original-unknown fixture failed"
        );
        assert!(output.len() <= 8192, "bounded child output exceeded");
        let output = std::str::from_utf8(&output).unwrap();
        if cfg!(windows) && output.contains(REFUSED) {
            assert!(!output.contains(MARKER));
            eprintln!("{REFUSED}");
        } else {
            assert!(
                output.contains(MARKER),
                "actual task unwind custody evidence absent"
            );
        }
        // Reaped process death releases OS objects; it does not upgrade its
        // unknown domain/journal facts or prove that the POST was unsent.
        return;
    }

    let mut h = Harness::new(128).await;
    if !h.initialize().await {
        h.fake.no_request().await;
        h.close().await;
        println!("{REFUSED}");
        return;
    }
    std::fs::write(h.f.root.path().join("work/missing.bin"), b"never captured").unwrap();
    let handle = h.owner.handle();
    let gate = h.gate();
    handle
        .registry
        .tests
        .source_unwind
        .store(true, Ordering::Release);
    let queued = handle
        .submit(h.cap.clone(), input("unwind_original"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(queued.status, FileStatus::Queued);
    gate.entered().await;
    let original = handle
        .registry
        .jobs
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert!(original.info.lock().unwrap().live);
    gate.release();
    // Fixture orchestration bound: the isolated child observes its own worker
    // unwinding. Under a whole-workspace run on a four-core host three
    // seconds expired on every iteration while the outer fixture allows
    // twenty-five; ten seconds keeps the observation bounded without turning
    // a missing unwind into a pass.
    tokio::time::timeout(Duration::from_secs(10), async {
        while handle.registry.ready.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    {
        let info = original.info.lock().unwrap();
        assert!(!info.live, "the original task has actually unwound");
        assert!(!info.releasable, "unwind is not a custody release");
    }
    {
        let custody = original.original.lock().await;
        assert!(custody.preparation.is_some());
        assert!(custody.admission.is_some());
        assert!(custody.snapshot.is_none());
    }
    let replay = handle
        .submit(h.cap.clone(), input("unwind_original"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(replay.delivery_id, queued.delivery_id);
    assert_eq!(replay.status, FileStatus::OutcomeUnknown);
    // Exact replay returns the retained live job, whose custody was neither
    // acknowledged nor released, so it carries the custody label (ADR-101
    // amendment); `inspect` below rebuilds through the durable receipt.
    assert_eq!(
        replay.error_code.as_deref(),
        Some(crate::file_service::types::UnknownOrigin::Custody.code())
    );
    assert!(replay.replayed);
    let inspected = handle
        .inspect(h.cap.clone(), queued.delivery_id.clone())
        .await
        .unwrap();
    assert_eq!(inspected.status, FileStatus::OutcomeUnknown);
    assert_eq!(inspected.error_code.as_deref(), Some("outcome_unknown"));
    // The receipt-derived projection stays `outcome_unknown`: `inspect` rebuilds
    // through `from_receipt` and never reads the job's custody-mutated view. The
    // local custody label is therefore observable only on the original job.
    assert_eq!(
        crate::file_service::types::UnknownOrigin::Remote.code(),
        inspected.error_code.as_deref().unwrap()
    );
    let receipt =
        h.f.store
            .inspect_file_delivery(h.cap.clone(), queued.delivery_id)
            .await
            .unwrap();
    assert!(receipt.captured.is_none());
    assert_eq!(
        receipt.stage,
        hagency_core::uploads::UploadStageState::Unbound
    );
    assert!(matches!(
        handle.submit(h.cap.clone(), input("new_after_unwind")),
        Err(FileError::Unavailable)
    ));
    assert_eq!(h.count(), 1);
    assert_eq!(handle.registry.jobs.lock().unwrap().len(), 1);
    assert!(Arc::ptr_eq(
        handle
            .registry
            .jobs
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap(),
        &original
    ));
    assert_eq!(h.owner.close().await, Err(FileError::Unknown));
    assert_eq!(h.owner.close().await, Err(FileError::Unknown));
    assert!(h.owner.pending_close.is_none());
    assert_eq!(h.owner.close_result, Some(Err(FileError::Unknown)));
    assert!(!h.owner.thread.as_ref().unwrap().is_finished());
    assert!(matches!(
        recovery::open(&Setup {
            directory: h.f.root.path().join("file_state/media"),
            namespace: "file_service_custody".into(),
            limit: 128,
        }),
        Err(FileError::Unknown)
    ));
    h.fake.no_request().await;
    println!("{MARKER}");
    // Only the enclosing, reaped process ends physical custody. No test setter
    // releases the original job, lock, preparation or execution Report.
    std::mem::forget(h);
}
