spec: task
name: "Bootstrap one native development attempt with authenticated Matrix refresh and required workspace registration"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, bootstrap, execution, development]
---

## Intent

Implement the reviewed ADR096 interface and path manifest. Run one
owned development attempt through the actual native service startup and register
its original Started workspace before a child or MCP request can begin.

## Constraints

### Must
- Use one shared non-test Bootstrap from actual serve and the executable integration fixture.
- Make one attempt per configured service start explicit in closed private configuration and safe status.
- Complete actual current-token Collector identity sync and full room observations before any compatible claim.
- Preserve refusal of already fenced generations and never remap old sessions to obtain a runnable fixture.
- Select compatible canonical verified-Matrix work inside the original claim transaction using time sampled after queue and lock waits.
- Obtain the original fresh capability once from the real writer and freeze actual scope before the acknowledged Started transition.
- Require one successful finite workspace registration acknowledgement before child launch in the explicitly selected mode.
- Bind workspace access to the original writer capability retained root and operation lifetime.
- Retain one actual Operation and unresolved Report on a fixed owner thread until owned shutdown or explicit unknown custody.
- Preserve the original consuming Collector close result and never reinterpret a second empty-owner close as a successful acknowledgement.
- Keep current execution sandbox approval guardian and whole-tree stop limits unchanged.
- Keep safe development status separate from production runtime capabilities canonical Done and file delivery.

### Must Not
- Do not allow runtime HTTP or model input to select executable arguments environment roots credentials routes or registration authority.
- Do not restore raw capabilities fabricate Started reissue lost claims or retry an uncertain attempt.
- Do not rely on first-request retries polling timing or fixture SQL availability restoration.
- Do not reset workspace or held-result budgets by reopening profiles for another attempt.
- Do not introduce continuous scheduling file tools media IO delivery schema or production activation.
- Do not call offline peer cross-compilation unavailable platform or unimplemented selector evidence a production pass.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency/Cargo.toml
- native/hagency/src/main.rs
- native/hagency/src/lib.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/bootstrap/workspace.rs
- native/hagency/tests/bootstrap.rs
- native/hagency/tests/bootstrap/fixture.rs
- native/hagency/tests/bootstrap/scope.rs
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- native/hagency/tests/cli.rs
- native/hagency/tests/http.rs
- native/hagency-execution/src/lib.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/registration.rs
- native/hagency-execution/tests/owned.rs
- native/hagency-execution/tests/owned/registration.rs
- native/hagency-execution/tests/support/reply_loss.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/tests/owned_claim.rs
- knowledge/decisions/adr-096-native-development-bootstrap.md
- specs/task-rust-development-bootstrap.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Real service startup authenticates Matrix before claiming and launching one assigned attempt
  Level: integration
  Test Double: actual native serve MCP and owned offline peer with real local TLS Matrix responses and protected SQLite
  Test: native_bootstrap_executable
  Given private initialized state and a legitimate queued verified-Matrix dispatch with no issued runner capability
  When the actual service starts with one development profile
  Then the real Collector refresh completes before a compatible claim and workspace registration precedes child startup
  And actual MCP reads and heartbeats mutate the original canonical task through the real runner API
  And no second attempt is claimed after the first ends

Scenario: Current authentication failure and fenced generations keep development execution unavailable
  Level: integration
  Test Double: actual executable current-token identity and full room TLS responses
  Test: native_bootstrap_refresh_refusal
  Given a configured profile with wrong identity unsafe room evidence or an already fenced exact generation
  When shared startup performs the real Collector refresh
  Then no capability is claimed and no child starts
  And status distinguishes known unavailability from unconfirmed fencing without restoring or rotating authority

Scenario: Compatible claiming preserves exact route workspace resource and after-lock authority
  Level: integration
  Test Double: actual SQLite writer queue transaction locks and mixed queued dispatches
  Test: native_owned_claim_profile
  Targets: native/hagency-store/src/domain.rs
  Given unsupported runtime shared workspace Done follow-up recovery-report stale and supported verified dispatches sharing the queue
  When the host profile claims using its original writer
  Then only current compatible canonical work can receive a new capability and exclusive lease
  And time is sampled after queue and transaction lock waits

