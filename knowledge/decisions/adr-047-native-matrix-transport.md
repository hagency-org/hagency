---
kind: decision
id: ADR-047
title: Collect bounded authenticated Matrix observations with owned encrypted SDK state
status: Accepted
requirements: [REQ-PALPO-OUTBOUND, REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Native Matrix observations require a configured authenticated origin, exact account/device identity and separately owned encrypted SDK state.

## Decision

`hagency-matrix` is a limited M5 account/device and room-observation collector.
It performs actual authenticated HTTPS reads and persists SDK state, but does
not admit Matrix events, execute owner approvals, publish keys, register remote
accounts or send messages. Its summary contains only generation and room count.
This is not a full Matrix client or a completed native deployment.

### Pinned SDK boundary

The existing SDK foundation pins matrix-sdk-base/common/crypto/sqlite and
store-encryption 0.18.0, with Ruma 0.16.0. The high-level SDK accepts a configured
reqwest client, but its response conversion collects `response.bytes()` without
an application body cap. Passing a reqwest builder alone cannot enforce our
preallocation body budget. We therefore own bounded HTTP reads and hand a
validated typed sync response to the lower-level BaseClient. This is an explicit
limited state-machine integration, outside the normal high-level Client path.
The inspected upstream code is the pinned
[HTTP implementation](https://raw.githubusercontent.com/matrix-org/matrix-rust-sdk/matrix-sdk-0.18.0/crates/matrix-sdk/src/http_client/mod.rs)
and [client builder](https://raw.githubusercontent.com/matrix-org/matrix-rust-sdk/matrix-sdk-0.18.0/crates/matrix-sdk/src/client/builder/mod.rs).

BaseClient uses encrypted SQLite state and crypto stores, CrossSigned decryption
trust, disabled verification-event handling and disabled threading support.
No SDK handle or OlmMachine escapes the owner. The collector never drains
outgoing crypto requests or invokes a send API. Existing offline crypto tests
prove cross-signed encrypted fixture restart independently; whoami alone does
not prove published device keys, cross-signing or historical key availability.

### Host identity and authenticated observations

HostConfig has neither Deserialize nor Debug nor a mutable credential setter.
It pins a canonical homeserver origin, external access token and storage key,
registration fingerprint/generation, engagement, full account MXID and device,
Matrix transport incarnation and a fixed set of host room/privacy/generation
intents. A Matrix transport incarnation is not Palpo's rotating machine-token
generation. Access-token rotation with the same authenticated account/device
does not replace the SDK identity. A changed homeserver, registration, account,
device or room set conflicts with the protected store binding. Such replacement
requires a separate explicit lifecycle; no import/reset fallback exists here.

GET account/whoami must return the exact full MXID and a present exact device ID;
missing device IDs, foreign localpart lookalikes and guests are refused. An
App Service user token without a real device is not enough. Only after whoami
does the collector open/create SDK state and accept a bounded sync response.
GET sync has timeout=0, full_state=true, explicit room filters, zero timeline
limit and no requested presence/ephemeral/account-data events. Returned sync
room IDs must remain inside the pinned set even if the server ignores filters.
No returned event becomes a domain message, approval verdict or send receipt.

Every room observation comes from an authenticated GET rooms/{room}/state full
snapshot, not lazy/stale SDK membership, m.direct, display names or a browser
claim. Duplicate (type,state_key), wrong room IDs, unknown membership values and
unsupported encryption are refused. Direct scope additionally requires exactly
the host-pinned human and Agent joined, invite-only rules and Megolm encryption.
The domain independently checks project owner, registration, room and transport
generation. Room generations are host-coordinated shared scope, not per-device
counters guessed by this collector. Unknown encryption never falls back to
plaintext. No members or history are silently truncated to meet a limit.

### Negative transport authority and schema 15

Domain schema 15 adds transport availability and a content-bound invalidation
receipt. Existing schema 14 identities stay positive until explicitly invalidated.
A host invalidation names the exact engagement, registration, Matrix generation,
full sender and device captured before I/O. Delayed negative responses cannot
retire a replacement generation. Initial failure may record unavailable gen 1;
no positive device observation is manufactured. Identical negative replay is
idempotent; changed replay conflicts. Same-generation positive replay cannot
restore unavailable state; fresh authenticated generation is required.

The available predicate joins every current Matrix route. In one transaction,
invalidation retires old sessions/conversations and approval grants, invalidates
pending/decided owner approvals, preserves applying approvals as uncertain, and
runs the existing final-reply and schema 14 notice custody retirement. A send
that may have crossed its external boundary remains uncertain; unsent work is
cancelled, never reported delivered. Other Agents' grants in the same owner
approval room survive. Required columns and the retirement trigger are checked
on open; partial migration failure rolls back without erasing device identity.

This first collector conservatively fences the entire device incarnation after
any incomplete authenticated collection. It also captures shared room scope
before each full-state request. Representable unsafe snapshots reach the domain
invalidation path; failed, malformed or unavailable snapshots invalidate the
exact previously captured room generation, retiring other Agents' old room
routes too. A newer concurrent room generation is preserved. The collector
attempts both the captured old transport and the configured attempted incarnation
on failure, covering positive commits whose response was lost without targeting
an unrelated newer owner. Fine-grained continuation in unaffected rooms is not
implemented. Successful close
also fences its exact current incarnation. A future live execution host must
stop using an incarnation whenever negative persistence itself returns Busy,
unavailable or OutcomeUnknown; a failed database cannot guarantee immediate
atomic retirement. The collector returns unknown in that case, never success.
No live sender/executor is wired to this collector in this slice.

### Storage ownership, sync receipts and cancellation

The private directory, create-only binding and public-key fingerprint files,
SQLite databases and journals reject public permissions, foreign ownership,
symlinks and hard links under the existing platform policy. Existing DBs without
our binding, missing/empty/corrupt files, missing SDK account, wrong external key
and another owner fail visibly. Interrupted fresh bootstrap is quarantined;
it does not generate a replacement identity. The token is never persisted.

One dedicated owner thread holds an exclusive filesystem lock outside its
current-thread Tokio runtime. SQLite pools have two connections each. The
worker drains accepted commands, closes stores and drops its runtime before
releasing the lock; an abandoned caller cannot release ownership while SDK work
is still running. Explicit close is acknowledged after that boundary; a store-close error returns
unknown rather than successful shutdown. Network
work uses one per-collector try-permit. Each collect runs as one finite owned
job so dropping its caller cannot abandon failure fencing; there is no detached
sync loop. Cancellation stops HTTP, and already accepted SDK work is settled or
reported unknown before the domain transition. Timed-out accepted SDK mutations
retain their lock and original receipt until completion/inspection.

The SDK's set_custom_value API stores caller bytes opaquely; enabling its store
encryption does not encrypt those bytes. We explicitly encrypt the entire sync
journal using pinned StoreCipher. Its random cipher is wrapped by the external
32-byte host key in a private create-only journal.key file. No plaintext pending
JSON or fractional payload is written into SQLite. Before SDK ingestion the
full response is durably frozen as pending; after successful SDK persistence a
content digest and next_batch token are recorded. A crash between stores cannot
be assumed atomic: retained pending work requires host inspection and stops
bootstrap. It is not silently replayed or declared successful. Exact completed
replay is content-bound; changed token replay conflicts. The SDK's own same-token
shortcut is not used as proof of matching content.

### Bounds and remaining gates

A collector accepts at most 16 fixed rooms, 1000 events per response and 1 MiB of
serialized JSON. Host limits may be lower. A private sync journal retains at
most 64 completed receipts plus one full pending response, with an 8 MiB bound on
its encrypted JSON envelope (ciphertext is serialized as decimal byte arrays). Exhaustion rejects
new work, retaining prior receipts and keys; continuous operation/retention
beyond that budget needs a separate design. SDK store files have a 64 MiB admission
limit and only exact known filenames are accepted. These finite input/history
bounds are not exact heap or filesystem quota guarantees for SDK internals.

HTTPS uses pinned reqwest 0.12.28/rustls, normal CA/hostname checks and optional
explicit host trust anchors. Only literal loopback IPs may use HTTP. Userinfo,
query/fragment/normalized URL tricks, environment proxies, redirects, implicit
HTTP retries, cookies, referer and compression are disabled. Default deadlines
are 5 s connect/header/body-idle,15 s total request and 20 s SDK wait; host timing
limits range 10 ms–60 s. Accepted headers cap 16 KiB/64 fields after the pinned
Hyper HTTP/1 parser's finite 417792-byte buffer. Actual body bytes and declared
length are checked. Strict JSON rejects duplicate decoded keys, extra documents
and depth above 64 after bounded parsing. DNS retains at most 3 resolver jobs and
16 addresses, including cancelled blocking lookups. Extra collection and SDK
queue callers get Busy; queued work holds at most one bounded response besides
one executing response.

Local scripted HTTP/TLS, real SQLite and offline crypto fixtures cover account
identity, full room state, unsafe DM changes, protocol bounds, cancellation,
restart, explicit pending uncertainty, changed replay, capacity, rollback and
ownership retention. They establish neither a real homeserver's registration
or key publication nor Matrix event provenance, recovery from pending SDK work,
notice/final send authentication, remote provisioning, historical key import,
ongoing room-set changes, live UX or M5 completion. Live device key matching and
cross-signing must be proven before enabling a native encrypted sender.

## Consequences

The bounded collector retains sync and storage uncertainty without implying event admission or message delivery. Enrollment, key lifecycle and continuous retention remain distinct gates.

## Alternatives Considered

Trusting caller-supplied verification flags or silently adopting another SDK device would bypass authenticated identity. Dropping pending sync history at capacity would erase the custody required for recovery.
