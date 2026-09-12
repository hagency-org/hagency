---
kind: decision
id: ADR-097
title: "Keep file metadata and publication custody separate from upload acceptance"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
---

## Context

ADR078 records an original upload before capture; ADR089 supplies the real retained
upload owner. An accepted media POST does not acknowledge a room event. Proposed
ADR092 needs immutable filename/caption and original captured facts retained beside
that upload, with separate current event authority and historical Delivered state.
This decision accepts only the domain slice; actual publisher, source, service and
MCP acceptance remain separate mandatory integration work.

## Decision

Schema020 adds a bounded file_deliveries row referencing one original file_uploads
row. reserve_file_delivery uses one BEGIN IMMEDIATE transaction for immutable
metadata and the existing uploads::reserve operation. It validates full bounded
metadata and the private selection/request commitment; changing filename or caption
while reusing a supplied digest conflicts. No source path is stored. Only a new
committed admission returns the existing opaque upload preparation. Exact replay,
lost replies and reopen cannot issue another capture. A pre-existing upload with
no delivery row is not adopted as permission or assigned invented metadata.

The host binds actual captured length and SHA256 plus original stage commitment
in one transaction using that original preparation. Captured size must equal
stage length for the fixed Matrix v2 AES-CTR codec, so the product bound also
covers the actual ciphertext. Both observations are immutable; reusing the request
digest cannot replace them. Initial capture facts refuse after a recorded staging
outcome or upload departure from Pending; old upload-only methods cannot backfill
them after IO. These are host-provided facts, not proof that this database performed
filesystem IO. Actual retained source/encryption
and qualified durability must be supplied by the consuming owner. The first file
product bound remains 4 MiB with fixed application/octet-stream MIME, filename 255,
caption 1000, call_id 128 and 64 lowercase-hex digest bounds. Global 4096 and
per-dispatch 16 rows are permanent; existing upload limits remain independently
enforced. The file row's metadata/capture/publication/receipt payload has an 8 KiB
encoded ceiling; original metadata plus frozen route is also bounded at admission.

Publication has its own Pending, Claimed, WritePossible and Delivered phases,
claim secret, increasing fence and stable transaction identity. A new claim/begin
requires the exact current original Started capability, task/epoch, exclusive lease,
resource/registration and frozen encrypted route plus original Accepted upload.
Current checks sample time after writer queue and SQLite lock waits. An expired
unstarted claim can be replaced under current authority; WritePossible never can.
Only the first durable begin returns an opaque nonclone publication send. Exact
validation must run after final private SDK preparation and before actual HTTP.
Losing begin's reply cannot reconstruct a send. Cancellation is sticky and affects
both original upload and event permission without erasing either historical fact.

The send compares the actual retained opaque UploadClaim, including its full
private identity and original upload fence; it cannot reconstruct consumed
UploadSend. The publisher separately compares its private SDK/media association.
The send borrows its original opaque file identity for retained cancellation or
uncertainty after consuming the unique send; this creates no new current grant.
The send binds original upload identity/fence/stage/acceptance, full immutable file
metadata and capture facts, frozen route, event fence and transaction/content
commitment. A historical locator is bounded correlation data; only its exact
protected row returns an opaque settlement handle. The handle supplies no claim,
send, preparation or current authority. Historical settlement may record the exact
event receipt after cancellation, retirement or process loss; substitutions and
conflicting repeated receipts refuse. It never changes canonical task state.

Receipt commitments follow the existing trusted host adapter boundary: public
correlation data is not a verified SDK proof type and has no runtime/HTTP decoder.
This domain slice does not introduce a fake verified acknowledgement constructor.
The future Matrix publisher must associate and durably retain its actual complete
private SDK acknowledgement before calling settlement. Domain fixtures can test
transaction/fence rules with explicit host data; they cannot qualify that publisher.

Safe status includes delivery ID, safe filename, captured size/hash when known,
separate upload/event phases, cancellation, fixed refusal codes and acknowledged
event ID after Delivered. Caption, route, source selection, private receipt IDs,
descriptor, MXC and keys are excluded. A possible upload/event or unknown staging
remains outcome_unknown even if cancelled; upload Accepted alone remains queued
or a known terminal refusal, never Delivered or Done. Status reads and exact
replay remain available at capacity under the original capability association.

## Consequences

The single domain writer owns atomic metadata, one original preparation and exact
event history without becoming a filesystem or Matrix verifier. Older upload rows
gain no delivery grants on migration. Service/MCP and actual SDK integration remain
required before file delivery is available; unsupported Windows durability and
runtime qualification remain unavailable. Domain ownership consumes finite retained
rows even when clients disappear; there is no pruning, reset or automatic resend.

## Alternatives Considered

Treating Upload Accepted as Delivered would conflate two external effects and could
report a file that was never posted to its conversation. Reserving metadata in a
second transaction could lose its original upload association after a partial
commit. Reusing the upload token or reconstructing a send from a historical lookup
would grant a repeated possible write. Accepting a caller's digest without comparing
metadata would let filename, caption or captured facts change under one call ID.
Public verified-acknowledgement constructors would imply proof this domain layer
cannot provide; the actual private SDK owner remains responsible for that evidence.
