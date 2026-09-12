//! One original file effect, retained by the application before the first poll.
//! Synchronous disk calls belong on the application's fixed owned sink worker.
use super::{Binding, StartedWorkspace, WorkspaceError, capability_digest};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use hagency_core::{received_files::ReceivedFileFacts, tasks::RunnerCapability};
use hagency_store::{ReceiveIdentity, ReceiveWrite, private};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    sync::Arc,
    time::Instant,
};

const CHUNK: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WorkspaceReceiveError {
    #[error("original workspace refused: {0}")]
    Workspace(#[from] WorkspaceError),
    #[error("received file facts or original write association differ")]
    Association,
    #[error("original receive attempt was already consumed")]
    Attempted,
    #[error("receive deadline expired")]
    Deadline,
    #[error("received destination is unavailable or changed")]
    Object,
    #[error("received file IO or required sync failed")]
    Io,
    #[error("received file has no completed original materialization")]
    Incomplete,
}

/// Non-Clone custody for one committed write grant. No handle or root escapes.
/// An error after creation retains the same actual File in this owner. The host
/// must retain this value on unwind and observe its fixed worker before release.
pub struct WorkspaceReceive {
    binding: Arc<Binding>,
    cap: RunnerCapability,
    write: ReceiveWrite,
    path: String,
    deadline: Instant,
    attempted: bool,
    complete: bool,
    directory: Option<Dir>,
    file: Option<File>,
    #[cfg(unix)]
    directory_sync: Option<File>,
    #[cfg(windows)]
    directory_sync: Option<private::WindowsDirectorySync>,
}

impl StartedWorkspace {
    /// No filesystem operation or await occurs here. Only the unique write grant
    /// is consumed; the same original private binding and writer remain retained.
    pub fn prepare_receive(
        &self,
        cap: &RunnerCapability,
        write: ReceiveWrite,
        deadline: Instant,
    ) -> Result<WorkspaceReceive, WorkspaceReceiveError> {
        self.binding.live()?;
        if capability_digest(cap)? != self.binding.capability
            || !write.matches_capability(cap)
            || write.scope_fingerprint() != self.binding.fingerprint
        {
            return Err(WorkspaceReceiveError::Association);
        }
        write
            .facts()
            .validate(write.limit())
            .map_err(|_| WorkspaceReceiveError::Association)?;
        if write.limit() > self.binding.root.limit {
            return Err(WorkspaceError::Limit.into());
        }
        let suffix = write
            .identity()
            .id()
            .strip_prefix("receive_")
            .filter(|value| {
                value.len() == 32
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .ok_or(WorkspaceReceiveError::Association)?;
        let path = format!(".hagency-received-{suffix}.bin");
        Ok(WorkspaceReceive {
            binding: self.binding.clone(),
            cap: cap.clone(),
            write,
            path,
            deadline,
            attempted: false,
            complete: false,
            directory: None,
            file: None,
            #[cfg(unix)]
            directory_sync: None,
            #[cfg(windows)]
            directory_sync: None,
        })
    }
}

impl WorkspaceReceive {
    /// Association data only: this path is not evidence of successful IO or Ready.
    pub fn relative_path(&self) -> &str {
        &self.path
    }
    pub fn facts(&self) -> &ReceivedFileFacts {
        self.write.facts()
    }
    pub fn identity(&self) -> &ReceiveIdentity {
        self.write.identity()
    }

    /// Set attempted before the first await, including pre-effect refusals.
    /// Dropping this borrowed future cannot reset the attempt or drop its File.
    pub async fn materialize(&mut self, bytes: &[u8]) -> Result<(), WorkspaceReceiveError> {
        if self.attempted {
            return Err(WorkspaceReceiveError::Attempted);
        }
        self.attempted = true;
        self.check_bytes(bytes)?;
        self.current(self.deadline).await?;
        self.create_and_write(bytes)?;
        self.current(self.deadline).await?;
        self.readback(self.deadline)?;
        self.current(self.deadline).await?;
        self.complete = true;
        Ok(())
    }

    /// Fresh read-only response deadline; the original write deadline is unchanged.
    /// Historical metadata and equal bytes in a replacement file cannot suffice.
    pub async fn revalidate(&mut self, deadline: Instant) -> Result<(), WorkspaceReceiveError> {
        if !self.complete {
            return Err(WorkspaceReceiveError::Incomplete);
        }
        self.current(deadline).await?;
        self.readback(deadline)?;
        self.current(deadline).await
    }

    fn local(&self, deadline: Instant) -> Result<(), WorkspaceReceiveError> {
        self.binding.live()?;
        if Instant::now() >= deadline {
            return Err(WorkspaceReceiveError::Deadline);
        }
        Ok(())
    }
    async fn current(&self, deadline: Instant) -> Result<(), WorkspaceReceiveError> {
        self.local(deadline)?;
        self.binding.check(&self.cap)?;
        tokio::time::timeout_at(
            deadline.into(),
            self.binding
                .domain
                .check_owned_dispatch(self.cap.clone(), self.binding.fingerprint.clone()),
        )
        .await
        .map_err(|_| WorkspaceReceiveError::Deadline)?
        .map_err(|_| WorkspaceError::Current)?;
        self.local(deadline)?;
        self.binding.check(&self.cap)?;
        Ok(())
    }
    fn check_bytes(&self, bytes: &[u8]) -> Result<(), WorkspaceReceiveError> {
        if bytes.len() as u64 != self.facts().size || bytes.len() > self.write.limit() {
            return Err(WorkspaceReceiveError::Association);
        }
        let mut digest = Sha256::new();
        for chunk in bytes.chunks(CHUNK) {
            self.local(self.deadline)?;
            digest.update(chunk);
        }
        if format!("{:x}", digest.finalize()) != self.facts().sha256 {
            return Err(WorkspaceReceiveError::Association);
        }
        self.local(self.deadline)
    }
    fn create_and_write(&mut self, bytes: &[u8]) -> Result<(), WorkspaceReceiveError> {
        self.local(self.deadline)?;
        self.binding.check(&self.cap)?;
        // Duplicate only the actual retained root, never a destination pathname.
        self.directory = Some(Dir::from_std_file(
            self.binding
                .root
                .file
                .try_clone()
                .map_err(|_| WorkspaceReceiveError::Io)?,
        ));
        #[cfg(unix)]
        {
            use cap_fs_ext::OpenOptionsMaybeDirExt;
            // cap-std retains O_PATH directory descriptors on Linux. Duplicating
            // one preserves lookup authority but does not make fsync legal.
            // Open only "." relative to that original object with read access,
            // and retain the identity-checked sync handle before creating a file.
            let mut options = OpenOptions::new();
            options
                .read(true)
                .follow(FollowSymlinks::No)
                .maybe_dir(true)
                .nonblock(true);
            let sync = self
                .directory()?
                .open_with(".", &options)
                .map_err(|_| WorkspaceReceiveError::Io)?
                .into_std();
            private::check_handle(&sync).map_err(|_| WorkspaceReceiveError::Object)?;
            if !hagency_platform::same_directory(&self.binding.root.file, &sync)
                .map_err(|_| WorkspaceReceiveError::Object)?
            {
                return Err(WorkspaceReceiveError::Object);
            }
            self.directory_sync = Some(sync);
        }
        #[cfg(windows)]
        {
            self.directory_sync = Some(
                private::WindowsDirectorySync::open(self.directory()?)
                    .map_err(|_| WorkspaceReceiveError::Io)?
                    .ok_or(WorkspaceReceiveError::Io)?,
            );
        }
        self.local(self.deadline)?;
        self.binding.check(&self.cap)?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(windows)]
        {
            use cap_std::fs::OpenOptionsExt;
            use windows_sys::Win32::{
                Foundation::{GENERIC_READ, GENERIC_WRITE},
                Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE, WRITE_DAC, WRITE_OWNER},
            };
            // The creation-only sealer needs these rights on this newly created
            // handle. Existing destinations never receive this access or seal.
            options
                .access_mode(GENERIC_READ | GENERIC_WRITE | WRITE_DAC | WRITE_OWNER)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
        }
        // Store the exact successful create_new result BEFORE sealing or writing.
        self.file = Some(
            self.directory()?
                .open_with(&self.path, &options)
                .map_err(|_| WorkspaceReceiveError::Object)?
                .into_std(),
        );
        private::seal_created_file_handle(self.file()?)
            .map_err(|_| WorkspaceReceiveError::Object)?;
        self.entry()?;
        for chunk in bytes.chunks(CHUNK) {
            self.local(self.deadline)?;
            let mut file = self.file()?;
            file.write_all(chunk)
                .map_err(|_| WorkspaceReceiveError::Io)?;
            self.local(self.deadline)?;
        }
        self.file()?
            .sync_all()
            .map_err(|_| WorkspaceReceiveError::Io)?;
        self.local(self.deadline)?;
        #[cfg(unix)]
        self.directory_sync
            .as_ref()
            .ok_or(WorkspaceReceiveError::Io)?
            .sync_all()
            .map_err(|_| WorkspaceReceiveError::Io)?;
        #[cfg(windows)]
        self.directory_sync
            .as_mut()
            .ok_or(WorkspaceReceiveError::Io)?
            .sync()
            .map_err(|_| WorkspaceReceiveError::Io)?;
        #[cfg(not(any(unix, windows)))]
        return Err(WorkspaceReceiveError::Io);
        self.local(self.deadline)?;
        self.binding.check(&self.cap)?;
        Ok(())
    }
    fn directory(&self) -> Result<&Dir, WorkspaceReceiveError> {
        self.directory
            .as_ref()
            .ok_or(WorkspaceReceiveError::Incomplete)
    }
    fn file(&self) -> Result<&File, WorkspaceReceiveError> {
        self.file.as_ref().ok_or(WorkspaceReceiveError::Incomplete)
    }
    fn entry(&self) -> Result<(), WorkspaceReceiveError> {
        self.binding.check(&self.cap)?;
        let original = self.file()?;
        regular(original)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let entry = self
            .directory()?
            .open_with(&self.path, &options)
            .map_err(|_| WorkspaceReceiveError::Object)?
            .into_std();
        regular(&entry)?;
        if !hagency_platform::same_file(original, &entry)
            .map_err(|_| WorkspaceReceiveError::Object)?
        {
            return Err(WorkspaceReceiveError::Object);
        }
        Ok(())
    }
    fn readback(&self, deadline: Instant) -> Result<(), WorkspaceReceiveError> {
        self.local(deadline)?;
        self.entry()?;
        let mut file = self.file()?;
        let before = file.metadata().map_err(|_| WorkspaceReceiveError::Object)?;
        if before.len() != self.facts().size {
            return Err(WorkspaceReceiveError::Object);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| WorkspaceReceiveError::Io)?;
        let mut chunk = [0; CHUNK];
        let mut total = 0;
        let mut digest = Sha256::new();
        loop {
            self.local(deadline)?;
            // Read one extra byte at most to detect append without excess buffering.
            let bound = (self.facts().size - total + 1).min(CHUNK as u64) as usize;
            let count = file
                .read(&mut chunk[..bound])
                .map_err(|_| WorkspaceReceiveError::Io)?;
            self.local(deadline)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > self.facts().size {
                return Err(WorkspaceReceiveError::Object);
            }
            digest.update(&chunk[..count]);
        }
        let after = file.metadata().map_err(|_| WorkspaceReceiveError::Object)?;
        if total != self.facts().size
            || after.len() != before.len()
            || after.modified().ok() != before.modified().ok()
            || format!("{:x}", digest.finalize()) != self.facts().sha256
        {
            return Err(WorkspaceReceiveError::Object);
        }
        self.entry()?;
        self.local(deadline)
    }
}

fn regular(file: &File) -> Result<(), WorkspaceReceiveError> {
    private::check_handle(file).map_err(|_| WorkspaceReceiveError::Object)?;
    if !file
        .metadata()
        .map_err(|_| WorkspaceReceiveError::Object)?
        .is_file()
    {
        return Err(WorkspaceReceiveError::Object);
    }
    Ok(())
}
