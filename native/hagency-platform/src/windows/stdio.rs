//! Local self-connected named pipes: child endpoints are synchronous, host
//! endpoints overlapped. Raw handles and IO cancellation stay in this boundary.
use std::{
    ffi::c_void,
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    pin::Pin,
    ptr,
    sync::OnceLock,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::windows::named_pipe::NamedPipeServer,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{Authorization::*, Cryptography::*, *},
    Storage::FileSystem::*,
    System::{
        IO::CancelIoEx,
        Pipes::*,
        Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken},
    },
};

pub(crate) struct ChildPipes {
    pub stdin: OwnedHandle,
    pub stdout: OwnedHandle,
    pub stderr: OwnedHandle,
}

fn reactor() -> io::Result<&'static tokio::runtime::Runtime> {
    // Mio's completion buffers may outlive the public pipe after cancellation.
    // A caller's short-lived runtime must not abandon those late IOCP packets.
    // Exactly one private reactor lives until process exit; it exposes no task
    // spawning handle, command interface or process ownership capability.
    static REACTOR: OnceLock<Result<tokio::runtime::Runtime, io::ErrorKind>> = OnceLock::new();
    match REACTOR.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .thread_name("hagency-windows-io")
            .enable_io()
            .build()
            .map_err(|error| error.kind())
    }) {
        Ok(runtime) => Ok(runtime),
        Err(kind) => Err(io::Error::new(
            *kind,
            "private pipe completion reactor unavailable",
        )),
    }
}
struct Allocation(*mut c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: Exactly one LocalAlloc-backed Win32 result belongs to this guard.
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn owner_descriptor() -> io::Result<Allocation> {
    // SAFETY: Native outputs use initialized aligned owned storage. Token SID
    // and converted strings are read only while their allocating owners live.
    unsafe {
        let mut raw = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(raw);
        let mut needed = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut needed,
        );
        if needed == 0 || needed > 65536 {
            return Err(crate::invalid());
        }
        let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let _text = Allocation(text.cast());
        let mut len = 0;
        while len < 256 && *text.add(len) != 0 {
            len += 1;
        }
        if len == 256 {
            return Err(crate::invalid());
        }
        let sid = String::from_utf16(std::slice::from_raw_parts(text, len))
            .map_err(|_| crate::invalid())?;
        let descriptor: Vec<u16> = format!("O:{sid}D:P(A;;GA;;;{sid})\0")
            .encode_utf16()
            .collect();
        let mut raw = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut raw,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Allocation(raw))
    }
}

pub(crate) fn pair(host_writes: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let descriptor = owner_descriptor()?;
    let mut random = [0u8; 16];
    // SAFETY: Fixed output array and the system RNG require no algorithm handle.
    if unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            random.as_mut_ptr(),
            random.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    } < 0
    {
        return Err(io::Error::other("native pipe name entropy unavailable"));
    }
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let name: Vec<u16> = format!("\\\\.\\pipe\\hagency-stdio-{suffix}\0")
        .encode_utf16()
        .collect();
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let direction = if host_writes {
        PIPE_ACCESS_OUTBOUND
    } else {
        PIPE_ACCESS_INBOUND
    };
    // SAFETY: Unique local name, owner-only descriptor, one instance, byte mode,
    // bounded kernel buffer request. No endpoint is initially inheritable.
    let raw = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            direction | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            &attributes,
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The successful creation returns one owned handle.
    let host = unsafe { OwnedHandle::from_raw_handle(raw) };
    // CreateFile connects to the already-created server immediately. No wait or
    // reconnect loop and no child or external address participates in admission.
    // SAFETY: NUL terminated private name and matching synchronous client access.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            if host_writes {
                GENERIC_READ
            } else {
                GENERIC_WRITE
            },
            0,
            &attributes,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The successful open returns one owned handle.
    let child = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut client_pid = 0;
    let mut server_pid = 0;
    // SAFETY: Retained connected endpoints and bounded scalar outputs. This is
    // self-connection verification, never PID-derived process signal authority.
    if unsafe { GetNamedPipeClientProcessId(host.as_raw_handle(), &mut client_pid) } == 0
        || unsafe { GetNamedPipeServerProcessId(child.as_raw_handle(), &mut server_pid) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The current process identifier has no pointer preconditions.
    let this_pid = unsafe { GetCurrentProcessId() };
    if client_pid != this_pid || server_pid != this_pid {
        return Err(crate::invalid());
    }
    Ok((host, child))
}

