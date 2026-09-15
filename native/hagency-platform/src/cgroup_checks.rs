//! Pure admission parsers. Passing these vectors is not Linux containment proof.
use std::{collections::BTreeSet, io};

#[derive(Clone, Copy)]
pub(crate) enum NamespaceKind {
    User,
    Cgroup,
}

pub(crate) fn kernel_release(release: &[u8]) -> io::Result<()> {
    let text = std::str::from_utf8(release).map_err(|_| namespace_refused())?;
    let mut parts = text.split('.');
    let supported = parts.next() == Some("6")
        && matches!(parts.next(), Some("8" | "12" | "14" | "17"))
        && parts
            .next()
            .is_some_and(|v| !v.is_empty() && v.as_bytes()[0].is_ascii_digit());
    if supported && text.len() <= 128 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Linux kernel family has not been source-qualified for cgroup recovery",
        ))
    }
}

pub(crate) fn namespace_identity(
    kind: NamespaceKind,
    filesystem: u64,
    inode: u64,
    namespace_type: i32,
) -> io::Result<()> {
    // Linux v6.8/v6.12/v6.14/v6.17 identities; see the pinned source in ADR048.
    // Dynamic namespace allocation starts at 0xF0000000 and cannot alias these.
    let (expected_inode, expected_type) = match kind {
        NamespaceKind::User => (0xEFFF_FFFD, 0x1000_0000),
        NamespaceKind::Cgroup => (0xEFFF_FFFB, 0x0200_0000),
    };
    if filesystem == 0x6e73_6673 && inode == expected_inode && namespace_type == expected_type {
        Ok(())
    } else {
        Err(namespace_refused())
    }
}

pub(crate) fn namespace_refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "initial Linux user and cgroup namespaces required",
    )
}

pub(crate) fn refuse() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "protected cgroup recovery admission refused",
    )
}

pub(crate) fn populated(bytes: &[u8]) -> io::Result<bool> {
    if bytes.len() > 4096 {
        return Err(refuse());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| refuse())?;
    let mut seen = BTreeSet::new();
    let mut populated = None;
    for line in text.lines() {
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        if fields.len() != 2
            || seen.len() >= 32
            || !seen.insert(fields[0])
            || !fields[0]
                .bytes()
                .all(|v| v.is_ascii_lowercase() || v == b'_')
            || fields[1].parse::<u64>().is_err()
        {
            return Err(refuse());
        }
        if fields[0] == "populated" {
            populated = Some(match fields[1] {
                "0" => false,
                "1" => true,
                _ => return Err(refuse()),
            });
        }
    }
    populated.ok_or_else(refuse)
}

pub(crate) fn host_status(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > 64 * 1024 {
        return Err(refuse());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| refuse())?;
    let required = [
        "Uid",
        "CapInh",
        "CapPrm",
        "CapEff",
        "CapBnd",
        "CapAmb",
        "NoNewPrivs",
    ];
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            return Err(refuse());
        };
        if !required.contains(&key) {
            continue;
        }
        if !seen.insert(key) {
            return Err(refuse());
        }
        let fields: Vec<_> = value.split_ascii_whitespace().collect();
        match key {
            "Uid" => {
                if fields.len() != 4
                    || fields.iter().any(|v| *v != fields[0])
                    || fields[0].parse::<u32>().ok().filter(|v| *v > 0).is_none()
                {
                    return Err(refuse());
                }
            }
            "NoNewPrivs" if fields.as_slice() != ["1"] => return Err(refuse()),
            "NoNewPrivs" => {}
            _ => {
                if fields.len() != 1
                    || fields[0].len() != 16
                    || u64::from_str_radix(fields[0], 16) != Ok(0)
                {
                    return Err(refuse());
                }
            }
        }
    }
    if seen.len() == required.len() {
        Ok(())
    } else {
        Err(refuse())
    }
}

