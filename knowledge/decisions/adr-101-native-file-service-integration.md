---
kind: decision
id: ADR-101
title: "Connect one development FileService to the original Started workspace and Matrix owner"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
tags: [rust, files, bootstrap, mcp, custody]
---

## Context

ADR027 requires current-conversation send_file and get_file_delivery. Proposed
ADR092 separated source capture, durable admission and room delivery. ADR093 now
provides an original-writer-bound StartedWorkspace. ADR096 starts one real owned
attempt through Bootstrap, current Collector authentication and required workspace
registration. ADR097 retains file metadata, original upload preparation and a
separate event publication state. ADR100 and the reviewed ADR098 publisher bind
that state to the actual original encrypted staging receipt and protected SDK
acceptance. They do not yet form an application file tool.

This executable implementation contract is accepted after root and independent
review; it is not implemented feature coverage. It depends on integrated ADR096/097/098/100. Its tests are future exact
selectors and must not be registered as passing migration coverage before they
exist and run. No current production capability is enabled by this document.

Current ownership matters: driver.rs creates its Collector and WorkspaceAccess
inside its own thread and closes Collector when the execution Report can close.
The App cannot reuse those owners yet. Merely creating another Collector or
installing a fixture-supplied capability/root would bypass the real bootstrap.
Current runner authentication also uses RunnerCommand::Check, whereas ADR097's
inspect_file_delivery intentionally supports exact historical capability
association. Those two uses must remain separate.

The existing host-exclusive private workspace and stable ancestor contract stays
in force. File snapshots use the actual retained root, not an ambient path. Codex
still receives its configured path and normalizes descriptor aliases; this slice
does not claim hostile same-UID replacement isolation or full runtime sandbox
qualification. Existing operation deadlines and platform cleanup limits remain.

## Decision

Implement one same-process, explicitly enabled development FileService inside the
existing hagency crate. Extend only the fixed private development-driver.json with
an optional send_file boolean, default false. The existing file_limit must be
1..4194304 bytes and is applied before capture, codec and staging allocation.
There is no second configuration file, credential, executable selector, SDK
account, CLI flag or model-writable authority setting. --development-driver still
means one attempt per service start. receive_file, previews and plaintext rooms
remain outside this slice.

Bootstrap creates one Shared context containing the original DomainStore clone,
one Collector and one WorkspaceAccess. Driver, FileService and App receive only
clones of these same owners or narrow handles. Neither App nor the MCP process
creates a Collector. Driver no longer closes the shared Collector independently;
Bootstrap orders closure after both execution and file custody are resolved.
WorkspaceAccess remains crate-private. It gains a sealed guard for one exact
original capability, with validate_current and bounded snapshot methods, never a
raw Workspace/root/capability accessor. Registration retains the ADR096 sequence:
real refresh, compatible claim, actual Started ACK, sealed workspace registration,
local launch ACK received, fresh domain/root/deadline check, then spawn. When files
are enabled, the fixed file worker must acknowledge initialization before that
launch ACK can be sent. The existing unconfigured mode remains unchanged.

### Application and MCP surface

Add closed runner operations under /api/native/v1/runner/file-deliveries:
POST with call_id, path, optional filename and optional caption; GET with an exact
delivery ID. The native MCP tools send_file and get_file_delivery use the existing
host-created task-client Context and bounded loopback transport. The host opts
TaskMcp into exactly these two additional enabled_tools only when the same private
profile enables files. It supplies a fixed inherited HAGENCY_FILE_TOOLS=1
presentation flag through the fixed helper env-vars allowlist; the actual helper
catalog and dispatch both honor that mode. Default tools/settings stay unchanged.
Host environment validation reserves this name even when disabled. This bounded
presentation flag is not execution authority: forging it cannot bypass App
configuration, current capability or historical row checks. No request accepts
room, URL, owner, descriptor, key, MIME, root, verification, capability or SDK fields.
A relative path is UTF-8 selection data only: RelativeFile's existing 4096-byte,
32-component rules apply; call_id is an identifier at most 128 bytes, filename
at most 255 bytes and caption at most 1000 bytes. Default filename is the last
validated component; MIME is application/octet-stream. JSON request bytes are at
most 16 KiB, duplicate/unknown fields are refused, and MCP frame/request-ID limits
and the existing five-second client deadline are unchanged.

POST uses existing current runner authentication plus the exact WorkspaceAccess
binding and domain reserve validation. GET checks local-origin/header rules and
ADR097's exact original capability-to-row equality in a separate read-only path.
GET does not call a current-operation Check or grant access to any other runner
operation. An expired/revoked original capability can inspect only its own exact
historical file receipt, never read a source or claim/send work. A foreign ID or
capability is refused identically to absence. The MCP helper does not restore a
capability from a delivery ID; it uses its already-provisioned original context.

