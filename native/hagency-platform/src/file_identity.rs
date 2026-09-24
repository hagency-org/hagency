//! Compare retained regular-file objects. Equality does not establish content,
//! permissions, link safety, durability, or workspace authorization.
use std::{fs::File, io};

pub fn same_file(left: &File, right: &File) -> io::Result<bool> {
    let left_metadata = left.metadata()?;
    let right_metadata = right.metadata()?;
    if !left_metadata.is_file() || !right_metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "regular file required",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(left_metadata.dev() == right_metadata.dev()
            && left_metadata.ino() == right_metadata.ino())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
        };
        fn identity(file: &File) -> io::Result<FILE_ID_INFO> {
            let mut value = FILE_ID_INFO::default();
            // SAFETY: the borrowed File retains its handle through this
            // synchronous call. Output is initialized, aligned, and exactly
            // FILE_ID_INFO-sized, and is inspected only after success.
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
            Ok(value)
        }
        let left = identity(left)?;
        let right = identity(right)?;
        Ok(left.VolumeSerialNumber == right.VolumeSerialNumber
            && left.FileId.Identifier == right.FileId.Identifier)
    }
    #[cfg(not(any(unix, windows)))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "regular-file identity unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(path: &std::path::Path) -> File {
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
    fn native_receive_file_identity() {
        let root = tempfile::tempdir().unwrap();
        let original_path = root.path().join("original");
        let other_path = root.path().join("other");
        std::fs::write(&original_path, b"same bytes").unwrap();
        std::fs::write(&other_path, b"same bytes").unwrap();
        let original = File::open(&original_path).unwrap();
        assert!(same_file(&original, &original.try_clone().unwrap()).unwrap());
        assert!(same_file(&original, &File::open(&original_path).unwrap()).unwrap());
        assert!(!same_file(&original, &File::open(&other_path).unwrap()).unwrap());

        let alias = root.path().join("alias");
        std::fs::hard_link(&original_path, &alias).unwrap();
        assert!(same_file(&original, &File::open(&alias).unwrap()).unwrap());
        // Link-count policy belongs to the sink, not this object comparison.
        std::fs::rename(&original_path, root.path().join("moved-original")).unwrap();
        std::fs::write(&original_path, b"same bytes").unwrap();
        assert!(!same_file(&original, &File::open(&original_path).unwrap()).unwrap());
        assert!(same_file(&original, &File::open(&alias).unwrap()).unwrap());

        let directory = directory(root.path());
        for (left, right) in [(&original, &directory), (&directory, &original)] {
            assert_eq!(
                same_file(left, right).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }
}
