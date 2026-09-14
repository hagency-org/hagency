---
kind: decision
id: ADR-095
status: Accepted
title: Native state ownership and migration recovery boundaries
---

## Context

The native migration needs explicit transaction owners before its independent transport, domain and crypto stores can recover interrupted work safely. This native state decision is renumbered from ADR-028 to ADR-095 to preserve the existing ADR-028 execution-authorization identifier; the ownership decision and its qualification limits are unchanged.

## Decision

Implements the operator's REQ-RUST-MIGRATION-EXECUTION. Baseline is
`5dbef22dc5ad4e0bb1a886538406ec91a5893f9b`; production continues using JS/TS.
Fresh native state is the default. No old router, JSON or SDK store is imported.

### Owners and commit boundaries

| Command | Owner | One atomic commit | Replay identity | Crash recovery |
| --- | --- | --- | --- | --- |
| Receive transport delivery | Registration-bound custody repository | Binding generation, complete payload, content digest, receipt | registration + lane + delivery ID, digest must match | Before commit: no ACK. After commit: same receipt; still unprocessed |
| Admit project request (M2/M5) | Domain service, `domain.sqlite3` | Authenticated intake evidence, request, domain inbox marker | registration + request ID + canonical authenticated fields | Transport retries admission; domain returns original outcome; no second request |
| Approve selected resource (M2) | Domain service | Current authority checks, pool/shared-seat reservation, fulfillment phase, provisioning intent in domain outbox | engagement ID + approval command ID + binding generation | Resume durable provisioning intent; never reserve twice |
| Provision identity (M4/M5) | Domain service coordinates runtime/Matrix adapters | Intent before external action; observed identity/effect receipt after action | stable Agent identity + provisioning operation | Inspect ambiguous account/process creation; do not launch again on timeout |
| Create/claim task dispatch (M3) | Domain service, same database as core allocations | Task mutation, dispatch lease/fence, scoped input, outbox intent | task/session/dispatch/attempt identities | Expired authority cannot resume; uncertain effects require inspection |
| Approve tool operation (M6) | Domain service | Owner/device/project/task/fence checks, one-use consumption or scoped grant, decision outbox | approval ID + current binding/fence | Retry original verdict; no generic reusable grant |
| Settle dispatch and send result (M3/M6) | Domain service | Authorized canonical task transition, usage observation, reply outbox | dispatch + delivery destination + result ID | Matrix retry uses stable transaction ID; runtime exit is not task completion |
| Rotate private credential or crypto device (M5/M6) | Credential/crypto adapter plus domain rotation saga | Each owner commits its own generation/intent; never a cross-DB pseudo-transaction | rotation ID + old/new generation | Remain unavailable until both acknowledgements reconcile; never rewind crypto ratchets |

The foundation implements transport custody. The M2 domain checkpoint additionally
implements project-defined request admission, reservation/decision/outbox commits
and explicit effect reconciliation using fixture-verified observations. Actual
Matrix transport and provisioning adapters remain absent. Core business state and
canonical router/task state share **one authoritative domain database**
with one transactional writer. Separate repositories are module boundaries,
not a license to split atomic invariants across independent SQLite files.
Transport custody and Matrix SDK crypto remain independently owned stores.
Cross-owner operations use durable inbox/outbox handoffs; ACK is not admission.

The native development API accepts operator-provided fixture envelopes. It does
not claim those envelopes are authenticated Matrix observations, and therefore
cannot approve, provision or execute Agents. M5 supplies and validates real
registration-bound provenance before crossing into domain admission.

`domain.sqlite3` uses a distinct application ID, `domain.lock`, WAL/FULL commits
and the same private file policy. Its dedicated worker has 16 queued commands by
default, a 64 KiB input limit, 8 MiB byte budget and two-second response deadline.
Queries page at 100 records. New identities are bounded to 1,024 registrations,
2,048 resources/seats and 10,000 requests; capacity never deletes pending work.
Decision/effect rows are indexed and updated individually, without lifetime-store
cloning. Completed audit retention/compaction remains an M8 release gate; this
checkpoint is not a continuous-operation retention claim.

Native project bindings pin the full owner, private room and project room to a
registration generation. Rotation fences old admission, approval and effect claims;
it does not erase allocations. Rebinding/rotation reconciliation remains closed
until the authenticated transport/credential saga exists. Local explicit revocation
can still persist its fence and pending cleanup. An old-generation cleanup must
not be reported completed merely because the registration changed.

