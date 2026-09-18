spec: task
name: "Port correlated Codex MCP approval through the native owner path"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, codex, approval, parity]
---

## Intent

Port the working TS `runCodexDispatch` native MCP elicitation handling. The real
local Palpo/Robrix file task exposed a Rust parser failure before file admission.
Use the installed 0.154.0 schema and existing TS approval tests as references.

## Constraints

### Must
- Correlate an empty form MCP tool approval with exactly one active, unconsumed item in the exact thread and turn, server and structurally equal arguments.
- Retain finite item and payload bounds, reject repeated, stale, ambiguous and substituted items, and refuse a late response after completion.
- Bind the original wire request and correlated tool identity/arguments into the existing durable owner request; unsupported reusable scope permits only Once or Deny.
- Keep the original callback, owner deadlines, durable grant consumption, response writer, resolution and process cleanup pipeline.
- Allow an explicit startup owner wait within the original operation budget instead of requiring the fixed one-second development wait; retain that default for existing profiles.
- Add deterministic offline coverage before repeating the separately operated real Palpo/Robrix file path.

### Must Not
- Do not autoapprove file tools, parse display text as authority, invent an item ID, grant reusable MCP scope, widen the sandbox, or infer task completion from a tool result.
- Ordinary tests must not contact live providers or Matrix.

## Boundaries

### Allowed Changes
- native/hagency-runtime/**
- native/hagency-execution/**
- native/hagency-store/tests/**
- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/bootstrap*
- specs/task-rust-codex-mcp-approval.spec.md
- knowledge/decisions/adr-160-native-codex-mcp-approval.md
- docs/**

## Acceptance Criteria

Scenario: One native MCP tool request remains exactly correlated
  Test: native_codex_mcp_approval_correlated_once
  Given one active scoped MCP item and an empty tool approval form
  When a native request is parsed and the host responds
  Then the original native response is accept or decline with null content and metadata
  And the host request binds the exact original request and correlated tool call

Scenario: Unsupported forms and ambiguous or stale calls cannot gain approval
  Test: native_codex_mcp_approval_rejects_uncorrelated
  Given missing repeated completed ambiguous or substituted items and malformed forms
  When the native adapter handles them
  Then it fails visibly without an accepting response

Scenario: Completion while owner approval waits prevents late acceptance
  Test: native_codex_mcp_approval_completion_invalidates_response
  Given the original correlated callback and a prepared owner response
  When its tool item completes before the response is sent
  Then the original operation refuses the response and cannot rearm the item

Scenario: Existing domain custody remains once-only for MCP
  Test: native_owned_mcp_approval_once
  Given the owned runner and private owner approval coordinator
  When an authenticated owner selects Once for the exact MCP request
  Then the durable application and exact native response complete through the original owner
  And Task and Always are not offered for an unrepresentable reusable scope

Scenario: A configured owner wait still fits original custody
  Test: native_bootstrap_approval_wait_bound
  Given an explicit owner wait and finite operation and response budgets
  When startup constructs the approval host
  Then zero overflow and waits exceeding the original operation budget are refused

## Out of Scope

Generic form or URL elicitation, new grant categories, automatic file approval,
Claude/Octos adapters, full migration acceptance and live services in tests.
