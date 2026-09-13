---
kind: decision
id: ADR-140
title: "Qualify the real Codex sandbox behind a compiled-out feature"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [runner, codex, sandbox, qualification, testing]
---

## Context

The runner evidence review (finding 3,
`.peer/evidence/context-runner-evidence-review.md`) showed that slice A's
"skipped, and reported as skipped" is not expressible with ordinary cargo test
mechanics: a test that returns early when the real binary is absent reports
**ok/passed**, and an `#[ignore]`d test reports *ignored* while
`cargo test -- --list` still emits its selector, so
`native/scripts/check-rust-spec-bindings.mjs` binds the name and the suite
reads covered without the real child ever running. The house rule is explicit
(`docs/workspace-agents-md-template.md:58`, the spec-governance template both
`CLAUDE.md` and `AGENTS.md` point at): "Never reinterpret a `fail`, `skip`, or
`uncertain` scenario as passing." Neither mechanism can honour it. The hosted
workflow (`.github/workflows/rust.yml`) has no `codex` binary, and the
sandboxed fixture peers cannot prove effective OS sandboxing by construction.

## Decision

Real-binary sandbox qualification is a **separate test target compiled only
under a non-default cargo feature `real-codex`** (the `hagency-execution`
crate already carries a default-off precedent, `test-diagnostics`). The
hosted workflow never enables the feature, so the two selectors
`native_codex_real_app_server_sandbox_write_inside` and
`native_codex_real_app_server_sandbox_refuses_outside` are **absent from the
default `cargo test --list` inventory** — that absence is the recorded,
machine-checkable "not run here" state, not a green result pretending to be
one. An operator with the binary runs
`cargo test --locked -p hagency-execution --features real-codex` on a host
with `HAGENCY_CODEX_QUALIFY_BIN` set to the pinned executable's path, and
records: the log path of the run, the pinned Codex version
(`codex --version`), the host OS/arch, and the commit under test. Until that
record exists for a release, effective sandbox qualification is **not
claimed**; the bound selector alone is never cited as gate evidence.

`native_codex_probe_sandbox_policy_echo` stays in the **default suite** — it
is the probe class (the typed initialize request's sandbox policy echoes back
through the offline fixture peer), unconditional because it does not need the
real binary.

## Consequences

Good, because absent-from-`--list` is honest and machine-distinguishable from
passed; the binding check cannot accidentally credit an unrun qualification.
Bad, because release evidence now depends on an operator-run procedure that
CI cannot enforce, and the feature must stay off by default to stay honest.

## Alternatives Considered

- Env-gated early return — rejected: reports *passed* when absent (review
  finding 3).
- `#[ignore]` — rejected: still binds in `--list`, so the name masquerades as
  covered.
- Running the real binary in CI — rejected: no `codex` on hosted runners and
  model execution is out of scope for the gate.