Native schema2 adds explicit role publication with an ordered SQL migration.
Fresh schema creation and all needed upgrades share one transaction; a failed
migration preserves the previous version and rows. Downgrades and foreign stores
remain refused. Resource role caches are never authority: eligibility is computed
from the existing embedded role-capacity policy and current provider/reasoning
fields. Review counts active allocated model families on the same registration;
unprovisioned configurations and another registration cannot satisfy that guard.

### Latency and storage

SQLite commands run on a dedicated thread. Defaults: 16 queued commands, 16 MiB
of serialized queued payload, 4 MiB per delivery, eight HTTP body/command slots,
64 connections, two-second body/command deadlines. An admitted command that loses
its response returns `outcome_unknown`; the caller reconciles with the same ID.
No external effect is replayed by this worker. Shutdown drains prior commands and
releases the database before acknowledging shutdown. Shutdown resolves only after
its closed word is observable: the worker drops its receiver before the reply that
resolves the caller, so `writer_open()` reads closed the moment `shutdown()` returns —
found on a loaded runner as `/health` reporting ok after shutdown had returned.

Custody retains at most 1,024 unprocessed records and 16 MiB of payload. Reaching
capacity rejects new IDs; identical retries still retrieve their original receipt.
No pending record is deleted to make space. M5 must add acknowledged completion,
tombstone retention and bounded pruning before continuous transport is enabled.
JSON parsing/validation remains bounded foreground CPU work; it is not described
as an unlimited or cost-free operation.

Future usage collection gets a **separate** worker and timestamped snapshot.
HTTP reads never scan runtime history. Snapshot states include unavailable,
fresh and stale with last collection time; unknown token usage stays unknown.
Before M7 completion, a populated 10,000-file fixture must show approval,
heartbeat and health p95 below 250 ms locally while collection is running, with
an explicit hardware record and memory ceiling. This workload gate is still open.
The foundation saturation test proves custody-worker separation only.

### Platform and dependency choices

Rust edition 2024, pinned toolchain 1.95.0, Salvo 0.96.0 (requires Rust 1.94).
SQLite 0.37 is selected to share the native SQLite library required by
matrix-sdk-sqlite 0.18; two incompatible `libsqlite3-sys` versions cannot be linked
into one eventual executable. Cargo.lock pins transitive dependencies. Browser
JS and Node-based build/test tooling remain development dependencies.

Unix state uses owner-only directories/files, ownership and hardlink checks,
exclusive file locking and durable commits. Windows uses native owner-SID DACLs,
reparse/hardlink rejection and file locks. Windows FFI is isolated in one module;
the rest of the store denies unsafe Rust and core/API forbid it.
Cross-compilation is not evidence of Windows execution or of sandbox support.

An offline SDK proof creates fresh encrypted state, exchanges its generated
cross-signing material through a fixture key-query response, reopens the device,
and decrypts under `CrossSigned` trust. It rejects wrong device IDs and encryption
keys. This is not a connected Palpo test, NAPI crypto import, multi-user trust
proof, device recovery flow or production authorization implementation.

### Early gates and dependencies

Before native Agent launch becomes available, execute a headless fixture runner
on native Windows and both Unix families: assigned ownership before work,
child/grandchild cancellation, parent crash, PID reuse and effective sandbox
verification. Windows Job Objects alone are not a sandbox. Unproven runner/platform
combinations remain unavailable. The current foundation executes no Agent.

M7 API/UI shell work can start after M2/M3, but integrated M7 acceptance also
requires M5 transport/crypto and M6 approvals/files. M8/M9 remain closed until
those workflows and platform proofs pass. There is no authorization for a live
cutover in this checkpoint.

Windows crash recovery also validates SQLite auxiliary files inherited from the
private state directory. Elevated Windows processes can assign those files the
Builtin Administrators owner SID. Only for these literal journal/WAL/SHM paths,
that privileged owner is accepted alongside the service SID; every access ACE
must still name only the service SID, and reparse/hardlink checks remain enforced.
Credentials, database and ownership-lock files retain exact owner-SID validation.
Administrator/root privileges are outside protection against ordinary local users.
The private-file regression also rejects public-read ACLs for journal files.

### Native canonical task and dispatch checkpoint

