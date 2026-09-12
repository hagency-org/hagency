use super::SignalOutcome;
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal},
};
use std::{
    fs::File,
    io::{self, Read},
    os::fd::OwnedFd,
    process::Child,
};

pub(super) struct Handle {
    fd: OwnedFd,
}
impl Handle {
    pub(super) fn capture(child: &Child) -> io::Result<(Self, u64)> {
        let pid = i32::try_from(child.id())
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid child PID"))?;
        // The caller retains its unreaped Child, so this PID cannot have been
        // replaced while its pidfd and birth metadata are collected.
        let fd = pidfd_open(pid, PidfdFlags::empty())?;
        let mut bytes = Vec::new();
        File::open(format!("/proc/{}/stat", child.id()))?
            .take(4097)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "process metadata exceeds bound",
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid process metadata"))?;
        let (_, tail) = text
            .rsplit_once(')')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid process record"))?;
        let birth = tail
            .split_whitespace()
            .nth(19)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing process birth identity")
            })?;
        Ok((Self { fd }, birth))
    }
    pub(super) fn is_current(&self) -> io::Result<bool> {
        let mut fds = [PollFd::new(&self.fd, PollFlags::IN)];
        poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )?;
        let flags = fds[0].revents();
        if flags.intersects(PollFlags::ERR | PollFlags::NVAL) {
            return Err(io::Error::other("native process descriptor is invalid"));
        }
        Ok(flags.is_empty())
    }
    pub(super) fn terminate(&self) -> io::Result<SignalOutcome> {
        if !self.is_current()? {
            return Ok(SignalOutcome::NoLongerCurrent);
        }
        match pidfd_send_signal(&self.fd, Signal::KILL) {
            Ok(()) => Ok(SignalOutcome::Sent),
            Err(rustix::io::Errno::SRCH) => Ok(SignalOutcome::NoLongerCurrent),
            Err(error) => Err(error.into()),
        }
    }
}
