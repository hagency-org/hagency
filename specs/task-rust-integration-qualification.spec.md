spec: task
name: "Record and validate the two-agent real-homeserver qualification evidence"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, qualification, evidence]
---

## Intent

Bind MA-M8b of ADR-144: the three claims an in-process fixture cannot honestly
prove — a foreign homeserver's membership/PL admission, E2EE against a second real
device, and the approval round-trip through a real owner client — are qualified by
an operator-run procedure whose record is a tracked evidence file, validated by an
always-present test that fails when the record is missing or partial, never skips
(the ADR-140 evidence class). One spec per subject; the in-process half is the
sibling spec `specs/task-rust-integration-acceptance.spec.md`.

## Constraints

### Must
- Keep the evidence record crate-side at native/hagency/qualification/two-agent.json, tracking the pinned artifact and version, the environment, one verdict per claim, and what remains unproven.
- Keep the validating test always present on every hosted leg and ungated; a missing, partial, stale or non-verdict record fails it.
- Keep the qualification itself operator-run against a real Palpo homeserver and a real owner client, with the runbook under docs/.
- State in the record which cells are qualification-only so the M8 exit gate is never read green from a hosted run.

### Must Not
- Do not treat a green validating test as proof the qualification ran; it proves the record's shape and verdicts, not the run.
- Do not gate the test by OS, feature or environment; do not skip on absence.
- Do not fake a foreign homeserver's admission or a second device's keys to make the record unnecessary.
- Do not add a hosted homeserver job; that is a service-lane decision.

## Boundaries

### Allowed Changes
- native/hagency/qualification/two-agent.json
- native/hagency/tests/qualification.rs
- native/hagency/tests/qualification/**
- docs/design/**
- specs/task-rust-integration-qualification.spec.md
- knowledge/decisions/adr-144-two-agent-integration-acceptance.md
- docs/progress.md

### Forbidden
- Live services inside tests, credentials in tree, deployed state.
- Any production crate change; any change to the sibling acceptance spec's selectors.

## Acceptance Criteria

Scenario: The qualification record exists, is pinned and carries a verdict per claim
  Test: native_two_agent_qualification_records_its_evidence
  Level: integration
  Test Double: the tracked evidence record file at its documented crate-side path
  Given the evidence record path named by ADR-144 and the pinned artifact and version
  When the validating test reads the record
  Then it exists, names the artifact and version, and carries one verdict for each of the three claims
  And a missing, partial, stale or non-verdict record fails the test never skips

## Out of Scope

The in-process two-engagement acceptance (the sibling spec), the operator procedure
itself (the runbook's own correctness is reviewed, not spec-bound), the outage and
recovery classes of M8 item 4, and any hosted execution of a real homeserver.
