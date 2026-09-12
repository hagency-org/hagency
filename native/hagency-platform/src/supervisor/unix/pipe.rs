//! Single-owner, length-prefixed socket IO with bounded partial-frame lifetime.
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

pub(super) const FRAME_LIMIT: usize = 512 * 1024;
pub(super) struct Pipe {
    pub(super) stream: UnixStream,
    header: [u8; 4],
    header_read: usize,
    body: Vec<u8>,
    body_read: usize,
    started: Option<Instant>,
}
impl Pipe {
    pub(super) fn new(stream: UnixStream) -> io::Result<Self> {
        // macOS rejects SO_RCVTIMEO after peer close even when a terminal report
        // is still buffered. Nonblocking IO plus poll drains those bytes without
        // mutating socket options after disconnect, and retains absolute bounds.
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            header: [0; 4],
            header_read: 0,
            body: Vec::new(),
            body_read: 0,
            started: None,
        })
    }
    pub(super) fn send<T: Serialize>(&mut self, value: &T, until: Instant) -> io::Result<()> {
        let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
        if bytes.is_empty() || bytes.len() > FRAME_LIMIT {
            return Err(invalid());
        }
        let header = (bytes.len() as u32).to_be_bytes();
        for mut remaining in [header.as_slice(), bytes.as_slice()] {
            while !remaining.is_empty() {
                let timeout = until
                    .checked_duration_since(Instant::now())
                    .filter(|v| !v.is_zero())
                    .ok_or_else(timed_out)?;
                if !ready(&self.stream, PollFlags::OUT, timeout)? {
                    continue;
                }
                match self.stream.write(remaining) {
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "guardian channel closed",
                        ));
                    }
                    Ok(count) => remaining = &remaining[count..],
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                        ) =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }
    /// A tick can expire without discarding partial bytes. A peer cannot keep a
    /// frame alive indefinitely by dribbling bytes; its absolute age is bounded.
    pub(super) fn receive<T: DeserializeOwned>(
        &mut self,
        until: Instant,
        limit: usize,
    ) -> io::Result<Option<T>> {
        loop {
            let now = Instant::now();
            if self
                .started
                .is_some_and(|start| now.duration_since(start) >= Duration::from_secs(1))
            {
                return Err(invalid());
            }
            let Some(timeout) = until.checked_duration_since(now).filter(|v| !v.is_zero()) else {
                return Ok(None);
            };
            if !ready(&self.stream, PollFlags::IN, timeout)? {
                continue;
            }
            let buffer = if self.header_read < 4 {
                &mut self.header[self.header_read..]
            } else {
                &mut self.body[self.body_read..]
            };
            match self.stream.read(buffer) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "guardian owner channel closed",
                    ));
                }
                Ok(count) => {
                    self.started.get_or_insert_with(Instant::now);
                    if self.header_read < 4 {
                        self.header_read += count;
                        if self.header_read == 4 {
                            let length = u32::from_be_bytes(self.header) as usize;
                            if length == 0 || length > limit.min(FRAME_LIMIT) {
                                return Err(invalid());
                            }
                            self.body.resize(length, 0);
                        }
                    } else {
                        self.body_read += count;
                        if self.body_read == self.body.len() {
                            let value =
                                serde_json::from_slice(&self.body).map_err(|_| invalid())?;
                            self.header_read = 0;
                            self.body_read = 0;
                            self.body.clear();
                            self.started = None;
                            return Ok(Some(value));
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }
    pub(super) fn required<T: DeserializeOwned>(
        &mut self,
        until: Instant,
        limit: usize,
    ) -> io::Result<T> {
        self.receive(until, limit)?.ok_or_else(timed_out)
    }
}
fn ready(stream: &UnixStream, flags: PollFlags, remaining: Duration) -> io::Result<bool> {
    let tick = remaining.min(Duration::from_millis(25));
    let timeout = Timespec {
        tv_sec: 0,
        tv_nsec: i64::from(tick.subsec_nanos()),
    };
    let mut fds = [PollFd::new(stream, flags)];
    match poll(&mut fds, Some(&timeout)) {
        Ok(count) => Ok(count > 0),
        Err(rustix::io::Errno::INTR) => Ok(false),
        Err(error) => Err(error.into()),
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid guardian frame")
}
fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "guardian channel timed out")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_guardian_early_exit_buffered_eof() {
        let (owner, mut peer) = UnixStream::pair().unwrap();
        let mut pipe = Pipe::new(owner).unwrap();
        let value = serde_json::json!({"kind":"stopped","leader_exited":true});
        let bytes = serde_json::to_vec(&value).unwrap();
        // A real guardian can send its terminal report and exit before the host
        // next runs. Both complete frames must be drained before reporting EOF.
        for _ in 0..2 {
            peer.write_all(&(bytes.len() as u32).to_be_bytes()).unwrap();
            peer.write_all(&bytes).unwrap();
        }
        drop(peer);
        for _ in 0..2 {
            assert_eq!(
                pipe.required::<serde_json::Value>(Instant::now() + Duration::from_secs(1), 1024)
                    .unwrap(),
                value
            );
        }
        assert_eq!(
            pipe.required::<serde_json::Value>(Instant::now() + Duration::from_secs(1), 1024)
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
