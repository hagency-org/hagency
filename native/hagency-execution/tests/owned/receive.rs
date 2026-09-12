//! Actual host Started receipts and filesystem effects. Ingress below is domain
//! fixture data; it does not claim SDK authentication or executable receive parity.
use super::*;
use hagency_core::{
    attachments::{AttachmentMetadata, MatrixAttachmentObservation},
    ingress::MatrixEventObservation,
    messages::InboundMessage,
    received_files::ReceivedFileFacts,
    replies::{MatrixRoomObservation, MatrixTransportObservation, RoomPrivacy},
};
use hagency_execution::{
    StartedWorkspace, WorkspaceError, WorkspaceReceive, WorkspaceReceiveError,
};
use hagency_store::ReceiveWrite;
use std::{collections::BTreeSet, future::Future, pin::pin, task::Poll, time::Instant};

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("received 工作区");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let p = proof(&request("receive_allocation", "Worker", &pool, 100));
    let e = db.admit(&p, 1000).unwrap();
    db.approve("approved", &p, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "offline provisioned".into(),
        },
    )
    .unwrap();
    db.observe_matrix_transport(
        &MatrixTransportObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "DEVICE_1".into(),
        },
        now(),
    )
    .unwrap();
    db.observe_matrix_room(
        &MatrixRoomObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: "!project:example.test".into(),
            generation: 1,
            privacy: RoomPrivacy::Group {},
            joined: BTreeSet::from(["@worker:example.test".into(), "@owner:example.test".into()]),
            invite_only: true,
            encrypted: true,
        },
        now(),
    )
    .unwrap();
    db.resolve_verified_matrix_session(
        &SessionBinding {
            id: "session".into(),
            engagement_id: e.id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: Some("$thread".into()),
        },
        now(),
    )
    .unwrap();
    db.create_canonical_task("task", "session", "Receive untrusted user data", now())
        .unwrap();
    db.register_workspace("work").unwrap();
    for index in 0..8 {
        let event = format!("$file_{index}");
        db.admit_matrix_attachment(
            &MatrixAttachmentObservation {
                event: MatrixEventObservation {
                    scope: db.matrix_ingress_scope("session").unwrap(),
                    event: InboundMessage {
                        server_name: "example.test".into(),
                        room_id: "!project:example.test".into(),
                        event_id: event.clone(),
                        sender_mxid: "@owner:example.test".into(),
                        thread_root: Some("$thread".into()),
                        body: "untrusted filename and bytes".into(),
                        kind: "m.file".into(),
                        origin_ts: now(),
                    },
                    mentions: BTreeSet::from(["@worker:example.test".into()]),
                    encrypted: true,
                },
                metadata: AttachmentMetadata {
                    filename: "untrusted-input-name.bin".into(),
                    mime_type: Some("application/octet-stream".into()),
                    declared_size: Some(1),
                },
                sdk_identity: "1".repeat(64),
                manifest_id: hagency_core::project::hash(event.as_bytes()),
                content_digest: hagency_core::project::hash(format!("cipher_{event}").as_bytes()),
            },
            now(),
        )
        .unwrap();
    }
    // Select all eight original actual canonical inputs in one frozen dispatch.
    db.enqueue_inbox_dispatch(
        &DispatchInput {
            id: "dispatch".into(),
            session_id: "session".into(),
            task_id: Some("task".into()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"receive original selected attachments"}),
        },
        &(1..=8).collect::<Vec<_>>(),
    )
    .unwrap();
    let cap = db
        .claim_dispatch("owned_host", now(), 60_000, 60_000, 1)
        .unwrap()
        .unwrap();
    let domain = DomainStore::start(db, 16).unwrap();
    Fixture {
        root,
        work,
        domain,
        cap,
        engagement: e.id,
    }
}

