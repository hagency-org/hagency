spec: task
name: "Compare retained regular-file objects for received attachments"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, files, platform]
---

## Intent

Implement only the retained regular-file comparison prerequisite of accepted
ADR105. This comparison observes object identity; it grants no workspace access
and does not implement a receive sink or qualify the complete file workflow.

## Constraints

### Must
- Compare only two live borrowed regular-file handles without opening an ambient path.
- Preserve the full Unix device and inode or Windows volume and 128-bit file identifier.
- Refuse directories unsupported targets and metadata or native query failures.
- Keep borrowed handles alive and initialize native output with exact size and alignment before the synchronous call.
- Verify real duplicate reopened aliased and replaced file objects and directory refusal.

### Must Not
- Do not infer private permissions link safety content equality durability or authorization from equal identities.
- Do not change directory comparison dependencies sandbox settings or deployment capabilities.
- Do not replace a failed native identity query with pathname or truncated-identifier comparison.

## Boundaries

### Allowed Changes
- native/hagency-platform/src/lib.rs
- native/hagency-platform/src/file_identity.rs
- specs/task-rust-received-file-identity.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Live regular-file identity distinguishes original objects from replacement
  Level: integration
  Test Double: actual temporary files duplicate handles and filesystem names
  Test: native_receive_file_identity
  Given a retained original regular file with duplicate reopened and hard-linked handles
  When it is compared with those aliases a distinct file and a fresh object at its old name
  Then only the original object aliases match and equal content does not imply equal identity
  And directory handles in either position refuse instead of matching or using a pathname fallback

## Decisions

This five-path prerequisite was reviewed against ADR105 and the pinned platform
interfaces before implementation. Run its actual selector and existing platform
library tests plus native and Windows GNU Clippy with warnings denied. Run strict
lifecycle with all actual changed paths. Cross-compilation is not Windows execution;
actual hosted Windows identity and the later materialization workflow remain
independent gates. Retain the original 157 knowledge-governance baseline errors.

## Out of Scope

Received-file reservation plaintext writes Matrix download current authorization
private ACL creation directory synchronization cleanup and production cutover.
