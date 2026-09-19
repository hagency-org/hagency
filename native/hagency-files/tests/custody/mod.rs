use super::*;
use std::{
    fs,
    sync::{Barrier, mpsc},
    time::Duration,
};
fn workspace(path: &std::path::Path, limits: Limits) -> Workspace {
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
fn native_file_snapshot_custody_ancestor_and_leaf_replacement_keep_open_objects() {
    for replace in [Stage::Ancestor, Stage::Opened] {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("nested")).unwrap();
        fs::write(root.path().join("nested/file"), b"original").unwrap();
        let ws = workspace(root.path(), Limits::default());
        let mut changed = false;
        let snapshot = ws
            .copy(&select("nested/file"), |stage| {
                if !changed && std::mem::discriminant(&replace) == std::mem::discriminant(&stage) {
                    changed = true;
                    if matches!(replace, Stage::Ancestor) {
                        // cap-primitives deliberately omits FILE_SHARE_DELETE
                        // for directory handles on Windows. The adversarial
                        // rename must be rejected while this capability lives.
                        #[cfg(windows)]
                        {
                            let error =
                                fs::rename(root.path().join("nested"), root.path().join("old"))
                                    .unwrap_err();
                            assert_eq!(error.raw_os_error(), Some(32));
                        }
                        #[cfg(not(windows))]
                        {
                            fs::rename(root.path().join("nested"), root.path().join("old"))
                                .unwrap();
                            fs::create_dir(root.path().join("nested")).unwrap();
                            fs::write(root.path().join("nested/file"), b"replacement").unwrap();
                        }
                    } else {
                        fs::rename(root.path().join("nested/file"), root.path().join("old"))
                            .unwrap();
                        fs::write(root.path().join("nested/file"), b"replacement").unwrap();
                    }
                }
            })
            .unwrap();
        assert!(changed);
        assert_eq!(snapshot.bytes(), b"original");
        #[cfg(windows)]
        if matches!(replace, Stage::Ancestor) {
            assert_eq!(
                ws.snapshot(&select("nested/file")).unwrap().bytes(),
                b"original"
            );
            assert!(!root.path().join("old").exists());
            // Prove the denial belonged to retained custody: releasing this
            // snapshot makes the same rename succeed, with no permission edit.
            drop(snapshot);
            fs::rename(root.path().join("nested"), root.path().join("old")).unwrap();
            fs::create_dir(root.path().join("nested")).unwrap();
            fs::write(root.path().join("nested/file"), b"replacement").unwrap();
        }
        assert_eq!(
            ws.snapshot(&select("nested/file")).unwrap().bytes(),
            b"replacement"
        );
    }
}
#[test]
fn native_file_snapshot_custody_root_path_replacement_keeps_retained_capability() {
    let parent = tempfile::tempdir().unwrap();
    fs::create_dir(parent.path().join("workspace")).unwrap();
    fs::write(parent.path().join("workspace/file"), b"owned root").unwrap();
    let ws = workspace(&parent.path().join("workspace"), Limits::default());
    #[cfg(windows)]
    {
        let snapshot = ws.snapshot(&select("file")).unwrap();
        let rename = || {
            fs::rename(
                parent.path().join("workspace"),
                parent.path().join("retained"),
            )
        };
        assert_eq!(rename().unwrap_err().raw_os_error(), Some(32));
        assert_eq!(ws.snapshot(&select("file")).unwrap().bytes(), b"owned root");
        drop(ws);
        assert_eq!(rename().unwrap_err().raw_os_error(), Some(32));
        assert_eq!(snapshot.bytes(), b"owned root");
        drop(snapshot);
        rename().unwrap();
        fs::create_dir(parent.path().join("workspace")).unwrap();
        fs::write(parent.path().join("workspace/file"), b"another root").unwrap();
        assert_eq!(
            fs::read(parent.path().join("retained/file")).unwrap(),
            b"owned root"
        );
        let fresh = workspace(&parent.path().join("workspace"), Limits::default());
        assert_eq!(
            fresh.snapshot(&select("file")).unwrap().bytes(),
            b"another root"
        );
    }
    #[cfg(not(windows))]
    {
        fs::rename(
            parent.path().join("workspace"),
            parent.path().join("retained"),
        )
        .unwrap();
        fs::create_dir(parent.path().join("workspace")).unwrap();
        fs::write(parent.path().join("workspace/file"), b"another root").unwrap();
        assert_eq!(ws.snapshot(&select("file")).unwrap().bytes(), b"owned root");
    }
}
#[test]
fn native_file_snapshot_custody_observed_growth_and_new_hardlink_are_refused() {
    for link in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("file"), b"old").unwrap();
        let ws = workspace(root.path(), Limits::default());
        let mut changed = false;
        let result = ws.copy(&select("file"), |stage| {
            if matches!(stage, Stage::Opened) && !changed {
                changed = true;
                if link {
                    fs::hard_link(root.path().join("file"), root.path().join("newlink")).unwrap();
                } else {
                    fs::write(root.path().join("file"), b"source grew").unwrap();
                }
            }
        });
        assert!(matches!(result, Err(Error::Changed | Error::Object)));
    }
}
#[test]
fn native_file_snapshot_bounds_concurrent_reads_and_retained_snapshots_share_permits() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("file"), b"abc").unwrap();
    let ws = workspace(root.path(), Limits::new(16, 2).unwrap());
    let gate = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let mut jobs = vec![];
    for _ in 0..2 {
        let ws = ws.clone();
        let gate = gate.clone();
        let tx = tx.clone();
        jobs.push(std::thread::spawn(move || {
            ws.copy(&select("file"), |stage| {
                if matches!(stage, Stage::Opened) {
                    tx.send(()).unwrap();
                    gate.wait();
                }
            })
            .unwrap()
        }));
    }
    for _ in 0..2 {
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    assert!(matches!(ws.snapshot(&select("file")), Err(Error::Capacity)));
    gate.wait();
    let a = jobs.pop().unwrap().join().unwrap();
    let b = jobs.pop().unwrap().join().unwrap();
    assert!(matches!(ws.snapshot(&select("file")), Err(Error::Capacity)));
    drop(a);
    assert_eq!(ws.snapshot(&select("file")).unwrap().bytes(), b"abc");
    drop(b);
}
#[test]
fn native_file_snapshot_custody_copy_changes_mid_read_without_atomic_source_claim() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("file");
    fs::write(&source, vec![b'a'; 128 * 1024]).unwrap();
    let ws = workspace(root.path(), Limits::default());
    let mut changed = false;
    let result = ws.copy(&select("file"), |stage| {
        if matches!(stage, Stage::Chunk) && !changed {
            changed = true;
            fs::write(&source, vec![b'b'; 129 * 1024]).unwrap();
        }
    });
    assert!(changed);
    assert!(matches!(result, Err(Error::Changed)));
}

