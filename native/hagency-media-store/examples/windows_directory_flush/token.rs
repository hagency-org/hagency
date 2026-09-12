//! No privileges are enabled. The guard cannot move to a different thread.
use super::{Failure, Result};
use std::{
    marker::PhantomData,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    rc::Rc,
};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Security::*,
    System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
};

pub(super) struct Ordinary {
    _token: OwnedHandle,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Ordinary {
    fn drop(&mut self) {
        // SAFETY: This !Send guard is dropped on the impersonating thread. No
        // impersonated IO follows it; failure terminates this disposable child.
        if unsafe { RevertToSelf() } == 0 {
            eprintln!("{{\"phase\":\"revert\",\"qualified\":false,\"code\":-1}}");
            std::process::exit(78);
        }
    }
}
struct Information {
    storage: Vec<u64>,
    bytes: usize,
}
fn information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Information> {
    let mut storage = vec![0u64; 8192];
    let mut needed = 0;
    // SAFETY: The owned live token and initialized 64KiB, u64-aligned buffer
    // remain valid for this synchronous call. No pointer escapes the buffer.
    if unsafe {
        GetTokenInformation(
            token,
            class,
            storage.as_mut_ptr().cast(),
            65536,
            &mut needed,
        )
    } == 0
    {
        return Err(Failure::last("token_query"));
    }
    if needed == 0 || needed > 65536 {
        return Err(Failure::refused("token_size"));
    }
    Ok(Information {
        storage,
        bytes: needed as usize,
    })
}
fn sid(info: &Information) -> Result<PSID> {
    if info.bytes < size_of::<TOKEN_USER>() {
        return Err(Failure::refused("token_user_size"));
    }
    // SAFETY: TOKEN_USER is initialized by GetTokenInformation and aligned.
    let sid = unsafe { (*info.storage.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let base = info.storage.as_ptr() as usize;
    let address = sid as usize;
    if address < base
        || address
            .checked_add(8)
            .is_none_or(|end| end > base + info.bytes)
    {
        return Err(Failure::refused("token_sid_range"));
    }
    // SAFETY: The fixed SID prefix fits. Check its bounded subauthority length
    // before passing the full SID to a Win32 validator.
    let count = unsafe { *(sid.cast::<u8>().add(1)) } as usize;
    if count > 15 || address + 8 + count * 4 > base + info.bytes {
        return Err(Failure::refused("token_sid_length"));
    }
    // SAFETY: All bytes declared by this SID are within initialized storage.
    if unsafe { IsValidSid(sid) } == 0 {
        return Err(Failure::refused("token_sid"));
    }
    Ok(sid)
}
impl Ordinary {
    pub(super) fn enter() -> Result<Self> {
        let mut original = ptr::null_mut();
        // SAFETY: Pseudo process handle is borrowed, output is initialized.
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE | TOKEN_ADJUST_PRIVILEGES,
                &mut original,
            )
        } == 0
        {
            return Err(Failure::last("process_token"));
        }
        // SAFETY: Success transfers this unique non-null real token handle.
        let original = unsafe { OwnedHandle::from_raw_handle(original) };
        let original_user = information(original.as_raw_handle(), TokenUser)?;
        let mut admin = [0u64; 9];
        let mut admin_bytes = size_of_val(&admin) as u32;
        // SAFETY: The aligned 72-byte SID buffer is initialized and retained.
        if unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                ptr::null_mut(),
                admin.as_mut_ptr().cast(),
                &mut admin_bytes,
            )
        } == 0
        {
            return Err(Failure::last("administrator_sid"));
        }
        let disable = SID_AND_ATTRIBUTES {
            Sid: admin.as_mut_ptr().cast(),
            Attributes: 0,
        };
        let mut restricted = ptr::null_mut();
        // SAFETY: Input handle/SID are live, optional arrays are null with zero
        // counts, and output is initialized. This only removes authority.
        if unsafe {
            CreateRestrictedToken(
                original.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE,
                1,
                &disable,
                0,
                ptr::null(),
                0,
                ptr::null(),
                &mut restricted,
            )
        } == 0
        {
            return Err(Failure::last("restrict_token"));
        }
        // SAFETY: Successful API returns one independently owned token.
        let restricted = unsafe { OwnedHandle::from_raw_handle(restricted) };
        // SAFETY: DisableAllPrivileges ignores the null optional state. No
        // privilege is enabled, including backup/restore or change-notify.
        if unsafe {
            AdjustTokenPrivileges(
                restricted.as_raw_handle(),
                1,
                ptr::null(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(Failure::last("disable_privileges"));
        }
        // SAFETY: This live same-user primary token is retained until reversion.
        if unsafe { ImpersonateLoggedOnUser(restricted.as_raw_handle()) } == 0 {
            return Err(Failure::last("impersonate"));
        }
        let guard = Self {
            _token: restricted,
            _thread: PhantomData,
        };
        let mut effective = ptr::null_mut();
        // SAFETY: Query the actual effective thread token; do not infer its
        // privileges from the source token or original process elevation.
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut effective) } == 0 {
            return Err(Failure::last("effective_token"));
        }
        // SAFETY: This is a newly owned successful query handle.
        let effective = unsafe { OwnedHandle::from_raw_handle(effective) };
        let current_user = information(effective.as_raw_handle(), TokenUser)?;
        {
            let app_container = information(effective.as_raw_handle(), TokenIsAppContainer)?;
            if app_container.bytes != size_of::<u32>() {
                return Err(Failure::refused("app_container_size"));
            }
            // SAFETY: The initialized aligned buffer contains the documented
            // DWORD result of TokenIsAppContainer, with its exact byte length.
            if unsafe { *app_container.storage.as_ptr().cast::<u32>() } != 0 {
                return Err(Failure::refused("app_container_token"));
            }
        }
        let current_sid = sid(&current_user)?;
        for service in [WinLocalSystemSid, WinLocalServiceSid, WinNetworkServiceSid] {
            // SAFETY: The validated SID remains borrowed from current_user.
            // A service identity is not an ordinary-user qualification.
            if unsafe { IsWellKnownSid(current_sid, service) } != 0 {
                return Err(Failure::refused("service_identity"));
            }
        }
        // SAFETY: Both validated SID buffers remain alive throughout comparison.
        if unsafe { EqualSid(sid(&original_user)?, sid(&current_user)?) } == 0 {
            return Err(Failure::refused("same_user"));
        }
        let mut is_admin = 1;
        // SAFETY: Null token selects actual thread impersonation. SID is live.
        if unsafe {
            CheckTokenMembership(ptr::null_mut(), admin.as_mut_ptr().cast(), &mut is_admin)
        } == 0
        {
            return Err(Failure::last("effective_admin"));
        }
        if is_admin != 0 {
            return Err(Failure::refused("effective_admin"));
        }
        let privileges = information(effective.as_raw_handle(), TokenPrivileges)?;
        if privileges.bytes < 4 {
            return Err(Failure::refused("privilege_size"));
        }
        // SAFETY: Read initialized count, then bound the flexible array before
        // constructing a slice. u64 alignment meets TOKEN_PRIVILEGES alignment.
        let header = privileges.storage.as_ptr().cast::<TOKEN_PRIVILEGES>();
        let count = unsafe { (*header).PrivilegeCount } as usize;
        let offset = std::mem::offset_of!(TOKEN_PRIVILEGES, Privileges);
        if count > (privileges.bytes - offset) / size_of::<LUID_AND_ATTRIBUTES>() {
            return Err(Failure::refused("privilege_count"));
        }
        // SAFETY: The entire array is initialized and bounded by returned size.
        let entries = unsafe {
            std::slice::from_raw_parts(
                privileges
                    .storage
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<LUID_AND_ATTRIBUTES>(),
                count,
            )
        };
        if entries
            .iter()
            .any(|entry| entry.Attributes & SE_PRIVILEGE_ENABLED != 0)
        {
            return Err(Failure::refused("enabled_privilege"));
        }
        println!(
            "{{\"phase\":\"effective_non_admin\",\"same_user\":true,\"enabled_privileges\":0,\"app_container\":false}}"
        );
        Ok(guard)
    }
}
