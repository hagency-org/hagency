---
kind: decision
id: ADR-105
title: "Receive an authenticated attachment into its original Started workspace"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [rust, matrix, attachments, workspace, mcp]
---

## Context

ADR027 requires receive_file(event_id), verified local bytes, frozen conversation
visibility, follow-up access and privacy floors. The JavaScript implementation
finds the event in the original dispatch conversation, downloads through the
configured homeserver credential, bounds and decrypts bytes, caches by digest,
revalidates the dispatch and checks the cached bytes again before returning a path.
Filename, MIME and content are user input; keys and private transport metadata do
not enter model context. Unmentioned group uploads are background context; DM
uploads wake their Agent. The native migration's M6 gate requires this workflow.

Native ADR073 already provides frozen attachment visibility and opaque tickets;
ADR074 retains manifests from actual verified SDK ingress; ADR068 authenticates
bounded media GET; ADR076 returns checked plaintext. ADR093 binds a retained root
to the original writer, Started receipt and exclusive workspace lease. ADR101
connects real serve and MCP to the same Collector and workspace for outgoing files.
None of those pieces writes a received file or exposes native receive_file.

Two additional integration gaps matter. The development driver currently refreshes
Collector observations but does not call attachment intake or select an inbox.
An ordinary enqueue_dispatch has deny-all attachment cutoffs. The runtime already
receives the immutable selected inbox as informational prompt data, including
event IDs, but has no attachment discovery tool for other visible background or
follow-up files. A sink-only fixture would not close either gap.

Root accepted the prerequisite design with20 source paths and five coordination
paths after review, then approved eleven test-only schema fixture paths. The
active prerequisite boundary is36 paths. This authorizes only atomic intake selection, current attachment discovery
and schema021 cache facts under task-rust-receive-file-prerequisites.spec.md.
The full workflow proposal is retained under docs/design so unimplemented selectors
do not enter root-spec binding coverage. Sink, application, MCP and incoming
executable qualification remain proposed and incomplete. Implementation starts
after rebasing the isolated design checkpoint onto eb06c35, which includes ADR102.
No test is declared passing by this decision.

## Decision

Root accepted the independent nine-path incoming executable acceptance partition
on 2026-09-11, governed by `specs/task-rust-receive-executable.spec.md`. It must use
actual fresh enrollment, independent encrypted input, native intake/selection and
MCP workspace bytes. Acceptance of this work is not a passing test verdict or
authorization for production activation.

Its local macOS executable observation now passes four cases: actual incoming DM
and addressed-group receive, retained read-only replay after the original write
deadline plus mutation refusal, truncated authenticated media refusal, and fresh
service refusal of a historical Ready path. Each success reads actual bytes in
the launched runtime workspace through the real native MCP. Earlier pre-helper
launch outcomes remain unknown and recorded; their cause is unresolved. This
evidence does not establish process-tree cleanup, every lifecycle fault, installed
Codex sandbox behavior, positive evidence on other platforms or production parity.
The current final lifecycle observation is non-passing: four checks pass and the
truncated-media selector fails before helper entry or any media GET. An earlier
five-check lifecycle passed. Neither that earlier pass nor later passing isolated
diagnostic trials resolves the current launch failure.

Root additionally accepted the application receive-service ownership partition on
2026-09-11 under the operator's existing full migration authorization. Its active
boundary is `specs/task-rust-receive-service.spec.md`: retained jobs, fixed worker,
safe projections and bootstrap ownership. Sink and transport integration remain
separate partitions; this acceptance does not declare executable or platform
qualification complete.

### One bounded development workflow

Add optional receive_file and receive_inbox settings to the existing private
development-driver.json, default disabled. receive_inbox names exactly one existing
session, canonical task, dispatch ID and configured exclusive workspace ID; all
IDs use the existing 128-byte validator. It accepts no event, room, URL, descriptor,
capability, approval or raw payload. This profile does not create tasks, sessions,
resources or trusted identities. Unknown fields and duplicate map keys refuse.

