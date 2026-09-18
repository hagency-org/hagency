spec: task
name: "Run the original configured inline factory agents as a native service"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, fleet, bootstrap, custody]
---

## Objective

Connect private configured startup to the actual original inline factory and
consume its successful non-cloneable agents into recurring service drivers.
Keep their original runtimes, enrolled SDKs, workspace and file owners through
dispatch and shutdown. This advances the full port; it does not replace the
remaining AS receiver/registration generation, recovery, effective sandbox,
real client approval, two-agent/live soak, parity or release requirements.

## Constraints

- An explicit `inline_factory_service_checkpoint_v1` composition opt-in is
  allowed only on the continuous driver with an original home/rooms provisioner
  and configured approval bot. Existing provisioning checkpoint markers retain
  their old behavior without this additional opt-in. This is not either complete
  ADR016 deployment profile; do not advertise those before their remaining gates.
- Construct the configured approval collector once, then attach that same Arc
  and the same eight-live/two-parked ApprovalHost to the original provisioner.
  Do not reconstruct credentials, account, SDK, home, runtime or readiness.
- Use the verified fixed executable/helper and private retained context root.
  Unmanaged factory agents use their own materialized home, not the coordinator's
  shared HOME. Managed-account launch still supplies its original private home.
- Discover only actual successful retained jobs. Take each original agent once;
  never derive owners from Active rows or retry failed/unknown provisioning.
  Discovery is bounded by the existing16-job non-evicting registry and does not
  wait for a later intake batch to finish before observing an earlier result.
- Each agent uses its original claim profile, session and workspace. Dispatch
  consumes ProvisionedAgent, never a replacement ordinary Host. Continuation
  still requires its private original completion witness. Existing root behavior
  remains unchanged outside explicit fleet mode; fleet claims share max-live8
  with the original eight-slot ApprovalHost, not eight slots per agent.
- Each driver has its own workspace registry and file/receive owners. A stopped
  driver cannot globally retire another agent's workspace. Routing authenticates
  the full original attempt; historical routing is data lookup only and restores
  no execution or file authority. Original current/historical checks still run.
- File/receive feature configuration and per-owner bounds stay explicit. Keep
  original SDK queues, private media namespaces and original job/recovery checks.
- Stop discovery/admission and quiesce every admitted owner before draining.
  Retain failed/unknown owners; drain other agents despite an individual error.
  Close retained factory custody before the coordinator SDK/writer. No cleanup
  proof from Report formatting, no global stop sweep or automatic unknown retry.
- Offline native tests only. No dependency/version, formatter, deployment,
  reset, commit/PR or cutover. Live qualification follows separate evidence.

## Allowed changes

- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/**
- native/hagency/src/lib.rs
- native/hagency/src/runner/files.rs
- native/hagency/src/runner/received.rs
- native/hagency/src/file_service.rs
- native/hagency/src/receive_service.rs
- native/hagency-execution/src/factory.rs
- native/hagency-matrix/src/provisioning/factory.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/owned_dispatch.rs
- native/hagency/tests/bootstrap.rs
- native/hagency/tests/inline_factory.rs
- native/hagency/tests/inline_factory/**
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: Configured factory composition requires its original dependencies
  Test: native_configured_fleet_profile
  Given the closed continuous profile and separate protected credentials
  When the explicit factory opt-in lacks approval or home/rooms or has invalid limits
  Then startup refuses without changing existing checkpoint semantics

Scenario: Discovery consumes only genuine successful original agents once
  Test: native_configured_fleet_original_handoff
  Given actual inline physical provisioning with its original runtime and SDK
  When service discovery runs before and after completion and after the first take
  Then only one original agent is returned and unknown or closed jobs cannot rearm

Scenario: The service executes successive tasks through the original factory owner
  Test: native_configured_fleet_recurring_driver
  Given the actual successful inline factory and native helper/API
  When the configured service claims tasks in successive turns
  Then first initialize is consumed once, later dispatch requires original completion, and unknown cleanup stops continuation

Scenario: Routing authenticates current and historical original attempt identity
  Test: native_fleet_runner_service_scope
  Given original attempts from separate engagements
  When current expired or altered credentials select a file backend
  Then only authenticated originals yield their own engagement and no execution authority is restored

Scenario: Closing one admitted owner does not stop another agent
  Test: native_configured_fleet_shutdown_isolation
  Given distinct original per-agent workspace and worker custody
  When one agent stops or fails closure
  Then other agents remain isolated and whole-service shutdown drains all original owners without inventing stop settlement
