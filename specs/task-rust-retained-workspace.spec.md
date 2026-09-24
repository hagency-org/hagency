spec: task
name: "Bind owned execution and file snapshots to one retained host workspace"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, workspace, execution, custody]
---

## Intent

Retain the actual private host directory selected for owned execution and hand off
one exact post-Started file binding under the accepted trusted-ancestor contract.

## Constraints

### Must
- Open and retain each immutable bounded host workspace once and derive file snapshots from that same object.
- Keep the accepted host-exclusive stable workspace and ancestor lifetime contract explicit.
- Reject missing replaced nonprivate or inconsistent host workspace configuration before child launch.
- Produce one sealed workspace handoff only after the actual exact durable Started acknowledgement.
- Retain the original execution writer and refuse duplicate live directory aliases or canonical parent-child roots within one Host.
- Bind source access to the original full capability and frozen scope and retire it on every operation exit cancellation panic or drop.
- Retain the physical root alongside unresolved owned process cleanup and held snapshots.
- Refuse a requested file bound smaller than the retained copy profile before any source read.
- Preserve current writer validation before capture and publication as a required host consumer step.
- Compare Windows directory handles with complete FileIdInfo identity and keep comparison distinct from object custody.

### Must Not
- Do not expose an unchecked Workspace clone raw directory handle serialized binding or model-selected root.
- Do not create binding authority from a lost Started response historical lookup or path equality.
- Do not claim hostile same-UID namespace isolation or that descriptor aliases survive pinned Codex sandbox normalization.
- Do not change guardian protocol sandbox policy process ownership domain schema Matrix transport MCP tools or production activation.
- Do not add unbounded handoff queues or background work.

## Boundaries

### Allowed Changes
- native/hagency-execution/Cargo.toml
- native/hagency-execution/src/lib.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/workspace.rs
- native/hagency-execution/tests/owned.rs
- native/hagency-execution/tests/owned/workspace.rs
- native/hagency-execution/tests/support/reply_loss.rs
- native/hagency/tests/owned_mcp.rs
- native/hagency/tests/owned_matrix/support.rs
- native/hagency-platform/src/lib.rs
- native/hagency-platform/src/directory_identity.rs
- ./Cargo.lock
- knowledge/decisions/adr-093-native-retained-workspace.md
- specs/task-rust-retained-workspace.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Actual owned process and snapshots use the selected retained private root
  Level: integration
  Test Double: real offline native peer guardian or Windows Job and filesystem
  Test: native_workspace_binding_process
  Given an immutable private host root and a current leased dispatch
  When owned execution acknowledges Started and writes through its configured cwd
  Then exactly one sealed handoff snapshots those actual bytes from the same root
  And no other workspace with the same relative filename can substitute

Scenario: Source access requires the original capability and current host checks
  Level: integration
  Test Double: actual domain writer leases native process and private root
  Test: native_workspace_binding_scope
  Given a post-Started binding to one exact capability and frozen scope
  When a different secret runner fence dispatch or revoked scope attempts access
  Then static association or current writer validation refuses before permitted capture
  And current checks use the original writer even if a separate stale copied database accepts the old capability
  And no raw workspace bypass is exposed

Scenario: Every owned exit retires late handoffs without discarding retained root custody
  Level: integration
  Test Double: actual native process cancellation retained cleanup and one handoff slot
  Test: native_workspace_binding_retirement
  Given a running operation and a held or not yet taken workspace handoff
  When the operation is cancelled completed unwound after Started or dropped
  Then subsequent source access refuses even if the binding is taken later
  And already held snapshot bytes remain unchanged through cleanup

Scenario: Replaced or missing configured roots refuse while held snapshots retain their object
  Level: integration
  Test Double: actual local directory rename replacement and platform refusal
  Test: native_workspace_binding_replacement
  Given a retained host root whose configured pathname is removed or replaced before launch
  When the operation validates its host configuration
  Then it refuses instead of launching against a new object
  And Unix retained snapshots remain bound to their original object after replacement
  And Windows ordinary rename refusal is distinguished from a successful replacement

Scenario: Lost durable Started response cannot manufacture a workspace handoff
  Level: integration
  Test Double: actual domain start commit with discarded response and no child
  Test: native_workspace_binding_lost_start
  Given a real durable Started mutation whose response is lost
  When the host finishes the failed operation
  Then no child or usable workspace handoff is produced

Scenario: Root and snapshot admission remain finite and reject unsafe inputs
  Level: integration
  Test Double: actual private directories held snapshots and bounded capabilities
  Test: native_workspace_binding_bounds
  Given the bounded immutable host map and retained file snapshot permits
  When limits invalid roots oversized capability fields or exhausted permits are supplied
  Then admission refuses without creating another source authority

Scenario: Live directory comparison preserves complete platform identity
  Level: integration
  Test Double: actual distinct directory handles and exact duplicates
  Test: native_workspace_directory_identity
  Given retained handles to equal and different directory objects
  When host configuration consistency is checked
  Then only the same directory object compares equal
  And file handles or unsupported identity queries refuse without truncated identity fallback

## Out of Scope

Physical namespace isolation from malicious same-OS-identity actors privileged
mount provisioning real Codex sandbox qualification runtime file service MCP
staging upload Matrix event delivery and native production activation. The fixed
path used by Launch and Settings depends on the existing trusted ancestor
lifetime obligation and is not an adversarial replacement guarantee.
