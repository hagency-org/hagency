//! Windows state ACL adapter. Only the current owner SID can access state.
use crate::Error;
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{Authorization::*, *},
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateDirectoryW, CreateFileW,
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    },
    System::{
        SystemServices::ACCESS_ALLOWED_ACE_TYPE,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

struct Allocation(*mut c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: Each instance owns one non-null LocalAlloc allocation returned by Win32.
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn wide(path: &Path) -> Result<Vec<u16>, Error> {
    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(Error::Private);
    }
    value.push(0);
    Ok(value)
}

fn current_sid() -> Result<String, Error> {
    // SAFETY: All output pointers reference initialized storage. Token information uses
    // usize-aligned storage, and its pointers are read only while that buffer is alive.
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut needed = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut needed,
        );
        if needed == 0 || needed > 65536 {
            return Err(Error::Private);
        }
        let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let _allocated = Allocation(text.cast());
        let mut len = 0;
        while len < 256 && *text.add(len) != 0 {
            len += 1;
        }
        if len == 256 {
            return Err(Error::Private);
        }
        String::from_utf16(std::slice::from_raw_parts(text, len)).map_err(|_| Error::Private)
    }
}

pub(super) fn create_directory_new(path: &Path) -> Result<(), Error> {
    let descriptor: Vec<_> = format!("O:{sid}D:P(A;OICI;FA;;;{sid})\0", sid = current_sid()?)
        .encode_utf16()
        .collect();
    let name = wide(path)?;
    // SAFETY: UTF-16 strings are NUL terminated; the descriptor remains owned until
    // CreateDirectoryW completes. No handle is inherited and no raw pointers escape.
    unsafe {
        let mut descriptor_ptr = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor_ptr,
            ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let _allocated = Allocation(descriptor_ptr);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor_ptr,
            bInheritHandle: 0,
        };
        if CreateDirectoryW(name.as_ptr(), &attributes) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

pub(super) fn check_directory(path: &Path) -> Result<(), Error> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    if !file.metadata()?.is_dir() {
        return Err(Error::Private);
    }
    check(&file, path)
}

fn check(file: &File, path: &Path) -> Result<(), Error> {
    check_with_policy(file, path, false)
}

#[cfg(test)]
#[path = "windows/directory_tests.rs"]
mod directory_tests;

pub(super) fn check_with_policy(
    file: &File,
    path: &Path,
    sqlite_journal: bool,
) -> Result<(), Error> {
    if std::fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::Private);
    }
    check_handle(file, sqlite_journal)
}

pub(super) fn check_handle(file: &File, sqlite_journal: bool) -> Result<(), Error> {
    let meta = file.metadata()?;
    if !(meta.is_file() || meta.is_dir())
        || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(Error::Private);
    }
    let sid = current_sid()?;
    let sid_wide: Vec<_> = sid.encode_utf16().chain(Some(0)).collect();
    // SAFETY: GetSecurityInfo owns the returned descriptor; its SID and ACL pointers
    // borrow that descriptor. Every ACE is checked before casting and accessing its SID.
    unsafe {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if GetFileInformationByHandle(file.as_raw_handle(), &mut info) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if file.metadata()?.is_file() && info.nNumberOfLinks != 1 {
            return Err(Error::Private);
        }
        let mut expected = ptr::null_mut();
        if ConvertStringSidToSidW(sid_wide.as_ptr(), &mut expected) == 0 {
            return Err(Error::Private);
        }
        let _expected = Allocation(expected);
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        let result = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        );
        if result != 0 {
            return Err(std::io::Error::from_raw_os_error(result as i32).into());
        }
        let _descriptor = Allocation(descriptor);
        if owner.is_null()
            || dacl.is_null()
            || (EqualSid(owner, expected) == 0
                && !(sqlite_journal && IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0))
            || !IsValidAcl(dacl).is_positive()
        {
            return Err(Error::Private);
        }
        if (*dacl).AceCount == 0 {
            return Err(Error::Private);
        }
        for index in 0..(*dacl).AceCount {
            let mut ace = ptr::null_mut();
            if GetAce(dacl, index.into(), &mut ace) == 0 || ace.is_null() {
                return Err(Error::Private);
            }
            let header = &*ace.cast::<ACE_HEADER>();
            if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
                || (header.AceSize as usize)
                    < std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart) + 8
            {
                return Err(Error::Private);
            }
            let sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
            let subauthorities = *ace.cast::<u8>().add(sid_offset + 1) as usize;
            if sid_offset + 8 + 4 * subauthorities > header.AceSize as usize {
                return Err(Error::Private);
            }
            let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            let sid_ptr = ptr::addr_of!(allowed.SidStart).cast::<c_void>().cast_mut();
            if IsValidSid(sid_ptr) == 0 || EqualSid(sid_ptr, expected) == 0 {
                return Err(Error::Private);
            }
        }
    }
    Ok(())
}

