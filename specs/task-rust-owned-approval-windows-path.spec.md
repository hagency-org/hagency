spec: task
name: "Project retained canonical Windows workspace approval metadata"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
tags: [active, rust, approvals, windows, ownership]
---

## Intent

Implement accepted ADR116 in the root-approved seven-path partition. Original
Windows CI reaches the native turn but rejects the retained canonical workspace
at approval context binding because the generic path parser refuses its verbatim
namespace. Keep launch and filesystem custody unchanged.

## Constraints

- Only the original retained Root can request this private approval metadata
  projection. Parse Windows path prefix/components; never blindly strip a prefix.
- Support canonical disk and UNC disk paths; reject device namespaces and forms
  whose projection would introduce ambiguous Win32 aliases.
- Keep the directory handle and callback params unchanged. The launch and
  session working directory string is the ordinary projection of the retained
  root (ADR-116 amendment); custody never derives from that string.
- Generic untrusted PathFlavor parsing remains unchanged. An unrepresentable
  callback cwd has no reusable scope and remains Once/Deny only.
- Preserve exact grants, original deadlines, sandbox defaults and no Applied claim.
- Actual drive directory custody and native continuation require Windows execution;
  lexical UNC/domain tests do not prove a live network share.

## Boundaries

### Allowed Changes
- native/hagency-execution/src/workspace.rs
- native/hagency-execution/src/workspace/approval_path.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/tests/owned.rs
- native/hagency/tests/owned_mcp.rs
- native/hagency/tests/received_files.rs
- native/hagency-execution/tests/owned/approvals.rs
- specs/task-rust-owned-approval-windows-path.spec.md
- knowledge/decisions/adr-116-owned-approval-windows-path.md
- docs/progress.md

### Forbidden
- All paths outside this exact partition; generic path-parser or callback rewriting; all live operations.

## Acceptance Criteria

Rule: original-workspace-projection — Approval metadata derives only from retained host custody

Scenario: Canonical workspace projections preserve exact filesystem ownership
  Test: native_owned_approval_workspace_projection
  Given the original retained canonical workspace directory and typed Windows path prefixes
  When its private approval metadata is projected
  Then disk and UNC forms normalize while device namespaces ambiguous aliases and relative paths refuse

Scenario: Request cwd remains independent original evidence
  Test: native_owned_approval_workspace_scope
  Given a projected host workspace and unchanged native callback parameters
  When reusable command scope is derived
  Then ordinary disk and UNC cwd are representable while verbatim or device callback strings remain unrepresentable

Scenario: Actual owned callbacks continue from canonical workspace custody
  Test: native_owned_approval_resume
  Given the actual canonical original workspace and owned child
  When the exact owner allows or denies its persisted callback
  Then the projected context retains exact workspace association and one original response continues without Applied

## Out of Scope

Generic device-path support, callback metadata rewriting, live UNC provisioning,
production cutover and proof of upstream native permission application.
