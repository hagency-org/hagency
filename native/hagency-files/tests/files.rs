use cap_std::fs::Dir;
use hagency_files::{Error, Limits, RelativeFile, Workspace};
use std::fs;
fn workspace(path: &std::path::Path, limits: Limits) -> Workspace {
    // Ambient authority exists only in this host fixture, not in the library API.
    Workspace::from_directory(
        Dir::open_ambient_dir(path, cap_std::ambient_authority()).unwrap(),
        limits,
    )
    .unwrap()
}
fn select(path: &str) -> RelativeFile {
    RelativeFile::new(path).unwrap()
}
#[test]
fn native_file_snapshot_capability_nested_unicode_and_fixed_hash() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("中文")).unwrap();
    fs::write(root.path().join("中文/说明.txt"), b"abc").unwrap();
    let ws = workspace(root.path(), Limits::default());
    let snapshot = ws.snapshot(&select("中文/说明.txt")).unwrap();
    assert_eq!(snapshot.bytes(), b"abc");
    assert_eq!(snapshot.len(), 3);
    assert!(!snapshot.is_empty());
    assert_eq!(
        snapshot.digest(),
        &[
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ]
    );
    fs::write(root.path().join("中文/说明.txt"), b"other content").unwrap();
    assert_eq!(snapshot.bytes(), b"abc");
    assert_eq!(
        ws.snapshot(&select("中文/说明.txt")).unwrap().bytes(),
        b"other content"
    );
}
#[test]
fn native_file_snapshot_paths_portable_aliases_are_rejected_without_normalizing() {
    for bad in [
        "",
        "/etc/passwd",
        "../file",
        "a/../file",
        ".",
        "..",
        "./file",
        "a//file",
        "a/",
        "a/./file",
        "a\\file",
        "C:file",
        "C:/file",
        "//server/share/file",
        "\\\\?\\C:\\file",
        "file:secret",
        "a\0b",
        "a\nb",
        "file.",
        "file ",
        "NUL",
        "con.txt",
        "COM1.log",
        "LPT².txt",
        "CLOCK$",
        "CONIN$",
        "conout$.txt",
        "CON .txt",
        "AUX  .json",
        "a?",
        "a*",
        "a\"",
        "a<",
        "a>",
        "a|",
    ] {
        assert!(
            matches!(RelativeFile::new(bad), Err(Error::Selection)),
            "{bad:?}"
        );
    }
    assert!(RelativeFile::new(&"x".repeat(256)).is_err());
    assert!(RelativeFile::new(&vec!["x"; 33].join("/")).is_err());
    assert!(RelativeFile::new(&vec!["x".repeat(255); 17].join("/")).is_err());
    for good in [
        ".hidden",
        "模型 版本/file.txt",
        "a-b_c.d",
        "normal/com10.txt",
        "🦀.txt",
    ] {
        assert!(RelativeFile::new(good).is_ok());
    }
}
#[test]
fn native_file_snapshot_paths_hardlinks_directories_and_missing_files() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), b"outside canary").unwrap();
    fs::write(root.path().join("local"), b"local").unwrap();
    fs::hard_link(outside.path().join("secret"), root.path().join("hard")).unwrap();
    fs::create_dir(root.path().join("directory")).unwrap();
    let ws = workspace(root.path(), Limits::default());
    for name in ["hard", "directory", "missing"] {
        assert!(ws.snapshot(&select(name)).is_err());
    }
    assert_eq!(ws.snapshot(&select("local")).unwrap().bytes(), b"local");
    fs::hard_link(root.path().join("local"), root.path().join("second")).unwrap();
    assert!(matches!(ws.snapshot(&select("local")), Err(Error::Object)));
}
#[test]
fn native_file_snapshot_bounds_limits_and_release_after_failure_or_drop() {
    for limits in [(0, 1), (1, 0), (16 * 1024 * 1024 + 1, 1), (1, 9)] {
        assert!(Limits::new(limits.0, limits.1).is_err());
    }
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("small"), b"1234").unwrap();
    fs::write(root.path().join("large"), b"12345").unwrap();
    fs::write(root.path().join("empty"), b"").unwrap();
    let ws = workspace(root.path(), Limits::new(4, 1).unwrap());
    assert!(matches!(
        ws.snapshot(&select("large")),
        Err(Error::Capacity)
    ));
    assert!(ws.snapshot(&select("missing")).is_err());
    let snapshot = ws.snapshot(&select("small")).unwrap();
    assert!(matches!(
        ws.snapshot(&select("empty")),
        Err(Error::Capacity)
    ));
    drop(snapshot);
    assert!(ws.snapshot(&select("empty")).unwrap().is_empty());
}
#[cfg(unix)]
#[test]
fn native_file_snapshot_platform_unix_symlinks_and_fifo_are_refused_without_reading() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), b"not a workspace file").unwrap();
    symlink(outside.path(), root.path().join("redirect")).unwrap();
    symlink(outside.path().join("secret"), root.path().join("leaf")).unwrap();
    fixture_command(
        std::process::Command::new("/usr/bin/mkfifo")
            .arg("pipe")
            .current_dir(root.path()),
    );
    let ws = workspace(root.path(), Limits::default());
    for path in ["redirect/secret", "leaf", "pipe"] {
        assert!(ws.snapshot(&select(path)).is_err());
    }
}
#[cfg(windows)]
#[test]
fn native_file_snapshot_platform_windows_reparse_points_and_junction_are_refused() {
    use std::{
        os::windows::fs::{symlink_dir, symlink_file},
        process::Command,
    };
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("target")).unwrap();
    fs::write(root.path().join("target/secret"), b"reparse target").unwrap();
    // Real fixture prerequisites must succeed; a denied symlink is not passing proof.
    symlink_dir(root.path().join("target"), root.path().join("redirect")).unwrap();
    symlink_file(root.path().join("target/secret"), root.path().join("leaf")).unwrap();
    // Shell arguments are fixed ASCII; no path/user data is interpolated. CWD is
    // passed separately to the process API and permits Unicode temp ancestors.
    fixture_command(
        Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J", "junction", "target"])
            .current_dir(root.path()),
    );
    let ws = workspace(root.path(), Limits::default());
    for path in ["redirect/secret", "leaf", "junction/secret"] {
        assert!(ws.snapshot(&select(path)).is_err());
    }
}

fn fixture_command(command: &mut std::process::Command) {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("filesystem fixture timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
