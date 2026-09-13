---
kind: decision
id: ADR-127
title: "Native Linux systemd unit and installer"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [service, systemd, linux, install, release]
---

## Context

The service/release inventory (brief 1, gaps G1/G3/G4) found no native service
wrapper: the three retained hardened units (`hagency-backend.service`,
`bridge-matrix.service`, `hagency-push-relay.service`) run the Node product,
`install-full.sh:492-518` enables and verifies only those, and the native
binary's only lifecycle surface is `serve` with a `CancellationToken`
(`native/hagency/src/bootstrap.rs:738`) plus the `/ready` boundary
(`native/hagency/src/lib.rs:121`). The migration plan's §5 service row
requires systemd on Linux with "Install/start/stop/uninstall, boot recovery
and account permissions" evidence, and M8 item 1 requires fresh-install
diagnostics and uninstall procedures.

## Decision

**Unit.** `deploy/hagency-native.service`, modelled on the retained units'
hardening block (`hagency-backend.service:20-47`): `Type=simple`,
`ExecStart` the native binary as `serve --state-dir <STATE_DIR> --listen
127.0.0.1:13300` (rendered from `__INSTALL_DIR__`/`__STATE_DIR__`/
`__USER__` placeholders like the retained units), `Restart=on-failure`,
`RestartSec=5`, `TimeoutStopSec=20` matching the retained budget,
`KillMode=control-group` (the process owns children; the group must be
signalled together), the same sandbox directives the retained units carry
(`NoNewPrivileges`, `ProtectSystem=full`, capability/namespace restriction,
`SystemCallFilter=@system-service`), plus `EnvironmentFile=-` for optional
env. **No `ExecStop`** is required: the default SIGTERM-to-main is the drain
trigger.

**Drain order.** SIGTERM → the existing `CancellationToken` cancels → `/ready`
flips to 503 naming the stopped components (the readiness contract,
`lib.rs:227-247`, stays verbatim) while `/health` keeps 200 → the bounded
writer drains → both stores close per ADR-120 (`SQLITE_DBCONFIG_NO_CKPT_ON_
CLOSE`, WAL left for next-open replay, `OutcomeUnknown` verdict unchanged) →
exit 0 within `TimeoutStopSec`. Nothing in this order is new code paths: the
unit binds the existing shutdown sequence to a signal.

**Installer.** A script mirroring `install-full.sh`'s shape: render the unit,
write it to `--systemd-dir` (default `/etc/systemd/system`, requiring
privileges exactly as `install-full.sh:87-105` determines), `daemon-reload`,
`enable`, `restart`, then verify with `is-active` (the pattern at
`install-full.sh:504-518`). Refusals: a non-empty state directory that is not
native state (the `serve`/`init` empty-or-fresh rule, `main.rs:166-169`), a
missing binary or unwritable state dir, an existing unit whose rendered
`ExecStart` differs unless explicitly overwritten, and systemd absent (the
plan's documented foreground alternative is a plain foreground run, not a
pretended unit). Uninstall: `disable --now`, remove the unit, leave state
and logs in place and say so.

**Evidence shape.** CI has no init system, so the spec scenarios bind a
supervisor-less harness: a fixture that spawns the binary, polls `/ready`,
sends SIGTERM, asserts the 503 flip, exit-before-budget, WAL-present close,
and pending-state preservation across a restart — the same assertions a real
unit check would make, without systemd.

## Consequences

Good, because the release gate's Linux service cell gets a real, hardened
unit and the cutover sequence (§9 step 7) gets its service-level evidence.
Bad, because the harness is not systemd itself; `is-active`-level
integration is verified only at install time on a real host.

## Alternatives Considered

- Reuse the Node units with a different `ExecStart` — rejected: the retained
  units' tmux/`/tmp` caveats do not apply and would be cargo-culted.
- A supervisor process instead of systemd — rejected: one more owned process
  with no added capability on Linux.
- `ExecStop` with a custom drain verb — rejected: duplicates the
  SIGTERM path the binary already owns.
