---
name: hagency-inner-loop
description: Use when an Hagency-managed Claude, Codex or other middle agent delegates repository work through Herdr and octoloop to a lower execution agent, or must monitor and verify such a job.
---

# Hagency inner execution

The Hagency-managed agent owns decomposition, execution choice, verification
and the reply to the originating task. Herdr owns terminal processes;
octoloop supplies its execution workflow. Choose the lower backend for the
task: octoscode, Claude Code (`claude` in Herdr), Codex, Grok or another
installed kind. This does not make octos a supported Hagency thread runner.

## Select and start

Use the operator-authorized Herdr session and project path. Authorization
already given for this task persists. Outside Herdr, an explicitly authorized
named session is required; never borrow the focused session. Discover the
installed CLI with `herdr --help`, `herdr agent` and `herdr pane`. Do not run
bare `herdr` for discovery. For octoloop, read its installed skill and the
selected project's startup protocol; it is a skill, not necessarily a binary.

Inspect the explicit session with `herdr --session "$SESSION" agent list`.
Reuse only an assigned agent that is ready for new work. Otherwise select an
available shell pane or create one within the authorized layout. Get IDs from
JSON responses; keep the user's focus and set the project working directory:

```bash
herdr --session "$SESSION" pane split "$PARENT_PANE" --direction right --cwd "$PROJECT" --no-focus
herdr --session "$SESSION" agent start "$INNER_NAME" --kind "$KIND" --pane "$NEW_PANE" --timeout 30000
herdr --session "$SESSION" agent get "$INNER_NAME"
```

Use `down` when it fits the layout. Pass authorized native agent arguments
after `--`; preserve sandbox and approval policy. Inspect blocked startup
instead of injecting a prompt or approving it. A started pane alone does not
prove readiness. Inspect octoloop's active goal/loop state before assigning a
new job; do not silently reuse an old active goal or take over another agent.

For a new named session, start its headless server and create its first
workspace with `workspace create --cwd "$PROJECT" --no-focus` before selecting
a pane. Two octoscode instances in the same project may collide on an Octos
instance lock. If startup reports `OCTOS_DATA_DIR_LOCKED`, preserve the existing
owner and give the new instance its own control directory through Octos's
`--instance-data-dir`. Confirm the installed `octos serve --help` and pass the
command through octoscode's `--stdio-command`; do not delete another instance's
lock or interrupt its job. A profile-list error may be a consequence of this
failed server startup, so inspect the actual server error.

On macOS, a deeply nested instance directory can exceed the Unix socket path
limit and leave the TUI visible after its server has exited. Inspect the
operator-control RPC error and use a short, dedicated runtime path (or a
verified private alias to it). Preserve the real control files outside the
edited repository. Restart only this job's failed instance, then confirm the
backend is connected before preparing or sending work.

## Prepare before dispatch

Resolve [scripts/monitor.mjs](scripts/monitor.mjs) relative to this skill's
directory and set `MONITOR` to that absolute path. The helper requires Node
22+, Git and Herdr. Set `CONTROL` to a directory outside the edited repository.
Choose an independent, task-specific verifier before dispatch; use the exact
test selector or a review script under the middle agent's control. A command
that merely prints success is not verification. The result must never choose
the verifier. Tests that intentionally edit source need a separate verification
checkout and a verifier that checks its correspondence to the submitted work.

```bash
node "$MONITOR" prepare --session "$SESSION" --agent "$INNER_NAME" \
  --cwd "$PROJECT" --jobs-dir "$CONTROL" \
  --verify-json '["npm","test","--","tests/relevant.test.js"]' \
  --timeout-ms 900000 --command-timeout-ms 60000
```

The JSON response contains `manifest_path`, `result_path`, `job_id`, `nonce`
and the observed execution identity, including the pane's shell pid and
foreground process group from `pane process-info`. Missing process evidence
fails preparation; do not substitute a pane label. Each preparation creates a fresh job.
Keep the manifest unchanged. Its directory is a local evidence record, not
Hagency task state or an authorization boundary between same-user processes.

Give the inner agent the actual task, boundaries and acceptance criteria plus
the generated result path, job id and nonce. Require it to write the result
after finishing its edits and checks, with no further writes for this job.
Use an atomic rename when possible; the helper also reads an in-place update.
The completed result schema is:

