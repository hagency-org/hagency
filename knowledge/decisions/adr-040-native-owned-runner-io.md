---
kind: decision
id: ADR-040
title: Hand guardian-owned child pipes to one bounded Codex session
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
---

## Context

Typed runner IO must consume the pipes of the same retained native process owner rather than introduce an independent launcher or cleanup identity.

## Decision

### Scope and ownership

This bounded M4 slice connects ADR-036's typed session to the existing native
guardian launcher. `SupervisedProcess::spawn_piped` returns the retained owner
and a non-cloneable `StdioPipes`; consuming `into_parts` transfers exactly one
stdin writer, stdout reader and stderr reader. It accepts the existing host-only
`Launch` value and an explicit guardian executable. No HTTP route constructs it.
There is no second launcher, ambient environment, model-selected executable or
numeric-PID cleanup path. Normal null-stdio launching remains available unchanged.

On Unix the host creates three anonymous pipes before spawning the guardian.
The private control socket carries PreparePiped and one SCM_RIGHTS transfer;
the guardian receives only after launch/scope admission and before Prepared or
Start. It then uses the existing OwnedProcess spawn, descriptor sealing, retained
leader identity and process scope. The control socket cannot reach workspace
code. Linux subreaper ownership and adopted-descendant checks remain unchanged.
Received endpoints must be FIFO descriptors, with read-only stdin and write-only
stdout/stderr; marker, message kind, exact descriptor count and access mode are
checked before Start. Every known received descriptor acquires RAII ownership
before an ordinary rejection can return. No received endpoint is cloned or
returned through a second public helper.

