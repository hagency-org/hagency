spec: task
name: "Run factory project mentions through the original native agent"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, matrix, fleet]
---

## Intent

Factory agents already join and authenticate their project, but the recurring
service selects only the private DM. Bind a project inbox from that original
collector's current observation and execute exact mentions through its retained
runtime, workspace and canonical reply path.

## Constraints

- Only original factory Group rooms use observed generations. Require the
  collector's own authenticated observation and the matching available writer
  generation; stored Active rows or caller values alone confer no authority.
- Resolve a separate project session for each transport/room generation. Never
  rebind or revive an old session, task, receipt, SDK or runtime.
- Refresh host claim metadata after collection. All actual intake, selection,
  claim and outgoing checks remain authoritative and fail on stale generations.
- Preserve exact Matrix mention gating and task completion. This slice admits
  main-room mentions; automatic thread discovery remains outside its scope.
- Default owned claims still require encrypted rooms. Only an explicit host
  project-room selection may admit plaintext Group work, and the writer must
  match the engagement's own registered project. Never extend this to a DM.
  Private DMs and approvals retain their existing room/generation checks.
- Use the existing per-agent workspace and physical owner. Do not share an
  agent's directory with another agent or invent DM/project filesystem isolation.
- A membership change can retire earlier group work. Do not automatically replay
  it or turn uncertain delivery into a successful result.
- Offline tests use synthetic local peers only. No live-account mutations in
  Cargo tests, formatter, dependency changes, commit, PR or production cutover.

## Allowed changes

- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/intake.rs
- native/hagency-matrix/src/outgoing.rs
- native/hagency-matrix/src/provisioning/factory.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/tests/owned_claim.rs
- native/hagency/tests/configured_fleet.rs
- native/hagency/tests/configured_fleet/**
- native/hagency/tests/inline_factory.rs
- knowledge/decisions/adr-178-factory-project-inbox.md
- specs/task-rust-factory-project-inbox.spec.md
- docs/**

## Scenarios

Scenario: Plaintext project execution is explicit and cannot widen private claims
  Test: native_owned_claim_plaintext_project
  Given a current plaintext Group project and the default owned claim profile
  When the host selects it with and without the project-only option
  Then only the exact explicit project selection can claim work
  And Direct rooms cannot acquire the option and stale project scope still refuses

Scenario: Another collector's current row cannot replace this owner's observation
  Test: native_factory_observed_group_generation
  Given a factory collector without a room observation or with an older snapshot
  When another original collector advances the shared room generation
  Then scheduling refuses until this collector authenticates that exact current state
  And an unavailable room remains refused without any generation revival

Scenario: Exact project mentions execute only on the addressed factory agent
  Test: native_configured_fleet_project_mentions
  Given two actual configured factory agents with a shared authenticated project
  When the owner posts unaddressed text and then separate exact mentions
  Then unaddressed and other-agent input creates no task on this agent
  And each addressed task reaches canonical Done and its own project reply
  And later private DM tasks retain separate encrypted routes and workspaces

Scenario: Another participant's request rides along only as labelled context
  Test: native_agent_inbox_names_the_waking_entry_as_the_request
  Given a shared project session holding an unprocessed request addressed to another participant
  When this agent's own exact mention selects its dispatch
  Then the frozen inbox lists the other request first with wake false and this agent's request last with wake true
  And the instruction names the last waking entry as the request and earlier entries as context, never instructions or approval

Scenario: A later agent's join does not end the earlier agent
  Test: native_configured_fleet_earlier_agent_survives_later_join
  Given the earlier agent's poll held between resolving its project inbox plan and selecting it
  When the later agent's join advances the shared project's generation under that poll
  Then the superseded plan ends the poll without work and the earlier agent stays running
  And it resolves the new generation and serves its own exact project mention

Scenario: Existing private file and approval isolation stays intact
  Test: native_configured_local_codex_fleet
  Given two actual original factory agents and private owner DMs
  When repeated encrypted tasks use file and approval tools
  Then original scopes, media, completion and cleanup remain isolated