```json
{
  "version": 1,
  "job_id": "copy the prepared job_id",
  "nonce": "copy the prepared nonce",
  "status": "completed",
  "summary": "Concrete changes and limitations",
  "commit": "exact git rev-parse HEAD of the submitted repository",
  "checks": [{"name": "exact check run by inner", "status": "passed"}]
}
```

`commit` identifies the base HEAD even when authorized edits are uncommitted;
do not commit solely to satisfy this field. Failed or blocked work must use
`status: "failed"` or `"blocked"` and a concrete `summary`. Optional progress
uses `"working"`, retaining the version, job id and nonce. Completed results
require at least one named passed check; skipped or uncertain checks do not
become passed. Inner test claims are evidence to examine, not acceptance.

## Keep the monitor owned

Start the monitor as a managed shell-tool process that gives you a resumable
process handle, then send the prepared task in the same active batch:

```bash
node "$MONITOR" watch --job "$MANIFEST"
herdr --session "$SESSION" agent prompt "$INNER_NAME" "$(cat "$PROMPT_FILE")"
```

These are separate tool operations: `watch` stays running while you send the
prompt. Keep its process handle and resume it at intervals of at most 60
seconds. Do not end the Hagency dispatch while an unowned watcher or inner
job is still outstanding. If prompt delivery fails, record the failure and
inspect the target; do not automatically resend a possibly accepted job.
`agent prompt --wait` and a pane that says idle can refer to another turn;
neither replaces the job monitor.

The helper polls the full result content, validates the fresh job and nonce,
matches the reported commit to HEAD and executes only the prepared argv in
the recorded directory. It compares tracked and non-ignored untracked file
contents, Git status/index, result bytes and agent identity around verification.
Ignored runtime files are outside that comparison; Git submodules are rejected.
Keep exclusive ownership of the submitted tree throughout verification: two
matching snapshots do not prove that another writer never ran between them.

It prints JSON, saves `report.json` beside the manifest, and exits:

| Exit | `work_status` | Action |
|---|---|---|
| 0 | `verified` | Review verifier output and evidence against task acceptance. |
| 1 | `failed` | Inspect reason and logs; fix or explicitly plan a fresh job. |
| 2 | `blocked` | Surface the actual blocker through the task's existing flow. |
| 3 | `timed_out` | Inspect bounded failure; do not assume the inner stopped. |

`runtime_status` and `runtime_state_change_seq` are separate observations.
Herdr may classify octoscode as idle with an unchanged sequence while it is
working. Neither field, a translated label, ACK substring nor file line count
can prove this job completed. `verified` means the chosen independent check
passed on the sampled unchanged work; it does not mean the runtime stopped,
Hagency marked its task done, or Matrix delivered a reply.

After review, use the current session's authenticated Hagency task lifecycle
tools and return the evidence-backed result to the originating thread. Follow
the existing octoloop receipt/goal-close protocol where applicable. Never
edit backend state or manufacture an ACK to force completion. Stop only
processes owned by this job when authorized; keep unrelated panes and sessions.

### Reconcile execution returns before closeout

For a monitor or observer with persisted native tool returns, run
`scripts/native-handle-summary.mjs` against **your own exact handle** before
reporting its terminal status. The directory contains one readonly JSON file
per actual exec/poll return, with `request_started_ms`, `request_returned_ms`,
the untouched `request` and `result` objects, and the SHA256 of `result.output`
as `output_sha256`. Persist the initial resumable return and every later
empty-input poll. Retain failed records; never manufacture a missing return.

```sh
node scripts/native-handle-summary.mjs --records "$HANDLE_RECORDS" --session-id "$NATIVE_HANDLE"
```

An observed nonzero terminal exits 1, even if the agent previously believed
the process was waiting. Exit 0 describes an observed zero execution exit,
not acceptance of the job. Exit 2 means `terminal_not_observed`: re-poll your
existing handle or inspect authoritative state; it does not mean running,
stopped or timed out. Exit 3 rejects incomplete/corrupt/changed evidence.
Read the summary before composing task status, then reconcile it with the
monitor report or observer outcome. A claim/ready file cannot override a
later terminal return. Distinguish process exit time from the later return
delivery time if an approval held native tool delivery.

