# Native Hagency migration

This worktree contains native foundation and selected-resource domain checkpoints. It is **not a
replacement for the deployed Hagency application**. The deployed workflows still
run in the existing JS/TS implementation. Native capability responses distinguish
development resource/task APIs from unavailable Agent execution, connected Palpo/
Matrix transport and production API parity.

The current developer checkpoint includes domain schema 22: scoped tasks,
internal groups, durable graphs, verified-input task activation, owner approvals
and notice/final-reply send custody, with exact negative Matrix transport fencing. Independent custody schema 2 preserves outbound work and publication
receipts across machine-token rotation; hagency-palpo adds bounded outbound HTTPS
with independent polling and publication. The runtime crate provides a bounded
Codex one-turn session connected to guardian-owned Unix pipes and Windows
overlapped pipes under atomic Job Object custody for offline fixtures. The native CLI and MCP helper maintain an assigned task through the scoped API.
The opt-in permissions coordinator consumes durable owner authority before an
exact typed Codex response; upstream application remains explicitly unproven.
The hagency-matrix library collects authenticated account and full room state,
then admits bounded verified sync events into existing sessions through encrypted
SDK and pending-sync custody. Offline encrypted DM fixtures exercise actual SDK
verification. The host-only sender now performs actual authenticated notice and
final-answer HTTPS writes, including verified encrypted DM/group fixtures, with
durable acceptance custody. A separate approval-bot collector now admits verified
private owner verdicts without sharing Agent identity/cursor state. Complete SDK
event refusals retain terminal records while later eligible chat continues;
unknown processing and negative identity/room evidence retain their fences.
Live key lifecycle, approval cards, history and service wiring remain gates.
The owned runner can now configure and launch the native task MCP helper against
the same canonical writer. Explicit `complete_task_with_reply` atomically commits
Done and holds final content while revoking the old execution epoch. The same
retained process owner must establish cleanup before the writer admits a final
reply. Plain task-only Done still provides no final content. Actual delivery is
separate from both transitions. An offline group/thread integration test now
joins intake, notice activation, real native MCP completion and the final sender;
local macOS proves cleanup refusal. Actual Linux and Windows CI at c0afefc passed
this final-delivery workflow; the Windows run failed a separate approval fixture,
so it was not an overall passing native run.
The hagency-files library copies bounded immutable bytes through retained workspace
directory/file capabilities. The hagency-media codec adds bounded attachment
encryption and fully checked decryption using the pinned Matrix SDK. The new
hagency-media-store retains bounded bytes and original encryption descriptors
through interrupted writes/restart under private directory/file handles. Missing
or incomplete storage remains explicit; Windows unconfirmed directory sync is
distinct from durable admission. PreparedEncrypted now retains the original codec object and its stable storage
commitment before journal IO. Qualified clean recovery returns distinct
RestoredEncrypted custody for that original operation and receipt digest,
retaining the exact ciphertext and descriptor. Neither reconstructs source
handles nor authorizes an upload retry. A host-only encrypted downloader now uses authenticated HTTPS to the configured
homeserver, consumes complete bounded ciphertext and verifies it before returning
checked plaintext. A sibling encrypted uploader borrows original codec ciphertext,
retains a finite attempt across cancellation, and accepts only complete bounded
repository responses. A possible write cannot automatically resend. Verified
encrypted file/image intake now retains private descriptors independently of sync
completion. Schema18 stores safe metadata and freezes attachment visibility from
the dispatch's actual selected inbox trigger plus a separate projection cutoff.
Host lookup checks current capability and exact scope before and after its
asynchronous work. Keys stay in the encrypted SDK journal. Current-dispatch
receive now coordinates the retained manifest, bounded authenticated download
and final authority revalidation under one deadline, returning host-only checked
bytes. Four held results remain bounded across SDK Owner reopen. Schema19 now
retains exact upload reservations, staging commitments, fenced claims and
nonrearmable possible writes. Lost replies never recreate preparation or send
grants; historical acceptance remains separate from current task/route authority.
Only opaque private receipt commitments enter the domain database. Actual checked
upload response bodies now stay in finite attempt custody, and a separate
encrypted SDK journal retains exact historical acceptance. Its one-use permits
cannot survive owner replacement as execution grants. Exact protected-row
settlement can recover after process exit without recreating a runner secret.
The consuming staged upload owner now retains exact original inputs through one
HTTPS upload, cancellation and historical settlement. It authenticates the current
token, rejects mismatched or oversized inputs before retained admission, and never
reissues its send grant. A fresh process can settle SDK acceptance that was not yet
committed to the domain, without another upload. Already enqueued negative Matrix
observations survive caller loss. Owned execution now retains its actual private
workspace root and exposes one file-source binding after the original Started
acknowledgement. That binding uses the original writer for current checks and
retires new reads after execution exits. Fixed paths and ancestors still require
trusted host provisioning; actual runtime sandbox qualification remains open.
Test-only Matrix diagnostics now retain each original operation's phases and
separate original/fencing errors across SDK work and caller loss.
Schema20 adds immutable file metadata and independent event-delivery custody.
The encrypted publisher consumes the actual original accepted upload and media
permit. Recipient SDK fixtures decrypt the original bytes, filename, caption and
frozen thread/private relation. Coherent metadata/key replacement cannot settle.
Complete event acceptance can recover first Delivered after restart without a
new upload or event write. A lost final SDK settlement ACK releases retained
media only after exact private receipt/already-Delivered acceptance replay;
unmatched retained jobs stay unknown. Native file publication and receive services
are now integrated development slices, with current-only HTTP/MCP discovery,
retained checked bytes and one-shot workspace writes. Incoming executable
qualification currently has four of five strict scenarios passing; an unresolved
pre-entry failure prevents claiming that workflow complete. Installed runtime and
production migration gates remain open. Schema17 also stores host-attributed token observations and exposes
aggregate operator reads while preserving unknown and incomplete evidence. The
private owned runner now binds a historical source before spawn and records exact
fresh-session usage through typed normalization. Fixed counter evidence and one
pending receipt survive bounded failure; retries cannot renew execution or claim
provider billing. Runtime stream coverage remains explicitly incomplete.
Default service Agent execution and connected Matrix delivery remain disabled.
The explicit `serve --development-driver` mode now permits one supported attempt
from a fixed private development profile. The actual Bootstrap authenticates the
same Collector, claims only compatible work and registers the original Started
workspace before launch. It retains process reports and writers when close is
unknown. This is not a continuous scheduler or production activation. See
[ADR096](../knowledge/decisions/adr-096-native-development-bootstrap.md) for the
closed profile, required host provisioning and remaining sandbox qualification.
The same-process FileService and MCP file-tool slice is still being implemented.
The sections below record the successive checkpoints.

