---
kind: decision
id: ADR-036
title: Bind one Codex turn to typed host-owned lifecycle state
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
---

## Context

A native Codex session must correlate one upstream turn and its terminal observations before host execution authority can be attached.

## Decision

### Scope and source

This M4 slice wraps ADR-032's Connection and ADR-034's owned-stream Driver with
`SessionDriver`. It has no domain store, child process handle, Matrix credentials,
lease, task identity or approval capability. The native server does not construct
it. Native Agent execution remains disabled, and this is not runner or sandbox
parity. The wrapper admits one upstream turn, then closes its disposable transport.
It has no second turn, warm reuse, reconnect, automatic resume or replay operation.

Wire names are checked against the existing local Codex 0.153.4 export from
`codex app-server generate-json-schema` and the official App Server documentation
at https://learn.chatgpt.com/docs/app-server. Tests record selected schema property
names, required fields, policy definitions and source file SHA-256 hashes in
`tests/fixtures/codex-session-0.153.4.json`. No child or model is started to produce
these fixtures. The typed implementation validates the correlation, policy and
lifecycle subset it uses; it is not a complete validator of every upstream DTO.

### Host inputs and fixed policy

Settings are constructed by Rust host code, without Deserialize or an arbitrary
configuration map. They require an absolute UTF-8 cwd of at most 4096 bytes, with
no parent components or control characters, an explicit model of at most 128 bytes,
and an explicit effort of at most 32 bytes. Model and effort must be nonempty and
have no surrounding whitespace or control characters. The installed effort schema
is an open nonempty string, so this layer does not invent a closed model-specific
enumeration. The host must separately resolve actual filesystem and model access.
Cwd validation does not establish canonicalization, existence or process custody.

Thread start/resume sends `sandbox: workspace-write`, `approvalPolicy: on-request`
and `approvalsReviewer: user`. The host may select the narrower `read-only` mode.
Turn start uses the schema's distinct camelCase `sandboxPolicy` representation,
`workspaceWrite` with the host cwd as its explicit writable root, or `readOnly`,
and explicit false `networkAccess`. Both requests carry the same cwd and model;
turn start additionally carries the chosen effort and a bounded text input.
Input text is only a `text` UserInput with `text_elements: []`; its content cannot
supply configuration, approve a request or complete a canonical task. No YOLO,
network enablement, reviewer substitution, config override or tool-output input
exists in this typed interface.

Fresh threads are ephemeral and name the `hagency` service. Explicit resume takes
a separately constructed `ResumeThreadId`, requests `excludeTurns: true` and
requires that exact upstream ID back. This is preparation for a future host
adapter, not authority to resume an outcome-unknown dispatch. Both start and
resume validate exact returned cwd/model, thread cwd, idle thread status and an
empty returned turn list. Active, substituted or historical sessions are refused.
The reported approval and sandbox policy must match. Schema-defaulted false
network access and empty writable roots may be omitted. Reported widened roots,
network or policies fail. Echoes cannot prove the effective sandbox: Codex defaults,
configuration loading, temp-directory behavior, runtime/platform qualification and
actual process restrictions still require independent tests and custody checks.

### Correlation and bounded observations

Initialization must finish before a thread opens, and its typed turn starts only
after that thread response. Request IDs remain exactly correlated by Connection
and are checked again with the expected operation at the wrapper boundary. The
returned turn ID is immutable for this wrapper. A terminal status in a start RPC
response alone is not a terminal notification and cannot produce completion.

Notifications can precede the matching response. At most 16 are deferred, under a
separate 1 MiB budget that counts their complete serialized payload and retained
metadata, using the same accounting as the transport. While opening a thread,
only thread lifecycle or benign global notices may race its response. Already-known
thread or turn identities are checked immediately. All deferred scopes are checked
against the response before any item/text activity is applied or exposed. A resume
cannot silently adopt a different session. Invalid or overflowing deferred data
closes the wrapper; actionable requests are not silently dropped as successful.

All scoped thread, turn and item events must match the current identities. Item
starts retain identities through completion, so duplicate starts, ID reuse,
unknown completions and deltas after completion fail. There are at most 128 items,
4096 observed notifications and 64 KiB of aggregate agent-message text. Delta text
is bounded before appending. A completed agent message must preserve the previously
observed text prefix and its established message phase. Completed final messages
are joined in completion order, with the separators also counting toward the final
64 KiB ceiling. Commentary is retained within the same cap and excluded from the
final result. Optional/null phase follows the installed schema and is treated as
an unclassified final message. Any overflow visibly fails; no final result is
silently truncated.

Known non-message item kinds and a narrow progress notification set produce typed
activity observations only. They do not expose tool payloads or gain execution
rights. Unsupported settings, auto-review and other notification features fail
closed. Upstream error strings and private stderr are not automatically logged or
serialized. Stderr remains ADR-034's private bounded tail with a total-byte count.

### Terminal ordering and transport limits

Completed, failed, interrupted and unsupported-request are distinct observations.
An unsuccessful RPC records its error code and a failed upstream outcome. A
retrying upstream error is activity only; the host does not send a new request.
Interrupt uses the current exact thread/turn pair and allows one explicit request.
Its successful empty-object RPC response records acknowledgement, not interruption
or task completion. Rejection also leaves the turn running until a later terminal
observation or transport failure.

