---
kind: decision
id: ADR-054
title: Retain authenticated Matrix SDK intake across a separate domain handoff
status: Accepted
requirements: [REQ-PALPO-OUTBOUND, REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Authenticated Matrix sync processing crosses separate SDK and domain commits, so raw input and derived evidence must survive lost handoff responses.

## Decision

The bounded M5 collector now has an actual event-intake operation using its pinned
HTTPS reader and owned Matrix SDK. It can admit supported text messages through
the existing verified-input transaction. It does not implement live key lifecycle,
message sends, owner verdicts, task activation or production cutover.

### Cursor and stage custody

The host supplies existing native session IDs, never event JSON, sender claims,
transport generations or verification flags from a browser/runtime. Before a new
poll, each session resolves to a current full ReplyRoute bound to the configured
registration, account/device, Matrix transport, shared room and session generation.
Targets are unique room/thread pairs inside the fixed host room set. Palpo machine
token generations do not appear in this API or replace Matrix incarnations.

The existing encrypted SDK custom-value journal gains backward-readable intake
fields. Its lifecycle is Prepared -> Applying -> Derived -> individually
acknowledged -> completed receipt. The complete authenticated JSON response and
original targets are persisted before Applying is persisted and BaseClient receives
the sync. Derived messages include private SDK provenance and frozen routes; they
are persisted before any domain admission. The journal is transport custody, not a
second canonical message/task store. Only the domain database owns canonical input.

Matrix SDK 0.18.0 returns an empty response when given its current next_batch again.
SDK state and our journal are not a shared transaction. An interrupted Applying
stage therefore stays explicit OutcomeUnknown with raw response and original
targets. Restart may inspect it but may not replay it as an empty successful batch,
advance its intake cursor, discard it or infer events that were never journaled.
Prepared and quarantined states likewise require explicit future host recovery;
this slice supplies no automatic discard or key-reset operation.

The journal cursor advances only after every derived message has its exact durable
domain receipt. Completed batch receipts preserve token, content digest, frozen
scope digest and acknowledgements. Repeated content is not reinterpreted through a
new target plan. Even an unchanged token inherited from observation-only bootstrap
persistently transfers cursor ownership to intake. Subsequent collect calls may
refresh whoami/full room state but cannot run another observation-only sync.

### Provenance and admission

HTTP uses ADR-047's bounded authenticated connection, strict single JSON document,
duplicate-key/depth validation, host-pinned origins, no redirects/proxies and finite
header/body/SDK deadlines. Intake requests timeout=0 with timeline limit100 and
explicit rooms. State comes from authenticated full room snapshots; lazy SDK room
state and host privacy intent do not stand in for membership evidence.

Only owned BaseClient SyncResponse objects derive event data. Encrypted messages
require SDK Decrypted with VerificationState::Verified, matching authenticated
sender, a present sender device, Megolm session and no forwarder. Unknown keys,
unverified identity, mismatched sender, unsupported algorithm, media or incomplete
limited timeline remain explicit custody. Plain JSON trust flags do nothing;
plaintext in an encrypted target is refused. Successful offline tests perform real
Olm/Megolm sharing and SDK signature verification between two devices, including a
human DM without an @ mention. Their synthetic key exchange and explicit trust
setup are test-only and do not constitute a production publication/verification API.

Supported m.text/m.notice/m.emote carry exact full MXID mentions and actual thread
relations into the existing native rules. A body containing @worker alone does not
manufacture a mention. Thread messages stay in an exactly frozen thread; unsupported
relation/edit types are quarantined. Unselected timelines are not projected into
an arbitrary session. Initial observer bootstrap and historical/pre-session message
backfill are outside this intake contract. Native admission independently enforces
current sender membership, source timestamp, session, registration, room and
transport scope in the same transaction that projects input.

### Lost results, cancellation and negative observations

Derived handoff survives Busy, capacity, cancellation and unknown domain responses.
Such custody errors are not fresh proof of a failed Matrix device. An exact
historical receipt lookup can settle only a previously committed message under its
archived original session/scope/content; it neither reactivates authority nor creates
new input. If no receipt exists, admission must pass current scope checks. Retired
or conflicting unadmitted work remains quarantined and cannot be retargeted.

An accepted collection is a finite owned task. Dropping its caller does not drop
received work. Cancellation before a domain mutation keeps the derived batch;
a known successful domain response is acknowledged even if cancellation arrives
immediately afterward. Tests interleave a real canonical commit, cancellation,
negative shared-room evidence and lost response, then recover only its old receipt.
Authenticated room/device failures retain the existing exact negative fencing.
An old inspector cannot invalidate an unrelated newer device incarnation.

Restore authenticates the encrypted journal, checks SDK identity, raw-content digest,
token, frozen account/device targets, event/receipt bounds and the actual SDK cursor.
Applying may have either the previous or attempted cursor; Derived requires the
attempted cursor. A mismatch fails visibly without deleting files or regenerating
keys. Public status reports phase, digest and bounded counts, not raw private
messages, room IDs, credentials or a deserializable proof constructor.

### Hard limits and remaining gates

One collector operation and the existing one-slot SDK queue retain ownership.
Each HTTP JSON body is at most1MiB, timeline at most100 events, target set at most64
unique room/thread sessions inside at most16 pinned rooms. Frozen derived content
is capped at1MiB. The encrypted serialized journal is capped at16MiB; existing
64MiB-per-SDK-file bounds remain. At most64 combined observation/intake sync receipts
are retained. Capacity rejects new work; it never evicts dedup history or drops a
pending batch. ACK and final receipt writes have actual SQLite rollback tests.
This finite receipt ceiling intentionally prevents indefinite deployment until a
separately designed safe retention/compaction lifecycle exists.

Still unimplemented: live key upload/query/sharing/verification and account-device
provisioning, automatic Applying reconstruction, quarantine disposition, backlog
paging/history gaps, attachment intake, dynamic room-set replacement, runtime
activation/dispatch, notices/final sends and live deployment. Offline encryption
and authenticated fixture delivery are not evidence that those gates are complete.
No new domain schema or independent domain store is introduced.

## Consequences

The collector advances its cursor only through the documented custody stages and exact receipts. Finite journal capacity is an explicit stop, not continuous-deployment retention parity.

## Alternatives Considered

Acknowledging a cursor before durable domain handoff or discarding pending batches would lose input. Accepting browser event JSON as verified SDK provenance would bypass the authenticated collector.
