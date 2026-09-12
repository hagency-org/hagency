//! Read-only birth metadata never constructs signal authority. The constructor
//! requires a Child actually owned by the host; there is no public PID constructor.
use std::{io, process::Child};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Handle;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos;
#[cfg(target_os = "macos")]
use macos::Handle;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;
#[cfg(windows)]
use windows::Handle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildIdentity {
    pub pid: u32,
    pub birth: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalOutcome {
    Sent,
    NoLongerCurrent,
}
pub struct OwnedChildIdentity {
    inner: Handle,
    identity: ChildIdentity,
}
impl OwnedChildIdentity {
    /// The host exclusively owns Child reaping. A runtime cannot manufacture a
    /// std::process::Child from an asserted PID or a JSON identity record.
    pub fn capture(child: &Child) -> io::Result<Self> {
        if child.id() <= 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid owned child",
            ));
        }
        let (inner, birth) = Handle::capture(child)?;
        Ok(Self {
            inner,
            identity: ChildIdentity {
                pid: child.id(),
                birth,
            },
        })
    }
    pub fn identity(&self) -> ChildIdentity {
        self.identity
    }
    pub fn is_current(&self) -> io::Result<bool> {
        self.inner.is_current()
    }
    /// The recorded dispatch identity narrows this handle's authority. It cannot
    /// retarget the handle, even if the supplied metadata names another live child.
    pub fn terminate(&self, expected: ChildIdentity) -> io::Result<SignalOutcome> {
        if expected != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "child identity does not match the owned handle",
            ));
        }
        self.inner.terminate()
    }
}
