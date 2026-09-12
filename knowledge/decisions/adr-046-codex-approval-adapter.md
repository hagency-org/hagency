---
kind: decision
id: ADR-046
title: "Typed Codex approval coordination before native cutover"
status: Accepted
---

# ADR-046: Typed Codex approval coordination before native cutover

- Status: Accepted for offline implementation; live native approval cutover remains gated
- Date: 2026-09-10
- Requirements: REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-RUST-MIGRATION-EXECUTION
- Related: ADR-032/034 native protocol/session, ADR-039 execution scopes, ADR-043 durable owner approvals

## Context

Native Codex permission callbacks must consume an exact durable owner decision without mistaking protocol resolution for actual permission application.

## Decision

Keep `hagency-runtime` independent of the domain database. The new
`hagency-permissions` crate owns one validated Codex `SessionDriver`, its current
host capability, and the schema13 `DomainStore` approval API. No HTTP setter or
runtime JSON field can select the host connection, Hagency identity, owner room,
workspace lease, or verdict.

The host explicitly attaches the coordinator after thread/turn startup. The
coordinator derives exact upstream thread/turn and cwd/read-only settings from
that session; the host supplies opaque context/connection/workspace-resource IDs.
The repository then proves the current full Matrix/private-owner binding,
canonical task epoch, dispatch fence and workspace lease. Writable contexts need
an exclusive clean lease; read-only contexts cannot become writable through an
upstream claim. Multi-environment and YOLO configuration remain refused.

The default `SessionDriver` and `OwnedSession` behavior continues to reject every
server request. Opt-in admits only bounded pinned request types. It emits a
private parsed event, then the coordinator asks the single repository writer to
persist the request and park its dispatch. Owner verdicts remain a separate
authenticated private-room host observation, including all schema13 revocation
and shared-room fences. Runtime requests never synthesize owner decisions.

`apply` consumes the exact durable decision before constructing/sending any
response bytes. The application descriptor is checked against the owned
connection/request/thread/turn/item. Runtime's typed response object has no
Deserialize or arbitrary-result constructor. The connection checks the complete
original method/params and exact JSON-RPC ID and permits one response, retaining
the pending scope until upstream resolution. Numeric and string IDs are distinct.
Schema13 cannot represent negative numeric IDs; the coordinator refuses those
without string coercion. Requests are bounded to 64 KiB, at most 16 retained
coordinator approvals per turn, and existing connection/event/deadline bounds.

## Pinned request-specific responses

Local `codex-cli 0.153.4` generated schemas were inspected offline with
`codex app-server generate-json-schema --experimental`. The official release tag
`rust-v0.153.4` resolves to commit
`3d2ee51ca2d5db578f328aa75e20aa22c0197c9a`.

| Request | Once allow | Deny |
| --- | --- | --- |
| `item/commandExecution/requestApproval` | `{"decision":"accept"}` | `{"decision":"decline"}` |
| `item/fileChange/requestApproval` | `{"decision":"accept"}` | `{"decision":"decline"}` |
| `item/permissions/requestApproval` | exact requested supported profile with `"scope":"turn"` | empty profile with `"scope":"turn"` |

An owner “once” verdict for a permission-profile request authorizes that explicit
turn-scoped profile once; it does not claim each subsequent command will ask
again. Task/always choices remain Hagency's scoped durable rules, never Codex
`acceptForSession`, session profiles, exec-policy amendments, or network-policy
amendments. Command requests offering no exact accept/decline pair, writeStdin,
file grantRoot, filesystem special/glob/deny entries, malformed/future fields,
and unsupported methods fail closed. Literal file paths are shape-checked by the
runtime; reusable scope normalization stays exclusively in core/store.

