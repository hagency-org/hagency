use super::*;
use hagency_execution::{StartedWorkspace, WorkspaceError};
use hagency_files::{RelativeFile, Snapshot};
const LIMIT: usize = 4 * 1024 * 1024;

async fn binding(f: &Fixture, operation: &mut Operation) -> StartedWorkspace {
    f.entered().await;
    operation
        .take_workspace_binding()
        .expect("actual Started must expose one handoff")
}
fn snapshot(
    value: &StartedWorkspace,
    cap: &RunnerCapability,
    name: &str,
) -> Result<Snapshot, WorkspaceError> {
    value.snapshot(cap, &RelativeFile::new(name).unwrap(), LIMIT)
}
async fn cancel(f: &Fixture, operation: &mut Operation) {
    operation.cancel();
    let report = operation.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::Cancelled));
    drop(report);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_workspace_binding_process() {
    let f = Fixture::new();
    let other = f.root.path().join("other");
    hagency_store::private::directory(&other).unwrap();
    fs::write(other.join("owned-dispatch.entered"), b"impostor").unwrap();
    let mut operation = f.operation("usage-gate");
    let binding = binding(&f, &mut operation).await;
    assert!(operation.take_workspace_binding().is_none());
    binding.validate_current(&f.cap).await.unwrap();
    let bytes = snapshot(&binding, &f.cap, "owned-dispatch.entered").unwrap();
    assert_eq!(bytes.bytes(), b"entered"); // written by actual child using cwd
    assert_eq!(
        fs::read(other.join("owned-dispatch.entered")).unwrap(),
        b"impostor"
    );
    cancel(&f, &mut operation).await;
    assert_eq!(bytes.bytes(), b"entered");
}