#[test]
fn native_file_snapshot_custody_same_size_write_and_restored_time_is_not_atomic_proof() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file");
    fs::write(&path, vec![b'a'; 128 * 1024]).unwrap();
    let original_time = fs::metadata(&path).unwrap().modified().unwrap();
    let ws = workspace(root.path(), Limits::default());
    let mut changed = false;
    let result = ws.copy(&select("file"), |stage| {
        if matches!(stage, Stage::Chunk) && !changed {
            changed = true;
            let mut writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
            writer.write_all(&vec![b'b'; 128 * 1024]).unwrap();
            writer
                .set_times(fs::FileTimes::new().set_modified(original_time))
                .unwrap();
        }
    });
    assert!(changed);
    match result {
        Ok(snapshot) => {
            // The first half was copied before the write, the second afterward.
            // Restored timestamps can hide same-size mutation. Only these copied
            // bytes and their digest are promised, not an atomic source version.
            assert_eq!(&snapshot.bytes()[..64 * 1024], vec![b'a'; 64 * 1024]);
            assert_eq!(&snapshot.bytes()[64 * 1024..], vec![b'b'; 64 * 1024]);
            assert_eq!(
                snapshot.digest().as_slice(),
                Sha256::digest(snapshot.bytes()).as_slice()
            );
        }
        Err(Error::Changed) => {} // A filesystem may still expose a detectable timestamp difference.
        Err(error) => panic!("unexpected bounded-copy outcome: {error}"),
    }
}
