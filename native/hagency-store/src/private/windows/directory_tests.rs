//! Actual current-token/default-owner observations; never change the token.
use super::*;
use crate::private;
use std::fs;

struct Security {
    _descriptor: Allocation,
    owner: PSID,
    dacl: *mut ACL,
    protected: bool,
}
fn security(file: &File) -> Security {
    // SAFETY: Win32 fills initialized outputs; owner and DACL borrow the owned
    // descriptor until Security is dropped, and every read stays within it.
    unsafe {
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        assert_eq!(
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor
            ),
            0
        );
        let allocation = Allocation(descriptor);
        assert!(!owner.is_null() && !dacl.is_null());
        assert_ne!(IsValidSid(owner), 0);
        assert_ne!(IsValidAcl(dacl), 0);
        let mut control = 0;
        let mut revision = 0;
        assert_ne!(
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
            0
        );
        Security {
            _descriptor: allocation,
            owner,
            dacl,
            protected: control & SE_DACL_PROTECTED != 0,
        }
    }
}
fn private_ace(security: &Security, user: PSID) {
    // SAFETY: The live descriptor was validated above. The test's exact one-ACE
    // parent descriptor must yield one bounded ACCESS_ALLOWED_ACE for this user.
    unsafe {
        assert_eq!((*security.dacl).AceCount, 1);
        let mut ace = ptr::null_mut();
        assert_ne!(GetAce(security.dacl, 0, &mut ace), 0);
        assert!(!ace.is_null());
        let header = &*ace.cast::<ACE_HEADER>();
        assert_eq!(header.AceType, ACCESS_ALLOWED_ACE_TYPE as u8);
        let offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        assert!(header.AceSize as usize >= offset + 8);
        let count = *ace.cast::<u8>().add(offset + 1) as usize;
        assert!(offset + 8 + 4 * count <= header.AceSize as usize);
        let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
        assert_eq!(
            allowed.Mask,
            windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS
        );
        let actual = ptr::addr_of!(allowed.SidStart).cast_mut().cast();
        assert_ne!(IsValidSid(actual), 0);
        assert_ne!(EqualSid(actual, user), 0);
    }
}
fn default_owner_is_user(user: PSID, directory_owner: PSID) -> bool {
    // SAFETY: Query-only token access, fixed bounded aligned information storage,
    // and no token mutation. The returned SID is used only while storage lives.
    unsafe {
        let mut raw = ptr::null_mut();
        assert_ne!(
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw),
            0
        );
        let token = OwnedHandle::from_raw_handle(raw);
        let mut needed = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenOwner,
            ptr::null_mut(),
            0,
            &mut needed,
        );
        assert!(needed as usize >= size_of::<TOKEN_OWNER>() && needed <= 65536);
        let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        assert_ne!(
            GetTokenInformation(
                token.as_raw_handle(),
                TokenOwner,
                storage.as_mut_ptr().cast(),
                needed,
                &mut needed
            ),
            0
        );
        let owner = (*storage.as_ptr().cast::<TOKEN_OWNER>()).Owner;
        assert!(!owner.is_null());
        assert_ne!(IsValidSid(owner), 0);
        assert_ne!(EqualSid(owner, directory_owner), 0);
        EqualSid(owner, user) != 0
    }
}
fn open_directory(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .unwrap()
}

#[test]
fn native_private_directory_creation() {
    let temporary = tempfile::tempdir().unwrap();
    let parent = temporary.path().join("private");
    private::directory(&parent).unwrap();
    let old = parent.join("default");
    fs::DirBuilder::new().create(&old).unwrap();
    let old_file = open_directory(&old);
    let old_security = security(&old_file);
    let fresh = parent.join("explicit");
    private::create_directory_new(&fresh).unwrap();
    let file = open_directory(&fresh);
    let before = security(&file);
    // SAFETY: ConvertStringSidToSidW owns a valid SID allocation. All compared
    // descriptors and the SID stay alive through these synchronous queries.
    unsafe {
        let sid: Vec<_> = current_sid()
            .unwrap()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut user = ptr::null_mut();
        assert_ne!(ConvertStringSidToSidW(sid.as_ptr(), &mut user), 0);
        let _user = Allocation(user);
        private_ace(&old_security, user);
        private_ace(&before, user);
        let default_is_user = default_owner_is_user(user, old_security.owner);
        assert_eq!(EqualSid(old_security.owner, user) != 0, default_is_user);
        assert_eq!(private::directory(&old).is_ok(), default_is_user);
        assert!(
            matches!(private::create_directory_new(&old), Err(Error::Io(error))
            if error.kind() == std::io::ErrorKind::AlreadyExists)
        );
        let old_after = security(&old_file);
        assert_ne!(EqualSid(old_security.owner, old_after.owner), 0);
        assert_eq!(old_after.protected, old_security.protected);
        private_ace(&old_after, user);
        assert_eq!(private::directory(&old).is_ok(), default_is_user);
        assert_ne!(EqualSid(before.owner, user), 0);
        assert!(before.protected);
        use std::io::Write;
        writeln!(std::io::stderr().lock(),
            "native directory creation observation: default_owner_is_user={default_is_user} default_acl_is_private=true explicit_owner_is_user=true explicit_dacl_is_protected=true"
        ).unwrap();
    }
    private::directory(&fresh).unwrap();
    private::write_new(&fresh.join("original"), b"original bytes").unwrap();
    assert!(
        matches!(private::create_directory_new(&fresh), Err(Error::Io(error))
        if error.kind() == std::io::ErrorKind::AlreadyExists)
    );
    let after = security(&file);
    // SAFETY: Both original descriptor allocations remain alive here.
    assert_ne!(unsafe { EqualSid(before.owner, after.owner) }, 0);
    assert!(after.protected);
    private_ace(&after, before.owner);
    assert_eq!(
        private::read_secret(&fresh.join("original")).unwrap(),
        b"original bytes"
    );
    assert!(private::create_directory_new(&parent.join("missing/child")).is_err());
    assert!(!parent.join("missing").exists());
    let existing_file = parent.join("file");
    private::write_new(&existing_file, b"existing file").unwrap();
    assert!(private::create_directory_new(&existing_file).is_err());
    assert_eq!(
        private::read_secret(&existing_file).unwrap(),
        b"existing file"
    );

    let contested = parent.join("contested");
    let barrier = std::sync::Barrier::new(2);
    let outcomes = std::thread::scope(|scope| {
        let create = || {
            barrier.wait();
            private::create_directory_new(&contested)
        };
        let first = scope.spawn(create);
        let second = scope.spawn(create);
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists))
            .count(),
        1
    );
    private::directory(&contested).unwrap();
}