Domain schema 3 adds tasks, session bindings, dispatch attempts, resource leases,
mutation receipts and an internal task event outbox. Starting a leased dispatch
atomically activates its task and acquires the already frozen payload once.
OS-random capabilities are stored as hashes and checked with runner, dispatch,
fence, deadline, current allocation and exact session/task binding. A coordinator
can read tasks created by its own session; it cannot mutate another assignee.
Task completion advances the authorization epoch. Runner final output cannot
complete a canonical task. Task comments, updates, receipts and events commit
or roll back together, without cloning the lifetime store.

The single bounded writer serializes competing resource claims. Parked work
retains leases. Startup and expiry requeue only unstarted attempts; started or
parked attempts become unknown and quarantine their session and writable resources.
An inspected host recovery creates a distinct instruction, supersedes stale queued
instructions, and preserves the original attempt and rejected late output. If the
canonical task already completed, recovery may report its inspected result without
reopening that task. Room admission and process inspection are still host adapter
responsibilities; no native HTTP endpoint accepts these fixture authority commands.
Mailbox ordering, task dependencies, delegation, follow-up reopening and actual
reply delivery remain subsequent M3 work. This checkpoint does not satisfy M4–M9.

### Message admission and input ownership

Schema 4 enforces one session per allocation/room/thread, with transactional
resolution and a unique SQLite index. Older conflicting native session records
stop migration and preserve the old schema for inspection. Message identity binds
server, room and event ID to immutable content. Each eligible session has its own
wake/input/processing projection. Fresh native stores use committed arrival order;
external timestamps remain source data and cannot reorder the consumption cursor.
No legacy deployment history is imported by this change.

Admitted events and their target projections commit together. Enqueue selects
bounded admitted input, requires a wake, freezes the inbox and claims the session
projection in one transaction. Runner reads use the exact current dispatch
capability and cannot include later arrivals. Successful dispatch settlement marks
only that session's input processed. Inspected unknown-work recovery retains prior
input as recovery context and releases superseded unstarted input for later work.
Quarantine blocks execution while allowing durable admission. Capacity failures
leave pending messages available; filtered projections do not acknowledge gaps.

The ingress command cannot deserialize external HTTP assertions. Its source data
must still be constructed by an authenticated Matrix adapter that verifies actual
membership, room privacy and mention/DM policy. No HTTP endpoint exposes this
constructor. The compiler assertion and local fixtures prove the internal boundary,
not live homeserver authentication. Group/MCP surfaces, task graph/delegation,
reply delivery and continuous retention remain later migration work.

### Private runner service boundary

The native runner HTTP surface exposes task reads, comments, typed mutations and
frozen inbox pages. It uses the exact loopback authority and a full current runner
capability in headers. Operator and runner credentials are separate. No runtime
command variant can admit a session, claim/start a dispatch, inspect recovery or
configure resources. Browser/forwarded requests and duplicate or URL credentials
are refused; responses are private and do not disclose storage errors or tokens.

The service sends a typed RunnerCommand to the bounded domain writer. That writer
obtains the current host clock after queueing and revalidates the started capability
at the actual operation. An initial HTTP authorization check cannot extend a lease
through body-reading or queue delay. A deterministic queue-delay regression expires
an already queued request before releasing the writer and proves rejection. Structured
request parsing rejects arbitrary identity/status fields, unknown actions and caller
clocks. Current task mutation receipts preserve exact replay and rollback behavior.
M4/M6 adapters must use this service boundary; this checkpoint launches no processes
and does not provide MCP transport or full task graph/delegation behavior.

## Consequences

Domain invariants remain under one bounded transactional writer. Cross-owner acknowledgements require durable handoffs, and fresh native development state does not authorize importing live state or cutting over production.

## Alternatives Considered

Splitting canonical tasks and allocation invariants across separate databases would lose their atomic boundary. Treating transport receipt as domain admission or adopting legacy stores would also bypass the distinct ownership and recovery checks recorded here.

## Amendment 2026-09-13 — the decision-receipt bound (retention Slice 3)

This ADR already names "Completed audit retention/compaction" as an M8 gate. This
amendment bounds the *command-half* of it — `decisions`, the command receipt — and
nothing else. `decisions` is `(id TEXT PRIMARY KEY, digest TEXT NOT NULL, result
TEXT NOT NULL)` (`domain.sql:45-49`): one row per verdict, no time column, no
engagement link, written once by `record_decision` (`domain.rs:273-281`) inside that
verdict's own transaction, and read only by `replay_decision` (`:254`). Native keeps
no event log and this amendment does not create one.

