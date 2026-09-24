---
kind: decision
id: ADR-136
title: "Windows is paused as a native release target"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
liveness: auto
tags: [platform, windows, release, ci]
---

## Context

The migration plan's platform matrix proposed Windows x86_64 alongside Linux
and macOS for the first native release
(`docs/design/hagency-rust-migration-plan.md:150-151`), and substantial native
Windows work exists: Job Object custody, overlapped pipes, DACL-checked
directories (ADR-104 sync), and a hosted `windows-2025` CI lane. On
2026-09-13 the operator decided that no coding agent runs on Windows, so
Windows is not a release target for now. Commit `cda737cd` made the hosted
lane non-blocking (`continue-on-error: ${{ matrix.os == 'windows-2025' }}` at
`.github/workflows/rust.yml:64`, rationale comment at `:61-63`); the same
decision is logged in `docs/progress.md:8285-8288`.

## Decision

Windows is paused as a release target. The `windows-2025` lane keeps running
for its diagnostics but its verdict does not block; Ubuntu and macOS are the
release gate. The plan's §5 Windows rows are retained, marked "paused, not a
release target, 2026-09-13", so the earlier Windows findings (Job Objects,
ConPTY, DACLs, directory-sync) keep their context; they are requirements for
a future Windows release, not the current one. Windows must not be advertised
as supported until a superseding decision lifts the pause.

## Consequences

Good, because the release gate reflects where agents actually run and stops
paying a blocking cost for an unused platform.
Bad, because Windows-only regressions can land unnoticed while the lane is
diagnostic-only, and the pause must be remembered when reading §5's matrix.

## Alternatives Considered

- Delete the Windows rows from the plan — rejected: it erases the context of
  the recorded Windows findings.
- Keep Windows blocking — rejected: ties the gate to a platform with no
  coding agent running on it.