Build and run from this worktree, using a new state directory:

```sh
cargo build --locked --release -p hagency
cargo run --locked -p hagency -- init --state-dir native/.state
cargo run --locked -p hagency -- serve --state-dir native/.state
```

The same Cargo commands work in PowerShell. The service defaults to
`127.0.0.1:13300`, requires loopback and never reads `.env`. Init creates an
owner-private `operator.token` and fresh custody/domain databases; it refuses nonempty
state and never replaces an existing token. Keep the token in its protected file.
Only one process may own a state directory. Use Ctrl-C for a drained shutdown.

- `GET /health`: process health and explicit foundation stage.
- `GET /api/native/v1/capabilities`: authenticated capability availability.
- `POST /api/native/v1/custody`: authenticated **fixture** custody. Requires exact
  Host and Bearer token; browser Origin/forwarding headers are rejected. The body
  contains `binding`, `generation`, `id`, `lane`, `kind`, and object `payload`.
  A 202 receipt means durable storage only. It cannot approve or run an Agent.
- Existing production routes deliberately remain absent until their native
  implementations meet the corresponding contracts. Do not point the live console
  at this service.

Operator-only development resource endpoints under `/api/native/v1`:

| Method/path | Behavior |
| --- | --- |
| `POST resources` | Create/edit a resource; creation defaults to publication, an omitted publication choice on edit preserves withdrawal |
| `GET resource-configurations` | Paged operator configuration, including withdrawn resources |
| `GET resources` | Paged catalog projection; omits internal preset and seat IDs |
| `GET resources/{id}/budget` | Selected pool/shared-seat commitments; unknown quota remains null |
| `GET engagements/{id}/usage` | Aggregate untrusted observations plus optional UTC day/month; `at_ms` selects a period, omission uses writer time |
| `GET/POST seats` | Paged quota declarations or save a declaration; no credentials are exposed |
| `GET engagements` | Paged request/allocation projection without private owner-room evidence |
| `GET roles` | Derived eligibility with explicit publication choices and cross-family requirements |
| `POST roles/{role}/publication` | Save `{ "published": false }` to withdraw a role, or true to restore it |

List endpoints accept `after` (last returned ID) and `limit` (1–100). These routes
share the operator bearer and exact-host/origin checks. They are not the existing
console API. Requests, approvals and effect receipts cannot be submitted through
fixture HTTP routes; the development service does not launch runners or connect
the authenticated Matrix collector yet.