**Placement — in-write, not a phase.** The trim runs inside the deciding command's
own transaction, after `record_decision`. It is **not** a retention phase and has no
period, bootstrap constant or shutdown token; the tick contract (ADR-125's "Retention
sweep tick" section) records that refusal, and because the tick is a sequence of
separate `Job::Run` transactions the trim shares no transaction with any phase. The
receipt it writes lives in that table, with `phase='decisions'`.

**1. The bound (D-3).** Count-only, cap `DECISION_RETENTION_LIMIT = 500`, ordered by
**`rowid`** — store-assigned and monotonic — with `DECISION_PRUNE_BATCH = 512` as the
per-command catch-up bound. `created_at` is not the key and is not added: a
caller-supplied clock sorting below older survivors is exactly the failure the
`rowid` key removes. The newest 500 verdicts by `rowid` replay idempotently; outside
it a command is refused `NotFound` for `approve`/`reject`/`revoke`, whose replay
check precedes every mutation. The tick contract's D-3, quoted: *"The newest
`DECISION_RETENTION_LIMIT = 500` verdicts by `rowid` replay idempotently; outside it
a command is refused …"* The retained `AUDIT_LIMIT = 2000` audit log has no native
analogue and this amendment does not create one; the retained record cap
(`ENDED_LIMIT = 500`) is the number ported.

**2. `retry_cleanup` is excluded from the bound, and the exclusion is computed in
Rust.** `retry_cleanup` (`domain.rs:819`) mutates at `:831` **before** it records at
`:835`, and on a fresh `command_id` its replay lookup at `:824` finds no row *by
design*. Absence therefore cannot distinguish "pruned" from "never seen", and any
rule that reads a pruned receipt as `NotFound` refuses a legitimate **first** retry —
a command-defeating clause. So every `retry_cleanup` decision is excluded from the
candidate set and survives indefinitely.

*How, exactly.* `decisions` has **no kind column**, and `digest =
sha256(canonical([kind, engagement]))` (`domain.rs:251-253`) is a non-invertible
64-hex hash. SQLite registers no `sha256`, so no SQL predicate can recognise a
`retry_cleanup` row by its digest — the form "the exclusion is by the command's own
digest form" is **struck as unimplementable**, and with it the tick contract's §6.1
fragment, which additionally writes `SELECT command_id FROM decisions` where
`command_id` is not a column (it is `id`). The implementable form is a two-step
candidate scan, inside the same transaction:

1. `SELECT id, rowid, digest, result FROM decisions WHERE rowid <= ?1 AND rowid <
   (SELECT MAX(rowid) FROM decisions) ORDER BY rowid` — the over-window rows, never
   the high-water mark.
2. For each candidate, parse `result` as the stored `Engagement` and recompute
   `decision_digest("retry_cleanup", engagement.id)`; a candidate whose stored
   `digest` equals that value **is** a `retry_cleanup` decision and is pinned. The
   comparison is exact — the digest is over the kind word and the engagement id, so
   an `approve` row for the same engagement recomputes to a different value — and it
   needs no schema change.

`DECISION_PRUNE_RETRY_KIND = "retry_cleanup"` is the named constant the recomputation
uses: the kind **word**, not a digest prefix. At most `DECISION_PRUNE_BATCH` rows are
removed per command, and the maximum-`rowid` row is never removed, so the high-water
mark stays monotonic (SQLite reuses `max(rowid)+1` after a delete).

*Cost, named.* The scan is O(candidates) per deciding command — bounded by the
window, not by the corpus — and its wall clock is an obligation to measure. A `kind`
column written by `record_decision` would make it O(1) and is **rejected**: `decisions`
is created in the base schema (`domain.sql:45`), no fixture and no `remove_*_schema`
helper drops it, so an `ADD COLUMN` migration would fail the store's replay rule (the
comment at `025-alert-transitions.sql:1-12`) at every rewind site unless each were
taught to drop the table — a strictly worse diff than the scan.

*Consequence, stated.* `decisions` is not strictly bounded: the `retry_cleanup`
residue grows at most one row per distinct command id (`decisions.id` is the primary
key) and never expires. That is a bounded-size residue, named rather than hidden.

**3. Never a refusal of new work (D-12).** The tick contract's D-12 reads, quoted:
*"Log and retry next tick; the store's caps (`100 000`, `10 000`, `30 000`) refuse at
a bound, and retention must not repeat that shape."* An over-window corpus is a
standing condition, reported by the receipt; it never refuses a verdict.

