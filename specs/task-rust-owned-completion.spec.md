spec: task
name: "Hold explicit canonical completion until the retained native owner stops"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runner, completion]
---

## Intent

Commit verified final content together with explicit canonical Done, fence further
execution, and publish only through the same retained owner's bounded cleanup path.

## Constraints

### Must
- Keep canonical_tasks as the only task truth and retain task-only Done behavior.
- Share task mutation call identifiers and digest conflict checks.
- Require a successful started-only opaque host scope and the exact historical attempt.
- Stop the same retained owner and observe all three cleanup facts before publication.
- Recheck finite deadline current Done epoch registration allocation and original private route.
- Keep dirty leases and held content when cleanup or authority remains unknown.
- Preserve unrelated stop and resource custody.

### Must Not
- Do not infer Done from protocol text or refresh execution authority after Done.
- Do not accept arbitrary OS cleanup observations through an execution or runner API.
- Do not publish after caller cancellation or automatically resume held content on restart.
- Do not enable native service availability or claim live model sandbox qualification.

## Boundaries

### Allowed Changes
- native/hagency-core/**
- native/hagency-store/**
- native/hagency-execution/**
- native/hagency/src/runner/**
- native/hagency/src/runner.rs
- native/hagency/src/task_client/**
- native/hagency/src/task_client.rs
- native/hagency/src/mcp/**
- native/hagency/src/mcp.rs
- native/hagency/tests/**
- native/hagency-runtime/src/codex/session/task_mcp.rs
- specs/task-rust-owned-completion.spec.md
- specs/task-rust-owned-completion-integration.spec.md
- knowledge/decisions/adr-060-native-owned-completion.md
- docs/**

## Acceptance Criteria

Scenario: Explicit finish holds content and retires execution
  Test: native_owned_completion_atomic_finish
  Given a current Started task and verified Matrix route
  When the exact helper explicitly completes with bounded final text
  Then Done and held content commit together with an execution fence and no send permission

Scenario: Invalid or foreign retained start scope is rejected
  Test: native_owned_completion_scope_and_custody
  Given a held completion and the exact started host scope
  When a host supplies a foreign scope expired route or unrelated unresolved custody
  Then no content is published and no unrelated lease or stop is cleared

Scenario: Mutation receipt loss preserves real canonical outcomes
  Test: native_owned_completion_queue_reply_loss
  Given a real queued finish command with its response withheld
  When response loss occurs before or after the durable commit
  Then no cancelled future is repolled and the exact held receipt distinguishes committed from unstarted work

Scenario: Cancellation reaches the actual queued publication
  Test: native_owned_completion_queued_cancellation
  Given held content and a blocked writer queue
  When the original operation is cancelled before publication eligibility
  Then no final reply or lease release is committed

Scenario: Original operation deadline remains enforced after queue delay
  Test: native_owned_completion_queued_deadline
  Given a held completion and an actual queued publication
  When the original monotonic operation deadline expires before writer eligibility
  Then final content stays held with no lease release despite a later persisted completion deadline

Scenario: Completion capacity rolls back the full transition
  Test: native_owned_completion_capacity_rolls_back_done
  Given a full bounded per-session completion history
  When another explicit finish is attempted
  Then task epoch receipt capability and lease remain unchanged

Scenario: Retired routes and epochs cannot publish old content
  Test: native_owned_completion_retirement_and_receipt_namespace
  Given a held completion and exact historical owner scope
  When task epoch owner allocation device or room privacy changes
  Then the old body remains unsendable without renewing execution authority

Scenario: Restart cannot reconstruct process authority
  Test: native_owned_completion_restart_has_no_reconstructed_owner
  Given canonical Done with a held completion
  When the writer reopens without a retained owner
  Then only the identical non-authorizing receipt can be replayed

Scenario: Schema upgrade does not invent completion custody
  Test: native_owned_completion_migration
  Given an older native database
  When schema sixteen is installed or required structure is missing
  Then only valid structure opens and no completion is invented for existing tasks

Scenario: A lost settlement names which store refusal produced it
  Test: native_settlement_cause_markers_are_distinct
  Given the store refusals that can lose a settlement command
  When the settlement failure marker is derived
  Then each mapped refusal keeps its own bounded discriminant and every other refusal reports as storage
  And the marker never carries error text or becomes authority retry reply or lease input

Scenario: A clean completion records no settlement cause
  Test: native_owned_dispatch_real_pipes
  Given a normal owned dispatch that completes and settles cleanly
  When its report is produced
  Then no settlement cause is recorded because no store refusal was observed

## Out of Scope

Automatic inspected restart recovery, model reporting continuation, live Matrix or
Codex services, new platform launchers and physical sandbox qualification.
