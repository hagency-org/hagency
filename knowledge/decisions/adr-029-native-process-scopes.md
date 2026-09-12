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

## Consequences

Owned handles and native observations constrain signalling and cleanup claims. Process launch, leader exit and fixture success remain separate from sandbox qualification and canonical task completion.

## Alternatives Considered

Assigning a Windows job after process creation leaves an unowned startup interval. Numeric-PID fallback or treating a POSIX group signal as whole-tree cleanup would discard the identity and descendant limitations documented below.
