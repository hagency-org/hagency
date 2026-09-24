use hagency_platform::{OwnedChildIdentity, SignalOutcome};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    child: Child,
    marker: PathBuf,
}
impl Fixture {
    fn spawn(root: &Path, name: &str, mode: &str) -> Self {
        let marker = root.join(name);
        let mut command = Command::new(env!("CARGO_BIN_EXE_hagency-platform-probe"));
        command
            .arg(mode)
            .arg(&marker)
            .current_dir(root)
            .env_clear()
            .env("PATH", "")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(value) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", value);
        }
        let fixture = Self {
            child: command.spawn().unwrap(),
            marker,
        };
        let until = Instant::now() + Duration::from_secs(5);
        while fixture.pulse() < 3 {
            assert!(
                Instant::now() < until,
                "child identity fixture did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fixture
    }
    fn pulse(&self) -> u64 {
        fs::metadata(self.marker.with_extension("pulse")).map_or(0, |v| v.len())
    }
    fn progress(&mut self) -> io::Result<()> {
        let prior = self.pulse();
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Err(io::Error::other(format!("observed child exit: {status}")));
            }
            if self.pulse() > prior {
                if let Some(status) = self.child.try_wait()? {
                    return Err(io::Error::other(format!("observed child exit: {status}")));
                }
                return Ok(());
            }
            if Instant::now() >= until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "child remained unexited but no new heartbeat was observed",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn running(&mut self) {
        self.progress()
            .expect("unrelated child must remain alive and make progress");
    }
    fn exited(&mut self) {
        let until = Instant::now() + Duration::from_secs(3);
        while self.child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < until, "target child did not stop");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn native_child_identity_signal() {
    let root = tempfile::tempdir().unwrap();
    let mut target = Fixture::spawn(root.path(), "target", "leaf");
    let mut other = Fixture::spawn(root.path(), "other", "leaf");
    let owned = OwnedChildIdentity::capture(&target.child).unwrap();
    assert_eq!(owned.identity().pid, target.child.id());
    assert!(owned.is_current().unwrap());
    assert_eq!(
        owned.terminate(owned.identity()).unwrap(),
        SignalOutcome::Sent
    );
    target.exited();
    other.running();
    assert!(!owned.is_current().unwrap());
}
#[test]
fn native_child_identity_expiry() {
    let root = tempfile::tempdir().unwrap();
    let mut target = Fixture::spawn(root.path(), "old", "leaf");
    let owned = OwnedChildIdentity::capture(&target.child).unwrap();
    target.child.kill().unwrap();
    target.exited(); // Explicitly reap while retaining native signal authority.
    let mut replacement = Fixture::spawn(root.path(), "replacement", "leaf");
    assert!(!owned.is_current().unwrap());
    assert_eq!(
        owned.terminate(owned.identity()).unwrap(),
        SignalOutcome::NoLongerCurrent
    );
    replacement.running();
    let replacement_identity = OwnedChildIdentity::capture(&replacement.child).unwrap();
    assert!(
        matches!(owned.terminate(replacement_identity.identity()),Err(e) if e.kind()==io::ErrorKind::PermissionDenied)
    );
    replacement.running();
}

#[test]
fn native_child_identity_observation() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Fixture::spawn(root.path(), "delayed", "pausable-leaf");
    fs::write(child.marker.with_extension("pause"), b"pause").unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while !child.marker.with_extension("paused").exists() {
        assert!(Instant::now() < until, "fixture did not acknowledge pause");
        assert!(child.child.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    // Reproduce the old false diagnosis without sending any signal: its single
    // 80 ms heartbeat sample is unchanged although the retained child is alive.
    let prior = child.pulse();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(child.pulse(), prior);
    assert!(child.child.try_wait().unwrap().is_none());
    let gate = child.marker.with_extension("pause");
    let resume = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(180));
        fs::remove_file(gate).unwrap();
    });
    child.running();
    resume.join().unwrap();
    // The stronger observation still rejects a real exit, even if a heartbeat
    // from before termination remains in the marker file.
    child.child.kill().unwrap();
    child.exited();
    let failure = child.progress().unwrap_err();
    assert!(failure.to_string().contains("observed child exit"));
}
#[test]
fn native_child_identity_generation() {
    let root = tempfile::tempdir().unwrap();
    let mut target = Fixture::spawn(
        root.path(),
        "generation",
        if cfg!(unix) {
            "exec-on-command"
        } else {
            "leaf"
        },
    );
    let owned = OwnedChildIdentity::capture(&target.child).unwrap();
    let original = owned.identity();
    let mut wrong = original;
    wrong.birth = wrong.birth.wrapping_add(1);
    assert!(matches!(owned.terminate(wrong),Err(e) if e.kind()==io::ErrorKind::PermissionDenied));
    target.running();
    #[cfg(unix)]
    {
        fs::write(target.marker.with_extension("exec"), b"exec").unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while !target.marker.with_extension("entered").exists() {
            assert!(
                Instant::now() < until,
                "exec fixture did not replace itself"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let fresh = OwnedChildIdentity::capture(&target.child).unwrap();
        assert_eq!(fresh.identity(), original); // Lifetime survives exec; macOS audit version may change.
    }
    assert!(owned.is_current().unwrap());
    assert_eq!(owned.terminate(original).unwrap(), SignalOutcome::Sent);
    target.exited();
}
