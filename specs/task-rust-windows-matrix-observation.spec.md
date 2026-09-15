spec: task
name: "Observe original Windows Matrix fixture failures without changing verdicts"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, windows, matrix, fixtures]
---

## Intent

Expose the original domain shutdown and HTTP script phases behind six observed
Windows Matrix library failures while preserving deadlines and error results.

## Constraints

### Must
- Use existing DomainStore shutdown_observed for the four known original failing shutdown call sites and retain its exact Result.
- Print only fixed labels bounded batch indices and the existing fixed shutdown or HTTP phase observations on failure.
- Drive the original intake bootstrap and explicitly observed attachment intake with existing common scripted error visibility while preserving unobserved intake helper behavior.
- Preserve every authentication restart negative-authority admission and corruption assertion.
- Keep original Windows7cf0dc0 failure and later diagnostic successes as distinct evidence.

### Must Not
- Do not edit production code change any timeout alter CI parallelism retry failed operations or infer historical causes from successful reruns.
- Do not print raw responses events routes secrets paths or private journal contents.

## Boundaries

### Allowed Changes
- native/hagency-matrix/tests/common/mod.rs
- native/hagency-matrix/tests/intake/mod.rs
- native/hagency-matrix/tests/intake/attachments.rs
- native/hagency-matrix/tests/approval_intake/mod.rs
- knowledge/decisions/adr-088-native-windows-matrix-observation.md
- specs/task-rust-windows-matrix-observation.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Approval framing refusal preserves original observed domain shutdown
  Test: native_matrix_approval_bounds_sync_framing_and_body_limits_do_not_advance_crypto_cursor
  Given real malformed bounded sync responses and an existing private SDK
  When original cleanup observes domain shutdown after SDK closure
  Then framing and crypto assertions remain unchanged and failed shutdown remains failure

Scenario: Historical rotation preserves original observed domain shutdown
  Test: native_matrix_intake_rotation_historical_commit_settles_without_new_projection
  Given a historical committed intake and replacement generation
  When exact replay and original domain shutdown complete
  Then no new projection occurs and shutdown failure retains its original phase snapshot

Scenario: Interrupted SDK custody preserves original observed domain shutdown
  Test: native_matrix_intake_sdk_custody_interrupted_apply_retains_exact_raw_and_targets
  Given an actual interrupted SDK application and encrypted retained custody
  When original cleanup observes the domain worker
  Then unknown intake and private-byte assertions remain unchanged and shutdown is not retried

Scenario: Corrupted SDK history preserves original observed domain shutdown
  Test: native_matrix_intake_sdk_custody_restore_checks_authenticated_journal_consistency
  Given altered protected identity target cursor or raw history
  When restoration is refused and the original domain shutdown runs
  Then expected corruption refusal remains intact and cleanup prints only fixed failure evidence

Scenario: Manifest bounds reveal original batch and HTTP phase
  Test: native_matrix_attachment_manifest_bounds
  Given two real64-event encrypted attachment batches
  When the existing intake returns failure or its scripted request stalls
  Then only bounded batch and fixed HTTP phase evidence is added without retry or admission changes

Scenario: An old inspector cannot retire the replacement incarnation
  Test: native_matrix_intake_rotation_old_inspector_cannot_retire_new_device_incarnation
  Given the original bootstrap followed by a newer device incarnation
  When the old inspector receives wrong-account evidence
  Then exact negative authority assertions remain intact and bootstrap errors are visible

Scenario: Early bootstrap refusal exposes its original collector error
  Test: native_matrix_intake_prime_reports_early_identity
  Given a real local whoami response for a different configured device
  When the bootstrap collector settles before its remaining scripted requests
  Then the actual Identity failure is reported instead of a later fake-peer timeout

Scenario: Observed attachment intake exposes its early original error
  Test: native_matrix_intake_observed_reports_early_identity
  Given a real local whoami response for a different configured device during observed intake
  When intake settles before the remaining observed HTTP script
  Then the original Identity failure and latest fixed HTTP phase remain visible without waiting for another request

## Out of Scope

Production timeout or durability fixes, backend stage attribution absent an
original probe, SDK payload logging, CI scheduling changes, live hosts and treating
later successful diagnostics as repair of the original Windows result.
