---
kind: decision
id: ADR-053
title: Bind one owned native session to exact durable dispatch authority
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
---

## Context

Owned native execution must attach one actual process session to the exact successful durable dispatch start and preserve uncertainty through cleanup.

## Decision

### Boundary

`hagency-execution` is a host-only library between DomainStore and ADR-040/044's
OwnedSession. Runtime remains database-independent. One operation consumes an
already claimed capability, explicit trusted host configuration and finite
limits. No HTTP route, CLI service, Matrix transport, scheduler or catalog enables
it. The host must exclusively consume a capability and supply a trusted guardian
and Codex executable. It cannot use this API as a general command endpoint.
There is no second launcher: the fixed argv is `app-server`, through the existing
SupervisedProcess and its prepare/start or atomic Windows Job Object boundary.

The explicit environment, guardian/executable paths and workspace resource map
have no Deserialize implementation. Host model/effort/cwd settings are constructed
from a writer-produced scope, never runtime text. The map has at most 16 entries;
this first integration accepts exactly one exclusive workspace resource. It
checks bounded identifiers, absolute canonical existing paths and the unchanged
Launch validator. Exposing that existing pure validator does not add a launcher
or effects before admission.

These are logical path checks, **not physical directory custody**. A rename,
symlink or ancestor replacement between checks and Launch.directory remains
unqualified. Offline fixtures use newly created fixed host-owned directories.
Any further host must protect each workspace and its ancestors from replacement
throughout the operation; this slice does not establish that protection, effective
sandbox efficacy, credential isolation or safe production workspace provisioning.
Native service availability remains false.

### Exact writer scope and durable start

OwnedDispatchScope has private fields and no runtime JSON constructor. The writer
checks the existing runner secret/fence, lease and capability expiry, active
engagement/registration, session route/current-task and graph authority. It loads
the frozen DispatchInput, current canonical task/epoch, every actual resource
lease and the resource snapshot from the completed provision effect. Resource
settings therefore do not drift with a mutable catalog record. The provisioned
resource ID, registration generation and runtime name must match. Dirty or
missing/mismatched leases refuse admission. Report-only recovery and an already
Done task before start are unsupported in this first slice.

A canonical digest includes those exact immutable inputs and bindings. Task
status and heartbeat may change through already authorized task operations; task
identity/epoch may not change for this attempt. Preflight has no child effect.
Start opens the existing immediate transaction, reads the actual writer clock
**after both queue delay and SQLite lock acquisition**, checks the scope, invokes
the existing start logic and rechecks it before commit. This preserves the
existing one-shot Started transition and canonical Created/Accepted to InProgress
behavior. The existing start/complete bodies are factored into private shared
transaction functions, without a second kernel implementation or schema change.

Only a successfully received start receipt permits child creation. A timed-out,
closed or cancelled start response is never repolled or treated as proof of
start. Negative reconciliation authenticates the historical attempt, so a
committed-but-unacknowledged start is quarantined while a command never executed
can relinquish its clean unstarted lease. Neither case launches a child.

### One retained execution worker

Each Operation owns one OS worker, one current-thread Tokio IO runtime, one
atomic cancellation signal and one bounded result slot. There are no detached
reader/cleanup tasks or unbounded channels. Windows also retains the already
qualified process-wide private completion reactor from ADR-044; this crate adds
no second reactor. Global admission/concurrency budgeting remains future work.

The worker pins each native operation across periodic authority checks. It never
drops and repolls a partially executed native future merely to check authority.
Every check renews only the exact current dispatch lease, capped by the existing
capability expiry, with the fresh writer clock after queue/lock delay. A revoked,
expired, replaced, dirty or quarantined scope stops the actual retained owner.
Default typed settings remain `on-request`, `workspace-write` and network disabled;
chosen model and effort come from the frozen provisioned resource. JSON fields
named task_id/cwd/model/done inside the frozen prompt remain informational text.
A peer echo does not establish effective sandbox behavior.

The wrapper sends initialize, thread/start and turn/start through actual owned
pipes. It drives one thread and one turn only. Typed protocol correlation and
bounded queues/text/stderr are inherited from ADR-032/034/036. Any server approval
request is explicitly unsupported; the existing typed authority/application
coordinator is not bypassed. There is no implicit approval grant. MCP launch
configuration and an actual helper Done round-trip are separate remaining work.

### Cancellation, timing and observability

