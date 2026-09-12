---
kind: decision
id: ADR-044
title: Use private overlapped pipes with the existing atomic Windows job launcher
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
---

## Context

Windows runner streams need cancellable owned IO while retaining the launcher's atomic Job Object assignment and strict handle inheritance.

## Decision

### Boundary and admission

This extends ADR-040's host-owned IO slice to Windows. Native service execution
remains disabled. The platform's existing Process launcher gains an optional
three-handle stdio input; it retains the same explicit executable/cwd/environment,
kill-on-close Job Object and atomic PROC_THREAD_ATTRIBUTE_JOB_LIST assignment.
There is no alternate CreateProcess path or suspended-unowned interval.

The piped path adds PROC_THREAD_ATTRIBUTE_HANDLE_LIST containing exactly stdin,
stdout and stderr, and STARTF_USESTDHANDLES. Only these three child endpoints
become inheritable. Host pipe ends, job/process handles and unrelated inheritable
handles remain excluded. Both handle arrays and their owned resources outlive
the attribute list and CreateProcess call. The original no-stdio path continues
with inheritance disabled. This follows Microsoft's
[attribute-list contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).

Before child creation, the host creates three byte-mode local named pipes, each
with one instance, first-instance refusal, remote-client rejection and a 128-bit
system-RNG name. A protected DACL grants access only to the current process token's
user SID. No default Everyone/anonymous read grant is accepted; see Microsoft's
[named-pipe security rules](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights).
Names are internal and never passed to a model or public listener. Same-user
process injection and administrator privilege are not sandboxed by these pipes.

The host immediately opens each synchronous child endpoint through CreateFile
and verifies that the client and server process IDs both identify itself. A
collision or substituted endpoint fails before launch, with no retry/reconnect
loop. This is endpoint admission, not PID-derived signal authority. CreateFile
establishes the connection before the server's first IO; Microsoft's
[connection contract](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-connectnamedpipe)
explicitly recognizes a client connecting before ConnectNamedPipe. No pending
connect request or stack-owned OVERLAPPED is created by our admission code.

Host endpoints use FILE_FLAG_OVERLAPPED and non-inheritance; child stdio remains
synchronous. A 4 KiB kernel buffer quota is requested in each direction. Kernel
rounding/temporary expansion is not an application allocation guarantee; retained
application buffers and submitted write sizes are separately bounded below.
The one-use StdioPipes value consumes all three handles into the private Pipe
adapter on a private Tokio IO runtime. Constructor, direction and raw handles
remain private. The safe public adapter has no reconnect operation.

### Write observation and cancellation

