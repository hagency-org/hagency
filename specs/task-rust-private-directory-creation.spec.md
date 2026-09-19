spec: task
name: "Create fresh private media directories with the required owner"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, windows, private-storage]
---

## Intent

Make atomic fresh directory creation establish the same private owner and ACL
that existing validation requires, without repairing existing directories.

## Decisions

Retain [ADR-066](../knowledge/decisions/adr-066-native-media-staging.md),
[ADR-101](../knowledge/decisions/adr-101-native-file-service-integration.md), and
[ADR-104](../knowledge/decisions/adr-104-native-windows-directory-sync.md).
The original Windows run 34622719221 reaches private_policy_refused after atomic
default directory creation. Rust passes NULL security attributes: ACL inheritance
does not imply TokenUser ownership. A token with a different default owner creates
a directory rejected by the unchanged current-user checker. The original CI
does not expose which SID predicate failed; that distinction remains explicit.

Add one creation-only API that uses the existing explicit current-user owner and
protected private DACL at Windows creation, and mode 0700 on Unix. Preserve the
actual create result and validate without recreating a disappeared entry.

## Constraints

### Must
- Use one atomic nonrecursive directory creation and report AlreadyExists from that actual operation.
- Establish the existing explicit Windows owner and DACL during creation before any private bytes are written.
- Retain all existing current-user, private-ACL, directory and no-follow validation.
- Keep Store creation restricted to the actual fresh directory result; existing missing journals remain refused.
- Exercise actual current-token owner behavior on Windows without changing the process or thread token.
- Report local Unix execution and Windows compilation separately from required hosted Windows execution.

### Must Not
- Do not reseal, chmod, reown, truncate, repair or replace an existing directory.
- Do not broaden accepted SIDs, ACL entries, reparse objects, filesystem profiles or privileges.
- Do not alter deadlines, retries, service workers, SQLite behavior, test parallelism or sync evidence.
- Do not claim the original CI owner SID was observed or treat a cross-compile as Windows runtime proof.

## Boundaries

### Allowed Changes
- native/hagency-store/src/private.rs
- native/hagency-store/src/private/windows.rs
- native/hagency-store/src/private/windows/directory_tests.rs
- native/hagency/tests/file_service/shutdown.rs
- native/hagency/src/file_service/recovery.rs
- specs/task-rust-private-directory-creation.spec.md
- knowledge/context/native-private-directory-creation.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Fresh private creation preserves existing objects and actual freshness
  Test: native_private_directory_creation
  Level: integration
  Test Double: actual disposable platform directories and original current process token
  Given a fixed existing parent and fresh and existing child entries
  When the creation-only API creates a fresh directory and encounters existing entries
  Then the fresh directory passes the unchanged private check
  And existing entries are refused without replacement or permission changes
  And Windows assertions inspect the actual default owner separately from the explicitly created owner

Scenario: Media journal creation retains its original atomic admission and lock
  Test: native_file_service_shutdown_media_creation_requires_atomic_fresh_directory
  Level: integration
  Test Double: actual private media directory and original journal handles
  Given a fresh directory and an existing directory without a journal
  When actual recovery opens storage and another open encounters its live original lock
  Then fresh creation succeeds with exclusive original custody
  And missing original journals remain refused without repair

Scenario: A real missing-journal refusal remains separate from server startup
  Test: native_file_service_media_startup_observation
  Level: integration
  Test Double: actual native service child and local TLS peer with an existing empty private media directory
  Given an existing media directory without its original journal
  When the real service reaches media startup
  Then the original outcome remains unknown with zero attempts and no journal creation
  And the original child still reports the distinct store refusal phase

## Out of Scope

Hosted workflow dispatch, observed Windows SID claims before native execution,
SQLite shutdown repair, general filesystem support, additional production tracing,
new Windows token privileges and migration activation.