Scenario: Missing registration acknowledgement never launches a child or rearms Started
  Level: integration
  Test Double: actual owned worker Started mutation and bounded registration channel
  Test: native_workspace_registration_gate
  Given a required workspace handoff after a real Started acknowledgement
  When registration is held failed dropped cancelled expired or acknowledged too late
  Then no child starts before a successful current registration acknowledgement
  And missing or lost acknowledgement retires the original handoff without another Started attempt

Scenario: Bootstrap retains the original binding and unresolved owned result through shutdown
  Level: integration
  Test Double: actual original writer retained root owned process and one bootstrap slot
  Test: native_bootstrap_custody
  Given a registered workspace and an executing or unresolved owned attempt
  When a foreign capability registration cancellation caller loss or shutdown is attempted
  Then new source access remains exact and retired access cannot be revived
  And the finite original owner and writers remain ordered until whole-tree stop is observed or explicitly retained unknown
  And a consumed SDK close with an unknown acknowledgement remains unknown across repeated close requests
  And a completed lost reply stays unknown without polling the completed receiver again

Scenario: Closed private configuration and default capabilities expose no runtime authority
  Level: integration
  Test Double: actual private files native CLI and HTTP capability responses
  Test: native_bootstrap_config
  Given absent malformed oversized nonprivate or inconsistent development configuration
  When the real startup path validates it
  Then it refuses unsafe opt-in inputs before claiming or preserves disabled defaults when omitted
  And no secret path raw identity executable override or unsupported production capability appears in safe status

Scenario: Lost original claim or Started result cannot become a replacement execution grant
  Level: integration
  Test Double: actual writer commits with deliberately lost replies and owned operation
  Test: native_bootstrap_unknown_start
  Given the real one-shot claim or Started transition loses its result
  When the driver observes the unknown outcome
  Then it issues no replacement claim capability handoff or child
  And the original custody is preserved with explicit outcome_unknown status

Scenario: Bootstrap registers the receive-inbox plan's workspace before the first claim
  Level: integration
  Test Double: actual native serve and received-file MCP peer with a receive-inbox plan naming a workspace no production path pre-creates
  Test: native_receive_executable
  Production caller: bootstrap::open_with_options (register_workspace before the first claim; config.rs refuses a plan whose workspace is missing from the map)
  Given a receive-inbox plan naming a workspace absent from production state
  When the service starts, registers that workspace, then claims
  Then the dispatch enqueued by select_receive_inbox satisfies its workspace_resources FK
  And a plan whose workspace is missing from the map is refused at config load, never at claim time

Scenario: The host settles pending conversation stops after the owned operation resolves
  Level: integration
  Test Double: actual DomainStore writer with a fenced started dispatch and an unsettled dispatch_stops row
  Test: native_bootstrap_settles_pending_stop_after_operation
  Production caller: bootstrap::driver::run → settle_pending_stops (sweeps pending_conversation_stops after the owned operation resolves)
  Given an operator stop fenced a started dispatch into outcome_unknown and wrote an unsettled dispatch_stops row
  When the owned operation resolves
  Then run() sweeps pending_conversation_stops and settles each with the observed report as evidence
  And a second pass over a settled row writes no duplicate

Scenario: The owned claim path reconciles a started dispatch after host death
  Level: integration
  Test Double: actual SQLite writer with a started dispatch whose lease has expired
  Test: native_owned_claim_reconciles_started_dispatch_after_host_death
  Production caller: execution::claim_owned_dispatch_for_host → claim_clock → expire (execution.rs:810) → lose (started → outcome_unknown)
  Given a started dispatch whose host died and whose lease has lapsed
  When another host claims
  Then expire() routes the started row through lose(): started → outcome_unknown, session quarantined, workspace dirtied, no new attempt
  And the claim returns None rather than re-claiming the row

## Out of Scope

Continuous scheduling Matrix timeline task admission file source read endpoints
MCP send_file receive_file media staging upload encrypted file-event publication
delivery schema project onboarding raw capability recovery live model traffic
effective sandbox qualification hostile same-UID isolation and production cutover.
The proposed selectors above are obligations rather than executed pass evidence.