**4. Rollback.** The trim is inside the verdict's own transaction, after the insert,
so it rolls back with a failed command: the decision count and the previously oldest
surviving command id are what they were, and no receipt records a prune that did not
happen.

**5. The receipt.** One `retention_prune_receipts` row with `phase='decisions'`,
`oldest_ref`/`newest_ref` = `decisions.rowid`, carrying `pruned`, `remaining` and
`elapsed_ms`, written inside the same transaction as the trim and trimmed by the
shared clause with `RETENTION_RECEIPT_LIMIT = 100`. Pruning the record does not erase
the fact that it happened. The tick contract's D-5, quoted: *"One read per slice, no
page work: `retention_status` ({corpus_rows, ceiling, over_by}),
`execution_retention_status`, `engagement_retention_status`, plus the peer phase's
`remaining` and the shared receipt row. `remaining > 0` is the standing over-ceiling
report."*

**6. What an operator loses.** The per-command engagement snapshot beyond 500
verdicts — the only per-command history native keeps. A receipt does not restore it;
recovering it would be a new event-log surface, which this amendment does not create.

**7. `effects` is out of scope.** Bounded to ≤2 rows per engagement by
`UNIQUE(engagement_id,kind)`, never deleted, and anchored on the engagement rather
than the command.

## Amendment 2026-09-13 — the ended-engagement record bound (retention Slice 6)

`engagements` (`domain.sql:22-40`) grows one row per accepted admission and nothing
deletes one: the only bound is `bounded_row(engagements, …)` (`domain.rs:658`), which
refuses new work rather than reclaiming. The retained product caps the analogous
record at `ENDED_LIMIT = 500` (`lib/engagement-store.js:56`). This amendment ports
that cap and states the cascade that makes it safe. It is **phase 4, `engagements`**,
of the retention sweep tick — **ADR-125's "Retention sweep tick" section**, which
this amendment cites rather than restates.

**Placement.** Fixed order `messages → peer → execution → engagements`, one period
`RETENTION_SWEEP_PERIOD = 60 s`, one phase budget `RETENTION_PHASE_BUDGET_MS = 600`,
one `Job::Run` per phase (**never one transaction for the tick** — this phase's
cascade is one transaction *for this phase*), and a `Busy`/`OutcomeUnknown` refusal
that logs `[engagement]` and waits for the next tick. `decisions` is trimmed in-write
and is not a phase.

**The bound (D-4).** Count-only, cap `ENDED_LIMIT = 500`, ordered `rowid ASC` —
store-assigned, monotonic and re-admission-correct, since a re-admitted row gets a
fresh larger `rowid`. The tick contract's D-4, quoted: *"Count-only, cap
`ENDED_LIMIT = 500`, ordered `rowid ASC`; `ended_at` is advisory metadata, never
`DEFAULT 0`; a pruned id **can** be re-admitted and starts from `pending`."*

**`ended_at` is metadata on a side table, not a column on `engagements`.** The
ordering key is `rowid` alone, but the operator's loss must be nameable in the
receipt, so the ended-at instant is recorded in a new side table:

```sql
CREATE TABLE IF NOT EXISTS engagement_ends (
  engagement_id TEXT PRIMARY KEY REFERENCES engagements(id),
  ended_at      INTEGER NOT NULL
) STRICT;
```

The rejected alternative is `ALTER TABLE engagements ADD COLUMN ended_at INTEGER`.
`engagements` is created in the base schema and **no** fixture and **no**
`remove_*_schema` helper drops or rebuilds it (`tests/common/mod.rs` has no
`engagements` handling), so an `ADD COLUMN` would be replayed over an
already-upgraded table by every fixture that rewinds `user_version` — the failure
the store's own rule names, quoted from `025-alert-transitions.sql:1-12`: *"a replay
over an already-upgraded table fails on the duplicate column, and no ADD COLUMN
migration in this store supports that replay."* Making every rewind fixture rebuild
`engagements` would be a large, silent change to eleven fixtures this slice does not
own; the side table costs one `DELETE` in the cascade and no fixture change, because
`CREATE TABLE IF NOT EXISTS` is replay-idempotent (the `024-alert-ceiling` pattern).
The side table is deleted in the same transaction as its engagement, before it.

