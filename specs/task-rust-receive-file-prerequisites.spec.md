spec: task
name: "Native receive-file intake selection discovery and cache facts prerequisite"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, attachments, domain]
---

## Intent

Implement the accepted ADR105 prerequisite: one original writer transaction for
host-configured inbox selection and admission, exact host claim restriction,
current attachment discovery and immutable bounded cache facts. Sink application
MCP receive and executable end-to-end behavior remain incomplete separate gates.

## Decisions

Root approved eleven test-only schema reconstruction/version paths. Preserve all
historical input versions and old-data refusal checks; remove only received_files
when constructing a database older than021. The active boundary is36 exact paths.

Follow [ADR105](../knowledge/decisions/adr-105-native-receive-file-workflow.md).
The unimplemented whole-workflow proposal lives in docs/design. Only the seven
actual prerequisite selectors below enter root binding coverage. Rebase the clean
design checkpoint onto eb06c35 before source edits and preserve ADR102 bootstrap.
Run focused tests meaningful queue/transaction and lost-reply cases affected
attachment/inbox/claim regressions and warnings-denied Clippy. Run scoped strict
lifecycle with the exact actual changed-path union and preserve all failures.

## Constraints

### Must
- Validate the immutable host plan inspect the original dispatch ID select its wake-bearing input and enqueue frozen inbox under one original writer transaction.
- Share the existing enqueue transaction body without nesting or partially committing selection.
- Return the existing original frozen selection before querying new selectable inputs on replay and conflict on changed plan.
- Preserve trigger inclusion actual ingress provenance privacy floor selected source and projection windows and every existing compatible claim eligibility predicate.
- Add an optional exact dispatch restriction only to host-owned claiming and preserve post-wait current clock sampling.
- Derive cache request ticket original current execution and workspace association internally from the original writer.
- Reserve finite original facts before GET and issue a unique WritePossible grant only once before any future local destination creation.
- Distinguish correlation data from actual SDK and filesystem custody and keep private ticket or capability fields out of safe observations.
- Bind complete immutable metadata source route scope configured bound and captured size/hash with exact conflict semantics.
- Preserve historical safe inspection without returning path current capability or another write grant.
- Bound records to thirty-two globally eight per workspace and 128 MiB reserved configured byte limits and use existing bounded writer queues.
- Keep current byte and path authority separate from historical negative or receipt recording and never overwrite Ready with a negative.

### Must Not
- Do not create a second domain owner accept raw runtime routes roots descriptors proof fields or bypass verified ingress.
- Do not redownload overwrite truncate repair or issue a replacement write after original custody or acknowledgement is lost.
- Do not change upload or event delivery state or mark canonical tasks Done for local receive facts.
- Do not implement sink file IO receive MCP tools runtime presentation platform qualification cleanup or production activation in this prerequisite.
- Do not change deadlines retry policy binding checkers or unimplemented full-workflow selectors into passing coverage.

## Boundaries

### Allowed Changes
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
- knowledge/decisions/adr-105-native-receive-file-workflow.md
- docs/design/task-rust-native-receive-file.spec.md
- docs/agent-knowledge.md
- docs/progress.md
- specs/task-rust-receive-file-prerequisites.spec.md

### Forbidden
- Matrix SDK crypto and receive bytes execution physical workspace platform IO HTTP MCP task-client and executable fixtures remain assigned to the dependent owner.
- New source paths dependencies schemas beyond021 or changed active API require an explicit boundary amendment.

## Acceptance Criteria

Scenario: One original inbox selection survives replay and competing input
  Level: integration
  Test Double: real canonical writer and actual original transaction
  Targets: native/hagency-store/src/domain/messages.rs
  Test: native_receive_inbox_selection
  Given an existing canonical task session and workspace with background input followed by a wake trigger
  When the configured host selects admits and replays the same dispatch while later input appears
  Then one transaction preserves the trigger and frozen original input without resampling
  And no wake makes no dispatch and changed plan duplicate ownership or stale route refuses
  And forged copied input fails original provenance and stale privacy input cannot replay
  And escaped payload bounds retain the wake trigger or refuse without committing any input

Scenario: Attachment discovery matches current frozen authorization
  Level: integration
  Test Double: actual verified ingress projections and original writer
  Targets: native/hagency-store/src/domain/attachments.rs
  Test: native_receive_visible_context
  Given current original foreign later and retired attachment scopes
  When bounded pages enumerate visible attachment metadata
  Then only exact ticket-authorized source and projection rows are returned with no private fields
  And trigger and follow-up visibility remain intact while privacy floor and current route refusals remain effective

Scenario: Cache admission binds the complete original content
  Level: integration
  Test Double: real SQLite original scope and coherent dependent hash mutations
  Targets: native/hagency-store/src/domain/received_files.rs
  Test: native_receive_record_binding
  Given a current original attachment and current Started execution
  When reservations and captured facts replay or change metadata association size or hash
  Then only exact original facts replay and coherent changed facts conflict

Scenario: Current receive authority samples time after the original writer wait
  Level: integration
  Test Double: real bounded writer original transaction and expired lease
  Targets: native/hagency-store/src/domain_worker.rs
  Test: native_receive_original_clock
  Given an actual original reservation and a blocked finite writer queue
  When a write request waits until its original lease expires
  Then the current clock is sampled after both writer and SQLite waits and no write is granted
  And the original reservation remains unchanged with acknowledged writer shutdown

Scenario: Lost write replies cannot reissue local write authority
  Level: integration
  Test Double: actual writer commits lost reply and reopened state
  Targets: native/hagency-store/src/domain/received_files.rs
  Test: native_receive_original_write_once
  Given one reserved receive operation with captured byte facts
  When WritePossible or Ready commits and its response is lost before inspection
  Then no replay or restore issues another unique write grant and incomplete original work remains unknown
  And safe historical inspection and negatives cannot grant a path or change Ready to a failure
  And upload event delivery and canonical task state remain unchanged

Scenario: Permanent capacity survives caller loss and restart
  Level: integration
  Test Double: real bounded reservations original records and reopen
  Targets: native/hagency-store/src/domain/received_files.rs
  Test: native_receive_domain_capacity
  Given global workspace and byte reservation bounds
  When new or replayed operations arrive after unknown completion or process restart
  Then exact existing requests remain inspectable but new work exceeding any bound refuses
  And unknown records retain their original quota and changed IDs do not reset accounting

Scenario: Exact host claim restriction preserves existing eligibility
  Level: integration
  Test Double: real competing queued dispatches and current resource leases
  Targets: native/hagency-store/src/domain/execution.rs
  Test: native_receive_inbox_claim_restriction
  Given a selected dispatch and another compatible queued dispatch ordered before it
  When the host claims with the optional exact restriction or no restriction
  Then restricted claiming admits only the selected eligible original dispatch and never bypasses leases or route checks
  And default claiming behavior and its actual transaction clock remain unchanged

## Out of Scope

This prerequisite cannot establish actual incoming SDK-to-runtime receive behavior.
The fourteen whole-workflow scenarios remain proposed until the dependent checked
result sink application MCP and executable implementation exists and runs. Native
Windows positive durability real Codex sandbox behavior live compatibility and
production cache completeness remain separate qualifications.
