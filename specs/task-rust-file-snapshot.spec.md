spec: task
name: "Native workspace capability file snapshots"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, files, custody]
---

## Intent

Copy bounded immutable file bytes from a retained host workspace capability for
ADR-027 without adding runtime paths, network delivery or a second domain store.

## Constraints

### Must
- Accept workspace authority only as an already host-opened directory handle without an ambient path constructor.
- Retain every opened ancestor and source handle through snapshot custody.
- Open one validated relative path component at a time without following symlinks or Windows reparse points.
- Reject absolute traversal alias ambiguous nonregular hardlinked and oversized selections.
- Copy bounded bytes and calculate the digest from the copied bytes without claiming atomic source consistency.
- Bound retained snapshots and allocation with a host limit and release ownership on failure or Drop.
- Keep content source paths physical metadata and private identity out of Debug serialization and error projections.
- Document private provisioning mount hardlink and same-size mutation limitations precisely.
- Pin inspected dependencies while preserving every existing lockfile package version.
- Verify Windows retained directory handles deny rename with ERROR_SHARING_VIOLATION and that the same rename succeeds after custody is released.

### Must Not
- Do not add arbitrary ambient filesystem access from runtime arguments or external JSON.
- Do not modify private storage helpers or connect this primitive to Matrix network outboxes services or domain schemas.
- Do not claim regular-file reads have cancellable kernel deadlines or immutable source files.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-files/**
- specs/task-rust-file-snapshot.spec.md
- knowledge/decisions/adr-058-native-workspace-file-snapshots.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Other crates production JavaScript services credentials domain schemas another checkout and live workspace files.

## Acceptance Criteria

Scenario: Host capability selects only explicit relative files
  Test: native_file_snapshot_capability
  Level: integration
  Test Double: real temporary filesystem and host-opened directory
  Given a retained host workspace handle and scoped relative selection
  When nested Unicode files are snapshotted
  Then immutable copied bytes digest and private source custody derive from that handle
  And no ambient path constructor or content metadata serialization exists

Scenario: Names and links cannot widen file selection
  Test: native_file_snapshot_paths
  Level: integration
  Test Double: real filesystem links and portable path rejection vectors
  Given ordinary absolute traversal ambiguous and link paths
  When snapshot selection walks every path component
  Then absolute parent symlink reparse nonregular and hardlinked inputs are rejected
  And no source outside a retained directory handle is selected by replacement

Scenario: Opened object custody survives path replacement
  Test: native_file_snapshot_custody
  Level: integration
  Test Double: real POSIX replacements and Windows sharing-denied directory renames before controlled handle release
  Given an opened ancestor or source handle
  When an adversary attempts to replace an ancestor pathname or leaf and source bytes later change
  Then reading keeps original opened-object custody and an existing snapshot stays unchanged
  And Windows directory replacement is denied while handles are retained and succeeds after their release
  And observed source mutation is refused without claiming detection of all same-size mutations

Scenario: Snapshot capacity remains finite across concurrent owners
  Test: native_file_snapshot_bounds
  Level: integration
  Test Double: finite shared permits real oversized files and controlled concurrent snapshot calls
  Given explicit per-file and retained-snapshot limits
  When allocation reads failures and retained snapshots compete for capacity
  Then byte and handle ownership remain bounded and failures or Drop release permits

Scenario: Platform file objects enforce explicit supported boundaries
  Test: native_file_snapshot_platform
  Level: integration
  Test Double: actual local Windows reparse and Unix FIFO fixtures with pinned safe filesystem APIs
  Given supported native filesystem handles and platform-specific file types
  When regular files directories links junctions or FIFOs are encountered
  Then supported platforms reject nonregular and redirecting objects without reading them
  And unavailable fixture prerequisites fail visibly rather than becoming proof

## Out of Scope

Physical private workspace provisioning external mount or hardlink prevention
atomic point-in-time source consistency cancellable regular-file syscalls
persistent staging restart identity authorization APIs Matrix media outboxes
network services runtime wiring deployment and migration cutover remain separate.