#[tokio::test]
async fn native_workspace_binding_scope() {
    let f = Fixture::new();
    let expected = f
        .domain
        .owned_dispatch_scope(f.cap.clone())
        .await
        .unwrap()
        .fingerprint()
        .to_owned();
    let mut operation = f.operation("usage-gate");
    let binding = binding(&f, &mut operation).await;
    let copy = f.root.path().join("copied-writer");
    hagency_store::private::directory(&copy).unwrap();
    // VACUUM preserves the permissions of this actual pre-created empty file;
    // default SQLite creation can be 0644 and is correctly refused by the store.
    drop(hagency_store::private::open(&copy.join("domain.sqlite3"), true).unwrap());
    f.sql()
        .execute(
            "VACUUM INTO ?1",
            [copy.join("domain.sqlite3").to_str().unwrap()],
        )
        .unwrap();
    let copied = DomainRepository::open(&copy).unwrap();
    // Real reopen correctly fences the copied Started row. This negative
    // fixture deliberately restores the exact live rows into that DIFFERENT
    // open database, modeling an incorrect stale replica; it is not a supported
    // recovery API or a claim that reopen authorizes execution.
    let replica = rusqlite::Connection::open(copy.join("domain.sqlite3")).unwrap();
    replica
        .execute(
            "ATTACH DATABASE ?1 AS live",
            [f.root.path().join("state/domain.sqlite3").to_str().unwrap()],
        )
        .unwrap();
    replica.execute_batch("BEGIN IMMEDIATE;
        UPDATE runner_dispatches SET (state,capability_hash,lease_until,capability_until)=(SELECT state,capability_hash,lease_until,capability_until FROM live.runner_dispatches WHERE id='dispatch') WHERE id='dispatch';
        UPDATE runner_sessions SET quarantined=0 WHERE id='session';
        UPDATE workspace_resources SET dirty=0 WHERE id='work';
        UPDATE runner_attempts SET outcome='started' WHERE dispatch_id='dispatch';
        COMMIT;").unwrap();
    drop(replica);
    assert!(
        copied
            .check_owned_dispatch(&f.cap, &expected, now())
            .is_ok()
    );
    let mut wrong = Vec::new();
    let mut value = f.cap.clone();
    value.secret = "a".repeat(64);
    wrong.push(value);
    let mut value = f.cap.clone();
    value.runner_id = "other_runner".into();
    wrong.push(value);
    let mut value = f.cap.clone();
    value.dispatch_id = "other_dispatch".into();
    wrong.push(value);
    let mut value = f.cap.clone();
    value.fence += 1;
    wrong.push(value);
    for cap in wrong {
        assert!(matches!(
            snapshot(&binding, &cap, "owned-dispatch.entered"),
            Err(WorkspaceError::Authority)
        ));
        assert_eq!(
            binding.validate_current(&cap).await,
            Err(WorkspaceError::Authority)
        );
    }
    f.domain
        .revoke("revoke".into(), f.engagement.clone())
        .await
        .unwrap();
    assert!(
        copied
            .check_owned_dispatch(&f.cap, &expected, now())
            .is_ok()
    );
    assert!(matches!(
        binding.validate_current(&f.cap).await,
        Err(WorkspaceError::Current | WorkspaceError::Retired)
    ));
    let report = operation.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::LostAuthority));
    assert!(matches!(
        snapshot(&binding, &f.cap, "owned-dispatch.entered"),
        Err(WorkspaceError::Retired)
    ));
    drop(report);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_workspace_binding_retirement() {
    let f = Fixture::new();
    let mut completed = f.operation("normal");
    let report = completed.wait().await.unwrap();
    assert_eq!(report.protocol, Protocol::Completed);
    let late = completed.take_workspace_binding().unwrap();
    assert!(matches!(
        snapshot(&late, &f.cap, "owned-dispatch.entered"),
        Err(WorkspaceError::Retired)
    ));
    drop(report);
    drop(late);
    f.domain.shutdown().await.unwrap();
    for late in [false, true] {
        let f = Fixture::new();
        let mut operation = f.operation("usage-gate");
        f.entered().await;
        let early = (!late).then(|| operation.take_workspace_binding().unwrap());
        operation.cancel();
        if let Some(binding) = &early {
            assert!(matches!(
                snapshot(binding, &f.cap, "owned-dispatch.entered"),
                Err(WorkspaceError::Retired)
            ));
        }
        let report = operation.wait().await.unwrap();
        assert_eq!(report.failure, Some(Failure::Cancelled));
        let binding = early.unwrap_or_else(|| operation.take_workspace_binding().unwrap());
        assert!(matches!(
            snapshot(&binding, &f.cap, "owned-dispatch.entered"),
            Err(WorkspaceError::Retired)
        ));
        drop(report);
        drop(binding);
        f.domain.shutdown().await.unwrap();
    }
    let f = Fixture::new();
    let mut operation = f.operation("usage-gate");
    let binding = binding(&f, &mut operation).await;
    let bytes = snapshot(&binding, &f.cap, "owned-dispatch.entered").unwrap();
    drop(operation); // waits for actual retained worker cleanup
    assert!(matches!(
        snapshot(&binding, &f.cap, "owned-dispatch.entered"),
        Err(WorkspaceError::Retired)
    ));
    assert_eq!(bytes.bytes(), b"entered");
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_workspace_binding_replacement() {
    let f = Fixture::new();
    let host = f.host("usage-gate", "work", false);
    let moved = f.root.path().join("moved");
    #[cfg(unix)]
    {
        fs::rename(&f.work, &moved).unwrap();
        hagency_store::private::directory(&f.work).unwrap();
        let mut operation =
            Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
        assert_eq!(
            operation.wait().await.unwrap().failure,
            Some(Failure::Admission)
        );
        assert!(operation.take_workspace_binding().is_none());
        assert!(!f.work.join("owned-dispatch.entered").exists());
        assert!(!moved.join("owned-dispatch.entered").exists());
    }
    #[cfg(windows)]
    {
        // Actual cap-std directory ownership denies ordinary rename. This is
        // not a successful hostile-ancestor replacement qualification.
        assert!(fs::rename(&f.work, &moved).is_err());
        drop(host);
        fs::rename(&f.work, &moved).unwrap();
    }
    f.domain.shutdown().await.unwrap();

    let f = Fixture::new();
    let mut operation = f.operation("usage-gate");
    let binding = binding(&f, &mut operation).await;
    let bytes = snapshot(&binding, &f.cap, "owned-dispatch.entered").unwrap();
    #[cfg(unix)]
    {
        fs::rename(&f.work, f.root.path().join("original")).unwrap();
        hagency_store::private::directory(&f.work).unwrap();
        fs::write(f.work.join("owned-dispatch.entered"), b"replacement").unwrap();
        assert!(matches!(
            snapshot(&binding, &f.cap, "owned-dispatch.entered"),
            Err(WorkspaceError::Root)
        ));
    }
    #[cfg(windows)]
    assert!(fs::rename(&f.work, f.root.path().join("original")).is_err());
    assert_eq!(bytes.bytes(), b"entered");
    cancel(&f, &mut operation).await;
}

#[tokio::test]
async fn native_workspace_binding_bounds() {
    let f = Fixture::new();
    assert!(
        Host::new(
            binary(),
            binary(),
            BTreeMap::new(),
            BTreeMap::from([("a".into(), f.work.clone()), ("b".into(), f.work.clone()),])
        )
        .is_err()
    );
    let nested = f.work.join("nested");
    hagency_store::private::directory(&nested).unwrap();
    assert!(
        Host::new(
            binary(),
            binary(),
            BTreeMap::new(),
            BTreeMap::from([("parent".into(), f.work.clone()), ("child".into(), nested),])
        )
        .is_err()
    );
    assert!(
        f.host("usage-gate", "work", false)
            .with_file_limit(0)
            .is_err()
    );
    assert!(
        f.host("usage-gate", "work", false)
            .with_file_limit(LIMIT + 1)
            .is_err()
    );
    let host = f
        .host("usage-gate", "work", false)
        .with_file_limit(16)
        .unwrap();
    let mut operation = Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
    let binding = binding(&f, &mut operation).await;
    // A nonexistent path still returns Limit: no larger-profile source open
    // happens before a smaller caller bound is refused.
    assert!(matches!(
        binding.snapshot(&f.cap, &RelativeFile::new("absent").unwrap(), 15),
        Err(WorkspaceError::Limit)
    ));
    let mut cap = f.cap.clone();
    cap.secret = "a".repeat(65_536);
    assert!(matches!(
        snapshot(&binding, &cap, "owned-dispatch.entered"),
        Err(WorkspaceError::Authority)
    ));
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(snapshot(&binding, &f.cap, "owned-dispatch.entered").unwrap());
    }
    assert!(matches!(
        snapshot(&binding, &f.cap, "owned-dispatch.entered"),
        Err(WorkspaceError::Snapshot(hagency_files::Error::Capacity))
    ));
    held.pop();
    held.push(snapshot(&binding, &f.cap, "owned-dispatch.entered").unwrap());
    fs::write(f.work.join("large"), [b'x'; 17]).unwrap();
    drop(held);
    assert!(matches!(
        snapshot(&binding, &f.cap, "large"),
        Err(WorkspaceError::Snapshot(hagency_files::Error::Capacity))
    ));
    cancel(&f, &mut operation).await;

    let missing = f.root.path().join("missing");
    assert!(
        Host::new(
            binary(),
            binary(),
            BTreeMap::new(),
            BTreeMap::from([("work".into(), missing)])
        )
        .is_err()
    );
    let too_many = (0..17)
        .map(|i| (format!("work{i}"), f.work.clone()))
        .collect();
    assert!(Host::new(binary(), binary(), BTreeMap::new(), too_many).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&f.work, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            Host::new(
                binary(),
                binary(),
                BTreeMap::new(),
                BTreeMap::from([("work".into(), f.work.clone())])
            )
            .is_err()
        );
    }
}
