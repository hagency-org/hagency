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
(`install/install-macos.sh:309-356`) plus a separate supervisor unit
(`deploy/com.hagency.supervisor.plist`). The migration plan §5 requires
launchd with install/start/stop/uninstall and boot recovery evidence. As with
ADR-127, the binary's lifecycle is **already complete**
(`main.rs:211-224` SIGTERM→token; `bootstrap.rs:738-812` drain, ADR-120
close, parked unknown-close); only the unit, installer and evidence shape
are missing. macOS has no systemd: KeepAlive replaces `Restart=`,
`ThrottleInterval` replaces `RestartSec`, and there is no journald — logs go
to plain files the operator must rotate (no `newsyslog` entry exists today;
the recorded posture is unbounded files until a rotation slice exists).

## Decision

**Unit.** `deploy/io.hagency.native.plist`, a launchd user agent: `Label
io.hagency.native`; `ProgramArguments` runs a small wrapper that execs the
native binary `serve --state-dir <STATE_DIR> --listen 127.0.0.1:13300` (a
wrapper because launchd starts jobs with a minimal environment and the paths
are rendered at install time; the loopback bind is **fixed** — a refusal, not
a default: `bootstrap.rs:537` fail-closes on non-loopback, so the plist must
not offer a knob that makes the service die-and-loop); `RunAtLoad` true
(boot recovery); `ThrottleInterval 10`; `ProcessType Background`;
`StandardOutPath`/`StandardErrorPath` under the install dir's `logs/`.

**KeepAlive is `true`.** Not `install-macos.sh:338`'s
`{SuccessfulExit: false}` — the supervisor plist's own comment block
(`deploy/com.hagency.supervisor.plist:55-67`) documents why that form is a
trap: the condition is **unsatisfiable for a job that has never run**, so
launchd showed `pended … speculative` with `runs = 0` and the service never
started; and the worry behind it was wrong anyway, because `launchctl
bootout` removes the job and stops it regardless of KeepAlive.

**What `true` actually does — stated so no reader inverts it.** `true` restarts
on **any exit: a crash, a clean exit-0, and a pid-kill** alike; it is not
"restart on failure". The one stop that stays stopped is **`bootout`**, and
that is not a KeepAlive exception — `bootout` *removes the job from launchd*,
so no KeepAlive policy of any form applies to it. The first start is untrapped
(`true` carries no precondition to evaluate). So the honest cost of `true` is:
an exit-0 stop made any way other than `bootout` restarts, which is why the
runbook and the installer name `bootout` as *the* deliberate stop. The
supervisor plist's comment block is the post-mortem that fixes this reading:
a first version used `{SuccessfulExit: false}` reasoning that a clean stop
would make `bootout` unusable, and the condition — *skip restart on a clean
exit* — is **unsatisfiable for a job that has never run** (nothing has exited,
cleanly or otherwise), so launchd showed `pended nondemand spawn =
speculative` with `runs = 0` and the service never started at all. The form
that would skip a clean exit-0 restart is exactly the form that cannot start
the job; it is kept out for that reason, not for style.

**State/auth provisioning and start gate** (as ADR-127): the installer runs
`hagency init --state-dir <fresh empty dir>` first, places ownership/mode,
then renders the plist with **explicit placeholders and no guessed
defaults** (the supervisor plist's `:20-23` rule). The start gate polls
`/ready` (the 503 boundary naming components) — never `/health`, which is
200-while-live by contract and proves nothing. Remote access is SSH tunnel
or reverse proxy; the plist hard-codes the loopback listen.

**Version identity** (review F7, for the SR-3 slice): the `ProgramArguments`
binary path is the versioned artifact name the release slice mints (native
workspace `0.1.0`, `Cargo.toml:6`, versus retained `1.2.0`,
`package.json:3`); this ADR requires only that unit and artifact cannot
disagree silently.

**Installer.** Mirrors `install-macos.sh`'s flow: render the plist, write to
`~/Library/LaunchAgents/`, `launchctl bootstrap gui/$(id -u)`, verify via
`launchctl print` plus the `/ready` poll. Stop is `launchctl bootout` — not
kill-by-pid, which KeepAlive `true` restarts. Refusals: non-empty non-native
state dir (the `init` rule, `main.rs:167-171`), missing binary, an existing
plist at the target label unless explicitly overwritten, unwritable
`~/Library/LaunchAgents`. Uninstall: `bootout`, remove the plist, leave
state and logs in place and say so.

**Evidence shape (review F4).** The spec's three launchd selectors live in
an **ungated test file** with `#[cfg(target_os = "macos")]` inside each
body and a **named refusal on the other OSes**, so every hosted leg lists
them. The harness renders the plist template and asserts its keys
(RunAtLoad, KeepAlive true, ThrottleInterval, log paths, fixed loopback
listen), then runs the serve command it names through the SIGTERM
drain/close/pending-state assertions of ADR-127's harness — CI does not
load the plist into a real `launchd`.

## Consequences

Good, because the keep-alive semantics are stated truthfully — restart on
any exit, a first start with no precondition, and `bootout` as the one
deliberate stop — the recorded trap is not repeated, and the selectors bind
on every leg.
Bad, because pid-kill restarts (the stated cost of `true`), CI never
exercises a real `launchctl bootstrap`, and unbounded logs remain real
operational debt until a rotation slice exists.

## Alternatives Considered

- `KeepAlive {SuccessfulExit: false}` — rejected: documented in-tree as the
  never-starts trap (`deploy/com.hagency.supervisor.plist:55-67`).
- `KeepAlive false` + an external watchdog — rejected: reimplements
  restart-on-crash badly.
- A system-level LaunchDaemon — rejected: per-operator state needs no root.
- Direct `ProgramArguments` to the binary — rejected: minimal launchd
  environment and rendered paths need the wrapper.
