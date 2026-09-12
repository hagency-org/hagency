spec: task
name: "Proposed native receive-file workflow from original ingress to Started workspace"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [proposed, rust, matrix, attachments, mcp]
---

## Intent

Propose the bounded development receive_file workflow specified in ADR105.
This contract is not accepted and authorizes no implementation. Its fourteen
selectors are planned tests, not coverage claims. Before code root reviews the
interfaces and partitions, accepts the required prerequisite contracts and records
the actual final boundary. Real executable integration is mandatory for completion.

## Decisions

Follow [ADR105](../../knowledge/decisions/adr-105-native-receive-file-workflow.md) and
preserve accepted ADR027, ADR073, ADR074, ADR076, ADR093 and ADR101 semantics.
ADR102 fresh enrollment is a prerequisite to actual first-use ingress evidence;
no service SDK fixture seeding may substitute for it. The proposed schema021 number
must be checked against the integration branch before implementation.

### Required implementation validation

Parse and lint before implementation, then run each exact selector against the
actual edited tree and inspect nonzero counts. Run existing selected-inbox and
attachment visibility, native Matrix receive, owned workspace, MCP, task-client,
bootstrap and HTTP regressions. Run warnings-denied Clippy for all affected crates
and final workspace tests after integration. Keep platform results distinct;
Windows GNU compilation is not runtime or durable-cache evidence.

Run strict lifecycle with the complete actual changed-path union, retaining every
fail skip and uncertain verdict. Preserve the preexisting knowledge-lint findings.
Do not run lifecycle now against nonexistent selectors and call the proposal
verified. No promotion or production activation precedes the actual executable
and all required platform qualification results.


## Constraints

### Must
- Run one real intake and deterministic wake-bearing inbox selection for an existing canonical task and session before the actual host-compatible claim.
- Preserve the selected trigger frozen source and projection windows privacy floor full sender identity and original current route.
- Restrict the configured intake plan to its exact dispatch without weakening other claim predicates or changing the actual transaction clock.
- Expose safe current-only attachment discovery and receive_file through actual Host enabled_tools inherited context native MCP and authenticated runner routes.
- Capture a single job deadline before admission and apply the configured lower byte bound before download allocation.
- Keep the actual checked result original ticket writer capability and physical workspace binding through queued sink work.
- Revalidate current original ticket and workspace after download after the sink queue immediately before file effects and after readback before path output.
- Reserve original immutable cache facts before GET and commit WritePossible before any destination creation.
- Generate one portable destination component and use retained-root create_new private permissions nofollow regular single-link checks and exact retained-file comparison.
- Require complete hash-verified bytes readback and acknowledged file and directory sync for Ready.
- Retain unknown write jobs source objects destinations and quota across caller loss timeout unwind and unsuccessful close.
- Revalidate current original authority and rehash the held original destination before Ready replay returns a path.
- Release checked bytes and live write-job quota after Ready while keeping bounded original Ready file owners and readonly scope for replay under a fresh read-only response deadline.
- Prepare and retain the one-shot workspace receive owner before effects and keep its created file through error or unwind.
- Preserve negative observations retirement and historical facts without granting a new download write or path disclosure.
- Bound this development profile to two live jobs thirty-two permanent records eight per workspace and 128 MiB reserved output with at most four MiB per file.
- Distinguish actual OS qualification known refusal missing tests and unknown outcomes from passing executable file receipt.