Checks:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
node native/scripts/canonical-vectors.mjs --check
node native/scripts/allocation-vectors.mjs --check
node native/scripts/project-vectors.mjs --check
node native/scripts/qualification-vectors.mjs --check
node native/scripts/check-rust-spec-bindings.mjs
```

The binary test clears PATH, launches native HTTP, submits a request, kills the
process and verifies the original receipt after restart. The offline Matrix SDK
proof tests encrypted device persistence and strict cross-signing; it does not
contact a homeserver. The offline proof uses dev dependencies; hagency-matrix uses the SDK as a
production library dependency, but neither is linked into the current `hagency` binary. SQLite is bundled; other native library
requirements still need a final packaging audit.

Canonical vectors execute the pure encoder from the pinned existing JS source.
The native encoder currently accepts JSON-safe integer DTOs, strings, arrays,
objects and null. It rejects fractional/unsafe numbers and prototype properties;
it is not a general Matrix canonical JSON replacement. Matrix crypto uses its
SDK's own encoder.

[State boundaries and remaining gates](../knowledge/decisions/adr-095-native-state-ownership.md)
explain the transaction model, bounded work, platform requirements and recovery.
[Source inventory](fixtures/legacy-inventory.json) now records 138 explicitly classified
helpers, 199 Express routes plus 3 middleware registrations, 12 custom dispatcher
branches, CLI/MCP declarations and the Next proxy surface. Run
`node native/scripts/inventory.mjs --check` with development dependencies installed.
The [inventory review](../docs/reviews/2026-09-10-native-migration-inventory.md) records
source links, detector limits and remaining M0 gates. Every row remains
`parity-unverified`; source classification does not prove native implementation.

M2 now includes a single `domain.sqlite3` owner for project bindings, immutable
request identities, resource reservations, decisions and provisioning/retirement
intents. The core verifies adapter-supplied room observations (including exact
source content and the encrypted private owner room); those observation types
cannot be deserialized by HTTP. Actual authenticated observation collection is M5.
Full observations remain in private audit storage, never public projections.

Two simultaneous approvals recheck capacity inside one transaction. An outbox
write failure rolls the reservation back. Started effects become uncertain on
restart, retain their allocation, and cannot be claimed again until reconciled.
Revocation fences provisioning and records retirement separately; confirmed failed
retirement requires explicit retry. These are tested with fixture observations,
not actual Matrix account/process creation. No effect worker executes externally.

Resource budgets match29 JavaScript vectors; Unicode names, public IDs and runtime
identity derivation match38 identity vectors. Native qualification embeds the existing
`lib/role-capacity.json` and compares99 model profiles plus18 ranking cases against
JavaScript. Resource input specifies framework/model/provider/reasoning, and the
API rejects supplied role lists. Catalog roles are derived, explicit role withdrawal
persists, and cross-family review requires active Agents on the same registration.
Changing the model/provider/reasoning of an allocated pool is refused. Native schema2
adds role publication through an atomic migration; older role caches grant nothing.
Framework detection, legacy role-only allocations, safe generation-rotation
reconciliation and continuous retention remain open implementation work. See the
[checkpoint review](../docs/reviews/2026-09-10-native-domain-checkpoint.md).

The M3 kernel adds canonical tasks, sessions, dispatches, resource leases, mutation
receipts and task event outbox to the same database through schema migration 3.
Private runner capabilities use OS randomness and stored hashes, are fenced per
attempt, and require a current active allocation. Task mutations require a started
runner bound to that exact task. Coordinator reads include its own session's
creations, without authority to mutate another assignee. Task completion is explicit
and advances its authorization epoch; runtime output never marks a task done.

Dispatch input is frozen before launch. Parked dispatches retain their resource
leases. On restart or expiry, unstarted attempts may requeue; started work becomes
unknown, quarantines its session and marks writable resources dirty. Only a host
inspection command can create a distinct recovery instruction. Earlier queued
instructions are superseded, and late output is retained as rejected audit input.
An already completed task remains complete if only its result needs recovery.

Five new native tests cover all 25 JavaScript transition-policy pairs, capability
scope, transactional rollback/replay, restart/expiry and concurrent resource claims.
These are kernel tests using fixture allocations. Host session binding, inspection
and process ownership still require the real transport/runtime adapters; no HTTP
route exposes fixture authority or recovery. Mailbox/graph/delegation and reply
outbox delivery are still open M3 work. A task event outbox is not proof of Matrix
delivery. No native runner is launched by this checkpoint.

Schema 4 adds canonical conversation uniqueness and message/input ownership.
Each source event has one content-bound identity and an independent projection
for every eligible session. Fresh native state uses committed arrival sequence
for total ordering; the external timestamp is retained as source data. Inbox reads
are bounded and do not acknowledge messages. A dispatch claims selected admitted
input and freezes the inbox payload in the same transaction; ordinary work requires
at least one wake input. A runner can read only its own frozen inbox.

Completion acknowledges only that session's input. Unknown work retains its input
for inspected recovery; superseded unstarted work releases its input back to the
pending inbox. Quarantined sessions can still receive messages. Failed admission,
enqueue, processing and schema migration are covered by rollback tests. The
non-deserializable ingress command is a host adapter boundary; real Matrix event
authentication and mention/DM classification are still required from M5. This
checkpoint does not implement room history, group management or task delegation.

The private runner API is available at `/api/native/v1/runner` when the domain
store is configured. It requires the exact loopback Host, a bearer runner secret,
and `X-Hagency-Dispatch`, `X-Hagency-Runner`, `X-Hagency-Fence` headers. Duplicate
headers, URL credentials and browser/forwarded requests are refused. Operator
credentials do not authorize this surface; runner credentials do not authorize
resource management. Responses use `Cache-Control: no-store`.

| Route | Operation |
| --- | --- |
| `GET /tasks` | Page the bound task and this session's coordinator creations |
| `GET /tasks/{id}` | Read a visible task |
| `GET /tasks/{id}/comments` | Page comments for a visible task |
| `POST /tasks/{id}/operations` | Submit a call ID and typed task mutation |
| `POST /delegations` | Create a scoped task intent for a project Agent |
| `POST /conversations` | Create an internal conversation with current project participants |
| `GET /conversations/{id}` | Read a conversation from its exact creator/participant session |
| `POST /conversations/{id}/operations` | Change members or close an internal group as its current exact creator |
| `POST /peer-messages` | Send a scoped peer message to current internal recipients |
| `GET /peer-inbox` | Read the current dispatch's frozen peer input |
| `GET /inbox` | Page only the current dispatch's frozen input |
| `POST /final-replies` | Persist bounded final content for the exact canonical Done task epoch |
| `GET /final-replies/{id}` | Read a scoped metadata-only reply receipt |

A mutation body is `{"call_id":"heartbeat-1","operation":{"action":"execution","heartbeat":true}}`.
The domain writer obtains its own clock after body reading and queueing, then
rechecks current authority. There is no runtime-supplied clock, author or arbitrary
endpoint command. Claim/start/recovery, session admission and configuration remain
host operations. The `runner_task_api` capability describes this interface;
`agent_execution` remains false until real native runner adapters are available.

The native task helper uses that same API and domain writer:

```sh
hagency task get
hagency task --call-id heartbeat-1 heartbeat
hagency task --call-id wait-1 wait --reason "Waiting for source" --until 2030-01-01T00:00:00Z
hagency task --call-id resume-1 resume
hagency task --call-id done-1 done
```

The host must provision `HAGENCY_RUNNER_API_ADDR` (literal loopback socket),
`HAGENCY_RUNNER_CAPABILITY` (private scoped JSON) and `HAGENCY_TASK_ID` in the
runner's inherited environment. No operator-token fallback or credential file is
used. The launcher integration that provisions these values is still to implement.
`start` records a heartbeat of the host-started task; it never creates another
task. `comment --text` adds a canonical task comment. Every mutation requires an
explicit call ID, and the response is one JSON task result with its replay status.

The client has a five-second total deadline, a 16 KiB request/header-buffer bound,
32 response headers and a 64 KiB response limit. It follows no redirects, resolves
no arbitrary DNS and uses no environment proxy. Lost mutation responses stay
unknown: inspect the task or retry the identical command and call ID. Raw remote
error bodies and credentials never appear in its diagnostics. Local fixtures test
the executable and actual loopback API, including task scope, parking, idempotent
replay, malformed/oversized responses, stalled bodies and cancellation closure.

Pure reusable permission-scope derivation matches 64 JavaScript policy/path
vectors. Candidate scopes alone do not grant permission. Schema 13 stores private owner
verdicts and scoped grants; actual runtime application and effective sandbox
qualification remain required. Verified task-notice sends now commit Sending
before returning a frozen host snapshot. A lost response, lease expiry or restart
keeps possible sends Uncertain; only exact host inspection can resolve them.
Retired routes, task epochs and explicit cancellation never reactivate from a
late delivery or NotSent receipt. The actual Matrix adapter must still coordinate
current membership, encryption recipients and cancellation before IO.

Schema 5 connects task metadata, admitted source inputs, canonical conversation
binding and an acknowledgement outbox in one transaction. Tasks remain pending
until an exact current transport claim receives its content-bound room/server/
transaction receipt. The acknowledgement event is an activation anchor; the
conversation still uses the original source event as its thread root. Stable
Matrix transaction IDs survive retry/restart. Permanent failures require an
explicit host retry; inactive allocations cancel their unsent notices.

Delegation uses the started creator capability, same project allocation and
creator-owned source inputs. Runtime JSON cannot supply a transport receipt or
owner assertion. Child tasks retain independent input processing and do not
complete their parent. Taskless dispatches cannot bypass a pending task binding.

A completed task stays done while fresh human follow-up is queued or leased. At
start, the shared readiness predicate requires an attached, unprocessed wake
input from the original sender and thread, with receipt and origin timestamps
after completion. Other senders, non-message peer events and old input cannot
reopen it. Start atomically advances the task epoch and queues one continuation
notice. Previous capabilities remain fenced by dispatch ownership.

These are task orchestration and private API tests using fixture Matrix data.
The real M5 adapter must authenticate source identities, classify agent traffic,
verify current membership and apply DM/promotion privacy before constructing host
commands or delivering notices. This step does not claim transport delivery,
model execution, filesystem write authority or full M3 parity. Graph scheduling,
final replies and internal group/MCP surfaces remain subsequent migration work.

The pure graph planner preserves dependency propagation, conditional skips,
primitive JSON equality (including missing versus null), truthiness and stable
node order. It produces an uncommitted transition with dispatch candidates; the
caller must atomically bind those candidates to canonical tasks and durable
mailbox admission before storing them. Host observations are separate from graph
request definitions. Graphs permit at most 128 nodes; each result is at most
64 KiB UTF-8 and depth 64, retaining fractional values. Conditions that introduce
cycles are rejected before planning, including cycles hidden outside depends_on.

The existing JavaScript policy generates 156 condition vectors and ten graph
transition vectors; CI checks the fixture against that policy before Cargo tests.
Prototype methods are modeled only as inert comparison/truthiness values and
blocked path segments remain blocked. No JavaScript code is evaluated in Rust.
Durable graph storage, canonical observation authority, group/mailbox routing and
native graph execution are not implemented by this pure policy step.

Execution payload encoding now accepts finite fractional values and follows
JavaScript IEEE-754 Number semantics, including rounding beyond the safe integer
range. Enqueue stores that canonical representation and hashes those exact bytes,
so a retry cannot hash one numeric value while running another representation.
Authority DTO encoding remains a separate strict integer path; token counts,
generations and timestamps retain their explicit typed bounds. Prototype fields
and excessive nesting remain rejected. Identifiers remain strings.

The pinned [ryu-js 1.0.3 formatter](https://docs.rs/ryu-js/1.0.3/ryu_js/)
provides ECMAScript shortest formatting through its safe finite-number API.
It adds no default runtime dependencies; its Apache-2.0/BSL-1.0 license and Rust
1.71 minimum were checked. serde_json float_roundtrip preserves parsed doubles.
CI compares 271 deterministic numeric payload vectors against the unchanged JS
canonicalizer; store tests cover numeric content conflict and restart replay.

Schema 6 adds explicit internal conversation routes. Matrix bindings retain their
room/thread wire format; internal bindings carry kind=internal and a conversation
ID, with no room fields. Both share canonical tasks, runner attempts, leases and
restart recovery. Internal routes cannot receive Matrix input or use its inbox
API. Mixed/unknown wire shapes fail, and Matrix uniqueness is preserved through
migration. All session loads check current allocation and internal membership.

A started runner creates a bounded same-project conversation using a call ID,
label and participant engagement IDs. The creator Agent is included automatically.
Conversation identity, content receipt and every participant session commit in one
transaction. Access is limited to the exact creator session or the internal session
bound to that conversation; another Matrix session for the same Agent is refused.
Completed tasks cannot create conversations. Revocation invalidates the affected
participant's existing execution capability. Conversation creation does not deliver
messages. Schema 7 supplies durable peer delivery and exact-session input receipts;
schema 8 scopes inspected completed-result reports to the original task epoch.

Schema 9 adds internal group membership and closure. Submit a body such as
`{"call_id":"members-1","expected_revision":0,"action":{"kind":"members","participant_engagements":["engagement_id"]}}`
or `{"call_id":"close-1","expected_revision":1,"action":{"kind":"close"}}`.
Membership is the complete desired set plus the creator's engagement. Mutations
require a current started creator capability; stale revisions and changed retry
payloads fail. Receipts return the original operation response, while reads obtain
the current active group. Closed groups cannot be reopened.

Removed participants retain history and get fresh sessions if added again. Closure
also closes groups created by the retired sessions. Obsolete dispatches lose their
capabilities; started work retains leases and a durable host stop intent until the
host has inspected actual process termination and workspace effects. Settlement
never marks tasks done or acknowledges input. Pending stops count against runner
capacity and survive restart. The host methods are deliberately absent from the
runtime API. See [ADR-030](../knowledge/decisions/adr-030-native-conversation-retirement.md).
These transactions do not implement Matrix room membership or actual process
termination; native runner adapters and final delivery remain.

Schema 10 binds durable task graphs to canonical tasks and peer inputs. The private
runner API accepts `POST /graphs` with a call ID, conversation ID and bounded graph
definition. Assignees are exact current internal participant session IDs. Creation
commits all canonical node tasks; dependency planning admits only ready assignment
messages. Host execution must bind the assigned task and message together. Runtime
definitions cannot supply owner identity, workspace grants or inspection evidence.

The exact creator can list metadata with `GET /graphs`, read `GET /graphs/{id}`,
and submit `POST /graphs/{id}/cancel` with a call ID. Bound node runners submit
`POST /graphs/{id}/results` with call ID, node ID and a typed complete or failed
outcome. Success requires explicit canonical done state for that exact task epoch;
failure requires blocked state. Graph state never completes the parent task.
Command receipts and dependent assignments commit atomically. Inspected report
recovery can repeat the exact completed result without rerunning the node.

Results stay outside graph views and assignment payloads. Workers page their
pinned references through `GET /graphs/{id}/dependencies?after=0&limit=32`, then
read one with `POST /graphs/{id}/dependencies` and `{"node_id":"dependency"}`.
Only the creator may read other completed node results. Each result is at most
64 KiB; dependency pages are at most 32 references. Cancellation or scope retirement
fences execution while retaining uncertain process custody for host inspection.
Lease expiry and restart retain unknown readers' leases and concurrency slots;
inspection releases only that attempt's custody. The migration also restores
missing unknown leases left by earlier native versions. Retired assignment history
is retained without consuming live queue capacity or fabricating an acknowledgement.
See [ADR-031](../knowledge/decisions/adr-031-native-task-graph-custody.md). Model
execution, MCP graph tools, final reply delivery and production parity remain open.

Schema 11 freezes Matrix server, room, sender/device, owner, explicit privacy and
generations before a fresh session receives reply authority. Legacy sessions are
not upgraded from current room state. Full member snapshots and negative evidence
retire old sessions and pending replies; restored access needs a fresh session.
DM promotion also fences sessions with a null thread root.

Final content uses `{"call_id":"final-1","body":"Completed result"}`. Admission
requires the exact canonical Done epoch or an inspected report grant and creates
one immutable intent per task epoch. Host claim, send-start, observed delivery and
inspection are separate operations. Possible sends remain uncertain after restart
or cancellation. A later NotSent inspection cannot revive explicitly cancelled
output. Runtime receipts expose no route, private owner room or transport token.
See [ADR-033](../knowledge/decisions/adr-033-native-final-reply-custody.md) for the
remaining verified-ingress, taskless, room-admission and live-transport boundaries.

The `hagency-runtime` crate implements bounded Codex 0.153.4 JSONL protocol state
and an asynchronous driver for host-owned streams. A complete write and flush is
a transport receipt only. Cancellation, timeout, EOF and queue pressure close the
connection with unresolved progress; they prove no process cleanup or task result.
Protocol requests and events cannot grant approval. The crate remains unlinked
from Agent execution; typed session, sandbox and guardian integration are open.
See [ADR-032](../knowledge/decisions/adr-032-native-codex-protocol.md) and
[ADR-034](../knowledge/decisions/adr-034-native-codex-transport.md).

## Native MCP task maintenance and coordination

`hagency mcp` serves twenty tools over stdio. Its six task tools are get_task, accept_task,
transition_task, comment_task, update_task_execution and complete_task_with_reply. The host must supply
the same inherited HAGENCY_RUNNER_API_ADDR, HAGENCY_RUNNER_CAPABILITY and
HAGENCY_TASK_ID context as the native task CLI. Each call names that assigned
task; mutations additionally require a stable call_id, preserved for an exact
retry across MCP connections. State changes go through the existing scoped API.

This helper advertises MCP 2025-11-25 tools only. It bounds newline frames and
request IDs, serializes tool calls, and uses a private executable watchdog for
partial input and blocked output. Exit 74 means its IO deadline expired; it does
not prove that a submitted mutation failed. Inspect or repeat the exact call ID
and content through the canonical writer. The watchdog is not linked as a public
library service function. The pinned Rust MCP SDK is a test-only dependency.

Fourteen additional tools expose delegation, internal conversations, peer inboxes
and canonical task graphs through the same scoped API. Conversation participants
and graph assignees use exact internal session IDs; delegation uses active
engagement IDs. Mutations preserve caller-supplied stable call IDs for exact
content-bound replay. Graph completion still requires the canonical Done epoch.
Pages default to eight items with a maximum of 32; ordinary task-maintenance
client requests are capped at 16 KiB, completion/coordination client requests at
32 KiB, and HTTP responses at 64 KiB. The completion handler separately limits
its decoded final body to 32 KiB. Unknown
mutation outcomes remain unknown. See
[ADR-051](../knowledge/decisions/adr-051-native-mcp-coordination.md).

The host-generated owned-runner configuration enables four fixed task tools:
get_task, update_task_execution, transition_task and complete_task_with_reply.
It keeps on-request approvals, workspace-write and the default disabled network
policy. See [ADR-060](../knowledge/decisions/adr-060-native-owned-completion.md)
for completion receipt replay and retained-owner cleanup. Discovery, files,
Matrix history, approval tools and live rollout remain separate work; deployed
MCP configuration is unchanged.


Schema 15 and [ADR-047](../knowledge/decisions/adr-047-native-matrix-transport.md)
add authenticated Matrix observation collection. Exact whoami account/device
matching precedes encrypted SDK bootstrap. Bounded HTTPS sync and full room state
feed the domain writer, which atomically fences stale sessions, grants and
possible notice/final sends after negative transport or shared-room evidence.
A stale failed request cannot retire a newer incarnation.

The collector owns SDK state, crypto and an explicitly encrypted sync journal.
An interrupted pending response is retained and requires inspection; completed
cursors are not assumed to prove SDK application. The current bounds are 16
host-pinned rooms and 64 completed sync receipts, with finite HTTP/storage/queue
budgets. Capacity exhaustion is visible. Room-set rotation, pending-sync recovery,
continuous retention, event provenance, key publication and encrypted sends remain
separate gates. No live device or service is connected by this checkpoint.


The [native formatter](../knowledge/decisions/adr-050-native-matrix-formatting.md)
preserves Matrix text bodies, relations, mentions and edits while producing
allowlisted HTML. Sixty-three vectors run against the retained JavaScript
implementation. Supplied truthy formatted HTML is caller-trusted passthrough,
not sanitized input. Syntax, JSON and output limits fail visibly; this pure
content library does not authenticate, route or send an event.

[Linux cgroup recovery](../knowledge/decisions/adr-048-native-linux-crash-custody.md)
is an optional host-provisioned capability. It requires protected, exclusively
reserved cgroup descriptors, initial user/cgroup namespaces, source-qualified
kernel families and a restricted host. The CI-only root provisioner creates
one disposable subtree and independently cleans it after real fault fixtures.
No runtime provisioner or service enablement is supplied. Closing descriptors
does not itself kill processes; simultaneous host/guardian crash containment
remains Unsupported, and a partial cleanup observation never releases a domain
lease or completes a canonical task.

The [native metering parser](../knowledge/decisions/adr-055-native-metering-parsers.md)
normalizes bounded, supplied Claude/Codex transcript snapshots. It preserves all
four token categories, excludes cache reads from ceiling arithmetic, and exposes
missing or contradictory evidence. Its 135 vectors execute the retained JS
parser; separate correction fixtures reject coerced/unsafe numbers, duplicate
keys and conflicting message identities. This library does not read transcript
files, authenticate usage, attribute it to an Agent/project, or enforce a quota.

[Schema17 usage custody](../knowledge/decisions/adr-063-native-usage-ledger.md)
adds exact host dispatch attribution, per-source high-water observations and
atomic observed UTC periods. It retains incomplete evidence and content-bound
receipts without pruning/recounting old identities. Finite capacity refusal is
explicit. [Operator aggregate reads](../knowledge/decisions/adr-067-native-usage-read-api.md)
preserve nulls and historical lower-bound labels and expose no transcript/source,
task, private room or workspace records. Secure transcript capture, provider
measurement, quota enforcement and the browser console are still separate work. ADR069 now retains optional upstream
usage counters from the exact native driver source and sequence, with explicit
missing/invalid/future evidence. It does not yet bind them to the usage ledger.

[Native progress policy](../knowledge/decisions/adr-052-native-progress-policy.md)
binds bounded event receipts and throttled redacted summaries to one immutable
host run. Tool titles, arguments and errors never enter outgoing text. ACP calls
with no terminal result remain explicitly pending or unresolved; a claimed
pending snapshot cannot hide a later completion. Unknown submission outcomes
block retries until host inspection. This is an in-memory projection, with 275
unchanged JS vectors and 20 documented corrections. ADR056 supplies an exact
native-driver attachment; domain-worker binding, durable status delivery and
actual Matrix edits remain separate integration work.

[Owned dispatch execution](../knowledge/decisions/adr-053-native-owned-dispatch.md)
connects an exact claimed capability and frozen writer scope to real native
Codex pipes. The writer commits Started before child creation; an unknown start
receipt never launches work. Cancellation retains the worker and process owner,
and incomplete cleanup keeps resource leases quarantined. Upstream completion,
canonical Done, process cleanup and dispatch settlement remain separate results.
The default service keeps execution disabled; the explicit development bootstrap
now connects one supported attempt. Physical workspace protection, effective
sandbox qualification and approval application remain required before operational
use.

[Actual Matrix sending](../knowledge/decisions/adr-059-native-matrix-outgoing.md)
uses the existing frozen notice/final claims and performs account, full-room and
recipient-key checks. Domain send custody and encrypted SDK journal records
precede writes; accepted responses persist before domain acknowledgement. A
retained accepted result can settle after restart without resending. A possible
write without an accepted response remains uncertain. Current bounds include
one outgoing attempt and 64 retained receipts per collector; automatic retention
and unknown-write recovery are unfinished. Private rooms require encryption and
the complete current verified device set; missing trust or Olm sessions refuses
the send. Fixtures exercise real SDK ciphertext and local HTTPS, not live account
provisioning or autonomous service operation.

## Retained native usage console (ADR107)

The opt-in console serves the existing usage page, preferences and English/Chinese
presentation through Salvo. It reads the typed engagement ledger. Null periods,
incomplete evidence, regressed latest counts and historical lower bounds remain
visible; it does not provide fleet totals, provider billing, quota enforcement,
dynamically provisioned Agent detail pages or the remaining console workflows.
ADR108 adds resource observations and explicitly scoped native catalog actions below.

Build assets with Node as a development tool, then use only the native executable
for serving and access. The asset output must be a new directory. Its real path
must contain no symbolic-link components and its root must be private to the
current owner. An existing Next font cache can be supplied with `--font-cache`
to replay the retained layout's actual downloaded font bytes without network.

```sh
node mockup/scripts/build-native-console.mjs --output /absolute/new-console-assets
hagency serve --state-dir /absolute/native-state --console-assets /absolute/new-console-assets
hagency console-access --state-dir /absolute/native-state
```

Open the printed link within two minutes. The browser removes its fragment and
exchanges that one-use read-only ticket for a 15-minute HttpOnly session. The
operator token remains in the private state directory and never enters browser
assets, JavaScript storage or the access link. Reissuing a link invalidates the
previous outstanding ticket; issuance is limited to one per second, and at most
four sessions coexist. End access confirms revocation only after success; a pending
resource action can return Busy and the browser offers an explicit retry. This is an opt-in
loopback HTTP development profile, without production configuration changes.

The startup manifest allows at most 512 assets, 4 MiB each, 32 MiB total and 128 KiB of
manifest JSON. Every actual source snapshot and digest is retained; later HTTP
requests serve only captured bytes. A build exceeding a limit fails explicitly.
The usage-and-resource artifact contains 136 assets totaling 6,069,438 bytes,
with a largest asset of 239,884 bytes.
Build staging and failed outputs are retained
for diagnosis; neither the builder nor runtime trims required chunks to fit.

Ordinary Cargo runs execute browser authority, file and usage tests without Node.
The separate mandatory browser CI job enables the explicit feature. Its local
equivalent is below; missing prerequisites fail in that enabled lane.

```sh
export HAGENCY_NATIVE_CONSOLE_ASSETS=/absolute/new-console-assets
export HAGENCY_BROWSER_NODE=/absolute/path/to/node
export HAGENCY_BROWSER_CHROME=/absolute/path/to/chromium
cargo test --locked -p hagency --features native-console-browser --test console -- --nocapture
```

The real browser fixture creates another canonical engagement while Chromium is
open, then verifies runtime selection and reload of its query URL. A separate
real executable fixture starts `hagency serve` and `console-access` with an empty
runtime PATH and drives that server from Chromium. Node/Chromium remain external
build/test tools; there is no deployed Next server. These local browser results
do not qualify untested operating systems or complete the remaining M7 workflows.


## Retained native resource controls (ADR108)

The existing resource page now reads safe configured profiles, included/withdrawn
catalog status, actual selected pool/shared-account allocation and native role
observations. Null limits and missing periods remain unknown. These are local
configuration/allocation facts, not provider usage, installed runtime readiness or
proof of delivery to Palpo. Selection uses `/console/resources/?resource_id=...`
and works for rows created after the static artifact was built.

Ordinary console tickets remain read only. To grant only native catalog visibility
changes for the finite session, request the explicit scope:

```sh
hagency console-access --state-dir /absolute/native-state --manage-resource-publication
```

The retained Include/Withdraw control compares the exact actual configuration
revision and changes publication and its derived roles only. Conflicts require a
fresh read. Lost responses remain unknown; reading current state does not prove
which action caused it. Writes never retry automatically. Busy or unknown logout
clears observations but keeps unresolved revocation visible with a retry button.

The store retains a concrete command from the original management session before
queue admission. SQLite IMMEDIATE precedes a nonblocking per-session mutation
gate; the global session map is never held across disk IO. Original deadlines,
session expiry and retirement are checked after SQLite waits and around commit.
Read-only sessions cannot construct commands. No internal account/preset identity,
full resource configuration, credential or auth-home enters browser DTOs.

First-resource enrollment, names, rate caps, execution policy, host discovery, Agent
routes, account membership/authentication and remote publication remain separate
migration slices. ADR108 is the first native resource management step, not full M7
parity. The mandatory browser lane above includes both resource selectors; it
exercises actual publication in both languages and a native executable with an
empty runtime PATH. It does not use live configuration or a deployed Node server.


### Additional resource configurations in the retained wizard (ADR111)

`hagency console-access --state-dir <state> --listen <loopback-address>
--manage-resource-configuration` issues a separate finite configuration link.
This flag and `--manage-resource-publication` are mutually exclusive. The default
link remains read-only. A configuration grant cannot change existing catalog
inclusion, edit accounts or use the closed operator API.

The original four-step resource wizard is served at `/console/resources/new/`.
`?source_resource_id=<public-resource-id>` creates another configuration from the
source's actual private account association; `?resource_id=<public-resource-id>`
edits the selected configuration. Dynamic IDs require no asset rebuild. An empty
installation has no source and explicitly refuses first-resource enrollment.
Model/reasoning choices come from the same embedded native qualification policy.
These choices are configuration facts, not authentication or runtime readiness.

Native writes accept only model/reasoning selection and explicit ceiling
`preserve`, `clear` or `monthly` operations. Preserve keeps exact null and missing
period semantics. Framework/provider/private account identity stay on the writer.
Creation generates a fresh private preset identity once and inserts a new resource;
its local publication flag defaults to true in that commit. Editing retains the
original IDs and withdrawal choice. Reserved/active profile changes refuse; ceiling
changes preserve all commitments and account declarations. No provider quota,
consumption, approval, runtime launch or remote publication is established by save.

Every command owns the original finite session gate and two-second deadline.
The original writer checks the actual configuration revision after SQLite waiting,
then changes only the selected fields. It never holds the global session map across
IO. Each grant holds one mutation-scope variant. Busy logout has an explicit retry;
possible commits and lost create responses stay unknown without automatic retry.
An unknown create has no durable deduplication journal: a later list is current
state, not proof of that command. Browser drafts are memory-only, retain their old
revision during same-selection refresh and clear when identity/access changes.

Friendly names, daily rate caps, execution policy, endpoints/keys/extra arguments,
credential roots and account enrollment are absent from native form inputs. They
need canonical managed-profile persistence and actual native consumers. First
resource enrollment specifically needs a separately contracted host-only versioned
account binding derived from normalized host, actual credential namespace and key
scope, consumed by the actual runtime. The existing Seat ledger or a browser model
selection cannot supply that missing binding. Complete M7 and production cutover
remain open.

New HTTP surfaces are bounded same-origin `/console/api/resources/{id}/configuration`
GET/PATCH and `/console/api/resources` POST. The browser sends a public source/target
ID, expected revision and closed change unions; no authority object, private preset
ID or seat ID is accepted. Input is 2 KiB, output 64 KiB and model choices at most
256; existing finite resource/page/asset limits remain unchanged.
