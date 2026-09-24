spec: task
name: "Native console workflow for inspected stopped tasks"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, console, recovery]
---

## Intent

Expose the existing ADR165 inspection and three explicit operator actions in the
native console, with bounded discovery of stopped dispatches per engagement.

## Constraints

- Keep the existing seven-field roster unchanged. Separate stopped-work reads
  require lifecycle authority and return only sixteen scalar rows per page.
- Show absence of original inspection explicitly. A listed receipt is historical
  evidence, not a promise that current custody permits resolution.
- Bind inspection, replacement and result to the selected original dispatch.
  Display task, workspace, inventory hashes and expiry before any decision.
- Keep the one-use secret in memory only. Never render, log, persist or place it
  in URLs. Request/body limits remain finite; only inspection permits up to 2 MiB.
- Require an operator note and a distinct instruction for continuation. Never
  issue mutations from effects or auto-retry a lost decision. Preserve the exact
  serialized pending decision for an explicit same-request retry on uncertainty.
- English and Chinese labels, accessible inputs and wrapped identifiers.
- Offline tests and real local browser fixtures; no live service from tests.

## Allowed changes

- native/hagency-store/src/domain/stopped_inspection.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency/src/console/agents.rs
- native/hagency/tests/console/**
- mockup/components/NativeAgents.jsx
- mockup/components/NativeStoppedWork.jsx
- mockup/lib/native-api.js
- mockup/lib/native-recovery.js
- mockup/lib/i18n.js
- mockup/scripts/native-console-browser.mjs
- tests/dashboard-native-recovery.test.js
- specs/task-rust-console-outcome-workflow.spec.md
- specs/task-console-outcome-protocol.spec.md
- knowledge/decisions/adr-170-console-outcome-workflow.md
- docs/**

## Scenarios

The browser-side decision protocol is a JavaScript module with a Vitest
selector, so its scenario lives in `task-console-outcome-protocol.spec.md`,
which the Node bindings check resolves. This spec binds Rust selectors only.

Scenario: Stopped-work discovery remains private and bounded
  Test: native_console_stopped_dispatch_list
  Given stopped dispatches across separate engagements
  When an operator pages the selected engagement
  Then only sixteen bounded rows and an exact cursor are returned
  And read-only sessions, malformed queries and foreign inspection are refused

Scenario: Lifecycle operators can review and resolve through the native browser
  Test: native_console_agent_lifecycle_browser
  Given the actual static console and local native service fixture
  When a lifecycle operator reviews stopped work and chooses an action
  Then the native API commits the chosen outcome and the browser renders its receipt
  And read-only sessions expose no recovery controls
