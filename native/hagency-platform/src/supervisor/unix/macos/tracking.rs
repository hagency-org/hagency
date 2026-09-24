//! Pure ancestry bookkeeping. No process can turn an asserted PID into authority.
use super::native::Snapshot;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};
/// The tracker's fixed refusal categories. Carried as the `io::Error` payload so
/// the guardian names one to its host instead of matching on a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// A bookkeeping bound, or a row that breaks an identity invariant.
    Gap,
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Gap => "descendant tracking bound or identity invariant broken",
        })
    }
}
impl std::error::Error for Refusal {}

/// Above this many remembered births a successful census forgets the foreign
/// ones it can no longer need. The hard 65536 bound below stays as the backstop.
const PRUNE_ABOVE: usize = 8192;

pub(super) struct Tracking {
    known: BTreeMap<u64, bool>,
    owned: BTreeMap<u64, Snapshot>,
    /// Births present in the previous successful census. A newcomer's parent was
    /// alive when it forked, so a parent that has already exited was still in
    /// that census; this is the whole window an ancestry lookup can need.
    previous: BTreeSet<u64>,
    /// The leader's own coalition, read while it was still suspended. Zeros mean
    /// the kernel named none, and then coalition evidence classifies nothing.
    leader_coalition: [u64; 2],
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
            previous: baseline.iter().map(|s| s.birth).collect(),
            leader_coalition: root.coalition,
            init: baseline.iter().find(|s| s.pid == 1).copied(),
            failed: false,
        }
    }
    #[cfg(test)]
    pub fn known_len(&self) -> usize {
        self.known.len()
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
            // Session before group: a group lies inside exactly one session, so
            // session evidence is sound for the same reason and is present far
            // more often. Every spawner that changes only the process group
            // (setpgid, Rust's process_group, shell job control) leaves its
            // survivor in a session that still holds classified members, while
            // its group may hold nobody at all.
            if pending.len() == before
                && !self.classify_by(rows, &mut pending, |s| s.session)
                && !self.classify_by(rows, &mut pending, |s| s.group)
                && !self.classify_foreign_coalition(&mut pending)
            {
                // No evidence reaches what is left. The retained tracker
                // (`owned-process-tree.ts`) tracks only what it can prove is its
                // own and ignores every other process on the machine; this one
                // used to refuse instead, and because the census is whole-system
                // that stopped a healthy running agent for an hourly browser
                // updater, for an operator's `ssh`, and for the OTHER agent's
                // shell command (live 2026-09-20, three soaks). A process nobody
                // can place is not proven owned, so it is not owned: it is never
                // signalled, and it never ends an observation. Every evidence
                // rule above only ever ADOPTS a straggler this rule would miss.
                // The cost is the retained product's own: an owned process that
                // daemonizes through a parent no census saw escapes cleanup.
                for unplaced in pending.drain(..) {
                    self.known.insert(unplaced.birth, false);
                }
                break;
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
        self.prune(rows, births);
        Ok(live)
    }
    /// `known` otherwise grows by one entry for every process ever created on
    /// the host and reaches the hard 65536 bound in about two hours of ordinary
    /// machine use, failing every live guardian within one census of each other.
    /// Forget only foreign births that nothing can still need: absent from this
    /// census, absent from the previous one, and named as no current row's
    /// parent.
    ///
    /// Soundness: an owned birth is never forgotten, so no owned process can be
    /// reclassified and nothing here ever writes an owned verdict. Forgetting a
    /// dead FOREIGN birth can only remove the ancestry that would have proved a
    /// later newcomer foreign, which demotes that newcomer to unclassified —
    /// session or group evidence, else the existing refusal. It can never turn
    /// an unrelated process into an owned one.
    ///
    /// The window is sufficient: a newcomer's parent was alive when it forked,
    /// so either no census ever saw the parent (already the unseen-parent case,
    /// untouched here) or the last census that saw it is the previous one, since
    /// every live process appears in every census. `previous` only advances on a
    /// successful update, so a refused or failed census never shifts it.
    fn prune(&mut self, rows: &[Snapshot], current: BTreeSet<u64>) {
        if self.known.len() > PRUNE_ABOVE {
            let previous = std::mem::take(&mut self.previous);
            let parents = rows.iter().map(|s| s.parent_birth).collect::<BTreeSet<_>>();
            self.known.retain(|birth, own| {
                *own || current.contains(birth)
                    || previous.contains(birth)
                    || parents.contains(birth)
            });
        }
        self.previous = current;
    }
    /// Ancestry has stalled: each pending newcomer's parent exited before any
    /// census saw it. That happens constantly on a working machine and says
    /// nothing about the owned tree, so look for evidence that cannot be forged.
    ///
    /// A process group lives inside exactly one session. A process enters a
    /// session only by being forked inside it or by creating it, and `setpgid`
    /// never crosses a session. The owned leader starts its own session, so
    /// every group — and every session — is wholly owned (the leader's session,
    /// or one a descendant created) or wholly unrelated. A newcomer sharing that
    /// scope with a process already classified in this same census therefore has
    /// that classification. A scope with no classified member, or with
    /// conflicting members, proves nothing, and the newcomer stays unconfirmed.
    /// XNU's fork allocation skips a PID while it still names a live process,
    /// group or session, so neither id can be recycled under a census. Zero is
    /// never a scope and classifies nothing.
    fn classify_by(
        &mut self,
        rows: &[Snapshot],
        pending: &mut Vec<&Snapshot>,
        scope: impl Fn(&Snapshot) -> i32,
    ) -> bool {
        let mut scopes: BTreeMap<i32, Option<bool>> = BTreeMap::new();
        for row in rows.iter().filter(|row| scope(row) > 0) {
            if let Some(&own) = self.known.get(&row.birth) {
                scopes
                    .entry(scope(row))
                    .and_modify(|verdict| {
                        if *verdict != Some(own) {
                            *verdict = None;
                        }
                    })
                    .or_insert(Some(own));
            }
        }
        let before = pending.len();
        pending.retain(|s| match scopes.get(&scope(s)).copied().flatten() {
            Some(own) => {
                self.known.insert(s.birth, own);
                if own {
                    self.owned.insert(s.birth, **s);
                }
                false
            }
            None => true,
        });
        pending.len() != before
    }
}
impl Tracking {
    /// Last resort, and negative evidence only. Live on 2026-09-20 an hourly
    /// launchd job (a browser updater) started four daemons, each through a
    /// middle that opened its own session and exited before any census: no
    /// ancestry, no session mate, no group mate, and every guardian on the host
    /// refused at once. A coalition is inherited across fork, exec and `setsid`,
    /// and leaving one takes a spawn attribute only launchd may use; whatever
    /// launchd starts on a descendant's behalf is launchd's child and was never a
    /// descendant by ancestry either. So a newcomer whose resource AND jetsam
    /// coalitions both differ from the leader's cannot descend from it.
    ///
    /// This can never classify a process as owned: the leader shares its
    /// coalition with the service, its terminal and everything else started
    /// there, so an equal coalition proves nothing and that newcomer still
    /// refuses. A zero on either side is no evidence. Both ids must differ, so a
    /// kernel that ever split only one of them would weaken nothing.
    fn classify_foreign_coalition(&mut self, pending: &mut Vec<&Snapshot>) -> bool {
        let leader = self.leader_coalition;
        if leader.contains(&0) {
            return false;
        }
        let before = pending.len();
        pending.retain(|s| {
            let foreign = !s.coalition.contains(&0)
                && s.coalition[0] != leader[0]
                && s.coalition[1] != leader[1];
            if foreign {
                self.known.insert(s.birth, false);
            }
            !foreign
        });
        pending.len() != before
    }
}
fn gap() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, Refusal::Gap)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(pid: i32, birth: u64, parent: u64) -> Snapshot {
        Snapshot {
            pid,
            session: pid,
            group: pid,
            // Every fixture row shares the leader's coalition unless a test says
            // otherwise, so an equal coalition keeps proving nothing.
            coalition: [7, 8],
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
        // A process nobody can place is not proven owned, so it is not owned
        // (the retained tracker's rule): it joins no tree, its own children are
        // unrelated by ancestry, and it never ends the observation.
        let unknown = row(50, 500, 999);
        assert!(t.update(&[unknown]).unwrap().is_empty());
        assert!(t.update(&[unknown, row(51, 510, 500)]).unwrap().is_empty());
        assert_eq!(t.owned().len(), 3);
        assert!(!t.failed());
        assert!(t.update(&[foreign]).unwrap().is_empty());
        assert!(!t.failed());
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
        // Launchd adoption proves nothing either way, so this orphan is simply
        // not owned; it is not taken for a direct launchd child and not adopted.
        assert!(t.update(&[orphan]).unwrap().is_empty());
        assert_eq!(t.owned().len(), 1);
        assert!(!t.failed());
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
    #[test]
    fn native_macos_group_evidence_classifies_unseen_parent() {
        let root = row(10, 100, 1);
        let foreign = row(20, 200, 1);
        let grouped = |pid, birth, parent, group| Snapshot {
            group,
            ..row(pid, birth, parent)
        };
        // An unrelated survivor of a parent no census saw, in a known foreign
        // group: foreign, and its own children then resolve by ancestry.
        let mut t = Tracking::new(&[root, foreign], root);
        let survivor = grouped(50, 500, 999, 20);
        let child = row(51, 510, 500);
        assert_eq!(
            t.update(&[root, foreign, survivor, child]).unwrap().len(),
            1
        );
        assert!(!t.failed());
        assert_eq!(t.owned().len(), 1);
        // The same shape inside the owned leader's group is owned and live.
        let escaped = grouped(60, 600, 998, 10);
        assert_eq!(
            t.update(&[root, foreign, survivor, escaped]).unwrap().len(),
            2
        );
        assert_eq!(t.owned().len(), 2);
        assert!(!t.failed());
        // Neither a group nor a session with no classified member proves
        // anything: this newcomer is alone in both, so it is not adopted, and it
        // ends nothing.
        let mut t = Tracking::new(&[root, foreign], root);
        assert_eq!(
            t.update(&[root, foreign, grouped(70, 700, 997, 70)])
                .unwrap()
                .len(),
            1
        );
        assert!(t.owned().len() == 1 && !t.failed());
        // Evidence must be present in the same census, not remembered.
        let mut t = Tracking::new(&[root, foreign], root);
        assert_eq!(
            t.update(&[root, grouped(50, 500, 999, 20)]).unwrap().len(),
            1
        );
        // Conflicting members prove nothing, so nothing is adopted on them.
        let mut t = Tracking::new(&[root, foreign], root);
        let mixed = grouped(20, 200, 1, 10);
        assert_eq!(
            t.update(&[root, mixed, grouped(80, 800, 996, 10)])
                .unwrap()
                .len(),
            1
        );
        assert!(t.owned().len() == 1 && !t.failed());
    }
    #[test]
    fn native_macos_session_evidence_classifies_detached_group() {
        let root = row(10, 100, 1);
        let foreign = row(20, 200, 1);
        let scoped = |pid, birth, parent, session, group| Snapshot {
            session,
            group,
            ..row(pid, birth, parent)
        };
        // The production shape: an unrelated spawner puts a shell in its own
        // process group, the shell forks and exits before any census, and the
        // survivor is the only member of that group. Its session still holds a
        // classified member, so it is unrelated and observation continues.
        let mut t = Tracking::new(&[root, foreign], root);
        let survivor = scoped(50, 500, 999, 20, 49);
        assert_eq!(t.update(&[root, foreign, survivor]).unwrap().len(), 1);
        assert!(!t.failed());
        assert_eq!(t.owned().len(), 1);
        // The same shape inside the leader's session is owned and live, even
        // though the leader's group holds nobody but the leader.
        let escaped = scoped(60, 600, 998, 10, 59);
        assert_eq!(
            t.update(&[root, foreign, survivor, escaped]).unwrap().len(),
            2
        );
        assert_eq!(t.owned().len(), 2);
        // A survivor that entered a session of its own through an unseen parent
        // has neither session nor group evidence: it is not adopted.
        let mut t = Tracking::new(&[root, foreign], root);
        assert_eq!(
            t.update(&[root, foreign, scoped(70, 700, 997, 70, 70)])
                .unwrap()
                .len(),
            1
        );
        assert!(t.owned().len() == 1 && !t.failed());
        // A refused session id (zero) is not evidence and adopts nothing.
        let mut t = Tracking::new(&[root, foreign], root);
        assert_eq!(
            t.update(&[root, scoped(20, 200, 1, 0, 20), scoped(90, 900, 995, 0, 89)])
                .unwrap()
                .len(),
            1
        );
        assert!(t.owned().len() == 1 && !t.failed());
    }
    #[test]
    fn native_macos_coalition_evidence_is_negative_only() {
        let root = row(10, 100, 1);
        let foreign = row(20, 200, 1);
        // The live shape: a daemon alone in a session of its own, its parent
        // never in any census, started by launchd in a coalition of its own.
        let daemon = |pid, birth, coalition| Snapshot {
            coalition,
            ..row(pid, birth, 990 + birth)
        };
        let mut t = Tracking::new(&[root, foreign], root);
        assert_eq!(
            t.update(&[root, foreign, daemon(70, 700, [31, 32])])
                .unwrap()
                .len(),
            1
        );
        assert!(!t.failed());
        assert_eq!(
            t.owned().len(),
            1,
            "coalition evidence never owns a process"
        );
        // A child that daemon forks later is unrelated by plain ancestry.
        let child = Snapshot {
            coalition: [31, 32],
            ..row(71, 710, 700)
        };
        assert_eq!(
            t.update(&[root, foreign, daemon(70, 700, [31, 32]), child])
                .unwrap()
                .len(),
            1
        );
        // An equal coalition, one the kernel would not name, or one that differs
        // in only one id proves nothing. Whatever the coalition says, the daemon
        // is never adopted and never ends the observation.
        for coalition in [[7, 8], [0, 0], [31, 0], [31, 8], [7, 32]] {
            let mut t = Tracking::new(&[root, foreign], root);
            assert_eq!(
                t.update(&[root, foreign, daemon(70, 700, coalition)])
                    .unwrap()
                    .len(),
                1,
                "{coalition:?}"
            );
            assert!(t.owned().len() == 1 && !t.failed(), "{coalition:?}");
        }
        // The same under a leader whose own coalition is unknown.
        let blind = Snapshot {
            coalition: [0, 0],
            ..root
        };
        let mut t = Tracking::new(&[blind, foreign], blind);
        assert_eq!(
            t.update(&[blind, foreign, daemon(70, 700, [31, 32])])
                .unwrap()
                .len(),
            1
        );
        assert!(t.owned().len() == 1 && !t.failed());
    }
    #[test]
    fn native_macos_known_births_are_pruned_without_losing_ancestry() {
        let init = row(1, 1, 0);
        let root = row(10, 100, 1);
        let mut t = Tracking::new(&[init, root], root);
        let foreign = |birth: u64| row((birth % 20000) as i32 + 1000, birth, init.birth);
        // Unrelated churn far past the prune threshold: each census replaces the
        // whole foreign population, and the map must stay bounded.
        let mut birth = 1000u64;
        for _ in 0..16 {
            let mut rows = vec![init, root];
            for _ in 0..1024 {
                birth += 1;
                rows.push(foreign(birth));
            }
            t.update(&rows).unwrap();
        }
        assert!(!t.failed());
        assert!(
            t.known_len() <= PRUNE_ABOVE + 1200,
            "known grew unbounded: {}",
            t.known_len()
        );
        // An owned birth is never forgotten: a late child of the leader is still
        // owned after all that churn, even though no census kept the leader's
        // ancestors alive.
        let owned_child = row(4242, birth + 1, root.birth);
        assert_eq!(t.update(&[init, root, owned_child]).unwrap().len(), 2);
        assert!(t.owned().len() == 2 && !t.failed());
        // A foreign parent seen in one census and gone in the next still
        // classifies the child it left behind.
        let parent = foreign(birth + 2);
        t.update(&[init, root, parent]).unwrap();
        let orphan = row(4243, birth + 3, parent.birth);
        assert!(t.update(&[init, root, orphan]).unwrap().len() == 1);
        assert!(!t.failed());
    }
}
