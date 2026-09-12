---
kind: decision
id: ADR-037
title: Retain registration custody across outbound machine credential rotation
status: Accepted
requirements: [REQ-PALPO-OUTBOUND, REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Outbound transport must retain registration-bound custody while machine credentials rotate independently of canonical domain authority.

## Decision

The native M5 custody kernel extends the existing bounded writer and
`custody.sqlite3` to schema 2. It does not create another domain store. Agent
allocations, canonical requests, tasks and approvals remain in `domain.sqlite3`.
This implements local custody transitions; it does not complete M5 or establish
an authenticated Matrix connection.

### Authority and separate generations

A host-only activation pins the binding ID, canonical side ID, fleet ID, Matrix
registration fingerprint and registration generation. A unique side/fleet key
refuses a second binding namespace for the same fleet. Existing operator fixture
bindings cannot be adopted, even with matching values; fixture receive cannot
write a managed binding. Replacing the Matrix registration requires a later
explicit reconciliation saga, never automatic adoption of another registration.

The machine generation and a credential fingerprint belong to a different
lifecycle. Activation returns an opaque scope and a durable random UUID consumer.
Repeating the same activation returns the same scope; advancing the machine
generation fences old scopes but retains the consumer and existing Matrix/request
custody. A changed fingerprint at the same generation or a decreasing generation
is refused. No machine/Matrix credential is stored here. The private database
retains lease tokens and internal scope/attempt keys; those types implement
neither Debug nor serialization. Public delivery views contain receipt metadata,
processing/lease state, attempt IDs and result digests, never private payloads or
lease/scope capabilities.

The command enum has no Deserialize implementation and no HTTP route. The later
host adapter must obtain registration identity from trusted configuration and
verify fleet-scoped HTTPS authentication, wire version, response generation and
source authority. Host clocks and inspection outcomes are not external message
fields. `Receipt.generation` remains the Matrix registration generation;
`origin_machine_generation` is separate diagnostic/custody provenance. Neither
field substitutes for the Matrix account/device incarnation used by domain reply
observations.

### Receipt and processing boundaries

Each lane's issued poll ticket is current-scoped. A new poll fences an older
response; the same poll can only replay an identical delivery response. Receive
atomically commits the full transaction, content digest, original receipt and
current lease before a private ACK ticket can be issued. Matrix transactions keep
all event, ephemeral, device and other JSON fields. The transport encoder uses
JavaScript finite-number and key ordering semantics, including fractions and
opaque `__proto__` data keys. Existing signed DTO and execution-payload encoders
keep their stricter checks; opaque transport data is not execution authority.

Beginning an ACK records uncertainty before external I/O. Its completion matches
the exact current scope, lane, delivery ID, machine generation and lease token.
A replaced token response cannot acknowledge or downgrade the current token.
Repeated accepted ACKs remain accepted even if a later response is lost. An
explicit stale-lease response requires a new lease; identical delivery replay
does not erase that observation. ACK means persisted local custody only.

Machine rotation retains the original payload, digest and receipt of locally
owned Matrix/request deliveries. Previously accepted ACK observations remain;
other old lease observations become `retired`, not fabricated `accepted` ACKs.
Under the unchanged Matrix registration, this retained custody can continue to
an idempotent host adapter handoff. Completed rows are never re-executed on
redelivery, but a fresh remote lease still gets its own ACK.

An old Matrix transaction may itself contain a connection-probe event. Its full
payload remains intact and its original machine generation remains explicit. The
future Matrix adapter must prevent those embedded old-generation events from
establishing new proof, as the legacy bridge does. A transaction completion cannot
enter the publication's probe-receipt path, which requires a current-generation
work delivery of kind Probe; preserving bytes is not permission to reuse proof.

Claim records a bounded attempt ID, private capability and deadline. Start returns
its complete payload once. Repeated start is refused, including a lost start
response. Completion records the exact adapter result receipt and replaces only
the completed inbox payload with an empty tombstone. Identical completion retries
return the same digest; changed results conflict. Completing a delivery is not
completing a canonical task or approving a project request.

Startup and expiry retire unstarted claims for safe retry. Started claims become
`unknown`; an adapter can also persist uncertainty immediately. Unknown work
blocks later Matrix work until the current host inspects it. Inspection can
confirm the exact completed result or explicitly permit retry after checking the
external owner's receipt. A claim cannot turn timeout into permission to repeat
an effect. Head/view queries expose enough bounded metadata to discover unknown
attempts after restart. Matrix delivery ordering uses committed arrival order;
the work lane proceeds independently.

### Frozen publication and retiring proof

One publication slot per binding contains an immutable v2 body and increasing
JSON-safe sequence. The writer adds v, machine generation and sequence; it never
refreshes host-supplied status timestamps or synthesizes readiness. Begin records
uncertainty, and retry sends the same bytes. Changed content at the pending sequence
conflicts. Accepted exact receipts advance the last accepted sequence/digest and
remove the pending body; a lost or rejected response retains it. An old accepted
response cannot clear a newer pending publication. Exhausted sequences fail
visibly. The latest accepted receipt suffices for this strictly serial slot;
earlier publication identities remain fenced by the monotonic sequence.

Old probes become retired on machine rotation, including claimed, started and
unknown attempts. Their completion authority is fenced. A publication containing
probe receipts additionally requires an equal completed probe result owned by
this exact machine generation. Old proof cannot be copied into the new generation.
This local check is necessary but does not establish real Matrix source-event or
membership proof: the authenticated host adapter and Palpo validation still must
supply that proof. A mere heartbeat is never connection verification.

Rotation explicitly retires the old generation's frozen publication. A late ACK
or update response for that retired scope cannot mutate the new generation. This
is the only implemented publication reset; a timeout does not discard, rewrite
or rebuild a pending body.

### Capacity, recovery and remaining integration

The existing worker retains a finite command queue (default 16), a 16 MiB
serialized input budget and a two-second response deadline. Host time is anchored
to a monotonic Instant before
validation/enqueue and advanced at execution, rounding elapsed milliseconds up.
Queued Start or Complete cannot retain a capability that expired in the queue.
An unsafe clock overflow fails closed. Delivery data is at most 4 MiB with depth
64; escaped envelope strings are charged conservatively.
Opaque ACK tickets include their maximum 4096-byte token in the budget. These are
serialized-byte bounds, not a claim that parsed JSON has no allocation overhead.
Publication bodies are at most 1 MiB with 200 statuses and 10 probe receipts;
adapter result objects are at most 64 KiB.

The repository retains at most 1024 binding records, 1024 inbox records including
tombstones, 4096 attempt records and 16 MiB of combined stored inbox payload,
adapter result and publication body bytes. Fixture intake shares that byte bound.
Capacity errors roll back the entire command. Identical receipts remain available
at record capacity. No pending input or arbitrary-ID dedup tombstone is evicted to
make space. Retention/capacity maintenance beyond these finite limits remains a
release gate, as it does in the pinned Palpo implementation.

Schema 1 fixture rows remain unchanged and unmanaged. Schema 2 DDL and structural
verification commit together; failed migration leaves version 1 and its original
rows intact. Opening/recovering the custody store does not inspect or change the
domain database. Durable receive, claim, completion, rotation and publication all
use the existing single writer and SQLite transaction boundaries.

No network/client crate, polling loop, bearer-secret storage, server deployment,
Matrix SDK authentication, domain request admission, status observation scheduler
or live E2E workflow is included. Those adapters must preserve these host-only
boundaries and reconcile original owner receipts before releasing unknown work.

## Consequences

The custody writer preserves exact receipts and frozen publication identities across supported rotation. Unknown cross-owner outcomes remain inspectable rather than becoming new execution grants.

## Alternatives Considered

Adopting fixture bindings or replacing registration identity through a token rotation would merge distinct authorities. Splitting domain invariants into custody storage would also violate their existing single-writer ownership.
