use super::*;
use std::{sync::mpsc, thread, time::Duration};
use windows_sys::Win32::{
    Foundation::GENERIC_READ,
    Security::{
        Authorization::{SE_FILE_OBJECT, SetSecurityInfo},
        DACL_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{DELETE, WRITE_DAC},
};

pub(super) struct Gate {
    ready: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}
impl Gate {
    pub(super) fn wait(self) {
        self.ready.send(()).unwrap();
        self.release.recv().unwrap();
    }
}

fn directory(root: &std::path::Path, name: &str) -> Dir {
    private::directory(&root.join(name)).unwrap();
    Dir::open_ambient_dir(root.join(name), cap_std::ambient_authority()).unwrap()
}
fn deletion_open(parent: &Dir) -> std::io::Result<cap_std::fs::File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No)
        .access_mode(GENERIC_READ | DELETE);
    parent.open_with("original", &options)
}

#[test]
fn native_windows_directory_completion_custody() {
    let root = tempfile::tempdir().unwrap();
    let original = directory(root.path(), "original");
    let parent = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (response_tx, response_rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let (file, frozen) = candidate(&original).unwrap().unwrap();
        drop(original); // Only the actual candidate now prevents rooted deletion.
        let file = query(
            file,
            Some(Gate {
                ready: ready_tx,
                release: release_rx,
            }),
        )
        .expect("actual native NTFS profile required on this Windows fixture");
        let mut owner = WindowsDirectorySync {
            file,
            identity: frozen,
        };
        owner.sync().unwrap();
        // Caller disappearance cannot transfer or recreate the actual owner.
        assert!(response_tx.send(()).is_err());
        drop(owner);
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(response_rx);
    assert_eq!(deletion_open(&parent).unwrap_err().raw_os_error(), Some(32));
    assert!(!worker.is_finished());
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    // The original completion and worker exit, not receiver drop, permits this.
    let reopened = deletion_open(&parent).unwrap().into_std();
    identity(&reopened).unwrap();
}

#[test]
fn native_windows_directory_rejects_identity_and_flush() {
    let root = tempfile::tempdir().unwrap();
    let original = directory(root.path(), "original");
    let different = directory(root.path(), "different");
    let original_file = original.try_clone().unwrap().into_std_file();
    let different_file = different.try_clone().unwrap().into_std_file();
    let frozen = identity(&original_file).unwrap();
    assert!(frozen != identity(&different_file).unwrap());
    let mut wrong = WindowsDirectorySync {
        file: different_file,
        identity: frozen,
    };
    assert!(matches!(wrong.sync(), Err(Error::Private)));
    let mut readonly = WindowsDirectorySync {
        file: original_file,
        identity: frozen,
    };
    // Actual ordinary cap-std read-only duplicate fails FlushFileBuffers; no
    // successful file sync or matching identity is substituted for this ACK.
    assert!(readonly.sync().is_err());
    drop(readonly);
    let mut options = OpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No)
        .access_mode(GENERIC_READ | WRITE_DAC);
    let file = original.open_with(".", &options).unwrap().into_std();
    // SAFETY: This fresh disposable fixture directory is our own retained
    // object. Null DACL is deliberately nonprivate, no secret bytes are present,
    // and SetSecurityInfo synchronously consumes the exact scalar/null inputs.
    assert_eq!(
        unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        0
    );
    assert!(matches!(
        WindowsDirectorySync::open(&original),
        Err(Error::Private)
    ));
}

#[test]
fn native_windows_directory_production_profile_flags() {
    assert!(supported_profile(7, 0x20));
    assert!(supported_profile(7, 0x20020));
    for device in [0, 1, 6, 8, u32::MAX] {
        assert!(!supported_profile(device, 0x20));
    }
    for flags in [0, 0x20000, u32::MAX] {
        assert!(!supported_profile(7, flags));
    }
    for bit in 0..32 {
        if bit != 5 && bit != 17 {
            assert!(!supported_profile(7, 0x20 | (1 << bit)));
        }
    }
}
