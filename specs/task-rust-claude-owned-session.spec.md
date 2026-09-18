spec: task
name: "Connect Claude streaming IO to retained native process custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, claude, lifecycle]
---

## Intent

Drive the ADR154 codec through bounded asynchronous pipes and the existing native
process guardian. This is one disposable upstream conversation, not production
Claude admission, permission delivery or canonical task settlement.

## Constraints

### Must
- Correlate the single initialize response and reject unsolicited control replies.
- Admit one prompt only, bind events to the first system init session and refuse
  changed session identities, duplicate init or events before the prompt.
- Pump stdin stdout and stderr together under finite write event and lifetime
  deadlines; enforce partial-frame deadlines even when no new bytes arrive.
- Bound queued messages, total queued wire bytes and private stderr retention.
- Closing or cancelling any polled IO operation permanently closes its streams.
- Retain original native process ownership through errors cancellation and drop;
  propagate the platform's exact cleanup observation without upgrading macOS.
- Keep result observations separate from canonical completion and cleanup.
- Test only local synthetic peers and offline native fixture processes.

### Must Not
- Do not enable production Claude or add a permission response/allow path.
- Do not export credentials, inherit an ambient environment or launch live models
  in ordinary tests. The explicit host Launch remains responsible for policy.
- Do not change Codex runtime semantics or weaken process containment.
- Do not infer safe replay from zero accepted bytes or ignore partial writes.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/claude.rs
- native/hagency-runtime/src/claude/**
- native/hagency-runtime/src/owned.rs
- native/hagency-runtime/src/owned/claude.rs
- native/hagency-runtime/src/bin/claude_probe/mod.rs
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- native/hagency-runtime/tests/claude_session.rs
- native/hagency-runtime/tests/claude_owned.rs
- specs/task-rust-claude-owned-session.spec.md
- knowledge/decisions/adr-155-native-claude-owned-session.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Claude session correlates one disposable conversation
  Test: native_claude_session_identity_and_result
  Level: integration
  Test Double: bounded in-memory synthetic peer
  Given initialization prompt scoped events and result observations
  When the native driver consumes them
  Then only matching control and session identities survive and result has no task or cleanup authority

Scenario: Claude IO is bounded under stalls and cancellation
  Test: native_claude_session_deadlines_and_cancellation
  Level: integration
  Test Double: in-memory backpressure and deterministic paused clock
  Given stalled writes partial frames stderr traffic and dropped futures
  When a deadline expires or operation is cancelled
  Then streams close permanently with original byte progress and no retry capability

Scenario: Claude IO drains pipes without unbounded queues
  Test: native_claude_session_backpressure_and_capacity
  Level: integration
  Test Double: in-memory simultaneous stdin stdout stderr
  Given full-duplex traffic and excess queued frames
  When host writes a prompt
  Then peer output drains under count byte and stderr bounds or fails closed

Scenario: Claude retained process owner survives protocol outcomes
  Test: native_claude_owned_lifecycle
  Level: integration
  Test Double: native offline fixture process through actual platform pipes
  Given a locally owned peer and a result message
  When the host observes the result and then stops the original owner
  Then result alone retains custody and cleanup reports platform evidence without macOS promotion

Scenario: Claude retained owner stops on cancellation and malformed input
  Test: native_claude_owned_failure_and_cancel
  Level: integration
  Test Double: native offline fixture process through actual platform pipes
  Given a malformed or stalled owned peer
  When initialization fails times out is cancelled or the owner is dropped
  Then original ownership attempts bounded stop and never supplies fabricated completion

## Decisions

ADR155 defines the observation-only session and remaining permission/MCP/usage
and production admission work. ADR154 protocol selectors must still pass.

## Out of Scope

Production execution Host, permission decisions, MCP/usage integration, account
binding, new containment mechanisms, Octos implementation and live qualification.
