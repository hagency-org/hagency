spec: task
name: "Retain the original pre-activation runtime and consume it on its owning reactor"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runtime, provisioning, custody]
---

## Objective

Implement genuine pre-activation ownership of the existing fixed native session,
with its initialized process, streams and current-thread IO reactor retained by
one worker. Consume that same worker/session in the original acknowledged owned
dispatch path, not a second launch or an initialized unused probe. Physical
account/SDK/Applied/Active/routes and complete configured fleet integration remain
required by the full goal; no new bootstrap profile exposes this prerequisite.

## Constraints

- Fixed typed Host, actual writer-produced original provision scope and actual
  opaque managed home. Resource/account/registration derive from current original
  store state and frozen approved payload, not caller-supplied model/cwd/receipts.
- Private scope has no Debug/serde or runtime constructor. Fresh current checks
  cover exact effect/fence/payload, registration, Reserved/Started or original
  Active/Complete, frozen qualified profile, account association/readiness and
  retained home. Reads create no Applied/task/session/approval authority.
- Exactly one owner/worker/result and bounded command slot, one consumed dispatch.
  Scopes pin their actual producing writer's private owner and share one sticky
  claim through its finite non-evicting registry; foreign/reopened writers refuse.
  No detached launcher, stream migration/new IO reactor, adoption, rearm or cold
  fallback after unknown warm ownership. Original synchronous join/drop cleanup
  and retained cancellation/start/spawn/settlement uncertainty stay intact.
- Use the existing OwnedSession::spawn/SupervisedProcess and shared owned worker
  execution/finalization. Fixed argv stays app-server, private reference inherited
  before capability exists, actual acknowledged Started before helper/task IO.
- A typed Ready/no-helper idle lifecycle may replace only its idle bound and be
  consumed once by original absolute dispatch deadline/limits. No extension of
  an active or normal driver, resetting correlation/connection, or arbitrary
  configuration/permission mutation. Warm idle cannot issue thread/turn requests.
- Default direct/cold path, workspace-write/on-request/network-disabled and fixed
  MCP tool policy remain unchanged. Warm readiness is not model/sandbox proof.
  Initial deadline includes preparation/queue delay. The existing live slot is
  reserved before possible spawn, transferred rather than reserved twice, and
  unknown or pending late startup cannot free it without observed full stop.
- Caller loss retains the actual operation/cleanup and fixed job; unknown or
  partial result cannot be replaced. Failed admission stops retained warm owner
  without starting a task or writing context. No fabricated canonical Done/reply.
- Offline local fixtures only; no live Cargo tests, formatter, external dependency,
  deployment/reset/commit/PR. No completion or lifecycle claim from listing.

## Allowed changes

- native/hagency-store/src/domain/provision_runtime.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/domain/accounts.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/agent_home.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/provision_runtime.rs
- native/hagency-execution/src/warm.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/lib.rs
- native/hagency-runtime/src/codex/transport.rs
- native/hagency-runtime/src/codex/session/driver.rs
- native/hagency-runtime/src/owned/session.rs
- native/hagency-runtime/tests/session.rs
- native/hagency/tests/warm_runtime.rs
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- specs/task-rust-preactivation-owned-runtime.spec.md
- knowledge/decisions/adr-053-native-owned-dispatch.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- docs/agent-knowledge.md
- docs/progress.md

## Scenarios

Scenario: Only original qualified current provision scope admits warming
  Test: native_warm_runtime_writer_scope
  Given actual Started/Reserved writer state and a fixed selected account/profile
  When the private scope is captured and revalidated
  Then it derives the original frozen configuration without creating authority

Scenario: Changed failed foreign scope cannot admit or retain readiness
  Test: native_warm_runtime_scope_refusals
  Given mismatched effect/registration/resource or failed current custody
  When warming or current validation is requested
  Then it refuses without reconstructing or replacing the original owner

Scenario: Managed warming requires actual current unexpired account observation
  Test: native_warm_runtime_managed_readiness
  Given the actual associated managed account and its writer-settled login facts
  When a missing expired newer refused or retired fact is checked
  Then warming refuses and an observed current fact alone permits its scope

Scenario: Warm idle is consumed once under the original dispatch deadline
  Test: native_runtime_warm_idle_dispatch_lifetime
  Given actual initialized Ready session with no helper
  When it waits past its initial IO lifetime then consumes its scoped deadline
  Then the same driver retains correlation/policy and cannot extend or rearm

Scenario: Same initialized owner runs actual native task helper
  Test: native_pre_activation_warm_owned_runtime
  Given original materialized home and Started/Reserved effect before task exists
  When warm ready precedes activation then actual dispatch consumes it
  Then original worker/reactor/process initializes once and native MCP maintains the real task

Scenario: Invalid warm dispatch does not cold-fallback or start work
  Test: native_warm_owned_runtime_refusals
  Given original warm owner and foreign revoked expired or cancelled dispatch
  When handoff is admitted once
  Then the retained owner stops and no alternate launch/helper/task context appears

Scenario: Lost warm-ready receiver keeps actual owner until explicit handoff or stop
  Test: native_warm_owned_runtime_custody
  Given retained original initialization and a polled wait
  When that receiver is dropped
  Then the same owner remains inspectable/consumable or explicitly stopped without replacement

Scenario: Possible late startup cannot free the original live slot
  Test: native_warm_owned_runtime_unknown_capacity
  Given an actual shared launcher stalled beyond its original budget
  When original startup is unknown and the actual late return is joined
  Then its live capacity remains held without observed full cleanup and a third owner is refused
