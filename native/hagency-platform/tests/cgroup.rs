//! Refusal evidence only. Protected cgroup execution uses the separate explicitly
//! provisioned hagency-cgroup-probe executable; no missing-delegation pass branch.
#[cfg(target_os = "linux")]
#[test]
fn native_cgroup_unprovisioned_refusal() {
    use std::{
        fs::File,
        io,
        process::{Command, Stdio},
    };
    let root = tempfile::tempdir().unwrap();
    let procs = tempfile::tempfile().unwrap();
    let kill = tempfile::tempfile().unwrap();
    let outcome = hagency_platform::CgroupRecovery::from_host_files(
        File::open(root.path()).unwrap().into(),
        procs.into(),
        kill.into(),
    );
    assert!(
        matches!(outcome, Err(e) if matches!(e.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported))
    );
    let output = Command::new(env!("CARGO_BIN_EXE_hagency-cgroup-probe"))
        .arg("guardian-death")
        .arg(root.path())
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(78));
    assert!(
        output.stdout.is_empty(),
        "refusal must not claim qualification"
    );
    assert!(!root.path().join("custody.pulse").exists());
}