Validation reuses RelativeFile, core project::identifier and FileDeliveryRequest::validate
rather than defining incompatible character or byte rules. Default filename is
resolved once before the canonical request digest.

The safe result contains delivery_id, filename, status, replayed and an optional
fixed error code. It omits paths, captions, hashes, room/thread IDs, MXC, descriptors,
keys and underlying errors. queued means a durable ADR097 reservation AND an owned
local pipeline capable of continuing. delivered requires the domain's acknowledged
Matrix event. failed requires known terminal refusal; outcome_unknown preserves
possible external effects or lost admission/pipeline custody. A persisted Pending
row without its original local pipeline must not be reported as actively queued.
Uploading, execution activity and delivery never change canonical Done.

### Finite admission and original ownership

A single fixed OS thread, hagency-file-service, owns one current-thread Tokio
runtime/LocalSet, the media Store and Codec. It retains at most two job slots in
all states combined: admitting, queued, active, and unresolved. Its closed command
channel is also two entries; a request cannot spawn a worker. At most two retained
job futures run on that thread, allowing the second durable admission to complete
while the first waits on HTTP. A single pipeline gate serializes capture, crypto,
staging, upload and publication. No source/codec/staging syscall runs on an HTTP
handler or a runtime's generic spawn_blocking pool. No Store borrow or global map
lock survives an await. Existing Collector-owned finite SDK/publication tasks
remain its implementation detail, not new unbounded file workers.

Synchronous admission validates request/capability sizes and reserves a slot plus
its exact dispatch/fence/runner/secret and versioned request-digest association
before enqueue. The original job lives in a bounded owner registry independent
of any HTTP receiver. Losing the caller neither cancels it nor returns its slot.
Exact duplicate calls attach to the existing outcome without reading again; changed
selection/metadata conflicts. Keep at most one admission waiter per job; additional
identical callers receive the bounded known status or Unknown, not new subscribers.
Known-completed jobs may release their slot only after all original custody is safe
to drop; later identical calls use original domain restore/inspect and never need
an in-memory result cache or new capture. A known terminal failure whose domain
recording is unconfirmed remains retained Unknown. Refused enqueue returns the known busy result without
DB work. A queued command that commits admission retains the first UploadPreparation
before replying. No queued result is returned before the ADR097 transaction ACK.
A lost writer ACK stays unknown; exact restore/inspect may acknowledge its persisted
identity, but cannot manufacture a replacement preparation or restart capture.
The two slots include such unknown admissions until explicitly resolved; process
restart is not a means of bypassing durable capacities or replay rules.

The owner can make progress on admission while a network future is pending. It
cannot promise hard real-time response during a blocked filesystem syscall or
scheduler starvation. Caller timeout preserves Unknown and original ownership;
no deadline is widened to conceal this distinction. Requests that lose their reply
use the same call_id, never a fresh key chosen automatically by the helper.

The media Store is one fixed private state child, file-media, with one retained
journal. Fresh creation uses create_new only after successful fresh directory
creation; an existing directory with a missing journal is unavailable, not silently
initialized. Its namespace is a bounded host-derived storage partition digest of
the fixed account/registration profile, not execution authority. OperationId is
the original upload ID. Limits are item=file_limit, journal=64 MiB, 64 records and
two held results; Codec also permits two results. Existing domain limits of 4096
lifetime deliveries and 16 per dispatch remain. There is no pruning, truncation,
identity replacement or capacity reset by reopening. Physical allocator overhead
is not claimed to equal logical byte limits.

### Exact pipeline and authority checks

1. The owner revalidates the original sealed workspace after its queue wait, then
   calls reserve_file_delivery on the original writer. It returns queued only
   after retaining the first admission result. Replay without preparation is
   inspection only and never enters source capture.
2. After the pipeline gate, revalidate the same binding/writer and the service
   retirement flag. Snapshot only through the retained root using file_limit.
   Record actual length/hash, retain Snapshot, and revalidate after copying.
   File replacement, hard links, traversal, source growth or expiry refuses.
3. Encrypt the actual Snapshot once. The resulting Encrypted retains its source
   custody. prepare_encrypted consumes it and computes original namespace,
   operation and stable receipt digest before journal I/O. Retain PreparedEncrypted
   while bind_file_delivery_stage commits captured facts and StageCommitment.
   Only then call stage_prepared and record its actual qualified/unknown storage
   observation. StageFailure's returned input or Store-held pending custody stays
   with this owner. No fallback re-encryption or invented successful storage state.
