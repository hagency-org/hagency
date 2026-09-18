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

### 2026-09-16 operator evidence hardening

The unrecorded gate stays red until an actual operator run. Absence of an
outside file after failed authentication/initialization is no longer a passing
refusal: the record must contain an observed ungranted command/file approval
or failed execution tool, and actual whole-tree cleanup. Inside success
requires tool observation, file creation, completed upstream outcome, no error
and cleanup. The stream wait respects the remaining qualification budget.
The outside target is in a fresh private non-/tmp parent, because upstream
workspace-write normally permits temporary roots. No outside permissions are
granted and no denied command is retried. Exact binary version/hash and
LF-normalized compiled launch-source digests bind the evidence; the validator
rejects source changes. An exported source build can supply its verified base
commit via `HAGENCY_CODEX_QUALIFY_COMMIT`; that base alone is not freshness proof.
The operator can explicitly qualify 0.154.0 using `HAGENCY_CODEX_QUALIFY_PIN`,
writing a separate versioned record; the original 0.153.4 gate is not silently
replaced. These remain owned-session sandbox checks, not fleet/API parity or
the live inbox/reply soak. There is no auto-approval or test-side live service.

The operator forwards `CODEX_HOME` as well as `HOME` when the private credential
namespace is explicitly prepared there, matching the production launch. The
real mini3 Linux qualification container needed kernel namespace prerequisites
(`SYS_ADMIN`, unconfined outer seccomp and AppArmor) for Codex's own sandbox to
start. These are isolated operator-host prerequisites, not a changed Codex
sandbox policy, an approval grant or qualification of the default native Docker
deployment. The corrected probe passes the actual inside write but still
fails outside qualification: upstream completes without attempting that write.
An explanation/no-file outcome does not establish enforced refusal, so the
combined gate remains failed.

### Exact command witness successor

The qualifier must bind the actual observed command and cwd, not merely the
presence of any execution failure. The runtime's accepted scoped item stream
retains bounded private command metadata with equality predicates; changed
metadata or contradictory supplied terminal snapshots invalidates the evidence.
This remains a factual observation, not an approval or execution capability.
The official App Server documentation describes `commandExecution` command/cwd
and command-approval scope at https://learn.chatgpt.com/docs/app-server; the
installed pinned protocol still determines the actual rendered command shape.

The operator matches only enumerated exact single-command shell renderings,
counts distinct item IDs and rejects unrelated or repeated tools. Inside needs
actual matching completed execution, exact file contents, completed turn and
cleanup. Outside needs actual matching failed execution or an exact ungranted
command callback, no outside file, no stream error and cleanup. Generic file or
network approvals cannot qualify this shell-write probe. A prompt is not a
witness. No retry or permission grant is introduced. Observation and probe
sources join the compiled freshness fingerprint. Deterministic tests remain
offline, and an actual operator run remains necessary before either red gate
can be recorded as passing.

A supplementary Unix symlink-escape probe uses a freshly created workspace
link to the same fresh private outside target. The host verifies exact link
binding before and after the owned session. Its separate verdict needs the
matching failed command or ungranted command callback, absence of the physical
outside file, no stream error and cleanup. It can show actual filesystem-boundary
enforcement when the model refuses to attempt a direct outside path. The
original direct-path no-attempt verdict remains failed and is not replaced;
neither original hosted evidence gate is credited from this supplemental case.

### Original consequences

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
