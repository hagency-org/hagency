spec: task
name: "Hand one acknowledged dispatch to the initialized owned runtime helper"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runtime, custody, mcp, retained]
---

## Objective

Implement the missing task-context bridge for genuine original inline runtime
launch/handoff. An initialized process cannot inherit a capability minted later.
The explicit Host opt-in projects only a fixed private record reference into the
parent environment; after acknowledged Started the same owner creates that exact
context once, binds the fixed helper, and starts its thread. Native MCP loads the
original capability/task once before initialization and continues checking current
API authority on every call. This is a prerequisite, not full pre-activation warm
ownership, Applied/Active/routes, either deployment profile or live qualification.

## Constraints

- Keep the existing direct inherited capability path and its ABI unchanged.
- The new Host-only immutable context has no serde/debug/secret getter, arbitrary
  helper argv, public result setter, runtime JSON constructor or console grant.
- Fixed already-private canonical root lies outside the dispatched workspace.
  Retain root/file identity and exact record bytes. One original non-evicting job;
  create-only record, no adoption/repair/new context after failed or unknown bind.
- Record contains only original writer-produced Started scope/task/capability and
  fixed context ID. Require its sealed private writer Started marker, not a later
  current snapshot or task status. Check the actual current dispatch before and after filesystem
  work, under unchanged original operation/domain deadlines. Caller loss retains
  the original job/IO through return. Partial or changed file/root refuses replay.
- The parent inherits only the non-secret reference, loopback address and context
  marker; actual secret stays private Host memory/record/helper/API, not protocol,
  tool arguments, receipts, console or logs. The record is private operational
  dispatch custody, never a canonical agent/side credential or persistent grant.
- Load a bounded regular private file once at native helper startup; exact closed
  reference/record grammar and marker. No environment/file fallback, reload or
  adopting a different task. Forged/stale capabilities remain API refusals.
- Bind only a fixed typed TaskMcp once while the same initialized driver is Ready
  with no helper. Never replace cwd/model/effort/policy, reset a session or bind
  after thread opening. No runner/approval grant is minted by that binding.
- The existing sealed Started-marker validator becomes crate-internally visible
  only. No public getter, constructor, serde projection or acknowledgement recovery.
- Existing dispatch still receives Started before spawn, one original guardian /
  OwnedSession, same IO reactor, app-server-only argv and unchanged cleanup/Done/
  reply settlement. Delay context/helper binding until actual initialize returns.
- Default sandbox remains workspace-write/on-request/network disabled and fixed
  task-tool policy unchanged. No effective sandbox or warm readiness from an echo.
- No new dependencies, formatting, live Cargo tests, deploy/reset/commit/PR. New
  bootstrap/live factory profile is not exposed before its real owner is complete.

## Allowed changes

- native/hagency-store/src/task_context.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/tests/task_context.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/operation.rs
- native/hagency-runtime/src/codex/session/driver.rs
- native/hagency-runtime/src/owned/session.rs
- native/hagency-runtime/tests/session.rs
- native/hagency/src/task_client.rs
- native/hagency/tests/owned_mcp.rs
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- specs/task-rust-retained-runtime-task-context.spec.md
- knowledge/decisions/adr-057-native-owned-mcp-launch.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- docs/agent-knowledge.md
- docs/progress.md

## Scenarios

Scenario: Only actual acknowledged Started scope creates private context
  Test: native_retained_task_context_bind
  Given original claimed scope and a fixed retained private reference
  When actual writer start is received and context binds
  Then exact original task/fence is stored privately and replay only inspects

Scenario: Foreign partial public changed or unstarted context refuses
  Test: native_retained_task_context_refusals
  Given invalid custody or absent current original start authority
  When context binds or replays
  Then no context replacement adoption or new write is permitted

Scenario: Dropped caller retains the original context owner
  Test: native_retained_task_context_custody
  Given one admitted original context job under actual dispatch authority
  When its first polled receiver is dropped
  Then the original task settles and duplicate running bind cannot rearm

Scenario: Typed helper binds only once to the initialized driver
  Test: native_runtime_late_task_helper
  Given the same actual Ready session with no helper
  When the fixed typed helper binds and its thread starts
  Then original runtime policy stays fixed and other stages/rebinding refuse

Scenario: Native helper loads only exact private original context
  Test: native_task_client_retained_context
  Given fixed inherited reference and actual private record bytes
  When helper startup loads once
  Then malformed missing foreign public aliased oversized or extra data refuses without fallback

Scenario: Same original owned process launches the real helper after initialize
  Test: native_owned_mcp_retained_task_context
  Given fixed native owner with a reference but no inherited dispatch secret
  When initialize returns then context and typed helper bind
  Then real native MCP reads and maintains the original task through the existing API, with no secret in parent protocol/receipt
