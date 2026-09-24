spec: task
name: "Sample native approval authority clocks inside the original writer transaction"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, approvals, authority, lifecycle]
---

## Intent

Implement the root-approved eight-path ADR043 correctness prerequisite under the
existing migration authorization. The original writer samples fresh authority
time after its queue and SQLite Immediate lock waits. This does not integrate the
approval coordinator into an owned runner or establish full M6 parity.

## Constraints

### Must
- Sample binding, request admission, direct and SDK verdict admission, one-shot consumption and application-observation authority clocks while the original Immediate transaction is held.
- Preserve deterministic repository timestamp entry points for existing fixtures while production async wrappers supply the actual writer clock closure.
- Preserve current capability, private binding, expiry, grant and exact application checks with no second writer or new authority.
- Expired pending or decided requests may yield only the existing exact deny while capability remains current; expired capability yields no descriptor.
- Historical application settlement may record actual evidence but must not resume expired execution.
- Exercise actual SQLite contention and the original bounded writer queue; retain the original failed checks and exact lifecycle counts.

### Must Not
- Do not broaden parked execution renewal, add a coordinator or control pump, alter sandbox policy, synthesize Applied evidence, or change schema or dependencies.
- Do not use SQL authority setters, a copied database or a replacement production writer to make a test pass.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/approvals/clock.rs
- specs/task-rust-approval-fresh-clock.spec.md
- knowledge/decisions/adr-043-native-owner-approvals.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- No execution renewal, runtime transport, permissions coordinator, schema, Cargo manifest, lockfile or live service changes.

## Acceptance Criteria

Rule: approval-clock-order — Fresh approval time follows original transaction acquisition

Scenario: Every authority clock observes the original physical transaction
  Test: native_approval_transaction_clock_owned
  Given the six production approval clock callbacks and the original database
  When each callback samples authority time
  Then another Immediate transaction on that same database fails with DatabaseBusy
  And each operation releases its transaction after success or refusal

Scenario: Invalid expired approval admission follows original SQLite lock acquisition
  Test: native_approval_admission_clock_after_lock
  Given original current capability and private approval context setup
  When actual SQLite contention holds binding request and verdict admission past expiry
  Then the queued operations refuse without creating context request receipt or grant authority

Scenario: Approval consumption distinguishes expired request from expired capability
  Test: native_approval_consumption_clock_after_lock
  Given original decided approvals and retained current or expired capability
  When the original writer waits for SQLite until the relevant deadline passes
  Then expired request consumption returns exact deny once and expired capability returns no descriptor

Scenario: Historical application evidence does not renew expired execution
  Test: native_approval_application_clock_after_lock
  Given an original consumed application and a parked dispatch
  When actual lock contention delays its authenticated observation beyond capability expiry
  Then historical application evidence settles without resuming the dispatch

Scenario: Approval clocks follow bounded writer queue admission
  Test: native_approval_clock_after_queue
  Given a blocked command in the original bounded writer
  When an approval request waits in that queue until its deadline passes
  Then it refuses after dequeue and creates no approval authority

## Out of Scope

Owned runtime/coordinator integration, parked approval maintenance, bounded control
pumping, Matrix cards, application inspection, cross-platform qualification and
production cutover remain separate gates.
