spec: task
name: "Qualify Linux guardian death recovery through a protected cgroup"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, linux, custody]
---

## Intent

Add an explicit host-only cgroup recovery boundary to the existing guardian
launcher without advertising complete POSIX crash containment.

## Constraints

### Must
- Establish membership before sending Prepare or Start to the trusted guardian.
- Validate exact cgroup filesystem, control descriptors, protected ancestors and host privileges.
- Retain independent kill and bounded empty-subtree observations after guardian loss.
- Preserve uncertain cleanup and keep complete crash containment unsupported.
- Require privileged host provisioning and separate real Linux fixture qualification.
- Label local parser and refusal tests as admission evidence only.
- Require verified current initial user and cgroup namespaces on source-inspected kernel families, including after guardian exec.
- Qualify actual guardian loss and nested namespace refusal only through a disposable GitHub-hosted Linux CI provisioner.
- Retain independent root cleanup for exactly the provisioner's newly created subtree on every helper outcome.
- Stage exactly two bounded fixed offline probe binaries into a fresh retained root-owned traversable directory for hosted namespace tests.
- Preserve source and checkout permissions and refuse staged path replacements during cleanup.

### Must Not
- Do not mount or change existing system cgroups; the CI-only provisioner may create and remove exactly one fresh private subtree and change only its own control files.
- Do not execute privileged cgroup qualification on local machines, Mini1 or self-hosted runners.
- Do not infer signal authority from a PID or add a second workspace launcher.
- Do not settle canonical tasks or resource leases from process observations.
- Do not treat absent delegation, skipped fixtures or cross-compilation as containment proof.
- Do not accept executable permission errors or exit126 as a native namespace refusal.

## Boundaries

### Allowed Changes
- native/hagency-platform/**
- native/scripts/qualify-linux-cgroup.py
- native/scripts/test-qualify-linux-cgroup.py
- .github/workflows/rust.yml
- knowledge/decisions/adr-048-native-linux-crash-custody.md
- specs/task-rust-linux-crash-custody.spec.md
- docs/**

## Acceptance Criteria

Scenario: Invalid cgroup observations cannot authorize recovery
  Test: native_cgroup_admission_vectors
  Test Double: bounded kernel text and metadata vectors
  Given invalid event state host capabilities or mount observations
  When the admission parser evaluates those observations
  Then it refuses the boundary without treating the vectors as kernel containment evidence

Scenario: Existing POSIX launch keeps its crash guarantee refusal
  Test: native_guardian_admission
  Level: integration
  Test Double: actual native guardian with no provisioned cgroup
  Given no protected host cgroup capability
  When a launch requires complete crash containment
  Then POSIX refuses before workspace code starts

Scenario: Relative namespace roots cannot hide migration authority
  Test: native_cgroup_namespace_vectors
  Test Double: namespace filesystem type inode and kernel release vectors
  Given a root-looking mount inside a nested user or cgroup namespace
  When current namespace admission is evaluated
  Then noninitial unknown and mismatched identities are refused without claiming real namespace qualification

## Decisions

Real Linux cgroup execution is a separate opt-in fixture with mandatory host
provisioning. Missing provisioning is a refusal, not a passing containment test.

## Out of Scope

Simultaneous backend and guardian death recovery by the runtime, production host privilege provisioning, live models,
actual sandbox qualification, server enablement, macOS containment and full M4.


CI-only executable staging is additionally verified by the exact ordinary-file
Python suite `python3 -B native/scripts/test-qualify-linux-cgroup.py`. Cargo-bound
lifecycle scenarios above retain native admission/refusal coverage; they do not
claim Python staging or actual hosted namespace execution. The hosted seven-mode
qualification remains a separate mandatory workflow command and requires exact
native exit78/namespace diagnostics for nested cases. Local ordinary-file or
mode700-ancestor fixtures cannot replace that hosted result.
