spec: task
name: "Fence verified task acknowledgement sends before external IO"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, matrix, custody]
---

## Intent

Give verified task notices the same explicit send-start and uncertainty boundary
as final replies before any live Matrix adapter is connected.

## Constraints

### Must
- Freeze the exact notice body source event route and canonical task epoch.
- Persist Sending before returning a one-shot host send snapshot.
- Recheck current route epoch exact claim secret and lease at send start.
- Recover possible sends as Uncertain on restart expiry or cancellation.
- Keep delivery observation distinct from canonical activation and process execution.
- Activate pending inputs only after exact observed delivery under current uncancelled scope.
- Preserve late delivery as audit evidence without reviving cancelled work.
- Require exact fence and durable content receipt for host inspection.
- Prevent legacy notice failure and retry methods from bypassing verified custody.
- Use fresh isolated stores and real domain worker fixtures without live Matrix.

### Must Not
- Do not automatically resend a possible send or infer NotSent from timeout.
- Do not enable live transport or claim cancellation retracts an accepted event.
- Do not expose host send inspection or cancellation authority to runners.
- Do not modify live services credentials or the original checkout.

## Boundaries

### Allowed Changes
- native/hagency-core/src/ingress.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/domain/verified_ingress.rs
- native/hagency-store/src/domain/notice_custody.rs
- native/hagency-store/src/domain/task_intents.rs
- native/hagency-store/src/domain/matrix_routes.rs
- native/hagency-store/src/migrations/014-notice-custody.sql
- native/hagency-store/tests/**
- native/hagency/tests/runner/verified_ingress.rs
- native/README.md
- specs/task-rust-notice-custody.spec.md
- knowledge/decisions/adr-045-native-notice-custody.md
- docs/**

## Acceptance Criteria

Scenario: A task acknowledgement starts once before delivery can activate work
  Test: native_notice_custody_activation
  Level: integration
  Test Double: fresh repository with authenticated host fixture observations
  Given a verified input and pending canonical task
  When a current claim starts the frozen notice and exact delivery is observed
  Then begin cannot repeat and only that delivery activates inputs atomically

Scenario: Possible sends survive restart and cannot be retried without inspection
  Test: native_notice_custody_recovery
  Level: integration
  Test Double: reopened SQLite repository with controlled clock
  Given a claimed or sending notice whose owner disappears
  When its lease expires or the repository reopens
  Then unstarted work may requeue but a possible send stays uncertain
  And exact host inspection alone resolves the original fence

Scenario: Promotion revocation cancellation and task epoch changes fence notices
  Test: native_notice_custody_fencing
  Level: integration
  Test Double: host room observations and canonical mutations
  Given a notice frozen before a scope change
  When the old route becomes unsafe or the host cancels it
  Then no late acknowledgement or NotSent report can revive that work

Scenario: Old development notices and legacy methods cannot bypass custody
  Test: native_notice_custody_migration
  Level: integration
  Test Double: prior schema fixture and legacy repository methods
  Given an older native notice without send-start evidence
  When schema migration or a legacy delivery method is attempted
  Then possible sends remain uncertain and no runtime gains transport authority

## Out of Scope

Actual Matrix SDK send cancellation and recipient encryption, taskless messages,
full operator recovery UI, continuous retention and live migration cutover.
