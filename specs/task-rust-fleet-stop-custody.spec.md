spec: task
name: "Do not settle another native dispatch from an unrelated operation report"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, fleet, cancellation, custody]
---

## Objective

Remove the bootstrap's global stop-settlement inference discovered while wiring
fleet isolation. One returned operation report is not physical cleanup or
workspace-inspection proof for every pending stop, nor does unknown cleanup
authorize release of its own leases. Preserve exact original owners and the
existing explicit recovery boundary. This correction reopens the unproven G8
positive settlement obligation; it does not redefine full migration completion.

## Constraints

- Exercise the actual production completion path with original owned operations,
  original Started acknowledgements and the real DomainStore stop command.
- Never fabricate a StopReport, current capability, inspected workspace or
  positive settlement just to keep an old wiring assertion green.
- A fenced dispatch retains pending stop, workspace dirtiness and leases until
  an exact stopped-owner/workspace-inspection path establishes release authority.
- An operation result never sweeps or settles another dispatch's stop records.
- Keep original completion publication, final delivery, negative observation,
  cancellation and explicit operator recovery behavior intact.
- Record the formerly green store-call fixture's missing physical proof and
  reopen G8 explicitly, including its owed positive selector. No claim that
  automatic stop settlement or the overall port is complete.
- Offline tests only; no dependency, formatter, live reset/deployment, commit/PR.

## Allowed changes

- native/hagency/src/bootstrap/driver.rs
- specs/task-rust-development-bootstrap.spec.md
- this spec
- knowledge/decisions/adr-130-native-agent-lifecycle-authority.md
- knowledge/decisions/adr-146-production-callers-and-store-surface.md
- docs/progress.md
- docs/agent-knowledge.md

## Scenarios

Scenario: Returning one actual operation cannot settle its own or another unknown stop
  Test: native_bootstrap_fleet_stop_custody
  Production caller: hagency::bootstrap::driver::finish_attempt
  Given two original Started operations and a real operator stop fencing the second
  When the first returns a real pre-child failure and the production completion path observes it
  Then both stops remain pending with their original leases and dirty workspaces
  And returning the second original operation also supplies no invented workspace-inspection proof
