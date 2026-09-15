spec: task
name: "Bind native child signals to kernel process identity"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, process-identity]
---

## Intent

Provide the kernel identity primitive needed by native descendant cleanup so an
expired process identity cannot authorize a signal to a replacement process.

## Constraints

### Must
- Construct public signal authority only from a host-owned Child, never a numeric PID supplied by a runtime.
- Use Linux pidfds Windows process handles and macOS unique identities plus version-checked audit-token signalling.
- Recheck macOS unique lifetime identity when exec changes its audit-token version.
- Keep read-only identity metadata separate from opaque signal authority.
- Distinguish a sent signal from no-longer-current identity; neither means canonical task completion.
- Verify lifetime expiry and unrelated process survival with real native fixtures.

### Must Not
- Do not fall back to pid-only signalling after a native identity check fails.
- Do not claim descendant discovery guardian cleanup or sandbox enforcement from this primitive alone.
- Do not start real models or modify deployed state.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-child-identity.spec.md
- knowledge/decisions/adr-029-native-process-scopes.md
- docs/**

### Forbidden
- Live services, credentials and the original dirty checkout.

## Acceptance Criteria

Scenario: Native signal authority targets its owned child
  Test: native_child_identity_signal
  Given a controlled child and an unrelated running child
  When its opaque identity authorizes termination
  Then the controlled child stops and the unrelated child continues

Scenario: Expired identities refuse replacement work
  Test: native_child_identity_expiry
  Given an exited and reaped child with a retained identity and another live child
  When the old identity is used
  Then it reports no current target and cannot signal the other child

Scenario: Native generation checks reject stale process versions
  Test: native_child_identity_generation
  Given a native process version that changes or mismatches the observed identity
  When the platform validates or signals through its generation guard
  Then stale metadata cannot supply authority and only the original process lifetime can be refreshed

Scenario: Child progress observations distinguish delayed scheduling from exit
  Test: native_child_identity_observation
  Level: integration
  Test Double: native child heartbeat with a host-controlled pause
  Given a live child that temporarily pauses its heartbeat without a signal
  When the fixture observes bounded progress and retained child exit status
  Then resumed work passes and an actually exited child still fails observation

## Out of Scope

Descendant discovery, native guardian startup and owner-crash handoff, process IO,
sandbox policy and actual runner dispatch integration remain required later work.
