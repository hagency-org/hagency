spec: task
name: "Deliver scoped peer inputs through the native domain writer"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, mailbox]
---

## Intent

Deliver internal Agent requests and responses to exact conversation sessions,
preserving dispatch input ownership and canonical task completion rules.

## Constraints

### Must
- Derive peer source identity from a current started runner capability and its conversation scope.
- Limit recipients to current internal participants or the exact original creator session.
- Commit one content-bound message receipt and all independent recipient projections atomically.
- Preserve bounded arrival-ordered pages finite numeric data and non-acknowledging reads.
- Freeze peer input ownership with dispatch enqueue and acknowledge only that dispatch's recipient projection on completion.
- Retain input for uncertain work and transfer it only during explicit inspected recovery.
- Let requests and responses wake incomplete work while notifications remain context.
- Keep Matrix activation and original-human follow-up gates; peer traffic cannot reopen completed Matrix tasks.

### Must Not
- Do not trust runtime source sender owner or registration fields.
- Do not permit a peer recipient to consume another recipient's copy.
- Do not infer canonical task completion from a peer response or runner output.
- Do not send live Matrix messages or launch real models.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-peer-mailbox.spec.md
- docs/**

### Forbidden
- Live services, deployed state and credentials.

## Acceptance Criteria

Scenario: Peer admission is scoped and atomic
  Test: native_peer_admission_scope
  Given current conversation participants foreign sessions and an injected projection failure
  When peer messages are sent and retried
  Then source identity and exact recipient scope are enforced and every projection commits with its receipt or rolls back

Scenario: Peer dispatch owns only its frozen input
  Test: native_peer_dispatch_ownership
  Given independent recipients and request response or notification input
  When pages are read and dispatches enqueue and complete
  Then reads never acknowledge input and completion acknowledges only the owning recipient's frozen batch

Scenario: Uncertain peer work retains its input
  Test: native_peer_input_recovery
  Given a started peer dispatch new arrivals and a repository restart
  When the host inspects and replaces uncertain work
  Then old inputs transfer without swallowing new messages or accepting stale capabilities

Scenario: Peer replies resume only incomplete Matrix work
  Test: native_peer_matrix_continuation
  Given pending active or completed Matrix task bindings
  When a conversation participant replies to the original creator session
  Then activation and original-human follow-up requirements remain enforced

Scenario: Private API preserves peer authority
  Test: native_runner_http_peer_mailbox
  Given a current runner and untrusted peer request fields
  When the private API sends and reads peer input
  Then the writer obtains current authority time and rejects forged source identity

## Out of Scope

Graph storage and atomic graph/task/message linkage, group membership lifecycle,
real Matrix transport, native processes and public operator UI remain subsequent work.