async fn started(f: &Fixture, operation: &mut Operation) -> StartedWorkspace {
    f.entered().await;
    operation
        .take_workspace_binding()
        .expect("actual child Started handoff")
}
fn facts(bytes: &[u8]) -> ReceivedFileFacts {
    ReceivedFileFacts {
        size: bytes.len() as u64,
        sha256: hagency_core::project::hash(bytes),
    }
}
async fn grant(f: &Fixture, index: usize, bytes: &[u8]) -> ReceiveWrite {
    let admission = f
        .domain
        .reserve_received_file(f.cap.clone(), format!("$file_{index}"), 4 * 1024 * 1024)
        .await
        .unwrap();
    f.domain
        .start_received_file_write(f.cap.clone(), admission.reservation.unwrap(), facts(bytes))
        .await
        .unwrap()
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(4)
}
async fn owner(
    f: &Fixture,
    workspace: &StartedWorkspace,
    index: usize,
    bytes: &[u8],
) -> WorkspaceReceive {
    workspace
        .prepare_receive(&f.cap, grant(f, index, bytes).await, deadline())
        .unwrap()
}
async fn close(f: &Fixture, operation: &mut Operation) {
    operation.cancel();
    let report = operation.wait().await.unwrap();
    assert!(matches!(
        report.failure,
        Some(Failure::Cancelled | Failure::LostAuthority)
    ));
    drop(report);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_receive_workspace_sink_original() {
    let f = fixture();
    let mut operation = f.operation("usage-gate");
    let binding = started(&f, &mut operation).await;
    let bytes = vec![b'p'; 3 * 64 * 1024 + 17];
    let mut held = owner(&f, &binding, 0, &bytes).await;
    let expected = format!(
        ".hagency-received-{}.bin",
        held.identity().id().strip_prefix("receive_").unwrap()
    );
    assert_eq!(held.relative_path(), expected);
    let path = f.work.join(held.relative_path());
    assert!(!path.exists());
    held.materialize(&bytes).await.unwrap();
    assert_eq!(held.facts(), &facts(&bytes));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    hagency_store::private::check_handle(&fs::File::open(&path).unwrap()).unwrap();
    held.revalidate(deadline()).await.unwrap();
    assert_eq!(
        held.materialize(&bytes).await,
        Err(WorkspaceReceiveError::Attempted)
    );
    let mut empty = owner(&f, &binding, 1, b"").await;
    empty.materialize(b"").await.unwrap();
    empty.revalidate(deadline()).await.unwrap();
    assert_eq!(
        fs::metadata(f.work.join(empty.relative_path()))
            .unwrap()
            .len(),
        0
    );
    // Same bytes and length in another regular object never restore identity.
    #[cfg(unix)]
    {
        fs::rename(&path, f.work.join("actual-original")).unwrap();
        let mut replacement = hagency_store::private::open(&path, true).unwrap();
        std::io::Write::write_all(&mut replacement, &bytes).unwrap();
        assert_eq!(
            held.revalidate(deadline()).await,
            Err(WorkspaceReceiveError::Object)
        );
        assert_eq!(fs::read(f.work.join("actual-original")).unwrap(), bytes);
    }
    #[cfg(windows)]
    {
        // Pinned capability file handles deny rename while held, preserving the
        // actual observed platform behavior rather than claiming replacement.
        assert!(fs::rename(&path, f.work.join("actual-original")).is_err());
        held.revalidate(deadline()).await.unwrap();
    }
    close(&f, &mut operation).await;
}

#[tokio::test]
async fn native_receive_workspace_sink_mutations() {
    for index in 0..4 {
        // Each independent mutation owns a fresh actual runtime. The probe waits
        // silently at usage-gate, so unrelated preceding fsyncs must not spend
        // this case's fixed runtime read deadline before its assertion begins.
        let f = fixture();
        let mut operation = f.operation("usage-gate");
        let binding = started(&f, &mut operation).await;
        let mut held = owner(&f, &binding, index, b"original").await;
        held.materialize(b"original").await.unwrap();
        let path = f.work.join(held.relative_path());
        match index {
            0 => fs::write(&path, b"modified").unwrap(),
            1 => fs::write(&path, b"appended_original").unwrap(),
            2 => fs::hard_link(&path, f.work.join("hard-link")).unwrap(),
            _ => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
                }
                #[cfg(windows)]
                {
                    fs::write(&path, b"").unwrap();
                }
            }
        }
        assert_eq!(
            held.revalidate(deadline()).await,
            Err(WorkspaceReceiveError::Object)
        );
        assert_eq!(
            held.materialize(b"original").await,
            Err(WorkspaceReceiveError::Attempted)
        );
        close(&f, &mut operation).await;
    }
    #[cfg(unix)]
    {
        let f = fixture();
        let mut operation = f.operation("usage-gate");
        let binding = started(&f, &mut operation).await;
        let mut held = owner(&f, &binding, 4, b"original").await;
        held.materialize(b"original").await.unwrap();
        let original = f.root.path().join("original-root");
        fs::rename(&f.work, &original).unwrap();
        hagency_store::private::directory(&f.work).unwrap();
        assert!(matches!(
            held.revalidate(deadline()).await,
            Err(WorkspaceReceiveError::Workspace(WorkspaceError::Root))
        ));
        assert!(!f.work.join(held.relative_path()).exists());
        assert_eq!(
            fs::read(original.join(held.relative_path())).unwrap(),
            b"original"
        );
        close(&f, &mut operation).await;
    }
}