The reader performs no process control and prints no command or output bodies.
Only the public observer codes `window_missed_checkpoint`, `observer_timeout`
and `action_outcome_unknown` may be copied as a diagnostic reason; arbitrary
output strings are omitted. Empty polls may omit `chars` or set it to `""`.
It reads at most 10,000 records, 2 MiB per file and 32 MiB total. It checks the
observed directory/file identities before returning, but cannot authenticate
same-user writers or prove an omitted record never existed. Keep exclusive
ownership and retain the underlying evidence. `full_acceptance` stays false.

## Bound native goal controls

Use `scripts/native-control.mjs` for inspection, one pause/resume of an
existing explicitly bound Octoscode goal, or one periodic-loop control. It requires the adjacent
`native-control-evidence.mjs`; `hagency-sync-skills` checks both resources
before changing any client links. This initial adapter uses the pinned Darwin
birth collector and Herdr's real argument arrays. It has no verified Windows
or Unix process adapter and does not launch workers or inject faults.

Prepare a fresh **readonly**, canonical absolute JSON plan and independently
pin its exact bytes. Do not overwrite an older plan. Invoke:

```sh
node scripts/native-control.mjs --plan "$CONTROL_PLAN_FILE" --sha256 "$CONTROL_PLAN_SHA256"
```

The plan fields are:

| Field | Required value |
| --- | --- |
| `version` | `1` |
| `operation_id` | Fresh lowercase UUIDv4; never reuse an attempted operation |
| `operation` | `inspect`, `goal-pause`, `goal-resume`, `loop-create`, `loop-pause`, `loop-resume` or `loop-delete` |
| `evidence_dir` | Existing private directory (0700), outside the lower business tree |
| `query_timeout_ms` | Integer 100–10000; total budget for each exchange including process inspection and delivery |
| `expected_goal` | `{goal_id, created_at_ms, objective_sha256}` from the original scoped goal |
| `binding` | The process/session binding described below |

`binding` has `version:1`, canonical absolute `project` and `trace` paths,
`herdr_session`, `lower_agent`, `pane_id`, `terminal_id`, `shell_pid`, `profile`
and `native_session`. The native session must be `PROFILE:local:tui#coding`.
`herdr`, `birth_tool` and `frontend_binary` each contain `{path,sha256}` with
canonical absolute paths and lowercase SHA256 digests. Supply the reviewed
Octoscode binary supporting `/goal --report` and `/loop list --report`; older
binaries may parse those strings as new work. A pin authenticates chosen bytes,
not the correctness or authorization of an arbitrary executable.

Both `frontend` and `backend` must contain exactly
`{pid,ppid,pgid,started,cwd,argv}`. `started` is the collector's decimal epoch
seconds plus six microsecond digits, and `argv` is the **actual token array**
from Herdr. Keep spaced arguments intact; never derive this array by splitting
a command-line string. The frontend parent is `shell_pid`, the backend parent
is the frontend, both have the bound project cwd and foreground process group,
and `frontend.argv[0]` equals the pinned frontend binary path. Missing facts,
duplicates, changed births, argument arrays or cwd stop the operation.

All supplied file/directory paths must already be canonical and contain no
symlink component; a macOS `/tmp` input should be prepared using its verified
`/private/tmp` path. Do not normalize process argv strings in the binding.
Collect the facts from the actual owned session; copied examples, PID-name
searches and an old task's identity are not substitutes.

Inspection requests goal, loop and complete session turns using nonmodal
commands. A trace fence (file identity, complete-line byte offset and prefix
digest) is captured **before** each delivery. Both request and response must
match exact scope, method, parameters, request ID and chronology. Trace
replacement, prefix rewriting, duplicates, RPC errors and missing responses
fail; a successful Herdr delivery by itself never proves native success.

Resume requires the original paused/blocked goal, a nonempty complete array
of terminal turns and a fresh Herdr idle observation. Pause allows an active
turn to settle and never reports session idle. The underlying TUI control is
get→set without an atomic expected-goal-ID condition. The helper checks both
real responses against the original goal ID, creation time and objective
digest; it cannot prevent a concurrent external writer from changing the goal
between observations. Keep exclusive operational ownership of the session.

Every plan claims `evidence_dir/operation_id` exclusively and writes readonly
plan/observation files. Before a mutation it publishes `intent.json` with the
pre-send fence. After that point, delivery failure, timeout or conflicting
identity yields `outcome_unknown`; it never retries or sends a rollback.
Retain the directory and inspect the actual outcome separately. Claims are
local to this evidence root, not a global registry across roots. Neither a
new root nor a new operation ID is permission to repeat an uncertain action.

