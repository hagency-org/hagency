---
spec: task
name: "One fact, one blast radius: the worker outlives its attempt and the process always exits"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, execution, fleet, custody, shutdown]
---

## Intent

Second slice of the closing order (gaps G1 and G2 of
`docs/reviews/2026-09-22-native-codex-architecture-review.md`; findings F1,
F2, F9, F10, F16 of `docs/reviews/2026-09-23-adr-failure-model-consistency.md`).
Decided in ADR-182: a failed attempt ends its dispatch and the worker goes
on; an unproven cleanup fences the agent durably and the in-memory owner is
dropped; shutdown always completes.

## Constraints

- The store's session quarantine, dirty workspace, thread notices and
  operator-only resolution are unchanged; nothing here resolves, retries or
  publishes.
- A worker continues after a failed, unsettled or negative attempt and after
  a refused handoff (5 s flat backoff); it ends only on cancellation, on
  `Failure::Refresh`/`Startup`/`OutcomeUnknown` from the Matrix and
  enrollment paths (the next slice), or on a worker panic.
- `agent_fences` is written before the owner is dropped, in the store, with
  the reason; the claim path and both selectors return no work for a fenced
  engagement; the operator's resolution of the fenced dispatch clears the
  fence; re-attach honours it; nothing else clears it.
- `Driver::close` returns `Ok` once every attempt is recorded and any
  unproven tree is fenced; `serve` completes `stop_graceful`; the process
  exits on one SIGTERM.
- The status keeps `last_failure` across attempts and shows `fenced`.
- ADR-181's evidence is recorded on every path this slice changes.

## Allowed changes

- native/hagency-store/src/migrations/038-agent-fences.sql
- native/hagency-store/src/domain/agent_fences.rs (new)
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain/messages.rs
- native/hagency-store/src/domain/task_intents.rs
- native/hagency-store/src/domain/outcome_resolution.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/stopped_inspection.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/**
- native/hagency-runtime/Cargo.toml
- native/hagency-runtime/src/owned/session.rs
- native/hagency-execution/Cargo.toml
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/factory.rs
- native/hagency-execution/src/warm.rs
- native/hagency/Cargo.toml
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/bootstrap/fleet.rs
- native/hagency/src/main.rs
- native/hagency/tests/**
- knowledge/decisions/adr-182-one-fact-one-blast-radius.md
- knowledge/decisions/adr-036-native-codex-session.md
- knowledge/decisions/adr-096-native-development-bootstrap.md
- knowledge/decisions/adr-127-*.md
- knowledge/decisions/adr-135-*.md
- knowledge/decisions/adr-162-native-stopped-owner-inspection.md
- knowledge/decisions/adr-169-continuous-outcome-resolution.md
- knowledge/decisions/adr-175-factory-failure-diagnostics.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- specs/task-rust-attempt-containment.spec.md
- docs/**

## Scenarios

Scenario: A failed turn takes the session out, not the agent
  Test: native_worker_outlives_a_failed_attempt
  Production caller: hagency::bootstrap::driver::run_continuous
  Given a continuous driver whose first turn fails on a refused notification
  When the attempt ends
  Then the dispatch is outcome_unknown, the session is quarantined and the notice is queued
  And the worker is still up, not failed, and its status counts the unresolved dispatch
  And the proven stop left the host's own receipt (ADR-162), so the same worker, without a restart, a resolution or any operator act, claims and completes the next dispatch of another session of the same agent
  And the status shows the last failure beside the live state

Scenario: A refused handoff is that attempt's failure, and the worker goes on
  Test: native_worker_outlives_a_refused_handoff
  Production caller: hagency::bootstrap::driver::run_continuous
  Given two agents whose provider directory permissions are revoked before their handoff
  When both handoffs are refused and the permissions are restored
  Then both workers are still running and the fleet is not failed; each refusal is recorded with its site, its attempt reads spawn_failed and its dispatch is requeued with the launch backoff
  And each dispatch completes on its second attempt, its replies delivered

Scenario: An unproven cleanup fences the agent, durably, and the owner is dropped
  Test: native_unproven_cleanup_fences_the_agent
  Production caller: hagency::bootstrap::driver::run_continuous
  Given an attempt whose stop verdict is Unknown (the diagnostics pin, the one shape no offline tree can produce)
  When the attempt ends
  Then agent_fences holds one open row for the engagement naming the dispatch and the reason, the dispatch is outcome_unknown and it has no host receipt
  And the worker continues but claims nothing while the fence is open, the status reads fenced, and the agent is not failed
  And the stopped-dispatch inspection stands on the attempt's own recorded stop evidence and says fenced; continue is refused (no receipt) and the orphan recovery is refused (open stop row)
  And the operator's settlement clears the fence, after which the waiting work runs
  And one SIGTERM ends the process: no owner was retained

Scenario: The claim path and the selectors honour an open fence
  Test: native_fenced_engagement_claims_nothing
  Given an engagement with an open fence and queued work in a healthy session
  When the host claims and the selectors run
  Then no dispatch is minted for it, and once the fence is cleared the same work is claimed

Scenario: The operator's resolution clears the fence
  Test: native_resolution_clears_the_fence
  Production caller: hagency::console::agents::recover_dispatch
  Given a fenced engagement whose fenced dispatch the operator recovers
  When the recovery commits
  Then the fence carries cleared_at, and a resolution of another dispatch clears nothing

Scenario: Re-attach honours an open fence
  Test: native_reattach_honours_the_fence
  Production caller: hagency::bootstrap::fleet::Service::reattach_known_agents
  Given an agent the product fenced on an unproven stop before a restart
  When the service restarts and re-attaches it
  Then the agent is registered, shows fenced with the dispatch, nothing was claimed across the restart, and the restart registered no account
  And the operator's settlement of that dispatch clears the fence, and the next restart re-attaches a working agent that runs its next task

Scenario: Shutdown completes past an unknown close verdict
  Test: native_configured_fleet_shutdown_isolation
  Production caller: hagency::bootstrap::Bootstrap::close
  Given a fleet in which one agent's file job is unknown at close (ADR-101's case)
  When the fleet is closed
  Then the drain reports the unknown verdict, the other agent's custody is untouched, and the factory close proceeds instead of being withheld
  And the served process's exit on one SIGTERM with a fence written is proven by the two fence scenarios above

## Out of scope

Fleet admission and readiness rollup, the approval pump's scope, and the
Matrix fence (the next slice); authority decoupling (G3); the approval leg
(G4); automatic re-observation of an unproven tree; the owner-facing fence
card in the agent's DM (the fleet/approval slice — the console route is this
slice's).
