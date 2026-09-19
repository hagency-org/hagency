spec: task
name: "Freshly qualify the retained original warm owner before factory activation"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, provisioning, runtime, custody]
---

## Objective

Make the original retained warm worker perform a fresh physical owner observation
and current writer/home/account checks. A cached initialize result or absence of
a terminal report is not positive physical proof for the full inline factory.
This is a required factory prerequisite, not a replacement deployment profile or
completion of the full port. Both full profiles and genuine Applied/Active/routes
remain required, including the two original missing factory selectors.

## Constraints

- Observe only the same retained native owner. Unix uses its anonymous guardian
  socket and the guardian's existing retained Scope/leader identity; Windows uses
  its original retained process handle. No PID lookup/adoption/signalling, new
  launcher, public inspection endpoint or credential/configuration export.
- Positive Unix replies correlate to a strictly increasing private request nonce.
  EOF, timeout, malformed/stale reply, stopped owner or observation error cannot
  become a positive result. Unknown observation is sticky; cleanup retains its
  original custody and can drain only the exact outstanding late reply.
- Ready waits perform fresh qualification on the original worker. The bounded
  slot retains admitted inspection after waiter loss. Current writer checks follow
  physical IO; no Applied/task/session/route/approval is written by inspection.
- Keep the original initialize/idle/dispatch budgets and default sandbox unchanged.
  Inspection captures its absolute deadline before enqueue, capped by original
  idle and response budgets. No IO reactor migration or lifetime extension.
- Idle checks and dispatch handoff must not retain stale cached readiness after
  observed owner loss. Failures remain sticky and stop the original owner through
  existing teardown; no cold fallback, replacement or repeated initialize.
- Offline fixtures only. No live Cargo tests, dependency, formatter, commit/PR,
  deploy/reset/restart or new bootstrap marker. Listing is not execution.

## Allowed changes

- native/hagency-platform/src/supervisor.rs
- native/hagency-platform/src/supervisor/unix.rs
- native/hagency-platform/src/supervisor/windows.rs
- native/hagency-platform/tests/guardian.rs
- native/hagency-runtime/src/owned/session.rs
- native/hagency-execution/src/warm.rs
- native/hagency/tests/warm_runtime.rs
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- knowledge/decisions/adr-040-native-owned-runner-io.md
- knowledge/decisions/adr-053-native-owned-dispatch.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/agent-knowledge.md, docs/progress.md

## Scenarios

Scenario: Positive liveness is observed through the actual retained owner
  Test: native_guardian_current_owner
  Given an actual guarded native child and an unrelated separately owned child
  When the original owner is freshly observed repeatedly then stopped
  Then positives come from that same live owner, stopped cannot be positive, and unrelated execution survives

Scenario: Guardian liveness replies cannot be replayed as fresh proof
  Test: native_guardian_observation_protocol
  Given an actual private guardian channel and retained live Scope
  When sequential observations are followed by a repeated nonce
  Then each positive is exactly correlated and replay causes original protocol-failure cleanup

Scenario: Unknown guardian observation cannot rearm and late proof cannot block cleanup
  Test: native_guardian_observation_unknown
  Level: integration
  Test Double: private Unix socket with a live retained disposable child and scripted missing/late/wrong reply, not physical factory proof
  Given one admitted private observation whose response is absent or malformed
  When it expires or receives a wrong reply then observation or stop is requested
  Then no new observation is admitted and only an exact late reply can be drained before original cleanup

Scenario: Ready checks cannot reuse cached initialization after actual owner loss
  Test: native_warm_owned_runtime_current_owner
  Level: integration
  Test Double: actual retained native worker/pipes/writer/home and controlled offline peer exit
  Given the original initialized warm worker with no task or helper
  When Ready is freshly checked then that same actual peer exits
  Then readiness becomes sticky negative without a second initialize, task IO, synthetic activation or replacement

Scenario: Lost fresh-read waiter retains its original inspection and absolute bound
  Test: native_warm_owned_runtime_observation_custody
  Level: integration
  Test Double: actual SQLite writer lock and original native worker/pipes, not synthetic physical proof
  Given an initialized original warm owner and an admitted Ready inspection
  When its wait drops during an actual held writer read or before a buffered result expires
  Then the retained read finishes on that same owner without replacement and an expired original result never becomes readiness

Scenario: Late teardown cannot overwrite the original readiness failure
  Test: native_warm_ready_failure_sticky
  Given the private original Ready result and its first observed deadline failure
  When later cancellation or a positive publication arrives
  Then the original failure remains unchanged and cannot rearm
