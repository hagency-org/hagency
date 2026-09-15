---
kind: decision
id: ADR-139
title: "Native Codex launch surface: app-server argv and opt-in driver"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
liveness: auto
tags: [runner, codex, launch, execution]
---

## Context

The runner evidence inventory (gap 7) found divergences between the native
Codex launch and the retained one that no decision named:

1. **argv shape.** The native host launches `codex app-server` with exactly
   one argument — the `Launch` literal at
   `native/hagency-execution/src/host.rs:289-295`, whose argv is
   `arguments: vec!["app-server".into()]` at `:291` — with sandbox and
   approval policy carried in the typed `initialize` request JSON
   (`native/hagency-runtime/src/codex/session.rs:186-187`: `"sandbox":
   self.mode()`, `"approvalPolicy": "on-request"`; validated back at
   `session/state.rs:75,86`). The retained router launches
   `app-server --stdio` (`router/src/runner.ts:201-202`), **also** passes
   `-c mcp_servers.<name>…` configuration on argv
   (`router/src/runner.ts:203-210`) where native carries the helper config
   in the `initialize` `config` field (`session.rs:188-190`), and the
   retained CLI additionally puts policy on argv (`bin/hagency-up:2020`:
   `--sandbox workspace-write --ask-for-approval on-request -C <path>`).
2. **Activation.** The native development driver is opt-in through
   `serve --development-driver` (`native/hagency/src/main.rs:109`) and runs
   one supported attempt from a fixed private profile (ADR-096). The
   retained runner is always-on for every managed agent.

Checked against the installed Codex CLI (0.154.0 on this host,
`codex app-server --help`): stdio is the default transport (`--listen` default
`stdio://`), and `--stdio` is documented as an explicit synonym
("Use stdio as the transport (equivalent to `--listen stdio://`)"). The
checked-in probe excerpt `native/hagency-execution/tests/fixtures/codex-app-server-help.txt`
was captured from **codex-cli 0.154.0 on 2026-09-13**, and the selector pins
that version by strict equality. The pinned protocol spec is Codex 0.153.4
(`specs/task-rust-codex-protocol.spec.md`) — that pin governs wire envelopes;
the launch-surface probe pins the version actually captured, and the two are
deliberately not conflated.
ADR-053 fixed the native argv: "There is no second launcher: the fixed argv
is `app-server`, through the existing SupervisedProcess and its prepare/start
or atomic Windows Job Object boundary" (`adr-053` Boundary) — and its scope
statement "Native service availability remains false" is the activation state
this ADR records. The wire-protocol spec pins Codex **0.153.4**
(`specs/task-rust-codex-protocol.spec.md:18`).

## Decision

Both divergences are deliberate, not accidents.

1. **`app-server` bare argv.** The native host keeps argv to the single
   subcommand because every launch input the typed protocol can carry —
   sandbox mode, approval policy, cwd, model, effort, MCP helper config —
   travels inside the validated `initialize` request where each field is
   typed and checked. **This ADR scopes ADR-004's argv mechanism for the
   native launch:** ADR-004's Decision fixes the *values* —
   `--sandbox workspace-write --ask-for-approval on-request`
   (`adr-004:16-17`) — and names them as argv flags because the retained
   product passes them that way. Natively those exact values are preserved
   (`session.rs:186-187`) but the *mechanism* is the typed request; ADR-004's
   flags remain the law for the retained CLI and router, and a reader of
   ADR-004 alone should read this ADR as its native-scope interpretation,
   not a change to the retained surface. The third argv divergence — the
   router's `-c mcp_servers.*` overrides (`runner.ts:203-210`) versus the
   native `config` field (`session.rs:188-190`) — is the same rule: launch
   configuration belongs in the request, not argv. The retained router's
   `--stdio` flag is redundant for current Codex and harmless; neither
   product's argv changes. The stdio equivalence is pinned by a **recorded
   fixture**, `native/fixtures/codex-cli/app-server-help-0.153.4.excerpt.txt`
   (a captured `--help` excerpt whose header records the capture version and
   the assertions: stdio is the `--listen` default, `--stdio` is an exact
   synonym) — **not** by a live binary. The excerpt was captured from 0.154.0
   with 0.153.4 not installed; it commits the *shape*, and
   `native_codex_stdio_flag_matches_pinned_cli` FAILS until the excerpt is
   re-captured from the real 0.153.4 binary or the spec's pin moves to the
   captured version.
2. **Opt-in driver.** ADR-053's scope — "No HTTP route, CLI service, Matrix
   transport, scheduler or catalog enables it" and "Native service
   availability remains false" — is the recorded boundary; the opt-in
   `--development-driver` flag is the reviewed intermediate state inside it:
   execution exists, is exercised, and cannot run unattended. It stays
   opt-in until the release-gate slices that own activation change it; this
   ADR records the state, it does not authorize always-on.

## Consequences

Good, because typed request fields cannot be confused with shell arguments,
the full argv account (policy flags, stdio flag, `-c` overrides) is named,
the divergence is cited at the launch site, and the stdio assumption is
pinned to a reviewable fixture rather than a live binary.
Bad, because the two products' launch surfaces differ (a reviewer must read
this ADR to know that is intended), and the fixture needs one re-capture from
the exact pinned version before its assertions count as evidence.

## Alternatives Considered

- Align the retained router to the bare form — deferred: churn with no
  behavioral gain; revisit when the retained router is edited anyway.
- Pass policy on argv natively — rejected: undoes the typed-request boundary
  and contradicts ADR-053's fixed argv.
- Assert the stdio default from the installed CLI at test time — rejected:
  evidence-from-a-live-binary at an uncontrolled version (review G1).
- Enable the driver always-on now — rejected: unblocks unattended execution
  ahead of its qualification slices.
