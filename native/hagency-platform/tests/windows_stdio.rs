#![cfg(windows)]
#![allow(unsafe_code)]
//! Real Windows fixtures; cross-compiling this file is not execution evidence.
use hagency_platform::{Launch, SupervisedProcess};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use windows_sys::Win32::{
    Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_TIMEOUT},
    System::{
        JobObjects::IsProcessInJob,
        Threading::{CreateEventW, GetCurrentProcess, SetEvent, WaitForSingleObject},
    },
};

#[tokio::test]
async fn native_windows_stdio_exact_handles_and_job_before_input() {
    if let Some(marker) = std::env::var_os("HAGENCY_STDIO_CHILD_MARKER") {
        let value = std::env::var("HAGENCY_STDIO_SENTINEL")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        // SAFETY: Adversarial test only: Windows validates an unrelated raw
        // handle value. It might have been reused locally, so the decisive
        // assertion is that the parent's retained event remains unsignalled.
        unsafe {
            SetEvent(value as _);
        }
        let mut joined = 0;
        // SAFETY: Current process pseudo-handle and valid scalar output.
        assert_ne!(
            unsafe { IsProcessInJob(GetCurrentProcess(), ptr::null_mut(), &mut joined) },
            0
        );
        assert_ne!(joined, 0);
        fs::write(marker, "job-before-input").unwrap();
        let mut input = [0u8; 5];
        std::io::stdin().read_exact(&mut input).unwrap();
        assert_eq!(&input, b"hello");
        std::io::stdout().write_all(b"owned-stdout").unwrap();
        std::io::stderr().write_all(b"owned-stderr").unwrap();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("job");
    // SAFETY: Non-inheritable unnamed manual-reset event, immediately owned.
    let sentinel = unsafe {
        let raw = CreateEventW(ptr::null(), 1, 0, ptr::null());
        assert!(!raw.is_null());
        OwnedHandle::from_raw_handle(raw)
    };
    // SAFETY: The event is deliberately inheritable to catch a broad handle leak.
    assert_ne!(
        unsafe {
            SetHandleInformation(
                sentinel.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        },
        0
    );
    let mut environment = BTreeMap::new();
    environment.insert(
        "HAGENCY_STDIO_CHILD_MARKER".into(),
        marker.as_os_str().into(),
    );
    environment.insert(
        "HAGENCY_STDIO_SENTINEL".into(),
        (sentinel.as_raw_handle() as usize).to_string().into(),
    );
    environment.insert("PATH".into(), "".into());
    if let Some(value) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), value);
    }
    let binary = std::env::current_exe().unwrap();
    let (mut owner, pipes) = SupervisedProcess::spawn_piped(
        &binary,
        &Launch {
            executable: binary.clone(),
            directory: root.path().into(),
            environment,
            arguments: vec![
                "--exact".into(),
                "native_windows_stdio_exact_handles_and_job_before_input".into(),
                "--nocapture".into(),
            ],
            require_crash_containment: true,
        },
    )
    .unwrap();
    let (mut stdin, mut stdout, mut stderr) = pipes.into_async_parts().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        stdin.write_all(b"hello").await.unwrap();
        stdin.flush().await.unwrap();
        let mut out = Vec::new();
        let mut err = Vec::new();
        stdout.read_to_end(&mut out).await.unwrap();
        stderr.read_to_end(&mut err).await.unwrap();
        assert!(String::from_utf8(out).unwrap().contains("owned-stdout"));
        assert_eq!(err, b"owned-stderr");
    })
    .await
    .unwrap();
    assert_eq!(fs::read_to_string(marker).unwrap(), "job-before-input");
    // SAFETY: Retained event; zero wait is a nonblocking observation only.
    assert_eq!(
        unsafe { WaitForSingleObject(sentinel.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    assert!(
        owner
            .stop(Duration::from_secs(3))
            .unwrap()
            .scope
            .whole_tree_stopped
    );
}
