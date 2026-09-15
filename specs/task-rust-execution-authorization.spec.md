spec: task
name: "Derive bounded native execution permission scopes"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-EXECUTION-AUTHORIZATION]
tags: [active, rust, approval, security]
---

## Intent

Port the pure execution-policy and reusable permission-scope rules to Rust.
Keep scope derivation distinct from an authenticated owner decision, persistent
grant, runner permission response and effective OS sandbox.

## Constraints

### Must
- Default to sandboxed execution; accept YOLO configuration only for Codex.
- Derive reusable scopes only from bounded host-owned structured runtime metadata.
- Match exact command, working directory, workspace, write capability and environment; never infer a domain from command or reason text.
- Admit only explicit supported network and filesystem permission shapes.
- Normalize paths using an explicit host path flavor, independently of the test OS.
- Bound descriptions, metadata, arrays and nesting; reject unknown escalation fields.
- Keep unrepresentable requests eligible only for the future once-or-deny adapter.
- Generate shared vectors from the existing JavaScript implementation and execute all tests offline.

### Must Not
- Do not expose authorization metadata as a writable runtime HTTP DTO.
- Do not implement an approval by text, automatically allow an operation or persist a grant in this pure module.
- Do not claim lexical path normalization proves filesystem containment or sandbox enforcement.
- Do not enable native Agent execution or change deployed policy.

## Boundaries

### Allowed Changes
- native/hagency-core/src/lib.rs
- native/hagency-core/src/execution.rs
- native/hagency-core/src/execution/**
- native/hagency-core/tests/execution.rs
- native/fixtures/execution.json
- native/scripts/execution-vectors.mjs
- .github/workflows/rust.yml
- specs/task-rust-execution-authorization.spec.md
- knowledge/decisions/adr-039-native-execution-authorization.md
- docs/**

### Forbidden
- Live services, credentials and the original dirty checkout.

## Acceptance Criteria

Scenario: Native scope derivation agrees with supported legacy inputs
  Test: native_execution_vectors
  Given independently generated command network and permission-profile vectors
  When Rust derives scopes using the same path flavor
  Then scope identities descriptions and policy validation agree
  And YOLO defaults off and rejects non-Codex enablement

Scenario: Permission context and exact commands cannot widen grants
  Test: native_execution_context
  Given an exact command or a structured network request
  When workspace write capability environment command or destination changes
  Then the scope changes or becomes unsupported
  And changing reason or upstream identifiers cannot grant wider authority

Scenario: Invalid unsupported and excessive metadata cannot create reusable permission
  Test: native_execution_bounds
  Given unfamiliar escalation fields malformed profiles and oversized metadata
  When reusable scope derivation is attempted
  Then no scope is returned and no authority is granted
  And unsupported requests remain once-or-deny only

Scenario: Paths remain explicit across native operating systems
  Test: native_execution_paths
  Given POSIX drive and UNC paths including Unicode and lexical parent components
  When the declared host flavor normalizes them
  Then exact normalized context is retained without IO or case folding
  And relative drive-relative and Windows device paths cannot become reusable scopes

## Out of Scope

Persistent grants, binding/owner generations, verdict identity, task epochs,
private cards, runner approval responses, YOLO dispatch, effective sandbox and
OS path-containment enforcement. These remain required M4/M6 integrations.
