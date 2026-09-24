---
kind: decision
id: ADR-127
title: "Native Linux systemd unit and installer"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [service, systemd, linux, install, release]
---

## Context

The service/release inventory (brief 1) and its review
(`context-service-release-review.md`) together define what is missing and
what already exists. **What exists** (the review's G3 correction — an unread,
not a gap): `main.rs:211-224` already traps SIGTERM (and ctrl-c) into the
cancellation token; `bootstrap.rs:738-812` already drains the driver, stops
the server gracefully, and closes the store per ADR-120 — and when the close
outcome is unknown it **parks** (keeps the status endpoint and the original
owner, refuses a false successful exit, `bootstrap.rs:795-806`) rather than
exiting 0. **What is missing:** the unit, the installer, and the
start/stop evidence shape (inventory G1, review F4/F5/F6). The retained
units set **no** `ExecStop`, `KillMode`, `KillSignal` or `SendSIGKILL`
(review F2) — both sides are stop-then-replace, not drain-then-replace; the
inventory's "drain order" prose claimed parity that does not exist.

## Decision

**Unit.** `deploy/hagency-native.service`, modelled on the retained units'
hardening block (`hagency-backend.service:20-47`): `Type=simple`,
`ExecStart` the native binary as
`serve --state-dir /var/lib/hagency-native --listen 127.0.0.1:13300`
(placeholders rendered at install time), `Restart=on-failure`,
`RestartSec=5`, `TimeoutStopSec=20` matching the retained budget, the same
sandbox directives the retained units carry (`NoNewPrivileges`,
`ProtectSystem=full`, capability/namespace restriction,
`SystemCallFilter=@system-service`), plus `StateDirectory=` and
`WorkingDirectory=` for the state root. **No `ExecStop`, no `KillMode`, no
`KillSignal`** — matching the retained units' shape (F2): systemd's default
SIGTERM-to-cgroup then SIGKILL-after-timeout applies, and the native binary
already owns the SIGTERM path. The 20s budget covers the normal
drain-and-close; a parked unknown-close exceeds it by design and is
SIGKILLed holding an unrelinquished owner — the honest terminal state, not a
lost one (ADR-120 leaves the WAL for the next open).

**Start gate is `/ready`, never `/health`** (review F8): `/health` is
200-while-live by contract (`lib.rs:226-235`) and proves nothing at cutover;
`/ready` is the 503 boundary that names components. The installer's
post-start check and the runbook's cutover check both poll `/ready` for the
component vocabulary, then `is-active`.

**State/auth provisioning** (review F5): `serve` does not load `.env`, takes
`--state-dir`, and `Bootstrap` fail-closes unless that dir yields a readable
`operator.token` (`bootstrap.rs:540-543`). The installer therefore runs
`hagency init --state-dir <fresh empty dir>` first — which refuses a
non-empty dir on purpose (`main.rs:167-171`) — places the service user's
ownership/mode on it, and only then renders and enables the unit. A unit
pointed at a JS-shaped or empty dir fail-closes at startup; the installer
refuses earlier with a named reason instead.

**Bind is loopback-only as a refusal, not a default** (review F6): the unit
hard-codes `--listen 127.0.0.1:13300`; `bootstrap.rs:537` returns
`Failure::Config` for any non-loopback address, so an operator edit to
`0.0.0.0` makes the unit die and loop under `Restart=`. The runbook states
remote access is SSH tunnel or reverse proxy.

**Version identity** (review F7, recorded for the SR-3 slice that owns it):
the native workspace version is `0.1.0` (`Cargo.toml:6`) while the retained
product is `1.2.0` (`package.json:3`), and `release.yml` has no cargo step.
This ADR takes no position on the source of the native version; it requires
only that the unit's `ExecStart` binary path be the versioned artifact name
the SR-3 slice mints, so unit and binary cannot disagree silently.

**Installer.** Render the unit, write it to `--systemd-dir` (default
`/etc/systemd/system`, privilege logic as `install-full.sh:87-105`),
`daemon-reload`, `enable`, `restart`, verify `is-active` **after** the
`/ready` poll (the pattern at `install-full.sh:504-518`, plus the gate).
Refusals: a non-empty state directory that is not fresh native state (the
`init` rule), a missing binary or unwritable state dir, an existing unit
whose rendered `ExecStart` differs unless explicitly overwritten, and
systemd absent (the plan's foreground alternative is a plain foreground run,
not a pretended unit). Uninstall: `disable --now`, remove the unit, leave
state and logs in place and say so.

**Evidence shape (review F4).** CI has no init system and the binding gate
runs on every matrix leg: the spec's three service selectors live in an
**ungated test file** with `#[cfg(target_os = "linux")]` inside each test
body and a **named refusal on the other OSes** (the
`hagency-platform/tests/guardian.rs` precedent: file ungated, `#[cfg(unix)]`
inside). On macOS the same names run and assert the documented
not-a-Linux-service refusal — the selectors are present in `--list` on every
leg. The harness spawns the binary, polls `/ready`, sends SIGTERM, asserts
the 503 flip, exit-before-budget, WAL-present close, and pending-state
preservation across restart — without systemd, which is install-time
evidence on a real host.

## Consequences

Good, because the unit's start gate is the boundary that means readiness,
the installer provisions state honestly, and the spec selectors bind
everywhere.
Bad, because a parked unknown-close is SIGKILLed at the budget (by design,
but it looks like a crash in `journalctl` until the runbook says otherwise),
and the harness is not systemd itself.

## Alternatives Considered

- `ExecStop` with a custom drain verb — rejected: duplicates the SIGTERM
  path the binary already owns; no retained unit sets one (F2).
- `KillMode=mixed` — deferred: nothing in-tree proves the native drain
  exceeds 20s; revisit with evidence, not speculation.
- File-level `#![cfg]` gating — rejected: drops selectors from other legs'
  `--list` and fails the binding gate (F4).

### Superseded: an unknown close records its fences and exits (2026-09-23, ADR-182)

The park on an unknown close ("rather than exiting 0"; "a park is not a
failure") is superseded by ADR-182: the service writes what it could not
prove as agent fences and exits inside the stop budget.
