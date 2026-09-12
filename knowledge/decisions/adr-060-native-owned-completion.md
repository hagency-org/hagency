---
kind: decision
id: ADR-060
title: "Explicit native Done and final content with retained owner cleanup"
status: Accepted
---

# ADR-060: Explicit native Done and final content with retained owner cleanup

Status: accepted for bounded offline integration; native service availability remains false.

## Context

Explicit canonical Done revokes the old execution epoch, while verified final content still needs publication after the same retained owner has stopped.

## Decision

ADR-057 proved a real native task helper can mark canonical Done. Done increments
`execution_epoch` and retires task approvals. The owned coordinator then correctly
rejects its old execution fingerprint, stops its child, and keeps an uncertain
lease. It cannot accept later model prose as a new execution epoch or final reply.
The original JavaScript runner waits for a final message after explicit Done and
settles after `terminateAndWait`; copying that sequence with an epoch refresh
would grant the old native runner more tool execution after canonical completion.

The accepted three-layer completion requirement still requires an explicit task
operation and independent verification. It does not require the same physical
runner to survive indefinitely. A separate reporting runner would need an
immutable result snapshot and qualified tool-free execution inventory; an echoed
`readOnly` sandbox field does not prove that. This slice instead adds one explicit
`complete_task_with_reply(id, call_id, body)` tool. The agent supplies its already
verified final result while its task authority is still valid. One writer
transaction marks Done, increments the epoch, stores the immutable final body,
and fences execution. Cleanup and final send admission happen afterwards.

Plain `transition_task(done)` is unchanged and truthfully leaves no final report
custody. It is suitable for task-only work. Calling the new tool after task-only
Done cannot backfill or renew authority. Graph result receipts remain separate;
the existing graph completion guard refuses a missing verified graph result and
rolls the entire combined transaction back. This slice does not invent graph
results from final text or replace inspected recovery report grants.

## Durable states and bounds

Schema 016 adds `owned_task_completions`. This is completion/send custody, never a
second task store: task state and execution epoch remain in `canonical_tasks`.
Each row records exact dispatch/fence, task/completed epoch, original logical
scope fingerprint, call/digest, original private Matrix route, final body and a
finite deadline. State is `held`, `ready` with one final reply ID, or `cancelled`.

The finish transaction:

1. validates bounded content and the exact current Started capability, assigned
   task, frozen resources, clean leases, current allocation and verified route;
2. uses the **same task operation receipt namespace** as heartbeat, transition
   and comments, with a digest tagged `complete_task_with_reply` including all
   supplied fields; conflicting method/content under the same call ID fails;
3. invokes the existing task mutation state machine in the same immediate
   transaction, including the Done epoch increment and existing grant triggers;
4. writes held content and fences only this execution attempt through the
   existing stop queue, capability retirement, dirty lease and session quarantine.

No `final_replies` row exists at this point. The body is at most 32 KiB UTF-8;
native task-client HTTP requests and MCP frames have their existing 32 KiB limit
and fail visibly if escaping/metadata exceeds it. The direct private HTTP handler
uses the existing 64 KiB request limit and still validates the decoded body at
32 KiB. Nothing truncates a final result. Completion custody
has a 30,000 row global ceiling and 128 row ceiling per session (including history;
no automatic pruning is claimed). Capacity refusal rolls back Done, the epoch,
operation receipt and stop fence together. The deadline is the earlier of original
capability expiry and 30 seconds after finish. Exact committed receipt replay may
outlive that deadline but grants no execution or send permission.

Both finish and publication obtain the writer clock after queue admission and
SQLite immediate transaction lock acquisition. Queue payload accounting includes
capability, scope input/task/resource/fingerprint, reference ID and full body.
The existing 64 KiB per-command queue bound also applies to these complete host
snapshots; an oversized aggregate fails visibly and cannot publish or release.
Library defaults and the SQLite 100 ms busy timeout are unchanged. Existing
absolute 30 second maximum operation and 60 second conservative synchronous join
allowance cover the added bounded observation/publication receipts; they are not
OS scheduling or kernel syscall guarantees.

## Opaque start scope and the host trust seam

`OwnedDispatchScope` has no public constructor, Deserialize or Debug. An admission
scope has no started marker. Only the successful exact `start_owned_dispatch`
transaction response contains a marker binding the dispatch, runner, fence and
capability hash. A clone preserves this in-memory evidence; it is **not kernel
custody**. No persisted held row or receipt reconstructs it, and a fenced attempt
cannot be admitted or started again to obtain a fresh marker.

`observe_owned_completion` requires that started-only value and authenticates the
same historical runner attempt, reference and original fingerprint. The private
host observation contains only the matching completion reference. No runtime or
HTTP route can mint this value or publish it.

The `hagency-execution::Operation` keeps the successful Start value alongside the
same `OwnedSession` it actually spawned with that scope/capability. It stops that
specific retained owner before publication and requires all three observed facts:
`whole_tree_stopped`, `leader_exited`, and `signals_accepted`. The public execution
API accepts no caller-supplied `StopReport`. The lower `DomainRepository`/`DomainStore`
publication methods are explicitly **trusted host seams**, like the existing
`complete_owned_dispatch`: SQLite validates domain identity/fences, not kernel
ownership. Calling them directly is reserved to the host adapter that retains that
same owner; repository tests do not pretend to provide process proof.

After stop, publication rechecks exact Done epoch, original logical scope and
provisioned resource, unchanged frozen Matrix route, registration/allocation,
deadline, same unresolved stop/fence and original leases. It checks while session
quarantine remains set. Another unresolved session or shared-resource attempt, or
another resource lease, blocks publication; no temporary quarantine bypass or
clearing of another stop occurs. Only then does one transaction insert the stored
body into the existing final outbox, finish the original frozen input bookkeeping,
settle this stop/attempt, and remove this attempt's leases. The final outbox keeps
its original route/body/transaction and existing send/uncertainty fences.

