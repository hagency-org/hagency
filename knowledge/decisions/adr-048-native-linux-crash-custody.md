---
kind: decision
id: ADR-048
title: Qualify Linux guardian loss recovery through protected host cgroup capabilities
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
---

## Context

Linux guardian loss needs independently retained process-scope custody, with honest limits when both the host and guardian disappear.

## Decision

### Exact guarantee

This implements an optional host-only recovery path when a Linux guardian dies
but its owning host remains alive. The existing POSIX launcher still refuses
`require_crash_containment=true`. A cgroup is not a Windows kill-on-close Job:
closing its descriptors does not stop processes. Simultaneous backend/guardian
loss still needs a separately qualified external service manager or keeper.
Same-UID workspace code can signal both processes; nondumpability does not block
those signals. No server, actual runtime or sandbox capability is enabled here.

### Provisioning and admission

`CgroupRecovery::from_host_files` consumes an already-open directory and exact
write-only `cgroup.procs`/`cgroup.kill` descriptors. The trusted provisioner must
reserve a fresh empty domain subtree and keep its exclusive lease until cleanup
is inspected. It opens migration capabilities with sufficient privileges before
dropping privileges or handing them to the host. The native library never mounts,
creates, delegates, chmods or removes a cgroup, and never receives a runtime path
or PID as recovery authority.

Admission verifies cgroup2 filesystem magic, regular control files, matching
device/inode relative to the retained directory, write access mode, CLOEXEC,
root ownership, domain type and empty recursive population. A procfs-verified,
bounded mountinfo observation plus descriptor-based statx requires its exact mount ID
and a full `/` filesystem root, rejecting bind-mounted subtrees hiding ancestors.
Up to 64 ancestors and their `cgroup.procs`/`cgroup.threads` files must be root
owned with no group/other write bits. Workspace-created child groups remain
within that protected boundary.

#### Initial namespace qualification

The mount root string `/` is insufficient by itself: Linux renders it relative
to the caller's cgroup namespace. UID 0 in file metadata is also relative to the
caller's user namespace. Admission therefore opens the **current executing
thread's** namespace entries itself. It requires procfs source symlinks and
parents on the same verified proc mount, then verifies nsfs filesystem magic,
`NS_GET_NSTYPE`, and the initial namespace inode together. It does not accept a
caller-supplied namespace descriptor, compare against `/proc/1`, or treat
`NS_GET_PARENT` returning EPERM as an initial-namespace proof.

