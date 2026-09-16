# Owner approval room manual review checklist

These checks require review of the runbook and separately recorded live evidence. They are not automated test selectors or a claim of GUI acceptance.

## Intent

Make the macOS E2E runbook require the actual membership, narrow Matrix state permission, and owner-button conditions needed to validate approval delivery. Prevent API verdicts and unrelated room markers from being reported as GUI approval acceptance.

## Decisions

- Read current membership and power levels from the owner room before changing it, then read power levels back after any change.
- Preserve existing administrators, defaults, users, and unrelated event levels; grant the current representative only the level needed for `com.agentchat.approval.room.v1` and `.v2`.
- Separate engagement resource allocation from native execution approval, and treat a missing native card or owner button as a failed GUI acceptance result.
- Record marker HTTP 403 separately from global Matrix 429 observations; two worker slots are not evidence of request pacing.

## Boundaries

### Allowed Changes
- docs/E2E-RUNBOOK-macos.md
- specs/task-e2e-owner-approval-room-precondition.spec.md

### Forbidden
- Do not change product code, runtime configuration, Matrix rooms, credentials, or test data.
- Do not document `state_default: 0` or an API verdict as an approval-card substitute.

## Acceptance Criteria

Scenario: Owner room preparation preserves unrelated authority
  Review: E2E-RUNBOOK-macos.md section 2.5
  Given an owner room whose default member level is zero and default state level is fifty
  When the operator prepares the current representative for marker publication
  Then the runbook requires fresh membership and power-level reads
  And it changes only the representative level and the two approval marker event levels
  And it requires a readback without lowering the global state default

Scenario: Missing GUI approval controls fail acceptance
  Review: E2E-RUNBOOK-macos.md sections 4 and 7
  Given engagement resources have been allocated and an actual agent task triggers a protected native tool call
  When the operator uses the owner approval card
  Then Approve once and Deny are checked against their exact request and digest
  Given the owner approval card or either button is absent
  When GUI acceptance is graded
  Then the result is failed
  And an API verdict is described only as a separate backend diagnostic

Scenario: Rate-limit observations stay qualified
  Review: E2E-RUNBOOK-macos.md section 7
  Given marker publication returns HTTP 403 and later traffic reaches a global HTTP 429 burst limit
  When the operator records the result
  Then the runbook distinguishes the two signals
  And it does not infer request pacing from a two-job concurrency bound

## Out of Scope

- Changing Matrix room state, implementing pacing, or validating a live GUI session.
