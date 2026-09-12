//! Retained local NTFS sync only. No namespace, dispatch or upload authority.
use crate::{Error, private};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, OpenOptions, OpenOptionsExt};
use std::{fs::File, os::windows::io::AsRawHandle, ptr};
use windows_sys::{
    Wdk::{
        Storage::FileSystem::{FileFsDeviceInformation, NtQueryVolumeInformationFile},
        System::SystemServices::FILE_FS_DEVICE_INFORMATION,
    },
    Win32::{
        Foundation::{STATUS_PENDING, STATUS_SUCCESS, WAIT_OBJECT_0},
        Storage::FileSystem::{
            FILE_ID_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo,
            GetFileInformationByHandleEx, GetVolumeInformationByHandleW,
        },
        System::{
            IO::IO_STATUS_BLOCK,
            SystemServices::FILE_READ_ONLY_VOLUME,
            Threading::{INFINITE, Sleep, WaitForSingleObject},
        },
    },
};

// Compile-time checks tie the fixed outputs to the pinned C layouts. No layout
// assertion, formatting or allocation is evaluated after a native call starts.
const _: () = {
    assert!(size_of::<FILE_FS_DEVICE_INFORMATION>() == 8);
    assert!(align_of::<FILE_FS_DEVICE_INFORMATION>() == 4);
    assert!(size_of::<IO_STATUS_BLOCK>() == 2 * size_of::<usize>());
    assert!(std::mem::offset_of!(IO_STATUS_BLOCK, Information) == size_of::<usize>());
    assert!(size_of::<FILE_ID_INFO>() == 24);
    assert!(std::mem::offset_of!(FILE_ID_INFO, FileId) == 8);
};

#[derive(Clone, Copy, Eq, PartialEq)]
struct Identity {
    volume: u64,
    id: [u8; 16],
}

/// Owns one fresh synchronous directory object. Opening and syncing can block
/// the calling worker; an outer timeout cannot prove that worker has stopped.
/// No raw handle, clone or caller-controlled options escape this boundary.
pub struct WindowsDirectorySync {
    file: File,
    identity: Identity,
}
impl WindowsDirectorySync {
    /// Derive only fixed relative dot from the actual retained directory. None
    /// preserves unconfirmed inspection for unavailable/unsupported sync. A
    /// wrong object or private policy refuses before any journal is created.
    pub fn open(directory: &Dir) -> Result<Option<Self>, Error> {
        let Some((file, identity)) = candidate(directory)? else {
            return Ok(None);
        };
        let Some(file) = query(
            file,
            #[cfg(test)]
            None,
        ) else {
            return Ok(None);
        };
        let mut owner = Self { file, identity };
        owner.check()?;
        // A supported profile does not replace an actual directory flush. This
        // initial ACK precedes journal IO; every later journal needs its own ACK.
        if owner.flush().is_err() {
            return Ok(None);
        }
        owner.check()?;
        Ok(Some(owner))
    }

    pub fn sync(&mut self) -> Result<(), Error> {
        self.check()?;
        self.flush()?;
        self.check()
    }

    fn check(&self) -> Result<(), Error> {
        if identity(&self.file)? != self.identity {
            return Err(Error::Private);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.file.sync_all()?;
        Ok(())
    }
}

fn identity(file: &File) -> Result<Identity, Error> {
    private::check_handle(file)?;
    if !file.metadata()?.is_dir() {
        return Err(Error::Private);
    }
    let mut value = FILE_ID_INFO::default();
    // SAFETY: The File owns its live handle. FileIdInfo has this exact initialized
    // output layout/size; Win32 completes before returning, and no pointer escapes.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut value as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(Identity {
        volume: value.VolumeSerialNumber,
        id: value.FileId.Identifier,
    })
}

fn candidate(directory: &Dir) -> Result<Option<(File, Identity)>, Error> {
    let original = directory.try_clone()?.into_std_file();
    let before = identity(&original)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    // Pinned cap-primitives4.0.3 MaybeOwnedFile::into_file opens dot through
    // open_unchecked/CreateFileAtW: a fresh NtCreateFile object with retained
    // RootDirectory, SYNCHRONIZE and FILE_SYNCHRONOUS_IO_NONALERT. There is no
    // overlapped flag, ReOpenFile, handle duplication or ambient path fallback.
    let file = match directory.open_with(".", &options) {
        Ok(file) => file.into_std(),
        Err(_) => return Ok(None),
    };
    if identity(&file)? != before {
        return Err(Error::Private);
    }
    Ok(Some((file, before)))
}

