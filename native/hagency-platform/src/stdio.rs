//! One-use IO ownership; these endpoints carry data, never process authority.
#[cfg(unix)]
use std::{io, os::fd::OwnedFd};

pub struct StdioPipes {
    #[cfg(unix)]
    stdin: OwnedFd,
    #[cfg(unix)]
    stdout: OwnedFd,
    #[cfg(unix)]
    stderr: OwnedFd,
    #[cfg(windows)]
    stdin: std::os::windows::io::OwnedHandle,
    #[cfg(windows)]
    stdout: std::os::windows::io::OwnedHandle,
    #[cfg(windows)]
    stderr: std::os::windows::io::OwnedHandle,
}
#[cfg(windows)]
impl StdioPipes {
    pub(crate) fn pair() -> std::io::Result<(Self, crate::windows::stdio::ChildPipes)> {
        let (stdin, child_stdin) = crate::windows::stdio::pair(true)?;
        let (stdout, child_stdout) = crate::windows::stdio::pair(false)?;
        let (stderr, child_stderr) = crate::windows::stdio::pair(false)?;
        Ok((
            Self {
                stdin,
                stdout,
                stderr,
            },
            crate::windows::stdio::ChildPipes {
                stdin: child_stdin,
                stdout: child_stdout,
                stderr: child_stderr,
            },
        ))
    }
    /// Consume the verified host endpoints once on the private completion reactor.
    pub fn into_async_parts(
        self,
    ) -> std::io::Result<(crate::WindowsPipe, crate::WindowsPipe, crate::WindowsPipe)> {
        Ok((
            crate::WindowsPipe::new(self.stdin, true)?,
            crate::WindowsPipe::new(self.stdout, false)?,
            crate::WindowsPipe::new(self.stderr, false)?,
        ))
    }
}
#[cfg(unix)]
pub(crate) struct ChildPipes {
    pub stdin: OwnedFd,
    pub stdout: OwnedFd,
    pub stderr: OwnedFd,
}
#[cfg(unix)]
impl StdioPipes {
    pub(crate) fn pair() -> io::Result<(Self, ChildPipes)> {
        let (child_stdin, stdin) = pipe()?;
        let (stdout, child_stdout) = pipe()?;
        let (stderr, child_stderr) = pipe()?;
        Ok((
            Self {
                stdin,
                stdout,
                stderr,
            },
            ChildPipes {
                stdin: child_stdin,
                stdout: child_stdout,
                stderr: child_stderr,
            },
        ))
    }
    /// Consume exactly once. Closing data pipes is not proof of child cleanup.
    pub fn into_parts(self) -> (OwnedFd, OwnedFd, OwnedFd) {
        (self.stdin, self.stdout, self.stderr)
    }
}
#[cfg(unix)]
pub(crate) fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    #[cfg(target_os = "linux")]
    {
        Ok(rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?)
    }
    #[cfg(target_os = "macos")]
    {
        let pair = rustix::pipe::pipe()?;
        for fd in [&pair.0, &pair.1] {
            rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::CLOEXEC)?;
        }
        // macOS lacks pipe2. Complete sealing before our spawn; every owned
        // launch also seals its complete post-fork descriptor table.
        Ok(pair)
    }
}
