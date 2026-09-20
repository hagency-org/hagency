---
kind: decision
id: ADR-029
title: Establish native process scope before execution and report its actual guarantee
status: Accepted
---

## Context

Native cancellation must identify the process scope it actually owns, including startup failures and platform-specific limits on descendant recovery.

## Decision

Implements the early platform proof required by REQ-RUST-MIGRATION-EXECUTION and
the M1/M4 migration gates. The initial platform crate remains separate from actual
Agent dispatch; it does not establish sandbox or full runtime parity.

Windows uses a non-inheritable, unnamed Job Object with kill-on-close and no
breakaway permission. `PROC_THREAD_ATTRIBUTE_JOB_LIST` associates the job during
`CreateProcessW`, before child code executes. This avoids the suspended-but-not-yet-
assigned crash window in the traditional three-call approach. See Microsoft's
[process-in-job explanation](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812)
and [Job Object documentation](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
Only retained handles authorize cancellation. Whole-tree stop requires the job's
active process count to reach zero and its retained leader handle to signal exit.

POSIX uses Rust's [process_group](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#method.process_group)
before exec. The host exclusively owns the child reaper; it must not install an
automatic SIGCHLD reaper or let another waitpid consumer reap this child. Keep the
leader unreaped until the final group/child signals, then reap it without sending
more signals by its numeric ID. This protects the group's identity from reuse.
Group cancellation alone never reports full descendant cleanup. Detached children
and owner-death guardians remain mandatory work before real POSIX Agent execution.
Requests requiring crash containment currently fail before POSIX spawn.

Launch configuration is host-only, explicit and bounded: absolute executable/cwd,
argument array, allowlisted environment, null/inactive console IO. There is no
runtime PID-to-authority conversion, shell wrapper or implicit environment copy.
The Windows FFI boundary owns buffers and handles, and retains the job handle array
until process creation finishes. Windows argv uses standard CRT quote/backslash
encoding; raw command-line parsing is not an Agent-facing API.

A Job Object is not filesystem/network sandboxing. Neither successful process
creation nor a leader exit completes a canonical task. Future runner integration
must separately prove current dispatch permission, actual sandbox policy, full
descendant handling, bounded stdio and recovery ownership.

Child signal identity is a separate primitive. `OwnedChildIdentity::capture`
requires a host-owned `Child`, and there is no public numeric-PID constructor.
Read-only `(pid, birth)` metadata can narrow an existing handle's target but cannot
construct or retarget authority. The host must continue to exclusively own reaping.

Linux uses [pidfd signalling](https://man7.org/linux/man-pages/man2/pidfd_send_signal.2.html)
through a retained descriptor. Windows duplicates the Child's existing process
handle; [process handles remain valid until closed](https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers),
including after exit. macOS reads the BSD/unique snapshot atomically and verifies
its lifetime identifier before refreshing the current audit-token PID version.
`proc_signal_with_audittoken` asks the kernel to check that version at signal time.
See Apple's [libproc wrapper](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.c)
and [native identity ABI](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info_private.h).
No backend falls back to pid-only signalling when its identity guard fails.

The API reports `Sent` or `NoLongerCurrent`; neither is a task-completion or full
descendant-cleanup receipt. A concurrent macOS exec can invalidate a token between
observation and signal, so callers must observe again rather than infer that all
work stopped. Birth metadata is native-only and not a JSON authority DTO (Windows
FILETIME exceeds JavaScript's safe integer range). Descendant adoption must prove
ancestry before it can construct equivalent internal signal authority.

Unix supervision now has a native `hagency guardian` entrypoint. The controlling
host supplies an anonymous Unix socket as stdin; there is no listening address,
state repository or runtime-token parser. Prepare/version validation precedes a
separate Start message. Launch data preserves native argv/environment values and
has bounded size. Partial frames expire; EOF, malformed commands and leader exit
trigger scope cancellation. A timed-out owner closes its endpoint rather than
killing the guardian and assuming work stopped. Windows uses the existing owned
Job Object directly, including kernel cleanup when its owner exits without Drop.

The guardian duplicates its socket atomically with CLOEXEC and gives work null
stdio. The controlled fixture checks that no socket survives into the work
process. A descriptor capable of writing guardian replies must never reach a
runner. This channel is process custody, not Matrix or runner authorization.

Native observation preserves the unreaped group anchor. On macOS, signalling a
group containing only a zombie can return EPERM (reproduced with a real native
child and consistent with XNU's group-iteration zombie filter). Cancellation must
still attempt the owned child and reap it. `signals_accepted` preserves a failed
signal in the report; an error is not proof that a group is empty. Whole-tree stop
remains false for POSIX. Detached-descendant discovery, guardian-loss recovery and
effective sandbox enforcement remain mandatory before advertising real runners.

Guardian CI exposed additional inheritable sockets supplied by its embedding host.
Marking only our socket CLOEXEC is insufficient: every extra host descriptor must
be sealed in the child after fork, both at guardian startup and at work startup.
The callback keeps descriptors 0–2 and marks the rest CLOEXEC rather than closing
them immediately, preserving Rust's internal exec-error reporting pipe.

Linux uses the direct [close_range CLOEXEC syscall](https://man7.org/linux/man-pages/man2/close_range.2.html),
requiring kernel 5.11 or newer for this launch path; unsupported kernels refuse
launch. macOS queries its own post-fork descriptor table through PROC_PIDLISTFDS
into fixed stack storage, checks completeness, then applies fcntl FD_CLOEXEC. The
libproc wrapper is a direct syscall, with no allocator or lock in the callback.
A table reaching the 4096-record bound refuses before exec. This avoids a racy
parent census and avoids guessing that the current soft FD limit bounds existing
descriptors. The already locked libc dependency supplies the platform ABI.

Linux guardian cleanup now uses [subreaper adoption](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html)
before work starts. Startup requires one guardian thread and no pre-existing
children; the CLI dispatches guardian mode before constructing Tokio. The guardian
restores normal SIGCHLD disposition so an embedding host cannot cause automatic
reaping. It checks pidfd wait support before acknowledging preparation.

The kernel adopts orphaned descendants, including double-forked processes and
processes that create a new session. The bounded proc children list only supplies
candidate IDs: the [interface can omit live children during concurrent exit](https://man7.org/linux/man-pages/man5/proc_tid_children.5.html).
Each candidate becomes an owned pidfd, then P_PIDFD waitability must confirm that
the same process is this guardian's child before signalling or reaping it. There
is no numeric-PID signal fallback. The root's std Child keeps its exclusive reaper
until its final group/individual signal attempts and confirmed exit.

A full Linux cleanup report requires the reaped root plus kernel ECHILD from
[waitid](https://man7.org/linux/man-pages/man2/waitpid.2.html) using __WALL to include
clone children with non-SIGCHLD exit signals. An empty census cannot supply that
proof. Discovery is bounded and repeated; errors or deadline expiry preserve an
unknown outcome. This is an observation after cleanup, not a promise that cleanup
will always succeed. Full requested POSIX crash containment still refuses while
guardian-death recovery remains open. macOS retains group-only reporting and its
explicit refusal of unsupported descendant custody. Windows retains Job Objects.

The guardian CLI fixture now launches the actual native `--version` command in a
Unicode cwd with empty PATH, through the same owned piped guardian/Job path. It
keeps the five-second terminal-report deadline and exact LeaderExited/platform
scope assertions. Since StopReport has no exit code, bounded stdout/stderr reads
also require the exact compiled version bytes and empty stderr through EOF.
Fresh Unicode token/database initialization remains in the separate native crash
and restart test; it is not a guardian responsiveness threshold.

The f4cdead combined macOS run failed when the previous fixture received no report
within five seconds during fresh initialization. The unchanged fixture later
passed in isolation (0.70 seconds). Eight bounded diagnostic launches observed
four valid version and four valid fresh-init exits; after spawn, version reports
arrived around 25–27 ms and init around 80 ms on that run. These measurements do
not establish the historical timeout's cause. The old test coupled schema/filesystem
initialization throughput to guardian exit observation without child-phase evidence.
The split retains both actual checks and improves exit evidence; it changes no
production startup, stop, identity or timeout behavior. Any later missing version
report still fails and requires investigation rather than being called a flake.

## Native macOS TS-parity amendment (2026-09-16)

The preceding group-only macOS limitation is superseded for a live guardian's
observed stop path, not for kernel crash containment. The behavior reference is
`router/src/runner-guardian.ts`, `owned-process-tree.ts` and the five existing TS
guardian tests. Native macOS now starts the actual executable with
POSIX_SPAWN_START_SUSPENDED and CLOEXEC_DEFAULT, explicit cwd/env/stdio and an
independent process group. The original guardian records the paused child's
identity and process census before SIGCONT. No shell pause wrapper or extra
guardian channel reaches the runner.

Private libproc observations bracket SHORTBSDINFO with unique-identity reads.
The complete bounded census uses the two flavors that support cross-user
observation, without collecting command lines or environments. Owned descendants
are found by native parent-birth identity and retained after reparenting. Audit
token signals revalidate original birth and current PID version; there is no
public PID-to-authority constructor. Known foreign processes never gain ownership.

Concurrent regression runs found an overly broad first implementation: treating
every new process with PPID1 as a discovery gap killed unrelated healthy runners.
XNU preserves the parent birth during reparenting, but refreshes it during exec.
Its original-parent PID version is retained across exec. The tracker therefore
distinguishes a known foreign parent, a direct init child matching init's original
version, and an unexplained adopted-and-exec'd process. The latter still refuses;
an empty later snapshot cannot repair that gap. See Apple's
[fork identity initialization](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_fork.c),
[parent insertion](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_proc.c),
and [libproc observation implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/proc_info.c).

During stop, the original owner pauses known members, refreshes descendants,
attempts TERM/CONT, escalates to KILL, and reaps only its own direct child. A
positive receipt requires two complete empty live-member censuses after leader
exit, accepted signals and no sticky discovery failure. Partial/ambiguous native
reads, missing ancestry, bounded-capacity exhaustion and deadline expiry remain
unknown. This is sampled native ancestry observation, not an adversarial process
containment or filesystem/network sandbox guarantee. macOS requests requiring
guardian-crash containment still refuse before work. Linux/Windows scopes are
unchanged. Unsupported kqueue NOTE_TRACK is not used.

Actual native fixtures cover still-parented and already-reparented detached tools,
early root exit, owner loss, inherited descriptor sealing, unrelated live progress,
sticky missing discovery and cached terminal receipts. Platform/runtime tests are
offline; actual installed-runner qualification and Palpo/Robrix remain separate.

## Session-scoped group evidence amendment (2026-09-18)

The remaining refusal was still too broad for a working machine. Every new
process anywhere on the host whose parent exited between two censuses, whether
adopted by init or not, was an unexplained ancestry, and the guardian's loop
takes a census about every 25 ms and stops the owned tree with
ObservationFailure on the first one. `sh -c '(tool &)'`, a build's helper
processes and another agent's own commands all have that shape. The live
two-agent fleet on 2026-09-18 recorded it as a warm Codex runtime losing
authority before Started: `lost_authority`, protocol `not_started`, transport
`host_closed`, cleanup `unknown`, 240 ms after the claim, after minutes of warm
idle. An idle fixture reproduces it offline: 48 quiet observations pass, and one
unrelated survivor of an unseen parent fails the second observation. The TS
reference never had this failure, because it has no gap rule at all: a process
whose visible parent is not owned is simply not owned.

The refusal stays, but only for a newcomer that nothing classifies. Two changes
make a sound classification available first.

The leader now starts with POSIX_SPAWN_SETSID instead of SETPGROUP, so it leads
its own session as well as its own group. A process enters a session only by
being forked inside it or by creating it, and `setpgid` never crosses a session.
Every process group therefore lies wholly inside the owned leader's session or a
session one of its descendants created, or wholly outside them: a group never
mixes owned and unrelated processes. XNU's fork allocation, cited above, skips a
PID while it still names a live process, group or session, so a group id cannot
be recycled under a census.

When ancestry stalls, the tracker reads the process group that SHORTBSDINFO
already returns. A pending newcomer that shares its group with a process already
classified in the same census takes that classification: owned, and then stopped
with the tree, or unrelated. A group with no classified member in that census, or
with conflicting members, proves nothing; that newcomer remains unexplained and
the sticky refusal applies as before. Evidence is never remembered across
censuses. A descendant that daemonizes through an unseen parent into a session of
its own is the case this cannot classify, and it still refuses.

This is evidence about sessions, not a new authority: signals still go only to
original lifetime identities, and crash containment on macOS still refuses.
`native_macos_group_evidence_classifies_unseen_parent` covers the rule,
`native_macos_unrelated_churn_keeps_descendant_proof` the idle owner under
unrelated churn, and `native_macos_unseen_parent_in_owned_group_is_stopped` an
owned survivor of an unseen parent. Both fixtures fail on the previous tracker.

## Session-scoped evidence, pruned bookkeeping and recorded stop causes (2026-09-19)

The 2026-09-18 group-evidence amendment narrowed the refusal but did not close
it, and its own text named the remaining case wrongly: "a descendant that
daemonizes through an unseen parent into a session of its own" describes an
owned process. The identical FOREIGN row is equally unclassifiable, is far more
common, and is what three live occurrences on 2026-09-19 were. Two agents under
one service lost their owned attempts 2-6 ms apart, both peer_unavailable with
transport peer_eof, stage update and cleanup unknown, and the service kept two
defunct children whose exit status says each guardian returned 1 from "complete
descendant cleanup is unproven". The census is whole-system, so one
unclassifiable row anywhere on the host is refused by every guardian at once;
the offending process need belong to no agent.

The trigger is an ordinary spawner that detaches. A process started in its own
process group -- Rust's Command::process_group, Node's detached: true, Python's
start_new_session=True, posix_spawn with SETSID, ssh -f, shell job control --
that forks a survivor and exits inside one 25 ms census interval leaves that
survivor as the only member of a group no census ever classified. Offline
reproduction: the existing idle-leaf fixture plus `sh -c '(sleep 0.4 &); exit 0'`
spawned with process_group(0) ended the observation at the second sample in five
runs out of five, while the same shell without process_group survived 35-37
samples. Two supervised trees observed beside each other both lost their owner
0.2, 3.9 and 5.1 ms apart from a single shell exit.

Ancestry now stalls into session evidence before group evidence. The reasoning
is the one already made here for groups, one level coarser: a process enters a
session only by being forked inside it or by creating it, setpgid never crosses
a session, and the leader starts its own session, so every session is wholly
owned or wholly unrelated. A pending newcomer sharing its session with a process
classified in that same census takes that classification; a session with no
classified member, or with conflicting members, proves nothing, and group
evidence is tried next. XNU's fork allocation skips a PID while it still names a
live process, group or session, so neither id is recycled under a census. The
session is read with getsid inside the existing identity bracket. A getsid
refusal other than ESRCH records zero, which classifies nothing: a kernel
refusal on one unrelated row must never end a census. ESRCH keeps meaning gone,
and a zero answer is never read as an error.

Birth ordering was considered and rejected for this slice. XNU's p_uniqueid is
monotone, so an owned row's parent birth is never below the leader's, but the
field is replaced with init's during exec after adoption, and the guards that
make the rule sound -- parent_pid above 1 and a present init row -- leave it
covering almost nothing that occurs. It is not implemented.

The refusal itself is unchanged and stays fatal. A survivor whose unseen parent
opened a session of its own has neither session nor group evidence; the tracker
refuses, sets its sticky failure, and denies the whole-tree receipt even though
the leader is reaped and every signal is accepted. Census data cannot decide
that case, because the process that would have decided it was never in a census.
Only a kernel fork feed could -- kqueue EVFILT_PROC with NOTE_TRACK attached to
the leader while it is still suspended -- and this ADR's refusal to rely on
NOTE_TRACK stands. The case is now pinned by a test that asserts both the
refusal and its reported cause, in its own test binary because the row it
creates ends every other guardian's observation on the same host.

`known` is pruned. It grew by one entry for every process ever created on the
host and reached the hard 65536 bound in about two hours at the measured 9.3
process creations per second, failing every live guardian within one census of
the others. Above 8192 entries a successful census keeps an entry only when it
is owned, present in this census, present in the previous one, or named as a
current row's parent. Owned births are never forgotten and the prune writes no
verdicts, so no owned process can be reclassified; forgetting a dead foreign
birth can only demote a later newcomer to unclassified, where session or group
evidence or the existing refusal applies. The window is sufficient because a
newcomer's parent was alive when it forked, so the last census that saw it is
the previous one, and `previous` advances only on a successful update. The
65536 bound stays as the backstop, and an owned tree that itself creates that
many processes under one guardian still reaches it.

The stop cause is recorded. Reply::Stopped already carried StopCause to the host
and the host dropped it, which is why three occurrences read only an unexplained
cleanup unknown. The guardian now also carries a fixed StopDetail --
ancestry_unconfirmed, tracking_gap, census_failed, leader_unreadable -- taken
from the tracker's typed error payload rather than a message; SupervisedReport
carries it; the guardian's unproven-cleanup error names both; and the operator
status gains an optional stop_cause beside the unchanged, pinned cleanup
vocabulary, from fixed categories only (ADR-175). Linux cgroup recovery and
Windows Job reports carry no detail and are otherwise unchanged.

## Coalition evidence for a daemon no other evidence reaches (2026-09-20)

The residual case the previous amendment kept fatal is what ended the first soak
on the amended tracker, in its first round, and the stop cause recorded for the
first time said so: `observation_failure:ancestry_unconfirmed`. The refused rows
were four processes of a browser updater. launchd runs that updater every hour;
each process is started through a middle that opens its own session and exits
before any census, so it has no ancestry, no session mate and no group mate. The
census is whole-system, so every guardian on the host refused at once. Any Mac
with that browser installed loses every owned tree once an hour.

A coalition is the one scope that move does not leave. It is inherited across
fork, exec and `setsid`. Leaving it takes a spawn attribute that only launchd may
use, and whatever launchd starts on a descendant's behalf is launchd's child: it
was never a descendant by ancestry either, so this rule concedes nothing the
ancestry rule had not already conceded. `PROC_PIDCOALITIONINFO` is readable
unprivileged for every process, root-owned ones included (measured against 962
live processes; the only refusals were processes that had already exited), and
770 of 796 launchd children on the measured host sat in a coalition other than
the service's.

When ancestry, session and group evidence have all stalled, a newcomer whose
resource AND jetsam coalition ids both differ from the leader's is classified
unrelated. The evidence is negative only. It can never classify a process as
owned: the leader shares its coalition with the service, its terminal and
everything else started there, so an equal coalition proves nothing and that
newcomer still refuses, sticky and fatal as before. A zero id on either side is
no evidence, a refusal to read one is recorded as zero and never ends a census,
and requiring both ids to differ means a kernel that ever split only one of them
would weaken nothing. The leader's coalition is read once, inside the identity
bracket, while the leader is still suspended.

`native_macos_coalition_evidence_is_negative_only` pins the rule and every case
that must keep refusing. `native_macos_coalition_survives_setsid_and_differs_from_launchd`
reads the two platform facts from real processes, so a platform that stopped
honouring either fails a test before it fails a guardian. What remains fatal: a
process in the service's own coalition that daemonizes through a parent no census
saw. kqueue NOTE_TRACK remains the only complete answer and remains unused.

## Consequences

Owned handles and native observations constrain signalling and cleanup claims. Process launch, leader exit and fixture success remain separate from sandbox qualification and canonical task completion.

## Alternatives Considered

Assigning a Windows job after process creation leaves an unowned startup interval. Numeric-PID fallback or treating a POSIX group signal as whole-tree cleanup would discard the identity and descendant limitations documented below.
