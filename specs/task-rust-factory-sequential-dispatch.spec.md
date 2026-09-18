spec: task
name: "Allow actual factory agents to consume sequential acknowledged dispatches"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, factory, execution, ownership]
---

## Objective

Remove the factory's permanent one-task limit without relaunching unknown work.
Its first dispatch consumes the original initialized warm owner. Since the
existing execution model stops that process after a turn, later distinct work
uses the same immutable Host and existing acknowledged-Started cold operation.
This is normal subsequent work, never fallback from an unsuccessful warm owner.

## Constraints

- Retain the same writer, account, physical workspace roots, approval budget,
  fixed native executable/helper and original enrolled Matrix SDK.
- First admission still spends the warm handoff before an ownership-worker job
  is queued. A typed non-cloneable ticket contains no process owner; its identity
  must match the originating runtime's retained admission. Caller loss and failed
  admission cannot choose another capability or rearm an unknown owner.
- A later admission requires a private original-operation completion witness:
  successful execution and original store settlement, positively observed full
  stop, no remaining process/late-spawn custody, no pending/rejected/failed usage,
  and the original result received plus actual worker joined. Public mutable
  Report fields, caller booleans, SQL status alone and retry_stop cannot mint it.
- Known concurrent/pending admission refuses without spending a later turn.
  Failed or abandoned admitted work remains blocked. Close cancels the retained
  original operation and permanently closes admission; joining is not proof of
  cleanup or authorization to release leases.
- The first warm process uses its one original create-only task-context record.
  Subsequent fresh Started launches use the existing direct capability/task
  environment and newly constructed typed helper, not the stale warm record.
  This private binding selection cannot be supplied by runtime/HTTP input. No
  Host/account/root reconstruction, per-task secret files, new launcher or public
  readiness/proof setter is introduced. Normal explicit-context callers remain
  unchanged.
- All original claim/start/scope/workspace-ACK/usage/approval/stop/settlement checks
  and production budgets remain unchanged. Same/foreign capabilities never gain
  execution from a previous success. No synthetic Done or cleanup proof.
- Offline physical tests provision both account kinds through actual factory
  HTTPS/SDK/runtime/Active/route work, then run distinct scoped tasks through the
  actual native helper. On platforms without full-tree cleanup proof they must
  assert retained refusal, never substitute a passing cleanup result. Positive
  sequential execution must actually run on a supported Linux host.
- No live services from Cargo tests; no deployment/restart/reset/re-soak, dependency
  addition, formatter, commit/PR or production cutover. An isolated offline Linux
  qualification container on mini3 may use only synthetic fixture state and
  must not mount the running service's state or credentials.

## Allowed changes

- native/hagency-execution/src/factory.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/warm.rs
- native/hagency-execution/src/lib.rs
- native/hagency-matrix/src/provisioning/factory.rs
- native/hagency/tests/inline_factory.rs
- native/hagency/tests/inline_factory/**
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- knowledge/decisions/adr-053-native-owned-dispatch.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: Original factory ownership admits two distinct settled dispatches
  Test: native_provisioning_factory_sequential_dispatch
  Given either actual factory account kind and its original initialized owner
  When two separately claimed tasks receive original workspace acknowledgment and run the actual native helper
  Then Linux observes two completed dispatches using the same retained Host and SDK, distinct task bindings and only one original warm context; unsupported cleanup platforms retain the first unknown and refuse the second

Scenario: Pending or lost admission cannot create a concurrent or replacement owner
  Test: native_provisioning_factory_dispatch_custody
  Given the actual initialized owner and a queued original ownership-worker handoff
  When failed admission or waiter loss is followed by another dispatch request
  Then no second handoff or cold fallback is admitted and original cancellation/join remain off the async caller

Scenario: Mutable report output cannot authorize another dispatch
  Test: native_provisioning_factory_report_is_not_authority
  Given an actual first factory dispatch with cleanup or execution failure
  When its public report fields are changed by the fixture to look successful
  Then the private original completion witness still refuses another operation

## Full goal remains

Native fleet/service integration, both complete deployment profiles and AS
receiver/registration generation, effective sandbox, canonical completion/recovery
reliability, real two-agent/Palpo/Robrix qualification, sustained soaking and all
M0–M9/parity/release gates remain required beyond these offline fixtures.