The CLI emits one JSON report and exits0 only for `observed` or `applied`.
`failed`, `outcome_unknown` and `rejected` exit1. Public reports omit raw
objectives, argv and subprocess stderr; evidence inside the private directory
contains the correlated raw native frames. Never paste those private frames
into public logs. `full_acceptance` is always false: these are individual
observations/controls, not monitor exit0, fault recovery, task completion or
Robrix delivery. Prefix hashing still reads the trace in O(N) time, with a
128 MiB file cap; this version does not solve long-trace scaling.

### Original periodic loop

`loop-create` adds `loop_template` to the plan:

```json
{
  "prompt": "Reviewed single-line periodic audit duty",
  "prompt_sha256": "SHA256 computed locally from those exact UTF-8 prompt bytes",
  "mode": "fixed_interval",
  "interval_seconds": 60
}
```

This is a schema example, not a ready-to-run plan or digest. Build the actual
prompt from the task's approved audit-only instructions and frozen publisher
path. Hash it locally in the same preparation that writes the readonly plan.
The controller rejects empty/multiline prompts, leading/trailing whitespace, mismatched hashes and other
schedules; it cannot infer the semantic safety of arbitrary prompt text.
The exact native command is `/loop every 60s PROMPT`, not `/loop create ...`.
Creation requires a complete empty loop list, the original paused/blocked goal,
complete terminal-only history and Herdr idle. It records the actual new loop
ID/creation time and requires a new full list to confirm the same record.

`loop-pause`, `loop-resume` and `loop-delete` instead require `expected_loop`
with exactly the intended `loop_id`, `created_at_ms`, `prompt_sha256`,
`mode: "fixed_interval"` and `interval_seconds: 60`. Obtain these values by
reading the actual creation evidence locally. The current full list must have
exactly that one loop, with matching nested profile/session and unchanged
prompt/schedule. This dedicated-session controller refuses missing, foreign
or additional loops before sending a mutation.

The native pause/resume/delete request uses `session_id` and `loop_id` only;
R5 omits outer profile_id on these mutation replies. The nested loop record
must still match the bound profile/session. A fresh goal read and complete
loop list follow the action. Delete must return the same deleted record and
the next list must be empty. Only the owned ID is ever addressed.

Resume requires the original loop paused. Both resume and delete require the
goal paused/blocked, terminal-only history and Herdr idle. Delete can address
the sole active loop but does not prove that a concurrently admitted turn was
cancelled. Loop pause allows a running turn to
settle; its success never asserts idle. An overdue loop may run immediately
on resume: wait for actual terminal evidence before attempting a goal resume
that requires no active turn. Do not fire a tick manually or recreate a loop
to obtain a new delay.

All loop controls use the same exclusive intent, bounded observations and
outcome_unknown/no-retry rules as goal controls. Creation/status proves only
the schedule's observed state. The middle must separately verify natural
scheduled turns, actual audit tests and immutable captures before/after
restart, same-ID restoration, and final loop removal/receipt/Matrix closure.

### Distinct recovery attempt preparation

After a consumed observer attempt and failed watch, preserve those exact files
and prepare a new explicitly authorized recovery namespace. The middle uses
`node scripts/prepare-native-fault-attempt.mjs --plan FILE --sha256 HEX` with an
existing canonical readonly JSON plan and its locally computed digest. The
synchronous library API is `prepareFaultAttempt(plan)`; library import does not
execute the CLI. Both native-control and preparer CLIs remain callable through
the complete Claude/Codex skill-directory symlink, including Node's
`--preserve-symlinks-main` mode.

The plan has exactly these fields:

| Field | Required value |
| --- | --- |
| `version` | `1` |
| `attempt_id` | Fresh UUIDv4 for the explicitly authorized recovery attempt |
| `base_control` | Existing canonical private0700 control directory, outside the business project |
| `project` | Existing canonical business project, disjoint from control |
| `trace` | Original current JSONL trace path, bound by predecessor evidence |
| `recovery_contract` | Canonical readonly `{path, sha256}` JSON pin |
| `predecessors` | Exact named pins: `audit`, `safe_state`, `observer_binding`, `observer_claim`, `observer_ready`, `watch_manifest`, `watch_exit` |

