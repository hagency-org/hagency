---
kind: decision
id: ADR-084
title: "Retain exact bounded upload acceptance under the SDK owner"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY]
---

## Context

The private SDK owner must preserve exact upload acceptance and its original route, stage and domain fence without issuing replacement live permits after reopen.

## Decision

ADR078's UploadSend is consumed once by a private SDK journal reservation. The
reference binds its original domain id/fence, complete StageCommitment and frozen
ReplyRoute to the actual persisted SDK Ed25519/Curve25519 identity and the existing
configured HTTPS-origin/account/device binding. Only the first durable reservation
returns a LivePermit; the permit also retains the exact ephemeral Owner generation.
Private Possible consumes it before persistence. Restart and historical inspection
never issue permits, and a possible summary is no HTTP grant. Exact-id lookup after
validated SDK reopen can restore only a historical Reference, exposing bounded
borrowed original id/fence/stage/route to a future trusted coordinator. The locator
is untrusted lookup data, not acceptance proof. There is no latest/inventory lookup.
Domain UploadIdentity retains additional private request/capability digests; this
journal cannot reconstruct it. ADR085 domain settlement restoration is a separate
integration boundary; this slice claims no automatic process-restart settlement.

The existing SDK thread, bounded command queue, lock, StoreCipher and SQLite state
store own a separate finite custom key, hagency.observer.uploads.v1. A versioned
identity marker in the main sync journal distinguishes never-enabled absence from
lost history. Initialization writes the marker and custom key separately; any
interrupted one-sided bootstrap is refused. Missing keys and malformed encrypted
records cannot regenerate an empty journal or SDK identity.

All64 records are permanent, including reserved, possible and accepted entries.
Reservation bounds the full serialized identity and reserves worst-case4096-byte
response representation, checked MXC and receipt capacity within a32KiB record.
The separate aggregate bound includes64 full records and bounded map/header
overhead. StoreCipher0.18 serializes ciphertext as decimal byte arrays; its envelope
bound allows four bytes per plaintext byte plus the16-byte AEAD tag,24-byte nonce
and bounded JSON structure. Logical reserved space cannot promise free disk or
physical durability and cannot be spent by concurrent main-journal intake growth.

Accept borrows only ADR083's sealed actual UploadResponse. It first reserves the
SDK queue and a process-wide finite64-slot memory permit, then makes one bounded
private owned copy. This shared64-slot limit covers queued/uncommitted copies;
successful persistence releases the permit while committed/restored response bytes
remain separately bounded by each SDK ledger's64 records. The owner persists
original body bytes, checked MXC, SHA256 and
a deterministic private receipt digest binding the record, HTTP200 and original
body. Exact duplicates acknowledge; changed bodies or references conflict. Reply
receiver loss does not cancel the queued owner operation. Inspection returns only
phase and receipt commitments, with no body, MXC or arbitrary map export.

Failed persistence poisons this owner's upload execution and retains copied bytes
and their memory permit in its in-memory journal until explicit owner close.
Validated reopen recovers only durably committed acceptance; absent acceptance
stays unknown forever without a resend grant. Explicit close and process death
can lose uncommitted bytes. No process-global response map or automatic recovery
write is added. A future long-lived HTTP coordinator must retain the original
UploadAttempt/response across SDK reopen. The shared memory semaphore persists
across owners, while returned inspection summaries carry no raw response allocation.

These are private unwired foundations. A sealed response proves validated actual
HTTP response bytes, not association with this domain operation. The future
coordinator must enforce that association, retain actual prepared/restored staging,
release SDK ownership before network polling, revalidate current domain authority
after the last await and before POST, and record private acceptance before domain
acknowledgement. Historical acceptance may settle after revocation/cancellation;
it cannot revive a task, send permit or room-event authority. SDK file checks and
SQLite acknowledgements are not ADR066 retained-directory qualification, Windows
metadata-flush proof, hardware power-loss proof or complete upload delivery.

## Consequences

One durable reservation supplies one ephemeral permit; historical inspection supplies settlement evidence only. A coordinator still must associate the actual response and revalidate current authority before HTTP.

The capacity regression fixture deliberately holds all64 process-wide response
permits. It runs through the exact original test selector in a separate process,
with a finite parent deadline and mandatory successful child verdict. Its former
module-local serial lock did not cover concurrent staged-upload and file-publication
tests, so their otherwise valid acceptance could receive Capacity during this
injection. Production limits and refusal behavior remain unchanged.

## Alternatives Considered

Turning a Possible summary or restored reference into a new POST grant would repeat unknown effects. Treating any valid response as belonging to this upload would omit the original operation association.