/// Owned overlapped IO. The constructor is private: only checked, self-connected
/// StdioPipes can create this adapter. It does not expose reconnect or raw handles.
pub struct Pipe {
    inner: NamedPipeServer,
    writes: bool,
    confirming: Option<Vec<u8>>,
}
impl Pipe {
    pub(crate) fn new(handle: OwnedHandle, writes: bool) -> io::Result<Self> {
        let _reactor = reactor()?.enter();
        // SAFETY: Only pair() produces these owned FILE_FLAG_OVERLAPPED server
        // handles, already connected to their retained synchronous child ends.
        // Tokio takes ownership even if reactor registration fails. The private
        // process-lifetime reactor services late cancellation completions.
        let inner = unsafe { NamedPipeServer::from_raw_handle(handle.into_raw_handle()) }?;
        Ok(Self {
            inner,
            writes,
            confirming: None,
        })
    }
    fn attempt(&self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>> {
        match self.inner.poll_write_ready(cx) {
            Poll::Ready(Ok(())) => {}
            other => return other.map(|result| result.map(|()| 0)),
        }
        match self.inner.try_write(bytes) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                // try_write cleared stale readiness. Repoll once on the next
                // executor turn to register the new wakeup; never spin here.
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            result => Poll::Ready(result),
        }
    }
}
impl AsyncRead for Pipe {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.writes {
            return Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}
impl AsyncWrite for Pipe {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if !self.writes {
            return Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()));
        }
        if let Some(accepted) = &self.confirming {
            if !bytes.starts_with(accepted) {
                return Poll::Ready(Err(io::ErrorKind::InvalidInput.into()));
            }
            let count = accepted.len();
            // Pinned Mio refuses even an empty write while its preceding write
            // is pending and surfaces its completion error here. An accepted
            // empty write therefore proves completion of the preceding bytes,
            // without adding payload. Tokio's ordinary flush cannot prove this.
            return match self.attempt(cx, &[]) {
                Poll::Ready(Ok(0)) => {
                    self.confirming = None;
                    Poll::Ready(Ok(count))
                }
                Poll::Ready(Ok(_)) => Poll::Ready(Err(io::ErrorKind::InvalidData.into())),
                other => other,
            };
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        match self.attempt(cx, &bytes[..bytes.len().min(16 * 1024)]) {
            Poll::Ready(Ok(0)) => Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
            Poll::Ready(Ok(accepted)) => {
                self.confirming = Some(bytes[..accepted].to_vec());
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            other => other,
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Every reported write already completed. A cancelled pending poll_write
        // invalidates the entire outer Driver, which drops this stream.
        if self.confirming.is_some() {
            Poll::Ready(Err(io::ErrorKind::InvalidInput.into()))
        } else {
            Poll::Ready(Ok(()))
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: Retained private server handle. Disconnect first prevents a
        // raced Mio partial-write callback from transmitting another payload;
        // CancelIoEx requests cancellation of ALL outstanding operations, writes
        // included. Mio's IOCP-owned buffers remain alive until completion even
        // after this wrapper drops. No blocking wait or detached reader exists.
        unsafe {
            DisconnectNamedPipe(self.inner.as_raw_handle());
            CancelIoEx(self.inner.as_raw_handle(), ptr::null());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::{Read, Write},
        time::{Duration, Instant},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn native_windows_stdio_caller_runtime_teardown_drains_handles() {
        reactor().unwrap();
        fn count() -> u32 {
            let mut count = 0;
            // SAFETY: Current process and initialized bounded scalar output.
            assert_ne!(
                unsafe {
                    windows_sys::Win32::System::Threading::GetProcessHandleCount(
                        GetCurrentProcess(),
                        &mut count,
                    )
                },
                0
            );
            count
        }
        let baseline = count();
        for index in 0..32 {
            let caller = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let (host, child) = pair(false).unwrap();
            let mut pipe = caller.block_on(async { Pipe::new(host, false).unwrap() });
            drop(caller);
            let mut child = File::from(child);
            if index % 2 == 0 {
                // Cancel a pending read after its caller runtime already ended.
                // The independent IOCP worker must retire the completion storage.
                drop(pipe);
                assert!(child.write_all(b"disconnected").is_err());
            } else {
                child.write_all(b"ok").unwrap();
                drop(child);
                let next = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let mut bytes = Vec::new();
                next.block_on(async {
                    tokio::time::timeout(Duration::from_secs(2), pipe.read_to_end(&mut bytes))
                        .await
                        .unwrap()
                        .unwrap();
                });
                assert_eq!(bytes, b"ok");
                drop(pipe);
            }
        }
        // Bounded tolerance permits concurrent unit-test/runtime handles, while
        // a leaked handle per cancelled caller would exceed it. Actual late
        // completions must drain; a fixed sleep is not treated as their proof.
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let current = count();
            if current <= baseline + 12 {
                break;
            }
            assert!(
                Instant::now() < until,
                "pipe handles grew from {baseline} to {current}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[tokio::test]
    async fn native_windows_stdio_pending_write_is_not_completion_and_drop_disconnects() {
        let (host, child) = pair(true).unwrap();
        let mut pipe = Pipe::new(host, true).unwrap();
        let mut child = File::from(child);
        // No reader: even ONE 16 KiB submission exceeds the 4 KiB pipe quota.
        // Test write, not write_all: an early buffered-success claim for its
        // first chunk must fail this fixture rather than hide behind chunk two.
        let bytes = vec![b'x'; 64 * 1024];
        assert!(
            tokio::time::timeout(Duration::from_millis(40), pipe.write(&bytes))
                .await
                .is_err()
        );
        let since = Instant::now();
        drop(pipe);
        assert!(since.elapsed() < Duration::from_secs(1));
        let result = child.read(&mut [0u8; 1]);
        assert!(matches!(result, Ok(0)) || result.is_err());
        // Give cancelled IOCP completions a reactor turn to release their owned
        // buffers. No completion here authorizes a task or process transition.
        tokio::task::yield_now().await;
    }
}