The recovery contract has exactly `version`, `attempt_id`, `base_control`,
`project`, `predecessors`, `goal`, `stages` and `predecessor_full_acceptance`.
Its shared fields match the plan; `goal` binds the original `goal_id`,
`created_at_ms` and `objective_sha256`, `stages` is
`["interrupt03", "restart04"]`, and `predecessor_full_acceptance` is false.
Read the actual failed audit, exit, old claim/ready and safe-state observations
locally to obtain these pins. Do not fabricate them from display text.

The preparer verifies the failed predecessor's actual exit3/manifest hashes,
original goal and consumed observer records, plus the original trace inode
and complete prefix. It creates
`base_control/recovery-attempts/attempt_id/` exclusively at0700, publishes
readonly `attempt.json`, hard-links the original trace as `trace.jsonl`, then
publishes readonly `prepared.json` after verifying source/link custody. No
copy, symlink, chmod of the trace, overwrite or retry fallback is allowed.
Source/link identity and retained prefixes are checked again after publication.

Successful stdout is one bounded `status: "prepared"` JSON record with
`attempt_path`, `manifest_path` and `full_acceptance: false` (exit0). Invalid
inputs fail with static reasons; a consumed attempt is rejected; failures after
claim preserve its directory and available manifest path (exit1). Keep every
claimed failed attempt. The CLI exposes no test hooks, force or retry switches.

Control/project ancestry must be owned by root or the effective user, with
trusted sticky-directory semantics where applicable; retained inode custody
is rechecked before mutations. The private control directories must be owned
by the effective user at0700. This does not make Node pathname operations
atomic against a malicious actor with the same effective UID; postchecks
reject observed substitutions rather than claiming that all such mutation
races are impossible.

Prepared lineage is historical evidence, not current process/readiness proof.
This command launches no observer, sends no native control or signal and writes
no stage-release marker. Before the later coordinator acts, independently
verify the new watch and approval continuity, current original goal/turns,
protected artifacts, actual observer argv/cwd/birth, pinned frozen helpers and
stage-specific prerequisites. Restart04 cannot be bound before the real03,
loop, blocked and observation-timeout evidence exists.

### Observer process metadata on macOS

An observer outside the lower Herdr pane needs its own actual PID metadata.
`native/darwin-process-metadata.c` provides a narrow, read-only query; it does
not launch or signal that process. Compile the reviewed source ahead of the
recovery window with the host SDK (`/usr/bin/clang -std=c11 -O2 -Wall -Wextra
-Werror SOURCE -o OUTPUT`), record the resulting binary's SHA-256 locally,
and pin that exact binary path and digest in the coordinator plan. There is
no runtime compiler, automatic download or alternate collector fallback.

The only command form is `BINARY --pid PID`, where PID is a canonical positive
decimal value no greater than INT_MAX. Successful exit0 publishes exactly one
JSON object with `version: 1`, `pid`, absolute `cwd`, and the exact `argv` token
array. Empty arguments after argv0, spaces and Unicode retain their boundaries.
Invalid observations produce nonzero exit and a static error category, with
no metadata. Never substitute a launch command or split the output of `ps`.

The caller must collect a fresh birth snapshot before the query and another
after it, check both with `assertProcessIdentity`, and enforce its own process
deadline. The collector additionally brackets its kernel reads with PID,
target bitness and birth checks. These separate observations are not atomic.
Its internal 1.5-second checks cannot interrupt a blocked syscall; the parent
deadline remains necessary.

Argument data is capped at 4 MiB and argc at 16384. The parser uses the observed
target width and exact XNU padding, rejects empty argv0 and malformed UTF-8,
and reads exactly argc NUL-terminated arguments. The kernel buffer contains
environment bytes in memory; they are not parsed or published. Non-Darwin
production builds explicitly return `unsupported_platform`. Portable parser
tests do not establish Linux or Windows process observation support.

Source tests query only independently owned fixture children. Managed-observer
permission, identity and freshness still require a live check during recovery;
this helper alone does not establish observer readiness or E2E acceptance.

### Activating an armed recovery stage

Use `scripts/run-stage-release.mjs --plan FILE --sha256 HEX` only with an
approved readonly plan for a prepared recovery attempt. The middle owns this
operation and its observer; the lower worker must never create its own stage
release. This command does not start an observer, send a fault signal, create
a goal or recreate a loop. Frozen observers retain the actual Ctrl+C/SIGKILL
and first-window checks.

