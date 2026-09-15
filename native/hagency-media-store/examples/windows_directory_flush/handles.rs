//! Pinned bindings; no raw volume, pathname fallback or unowned handle escape.
use super::{Failure, Result};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, OpenOptions, OpenOptionsExt};
use hagency_store::private;
use std::{fs::File, io::Write, os::windows::io::AsRawHandle, ptr};
use windows_sys::{
    Wdk::{
        Storage::FileSystem::{
            FILE_RENAME_INFORMATION, FileFsDeviceInformation, FileRenameInformation,
            NtQueryVolumeInformationFile, NtSetInformationFile,
        },
        System::SystemServices::FILE_FS_DEVICE_INFORMATION,
    },
    Win32::{
        Security::{
            Authorization::{SE_FILE_OBJECT, SetSecurityInfo},
            DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::*,
        System::{IO::IO_STATUS_BLOCK, SystemServices::FILE_READ_ONLY_VOLUME},
    },
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct DirectoryIdentity {
    volume: u64,
    id: [u8; 16],
}

pub(super) fn identity(file: &File) -> Result<DirectoryIdentity> {
    private::check_handle(file).map_err(|_| Failure::refused("private_acl"))?;
    if !file
        .metadata()
        .map_err(|e| Failure::io("metadata", e))?
        .is_dir()
    {
        return Err(Failure::refused("directory_type"));
    }
    let mut info = FILE_ID_INFO::default();
    // SAFETY: File owns the handle; the initialized exact output type and size
    // correspond to FileIdInfo. No pointer or handle outlives the borrow.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(Failure::last("full_file_id"));
    }
    Ok(DirectoryIdentity {
        volume: info.VolumeSerialNumber,
        id: info.FileId.Identifier,
    })
}
pub(super) fn candidate(dir: &Dir) -> Result<File> {
    let original = dir
        .try_clone()
        .map_err(|e| Failure::io("baseline_clone", e))?
        .into_std_file();
    let before = identity(&original)?;
    let baseline = original.sync_all();
    println!(
        "{{\"phase\":\"baseline_sync\",\"ack\":{},\"code\":{}}}",
        baseline.is_ok(),
        baseline.err().and_then(|e| e.raw_os_error()).unwrap_or(0)
    );
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    let candidate = dir
        .open_with(".", &options)
        .map_err(|e| Failure::io("rw_dot_open", e))?
        .into_std();
    let after = identity(&candidate)?;
    if before != after {
        return Err(Failure::refused("same_object"));
    }
    println!("{{\"phase\":\"same_object\",\"full_identity\":true,\"private_acl\":true}}");
    profile(&candidate)?;
    candidate
        .sync_all()
        .map_err(|e| Failure::io("directory_before", e))?;
    println!("{{\"phase\":\"directory_before\",\"ack\":true}}");
    Ok(candidate)
}
fn profile(file: &File) -> Result<()> {
    let mut filesystem = [0u16; 32];
    let mut flags = 0;
    // SAFETY: Only a borrowed actual directory handle, fixed initialized output
    // buffers, and explicitly optional null outputs are passed synchronously.
    if unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut flags,
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(Failure::last("filesystem_query"));
    }
    if filesystem[..5] != [78, 84, 70, 83, 0] || flags & FILE_READ_ONLY_VOLUME != 0 {
        return Err(Failure::refused("ntfs_profile"));
    }
    let mut device = FILE_FS_DEVICE_INFORMATION::default();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: This is the pinned native signature with exact initialized result
    // structs and live handle. The relative open is synchronous. STATUS_PENDING
    // is an unexpected asynchronous ownership boundary: terminate the child
    // without unwinding the live result storage. Other failures are refused.
    let result = unsafe {
        NtQueryVolumeInformationFile(
            file.as_raw_handle(),
            &mut status,
            (&mut device as *mut FILE_FS_DEVICE_INFORMATION).cast(),
            size_of::<FILE_FS_DEVICE_INFORMATION>() as u32,
            FileFsDeviceInformation,
        )
    };
    if result == 0x103 {
        eprintln!("{{\"phase\":\"device_query_pending\",\"qualified\":false,\"code\":259}}");
        std::process::exit(78);
    }
    if result != 0 {
        return Err(Failure {
            phase: "device_query",
            code: i64::from(result),
        });
    }
    // SAFETY: This operation returns the Status member of the initialized
    // IO_STATUS_BLOCK union; the synchronous successful call has completed.
    let completion = unsafe { status.Anonymous.Status };
    if completion != 0 {
        return Err(Failure {
            phase: "device_completion",
            code: i64::from(completion),
        });
    }
    if status.Information != size_of::<FILE_FS_DEVICE_INFORMATION>() {
        return Err(Failure::refused("device_query_length"));
    }
    println!(
        "{{\"phase\":\"filesystem_profile\",\"ntfs\":true,\"device_type\":{},\"characteristics\":{}}}",
        device.DeviceType, device.Characteristics
    );
    // The optional named app-container traversal bit is classified explicitly.
    // The actual token must not be an app container and has no enabled
    // privileges. Every other characteristic still refuses.
    if !super::supported_profile(device.DeviceType, device.Characteristics) {
        return Err(Failure::refused("local_mounted_disk"));
    }
    Ok(())
}
fn new_file(dir: &Dir) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No)
        .access_mode(
            windows_sys::Win32::Foundation::GENERIC_READ
                | windows_sys::Win32::Foundation::GENERIC_WRITE
                | WRITE_DAC
                | WRITE_OWNER
                | DELETE,
        );
    let file = dir
        .open_with("probe.bin", &options)
        .map_err(|e| Failure::io("relative_create", e))?
        .into_std();
    private::seal_created_file_handle(&file).map_err(|_| Failure::refused("created_acl"))?;
    Ok(file)
}
fn delete_created(file: &File, phase: &'static str) -> Result<()> {
    let mut before = FILE_STANDARD_INFO::default();
    // SAFETY: The initialized standard-info buffer matches the information
    // class and exact size; the same newly created File remains live.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&mut before as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    } == 0
    {
        return Err(Failure::last("delete_file_info"));
    }
    if before.Directory
        || before.NumberOfLinks != 1
        || before.DeletePending
        || before.EndOfFile < 0
        || before.EndOfFile > 4096
    {
        return Err(Failure::refused("delete_created_object"));
    }
    let mut disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: DELETE was requested at this exact create_new, not by reopening
    // a path. The initialized one-byte disposition is synchronous and refers
    // only to this retained synthetic file. No readonly/privilege override.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&mut disposition as *mut FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(Failure::last(phase));
    }
    let mut after = FILE_STANDARD_INFO::default();
    // SAFETY: The borrowed same File still owns the delete-pending object and
    // the exact initialized output buffer remains valid for the complete call.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&mut after as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    } == 0
    {
        return Err(Failure::last("delete_pending_query"));
    }
    if !after.DeletePending {
        return Err(Failure::refused("delete_pending_missing"));
    }
    Ok(())
}
pub(super) fn mutate(dir: &Dir, candidate: &File) -> Result<()> {
    let original_id = identity(candidate)?;
    {
        let file = new_file(dir)?;
        // SAFETY: Only this freshly created empty synthetic fixture is changed.
        // A null DACL deliberately creates a negative private-check fixture;
        // no secret bytes are written and the exact owned entry is removed.
        let result = unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
            )
        };
        if result != 0 {
            return Err(Failure {
                phase: "negative_acl_setup",
                code: i64::from(result),
            });
        }
        if private::check_handle(&file).is_ok()
            || file
                .metadata()
                .map_err(|e| Failure::io("negative_metadata", e))?
                .len()
                != 0
        {
            return Err(Failure::refused("negative_acl_refusal"));
        }
        delete_created(&file, "negative_disposition")?;
    }
    // This real create_new must succeed after the original deletion handle
    // closes; a pending/unremoved old entry cannot be silently reused.
    let mut file = new_file(dir)?;
    println!("{{\"phase\":\"negative_acl_cleanup\",\"fresh_create_new\":true}}");
    file.write_all(b"bounded directory flush fixture")
        .map_err(|e| Failure::io("file_write", e))?;
    file.sync_all().map_err(|e| Failure::io("file_sync", e))?;
    candidate
        .sync_all()
        .map_err(|e| Failure::io("directory_after", e))?;
    let after = identity(candidate)?;
    if after != original_id {
        return Err(Failure::refused("post_write_identity"));
    }
    println!(
        "{{\"phase\":\"file_and_directory\",\"file_ack\":true,\"directory_ack\":true,\"negative_acl_refused\":true}}"
    );
    delete_created(&file, "probe_disposition")?;
    drop(file);
    let mut read = OpenOptions::new();
    read.read(true).follow(FollowSymlinks::No);
    match dir.open_with("probe.bin", &read) {
        Err(error) if error.raw_os_error() == Some(2) => {}
        Err(error) => return Err(Failure::io("deleted_entry_check", error)),
        Ok(_) => return Err(Failure::refused("deleted_entry_present")),
    }
    candidate
        .sync_all()
        .map_err(|e| Failure::io("directory_remove", e))?;
    Ok(())
}

