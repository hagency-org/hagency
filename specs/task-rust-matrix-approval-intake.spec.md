spec: task
name: "Authenticated private Matrix owner verdict intake"
inherits: project
satisfies: [REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, approval]
---

## Intent

Derive bounded owner decisions from authenticated private Matrix SDK events and
settle existing native approval requests without changing Agent chat transport.

## Constraints

### Must
- Use a distinct host-owned approval SDK purpose identity binding and sync cursor.
- Resolve owner bot project request device and binding scopes from current domain truth before network intake.
- Verify exact whoami full private membership and fresh signed device identities including current published bot identity.
- Construct verdict observations only from owned SDK verified encrypted events from the exact owner without forwarded or plaintext proof.
- Freeze the exact request digest action scope raw response targets and crypto derivation before domain handoff.
- Revalidate current private binding request expiry dispatch task and resource scope atomically with grants and source receipts.
- Preserve SDK Applying uncertainty and exact accepted historical receipt recovery without new authority after rotation.
- Retain rejected source outcomes and content-bound replay history with finite capacity instead of retargeting or eviction.
- Preserve existing outgoing and Agent intake behavior while factoring only shared bounded verification logic.

### Must Not
- Do not expose arbitrary Matrix JSON crypto proof grant or verdict setters to runtime HTTP.
- Do not interpret chat text edits public room replies browser assertions or wrong-owner events as approval.
- Do not manufacture native application proof enable live services or claim approval card compatibility.
- Do not mix native request ID format with the legacy parser or silently weaken that parser.

## Boundaries

### Allowed Changes
- native/hagency-matrix/**
- native/hagency-core/src/approvals.rs
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/approvals/**
- specs/task-rust-matrix-approval-intake.spec.md
- knowledge/decisions/adr-064-native-matrix-approval-intake.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Native schema migrations runtime application adapters service wiring credentials and other worktrees.

## Acceptance Criteria

Scenario: Authenticated approval owner and bot remain isolated from Agent transport
  Test: native_matrix_approval_identity
  Given current native approval room bindings and an independent Agent SDK route
  When the host approval reader verifies account device and full private room snapshots
  Then only exact registered approval authority is observed and Agent transport remains unchanged

Scenario: Verified encrypted owner actions settle only their immutable requests
  Test: native_matrix_approval_verdict
  Given real offline cross-signed identities and pending native requests
  When actual local HTTPS sync supplies valid encrypted structured verdicts
  Then once task always and deny map only to the frozen request digest and supported stored scope

Scenario: Public stale or altered owner evidence cannot grant authority
  Test: native_matrix_approval_scope
  Given plaintext wrong-owner third-member stale device expiry or changed request evidence
  When verdict admission races with negative room or task observations
  Then no stale grant is committed and shared negative evidence retires old approval bindings

Scenario: Interrupted crypto and domain handoff retain exact custody
  Test: native_matrix_approval_recovery
  Given a captured sync response interrupted SDK apply or lost domain result
  When cancellation restart or registration rotation occurs
  Then applying stays inspectable and exact accepted historical receipts settle without retargeting

Scenario: Persistent rejected outcomes and capacity prevent reinterpretation
  Test: native_matrix_approval_bounds
  Given malformed oversized duplicate or replayed events and finite receipt storage
  When requests or target plans change and the reader reopens
  Then original rejected outcomes remain rejected and capacity never evicts pending or dedup history

## Out of Scope

Live account provisioning device key publication missing-session maintenance
approval card delivery Robrix legacy parser changes native decision application
receipt compaction deployment and overall migration cutover remain separate gates.
