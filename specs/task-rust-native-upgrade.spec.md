spec: task
name: "Prove the native upgrade procedure continues state and its rollback restores"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, release, upgrade, recovery]
---

## Intent

Bind the definition-of-done line "fresh installation, upgrade, recovery and
the selected state-continuity strategy have tested procedures" for the
native half: ADR-134's versioned-artifact procedure (install N → install
N+1 → restart) and ADR-135's rollback step, exercised as a bound test —
state continues across the upgrade and version N runs again on the same
state after rollback. No decision here changes the procedure; the one new
decision — how N and N+1 are produced in a test without the release
workflow — is recorded in this spec and in ADR-134's note.

## Constraints

### Must
- Produce versions N and N+1 in-test WITHOUT the release workflow: two local builds of the same tree with different workspace versions (`hagency --version` derived from the workspace version constant), each built to its own path and named as ADR-134's versioned artifacts are (the binary name carries the version, so both coexist in the install dir exactly as the procedure assumes).
- Install version N by the documented procedure, write live state through it (admit at least one engagement/agent and one store row whose head is recorded), then install version N+1 and restart the unit.
- Assert the upgrade continues state: the store head advances (or is already at head with zero replay), the previously written rows are readable, and the readiness word returns.
- Assert the rollback: applying the procedure's rollback step (point the unit back at version N's artifact, restart) runs version N again on the same state, with no data loss and no re-initialization.

### Must Not
- Do not invoke the release workflow, a registry, a tag or any network artifact source — the two local builds are the whole fixture.
- Do not modify the store's schema-head or migration chain — the upgrade test consumes migrations as they are; no new migration is licensed here.
- Do not touch the retained JS deployment or its installer.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/scripts/ (the two-version build and install harness)
- native/hagency/tests/
- specs/task-rust-native-upgrade.spec.md
- knowledge/decisions/adr-134-native-versioned-release.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed state, the production host.
- deploy/** (read-only inputs); .github/workflows/**; native/hagency-store/src/migrations/**.

## Acceptance Criteria

Scenario: The documented upgrade procedure continues live state
  Owed Selector: native_upgrade_procedure_continues_state (parked — the name is owed by the implementing slice and binds only when it lands; no Test: line here yet)
  Level: integration
  Test Double: two local builds of the same tree at workspace versions N and N+1, installed by the documented procedure over one state dir
  Given version N installed with live state — at least one admitted engagement and one recorded store row at a recorded head
  When version N+1 is installed by the documented procedure and the unit restarted
  Then the store head advances with no data loss — the previously written rows are readable
  And the readiness word returns

Scenario: The procedure's rollback step restores the previous version on the same state
  Owed Selector: native_upgrade_rollback_restores_previous (parked — binds with the slice above; no Test: line here yet)
  Level: integration
  Test Double: the same two-version fixture after the upgrade scenario
  Given version N+1 running on the upgraded state
  When the procedure's rollback step is applied and the unit restarted
  Then version N runs again on the same state — no re-initialization and no data loss
  And the recorded rows from before the upgrade are still readable

## Decisions

**How N and N+1 are produced in a test.** Two local builds of the same
tree with different workspace-version constants — not the restart fixture's
single binary path (it cannot produce two coexisting versioned artifacts)
and not the release workflow (operator-gated, network-sourced). The version
constant is the same one `hagency --version` reads, so the artifacts are
named exactly as ADR-134's procedure installs them; ADR-134's note records
this as the tested form of the procedure.

## Out of Scope

The release workflow's enablement (operator), real tagged artifacts, the
two-host cutover drill (ADR-135's production exercise, operator), and the
retained JS deployment's upgrade path.