The supported kernel release families are explicitly restricted to 6.8, 6.12,
6.14 and 6.17. Other releases return Unsupported; changed namespace identities do too.
These are source-inspected implementation constants, **not a portable Linux
ABI**: user `0xEFFFFFFD`, cgroup `0xEFFFFFFB`, nsfs `0x6e736673`, and namespace
types `CLONE_NEWUSER`/`CLONE_NEWCGROUP`. Initial constants agree in
[v6.8 proc_ns.h](https://github.com/torvalds/linux/blob/v6.8/include/linux/proc_ns.h#L37),
[v6.12 proc_ns.h](https://github.com/torvalds/linux/blob/v6.12/include/linux/proc_ns.h#L37),
and [v6.14 proc_ns.h](https://github.com/torvalds/linux/blob/v6.14/include/linux/proc_ns.h#L37).
Dynamic namespace inodes start at `0xF0000000`, excluding those reserved values
([v6.12 generic.c, lines 181–198](https://github.com/torvalds/linux/blob/v6.12/fs/proc/generic.c#L181)).
The initial cgroup is owned by the initial user namespace; cgroup mount paths
are relative to current cgroup namespace
([v6.12 cgroup.c, lines 193–200 and 1774–1797](https://github.com/torvalds/linux/blob/v6.12/kernel/cgroup/cgroup.c#L193)).

`NS_GET_USERNS` on the opened initial cgroup namespace supplies an additional
kernel check: `ns_get_owner` must find the **calling** thread's user namespace
while walking the owner and its ancestors. The initial user namespace has no
parent, so a nested caller cannot substitute an accessible initial namespace
descriptor ([v6.12 user_namespace.c, lines 1291–1305](https://github.com/torvalds/linux/blob/v6.12/kernel/user_namespace.c#L1291)).
The returned CLOEXEC namespace descriptor is adopted once and checked against
the same nsfs/type/inode triplet; both original namespace descriptors must share
its global nsfs mount identity, excluding bind-mounted source substitutions
([v6.12 nsfs.c, lines 89–108 and 158–173](https://github.com/torvalds/linux/blob/v6.12/fs/nsfs.c#L89)).
Proc namespace entries resolve the actual proc task through `proc_ns_get_link`
([v6.12 namespaces.c, lines 39–64](https://github.com/torvalds/linux/blob/v6.12/fs/proc/namespaces.c#L39)).
The proc `ns` directory has mode 0511, so directory traversal uses O_PATH after
nondumpability rather than requiring directory-read permission
([v6.12 base.c, lines 3118–3122](https://github.com/torvalds/linux/blob/v6.12/fs/proc/base.c#L3118)).
Checks repeat at host admission, before attachment, and in the guardian after
exec and dumpability reset, before any workspace launch. The provisioner must
exclude concurrent privileged namespace/mount replacement and use a kernel
whose source implements the inspected contracts. A version string alone does
not prove a vendor kernel's implementation; real execution qualification on its
exact kernel release is also required.

#### Linux 6.17 source qualification

Hosted CI reached a protected subtree on `6.17.0-1022-azure`, then correctly
refused the uninspected family. The follow-up admits 6.17 after inspecting
upstream commit `e5f0a698b34ed76002dc5cff3804a61c80233a7a` (v6.17):

- Reserved namespace identities moved into [uapi/linux/nsfs.h](https://github.com/torvalds/linux/blob/e5f0a698b34ed76002dc5cff3804a61c80233a7a/include/uapi/linux/nsfs.h); proc_ns.h aliases them. Dynamic inodes still start above the reserved range in [proc/generic.c](https://github.com/torvalds/linux/blob/e5f0a698b34ed76002dc5cff3804a61c80233a7a/fs/proc/generic.c#L194).
- [nsfs.c](https://github.com/torvalds/linux/blob/e5f0a698b34ed76002dc5cff3804a61c80233a7a/fs/nsfs.c#L198) preserves inode/type identity and returns a fresh CLOEXEC owner descriptor. [ns_get_owner](https://github.com/torvalds/linux/blob/e5f0a698b34ed76002dc5cff3804a61c80233a7a/kernel/user_namespace.c#L1380) still requires the calling user namespace in the owner ancestry.
- [Proc namespace links](https://github.com/torvalds/linux/blob/e5f0a698b34ed76002dc5cff3804a61c80233a7a/fs/proc/namespaces.c#L43) resolve the actual task, and [base.c](https://github.com/torvalds/linux/blob/e5f0a698b34ed76002dc5cff3804a61c80233a7a/fs/proc/base.c#L3667) preserves the traversable namespace directory mode.
- [cgroup.c](https://github.com/torvalds/linux/blob/e5f0a698b34ed76002dc5cff3804a61c80233a7a/kernel/cgroup/cgroup.c) preserves the initial owner and namespace-relative mount path, uses opener credentials for migration, and serializes recursive kill with the cgroup mutex. Its fork path compares kill sequence numbers before userspace execution. The [versioned cgroup contract](https://github.com/torvalds/linux/blob/e5f0a698b34ed76002dc5cff3804a61c80233a7a/Documentation/admin-guide/cgroup-v2.rst#L1030) retains recursive live-population and fork/migration semantics.

This source check permits testing the observed vendor kernel; it does not replace
the seven actual hosted qualification cases or widen any filesystem, namespace,
privilege, lease or complete-crash-containment condition. Unknown families
(including 6.16 and 6.18) remain refused. No local privileged test is enabled.

The calling host must already have equal nonzero real/effective/saved/fs UIDs,
zero effective/permitted/inheritable/bounding/ambient capabilities, NNP=1 and
dumpability disabled. `/proc/thread-self/status` is bounded and required fields
must be present exactly once. SIGCHLD must be default without SA_NOCLDWAIT, and
the host exclusively owns reaping of its Child. The disposable guardian disables
dumpability again after exec, checks the same privilege conditions, and only then
acknowledges Prepare. Workspace exec inherits NNP and the empty capability sets.
The backend does not silently change its process-wide security configuration.

Checks cannot prove what a trusted privileged provisioner will do later. It must
not duplicate control capabilities to workspace code, modify protected permissions,
move processes across the boundary, insert unrelated work, remove/reuse the group,
or change host privilege/reaping policy during custody. Those are explicit host
preconditions, not boolean claims derived from a runner or model. Full production
qualification must validate this provisioning boundary and actual escape attempts.

### Launch and recovery

The existing launcher starts the trusted guardian waiting on its anonymous
control socket. Before sending Prepare, the host moves that retained unreaped
Child into the protected cgroup. Its PID is used solely to migrate this owned,
not-yet-started guardian; no numeric PID supplies kill authority. All later
workspace forks inherit the guardian's membership, avoiding the race where a
guardian dies before an uncontained child performs a pre-exec self-migration.
The existing descriptor seal and exact-three stdio transfer remain in use.

The optional path retains independent kill/events descriptors in the backend.
Stop or guardian-channel failure writes `1` to the retained kill capability and
observes recursive `populated=0` within the caller's absolute Instant deadline.
No PID census or PID kill fallback exists. A successful guardian reply alone is
not this independent proof. Empty population means no live execution in the
subtree, not that every adopted zombie has been reaped. The host reaps only its
retained guardian; host init/service management remains responsible for orphans.
No result completes a canonical task or releases any domain lease.
The host must actively call `wait` or `stop`; this primitive creates no background
monitor. The actual service/OwnedSession host integration remains open. A
`GuardianLost` cause means its trusted channel failed, including protocol errors;
it does not itself assert process death. Known failed guardian cancellation
signals remain recorded even if the independent cgroup later becomes empty.

Unknown writes, read errors and timeout keep cleanup unresolved. Explicit stop
retains the capability for inspection/retry; Drop makes bounded best-effort
attempts without claiming completion. Supervisor Drop shares a three-second
observation deadline; a still-armed capability's final Drop may add two seconds.
This synchronous five-second upper observation budget can block a worker and is
not nonblocking service orchestration. Kernel syscalls are not hard-real-time
operations; deadlines bound retry/poll behavior after syscalls return.

### Pinned kernel contract and remaining qualification

Linux v6.12 documents inherited cgroup membership, ancestor migration permission
checks, recursive population and fork/migration-safe `cgroup.kill`; threaded
groups are unsuitable. See the versioned
[cgroup-v2 documentation](https://www.kernel.org/doc/html/v6.12/admin-guide/cgroup-v2.html).
The inspected v6.12 implementation uses the opener's `f_cred` for migration
permission checks (`__cgroup_procs_write`, lines 4869–4910), rather than treating
the current unprivileged writer as the opener. Kill uses the exact kernfs cgroup
under the cgroup mutex (lines 3755–3824). See
[kernel/cgroup/cgroup.c](https://github.com/torvalds/linux/blob/v6.12/kernel/cgroup/cgroup.c#L4869).
NNP and dumpability follow the Linux
[NNP contract](https://man7.org/linux/man-pages/man2/PR_SET_NO_NEW_PRIVS.2const.html)
and [dumpability contract](https://man7.org/linux/man-pages/man2/PR_SET_DUMPABLE.2const.html).
The unchanged Cargo.lock resolves rustix 1.1.4 and libc 0.2.189; no dependency
upgrade is part of this slice.

Local macOS tests exercise pure admission vectors and the unchanged POSIX
guarantee refusal. Linux cross-compilation is not execution. The Linux refusal
test rejects ordinary files/unprovisioned hosts and makes no containment claim.

The separate `hagency-cgroup-probe` executable has no ignored-test or missing-host
success branch. A qualified host must supply exactly three distinct inherited
descriptors (directory/procs/kill, numeric CLI arguments after mode/directory) and the
required identity/capability/NNP conditions, plus a fresh user-owned mode-0700 test
directory. The disposable fixture seals those descriptors before spawning and
sets its own nondumpability after exec. Run each mode with a fresh reserved group:
`guardian-death`, `stop`, `failed-spawn`, `guarantee-refused`. The first two run a
real native child and double-fork detached descendant; guardian loss occurs only
after Start and live-heartbeat observation. Closing all child IO first must leave
work alive; cgroup cleanup must stop it. Failed spawn and complete-guarantee
requests must leave no workspace execution. Admission/refusal exits 78 and never
prints the fixture's qualification result. No privileged fixture has been run in
this macOS environment. Provisioned Linux execution and adversarial reassignment/
ptrace checks remain required before advertising recovery availability.

#### Disposable hosted CI provisioner

`native/scripts/qualify-linux-cgroup.py` is a CI-only external custodian. It
requires Linux X64, root and GitHub-hosted runner markers, initial user/cgroup
namespaces and an existing writable full cgroup2 mount. Those environment
markers prevent accidental execution; they are not authentication against a
malicious root operator. It creates one exclusive random `hagency-ci-*` subtree,
then fixed per-case child groups. Only its created directories/control files
are chmodded. No mount, ancestor permission change, delegation, remount or local
deployment action occurs. Unavailable kernel/delegation/unshare or cleanup
failure is a nonzero **qualification failure**, never success or an ignored test.

The helper retains exact root directory/kill/events capabilities. For ordinary
positive cases it executes only `/usr/bin/setpriv` and the built offline probe,
with the runner's nonzero UID/GID, no supplementary groups, NNP and emptied
bounding/inheritable/ambient sets. The probe sets nondumpability after exec and
verifies all five capability sets and all four UIDs before launch. Exactly three
cgroup descriptors are passed and immediately sealed CLOEXEC by the disposable
probe. stdout/stderr are independently capped at 64 KiB and drained by one
selector; a monotonic 20-second fixture deadline applies even during output.
The helper is retained by pidfd for bounded identity-safe failure cleanup.

Four actual process cases cover guardian death with detached descendants,
explicit stop, failed executable, and the unchanged full-guarantee refusal.
Two real `/usr/bin/unshare` cases create nested user and cgroup namespaces and
must observe the exact namespace admission refusal before a workspace starts.
Failure to create those namespaces is a failed qualification, not a substitute
for the refusal result. A separate `custodians-abort` injection kills the
guardian and exits the host without Drop while a descendant still runs; the
**external root helper** must observe that live population and stop it through
its own retained cgroup capability. That case proves the test custodian's
cleanup, not simultaneous-custodian-loss recovery inside the runtime.

Every fixture outcome invokes root `cgroup.kill` and observes recursive
`populated=0` for at most five seconds. A hung helper is signalled only through
its retained pidfd and waited for at most five seconds. Finally, the provisioner
again attempts independent cleanup and removes only its known children and
root whose device/inode still match the retained identities; unknown descendants
or changed identities refuse removal. The at most eight groups bound final
cleanup observations to forty seconds (kernel syscall stalls remain outside a
hard real-time guarantee). Root-helper death itself is outside this test cleanup
guarantee. No success message is printed before observed cleanup and removal.

The workflow runs these cases only on the hosted Ubuntu job. Python gate,
event-parser and replacement-refusal tests manipulate ordinary temporary files.
An actual local subprocess tests collector timeout/output bounds; a Linux-only
ordinary-subprocess case drives the real pidfd finally cleanup on both errors,
using an explicitly labeled stop-invocation test double. Those are bounded helper
IO/cleanup checks and do not prove cgroup containment. Local source/compile checks in this change
do not count as the still-pending real positive and nested-namespace CI results.
The optional library path is not wired to a service or OwnedSession; production
availability, hostile ptrace/reassignment qualification and complete POSIX crash
containment remain closed gates.


#### Fixed CI binary staging and separate permission reproduction

Hosted run34529853081 at ae284b9 passed guardian-death, stop, failed-spawn and
unchanged guarantee-refused, then nested-user returned unshare exit126 with
"Permission denied" before the native probe ran. This proves an executable access
refusal, not the exact historical ancestor modes: those modes were not logged.
A user namespace mapping only initial UID0 cannot use namespace capabilities to
bypass DAC on an unmapped runner UID's private0700 build ancestor. A separately
controlled fresh fixture now reproduces that mechanism, consistent with the log.
It never changes the historical checkout or treats126 as namespace qualification.

The hosted-only provisioner stages exactly the fixed `hagency-cgroup-probe` and
`hagency-platform-probe` bytes into one exclusive random directory under fixed
`/tmp`. Hosted/root/initial-namespace gates run first. The `/tmp` parent is opened
without following a final symlink, must be root-owned and either nonwritable by
others or sticky, and both `/` and `/tmp` must remain traversable. No existing
parent is chmodded. The fresh directory is root-owned0700 while copying, then0555;
its two regular files are exclusively created, root-owned, single-link and0555.
All seven cases use these staged paths. This prevents an unmapped build UID or
private Cargo ancestor from blocking the exact intended native admission check.

Source names are fixed, final symlinks and nonregular/nonexecutable files are
refused, and each source must contain1..128MiB. Copying uses retained descriptors,
64KiB blocks, bounded exact byte count, full writes, final EOF and before/after
identity/size/mtime/ctime comparisons. Staging does not authenticate an arbitrary
binary: these are the trusted current CI revision's built fixtures, and concurrent
privileged source mutation remains excluded. No source or checkout permission is
modified and no model or service is invoked. Root retains every created directory
and file identity; cleanup removes only exact fixed entries through the retained
directory descriptor. Changed entries refuse deletion. Write permission may be
restored only on the verified self-created directory, which allows ordinary
nonroot file tests to clean their own fixtures. There is no recursive stage walk,
replacement adoption, deletion of unknown descendants or generic chmod fallback.

Before the actual nested-user case, the helper copies the same fixed bounded
probe behind a fresh fixture-UID-owned0700 directory. Through the same retained
child/pidfd and root cgroup cleanup helper it requires actual exec126 plus the
permission diagnostic, labels this **NOT namespace qualification**, then removes
only that exact copied file and directory. The actual staged nested-user and
nested-cgroup cases still require native exit78, no stdout and the existing exact
initial-namespace refusal diagnostic. Missing unshare capability, permission
failure in that actual case, another exit or incomplete cleanup still fails CI.
No prior failed run is retroactively reported as passing.

The Python suite executes a tiny fixed compiled native program from staged copies
whose source ancestor/files remain0700 and unchanged. It also checks empty/large/
nonexecutable/symlink/directory sources, growth and partial write failures, exact
stage cleanup, replacement refusal, controlled precheck classification, and126
rejection by the real qualification verdict function. `/usr/bin/cc` builds only
that fixed ordinary test program; copying macOS's platform-signed `/bin/echo`
out of its protected path was correctly killed by AMFI and is not used as a
portable native fixture. Local Python checks and Cargo-bound admission scenarios
are separate evidence. Real nested-user/cgroup execution, all seven outcomes and
independent subtree cleanup remain mandatory hosted CI gates.

## Consequences

Protected cgroup capabilities support the documented surviving-host recovery path. A cgroup is not kill-on-close, and namespace and hosted execution qualification remain mandatory separate evidence.

## Alternatives Considered

Treating a cgroup descriptor like a Windows Job Object would falsely promise cleanup on owner loss. Accepting exec permission failure as namespace refusal would replace the intended qualification with an unrelated denial.