The inspected dependency baseline is Tokio 1.53.1 with Mio 1.2.3 from Cargo.lock.
Tokio's named-pipe poll_flush is a no-op. Mio writes can report a queued buffer's
length before its overlapped operation completes; its Drop cancels reads/connects
but deliberately leaves writes running. We cannot use those defaults as proof
of completed transport writes. Reviewed source:
[Tokio named_pipe.rs](https://github.com/tokio-rs/tokio/blob/tokio-1.53.1/tokio/src/net/windows/named_pipe.rs),
[Mio named_pipe.rs](https://github.com/tokio-rs/mio/blob/v1.2.3/src/sys/windows/named_pipe.rs).

The wrapper submits at most 16 KiB and retains at most a matching 16 KiB prefix.
It reports no bytes until Mio accepts a zero-payload write: this pinned version
checks its prior write state before even an empty write, refuses while pending,
and surfaces any completion error there. The probe therefore observes the prior
submission's completion without sending extra payload. Each poll does bounded
work and yields after submission or stale readiness. A changed pending buffer
is refused. This relies on the inspected implementation and must be requalified
when Tokio/Mio changes; a Windows test requires even a single over-quota write
to remain pending while the child endpoint does not read.

Every returned write count is an observed lower bound; a cancelled operation may
have delivered more bytes than the adapter could confirm. Flush adds no stronger
claim after those writes. Neither operation proves that Codex parsed a request,
accepted a turn, applied permission, or completed a canonical task. The existing
transport preserves an unconfirmed request on cancellation and never replays it.
The Windows fixture records bytes actually read before cancellation to demonstrate
that a zero confirmed count does not mean that no partial effects occurred.

Drop disconnects the retained server endpoint before requesting CancelIoEx for
all operations, including writes. Disconnection prevents a racing partial-write
callback from continuing through the old connection and discards unread pipe
data; see [DisconnectNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-disconnectnamedpipe).
Cancellation itself is a request, may race normal completion, and does not wait.
Mio's IOCP-owned buffers and OVERLAPPED records remain alive until completion;
we do not free them early, synchronously wait, or create a detached blocking reader.
This follows [CancelIoEx's lifetime contract](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelioex).
Drop does not claim that zero bytes escaped or that the process stopped.

The pinned Mio reactor teardown can miss late cancellation completions and retain
pipe handles when a caller's runtime ends; see the upstream
[handle-lifetime report](https://github.com/tokio-rs/mio/issues/1944).
All these private pipe registrations therefore use one lazy process-lifetime
Tokio completion reactor held in a static OnceLock. It has exactly one worker,
IO enabled, no timers needed for the adapter, and no public spawn handle. No
blocking task is submitted. The fixed worker/runtime cost must enter future
execution budgets. Caller runtimes may end without shutting down this reactor;
pending completion storage stays serviced, and final process exit closes OS
resources. There is no per-session reactor teardown or replacement path.

This reactor owns IO completion only, never a child, job, task or lease. The
host still retains the process scope and stops it synchronously as before.
A Windows fixture repeatedly drops short-lived caller runtimes with pending
reads, checks disconnection and later EOF across a fresh caller, and observes
actual process handle counts return within a bounded concurrent-test allowance.
Waiting a fixed duration alone is not accepted as a cleanup observation.

There is one pending write per writer, a bounded comparison copy and Mio's
per-pipe bounded active read/write storage, plus the existing Driver's bounded
read/event/text queues. Stderr remains a private observed-byte count and 16 KiB
tail. No unbounded channel or per-message task is introduced. The Mio source
SHA-256 is `a6254fb522bb45f17ae1e3f6c70f63dd9a9fae66137eae60bb47be254b0560b4`;
Tokio's is `da168cff030a5e3335263f29d7279bb3ee17220f2e188f42460a5b6985d41f9f`.

### Retained job and qualification

OwnedSession shares its existing lifecycle guards between platforms; only stream
conversion differs. Errors, deadlines, cancelled futures and terminal observations
close streams and stop through the retained owner. A report with
whole_tree_stopped=false remains explicit and may be retried; only a full stop
report suppresses another stop attempt. Windows whole-tree proof still requires
zero active job processes and the retained leader handle signalling exit. No
protocol text, clean exit or pipe EOF releases a task or domain lease. Synchronous
bounded stop-in-Drop remains a dedicated execution-worker limitation from ADR-040.

Actual Windows fixtures now cover the shared full offline Codex lifecycle,
Unicode paths, failed executable, EOF, silence, stderr pressure, partially
delivered cancelled input, and a child that survives pipe closure until owned
stop. Windows-specific fixtures check an excluded inheritable event handle,
job membership before stdin processing, blocked single-write completion, pending
IO drop, caller-runtime teardown and handle counts, descendant/new-process-group stop, rejected breakaway and controller
exit without Rust Drop.
These replace the previous Windows Unsupported placeholders; they do not invoke
a live model or test sandbox efficacy.

Cross-target compilation and local Unix regression tests are available in this
worktree. Windows-specific execution still requires real Windows CI, and is not
reported as passed by cross-compilation or by the shared selectors on macOS.
The Unix launcher, guardian admission and SCM_RIGHTS receiver are unchanged.
POSIX guardian-death recovery, macOS detached-descendant proof, actual Codex
sandbox/runtime qualification, authenticated dispatch/approvals and full M4
remain open. This accepted adapter decision does not advertise runtime availability.

## Consequences

Private overlapped pipes extend the existing owned-session path without changing its execution policy. Cross-compilation and pipe fixtures do not qualify actual sandbox enforcement or production availability.

## Alternatives Considered

A separate CreateProcess path or suspended-but-unassigned startup would reopen process-custody gaps. Unbounded blocking pipe workers would not preserve the documented cancellation and handle-lifetime behavior.