Plan version1 identifies `activation_id`, `stage` (`interrupt03`/`restart04`),
the `attempt_manifest` and `prepared` pins, launch `observer_binding`, exact
`observer_claim`/`observer_ready` pins and observed `observer` identity. Include
`metadata_tool`, `frozen_helpers` (`observer`/`adapter`), all prior increment
`protected` hashes, the native controller `binding`, and `expected_goal`.
Restart04 additionally requires the original `expected_loop`. Every pin is
exactly `{path, sha256}` and refers to locally read bytes, never display text.
`controller_tools` pins the adjacent `native-control.mjs` as `controller` and
`native-control-evidence.mjs` as `evidence`; calculate these from the actual
distributed files when preparing the plan.

The controller `binding` keeps its existing native-control schema. The separate
frozen observer binding carries the stage, attempt control directory, instance,
trace prefix, goal/loop hashes and protected subset. Shared identities must
match; do not add these observer-only fields to the controller binding.

On macOS, the frozen observer's instance may retain `/tmp/` in the actual
backend argv. Only this field permits the verified root-owned system symlink
directly to `/private/tmp`, with no additional symlink in the descendant.
Keep the original spelling and exact argv; do not rewrite a claimed binding
to make it canonical. Other plan, tool and evidence paths remain canonical.

`child_operations` fixes distinct lowercase canonical UUIDv4s for `initial_inspect`, `goal_resume`
and `final_inspect`. Restart04 also fixes `loop_resume` and an ordered array
of 1–128 `settlement_inspects`. All IDs must differ from one another and from
the activation ID, which also uses lowercase canonical UUIDv4. Invalid native
child inputs are rejected before consuming the stage claim. Specify
`activation_timeout_ms` (5000–240000),
`query_timeout_ms`/`observer_timeout_ms` (100–10000),
`settlement_timeout_ms` (100–120000) and `settlement_interval_ms` (100–5000).
These are bounded control/query windows; they do not extend the original
managed task or observer deadline.

`prerequisite_pins` retain already reviewed historical evidence. Restart04
requires `natural03`, `pre_restart_audits`, `prerequisite_waits`,
`blocked_terminal_order`, `tiny_timeout`, and `same_backend_readonly_recovery`.
Their hashes preserve inputs. They cannot establish that those experiments
were valid: the middle must verify their real native evidence before creating
the plan and again during the final independent audit.
Interrupt03 permits an empty prerequisite map. Pins require readonly JSON
objects; no added `version` or success label can replace historical evidence.

The coordinator first checks the original paused/blocked goal, terminal-only
history and Herdr idle;03 requires no loop,04 the sole original paused loop.
It then queries fresh observer birth/argv/cwd, checks the readonly ready
record, trace prefixes and protected artifacts, and writes a durable intent.
The exact stage release is published without replacement.03 resumes the
original goal once.04 resumes the original loop once, observes any natural
audit through bounded readonly inspections until idle, checks the observer
again, then resumes the original goal once. A final native inspection must
observe the original goal active and the required original loop state.

Immediately before each mutating child starts, the parent checks that the
latest observer proof is at most two seconds old, after hashing tools and
publishing the child input. If those writes made it stale, activation stops.
This is a parent dispatch check; downstream native queries and mutation are
not an atomic observer admission operation.

One fixed `attempt_path/activation-STAGE/` directory consumes that stage even
if the process fails. It retains the plan, intent, observer evidence, actual
child exits and controller reports, plus `result.json` when publication
succeeds. New activation IDs, child IDs or repeated invocations cannot bypass
that claim. Each controller runs as a bounded child; an exit code or report
alone is insufficient—both must match the planned operation and evidence.

`activated` exits0 but always has `full_acceptance: false`. `failed`,
`outcome_unknown` and `rejected` exit1. A failure after release publication
may have begun is unknown; preserve the claim, release and partial child
evidence. Inspect the actual state separately. Do not retry a mutation, remove
a guard, roll back the release or create a replacement loop to get a green
result. Private child logs can contain native frames and process metadata;
the bounded public report does not include them.

These controls do not prove interruption, automatic restart, natural business
completion, monitoring/approval continuity, final receipt or Robrix delivery.
Every original full-chain acceptance gate remains required.