### Must Not
- Do not accept room URL MXC key descriptor root destination path capability or verification assertions from the tool caller.
- Do not treat file metadata content a helper marker a ticket digest or a cache record as authority.
- Do not use a copied writer duplicate Collector imported service crypto seed or fixture-created Started binding to satisfy the executable scenario.
- Do not follow links seal existing files truncate overwrite unlink uncertain entries or automatically create a replacement destination.
- Do not redownload on replay after losing original job custody or reset quota after restart.
- Do not expose a path through historical-only authentication or treat Ready as Delivered or Done.
- Do not widen deadlines claim synchronous filesystem cancellation or turn unavailable Windows qualification into positive evidence.
- Do not implement production scheduling cache cleanup plaintext attachments previews live runtime enrollment or a second state owner.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/media_download.rs
- native/hagency-matrix/src/lib.rs
- specs/task-rust-receive-file-prerequisites.spec.md
- specs/task-rust-received-file-identity.spec.md
- specs/task-rust-received-scope.spec.md
- native/hagency-store/tests/common/mod.rs
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/replies.rs
- native/hagency-store/tests/conversations.rs
- native/hagency-store/tests/usage.rs
- native/hagency-store/tests/file_delivery.rs
- native/hagency-store/tests/file_uploads.rs
- native/hagency-store/tests/owned_completion.rs
- native/hagency-store/tests/verified_ingress/notice_custody.rs
- native/hagency-store/tests/workflows/mod.rs
- native/hagency-store/tests/workflows/custody.rs
- native/hagency-core/src/attachments.rs
- native/hagency-store/src/domain/attachments.rs
- native/hagency-store/src/domain/messages.rs
- native/hagency-store/tests/verified_ingress/attachments.rs
- native/hagency-store/tests/messages.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/bootstrap/inbox.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/tests/owned_claim.rs
- native/hagency-core/src/lib.rs
- native/hagency-core/src/received_files.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain/received_files.rs
- native/hagency-store/src/migrations/021-received-files.sql
- native/hagency-store/tests/received_files.rs
- native/hagency-store/tests/schema_fixtures.rs
- native/hagency-matrix/src/receive.rs
- native/hagency-matrix/tests/intake/receive.rs
- native/hagency-execution/Cargo.toml
- native/hagency-execution/src/lib.rs
- native/hagency-execution/src/workspace.rs
- native/hagency-execution/src/workspace/received.rs
- native/hagency-execution/tests/owned.rs
- native/hagency-execution/tests/owned/receive.rs
- native/hagency-platform/src/lib.rs
- native/hagency-platform/src/file_identity.rs
- native/hagency/src/lib.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/workspace.rs
- native/hagency/src/receive_service.rs
- native/hagency/src/receive_service/job.rs
- native/hagency/src/receive_service/worker.rs
- native/hagency/src/receive_service/types.rs
- native/hagency/src/runner.rs
- native/hagency/src/runner/received_files.rs
- native/hagency/src/task_client.rs
- native/hagency/src/task_client/received_files.rs
- native/hagency/src/task_client/transport.rs
- native/hagency/src/mcp.rs
- native/hagency/src/mcp/catalog.rs
- native/hagency/src/mcp/receive_catalog.rs
- native/hagency-execution/src/host.rs
- native/hagency-runtime/src/codex/session/task_mcp.rs
- native/hagency/Cargo.toml
- native/hagency/tests/received_files.rs
- native/hagency/tests/received_files/fixture.rs
- native/hagency/tests/received_files/recovery.rs
- native/hagency/tests/fixtures/receive_mcp_peer.rs
- native/hagency/tests/mcp.rs
- native/hagency/tests/task_client/mcp.rs
- native/hagency/tests/http.rs
- ./Cargo.lock
- knowledge/decisions/adr-105-native-receive-file-workflow.md
- docs/design/task-rust-native-receive-file.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Original JavaScript services live state credentials unrelated schemas upload and publication journals Matrix trust policy runtime sandbox and guardian code are outside this proposal.
- Additional files or API prerequisites require an explicit contract amendment before implementation.

## Acceptance Criteria

<!-- lint-ack: bdd-rule-grouping — Fourteen independently bounded source and actual workflow gates retain an explicit integration requirement. -->

Scenario: Real intake produces one original frozen dispatch
  Level: integration
  Test Double: actual SDK intake canonical writer and real host claim
  Targets: native/hagency/src/bootstrap/inbox.rs
  Test: native_receive_inbox_selection
  Given an existing host-configured session task and workspace with unmentioned background uploads
  When one actual intake admits an addressed trigger or a direct upload and the host selects its inbox
  Then the exact trigger is retained within the bounded payload and later attachments remain outside its frozen window
  And unmentioned group-only input creates no dispatch and unrelated earlier queued work is not claimed
  And lost enqueue or claim replies do not resample input or manufacture another attempt