fn rename_source(root: &Dir, reverse: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No)
        .access_mode(windows_sys::Win32::Foundation::GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    root.open_with(if reverse { "moved" } else { "stage" }, &options)
        .map(cap_std::fs::File::into_std)
}

pub(super) fn held_rename(root: &Dir) -> Result<()> {
    // DELETE access is required for the actual rename. A sharing failure at
    // this rooted open is the precise expected boundary while custody is held.
    match rename_source(root, false) {
        Err(error) if error.raw_os_error() == Some(32) => Ok(()),
        Err(error) => Err(Failure::io("held_rename_open", error)),
        Ok(_) => Err(Failure::refused("held_rename_open_allowed")),
    }
}

pub(super) fn rename_released(
    root: &Dir,
    reverse: bool,
    expected: DirectoryIdentity,
) -> Result<()> {
    let source =
        rename_source(root, reverse).map_err(|e| Failure::io("released_rename_open", e))?;
    if identity(&source)? != expected {
        return Err(Failure::refused("rename_original_identity"));
    }
    // These are the only destination leaves; no runtime/path input exists.
    let name: [u16; 5] = if reverse {
        [115, 116, 97, 103, 101] // stage
    } else {
        [109, 111, 118, 101, 100] // moved
    };
    const BYTES: usize = 64;
    const NAME: usize = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    const _: () = {
        assert!(std::mem::align_of::<FILE_RENAME_INFORMATION>() <= std::mem::align_of::<u64>());
        assert!(std::mem::offset_of!(FILE_RENAME_INFORMATION, Anonymous) == 0);
        assert!(std::mem::offset_of!(FILE_RENAME_INFORMATION, RootDirectory) == size_of::<usize>());
        assert!(
            std::mem::offset_of!(FILE_RENAME_INFORMATION, FileNameLength) == 2 * size_of::<usize>()
        );
        assert!(NAME == 2 * size_of::<usize>() + size_of::<u32>());
        assert!(size_of::<FILE_RENAME_INFORMATION>() + 12 <= BYTES);
        assert!(NAME + 12 <= BYTES);
    };
    let mut storage = [0u64; BYTES / size_of::<u64>()];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // SAFETY: Fixed initialized u64 storage provides the pinned struct's exact
    // alignment and more than its header + five UTF16 characters + terminator.
    // Offsets/size are checked above. Root/source handles remain borrowed/live;
    // the flexible name is copied within the allocation, not a one-item array.
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = root.as_raw_handle();
        (*info).FileNameLength = 10;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            storage.as_mut_ptr().cast::<u8>().add(NAME).cast::<u16>(),
            name.len(),
        );
    }
    let mut status = IO_STATUS_BLOCK::default();
    status.Anonymous.Status = 0x103; // Must be replaced by actual final success.
    // SAFETY: The synchronous rooted NtCreateFile handle owns the source. The
    // exact NT class uses this initialized/aligned structure and complete size,
    // with a live retained RootDirectory and no-replace fixed relative leaf.
    // Both input and IOSB storage remain alive through completion. Unexpected
    // pending terminates without unwinding stack storage still possibly in use.
    let result = unsafe {
        NtSetInformationFile(
            source.as_raw_handle(),
            &mut status,
            info.cast(),
            BYTES as u32,
            FileRenameInformation,
        )
    };
    if result == 0x103 {
        eprintln!("{{\"phase\":\"rename_pending\",\"qualified\":false,\"code\":259}}");
        std::process::exit(78);
    }
    if result != 0 {
        return Err(Failure {
            phase: "released_rename_nt",
            code: i64::from(result),
        });
    }
    // SAFETY: A synchronous successful call has completed the initialized IOSB;
    // Status is the documented result union member for this operation.
    let completion = unsafe { status.Anonymous.Status };
    if completion != 0 {
        return Err(Failure {
            phase: "rename_completion",
            code: i64::from(completion),
        });
    }
    println!(
        "{{\"phase\":\"released_rename_nt\",\"ack\":true,\"information\":{}}}",
        status.Information
    );
    if identity(&source)? != expected {
        return Err(Failure::refused("rename_retained_identity"));
    }
    // cap-primitives' maybe_dir preparation clears FILE_SHARE_DELETE even if
    // requested below. Finish this released-custody rename handle before the
    // rooted destination open; compare against the original frozen full ID.
    // The exclusive private fixture root remains held, with no new authority.
    drop(source);
    let mut options = OpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    let destination = root
        .open_with(if reverse { "stage" } else { "moved" }, &options)
        .map_err(|e| Failure::io("rename_destination_open", e))?
        .into_std();
    if identity(&destination)? != expected {
        return Err(Failure::refused("rename_destination_identity"));
    }
    Ok(())
}
