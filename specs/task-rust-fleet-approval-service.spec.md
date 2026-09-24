spec: task
name: "Multiplex original fleet approval notices through one serialized approval service"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, fleet, approvals, ownership]
---

## Objective

Wire the native service to drain simultaneous original operation notice receivers
without waiting for the first operation to close. Serialize service approval
work with genuine factory membership observation on the SAME approval collector.
This is required fleet composition, not a substitute for configured fleet drivers,
per-agent file routing, real two-agent operation, client decisions or live soaking.

## Constraints

- Consume each original ApprovalRequests receiver once. No new sender, copied
  receiver, callback verdict, runtime capability, workspace or Matrix destination
  enters the pump. Its existing one-slot handoff channel remains bounded.
- Reserve that handoff slot BEFORE claiming a dispatch. Full capacity waits
  cancellably with no new task capability, workspace lease or running operation;
  a closed pump refuses before claim. After original Operation creation, move its
  one receiver synchronously through the reserved slot. Never drop it merely
  because another driver filled the slot, or wait after Started before its ACK.
- Keep at most16 active original receivers. Poll them fairly; an idle or closed
  receiver cannot block another. A full set backpressures the existing handoff.
- One shared set of at most64 current pending IDs and one serial card/verdict
  loop; prioritize a due verdict poll over a permanently ready notice stream.
- Closing one receiver removes only that source. Closing handoff intake drains
  remaining sources; shutdown or an original refused/unknown operation stops
  the pump with original custody preserved and no automatic re-entry.
- A host-only scheduling permit on the original ApprovalCollector bounds service
  participation to16, with one active turn. Waiting is cancellation-safe, bounded
  by its existing SDK wait limit, and grants no SDK/domain/approval authority.
- Pump initialization, card sends, verdict intake, close and factory membership
  observation share that permit. Original collector busy permits, sticky jobs,
  deadlines, current-scope checks and unknown/closed refusal remain unchanged.
  Original owner/card cutoffs and warm readiness are rechecked after waiting;
  close's existing2s envelope includes its scheduling wait.
- Channel arbitration fixtures are explicitly scheduling tests, not crypto,
  factory activation or real runtime-callback proof. Preserve actual executable
  encrypted-owner/plaintext-refusal/bad-anchor tests and physical factory tests.
- No extra task per receiver, unbounded queue, dependency, larger SDK/notice/live
  or request budgets, formatter, live Cargo test, deploy/reset, commit or PR.

## Allowed changes

- native/hagency-execution/src/approval.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/approval.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency-matrix/src/approval_intake.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/provisioning/factory.rs
- native/hagency-matrix/tests/approval_intake/mod.rs
- native/hagency/tests/bootstrap/approval.rs
- native/hagency/tests/inline_factory/**
- knowledge/decisions/adr-064-native-matrix-approval-intake.md
- knowledge/decisions/adr-112-native-private-approval-delivery.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: Idle or closed operation receivers do not starve another source
  Test: native_fleet_approval_notice_multiplex
  Given bounded independent original-shaped notice channels
  When one stays open and idle while another emits notices and closes
  Then round-robin polling receives the ready notices and retains the idle source

Scenario: Receiver capacity and cancellation retain bounded ownership
  Test: native_fleet_approval_notice_capacity
  Given16 held receivers and a waiting next notice
  When another source is offered or the wait is cancelled
  Then capacity refuses replacement, cancelled waits lose no notice, and closure releases only its original source

Scenario: Service scheduling is bounded and cannot mint SDK authority
  Test: native_fleet_approval_service_turn
  Test: native_fleet_approval_service_turn_timeout
  Test: native_fleet_approval_service_turn_lost_observer
  Given one actual configured approval collector and its held service turn
  When other service turns wait, cancel or exceed capacity
  Then at most16 participate, one turn proceeds at a time, cancellation returns its slot and no SDK or domain mutation is fabricated
  And virtual-time scheduling timeout releases no other turn and actual held TLS retains the original observer's busy permit after caller loss

Scenario: Driver handoff backpressure precedes task and workspace acquisition
  Test: native_fleet_approval_handoff_backpressure
  Given the original driver's one-slot approval channel has a held reservation
  When actual Matrix refresh completes and dispatch would otherwise be claimed
  Then no dispatch attempt exists while capacity is full, cancellation or receiver closure creates none, and released capacity allows only the original next claim

Scenario: The production pump still completes authenticated original callbacks
  Test: native_private_approval_roundtrip_encrypted_owner
  Test: native_private_approval_roundtrip_plaintext_refused
  Test: native_private_approval_startup_wrong_anchor
  Given actual executable startup and independent owner crypto
  When service scheduling and notice multiplexing carry the existing native request
  Then only the exact encrypted authenticated verdict produces its original callback

Scenario: The genuine factory keeps original approval membership and runtime custody
  Test: native_provisioning_factory_first_dispatch
  Test: native_provisioning_factory_sequential_dispatch
  Test: native_provisioning_factory_approval_refusal
  Given actual inline home account SDK runtime and activation owners
  When factory membership uses the same serialized approval collector
  Then success and current-owner refusal preserve all original physical and dispatch custody checks
