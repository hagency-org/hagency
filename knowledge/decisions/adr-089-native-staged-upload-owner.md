---
kind: decision
id: ADR-089
title: "Own one staged encrypted upload through private historical settlement"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

The staged upload owner must join original domain, staging, HTTP and SDK custody without repeating a possible POST or confusing upload acceptance with file-event delivery.

## Decision

ADR078 records original upload identity before staging and never reissues a send
grant after WritePossible. ADR079 commits actual encryption identity before local
IO; ADR077 restores original qualified ciphertext without reconstructing source
Snapshot authority. ADR083 retains actual bounded HTTP response bytes, ADR084
protects those bytes in the SDK journal, and ADR085 restores historical domain
settlement without original runtime secrets. This slice joins those primitives
into a host-driven staged upload. It sends no room event and declares no task Done.

StagedUpload consumes the actual RunnerCapability, original UploadClaim, unique
UploadSend and actual RestoredEncrypted. ADR090 compares the send's complete
private upload/request/capability identity and fence to the claim. The constructor
also compares original namespace digest, operation ID, receipt digest, encrypted
kind, qualified sync evidence and exact ciphertext length to the staging
commitment. Public RunnerCapability strings are also synchronously checked against
original upload admission bounds (dispatch/runner identifiers128, canonical secret64,
nonzero fence) before retention or cloning. Collector admission repeats this check: a returned failed constructor
contains original custody, not a way to assert valid association. Failure returns
all original inputs without Debug/serialization or replacement grants.

Collector::stage_upload admits synchronously into a two-slot retained registry.
The original UploadOperation drives one future, with its attempt marker set before
any await. Each job holds original ciphertext, descriptor and bounded original
capability/claim; its HTTP request adds at most one ciphertext copy. Both ciphertexts
are at most16MiB, checked response body at most4096bytes, and the existing SDK
journal has independent permanent limits. Held handles, active attempts, unknown
outcomes and uncommitted responses share the two slots across Collector aliases
and SDK owner reopen. These are logical byte/record bounds, not allocator, TLS or
SQLite physical memory guarantees. No upload network task or wait queue is spawned.

Every run requires the exact available prior Collector transport and a current
Started dispatch/task/lease/route, then authenticates the CURRENT configured token
with the actual whoami endpoint. Config.binding intentionally excludes credentials;
old observation plus old SDK alone cannot authenticate a replacement token.
Negative account evidence owns one independent finite completion retaining the
same job permit through exact domain fencing. Dropping the driving future cannot
discard that already-observed failure before enqueue. This is the only spawned
completion; it never performs HTTP or creates upload permission. Failed enqueue
or uncertain database completion remains OutcomeUnknown, never claimed fenced.
The separate ADR091 enqueued-invalidation rule closes downstream receipt-loss
cancellation without changing ordinary domain work.

The private SDK owner consumes Send, persists its exact reserve and Possible
records, and returns one ephemeral LivePermit only once. Missing owner, bootstrap
corruption, conflicts or lost acknowledgements cannot authorize POST. Before HTTP,
the SDK mutex and Collector busy permit are released; authenticated negative
observations remain runnable during the network wait. The exact original claim
is validated again by the domain transaction AFTER the final SDK Possible await
and immediately before first HTTP polling. No replacement scope or grant is used.
The finite host operation deadline covers all normal awaits, with existing tighter
HTTP header/body/request limits. Synchronous bounded copies/JSON work are not hard
real-time. Current permission is checked at this boundary; remote membership can
change afterwards, and an already-started network effect cannot be recalled.

Only the configured HTTPS homeserver receives ciphertext and bearer through the
existing no-proxy/no-redirect/no-retry fixed binary upload path. RestoredEncrypted
has no public arbitrary upload method. UploadSend cannot be cloned or recovered
from a claim after ACK loss. The response moves into the original retained job
before another await or the final cancellation check. Complete validated late
response bytes are historical evidence; cancellation/deadline refusal remains a
failure and never rearms the POST. Malformed/truncated responses produce no sealed
accepted evidence. Dropping every owner still loses uncommitted memory; no library
claims that absence of a retained response means the repository has no upload.

Exact settle_upload(id) either uses that job's own sealed response or an actual
accepted record restored by the existing protected SDK owner. It never accepts
caller-provided body, URI or acceptance data. Persistence failure poisons the SDK
owner but the Collector retains its original response. Explicit bounded
reopen_upload_owner can replace only the existing SDK owner; retrying historical
settlement submits the same original response to the journal without POST.
Only the SDK's opaque receipt commitment reaches exact ADR085 historical domain
settlement. Revocation/cancellation stay sticky and do not prevent acknowledging
actual old acceptance. This does not revive current send authority.

Collector::close now borrows its caller so refusal cannot consume the last
retained owner. It atomically refuses retained jobs before closing admission.
Known completed pre-HTTP refusal may release map retention; a held Operation still
holds its slot. release_unstarted_upload uses the same per-job mutex as run and
terminalizes the local attempt before removing its map entry. It refuses any
active driver or possible HTTP write. This is local memory release, not a NoWrite
verdict, domain rollback or retry permission. Once HTTP was possible, only exact
historical settlement releases map retention. Closing a revoked engagement still
returns the preexisting domain-authority error; this slice does not reinterpret
that result as successful transport invalidation.

Tests use actual files, codec encryption, qualified private staging, authenticated
SDK collection, local TLS POST, SDK SQLite failure triggers and actual domain
mutations. The completion fixture replays already-accepted history in a fresh process.
The recovery fixture separately aborts the first domain acceptance UPDATE after
actual private SDK acceptance, verifies WritePossible, closes/drops all original
owners and capabilities, and launches a fresh native process with only a known ID,
original host config and protected journals. That child performs first acceptance
with replayed=false and then acknowledges the same receipt with replayed=true. The actual SDK/domain historical path reconstructs accepted
settlement with no network call. This is recovery from lost process memory given
a retained selector; it is not selector/key discovery or a claim of arbitrary
production startup wiring. Tests also cover exact same-fence mismatches, failed
constructor roundtrip, bounded caller-mutable capability fields, current-token identity change, negative fencing after caller
abandonment, release/close races, privacy retirement after Possible, future drop and cancellation after complete response observation,
malformed/truncated HTTP, finite held capacity and post-revocation response recovery.

On Windows, FileSyncedDirectoryUnconfirmed cannot create PreparedEncrypted or
RestoredEncrypted. Each scenario explicitly permits only the refusal qualification
on such a platform: exact original ciphertext/descriptor/custody is returned,
plaintext still decrypts, domain stays Pending, and no POST occurs. Those printed
refusal branches are not positive Windows end-to-end upload passes. Only platforms
with actual qualified sync can execute the positive workflow; cross-compilation
proves neither directory durability nor network workflow execution.

Physical capture-to-dispatch workspace authority, blocking staging worker, original
file-event filename/MIME metadata retention, encrypted m.file/m.image sending,
cache paths, MCP tools, automatic startup selectors, service activation and full
migration parity remain gates. UploadRequest metadata currently participates in
the domain request digest; this owner does not pretend it can reconstruct that
metadata from RestoredEncrypted. Safe receipts carry only upload state/opaque
identity, never private descriptors, response bytes, MXC, paths or capability keys.

## Consequences

Bounded owned work retains exact response association before historical domain settlement. Windows durability refusals remain negative coverage, and source binding, metadata, MCP and event publication remain separate gates.

## Alternatives Considered

Rearming an attempted upload after cancellation or reopening would repeat unknown external effects. Settling from an unrelated checked response or calling upload acceptance Delivered would bypass distinct association and room-event custody.
