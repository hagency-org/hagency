---
kind: decision
id: ADR-139
title: "Native Codex launch surface: app-server argv and opt-in driver"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [runner, codex, launch, execution]
---

## Context

The runner evidence inventory (brief on `context-runner-evidence-inventory.md`,
§2.2 and gap 7) found two divergences between the native Codex launch and the
retained one that no decision named:

1. **argv.** The native host launches `codex app-server` with exactly one
   argument — `vec!["app-server".into()]` at
   `native/hagency-execution/src/host.rs:289-295` — with sandbox and approval
   policy carried in the typed `initialize` request JSON, not argv. The
   retained router launches `app-server --stdio`
   (`router/src/runner.ts:201-202`), and the retained CLI additionally puts
   policy on argv (`bin/hagency-up:2020`:
   `--sandbox workspace-write --ask-for-approval on-request -C <path>`).
2. **Activation.** The native development driver is opt-in through
   `serve --development-driver` (`native/hagency/src/main.rs:109`) and runs one
   supported attempt from a fixed private profile
   (`native/hagency/src/bootstrap.rs:783+`; ADR-096). The retained runner is
   always-on for every managed agent.

Checked against the installed Codex CLI (0.154.0 on this host,
`codex app-server --help`): stdio is the default transport (`--listen` default
`stdio://`), and `--stdio` is documented as an explicit synonym
("Use stdio as the transport (equivalent to `--listen stdio://`)"). The pinned
protocol spec is Codex 0.153.4 (`specs/task-rust-codex-protocol.spec.md`).

## Decision

Both divergences are deliberate, not accidents.

1. **`app-server` bare argv.** The native host keeps argv to the single
   subcommand because every launch input the typed protocol can carry —
   sandbox mode, approval policy, cwd, model, effort — travels inside the
   validated `initialize` request where each field is typed and checked
   (ADR-004's sandboxed defaults therefore live in the request, with the same
   workspace-write / on-request defaults; `bin/hagency-up:2020`'s `-C` is
   replaced by the host-owned `directory` in the `Launch` struct). The
   retained router's `--stdio` flag is redundant for current Codex (it
   restates the default) and harmless; the retained CLI's argv policy is a
   retained surface and stays as it is. Neither product's argv changes.
   `native_codex_stdio_flag_matches_pinned_cli` pins this against the CLI:
   whenever the pinned version's default transport stops being stdio, the
   test fails and this decision is reopened — the bare argv is only correct
   while stdio is the default (or explicitly equivalent).
2. **Opt-in driver.** Always-on execution is an M4/M6-scale commitment
   (scheduler, approval application, sandbox qualification — ADR-053 keeps
   "production activation" explicitly out). The opt-in
   `--development-driver` flag is the reviewed intermediate state: execution
   exists, is exercised, and cannot run unattended. It stays opt-in until the
   release-gate slices that own activation change it; this ADR records the
   state, it does not authorize always-on.

## Consequences

Good, because typed request fields cannot be confused with shell arguments,
the divergence is now named at the launch site, and the stdio assumption is
testable rather than implicit.
Bad, because the two products' launch surfaces differ, and a reviewer must
read this ADR to know that is intended; the stdio pin adds a fixture probe
that needs maintenance when the pinned CLI version moves.

## Alternatives Considered

- Align the retained router to the bare form — deferred: correct but churn in
  the retained product with no behavioral gain; revisit if the retained
  router is edited for other reasons.
- Pass policy on argv natively — rejected: undoes the typed-request boundary
  ADR-004/053 rely on.
- Enable the driver always-on now — rejected: unblocks unattended execution
  ahead of its qualification slices.