After existing current refresh and explicitly configured fresh-account enrollment,
run one real Collector::intake with that already-current session. Select the oldest
unassigned wake-bearing input and a bounded preceding context from actual canonical
session inputs, then use the existing enqueue_inbox_dispatch transaction to freeze
the original selected input and attachment cutoffs. Preserve the trigger even when
older context exceeds the 100-event/64-KiB dispatch bound, including JSON escaping;
do not replace an
oversized trigger with a later one. Before admission, recheck every selected
message against the original verified ingress and current privacy floor. The
replay branch also refuses original input whose current scope has retired.
No wake means no new dispatch. Unmentioned
group attachments alone therefore do not start a runtime.

Perform plan validation, original-ID lookup, input selection and inbox admission
in one original writer transaction keyed by the configured dispatch ID. Factor
the existing enqueue_inbox_dispatch transaction body into a shared transaction
helper; do not nest its public transaction or commit a partially selected plan.
The replay branch checks the stored immutable plan and frozen input before any
new selection query. A lost enqueue reply therefore returns only that original
selection, never resamples a changed replay. Changed task/session/workspace/selection
conflicts. Apply
an optional exact dispatch restriction to the existing host-compatible claim,
preserving every current eligibility predicate, transaction and actual claim
clock. This prevents another queued task from being claimed for this plan. Lost
claim or intake evidence halts the attempt. There is one intake/selection/launch
attempt per configured serve start, with no polling scheduler or unknown retry.

This ingress/context prerequisite is a separate review partition from receive
storage. It must have its own accepted bounded implementation checkpoint before
the application is claimed complete. It must not be replaced by fixture calls to
raw admission, session visibility, Started, registration or capability setters.

### Runtime discovery and transport

Expose list_received_files({after?, limit?}) and receive_file({event_id}) through
the actual native MCP, task client and loopback runner routes. Discovery returns
at most 16 safe records and a bounded next cursor: event ID, source sequence,
filename, optional MIME and untrusted declared size. It uses the same source and
projection cutoffs, provenance, lineage, privacy floor and current route checks
as AttachmentTicket. A runner inbox page alone must not silently substitute for
older visible attachments. Prompt input and tool descriptions label all attachment
metadata and content as untrusted user data, never execution instructions.

Both operations require current runner authentication. There is no historical
GET exemption for a cache path. New routes are GET /api/native/v1/runner/received-files
and POST /api/native/v1/runner/received-files. POST accepts only event_id, retaining
ADR027's call shape. No destination path or caller-provided filename is accepted.
Exact original capability plus event forms the request identity; stable event
selection is sufficient without introducing a second caller-generated call ID.

Successful receive output contains event_id, filename, optional MIME, actual size,
plaintext SHA256, a generated workspace-relative path and replayed. It contains no
absolute root, MXC, key, descriptor, route, token, private SDK identifier or backend
error. A fixed error code represents refusal, capacity or outcome_unknown; neither
Queued nor Ready means Matrix delivery or canonical task completion. Receive does
not send room messages. The path is .hagency-received-<32 lowercase hex>.bin, a single
portable component independent of filename metadata. Output is at most 4096 bytes;
discovery is at most 16 KiB; requests remain at most 16 KiB and use existing MCP
framing, request-ID validation and the unchanged five-second client deadline.

Use a separate private Host receive-tools option and fixed inherited
HAGENCY_RECEIVE_FILE_TOOLS=1 presentation marker. The actual TaskMcp enabled_tools
and helper env_vars must include precisely the two receive tools and marker only
when this service is initialized. Send-only and default profiles remain unchanged.
The marker is not backend authority. The host reserves it against arbitrary
environment injection. A helper with a forged marker still cannot obtain bytes.

### Original checked bytes and writer association

Retain the actual ticket, original capability and original DomainStore in
ReceivedAttachment. Keep its constructor private and omit Clone, Debug and serde.
Borrowing a ticket or digest exposes association data, not a verification setter.
Its current revalidation uses the captured writer; it cannot accept a replacement
store. No Collector/Inner cycle is introduced.

Add a bounded receive entry point accepting an earlier absolute deadline and the
configured lower byte limit. Capture the job deadline before the first local
queue; clamp it to the existing SDK deadline rather than restarting time at GET
or disk work. Apply the lower limit before result allocation and HTTP reading.
Retain existing configured-origin authentication, no redirects/proxy/fallback,
complete framing, ciphertext hash verification and decryption. Declared size is
only an early bound. Preserve the existing early result and downloader permits.

