mod common;
use common::*;
use hagency_core::tasks::*;
use hagency_store::{
    DomainRepository, DomainStore, EffectOutcome, Error, OwnedDispatchScope, private,
    task_context::RetainedTaskContext,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    future::{Future, poll_fn},
    io::Write,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    task::Poll,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
struct Fixture {
    root: tempfile::TempDir,
    domain: DomainStore,
    cap: RunnerCapability,
    scope: OwnedDispatchScope,
    engagement: String,
    work: PathBuf,
    contexts: PathBuf,
}
impl Fixture {
    async fn new(start: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("workspace");
        let contexts = root.path().join("contexts");
        private::directory(&work).unwrap();
        private::directory(&contexts).unwrap();
        let work = work.canonicalize().unwrap();
        let contexts = contexts.canonicalize().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("context", "Worker", &pool, 100));
        let engagement = db.admit(&proof, 1000).unwrap().id;
        db.approve("approved", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "offline_fixture".into(),
            },
        )
        .unwrap();
        db.register_session(&SessionBinding {
            id: "session".into(),
            engagement_id: engagement.clone(),
            room_id: "!project:example.test".into(),
            thread_root: None,
        })
        .unwrap();
        db.register_workspace("work").unwrap();
        db.create_canonical_task("task", "session", "Original context", now())
            .unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "dispatch".into(),
            session_id: "session".into(),
            task_id: Some("task".into()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"task_id":"impostor"}),
        })
        .unwrap();
        let cap = db
            .claim_dispatch("owner", now(), 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
        let domain = DomainStore::start(db, 16).unwrap();
        let mut scope = domain.owned_dispatch_scope(cap.clone()).await.unwrap();
        if start {
            scope = domain
                .start_owned_dispatch(cap.clone(), scope.fingerprint().into())
                .await
                .unwrap();
        }
        Self {
            root,
            domain,
            cap,
            scope,
            engagement,
            work,
            contexts,
        }
    }
    fn context(&self) -> Arc<RetainedTaskContext> {
        RetainedTaskContext::new(self.contexts.clone(), &"a".repeat(64)).unwrap()
    }
    fn path(&self) -> PathBuf {
        self.contexts
            .join(format!("context-{}.json", "a".repeat(64)))
    }
    async fn close(self) {
        self.domain.shutdown().await.unwrap();
    }
}
fn guard() -> (Instant, Arc<AtomicBool>) {
    (
        Instant::now() + Duration::from_secs(10),
        Arc::new(AtomicBool::new(false)),
    )
}

#[tokio::test]
async fn native_retained_task_context_bind() {
    let f = Fixture::new(true).await;
    let context = f.context();
    let (deadline, cancel) = guard();
    let mut environment = BTreeMap::new();
    context.apply_environment(&mut environment).unwrap();
    let reference = environment
        .get(&std::ffi::OsString::from("HAGENCY_RUNNER_CAPABILITY"))
        .unwrap()
        .to_str()
        .unwrap();
    assert!(!reference.contains(&f.cap.secret));
    let reference: Value = serde_json::from_str(reference).unwrap();
    assert_eq!(reference["profile"], "retained_task_context_v1");
    assert_eq!(reference["path"], json!(f.path()));
    assert_eq!(reference.as_object().unwrap().len(), 3);
    assert!(!f.path().exists());
    context.separate_from(&f.work).unwrap();
    assert!(context.separate_from(f.root.path()).is_err());
    assert!(context.separate_from(&f.contexts).is_err());
    context
        .bind(
            f.domain.clone(),
            f.cap.clone(),
            f.scope.clone(),
            deadline,
            cancel.clone(),
        )
        .await
        .unwrap();
    let before = fs::read(f.path()).unwrap();
    let held = private::open(&f.path(), false).unwrap();
    let record: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(record.as_object().unwrap().len(), 5);
    assert_eq!(record["version"], 1);
    assert_eq!(record["scope"], f.scope.fingerprint());
    assert_eq!(record["task_id"], "task");
    assert!(record["capability"] == json!(f.cap));
    assert!(before.len() <= 4096);
    context
        .bind(
            f.domain.clone(),
            f.cap.clone(),
            f.scope.clone(),
            deadline,
            cancel.clone(),
        )
        .await
        .unwrap();
    assert_eq!(before, fs::read(f.path()).unwrap());
    assert!(hagency_platform::same_file(&held, &private::open(&f.path(), false).unwrap()).unwrap());
    assert_eq!(
        f.domain
            .check_owned_dispatch(f.cap.clone(), f.scope.fingerprint().into())
            .await
            .unwrap()
            .status,
        TaskState::InProgress
    );
    assert!(matches!(
        context
            .bind(
                f.domain.clone(),
                f.cap.clone(),
                f.scope.clone(),
                deadline + Duration::from_secs(1),
                cancel.clone()
            )
            .await,
        Err(Error::Conflict)
    ));
    assert!(matches!(
        context
            .bind(
                f.domain.clone(),
                f.cap.clone(),
                f.scope.clone(),
                deadline,
                Arc::new(AtomicBool::new(false))
            )
            .await,
        Err(Error::Conflict)
    ));
    f.domain
        .revoke("revoke".into(), f.engagement.clone())
        .await
        .unwrap();
    assert!(
        context
            .bind(
                f.domain.clone(),
                f.cap.clone(),
                f.scope.clone(),
                deadline,
                cancel
            )
            .await
            .is_err()
    );
    assert_eq!(before, fs::read(f.path()).unwrap());
    f.close().await;
}

