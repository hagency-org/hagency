//! Exactly one SCM_RIGHTS transfer before Start on the private guardian socket.
//! Unlike a generic ancillary iterator, reject unknown control-message kinds.
use crate::stdio::ChildPipes;
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    fs::{FileType, OFlags, fcntl_getfl, fstat},
    io::{FdFlags, fcntl_setfd},
    net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg},
};
use std::{
    io::{self, IoSlice},
    mem::MaybeUninit,
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    time::{Duration, Instant},
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid guardian stdio transfer",
    )
}
fn ready(socket: &UnixStream, flags: PollFlags, until: Instant) -> io::Result<()> {
    loop {
        let remaining = until
            .checked_duration_since(Instant::now())
            .filter(|v| !v.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "guardian stdio transfer timed out")
            })?;
        let duration = remaining.min(Duration::from_millis(25));
        let timeout = Timespec {
            tv_sec: 0,
            tv_nsec: duration.subsec_nanos().into(),
        };
        match poll(&mut [PollFd::new(socket, flags)], Some(&timeout)) {
            Ok(0) | Err(rustix::io::Errno::INTR) => {}
            Ok(_) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
}
pub(super) fn send(socket: &UnixStream, pipes: ChildPipes, until: Instant) -> io::Result<()> {
    let fds = [
        pipes.stdin.as_fd(),
        pipes.stdout.as_fd(),
        pipes.stderr.as_fd(),
    ];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(3))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
        return Err(invalid());
    }
    #[cfg(target_os = "macos")]
    rustix::net::sockopt::set_socket_nosigpipe(socket, true)?;
    let flags = SendFlags::DONTWAIT;
    #[cfg(target_os = "linux")]
    let flags = flags | SendFlags::NOSIGNAL;
    loop {
        ready(socket, PollFlags::OUT, until)?;
        match sendmsg(socket, &[IoSlice::new(b"I")], &mut control, flags) {
            Ok(1) => return Ok(()),
            Ok(_) => return Err(invalid()),
            Err(rustix::io::Errno::INTR | rustix::io::Errno::AGAIN) => {}
            Err(error) => return Err(error.into()),
        }
    }
    // The consumed sender endpoints close on every path. SCM_RIGHTS duplicates
    // only these three endpoints; the guardian owns their received copies.
}

pub(super) fn receive(socket: &UnixStream, until: Instant) -> io::Result<ChildPipes> {
    loop {
        ready(socket, PollFlags::IN, until)?;
        match receive_once(socket) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            result => return result,
        }
    }
}

fn receive_once(socket: &UnixStream) -> io::Result<ChildPipes> {
    receive_bounded(socket, 4096)
}

