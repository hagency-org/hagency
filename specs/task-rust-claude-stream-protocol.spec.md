spec: task
name: "Implement the bounded native Claude streaming protocol prerequisite"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, claude, protocol]
---

## Intent

Begin the native Claude adapter using the installed 2.1.270 CLI and the official
Agent SDK streaming envelope. This slice is framing, typed envelope validation
and fixed host-generated input only. It does not enable Claude in the production
Host before permission, task tools, usage and owned lifecycle are wired.

## Constraints

### Must
- Bound each JSONL frame to one MiB and partial-frame age to ten seconds.
- Reject duplicate JSON keys at every depth, excessive nesting, invalid UTF8,
  malformed identities and unsupported envelope kinds; close after a refusal.
- Pull at most one message per feed, including multiple frames in one read.
- Treat all decoded payloads as untrusted observations, never domain authority.
- Keep auto permission mode and a fixed stdio permission callback; no caller
  argument passthrough, bypass flags or ambient-key adoption.
- Generate bounded initialize, user and interrupt frames with exact request IDs.
- Reuse the existing strict JSON implementation without weakening Codex behavior.
- Keep ordinary tests offline and preserve explicit production Claude refusal.

### Must Not
- Do not add a permission-allow encoder or infer Done from a result message.
- Do not claim a byte buffer proves delivery, a successful model call or cleanup.
- Do not enable Claude dispatch, copy credentials, change account schemas,
  upgrade dependencies or contact live providers inside tests.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/lib.rs
- native/hagency-runtime/src/json.rs
- native/hagency-runtime/src/codex.rs
- native/hagency-runtime/src/codex/json.rs
- native/hagency-runtime/src/codex/wire.rs
- native/hagency-runtime/src/claude.rs
- native/hagency-runtime/tests/claude.rs
- specs/task-rust-claude-stream-protocol.spec.md
- knowledge/decisions/adr-154-native-claude-stream-protocol.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Stream framing is bounded and terminal on corruption
  Test: native_claude_stream_framing
  Level: integration
  Test Double: synthetic byte streams only
  Given split UTF8 CRLF concatenated frames and malformed inputs
  When the native decoder reads them with monotonic host timestamps
  Then one frame is returned at a time and duplicate keys excessive depth oversized frames expiry backwards time and partial EOF refuse without reopening

Scenario: Typed control and event envelopes preserve identity
  Test: native_claude_stream_envelopes
  Level: integration
  Test Double: synthetic official-SDK-shaped envelopes
  Given permission requests control replies cancellations and scoped result messages
  When the native parser classifies them
  Then original IDs and payloads are preserved while malformed or unsupported envelopes fail closed and result success is not canonical task completion

Scenario: Native input cannot widen execution policy
  Test: native_claude_stream_host_input
  Level: integration
  Test Double: host argument and frame construction without launching a provider
  Given selected model text session IDs and request IDs
  When native launch arguments and input frames are generated
  Then auto mode remains fixed and unsafe arbitrary options malformed IDs oversized prompts and flag injection refuse

## Decisions

ADR154 records the upstream sources and the remaining real-CLI/owned-Host gates.
The shared JSON helper retains the original Codex parser and its depth bound;
the existing Codex protocol tests must also pass after its move.

## Out of Scope

Production Claude admission, permission decision delivery, MCP configuration,
task completion, authenticated usage, process cleanup, Octos and live soaking.