#[tokio::test]
async fn native_receive_workspace_once() {
    let f = fixture();
    let mut operation = f.operation("usage-gate");
    let binding = started(&f, &mut operation).await;
    let mut occupied = owner(&f, &binding, 0, b"new").await;
    let path = f.work.join(occupied.relative_path());
    fs::write(&path, b"existing").unwrap();
    assert_eq!(
        occupied.materialize(b"new").await,
        Err(WorkspaceReceiveError::Object)
    );
    assert_eq!(fs::read(&path).unwrap(), b"existing");
    assert_eq!(
        occupied.materialize(b"new").await,
        Err(WorkspaceReceiveError::Attempted)
    );
    let mut wrong = owner(&f, &binding, 1, b"right").await;
    assert_eq!(
        wrong.materialize(b"wrong").await,
        Err(WorkspaceReceiveError::Association)
    );
    assert!(!f.work.join(wrong.relative_path()).exists());
    assert_eq!(
        wrong.materialize(b"right").await,
        Err(WorkspaceReceiveError::Attempted)
    );
    let mut directory = owner(&f, &binding, 2, b"new").await;
    fs::create_dir(f.work.join(directory.relative_path())).unwrap();
    assert_eq!(
        directory.materialize(b"new").await,
        Err(WorkspaceReceiveError::Object)
    );
    #[cfg(unix)]
    {
        let mut linked = owner(&f, &binding, 3, b"new").await;
        std::os::unix::fs::symlink(&path, f.work.join(linked.relative_path())).unwrap();
        assert_eq!(
            linked.materialize(b"new").await,
            Err(WorkspaceReceiveError::Object)
        );
        assert_eq!(fs::read(&path).unwrap(), b"existing");
    }
    // The caller owns held before polling and unwinds while borrowing its real
    // materialization future. It retains that owner even after a file exists.
    let mut held = owner(&f, &binding, 4, b"retained across unwind").await;
    let path = f.work.join(held.relative_path());
    {
        let mut effect = pin!(held.materialize(b"retained across unwind"));
        std::future::poll_fn(|cx| {
            let observed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let result = effect.as_mut().poll(cx);
                if path.exists() {
                    panic!("controlled caller unwind after actual file creation");
                }
                result
            }));
            match observed {
                Err(_) => Poll::Ready(()),
                Ok(Poll::Pending) => Poll::Pending,
                Ok(Poll::Ready(value)) => {
                    panic!("effect ended without expected actual file: {value:?}")
                }
            }
        })
        .await;
    }
    assert_eq!(fs::read(&path).unwrap(), b"retained across unwind");
    assert_eq!(
        held.materialize(b"retained across unwind").await,
        Err(WorkspaceReceiveError::Attempted)
    );
    #[cfg(windows)]
    assert!(fs::rename(&path, f.work.join("cannot-move-held")).is_err());
    let mut lost = owner(&f, &binding, 5, b"never created").await;
    let absent = f.work.join(lost.relative_path());
    let lock = f.sql();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    {
        let mut effect = pin!(lost.materialize(b"never created"));
        std::future::poll_fn(|cx| {
            assert!(effect.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        // Drop the borrowed caller while the original writer cannot acquire its
        // current-authority transaction; the unique owner's attempt stays spent.
    }
    lock.execute_batch("ROLLBACK").unwrap();
    assert!(!absent.exists());
    assert_eq!(
        lost.materialize(b"never created").await,
        Err(WorkspaceReceiveError::Attempted)
    );
    assert_eq!(
        lost.revalidate(deadline()).await,
        Err(WorkspaceReceiveError::Incomplete)
    );
    close(&f, &mut operation).await;
}

#[tokio::test]
async fn native_receive_workspace_authority_retirement() {
    let f = fixture();
    let mut operation = f.operation("usage-gate");
    let binding = started(&f, &mut operation).await;
    let mut bad_cap = f.cap.clone();
    bad_cap.secret = "0".repeat(64);
    assert!(matches!(
        binding.prepare_receive(&bad_cap, grant(&f, 0, b"no").await, deadline()),
        Err(WorkspaceReceiveError::Association)
    ));
    let mut expired = binding
        .prepare_receive(&f.cap, grant(&f, 1, b"expired").await, Instant::now())
        .unwrap();
    assert_eq!(
        expired.materialize(b"expired").await,
        Err(WorkspaceReceiveError::Deadline)
    );
    assert!(!f.work.join(expired.relative_path()).exists());
    assert_eq!(
        expired.revalidate(deadline()).await,
        Err(WorkspaceReceiveError::Incomplete)
    );
    let mut current = owner(&f, &binding, 2, b"current").await;
    current.materialize(b"current").await.unwrap();
    let mut before = owner(&f, &binding, 3, b"not written").await;
    operation.cancel();
    assert!(matches!(
        before.materialize(b"not written").await,
        Err(WorkspaceReceiveError::Workspace(WorkspaceError::Retired))
    ));
    assert!(!f.work.join(before.relative_path()).exists());
    assert!(matches!(
        current.revalidate(deadline()).await,
        Err(WorkspaceReceiveError::Workspace(WorkspaceError::Retired))
    ));
    assert_eq!(
        fs::read(f.work.join(current.relative_path())).unwrap(),
        b"current"
    );
    close(&f, &mut operation).await;
}

#[tokio::test]
async fn native_receive_workspace_authority_read_deadline() {
    let f = fixture();
    let mut operation = f.operation("usage-gate");
    let binding = started(&f, &mut operation).await;
    // Hosted Windows once needed more than one second to materialize under the
    // parallel test binary, so the write window is generous. The original
    // write deadline still expires before the fresh revalidation below, which
    // keeps proving that an elapsed write deadline never gates later reads.
    let until = Instant::now() + Duration::from_secs(3);
    let mut held = binding
        .prepare_receive(&f.cap, grant(&f, 0, b"cached").await, until)
        .unwrap();
    held.materialize(b"cached").await.unwrap();
    tokio::time::sleep_until((until + Duration::from_millis(5)).into()).await;
    assert_eq!(
        held.revalidate(until).await,
        Err(WorkspaceReceiveError::Deadline)
    );
    held.revalidate(deadline()).await.unwrap();
    assert_eq!(
        held.materialize(b"cached").await,
        Err(WorkspaceReceiveError::Attempted)
    );
    // Direct capability expiry is deliberately negative evidence. Only the
    // sink's captured original writer observes this changed current authority.
    f.sql()
        .execute(
            "UPDATE runner_dispatches SET capability_until=1 WHERE id='dispatch'",
            [],
        )
        .unwrap();
    assert!(matches!(
        held.revalidate(deadline()).await,
        Err(WorkspaceReceiveError::Workspace(
            WorkspaceError::Current | WorkspaceError::Retired
        ))
    ));
    close(&f, &mut operation).await;
}