fn supported_profile(device: u32, characteristics: u32) -> bool {
    const MOUNTED: u32 = 0x20;
    const ALLOW_APPCONTAINER_TRAVERSAL: u32 = 0x20000;
    device == 7
        && characteristics & MOUNTED != 0
        && characteristics & !(MOUNTED | ALLOW_APPCONTAINER_TRAVERSAL) == 0
}

// Heap storage has a stable address even across the exceptional parking path.
// The File is a fresh private synchronous object: no external caller can clone
// it or run competing IO and signal completion of a different request.
struct Query {
    file: File,
    device: FILE_FS_DEVICE_INFORMATION,
    status: IO_STATUS_BLOCK,
}

fn query(file: File, #[cfg(test)] gate: Option<tests::Gate>) -> Option<File> {
    let mut filesystem = [0u16; 32];
    let mut flags = 0;
    // SAFETY: Live borrowed File, initialized fixed output storage and optional
    // null outputs meet the synchronous Win32 signature. No buffer escapes.
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
        || filesystem[..5] != [78, 84, 70, 83, 0]
        || flags & FILE_READ_ONLY_VOLUME != 0
    {
        return None;
    }
    let mut original = Box::new(Query {
        file,
        device: FILE_FS_DEVICE_INFORMATION::default(),
        status: IO_STATUS_BLOCK::default(),
    });
    original.status.Anonymous.Status = STATUS_PENDING;
    // This test-only gate is BEFORE the native call: it proves caller-loss
    // ownership, not an actual STATUS_PENDING result. No callback can execute
    // after the kernel may hold the output pointers.
    #[cfg(test)]
    if let Some(gate) = gate {
        gate.wait();
    }
    // SAFETY: Exact pinned native layout/class/length, live private synchronous
    // File and initialized fixed Box storage. The Box never moves its pointee.
    // Any pending or inconsistent completion retains it through completion or
    // parks the original worker permanently without freeing kernel IO storage.
    let result = unsafe {
        NtQueryVolumeInformationFile(
            original.file.as_raw_handle(),
            &mut original.status,
            (&mut original.device as *mut FILE_FS_DEVICE_INFORMATION).cast(),
            size_of::<FILE_FS_DEVICE_INFORMATION>() as u32,
            FileFsDeviceInformation,
        )
    };
    if result == STATUS_PENDING {
        // SAFETY: The unique fresh object includes SYNCHRONIZE and no other IO
        // can signal it. The File and stable query storage remain owned here.
        if unsafe { WaitForSingleObject(original.file.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0
        {
            park_original(original);
        }
    } else if result != STATUS_SUCCESS {
        // A non-pending terminal error has no outstanding native buffer use.
        return None;
    }
    // SAFETY: Immediate synchronous completion or original-object wait ACK
    // establishes completion of this exact query before reading its Status.
    let completion = unsafe { original.status.Anonymous.Status };
    if completion == STATUS_PENDING {
        park_original(original);
    }
    if completion != STATUS_SUCCESS
        || original.status.Information != size_of::<FILE_FS_DEVICE_INFORMATION>()
        || !supported_profile(original.device.DeviceType, original.device.Characteristics)
    {
        return None;
    }
    let Query { file, .. } = *original;
    Some(file)
}

fn park_original(original: Box<Query>) -> ! {
    // Box::leak does not allocate, format, invoke callbacks, or unwind. It keeps
    // the original File AND stable native output memory alive even if a compiler
    // removes this nonreturning function's stack. This worker never admits again:
    // one fixed retained allocation, not a leak in a reusable retry/admission loop.
    let _original = Box::leak(original);
    loop {
        // SAFETY: Sleep takes a scalar timeout and no pointers. It does not
        // relinquish our retained object or native IO storage. No Rust parking
        // initialization, allocation, formatting, callback or panic path occurs.
        unsafe { Sleep(INFINITE) };
    }
}

#[cfg(test)]
mod tests;