The operation deadline is absolute monotonic time, 100 ms through 30 seconds.
Native response/write waits are 10 ms through 2 seconds and cannot exceed that
operation limit. Cancellation checks are scheduled every 20 ms; authority renewal is scheduled
every 100 ms while a native operation is pending. A pending writer receipt may
take up to its two-second bound before authority becomes unknown. Always-ready
updates recheck the deadline before the next operation. DomainStore retains its
existing two-second receipt deadline, bounded queue/byte budget and 100 ms SQLite
busy timeout. This change does not increase those production defaults.

Existing platform startup handshakes allow five seconds. OwnedSession stops allow
three seconds per attempt; failed operation, explicit stop, failure reconciliation
and final drop can each retry. Supervisor final drop adds at most three seconds
(two seconds for the Windows OwnedProcess fallback). Execution plus these bounded
waits and final two-second settlement/negative receipts has a conservative
**60-second combined wait allowance** per Operation including join. Cancellation
can be delayed by an in-progress synchronous platform call and its retained stop.
This is a bound on library waits under ordinary OS scheduling, not a hard realtime
promise about a stalled filesystem/kernel syscall. There is no unsafe thread kill
or timeout that discards process custody.

Dropping a polled wait future sets cancellation, leaving the result and worker
retained; a fresh wait can retrieve that result. It does not repoll the cancelled
native/domain future. Dropping Operation closes its receiver before joining the
worker, so failure to deliver its report drops the retained owner on that worker.
Join is synchronous and must not run on a latency-sensitive HTTP/UI worker. An
explicitly returned Report may retain an unresolved owner; retry_stop and report
drop also retain the existing synchronous stop behavior.

A private Report separates protocol outcome, exact cleanup observation, last
writer-observed canonical task status, settlement outcome, failure and bounded
upstream text. It has no automatic Serialize/Debug/Matrix/console projection.
A failed negative receipt retains the original capability and DomainStore for an
explicit bounded negative-only retry. Cancelling that retry preserves its handle.
It never retries successful settlement or authorizes clearing dirty leases.
Dropping a report does not silently certify an unresolved cleanup or settlement.

**Amendment (2026-09-12) — attributing a bounded reply expiry.** The two-second
`DomainStore` receipt deadline is unchanged, and this amendment raises no
production bound. A hosted eight-way probe showed the runner task surface
returning 504 (`Error::OutcomeUnknown`, the runner failure mapping) for commands
whose expiry was caused by queue contention rather than by a stalled writer, and
the artifact could not distinguish the two. `DomainStore` now records, per
expiry, whether the writer had begun that command: one monotonic ticket per
call, published when the dequeued job starts running, compared by equality when
the bound expires, exposed as `DomainStore::last_unknown_dequeued()` and
projected into the runner's refusal code as `outcome_unknown_running` versus
`outcome_unknown`. The verdict is deliberately single: a command that is still
queued has not been withdrawn and may commit after the caller returned, so it
remains `OutcomeUnknown` and "reconcile before retrying" stands unchanged. The
new code is a diagnosis for the operator and grants no retry, reply, lease or
completion authority. Deciding whether to schedule the receipt wait after
dequeue is explicitly deferred: it would change when the bound starts, so it
needs its own decision and its own evidence.

### Settlement and historical fencing

Protocol Completed alone cannot mark a canonical task Done, send a Matrix reply,
release a lease or prove clean child termination. Successful dispatch settlement
requires Completed plus a retained stop report proving whole_tree_stopped,
leader_exited and signals_accepted. macOS's whole_tree_stopped=false is preserved:
its successful upstream text still yields cleanup-unknown quarantine and a held
lease. Startup uncertainty, EOF, unsupported approvals, cancellation, stale
scope and other failures also preserve or establish quarantine after Started.

After known full stop and a final cancellation checkpoint, the worker submits one
fresh authorized writer settlement. That bounded commit is intentionally not
raced against later cancellation: cancellation after this commit checkpoint is too
late to undo it. Fresh scope/expiry/revocation/dirty checks still run atomically
inside the writer. Lost settlement receipt remains explicit and is never retried
as success. A subsequent negative observation may reveal AlreadySettled but is
not represented as a recovered successful response. Canonical status is observed,
never promoted to Done; no final reply is created.

The domain's existing 32 KiB output ceiling remains. Protocol text may be larger
(up to its own 64 KiB bound); oversized settlement fails visibly instead of
truncating final text or freeing custody. Historical negative evidence requires
the original attempt's runner ID, fence and capability hash even after expiry or
revocation, and is bounded to 128 observations per attempt. It can fence only the
same current attempt using the existing lifecycle kernel. A replacement attempt
or already resolved older fence is untouched. Unknown started/parked work retains
its leases and marks exclusive workspaces dirty; no negative method can clear
quarantine or dirty resources.

