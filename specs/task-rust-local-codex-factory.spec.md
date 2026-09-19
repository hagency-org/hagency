spec: task
name: "Join provider-owned local Codex to the native fleet factory"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, codex, factory]
---

## Intent

Let one native service provision and run multiple isolated Matrix agents using
the explicitly selected provider-owned local Codex installation and login.

## Constraints

- Retain the original LocalCodex directory handles; do not copy credentials,
  create managed readiness, or inherit arbitrary coordinator environment.
- Every provision/dispatch must match the selected unmanaged Codex/OpenAI
  preset and seat. Each agent keeps its own workspace and task context.
- Check original provider directories throughout initialization, idle,
  activation and dispatch. Changed identities or writable paths fail closed.
- Warm initialization retains its original absolute deadline, at most30seconds.
  Active execution/owner approval use their distinct configured budgets.
  Initialization cannot run a turn or answer an approval request.
- Keep original live/parked reservations, activation proofs and stop custody.
  No live services or provider calls from ordinary tests.

## Allowed changes

- native/hagency-execution/src/{host,local_codex,factory,warm}.rs
- native/hagency/src/bootstrap/{config,fleet}.rs
- native/hagency/tests/{configured_fleet,warm_runtime}.rs
- native/hagency/tests/configured_fleet/**
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- specs/task-rust-local-codex-factory.spec.md
- knowledge/decisions/adr-173-local-codex-factory.md
- docs/**

## Scenarios

Scenario: Configured local Codex fleet preserves two original agents
  Test: native_configured_local_codex_fleet
  Given a provider-owned local profile and separate authenticated Matrix agents
  When the actual configured service provisions two agents and runs two rounds
  Then each original warm owner uses the retained provider paths
  And private task, file, approval and reply scopes remain separate
  And no managed account or login observation is fabricated

Scenario: Warm local provider custody refuses replacement and mismatched scope
  Test: native_warm_local_codex_custody
  Given the original provider directory handles and a pending provision scope
  When directories change during initialization or idle, or the seat mismatches
  Then no task or successful activation is manufactured
  And original owned cleanup and sticky provision attempt remain in force

Scenario: Active approval duration never extends warm initialization
  Test: native_warm_local_codex_budgets
  Given an owner wait longer than the initialization budget
  When warm initialization succeeds and an active dispatch is admitted
  Then the original startup budget and physical owner remain separate
  And a dispatch whose budget cannot fit the owner wait is refused
