---
kind: decision
id: ADR-140
title: "Qualify the real Codex sandbox with an operator-run example and a fail-closed CI check"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [runner, codex, sandbox, qualification, testing]
---

## Context

The runner evidence review (finding 3) showed that "skipped, and reported as
skipped" is not expressible with ordinary cargo test mechanics: an env-gated
early return reports **ok/passed** when the real binary is absent, and
`#[ignore]` still appears in `cargo test -- --list`. The service review's F4
adds the binding constraint: `native/scripts/check-rust-spec-bindings.mjs`
inventories selectors on **every hosted matrix leg** (`rust.yml:59` matrix,
`:108` step) and exits 1 on any missing name, so a selector hidden behind a
cargo feature or a file-level `#![cfg]` fails CI on the legs that do not
compile it. The hosted workflow has no `codex` binary, and the sandboxed
fixture peers cannot prove effective OS sandboxing by construction. The house
rule stands (`docs/workspace-agents-md-template.md:58`): "Never reinterpret a
`fail`, `skip`, or `uncertain` scenario as passing."

## Decision

Real-binary sandbox qualification is an **operator-run example binary**:
`cargo run --locked -p hagency-execution --example codex_qualify` on a host
with `HAGENCY_CODEX_QUALIFY_BIN` pointing at the pinned executable. The
example launches the real `codex app-server`, exercises the write-inside and
write-outside cases through the existing `Host`/owned-session path, and
writes an **evidence file** at a documented path
(`native/hagency-execution/qualification/codex-sandbox.json`, tracked in git)
containing: the pinned Codex version string, the two verdicts
(`write_inside`, `refuses_outside`), the full log path, the host OS/arch, and
the commit under test.

The two CI tests — `native_codex_real_app_server_sandbox_write_inside` and
`native_codex_real_app_server_sandbox_refuses_outside` — live in an
**ungated** test file, run on every hosted leg, and **validate the evidence
file**: its shape, its pinned version, and its freshness against the
repository state. **They FAIL — never skip — when the evidence file is
missing, stale, or records a non-passing verdict.** A missing evidence file
is a red gate, not an untested one; the house rule is honoured by failing
closed. The selectors are therefore always present in `--list` on every leg.
`native_codex_probe_sandbox_policy_echo` stays unconditional and unchanged:
it is the probe class (the typed initialize request's sandbox policy echoing
through the offline fixture peer), needing no real binary.

When the pinned Codex version moves, the operator re-runs the example and
the evidence file changes in the same commit; the CI tests pin that
co-movement.

**The hosted workflow skips the two selectors by name, never inside a test:**
`rust.yml`'s `native-tests` step runs the suite with
`-- --skip native_codex_real_app_server`, because hosted runners have no codex
binary and will never hold real evidence — the skip lives in the workflow (the
one honest place for "not run here"), the selectors stay present for the
binding gate's `--list` inventory, and the operator runs them on a host with
the binary, as this ADR defines.

## Consequences

Good, because the gate fails closed on missing evidence, the selectors bind
on every hosted leg, and the evidence file is reviewable diff, not prose.
Bad, because qualification freshness is a commit-time discipline — a source
change to the launch path can land without re-qualification unless the check
also pins the relevant source digests (left to the builder lane to wire).

## Alternatives Considered

- A `real-codex` cargo feature gating the target — rejected: the selectors
  vanish from `--list` on legs that do not enable the feature and the
  binding gate fails everywhere (review F4).
- Env-gated early return — rejected: reports *passed* when absent.
- `#[ignore]` — rejected: still binds in `--list*, masquerading as covered.
- Running the real binary in CI — rejected: no `codex` on hosted runners and
  model execution is out of scope for the gate.