pub(super) fn seal_created_file_handle(file: &File) -> Result<(), Error> {
    fn candidate(file: &File) -> Result<(), Error> {
        let meta = file.metadata()?;
        if !meta.is_file()
            || meta.len() != 0
            || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(Error::Private);
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the live borrowed File owns its handle; output is initialized
        // and used only after Win32 reports success. No raw handle escapes.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if info.nNumberOfLinks != 1 {
            return Err(Error::Private);
        }
        Ok(())
    }
    candidate(file)?;
    let sid = current_sid()?;
    let sddl: Vec<u16> = format!("O:{sid}D:P(A;;FA;;;{sid})\0")
        .encode_utf16()
        .collect();
    // SAFETY: the borrowed File remains live and was created with WRITE_OWNER
    // and WRITE_DAC. The allocated descriptor owns owner/DACL pointers until the
    // synchronous SetSecurityInfo completes. Nothing is resolved by pathname.
    unsafe {
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let _allocated = Allocation(descriptor);
        let mut owner = ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        let mut dacl = ptr::null_mut();
        if GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) == 0
            || GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) == 0
            || present == 0
            || owner.is_null()
            || dacl.is_null()
        {
            return Err(Error::Private);
        }
        let result = SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        );
        if result != 0 {
            return Err(std::io::Error::from_raw_os_error(result as i32).into());
        }
    }
    candidate(file)?;
    check_handle(file, false)
}

pub(super) fn create_file(path: &Path) -> Result<File, Error> {
    let sid = current_sid()?;
    let descriptor: Vec<_> = format!("O:{sid}D:P(A;;FA;;;{sid})\0")
        .encode_utf16()
        .collect();
    let name = wide(path)?;
    // SAFETY: NUL-terminated names and a live descriptor are passed to a synchronous
    // create-only call. A valid returned handle transfers exactly once to File.
    unsafe {
        let mut descriptor_ptr = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor_ptr,
            ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let _allocated = Allocation(descriptor_ptr);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor_ptr,
            bInheritHandle: 0,
        };
        let handle = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(File::from_raw_handle(handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_storage_rejects_public_access() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("state");
        crate::private::directory(&root).unwrap();
        let token = root.join("operator.token");
        drop(create_file(&token).unwrap());
        let sid = current_sid().unwrap();
        // Fixture ACL deliberately adds World read access. It must fail closed.
        let sddl: Vec<_> = format!("D:P(A;;FA;;;{sid})(A;;GR;;;WD)\0")
            .encode_utf16()
            .collect();
        let name = wide(&token).unwrap();
        // SAFETY: The descriptor and all output storage live through the synchronous
        // ACL calls; only this test-owned file is changed and allocations are released.
        unsafe {
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    ptr::null_mut()
                ),
                0
            );
            let _allocated = Allocation(descriptor);
            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted),
                0
            );
            assert_ne!(present, 0);
            assert_eq!(
                SetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    dacl,
                    ptr::null()
                ),
                0
            );
        }
        assert!(matches!(
            crate::private::read_secret(&token),
            Err(Error::Private)
        ));
        assert!(matches!(
            crate::private::check_handle(&File::open(&token).unwrap()),
            Err(Error::Private)
        ));
        assert!(matches!(
            crate::private::open_journal(&token),
            Err(Error::Private)
        ));
    }
}