The original operation cancellation signal and absolute monotonic `Instant`
deadline are checked inside the publication transaction after the queue and DB
lock. The stored finish/cap deadline is an additional fence; it never extends the
original operation budget. That eligibility decision is the
linearization point; a later cancellation cannot undo the committed intent.
Cancellation, deadline, unsupported approval or a negative/unknown cleanup path
never publishes. Negative observation cancels a still-held row without deleting
its body or releasing leases. Report retries remain negative-only; a later stop
retry does not silently revive cancelled content. Lost publication receipt keeps
settlement unknown; the normal outbox truth, if committed, remains durable.

Explicit held content is independent of upstream turn completion. An EOF or
protocol error after the actual finish transaction can still leave a known
canonical Done/body. The retained owner must still be stopped and all fresh writer
checks pass; its protocol outcome remains Unknown/Failed, never fabricated as
Completed. Model output text is never substituted for the held body. Final reply
admission, upstream completion, actual cleanup and canonical Done are reported
separately (`CanonicalReplyReady` is not `Protocol::Completed`).

On host restart without the original retained owner/start value, held body remains
unpublishable. Task truth stays Done and leases remain dirty. Automatic inspected
recovery for such a row is a future transition; the current inspection/report
mechanism is not implicitly reused. MacOS whole-tree uncertainty remains a hard
release/send gate even when leader exit or helper exit was observed. Physical
workspace directory custody, effective sandbox enforcement, POSIX simultaneous
custodian loss and live model qualification remain the existing open gates.

## Narrow native MCP and HTTP boundary

The generated host helper configuration from ADR-057 adds exactly one fixed tool
name. The pinned Codex 0.153.4 config/environment semantics cited there are
unchanged: native executable, fixed `mcp` argument, names-only forwarding of exact
private context, on-request approvals, workspace-write and disabled default
network access. Guidance explains explicit verification, full final content and
stopping tools after finish. No new arbitrary argv/env/URL/identity or permission
input exists; effective hostile-runtime confinement is still unqualified.

`POST /api/native/v1/runner/complete-task-with-reply` is separated from the generic
current-capability authentication hoop. It still requires the existing local
host authority and complete exact capability headers. Its writer operation alone
can replay an already committed identical receipt after fencing. Different cap,
task, call method/body or arbitrary route fields cannot obtain authority. All
ordinary task reads, mutations, delegation and approval operations retain their
current-capability gate. Receipts expose no body, route, owner DM or capability.

## Evidence and limits

Store fixtures cover atomic Group and encrypted null-root DM finish/publication,
bounded content, call namespace conflicts, old capability denial, wrong/admission
scope and wrong reference, finite deadline, task epoch drift, revocation, DM
promotion and device rotation, unrelated stop/lease custody, capacity rollback,
schema migration and ownerless restart. Worker queue fixtures use real transactions
with only response delivery withheld before/after finish; cancelled futures are
never repolled. Other actual queued publications see cancellation or the original monotonic
deadline expire after enqueue and before writer eligibility, preserving held body
and lease.

The actual offline app-server executable consumes generated configuration and
launches the real native MCP helper through retained child pipes against the same
fresh loopback writer. The finish fixture commits canonical Done through real MCP;
helper ACK/exit can race revocation and are recorded only if observed. The separate
heartbeat fixture still requires real helper ACK, readback and successful exit.
The host reads canonical truth directly from that same test DB after stopping.
MacOS local execution proves refusal on incomplete cleanup; Linux/Windows positive
owned cleanup and resulting reply admission require their actual CI fixtures.
No test here is a live Codex model or production Matrix send. ADR-059 separately
owns the native final Matrix delivery adapter and its offline HTTPS qualification.

## Consequences

Held completion binds original content and exact Started evidence to qualified cleanup before outbox admission. Upstream completion, helper acknowledgement, task Done and final delivery remain separate observations.

## Alternatives Considered

Refreshing the old runner's epoch after Done would authorize more tools. A separate reporting runner needs its own qualified tool-free inventory; neither an echoed readOnly setting nor later model prose supplies that proof.

## Amendment 2026-09-12 — Settlement cause marker

Hosted Windows returned `Failure::SettlementUnknown` where `None` was
expected, and the artifact could not say which of three producer sites or
which store refusal it was: all three discarded the `hagency_store::Error`,
and `Failure` is a payload-free `Copy` enum. Three distinct product
situations — the command never entered the writer queue (`Busy`), the writer
reply did not arrive inside its bound (`OutcomeUnknown`), and an outright
refusal (`RunnerAuthority`, `State`, `Quarantined`) — collapsed into one
discriminant.

`Report` now carries `settlement_cause: Option<SettlementCause>`, a bounded
`Copy` marker set by a `settlement_failure(report, &error)` helper at the
three producer sites (`observe_owned_completion`, `publish_owned_completion`
with checkpoint precedence preserved, `complete_owned_dispatch`).
`SettlementCause::of` maps `Busy → QueueBusy`, `Unavailable →
QueueUnavailable`, `OutcomeUnknown → ReplyTimedOut`, `RunnerAuthority`,
`State`, `Quarantined` to themselves, and every other refusal to `Storage`.
The bootstrap status projection exposes it as a `settlement_cause` label
next to `owned_failure`.

The marker is a **diagnostic discriminant only**: it never carries the
store's error text, path, payload or capability material, and is never
authority, retry, reply or lease input. It changes no verdict: every
`assert_eq!(report.failure, Some(Failure::SettlementUnknown))` still holds,
`Failure` stays `Copy`, and a clean completion records `None`.