Linux creates pipes and receives rights with atomic CLOEXEC flags. macOS lacks
those flags and sets CLOEXEC in the sole pre-start receiver before any workspace
child spawn. The existing post-fork descriptor seal remains mandatory. Pipe
creation uses pipe plus fcntl on macOS; it does not claim atomic creation against
unrelated external launch paths. That is also the implementation used by the
reviewed [Rust 1.94 Unix pipe source](https://github.com/rust-lang/rust/blob/1.94.0/library/std/src/sys/pipe/unix.rs).

### Ancillary bounds and the disposable macOS receiver

The parser uses initialized, aligned 4 KiB control storage. It clamps the returned
control length before libc traversal and checks each header/payload bound.
Unknown ancillary kinds, additional messages, wrong counts, invalid marker,
truncated data and wrong descriptor types are admission failures. Linux's
received rights use MSG_CMSG_CLOEXEC; its kernel closes excess truncated rights.
See the primary [Linux UNIX socket specification](https://www.man7.org/linux/man-pages/man7/unix.7.html)
and [recvmsg specification](https://www.man7.org/linux/man-pages/man2/recvmsg.2.html).

The reviewed XNU source baseline is commit
`f6217f891ac0bb64f3d375211650a4c1ff8ca1ea`. Its control allocation rejects sizes
above MCLBYTES after LP64 expansion
([uipc_syscalls.c, line 3292](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_syscalls.c#L3292)).
MCLBYTES is 2048 on both
[arm, line 84](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/arm/param.h#L84)
and [i386, line 108](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/i386/param.h#L108).
The source caps its descriptor bookkeeping at 512 and requires one complete
SCM_RIGHTS control block
([uipc_usrreq.c, line 121](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_usrreq.c#L121),
[line 2502](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_usrreq.c#L2502)).
Thus 4 KiB covers the reviewed native control maximum; this is a version-bound
source observation, not an assumption that all future kernels retain that bound.

XNU installs the descriptors before returning their IDs
([unp_externalize, line 2383](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_usrreq.c#L2383));
control copyout can then truncate while retaining the original header length
([uipc_syscalls.c, line 2106](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_syscalls.c#L2106)).
An undersized receiver therefore cannot recover every installed descriptor ID.
On macOS any MSG_CTRUNC, impossible returned control length or overlong header
terminates this disposable pre-start guardian with exit 125. OS process teardown
closes undisclosed descriptors. The parent observes failed/uncertain admission;
it never reports successful launch. The receiver and its capacity helper are
private to the guardian module, and no daemon-thread path may call them.

The pinned source file SHA-256 values are `72090fdc19c2c96481457beb70579f4d5292b5b1139b0ef735d7b01508799ac0`
for uipc_syscalls.c and `c34d28100931762957c2cb2f3b7a2bef5fe078e4d9c1dca1c494345cbec0e12c`
for uipc_usrreq.c. Tests ran on macOS 26.5 / Darwin 25.5.0; this does not assert
that the running kernel was built from that published source commit. A disposable
subprocess fixture deliberately supplies 64 bytes, verifies fatal truncation,
and observes EOF after all received writer descriptors have been closed.

### Session IO and stopping

`OwnedSession` consumes the returned streams through Tokio's native Unix pipe
Sender/Receiver adapters, making only host endpoints nonblocking. It requires a
host execution worker with an active Tokio IO runtime. Existing bounded read,
write, event queue, stderr tail and absolute monotonic deadline rules apply.
Stderr is private diagnostic state, without automatic console or log projection.
Its counter measures observed bytes; closing a terminal disposable transport does
not promise to consume stderr still buffered in the kernel.
No unbounded tasks, channels or blocking pipe-reader wrappers are created.

Typed host settings must match the launch cwd. One fresh thread and one turn
are supported; the inherited fixed on-request/user and workspace-write policy
remains, with no arbitrary permission overrides. Server requests remain explicitly
unsupported. This wrapper does not expose resume or warm reuse. Request bytes,
complete transport write/flush, upstream response, terminal turn notification,
child termination and canonical task completion remain separate observations.

Initialization, thread/turn start, interrupt and update operations retain the
owner. Error, terminal state or dropping a started operation closes the typed
transport and synchronously stops through that owner. A drop cannot detach a
cleanup task or transfer authority to a PID. `Cleanup::Observed` preserves the
exact platform report, including macOS `whole_tree_stopped: false`; it is not
automatically proof that all descendants stopped. IO errors remain
`Cleanup::Unknown`, which a retained caller can inspect and retry. Startup or
handoff failure is `StartError::Uncertain` because a lost startup acknowledgement
does not prove that no child executed. Completed protocol text stays distinct
from those reports and cannot complete a task or release a domain lease.

The existing stop API is synchronous, bounded to three seconds per attempt.
An OwnedSession drop can be followed by the supervisor's existing bounded
owner-EOF fallback (up to another three seconds); an earlier failed operation
stop can also be retried on final drop. This can block a Tokio worker and is
explicitly not nonblocking server orchestration. A future host must run this
ownership in a dedicated execution worker or supply an equally strong cancellation
custody design; spawning detached cleanup is insufficient.

### Offline evidence and remaining gates

The native fixture executes actual initialize, initialized, thread/start,
turn/start, streamed item output and completion through owned child pipes. It
stays alive after protocol completion, requiring an explicit owner stop. Fixtures
cover missing executable, invalid settings, EOF, silence, mid-write cancellation,
256 KiB stderr pressure with a 16 KiB retained tail, independent stream closure,
and an observed descendant with its own pulse file. Linux additionally exercises
a detached process group through the existing subreaper. Separate platform tests
cover malformed rights, closure on rejection and macOS fatal truncation.

Windows piped launching returns Unsupported until native cancellable overlapped
IO is implemented. Its existing atomic job-assigned null-stdio launch is untouched;
refusal tests do not establish Windows runner parity. Linux and Windows execution
of this new slice still require their CI hosts. POSIX crash-containment requests
remain refused; macOS detached-descendant and POSIX guardian-death recovery remain
open. No live model, effective sandbox, authenticated dispatch/input ACK, approval
adapter, usage accounting, canonical task transition or lease settlement was
qualified. Native service execution remains disabled and M4 remains incomplete.

## Consequences

Pipe transfer, cancellation and process-stop observations remain tied to existing guardian custody. Unsupported platform guarantees and native service activation stay explicit.

## Alternatives Considered

A second launcher, inherited ambient environment or numeric-PID cleanup path would separate execution from its retained owner. Passing guardian control descriptors into work would expose process-custody authority.

## Runtime-only cooperative approval opt-in

OwnedSession now forwards the bounded runtime control-pump API without extracting
or reconstructing SessionDriver or moving the retained process owner out of its
original field. Synchronous preparation returns a unique original typed frame
before the host can await durable response begin. Every new started async
operation retains the same synchronous stop-on-drop guard. Terminal/error results
still close the original session and stop through the exact original owner;
control results alone neither stop nor assert completion.

This is a runtime mechanism, not execution-host integration. The current native
host's 30-second operation and 2-second response policies remain unchanged. A
future host must explicitly admit a finite owner deadline plus its margins within
the original launch and all current domain authority. No default request behavior,
process privileges, platform containment or live-provider qualification changes.
The offline owned regression cancels the new pump while the actual original child
waits at its existing usage gate and checks its unchanged cleanup report.
