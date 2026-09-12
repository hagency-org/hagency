spec: task
name: "Retain original attachment scope across bounded receive and cache reads"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, attachments]
---

## Intent

Implement the checked-result prerequisite from accepted ADR105. Preserve the
original writer capability and ticket after download; a later cache read must
revalidate that same association while releasing completed download memory.
This slice adds no local file write service MCP route or cache-path authority.

## Constraints

### Must
- Keep ReceivedAttachment and ReceivedScope opaque without public constructors Clone Debug serde or verification setters.
- Capture the original DomainStore capability ticket cancellation and absolute receive deadline and use them for live-result revalidation.
- Apply a caller lower byte limit before HTTP buffer allocation using the same shared downloader client codec and permits.
- Clamp the original deadline before the first queue and never renew it at SDK lookup HTTP or revalidation.
- Consume checked bytes and result permits when producing a read-only original ReceivedScope.
- Allow a fresh bounded read-only revalidation deadline only on that captured scope and never turn it into another download or write grant.
- Preserve current authorization revocation privacy and selected-window predicates and sample writer time after its queue waits.
- Keep original download API behavior and all existing strict transport and crypto checks.

### Must Not
- Do not accept a replacement writer capability ticket destination descriptor or source URL on retained-result methods.
- Do not add a new network client result pool retry background worker or dependency.
- Do not claim scoped metadata establishes filesystem custody or a completed incoming runtime workflow.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/receive.rs
- native/hagency-matrix/src/media_download.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/tests/intake/receive.rs
- specs/task-rust-received-scope.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- No domain schema execution workspace SDK trust HTTP framing MCP service or production configuration changes.

## Acceptance Criteria

Scenario: A checked result retains its exact current scope
  Test: native_matrix_received_scope
  Given actual encrypted SDK ingress and authenticated complete media bytes
  When the original result is revalidated and consumed into a read-only scope
  Then the actual original ticket remains associated and result permits are released
  And later original authority retirement refuses without another download

Scenario: Lower byte limits apply to actual authenticated transport
  Test: native_matrix_receive_lower_limit
  Given actual authenticated attachment manifests with oversized or understated metadata
  When a caller applies a smaller positive receive limit
  Then declared oversized input refuses before GET and actual oversized framing refuses without checked bytes
  And the same shared downloader permits recover after refusal and a bounded original object succeeds

Scenario: Read-only scope cannot renew the original receive deadline
  Test: native_matrix_received_scope_deadline
  Given a checked result produced before its original absolute deadline
  When that deadline or cancellation occurs and a fresh cache-read check is requested
  Then live-result revalidation refuses but read-only scope may check still-current original authority under its own bounded deadline
  And an already expired or cancelled read-only check refuses without network IO

## Out of Scope

Writing local bytes cache Ready admission directory sync application queueing and
actual runtime receive_file remain separate ADR105 gates. These tests use real
SDK and TLS but do not qualify the complete native executable receive workflow.
