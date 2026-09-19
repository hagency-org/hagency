//! Read-only bounded inventory, never semantic replay or release authority.
use super::{Binding, Root};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

const ENTRIES: usize = 1024;
const BYTES: u64 = 64 * 1024 * 1024;
struct Budget {
    until: Instant,
    entries: usize,
    bytes: u64,
}
impl Budget {
    fn check(&self) -> Result<(), ()> {
        if Instant::now() < self.until {
            Ok(())
        } else {
            Err(())
        }
    }
}
fn scan(
    dir: &Dir,
    path: &Path,
    depth: usize,
    budget: &mut Budget,
    items: &mut Vec<Value>,
) -> Result<(), ()> {
    budget.check()?;
    if depth > 32 {
        return Err(());
    }
    let mut names = Vec::new();
    for entry in dir.entries().map_err(|_| ())? {
        budget.check()?;
        if names.len() >= budget.entries {
            return Err(());
        }
        names.push(entry.map_err(|_| ())?.file_name());
    }
    names.sort();
    for name in names {
        budget.check()?;
        budget.entries = budget.entries.checked_sub(1).ok_or(())?;
        let relative = path.join(&name);
        let label = relative.to_str().ok_or(())?;
        if label.len() > 4096 {
            return Err(());
        }
        let metadata = dir.symlink_metadata(&name).map_err(|_| ())?;
        if metadata.is_symlink() {
            let target = dir.read_link_contents(&name).map_err(|_| ())?;
            let target = target.to_str().ok_or(())?;
            if target.len() > 4096 {
                return Err(());
            }
            items.push(json!({"path":label,"kind":"symlink","target":target}));
        } else if metadata.is_dir() {
            let child = dir.open_dir_nofollow(&name).map_err(|_| ())?;
            items.push(json!({"path":label,"kind":"directory","readonly":metadata.permissions().readonly()}));
            scan(&child, &relative, depth + 1, budget, items)?;
        } else if metadata.is_file() {
            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No).nonblock(true);
            let mut file = dir.open_with(&name, &options).map_err(|_| ())?;
            let before = file.metadata().map_err(|_| ())?;
            if !before.is_file() || before.len() > budget.bytes {
                return Err(());
            }
            let mut digest = Sha256::new();
            let mut size = 0u64;
            let mut chunk = [0u8; 64 * 1024];
            loop {
                budget.check()?;
                let n = file.read(&mut chunk).map_err(|_| ())?;
                if n == 0 {
                    break;
                }
                size = size.checked_add(n as u64).ok_or(())?;
                budget.bytes = budget.bytes.checked_sub(n as u64).ok_or(())?;
                digest.update(&chunk[..n]);
            }
            let after = file.metadata().map_err(|_| ())?;
            if size != before.len()
                || before.len() != after.len()
                || before.modified().ok() != after.modified().ok()
            {
                return Err(());
            }
            items.push(json!({"path":label,"kind":"file","bytes":size,"sha256":format!("{:x}",digest.finalize()),"readonly":after.permissions().readonly()}));
        } else {
            return Err(());
        }
    }
    Ok(())
}
impl Root {
    fn inventory(&self, until: Instant) -> Result<Value, ()> {
        self.check().map_err(|_| ())?;
        let dir = Dir::from_std_file(self.file.try_clone().map_err(|_| ())?);
        let mut entries = Vec::new();
        scan(
            &dir,
            Path::new(""),
            0,
            &mut Budget {
                until,
                entries: ENTRIES,
                bytes: BYTES,
            },
            &mut entries,
        )?;
        self.check().map_err(|_| ())?;
        let value = json!({"profile":"stopped-content-inventory-v1","root":hagency_platform::directory_identity(&self.file).map_err(|_|())?,"entries":entries});
        if serde_json::to_vec(&value).map_err(|_| ())?.len() > 1024 * 1024 {
            return Err(());
        }
        Ok(value)
    }
    fn inspect_stopped(&self) -> Result<Value, ()> {
        let until = Instant::now() + Duration::from_secs(2);
        let first = self.inventory(until)?;
        let second = self.inventory(until)?;
        if first != second {
            return Err(());
        }
        Ok(first)
    }
}
impl Binding {
    /// The original worker calls only AFTER full process stop. Retired task
    /// access stays retired; this method exposes no fresh workspace authority.
    pub(crate) fn inspect_stopped(&self) -> Result<Value, ()> {
        self.root.inspect_stopped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_stopped_workspace_inventory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work");
        hagency_store::private::directory(&path).unwrap();
        let path = path.canonicalize().unwrap();
        std::fs::create_dir(path.join("nested")).unwrap();
        std::fs::write(path.join("nested/file"), b"exact bytes").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/not-an-inspection-target", path.join("outside")).unwrap();
        let root = Root::open(path.clone()).unwrap();
        let first = root.inspect_stopped().unwrap();
        assert_eq!(
            first["entries"][1]["sha256"],
            format!("{:x}", Sha256::digest(b"exact bytes"))
        );
        #[cfg(unix)]
        assert_eq!(first["entries"][2]["target"], "/not-an-inspection-target");
        std::fs::write(path.join("nested/file"), b"changed").unwrap();
        assert_ne!(first, root.inspect_stopped().unwrap());
        let dir = Dir::from_std_file(root.file.try_clone().unwrap());
        for (entries, bytes) in [(0, BYTES), (ENTRIES, 0)] {
            assert!(
                scan(
                    &dir,
                    Path::new(""),
                    0,
                    &mut Budget {
                        until: Instant::now() + Duration::from_secs(1),
                        entries,
                        bytes
                    },
                    &mut Vec::new()
                )
                .is_err()
            );
        }
        assert!(root.inventory(Instant::now()).is_err());
        #[cfg(unix)]
        {
            let socket =
                std::os::unix::net::UnixListener::bind(path.join("unsupported.socket")).unwrap();
            assert!(
                root.inspect_stopped().is_err(),
                "a socket is not content inventory"
            );
            drop(socket);
            std::fs::remove_file(path.join("unsupported.socket")).unwrap();
        }
        std::fs::rename(&path, temp.path().join("original")).unwrap();
        hagency_store::private::directory(&path).unwrap();
        assert!(
            root.inspect_stopped().is_err(),
            "replacement path is not the retained root"
        );
    }
}