Scenario: Current discovery preserves attachment lineage and privacy
  Level: integration
  Test Double: actual canonical attachment projections selected dispatch and current runner commands
  Targets: native/hagency-store/src/domain/attachments.rs
  Test: native_receive_visible_context
  Given selected trigger background follow-up foreign later and pre-promotion attachment events
  When current attachment pages and exact ticket authorization query the original writer
  Then only the same visible event set is returned with bounded safe metadata
  And a trigger outside a bounded context page remains discoverable and expired or retired authority gets no page
  And metadata has no cache path descriptor key SDK identity or private transport values

Scenario: Cache facts bind every original request field
  Level: integration
  Test Double: real SQLite transactions original attachment ticket and dependent recomputed hashes
  Targets: native/hagency-store/src/domain/received_files.rs
  Test: native_receive_record_binding
  Given a current original ticket and complete request metadata with configured bounds
  When original reservation exact replay and coherent metadata scope or hash substitutions are committed
  Then only exact content replays and every changed association conflicts
  And writer time sampled after a held queue or SQLite wait refuses expired authority
  And schema migration preserves prior attachment and upload records without using them as local Ready

Scenario: Local write authority is issued only once
  Level: integration
  Test Double: real writer lost transaction reply and actual reopen
  Targets: native/hagency-store/src/domain/received_files.rs
  Test: native_receive_original_write_once
  Given a reserved original operation and actual checked byte facts
  When WritePossible or Ready commits but its reply is lost and original state is inspected
  Then inspection cannot produce a second write grant and incomplete work remains unknown
  And Ready evidence replay binds the exact original destination and immutable facts
  And receive status never changes upload event delivery or canonical task completion

Scenario: Checked results preserve their original writer across queue waits
  Level: integration
  Test Double: counted authenticated TLS real decryption original and stale copied writers
  Targets: native/hagency-matrix/src/receive.rs
  Test: native_receive_checked_result
  Given an actual authenticated manifest and admitted receive with a lower configured byte limit
  When GET or a later sink queue is delayed before final result revalidation
  Then the original writer ticket cancellation and initial absolute deadline still govern the checked result
  And a replacement writer cannot validate it and an oversized actual body fails before excess buffering
  And hash mismatch truncation redirect and wrong-origin attempts expose no plaintext result

Scenario: A sink writes only through the original physical root
  Level: integration
  Test Double: real owned Started operation physical files and controlled write checkpoints
  Targets: native/hagency-execution/src/workspace/received.rs
  Test: native_receive_workspace_sink
  Given a current original binding and bounded fully verified byte data
  When the sink writes or encounters root replacement links reparse points occupied destinations or retirement during actual IO
  Then success retains the exact created private single-link file with matching readback hash
  And refusal never follows a link overwrites an entry or returns an unverified path
  And retirement denies result and replay while possible partial effects and held ownership remain explicit

Scenario: Retained file comparisons preserve platform identities
  Level: integration
  Test Double: actual open files aliases and independent replacement files
  Targets: native/hagency-platform/src/file_identity.rs
  Test: native_receive_file_identity
  Given the original regular file a duplicate handle and a separately created object
  When platform comparison checks both retained objects
  Then only the duplicate original matches and all Windows volume and 128-bit file identity data are preserved
  And unsupported identity or file-type checks refuse without a pathname fallback

Scenario: Actual native MCP receives bytes from real encrypted ingress
  Level: integration
  Test Double: real native serve native MCP fixed protocol peer local TLS and independent SDK sender
  Targets: native/hagency/tests/received_files.rs
  Test: native_receive_executable
  Given fresh private service state original host provisioned canonical work and actual explicit enrollment
  When the independent sender supplies a real encrypted attachment through sync and the launched runtime discovers and receives its original event
  Then the actual frozen prompt names the selected input and enabled_tools and helper environment expose the intended receive tools
  And one authenticated bounded GET yields a generated relative file read by the same runtime workspace with exact plaintext size and hash
  And direct and addressed group cases preserve origin and task remains not Done
  And no service crypto seed raw capability registration or visibility setter substitutes for the actual path

