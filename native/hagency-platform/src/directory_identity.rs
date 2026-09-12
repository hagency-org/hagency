//! Compare two live directory objects. This is a consistency observation, not
//! a portable identity, namespace lock, or replacement for retaining the objects.
use std::{fs::File, io};

/// A local physical observation. Serialized identity is never a directory grant.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryIdentity {
    platform: String,
    volume: String,
    object: [u8; 16],
}

pub fn directory_identity(file: &File) -> io::Result<DirectoryIdentity> {
    if !file.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory required",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        let mut object = [0; 16];
        object[8..].copy_from_slice(&meta.ino().to_be_bytes());
        Ok(DirectoryIdentity {
            platform: "unix-v1".into(),
            volume: format!("{:016x}", meta.dev()),
            object,
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
        };
        let mut value = FILE_ID_INFO::default();
        // SAFETY: the borrowed File retains a live handle; the initialized output
        // has the exact Win32 size/alignment and is read only after success.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut value as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(DirectoryIdentity {
            platform: "windows-v1".into(),
            volume: format!("{:016x}", value.VolumeSerialNumber),
            object: value.FileId.Identifier,
        })
    }
    #[cfg(not(any(unix, windows)))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "directory identity unavailable",
    ))
}

pub fn same_directory(left: &File, right: &File) -> io::Result<bool> {
    Ok(directory_identity(left)? == directory_identity(right)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(path: &std::path::Path) -> File {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
            options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
        }
        options.open(path).unwrap()
    }
    #[test]
    fn native_workspace_directory_identity() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let first = open(root.path());
        assert!(same_directory(&first, &first.try_clone().unwrap()).unwrap());
        assert!(same_directory(&first, &open(root.path())).unwrap());
        assert!(!same_directory(&first, &open(other.path())).unwrap());
        let file = File::create(root.path().join("ordinary")).unwrap();
        assert_eq!(
            same_directory(&first, &file).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
