---
kind: decision
id: ADR-135
title: "Two-host cutover and rollback runbook (one Linux, one macOS)"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
liveness: auto
tags: [cutover, runbook, rollback, release, operations]
---

## Context

The migration plan §9 fixes the procedure ("Complete offline parity tests and
produce a versioned release plus rollback kit", `:474`; drain rule
"Record unresolved work explicitly; do not mark it done to simplify
shutdown", `:476-477`; quiesce and backup `:478-479`; fence `:480-481`;
init/import and validate `:482-483`; "Start Rust, verify local
API/authentication … Keep execution paused until owner and runner readiness
are proven", `:484-485`; bounded acceptance `:486-487`; "Resume intake,
observe reconnect/restart behavior, then retire JS services after the
acceptance window", `:488-489`) and the rollback limits ("Before Rust
creates external effects … rollback can restore the quiesced snapshot …
after stopping Rust", `:493-494`; "After Rust accepts requests, sends
messages, rotates credentials or executes work … Drain and reconcile
effects, then use a proven compatible state export or forward repair",
`:495-497`; crypto/generation/sent-events cannot be rewound `:498-499`;
"Never start the old runtime on an unknown newer schema or reuse a revoked
credential … **Preserve explicit unknown/pending states**", `:500-501`).
The service review adds the two-host concretes this runbook must carry: the
stop contract, the `/ready` start gate (the retained side has **no** `/ready`
at all), the port map (retained `127.0.0.1:8090` backend / `8084` dashboard,
`install-full.sh:432`, `install-macos.sh:404,422`, vs native
`127.0.0.1:13300`, `main.rs:103` — loopback-only, remote via SSH tunnel or
reverse proxy, the retained `.env` never merged into the native unit), the
version identity (native workspace `0.1.0` vs retained `1.2.0`, ADR-134's
separate channels), macOS `bootout`-before-unlink ordering (under
`KeepAlive: true` a kill re-spawns), and the native migration head
(`user_version: 25`; next free migration `026`) that makes §9's
"unknown newer schema" fence concrete.

## Decision

The runbook below is the procedure for the first cutover: one Linux host
(systemd, ADR-127) and one macOS host (launchd, ADR-133), fresh-install
profile (the plan's default — no importer; a deliberate importer is the
plan's separately gated option, `:461-462`). Each step names the command,
the check that proves it, the store's required state, the rollback step,
and the evidence recorded. Steps 1–3 are host-symmetric; 4+ differ by OS
where marked.

**0. Preconditions (both hosts).** Command: `hagency --version` on the
staged artifact. Check: output equals the workspace version and the
artifact filename's `nvX.Y.Z` (ADR-134). Store: none yet. Rollback: none
needed — nothing has run. Evidence: version line, artifact `SHA256SUMS`
entry, the workflow dispatch URL.

**1. Fresh native state.** Command:
`hagency init --state-dir /var/lib/hagency-native` (Linux) or
`~/Library/Application Support/hagency-native` (macOS). Check: `init`
succeeds and writes `operator.token` (it **refuses a non-empty dir**,
`main.rs:167-171`); mode 0600, service-user owned. Store: empty SQLite
pair created by init. Rollback: `rm -rf` the state dir — nothing else has
touched it. Evidence: `ls -l` of the token, the init stdout line.

**2. Install the unit.** Linux: run the SR-1 installer (render unit →
`daemon-reload` → `enable`). macOS: render the plist →
`~/Library/LaunchAgents/io.hagency.native.plist` (no `launchctl` yet).
Check: unit file exists, `ExecStart`/`ProgramArguments` names the
versioned artifact path and `--listen 127.0.0.1:13300`; plist keys per
ADR-133 (RunAtLoad, KeepAlive true, ThrottleInterval 10, log paths).
Rollback: remove the unit file / plist — nothing has started. Evidence:
the rendered file's checksum.

**3. Start and gate on `/ready` — never `/health`.** Linux:
`systemctl start hagency-native`; macOS:
`launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/io.hagency.native.plist`.
Check: poll `curl -s http://127.0.0.1:13300/ready` until the payload is
`"status":"ok"` with every component's state word ready; `/health` is
200-while-live and proves nothing (F8). Then `systemctl is-active` /
`launchctl print` for the manager's own view. Store: open, WAL files
present (normal for ADR-120). **Rollback point R1 starts here.** Rollback
(before any external effect): stop the unit (Linux `systemctl stop`;
macOS `launchctl bootout` — **bootout before any plist removal**, and never
kill by pid: KeepAlive true re-spawns), restore the state dir from the
step-1 copy if any drift is suspected, remove the unit. Evidence: the
`/ready` JSON at gate time, `is-active`/`launchctl print` output.

**4. Record the baseline and the migration head.** Command: on each host,
query the native store's `user_version` (read-only). Check: exactly `25`
(the head this runbook is written against; `026` is the next free
migration). This is §9's "unknown newer schema" fence made concrete: **if
either store reports a version newer than the artifact knows, stop — the
old runtime must not be started against it** (`:500-501`). Store: must be
at head 25 with zero rows of live work (fresh install). Evidence: the
version query output on both hosts.

**5. The bounded acceptance task** (plan `:486-487`). Command: run the
approved check in the dedicated validation scope — one bounded task,
follow-up, private approval, file transfer, revoke. Check: each completes
and reconciles; delivery and resource release recorded. Store: unknown and
pending rows **created by acceptance must be resolved or still explicitly
pending before proceeding** — never silently dropped. **Rollback point R1
ends here: from step 6 the store has accepted external effects and
snapshot-restore rollback is REFUSED** (plan `:495-497`); from here
rollback is drain-and-reconcile plus forward repair only. Evidence: the
acceptance transcript.

**6. Drain and stop test (the stop contract).** Command: stop the unit
(Linux `systemctl stop` within `TimeoutStopSec=20`; macOS
`launchctl bootout`). Check: `/ready` flips to 503 naming stopped
components while `/health` keeps 200; the process exits 0 inside the
budget **or parks on an unknown close without exiting 0**
(`bootstrap.rs:795-806`) — a park is not a failure; SIGKILL at the budget
leaves the WAL for the next open (ADR-120). Store after: closed cleanly or
parked-unknown; WAL present; pending/unknown rows **unchanged and
preserved**. Rollback: none — this step is itself the rollback rehearsal.
Evidence: exit status, the last `/ready` payload, `journalctl` /
`launchd` log tail.

**7. Restart and confirm preservation.** Command: start again (as step 3).
Check: `/ready` returns to ok; every pending/unknown row that existed
before step 6 is still reported pending/unknown — none resolved, none
dropped, none marked done (`:476-477`, `:501`). Store: same rows, same
states. Evidence: before/after row dumps.

**8. Resume intake; retire JS after the acceptance window**
(plan `:488-489`). Command: open intake on the native service; after the
window, stop the retained services (Linux `systemctl disable --now
hagency-backend hagency-push-relay [bridge-matrix]`; macOS
`launchctl bootout gui/$(id -u)/io.hagency.services` — **bootout before
removing that plist too**). Check: native `/ready` ok; retained units
inactive; port map: 8090/8084 dark, 13300 serving loopback. **This is the
point past which rollback to the JS service as the authority is refused** —
the retirement is the authority handover (M9's "one active authority owns
each deployment, Matrix device, queue and database", plan `:396-398`).
Evidence: the retirement log, the final port check.

**What CI can prove** (the spec scenarios): against a temp state directory,
a dry-run of the runbook's checks — version identity, ready gate, stop
contract (SIGTERM → 503/exit shape), and pending-preserved across the
stop-start pair — without either init system.

## Amendment 2026-09-15: two stale steps, and three gaps the runbook does not yet carry

The procedure below stands as written on its date. This amendment corrects two
steps that have since gone stale and records three conditions the runbook does
not cover. It does not rewrite the original text.

**Correction A — step 4's migration fence is eight migrations stale.** Step 4
checks `user_version` is "exactly `25`" with "`026` the next free migration".
The store now carries migrations `022`-`033` (`native/hagency-store/src/migrations/`,
32 files). An operator following step 4 literally against a current artifact sees
`33`, reads "stop - the old runtime must not be started against it", and aborts a
healthy cutover. A fence that fires on the correct case is worse than none: the
check must compare against the artifact's own head, not a number written into the
runbook.

**Correction B — step 3's rollback names a copy that no step takes.** Step 3's
rollback says to "restore the state dir from the step-1 copy". Step 1 takes no
copy: its actions are `init`, a mode/ownership check, and evidence of `ls -l` plus
the init stdout line. The "quiesced snapshot" at the head of this record is the
migration plan's phrase for the *retained* side, not an instruction to copy native
state. Worse, the copy as imagined would not be sound: `database.rs:118` sets
`journal_mode=WAL` and `:114-117` sets `SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE`, so a
naive directory copy of a live store is both unsafe and incomplete. No backup or
`VACUUM INTO` path exists anywhere in production code.

**Gap 1 - a matrix-configured profile cannot bootstrap on fresh state. This
runbook's own profile is NOT affected; it is recorded here because this is the
cutover record.** Steps 0-4 configure no matrix host - there is no
`development-driver.json` step and no matrix step anywhere in the procedure (the
only "matrix" string in this record is step 8's `[bridge-matrix]` retirement), so
the `/ready` gate at step 3 is reached without the precondition below ever firing.
The constraint applies to any profile that DOES configure matrix, and a future
revision of this runbook that adds one inherits it:
`Prepared::load` (`native/hagency/src/bootstrap/config.rs:305-318`, straight-line,
unskippable) requires `provisioning_registration_for_engagement(&engagement_id)`
to return a row before a matrix serve starts. That query
(`domain/verified_ingress.rs:206-207`) joins `engagements e JOIN registrations r
ON r.fleet_id=e.fleet_id AND r.generation=e.generation WHERE e.id=?1` - it needs
BOTH rows. G11 supplies the registrations half from the CLI (`registration::run`
-> `DomainRepository::register`, `domain.rs:730`). The engagements half has
exactly one production writer, `domain.rs:1138` inside `admit()`, reachable only
through provisioning ingress on a RUNNING serve; the `Engagements` subcommand is
read-only inspection against a running service, not a seeding path. It is a
bootstrapping problem, not a migration one: a config string names an engagement,
it does not materialise the row.

**Gap 2 - `operator.token` has no rotation, and step 1's rollback assumes it is
cheap.** `init` writes the token exactly once under `O_EXCL` (`main.rs:203`,
`private.rs:158`); no CLI verb, console route or bootstrap path rewrites it, and
`init` refuses a non-empty dir. Step 1's `rm -rf` rollback is correct only while
the dir is untouched. Later, losing the token forfeits every authority surface of
that state dir, and the only recovery is a fresh state dir with everything
re-registered - a one-way door that belongs beside the crypto/generation/sent-event
limits this record already names.

**Gap 3 - the qualification evidence gate is enforced on no path.** Step 5's
bounded acceptance task is where the ADR-140/ADR-144 evidence class should bite.
Today `native_two_agent_qualification_records_its_evidence` and the two
`native_codex_real_app_server_sandbox_*` tests fail locally against a checked-in
placeholder ("no operator qualification run recorded") and are skipped by name in
CI (`.github/workflows/rust.yml:136`). Red locally plus skipped in CI plus a
placeholder on disk is zero enforcement on every path a merge can take.

**Not gaps, recorded to stop them being re-raised.** The absence of an importer is
this runbook's deliberate fresh-install posture (Decision, and Alternatives).
Rollback *is* specified - window R1 opens at step 3 and closes at the end of step
5, after which snapshot-restore is refused. Dual-running is rejected by policy
("No unattended dual-writer phase is proposed"); the finding worth carrying is
narrower: that policy has no mechanical enforcement, since the only exclusive lock
is intra-native (a held-fd `try_lock` per store open), so nothing prevents the
retained and native services running against one homeserver.

## Consequences

Good, because every forward step has a named check, a named rollback, and
named evidence; the refusal point (end of step 5) is explicit; the macOS
bootout ordering and the migration-head fence are concrete, not principles.
Bad, because the runbook is long, and its value depends on the operator
actually recording the evidence rather than skipping to step 8.

## Alternatives Considered

- An importer-based cutover preserving the existing deployment — rejected
  for the first cutover (the plan's own default; the importer is a
  separately gated option).
- Dual-running both services during acceptance — rejected: "No unattended
  dual-writer phase is proposed" (plan `:394`); acceptance runs against the
  native service with the retained side drained.
- A generic runbook not naming the ports, head, or refusal point — rejected:
  the review's findings exist precisely because those details were missing.

### Superseded: no park on an unknown close (2026-09-23, ADR-182)

See ADR-127's amendment of the same date: the unit's service records its
fences and exits; nothing parks.
