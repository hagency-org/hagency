---
kind: decision
id: ADR-043
title: Persist exact private owner authority before one-shot native approval application
status: Accepted
---

## Context

Native tool approvals require durable private owner authority tied to the current task, dispatch, room and device generations.

## Decision

Schema 13 provides the durable authority boundary for REQ-OWNER-UI-APPROVAL and
REQ-EXECUTION-AUTHORIZATION. It builds on ADR-003/005/028/039 and existing canonical
task/dispatch custody. It does not enable a native runtime permission response,
Matrix card sender or verdict crypto adapter. A persisted approval is not proof
that the native process received it, executed its tool or completed the task.

An authenticated host observes the current project-owner private approval room.
Owner, server, project and registered approval-bot identity come from allocation
and registration truth; the observation supplies actual joined members, encryption,
invitation policy, device identity and room generation. A positive snapshot requires
exactly owner and approval bot, encryption and invitation-only access. The full
bounded snapshot is stored once per server/room. Several Agents in the same project
may share that room, with independent Agent binding incarnations. Sharing cannot
substitute another registration, owner or project. The existing registered project
owner/private-room policy is retained; generalized owner rebinding remains separate.

Current negative evidence retires availability and saved grants immediately,
including when the reporting adapter has not advanced its generation yet. A third
member, missing/unknown room, encryption loss or invitation-policy loss reported
through Agent B also fences Agent A. Same-generation positive mutation/restoration
is refused. Replaying a previously safe observation cannot restore availability;
restoration needs a new room generation and refreshed per-Agent incarnation.
Registration, project owner/private binding and engagement changes revoke grants
with database triggers. Restoring the previous owner does not resurrect them.

The host freezes execution context under an exact current dispatch capability:
connection, upstream thread/turn, canonical task epoch, verified Matrix session,
private binding, workspace resource/path, path flavor, environment and mayWrite.
The repository verifies the actual current resource lease; a supplied mayWrite
boolean is never evidence of write custody. Writable context requires the exact
exclusive lease and a clean workspace. Context cannot be mutated by replaying its
ID. Upstream metadata must match the host's expected thread, turn, item and
explicit environment; echoed runtime fields cannot define those expectations.
No context/request/verdict/grant observation DTO implements Deserialize and no
Agent HTTP command constructs it.

Request identity includes the original upstream JSON-RPC ID and host connection.
Numeric and string IDs are distinct. Immutable request digests bind full input,
context, expiry and source identity. Parameters are size/depth bounded before
canonical processing. Scope candidates come only from core execution::derive and
its shared vectors: exact command/context, structured network host or explicit
permission profile. No prose-domain inference, shell-prefix parser or broad native
session rule is introduced. Unknown writable scopes offer only once/deny.
Read-only contexts currently admit only representable network-only requests;
unknown, command or additional-permission escalation is refused because this
slice cannot prove it preserves confinement. YOLO contexts are refused entirely
until persisted operator policy and an effective runtime adapter are integrated.

Request admission and dispatch parking commit together. A matching saved grant
creates a per-request decided state, still parked and unconsumed. A structured,
decrypted private owner verdict must match full sender MXID, server, room, binding
incarnation and exact request digest before expiry. Ordinary text has no verdict
constructor. The verdict receipt, request decision and any task/always grant commit
atomically. A duplicate source event retrieves the same record without creating
another grant; changed content conflicts. Console/public summaries contain only
request ID, state, choice and representable-scope marker. Grant summaries expose
only grant ID, static scope kind, mode and revoked status. Raw owner room, workspace,
upstream/source IDs, command and input remain host/private-card metadata.

Grants bind the exact Agent allocation incarnation, registration, project, owner,
private room/device generations, normalized workspace, resource, environment,
mayWrite and exact derived scope. Task grants additionally bind canonical task and
execution epoch, and are permanently revoked on Done or epoch change. Always grants
can survive a completed dispatch/restart for that same current context; they do not
transfer to another Agent or changed workspace/environment. Every reuse creates a
fresh one-shot request. Explicit revocation is rechecked before consumption,
including revocation after a decision was saved. It cannot undo an operation whose
allow decision was already consumed or permissions already held by a native turn.

Consumption revalidates capability, lease, task, binding, scope and grant. Expired
pending requests can produce one exact deny only while the dispatch remains current;
expired capability or retired task/binding yields no application authority. The
transaction persists Applying before returning the host application descriptor.
That descriptor binds exact upstream request/thread/turn/item and allow/deny. A
second consumption always fails, even if the caller lost the first response.
No method re-arms an Applying, Uncertain, Applied or NotApplied record.

### Accepted router-authorization amendment (2026-09-11)

The root reviewed the retained REQ-TSS-APPROVAL-RECONCILE and original router's
decision-then-resume-then-write path and approved the domain-only task
`specs/task-rust-approval-router-authority.spec.md`. Schema22 separates exact
durable router authorization, response admission/transmission and independent
native application observation. The new ledger creates no response authority
from historical schema13 Applying, Uncertain or Applied records.

