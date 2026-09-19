spec: task
name: "Persist native message admission and dispatch input ownership"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, messages, tasks]
---

## Intent

Connect admitted message history to canonical sessions and frozen dispatch input
in the native domain database. Preserve independent processing for multiple Agents
and make event retries and crash recovery atomic.

## Constraints

### Must
- Enforce one canonical session per allocation room and thread, including under concurrent resolution.
- Store each source event once with content-bound identity and independently project it into eligible sessions.
- Use stable total ordering and bounded pages; filtered reads must not acknowledge unseen messages.
- Freeze admitted input and claim its session projection in the same transaction as dispatch enqueue.
- Require a wake input before ordinary dispatch and expose only that dispatch's frozen inbox through its current capability.
- Mark inputs processed only for the completing dispatch session; retain uncertain input for inspected recovery.
- Keep source-event construction internal to authenticated transport adapters and reject inconsistent registration room or thread claims.

### Must Not
- Do not accept externally deserialized assertions of Matrix authentication.
- Do not acknowledge or discard pending input on persistence failure.
- Do not treat message admission or runner completion as canonical task completion.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-message-inputs.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- docs/**

### Forbidden
- Live state, deployed JS runtime, credentials and Matrix server changes.

## Acceptance Criteria

Scenario: Session and event identities stay canonical
  Test: native_message_identity_and_scope
  Given non-deserializable transport commands and matching or conflicting room thread registration and source-event identities
  When messages are admitted again or sessions are resolved concurrently
  Then one canonical session and one source event survive and inconsistent claims fail

Scenario: Message pages do not skip unseen work
  Test: native_message_order_and_pages
  Given events sharing or reversing external timestamps and different message kinds
  When bounded or filtered inbox pages are read
  Then committed arrival order is stable and unselected input remains pending

Scenario: Input ownership and dispatch commit together
  Test: native_message_dispatch_atomicity
  Given a queued wake with context and an injected enqueue failure
  When dispatch input is frozen or replayed
  Then admission input claims and launch payload remain atomic and content-bound

Scenario: Completion and recovery preserve per-session input
  Test: native_message_session_recovery
  Given the same event projected into two Agent sessions and an unknown dispatch
  When one session completes or inspected recovery creates a new dispatch
  Then the other session retains its input and unknown input is never blindly replayed

## Out of Scope

Actual Matrix SDK authentication, DM promotion privacy, room-history backfill,
task graph/delegation, task follow-up and reply delivery remain subsequent work.
This is an internal message/dispatch checkpoint, not full M3 or migration parity.