### Evidence and remaining qualification

Fresh repository tests exercise frozen settings, exact input/scope, one-shot start,
dirty leases, revocation/expiry, forged secrets and historical replacement fences.
A real DomainStore test uses bounded private reply-channel gates before and after
a real start commit. A library-test-only response discard then drives the actual
coordinator's normal reconciliation and proves no child launch after receipt loss.
That receipt-discard seam is absent from a normal library build; it neither
manufactures a commit nor skips reconciliation.

Separate fresh DomainStore plus actual native subprocess tests traverse the real
owned pipes with frozen Unicode cwd, model/effort and hostile informational prompt
keys. They cover Completed versus canonical InProgress, actual cancellation and
Drop, expiry/revocation, failed spawn, EOF, wrong scope, unsupported approval,
absolute deadline and held-writer admission failure with explicit retry. The
existing owned-pipe tests also retain descendant, stream-close, mid-write cancel,
EOF, silence and noisy-stderr coverage. Offline fixture requests use the fixed
app-server argv; only the explicitly supplied host test environment selects the
fixture mode. No live model, credential or network service is used.

Local execution evidence is macOS only. Linux and Windows require this commit's
actual CI execution, even though their earlier owned-IO foundations have separate
CI evidence. Physical workspace provisioning, actual Codex/model qualification,
approval application, helper launch/auth, input ACK parity, final Matrix delivery,
warm reuse, terminal mode, global budgets and full M4 remain open. POSIX/macOS
unsupported crash-containment guarantees are unchanged; this integration does not
activate the optional cgroup recovery path or claim simultaneous custodian loss.

## Consequences

One retained worker owns cancellation, domain reconciliation and the actual process handle. Upstream completion remains distinct from canonical Done, reply custody and platform qualification.

### Original runtime observation amendment

Before stop or reconciliation can discard the original OwnedSession, Report
copies its last entered operation stage, typed Unknown session reason and first
transport termination cause with optional original pending counts and unfinished
write byte counts. The immutable observation contains no request identity, text,
stderr, path or authority token. A missing transport termination stays absent;
normal cleanup must not replace it with HostClosed. Later retry_stop preserves
the same snapshot even when proven whole-tree cleanup removes the process owner.
The existing failure, protocol, cleanup and settlement remain independent facts.
This observation diagnoses an original attempt; it cannot authorize replay,
settlement, cleanup or a longer execution interval.

## Alternatives Considered

Launching before the original Started acknowledgement or reconstructing authority after losing that response could execute unowned work. A general command endpoint or detached cleanup path would bypass the fixed host operation.

### Accepted retained approval owner and capacity (2026-09-11)

The root approved the exact 28-path owned coordinator task. Its original
OwnedSession is retained in Report immediately after spawn and before every
startup or binding await. The immutable RuntimeObservation is still captured
from that same owner before coordinator stop/removal; an earlier runtime error
guard may already have stopped it while retaining the original first cause.

One application-owned ApprovalHost supplies a shared finite live budget and a
smaller parked budget to every enabled operation and the existing domain claim.
A parked reservation precedes actual request submission and remains owned across
unknown acknowledgment. Unknown process cleanup cannot manufacture a free live
slot. The original 30 second operation and 2 second response ceilings remain.
The enabled coordinator uses separate original-scope maintenance while genuinely
parked, preserving task refusal and the existing generic Started renewal.

Existing UsageRun custody remains one pending observation: process its real
receipt before reading another update while retaining any older pinned control
future. There is no canceled-read replay or unbounded event backlog. The bounded
ID/owner-cutoff notice receiver carries no verdict or private callback payload;
application and private SDK wiring remain separate. Implementation and the eight
selectors in `specs/task-rust-owned-approval-coordinator.spec.md` are in progress;
no actual provider application or full M6 qualification is claimed.

### Coordinator partition qualification (2026-09-11)

The accepted task is implemented and qualified across its final29 paths, which
include only two exhaustive bootstrap failure labels beyond the original28.
Final strict lifecycle passes the full boundary and all eight selectors (9/9),
with11 distinct actual regression tests. Complete affected-package records pass
316 tests, plus the bootstrap projection. Native warnings-denied Clippy and
Windows GNU all-target compilation pass; actual Windows execution and complete
private SDK/MCP service integration remain outside this partition. Earlier failed
fixtures and the pre-fix usage-slot negative control remain preserved in the
external migration cache. No synthetic Applied or production-cutover claim is made.
