---
kind: decision
id: ADR-078
title: "Retain exact encrypted upload preparation and nonrearmable possible writes"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

File retries need an original domain operation recorded before staging, while possible upload writes must remain nonrearmable across owner loss.

## Decision

ADR027 requires the original file snapshot to survive retries without silently
reading a changed file. ADR066/077 retain encrypted media but do not prove an
upload was unsent. ADR072's in-memory POST attempt cannot survive owner loss.
Schema019 adds a host-only registry under the existing protected DomainRepository
and bounded DomainStore writer. It performs no filesystem or network operation.

### Original admission and storage commitment

reserve_upload takes the exact RunnerCapability and UploadRequest (bounded call
ID, original request digest and safe filename/MIME/declared-size metadata). The
writer verifies a real Started dispatch, canonical task not Done, exact task
execution epoch, active engagement/resource/registration, exactly one exclusive
workspace lease, and current encrypted Matrix route. It freezes the owned scope
fingerprint, full ReplyRoute and capability digest; no path is accepted. Legacy,
internal and missing-private-generation sessions fail the current-route gate.

Only the first committed reservation returns an opaque UploadPreparation. Exact
replay returns the same safe receipt and UploadIdentity with no preparation.
A lost initial reply, cancellation, capacity exhaustion or restart cannot grant a
second file capture. The preparation has no Clone/Debug/Serialize/Deserialize or
raw constructor. DomainStore takes an Arc of the original preparation during
binding so queue failure does not force the caller to relinquish its retained
copy. This proves registry issuance, not that a future adapter uses it only once.
The future file owner must consume one actual snapshot/encryption path under it.

The immutable StageCommitment binds namespace digest, original operation string,
original frame receipt digest and actual encrypted length before storage IO.
Distinct operations cannot bind the same namespace/operation. ADR079's actual
PreparedEncrypted must supply these fields; inventing strings proves no disk
custody. A changed commitment conflicts even on exact preparation replay. The
commitment is serializable/deserializable DATA because the protected DB restores
it. That precise exception never applies to preparation, claim, send or typed
storage observations; no upload type is wired into RunnerCommand or HTTP.

observe_upload_staged accepts host-only FileAndDirectorySynced or OutcomeUnknown
observations for the exact historical identity/commitment. Unknown can later be
resolved by actual qualified staging recovery. Once qualified sync is recorded,
a missing later response never erases that positive historical fact. Claimed,
possible and accepted states retain it. Windows directory-unconfirmed staging
cannot supply this positive variant. This store cannot independently verify a
filesystem, receipt or hardware durability claim; its caller is the trusted
future adapter and must preserve actual retained handles and evidence.

### Upload execution and historical settlement

claim_upload is restricted to the exact current original capability, qualified
staging and uncancelled row. Its private secret and increasing fence expire after
1..60000ms. Only an unstarted expired claim may be reclaimed; the old token is
refused. begin_upload repeats current authority and exact token checks inside
BEGIN IMMEDIATE, durably sets WritePossible/outcome_unknown, commits, and only
then returns UploadSend with the frozen route and stage. All DomainStore current
operations sample wall-clock time after the writer queue and SQLite lock wait.
validate_upload_send repeats these checks immediately before a future POST.

A second begin is refused even with the original claim. A lost begin response
cannot recreate UploadSend. Claim expiry, cancellation, owner/task/registration
or room retirement and restart NEVER reset WritePossible. A cancelled or expired
claim cannot validate a send. The real network owner must consume UploadSend
once and mark HTTP possibility before polling; holding or cloning UploadClaim
must not become a way to reconstruct a send after a lost begin response. This
slice deliberately provides no restored-upload transport constructor.

Cancellation is sticky and orthogonal to external outcome. Pre-upload rows can
remain pending/claimed in storage but are permanently unclaimable. Possible
uploads remain possible/unknown; accepted uploads remain accepted and cancelled.
mark_upload_uncertain retains the original fence and clears the executable token.
No NotSent, retry, reset, replacement-ID or expiry-based resend API exists.

record_upload_acceptance is HISTORICAL: exact UploadIdentity, upload fence and
StageCommitment plus bounded private receipt ID/digest must match a recorded
WritePossible. It can settle after cancellation, lease expiry, task completion,
revoke or restart. An old/reclaimed fence, changed stage, conflicting receipt or
claim that never began is refused. A same-receipt replay acknowledges the fact
only. Acceptance clears unknown/token state without restoring current authority
and says nothing about delivery of a Matrix file event. The actual MXC, encrypted
file descriptor and response bytes belong in a future private transport journal,
committed before the domain acceptance call; no URI or key is stored here.

restore_upload requires the exact original capability and request, including
content digest. inspect_upload returns only the safe receipt. The host may inspect
that exact identity's stage commitment/fence; there is no latest-operation scan,
preparation recovery or executable token retrieval. Missing evidence is not proof
of no capture, no staging or no upload. Positive historical inspection never
revives the original route, task epoch, dispatch or lease.

### Bounds, tests and remaining integration gates

The registry retains at most4096 rows globally and16 per dispatch, permanently
including cancelled/unknown rows. Each stage is at most16MiB; names and identities
have fixed bounds. Existing exact replay remains readable at capacity. No pruning
or replacement admission is provided. This is a finite first slice rather than
an unbounded-lifetime retention policy. The two hard counts are evaluated before
insertion inside the same transaction. Public receipts exclude all room, owner,
workspace, preparation secret, stage/receipt digest and request metadata fields.

Tests use actual private SQLite databases, real state transitions, concurrent
DomainStore calls, deliberately discarded successful worker results, queued lease
expiry and actual SQL failure triggers. Migration18->19 creates no historical
preparation or upload authority; older migration fixture expected versions are
updated without loosening their assertions. Global-capacity fixtures use synthetic
retained historical rows in one transaction solely to exercise the real counter;
all scope/fence/lifecycle assertions use real registered dispatches. Epoch, owner
and workspace removal cases deliberately mutate fixture SQL to test negative
persisted evidence; room, transport, registration and engagement changes use the
actual host domain observation/revocation APIs. They do not claim that SQL fixture
mutation is an authenticated event adapter.

Source capture, physical workspace/ancestor custody, stage worker cancellation,
qualified Windows directory persistence, durable private POST response storage,
upload SDK integration, file-event publication and safe runtime/MCP output remain
separate gates. Actual request metadata must remain available to the future owner
under the original content commitment; this registry stores its digest, not a
model-readable request/path. Taskless/front-desk, plaintext and post-Done upload
execution are unsupported here. No service flag, endpoint, model or live room is
enabled and no M6/file-delivery parity or production-readiness claim is made.

## Consequences

The existing domain writer owns exact request and stage commitments plus historical upload states. Filesystem capture, private HTTP acceptance, event publication and runtime tools remain separate gates.

## Alternatives Considered

Reading a changed source on retry or treating restored ciphertext as proof that POST never happened would discard original custody. Storing a digest cannot replace the actual metadata needed by a future file-event owner.