First atomic decision consumption returns one opaque response grant bound to the
original repository instance and complete original capability/context. Response
admission borrows retained grants and marks their local one-shot attempt before
queueing. Inside the original Immediate transaction, fresh time and the original
deadline guard full capability, private owner/binding, task epoch, workspace,
lease, expiry and grant checks. Only when all currently admitted barriers have
exact router authorization may response admission persist response-may-send and
resume the same retained attempt. A positive acknowledgement is required before
the original owner sends its exact typed response. Lost acknowledgement cannot
recreate that grant or authorize another attempt. A valid live owner deny may
resume ordinary sandboxed continuation; it grants no escalation.

Generic unpark cannot bypass response admission. Genuinely parked attempts still
cannot mutate tasks. New requests repark and create a fresh barrier. Unknown
transmission removes current continuation authority and retains custody; receipt
inspection and future native application evidence never rearm sending. A local
write observation consumes the grant's local transmit admission before its
receipt is awaited; only historical observation settlement remains possible.
An original never-written frame may use a readonly admitted-grant recheck after
a fresh callback barrier resolves, retaining its original deadline. Domain
request expiry remains independent; runtime response reserve cannot revive an
expired decision. A local write/flush or callback resolution is not native Applied. Actual runtime control
pumping, response transmission and finite parked maintenance remain separate
implementation gates; this amendment changes no runtime timeout or sandbox.

A separate authenticated host observation records whether the exact native response
was applied. It must match the full persisted application descriptor and bounded
evidence; altered upstream ID or decision fails. Lost response, restart or observed
unknown outcome preserves uncertainty. NotApplied remains terminal rather than
silently permitting another allow. The host may later record actual Applied truth
for an old uncertain operation without reviving dispatch authority. Native
application observation is evidence only and never resumes a dispatch. Current
router continuation follows the separate one-shot response-admission boundary
above; unconfirmed native application alone does not revoke a current exact
router decision. Parked/unknown work retains existing workspace custody and
cleanup rules.

The single DomainRepository writer commits all state. Admission caps are 16 live
requests per dispatch attempt, 64 per Agent and 1,024 globally; retained contexts,
requests, grants and verdict receipts each stop at 100,000. Exact request retries
retrieve their receipts at capacity. Metadata is bounded to 48 KiB and private room
membership to 1,000 entries/48 KiB. This is bounded backpressure, not operational
retention/compaction parity. Fresh schema upgrades create no approval binding or
grant from legacy rows.

Remaining gates are explicit: real authenticated Matrix sync/verdict decryption,
late-key custody, one-time-key/device maintenance, private card delivery, supported
native decision mapping, connection cancellation and immediate validation before
external I/O, runtime application inspection after a lost response, operator YOLO
policy, sandbox enforcement, graphical controls and deployment cutover. Native
runtime decisions remain unsupported until that adapter exists. Task-notice sending
retains ADR-038's separate unresolved send-custody gate. No model, service, credential,
production room or runtime process was used or changed in this slice.

## Consequences

### Accepted fresh-clock prerequisite (2026-09-11)

The root-approved eight-path task `specs/task-rust-approval-fresh-clock.spec.md`
requires production binding, request admission, direct and SDK verdict admission,
consumption and application-observation authority clocks to be sampled after the
original writer acquires its SQLite Immediate transaction. Explicit repository
timestamps remain deterministic fixture inputs. Queue and lock delays cannot
extend capability, request or grant authority. Historical application settlement
remains distinct from current permission to resume execution. All six production
clock callbacks now run under the original Immediate lock. Actual contention and
queue tests pass; restoring the former pre-lock wrappers makes the three actual
lock test groups fail by accepting expired binding, consuming expired allow and
resuming expired execution. Final verification passes 52 store unit tests,
19 approval tests and eight permission-coordinator tests, with zero failures or
ignored cases. Both packages pass all-target warnings-denied Clippy. Strict
agent-spec 1.4 lifecycle passes the eight-path boundary and five selectors, each
running one actual test (6/6); one advisory file-output lint warning remains.
This prerequisite does not enable the owned approval coordinator, parked renewal
or production M6 parity.

The single domain writer records bounded one-use decisions and exact replay receipts. Persistence alone proves neither runtime application nor tool execution, and card delivery remains separate.

## Alternatives Considered

Public-room text, runtime-generated verdicts or reusable unscoped grants would bypass owner consent. Treating an approved row as proof of process application would erase the unresolved adapter boundary.

### Accepted owned maintenance scope (2026-09-11)

The root approved the exact 28-path owned coordinator contract after the schema22
router-authority prerequisite. A fresh original writer may mint one opaque owned
approval scope only for the first context of the exact dispatch/fence. It binds
its current fingerprint, private context, original repository instance and the
absolute operation end captured before parking. Existing or legacy context rows
cannot mint a later bound. Separate approval maintenance may keep only that same
lease current within its original capability, operation and unresolved request
expiry. It samples time after the original Immediate lock and never mutates a
task, resumes execution, revives expiry or admits a response. Generic Started
renewal and schema22 response-grant authority remain unchanged.

The active contract is `specs/task-rust-owned-approval-coordinator.spec.md`.
Implementation and qualification are in progress. No schema or private Matrix
intake behavior changes in this partition.

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
