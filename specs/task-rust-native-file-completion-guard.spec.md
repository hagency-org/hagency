spec: task
name: "Refuse dispatch completion while a file delivery is unsettled"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-EXECUTION-AUTHORIZATION]
tags: [active, rust, files, ownership, testing]
---

## Intent

Correct the original 1baa80d Linux and Windows CI failures of
`native_file_service_uncertainty` and `recovery::native_file_service_restart`.
Both expect `outcome_unknown` with a negative settlement after a possible or
refused file write. macOS reached that state only through its unqualified
process-tree census (`cleanup_unknown`); Linux observed complete cleanup, the
peer's turn completed normally, and the domain writer completed the dispatch
although its file delivery was still `write_possible`.

## Constraints

- The domain writer, never the driver or a test seam, refuses completion while
  any file delivery of the dispatch is unsettled: not `delivered` and either
  without a recorded failure or still a possible external write
  (`write_possible` event or upload, or an `outcome_unknown` upload).
- The refusal is `Error::State`; the owned operation records it as its existing
  `SettlementUnknown` failure and negative observation. No new schema, failure
  variant, deadline, retry or delivery mutation is introduced.
- Apply the same guard to host completion, runner completion and the explicit
  `complete_task_with_reply` hold and publication paths.
- A delivery whose recorded failure precedes any possible write is settled; it
  does not block completion. A cancelled possible write stays unsettled.
- No live services, dependencies, runtime protocol or production cutover.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/file_delivery.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain/owned_completion.rs
- native/hagency-store/tests/file_delivery.rs
- specs/task-rust-native-file-completion-guard.spec.md
- knowledge/decisions/adr-117-native-file-completion-guard.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- All paths outside this exact partition and all live operations.

## Acceptance Criteria

Rule: unsettled-file-write — Ordinary completion never promotes a possible external file write

Scenario: Domain writer refuses completion until a delivery is delivered or failed
  Test: native_file_delivery_completion_guard
  Given a dispatch whose file delivery is reserved or write_possible
  When the runner or host completes the dispatch
  Then the writer refuses with State until the delivery is delivered, and a cancelled possible write never settles it

Scenario: Possible upload or event write keeps the native attempt unknown
  Test: native_file_service_uncertainty
  Given an unclean upload or event response with no complete server ACK
  When the peer's turn completes and the driver settles the attempt
  Then the state is outcome_unknown with a negative settlement on every platform

Scenario: Refused domain settlement keeps the native attempt unknown
  Test: native_file_service_restart
  Given an injected refusal of the delivered settlement
  When the driver settles the attempt
  Then the state is outcome_unknown or unavailable with a negative settlement

## Out of Scope

Windows file-service startup observation, Windows approval fixtures and the
macOS process-tree census remain separate gates. Local Linux reproduction in a
container does not replace the hosted Ubuntu and Windows runs.