#[tokio::test]
async fn native_retained_task_context_refusals() {
    let f = Fixture::new(false).await;
    let context = f.context();
    let (deadline, cancel) = guard();
    assert!(matches!(
        context
            .bind(
                f.domain.clone(),
                f.cap.clone(),
                f.scope.clone(),
                deadline,
                cancel.clone()
            )
            .await,
        Err(Error::RunnerAuthority)
    ));
    assert!(!f.path().exists());
    let started = f
        .domain
        .start_owned_dispatch(f.cap.clone(), f.scope.fingerprint().into())
        .await
        .unwrap();
    assert!(
        context
            .bind(f.domain.clone(), f.cap.clone(), started, deadline, cancel)
            .await
            .is_err()
    );
    assert!(!f.path().exists());
    f.close().await;
    let f = Fixture::new(true).await;
    assert_eq!(
        f.domain
            .check_owned_dispatch(f.cap.clone(), f.scope.fingerprint().into())
            .await
            .unwrap()
            .status,
        TaskState::InProgress
    );
    // Inspection returns current Task state, not the sealed Started receipt.
    // Even this authorized getter cannot reconstruct a lost acknowledgement.
    assert!(f.domain.owned_dispatch_scope(f.cap.clone()).await.is_err());
    assert!(!f.path().exists());
    f.close().await;
    for fault in ["forged", "partial", "bytes", "replacement", "root"] {
        let f = Fixture::new(true).await;
        let context = f.context();
        let (deadline, cancel) = guard();
        let mut cap = f.cap.clone();
        let mut original = None;
        match fault {
            "forged" => cap.secret = "0".repeat(64),
            "partial" => private::write_new(&f.path(), b"partial").unwrap(),
            "bytes" | "replacement" => {
                context
                    .bind(
                        f.domain.clone(),
                        cap.clone(),
                        f.scope.clone(),
                        deadline,
                        cancel.clone(),
                    )
                    .await
                    .unwrap();
                let bytes = fs::read(f.path()).unwrap();
                original = Some(bytes.clone());
                if fault == "bytes" {
                    let mut file = private::open(&f.path(), false).unwrap();
                    file.set_len(0).unwrap();
                    file.write_all(b"changed").unwrap();
                } else {
                    fs::rename(f.path(), f.contexts.join("held-original")).unwrap();
                    private::write_new(&f.path(), &bytes).unwrap();
                }
            }
            "root" => {
                fs::rename(&f.contexts, f.root.path().join("held-contexts")).unwrap();
                private::directory(&f.contexts).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            context
                .bind(
                    f.domain.clone(),
                    cap,
                    f.scope.clone(),
                    deadline,
                    cancel.clone()
                )
                .await
                .is_err(),
            "{fault}"
        );
        if fault == "partial" {
            assert_eq!(fs::read(f.path()).unwrap(), b"partial");
            fs::remove_file(f.path()).unwrap();
        }
        if fault == "bytes" {
            let mut file = private::open(&f.path(), false).unwrap();
            file.set_len(0).unwrap();
            file.write_all(original.as_ref().unwrap()).unwrap();
        }
        assert!(
            context
                .bind(
                    f.domain.clone(),
                    f.cap.clone(),
                    f.scope.clone(),
                    deadline,
                    cancel
                )
                .await
                .is_err(),
            "sticky {fault}"
        );
        if matches!(fault, "forged" | "partial" | "root") {
            assert!(!f.path().exists());
        }
        if fault == "replacement" {
            assert_eq!(fs::read(f.path()).unwrap(), original.unwrap());
        }
        f.close().await;
    }
    let f = Fixture::new(true).await;
    assert!(RetainedTaskContext::new(f.root.path().join("missing"), &"a".repeat(64)).is_err());
    assert!(!f.root.path().join("missing").exists());
    assert!(RetainedTaskContext::new(f.contexts.join("../contexts"), &"a".repeat(64)).is_err());
    assert!(RetainedTaskContext::new(f.contexts.clone(), &"A".repeat(64)).is_err());
    private::write_new(&f.path(), b"foreign").unwrap();
    assert!(RetainedTaskContext::new(f.contexts.clone(), &"a".repeat(64)).is_err());
    assert_eq!(fs::read(f.path()).unwrap(), b"foreign");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let id = "b".repeat(64);
        symlink("absent", f.contexts.join(format!("context-{id}.json"))).unwrap();
        assert!(RetainedTaskContext::new(f.contexts.clone(), &id).is_err());
        fs::set_permissions(&f.contexts, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(RetainedTaskContext::new(f.contexts.clone(), &"c".repeat(64)).is_err());
    }
    f.close().await;
}

#[tokio::test]
async fn native_retained_task_context_custody() {
    let f = Fixture::new(true).await;
    let context = f.context();
    let weak = Arc::downgrade(&context);
    let (deadline, cancel) = guard();
    let mut receiver = Box::pin(context.bind(
        f.domain.clone(),
        f.cap.clone(),
        f.scope.clone(),
        deadline,
        cancel.clone(),
    ));
    poll_fn(|cx| {
        assert!(receiver.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(receiver);
    assert!(matches!(
        context
            .bind(
                f.domain.clone(),
                f.cap.clone(),
                f.scope.clone(),
                deadline,
                cancel
            )
            .await,
        Err(Error::Busy)
    ));
    drop(context);
    assert!(
        weak.upgrade().is_some(),
        "only admitted original job retains the context"
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while weak.upgrade().is_some() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let bytes = fs::read(f.path()).unwrap();
    let record: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(record["capability"] == json!(f.cap));
    assert_eq!(record["task_id"], "task");
    assert!(RetainedTaskContext::new(f.contexts.clone(), &"a".repeat(64)).is_err());
    f.close().await;
}
