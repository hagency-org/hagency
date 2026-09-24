spec: task
name: "Preserve exact workspace and final-reply ownership across native fleet dispatches"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, fleet, workspace, reply, custody]
---

## Objective

Remove single-slot workspace and global reply-claim assumptions from the native
service before wiring concurrent provisioned agents into it. This is part of
the full fleet implementation, not a substitute for startup, file-service
routing, approval multiplexing, real two-agent qualification or sustained soak.

## Constraints

- At most16 simultaneous bootstrap workspace entries. Each comes only from an
  actual original acknowledged Started binding. No root/authority reconstruction.
- Acquire selects and retains one exact full capability/binding atomically; the
  same entry is validated afterward. No second lookup can select a successor.
- Release removes only an exact full-capability match and retires its held
  guards; unrelated entries remain usable. Global retirement refuses all access.
- Capture and receive paths check both global and per-entry retirement. Original
  writer checks and underlying operation liveness remain required.
- Native final delivery claims only the reply produced by its exact completed
  dispatch attempt. Authenticate the historical attempt's full capability and
  completed current fence, because completed dispatches clear live lease fields.
- A historical capability here selects only existing current pending reply
  custody; it grants no execution, new intent, route, cleanup or retry authority.
- Preserve original reply reconciliation, claim leases, stable transaction IDs,
  current route/task-epoch checks, begin-send and original SDK delivery rules.
- No global-queue fallback when the exact completed dispatch has no reply.
- Real offline store/workspace tests, no live external services, no fake Started
  or cleanup proof, no larger deadlines, dependency, formatter, deployment/reset,
  commit or PR. Full objective remains active beyond these prerequisites.

## Allowed changes

- native/hagency/src/bootstrap/workspace.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency-store/src/domain/replies.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/owned_completion.rs
- native/hagency-store/tests/replies.rs
- native/hagency/tests/owned_matrix/**
- native/hagency/tests/owned_matrix.rs
- native/hagency/tests/owned_mcp.rs
- knowledge/decisions/adr-033-native-final-reply-custody.md
- knowledge/decisions/adr-093-native-retained-workspace.md
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: Concurrent original Started bindings keep independent source custody
  Test: native_bootstrap_fleet_workspace_isolation
  Given multiple actual Started scopes with distinct private roots and identical relative filenames
  When the bootstrap service registers and acquires them concurrently
  Then each guard reads only its original root and foreign capabilities refuse

Scenario: Exact release retires old guards without affecting other dispatches
  Test: native_bootstrap_fleet_workspace_retirement
  Given actual retained concurrent bindings and acquired guards
  When a foreign release is attempted then one exact entry is released
  Then foreign release changes nothing, released guards refuse and other bindings survive until global retirement

Scenario: Registry capacity refuses additional actual Started handoffs
  Test: native_bootstrap_fleet_workspace_capacity
  Given16 original registered bindings
  When another actual handoff attempts registration
  Then it is returned as rejected without replacing or retiring an existing entry

Scenario: Exact completed dispatch cannot consume an older foreign reply
  Test: native_final_reply_dispatch_claim_isolation
  Given two current final replies from distinct completed dispatches
  When the later dispatch's original owner claims its reply
  Then only its own intent is leased while the older intent remains pending

Scenario: Expired execution credentials cannot select successor or retired output
  Test: native_final_reply_dispatch_claim_refusals
  Given a completed attempt and pending output
  When secret runner dispatch fence current completion or route does not match
  Then claim refuses without leasing foreign output or reviving execution authority
