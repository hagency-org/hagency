spec: task
name: "Bind native internal conversations to current project authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, sessions]
---

## Intent

Support independent internal Agent conversations without pretending they are
Matrix rooms or creating a second task/dispatch state owner.

## Constraints

### Must
- Distinguish internal conversation routes from validated Matrix room and thread routes.
- Keep both route types under canonical session task and dispatch ownership.
- Create conversation identity participant bindings and idempotent receipt atomically under a current started creator capability.
- Restrict participants to current active allocations in the creator project and registration generation.
- Limit conversation access to its exact creator session or its bound internal participant session.
- Reject Matrix event admission and Matrix notice delivery through internal routes.
- Preserve canonical session uniqueness restart recovery and old schema migration checks.

### Must Not
- Do not invent Matrix room IDs for internal conversations.
- Do not accept runtime-supplied creator owner or registration identity.
- Do not report conversation creation as peer message delivery or graph execution.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-internal-sessions.spec.md
- docs/**

### Forbidden
- Live services and deployed state.

## Acceptance Criteria

Scenario: Conversation creation commits atomically
  Test: native_internal_conversation_atomicity
  Given current project participants and an injected participant write failure
  When a conversation is created and retried
  Then all session bindings and the content-bound receipt commit together or roll back

Scenario: Conversation authority is scoped
  Test: native_internal_conversation_authority
  Given expired parked foreign-project revoked or different-session callers
  When internal conversations are created or read
  Then only the current exact creator or bound participant session receives access

Scenario: Internal tasks retain independent ownership
  Test: native_internal_task_isolation
  Given two internal conversations involving the same Agent
  When canonical work is queued started completed and restarted
  Then tasks input authority and unknown-attempt recovery remain scoped to their own sessions

Scenario: Matrix ingress refuses internal routes
  Test: native_internal_matrix_separation
  Given internal routes and existing Matrix bindings across schema migration
  When Matrix messages or route data are admitted
  Then internal routes cannot impersonate rooms and canonical Matrix uniqueness remains enforced

Scenario: Private runner API cannot forge conversation identity
  Test: native_runner_http_conversations
  Given a current runner and untrusted conversation request fields
  When the private API creates reads and retries an internal conversation
  Then creator identity is host-owned and completed tasks cannot create new conversations

## Out of Scope

Peer mailbox delivery, graph storage and canonical graph linkage, local standalone
Agent provisioning, real processes and Matrix transport remain subsequent work.
