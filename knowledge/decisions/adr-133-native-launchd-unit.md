---
kind: decision
id: ADR-133
title: "Native macOS launchd agent"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [service, launchd, macos, install, release]
---

## Context

The service/release inventory (brief 1, gap G2) found no native macOS service
wrapper. The retained answer is a launchd **user** agent
(`install/install-macos.sh:309-356`): plist at
`~/Library/LaunchAgents/io.hagency.services.plist` running a bash wrapper
(`.hagency-launchd.sh`) that sources `.env` and starts `services/hagency-services.mjs`,
plus a separate supervisor unit (`deploy/com.hagency.supervisor.plist`) whose
header records why it exists and why its placeholders are not defaults. The
migration plan §5 requires launchd with install/start/stop/uninstall and boot
recovery evidence. macOS has no systemd: KeepAlive replaces `Restart=`,
`ThrottleInterval` replaces `RestartSec`, and there is no journald — logs go
to plain files the operator must rotate (the cutover-runbook counterpart's
finding: **no `newsyslog` entry exists** for any Hagency log today).

## Decision

**Unit.** `deploy/io.hagency.native.plist`, a launchd user agent modelled on
`install-macos.sh:330-345`: `Label io.hagency.native`; `ProgramArguments`
runs a small wrapper (`install-dir` relative) that execs the native binary
`serve --state-dir <STATE_DIR> --listen 127.0.0.1:13300` — a wrapper rather
than the binary directly because launchd starts jobs with a minimal
environment and the state-dir path is rendered at install time;
`WorkingDirectory` the install dir; `RunAtLoad` true (boot recovery);
`KeepAlive { SuccessfulExit: false }` — restart on abnormal exit only, so a
deliberate `launchctl bootout`-free operator stop is not fought by launchd
(the supervisor plist's `:57-68` discussion of `KeepAlive: true` costing
kill-by-pid restarts is the recorded trade-off; a service that exits 0 on
SIGTERM must stay down); `ThrottleInterval 10`; `ProcessType Background`;
`StandardOutPath`/`StandardErrorPath` under the install dir's `logs/`.

**Differences from systemd (SR-1/ADR-127) that are deliberate.** User agent,
not a system unit — the native service is per-operator state, `launchctl
bootstrap gui/$(id -u)` is the enable path, and no root is required.
KeepAlive is exit-class-based, not `Restart=on-failure`'s cause-based.
ThrottleInterval is launchd's own floor; the unit sets 10s and does not fight
it. Logs are plain files with **no `newsyslog`/rotation entry — the recorded
posture is unbounded files under `logs/` until a rotation slice exists**; the
installer prints that fact rather than silently assuming rotation.

**Installer.** Mirrors `install-macos.sh`'s flow: render the plist with
explicit placeholders (no guessed defaults, per the supervisor plist's
`:20-23` rule), write to `~/Library/LaunchAgents/`, `launchctl bootstrap
gui/$(id -u)`, verify with `launchctl print`/`launchlist`-equivalent status,
and on stop use `launchctl bootout` (not kill-by-pid, which KeepAlive would
defeat). Refusals: non-empty non-native state dir, missing binary, an
existing plist at the target label unless explicitly overwritten, and an
unwritable `~/Library/LaunchAgents`. Uninstall: `bootout`, remove the plist,
leave state and logs in place and say so.

**Evidence shape.** CI macOS runners have launchd, but a user-agent install
in CI is brittle; the spec scenarios bind the same supervisor-less harness
as SR-1 (spawn, poll `/ready`, SIGTERM, assert drain/close/pending-state),
with the plist's directives asserted by rendering the template and checking
the keys — not by loading it.

## Consequences

Good, because the release gate's macOS service cell gets a real unit whose
keep-alive semantics match a stoppable service, and the log-rotation gap is
recorded instead of assumed away.
Bad, because keep-alive/`bootout` integration is only install-time-verified
on a real host, and unbounded logs are a real operational debt until rotated.

## Alternatives Considered

- `KeepAlive: true` — rejected: defeats operator stop by pid (recorded
  trade-off in the supervisor plist).
- A system-level LaunchDaemon — rejected: per-operator state needs no root.
- Direct `ProgramArguments` to the binary — rejected: minimal launchd
  environment and rendered paths need the wrapper.