**The candidate predicate is the safety mechanism.** `foreign_keys=ON`
(`database.rs:120`) refuses a delete while a child exists — but this cascade must
delete those children first, so the refusal guarantees no orphan and no half-delete
and **does not** substitute for the pins. An engagement is a candidate only when it
is terminal (`state IN ('rejected','revoked','failed')`), no custody pin holds
(P1–P7), and **every child is already gone or is cleared by this slice's own cascade
in this same tick**.

**P5 is the raw dispatch state pair, never the reporting view.** Quoting the tick
contract's D-1: *"A row whose dispatch outcome is `outcome_unknown` is retained
**indefinitely**, including through a dispatch recovery … The pinning pair is P4
**and** P5, not P5 alone; `unresolved_dispatches` (`009:23-26`) is narrower and is for
**reporting**, not pinning."* P5 therefore pins on the raw pair —
`runner_dispatches.state IN ('leased','started','parked','outcome_unknown')`, reached
through `runner_sessions.engagement_id` — **and not** on membership in
`unresolved_dispatches`, which is a narrower reporting view that excludes a dispatch
with a settled stop or a recovery. A pin keyed on the view would prune an engagement
whose dispatch an unknown-fate recovery left behind; the raw pair cannot.

**Ownership of the shared tables (ADR-125's "Retention sweep tick", ownership
table).** Slice 6 holds **cascade delete rights** over the reachable set — including
`owned_task_completions` (**tier 1**, ordered **before** `task_operation_receipts`,
whose composite FK it names `016:13`, and **before** `final_replies`, `016:10`) and
`retained_message_archive` (**tier 2**, Slice 1's object). It holds **no bound** over
any table Slice 2 or Slice 1 owns: those slices state the pins and the windows, and
this slice's rights fire only inside a candidate engagement's cascade. The
`owned_task_completions` grant is the fix for the wedge this table caused: it has two
`NOT NULL` FKs (`016:4`, `016:5`) plus a composite FK to `task_operation_receipts`
and a `reply_id` FK to `final_replies`, and **no** writer deletes a row of it, so a
cascade that omitted it would stop at every engagement whose session ever ran an
owned completion and defer forever.

**`task_outbox` scoping (one sentence, as the contract requires).** This cascade
deletes `task_outbox` rows **only** whose owning `canonical_tasks` row is itself
being deleted in the same transaction — **tier 1**, honouring the `canonical_tasks`
FK (`003:48`), which is why the child rows go before the task row; the pager's next
page (`execution.rs:1079`) then skips forward correctly, because the task the events
described is gone. That is a different operation from the **bound** prune Slice 2
defers (D-6), and the scoping is what reconciles the two. The deferral's reason is
that the pager has **no production consumer** — the cursor is caller-supplied and
persisted nowhere — not that the pager is production: this cascade is the only delete
`task_outbox` ever receives, and a real acknowledgement path stays a named
retained-product gap.

**The receipt.** One `retention_prune_receipts` row with `phase='engagements'`,
`oldest_ref`/`newest_ref` = `engagements.rowid`, carrying `pruned`, `remaining` and
`elapsed_ms`, and — in the table's `payload` column, defined in ADR-125's one
`CREATE TABLE IF NOT EXISTS` — the per-id terminal state `[{id,state,ended_at}]` and
the per-table child counts. Native keeps no audit event log, so pruning erases the
engagement's last-known state, and the payload is the receipt's whole reason. The
row is trimmed by the shared clause with `RETENTION_RECEIPT_LIMIT = 100`.

**What an operator loses, and the two named consequences.** The per-id terminal state
beyond 500 engagements; the receipt carries it once and does not restore the rows.
Because engagement ids are deterministic, a pruned id **can** be re-admitted and
starts from `pending`; and because `usage_sources`/`usage_periods` do not survive the
cascade, **spend is forgiven at re-admission** (D-11, quoted: *"A pruned engagement's
`usage_sources`/`usage_periods` do not survive, so a re-admitted (deterministic) id
starts from zero; safe for admission, named as a consequence."*).

**A retention failure is never a work refusal (D-12).** The tick contract's D-12
reads, quoted: *"Log and retry next tick; the store's caps (`100 000`, `10 000`,
`30 000`) refuse at a bound, and retention must not repeat that shape."* A deferred
engagement reports `remaining > 0`; it never refuses admission or an operator
command.

---

## Amendment 2026-09-14 — the production provisioning ingress is absent

**The gap, read from the tree.** The only statement that mints an engagement
is `DomainRepository::admit(proof: &VerifiedRequest)` (`domain.rs:1081`,
the INSERT at `:1135`) — the single minting write, idempotent on
`request_id`, with the request row as the domain inbox marker. But **no
production code calls it**: `verify_request` (`hagency-core/src/authority.rs:196`)
and its `VerifiedRequest` result (`:164`) are invoked from test helpers
alone; the only production consumers of the authority module are the store's
`authority`/`project_authority` checks inside `admit` itself. So the native
product cannot provision an engagement in production — every agent in every
native test was admitted by a fixture, never through the product ingress.

**The retained ingress this must match.** `POST /api/engagements`
(`backend-v2.js:15103` → `createEngagementRequest` →
`engagementStore.createRequest`, `lib/engagement-store.js:486`) is idempotent
on `requestId` — which the bridge passes as the **Matrix event id** — and
refuses a reused id with a different digest. The provider's approval is the
separate `decide()` verdict (`:593`, called at `backend-v2.js:14732/14824`)
after project-side and provider authority. Native already models both halves
(`admit` for the mint; `approve` for the verdict), but only the writes exist,
not the ingress that feeds them.

**The decision.** The production ingress is the **Matrix intake admission
chain** — owner message → intake admit → provider approval → effect observed —
carried by the `com.hagency.engagement.request.v1` event (`authority.rs:225`
is the event-type check `verify_request` already requires). The Matrix
adapter observes the authenticated event, calls the same `verify_request` to
build the `VerifiedRequest`, and calls `admit` once — `admit` stays the
single minting write, and the provider approval is observed as the separate
`approve` verdict, never folded into the mint. A duplicate request (same
`source_event_id`) is refused by the same id before any second INSERT; an
unverified request is refused before `admit`. This changes nothing about the
store: it wires the production caller the ownership table already assumed.

Cross-reference: ADR-022 ("provisions agents on approval") and ADR-013
(the inbound engagement request) describe the retained product's flow; this
amendment is the native admission half of that flow, which was absent.

### Mapping — what a builder implements (fix-up 2026-09-14)

The intake's only carrier is `MatrixEventObservation`'s `InboundMessage`
(`{server_name, room_id, event_id, sender_mxid, thread_root, body, kind,
origin_ts}` — `messages.rs:10`), and `verify_request` needs a full
`ProjectRequest` plus a `RequestObservation` of three `RoomObservation`s
(`authority.rs:128-138`). Nothing in production builds either from an
inbound event today; the mapping below is the contract.

**Carrier.** The retained `POST /api/engagements` body is honestly carryable
in an event: `kind = "com.hagency.engagement.request.v1"` is the
discriminator (the same string `verify_request` already gates on at
`authority.rs:225`), and the JSON body is carried in `InboundMessage.body`
(the event's text). No new event type or `content` field is invented.

**Idempotency key — `request_id` is the key and can never be the Matrix
event id (finding 1).** `ProjectRequest.request_id` is the **idempotency
key**: `admit` replays on the same `request_id`+digest and refuses a reused
`request_id` with a different digest (`domain.rs:1090-1099`), and the minted
id is `en_` + `hash([fleet_id, request_id])` (`authority.rs:120-124`).
`ProjectRequest.source_event_id` is the **carried** Matrix event id, checked
equal to the observation's `source.event_id` (`authority.rs:225`). **The
event id can never be `request_id`**: `project::identifier` admits only
`[A-Za-z0-9_-]` (`project.rs:47-57`), and an event id starts with `$` (and
carries `:`/base64), which `validate` refuses (`authority.rs:83`). So the
request body carries a **native-valid `request_id`** — the requester's own
idempotency key, distinct from the event id — while the event id is carried
as `source_event_id`. Idempotency matches the retained store: same
`request_id` + same digest returns the prior admission; same `request_id` +
different digest is a refused `conflict` (never re-keyed, never overwritten).
The provider verdict stays the separate `approve` write; it is never folded
into the mint.

**Field-by-field `ProjectRequest` mapping (from the retained body):**

| ProjectRequest field | Source |
|---|---|
| `v`, `auth_version` | constant `1` / `1` |
| `request_id` (idempotency key) | retained `requestId` carried as a **native-valid** identifier (`[A-Za-z0-9_-]`), distinct from the event id; refused when invalid per `project::identifier` |
| `source_event_id` | `InboundMessage.event_id` |
| `fleet_id` | the collector's `Registration` (never the event) |
| `requester_mxid` | retained `requester` |
| `source_room_id` | `InboundMessage.room_id` (must equal `registration.reception_room_id`) |
| `target_project_id` | retained `project` |
| `target_room_id` | retained `projectRoomId` |
| `owner_mxid` | the collector's `Registration` (never the event) |
| `owner_dm_room_id` | the collector's owner-DM observation (private, never the event) |
| `role` | retained `role` |
| `requested_tokens` | retained `requestedTokens` |
| `rate_per_day` | retained `ratePerDay` (nullable) |
| `agent_definition` | retained `agent` + `requestContext.agentDefinition`, resolved against the resource catalogue |

Refused when absent: `project`, `projectRoomId`, `role`, `requester`,
`requestedTokens` (retained `text()`/`posInt()` throw), and `request_id`
(native requires a valid `[A-Za-z0-9_-]` key; the event id is the
`source_event_id` carry, not a generated key).

**The three `RoomObservation`s are collector facts, never event-asserted
values — and the five absent fields are observed at intake, not stored.**
`matrix_room_scopes` already persists `room_id`/`joined`/`invite_only`/
`encrypted` (`011-final-replies.sql:16-23`), and those four come from it.
But `powers`, `default_power`, `invite_power`, `binding` and `name` exist in
**no** store snapshot today. **Decision: the collector observes them from
room state at intake time and passes them in the `RequestObservation`
without storing them — no migration, no schema change.** They are
verification-time-only inputs: `verify_request` reads them once and nothing
else consumes them, so persisting them would add a migration (034) and a
head-pin move for facts no later read needs. The event's own content never
asserts any of them — the collector's room parser (`collector.rs:475`) today
handles only `m.room.member`/`join_rules`/`encryption`; **this slice
extends it** to additionally parse `m.room.power_levels` (`users` →
`powers`, `users_default` → `default_power`, `invite` → `invite_power`),
the project-binding state event → `binding`, and `m.room.name` → `name` —
the assembled `authority::RoomObservation` is a **separate shape** from the
stored `MatrixRoomObservation`. They are carried **in
memory** on the observation type (extending `MatrixRoomObservation`,
`hagency-core/src/replies.rs:44-54`) and the intake batch/event types
(`event_batch.rs:41-61` and the intake `Event`), handed to `verify_request`
and then dropped — **nothing stored, no migration, no schema change.** A
missing/expired observation is refused, never fabricated.

- **reception room** (`source_room_id`) — the collector's full-room snapshot of
  the room the event arrived in; `verify_request` requires invite-only,
  unencrypted, `joined ⊇ {requester, representative}`. This room is NOT
  recordable via `observe_matrix_room` (which refuses the reception room,
  `matrix_routes.rs:320`). **Decision: the bootstrap adds
  `registration.reception_room_id` to the collector's observed room set, so
  its `/state` is fetched exactly like the project and owner rooms
  (`collector.rs:342`)** — the collector observes it in memory only,
  `observe_matrix_room` keeps refusing it for the scope table, and nothing
  is stored.
- **project room** (`target_room_id`) — `room_id`/`joined`/`invite_only`/
  `encrypted` from `matrix_room_scopes`, plus the intake-time-observed
  `powers`/`default_power`/`invite_power`/`binding`/`name`; `verify_request`
  requires invite-only, unencrypted,
  `joined ⊇ {requester, owner, representative}`,
  `power(requester) >= invite_power`, `power(owner) >= 100`, and the
  `{v:1, purpose:"project", fleetId, projectId, ownerMxid, authVersion:1}`
  binding.
- **owner DM room** (`owner_dm_room_id`) — the approval intake's owner-room
  observation; `verify_request` requires invite-only, megolm-encrypted, and
  `joined == {owner, approval_bot}` exactly.

**Dispatch point.** The intake hook that sees
`kind == "com.hagency.engagement.request.v1"` assembles the `ProjectRequest`
and `RequestObservation` from these SDK facts and hands them through the
domain worker's `DomainStore::admit` — an **unconditional, already-public
async method** (`domain_worker.rs:2845`, no `#[cfg(test)]` gate; what is
absent is the production *caller*, not the method's compilation) — `admit`
is the single write. The provider verdict is observed afterwards through
the existing `approve` path, never folded into the mint.