/// Require a full cgroup mount, not a bind-mounted subtree hiding ancestors.
pub(crate) fn full_mount(bytes: &[u8], id: u64) -> io::Result<()> {
    if bytes.len() > 256 * 1024 {
        return Err(refuse());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| refuse())?;
    let mut found = false;
    for (index, line) in text.lines().enumerate() {
        if index >= 2048 || line.len() > 8192 {
            return Err(refuse());
        }
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        if fields.len() < 10 {
            return Err(refuse());
        }
        if fields[0].parse::<u64>().map_err(|_| refuse())? != id {
            continue;
        }
        let split = fields.iter().position(|v| *v == "-").ok_or_else(refuse)?;
        if found
            || fields[3] != "/"
            || split < 6
            || fields.len() != split + 4
            || fields[split + 1] != "cgroup2"
        {
            return Err(refuse());
        }
        found = true;
    }
    if found { Ok(()) } else { Err(refuse()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_cgroup_namespace_vectors() {
        for release in [
            "6.8.0-1018-azure",
            "6.12.23",
            "6.14.0-1014-azure",
            "6.17.0-1022-azure",
        ] {
            kernel_release(release.as_bytes()).unwrap();
        }
        for release in [
            "6.8", "6.8.x", "6.1.0", "6.13.0", "6.15.0", "6.16.0", "6.18.0", "7.0.0", "6.080.0",
        ] {
            assert!(kernel_release(release.as_bytes()).is_err());
        }
        namespace_identity(NamespaceKind::User, 0x6e73_6673, 0xEFFF_FFFD, 0x1000_0000).unwrap();
        namespace_identity(NamespaceKind::Cgroup, 0x6e73_6673, 0xEFFF_FFFB, 0x0200_0000).unwrap();
        // '/' may hide ancestors in a nested namespace, and UID0 is relative.
        full_mount(b"41 23 0:30 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n", 41).unwrap();
        for (kind, inode, ty) in [
            (NamespaceKind::User, 0xF000_0000, 0x1000_0000),
            (NamespaceKind::Cgroup, 0xF000_0001, 0x0200_0000),
            (NamespaceKind::User, 0xEFFF_FFFD, 0x0200_0000),
            (NamespaceKind::Cgroup, 0xEFFF_FFFD, 0x1000_0000),
        ] {
            assert!(namespace_identity(kind, 0x6e73_6673, inode, ty).is_err());
        }
        assert!(namespace_identity(NamespaceKind::User, 0x9fa0, 0xEFFF_FFFD, 0x1000_0000).is_err());
        assert!(namespace_identity(NamespaceKind::Cgroup, 0x6e73_6673, 0, 0x0200_0000).is_err());
    }
    #[test]
    fn native_cgroup_admission_vectors() {
        assert!(!populated(b"populated 0\nfrozen 0\n").unwrap());
        assert!(populated(b"frozen 0\npopulated 1\n").unwrap());
        for input in [
            b"".as_slice(),
            b"populated 2",
            b"populated 00",
            b"populated 0\npopulated 1",
            b"frozen 0",
            b"populated 0 extra",
            b"populated -1",
            b"populated 0\n\xff",
        ] {
            assert!(populated(input).is_err());
        }
        assert!(populated(&[b' '; 4097]).is_err());
        let host = "Uid:\t1000 1000 1000 1000\nCapInh:\t0000000000000000\nCapPrm:\t0000000000000000\nCapEff:\t0000000000000000\nCapBnd:\t0000000000000000\nCapAmb:\t0000000000000000\nNoNewPrivs:\t1\n";
        host_status(host.as_bytes()).unwrap();
        for key in ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"] {
            assert!(
                host_status(
                    host.replace(
                        &format!("{key}:\t0000000000000000"),
                        &format!("{key}:\t0000000000000001")
                    )
                    .as_bytes()
                )
                .is_err()
            );
            assert!(
                host_status(
                    host.replace(&format!("{key}:\t0000000000000000\n"), "")
                        .as_bytes()
                )
                .is_err()
            );
        }
        for bad in [
            host.replace("1000", "0"),
            host.replace("1000 1000 1000 1000", "1000 1000 0 1000"),
            host.replace("NoNewPrivs:\t1", "NoNewPrivs:\t0"),
            format!("{host}CapEff:\t0000000000000000\n"),
        ] {
            assert!(host_status(bad.as_bytes()).is_err());
        }
        let mount = b"41 23 0:30 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n";
        full_mount(mount, 41).unwrap();
        for bad in [
            "41 23 0:30 /delegated /sys/fs/cgroup rw - cgroup2 cgroup rw",
            "41 23 0:30 / /sys/fs/cgroup rw - tmpfs none rw",
            "bad",
            "41 23 0:30 / /sys/fs/cgroup rw - cgroup2",
        ] {
            assert!(full_mount(bad.as_bytes(), 41).is_err());
        }
        assert!(full_mount(mount, 42).is_err());
        assert!(full_mount(&mount.repeat(2), 41).is_err());
    }
}
