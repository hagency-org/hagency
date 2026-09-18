spec: task
name: "Join the existing local Codex login to native dispatch"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, execution]
---

## Intent

Port the explicit provider environment used by router/src/runner.ts::runnerEnv
to the existing native Codex dispatch. Reuse provider-owned login in place,
without importing it into a fresh managed namespace or asserting readiness.

## Constraints

### Must
- Require explicit private host configuration naming the existing home, Codex
  directory and one resource preset/seat; select and admit only that resource.
- Retain and recheck original directory objects; refuse replacement or unsafe
  write permissions without changing the operator's directory permissions.
- Keep provider files opaque. Copy no credentials and synthesize no account,
  authentication, quota, managed-readiness or canonical completion fact.
- Preserve the original managed-account path and its readiness checks.
- Preserve workspace sandbox, scoped task helper, original operation deadline,
  approval path, cleanup custody and canonical completion contract.
- Keep ordinary tests offline; run real Palpo/Robrix separately.

### Must Not
- No implicit fallback from a managed account to the local provider.
- No arbitrary environment map, API-key import or factory/warm-path enablement.
- No claim that directory identity proves provider identity or authentication.

## Boundaries

### Allowed Changes
- native/hagency-execution/Cargo.toml
- Cargo.lock
- native/hagency-execution/src/local_codex.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/lib.rs
- native/hagency-execution/tests/**
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/tests/owned_claim.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/bootstrap.rs
- native/hagency/tests/bootstrap/**
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- knowledge/decisions/adr-159-native-local-codex-binding.md
- specs/task-rust-local-codex-binding.spec.md
- docs/design/native-execution-parity.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Local provider directories remain original and credentials stay opaque
  Test: native_local_codex_binding
  Given explicitly selected existing provider directories
  When a binding is used changed or replaced
  Then only the retained current directories are admitted without reading auth files

Scenario: Local dispatch cannot use another resource or a managed account
  Test: native_local_codex_host
  Given actual native guardian pipes and a store-minted dispatch
  When the host selects matching foreign or managed resources
  Then only the matching unmanaged resource reaches the original runner

Scenario: Host selection excludes foreign resource attempts before leasing
  Test: native_owned_claim_resource_binding
  Given queued work and an exact host resource restriction
  When the original writer selects a dispatch
  Then foreign resource work remains queued without an attempt

Scenario: Private startup configuration explicitly selects the local provider
  Test: native_bootstrap_local_codex
  Given the existing bootstrap with an explicit local provider profile
  When startup validates local managed or factory combinations
  Then only the supported exact local binding is admitted

## Decisions

ADR159 adds an explicit local provider binding beside ADR114 managed namespaces.
It does not convert an unmanaged resource into a managed ready account. The
provider still decides whether its existing login can execute a real request.