fn receive_bounded(socket: &UnixStream, capacity: usize) -> io::Result<ChildPipes> {
    let mut marker = [0u8; 1];
    // Aligned initialized fixed storage. Even a malicious sender cannot make
    // more than this finite number of descriptors reach the parser.
    // Cover the complete accepted native control-message bound, not merely
    // space for our three expected descriptors. XNU externalizes descriptors
    // before control copyout; undersizing this buffer can hide allocated IDs.
    let mut control = [0usize; 512];
    let mut iovec = libc::iovec {
        iov_base: marker.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: Zero is a valid empty msghdr; every active pointer below references
    // initialized owned storage that remains alive throughout recvmsg/parsing.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iovec;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    if capacity == 0 || capacity > std::mem::size_of_val(&control) {
        return Err(invalid());
    }
    message.msg_controllen = capacity as _;
    let flags = libc::MSG_DONTWAIT;
    #[cfg(target_os = "linux")]
    let flags = flags | libc::MSG_CMSG_CLOEXEC;
    // SAFETY: Live private socket and bounded writable buffers with native ABI.
    let bytes = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut message, flags) };
    if bytes < 0 {
        return Err(io::Error::last_os_error());
    }
    // XNU externalizes ALL rights before copying control bytes to userspace.
    // Truncation can hide installed fd numbers. Only this disposable guardian
    // receives rights, and only BEFORE Start: process exit closes even unknown
    // descriptors. Never turn this into a helper callable by a daemon thread.
    #[cfg(target_os = "macos")]
    if message.msg_flags & libc::MSG_CTRUNC != 0 || message.msg_controllen as usize > capacity {
        std::process::exit(125);
    }
    let mut valid = bytes == 1
        && marker == *b"I"
        && message.msg_controllen as usize <= capacity
        && message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) == 0;
    // Preserve invalidity but also bound libc's header traversal itself. Merely
    // bounding payload reads would still let an unexpected ABI length produce
    // a header pointer beyond the supplied allocation on Linux.
    message.msg_controllen = (message.msg_controllen as usize).min(capacity) as _;
    let mut fds = Vec::with_capacity(3);
    let mut messages = 0;
    // SAFETY: The native kernel writes ancillary headers into aligned owned
    // storage; traversal is bounded to that storage, and each header/payload
    // length is checked before reads. All returned SCM_RIGHTS descriptors acquire
    // unique RAII ownership before a validation error can return; unexpected
    // kinds never skip cleanup. Truncated macOS output cannot reach this parser.
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&message);
        while !header.is_null() {
            let offset = header
                .cast::<u8>()
                .offset_from(control.as_ptr().cast::<u8>()) as usize;
            let available = (message.msg_controllen as usize)
                .min(capacity)
                .saturating_sub(offset);
            if available < std::mem::size_of::<libc::cmsghdr>() {
                valid = false;
                break;
            }
            messages += 1;
            let base = libc::CMSG_LEN(0) as usize;
            let advertised = (*header).cmsg_len as usize;
            let length = advertised.min(available);
            if advertised > available {
                #[cfg(target_os = "macos")]
                {
                    std::process::exit(125);
                }
                #[cfg(target_os = "linux")]
                {
                    valid = false;
                }
            }
            if length < base {
                valid = false;
                break;
            }
            let payload = length - base;
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                if !payload.is_multiple_of(std::mem::size_of::<libc::c_int>()) {
                    valid = false;
                }
                for index in 0..payload / std::mem::size_of::<libc::c_int>() {
                    let fd = libc::CMSG_DATA(header)
                        .cast::<libc::c_int>()
                        .add(index)
                        .read_unaligned();
                    // A received descriptor is created by the kernel, not a PID
                    // or caller-supplied raw handle. Refuse invalid ABI output.
                    if fd < 0 {
                        valid = false;
                    } else {
                        fds.push(OwnedFd::from_raw_fd(fd));
                    }
                }
            } else {
                valid = false;
            }
            if advertised > available {
                break;
            }
            header = libc::CMSG_NXTHDR(&message, header);
        }
    }
    // Linux receives atomically CLOEXEC. macOS has no such recvmsg flag: this
    // single-thread guardian seals every received fd before any child can spawn.
    for fd in &fds {
        fcntl_setfd(fd, FdFlags::CLOEXEC)?;
    }
    if !valid || messages != 1 || fds.len() != 3 {
        return Err(invalid());
    }
    for (index, fd) in fds.iter().enumerate() {
        if FileType::from_raw_mode(fstat(fd)?.st_mode) != FileType::Fifo {
            return Err(invalid());
        }
        let mode = fcntl_getfl(fd)? & OFlags::ACCMODE;
        if mode
            != if index == 0 {
                OFlags::RDONLY
            } else {
                OFlags::WRONLY
            }
        {
            return Err(invalid());
        }
    }
    let mut fds = fds.into_iter();
    Ok(ChildPipes {
        stdin: fds.next().ok_or_else(invalid)?,
        stdout: fds.next().ok_or_else(invalid)?,
        stderr: fds.next().ok_or_else(invalid)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::io::{fcntl_getfd, read, write};
    fn raw_send(socket: &UnixStream, fds: &[std::os::fd::BorrowedFd<'_>], marker: &[u8]) {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(64))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if !fds.is_empty() {
            assert!(control.push(SendAncillaryMessage::ScmRights(fds)));
        }
        assert_eq!(
            sendmsg(
                socket,
                &[IoSlice::new(marker)],
                &mut control,
                SendFlags::DONTWAIT
            )
            .unwrap(),
            marker.len()
        );
    }
    #[test]
    fn native_owned_stdio_transfer_exact_once_and_cloexec() {
        let (host, child) = crate::StdioPipes::pair().unwrap();
        let (owner, worker) = UnixStream::pair().unwrap();
        send(&owner, child, Instant::now() + Duration::from_secs(1)).unwrap();
        let received = receive(&worker, Instant::now() + Duration::from_secs(1)).unwrap();
        for fd in [&received.stdin, &received.stdout, &received.stderr] {
            assert!(fcntl_getfd(fd).unwrap().contains(FdFlags::CLOEXEC));
        }
        let (stdin, stdout, stderr) = host.into_parts();
        assert_eq!(write(&stdin, b"input").unwrap(), 5);
        let mut buffer = [0u8; 5];
        assert_eq!(read(&received.stdin, &mut buffer).unwrap(), 5);
        assert_eq!(&buffer, b"input");
        assert_eq!(write(&received.stdout, b"reply").unwrap(), 5);
        assert_eq!(read(&stdout, &mut buffer).unwrap(), 5);
        assert_eq!(&buffer, b"reply");
        assert_eq!(write(&received.stderr, b"error").unwrap(), 5);
        assert_eq!(read(&stderr, &mut buffer).unwrap(), 5);
        assert_eq!(&buffer, b"error");
    }
    #[test]
    fn native_owned_stdio_transfer_rejects_counts_truncation_and_closes_received_fds() {
        for count in [0, 1, 2, 4, 32, 64] {
            let (reader, writer) = crate::stdio::pipe().unwrap();
            let (owner, worker) = UnixStream::pair().unwrap();
            raw_send(&owner, &vec![writer.as_fd(); count], b"I");
            assert!(
                receive(&worker, Instant::now() + Duration::from_secs(1)).is_err(),
                "count {count}"
            );
            drop(writer);
            // Every delivered duplicate writer must have been closed on error;
            // otherwise this nonblocking read would remain WouldBlock, not EOF.
            rustix::fs::fcntl_setfl(&reader, OFlags::NONBLOCK).unwrap();
            let until = Instant::now() + Duration::from_secs(2);
            loop {
                // A concurrent test may briefly fork a CLOEXEC descriptor;
                // wait for that exec boundary, without accepting a leaked fd.
                match read(&reader, &mut [0u8; 1]) {
                    Ok(0) => break,
                    Err(rustix::io::Errno::AGAIN) if Instant::now() < until => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    result => panic!("leaked fd at count {count}: {result:?}"),
                }
            }
        }
    }
    #[test]
    fn native_owned_stdio_transfer_rejects_marker_type_mode_and_silence() {
        let (owner, worker) = UnixStream::pair().unwrap();
        let (_, child) = crate::StdioPipes::pair().unwrap();
        raw_send(
            &owner,
            &[
                child.stdin.as_fd(),
                child.stdout.as_fd(),
                child.stderr.as_fd(),
            ],
            b"X",
        );
        assert!(receive(&worker, Instant::now() + Duration::from_secs(1)).is_err());
        raw_send(
            &owner,
            &[
                child.stdout.as_fd(),
                child.stdin.as_fd(),
                child.stderr.as_fd(),
            ],
            b"I",
        );
        assert!(receive(&worker, Instant::now() + Duration::from_secs(1)).is_err());
        raw_send(&owner, &[owner.as_fd(), owner.as_fd(), owner.as_fd()], b"I");
        assert!(receive(&worker, Instant::now() + Duration::from_secs(1)).is_err());
        assert!(
            matches!(receive(&worker, Instant::now() + Duration::from_millis(20)), Err(e) if e.kind() == io::ErrorKind::TimedOut)
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn native_owned_stdio_transfer_rejects_unexpected_credentials() {
        let (owner, worker) = UnixStream::pair().unwrap();
        rustix::net::sockopt::set_socket_passcred(&worker, true).unwrap();
        let (reader, writer) = crate::stdio::pipe().unwrap();
        raw_send(
            &owner,
            &[writer.as_fd(), writer.as_fd(), writer.as_fd()],
            b"I",
        );
        assert!(receive(&worker, Instant::now() + Duration::from_secs(1)).is_err());
        drop(writer);
        rustix::fs::fcntl_setfl(&reader, OFlags::NONBLOCK).unwrap();
        assert_eq!(read(&reader, &mut [0u8; 1]).unwrap(), 0);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod truncated_receiver {
    use super::*;
    use std::process::{Child, Command, Stdio};
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[test]
    fn native_owned_stdio_transfer_fatal_truncation_closes_disposable_receiver() {
        if std::env::var_os("HAGENCY_STDIO_TRUNCATION_CHILD").is_some() {
            let socket =
                UnixStream::from(rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 3).unwrap());
            ready(
                &socket,
                PollFlags::IN,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
            let _ = receive_bounded(&socket, 64);
            panic!("truncated guardian admission must terminate the receiver");
        }
        let (owner, child_socket) = UnixStream::pair().unwrap();
        let (reader, writer) = crate::stdio::pipe().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .arg("--exact")
            .arg(
                concat!(
                    module_path!(),
                    "::native_owned_stdio_transfer_fatal_truncation_closes_disposable_receiver"
                )
                .strip_prefix("hagency_platform::")
                .unwrap(),
            )
            .env_clear()
            .env("PATH", "")
            .env("HAGENCY_STDIO_TRUNCATION_CHILD", "1")
            .stdin(Stdio::from(OwnedFd::from(child_socket)))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::unix_spawn::seal(&mut command);
        let mut child = ChildGuard(command.spawn().unwrap());
        let rights = [writer.as_fd(); 32];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(32))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
        assert_eq!(
            sendmsg(
                &owner,
                &[IoSlice::new(b"I")],
                &mut control,
                SendFlags::DONTWAIT
            )
            .unwrap(),
            1
        );
        let until = Instant::now() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < until, "disposable receiver did not exit");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.code(), Some(125));
        drop(writer);
        rustix::fs::fcntl_setfl(&reader, OFlags::NONBLOCK).unwrap();
        assert_eq!(
            rustix::io::read(&reader, &mut [0u8; 1]).unwrap(),
            0,
            "undisclosed writer descriptors survived guardian exit"
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod truncated_receiver {
    use super::*;
    #[test]
    fn native_owned_stdio_transfer_kernel_truncation_closes_excess_fds() {
        let (owner, worker) = UnixStream::pair().unwrap();
        let (reader, writer) = crate::stdio::pipe().unwrap();
        let rights = [writer.as_fd(); 32];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(32))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
        assert_eq!(
            sendmsg(
                &owner,
                &[IoSlice::new(b"I")],
                &mut control,
                SendFlags::DONTWAIT
            )
            .unwrap(),
            1
        );
        assert!(receive_bounded(&worker, 64).is_err());
        drop(writer);
        rustix::fs::fcntl_setfl(&reader, OFlags::NONBLOCK).unwrap();
        assert_eq!(rustix::io::read(&reader, &mut [0u8; 1]).unwrap(), 0);
    }
}