Revalidate the exact captured ticket and original StartedWorkspace after download,
after waiting for the sink worker, immediately before the first file effect and
after writing and rereading bytes, before returning the path. Writer time is sampled
after queue/SQLite waits. The workspace guard uses its captured original writer,
root, scope generation, capability, lease and operation retirement; path equality
or a copied database cannot supply current authority.

### Durable cache facts and one original write

Propose schema021 for received-file cache facts, separate from outgoing upload and
event state. Existing attachment tables describe ingress/visibility, not a local
write identity; outgoing reservation would incorrectly imply publication rights.
Reuse existing canonical transaction, immutable-content comparison, bounded writer
queue and lost-reply conventions instead of creating another database owner.

Reserve one opaque local operation under exact original capability/event before
GET. Bind the complete original ticket association, metadata, original execution
scope/workspace association and configured limit; after download bind actual length
and hash once. Domain values are host-provided correlation facts, not SDK or
filesystem proofs. The application still owns the actual ReceivedAttachment and
StartedWorkspace. A changed field conflicts even if a dependent hash is recomputed.

States are Reserved, WritePossible, Ready, Failed and OutcomeUnknown. Commit
WritePossible before any destination creation. A unique non-deserializable local
write grant is issued once; restoring a record never recreates it. Ready requires
the actual original destination result, acknowledged file and directory sync,
readback/hash checks and current authorization. Lost domain Ready ACK is inspected
against the exact original record and held destination, not blindly retried.

After acknowledged Ready, release the checked plaintext/result permits and live
write-job slot. A separate bounded Ready owner retains the actual file plus an
opaque read-only ReceivedScope containing the same captured writer, capability and
ticket. There are at most32 such owners/eight per workspace, within the permanent
record quota. The separately contracted ReceivedScope consumes the checked result
to release its plaintext and permits while retaining the same writer, cap and
ticket. It cannot accept a replacement writer or create download/write authority.
Replay uses a fresh bounded read-only response deadline, not the old
write-job deadline. It revalidates current authority and the held original file;
it cannot restart a download or write. Missing original Ready custody or a retired
binding refuses even when historical metadata says Ready.

Before writing, a known failure can be recorded without a path. During caller loss
the already-admitted owner continues only within the original deadline. A missing
owner, worker unwind, write error or expired deadline after write admission stays
unknown and retains custody. Replaying Reserved without its actual original job
does not redownload; replaying WritePossible never creates a replacement file.
Only an acknowledged Ready record plus the same live original binding and retained
destination can return a path, after read-only revalidation and rehash. Modified,
replaced, missing or linked files refuse; there is no overwrite or repair.

On fresh process open, incomplete records remain unknown. Old Ready metadata is
historical data, never a current root binding, disk-write grant or returned path.
This first profile does not recover an interrupted write into Ready after process
death. A legitimately selected follow-up has a new current capability and may
receive an older still-visible event as a new bounded copy. It cannot use an old
capability or digest alone. No automatic pruning or reset is introduced.

### Retained workspace sink and lifecycle

Implement the sink inside execution/workspace/received.rs using a duplicate of
Root's actual retained directory, not an ambient pathname or an exported root.
hagency-files remains the source snapshot crate; Matrix/media do not become its
dependencies. The sink accepts checked byte/hash data from trusted host code but
does not claim those values authenticate a sender.

Before any effect, prepare a non-Clone WorkspaceReceive owner from the original
StartedWorkspace, ReceiveWrite and facts, and store it in the retained application
job. Its borrowed one-shot materialize operation stores the actual newly created
file in that owner before writing. Error or worker unwind leaves the same owner
and partial file retained; a consuming Result must not destroy the only custody.
Repeated materialize cannot rearm. Validation and readback use this same owner.

Use relative create_new for the generated single component, NoFollow and bounded
nonblocking file-type inspection. Retain the actual created regular file and root;
require a single link and private current-owner permissions. Reuse the existing
creation-only Windows ACL sealer before any plaintext write; never seal an existing
file. Preserve all 128 Windows file-ID bits in retained-object comparisons.
Recheck the current directory entry against the held original file, type, links,
length and digest. Parent/root substitution, reparse points, symlinks and hard links
refuse. Never truncate, overwrite, rename over or unlink an uncertain entry.

