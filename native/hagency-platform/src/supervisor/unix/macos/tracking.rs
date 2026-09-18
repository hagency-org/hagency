//! Pure ancestry bookkeeping. No process can turn an asserted PID into authority.
use super::native::Snapshot;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};
pub(super) struct Tracking {
    known: BTreeMap<u64, bool>,
    owned: BTreeMap<u64, Snapshot>,
    init: Option<Snapshot>,
    failed: bool,
}
impl Tracking {
    pub fn new(baseline: &[Snapshot], root: Snapshot) -> Self {
        let mut known = baseline
            .iter()
            .map(|s| (s.birth, false))
            .collect::<BTreeMap<_, _>>();
        known.insert(root.birth, true);
        Self {
            known,
            owned: BTreeMap::from([(root.birth, root)]),
            init: baseline.iter().find(|s| s.pid == 1).copied(),
            failed: false,
        }
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn fail(&mut self) {
        self.failed = true;
    }
    pub fn owned(&self) -> Vec<Snapshot> {
        self.owned.values().copied().collect()
    }
    pub fn update(&mut self, rows: &[Snapshot]) -> io::Result<Vec<Snapshot>> {
        let result = self.update_inner(rows);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn update_inner(&mut self, rows: &[Snapshot]) -> io::Result<Vec<Snapshot>> {
        if rows.len() > 32768 || self.known.len() > 65536 {
            return Err(gap());
        }
        let mut births = BTreeSet::new();
        let mut pids = BTreeSet::new();
        if rows
            .iter()
            .any(|s| s.birth == 0 || s.pid <= 0 || !births.insert(s.birth) || !pids.insert(s.pid))
        {
            return Err(gap());
        }
        let mut pending = rows
            .iter()
            .filter(|s| !self.known.contains_key(&s.birth))
            .collect::<Vec<_>>();
        if self.known.len() + pending.len() > 65536 {
            return Err(gap());
        }
        loop {
            let before = pending.len();
            pending.retain(|s| {
                match self.known.get(&s.parent_birth).copied() {
                    // Reparenting alone preserves the native parent birth. Exec
                    // after adoption can replace it with init's, which is not
                    // sufficient evidence alone. The original parent audit
                    // version survives exec: direct launchd children match it,
                    // while an adopted-and-exec'd child retains its real parent's.
                    Some(own)
                        if own
                            || s.parent_pid > 1
                            || self.init.is_some_and(|init| {
                                s.parent_birth != init.birth
                                    || s.original_parent_version == init.version
                            }) =>
                    {
                        self.known.insert(s.birth, own);
                        if own {
                            self.owned.insert(s.birth, **s);
                        }
                        false
                    }
                    _ => true,
                }
            });
            if pending.is_empty() {
                break;
            }
            if pending.len() == before {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    if pending.iter().any(|s| s.parent_pid <= 1) {
                        "new orphan ancestry is unconfirmed"
                    } else {
                        "missing parent ancestry is unconfirmed"
                    },
                ));
            }
        }
        let mut live = Vec::new();
        for &row in rows {
            if self.known.get(&row.birth) == Some(&true) {
                if self
                    .owned
                    .get(&row.birth)
                    .is_none_or(|old| old.pid != row.pid)
                {
                    return Err(gap());
                }
                self.owned.insert(row.birth, row);
                if row.status != 5 {
                    live.push(row);
                }
            }
        }
        Ok(live)
    }
}
fn gap() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "descendant ancestry is unconfirmed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(pid: i32, birth: u64, parent: u64) -> Snapshot {
        Snapshot {
            pid,
            birth,
            parent_birth: parent,
            parent_pid: 10,
            version: birth as u32,
            original_parent_version: parent as u32,
            status: 2,
        }
    }
    #[test]
    fn native_macos_descendant_tracking() {
        let root = row(10, 100, 1);
        let foreign = row(20, 200, 1);
        let child = row(30, 300, 100);
        let leaf = row(40, 400, 300);
        let mut t = Tracking::new(&[root, foreign], root);
        assert_eq!(t.update(&[leaf, foreign, child, root]).unwrap().len(), 3);
        let mut reparented = leaf;
        reparented.parent_pid = 1;
        assert_eq!(t.update(&[foreign, reparented]).unwrap().len(), 1);
        assert!(
            t.update(&[row(30, 301, 200)]).unwrap().is_empty(),
            "reused child PID is foreign"
        );
        assert_eq!(t.owned().len(), 3);
        assert!(!t.failed());
        let unknown = row(50, 500, 999);
        assert!(t.update(&[unknown]).is_err());
        assert!(t.failed());
        assert!(t.update(&[foreign]).unwrap().is_empty());
        assert!(t.failed());
        let init = row(1, 1, 0);
        let mut t = Tracking::new(&[root, foreign, init], root);
        let mut orphan = row(50, 501, 200);
        orphan.parent_pid = 1;
        assert!(
            t.update(&[orphan]).unwrap().is_empty(),
            "known foreign parent remains foreign after reparenting"
        );
        orphan.birth = 502;
        orphan.parent_birth = init.birth;
        assert!(
            t.update(&[orphan]).is_err(),
            "new orphan is not proved foreign by launchd adoption"
        );
        let mut t = Tracking::new(&[root, foreign, init], root);
        orphan.original_parent_version = init.version;
        assert!(
            t.update(&[orphan]).unwrap().is_empty(),
            "direct launchd child has its original parent version"
        );
        let mut t = Tracking::new(&[root], root);
        let mut recycled = root;
        recycled.pid = 99;
        assert!(t.update(&[recycled]).is_err());
        let mut t = Tracking::new(&[root], root);
        assert!(t.update(&[root, root]).is_err());
        t.fail();
        assert!(t.failed());
    }
}
