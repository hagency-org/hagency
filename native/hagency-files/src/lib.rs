//! Host-capability file snapshots. This synchronous primitive does not promise
//! cancellable disk syscalls, atomic source consistency or physical provisioning.
mod path;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
pub use path::RelativeFile;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid host snapshot limit")]
    Limit,
    #[error("invalid relative file selection")]
    Selection,
    #[error("snapshot capacity is exhausted")]
    Capacity,
    #[error("file selection is not a permitted regular object")]
    Object,
    #[error("source changed while copied")]
    Changed,
    #[error("file snapshot I/O failed")]
    Io,
    #[error("filesystem platform is unsupported")]
    Unsupported,
}

/// Host configuration, not runtime or browser JSON.
#[derive(Clone, Copy)]
pub struct Limits {
    max_bytes: usize,
    max_snapshots: usize,
}
impl Limits {
    pub fn new(max_bytes: usize, max_snapshots: usize) -> Result<Self, Error> {
        if max_bytes == 0 || max_bytes > 16 * 1024 * 1024 || max_snapshots == 0 || max_snapshots > 8
        {
            return Err(Error::Limit);
        }
        Ok(Self {
            max_bytes,
            max_snapshots,
        })
    }
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_snapshots: 4,
        }
    }
}
struct Root {
    dir: Dir,
    limits: Limits,
    live: AtomicUsize,
}
/// Opaque host-owned directory authority. No ambient path constructor, raw handle
/// accessor, Debug or serialization is exposed by this crate.
#[derive(Clone)]
pub struct Workspace {
    root: Arc<Root>,
}
struct Permit {
    root: Arc<Root>,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.root.live.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Custody {
    // Retain the actual source and every ancestor. Identity is object custody,
    // not a pathname or a portable/reusable 64-bit inode serialization.
    _file: File,
    _ancestors: Vec<Dir>,
    _permit: Permit,
}
/// Owned copied bytes are immutable through this API. The retained source may
/// still be modified elsewhere; that does not change these bytes or their hash.
pub struct Snapshot {
    bytes: Vec<u8>,
    digest: [u8; 32],
    _custody: Custody,
}
impl Snapshot {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}
impl Workspace {
    /// The host must already have provisioned and opened a private workspace.
    /// Supplying this capability is an explicit authority decision; this method
    /// neither resolves an ambient path nor verifies physical provisioning.
    pub fn from_directory(dir: Dir, limits: Limits) -> Result<Self, Error> {
        directory(&dir)?;
        Ok(Self {
            root: Arc::new(Root {
                dir,
                limits,
                live: AtomicUsize::new(0),
            }),
        })
    }
    pub fn snapshot(&self, selection: &RelativeFile) -> Result<Snapshot, Error> {
        self.copy(selection, |_| {})
    }
    fn copy(
        &self,
        selection: &RelativeFile,
        mut observe: impl FnMut(Stage),
    ) -> Result<Snapshot, Error> {
        self.root
            .live
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.root.limits.max_snapshots).then_some(n + 1)
            })
            .map_err(|_| Error::Capacity)?;
        let permit = Permit {
            root: self.root.clone(),
        };
        let mut ancestors = Vec::new();
        for component in &selection.components[..selection.components.len() - 1] {
            let parent = ancestors.last().unwrap_or(&self.root.dir);
            let child = parent
                .open_dir_nofollow(component)
                .map_err(|_| Error::Object)?;
            directory(&child)?;
            ancestors.push(child);
            observe(Stage::Ancestor);
        }
        let parent = ancestors.last().unwrap_or(&self.root.dir);
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let mut file = parent
            .open_with(
                selection.components.last().ok_or(Error::Selection)?,
                &options,
            )
            .map_err(|_| Error::Object)?;
        let before = regular(&file)?;
        if before.len() > self.root.limits.max_bytes as u64 {
            return Err(Error::Capacity);
        }
        observe(Stage::Opened);
        let capacity = self.root.limits.max_bytes + 1;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| Error::Capacity)?;
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let remaining = capacity - bytes.len();
            if remaining == 0 {
                return Err(Error::Capacity);
            }
            let take = remaining.min(chunk.len());
            let n = file.read(&mut chunk[..take]).map_err(|_| Error::Io)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() > self.root.limits.max_bytes {
                return Err(Error::Capacity);
            }
            observe(Stage::Chunk);
        }
        let after = regular(&file)?;
        if before.len() != after.len()
            || after.len() != bytes.len() as u64
            || before.modified().ok() != after.modified().ok()
        {
            return Err(Error::Changed);
        }
        let digest = Sha256::digest(&bytes).into();
        Ok(Snapshot {
            bytes,
            digest,
            _custody: Custody {
                _file: file,
                _ancestors: ancestors,
                _permit: permit,
            },
        })
    }
}
#[derive(Clone, Copy)]
enum Stage {
    Ancestor,
    Opened,
    Chunk,
}
fn redirected(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        // Reject every reparse type, not just Rust's recognized symbolic links.
        const REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(unix)]
    {
        metadata.is_symlink()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        true
    }
}
fn directory(dir: &Dir) -> Result<(), Error> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = dir;
        return Err(Error::Unsupported);
    }
    #[cfg(any(unix, windows))]
    {
        let metadata = dir.dir_metadata().map_err(|_| Error::Io)?;
        if !metadata.is_dir() || redirected(&metadata) {
            return Err(Error::Object);
        }
        Ok(())
    }
}
fn regular(file: &File) -> Result<Metadata, Error> {
    let metadata = file.metadata().map_err(|_| Error::Io)?;
    if !metadata.is_file() || redirected(&metadata) || metadata.nlink() != 1 {
        return Err(Error::Object);
    }
    Ok(metadata)
}

#[cfg(test)]
#[path = "../tests/custody/mod.rs"]
mod tests;