Write only already-completely-verified plaintext in bounded chunks, observing local
retirement between chunks; then check actual readback and required sync results.
An owned fixed sink worker retains an in-progress synchronous operation: timeout
does not stop a kernel call, prove absence of a file, or release its slot. There
are at most two receive jobs, one synchronous sink worker and one bounded command
queue. No per-request spawn_blocking or detached cleanup. Exact task IDs associate
worker failures with original retained jobs. Read-only GET completion and known
pre-write refusals may release after original completion is observed; ambiguous
writes retain ownership. Close is borrowed, quiesces admission, retires access and
acknowledges only after joined receive work. Collector closes once, after send,
receive and execution owners. Pending or failed close remains sticky unknown.

The filename may become observable in the workspace while the one-shot write is
in progress. It is never returned as successful before complete verification.
Domain revocation and filesystem writes are not one atomic transaction: a racing
retirement can leave a partial or complete quarantined entry and cannot erase
bytes already observed by a runtime. The implementation must check retirement
around actual IO, deny subsequent output/replay, and report the retained effect
honestly. It must not claim stronger transactional non-observation.

### Bounds and platform qualification

Proposed permanent limits are 32 records globally, eight per logical workspace
under this host's non-overlapping root configuration, and 128 MiB reserved output
bytes globally. Reserve each configured maximum before GET and retain charges for
unknown writes. Per item is 1..4 MiB as configured; sender size does not increase
it. At most two live results add bounded memory to the existing Matrix pools;
these accounting limits are not an RSS or kernel scheduling guarantee.

Without cleanup these quotas eventually exhaust. This is a bounded development
profile, not a generally usable production cache. Capacity refuses before a new
GET/write; service restart, changed IDs and caller cancellation cannot reset quota.
No operator cleanup command or production activation is included.

Keep ADR093's host-exclusive root and stable-ancestor trust boundary. This does
not defend against hostile same-UID namespace control. Codex's pinned path
canonicalization means descriptor aliases and child fchdir are not independent
sandbox qualification. Linux, macOS and Windows require actual executable and
filesystem evidence, including permissions, identity, reparse/link refusal and
sync behavior. Windows qualification must work without privilege escalation;
ordinary non-elevating token privileges are not a reason to reject an otherwise
qualified object. Zero enabled token privileges is not a production requirement.
Unavailable directory sync or private creation qualification
refuses the positive cache operation; cross-compilation is compilation only.
OS sync acknowledgement is not physical power-loss evidence. Neither a negative
Windows branch nor a fixture runtime proves M6 or real model/runtime parity.

## Consequences

The proposed feature completes a finite real ingress-to-workspace development
workflow while preserving the same original writer, SDK and execution authority.
It introduces local cache fact state because lost write replies cannot safely be
treated as permission to create again. Unknown entries consume space until a
separate reviewed cleanup policy exists. Follow-up access remains current scoped
receive, and old paths do not become a historical authorization channel.

Mandatory acceptance includes real native serve, real SDK intake, actual selected
runtime context, real native MCP, a counted authenticated TLS download and byte
access through the original runtime workspace. Sink/unit tests alone cannot mark
the workflow complete. Dedicated-account Matrix compatibility, installed Codex
sandbox behavior, all-platform positive cache evidence and production migration
gates remain separate and cannot be enabled by this proposal.

## Alternatives Considered

Returning the host's private media cache path would not establish access from the
original workspace. Passing an MXC, descriptor or arbitrary destination from MCP
would bypass the existing provenance or physical root boundary. Using the sender's
filename as a path would add Windows reserved-name and traversal ambiguities.

A memory-only idempotency map cannot fence a lost reply across restart. Reusing
upload Accepted or file Delivered would confuse local receipt with external
publication. Hash-only replay could accept a substituted file or another scope.
Automatic partial-file deletion or overwrite could destroy an unrelated entry;
the first profile instead retains unknown outcomes and finite quota.

Full conversation-history porting and a continuous dispatcher are larger slices.
The proposed current attachment page and one explicit intake plan are sufficient
to make receive_file discoverable and testable without claiming that broader
parity. Mandatory hostile same-UID mount namespaces would exceed the accepted
host-provisioning boundary and do not replace real platform/runtime qualification.
