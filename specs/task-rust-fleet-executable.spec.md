spec: task
name: "Qualify the configured native executable with two original factory agents"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, fleet, executable, matrix, offline]
---

## Objective

Exercise the configured native executable from authenticated reception request
and representative verdict through two physical factory owners, encrypted owner
input, concurrent native helper operations, canonical completion and independently
decrypted final replies. Prove subsequent work reuses each original factory owner.
This is a necessary offline gate before real Palpo/Robrix fleet qualification,
not a replacement for real model, sandbox, owner approval, media or live soaking.

## Constraints

- Launch the real `hagency serve --agent-driver` child from closed private
  configuration. Only its unrelated preexisting coordinator may be fixture-seeded.
  Neither target may receive seeded Active, effect result, session, task, attempt,
  workspace registration, approval, completion or reply rows.
- Use an offline TLS peer and independent owner cryptography. Account registration,
  room creation/invitation/join, key upload/enrollment and native initialize must
  actually occur. Both registration-token and AS-login account kinds are required.
- Use the same owner identity but distinct target MXIDs, devices, DMs, homes,
  SDKs, workspaces, native processes and authenticated task contexts. Runtime
  observations never create domain authority. Do not copy private service keys.
- Observe both actual Started attempts and native helper readbacks before releasing
  a fixture-only completion gate. Complete through the real MCP helper/API, never
  SQL or a Host setter. Ordinary assistant output alone cannot mean Done.
- Verify exact per-agent canonical task/reply binding and decryption, then a second
  encrypted input/reply per agent without re-registration or fresh enrollment.
- Positive whole-tree cleanup/continuation runs on actual Linux. Compilation or
  macOS leader-only cleanup must not be substituted for that platform evidence.
- Tests never contact live services. No dependency, formatter, weakened sandbox,
  larger production deadline, reset, automatic unknown retry, deployment, commit,
  PR or production cutover. Existing failed live soak evidence stays failed.

## Allowed changes

- native/hagency/tests/configured_fleet.rs
- native/hagency/tests/configured_fleet/**
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- native/hagency/src/bootstrap/**
- native/hagency/src/bootstrap.rs
- native/hagency-matrix/src/provisioning/**
- native/hagency-execution/src/factory.rs
- native/hagency-execution/src/warm.rs
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: One configured executable provisions and serves two genuine agents
  Test: native_configured_fleet_executable_two_agents
  Given a fresh private service and offline authenticated reception/owner actors
  When two approved agents receive encrypted owner messages concurrently
  Then distinct original runtimes both hold actual Started work before completion
  And real native MCP completion produces each canonical Done and exact encrypted reply
  And a second input per agent completes without repeated provisioning or enrollment
  And shutdown drains the original service without retained live leases or unknowns
