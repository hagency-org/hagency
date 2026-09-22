spec: task
name: "Bring back the factory agents after a restart"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, fleet, recovery]
---

## Intent

After any restart the fleet listed one worker instead of three and reported
healthy: the inline factory's agents lived only in the memory of the process
that made them, although their credential, SDK store, rooms, home and domain
rows had all survived. The retained product brings its agents back at startup
from what it persisted, stored credential first, never registering again, and
skips an agent that cannot come back. Do the same, read-only, from what the
original provision left.

## Constraints

- Re-attach only a provision this factory completed: effect Complete with the
  factory's own receipt, engagement Active. Rebuild the original claimed scope
  and prove it by recomputing the stored receipt digest; an adopted account, a
  revoked engagement or a changed payload yields nothing.
- Write nothing to create: the home is reopened and checked, never repaired;
  the account custody is read and the single register/login path refuses;
  rooms custody is replayed and create, invite and join refuse; the SDK store
  is opened, never bootstrapped, and an incomplete enrollment ledger is
  refused. No claim, completion, activation, key upload or generation change.
- The re-attached runtime starts no warm child and reads no retained task
  context; its next task launches as an ordinary follow-up.
- One agent's failure is that agent's only: it is shown as `not_attached` with
  its Matrix cause, the fleet is not failed, readiness is unchanged, its work
  waits. A provision this process already owns is left to discovery.
- The caller's own cancellation of a read-only room observation retires
  nothing: the room, like the transport, is not evidence of anything.

## Allowed changes

- native/hagency-store/src/domain/provision_runtime.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/agent_home.rs
- native/hagency-store/tests/provision_runtime.rs
- native/hagency-execution/src/factory.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/warm.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/token_provision.rs
- native/hagency-matrix/src/token_provision/rooms.rs
- native/hagency-matrix/src/enrollment/provisioning.rs
- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/src/provisioning/factory.rs
- native/hagency-matrix/tests/token_provision.rs
- native/hagency-matrix/tests/transport.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/fleet.rs
- native/hagency/tests/inline_factory.rs
- native/hagency/tests/inline_factory/mod.rs
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- knowledge/decisions/adr-047-native-matrix-transport.md
- specs/task-rust-factory-agent-reattach.spec.md
- specs/task-rust-configured-fleet-service.spec.md
- specs/task-rust-token-account-provision.spec.md
- specs/task-rust-matrix-transport.spec.md
- docs/**

## Scenarios

Scenario: A restart brings back the agents the factory completed, and shows the one it cannot
  Test: native_configured_fleet_reattaches_after_restart
  Production caller: hagency::bootstrap::fleet::Service::reattach_known_agents
  Given a fleet whose inline factory completed one agent that ran one task, then a clean stop
  When the process restarts over the same state and the domain repository is reopened
  Then the agent is re-attached, registers no account, uploads no keys, and runs its next task through a follow-up launch
  And with its home binding tampered it is listed as not_attached, the fleet is not failed, and its work stays queued

Scenario: The rebuilt scope is the original claim, proven by its receipt
  Test: native_reattach_scope_rebuilds_only_what_the_factory_completed
  Production caller: hagency::bootstrap::fleet::Service::reattach_known_agents
  Given a provision the factory completed, and ones an operator adopted, a revocation retired or a payload change altered
  When the repository is reopened and asked for the scope
  Then only the factory's own completion yields a scope, equal to the original claim, warm-claimable once and never completable again

Scenario: A completed home reopens without a write and refuses when changed
  Test: native_managed_home_reopens_after_a_restart
  Production caller: hagency::bootstrap::fleet::Service::reattach_known_agents
  Given the home a completed provision created
  When the plan reopens it after a restart
  Then the same workdir is returned and nothing on disk changes
  And a changed binding or a missing custody record refuses

Scenario: A re-attach reads the stored credential and can never register
  Test: native_token_account_reattach_only_reads
  Production caller: hagency::bootstrap::fleet::Service::reattach_known_agents
  Given empty custody, then a completed registration
  When the account is re-attached
  Then empty custody refuses before any request, a completed one issues exactly one GET whoami and writes nothing
  And a token the homeserver no longer knows is refused without registering again

Scenario: A cancelled read retires neither the transport nor the room
  Test: native_matrix_cancelled_read_retires_nothing
  Given an available transport with its room observed
  When the caller cancels a collection during whoami, and again during the room-state read
  Then the transport and the room stay available and the same incarnation collects again

## Out of scope

A home whose task-client binary changed (its binding fails closed), agents on
provider-managed accounts, and the retained product's thread notice on
restart-settled dispatches remain separate.
