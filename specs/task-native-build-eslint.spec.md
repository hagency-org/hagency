spec: task
name: "Keep native build artifacts outside the JavaScript identifier gate"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, build, node, regression]
---

## Intent

Concurrent Cargo compilation removes temporary target directories while ESLint
walks the repository. Exclude generated Rust target trees from JavaScript lint
traversal while retaining the real native JavaScript build scripts in the gate.

## Constraints

### Must
- Keep undefined identifiers fatal in real source files.
- Exclude root and nested Cargo target trees through global ESLint ignores.
- Preserve the original failed verify-ci result separately from corrected runs.

### Must Not
- Never ignore native source trees or disable the no-undef rule.

## Boundaries

### Allowed Changes
- ./eslint.config.js
- tests/undefined-identifier-gate.test.js
- specs/task-native-build-eslint.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Generated Rust artifacts do not participate in JavaScript lint
  Test: Rust targets are ignored while native JavaScript remains checked
  Given the actual ESLint configuration and an undefined-identifier probe
  When checking root and nested target paths and a native JavaScript source path
  Then target paths are ignored and the source probe fails with no-undef

Scenario: Real repository source still passes the identifier gate
  Test: the repository currently has NO undefined identifiers
  Given the complete repository source
  When running the actual ESLint binary
  Then no undefined identifiers are accepted or reported
