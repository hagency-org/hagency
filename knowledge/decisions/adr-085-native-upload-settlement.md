---
kind: decision
id: ADR-085
title: "Restore exact historical upload settlement without execution grants"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Full process loss removes original runner secrets, yet an authenticated private upload acceptance must still settle its exact historical domain row.

## Decision

ADR078's restore_upload requires the original RunnerCapability and UploadRequest.
The capability contains a random secret; runner_dispatches and runner_attempts
retain only hashes. Original file_uploads rows retain request/capability digests,
not the secret or full request body. Existing reopen fixtures preserve the old
capability in memory. They cannot demonstrate full process loss recovery using
only ADR084's private persisted upload reference and acceptance.

Add a host-only historical association API using the existing protected domain
row. No schema, new secret, current capability or storage protocol is required.
restore_upload_settlement takes bounded upload ID, upload fence, StageCommitment
and complete frozen ReplyRoute. These are correlation data, not proof of SDK
acceptance. It reads exactly that row, refuses mismatches and requires qualified
staging plus WritePossible or Accepted. It takes the original request/capability
digests from the row itself. Missing returns None, never a claim that nothing was
uploaded. Pending/claimed rows and unknown staging cannot be restored this way.

The result is UploadSettlement with private fields. It has no raw constructor,
Clone, Debug, Serialize, Deserialize or UploadIdentity conversion/getter. The
only public accessor is its bounded historical id label. Neither capture, stage,
claim, begin nor current validation accepts this type. It cannot reconstruct
UploadPreparation, UploadClaim, UploadSend or RunnerCapability. Restoration
performs no write, issues no execution grant and never changes upload state.

inspect_upload_settlement and record_upload_settlement re-read the protected row,
matching original identity, exact fence, stage and full frozen route. Recording
uses the existing historical acceptance transaction: exact receipt replay is an
acknowledgement, a changed receipt conflicts, and failed commit rolls back.
Cancellation remains sticky. Acceptance may be recorded after revocation, lease
expiry, task completion, privacy promotion or process exit without reviving the
old route or current execution. Current authority is not required to record what
already happened, and remains required by every actual execution API.

The bounded DomainStore wrapper validates fixed locator fields before queue
weight encoding. Its inspect/record methods accept Arc<UploadSettlement> so the
host can retain the original immutable handle if an async result is lost, without
adding Clone or execution authority to the sealed type. Recording samples writer
time inside BEGIN IMMEDIATE after queue/lock wait. Each call still rechecks its
original protected row. Existing queue, retained row and clock bounds are unchanged.
No new retained map, latest-record lookup, pruning or replacement ID is introduced.
Lookup uses the existing SessionBinding parser for thread roots: Ruma permits
opaque no-colon EventIds longer than 255 bytes. Historical lookup must not
reinterpret those already issued routes through the stricter event-admission
parser. A real 601-byte-root route is covered, with the existing 64 KiB command
ceiling retained before cloning unusually large roots. Direct owner/server,
other identity and generation checks match existing route admission.

This domain API cannot authenticate an SDK journal or HTTP response. The private
SDK owner must open its actual existing store, verify key/identity/config binding,
marker and full record, then return its sealed exact historical reference and
accepted receipt. A future private coordinator copies only that sealed reference's
locator fields into domain restoration and records its opaque acceptance. Raw
model JSON or an accepted boolean cannot substitute for the SDK observation.
SDK exact-reference lookup is separate ADR084 work. Automatic startup selector
inventory, old host config/key rediscovery and cross-store process coordination
remain separate; this change does not pretend to solve those provisioning gates.

Tests use actual domain databases, registered dispatches, qualified host fixture
staging observations, lifecycle transitions and a real SQLite failure trigger.
An explicit negative SQL mutation changes the frozen stored route after handle
creation to prove rechecking; it is not presented as authenticated Matrix evidence.
The receipt ID/digest supplied by domain fixtures is synthetic host data, not an
actual SDK response. These tests prove domain association and atomicity only.

The process fixture launches three separate instances of the native test binary:
writer, settler and exact replay. The writer persists only upload ID/fence/stage/
route and opaque acceptance data, closes and moves its private test store to the
host fixture root, then exits. The next processes receive no original capability,
request, secret, UploadIdentity or sealed handle through file, environment or
parent variables. They open the existing database and settle via the bounded
writer. The parent has only fixture location and phase selectors. Its retained
child handle bounds waiting and kills/reaps only that child if the fixture fails;
no foreign process or service is inspected. Windows SystemRoot and temporary-dir
environment remain available to the fixture, without forwarding runner secrets.

Native subprocess and repository tests, strict scoped lifecycle and Windows GNU
compilation are recorded separately. Cross-compilation is not actual hosted
Windows runtime qualification. No Matrix network, SDK persistence, upload POST,
file-event sending, runtime endpoint, service activation or production-readiness
claim is added by this slice.

## Consequences

The host-only restoration API uses bounded persisted association to settle history without reconstructing execution capability. Fresh-child fixtures remain separate from HTTP, SDK and positive Windows upload qualification.

## Alternatives Considered

Reconstructing a runner secret or treating historical acceptance as a current send grant would revive execution authority. Requiring live in-memory capability for all settlement would prevent the documented owner-loss recovery.