4. Restore encrypted custody only from the exact original operation/digest and
   qualified FileAndDirectorySynced journal. File-synced/directory-unconfirmed,
   partial tails, absent records, corruption or mismatches cannot authorize upload.
   A known crypto refusal before producing encrypted bytes is terminal and does
   not cause recapture; abrupt process/worker failure is not successful cleanup.
5. Claim/begin upload through the original capability and consume the sole Send
   with matching claim and RestoredEncrypted via StagedUpload. Retain UploadOperation
   before awaiting its one run. ADR089 performs current whoami, protected SDK
   Possible persistence, final current validation and the actual encrypted POST.
   Possible/unknown outcomes never rearm. Only actual protected acceptance permits
   historical domain acknowledgement.
6. Obtain the separate current FilePublicationClaim/Send. Consume the accepted
   original UploadOperation with those exact objects using prepare_file_publication.
   Retain FilePublicationOperation before awaiting run. ADR098 constructs m.file
   from original metadata and private accepted upload evidence, verifies current
   account/room/privacy/complete trusted recipients, and validates current domain
   scope after the final SDK await before encrypted PUT. It commits private SDK
   acceptance before historical domain Delivered. A null-root DM stays null-root;
   promotion never retargets its output to a group.

Caller disappearance affects none of the ownership above. Service cancellation,
operation retirement, task completion, lease expiry and negative Matrix observations
fence new source access and new external writes at their existing boundaries.
An already possible write remains uncertain even when cancelled. The worker never
holds Collector's busy/owner lock around snapshot or POST; actual adapter locking
continues to allow the existing negative-observation and current-check behavior.
The one-operation development limit and current 30-second execution ceiling stay
visible: slow work may expire and be refused, rather than silently extending leases.

### Recovery and shutdown

On configured startup Driver first performs actual current collect using the SAME
Collector. This is the only fresh SDK initialization path; do not call open_existing
resume before it and require a fixture-preseeded SDK for first startup. Then run
at most one retained resume_outgoing_custody pass. Only a successful current
refresh plus successful media-owner initialization/readiness ACK allows compatible
claim, Started registration or child launch. Media initialization is requested
from its one owner thread after this refresh/resume sequence; failures are visible
before launch. Validate/open its original journal without repair.

If current refresh fails, a bounded open-existing historical resume may still
settle a valid old protected SDK receipt. Missing/torn SDK state refuses rather
than being created through this historical path. Successful historical settlement
never changes the failed refresh result or enables admission/claim/launch. The App
can still provide exact historical status. No path-existence shortcut asserts that
a missing SDK store is pristine. The one recovery pass precedes new file admission. This performs no upload, event PUT, runtime launch or
scope restoration. SDK Complete may finish the first historical domain settlement
across real process death. SDK Possible, prepared/incomplete media or a lost
capture/preparation result remains unknown. No inventory API or serialized source
capability is introduced. Media recovery checks frame integrity and original
commitments; it cannot prove absent network effects. A legacy/foreign SDK owner or
missing key is not replaced to force recovery success.

GET is a read-only domain inspection plus local ownership projection; it does not
send or trigger repeated reconciliation. A same-process worker may explicitly
finish its exact retained historical acceptance once after a lost result without
another POST/PUT. Normal successful UploadOperation::run already removes its upload
registry entry; only a still-retained unknown upload needs exact settle_upload.
Publication completion removes its separate publication entry. Service job release
then drops the original completed handles and their media permits; no redundant
settlement is added to the successful path. Unknown handles keep their slots. Startup reconciliation is bounded and does not turn an old
Pending row into new executable work. Restart tests reuse only genuinely
host-provisioned task-client credentials as original historical read credentials,
not as a restored current execution capability. No replacement runner is spawned
for a replayed file operation.

Bootstrap quiesces file admission first, cancels execution/file activity and retires
WorkspaceAccess. The file owner retains live futures and original custody while
bounded operations settle or report unknown. Driver keeps its original Report and
physical root until real owned cleanup. Only once both owners acknowledge safe
closure does Bootstrap close the SAME Collector and then its original writers.
Shared retains one original Collector-close wait independently of HTTP/service
caller loss. It records a sticky Unknown if that close itself returns Unknown;
it must not call close again and interpret an empty Owner slot as success. Existing
Collector::close takes its SDK Owner and Owner::close consumes its acknowledgement
receiver on timeout. The wrapper then proves neither a still-live SDK worker nor
completed closure: the original SDK worker may still be closing or have finished.
The fixed status and writer ownership remain retained without a fresh close attempt.
Unknown file/media/process jobs retain their actual local objects where those
owners still exist; no success is inferred from a closed response channel. Existing macOS uncertain
whole-process cleanup remains explicit. File syscall cancellation and power-loss
proof beyond qualified storage evidence are not promised.

### Acceptance evidence

