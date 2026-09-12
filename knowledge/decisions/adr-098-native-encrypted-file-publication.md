---
kind: decision
id: ADR-098
title: "Publish encrypted files from the original accepted upload owner"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
---

## Context

ADR097 separates immutable file metadata, accepted upload and event delivery.
ADR089 retains the original staged ciphertext and private SDK upload acceptance.
Neither a public receipt nor an upload ID authorizes a new room event. The actual
SDK publisher is the next bounded dependency of proposed ADR092; the service and
MCP entry points remain separate work.

## Decision

Synchronous admission consumes the original accepted UploadOperation, its matching
FilePublicationClaim and the unique FilePublicationSend already issued by the
original domain writer. Refusal returns those exact inputs. Admission compares the
original opaque upload claim, stage, route, receipt and capture length. It retains
the original media Job and its existing one-of-two permit in a publication index
before any await. It never copies the ciphertext or creates another media budget.
Collector close refuses unresolved publication custody. A finite owned operation
continues journaling after caller loss; the handle cannot run it twice. Exact
historical settlement may release a completed job, never rearm a possible write.
When final SDK settlement persisted but its acknowledgement was lost, the active
attempt may already be absent. Releasing retained media then requires the actual
private settled File receipt with the same delivery ID and fence, full original
domain content, an already-Delivered domain event ID and exact acceptance replay
using that SDK receipt's digest. This path cannot commit first Delivered. Both
recovery paths update the retained operation outcome and remove only its original
Arc. An unmatched retained job remains unknown and prevents close; it never turns
absence of an active SDK attempt into Idle. A negative state update after proven
historical delivery preserves the original acknowledgement error and custody.

Only the private SDK owner constructs m.file from the retained descriptor and its
exact accepted upload record. It verifies original upload ID/fence/stage/route and
receipt commitment against the unique send, then journals the full content and
correlation. It reuses the existing bounded outgoing encryption and write engine.
Filename, optional caption, fixed application/octet-stream and captured length are
immutable. Caption becomes body with explicit filename; without caption body is
filename. The original thread relation or null-root private room is preserved.
File events are always encrypted; no plaintext URL or key is projected publicly.

Each write uses current-token identity and current room/privacy observations and
validates the original Started capability and publication claim after the last SDK
Possible await. Observed negative fences retain their bounded completion. Unknown
crypto, HTTP or acknowledgement outcomes never repeat encryption or network writes.
Only actual protected Complete event acceptance supplies the domain's historical
receipt correlation. Recovery can commit the first Delivered after restart without
old capability or media, but cannot restore send authority. Upload success alone
never reports Delivered or completes the canonical task.

The protected attempt binds full file content, immutable metadata/captured facts
and the original upload locator. Loading validates those correlations and the
accepted upload ledger again. ADR100 verifies the descriptor against the original
stage receipt using the same frame identity algorithm and requires full original
domain metadata/capture equality before historical settlement. Rebuilt dependent
content hashes cannot substitute another filename, source hash or encryption key.
The existing finite SDK receipt and event bounds
also apply to files. Safe results contain only delivery ID/state/replay status.

## Consequences

The real recipient must decrypt both the event and original file bytes in offline
integration tests. Windows directory-sync refusal remains a negative durability
qualification, not a passing positive send workflow. This change does not expose
send_file, configure a workspace, launch a model or enable production execution.

## Alternatives Considered

Constructing m.file from a public MXC or receipt would discard original custody.
Cloning ciphertext into a second publisher pool would reset the actual memory
budget. A separate crypto implementation would duplicate sensitive identity and
recovery logic. Replaying a possible transaction after restart is excluded even
with a stable Matrix transaction ID because uncertainty is not new authority.
