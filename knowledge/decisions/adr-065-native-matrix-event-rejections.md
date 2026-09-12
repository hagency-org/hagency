---
kind: decision
id: ADR-065
title: "Persist terminal Matrix event refusal without retiring healthy transport"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
---

## Context

A conclusively ineligible event must not retire otherwise healthy Matrix transport or block later eligible messages in the same authenticated batch.

## Decision

### Problem and decision

ADR062 demonstrated that a plaintext message in an encrypted private room caused
the entire Agent transport to become unavailable. `Batch::derive` treated that
conclusive event refusal as Unsupported for the complete sync; the SDK owner
quarantined the batch and the intake staging error retired transport authority.
Unsupported attachments, malformed content and crypto ineligible events could
similarly prevent later eligible messages from being processed.

Keep the existing authenticated HTTP, SDK ownership and domain admission paths.
After a known successful SDK sync, derive a complete private source/disposition
ledger alongside the eligible message candidates. Every conclusive rejected or
non-target source receives a terminal tombstone. Persist that ledger in Derived
before any candidate is admitted. Then continue eligible events and advance the
cursor only through the existing exact domain acknowledgements and final receipt.

This change does not broadly catch Unsupported, RunnerAuthority or storage errors
and call them harmless. It changes the known per-event classification inside a
successfully observed SDK response. No production approval path, schema, service
availability, credentials or live deployment is added.

### Raw coverage and SDK provenance

The bounded authenticated raw response is still saved before Applying and SDK
mutation. Raw joined timelines are indexed by room and order, with at most 100
entries. Every entry must have exactly one actual returned SDK timeline event.
Plaintext and UnableToDecrypt events must retain the complete original raw digest;
decrypted events must retain original event ID, sender, timestamp and exact room.
Duplicate valid room/event IDs, missing/extra SDK entries, changed source identity
or limited timelines remain batch errors with retained raw custody. No event is
silently omitted because parsing or decryption failed.

Pinned `matrix-sdk-base 0.18.0` source
`src/response_processors/timeline.rs:58–154` constructs a TimelineEvent for each
raw entry and pushes it even after a deserialize warning. Thus malformed content
is normally still available for an explicit disposition. Its client sync path
`src/client.rs:582–598` returns empty output for an already-consumed token; this
does not recover an interrupted Applying stage. SDK-common 0.18
`deserialized_responses.rs:554–615` caches a timestamp separately; `raw()` retains
the original event. No clamped display timestamp supplies source authority.

On restore, each candidate index is bound back to the original raw room, event ID,
sender and timestamp; plaintext candidates also match their complete original
parsed input. Prepared/Applying cannot carry derived candidates, dispositions or
acknowledgements. The opaque SDK digest is retained rather than reconstructed as
new verification evidence.

Each disposition also contains a private digest of the actual SDK TimelineEventKind
including its decryption observation. Candidate admission retains the pre-existing
Verified/CrossSigned sender/device/session/no-forwarder requirements. UnableToDecrypt
provides no decrypted body or crypto success proof. JSON `encryption_info` flags
remain ordinary untrusted content. Verification-event auto-handling stays disabled.

### Terminal dispositions and replay

Decisions are Candidate with an exact derived-event index, Rejected with a fixed
reason, or NotTarget. Conclusive rejections include malformed supported content,
unsupported message/relation types, missing/unverified/mismatched crypto proof,
and plaintext in an encrypted target. Non-target traffic stays outside every
model session; rejected content never becomes a canonical message or task.

The source record contains hashes of the room, optional valid Matrix event ID,
complete raw event and immutable raw event. Only top-level unsigned metadata is
excluded from the immutable digest because age and bundled presentation can
change on retransmission. The complete raw digest still establishes this batch's
exact coverage. A malformed event with no valid ID uses its exact raw fingerprint.
Neither a synthetic ID nor a model/HTTP-provided proof is created.

Completed intake receipts retain these dispositions under the original frozen
target digest and SDK identity. On a later token, an exact terminal source remains
terminal even if the target plan changes or keys/trust become available. Changed
immutable content under the same valid event ID gets a terminal SourceConflict;
it cannot replace the first outcome or reach admission. Existing admitted-source
replay still uses the exact canonical domain receipts and current route rules.

Missing-key and unverified refusals are intentionally terminal in this bounded
mode. A human must send a new event, or a separately designed future explicit
manual recovery must establish fresh authority. This implementation does not
claim automatic later decryption/replay or reinterpret old ciphertext after a
trust update. Actual SDK fixtures separately prove that new verified messages
continue while the earlier event remains rejected.

### Failure, privacy, bounds and old state

Whoami/device mismatch, unsafe full-room observations and existing generation
failures still fence authority. Interrupted SDK calls, failed Derived persistence,
coverage ambiguity and limited timelines retain their original unknown/quarantine
state and conservative transport fencing. Current domain handoff authority/content
conflicts remain quarantined under their original scope; this slice does not turn
stale timestamps or changing membership into retryable event decisions. A later
typed writer refusal needs its own lost-response/replay contract.

One protected ledger has at most 100 entries; at most 64 completed sync receipts
remain, with the existing 1 MiB derived projection and 16 MiB encrypted journal bounds.
No capacity branch evicts tombstones, resets SDK keys or drops unresolved custody.
The SDK journal is transport custody, not another canonical message/task store.
Public intake summaries/status add only a rejected count. Bodies, room/sender/
device IDs, keys, proof details and claim secrets are not projected or logged by
this code. Receipt tombstones retain hashes and static reasons, not message bodies.

Old receipts without dispositions remain readable. If an old intake receipt has
filtered events, its source history cannot be reconstructed from a count. It may
be inspected, but new intake refuses rather than silently promising tombstone
coverage or deleting state. Old all-admitted receipts rely on existing canonical
receipts; observation-only bootstrap history remains outside event intake as in
ADR054. A legacy pending Derived batch may finish only its original frozen handoff;
Applying/Quarantined is never rederived automatically. This is not an automatic
import/compaction feature.

### Qualification

Fixtures use the real local TLS collector and owned SDK. They cover malformed/
unsupported/non-target plus eligible messages, disposition-before-admission,
actual encrypted private bad-to-good continuation, original ciphertext after
key/trust changes, cross-token/plan restart replay, real SQLite receipt rollback,
SDK interruption, missing coverage, identity fencing and altered/legacy journals.
ADR062's plaintext-private expectation changes only after these continuation
fixtures pass. It still does not claim full encrypted DM plus owned native helper
completion. Live crypto enrollment, automatic recovery, journal compaction,
domain rejection redesign and overall native cutover remain open.

Local macOS verification passed all 58 Matrix tests, the nine new rejection
fixtures, and three amended ADR062 owned-workflow tests. Native and Windows GNU
cross-target Clippy pass with warnings denied. These cross-target checks do not
claim actual Windows execution; integrated CI must provide that evidence.

## Consequences

Durable terminal dispositions allow continuation without reopening previously refused sources. SDK uncertainty and missing source coverage remain quarantined rather than being treated as terminal success.

## Alternatives Considered

Retiring the whole transport for a plaintext event in an encrypted room blocks unrelated valid input. Reinterpreting that same ciphertext after key or trust changes would erase its original refusal receipt.