A terminal event first validates current item and text state. The wrapper then
checks the already-received suffix, consistently across its deferred queue and the
transport's queued/unparsed buffers. Same-thread idle status, benign notices and
resolved-request bookkeeping can follow a terminal event. Another turn, wrong
scope, contradictory activity or actionable server request cannot be discarded as
ordinary success. If the received suffix contains a partial frame, the wrapper
finishes that already-started frame under one absolute Instant-based drain deadline
(the host's RPC timeout), additionally limited by the existing transport lifetime,
partial-frame and event deadlines. Each loop checks time and yields. It does not
begin a new read when the received snapshot is empty or wait for hypothetical
future notifications. This accepts a normal completed-plus-idle stream independently
of its packet splits; an unfinished suffix eventually returns unknown, and a
received wrong-scope suffix fails. No claim is made about bytes that arrive after
the disposable transport has closed.

The transport's existing bounded read buffers, event queue and private stderr
remain in force. Deferred reads check its original monotonic deadlines before
using buffered activity. Cancellation of any started wrapper future, including a
future paused between lower-level IO calls, synchronously poisons typed state and
closes owned streams through an operation guard. IO loss, EOF, timeout, malformed
or out-of-scope data yields an explicit unknown outcome. It never reopens a stream
or replays a request. The transport retains its first termination record for the
future host's reconciliation. An upstream completion followed by failed terminal
validation is not silently promoted to successful completion.

All server requests are explicitly unsupported while the authority adapter is
absent. The wrapper sends the protocol's -32601 unsupported-handler error, then
records `UnsupportedRequest` and closes. Failure to deliver that error remains a
transport-unknown outcome. No approval request can be accepted by event fields,
model text, a claimed auto-review result or a successful transport write.

### Verification and open integration gates

Offline fixtures use real bounded Tokio duplex streams and schema-shaped payloads.
They cover fixed settings, both sandbox modes and omitted defaults, exact resume
and policy rejection, early notifications, stale IDs, item tombstones, text/item/
event/deferred limits, terminal contradictions, interrupt ACK and rejection,
unsupported requests, RPC failure, EOF, cancellation and elapsed deadlines. The
same completed-plus-idle stream is tested at every byte split, as well as deferred
before interrupt ACK, coalesced wrong-scope tails and an unfinished tail timeout.

Still open: actual child/guardian stdio handoff and verified input acknowledgement;
effective sandbox and installed-runtime qualification on every platform; host-
authenticated thread/dispatch binding; permission grants and approval delivery;
MCP/Agent integration; authoritative usage and billing; process-tree stop and
inspection; durable task transitions, resource custody and final delivery. A
Completed observation proves none of these. Leases and canonical tasks remain
entirely under the existing host authority. One-turn support without production
warm reuse or automatic resume is an explicit remaining M4 integration limit.

## Consequences

Typed settings and finite lifecycle state constrain the disposable transport. A Completed observation does not settle tasks, release leases or prove sandbox and process-tree behavior.

## Alternatives Considered

Warm reuse, automatic resume or reconnect would require additional identity and replay contracts. Accepting textual completion or unsupported server requests would bypass the documented typed one-turn boundary.

## Optional finite approval control mechanics

The runtime-only M6 control API adds `enable_approval_control`, synchronous
`prepare_approval`, `next_observed_or_control(Pin<&mut F>)` and
`send_prepared_approval(&mut PreparedApproval)`. The generic control future belongs
to the host and may hold a separate admission batch of unique durable grants.
The session, pending-callback map and prepared frames remain separate retained
fields, so a new callback or usage update can be handled without cancelling the
future borrowing that batch. No store type or database operation enters runtime.

Every successful typed result uses the same update validation, opaque source and
contiguous observation sequence as ordinary reads. A control result mints no
observation. Prepared-send update returns also mint the next original observation;
the host must deliver it before any further response admission. The prepared
object has no Clone/Deserialize/reconstruction API and binds the exact live driver
instance as well as callback ID. Numeric and string IDs remain distinct. Callback
reservation blocks repeated preparation. Observed resolution makes an unsent
response unusable; a foreign driver cannot use it even with identical textual IDs.

Runtime opt-in and frame preparation grant no write authority. The host must
first obtain durable router authorization, retain the original frame before
awaiting response-begin, receive its positive acknowledgment, and compare the
exact current grant/connection/request scope. If a returned update reparks the
attempt, the host must process it and recheck the ORIGINAL admitted grant before
continuing that original unsent frame. A lost acknowledgment never authorizes
sending or rearming begin. Runtime `WriteAccepted` is only the complete write and
flush receipt; `serverRequest/resolved` remains independent of native permission
application. Domain transition semantics belong to the separate approval contract.

`approval_deadline(id)` exposes the original owner-decision bound, and the original
PreparedApproval exposes its immutable `response_deadline()`. Preparation must
finish before owner expiry. A positive durable response-begin acknowledgment may
arrive after owner expiry but before the fixed response bound; no caller-derived
new timestamp is substituted. Unprepared callbacks never acquire this margin.
The boxed opaque Observation in either update variant is the same receipt object,
not a new provenance source or sequence. A host unwraps it once for usage capture.
