---
spec: task
name: "Every owned attempt leaves evidence a lost agent can be diagnosed from"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, execution, guardian, store, diagnostics]
---

## Intent

The first slice of the closing order in
`docs/reviews/2026-09-22-native-codex-architecture-review.md` (gap G6): the
port must record, per owned attempt, what the retained product records — phase
timestamps, the failure uncollapsed, the guardian's own verdict and exit, the
runtime's exit identity and stderr tail — so that the next lost agent is
diagnosed from the store and the service log, not from another live run.
Decided in ADR-181. No verdict, rule or recovery path changes in this slice.

## Constraints

- Evidence is written beside every existing rule and authorizes nothing: no
  event, exit status, refusal category or stderr byte may strengthen a
  `StopReport`, promote a status, release custody or trigger a retry.
- Every recorded field is a fixed label, an integer, a bounded identifier or
  the bounded stderr tail (512 bytes in `terminal_reason`, 4 KiB for the
  guardian); control characters are replaced; nothing else is free text
  (ADR-175).
- Event writes are best-effort observations in their own savepoint through the
  domain worker; a failed write never changes an outcome.
- `Failure::LostAuthority { site, cause }` replaces the unit variant at every
  producer; no producer may keep discarding the store error. The `owned_failure`
  label stays `lost_authority`; `authority_site` and `authority_cause` are added
  beside it.
- The guardian's stderr pipe is sealed CLOEXEC and never reaches the work; a
  guardian started without the pipe behaves exactly as today.
- The runtime's stderr keeps its 16 KiB retained tail (ADR-040); only the last
  512 bytes are copied into the private store at settlement or failure.
- Retention prunes `runner_attempt_events` with their dispatch.

## Allowed changes

- native/hagency-store/src/migrations/037-runner-attempt-evidence.sql
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/attempt_events.rs (new)
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/**
- native/hagency-execution/Cargo.toml
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/warm.rs
- native/hagency-execution/src/factory.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/local_codex.rs
- native/hagency-execution/src/approval/**
- native/hagency-execution/tests/**
- native/hagency-runtime/Cargo.toml
- native/hagency-runtime/src/owned.rs
- native/hagency-runtime/src/owned/session.rs
- native/hagency-runtime/src/codex/transport.rs
- native/hagency-runtime/tests/**
- native/hagency-platform/Cargo.toml
- native/hagency-platform/src/supervisor.rs
- native/hagency-platform/src/supervisor/unix.rs
- native/hagency-platform/src/supervisor/unix/macos.rs
- native/hagency-platform/src/supervisor/unix/pipe.rs
- native/hagency-platform/tests/**
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/main.rs
- native/hagency/tests/**
- knowledge/decisions/adr-181-native-owned-attempt-evidence.md
- knowledge/decisions/adr-029-native-process-scopes.md
- knowledge/decisions/adr-040-native-owned-runner-io.md
- specs/task-rust-owned-attempt-evidence.spec.md
- docs/**

## Scenarios

Scenario: An attempt's phases are recorded in order with their clock
  Test: native_attempt_events_record_every_phase
  Production caller: hagency::bootstrap::driver::run
  Given an owned dispatch that is spawned, initialized, runs a turn, is stopped and settled through the real helper
  When the attempt completes
  Then runner_attempt_events holds the phases in visit order with increasing seq, and the stop record names the guardian's cause and exit
  And an attempt the store revoked mid-turn records the same phases without a settled one

Scenario: A failure is persisted uncollapsed beside the existing word
  Test: native_configured_fleet_handoff_diagnostics
  Production caller: hagency::bootstrap::driver::run
  Given two agents whose provider directory permissions are revoked before their handoff
  When each handoff is refused
  Then each attempt's failed event carries the full status with owned_failure lost_authority, authority_site local_codex_check and authority_cause io
  And the operator status shows the same two labels beside owned_failure, and no verdict differs from today

Scenario: Lost authority never discards the reason
  Test: native_local_codex_host
  Given an attempt whose local provider directory is replaced while it runs
  When the failure is reported
  Then the report is LostAuthority with site local_codex_check and cause io, not a bare word

Scenario: The store bounds and sanitizes attempt records
  Test: native_attempt_events_store_bounds
  Given fourteen phases recorded for one attempt, then an oversized, a control-laden and a 257th record
  When they are read back
  Then the phases return in order with increasing seq and at_ms, strings are sanitized and truncated, the oversized and the 257th are refused, and a refused record leaves the writer usable

Scenario: The guardian names why a stop after the leader exited did not prove the tree gone
  Test: native_guardian_report_names_the_stop_refusal
  Given a leader that exits leaving one live descendant the stop budget cannot end
  When the guardian reports Stopped with whole_tree_stopped false
  Then the frame carries refusal live_descendants with that row's pid, parent pid and executable name
  And the host reads the guardian's exit status and records both with stop_reported

Scenario: The guardian's stderr reaches the host and nothing else
  Test: native_guardian_stderr_reaches_the_host
  Given a guardian whose stop refusal writes one diagnostic line
  When the host observes the stop
  Then the host's 4 KiB tail holds that line and the work's own stderr holds nothing of the guardian's
  And a guardian started without the pipe still reports exactly as before

Scenario: The runtime names the leader's exit and keeps its stderr tail
  Test: native_runtime_exit_and_stderr_tail_persist
  Given a runner that exits with status 1 after writing a control-laden line to stderr
  When the session is stopped
  Then exit_identity reads code:1 and the 512-byte tail holds that line with the control character replaced
  And a runner exiting 0 reads code:0 with an empty tail

Scenario: The driver keeps the terminal reason and the clock with the attempt
  Test: native_continuous_driver_operator_resolution
  Production caller: hagency::bootstrap::driver::run
  Given a continuous driver whose first turn fails on a refused notification
  When the attempt fails
  Then runner_attempts.terminal_reason reads failure, exit identity and tail in the retained product's shape, started_at and settled_at are set, and the attempt's events run from claimed to failed

Scenario: A lease loss names the writer that settled it
  Test: native_lease_loss_names_its_writer
  Given a started dispatch whose lease another writer's expire() settles
  When the lost event is read
  Then it names the writer call, the lease_until, the now and the last_renew_at that the loss was judged from

Scenario: The executing crates say what they do
  Test: native_execution_phases_are_traced
  Given the service subscriber at INFO
  When an owned attempt runs
  Then one line per phase transition names the dispatch, fence, engagement and elapsed milliseconds, and a failure line carries the same fixed labels as the event

## Out of scope

Containment (a failed attempt ending the agent), bounded or re-observable
cleanup, the retained-owner rule, authority decoupling, the approval leg and
the Matrix fence: gaps G1–G5 of the review, each its own slice after this one.