File and explicit permission requests can precede `item/started`. Their exact
callback item is bound as upstream correlation data without inventing an active
item or converting that value to host task/session identity. The pinned
[request creation paths](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server/src/bespoke_event_handling.rs#L640)
show why requiring every item to have started would reject valid requests.

## Resolution is not application acknowledgment

This is established from source, not inferred from schema names. For command,
file, and permission callbacks, Codex awaits the response channel, sends
`serverRequest/resolved`, and only then parses the selected typed response and
submits the core operation. Errors, callback drops and turn-transition
cancellation also traverse this resolution path. See the pinned
[file/command handlers](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server/src/bespoke_event_handling.rs#L2011),
[permission handler](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server/src/bespoke_event_handling.rs#L1885),
[cancellation path](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server/src/outgoing_message.rs#L185),
and [resolution emission](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server/src/request_processors/thread_lifecycle.rs#L848).

Consequently flush and observed resolution never record native Applied. The
original offline coordinator records Uncertain and does not yet use schema22
router authorization; its execution integration remains a separate gate. Failed
writes and explicit close also record Uncertain. Dropping a started async operation closes the owned
session. If a database response or async uncertainty observation is lost,
Applying survives and becomes Uncertain on repository reopen; neither state can
be consumed again. Resolution before a decision closes the session so a later
owner approval cannot answer the cancelled callback. Stable duplicate owner
verdicts remain repository receipts, not permission to resend bytes.

No model text, item completion, turn completion, generic callback termination,
or empty stdin queue supplies native application proof. A host inspector or
upstream protocol extension would need to bind the exact request and selected
decision/profile to a core acknowledgment after actual application, distinguishing
cancellation. Merely moving notification after `submit` proves queue admission.

### Accepted router-authorization amendment (2026-09-11)

The root approved ADR043's schema22 domain prerequisite after comparing the
retained requirement for router application before allow delivery with the
original router implementation. Interactive continuation may use exact durable
router authorization and the original one-shot typed response while native
application remains unconfirmed. No post-core acknowledgement is required to
authorize that router continuation, and no Applied evidence is fabricated.

The retained original runtime owner must prepare its exact callback/response,
obtain positive acknowledgement of the fresh one-shot response-admission
transaction, then send that response once under its original bounded deadline.
Only that transaction can resume the same attempt after every current approval
barrier has router authorization. Resolution observed before response admission
cancels the callback. Uncertain transmission closes/fences the original attempt;
unknown consumption or admission acknowledgements never permit reconstruction or
resend. After known local write acceptance, callback resolution remains native
application-unconfirmed and does not by itself revoke current router authority.
Future application inspection only adds evidence; it cannot rearm a response or
revive parked, expired, retired or successor execution.

The active domain contract is `specs/task-rust-approval-router-authority.spec.md`.
Runtime pumping, finite parked maintenance, actual typed transmission, owner UI
integration and executable qualification remain explicit separate gates. No
timeout, sandbox, YOLO or production-cutover change follows from this domain task.

## Verification and limits

Regression fixtures use real SQLite/domain authority plus bounded duplex streams.
A writer inspects durable Applying on the first response-byte attempt. Tests
cover request-specific mappings, exact IDs/body/threads, default refusal,
revoked grants, read-only scope, failed transactions, duplicate application,
multiple pending approvals, resolution before/after decision, write failure,
future cancellation and restart. These prove domain/protocol sequencing; they do
not prove a live Codex permission was applied or an OS sandbox was enforced.

No Matrix cards/sends, automatic process attachment, live model, runner cutover,
new taskless path, or operational grant compaction is enabled. Requests racing
startup before the validated turn response remain refused. This is a bounded
integration seam, not completed native approval feature parity.

## Windows cancellation fixture correction

Native CI run 34539380960, Windows job 103078328251, failed at revision c0afefc
in `native_codex_approval_uncertainty_write_cancel_restart`: its final `seen`
assertion was false. The original log did not label the loop mode or capture the
instruction schedule. This establishes a failed fixture assertion, not a proven
production write-loss or shutdown defect. Every other original Windows test
target passed, including the three ADR062 owned Matrix workflow tests. The later
serial Matrix/Palpo diagnostic also passed; it did not rerun the failed approval
selector and does not replace the original failed verdict.

The cancellation branch had a concrete phase race: its 30 ms timer covered both
awaiting durable approval consumption and the blocked native response write.
SQLite can commit Applying before the awaiting caller receives its application
descriptor. Cancellation at that point correctly closes the session with
`seen=false`, although the test asserted a byte attempt must already have
occurred. The existing real `attach_and_consume_lost_response` fixture proves
that exact valid state by holding the transaction briefly, polling the actual
operation once, observing its real committed row, and dropping it without a
further poll. It passed both the original Windows job and the local check.

The corrected blocked-write case polls the real operation under the existing
2 second fixture handshake bound until WireGate verifies durable Applying and
reports the first write attempt. Only then does it begin the same 30 ms
cancellation interval. The future is dropped exactly once after timeout, with
no canceled-future repoll. Completion or failure before the gate is an explicit
test failure; mode labels identify the failed subcase. Closed-session, Applying,
restart-to-Uncertain and no-duplicate-application assertions remain intact.
Production runtime/store deadlines, permission semantics and cleanup guarantees
are unchanged. Local macOS success and Windows cross-compilation cannot establish
that the corrected fixture passes actual Windows CI; that remains an integration
gate. No missing Applied proof or native approval cutover is inferred.

## Consequences

Native application stays Applying or Uncertain until qualified application
evidence exists. The new domain layer permits exact router continuation after
its unique response-admission acknowledgement without claiming that application
evidence. The existing offline coordinator does not use this new authority and
cannot resume an approval-parked attempt. Owned runtime integration and actual
interactive qualification remain separate gates; offline mapping and
cancellation fixtures do not enable live approval cutover or weaken default
request refusal.

## Alternatives Considered

Treating serverRequest/resolved or a flushed response as Applied would contradict the pinned callback ordering. Session-wide acceptance or policy amendments would widen the recorded once/task/always scope mappings.

### Accepted original owned coordinator integration (2026-09-11)

The root approved a distinct execution-owned integration of the cooperative
runtime pump and schema22 unique router grants. Report retains original callback
frames, a separate admission batch and uncertain observations alongside the
actual OwnedSession. Every domain future stays pinned across control updates.
New observed callbacks must be persisted as barriers before any original unsent
response can continue. Local write acceptance consumes transmit admission;
callback resolution and native application evidence remain independent. The
old standalone SessionDriver coordinator stays an offline compatibility seam.

Only an explicit shared host policy enables this owned integration. Owner cutoff,
domain expiry, prepared response deadline and the original operation end remain
fixed; response reserve never revives an expired domain decision. Actual private
Matrix request delivery and encrypted executable qualification are separate
root-owned gates. No sandbox or timeout ceiling changes are authorized here.
The active 28-path contract is `specs/task-rust-owned-approval-coordinator.spec.md`;
implementation and qualification are in progress.

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

### Reconciling a lost write-acceptance record (2026-09-12)

The host's response frame may be physically accepted by the transport while the
domain call that records that acceptance fails under the store's bounded reply
wait. The operation previously reported `SettlementUnknown` with no way to tell
"the store never recorded it" from "the store recorded it and the reply was
lost". It now performs ADR-053's reconcile-before-retry step: exactly one bounded
read of `approval_response_summary` for the same request id, inside the
operation's remaining deadline.

The read is **not a snapshot**. It runs through the same single-writer FIFO queue
as the acceptance write, and that queue skips an enqueued job whose caller stopped
waiting. Once this caller's two-second wait has expired and dropped its receiver,
the read is therefore ordered behind the abandoned acceptance job's fate: either
that job was skipped (the row stays `response_may_send`, `write_accepted` 0) or it
had already executed (`write_accepted` 1). An acceptance write uses
`ReceiverPolicy::CancelIfDropped`, never `RetainEnqueuedInvalidation`, so an
abandoned acceptance job cannot execute *after* a negative read. The consequence
is stated plainly: **when the read answers it is conclusive; when it does not
answer at all (the writer is stalled) it is inconclusive** and the operation stays
`SettlementUnknown`.

The two halves of that ordering are proven separately and by different means, and
the spec scenarios say which is which. The *skipped* half — the abandoned job does
not execute, so no accepted row is manufactured from a lost reply — is proven on
the store side by `native_domain_acceptance_reply_timeout_reconciles`, driving a
real two-second reply-wait expiry with a parked writer. The *already executed*
half — a committed record whose reply was lost, so the reconcile continues on the
successful path — is reached by a test seam (`Fault::WriteAckLost`) that converts a
successful call's result to an unknown; it is not an end-to-end replay of the
timeout, because the host's own acceptance call cannot be made to hit the
two-second wait from the execution crate.

If the row reports `write_accepted`, the operation continues along precisely the
path a successful call would have taken, and the trace marks the reconcile. If it
does not, the operation reports `SettlementUnknown` with an `AcceptanceUnrecorded`
cause marker, because the ordered read answered and found no accepted row. If the
read itself is refused or does not answer in time, the operation stays
`SettlementUnknown` with the read's own refusal as the cause, and the original
write error is retained in the trace.

The frame is never re-sent and the acceptance write is never re-issued, so no
second frame and no double-write is possible. This amendment changes no bound, and
`write_accepted` continues to mean only that the store recorded the host's
*local* acceptance for that id — it is not runtime application, not peer receipt,
and never `Applied`; native application remains unconfirmed exactly as this ADR
states. The reconcile read is read-only and grants no retry, reply, lease or
completion authority.
## Amendment (2026-09-12)

ADR-046's two named states were not exhaustive, and the case between them was
cancel. The Windows trace (upstream 63c4ac9, run under eight-way load) records,
for every cancellation, an entry whose phases are

    retained, acknowledged, prepared, begun, admitted, checked, resolved-before-write

with `stage: Update` and no transport termination. The primitive is therefore
`serverRequest/resolved` observed for an entry that had passed
`check_approval_response` and had not recorded a write receipt, while the peer had
already read that entry's response frame. `turn/completed` is excluded: it closes
the wire, so its observation necessarily carries a termination cause.

The mechanism is an ordering hazard between two paths that are not atomic with
respect to each other. The session's event delivery
(`SessionDriver::receive` -> `Driver::next_event`) pops the transport's parsed
event queue unconditionally (`transport.rs`, the `self.events.pop()` arm) and is
not gated on whether a frame write is in flight; the write path returns its
receipt only after the flush is observed. A resolution parsed on one of the
host's pumps can therefore be consumed while the entry's frame is committed but
its receipt is not yet recorded. The previous code cancelled in that window.

An entry that has been committed to the transport is not cancellable by its own
resolution. Its frame is one-shot and authoritative: the durable decision was
consumed by `begin_approval_responses`, the typed frame was built by the pinned
response constructor, and that value is retained by the entry. The host must
complete that frame's bounded write and record its acceptance, or fail with the
transport's own error; it must not cancel, and it must not re-enter the send path
with a frame it has already committed. Re-entry is specifically harmful: a parsed
resolution removes the connection's pending server request, so a subsequent
attempt to send the same frame is refused as closed, which would report a
transport failure for a frame the host itself had already committed.

The three cases are now named. **Before admission**: a resolution cancels with
`Failure::ApprovalCancelled` and no write (unchanged). **Committed to the
transport, receipt not yet recorded**: the resolution is recorded and the write
completes or fails on its own error; no cancellation. **After write acceptance**:
the resolution is recorded and the drive continues; it neither revokes nor renews
authority (unchanged).

Deliberately not claimed: none of this proves the runtime *applied* the response.
Local write acceptance is transport-level, and native application remains
unconfirmed, exactly as ADR-046 requires. No acknowledgment, retry,
reconstruction, renewed authority, widened deadline or new fallback follows.
Missing phases and absent observations remain unobserved rather than
reinterpreted.

**Addendum to the amendment (review corrections).** This amendment interprets,
and does not alter, ADR-046's own rulings at lines 121–125: a resolution
observed before response admission cancels the callback; uncertain
transmission closes/fences the original attempt; after known local write
acceptance a resolution remains native application-unconfirmed and neither
revokes nor renews authority. The `in_flight` flag only prevents a
cancellation the wire already contradicts; it relaxes none of those rulings.
Two outcomes are possible for the corrected window, and both are correct: if
the resolution is delivered by a host pump (M1), the in-flight frame
completes to `WriteAccepted` and the drive continues; if the session dropped
the write receipt after the bytes were written (M2), the send path fails with
the transport's own error and the operation reports `Failure::Protocol`
truthfully — the middle-case test fails on that path, which is correct,
because a real receipt loss must not be hidden behind a clean completion. A
turn end remains a cancellation everywhere, including on the recheck pump
next to an armed frame: a turn end invalidates transmission — the wire is
closed — so the frame is never sent and the operation reports
`ApprovalCancelled`; only a resolution exempts an in-flight frame.

**Amendment (2026-09-12, reshaped): a pre-send resolution completes quietly.**
This amendment previously added two rules — a transport parse hold (with
ADR-034) and a `Failure::ResponseUnavailable` verdict for a resolved-away
frame. Both are withdrawn per the VM verdict: the hold contradicted
ADR-034's own transport contract (two pinned integration tests fail under
it; see ADR-034's withdrawal amendment), and `ResponseUnavailable` is the
wrong verdict for a resolution that arrives before the first byte.

*The quiet path.* The send path keeps its admissibility check
(`prepared_admissible(id)`, still exported read-only from the transport):
it fires exactly when the connection has already parsed a
`serverRequest/resolved` for the armed frame and the host has accepted no
byte. The retained rule is the one the pre-admission resolution already
follows: **the resolution is informational, never a cancellation and never
a named failure.** The armed frame is dropped (its transmit path is gone;
it is never re-sent and never surfaces as `Closed`), the entry keeps
`in_flight` so it is never re-selected, and the drive continues to its
normal quiet completion — the operation ends `Completed` with no failure
raised for the dropped frame. The trace stamps `resolved-before-send` on
the entry for diagnostics. What remains of the earlier vocabulary note is
`write-flushed` (the receipt arrived, stamped immediately before
`write-accepted`); `write-started` is withdrawn with the hold. A lost
acceptance observation after a **written** frame keeps its named cause via
the reconcile (the `settlement_cause` rules above), which owns that path.
