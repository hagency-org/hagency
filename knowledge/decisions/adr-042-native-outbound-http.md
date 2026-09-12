---
kind: decision
id: ADR-042
title: Confine outbound HTTPS effects to durable host transport custody
status: Accepted
requirements: [REQ-PALPO-OUTBOUND, REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Actual outbound HTTPS effects must preserve the registration custody kernel's finite queues, frozen publication and explicit uncertain outcomes.

## Decision

`hagency-palpo` implements the next M5 transport slice over ADR-037 custody. It
does not replace canonical domain admission, authenticate Matrix sender/member
evidence, run an Agent or complete the Rust migration. The existing custody
database remains schema 2; no domain schema is changed.

### Host identity and actual transport

Only trusted host code constructs `HostConfig`. It binds the exact fleet URL,
registration identity, machine credential and machine generation. It has no
Deserialize/Debug implementation, browser handler or mutable credential setter.
The fleet ID must match Palpo's `hf_` plus 32 lowercase hexadecimal digits and
the URL must end exactly at `/api/fleet/v2/{fleet}`. URL userinfo, query, fragment,
encoded/normalized path tricks and token-bearing URLs are refused. HTTPS is
required except explicit literal loopback HTTP; unlike legacy configuration,
plain HTTP `localhost` names are not admitted. URL spelling must be canonical.
Changing endpoint/credential at the same generation conflicts with the stored
credential fingerprint. Registration replacement is still refused by custody;
machine rotation does not replace the Matrix registration.

The pinned reqwest 0.12.28 client uses rustls and WebPKI roots, normal certificate
chain and hostname verification, HTTP/1, sensitive bearer headers and the exact
`X-Hagency-Generation`. Host code may add at most four 16 KiB PEM trust anchors;
there is no option to disable verification. Environment proxies, redirects,
referer, implicit protocol retries, cookies and automatic compression are
disabled. No dependency error is propagated as an error source or formatted for
the caller; public errors contain static classifications and numeric HTTP
status only. Opaque payload, token and URL types have no public Debug projection.

The crate launches no reverse listener or detached polling task. One per-lane
try-lock admits a Matrix call, a work call and a publication call; extra callers
receive Busy rather than occupying an unbounded waiter queue. DNS is confined
to the configured host with at most three blocking resolver jobs and 16 returned
addresses. A permit stays inside each OS resolver job even if its async caller
cancels; an uninterruptible OS lookup may occupy a slot, never create unlimited
abandoned lookups. This is a bound per Adapter instance, not a global guarantee
for arbitrary host-created instances or dependency runtime tasks.

### Framing, deadlines and finite custody

Default connect/header/body-idle budgets are 5 seconds. A normal request has a
10-second absolute deadline; a long poll adds its explicit wait (at most 25
seconds) to both header and total deadlines. Body chunks cannot extend the
absolute deadline by arriving just before the idle timeout. Host-configured
non-poll timing budgets stay between 10 ms and 60 seconds. Retry delay increases
from one to at most 30 seconds by default, resets after success and is cancellable.
Only transport failures/timeouts, Busy, HTTP 429 and 5xx are retried by the run
loop. Authentication, generation, malformed success, capacity, identity conflict
and unknown writer outcomes stop it visibly. There is no hot-loop fallback.

Accepted headers are bounded to 16 KiB and 64 fields after parsing. The pinned
Hyper 1.11.1 HTTP/1 parser itself uses its finite 417792-byte buffer; this crate
does not claim the stricter accepted-header limit applies before that parse.
Poll response bodies are capped at 4 MiB plus 16 KiB (host may lower the cap),
other response bodies at 64 KiB. Publication bodies are at most 1 MiB. Reads
check declared length and actual bytes; HTTP framing is delegated to the pinned
HTTP/1 implementation. The strict JSON reader rejects duplicate decoded keys
at every object depth, trailing/coalesced JSON documents and malformed numeric
values. Serde's parser depth is bounded at 128, then the existing transport
encoder enforces retained depth 64. Parsed values have allocation overhead;
these are serialized byte limits, not exact heap-usage promises.

Successful polls require a single v2 envelope, the exact current machine
generation, an explicit delivery field, exact lane/kind, bounded ID/token and
RFC3339 lease expiry. Matrix deliveries additionally bind transactionId to ID
and require a bounded events array. All transaction fields, ephemeral/device
data, opaque object keys and fractional values are retained. Receiving these
bytes establishes local custody only, never Matrix source authority.

The inherited bounded Store worker records the full delivery before any ACK.
It then records an unknown ACK outcome before external I/O. Only an exact
successful response completes that ticket; an exact stale_lease response
records the need to reclaim. Timeout, cancellation, invalid success and missing
response leave original custody/uncertainty intact. Repeating an accepted ACK is
safe, including Palpo's accepted-but-response-lost case. The bounded worker's
execution-time clock still applies to its own processing leases.

The sole new repository seam is host-only `AckHead`: the earliest current-scope
unconfirmed lease, including a processing tombstone already marked done. Normal
Head excludes done rows, so it cannot recover a lost ACK on completed redelivery.
AckHead checks current registration/scope and machine generation and excludes
accepted, stale and retired leases. It uses existing schema-2 columns; no schema
or canonical state is duplicated. Old-generation pending Matrix/request rows
survive rotation with retired ACK observations, never invented remote acceptance.

### Independent loops, publication and cancellation

Matrix polling waits for host consumption after local custody is acknowledged
or retained across rotation. Its FIFO Head cannot be bypassed by later work.
The work and publication loops continue independently. A host consumer uses
the opaque scope with Store Claim/Start/Complete/Inspect, keeping a stable
attempt ID distinct from a delivery ID. An unknown attempt cannot be started
again; its original owner's receipt must be inspected. HTTP receipt handling
does not approve a resource request or settle a canonical task.

Host status/capability observations are frozen with Store-owned version,
generation and sequence before POST `/updates`. Exact bytes and original
observedAt survive retries and restart. Conflicting replacement data is refused
while an older body is pending. Only the exact successful ticket removes that
body. Explicit machine rotation fences the old outbox and late responses; an
ordinary failure does not reset sequence, rebuild status or refresh timestamps.
The run loop creates heartbeat-only publications when no body is pending.
Heartbeat is transport liveness, never readiness or connection proof. Producing
current domain statuses, catalog observation and exact Matrix probe receipts
remain host-adapter responsibilities.

`run` joins its three loops. Cancellation stops network work and backoff; already
validated received work and known successful receipts are allowed to commit
before returning. A fatal loop error cooperatively cancels siblings and waits
for that same commit boundary. Dropping the whole host future or process can
leave a bounded worker command's outcome unknown; recovery consults its original
durable identity. No ACK is sent ahead of persistence, and no received record is
deleted to implement shutdown. Old in-flight poll/ACK/publication responses are
fenced when the host activates a new machine generation.

### Evidence and remaining gates

Protocol reference is read-only Palpo commit
`c7c400e04ab05479a63c30f14679ec0180457d85`: `web-admin/server.mjs` lines 52-77
provide routing, fixed-origin/bearer/generation checks and request size limits;
`web-admin/lib/outbound.mjs` provides leased poll/ACK and frozen update behavior.
The exact paths are GET `/poll`, POST `/ack`, POST `/updates` (plural).
The reference fixture actually instantiates that commit's createApp/Outbound
with an in-memory database and a homeserver adapter that throws on any I/O,
then invokes the native client over literal loopback HTTP. Palpo accepts both
lanes' ACKs, empty poll and a v1 status inside v2 updates. Its old observedAt
stays old and its fleet stays pending_connection with no connection proof.

Normal Cargo tests use independently scripted TCP/TLS listeners and real SQLite
custody. They cover verified TLS/hostname refusal, disabled environment proxies,
redirect/status/error redaction, header/body/depth/duplicate/framing limits,
deadlines/backoff, lost-response/restart, completed tombstone ACK recovery,
changed replay, stale lease, rotation, unknown host attempts and lane concurrency.
The intentionally public fixture EC key scalars are 42/43, not real credentials.

These tests do not establish a real deployment's DNS/CA/network availability,
Palpo version compatibility beyond the pinned reference, Matrix SDK source trust,
domain request/probe admission, runtime execution, continuous storage retention,
secure host configuration persistence, Agent retirement HTTP endpoint parity,
or live end-to-end UX. Those M5-M9 gates stay open. CI must still exercise this
slice on every supported platform before native production cutover.

### Windows CI diagnostic after ae284b9

The Windows full-suite run reports OutcomeUnknown during transport attachment
and domain observations; several subsequent scripts therefore receive no HTTP
request. This is a failure, not evidence of a rolled-back activation. Earlier
platform passes do not invalidate it, and the log does not establish its cause.
The native workflow now runs the same Matrix/Palpo transport targets with
`--test-threads=1 --nocapture` only after that platform's full suite fails, with
an eight-minute outer bound. The original verdict remains failed regardless of
the diagnostic result. Assertions, worker/HTTP deadlines and production
concurrency remain unchanged. The comparison supplies additional evidence about
concurrent fixture load; a serial pass alone does not establish a root cause.

## Consequences

Authenticated local TLS fixtures qualify transport boundaries, not Matrix provenance or native deployment parity. Failure-only diagnostic runs preserve the original failed full-suite verdict.

## Alternatives Considered

Redirects, environment proxies or implicit HTTP retries could change the destination or repeat uncertain effects. Replacing a failed full run with a serial diagnostic pass would also discard original evidence.