Scenario: Transport authentication cannot become historical byte authority
  Level: integration
  Test Double: actual loopback requests native MCP contexts and canonical retirement mutations
  Targets: native/hagency/tests/received_files.rs
  Test: native_receive_authority
  Given original current expired retired foreign and same-localpart impostor contexts
  When discovery receive and cached replay are requested before and after privacy promotion
  Then only a currently valid original binding can reach its next media or filesystem effect
  And old receipt or helper flags cannot disclose a cache path and an unrelated event performs no GET
  And malicious filename and content remain user data and never become path selection or instructions

Scenario: Bounded replay keeps the original bytes and destination
  Level: integration
  Test Double: counted real TLS retained jobs actual modified files and permanent quota
  Targets: native/hagency/tests/received_files.rs
  Test: native_receive_replay_bounds
  Given two live operations or exhausted original disk record quota and a completed original file
  When exact requests replay a caller disappears or the existing destination changes
  Then live replay joins original custody Ready replay rehashes and changed files refuse without download or overwrite
  And new work refuses before GET when job record or byte quota is exhausted
  And caller drop new IDs and service restart cannot reset permanent quota

Scenario: Restart preserves historical facts without revived workspace authority
  Level: integration
  Test Double: real child process exit reopened original database and counted TLS
  Targets: native/hagency/tests/received_files/recovery.rs
  Test: native_receive_restart
  Given Reserved WritePossible and Ready records from actual prior receive operations
  When all original process owners are gone and a fresh service opens the same state
  Then incomplete original work remains unknown with no GET file creation replacement or deletion
  And old Ready metadata does not restore Started binding or authorize a returned path
  And a legitimate new follow-up dispatch must independently select the visible event and obtain current authority

Scenario: Interrupted download and write evidence remain distinct
  Level: integration
  Test Double: paused or truncated real TLS controlled actual sink write and canonical negative observations
  Targets: native/hagency/tests/received_files.rs
  Test: native_receive_uncertainty
  Given an original receive before and after possible local creation
  When authority expires cancellation occurs or a network or filesystem acknowledgement is lost
  Then no partial download becomes a file result and no possible write is silently reported absent
  And final current validation after queued work suppresses stale output and preserves original custody
  And identical calls never trigger a replacement write or hide the original unknown

Scenario: Receive close retains unfinished owners
  Level: integration
  Test Double: actual worker task unwind channel loss original destinations and shared Collector close
  Targets: native/hagency/tests/received_files.rs
  Test: native_receive_shutdown
  Given queued active and uncertain receive work together with original execution and send owners
  When close quiesces admission and retirement races an actual worker operation
  Then the exact original job receives uncertainty and its destination slot and source remain retained
  And no dead task remains queued and no closed receiver is repolled as a fresh success
  And one Collector closes only after all original owners acknowledge release

Scenario: Closed tool schemas and original context stay compatible
  Level: integration
  Test Double: real native MCP process and bounded loopback protocol decoder
  Targets: native/hagency/tests/task_client/mcp.rs
  Test: native_receive_protocol
  Given default send-only receive-only and combined host tool profiles
  When actual enabled tools inherited environment and tool arguments are exercised
  Then disabled receive tools are absent and fixed flags match actual helper catalog behavior
  And unknown duplicate oversized private authority and destination fields are refused
  And safe results validate event identity relative path fixed errors and complete size hash fields without private metadata
  And existing task and send tools retain their original schemas and deadlines

## Out of Scope

Continuous intake scheduling new task or session provisioning full conversation
history plaintext attachments image previews arbitrary destinations cache pruning
post-crash incomplete-write repair hostile same-UID namespace guarantees physical
power-loss qualification live Matrix tests and production cutover remain separate.