Use the actual native init/serve --development-driver executable and fixed pinned
offline peer installation. Legitimate queued work is established through existing
domain ingress/task APIs before startup; no capability, Started flag, workspace
registration or transport-available SQL is injected. Real collector TLS refresh
must precede claim. The first native MCP request sends a file, proving registration
was usable before launch. A real encrypted recipient decrypts the resulting room
event and downloaded ciphertext and compares original bytes, filename, caption,
relation and sender. Canonical task remains not Done. Separate ordinary group-thread
and null-root DM cases exercise the real entry path, with no live model or service.

Bounded fixtures also cover second admission while HTTP is paused, lost caller,
wrong capability/path, expired queue, source mutation, duplicate/conflicting call,
two retained unknown slots, missing/corrupt staging and SDK state, no second POST
or PUT after uncertain writes, post-promotion refusal, and original owners retained
on close. A true fresh-process SDK-Complete/domain-uncommitted fixture proves first
historical Delivered after restart. On Windows, unavailable directory sync is an
explicit no-POST refusal qualification and is reported separately; a returned test
function is not evidence of positive encrypted delivery. Production capability
flags remain false even where the offline development workflow succeeds.

## Consequences

Original executable fixture failures must retain their exact fixed variant,
current wait phase, last fixed HTTP route category and bounded request count.
Each launched service child has its own finite observation shared only with its
fixture. Before panic cleanup kills/reaps that original PID, record its actual
try_wait result and bounded fixed stderr classification from a retained read
handle acquired before that original child is spawned. Path replacement cannot
substitute another child output. Diagnostic writes are fallible and ignored on
output failure so the original panic and cleanup remain intact. Never print stderr text,
paths, credentials, room IDs or request payloads. Observation preserves original
wait budgets, errors and cleanup and does not infer service exit from missing HTTP.
The original Linux 85427cb failures remain failed and unexplained by local passes.

The original b856b47 Windows executable failure is also retained unchanged.
The existing authenticated operator status now projects the original owned
failure category and ADR053 runtime snapshot with closed labels and optional
counts. It does not expose the private report or change the product flow.
The original offline fixture separately observes five existing helper receipts
through reads capped at 8,193 bytes each, reporting only known status labels.
Absent, malformed, oversized, unsupported and read-failed observations remain
distinct. A receipt present before this service launch stays preexisting and
cannot be attributed to the later launch. No private helper context is read.
These local fixture receipts are observations, not domain or delivery authority;
an absent receipt cannot prove that the helper never executed its preceding step.
No event, keepalive, deadline, retry or original failure behavior changes.


Positive: one original bootstrap and Matrix owner can support a real scoped file tool.
Negative: unknown jobs consume finite capacity and can prevent clean shutdown.

The benefit is an actual opt-in executable/MCP send_file path for one supported
current development attempt. It does not complete ADR027: receive_file output
paths, image previews, unencrypted rooms, continuous scheduling, actual Codex
approval/sandbox qualification and production cutover remain separate. There is
one configuration, Matrix owner and retained source binding; no fixture-only
service authority. The cost is deliberately restricted availability: two uncertain
jobs can intentionally exhaust the service until
host investigation. Finite journals/domain records can exhaust over time and are
not silently pruned. A process crash before durable source capture cannot recover
lost plaintext or keys; it must not recapture changed input under the old call ID.

## Alternatives Considered

A second file-service config/Collector would duplicate identity and shutdown
ownership. Direct source reads in request handlers would let caller cancellation
lose custody and would block shared request workers. Waiting for upload before
returning queued would turn the existing MCP deadline into false failure. An
unbounded spawn-per-request design would evade media/result bounds. A new generic
recovery journal or raw capability restoration duplicates protected domain/SDK
state and risks granting new execution after crash. Automatic POST/PUT retries or
fresh encryption after Unknown would alter the exact original external operation.
Privileged mount namespaces could provide a stronger optional profile, but are
not required by the accepted host-exclusive stable-ancestor development boundary.

**Amendment (2026-09-12) — the safe result names which unknown it is.** The
fixed `error_code` for `outcome_unknown` now distinguishes the two causes this
ADR already enumerates: the durable-receipt projection (`FileView::from_receipt`)
keeps the existing `outcome_unknown` code, while local custody that was neither
acknowledged nor released (`Job::mark_unknown`) reports
`outcome_unknown_custody`. Both remain `FileStatus::OutcomeUnknown`;
`FileView::validate` accepts the closed set of exactly these two codes and
still refuses any other string for that status. No status, deadline, retry,
schema or settlement changes. The custody label is observable on the live job
view, and an exact replay that returns the retained live job reports it too;
`inspect` rebuilds through the durable receipt and therefore reports the base
code, which is stated here so the two are not confused.
