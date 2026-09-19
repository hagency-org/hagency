---
kind: decision
id: ADR-064
title: Derive native owner decisions in a separate authenticated Matrix approval cursor
status: Accepted
requirements: [REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Structured owner verdicts need an authenticated private Matrix intake path with custody independent of ordinary task-message collection.

## Decision

The native host may consume private structured owner verdicts through
`hagency-matrix::ApprovalCollector`. This bounded M6 slice settles existing
native requests and grants; it does not deliver approval cards, apply a runtime
permission decision, enable services or complete the migration.

### Separate host and device custody

`HostApprovalConfig` consumes a fixed host configuration and at most 64 distinct
engagement IDs. Its registration fingerprint, account/device incarnation,
homeserver, token and private room intents retain ADR047's bounded HTTPS and
protected store rules. HostConfig permits at most 16 rooms. The domain resolves
actual fleet, project, registered approval bot, owner and private room authority;
all configured engagements must use that fleet/registration and bot. Names,
browser state and Matrix event content cannot choose these values.

The SDK store binding includes a distinct approval-reader purpose. An Agent
collector cannot adopt it, and an approval reader cannot adopt an Agent cursor
or outgoing journal, even when a host accidentally configures the same path.
Approval observation never calls observe_matrix_transport: that table describes
Agent accounts and deliberately refuses the representative and approval bot.
The HostIdentity transport tuple here identifies the owned approval SDK device;
it is not published as Agent authority or confused with Palpo machine generation.

Whoami must return exactly the registered bot and configured device. Each full
room snapshot must be encrypted, invite-only and have exactly the owner and bot
joined. The configured room must equal the domain's private approval room and
cannot be its project/reception room. Fresh positive evidence updates existing
approval bindings; negative evidence fences the exact captured prior shared
snapshot and exact attempted candidate, covering a lost positive-domain reply.
It cannot invalidate an unrelated newer device or generation. Existing domain
triggers retire affected grants. Same-generation positive replay cannot revive
negative availability; a new host-approved generation is required.

### Actual SDK evidence and immutable actions

The original inline factory may append an engagement to this same collector's
retained membership after receiving its exact Active acknowledgment. This is a
crate-private admission using the original writer-produced provision scope, not
a caller-supplied engagement setter. The owned observation job holds the shared
busy permit, checks that scope on its producing writer before and after actual
private room observation, and only then records membership. Failed observation
adds nothing; lost waiters do not abandon the original job. Configured order is
preserved and the combined static/factory set keeps the existing 64-member bound
without eviction. Every observe/intake/enrollment/close operation freezes its
membership under that same permit. Membership never changes fixed bot identity,
room intents, peer anchors, SDK purpose or current card/verdict authority.

Only the owned SDK receives the authenticated sync response. The inspected
pinned matrix-sdk-base/crypto 0.18.0 paths are
`response_processors/timeline.rs`, `machine/mod.rs` and `identities/manager.rs`.
`receive_sync_response` supplies the TimelineEventKind and EncryptionInfo; raw
HTTP plaintext or deserialized external proof is never accepted as crypto proof.
The private proof requires verified cross-signed Megolm, no forwarder, the exact
owner MXID and a sender device from the freshly verified device query.

Fresh queries include the bot's own published identity and every configured
owner, at most 17 users and 64 devices. SDK cached trust is insufficient:
supplied master/self-signing and device authority fields must exist and match
what the SDK accepted. ADR059's small verifier is shared; existing outgoing
crypto regressions cover malformed fresh key entries despite cached trust.
Trust establishment, key publication and maintenance of missing Olm sessions
remain externally provisioned host responsibilities, not automatic fallbacks.

A current native request freezes engagement/project/private/project-room IDs,
registration/device/room/binding generations, exact input digest, expiry and
whether a reusable scope exists. Only `com.agentchat.approval.verdict.v1` with
complete `com.agentchat.approval` version 1 details is recognized. Agent and
project identifiers must equal that frozen target. Actions map to once, task,
always or deny; task/always additionally require the stored reusable scope.
Plain chat, plaintext verdicts, edits, legacy partial fields, wrong request,
owner, room, agent, project or input digest cannot authorize anything.

Native request IDs are `approval_` plus 40 hexadecimal characters. The current
legacy JavaScript/Robrix path expects 32 characters. This change does not relax
that parser, send compatible cards or claim a working Robrix approval UI.
Native card formatting/delivery and explicit client protocol work are still gates.

Immediately before possible decision admission, the collector refreshes full
private membership and requires the identical fresh signed key response. The
writer then atomically revalidates the exact frozen request, private binding,
registration, dispatch capability/lease, task/epoch and workspace resource scope
with the decision, grant and source receipt. A concurrent negative snapshot,
revocation, completed task or expiry cannot use an earlier check. A future remote
room change can still race any completed authenticated snapshot; no HTTP read is
claimed atomic with remote server state.

### Durable handoff and replay

The protected journal stores the complete bounded raw sync response, key query,
frozen targets and SDK identity in Prepared before applying SDK mutation. It
persists Applying before invoking crypto and sync. Only completed derivation
becomes Derived. The approval cursor is independently checked on reopen:
Prepared expects the prior cursor, Derived the captured next token; Applying or
Quarantined permits only those two exact values. An interrupted SDK update
remains inspectable and cannot be repeated, skipped or counted as processed.

Every supported raw timeline entry gets source custody, including ordinary
messages and refused verdicts. Source identity includes server, room and event
ID; a separate immutable wire digest covers the entire raw event except top-level
unsigned data. The SDK plaintext/proof digest is separate. Domain receipt input
binds both wire and proof digests plus the full frozen target and action. Thus
ciphertext replay cannot substitute plaintext or acquire a new target scope.
Per-room raw/SDK order and counts must match. The selected typed action and
indexed acknowledgment are validated again when restoring the encrypted journal.

A terminal rejected/non-verdict source remains rejected when a later plan or
SDK trust changes. An identical prior accepted source only replays its original
outcome. Changed immutable content under an existing source quarantines the
batch and retains the original tombstone; it never replaces the source or
submits a new grant. A bounded host status reports stage and counts/bytes, with
no account, room, event, content, key or path projection.

SDK proof, target and domain decision do not share a transaction. A Busy or lost
domain reply keeps Derived custody and does not assert device failure. Exact
historical receipt lookup can settle an already accepted source after expiry or
rotation, without current authority, a new grant or runtime application. An
unaccepted retired target becomes a persistent rejection; an unchanged current
target remains pending during network-free resume. The live intake entrypoint
may retry that current handoff only after fresh checks. Rejected current admission
also retries the historical read to cover another exact host committing between
lookup and admission. Acknowledgment persists before the cursor batch finishes.

An explicit cancellation or timeout cannot abandon a received response between
Start and SDK ownership. Accepted mutations are settled or retained even if the
waiting caller disappears. Closing attempts exact negative fencing and always
waits for owned SDK shutdown, even when the domain target has retired. No shutdown
error is reported as success, and no failed store is silently recreated.

### Bounds and explicit limits

- One owned job per collector; the existing SDK owner has one executing and one
  queued command, bounded deadlines and finite filesystem/journal budgets.
- At most 64 frozen request targets, 100 timeline entries per batch, 256 terminal
  source tombstones and 64 completed approval batches. No history eviction.
- Existing strict HTTP framing, no redirects/proxies/URL credentials, 1 MiB
  response maximum, 256 KiB key query and 3 MiB serialized pending-command bound;
  the entire encrypted journal retains its 16 MiB bound.
- Exhausted completed/tombstone capacity refuses before another HTTP poll.
  An oversized server batch is refused without advancing the prior cursor;
  it is not counted as processed. Retention/compaction needs separate design.
- UTD, malformed events without representable identity, unsupported limited/left
  timelines, ambiguous SDK coverage and interrupted SDK application remain
  quarantined/unknown. This slice does not auto-decrypt them later or resume an
  ambiguous SDK transaction. A host inspection/recovery lifecycle is required.
- The host must supply an existing request plan. Pending-request discovery,
  ordinary users' registration approval, card delivery, runtime application,
  live key lifecycle, service wiring and production cutover remain unsupported.

### Evidence

Local scripted TLS fixtures use real SDK-encrypted owner events and offline
cross-signed identities; fixture provisioning is not in production builds.
They cover all four actions, purpose isolation, wrong device/unverified owner,
plaintext/altered/edited requests, private membership and fresh key negatives,
concurrent cancellation and room retirement, lost domain responses and restart,
Applying status/cursor retention, immutable rejected-source replay, nine protected
journal corruptions, malformed framing, duplicate/UTD batches and actual finite
capacity. Store fixtures cover historical receipt-only recovery, stale negative
CAS, exact scopes/expiry and SQLite rollback of grant plus source receipt.
Cross-platform integrated CI and live deployment are not inferred from local tests.

## Consequences

The approval collector preserves exact encrypted source and durable action receipts. It settles domain decisions without implying that cards were delivered or runtime permissions applied.

## Amendment (PC-C3): the task-bound MCP approval pair and the named refusals

The runner helper's two catalog tools (`get_approval`, `consume_approval`) are
bound to the assigned task by derivation, never by a caller-supplied approval
id: task → live dispatch → `approval_contexts` at the live fence → the newest
`owner_approvals` row at that fence. The read serves `ApprovalSummary`'s four
keys only — no owner mxid, no private room, no tool name or request detail —
so the wire carries exactly what the summary shape already permits. The
consume refuses with a NAMED word instead of the generic authority refusal:
`already_consumed` once the row has settled into `applying`/`applied`, and
`not_consumable` for `invalidated`/`not_applied`/`uncertain`; an undecided
`pending` keeps the generic refusal. The decision input remains the owner's
alone (no `choice` key exists on either tool). The helper process reaches the
store through the runner host API; that host leg's approval route is the
deferred piece (recorded in the spec's Out of Scope), which is why the tools'
dispatch arms enforce their gates and name the leg rather than fabricating a
result.

## Alternatives Considered

Using ordinary room messages, caller verification flags or another collector's cursor would merge distinct authority purposes. Reinterpreting rejected ciphertext after trust changes would abandon immutable source custody.

### ADR112 explicit private sender amendment

ADR112 adds a separate card ledger and explicit fresh-account enrollment to this
same approval-purpose owner. Ordinary outgoing journals remain forbidden. The
existing reader binding stays purpose separated; an inherited ordinary enrollment
profile is rejected, and existing externally provisioned readers do not acquire
fresh sender readiness. Sending requires a qualified original Complete enrollment
and fresh private room/key/domain card checks. Unknown intake or enrollment cannot
be stepped over by a new card mutation. Historical delivery is network-free and
cannot create a verdict. Borrowed close retains its original job/result and blocks
all later intake/observe admission after closure, including failed shutdown.

### Native service notice-driven polling

The native service pump supplies existing request plans from its original
single-consumer run notices, never from discovery or event fields. It retains at
most 64 pending IDs, re-reads their domain summaries, and polls the same original
collector serially at a bounded cadence while pending requests exist. Channel
end/cancellation stops it. Any intake refusal or unknown outcome stops further
polling, retaining original custody without automatic retry or synthetic denial.
All existing SDK proof, current target, source receipt and finite journal bounds
remain unchanged; this does not claim unlimited production polling. The store's
decision is still distinct from card acceptance and native callback application.

### Fleet service scheduling (2026-09-16)

The native pump consumes up to16 original operation notice receivers, each once,
with round-robin polling and no new notice sender or receiver task. The existing
one-slot handoff backpressures admission at capacity. Closing one source does not
close the others; handoff closure drains admitted sources. The shared pending set
remains64. Due verdict intake has priority over ready notices, and its interval is
reset after actual intake returns so slow IO cannot instead starve new cards.
The driver reserves handoff capacity before claiming a dispatch, waits cancellably
without task/workspace authority, and refuses a closed pump before claim. Once its
original Operation exists it moves the one receiver through that reserved slot
synchronously; neither a full queue nor post-Started waiting can lose the handoff
or consume the original workspace acknowledgment deadline.

The same original ApprovalCollector provides a host-only service scheduling turn
to pump initialization/send/intake/close and genuine factory membership admission.
At most16 service callers participate, with one active and waiting bounded by the
existing SDK wait limit. Cancellation/drop returns only scheduling capacity.
Original busy permits, retained jobs, unknown/closed refusal, SDK authority and
current domain checks are unchanged; a lost caller cannot release an in-flight
owned observer. This is composition, not a new queue for SDK operations or an
approval proof. Direct collector users retain their existing Busy semantics.
Pure channel and virtual-time fixtures prove scheduling only; actual executable
encrypted callbacks, original TLS ownership and physical factory tests remain
required. Configured fleet execution and real client soaking are separate gates.
