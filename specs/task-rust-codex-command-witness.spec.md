spec: task
name: "Bind sandbox qualification to the observed command and working directory"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, codex, sandbox, qualification]
---

## Intent

Qualify the actual intended disposable write, not any failed tool or absent
file. Keep upstream observations separate from host execution authority.

## Constraints

### Must
- Mint command evidence only after the original session accepts exact thread/turn/item lifecycle.
- Retain exact command and cwd privately, each at most 8192 bytes, within the original 128-item bound. Expose equality predicates only, no automatic Debug/Serialize/content projection.
- Treat missing, malformed, nonabsolute cwd or oversized metadata as no matching command witness, never inferred intent from a prompt.
- Invalidate changed command/cwd between item phases or contradictory supplied terminal snapshots. Consumers must reject invalidated evidence.
- Match the operator's single shell-quoted disposable target and original cwd exactly. Permit only explicitly enumerated exact shell renderings, never substring/target-only matching.
- Count distinct observed execution item IDs. Qualification requires one matching command, no unrelated tools, and actual whole-tree cleanup.
- Inside qualification requires matching completed execution, exact expected file bytes, completed turn and no stream error.
- Outside qualification requires no target file, matching failed execution or matching ungranted command approval, no stream error and cleanup.
- Match approval scope to the original thread/turn/item, exact command/cwd and reject network/file/unrelated approvals as write witnesses. Never grant or retry.
- Bind compiled observation/probe implementation into the recorded source digests, preserving the exact pinned executable/version and separate versioned qualification records.
- Keep a no-attempt direct-path verdict failed. A supplementary symlink-escape case must be separate, bind the link's exact fresh outside target before/after the owned session, and require the same exact command/cleanup witness. It does not replace either original gate.
- Keep deterministic tests offline. Actual model runs are operator examples only.

### Must Not
- Do not weaken workspace-write/on-request/user/network-disabled policy or use host command execution as a substitute for the owned-session attempt.
- Do not report missing evidence, explanation-only responses, unrelated failures or a failed stream as successful sandbox enforcement.
- Do not equate qualification on an isolated namespace-enabled host with qualification of the default native container or full-port parity.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/codex/session/observation.rs
- native/hagency-runtime/tests/session.rs
- native/hagency-runtime/tests/session/command_witness.rs
- native/hagency-execution/examples/codex_qualify.rs
- native/hagency-execution/examples/codex_qualify/witness.rs
- native/hagency-execution/qualification/source_digests.rs
- native/hagency-execution/tests/qualification.rs
- specs/task-rust-codex-command-witness.spec.md
- knowledge/decisions/adr-140-real-codex-sandbox-qualification.md
- docs/progress.md

## Acceptance Criteria

Scenario: Original scoped runtime events bind exact private command metadata
  Test: native_codex_command_witness_exact
  Given real bounded duplex session IO and accepted command item phases
  When expected command/cwd match or metadata is missing or substituted
  Then only the exact original command matches and changed evidence invalidates

Scenario: Contradictory terminal metadata cannot preserve a positive witness
  Test: native_codex_command_witness_terminal
  Given a completed command and an exact current terminal snapshot
  When supplied command/cwd changes or metadata exceeds its bound
  Then contradiction invalidates and malformed metadata cannot mint a witness

Scenario: Operator evidence fails closed on generic or unrelated effects
  Test: native_codex_command_probe_witness
  Given exact and unrelated command/approval fixtures
  When one matching write or generic failed/unrelated/repeated effects are observed
  Then only the exact one-command probe can qualify and no approval is granted

## Out of Scope

Actual operator evidence capture is a separate required run, not an offline test.
Default-container kernel qualification, other platforms, full approval/media,
fleet/lifecycle parity and production cutover remain required separately.
