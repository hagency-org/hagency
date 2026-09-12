spec: task
name: "Connect owned native approval control to exact router authorization"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, approvals, ownership, execution]
---

## Intent

Implement the root-approved 29-path owned coordinator partition after the
schema22 router-authority and cooperative runtime-pump prerequisites. Build on
native_ci's immutable pre-stop runtime observation checkpoint. Root separately
owns application/bootstrap/private Matrix delivery and encrypted executable
qualification. This task enables only explicit host-library configuration.

## Constraints

### Must
- Retain the original OwnedSession in Report immediately after spawn and before any startup or binding await; preserve the original pre-stop RuntimeObservation and unresolved cleanup.
- Keep exact original callback frames grants admission batches and uncertain observations in the retained execution owner across caller loss cancellation and panic.
- Preserve pinned database futures across cooperative runtime updates and never cancel and replay a pending native read or reconstruct an attempted response.
- Persist current requests and park atomically before an unsent response; admit only every exact current router barrier and require the positive original grant acknowledgement before bytes.
- Native write acceptance and callback resolution remain independent from unconfirmed application; never fabricate Applied or let a later observation authorize execution.
- Maintain only the same current approval context capability fingerprint task epoch and exclusive lease under immutable original operation owner and response bounds with fresh post-lock clocks.
- Keep genuine parked task mutation refused and generic Started renewal unchanged; maintenance never resumes execution revives expiry or grants a response.
- Share one finite host live and parked capacity owner with the parked cap below the live cap; acquire before request parking and retain capacity through unknown process or response custody.
- Process every real opaque usage observation in order using the existing one-slot pending receipt before reading another update; retain older domain control futures during that bounded wait.
- Use actual supported offline native callback shapes and actual response-byte observations; keep private Matrix observation doubles distinct from SDK encrypted intake proof.

### Must Not
- No schema Cargo dependency production runtime protocol Matrix HTTP MCP authorization default sandbox or timeout-ceiling changes. Bootstrap changes are limited to the two exhaustive new failure label arms.
- No ambient opt-in verdict setter per-request process spawn hidden writer detached cleanup response retry or manufactured cleanup capacity.

## Boundaries

### Allowed Changes
- native/hagency/src/bootstrap.rs
- native/hagency-execution/src/lib.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/approval.rs
- native/hagency-execution/src/approval/state.rs
- native/hagency-execution/src/approval/control.rs
- native/hagency-execution/src/approval/capacity.rs
- native/hagency-execution/src/approval/observations.rs
- native/hagency-execution/tests/owned.rs
- native/hagency-execution/tests/owned/approvals.rs
- native/hagency-execution/tests/owned/approval_fixture.rs
- native/hagency-execution/tests/support/approval_loss.rs
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- native/hagency-runtime/src/bin/approval_probe/mod.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain/approvals/owned.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/approvals/owned.rs
- native/hagency-store/tests/approvals/owned_worker.rs
- specs/task-rust-owned-approval-coordinator.spec.md
- knowledge/decisions/adr-043-native-owner-approvals.md
- knowledge/decisions/adr-046-codex-approval-adapter.md
- knowledge/decisions/adr-053-native-owned-dispatch.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- All paths outside the exact approved partition and all live or production-cutover actions.

## Acceptance Criteria

Rule: owned-approval-control — Only the original owned callback and exact router grant may continue

Scenario: Actual owned callbacks continue after exact allow or deny admission
  Test: native_owned_approval_resume
  Given a guardian-owned supported callback with a current private owner binding
  When the actual persisted request receives a legitimate owner decision and unique response admission
  Then original response bytes precede continuing native updates under unchanged sandbox without Applied evidence

Scenario: New callbacks retain every current barrier during pending control
  Test: native_owned_approval_barriers
  Given multiple original callback frames and a pinned pending database control
  When a new barrier arrives during admission or readonly recheck
  Then every observed barrier parks and resolves before the original never-written response continues without another begin

Scenario: Cancellation and resolution preserve original uncertain custody
  Test: native_owned_approval_cancellation
  Given the actual retained process and original pending response owners
  When resolution EOF retirement or caller cancellation occurs
  Then original cleanup and response uncertainty remain distinct and no response is replayed

Scenario: Lost acknowledgements never manufacture sending authority
  Test: native_owned_approval_caller_loss
  Given actual request consumption admission and local write operations
  When their acknowledgement is lost or the owner unwinds
  Then the original child frames and facts remain retained without synthetic success or duplicate response bytes

Scenario: One shared host budget bounds live and parked work
  Test: native_owned_approval_capacity
  Given one original application-owned finite live and smaller parked budget
  When another actual callback reaches full parked capacity
  Then admission refuses before parking retains its exclusive lease and leaves bounded non-parked capacity

Scenario: Approval maintenance preserves only original bounded custody
  Test: native_owned_approval_maintenance
  Given an original instance-bound current approval scope and immutable operation end
  When maintenance encounters current or changed authority
  Then only its original lease may remain current while parked task mutation and expired foreign or replaced authority refuse

Scenario: Maintenance clocks follow original queue and SQLite lock waits
  Test: native_owned_approval_maintenance_clock
  Given the physical original SQLite transaction and bounded writer queue
  When contention crosses immutable scope or lease expiry
  Then maintenance refuses without new deadlines task mutation or authority setters

Scenario: Pending control preserves exact ordered usage observations
  Test: native_owned_approval_usage
  Given actual native usage events during pending approval control and response writes
  When domain receipts are delayed or refused
  Then the original usage slot and ordered facts remain retained with explicit unknown status and no fabricated observations

Scenario: An in-flight approval frame survives its own resolution
  Test: native_owned_approval_in_flight_resolution_completes_write
  Given a host held at the recheck gate with the prepared frame committed to the transport
  When the fixture emits the resolution for that in-flight id before the write lands
  Then the bounded write completes and is recorded and the operation reports no failure

Scenario: Pre-admission resolution still cancels with the named variant
  Test: native_owned_approval_resolution_before_admission_cancels
  Given an entry retained before any durable admission
  When the fixture resolves it
  Then the operation fails with ApprovalCancelled and no frame reaches the wire

Scenario: No second frame is written after a resolution
  Test: native_owned_approval_no_second_frame_after_resolution
  Given one written response frame and its legal post-write resolution
  When the drive continues to completion
  Then exactly one frame exists on both the host-written and probe-read streams and the durable write count is one

Scenario: A write receipt is processed before its own resolution is delivered
  Test: native_owned_approval_receipt_before_resolution
  Given the host held between the transport write receipt and the acceptance observation
  When the fixture reads the frame and then resolves it
  Then the operation completes with the acceptance row recorded and no transport cause

Scenario: A resolved-away frame is never sent
  Test: native_owned_approval_resolved_before_first_byte
  Given the resolution emitted while the frame is armed before its first byte
  When the host reaches the send
  Then the operation completes quietly with no failure no Closed cause no frame on the wire and no accepted row

## Out of Scope

Application bootstrap selection and request delivery, private SDK collection,
Matrix card publication, actual native helper transport and encrypted executable
qualification are separate root-owned gates. Scripted native processes and domain
owner-observation doubles do not establish live provider application, platform
sandbox efficacy, production availability or complete M6 parity.
