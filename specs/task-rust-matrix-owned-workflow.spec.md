spec: task
name: "Qualify authenticated Matrix input through owned native completion and final send"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, mcp, integration]
---

## Intent

Join the existing public native APIs in a deterministic offline workflow without
inventing delivery, trust, task completion or process-cleanup evidence.

## Constraints

### Must
- Authenticate Matrix input through actual local TLS and the owned SDK collector.
- Keep the original human thread root separate from the accepted notice event.
- Activate task inputs only after actual notice acceptance and freeze those inputs through the normal dispatch API.
- Invoke the real native MCP helper against the same canonical writer through an actually owned native child.
- Preserve separate canonical Done, upstream protocol, owner cleanup and final Matrix acceptance outcomes.
- Refuse final sending when macOS whole-tree cleanup remains unknown.
- Keep all fixture keys tokens accounts files and servers synthetic and disposable.

### Must Not
- Do not add production authority setters, schema, scheduler or service availability.
- Do not fake SDK encryption verification, Matrix delivery observations or StopReport.
- Do not interpret plaintext refusal as successful encrypted DM integration.
- Do not claim actual model execution, sandbox efficacy or physical workspace provisioning qualification.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency/Cargo.toml
- native/hagency/tests/owned_matrix.rs
- native/hagency/tests/owned_matrix/**
- specs/task-rust-matrix-owned-workflow.spec.md
- knowledge/decisions/adr-062-native-matrix-owned-workflow.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Authenticated input reaches explicit native completion and original thread delivery
  Test: native_matrix_owned_complete_workflow
  Level: integration
  Test Double: local TLS Matrix peer and offline app-server child invoking the real native MCP executable
  Given authenticated exact-MXID group input and a pending canonical task
  When actual notice acceptance activates the task and an owned native helper explicitly completes it with full final content
  Then the original thread and content reach actual final HTTPS only when leader_exited whole_tree_stopped and signals_accepted are all true while macOS keeps the final unsent
  And the test uses existing public APIs with no production authority setters schema scheduler or service availability changes
  And actual SDK intake HTTP responses and retained owner observations are used without fake SDK encryption verification Matrix delivery observations or StopReport

Scenario: Rejected or lost notice acceptance cannot start work
  Test: native_matrix_owned_notice_failure
  Level: integration
  Test Double: local TLS peer rejecting or losing the actual notice HTTP response
  Given a pending canonical task and real notice send
  When the server rejects the request or its response is lost
  Then task inputs remain inactive and no dispatch child or final send exists

Scenario: Plaintext cannot impersonate encrypted private input
  Test: native_matrix_owned_private_plaintext_refused
  Level: integration
  Test Double: local TLS Matrix peer returning plaintext with forged trust metadata in an encrypted DM
  Given the exact configured private pair in an encrypted room
  When plaintext claims to be verified encrypted input
  Then the event is durably rejected without retiring transport and no task child or final send exists

## Out of Scope

Autonomous production scheduling, live services or models, full encrypted DM plus
native helper execution, approval application, sandbox qualification, physical
workspace custody and overall migration cutover remain separate gates.
