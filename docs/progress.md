# Repository audit — 2026-09-05

## 2026-09-12 — Ceiling slice 3: admission uses the draw, headroom published (ADR-123)

- Folded the ADR-121 ceiling report into admission (`DomainRepository::approve`),
  mirroring the retained `remainingFor` (backend-v2.js:14036-14060) verbatim:
  `drawn = max(reserved, spent)` with unknown measurement falling back to
  reserved (never zero), `by_ceiling = ceiling.saturating_sub(drawn)`, admitted
  figure = min of the non-null limits (ceiling after draw, seat quota, pool),
  and a seat-period mismatch nulls the figure (:14057). The engagement being
  decided is excluded from its own commitment sum exactly like the retained
  `decide()` call; `budget()`'s previously hardcoded `exclude_engagement_id`/
  `for_auto_join` are now parameters (`resource_budget` keeps its shape).
  The slice-2 `SpendContext` now carries `spent`/`consumed`/
  `spend_period_key`, so `Error::OverCommit` names the binding draw, the
  period key and the cache-read discrepancy from real measurements.
- Published the authority's own figures on `UsageReport`: new
  `ceiling: { tokens_drawn, tokens_used, remaining_tokens }` computed from the
  same report and budget the decision uses; every existing key unchanged (the
  usage API exact-key test extends with `ceiling`).
- Oracle: `ceiling-vectors.mjs` gained end-to-end admission expectations
  computed by the retained ledger + mirrored remainingFor arithmetic —
  lockout approves 1M (remaining after 9.0M), fresh exhaustion refuses (0
  left); fixture regenerated, `--check` green.
- Tests: `native_ceiling_admission_uses_drawn_not_consumed` (13.6M consumed /
  681k drawn approves 1M), `native_ceiling_admission_refuses_fresh_exhaustion`
  (10M fresh refuses, naming measured spend), and
  `native_ceiling_publishes_headroom_after_approval` (drawn 1_000_100, used
  13.6M, remaining 8_999_900). The ceiling overrun alarm (G5) is a listed
  follow-up, not implemented.

## 2026-09-12 — Ceiling slice 2: refusals name the binding draw (ADR-122)

- Implemented slice 2 of the accepted ceiling-enforcement plan: pure wording
  plus refusal identity, admission decision unchanged. `hagency_core::ceiling`
  now exports `over_commit_message(agent, alloc, remaining, ctx)` and
  `SpendContext` — a byte-faithful port of the retained
  `lib/engagement-store.js:58-103` (`compactTokens`, plain head, ceiling
  sentence, both binding-draw sentences with the period-key parenthetical,
  the conditional cache-read note, preset/period remedy).
- Store error split at the approve sites (`domain.rs:717-719`): unknown
  capacity → new `Error::NoCeiling` (the JS `no_ceiling` wording); a known
  pool ceiling the allocation exceeds → new `Error::OverCommit { message }`
  carrying the wording; the shared-seat quota binding keeps the existing
  `Error::InsufficientCapacity` — the resource-pool refusal the JavaScript
  does not model, per the brief. The three tests matching the old variant are
  re-pointed (`tests/domain.rs:130`, `tests/resource_configuration.rs:120` →
  `OverCommit`; `tests/domain.rs:150` seat-binding keeps
  `InsufficientCapacity`).
- Oracle: `ceiling-vectors.mjs` now imports the retained `overCommitMessage`
  (sha256-pinned via `engagementStoreSha256`), computes three message vectors
  mirroring `tests/ceiling-draws-fresh-tokens.test.js:279-346`; fixture
  regenerated, `--check` green. Three core tests replay it and pass anywhere;
  the fourth (store-level `no_ceiling` vs `over_commit` distinction) uses
  explicit match arms and needs a repository open (CI).
- Gates: fmt, clippy `-p hagency-core -p hagency-store --all-targets` clean;
  `cargo test -p hagency-core --locked` green (wording tests 3/3); the store
  suite cannot open repositories in this sandbox (known cap-std ancestor
  EPERM, as in every slice) — the store selector compiles and is CI's to run.

## 2026-09-12 — OutcomeUnknown attribution: writer dequeue, runner 504, heartbeat, file view

- Implemented the accepted design (`.peer/evidence/context-outcome-unknown-attribution.md`)
  under ADR-053/ADR-101 amendments, no production bound or behavior change.
  `DomainStore` now attributes each `Error::OutcomeUnknown` to whether the
  writer had dequeued that command: one monotonic ticket per call, published
  when the dequeued job starts running, compared by equality on reply-bound
  expiry, exposed as `DomainStore::last_unknown_dequeued()`. The verdict stays
  single (`OutcomeUnknown` — reconcile before retrying); the bit is diagnostic
  only. The runner task surface projects it into its 504 code
  (`outcome_unknown_running` vs `outcome_unknown`).
- Deviations from the design, both flagged in the peer report: (1) the ticket
  is published inside the dequeued closure rather than as a new `Job::Run`
  field, because the field would force edits to seven `Job::Run` test
  constructors outside this slice's ownership; the §1.1 invariant (publish
  before the operation can block) is preserved. (2) `failure()` keeps its
  2-arg signature and delegates to `attributed_failure(Option<&DomainStore>)`
  — the design's signature change broke four callers in runner submodules
  (`completion.rs`, `replies.rs` ×2, `workflows.rs`) that the design's
  13-site count missed and that are outside ownership; their behavior is
  unchanged (plain `outcome_unknown`).
- Heartbeat observability: the MCP probe writes an atomic `owned-mcp.spawned`
  receipt immediately after the helper exists, and the failing owned-MCP
  assertion now reports `parent_started`/`helper_spawned` plus protocol and
  cleanup, so an empty stage list names its own cause. File view:
  `UnknownOrigin::{Remote,Custody}` names the producer of an unknown
  (`outcome_unknown` from the durable receipt vs `outcome_unknown_custody`
  from unacknowledged local custody) with a closed validate set; `inspect`
  and replay still report the receipt-derived base code (ADR-101 amendment
  says so explicitly). ADR-053 and ADR-101 gained dated amendment paragraphs;
  the dispatch spec gained the dequeue scenario (and `runner.rs` in Allowed
  Changes), the file-service spec the two-label scenario.
- Gates: `cargo fmt --all --check`, `cargo clippy -p hagency-store -p
  hagency --all-targets --locked -- -D warnings` and `cargo check --tests -p
  hagency` pass. `cargo test -p hagency-store --locked
  native_domain_unknown_reports_dequeue` cannot execute here: the selector
  compiles and runs but fails at fixture setup (`DomainRepository::open`,
  `Io(EPERM)`) — the same sandbox ancestor-directory denial recorded in the
  two entries above; CI is the authoritative executor.

## 2026-09-12 — Ordered per-entry approval phase trace (ADR-046 stage-1 diagnostic)

- Diagnostic only, no behavior change, per the accepted approval-barriers
  analysis Q4 (`.peer/evidence/context-approval-barriers.md`): the failing
  owned approval tests report only `ApprovalCancelled` at `RuntimeStage::Update`
  and cannot say which primitive fired (`ApprovalResolved` on an unwritten
  entry vs `TurnEnded` while any entry is unwritten) or where the entry's
  frame had reached. Added an ordered per-entry phase trace and a
  cancellation record so the next failure names both.
- `native/hagency-execution`: pending entries carry a test-only `PhaseTrace`
  (`["retained"]` at insertion, then `acknowledged`, `prepared`, `begun`,
  `admitted`, `checked`, `write-accepted`, `recorded`, plus the cancellation
  primitives `resolved-before-write`/`resolved-after-write` and
  `turn-ended-unwritten`). Every mark also lands in a process-wide journal;
  when a cancellation primitive fires, the offending entry id, primitive and
  full trace are recorded in a slot read by
  `hagency_execution::diagnostics::last_cancellation_trace()`. The five
  failing assertion sites (`approvals.rs` resume/reuse/barriers/usage,
  `approval_loss.rs` pending-receipt) now include that trace in the panic
  message. All of it compiles only under `cfg(test)` or the default-off
  `test-diagnostics` feature (self dev-dependency lights it for test builds
  only); `strings` on a production build of the rlib finds none of the labels.
  New unit test `native_approval_trace_labels_every_phase` pins the
  vocabulary and both resolution outcomes.
- Gates: `cargo fmt --all --check` and `cargo clippy -p hagency-execution
  --all-targets --locked -- -D warnings` pass. `cargo test -p
  hagency-execution --locked` cannot complete here: every owned/lib fixture
  dies at `DomainRepository::open` with `Io(EPERM)` — the same sandbox denial
  of the cap-std ancestor-directory walk recorded in the ceiling-draw entry
  (clean-HEAD `git stash` control reproduces it identically, e.g. at
  `approval_loss.rs:52` unmodified vs `:53` with the reset added). The
  deterministic unit test passes in the lib binary (5/5 runnable tests,
  including the new one). CI is the authoritative executor for the owned
  selectors.

## 2026-09-12 — Native read-side resource ceiling draw (ADR121)

- Slice one of the ceiling-enforcement port (accepted gap analysis,
  `.peer/evidence/report-ceiling-enforcement.md`): added
  `DomainRepository::resource_ceiling(resource_id, at)` returning a
  `CeilingReport` that mirrors the retained JavaScript `ceilingSpendFor`
  object — `reserved` (SQLite SUM over the resource's `reserved`/`active`
  engagements), `spent` (fresh kinds only: input+output+cacheWrite from the
  current period's per-engagement `known_growth` lower bounds), `consumed`
  (display sum over all four kinds), `ceiling_tokens`, `preset_name`,
  `spend_period_key`, and `drawn = max(reserved, spent)` with an unmeasured
  period falling back to commitments (`backend-v2.js:14052-14053` mirrored
  verbatim). An absent period bucket stays `None`, never zero. Read-side
  only: no admission, refusal or publication change; evidence stays
  host-attributed and untrusted.
- Oracle `native/scripts/ceiling-vectors.mjs`: the retained
  `lib/metering/ledger.js` computes spent/consumed for eight vectors
  including the BigLittle lockout (681,089 drawn vs 13,609,601 consumed) and
  a month-roll unknown case; ledger and backend sources pinned by sha256;
  `--check` added to the Rust CI job after `usage-vectors.mjs --check`.
  Rust replay: the four selectors in
  `specs/task-rust-usage-ceiling-draw.spec.md` (ADR-121, Proposed).
- Local gates: `cargo fmt --all --check`, `cargo clippy -p hagency-store
  --all-targets --locked -- -D warnings`, `node ceiling-vectors.mjs --check`
  and `node usage-vectors.mjs --check` pass. `cargo test -p hagency-store`
  cannot run in this sandbox: every `DomainRepository::open` fails with
  `Io(EPERM)` because `accounts::Registry::open` walks the store directory's
  ancestor components via cap-std `openat` from `/`, and the sandbox denies
  opening `/Users` and the home ancestors (probe evidence in the peer
  report). Verified pre-existing: a clean-HEAD stash control reproduces the
  identical failure on untouched test binaries.

## 2026-09-09 — Project names and revoke feedback

Verified Edison's prior revocation and the missing project-name projection.
Implemented observed Matrix name metadata, room-keyed labels, persisted departure
results, explicit retries and lost-response reconciliation.144 distinct tests,
production console build, scoped lint and fixture Playwright pass. Live names and
unchanged allocations verified; one unrelated usage read still returned502.
Native lifecycle boundary passes with five behavioral skips. Local idle services
restarted; no Palpo deployment, commit/push or canonical task mutation.
Details: docs/reviews/2026-09-09-engagement-console-recovery.md.

Latest update: the operator subsequently requested closing the findings. The fixes
and their evidence are recorded in the
[closure report](reviews/2026-09-05-review-closure.md), on
`fix/spec-review-closure`. Full Vitest passed **226 files / 3,783 tests**, with one
platform skip. Console fixtures passed **110 static/rendered and 39 browser
checks**; the final runtime passed **5/5 real-model continuity conversations**.
The CI wrapper passed; the final readiness persistence change passed its 18-test
closure suite. Agent-spec boundary verification passed with 21 lifecycle skips
because its verifier does not execute Vitest.
Claude Code Fable supplied two further independent static reviews, preserved
alongside the closure report. The full deployed workflow, external Agent
Operations release evidence and conflicting approval-channel decision remain open.

The text below records the preceding audit, before implementation.

This is an observation report, not canonical control-plane task state. The source
checkout has no provisioned `./task-writer` or agent-home project manifest.

Requested work: pull the latest HAFleet and assess completion against the planned
specification. `git pull --ff-only` advanced `master` from `0b1193d` to
`75ca1ecbf8c4623359094f000fa4968693f4a27e` in `/Users/yuechen/home/hagency`.
The checkout was initially clean. No implementation or test source was changed.

**Verdict: the project cannot be signed off as fully complete against its current
contracts.** Much is implemented and tested, but a reproduced behavior contradicts
the latest contract, acceptance coverage is incomplete, and release evidence is
still outstanding. This audit does not assign a completion percentage: passing
test counts are not a measure of fulfilled requirements.

## Findings

1. **Transport-provenance errors can be acknowledged instead of retried.**
   [The side-provenance contract](../specs/task-side-provenance.spec.md) (lines 54,
   114–125) requires missing, invalid, or inconsistent adapter provenance to produce
   retryable `invalid_transport_provenance`, HTTP 500, and no completed transaction,
   edge acknowledgement, or sync cursor advance. However,
   [lib/side-provenance.js](../lib/side-provenance.js) (lines 60–78) classifies an
   invalid mode, mismatched side, or mismatched registration as terminal
   `provenance_mismatch`. [bridge-matrix.js](../bridge-matrix.js) (lines 4401–4436)
   consumes that terminal result, allowing the receiver to remember the transaction
   and return 200. A local HTTP probe through the real listener, router, and bridge
   ingress, with a controlled internal provenance fault after authentication,
   reproduced 200 on both delivery and replay for all three inconsistent-context
   cases. Missing provenance correctly returned 500. Every case made zero typed
   business calls and zero event claims; the defect is acknowledgement/retry
   behavior, not demonstrated unauthorized execution. An internal wiring fault can
   therefore discard work that the contract requires retaining for retry.

2. **The side-provenance tests do not prove the promised scenario matrix.**
   The contract's binding instructions (line 57) require each of its 24 named
   scenarios through actual push, edge, and sync adapters. The test named
   `side_provenance_missing_or_inconsistent_context_keeps_batch_retryable` explicitly
   expects inconsistent provenance to succeed without throwing, contrary to its
   acceptance text ([tests/side-provenance.test.js](../tests/side-provenance.test.js),
   lines 165–201). The later `r5_all_spec_titles_across_edge_and_sync` test (lines
   1304–1365) loops over all 24 labels but uses the same valid-room, valid-credential,
   successful-message fixture each time; the label only changes the event ID. It
   does not inject the corresponding negative condition or multi-instance setup.
   These green checks cannot establish the required negative-case coverage.

3. **Executable contract bindings and requirement links have drifted.**
   Of 157 declared `Test:`/`Filter:` selectors, 13 match no registered test title.
   Some corresponding behavior exists under different names, so this is not a
   claim that all 13 behaviors are missing. Nevertheless, those exact acceptance
   bindings select no evidence. The sync-intake contract also declares
   `satisfies: REQ-AGENT-OPS-MATRIX-INTAKE`, for which no defining knowledge artifact
   or requirement statement was found. The checked-in traceability baseline is
   historical and cannot establish current coverage.

4. **Release completion remains unproven.**
   [The accepted thread-session requirement](../knowledge/requirements/req-thread-scoped-agent-sessions.md)
   requires a five-run, three-turn real-model continuity probe with at least four
   successes. [docs/THREAD-SESSIONS.md](THREAD-SESSIONS.md) (lines 3–10, 62–71)
   records a local canary but explicitly says this probe has not run and blocks
   non-local release. No superseding result was found in the reviewed repository.
   The [Agent Operations manifest](../specs/fixtures/agent-ops-client-v1/manifest.json)
   still has `release_status: "development"` and `source_commit: null`; its passing
   integrity check is not evidence of a released client contract.

5. **Local full regression verification is not clean.**
   `tests/api-engagement-room-admission.test.js` failed during fixture setup:
   `POST /api/framework-presets` expected 200 and received 404, before the selected
   room-withdrawal assertion ran. The exact test passed on an isolated rerun; the
   cause remains unresolved. `tests/hafleet-up-selfcheck.test.js` failed and failed
   again in isolation because lines 57, 59, and 76 hardcode `/usr/bin/tmux`, which
   does not exist on this Mac; the installed executable is `/opt/homebrew/bin/tmux`.
   The one skipped full-suite test is the non-macOS refusal case in
   `tests/install-macos.test.js`, skipped on macOS.

## Verification

The root dependencies were installed from the pulled lockfile using `npm ci`.
The ignored `remote-dist/` snapshot was initially stale and was rebuilt with
`npm run build:remote`; its subsequent checks passed. This was a generated-artifact
refresh, not a source fix. Local runtime: Node v24.10.0, npm 11.6.0, macOS.

| Check | Result |
|---|---|
| Latest-commit GitHub CI | Lint and full-test jobs passed on Node 22.22.0: [run 33992646874](https://github.com/hagency-org/HAFleet/actions/runs/33992646874) |
| Full local `npm test`, with JSON reporter | 223 files: 221 passed, 2 failed; 3,702 tests passed, 2 failed, 1 skipped; 351.07 seconds |
| Separate Vitest run using the literal contract selectors as escaped name filters | 149 tests passed; 13 selectors matched no title. The 3,556 filtered/skipped tests are not passing evidence from this run |
| Syntax, ESLint undefined-identifier check, CLI contract | Passed |
| Architecture and dependency boundaries | Passed |
| Router typecheck, import boundary, generated build check | Passed |
| Remote source snapshot, source sync, generated package smoke | Passed after rebuilding ignored `remote-dist/` |
| Agent Operations artifact integrity | Passed, explicitly in development status |
| Contribution console `npm run verify` | 110 rendered/static checks and 39 browser checks passed against a temporary localhost fixture-mode console; this does not prove live backend integration |
| `npm run verify:ci` wrapper | Could not start: GNU `timeout`/`gtimeout` absent. Available static/build gates were run directly; no claim that the local wrapper passed |
| `agent-spec parse`, lint, lifecycle | Not run: `agent-spec` unavailable on PATH and absent from checked installation locations. The repository also documents the older Cargo-only lifecycle limitation for Node |
| Live Matrix/model/federation and deployment release checks | Not run; external behavior is not certified by the local fixture tests |

The temporary console was stopped after verification. Logs, JSON test reports,
the binding audit, and the local provenance-probe output are retained under
`/Users/yuechen/Library/Caches/hafleet-audit/2026-09-05/`.

### Contract binding inventory

“Resolved” means the selector matched an executed test title; it does not mean
the test proves every clause of its scenario, as finding 2 demonstrates.

| Contract | Resolved selectors | Declared selectors |
|---|---:|---:|
| Project baseline | 4 | 4 |
| Agent Operations client access | 18 | 18 |
| Appservice sync intake | 8 | 8 |
| Matrix DM privacy | 6 | 6 |
| Matrix thread continuity | 10 | 10 |
| Owner UI approval | 17 | 21 |
| Project board | 10 | 18 |
| Side provenance | 24 | 24 |
| Thread-scoped agent sessions | 47 | 48 |
| Total | 144 | 157 |

Unresolved selectors:

- Owner UI approval: `project room retains independent room-agent approval bindings`;
  `stale crypto store is archived before the token device starts syncing`;
  `launchers_keep_sandbox_defaults_and_wire_only_supported_adapters`;
  `repairs_identical_duplicate_sections_before_codex_parses_the_file`.
- Project board: `project_board_redacts_runtime_secrets_and_paths`;
  `project_board_groups_tasks_by_status`; `project_board_includes_related_task_graph`;
  `project_board_marks_stale_agent_task`; `project_page_renders_board_surfaces`;
  `project_board_proxy_is_read_only`; `project_page_coalesces_refresh`;
  `project_agents_link_to_the_monitor_and_monitor_has_complete_navigation`.
- Thread-scoped sessions: `runner workspace configuration requires operator authority`.

Scope interpretation: accepted `knowledge/` artifacts and the nine current specs
were the baseline. The console integration plan records P0–P4 as implemented.
The older PDU PRD is partly withdrawn and still contains unresolved scope questions;
withdrawn scheduler/pricing work was not counted as missing functionality. The
Octos/remote thread-runner expansion is explicitly a non-normative future roadmap,
so its exclusions were not treated as current implementation defects.

To reach sign-off, first align provenance retry behavior and its real adapter
tests with the accepted contract, repair or formally retire stale acceptance
bindings and resolve the dangling requirement link, address regression reliability
and the portable tmux lookup, then collect the required lifecycle and release
evidence. This audit did not implement those follow-up changes.

## Extended code review

The operator next requested a comprehensive review before live end-to-end testing,
and explicitly requested Claude Code Fable as an independent parallel reviewer.
The detailed findings, accepted-scope distinctions, coverage matrix and proposed
sign-off sequence are in
[the extended report](reviews/2026-09-05-spec-gap-review.md).

The local review added temporary backend API fixtures and real RouterStore/runner
process fixtures. Confirmed effects include leases released while processes can
still write; surviving runtime descendants; incorrect side/retired-agent selection;
unknown seat periods permitting auto-join; independent API keys merging into one
seat; active success despite a missing owner binding; and replay refused after the
original request consumes a side's allocation. A fresh-fleet request with a valid
resource preset still cannot provision its serving agent on approval. An HTTP push
probe also demonstrated that submitted body.mode controls the intake-mode metadata.
These probes used local fixtures and cleaned up their stores, sockets and processes.

Same-revision broad test/build evidence above was reused. No application or test
implementation was edited, and no live Matrix, remote mini, or real task-executing
model workflow was run. Claude Code was separately launched as the requested
reviewer with model claude-fable-5 and read-only file tools; it has no authority to
change the repo or contact project services. Review evidence is under
`/Users/yuechen/Library/Caches/hafleet-review/2026-09-05/`.

Claude Code Fable completed its independent static review successfully. Its original
output is preserved in [the Fable review](reviews/2026-09-05-claude-fable-review.md).
The consolidated report reconciles its findings rather than treating its conclusion
as test evidence. Additional fixtures reproduced ambiguous-token first-match
dispatch, requester-token room claims gaining whitelist admission, private owner/
configuration details in requester responses, and deletion of one side resolving
another side's identity alert. The side-removal membership-cleanup omission was
confirmed by source tracing. Accepted approval-channel documents still conflict;
cross-family review coherence and representative identity composition are documented
with their practical limits. The review is complete; implementation and live
remote-mini/Palpo/model testing remain separate follow-up work.


## Live remote Palpo and local Robrix2 UX verification — 2026-09-06 UTC

Following implementation closure commit `f89c746`, deployed an isolated remote
Palpo/DB/appservice edge and exercised local HAFleet with real Playwright Chrome and
native headless Robrix2. Earlier review entries above remain historical; the closure
commit fixes their bounded implementation findings, but does not establish live
workflow acceptance.

The live walkthrough finished with failures. Definitions, real room interaction,
request admission after setup recovery, explicit verdicts/revocation, whitelist
auto-admission/removal, budget refusal, tested authorization and uncertain-outcome
recovery passed. Fresh setup still needs out-of-band recovery. On-demand Claude
homes lack required MCP config; exposed messaging/lifecycle tools conflict with the
ephemeral allowlist; Codex stalls on unhandled MCP elicitation. Scoped create_task
produced a real child/thread, but no completed delegation or integrated artifact.
Native Agent Operations remains gated. These findings were recorded, not fixed in
this run. See [the live UX report](reviews/2026-09-06-live-ux.md).

No model dispatch remains queued/running. The coding task stays blocked, readiness
stays in_progress and the queued child was cancelled before start. Extra engagements
ended and the test whitelist was removed; two original engagements remain for
inspection. Existing remote services were preserved. Full evidence and host inventory
are kept privately outside this repository. Robrix gained an uncommitted isolated
profile override, with locked headless build and three exact/lifecycle tests passing;
no HAFleet application implementation changed during this live test.

## Fresh live closure in progress — 2026-09-06 UTC

The authorized follow-up repaired runtime MCP approval handling, scoped task and
peer tools, Claude home preparation, ordinary representative sync, private owner
validation, preset capacity, console allocation/approval controls and runtime
cleanup reporting. Claude Code Fable completed the requested independent review;
confirmed follow-ups are recorded in the current closure report. The stable full
suite after the test-server address-family isolation fix passed 234 files and
3,863 tests with one platform-specific skip. Earlier failed full runs and skipped
agent-spec lifecycle scenarios remain preserved.

A separate clean remote Palpo deployment and local runtime now pass fresh preset
creation, registration-token verification, native room creation, allocation, both
role requests and automatic home/Matrix membership fulfillment. The project bot
stays outside the room while representative intake routes the native task mention.
The real coding agent produced three passing tests. Native owner Deny produced an
encrypted Matrix verdict, was consumed as `owner_denied`, and created no child.

The same-turn retry then exposed a database uniqueness defect in approval waits;
that dispatch is `outcome_unknown`. Independent workspace inspection still finds
three passing tests and no README or child task. Console Stop rejects the fresh
thread agent because it lacks a legacy tmux session. Repairs and the allow,
delegation and integration retest continue. Source changes remain uncommitted.
See [the live closure report](reviews/2026-09-06-live-ux-closure.md); this entry does
not declare the workflow complete.

## Core live workflow verified; final client checks — 2026-09-06 UTC

The repaired original workflow completed real Codex implementation, encrypted
owner Deny followed by a fresh Allow, a real Claude documentation child, automatic
recovery of the unchanged scoped reply, parent integration and final Matrix
delivery. Both tasks are done. Independent tests and README/package examples
pass from the actual integrated project. The original approval retry and missing
peer-association failures remain in history, including the authenticated API
outcome-recovery step.

A subsequent native two-stage review completed without backend restart or outcome
recovery: the human thread follow-up became the child input, the binding used the
valid outer Matrix root, separate parent/child sessions executed, the child reply
automatically resumed the parent, and both tasks plus all four dispatches and
Matrix replies completed. Existing invalid nested-thread history was not rewritten.

The real browser Stop probe first exposed false success: an owned detached Node
subprocess survived and wrote 65.192 seconds after Stop. Observed-descendant
tracking and stopped-agent admission were repaired. A new native request/browser
approval provisioned a distinct agent without altering the old engagement,
binding or fence. Its live Stop retest removed the exact observed guardian,
Codex and detached Node processes. An independent check after the full timer
deadline found no completion marker, no late write and no active model dispatch.
The intentionally interrupted task remains blocked/uncertain and its workspace
quarantined. Portable process observation is not universal daemon containment.

HAFleet's later bounded checks pass, including 39 guardian/Stop tests and 116
admission/capacity tests; the earlier full-suite snapshot remains 3,863 passed and
one platform skip. The current agent-spec lifecycle remains non-passing with 15
behavioral skips. Native Robrix rendering, approval previews and drawer action
ownership also have real regression/build evidence. The last native member-cache
refresh retest is in progress. Changes remain uncommitted; the current closure
report separates live acceptance, historical failures and remaining project gates.

## Final native checks verified — 2026-09-06 UTC

The rebuilt Robrix app passes a live membership refresh check against Mini1:
the exact fixture account is absent before join, present after join, and absent
after leave in the same native process. Owned Matrix API calls provide fixture
setup only; no native invite acceptance is claimed. Original five-member project
topology is restored, the owner room was never mutated, and outsider history
access remains 403. The real SDK integration and 56 mention-widget tests pass;
its native lifecycle has two behavioral and one boundary pass with no skips.

Live Threads and Info toolbar actions now affect only the main project screen.
The mounted owner room and same-room thread remain unchanged after opening either
drawer and selecting a thread row; no foreign search modal appears. The actual
three-RoomScreen regression and its behavioral/boundary lifecycle also pass.
Private app-rendered captures and detailed receipts support both native checks.

Final read-only health finds Palpo responding 200, all 15 approval requests
consumed and no active or queued dispatch. Nine historical dispatches completed;
three remain outcome_unknown, including the preserved approval failure and two
intentional Stop probes. The current closure report is finalized for this scoped
workflow run. The project still lacks full signoff for the stated release,
approval-channel, runtime/recovery and HAFleet lifecycle requirements. Source
changes remain uncommitted; no PR or push was made.

## Operator manual web onboarding repaired — 2026-09-06

The operator's `sunwukong-01` home existed, but the shell launcher sourced repo
dotenv over an environment-only backend deployment and fetched its launch profile
from the wrong loopback instance (401). Both entrypoints now preserve the backend's
resolved environment. The launcher's exit cause survives an earlier missing-session
observation. Web onboarding now waits for real health, retains phase-specific
errors and supports retrying the same offline agent without reprovisioning.
Hard-coded restart/supervisor claims and the incorrect ACP remedy were removed
in both languages.

The real web retry passes: online, healthy, MCP present and the selected
`claude-opus-5` runtime. Agent identity, home, token fingerprint and selected preset
are unchanged. Only the exact requested name was added to the dedicated session
allowlist. Four red assertions were preserved; 36 focused and 86 related tests
pass, plus the isolated browser failure fixture and production build. All 230
selectors resolve. The bounded agent-spec lifecycle remains non-passing with four
behavioral skips and one boundary pass. See the
[manual onboarding recovery report](reviews/2026-09-06-sunwukong-onboarding.md).

## Operator desktop client connected — 2026-09-06

Built the current Robrix2 source for macOS and opened a visible desktop window
against the existing Mini1 Palpo deployment. The owned headless app exited
normally before the desktop reused its profile. The persisted login session is
unchanged; the existing account, project timeline and encrypted owner room loaded.
A targeted window capture and private launch receipt verify the visible result.
No new engagement request or approval was submitted during this launch.

## Palpo requirements drafted; operator walkthrough prepared — 2026-09-06

Wrote the requested draft covering Palpo-managed HAFleet admission, scoped
App Service registration, per-fleet reception rooms, verified project targeting,
provider approval and Matrix agent identity lifecycle. It separates upstream
API availability from deployed verification and preserves open approval-channel
and source-room contract questions. Existing accepted requirements are unchanged.

Prepared a six-step manual request/approval/task guide. Read-only prechecks find
the representative connection accepted, coding on offer, no pending engagement,
100k remaining allocation and the visible desktop Robrix2 process. A new coding
request is expected to provision a new agent; the local manually onboarded agent
has no project-side binding. No request or approval has yet been submitted in
this new walkthrough. Later steps remain explicitly pending.

The requirement is machine-readable as proposed, unplanned and unproven; its 23
clauses do not claim implementation coverage. Relative links and whitespace were
checked. The existing live-UX task contract parses and lints; the recorded
documentation-only lifecycle uses lint/boundary layers and remains non-passing
with 15 skipped behavioral scenarios. No runtime test pass is claimed by that run.

## Repeated command reply recovery and manual request diagnosis — 2026-09-06

Palpo and the local tunnel returned 200 while the representative received the
operator's new `!offer`. HAFleet derived the outbound transaction id from room
and reply text, so Palpo deduplicated the fresh answer against an earlier answer.
Five of six new deterministic tests failed before the repair. Reply identities
now include the authenticated input event, per-command reply position and content,
with asynchronous isolation for concurrent commands. Explicit durable send seeds
remain stable; calls without replay identity generate independent send identities.

All six regressions and 279 related tests in eight files pass. Syntax and
whitespace checks pass; all 236 spec selectors resolve. Agent-spec 1.4 parses and
lints the bounded contract at quality 1.0, but its native Node lifecycle remains
non-passing with three behavioral skips. Evidence is retained under the private
run cache's `command-reply-recovery/`. Only the owned local bridge was restarted;
Palpo was not changed. The bridge retained an older blocked history-gap record
with no proven boundary; no historical events were guessed or force-replayed.

A new encrypted owner-room `!offer` and visible desktop response verify current
bot delivery. The representative project-room regression still awaits a fresh
manual command. The operator also sent a coding request for 100,000 tokens and
20,000/day from the private approval room, rather than the intended project room.
Its target is therefore the approval room and it has no project owner binding;
the attempted approval left it pending and unbound. The assistant did not create,
approve, reject or retarget this request. The next manual step is to submit the
intended request from the project room and explicitly verify its private owner
binding before approval.

## Operator-created project room receives requests — 2026-09-06

The operator created a new invite-only, unencrypted project engagement room and
invited the existing representative, which joined through the normal collector.
The assistant performed read-only verification and did not create a duplicate
room or submit a request. Native operator `!offer` received a representative reply;
the operator then requested coding for 100,000 tokens and 20,000/day, and received
the pending-decision acknowledgement. These are the operator's actual amounts,
superseding the earlier walkthrough example of 10,000 and 2,000/day. The new room
requires an explicit owner/private-room binding on its first approval. Detailed
room and event receipts remain in the private cache's manual walkthrough folder.

## Existing customer project registered; approval form prepared — 2026-09-06

The operator encountered the server credential wizard while trying to establish
the new project's customer record. Read-only inspection confirms the existing
Mini1 side is accepted with a registration-token credential; its project metadata
list was empty. The current project request was pending with no fulfillment or
recorded binding failure. Registered the operator-created room under that existing
side through `POST /api/project-sides/:id/projects`, preserving credential kind,
issuance metadata, accepted status and allocation.

Verified that the borrower and representative are joined to the unencrypted
invite-only project, and that the existing separate approval room is encrypted,
invite-only and joined only by the borrower and fleet bot. Opened the new request's
approval form in the dedicated browser and filled its 100,000-token amount, exact
borrower MXID and private approval-room ID. No verdict request was sent; the request
remains pending and unallocated. The screenshot and readback receipt are in the
private manual walkthrough folder. This resolves the immediate setup confusion
using existing APIs; it does not claim a new project-management UI was implemented.

## First-project approval repaired and operator retry completed — 2026-09-06

The operator's repeated `owner_unavailable` result was a missing project-scoped
owner binding. Registering project metadata does not establish that binding, and
the old approval form allowed both ownership fields to remain empty inside a
collapsed section. Prefilling the dedicated testing browser did not repair the
operator's other browser. No request-body loss or unavailable Palpo was found.

The pending queue now reports owner setup readiness from the current requested
project's binding. First approvals open and require the explicit owner MXID and
separate private approval room; existing bindings remain reusable. A stale
`owner_unavailable` response retains its structured code and reopens required
setup with actionable bilingual guidance. No requester-derived ownership,
credential replacement or authority bypass was introduced. The bounded contract
is `specs/task-first-project-owner.spec.md`.

All three new regression selectors failed before the repair. Afterward, 78 tests
in five related files pass, all 239 spec selectors resolve, syntax/diff checks
pass and the production console builds. Isolated Playwright checks against the
built UI intercepted all API calls: missing fields send no verdict, complete
ownership reaches the payload, and a revoked binding reopens required setup.
Agent-spec 1.4 parse/lint pass at quality 0.95238095; its native lifecycle remains
non-passing with three behavioral skips. Separate Vitest evidence is not a native
lifecycle pass.

Rebuilt and restarted only the owned local backend and console. Using the actual
updated browser form, retried the operator's already-attempted 100,000-token,
20,000/day approval for the operator-created engagement room with the independently
verified borrower and encrypted private owner room. The real verdict returned
HTTP 200, active, bound and fulfillment complete; a fresh Codex gpt-5.6-sol/high
agent was created. Direct Palpo membership reads confirm the new identity joined
the intended project alongside the borrower and representative. The private
approval room remains encrypted with exactly borrower and fleet bot joined.
The refreshed console shows the new serving agent. The older request accidentally
submitted from the private approval room remains pending and was not changed.

Evidence is retained under the private run cache's `first-project-owner/`, including
browser regression receipts, actual approval request/response, Matrix readbacks,
console screenshots, build output and lifecycle results. No work message was sent
for the new agent: execution remains unverified. A separate initial runtime-display
gap remains: an eligible new thread agent with no session falls back to the legacy
tmux observer and reports offline/tmux-missing:auto. Its home and task-writer exist,
and no operator stop fence is set; this is not evidence of a completed task or a
healthy running process. Do not equate successful admission with execution signoff.

## Borrower approval receipt and representative loop guard — 2026-09-06

The operator still saw the original “awaiting a decision” Matrix acknowledgement.
Direct timeline reads proved that approval had emitted invite/join membership
events but no result message. Added `specs/task-engagement-approval-notice.spec.md`:
manual approval on a configured side commits pending notification intent alongside
allocation. The authenticated bridge materializes durable Matrix work and sends a
public receipt as that side's representative. It includes role, allocated tokens,
agent identity and serving configuration, and replies to the original request.
Private owner, credential and deployment fields are excluded. Transient failures
retry with one stable transaction identity; delivery requires a Matrix event ID.
Completed work and explicit verdict replay do not reallocate or duplicate receipts.
Revoked engagements are fenced before an unsent or expired-lease notice is claimed.
Unrelated historical approvals are not automatically backfilled.

Three of four initial scenario tests failed before implementation. The first
delivery implementation passed 223 tests in eight related files, but its real
Palpo receipt exposed an additional loop defect: the representative's own message
was parsed as human work because it contained the new agent's full Matrix ID.
This created an unintended task and runtime approval request. The receipt delivery
itself succeeded; the initial live no-work side-effect check FAILED. Do not count
that run as a clean notification acceptance.

Cancelled that exact dispatch through the operator API without granting its pending
execution approval. The runtime trace showed a failed bootstrap/task-writer attempt
and another task-writer attempt awaiting approval; no file-change items or workdir
files modified after the receipt were observed. Codex exit 143 was recorded. Used
the bound outcome-inspection flow with `keep_blocked`: the accidental task remains
blocked and the dispatch remains outcome_unknown with an explicit resolution;
it was not accepted as completed or replayed. The inspected workspace was released
for a future real borrower task. The bridge now ignores its recorded representative's
own output before control-command parsing and agent routing. All three new loop
fixtures failed before that guard and pass afterward, alongside 25 intake/reply
tests and 49 further provenance/ACL tests. Syntax, ESLint and whitespace checks pass;
all 244 spec selectors resolve. The final native agent-spec lifecycle is still
non-passing with five behavioral skips, despite independent passing Vitest runs.

Updated only the owned local backend/bridge processes. Replayed the already-active
operator verdict to recover this one historical missing receipt through the new
supported path. Direct borrower-authenticated Palpo reads verify the representative
receipt and its reply relation. The original approval timestamp, serving agent,
100,000-token allocation and side commitment were unchanged. Native Robrix was
scrolled to the latest messages and visibly renders “已批准 / Approved coding for
100000 tokens” with the actual agent and model. Final readback: engagement active
and bound, receipt delivered, agent idle/unblocked, zero live dispatches. Historical
pending acknowledgement and accidental-task messages remain as audit history.

Private evidence is under the run cache's `engagement-approval-notice/`: receipts,
native screenshots, red/green tests, lifecycle reports and the accidental dispatch's
cancel/inspection/resolution records. Successful execution of a newly submitted
borrower work task remains the next manual validation step.

## Parallel Matrix admin Web App started — 2026-09-06

At the operator's explicit request, delegated implementation to `palpo_admin_app`
in the independent `/Users/yuechen/home/palpo-admin-web` worktree on
`feat/hafleet-admin-web`. The agent located no local Palpo source, cloned upstream
at 3e4fbd33 and is implementing an initial `web-admin/` service against Palpo's actual
admin/Matrix APIs using the existing proposed HAFleet onboarding requirements.
Initial scope is administrator authentication, fleet/App Service management and
managed-agent identity CRUD with honest readiness reporting. This is ongoing work;
it is not deployed and does not change the current Mini1 server or shared HAFleet tree.

## HAFleet side of Palpo admin integration — 2026-09-06

Implemented the bounded `task-palpo-fleet-protocol` contract for the coordinator's
isolated admin deployment. The existing AS listener now exposes four narrowly
scoped v1 operations under the registration's own hs_token. Real push-only custom
probe receipts establish reception delivery; custom request events retain exact
sender/event/source/target identity and require the fleet's registered project
marker, target membership/invitation authority, room-admin owner, and separate
encrypted owner/bot approval room. Private approval room IDs remain outside the
plaintext reception event and public status. Current target authorization is
rechecked before allocation. Provider approval stays manual; direct-room requests
retain their existing rules. Approved results are queued to reception while agent
admission and work bindings remain attached to the verified target.

Added a registration JSON file import to the existing credential form: it validates
the selected server and exact fleet namespace, populates masked fields, displays
scope, and requires explicit Save. Verified ownership proposals prefill the operator
approval form without submitting a verdict. The production console build succeeds
under `HAFLEET_CONSOLE_DIST_DIR=.next-admin-e2e`, preserving the existing `.next`
build. The same environment setting is required at start.

Validation: one combined run passed 212 tests in 10 files; a subsequent four-test
API run added the target-admission/reception-queue check and passed all four,
covering 213 distinct relevant tests. Existing side-provenance coverage is 99 tests,
not 100. A first regression run exposed unconditional adapter invocation in partial
bridge fixtures; the production path was narrowed to the two custom event types
and the full suite passed afterward. Two new fixture assumptions were corrected
(the established guard returns 403, and bridge state lives under data/matrix).
Syntax, ESLint, architecture boundaries and whitespace checks pass. Native
agent-spec 1.4 parses/lints the contract at quality 1.0 but remains nonpassing with
eight behavioral skips; Vitest evidence is separate. No native skip is counted as
passing. Test/build/lifecycle receipts are saved in the private live-run cache's
`fleet-protocol/` directory.

Deployment and live Playwright acceptance belong to the coordinating task and are
not claimed here. Browser onboarding/resource approval plus a non-escalating task
does not validate the still-native private runtime approval UI. No live process,
room, credential or existing manual runtime was changed by this implementation
subtask. Protocol details are in `docs/design/palpo-fleet-protocol-v1.md`.

## 2026-09-06 — Palpo admin deployment and live Playwright acceptance (in progress)

- Deployed the separate `palpo-admin-web` worktree's Node web administration app
  to Mini1's dedicated closure Palpo Docker network. A loopback SSH tunnel exposes
  the app locally on 18080; reverse callback 19094 reaches isolated HAFleet AS18195.
- Real Playwright administrator sign-in and provider/project/outsider authentication
  isolation pass. Cross-owner pairing is rejected, administrator operations return
  403 to normal users, and scoped reads do not expose other owners' records.
- First real dynamic App Service installation exposes a Palpo server defect:
  registration/read-back succeed but ordinary AS token authentication uses a
  startup-only file cache. The failed registration remains durable and retries
  reuse its identity. The admin UI now preserves login on upstream AS401.
- A dedicated Palpo Rust worktree and isolated Linux builder/test PostgreSQL are
  validating the server fix before rollout. No startup-registration workaround
  or manual database mutation is counted as successful dynamic onboarding.
- The isolated local HAFleet console created a Codex contribution preset and Mini1
  project-side record through Playwright. Full connection, request, fulfillment
  and actual task execution remain pending the real server fix. A further default
  agent-prefix/import mismatch is being fixed before testing admission.
- Private evidence and credentials are under the operator cache
  `palpo-admin-e2e/2026-09-06`; source deployment artifacts live in
  `/Users/yuechen/home/palpo-admin-web/web-admin/deploy/`. Existing manual HAFleet
  runtime18193 and native Robrix remain separate. This is not full acceptance.

## 2026-09-06 — Imported fleet naming blocker closed

Imported Palpo registration credentials now determine a side-specific
`hf_<id>_agent_` prefix. Backend identity minting, on-demand provisioning, target
admission, withdrawal and roster authorization use that same scope; bridge
senders, member recognition and modern/HTML/text mentions agree. Legacy
registrations retain the configured global prefix. Inconsistent imported
namespace/sender pairs fail closed. No runtime environment override is needed.

The bounded contract is `specs/task-managed-fleet-identity.spec.md`. Nine focused
files pass 198 tests, including real temporary-home on-demand provisioning under
global `ac_`, assigned MXID registration/invite/join, foreign-fleet rejection and
legacy side regressions. An additional same-homeserver sender assertion passes
in the 14-test bridge intake suite. Syntax, ESLint, architecture boundaries,
256 spec selectors and whitespace checks pass. Native agent-spec 1.4 remains
nonpassing: three behavioral skips and one boundary pass (quality 0.9583); it
cannot execute these Node/Vitest scenarios. This does not claim live acceptance.

Evidence is preserved in the private live UX closure cache under
`managed-fleet-identity/`. No live service, existing credential, old runtime or
console build was changed by this backend/bridge fix. The deployment coordinator
can remove its temporary prefix override before restarting the isolated runtime.

## 2026-09-06 — Canonical completion after the Palpo browser task

Independent read-only inspection of the isolated admin E2E runtime confirmed one
manual engagement approval receipt delivered in reception, replying to the exact
request event. The borrower task created exactly one canonical task and dispatch;
the completed dispatch reply used the originating task thread. No representative
receipt or agent echo created another task. The agent generated the requested
files, but its task remained in_progress after a successful model turn.

Root cause: runner context did not require the explicit canonical task transition,
and provisioned task-writer still targeted legacy agent metadata. The runner now
requires verified work and a confirmed scoped done transition before reporting
completion. Within a complete authenticated ephemeral context, task-writer uses
/api/router/session-task for its own task; incomplete/expired authority cannot
fall back to legacy writes. Heartbeat, wait and resume preserve unfinished states.
A successful model turn alone still leaves its task open. Ordinary home and graph
commands retain their legacy paths. The existing home wrapper references the
updated script directly and needs no reprovisioning.

Five deterministic files pass 93 tests, including real CLI child processes against
the scoped backend under hard agent-token mode, foreign-task rejection, expired
capability rejection, explicit completion, waiting/resume and legacy behavior.
Router build/reproducibility, syntax, ESLint, architecture, 259 spec selectors and
whitespace checks pass. The extended Palpo protocol contract has native quality
1.0 but remains NONPASS with ten behavioral skips and one boundary pass. Native
Node scenarios are unsupported; Vitest results are separate. Evidence is in the
private live UX closure cache under canonical-task-completion/.

The coordinator owns restarting only isolated backend18194 and a genuine browser
follow-up to validate completion. No existing live task state was manually changed.
A separate observed health projection still combines router idle state with stale
tmux-missing/offline flags; this audit does not describe those flags as healthy.

## 2026-09-06 — Mentionless project-thread followup admission

The coordinator's real Element followup was received by the bridge but ignored as
unaddressed: it never entered router_messages or task_inputs and queued no new
dispatch. The original completed dispatch and reply remained intact. The failure
was recipient resolution before backend ingestion, not a running or stuck model.

Project-thread followups now ask the bridge-authenticated approval-binding read
for the exact room/root/full original requester. Only one unfinished canonical
task with a current approval binding may identify the recipient. The bridge also
requires current requester, representative and assigned-agent membership; unknown
or ambiguous roots, another requester, substituted lookup scope and revoked
agent admission fail closed. Plain unaddressed room messages keep their previous
behavior. No last-active-agent fallback or replay of the ignored event was added.

Six files pass 149 deterministic tests, including a fresh bridge with no thread
memory, canonical task lookup, foreign scope rejection, same-name requester on
another server, ambiguous bindings, legacy direct intake and side provenance.
Syntax, ESLint, architecture, 261 spec selectors and whitespace checks pass. Native
agent-spec quality is 1.0, with eleven unsupported behavioral skips and one
boundary pass; the lifecycle remains NONPASS. Private evidence is in the closure
cache's mentionless-thread-followup/ directory.

The coordinator owns isolated backend18194 and bridge18195 restarts and a new
genuine Element followup event. The first ignored event remains failed evidence.
No live messages, runtime-state edits or restarts were performed by this subtask.


## 2026-09-06 — Mini1 Palpo admin deployment and live acceptance completed

Deployed the web administration worktree to Mini1. The live Palpo baseline keeps
its existing dependency/schema version with the tested dynamic App Service auth
backport; the rollback container and database backup remain available. The admin
release is `palpo-web-admin:7568134cb58d3062`.

Real Playwright coverage passes administrator/owner/project sign-in, hot App
Service installation, scoped pairing and HAFleet credential import, real Matrix
push receipt, reception recovery, a separate target project and encrypted owner
approval room, published coding role, a manually pending request, HAFleet resource
approval and actual admitted agent identity. The project member used Element's
real composer to request code; the agent wrote real files and returned results in
the original thread. Five tests pass independently from the edited workdir.

The extended thread test closed two further defects: scoped task-writer lifecycle
updates and exact persisted-thread recipient lookup for mentionless replies.
After actual private owner approval, the same canonical task reached done; all
three dispatches settled with zero queued work or active leases. Approval receipts
return to the original reception event, while task results and private approval
information stay in their proper rooms. No self-dispatch or replay of the ignored
first followup occurred. Request idempotency/conflict, foreign-project refusal,
admin/backend restart persistence and live offer withdrawal/publication refresh
also pass. A separate second fleet proves identity/token/owner isolation and
identity lifecycle; it is revoked and its test identity retired.

This is NOT full pure-Playwright or full Proposed-spec acceptance. Element and the
Palpo admin lack private permission-card buttons. Shell and scoped MCP completion
both triggered real owner approval; text replies were verified insufficient. A
separate native Robrix profile performed the exact Approve once, after which the
final result was verified in the browser. That native step is recorded separately;
the consumed card's static Pending header is also a remaining presentation issue.
Credential rotation, coordinated runtime stop/retirement acknowledgement, complete
owner self-service/identity membership management, metering/health projection and
other draft coverage gaps remain explicit in the final report. No Node agent-spec
behavioral skip is counted as passing.

Validation checkpoints: admin 30 tests plus Chromium/live browser coverage;
HAFleet scoped lifecycle 93 tests and thread routing 149 tests (overlapping sets,
not summed); live-baseline Palpo PostgreSQL regression 1/1 and Linux arm64 build.
All original failures, the ignored thread input, expired/denied permissions, and
one refused extra request caused by an early harness ID-reset mistake are retained.
The corrected same-ID test verifies the explicit idempotency conflict. The extra
refused submission has no agent binding or quota allocation and remains unusable.

Final report and screenshots: operator-restricted cache
`palpo-admin-e2e/2026-09-06/acceptance-report.md`. Mini1 and the isolated local
HAFleet/Element services remain running; temporary native approval processes and
the dedicated build VM were stopped, with profiles/artifacts retained. The original
manual HAFleet runtime and Robrix session were not modified. No commit/push/reset.


## 2026-09-07 — Resource allocation operator walkthrough

Checked current live resources, project-side allocation, active engagement and
canonical task/usage state. Mini1 Palpo/admin containers remain healthy; expired
local SSH forwards were restored with an owned background control socket in the
private run cache, and a real push verification renewed the existing reception.
The operator's complete App Service walkthrough and role-specific account reference
are in private `palpo-admin-e2e/2026-09-06/` as
`resource-allocation-walkthrough.zh.md` and `walkthrough-accounts.md` (0600).

Observed: 200k resource declaration, 200k side cap, 100k committed, one done task.
The configured ceiling is not enforced, actual token usage is still unattributed,
busy time is unobserved for the ephemeral runner and project rollups remain partial.
The side's project summary is empty despite the valid Active engagement binding.
These are documented limitations, not zero usage or absence of the real project.


## 2026-09-07 — Operator requested restarting project-side onboarding

Removed only the isolated HAFleet18194 project-side record
`hfux-closure-20260906.test` / `Mini1 Palpo admin E2E` through the supported
DELETE endpoint with explicit force, as requested by the operator. The endpoint
returned 200 with cascade=performed: ended engagement `en_mtqrj5du_2459b5`,
released its 100k commitment, deactivated its owner binding, withdrew the agent
and representative from the project room, and retired the test agent. Records,
completed task and test artifacts remain. Palpo, remote App Service registration,
project rooms, resource preset and the separate manual HAFleet18193 are retained.

Verified the side list is empty, discovery reports alreadyASide=false and the
server reachable, the engagement is ended, agent.retiredAt exists and it is not
active, and actual Matrix membership is leave for both withdrawn identities.
Playwright reached the empty name field in step 2 without creating a new record.
Evidence: private cache `palpo-admin-e2e/2026-09-06/reset-project-side-*`.
The generic health observer overwrites offlineReason with tmux-missing:auto;
retiredAt remains the retirement truth and admission rejects retired agents.
No code change was made for that existing projection issue.


## 2026-09-07 — Remove preconfigured onboarding candidate

The operator still saw Mini1 after deleting the side because matrix/reach also
lists MATRIX_SERVER_NAME / MATRIX_HOMESERVER supplied by the test launcher.
Backed up private cache rig.py and removed homeserver defaults from its backend
process only, retaining the bridge's live connection configuration. All three
router dispatches were completed and no side existed before the owned backend
restart (new PID 67354, port18194). No repository code was changed.

Playwright verified zero candidates and empty manual fields, successfully probed
Mini1 after typing the server name/address, reached the empty step-2 name field,
and reloaded to an empty candidate list. It did not create a side. Matrix18010
and admin18080 both still answer HTTP200. Evidence is in private cache
palpo-admin-e2e/2026-09-06/reset-candidate-*.


## 2026-09-07 — Palpo import joined to the onboarding wizard

Implemented the operator-requested missing step directly in projects/new:
Appservice defaults to importing Palpo's existing authorization, previews its
actual representative/callback, and saves then verifies in the wizard. Retained
manual generation and registration tokens. Invalid/stale imports cannot write;
save and verification failures have distinct recovery paths; success does not
claim inbound reception readiness. Task contract:
specs/task-palpo-wizard-import.spec.md.

14 focused Vitest tests and four controlled Playwright scenarios pass; production
build and264 selector bindings pass. Agent-spec1.4 remains nonpassing with3
behavioral skips; its boundary passes. Deployed final .next-palpo-wizard-v2 to
console13202 (owned PID35312), stopped temporary13203. Actual Palpo owner download
and preview pass; the operator's 测试房间 1 remains without credentials so they can
click 保存并验证 themselves. Full scope/evidence:
reviews/2026-09-07-palpo-wizard-import.md. No commit/push.


## 2026-09-07 — Project-side visibility after App Service onboarding

The operator reported an empty Projects page after successful Palpo import.
Root cause: Projects consumed invitations and contribution bindings but omitted
the existing projectSides projection. Added a separate registered-side section
with server, representative, credential type/status and a connection/allocation
link. Missing credentials, failed verification, inactive state, loading and read
failure retain distinct meanings. Agent access lists remain unchanged.

15 focused Vitest tests pass and four controlled Playwright scenarios pass,
including English/Chinese visibility without agent grants, missing/inactive
credentials, empty registrations and failed reads. Production build, translation
parity, whitespace and265 selector bindings pass. Task:
specs/task-project-side-visibility.spec.md; agent-spec quality100%, boundary pass,
one unsupported behavioral skip (native lifecycle remains nonpassing).

Deployed .next-palpo-wizard-v3 to console13202 (PID13900) and stopped temporary13203.
Live Playwright confirms 测试房间 1 / 凭据已验证 with the actual representative;
the complete side object stayed unchanged (no allocation or grant was created).
Private evidence: projects-visibility-live.json, projects-side-after.png,
projects-visibility-browser.log and projects-visibility-lifecycle.json under the
palpo-admin-e2e/2026-09-06 cache. No commit/push.

During the fix, the operator asked how to log into Robrix2. Supplied the actual
provider Matrix ID/password and explicit local homeserver18010. The !Z3r... value
is the reception room ID, not a login ID. Provider membership was read and is
joined; no Matrix invitations, membership changes or native account switches
were performed. Robrix's password form supports separate user ID/password/server
inputs; no actual operator login error was supplied or reproduced.


### Project-side layout correction in final build v4

Visual inspection (and the operator's immediate report) caught a layout defect
that text-only browser assertions missed in v3: `.steps li` defines20px +1fr
columns, and its sole child occupied the20px marker column. Corrected that child
to span both columns and added an actual rendered-width regression assertion.
Four controlled browser checks pass again. Live Chinese screenshot on13202 now
measures1124px content width equal to the row, height151px rather than a vertical
column. Final deployed console is .next-palpo-wizard-v4, PID26554. Temporary13203
was stopped. The incorrect v3 screenshot is retained as
projects-side-after-v3-layout-failure.png; corrected evidence is
projects-side-after.png and projects-visibility-layout-v4.json. No backend state
or Matrix membership changed.


## 2026-09-07 — Palpo request form explains expired connection and delivery

The operator reported Send agent request produced no visible HAFleet engagement.
The new project owner channel was ready, but Palpo connection evidence had
expired; both inspected request and matching engagement lists were empty. The
form checked roles but ignored readiness and showed errors only at page top.
Fixed the Palpo web-admin worktree with inline readiness/recovery, owner-scoped
reverification, expiry gating, preserved request fields/ID and inline delivery
receipts. No allocation or replacement request was inferred or submitted.

30 Node tests, the existing browser workflow and six new Playwright recovery
scenarios pass. Deployed to the dedicated Mini1 web-admin container; live real
event verification succeeded at22:56:35Z after one honest probe_pending retry.
The existing reception/project remain and Send is enabled. Full findings and
evidence limits: reviews/2026-09-07-palpo-request-readiness.md. No commit/push.


## 2026-09-07 — Operator completed the octos-code-use single-agent task

The operator submitted Palpo request1ce1c56f-9d12-488f-b523-714d512e5540
for1,000,000tokens and200,000/day, then approved engagement
en_mtruf5yz_6c4bc1 in HAFleet. Fulfillment completed with the actual joined
agent mx_hfux_closure_20260906_te_coding_126926a91ba1. The operator sent the
sum(a,b) task through a real Robrix mention and manually approved its heartbeat
and done commands in the private encrypted owner room. Both verdicts were
allowed and consumed; no assistant verdict was submitted.

Independent verification from the edited agent project path reran
node --test sum.test.js:3passed,0failed,0skipped. HAFleet task API confirms
task_5cb9646b-ddc5-412c-8a7b-a1fdf92517f9 is done (23:10:15.118Z). The final
Matrix reply names the real files and belongs to the original project thread.
Evidence: private octos-sum-task-completion.json. This validates this manually
guided single-agent request/approval/task/result flow, not delegation, accurate
token metering or the complete proposed specification. Routine task-writer
heartbeat/done still trigger runtime permission prompts; this UX gap remains.


## 2026-09-07 — Recover the unanswered completed-thread Python follow-up

The real explicitly mentioned second message was received and queued, but its
canonical task was done, so the scheduler skipped it forever without a notice.
ADR-020 and the completed-thread-followup task contract implement a fresh-input
continuation of the unique task: exact original human sender, Matrix thread,
session, active binding, unprocessed supplementary input and never-started batch
are checked before atomically reopening and claiming. Prior dispatches and outputs
remain, with the prior completion and source input recorded in a router event.
Blocked, unknown, quarantined and running-work gates remain enforced.

114 related tests pass (six files), including an actual backend bridge-intake
regression. Router build/comparison, module boundary and270 spec selectors pass.
Native agent-spec retains5 unsupported behavioral skips and is not passing.
Deployed by gracefully restarting only the idle owned backend18194, PID67354
→52844. Its original queued dispatch20de23cb-830b-41f4-9d5b-ef986d5c062a started
at23:30:38Z using the same msg_0005 and session. No message was resent.

Python files were generated and3unittest checks independently pass. The model
then requested the owner's done approval; it remains a real manual gate, not
a fabricated test verdict. Current evidence and limits are recorded in
reviews/2026-09-07-completed-thread-followup.md and the private completed-followup
artifacts. No commit or push.

## 2026-09-07 — Repair routine task-maintenance approval

Added ADR-021 and a bounded contract, then reproduced five failing scenarios.
Fixed the ephemeral MCP heartbeat route, scoped execution field projection,
Codex lifecycle developer instructions and exact per-tool launch authorization.
The first real Codex probe found a further get_task confirmation gate; its
failure is retained. The second real probe created and tested code, then reported
heartbeat and confirmed canonical done with zero approvals. Independent tests
pass3/3. All92 related offline regressions pass; native agent-spec retains5
unsupported behavioral skips and is not passing.

After fresh idle checks and a consistent private SQLite backup, restarted only
the owned backend18194 from PID52844 to53826. Post-deployment API preserves all
five completed dispatches and the active Agent. The user's Python follow-up was
already done at23:38:00.660Z; no message replay or assistant owner verdict was
needed. Explained that Create Agent provisions a local worker, whereas the
Palpo application requests provider capacity and already provisioned this
project's Agent. Details: reviews/2026-09-07-task-maintenance-approval.md.
No commit or push.

## 2026-09-07 — Remove the independent provider Agent creation workflow

Operator confirmed Resource → borrower request → provider approval → automatic
Agent provisioning → management. Removed creation links, replaced /onboard with
a redirect, placed Resource configurations first and corrected both locales'
empty states and navigation counts. Backend provisioning and existing Agent
management remain available.31 Vitest tests and8 controlled Playwright scenarios
pass; the production build and278 spec bindings pass. Native agent-spec retains
3 unsupported behavioral skips, separately recorded as non-passing.

Deployed only console13202 as `.next-resource-first-v5`, PID90451. Real browser
checks confirm redirection and the current Agent detail, with zero writes and
the same two Agent identities/preset bindings. Evidence and limits are recorded
in reviews/2026-09-07-resource-first-console.md. No commit or push.

## 2026-09-07 — Project discussion context and mentionless private chat

Implemented ADR-023 and its bounded task contract: durable room history,
per-room/agent successful positions, frozen dispatch ranges and fenced paginated
MCP reads. Project discussion and public agent replies are context; explicit
mentions are required to start work, including in project task threads.
Native two-person DM admission retains existing project allocation and approval
authority. Dedicated App Service agent device sessions support encrypted intake
and replies, persistent history retry and conversation continuity after done or
restart. No global tool or network permission was introduced.

746 tests in40 files pass, as do router build/output comparison, scoped ESLint,
architecture ownership, remote MCP synchronization and286 specification bindings.
The native agent-spec lifecycle retains8 unsupported behavioral skips and is
recorded as non-passing, with separate Vitest evidence. Actual Playwright sends
against Mini1 Palpo verify multiple participants' discussion, successive mention
summaries, recovery of the user's original Robrix2 DM, and encrypted private
conversation across three turns and a service restart. The browser visibly
decrypts replies that are encrypted on the wire. Private content is absent from
the project archive; no new operation approvals were created. Removed the
temporary test member and confirmed the original project membership.

After idle checks and consistent private database backups, gracefully deployed
backend18194 PID75564 and bridge18195 PID75727. Console13202 remains v5. Evidence,
scope limits and the user guide are in reviews/2026-09-07-matrix-conversations.md
and guides/matrix-conversations.zh.md. No root task-writer exists in this source
checkout; no canonical task state was fabricated. No commit or push.

## 2026-09-08 — Diagnose second Agent request workflow

Confirmed the operator's new medium-reasoning resource exists and qualifies for
coding. Inspected the actual Palpo request form with Playwright: `octos-code-use`
is its target project, while `coding` is the only published role. Read the request
matching and fulfillment paths: they reuse an existing qualifying Agent, and
the operator approval form/API has no explicit resource or new-Agent selection.
This is an unresolved product gap despite support for multiple Agents in a
project room. Recorded the limitation without creating a manual Agent workaround
or submitting the operator's next request. Palpo connection verification is
expired and must be renewed before a future submission. No implementation or
service state changed; private read-only browser evidence was saved.

## 2026-09-08 — Resource-owned Agent definitions and explicit approval choice

Implemented the operator's revised flow under ADR-024: multiple Resources, each
with multiple named Agent definitions; explicit resource catalog publication;
and a validated definition/existing-Agent choice at provider approval. Defining
an Agent creates no runtime home or Matrix identity. Approval provisions the
selected definition with its resource profile and retains that identity across
interrupted fulfillment and retry. Existing project-side budgets still apply.
Palpo web-admin now displays the published resources and Agent definitions while
requesting the role; exact allocation remains the provider's approval decision.

105 related Vitest tests pass in 10 files; the updated proxy suite's 11 tests
also pass after adding exact route/security coverage. Two new bilingual and
eight existing controlled browser flows pass. Palpo's 31 Node tests and both
browser suites pass. Production build, scoped lint, architecture ownership and
290 selector bindings pass. Native agent-spec reports four unsupported behavior
skips and remains non-passing; separate Vitest evidence is retained.

Deployed backend18194 PID89234, console13202 PID31288 with isolated build
`.next-resource-agents-v8`, and Mini1 Palpo web-admin image
`palpo-web-admin:318f47082b8092da`. Bridge18195 and Matrix device state were
preserved. Actual Playwright creation of two temporary definitions through the
HAFleet console and publication to Mini1 Palpo passed. Removed only the test
resource/definitions and verified all three original Resources, Agent identities
and engagement allocations unchanged. No second real request or approval was
submitted and no budget increased. The project's 1M allocation is fully committed
to the first Agent, which is a prerequisite for the operator's next request.

Guide: guides/resource-agents.zh.md. Evidence and limits:
reviews/2026-09-08-resource-agent-definitions.md. No root task-writer exists in
this source checkout; no canonical task state was fabricated. No commit or push.

## 2026-09-08 — Correct definition ownership to the Palpo project side

The operator clarified that all project Agent definitions are made on Palpo.
Implemented ADR-025: removed HAFleet's definition form/proxy writes, retained
Resource configuration/publication, added Palpo Agent name and resource selection,
and bound that definition through Matrix source verification and request replay.
HAFleet approves the exact requested definition and provisions a distinct Agent;
it does not require a local definition or silently reuse the first Agent.

115 related Vitest tests in13 files pass, as do two corrected bilingual HAFleet
browser flows, eight existing Resource browser flows,33 Palpo Node tests and both
Palpo browser suites. Build, scoped lint, architecture and294 selectors pass.
Native lifecycle retains four unsupported behavioral skips, not a passing result.
Tests retain real backend provisioning with localhost Matrix and controlled launch
evidence; live UI inspection does not submit or approve an extra real request.

Deployed local backend18194 PID20010, bridge18195 PID20071 and console13202 PID20150
with `.next-resource-agents-v9`, plus the revised Mini1 Palpo web-admin. Before and
after restart, both existing Agents, three Resources, allocations and18 completed
dispatches are unchanged. The private run cache retains backups, exact image
receipt and verification evidence. Revised guide: guides/resource-agents.zh.md;
review: reviews/2026-09-08-palpo-agent-definitions.md. No commit or push.

## 2026-09-08 — Expose the actual resource pool and automate publication

Published the operator's three actual Resources and five currently supported
roles. Reworked Palpo's Agent request flow to display a deduplicated resource pool
first; choosing a resource then limits Role to what it can provide. The medium
Resource supports coding/testing/integration/documentation; high also supports
architect. Review still requires the existing cross-family qualification.

The operator then required automatic publication. New resource creation now saves
its default publication choice atomically. The Palpo callback derives roles from
qualifying published Resources without requiring manual role offers. Explicit
resource/role withdrawal remains effective, and this projection does not enable
automatic acceptance. Palpo polls the catalog every10 seconds while visible and
on return, preserves request drafts and refuses submission on failed reads.

110 backend regression tests passed in9 files;33 Palpo Node tests, both Palpo
browser suites, two bilingual HAFleet flows, production build, scoped lint,
architecture boundaries and296 selector bindings pass. Native agent-spec retains
six unsupported behavioral skips and is non-passing; separate Vitest evidence is
recorded. The exact bound source-authentication selector is checked separately.

Deployed backend18194 PID40252, console13202 PID40253 with
`.next-resource-pool-v10`, and Mini1 web-admin image
`palpo-web-admin:f67999ec23458a6a`. Preserved bridge18195 PID20071. Actual Playwright
creation via HAFleet's web wizard appeared in the already-open Palpo after9311ms
without manual publication or refresh. Deletion also propagated automatically and
retained draft inputs. Cleaned only the temporary Resource; verified all three
real Resources, both existing Agents, allocations and18 completed dispatches are
unchanged. No actual request, approval or budget increase. No commit or push.

## 2026-09-08 — Restore Send agent request readiness

Read the actual Palpo session, catalog and project state after the operator
reported a disabled Send button. The project and all three Resources were
available; connection verification had expired. The first real reconnect returned
probe_pending before Matrix asynchronously delivered its event. The original
browser verification script timed out waiting for success and is recorded as
failed; retrying the same probe succeeded without replacing rooms or credentials.
The Send button is now enabled after choosing the medium Resource.

Fixed the companion Palpo backend to wait for an authentic probe receipt, retrying
only probe_pending at500ms intervals (20 attempts maximum) with the same event and
challenge. Other errors fail immediately. A pause/revocation while verification
is in flight remains authoritative.35 Node tests and both browser suites pass;
syntax and diff checks pass. Deployed Mini1 image
`palpo-web-admin:b26d42db1b711ca9`. A single real Playwright click now verifies
actual Matrix delivery successfully in1207ms, preserves the request draft and
enables Send. The existing one active Palpo request is unchanged.

The project's1M-token allocation remains fully committed to the first Agent.
Requested the operator's intended second-Agent token amount before changing that
budget. No new Agent request or approval was submitted during diagnosis.

## 2026-09-08 — Separate Palpo definition intake from resource approval

The operator's fresh-resource request exposed the misplaced side-budget check:
Palpo definitions always need manual review, yet intake applied the gate for
possible automatic admission. Updated ADR-025 and the active contract, reproduced
the exact refusal in a regression, then exempted only authenticated fleet intake
from this pre-recording budget check. Approval/reservation still checks project,
resource and seat budgets. Existing automatic admission remains gated.

61 tests pass in5 relevant suites, covering fresh-resource pending definitions,
no headroom or assigned budget, replay without allocation, refusal at approval,
and later correctly funded approval. Syntax, scoped lint, architecture and297
selector bindings pass. Native lifecycle has seven unsupported behavioral skips,
zero failures, and is recorded as non-passing.

Deployed backend18194 PID16597 with private runtime backups. Console13202 PID40253,
bridge18195 PID20071 and Mini1 Palpo web-admin b26d42db1b711ca9 were preserved.
Retried the original edison request using Playwright. Palpo acknowledged201;
the initial verification script incorrectly asserted200 and failed after the
successful submission. Corrected the assertion and completed read-only verification
without resubmitting. The original event/ID/definition is preserved, and
HAFleet now has one pending integration request for edison,100000 tokens on the
medium Resource. Actual HAFleet web review opens the correct definition.

No approval, runtime creation or budget increase occurred. Both existing
identities, all three Resources, allocations and18 completed dispatches are
unchanged. The side's1M allocation remains fully committed, to be addressed before
approval. No commit or push. Evidence: palpo-pending-live-result.json,
palpo-pending-deployment.json and the Palpo/HAFleet screenshots in the private cache.

## 2026-09-08 — Fund the operator's concrete edison approval attempt

The operator supplied the actual approval refusal for100000 tokens while pointing
out the new medium Resource's100M ceiling. Confirmed that candidate has99M capacity;
the separate project-side total was1M, committed1M, remaining0. Raised only that
side's allocation to1100000 through its operator API to cover this approval amount.
Readback confirms committed1M, available100000 and edison still pending without any
reserved tokens or Agent. No verdict, model run, code change or service restart.
Existing first-Agent allocation is unchanged. This supersedes the earlier zero
headroom state and leaves approval to the operator.

## 2026-09-08 — Correct Edison's selected-pool accounting

The operator rejected the side-cap workaround. Updated ADR-025 and its Task
Contract, reproduced independent pool capacity failures, and corrected approval
to draw project-defined Agents from their selected Resource. Pool and shared-seat
commitments are separate; declared account quotas remain enforced. Legacy requests
retain side caps, with separate reporting for pool-funded commitments. Approval
shows both limits. Pending, active and reserved capacity share the store predicate;
concurrent approvals and retry cannot duplicate allocations.

275 distinct Vitest checks pass, plus both language Playwright fixtures. Production
console build, syntax, lint, architecture and299 bindings pass. Native lifecycle
reports9 unsupported behavioral skips,0 failures, non-passing. Details and the
initial red regression are recorded in reviews/2026-09-08-palpo-pool-accounting.md.

Deployed backend4587 and console4681 (.next-pool-budget-v11) to the existing local
rig. Bridge20071 and Mini1 Palpo are preserved. Real browser review confirms
edison medium100M/0/100M with100k requested, pending and not provisioned. No live
approval, budget change or model invocation. Two identities, three Resources,
18 completed dispatches and the prior1.1M side cap are unchanged. The side cap
is retained for legacy requests and no longer gates edison. Private deployment
backups and edison-selected-pool-fixed.json/png contain the readback evidence.

## 2026-09-08 — Verify rooms after operator approval

Edison is now active with completed fulfillment. Verified real Matrix membership:
it joined octos-code-use, while Reception received the representative’s delivered
approval receipt and has no Edison membership. Both the original Agent and Edison
are in the project room. Read-only check; no message, invite or approval submitted.
Evidence: edison-room-membership.json in the private Palpo admin E2E cache.

## 2026-09-08 — Invite existing Agents and render Matrix Markdown

Implemented the operator's correction in ADR-023 and its Task Contract. Ordinary
invitations use per-room per-Agent bindings to existing allocations; DM promotion
requires mentions, isolates prior private context and retains group thread
relations. Multiple addressed Agents consume one authenticated event through
separate tasks and send using their own identities. Revoked/departed bindings
cannot block another valid participant. Markdown is converted to safe Matrix
formatted HTML at the HAFleet send boundary, retaining plaintext and encryption.

179 regressions across13 suites pass, with router build/generated output, lint,
architecture and302 selector bindings. Native lifecycle reports0 failed and11
unsupported behavioral skips, non-passing. Deployed backend65390 and bridge75228
after preserving the complete runtime; console4681 and Mini1 Palpo are unchanged.

Mini1 live testing created two explicitly named test rooms, admitted Edison and
coding, verified implicit DM replies, ordinary invitations, DM-to-group promotion,
no unmentioned dispatch, individual and simultaneous mentions, background-context
summarization, identity and thread relations. Five new model dispatches completed;
all25 dispatches are complete. Playwright opened actual Element Web DM/thread
messages and asserted headings, bold, lists, links and code for both Agents.
Existing identities, Resource definitions and allocated budgets are unchanged.
No native Robrix rerun or live encrypted-room rerun is claimed. Full evidence and
limits: reviews/2026-09-08-invited-agent-rooms-markdown.md. No commit or push.

## 2026-09-08 — Restore and supervise Mini1 connectivity

Reproduced both operator-reported Robrix history requests as connection refused:
local18010/18080 and mini1-tunnel.sock were absent. Mini1 SSH and Palpo containers
were healthy. Restored all existing forward directions through a per-user launchd
service, with KeepAlive,15s SSH liveness checks and explicit foreground/no-persist
options overriding the user's ControlPersist600 setting. No Palpo, HAFleet or
Robrix code change or room-data modification was needed.

The exact thread URL returns4 relations. The exact ordinary-history cursor returns
an empty successful page because it is already at the start; fetching its current
history returns13 events including4 messages. A controlled tunnel SIGTERM caused
automatic recovery in0.48s; launchd owns the replacement PID99248. Both original
URLs pass again, and Playwright opens the real room and its thread successfully.
Reverse callback TCP connectivity also returns the bridge's authentication403
(reachability evidence, not an unauthenticated health-check success).

Service: /Users/yuechen/Library/LaunchAgents/com.hafleet.mini1-tunnel.plist.
Private cache evidence: mini1-history-connectivity-recovery.json,
mini1-tunnel-restart-test.json and mini1-recovered-history-browser.png.

## 2026-09-08 — Open Mini1 Palpo on the operator's public domain

The operator requested public access and specified crew.ominix.io on a separate
port. Confirmed DNS points to the configured Mini1 host, its certificate is valid, and18443
is occupied by an unrelated service. Added crew.ominix.io:19443 to the existing
/etc/caddy/Caddyfile, retaining all prior routes and gracefully reloading the
existing io.ominix.caddy process. Backed up the original configuration first.
Only Matrix client/media and client discovery endpoints are exposed; original
443website, Palpo containers, account IDs, rooms and local integrations remain.

Public HTTPS from this computer and Chrome validates TLS1.3 and the correct domain
certificate. A temporary real provider login succeeded; whoami,9joined rooms,
sync,13history events and4thread relations all returned200. The temporary session
was logged out. No native Robrix restart or credential/profile edit was performed.
Robrix can now use https://crew.ominix.io:19443 directly; the old local forward
remains for existing clients and HAFleet's callback. See the public connection
guide and private mini1-public-matrix deployment/verification evidence.

## 2026-09-08 — Agent names and visible runner activity

Accepted ADR-026 and task-visible-runner-activity. Repaired Edison's generated
Matrix display name without changing its MXID. Added native Codex/Claude tool
activity, fenced durable status projection, coalesced Matrix edits, periodic
liveness, approval/terminal state and context exclusion. Custom names and existing
sandbox/approval policies are preserved. DM edits retain encryption and reject
private-thread edits after room promotion.

161 distinct tests across 14 suites pass. Native agent-spec lifecycle has one
boundary pass and five unsupported skips, explicitly non-passing. Full runtime
backup preceded a graceful restart after the user's current execution finished:
backend95185, bridge95205. Live Mini1 DM plus a two-Agent thread completed three
real Codex dispatches with one editable status each and 19 acknowledged updates.
See docs/reviews/2026-09-08-runner-activity.md and the private rig evidence.

## 2026-09-08 — Bidirectional Agent files

Implemented and deployed ADR-027/session-file-delivery: managed MCP files in the
current room/thread/DM, immutable durable output snapshots, prepared media retry,
current sender admission, encrypted media/message delivery, bounded member upload
staging, readable conversation attachment metadata and scoped receive_file.
Group mention gating and DM no-mention behavior are preserved; file failures are
explicit, and filename text is not interpreted as a bot command.

141 focused tests passed across 11 suites, with subsequent tampered ciphertext
and undeclared oversized stream checks passing. Syntax, lint, route/architecture,
router build consistency, remote sync and spec bindings passed. Native lifecycle:
one boundary pass, six unsupported skips (non-passing). Local backend99858 and
bridge99880 deployed after idle check and full runtime backup. Real group, plain
DM and encrypted DM CSV→Agent→TXT workflows passed server/public download hashes
and Playwright actual attachment downloads. Corrected the isolated Element test
server CSP for its own sandboxed download helper. Palpo and Robrix source and
containers were unchanged. See docs/reviews/2026-09-08-session-files.md.

## 2026-09-08 afternoon — Robrix permanent attachment spinner

Reproduced the native client's picker panic and independent empty-filename
directory write failure from the user's actual desktop log. Fixed Robrix's Save
picker threading, unified links/buttons through authenticated SDK media with full
encryption metadata, added a bounded network timeout, and removed the obsolete
encrypted-download unsupported display. Code changes are in the actual Robrix2
source repo; unrelated existing work was preserved.

Five focused native tests and native build/check passed. Final native agent-spec
lifecycle is 3 pass, 0 fail/skip/uncertain; original transient HTTP fixture failure
and diagnostic reruns remain recorded. Actual native UI cancellation/retry and
group/plain-DM/encrypted-DM downloads passed, with four saved files hashing to the
expected total=8 newline payload. Updated/restarted Robrix2 Mini1 after preserving
the original executable/profile; final PID22930. No Palpo/HAFleet restart or code
change for this fix. Native screenshots, files and checks are in the private rig's
robrix-files-native-0908 evidence directory. No task-writer wrapper is provisioned
at this source repository root; no canonical task completion was fabricated.

## 2026-09-08 — YOLO and persistent scoped approvals

Implemented resource-default and per-Agent execution settings, exact native
task/always grants, atomic rule persistence, contributor revocation, and native
Robrix scoped cards. Added durable task completion epochs to prevent grant
revival on thread follow-ups. Default and existing Agent policies remain sandboxed.
154 focused HAFleet tests, 35 native approval tests, bilingual Playwright flows,
builds and required code checks passed. HAFleet native lifecycle retains five
Vitest-related skips; Robrix's two scoped scenarios pass. Live Mini1 encrypted
card click, fresh-runner rule reuse, webpage revocation/reapproval and isolated
real Codex YOLO verified. Initial shell-mode mismatch and blocked-task probe
timeouts preserved in evidence. Deployed locally after idle check and backup;
no Palpo changes, commits or PRs. Review: docs/reviews/2026-09-08-execution-authorization.md.

## 2026-09-06 macOS E2E

Operator requested Computer Use E2E with local Docker Palpo only, formal GUI @ member selection, and mempal disabled only for this E2E agent. Source baselines: HAFleet 0a0ae88, Palpo 8433b4a1, Robrix2 e28e118e. Palpo and PostgreSQL run in isolated Compose project hafleet-e2e at 127.0.0.1:8008; existing 8128 deployment is untouched. Palpo Docker source build and Robrix release build passed. Runtime: `~/.hafleet/e2e`; full evidence and per-layer RESULT.md: `~/.octos/outer/verify/e2e-{1,2,3}`.

GUI request/verdict/@ picker/real nonce reply passed. API isolation and recovery cases have explicit verified/partial ratings. F07 agent-leave test produced the expected warning in project 2; agent membership was restored in Matrix and HAFleet, confirmed in the GUI picker, and a test-end note posted.

Real thread runner launched Herdr session hafleet-agents-e2e, pane w1:p1, octoscode inner. Hello CLI implementation commit 0ad443b has four passing tests. Initial autonomous monitoring failed to recognize in-place ACK and the actual completion label, requiring intervention. Subsequent fresh-nonce E2EAUTOWATCH20260906A recheck completed autonomously through agent-authored monitoring, independent testing and a reply to the same Matrix thread; Codex only observed. mempal Stop hook and MCP are excluded by E2E-only wrappers for headless and ordinary tmux sessions; global settings hash is unchanged.

Source fixes: common startup now drains router outboxes without bot login; limited sync recovery now persists cursor bounds, pages the correct interval, validates complete responses, preserves failed/legacy recovery, and routes only messages through the authenticated router. Original 45-message burst delivered only 22; corrected live retest delivered all 45, including recovery after a real history-read rate limit. Red/green tests and independent review are recorded. Final regression/CI and commit evidence are linked in E2E-3 RESULT.md and `.octos/OUTER_LOOP_REVIEW.md`.

Remaining product gaps are not passing: task stays in_progress because legacy task lifecycle MCP is unavailable to the session runner; botless SSE membership path has misleading success logging after bot-client failure; task-title mention truncation. No state was manually changed to make lifecycle appear complete, and remote/ was not edited.

This checkout has no projects/, task-writer, or provisioned control-plane task object. No canonical task state was fabricated. These docs are coordination notes only.

## 2026-09-06 three-layer repair and autonomous retest

Operator authorized repairs and another autonomous test, preserving local Docker Palpo and E2E-only mempal isolation. Source work continues from 3a0ae55 on fix/botless-thread-outbox. Added scoped runner task operations with transactional receipts and authentication regression coverage; repaired botless same-side membership and promoted title extraction; added the reusable hafleet-inner-loop skill, bounded monitor and complete directory installation. Independent code review and schema 8-to-9 preservation checks passed.

Local API-driven run E2EREPAIR20260906A completed autonomously: Claude decomposed acceptance, started a real octoscode in named Herdr session hafleet-repair-e2e, resolved its own instance-lock startup failure, prepared a fresh monitored job, independently verified commit 58ef12b (9 tests plus CLI/fmt/clippy checks), commented evidence and transitioned task task_c52a2495-bec0-4410-a433-6088512c39ae to done. Dispatch a4d3fc89-5650-44d3-9c3c-d93288c85e79 separately completed. Agent reply $pZflpDMDS--xp6CUxKfpUsodg-foTo1sEGXSYPHf0xc reached original Matrix thread $MoYTvt4cPWI031B4Iz1xRcwB4DuAR5B6aARTctIcqs4. The driver only observed after sending the request. Old Herdr sessions and historical task states were preserved.

Project 2 HAFleet remove/add caused actual Matrix leave/join without manual invitation repair. New Computer Use GUI retest remains unverified: Robrix/Finder return -10005 cgWindowNotFound, while app discovery reports them running; operator desktop-readiness input is pending. Codex middle-agent coverage and final CI are being completed separately. Two intermittent CI failures (retention socket hangup and server-list HTTP 401) are retained in evidence; exact reruns passed and the latter now preserves response diagnostics without changing auth or adding retries. No root-cause fix is claimed for these intermittent failures.

Evidence: `~/.octos/outer/verify/e2e-repair-20260906/`. Agent-spec checks boundaries but skips Node scenarios; exact Vitest selectors are the executable verification, and skipped lifecycle scenarios are not passing.

Deeper rechecks supersede the initial project-2 snapshot rating: e2e-claude joined at 03:38:02.642Z but was kicked again at 03:38:05.217Z by a delayed Matrix-membership -> backend-roster -> SSE -> Matrix-operation echo. The later e2e-codex addition was unrelated. The original immediate join evidence remains intact; membership convergence is failed pending the source repair and sustained recheck. Codex's first actual dispatch also stalled on an unhandled native MCP elicitation request; an independent real app-server probe reproduced it. The stalled test was cancelled through the router API after confirming no inner process or repository changes, retaining outcome_unknown for inspected recovery. The existing user Herdr sessions were preserved.

Final source review found no remaining blocker. `npm run verify:ci` passed all 502 tests across 45 files; the separate integrated repair suite passed 336 tests. Build freshness, syntax, architecture and MCP mirror checks passed. The agent-spec boundaries passed while Node scenarios remain skipped and separately covered by Vitest.

The project-2 echo repair now preserves bridge-authenticated Matrix observation provenance and rechecks current membership before applying incoming member events. After restarting only the isolated backend/bridge, a fresh remove/add test held both Matrix and backend membership through 31 observations over 60 seconds; the member-event history showed no later kick (evidence 32 and 36). The initial failed convergence record is retained.

The Codex native MCP adapter now handles the observed elicitation protocol with exact active-item correlation, existing narrow coordination exceptions, owner approval for other supported calls and explicit failure for unknown/stale input. Twelve native adapter regressions and a real-model protocol probe passed. The original failed dispatch was formally inspected and continued as de95c608-4d2b-42b0-92d4-6f63d86d6b42. The retry's actual task read/comment/heartbeat calls succeeded; native command approval cards and one-time decisions traverse local Matrix. The test driver reviews concrete E2E commands under the operator's existing authorization, so this run is reported separately from Claude's observation-only execution. Owner-room representative membership and the Codex owner binding were explicitly prepared for this test, not credited as automatic provisioning.

Codex R1 subsequently reached the configured 20-minute wall-clock runner limit during native approvals and startup preparation. The task correctly became blocked/outcome_unknown. Inspection found a clean baseline repo, a ready dedicated lower with zero loops and an undispatched prepared nonce job; no implementation prompt, result or live monitor. The driver restarted only the idle E2E backend with HAFLEET_RUNNER_LEASE_MS=3600000, leaving source defaults, bridge, original Herdr sessions and native permissions intact, then formally continued the same task as dispatch 0474814b-63ba-4bbf-98ff-c47ae657d8a6 (nonce label E2EREPAIR20260906CODEX-R2). Evidence 54–56 preserves the failed R1 and recovery. This is operator recovery, not an autonomous-success claim for R1.

Final Codex R2 acceptance is verified (local Matrix API driven). Task task_9bf127e1-34fd-401a-8c15-4edf81394bd2 became done through the agent's scoped transition at 05:12:55.905Z; dispatch 0474814b-63ba-4bbf-98ff-c47ae657d8a6 separately completed. Exactly one matching final reply, $SLHElY98maZy9blIua7scY9gA9GMMmWm_MZDou2j3ts, reached original thread $8ArcXmH5MYMBG-TAhuiBdl_2ZNb3CFCScZNiNVkntRs. Real lower commit b1309f61acd23bd2595f5b3b6610f56a78e95057 passed 18 unit + 23 CLI integration tests and 26 additional middle-verifier CLI cases, plus fmt/clippy/build and scope/integrity checks. The middle naturally completed its owned monitor (fresh job a8b3f80a-61e7-4814-a7d8-c78d1fd37f5f), independently noticed and followed up the missing lower result, and corrected the lower's misreported test count using actual output. Driver did not implement, publish inner results, drive the monitor, or force business completion; it did perform documented formal recovery and native approvals. A separate clean verification clone also passed. Evidence 62, 68–74 contains artifacts, integrity hashes, lifecycle, delivery and runtime facts.

Final runtime check: all 26 one-time native approval decisions (14 in R1, 12 in R2) were consumed through local Matrix; no active router dispatch remained. Both Docker services are healthy and Palpo is bound to 127.0.0.1:8008. Project-2 membership remained correct after the later backend restart (evidence 64). All old Herdr sessions remain running. Global Claude settings and Codex config/hooks hashes are unchanged (evidence 44). Computer Use retry still failed with cgWindowNotFound (evidence 45), so fresh Robrix @ GUI acceptance remains unverified. Lower octoscode/kimi is the real tested execution backend; lower Claude/Codex/Grok combinations and the non-local continuity gate are not claimed. Source changes are reviewed and locally committed without push; final source commit and exact ACK are recorded in the external RESULT.md and .octos/OUTER_LOOP_REVIEW.md.


## 2026-09-06 Dashboard E2E repair

Operator added the HAFleet web dashboard to the same local E2E scope. Started the current Next console in mockup/ at 127.0.0.1:3100 against the isolated backend on 8090; the server-only proxy retains the API token. Computer Use in Chrome inspected resources/workforce, agent details/runtime/profile, capability/projects/engagements/usage/alerts/config/onboard. It reproduced false Config navigation and Profile save-success, a headless Codex marked tmux-missing, usage chart/card disagreement, and an untranslated probe state. An actual four-step preset creation persisted exact budget values (12345 total, 1234 daily), independently checked through the API.

Repairs add allowlisted on-demand runner readiness and durable dispatch activity without changing process online semantics, managed-workdir transcript attribution, consistent usage/task sets, working configuration form links, truthful readonly profile/runtime/oversight views, real probe refresh/timestamps and localized unusable state. Visible data now refreshes every 15 seconds and on focus/visibility return with concurrency/generation protection. Independent review additionally found that fixture agent details could delete a same-named real agent; all non-live action controls and the actual delete handler now reject that path, with an immediate in-flight guard.

Final Dashboard regressions: 53 tests / 6 files passed; production build, static invariants and ESLint passed. Backend related suite: 139 tests / 10 files passed plus an overlapping 22-test review check. npm run verify:ci passed its 502-test/45-file kernel; it does not replace the separately executed Dashboard selectors. agent-spec parse/lint/boundary checks ran; Node lifecycle scenarios remain skip and are not counted as passing. The console root package boundary needed ./package.json normalization; initial failed evidence is retained.

The final bundle is running on 3100. New same-origin API reads confirm Codex ready/idle with no active/queued/parked dispatches and no invented model, while the existing Claude tmux remains online. The test preset survived service restart and was then removed only after confirming no agent binding; e2e-fable remains. Both earlier accepted tasks stay done, project-2 memberships stay joined, both Docker services are healthy, and all original Herdr sessions remain running. No global mempal/Claude/Codex settings, original job processes or remote services were changed.

Fresh GUI acceptance of the final bundle is blocked: Chrome began returning cgWindowNotFound and read-only OS diagnostics confirmed the Mac was locked. Operator unlock was requested; the last retry still fails. Final click flows, visible automatic refresh and language/theme switching remain unverified, as does a new Robrix @ test. Initial successful GUI evidence and final HTTP/component checks are explicitly separate. Full screenshots, failures, tests, cleanup, local source commit and remaining acceptance work: ~/.octos/outer/verify/e2e-dashboard-20260906/RESULT.md.


## 2026-09-06 PR preparation and resumed Dashboard GUI

The operator authorized publishing the repairs as a PR. The Mac desktop became accessible again, allowing Computer Use to verify the final bundle's resources/workforce, matching task counts and 1k allocation charts, Codex on-demand runtime/Profile/Oversight, and the real Claude tmux pane. Config now opens both existing forms; Rescan updates the observed timestamp; Chinese unusable labels, light/dark/system themes and agent filtering work. Fixture agent action buttons and preset deletes are visibly disabled. A separately created, unbound local preset appears automatically in the resource list without manually reloading; cleanup targets that test record only. One remaining old stop-help text incorrectly promised supervisor restart and advised killing tmux for headless agents; it was corrected to state the unsupported operation without inventing a process.

Merged current origin/master (346cd8a, docs-only CI gating) and resolved its single adjacent CI conflict by applying the same code-change condition to root and dashboard dependency installation. The workflow's three tests passed. Independent review of 346cd8a..82b8712 found no evidenced credential leakage or Critical/Important blocker; thirty task-lifecycle/native-MCP/monitor tests passed independently. PR-preparation verify:ci first had one read ECONNRESET in api-runtime's Codex MCP test; the entire 24-test file then passed, followed by a complete 502-test/45-file verify:ci pass. Both outcomes are retained; no root-cause fix is claimed for the repository's documented intermittent socket-failure class. Evidence: ~/.octos/outer/verify/e2e-pr-20260906/.

The resumed Robrix GUI check also passed: selected e2e-codex through the actual @ member picker in project 2, inspected the complete composer before sending, and received exact E2EPR20260906GUIOK in the original thread. Matrix independently confirms the structured m.mentions target and one exact reply. The router automatically created probe task task_596aea36-3954-426e-8e96-ded362082c0e and it reached done; the driver did not force its status. This is a GUI transport/echo check, separate from the earlier real lower-work acceptance. Computer Use type_text dropped part of the draft and paste returned its clipboard timeout despite inserting the complete text; both were caught before sending. Final probe artifacts and screenshots are in e2e-pr-20260906/31–45.


## 2026-09-08 upstream integration closure

Resolved twenty HAFleet conflicts against origin/master4fb9749 in an isolated
worktree, preserving both native task lifecycle and Matrix/YOLO workflows. Fixed
the divergent migration-9 schema collision and retained SDK dependency isolation.
All279files/4151tests pass with one platform skip; final full-suite log is
/tmp/hafleet-integration-sharded-final.log. Four DM/startup regression files also
pass after the SDK injection adjustment. Build, syntax/lint, architecture, remote
package, CLI, dependency and447spec-binding checks pass, as do the webpack console
build and bilingual fixture browser workflows. Native agent-spec boundary passes;
its four Node scenarios remain Skip. Earlier fixture failures and the monolithic
OOM are preserved separately, with no automatic retries or skipped test files.

Robrix and Palpo upstream integrations were committed independently with native
validation. Only the separately requested Palpo web renewal/timeout repair was
deployed to Mini1; the broad HAFleet/Robrix/Palpo-Rust integration is not deployed.
No pushes or changes to the original concurrent website work. Root task-writer
is absent at this source checkout, so no canonical task completion was invented.
Review: docs/reviews/2026-09-08-upstream-integration.md.


## 2026-09-08 outbound implementation and no-tunnel acceptance

Implemented HAFleet durable outbound receive/publish and Palpo colocated Matrix relay, lease/ACK/sequence/generation checks, stored resource/status reads, automatic startup and import UI. HAFleet57d56da and Palpo9040bbcb were validated from isolated source trees before replacing the idle local services and pinned Mini1 containers. The minimal live Rust URL-CAS backport preserved existing authentication behavior and all registration identities. The original concurrent website checkout was untouched.

Real browser admin migration, owner download, HAFleet import and exact Mini1 Matrix proof passed. The owned SSH forwarding service and old bridge inbound listener were stopped. Repeated public browser/API checks and an independent Agent confirmed advancing heartbeat and three active usable verified requests. Old laptop18080 is retired; use https://crew.ominix.io:19444 and local HAFleet13202. Migration replays the old edision request as pending; no user request was auto-approved.

Post-cutover inspection found stale private-device endpoint caches. Follow-up4953baf validates and reuses original devices across the endpoint change. All fifty existing dispatches were complete before the coordinated bridge restart. All three live sessions changed only baseUrl and resumed sync with unchanged tokens/devices and no private startup warnings. No-tunnel browser acceptance passed again after that restart.

Validation: full HAFleet suite4174passed/oneplatformskip before the narrow device fix, followed by37passing tests across four exact direct-chat/outbound files. Palpo57Node and three browser suites passed; Linux minimal-backport CAS3 plus existing dynamic-auth1 passed. Production console, static checks, architecture and exact spec bindings passed. Native agent-spec cannot execute the seven Node lifecycle scenarios (Skip, not pass); the general console verifier retains three baseline invariant failures and one existing layout failure. No new native Robrix/model/file acceptance is claimed. Detailed evidence and recovery paths: docs/reviews/2026-09-08-palpo-outbound-implementation.md.


## 2026-09-08 shared thread context recovery

Reproduced why Edison could not see the operator's thread discussion with xiaobai: own Matrix replies were discarded by delivery dedup and direct-device own-message filtering before reaching the shared archive. Implemented adf3294 with79passing relevant tests and deployed only the bridge after existing user work finished. Retrieved actual shared-room history and repaired missing rows through the normal archive API, preserving source identity, promotion boundary and successful positions. Real read_conversation on a live-data copy included the recovered xiaobai answers for Edison. No Matrix message or model task was sent by verification. Native lifecycle12Node skips remain explicit. Report: docs/reviews/2026-09-08-shared-agent-thread-context.md.

## 2026-09-08 — Hagency website research and plan

Inspected the HAFleet source checkout, Robrix2 source checkout, Palpo source and
its separate web-admin worktree. Fetched the relevant public remote refs without
checking out or merging branches; read GitHub release metadata and public project
pages. Confirmed distinct local integration, public default-branch and released
states. Latest published releases observed: HAFleet1.2.0, Robrix/Robrix2 1.1.0,
Palpo0.4.0. Existing bilingual HAgency book and historical console/native imagery
are reusable only after naming, behavior and revision review.

Prepared docs/design/hagency-website-plan.md with positioning, audience paths,
16 core localized routes, project narratives, an eight-step demonstration,
visual direction, ten integrated guides, implementation architecture, maintenance
and measurable acceptance criteria. English/Chinese and developer-first audience
remain proposed defaults. Website implementation and publication were not started;
no application source, service, account or runtime profile was changed.

Research uses existing dated product-test reports, not a new application test run.
No executable website Task Contract is active: this artifact is an editorial
proposal; the independent website's implementation contract belongs to its next
stage. Agent-spec1.4 was inspected; an unretained draft's lint rejected manual
editorial scenarios without test selectors, so no lifecycle success is claimed
and no artificial test bindings were added. Root task-writer is absent; no
canonical task-state update was fabricated.

## 2026-09-08 — Adora website style and hero script review

The operator selected ymote/adora-website as the design reference and requested
inspection of its hero generation script. Local44ff68f matches remote HEAD.
Read dark/light generation scripts, Hero.astro, theme tokens, layout, theme
switcher and architecture section. Viewed both generated PNGs and captured
the deployed desktop hero in dark/light CSS states with an isolated headless
browser. Public page returned200. Screenshot captures are temporary review
artifacts under /tmp/hagency-adora-reference-{dark,light}.png.

The scripts use Google GenAI with gemini-3.1-flash-image-preview, separate prompts
and static PNG output. Their1920×1080 prose request is not an enforced API size;
both saved images are1376×768. Updated the Hagency proposal with the selected
charcoal/teal/amber design, HTML-over-generated-art hero, original collaboration
motif, a concrete generation brief, dark/light consistency and responsive image
checks. Product screenshots move below the hero. No image-generation request,
Adora source edit, website implementation or publication was performed.

## 2026-09-08 — Bilingual Hagency website implemented

The operator confirmed English and Chinese i18n. Created the independent Git
repository at projects/hagency-website (not a symlink). Implemented 29 routes per
language: 16 main pages, 10 guides, and 3 articles. Included all three project
narratives, eight-step illustrative workflow, search, theme/locale persistence,
verified download filters, documentation, security, roadmap, community, media,
localized metadata, RSS, sitemap, and 404. Generated original matching dark/light
hero art using the native image tool; prompts and outputs are documented in the
website's docs/artwork.md. This follows the reviewed Adora style.

Verified latest published releases through the GitHub API (11 binary/archive
assets), checked 16 source/documentation URLs (all HTTP 200), and retained the
distinction between published packages and local September 8 integration work.
No live product screenshots are invented; interface diagrams are labeled as
conceptual and the walkthrough explicitly uses illustrative data.

Validation from the edited website tree: Astro typecheck 0 errors/warnings/hints,
static build passes, all 8 Node/Playwright tests pass. Coverage includes 58 routes
and internal links/anchors, both locales, theme persistence, walkthrough/search/
downloads, all 16 main pages at 320/390/768px, and representative axe checks in
both themes. Native agent-spec 1.4 lifecycle boundary passes; six Node scenarios
remain native skips and its overall result remains non-passing. Independent Node
execution passes; see website docs/verification.md and lifecycle-result.json.

Static preview is running at http://127.0.0.1:4328/en/ and /zh-cn/, managed by
the website's Astro preview command. Original application source and services
were not modified. No public deployment, remote creation, commit, or push.
The root task-writer wrapper remains absent, so no canonical state was invented.

## 2026-09-08 — Matrix, federation, and agent-native positioning

The operator requested Matrix protocol education, its advantages over centralized
chat, open-source WeChat positioning, federation, and agents granted the same
privileges as humans. Added /en/matrix/ and /zh-cn/matrix/, a prominent homepage
section and hero copy, primary/footer navigation, and an expanded Matrix article.
The site now has 60 localized content pages. The new page explains clients,
homeservers and rooms, a five-row centralized/federated comparison, and explicit
room-role parity for human and agent identities. It distinguishes room grants
from runtime execution authority and explains relevant federation tradeoffs.

Added local-only interactive network and room-role illustrations. Actual Matrix
accounts, roles, servers and HAFleet runtime permissions were not changed. Six
primary Matrix source URLs returned HTTP 200. Typecheck/build pass; all ten Node
browser tests pass, including both new interactions, 60-route link validation,
17 main pages at 320/390/768px, and axe checks in both themes and languages.

Active website contract is specs/task-matrix-positioning.spec.md. Native
agent-spec1.4 boundary passes; four scenarios remain native skips because Node
is not executed, and the native overall result remains non-passing. Independent
browser evidence and lifecycle output are retained in the website docs. Local
preview remains on 127.0.0.1:4328; no public deployment, commit, or push.

## 2026-09-08 — Real project screenshot galleries

Added six genuine integration screenshots to the Hagency website: HAFleet
resources/engagements, Robrix2 native group/encrypted-room file collaboration,
and Palpo companion admin project access/resource catalog. Homepage previews
and two-image project galleries have English/Chinese descriptions, original
UI-language labels, and development-version context. Palpo's separate companion
app is explicitly distinguished from its server release. Original PNGs remain
byte-identical to their reviewed test captures; SHA-256 provenance and optimized
WebP previews are retained in the website tree. No live service was contacted
or changed for the captures.

The image viewer supports keyboard open/close and focus return, actual-size
scrolling, direct original links, no-JavaScript navigation, and localized load
errors. Typecheck and build pass. The complete browser suite passed 13/13;
after a final CSS-only catalog framing adjustment, all three screenshot tests
passed again (0 skips). Automated mobile checks cover 320/390/768px; visual
review covers both themes, both languages, desktop/mobile and long images.

Active contract: specs/task-project-screenshots.spec.md. Native agent-spec1.4
reports one boundary pass and five skips; its overall result remains non-passing
because it does not execute Node tests. Separate browser logs, lifecycle output,
and provenance are under the website docs. Preview is running at 127.0.0.1:4328.
No public deployment, commit, or push. The absent task-writer was not replaced.


## 2026-09-09 — Chinese Agent name validation repaired

Updated Palpo form/API and HAFleet protocol validation. Chinese display names
survive approval/provisioning fixtures while runtime/Matrix IDs remain ASCII.
67 Palpo and23 HAFleet tests pass, plus the Chinese-name Playwright fixture.
Deployed Mini1 web136171fcade9cd56 and restarted idle HAFleet backend/bridge.
Live 中文验证-0909 request reached HAFleet as pending without allocation.
Native agent-spec boundary passes;10 Node scenarios remain skipped.
Full evidence: docs/reviews/2026-09-09-unicode-agent-names.md. No commit/push.

## 2026-09-09 — Final-allocation Matrix Agent retirement

Implemented and deployed the operator's Edison retirement request: local runtime
stop and admission fencing, outbound fleet/request-scoped deactivation, zero-room
and denied-AS-authentication verification, durable retry and console feedback.
Other active allocations prevent whole-account retirement. Reconciled legacy
management aliases by exact MXID after real acceptance found one stale registered
row. Original revocation time and chat history remain intact.

Playwright invoked the deployed console action. Edison is now deactivated, has
zero joined rooms, fails AS authentication403 and AS discovery404; its
representative remains200 and four sibling identities remain active accounts.
All four sampled historical messages remain unchanged. 165 HAFleet tests and71
Palpo tests pass; production console build and468 spec bindings pass. Native
lifecycle boundary passes but four Node scenarios remain Skip (non-passing).
Unrelated usage502s remain recorded. Local backend7238/bridge7239/console7240;
Mini1 Web image177462cdd1d6be2d. Matrix Rust service unchanged. No commit/push or
fabricated canonical task transition. See
docs/reviews/2026-09-09-agent-matrix-retirement.md.

## 2026-09-09 — Account requests approved through Robrix

Implemented and deployed the requested Palpo Web signup → private administrator
room → native Robrix Approve/Reject → Matrix registration → ordinary-user login
flow in the isolated Palpo account-approval worktree. Real Mini1 approval created
a usable ordinary account; real rejection prevented login. The approved user
created a project and sent Agent request f3b7e3d6-49e1-4655-9da0-dfafd226e1fb,
which HAFleet received and left pending its owner's resource decision.

66 Node tests and four fixture browser scripts pass. Separate native evidence
records actual Matrix verdict events and post-restart receipt/login recovery.
Closed the first-use administrator history gap and stale project-readiness UI.
The earlier web release required forced shutdown and left a stale lock; recovered
only after confirming its owner stopped, then bounded shutdown and tested worker
I/O cancellation. Later upgrade exited0 and kept both request decisions.

Final web image: palpo-web-admin:cc23a8c98efb31c9. HAFleet runtime, Matrix Rust
binary and operator Robrix desktop profile were preserved. Source changes are
uncommitted in feat/account-approval-20260909; no push/merge was performed for
this batch. The source checkout still has no provisioned task-writer, so no
canonical task-state transition was fabricated.


## 2026-09-09 — Investigate approved ymote login failure

Verified actual administrator verdict and successful registration of @ymote at
2026-09-09T16:31:14Z. Matrix reports an active ordinary account, unlocked and
not deactivated; the pending encrypted password was removed after registration.
Observed two HTTP403 login attempts, followed by HTTP429 even for login
discovery. Adjusted only the live Matrix login rate configuration (burst20,
refill0.1/sec), backed it up and restarted the homeserver. Six consecutive
public login discovery checks returned200, an existing approved ordinary test
account authenticated successfully, and the account approval worker is ready.
No password reset, account recreation, Matrix source edit, commit or push.
The operator was asked for the exact remaining error and to retry with the
password chosen for ymote; that user's password has not been independently
verified.

## 2026-09-09 — Account and Agent workflow integration

Committed the HAFleet changes as1e2d279 and Palpo Web as3d63ae11; the latter is
now on local main. Integrated HAFleet with local master in an isolated worktree,
retaining both sides of two additive documentation conflicts and preserving
the primary workspace's unrelated website edits separately. CI exposed an
extracted-handler visibility issue and a separate outbound inbox ownership gap;
the explicit local guard and exact adapter-owner rule now pass, with negative
coverage retaining the router internal-import restriction.

Final CI:505 kernel/CLI tests and470 spec bindings pass. Related regression190,
new boundary1 and Palpo71 tests pass; all four Palpo browser scripts pass. Counts
overlap. Native lifecycle remains non-passing with two Node skips; optional live
CI probes skipped without a runtime. No live changes or push. Palpo upstream
Rust commit62fa8566 remains outside this local Web merge. Full evidence and
restoration notes: docs/reviews/2026-09-09-account-agent-lifecycle-merge.md.

## 2026-09-09 — Close the c380959/f89c746 review findings

Retraced the supplied review against merged master 8dfea48 and repaired the
remaining findings in fix/review-closure-20260909. Direct commands use their
Agent device and cannot wedge sync on a failed reply; retired mentions no longer
defer admission. Host-owned session provenance prevents private replies, files
and activity from entering promoted group rooms. Explicit unreachable-side
abandonment now uses the structured cleanup result and retains remote failures.

Admission floors and bounded history windows limit context work. Historical
attachments download only on authorized receive_file calls. Grant responses
exclude internal approval metadata; null-Agent owner resolution requires room
agreement. Approval rollback/retention, permanent notice settlement, schema
migrations, runtime spawn ownership and the remaining portability/UI findings
are covered by targeted regressions. Earlier merged fixes for SDK isolation,
the five original tests and catalog withdrawal remain intact.

Final full suite: 4,226 passed, zero failed, one platform skip in 286 files.
CI passes with 479 spec bindings and 505 overlapping kernel/CLI tests. Console
Webpack build and English/Chinese Playwright permission flows pass. Default
Turbopack cannot follow the isolated worktree's external dependency symlink.
Native agent-spec has one boundary pass and nine skipped Node scenarios, so its
overall result remains non-passing. Optional live probes skipped; no live
deployment or Matrix/LLM acceptance is claimed. Findings, evidence and limits:
docs/reviews/2026-09-09-review-followup.md. The source task-writer is absent.

## 2026-09-09 — Merge conflict-free dependency PR 157

Merged HAFleet PR 157 into the isolated review integration; GitHub confirms
MERGED at 7f61fcd. Fresh root/console npm installs, the actual registry advisory
ratchet and 22 focused tests pass. Full verify:ci passes with 483 executable
spec bindings and 505 kernel/CLI tests. The new inventory rejected the PR
manual-test placeholder; its mandatory registry check now lives explicitly in
Constraints, while all four real offline test bindings remain intact. Native
agent-spec records one boundary pass and four Node skips, not lifecycle success.

HAFleet PRs 154/155/156/158 conflict with local master and remain unmerged.
The current account has only READ permission on palpo-im/palpo, so its upstream
PRs cannot be merged here. No live deployment changed. Website coordination
edits remain outside the integration. Full details and evidence locations:
docs/reviews/2026-09-09-pr157-integration.md.

## 2026-09-09 — Resolve four open HAFleet PR conflicts

Integrated PRs 154/155/156/158 into an isolated branch from 212de5f. Preserved
loopback custody, runner activity and cleanup proof, private execution policy,
legacy terminal observation, reusable approval authority and representative
identity. PR 156 integration regressions were reproduced before repair. All
focused batches pass: 88, 34, 97 and 191 tests (overlapping coverage). Final
suite: 4,268 pass, one platform skip in 290 passing files. CI passes with 508
kernel/CLI tests and 523 specification bindings. Console production build and
English/Chinese Playwright permissions and hybrid runtime checks pass. Native
Node lifecycle skips remain non-passing; actual Vitest results are separate.

All four PRs merged on GitHub; master is 0ab52fe and there are no open HAFleet
PRs. Their merge receipts joined the local integration at f8c82c4 without
changing the verified tree. Local review/workflow history and conflict fixes
remain unpublished. Website coordination edits are preserved separately;
no live service changed. Details: docs/reviews/2026-09-09-open-pr-integration.md.

## 2026-09-09 — Console debugging-text cleanup

- Implemented concise bilingual presentation and closed diagnostic details across
  the provider console on `fix/console-product-copy`. Status, sample/unavailable
  data, execution permissions and budget limitations remain explicit.
- Fixed the Resources toast-hook mismatch and a null provider label found during
  visual inspection. 84 Vitest tests and 16 controlled browser cases passed; the
  production build passed. Legacy invariant failures remain identical to the
  baseline; native agent-spec lifecycle has four unsupported, non-passing skips.
- Replaced only the local web UI at port13202 with the verified build. Backend
  PID7238/port18194 and Agent/Matrix processes remain running. Six live pages and
  both deployed UI languages were checked read-only. No live requests were approved.
- Source is not committed. Prior documentation edits were preserved. See
  [review and deployment evidence](reviews/2026-09-09-console-product-presentation.md).
- Final deployment check exposed an intermittent usage timeout against the
  unchanged 8-second proxy limit. Resource data remains live; the page labels
  usage unavailable. This was not counted as successful live usage verification.

## 2026-09-09 — Start native migration in an isolated worktree

- Created `feat/rust-migration` from merged `5dbef22` in a separate worktree.
  Copied only the migration draft, requirement and review from the original dirty
  checkout. The original checkout and running deployments remain independent.
- Revised the Node-only project contract, added REQ-RUST-MIGRATION-EXECUTION and
  a bounded Cargo task contract. Implemented native Salvo HTTP/CLI, private state,
  exclusive custody ownership, schema checks, atomic content-bound receipts,
  bounded queues/storage, explicit overload and drained shutdown.
- Added native subprocess crash/restart tests with PATH empty, JavaScript golden
  vectors, and a fresh encrypted Matrix SDK restart proof with strict cross-signing.
  M0 discovery records112 entry/helper candidates and201 literal routes. ADR-095
  documents exact future domain commit boundaries and remaining release gates.
- This checkpoint does not implement project/resource allocation, Agent execution,
  connected Matrix/Palpo transport or console parity. No production cutover,
  live credentials, canonical runtime task update or deployment is claimed.
- Local verification:13 Rust tests passed, two Vitest spec-binding tests passed,
  rustfmt/Clippy/ESLint passed, and the Cargo-backed agent-spec lifecycle passed
  all8 scenarios plus the explicit boundary check (9/9, no skips). Windows GNU
  cross-compilation/Clippy of the native app/store/tests passed; native OS CI
  is still required. Production listeners remain PID46398/13202 and PID7238/18194.

- M2 started: ported the selected-resource/shared-seat projection with29 golden
  vectors from the current JS function. Added JSON-safe token values, checked
  accumulation and missing/null period distinction. The native API still cannot
  allocate or provision Agents. All15 local Rust tests pass; the allocation task
  lifecycle passes2 scenarios plus its boundary check. Native CI for080cf90 passed
  on Linux and macOS; Windows stopped at CRLF-converted golden JSON. Added explicit
  LF attributes for those byte fixtures before rerunning native Windows checks.

## 2026-09-10 — Native selected-resource domain checkpoint

- Continued the full migration goal in `feat/rust-migration`; preserved the
  original checkout and live services. Prior checkpoint47d217a passed native
  Windows/Linux/macOS CI and existing Node CI (4,280 passed, one platform skip).
- Added one domain SQLite writer for project binding/evidence, immutable intake,
  selected-pool/shared-seat reservation, decision replay and effect intents. Native
  admission verifies full source/room/owner observations and cannot deserialize
  HTTP-provided approval flags. Runtime and Matrix adapters remain unimplemented.
- Added38 JS-produced Unicode/public/runtime identity vectors. Added concurrent
  reservation and injected-commit-failure coverage, uncertain effect recovery,
  generation/project remapping fences and explicit retirement retry. Resource
  configuration/publication APIs now preserve withdrawal on ordinary edits and
  retain operator access to withdrawn configurations.
- Local checks:22 Cargo tests passed, rustfmt/Clippy and three golden vector
  generators passed. Native bindings have17 selectors and zero missing tests;
  the new agent-spec lifecycle passed all6 scenarios plus its boundary check.
  Windows GNU cross-Clippy passed earlier in this checkpoint; native OS CI must
  still validate the final committed changes. See the checkpoint review for
  remaining M2 scope and M3–M9 gates. Full migration remains active.
- The existing documentation scan initially consumed extensionless Cargo binaries
  under `target/` and failed at Node's maximum string length. Added Cargo output
  exclusion plus a fixture proving ordinary extensionless scripts remain scanned.
  The documentation and Node spec-binding checks now pass all9 tests. No runtime
  behavior or assertion was bypassed to hide this migration/build interaction.

## 2026-09-10 — Native model qualification checkpoint

- Embedded the existing role-capacity policy without changing its model table.
  Native model/provider/reasoning qualification matches 99 JavaScript profile
  cases and 18 resource ordering cases. Catalog roles are derived; supplied role
  grants are rejected. Explicit withdrawal persists, and review qualification
  counts only active model families on the requesting registration.
- Admission and first reservation recheck current qualification while exact
  replays and already reserved effect identities remain recoverable. Native
  domain schema 2 uses ordered transactional migration files; injected failures,
  old cached roles, repeated startup and downgrade rejection are covered.
- Local validation: 25 Cargo tests passed with no failures or ignored tests;
  rustfmt, Clippy, qualification golden check and generator ESLint passed.
  Native spec bindings resolve all 20 selectors. The qualification lifecycle
  passed all 3 scenarios plus the boundary check, with no skip or uncertainty.
- Prior commit 5c08ad9 passed Linux/macOS/Windows native CI and Node CI
  (4,281 passed, one platform skip). That CI result does not validate this
  checkpoint until its own committed head runs. Full M0–M9 migration remains
  active; task/dispatch authority and recovery are the next bounded implementation.

## 2026-09-10 — Native canonical task and dispatch kernel

- Committed qualification checkpoint f7a2c89 passed native Linux/macOS/Windows
  CI (run 34459254680) and existing Node CI (run 34459254681).
- Added domain schema 3 for tasks, sessions, dispatch attempts, resource leases,
  content-bound mutation receipts and task events. Current started capabilities
  are required for task writes; coordinator reads remain scoped to its session.
  Comments use the host-owned Agent name, heartbeat uses the host clock, and
  explicit completion advances the task authorization epoch. Missing wait patches
  retain metadata; explicit null clears it. Runtime completion cannot finish tasks.
- Dispatch payloads freeze before launch. Parked work retains resources. Expired
  or restarted unstarted work can requeue; started work becomes unknown, with
  session/workspace quarantine and authenticated rejected-output audit. Inspected
  recovery creates a distinct dispatch and supersedes old queued instructions.
- Local validation: all 30 native tests passed; subsequent final test-only expiry
  additions passed their bound lifecycle scenario. The task lifecycle passed all
  5 scenarios plus its boundary check, with zero failures/skips/uncertainty.
  Clippy, rustfmt and the JS transition golden/ESLint checks passed. Shared vectors
  cover every 25-pair canonical task transition; 25 native spec selectors resolve.
- No real runner, Matrix ingress or live deployment was changed. Host-only session
  admission/inspection still requires the M4/M5 adapters. Next: mailbox and group
  ordering/deduplication, graph/delegation, follow-up and durable reply delivery.
  Full migration goal remains active; the draft PR is not a cutover candidate.

## 2026-09-10 — Native message admission and frozen input

- Previous commit 66168f4 passed native Linux/macOS/Windows CI (34460927082) and
  existing Node CI (34460927153). Its task/dispatch changes are verified at that head.
- Added schema 4 canonical session uniqueness, immutable event identity and
  independent per-session input projections. Committed arrival sequence preserves
  total order even with equal or reversed source timestamps. Bounded/filtered
  inbox reads leave unseen work pending.
- Dispatch enqueue freezes and claims admitted input atomically. Only a wake can
  start ordinary work, and runner inbox reads are restricted to the current frozen
  batch. Completing one Agent's dispatch preserves another Agent's copy. Unknown
  work transfers input only through inspected recovery; superseded unstarted input
  returns to the pending inbox and quarantine still accepts new messages.
- Validation: 34 native tests passed, zero failed/ignored. Message lifecycle passed
  four scenarios plus boundary, zero skips/uncertainty. Native spec catalog has 29
  selectors, none missing. Clippy and rustfmt passed. Injected admission/input/
  settlement failures roll back; oversized input remains pending; conflicting old
  session bindings roll back the schema upgrade. A compiler assertion prevents
  adding DeserializeOwned to the trusted ingress command.
- Real Matrix authentication, room privacy/history and mention policy remain M5;
  native ingress has no public HTTP constructor. Next implement narrow runner APIs,
  task input activation/follow-up, dependency/delegation and reply delivery. The
  complete migration remains active and production services remain unchanged.

## 2026-09-10 — Native scoped runner service API

- Message/input commit 22fc509 passed native three-OS CI (34462648100) and existing
  Node CI (34462648110). Continued the full migration goal in the same worktree.
- Added a narrow private runner API for tasks, comments, typed mutations and frozen
  input pages. Current runner headers authorize this surface separately from
  operator resource management; browser/proxy, duplicated and URL credentials fail.
  Unknown actions, caller clock/identity fields and oversized bodies are rejected.
- RunnerCommand now obtains its clock inside the bounded domain writer. A request
  that entered with valid credentials is rejected if its lease expires while queued.
  Each final operation also rechecks parked/revoked/current task authority. Canonical
  completion still requires an explicit transition and advances the task epoch.
- Validation: 38 native tests passed, zero failed/ignored; four new scenarios plus
  lifecycle boundary passed with no skip/uncertainty. All 33 native spec selectors
  resolve. Clippy and rustfmt passed. HTTP tests exercise the real Salvo handlers
  with in-process requests and fixture allocations; they do not prove live runtime
  or homeserver integration. The queue-time authority test exercises the actual
  dedicated writer and expires its queued capability before releasing that writer.
- Next: task metadata/input activation, graph/delegation, authenticated follow-up,
  durable reply delivery and real runtime/platform ownership. Full migration remains
  active. Native execution is still unavailable and production services are intact.

## 2026-09-10 — Native task activation, delegation and human follow-up

- Previous runner API commit 99aaac6 passed native three-OS CI (34464031294)
  and Node CI (34464031312). Continued the full migration in the isolated worktree.
- Added schema 5: task metadata, task/source binding, idempotent attachment receipts
  and a fenced notice outbox share domain.sqlite3. Failed notice or input writes
  roll back the complete intent/activation transaction. Receipts bind exact server,
  room and stable Matrix transaction ID; restart preserves claims until expiration.
  Revoked allocations cannot activate, and permanent send failure requires retry.
- Runner POST /delegations goes through the bounded writer and its execution-time
  clock. Only the current started creator can delegate admitted source input within
  the project; an explicit parent must be its bound task. Child progress and input
  acknowledgement remain independent from the parent. Forged owner/delivery fields
  are rejected by the actual Salvo endpoint.
- Human follow-up keeps the original task done during enqueue and claim. Actual
  start rechecks attached, unprocessed original-sender thread input, new receipt/
  origin time and current dispatch authority, then changes the epoch and commits
  a continuation notice. Non-message peer traffic, another sender and stale input
  do not reopen tasks. Taskless dispatches cannot bypass an intent's activation.
- Validation: all 43 native tests passed, zero failed/ignored. Five specification
  scenarios plus the lifecycle boundary passed without skip/uncertainty; all 38
  native spec selectors resolve. Clippy, rustfmt and diff checks passed. Coverage
  includes rollback, restart/retry, delegation, follow-up and private HTTP authority.
- This is an internal task orchestration implementation, not a live transport or
  runner claim. Graph scheduling, final replies, group/MCP interfaces, real Matrix
  provenance/privacy and M4–M9 remain open. No live deployment was changed.

## 2026-09-10 — Native task graph planning policy

- Task-intent commit 5e68a06 passed Windows/macOS/Linux native CI (34467398049);
  Node CI (34467398054) also passed. No live service was changed.
- Added a bounded pure graph planner with deterministic dependency failure,
  cancellation, skip and readiness transitions. Fractional results remain JSON
  values. Missing/null comparison and primitive truthiness match the existing
  JavaScript policy; inherited functions are inert values only. String property
  traversal remains refused, as required by the actual legacy getNestedValue.
- The unchanged JavaScript implementation generates 156 condition and ten graph
  transition vectors. Native CI now checks fixture freshness. The initial vector
  run caught a string-property mismatch, which was corrected against the source;
  all vectors now pass. Definitions reject missing/duplicate nodes, dependency and
  condition cycles; results and nesting are bounded. Runtime definitions cannot
  assert an owner or completed node status.
- Validation: all 46 native tests passed, zero failed/ignored; Clippy, rustfmt and
  fixture checks passed. Three graph scenarios plus lifecycle boundary passed
  without skip/uncertainty; all 41 native specification selectors resolve.
- This is a pure planning step. Durable graph storage, authenticated canonical-task
  observations, internal/local session routes and mailbox/group delivery still need
  integration. Proposed dispatch status is never evidence of actual task execution.
  The full M0–M9 migration remains active.

## 2026-09-10 — Structured numeric runner payloads

- Graph-policy commit a818f0f passed native three-OS CI (34468571499); its Node
  CI (34468571496) is still running. The migration continues in the isolated tree.
- Removed the integer-only restriction from execution data while preserving it for
  signed authority DTOs. Queued input now stores and hashes the same canonical
  representation. Finite numbers follow existing JavaScript double semantics;
  exact identity/count/generation fields keep their typed authority validation.
- Used the Rust ecosystem integration skill to inspect pinned ryu-js 1.0.3, its
  safe finite formatting API, license and minimum Rust version. Enabled JSON float
  round-trip parsing. No authority endpoint or policy was broadened.
- The unchanged JS canonicalizer generates 271 numeric vectors, including exponent
  boundaries, subnormals, negative zero, very large values and deterministic double
  bit patterns. Those exposed why exact-integer-literal rejection is incompatible
  with JS shortest formatting; the final data contract uses JS Number semantics.
- All 48 native tests passed, zero failed/ignored. Numeric dispatch replay and
  conflicting content were verified across repository restart. Both scenarios and
  the lifecycle boundary passed with no skip/uncertainty; all 43 native selectors
  resolve. Clippy, rustfmt and fixture checks passed. The initial boundary check
  required explicit ./ prefixes for root Cargo paths; the corrected exact scope
  passed without broadening the intended changes.
- Internal session/group/mailbox routes and graph-to-canonical-task integration
  remain next. Real process/transport, console and cutover work remain open.

## 2026-09-10 — Internal conversation session ownership

- Numeric payload commit bcd028e passed native three-OS CI (34469526457) and Node
  CI (34469526454). Graph-policy Node CI (34468571496) also passed.
- Schema 6 adds explicit internal routes under the existing task/dispatch owner,
  without fake Matrix rooms. Matrix and internal wire variants reject mixed or
  unknown fields. Canonical route uniqueness and old-schema migration tests remain.
- Conversation creation requires a current started creator, active participants
  from the same project/generation, and an idempotent content-bound call ID. Its
  conversation row and all participant session/membership rows commit atomically.
  The private API obtains authority time inside the bounded domain writer.
- Reads allow the exact creator session or the internal session belonging to that
  conversation. Another Matrix session of the same Agent is denied. Revoked
  participants lose current capability access. Matrix ingress and inbox reads
  explicitly refuse internal routes. Completed tasks cannot create conversations.
- Tests cover injected write rollback, replay/conflict, foreign/parked/expired and
  revoked callers, independent tasks for one Agent in two conversations, and
  unknown-attempt recovery across restart. An initial recovery test reused the old
  payload and was correctly refused; the test now asserts that refusal and submits
  a distinct inspected recovery instruction without weakening the guard.
- All 53 native tests passed, zero failed/ignored. Five internal-session scenarios
  plus lifecycle boundary passed without skip/uncertainty; all 48 native selectors
  resolve. Clippy, rustfmt and diff checks passed. No live service changed.
- Peer mailbox and group lifecycle, atomic graph/message/task linkage, standalone
  local Agents and the remaining M4–M9 work remain open. Conversation admission
  alone is not peer delivery, model execution or complete migration parity.

## 2026-09-10 — Scoped native peer input delivery

- Internal-session commit b5373b2 passed native Windows/macOS/Linux CI (34470955482)
  and Node CI (34470955561).
- Schema 7 commits a content-bound peer receipt and every recipient projection
  atomically. Sources come from the current started capability. Recipients are
  exact conversation sessions, including the original creator's Matrix session.
  Arbitrary rooms belonging to the same Agent cannot send or receive this traffic.
- Request and response messages can wake incomplete work; notification messages
  remain context. Pages are bounded and do not acknowledge input. Dispatch enqueue
  freezes input, completion acknowledges only its recipient copy, and inspected
  recovery transfers uncertain input while releasing superseded queued arrivals.
- Matrix activation still needs the original admitted input; peer replies cannot
  reopen completed tasks. Current membership is checked at send, enqueue and start,
  with closed-conversation work filtered before lease. Runtime authority also now
  rechecks task binding, including processes started before a new pending intent.
  Separate host-only unstarted cleanup remains available after binding changes.
- All 58 native tests passed, zero failed/ignored. Five peer scenarios and lifecycle
  boundary passed without skip/uncertainty; all 53 native selectors resolve.
  Clippy, rustfmt and diff checks passed. Tests include injected admission/claim/ACK
  rollback, stale/revoked/parked callers, finite data, retry conflicts, separate
  recipient custody, new arrivals across restart and actual protected Salvo routes.
- No models or live homeservers were contacted. Group lifecycle, atomic graph/task
  linkage, final delivery and M4–M9 remain open. An existing completed Matrix task
  cannot yet use a taskless inspected recovery report because the intent binding
  guard refuses it; a durable scoped report exception is the next correction.

## 2026-09-10 — Completed task report recovery

- Peer-mailbox commit 17246a0 passed native Windows/macOS/Linux CI (34474218701).
  Its Node CI (34474218712) was still running at this checkpoint.
- Schema 8 closes the completed Matrix intent recovery gap. A host-inspected
  taskless replacement receives a durable grant for the exact completed task and
  epoch, committed with recovery history and both input transfers. Grant failure
  rolls back replacement, quarantine clearing and input assignment together.
- Runtime authority distinguishes reporting from creating work. A report may read
  its completed task and frozen input, but cannot reopen it, read unrelated child
  tasks, delegate, create groups/tasks or send peer requests. Revoked allocations
  and changed task epochs invalidate report authority at current dispatch gates.
- Consecutive report interruptions remain recoverable after separate host
  inspection. Fresh human input remains pending independently; a new human turn
  invalidates old reports even after that new turn completes. Historical native
  report migration requires the original attempt's persisted done receipt.
- Distinct recovery instruction comparison now uses canonical finite-number JSON,
  preventing 1 versus 1.0 from bypassing the different-payload requirement.
- All 61 native tests passed, zero failed/ignored. Three report scenarios plus the
  lifecycle boundary passed without skip/uncertainty; all 56 native selectors
  resolve. Clippy, rustfmt and diff checks passed. An initial fixture used incorrect
  attachment argument order; the fixture was corrected and full validation rerun.
- This grants neither filesystem access nor external delivery authority. Real
  process stop/ownership proof, group lifecycle, graph/task linkage, final output,
  transport, console and cutover gates remain open. Live services are unchanged.

## 2026-09-10 — Initial native process scope proof

- Recovery-report commit c043ea1 passed native three-OS CI (34475309134) and Node
  CI (34475309167). Peer commit 17246a0 Node CI (34474218712) also passed.
- Added the separate hagency-platform library and native probe binary. Its host
  launch DTO has explicit absolute executable/cwd, bounded argument/environment
  data and no inherited environment or shell wrapper. Existing locked rustix and
  windows-sys dependencies are reused; no dependency versions changed.
- Windows JOB_LIST establishes kill-on-close Job Object ownership inside process
  creation. Microsoft documents a crash window in suspended-then-assign startup;
  this implementation avoids that window. Cancellation uses owned handles and
  reports whole-tree stop only after zero active job processes and leader exit.
- POSIX establishes a process group before exec and holds the unreaped leader until
  final signals. Early-exit and repeated-stop tests preserve cancellation authority
  without signalling a reaped PID again. Group cancellation does not claim detached
  child or owner-crash containment; required crash guarantees refuse before spawn.
- ADR-029 records the API guarantee, exclusive host child-reaper requirement and
  separation from sandbox permission. Probe tests cover grandchildren, early exit,
  Drop cleanup, Unicode/quote/backslash/literal shell characters, explicit env,
  invalid/missing executable, embedded NUL and unrelated-process isolation.
- All 64 native tests passed locally on macOS, zero failed/ignored. Three scoped
  scenarios plus boundary passed; 59 native selectors resolve. Workspace Clippy,
  rustfmt and diff checks passed. Windows all-target Clippy cross-compilation also
  passed. Windows actual owner-crash Job Object execution remains a CI gate for
  this commit; the POSIX scenario proves refusal, not unimplemented containment.
- Agent execution remains disabled. POSIX guardian and detached-descendant identity,
  actual sandbox/runner IO, M3 graph/group/final delivery and M5–M9 remain open.
  No live deployment or model was contacted.

## 2026-09-10 — Kernel-bound child signal identities

- Process-scope commit e78f274 passed native CI on all three OS families
  (34476712504), including the actual Windows owner-exit Job Object test. Its
  Node run (34476712391) was still running when this step began.
- Added opaque child signal authority constructed only from a host-owned Child.
  Its read-only PID/birth metadata cannot create authority or retarget the handle.
  Linux retains pidfds, Windows duplicates existing process handles, and macOS
  checks unique process lifetime plus kernel-checked audit PID versions.
- Local SDK/source inspection found macOS proc_signal_with_audittoken. A harmless
  SIGCONT probe accepted the current version and rejected a changed version with
  ESRCH; signal zero was rejected with EINVAL and was not treated as a pass. Rust
  tests now preserve that real kernel check. An exec fixture confirms lifetime
  continuity while audit versions can be refreshed before a guarded signal.
- Tests terminate controlled children while unrelated children keep writing,
  retain expired handles after reaping, reject mismatched metadata and exercise
  in-place Unix exec. They do not force numeric PID recycling or prove descendant
  discovery. Existing process-scope tests remain unchanged and pass.
- All 68 native tests passed locally on macOS, zero failed/ignored, including one
  macOS-only kernel test. Three child-identity scenarios plus boundary passed;
  62 native selectors resolve. Workspace Clippy, rustfmt and diff checks passed;
  Windows all-target Clippy cross-compilation passed. New Linux/Windows runtime
  checks remain CI gates for this commit.
- Native guardian handoff, complete descendant discovery, runner IO and effective
  sandbox integration remain next. No model or live service was contacted; the
  platform primitives are not yet connected to Agent dispatch.

## 2026-09-10 — Native guardian startup and owner-loss cleanup

- Child-identity commit 2f44bd9 passed native three-OS CI (34479002630) and
  Node CI (34479002645). Its predecessor e78f274 also passed both pipelines.
- Added a native guardian entrypoint and shared SupervisedProcess API. Unix uses
  an anonymous socket with separate Prepare and Start messages, bounded frames,
  absolute partial-frame deadlines and explicit launch environment. Windows
  retains direct atomic Job Object ownership without an unnecessary helper.
- Owner EOF, malformed control input and leader exit now cancel the owned scope.
  Guardian startup failure launches no fallback. A host timeout closes its channel
  and preserves independent guardian cleanup; it does not kill the guardian and
  infer descendant stop. POSIX reports still refuse full cleanup guarantees.
- Auditing found that a plain dup would leak the reply endpoint into work. The
  implementation now duplicates it with CLOEXEC; real fixtures verify that no
  socket descriptor reaches the work process. No runtime secrets are printed.
- The actual hagency CLI integration test found macOS group signalling returning
  EPERM for a zombie-only group. A standalone native reproduction and XNU source
  confirmed the behavior. Stop now attempts both group and owned-child signals,
  reaps the leader and retains signal failure in signals_accepted. It never treats
  EPERM as evidence that a group is empty or complete descendant cleanup succeeded.
- All 73 native tests passed locally on macOS, zero failed/ignored. Four guardian
  scenarios plus the boundary passed; 67 native selectors resolve with none
  missing. Workspace Clippy/rustfmt, Windows all-target Clippy cross-compilation
  and diff checks passed. Actual new Linux/Windows behavior remains a CI gate.
- Tests exercise controller exit without Drop, grandchildren, early leader exit,
  unrelated-process survival, repeated stop, Unicode/literal argv, uncommitted
  startup, oversized and partial frames, active protocol failure and the actual
  native CLI. No model, deployed store or live service was contacted.
- Complete detached-descendant discovery, guardian-loss recovery, bounded runner
  IO, effective sandbox/approval adapters and M3/M5–M9 parity remain open. Agent
  execution remains disabled; this checkpoint does not complete the migration.

## 2026-09-10 — Close the guardian CI descriptor leak

- Guardian commit 5678697 passed Ubuntu and Windows native CI but failed macOS
  native CI (34481110874). The existing zero-inherited-sockets assertion found
  two extra descriptors from the embedding CI host. This failure was not waived.
- Added a controlled parent socket pair with CLOEXEC explicitly cleared. It
  reproduced the same two-socket failure locally before the fix. Both guardian
  and work startup now seal every descriptor above stderr in the post-fork child.
  The original strict assertion passes, and parent descriptor flags stay intact.
- Linux uses direct close_range(CLOEXEC), requiring kernel 5.11+. macOS uses its
  actual post-fork descriptor table and fcntl with fixed stack storage, refusing
  truncated observations before exec. Only direct syscalls run in the callback.
  The existing locked libc supplies its ABI; no dependency version changed.
- Descriptors are marked CLOEXEC rather than closed immediately, so Rust can
  still deliver exec failures through its internal pipe. Existing missing-binary
  and invalid-startup tests continue to pass. Windows handle inheritance remains
  disabled by its native CreateProcess path.
- All 73 native tests passed locally with the reproducer enabled, zero failed or
  ignored; workspace Clippy and rustfmt passed. Linux and Windows all-target
  Clippy cross-compilation passed. Four guardian scenarios plus boundary passed
  again. The corrected OS CI remains a gate; no deployed service changed.

## 2026-09-10 — Linux detached-descendant custody

- Descriptor-sealing commit df52fd3 passed Ubuntu/Windows native CI and the strict
  socket check on macOS. macOS then failed the unrelated-progress assertion that
  assumed a write within exactly 80 ms. The test now checks native process liveness
  while requiring fresh writes within a bounded deadline; death or stalled work
  still fails. Run 34482313701 therefore remains failed, not waived or green.
- Added Linux guardian subreaper custody before workload startup. The guardian
  must be single-threaded with no pre-existing children, restores normal SIGCHLD
  disposition, and verifies pidfd wait support before admitting work. Native CLI
  guardian mode now runs before constructing Tokio.
- Discovery is a bounded prefix of the kernel child list. Each candidate requires
  P_PIDFD waitability before a pidfd signal; stale or unrelated candidates supply
  no signal authority. The root retains its exclusive std Child reaper. Only a
  reaped root plus kernel ECHILD with __WALL can establish full observed cleanup;
  empty/truncated discovery is never sufficient. Timeout/errors remain unknown.
- Added native detached-session/double-fork fixtures for cancellation, controller
  exit without Drop and early root exit. Windows uses its existing Job Object
  path. macOS refuses the unsupported custody requirement before starting these
  fixtures; it does not claim detached-descendant cleanup.
- All 76 native tests passed locally on macOS, zero failed/ignored; three new
  descendant selectors prove the explicit macOS refusal. Linux and Windows
  all-target Clippy cross-compilation passed. The three bound scenarios plus
  boundary passed, and all 70 native selectors resolve. Actual Linux/Windows
  descendant execution remains a required CI gate for this implementation.
- Workspace Clippy/rustfmt and diff checks passed. The guardian suite passed again
  after strengthening its liveness observation. Guardian-death recovery, complete
  macOS custody, actual runner/sandbox adapters and the remaining migration phases
  are still open. Full requested POSIX crash containment continues to refuse;
  observed successful cleanup is distinct from a promise that every cleanup will
  succeed. No deployed service, credential or live model was touched.

## 2026-09-10 — Native internal conversation lifecycle

- Previous head a737b83 passed Native Rust CI 34483826435 on all three operating
  systems and Node CI 34483826386. This includes real Linux/Windows detached
  descendants and the corrected macOS liveness observation; earlier failed runs
  remain recorded as failures.
- Implemented schema 9: creator-scoped member replacement and closure with
  expected revisions and content-bound replay receipts. Same-project/current
  allocation checks remain required, and rejoining creates a new internal SID.
  Retired sessions, tasks and immutable messages remain available for inspection.
- Closure fences affected dispatches and recursively closes child conversations
  created by retired sessions. Queued/unstarted work is superseded; started,
  parked and unknown work retains resource custody and durable host stop intents.
  Stop intents survive restart and count against the scheduler's concurrency cap.
- Host inspection settlement is fenced, idempotent and transactional. It cannot
  clear another unresolved attempt's custody or mark canonical tasks done. Input
  assignments are released without delivery acknowledgements. Closing a group
  fences creator batches containing its peer input, while unrelated live input
  becomes separately schedulable after inspection.
- Added the private runner operation endpoint with strict typed bodies. The new
  HTTP test caught Serde ignoring extra fields on a unit enum variant; Close now
  uses an empty struct variant and the original refusal assertion passes.
- All 80 native tests passed locally, zero failed or ignored. Coverage includes
  mutation/settlement rollback, stale/foreign/member authority, fresh rejoin,
  queued/leased/started/parked retirement, nested closure, restart, two shared
  readers retaining custody until both are inspected, and frozen input recovery.
  Workspace Clippy/rustfmt and diff checks passed. All 74 native spec selectors
  resolve; four scenarios plus boundary passed under agent-spec 1.4.0.
- Actual process stop observation, Matrix room membership, graph/task execution,
  final delivery and M4–M9 integration remain open. Fixture inspection records
  do not prove live process termination. Corrected native CI remains a gate for
  this commit. Original dirty checkout and deployed services remain unchanged.

## 2026-09-10 — Drain guardian reports after peer exit

- Conversation lifecycle commit 403833c passed native Ubuntu/Windows CI. macOS
  failed native_guardian_early_exit with OS EINVAL while receiving the terminal
  report; Native Rust run 34487513248 remains a failed run. The new business
  lifecycle and HTTP tests passed on macOS too.
- A deterministic Unix socket fixture reproduced the cause: after the peer sends
  complete frames and closes, macOS rejects setting SO_RCVTIMEO before reading
  the still-buffered bytes. The original early-exit timing test passed 100 local
  repetitions, confirming that repetition alone did not cover this ordering.
- Replaced per-read/write socket timeout mutation with nonblocking IO and bounded
  poll readiness. Absolute deadlines, partial-frame age, EOF errors and size caps
  remain enforced. Channel configuration precedes host guardian spawn. A new
  fixture drains two terminal frames after peer exit and then requires EOF.
- The new fixture failed with EINVAL before the fix and passed after it. All 81
  native tests passed locally, zero failed or ignored, plus workspace Clippy and
  rustfmt. All 74 native selectors resolve, and the guardian lifecycle's four
  scenarios plus boundary passed. The extra Unix regression runs under the
  existing early-exit selector; Windows continues its real Job Object scenario.
- This fixes observation of an existing cleanup report. It does not establish
  guardian-death recovery, complete macOS descendant custody or actual native
  Agent execution. Corrected CI is still required; no live service was changed.

## 2026-09-10 — Durable native graphs and unknown execution custody

- Previous head 8aa01af passed Native Rust CI 34488421043 on Ubuntu, macOS and
  Windows, including release builds, and existing Node CI 34488421013.
- Schema 10 creates a finite graph and all canonical node tasks atomically, then
  admits ready immutable peer assignments with pinned dependency results. Exact
  current internal sessions, project generation and canonical task/assignment
  binding govern dispatch. Success requires explicit done for the exact task
  epoch; neither graph terminal state nor model output completes a parent task.
- Creator graph reads/cancellation and node result/dependency commands use the
  private runner API and single writer. Inspected result-only recovery can repeat
  a completed result, even for a terminal graph, without duplicating downstream
  work. Failed results deliberately fence the failed capability; after a lost
  response the creator reads the committed failure. There is no failed-capability
  exemption or runtime inspection endpoint.
- Result values remain outside graph views and assignment payloads. Metadata and
  bounded dependency references avoid repeated 64 KiB values across fan-out.
  Fractional conditions, ordering, skips and failure propagation retain the pure
  policy. Broad Unicode dependency failures remain valid within 4000 bytes.
- The user requested more parallel agents. Independent review reproduced two
  additional gaps: cancelled input exhausted a current member's pending quota,
  and expired shared readers lost resource custody before cancellation. Fixed
  both. All unresolved attempts now retain leases and concurrency slots until
  inspection. Recovery releases only its original leases transactionally, and
  schema 10 restores missing pre-fix unknown leases without reviving inspected
  attempts. Two-reader and late-transaction-failure fixtures verify isolation.
- The quota regression retains 4050 unacknowledged historical/live inputs, proves
  the actual 2000-live-input bound still rejects atomically, and permits new work
  after cancellation. Its initial unoptimized run was interrupted after 176.89
  seconds and is not passing evidence. Transaction-bound per-recipient counting
  and distinct assignee checks reduced the complete test to 4.74 seconds.
- Expanded schema verification initially exceeded SQLite's 64-table join limit.
  Independent verification queries now all prepare before migration commit and
  on reopen. A later failed statement rolls back the entire migration.
- All 94 local native tests passed, zero failed or ignored. Workspace Clippy,
  rustfmt and diff checks passed. All 81 native spec selectors resolve; seven
  graph scenarios plus boundary passed under agent-spec 1.4.0. Corrected CI for
  this new commit remains required. Native runtime protocol work is being
  developed independently and is not included in these counts.
- Actual model execution, graph tool adapters, final reply privacy/delivery,
  Matrix/Palpo transport, sandbox policy and remaining migration gates are open.
  Original dirty checkout and deployed services remain unchanged.


## 2026-09-10 — Native Codex protocol foundation in parallel worktree

- Implemented an independent M4 protocol slice on `feat/rust-runner-protocol`,
  based on committed 8aa01af while the primary worktree closes M3 graphs. Original
  dirty source checkout, active services and credentials were not changed.
- Added `hagency-runtime` with a bounded incremental JSONL codec and an IO-free
  host connection state. Integer/string identity namespaces, duplicate JSON keys,
  handshake order, absolute deadlines, finite pending requests and permanent
  server request tombstones are explicitly checked. No server success/allow
  response is implemented, and runtime text cannot settle work.
- Inspected installed Codex CLI 0.153.4 and exported its JSON schemas using its
  schema generator; committed selected schema snapshots/digests and reusable
  wire vectors. Checked the official App Server protocol documentation. No model
  execution or external-service test was run.
- Nine focused native tests pass, zero failed or ignored. They cover fragmented
  UTF-8, CRLF/coalesced lines, exact/oversized frames, invalid/duplicate fields,
  out-of-order/type-substituted RPC IDs, pending/lifetime limits, resolution before
  request, stale server requests, interruption ACK/error, timeout amid output,
  partial EOF, transport loss and sanitized locally generated errors. Rustfmt and
  focused Clippy pass. The initial Clippy run found one collapsible condition; it
  was corrected, with no suppressed lint or altered assertion.
- The new task contract parses and lints at 100% quality. agent-spec 1.4.0 lifecycle
  ran against `native/hagency-runtime`: all four scenario selectors execute real
  tests (3 + 2 + 2 + 2), and the explicit boundary check passes, 5/5 overall.
- This is protocol preparation only. Runtime IO/write ACK, typed thread/turn and
  approval authority, sandbox observation, process custody, real runtime platform
  qualification, task completion and final delivery remain integration gates.
  The primary agent will review this commit and run the combined workspace suite
  after the graph checkpoint; full old-workspace tests were not repeated here.

- Independent parallel review found no blocking defect in the protocol foundation.
  Added direct coverage for outbound nesting refusal, EOF with a pending server
  request, initialize RPC failure and a valid frame followed by a malformed
  coalesced suffix. Also verified simultaneous host/server requests sharing the
  exact same ID remain separate. All nine expanded tests, focused Clippy and the
  four-scenario-plus-boundary lifecycle pass again; production code is unchanged.

## 2026-09-10 — Integrate the reviewed Codex protocol foundation

- Task graph commit a41ab10 passed Native Rust CI 34506422131 on Windows, macOS
  and Linux, including release builds. Its existing Node CI 34506422137 is still
  running; no uncompleted run is counted as passing.
- Integrated the independent protocol implementation as 13113ad and its review
  regressions as bd0ce65. Append-only coordination conflicts were resolved by
  preserving both the graph work and protocol work. No runtime logic conflicted.
- Combined workspace validation passed 103 tests, zero failed or ignored, plus
  Clippy, rustfmt and diff checks. All 85 native spec selectors resolve. The
  protocol lifecycle passed four scenarios plus boundary in the integrated tree.
- The new crate remains unlinked from native Agent execution. Its observations
  confer no dispatch, approval, task-completion or process-custody authority.
  Actual bounded transport and final-reply privacy/outbox work are proceeding in
  separate worktrees, along with the remaining source inventory classification.

## 2026-09-10 — Bound asynchronous native Codex transport

- Added the next independent M4 slice in the runner worktree: an async driver
  around the existing protocol, taking three host-owned read/write streams. It
  creates no process, channel, background task or public endpoint. Existing
  deployed services and the original dirty checkout remain unchanged.
- The driver pumps stdout/stderr while a complete stdin frame and flush are
  pending. Its write receipt is separate from RPC responses and domain ACK.
  Dropped operation futures synchronously close streams and poison the connection;
  partial/failed output cannot replay. Termination retains unresolved byte/RPC
  progress, including counts cleared internally by a failed protocol operation.
- Added Instant-derived absolute deadlines, cooperative yields under ready IO,
  fixed read buffers, count plus complete-byte event caps and a bounded private
  stderr tail with total bytes. Overflowing actionable events close visibly.
  These byte caps do not claim an equivalent process RSS budget.
- All 17 focused runtime tests pass: nine protocol tests and eight new transport
  tests, zero failed or ignored. Real bounded duplex fixtures cover early RPC
  responses, partial/blocked writes, stalled flush, IO loss, partial/idle EOF,
  future cancellation, absolute deadlines, slow input, event pressure and stderr
  truncation. Most timing uses a deterministic clock; real timer tests also
  verify silence and continuously ready stdout without a producer sleep.
- Focused all-target Clippy, workspace rustfmt and diff checks pass. Initial
  compilation caught overlapping mutable write/flush borrows; the driver now
  polls one output operation per iteration. The initial Clippy run caught a
  constant assertion, which now runs at compile time without a suppressed lint.
- The scoped contract parses and lints at 100% quality. agent-spec 1.4.0 lifecycle
  runs the four selectors against the runtime crate (1 + 2 + 3 + 2 actual tests)
  plus the explicit boundary check, all five passing. ADR-034 records the exact
  guarantees and open integration work.
- Stream closure remains an unresolved execution outcome, requiring future host
  fencing and guardian inspection. It proves no process stop, released resource
  lease, canonical completion or Matrix delivery. The existing guardian's inactive
  stdio launch, actual runtime qualification and permission/sandbox integration
  are unchanged and remain gates before enabling native Agent execution.

## 2026-09-10 — Verify integrated native transport

- Combined protocol head 63b84b4 passed Native CI 34507529088 on all three
  operating systems and Node CI 34507529055. The graph checkpoint's Node CI
  34506422137 also completed successfully after the earlier progress entry.
- Integrated the reviewed host-stream transport as 00155ac. The only conflict
  was append-only progress text; both independent work records are preserved.
- All 111 combined native tests pass, zero failed or ignored. Workspace Clippy,
  rustfmt and diff checks pass. All 89 native selectors resolve, and the transport
  lifecycle passes four scenarios plus boundary in this integrated tree. The
  first lifecycle invocation had an invalid CLI argument layout; the corrected
  invocation used repeated --change options and executed the actual selectors.
- These results validate bounded stream transport, not actual model execution,
  effective sandbox permissions or process cleanup. The parallel agent continues
  with typed thread/turn handling while final-reply custody and source inventory
  work remain under independent review. Deployed services remain unchanged.

## 2026-09-10 — Source-derived native migration entrypoint inventory

- Isolated `feat/rust-inventory` worktree starts at `a41ab10`. The behavioral
  baseline remains `5dbef22`; source evidence pins the expanded snapshot to
  `a41ab10` and hashes every inspected JavaScript source and helper candidate.
  Original dirty checkout, deployed services, credentials and runtime data were
  not changed.
- Replaced regex candidates with deterministic Espree syntax inspection: 199
  Express routes and 3 middleware registrations; 12 custom branches through two
  raw listeners; 32 CLI declarations; 32 root/remote MCP registrations representing
  16 names; and 61 Next proxy rules across four method exports. Dynamic SSE
  installation, delivery delegation and inbound/outbound fleet protocol call
  sites retain exact source locations. Parsing imports no application module.
- Explicitly classified 138 helpers by role, owner, phase, disposition and gate.
  Installed autodeploy watchers and runtime team provisioning are not build-only;
  audit/CD helpers have a dual-use role. The checker refuses unresolved recognized
  registrations, unclassified listeners/helpers, stale module links and snapshot
  drift. Reflection, arbitrary aliases, generated code, external SDK internals
  and a complete shell call graph remain declared detection limits in ADR-035.
- Espree 11.2.0 is a direct pinned devDependency. Only root package/lock metadata
  changed; an isolated `npm ci --offline --ignore-scripts` completed successfully
  with 432 packages. This verifies the lock, not native-addon or production
  installation. Existing root/console development dependencies were linked into
  this worktree for checks; no install wrote through either shared link.
- Exact `tests/native-migration-inventory.test.js`: 3 passed, zero failed/skipped.
  Inventory reproduction, ESLint and diff checks passed. All 533 Node selectors
  resolve. The first binding scan failed due to absent Next dependencies in this
  worktree; after adding the existing console dependency link the full scan passed.
- Agent-spec 1.4.0 parse/lint passed, quality 100%, with advisory warnings.
  Lifecycle invoked with `--layers lint,boundary` still attempted Cargo test
  selectors and returned `passed:false`, `failed:0`, `passed:0`, `skipped:3`,
  `uncertain:0` (exit 1). These are Node/Vitest selectors; this lifecycle remains
  non-passing and is not counted as test evidence. Full output is retained in the
  local migration validation cache as `inventory-lifecycle.json`; actual Vitest
  evidence is `inventory-tests-final.log` and Node bindings are
  `inventory-bindings-final.log` in the 2026-09-10 cache directory.
- Source classification does not finish M0 or establish native parity. Every row
  remains `parity-unverified`. Complete event/schema/feature traceability, exact
  supported runtime versions, qualified hardware/libc and measured budgets,
  terminal/sandbox/guardian behavior, live transport/browser integration,
  packaging, soak and controlled cutover gates remain open. No model or homeserver
  was contacted by this slice.

## 2026-09-10 — Native final-reply intent and private route custody

- Isolated worktree based on a41ab10 adds schema 11 and ADR-033. Fresh Matrix
  sessions freeze host-observed server, account/device, project owner, explicit
  privacy and generations. Legacy sessions remain without delivery authority;
  internal sessions remain internal. The existing internal-kind uniqueness and
  rejoin policy is preserved.
- Full room snapshots retain members and invitation policy. Current negative
  evidence durably retires old routes and internal descendants; unknown/missing
  rooms have an explicit host invalidation command. A newer snapshot through one
  group Agent also fences another Agent that left. Restoring access requires
  renewed evidence and a fresh SID; null-root DM sessions cannot survive promotion.
- The runtime API admits only call ID and bounded final content under exact
  canonical Done-epoch or inspected-report authority. One immutable intent per
  task epoch has content-bound call receipts. No reply marks a task done and no
  runtime command chooses transport identity or asserts external delivery.
- Host claim, begin-send, observed delivery and inspection have distinct durable
  transitions. Lost/unstarted claims can retry; possible sends remain Uncertain.
  Explicit cancellation persists independently, so a later NotSent observation
  cannot revive cancelled output. Replayed inspection binds exact content/fence;
  an actual delivered observation records truth without authorizing another send.
- Parent review found the cancellation-revival and discarded-negative-observation
  gaps before commit; both are closed with restart and two-Agent membership tests.
  The initial new migration fixture reused the original work payload and correctly
  failed inspected recovery; it now uses a distinct report-only instruction. The
  Clippy requested equivalent boolean and Result-return simplifications.
- All 105 workspace tests passed locally, zero failed or ignored, including ten
  final-reply repository tests, private HTTP, migration and internal rejoin cases.
  Existing guardian/crypto/graph tests are included; this worktree does not yet
  include the independently developed Codex protocol crate. Logs are under the
  operator cache path, not the repository. Workspace Clippy and rustfmt pass;
  all 87 native spec selectors resolve and the final-reply lifecycle passes
  six scenarios plus boundary. CI for the combined commit is pending.
- This is an offline domain/API proof. Actual Matrix authentication, task-intent
  route integration, encryption/send/inspection, arbitrary first invited groups,
  non-owner/federated DM policy, taskless/front-desk output and live end-to-end
  tests remain open. No deployed service, credential, model or live room changed.

## 2026-09-10 — Integrate inventory and private final-reply custody

- Integrated source inventory as bf7a47d and final-reply custody as b89c576.
  Only append-only coordination files conflicted; all independent records were
  retained. The native README now states the current checkpoint and reply API.
- Combined native workspace: 122 passed, zero failed or ignored, plus Clippy and
  rustfmt. All 95 native selectors resolve. The integrated final-reply lifecycle
  passes six scenarios plus boundary, with zero skipped or uncertain results.
- Exact inventory Vitest: three passed, zero failed/skipped. Inventory reproduction,
  ESLint, diff and README link checks pass. All 533 Node selectors resolve. Its
  Cargo-only lifecycle limitation remains non-passing as recorded above; the
  successful Vitest run is the applicable test evidence.
- Prior transport head c1daf95 passed Native CI 34509445157 on all three OSes.
  New integrated CI is still required. No live runtime or Matrix state was touched.
- Parallel work continues on verified Matrix ingress/task-intent integration,
  Codex one-turn session handling and durable outbound custody. Machine-token
  rotation is separate from AS registration replacement; already received work
  must survive the former. No migration phase is declared complete by this check.
## 2026-09-10 — M4 typed Codex session slice (ADR-036)

Added a one-turn host-owned SessionDriver over the bounded protocol/transport.
Fixed workspace-write/on-request/user settings (optional read-only), bounded
absolute cwd/model/effort, exact resumed-thread/returned-turn correlation and
item tombstones now gate lifecycle observations. Early notifications are bounded
and scoped before activity is applied. Text and final separators count toward
explicit limits; server requests return unsupported errors. Cancellation, EOF
and timeout remain unknown execution, without task, lease or process transitions.

Root review identified inconsistent terminal-suffix handling. The wrapper now
validates received deferred/transport data consistently and finishes an already-
started trailing frame under an absolute bound. Every byte split of a normal
completed-plus-idle stream and completion/idle before interrupt ACK are covered;
wrong-scope, unsupported and unfinished suffixes fail visibly.

Focused verification in the isolated runner-protocol worktree: all 34 runtime
tests passed (9 protocol, 8 transport, 17 session), runtime all-target Clippy with
warnings denied passed, workspace formatting and diff checks passed. The fixture
uses the existing local Codex 0.153.4 schema export and records source hashes;
no model or service was started. Agent-spec 1.4 lifecycle passed all 4 scenarios
plus the explicit 13-file boundary check (5/5, quality 100%, zero fail/skip/uncertain);
its selectors ran 3 settings, 4 identity, 4 item and 6 outcome tests. Native execution
stays disabled until actual guardian/stdio,
input acknowledgement, sandbox, host authority and durable-domain gates close.

## 2026-09-10 — Native execution permission scope port

- Integrated the reviewed typed Codex session as 7a16b86, then added ADR-039's
  pure permission-scope and YOLO policy validation. Scope derivation never applies
  an owner decision or changes runtime permissions. Exact commands, structured
  network hosts and explicit permission profiles bind workspace/write/environment.
- Sixty-four vectors are generated from the existing pure JavaScript module with
  both standard path flavors. Native entry ordering is deliberately deterministic
  UTF-16 rather than locale-dependent; unsupported Windows device/root-relative
  paths remain once/deny-only. Metadata, description and array limits fail closed.
- All 143 combined native tests pass, zero failed or ignored. Workspace Clippy,
  rustfmt, diff and vector/ESLint checks pass; all 103 native selectors resolve.
  Logs: combined-session-execution-{tests,clippy}.log and bindings.json in the
  2026-09-10 local migration validation cache. Execution scope lifecycle passes
  four scenarios plus the 12-file boundary, quality 100%, zero fail/skip/uncertain.
- Integrated prior head 6906851 passed Native CI 34511063114 on all three OSes
  and Node CI 34511063106. New combined head still requires CI.
- Native grant persistence, exact private owner decisions, binding/task epochs,
  runner responses, YOLO dispatch and effective sandbox remain implementation
  work. The parallel guardian-owned stdio, verified ingress and outbound custody
  slices continue; production runtimes and live Matrix state remain unchanged.

## 2026-09-10 — Native outbound custody lifecycle

- Separate `feat/rust-outbound-custody` worktree starts at `bf7a47d`. Custody
  schema 2 extends the existing repository/worker; domain schema, original dirty
  checkout and services were not changed. Palpo source was read-only at
  `c7c400e04ab05479a63c30f14679ec0180457d85`; no live version claim is made.
- Host-only activation pins canonical side/fleet/registration identity and refuses
  fixture adoption or another binding namespace for the same fleet. A stable
  UUID consumer survives machine rotation. Opaque scopes/tickets have no
  Deserialize, Serialize or Debug projection. Machine and Matrix registration
  generations stay separate, including the delivery's preserved origin.
- Receive commits the full JSON transaction and content-bound receipt before ACK.
  Current poll and exact lease tickets reject replaced/stale responses. Retirement
  of an uncertain machine lease is explicit; rotation never invents a successful
  remote ACK. Already owned Matrix/request work stays recoverable. Old probe
  processing and publication authority is fenced.
- Claims/start/result receipts commit atomically. Startup/expiry retire unstarted
  claims, while started or explicitly uncertain work requires host inspection.
  Matrix order is preserved and the separate work lane continues. Completion
  compacts only payload, preserving arbitrary-ID dedup and exact result receipts.
  No domain task or request approval is inferred from transport completion.
- The one-slot publication outbox preserves sequence, bytes and original
  observedAt timestamps across lost/rejected responses. Exact acceptance receipts
  cannot clear a newer body. Current probe publication requires an equal completed
  work-probe result from the exact machine generation. The later Matrix adapter
  must also suppress embedded old-generation probe evidence in retained full
  transactions; no authenticated source/event or connection proof is fabricated.
- Shared JavaScript vectors verify opaque full transactions, prototype-named data,
  fractional/exponent numbers, Unicode and numeric keys. Existing signed DTO and
  execution-payload encoders remain strict. Queue accounting covers serialized
  payload plus escaped envelopes and maximum lease tokens; stored payload/result/
  publication bytes share the 16 MiB bound. Finite records/attempts never evict
  pending work or historical dedup markers for capacity.
- Independent review found that the original queued command reused its enqueue
  clock. The writer now advances a host-clock anchor by monotonic elapsed time,
  rounding up milliseconds. A controlled paused-writer regression proves queued
  Start and Complete cannot use authority that expired while queued.
- Ten focused outbound tests passed. All 74 core/store tests passed, zero failed
  or ignored; this includes existing custody and domain regressions. Clippy with
  warnings denied, rustfmt and diff checks passed. All 95 native selectors on this
  base resolve. Agent-spec 1.4.0 parse/lint scored 100%; exact lifecycle passed
  six scenarios plus the explicit boundary (7 pass, 0 fail/skip/uncertain).
  Output is retained in the local 2026-09-10 migration validation cache as
  `outbound-tests.log`, `outbound-full-tests.log`, `outbound-bindings.log` and
  `outbound-lifecycle.json`. Cross-platform CI for this slice remains required.
- ADR-037 records the host boundary and next gates: actual HTTPS/wire validation,
  credential ownership, Matrix source/membership proof, domain handoff, bounded
  status observation and network loops. This task added no client crate, ran no
  model/homeserver/browser and made no deployment. Continuous retention beyond
  finite custody capacity remains a release gate.
## 2026-09-10 — Verified Matrix input to canonical task integration

- Isolated worktree based on b89c576 adds schema 12 and ADR-038. Host-only typed
  observations bind current full Matrix identity and generations; runtime HTTP
  cannot choose input target, wake policy or transport truth. Legacy intake stays
  fenced from verified sessions.
- Current joined-human and full-MXID mention evidence derives wake. Direct main
  stays null-root without @; Agent/service messages remain background. Independent
  session/task copies retain exact original source SIDs, body and scope receipts.
  Task creation, dormant input, ACK and request dedup commit atomically.
- Actual ACK observation activates canonical work; frozen dispatch and original-
  human follow-up carry the same task through canonical epochs to final intent.
  Repository and HTTP fixtures now begin with admitted input instead of manually
  creating the task. Wrong device/sender/content, runtime forged fields, promoted
  null-root DM, retired parent, allocation/device rotation, missing legacy
  boundaries, transaction failure and restart have deterministic coverage.
- Review retained the live notice adapter as an explicit unimplemented gate:
  claim/reclaim alone lacks begin-send and uncertain-send custody. A later rejected
  ACK cannot undo a delayed private send. ADR/spec and API comments require current
  route validation, cancellation and durable uncertainty before any live adapter.
  Self-review also added parent provenance for pre-resolved explicit threads.
- Validation found three existing migration expectations still naming schema 11;
  they now assert 12. New HTTP fixtures initially used the wrong App constructor
  arguments and response nesting, and one follow-up fixture expected epoch 2 after
  a second Done although the canonical result is 3. These fixture failures are
  preserved in cache logs. Clippy requested a grouped projection argument and
  removal of redundant borrows in the shared intent persistence helper.
- The full native workspace passed 131 tests with zero failures or ignored tests;
  workspace Clippy, rustfmt, task parse/lint and final lifecycle pass. All seven
  scenarios plus boundary passed with zero skips/uncertain results, and all 102
  native spec selectors resolve. The final workspace rerun after helper cleanup
  also passed 131 tests. Logs live under the operator cache, outside the repo.
- No model, live Matrix service, credential or deployment changed. Real sync/crypto
  and transport, generalized invitation/DM policy, taskless/front-desk output,
  automatic unread discussion-window selection and migration cutover remain open.

## 2026-09-10 — Windows execution-vector reproducibility

- Head 8ba0590 passed Node CI 34513475238 but failed the Windows native vector
  check in 34513475271. The legacy source is checked out with CRLF there; its raw
  source hash differed while the permission vectors were unchanged. The generator
  now hashes canonical LF source text. Native JSON fixture bytes remain LF.
- Both ordinary input and an in-memory simulated CRLF checkout reproduce all 64
  vectors locally; ESLint and diff checks pass. The next Windows CI run must
  verify the fix; this local simulation is not substituted for that run.
- Outbound custody is integrated as ffaad85 and verified ingress as 7eabc42.
  Only append-only coordination records conflicted; both histories were retained.
  Their combined full-workspace verification is still in progress alongside the
  native task-client integration. Deployed services remain unchanged.

## 2026-09-10 — Native task maintenance CLI

- Added task get/start/heartbeat/wait/resume/done/comment to the native executable.
  An inherited exact runner context chooses one task and loopback socket; mutation
  call IDs go to the existing scoped API and sole canonical writer. Credentials
  never enter CLI arguments, configuration or model-visible output.
- Actual local Salvo/SQLite fixtures and the native executable verify lifecycle,
  identical retries, changed-content conflict, wrong task/secret/fence, parked
  rejection and sanitized failure. Controlled peers verify redirects, excess
  headers/chunked data, partial EOF and stalled body cancellation without retry.
- Four task-client tests and workspace Clippy pass. Only direct dependency entries
  for already-locked Hyper/Hyper-util/http-body-util were added; no package version
  changed. Combined ingress/outbound/client validation passed 166 native tests
  with zero failures or ignored tests; all 120 Rust spec selectors resolve.
  Integrated agent-spec lifecycle passed task client 5/5, outbound custody 7/7
  and verified ingress 8/8, including explicit file boundaries and no skips or
  uncertain results. Logs are in the 2026-09-10 migration cache. Windows CI for
  the line-ending correction remains pending the next push.
- Host environment provisioning and full legacy/MCP helper parity are not enabled
  by this CLI. The production service and original checkout remain unchanged.

## 2026-09-10 — M4 owned child stdio integration (ADR-040)

Added SupervisedProcess::spawn_piped and one-use StdioPipes through the existing
guardian prepare/start and retained child-identity launcher. Exactly three
checked endpoints cross the private socket before Start. OwnedSession consumes
native Tokio Unix pipe adapters and retains process custody through completion,
EOF, timeout and dropped-operation cancellation. Protocol outcome and exact
platform termination report remain separate. Its bounded synchronous stop can
block an execution worker; nonblocking server orchestration is not claimed.

The offline native fixture now traverses initialize, initialized, thread/start,
turn/start, streamed item text and completion over actual child pipes. It also
exercises a failed executable, silent and closed output, a cancelled partially
written request, 256 KiB stderr pressure, a child surviving stream closure, and
an independently observed descendant. It never launches a model or live service.

Adversarial descriptor tests exposed a real macOS truncation hazard during
development: the original ancillary header can exceed copied control bytes and
the kernel can install descriptor IDs omitted by truncation. The receiver now
uses a source-bound 4 KiB buffer and exits its disposable pre-start process on
macOS truncation; a subprocess test verifies exit 125 and closure of undisclosed
rights. Parser traversal and payload lengths are bounded before reads. Test
pipes were also sealed to avoid unrelated concurrent fixture inheritance.
Parent review requested the defensive control-length clamp and it is included.

A final test pass exposed an overly strong stderr fixture expectation: terminal
stdout can close streams while a final chunk remains in the kernel. The test
now checks observed drainage and exact bounded tail content, without equating
produced bytes with observed bytes. Descendant markers were separated and must
show activity before protocol completion, removing a weak shared-file assertion.

Focused platform/runtime verification passed 59 tests locally on macOS 26.5 /
Darwin 25.5.0 (19 platform and 40 runtime), zero failed or ignored. Linux-specific
subreaper/ancillary tests and Windows refusal still require their CI hosts.
Windows cancellable piped IO remains Unsupported; atomic job launching is
unchanged. POSIX crash-containment refusal and macOS incomplete descendant
reports remain intact. Real sandbox/runtime qualification, guardian-death
recovery, authenticated dispatch and durable task/lease settlement remain open.
Native execution stays disabled; no migration phase is declared complete.

All-target focused Clippy with warnings denied, workspace formatting and diff
checks passed. Agent-spec 1.4 lifecycle passed all four bound scenarios plus the
explicit 20-file boundary check (5/5, quality 98%, zero fail/skip/uncertain).
Selectors ran 1 lifecycle, 1 admission, 2 IO-failure and 2 custody tests; platform
ancillary unit evidence comes from the separate focused platform test run.
Logs and the exact lifecycle command are in the operator migration cache outside
the repository. No changes were pushed, merged or deployed from this worktree.

## 2026-09-10 — Combined native client and owned runner verification

- Integrated native task CLI as dbf3162 and owned child IO as 1609289. Review
  confirmed exact framed guardian reads do not consume the ancillary marker;
  disposable pre-start macOS failure handling and bounded owner stop remain
  explicit. Append-only coordination conflicts preserve both histories.
- All 176 native workspace tests passed, zero failed or ignored. Workspace
  Clippy, rustfmt and diff checks passed; all 124 native selectors resolve.
  Integrated owned-IO lifecycle passed 5/5 with zero skips or uncertainty.
  Combined logs are in the migration cache as combined-owned-cli-* and
  combined-owned-io-lifecycle.json. Cross-platform CI is pending this push.
- Parallel work continues on Windows cancellable pipes, outbound HTTPS and
  persisted owner approvals. No deployed service or original checkout changed.

## 2026-09-10 — Durable private owner decisions and exact approval application

- Isolated worktree based on 7eabc42 adds schema 13 and ADR-043. Full shared
  project-owner approval-room observations derive private authority; per-Agent
  incarnations remain separate. Current negative evidence, including a conflicting
  same-generation snapshot, retires all old room bindings and grants. Positive
  replay cannot restore them. Project/owner/registration and allocation changes
  revoke old grants durably.
- Host context pins current capability, verified Matrix session, canonical task
  epoch, exact native connection/thread/turn/item and leased workspace. Request
  admission and parking commit together. Core execution scope derivation supplies
  exact reusable command/network/profile candidates; unknown writable requests
  retain once/deny. Read-only escalation is limited to network-only scope and YOLO
  is refused pending operator policy plus actual sandbox/runtime integration.
- Structured encrypted owner verdicts commit content-bound receipts and grants
  atomically. Task grants stop at completion/epoch change; Always grants survive a
  clean dispatch/restart only under the same Agent/private/workspace context.
  Explicit revocation after decision but before consumption prevents another allow.
- Consumption persists Applying once before returning host application data. Lost
  response/restart remains Uncertain, and neither NotApplied nor expired authority
  can rearm it. Exact observed application may record old truth without resuming
  an expired dispatch. Every unresolved approval blocks the generic resume path.
  Public/console summaries omit private room, owner, workspace, source and payload.
- Parent review found the same-generation negative-evidence gap; a regression now
  covers B invalidating A's decided reusable request, old safe replay and fresh
  generation restoration. Additional fixtures cover shared-room cross-project
  refusal, exact ID types, wrong owner/scope/capability, multiple requests, injected
  rollback, pending limits, private projections, clean grant restart and schema
  upgrade. One existing schema assertion still expected 12 and now asserts 13.
- Final full native workspace: 172 tests passed, zero failed or ignored, including
  ten approval test groups. Workspace Clippy, rustfmt, spec parse/lint and lifecycle
  pass; all six scenarios plus boundary passed with zero skipped/uncertain results.
  All 122 native spec selectors resolve. Logs live in the operator cache outside
  the repository; the earlier fixture/compiler and schema-expectation failures
  remain in their original logs.
- No live Matrix/model/runtime, credential or deployed service changed. Native
  decision wiring, application inspection adapters, Matrix cards/verdict crypto,
  operator YOLO policy, general owner rebinding and M6/M9 operational gates remain
  open. Task-notice sending retains its separate custody gate from ADR-038.

- Windows follow-up: the schema reconstruction fixture sliced at a literal blank
  line before CREATE TABLE, which failed under CRLF checkout. It now locates SQL
  statement text independently of line endings. An in-memory fixture reconstructs
  and prepares the route view under both LF and CRLF; focused test and Clippy pass.

## 2026-09-10 — M5 bounded outbound HTTP client, isolated worktree

- `feat/rust-outbound-http` starts at integrated custody commit `ffaad85` in its
  own clean worktree. Added `hagency-palpo` with immutable host configuration,
  pinned verified reqwest/rustls HTTPS, explicit bearer/generation, disabled
  proxies/redirects/protocol retries and finite DNS/concurrency/header/body/JSON/
  deadline/backoff budgets. No deployed service or original checkout changed.
- Matrix/work polling persists full payload before exact ACK. Cancellation,
  delayed/lost responses and rotation retain work plus explicit uncertainty.
  Host-only AckHead resumes current unconfirmed leases even for done redelivery
  tombstones, using existing schema-2 columns. Neither custody nor domain schema
  changes. FIFO Matrix consumption does not block work or frozen publication.
- Publication uses Palpo's actual `/updates` path and exact persisted v2 bytes,
  sequence and observedAt. Machine rotation fences old in-flight responses and
  credentials; old locally owned work retains its original generation. A simple
  heartbeat never manufactures Matrix readiness, membership or approval proof.
- Thirteen actual local HTTP/TLS fixtures passed, including TLS/hostname failure,
  environment proxies, redirects/status redaction, malformed/duplicate/coalesced/
  oversized JSON, slow headers/body/ACK, exact ACK-before-consumer ordering,
  response-loss/restart, done-tombstone ACK, changed replay, stale leases, token
  rotation, unknown attempts and independent cancellable lanes/backoff.
- The first fixture run exposed two incorrect test assumptions: retry Claim
  returns its original unknown capability, whose Start remains refused; a server
  sending ACK does not mean the client committed it before cancellation. Tests
  now explicitly verify these boundaries; no production guard was weakened.
- Additional reference check executed the pinned read-only Palpo commit
  `c7c400e04ab05479a63c30f14679ec0180457d85`'s real createApp/Outbound with only an
  in-memory database and forbidden homeserver I/O. The native client completed
  both lanes' ACK, full payload handoff, empty poll and frozen v1 status in a v2
  update. Palpo preserved old observedAt and remained pending_connection.
- Core/store/client integration passed 101 tests, zero failed/ignored (plus one
  isolated proxy-environment child invocation). All 114 native selectors resolve.
  Clippy with warnings denied, rustfmt and diff checks passed. Agent-spec 1.4.0
  parse/lint scored 100%; scoped lifecycle passed five scenarios plus boundary,
  six pass and zero fail/skip/uncertain. Evidence logs in the local 2026-09-10
  migration cache use prefix `outbound-http-`: fixtures-final, all-tests, clippy,
  bindings, lifecycle and palpo-reference.
- Initial offline dependency resolution downgraded seven uncached WASM/Hermit
  target packages; those baseline blocks were restored. Matching new
  wasm-bindgen-futures 0.4.78 comes from cached registry metadata. Every existing
  lock version/checksum remains unchanged and `cargo check --locked --offline`
  passes. New dependencies are scoped to the client/TLS fixture; no broad upgrade.
- ADR-042 and the crate README state remaining gates: secure host configuration
  persistence, current domain catalog/status observation, authenticated Matrix
  SDK event/member proof, idempotent domain handoff, Agent retirement endpoint,
  continuous retention/capacity maintenance, platform CI and live UX. This does
  not declare M5 or the full migration complete. No model or deployment was run.

## 2026-09-10 — Task notice send custody and integrated outbound/approval checks

- Integrated private owner decisions as 36366bb and outbound HTTP as c52a97c.
  Root reviewed exact ACK recovery, TLS/configuration bounds, independent lanes,
  scope matching and one-shot approval application. Append-only coordination
  conflicts preserve both histories; existing lock packages remain pinned.
- Added schema14 and ADR045: verified notices commit Sending once before host
  send data is returned, freeze the canonical epoch/source, and retain possible
  sends as Uncertain. Promotion, revocation and explicit cancellation fence old
  output; late delivery can record history without activating retired work.
  Exact fenced inspection receipts and rollback protect activation. Generic
  legacy failure/retry cannot bypass this path. Older development notices are
  conservatively fenced because their original epoch/send-start was not recorded.
- Four new repository test groups cover direct/group activation, one-shot begin,
  rollback, wrong secrets, expiry/restart, explicit cancellation, late delivery,
  inspection conflict, epoch change and old-schema migration. Existing verified
  ingress and actual scoped HTTP fixtures now call begin before delivery.
- First combined run exposed one stale graph schema expectation (13 versus 14);
  it was corrected. Final combined workspace passed 204 tests, zero failures or
  ignored tests, plus the HTTP proxy fixture's isolated child invocation.
  Workspace Clippy/fmt pass; all 139 native selectors resolve. Logs use prefix
  combined-notice-http-approval-* in the local migration validation cache.
  Integrated lifecycle passed notice custody 5/5, owner approvals 7/7 and
  outbound HTTP 6/6, all with explicit boundaries and zero skipped/uncertain.
- Native CI d66b4e9 (34516408230) passed Linux and macOS. Windows passed the
  corrected execution-vector check, then exposed CRLF SQL fixture slicing and
  missing SystemRoot in the isolated task-client process. The fixture fixes are
  committed as 5c3d764 and 45f4a62; actual Windows rerun remains required.
- Actual Matrix sends, crypto/recipient handling, runtime approval application
  and Windows owned pipes remain in progress. No deployed service, credential
  or original dirty checkout changed; no migration phase is declared complete.

## 2026-09-10 — M4 Windows owned stdio adapter (ADR-044)

Added Windows local self-connected pipes with owner-only access and exact child
handle inheritance through the existing atomic Job Object launcher. Host endpoints
are overlapped and non-inheritable. OwnedSession now shares its original guards
between platforms through an owned/session.rs extraction; its Unix conversion and
guardian path are retained. Incomplete stop observations remain explicit and can
be retried. Native service execution stays disabled.

Local dependency inspection found that Tokio/Mio flush and drop behavior cannot
establish the required write/cancel boundary. A private platform adapter now
bounds each write, waits for the pinned Mio previous-write completion check, and
disconnects plus requests cancellation of every pending operation before drop.
No blocking IO reader or alternate child launcher was added. ADR-044 records
source versions/hashes and the implementation dependency that future upgrades
must requalify. Complete write still does not establish domain acknowledgement.

Windows fixtures now use actual child IO for the full offline Codex lifecycle,
partial writes, EOF, silence, stderr pressure, ordinary and new-group descendants.
Separate tests exercise exact handle exclusion, job membership before input,
blocked single-write completion, disconnecting pending IO, denied breakaway and
owner exit without Drop. Shared test cleanup keeps true platform guarantees.
No model, external service, runtime token or deployed process was touched.

A shared fixture regression was caught locally: allowing the Unix child to read
a small prefix freed enough anonymous-pipe capacity for the entire request, so
the timeout moved from write to response. Prefix consumption is now Windows-only,
where confirmed write counts may remain zero despite partial delivery. The Unix
fixture retains its original blocked-write proof. Cross-target Clippy also caught
a Windows-only unused fixture assignment; it was removed without suppressing lint.

All 59 platform/runtime tests passed on macOS, zero failed or ignored. All-target
Clippy with warnings denied passed for Windows GNU and Linux GNU cross-targets;
cross-compilation does not execute the Windows fixtures. Real Windows CI remains
an open qualification gate, as do effective sandbox/runtime tests, POSIX guardian
loss, macOS complete detached-child proof and authenticated dispatch/approval
integration. This code does not close M4 or advertise runtime availability.

Final source review found upstream Mio #1944: reactor teardown can abandon late
pipe completions and retain handles. With coordinator agreement, Windows pipes
now register on one private process-lifetime completion reactor with one fixed
worker. It exposes no arbitrary spawn or command API. A Windows fixture cycles
32 short-lived caller runtimes, cancels pending reads, observes disconnect/EOF,
and checks actual process handle counts. This adds explicit completion custody
without another process launcher or blocking pipe reader; Windows execution of
the fixture remains pending CI.

Final macOS Clippy, Windows cross-target Clippy, formatting and diff checks pass.
Agent-spec 1.4 lifecycle passes four shared scenarios and the explicit 18-file
boundary (5/5, quality 97%, zero fail/skip/uncertain). That lifecycle executed six
shared runtime tests on macOS; it did not execute Windows-only scenarios.
Exact commands and logs are saved under the operator migration cache's
codex-protocol/windows-io-* paths. No push, merge or deployment was performed.

## 2026-09-10 — Integrated Windows IO review and three-OS regression results

Integrated ADR-044 as 4c0de90. The combined platform/runtime suite passed all
59 macOS tests; workspace Clippy, formatting and diff checks passed. Integrated
agent-spec lifecycle passed 5/5 including the explicit file boundary, with zero
fail/skip/uncertain. Its six selected shared tests ran on macOS; Windows-specific
execution is still pending the new CI run.

Previous integrated head 47b6a51 passed Native CI 34518706774 on Windows 2025,
macOS 15 and Ubuntu 24.04. This verifies the SystemRoot task-client fixture fix
and CRLF migration-fixture correction on actual Windows. That run predates the
new Windows IO code. Local full-workspace evidence at 47b6a51 remains 204 unique
tests plus the isolated proxy-environment child check. Node CI 34518706889 was
still running when this entry was recorded. Logs remain in the private operator
migration cache. No live service or original dirty checkout changed.

## 2026-09-10 — Native MCP assigned-task helper (ADR-049)

Added an executable stdio MCP helper using the existing task-client transport
and sole canonical writer. Its five tools preserve exact task/capability scope
and stable mutation call IDs. Native fixtures exercise lifecycle, task transitions,
replay/conflict, wrong task, wrong secret, parked authority, bounded malformed
frames, duplicate IDs, EOF and whole-helper IO deadlines. The actual pinned
rmcp 1.8 client initializes a native child and reads/mutates a fresh API task.

The first SDK run exposed its automatic _meta progress data on tools/list; the
adapter now accepts bounded metadata without deriving authority. Independent
review caught unknown tools being represented as execution errors; they and
invalid call envelopes now return JSON-RPC -32602 while leaving the connection
usable. Added independent connection replay coverage. The watchdog was confined
to the binary's private module; a library caller cannot invoke process exit.

Initial compilation lacked the test-only Tokio process feature; it is explicit
now. Offline dependency resolution tried unrelated WASM/Hermit downgrades; all
preexisting package versions were restored before locked verification. Eight
focused task-client/MCP tests passed before the final fresh-connection addition;
final checks and lifecycle are recorded below after completion. No live or
generated MCP configuration changed.

Final focused verification passed all eight MCP/task-client tests, and the added
fresh-connection replay/conflict test passed separately. Workspace Clippy with
warnings denied, formatting and diff checks passed. Agent-spec 1.4 lifecycle
passed all four scenarios plus the explicit 17-file boundary (5/5), zero
fail/skip/uncertain. Review independently exercised the rebuilt executable's
protocol-error recovery. SDK remains pinned as a dev dependency; native packaging
has no SDK requirement. New three-OS execution remains a CI gate.

## 2026-09-10 — Diagnose macOS child identity CI observation failure

Native CI 34519683281 failed `native_child_identity_expiry` at `68af16c` with
the fixture's "unrelated child was signalled" message. Its only evidence was no
heartbeat change during a fixed 80 ms sleep; it never checked child exit status.
The test finished within 0.84 seconds, below the probe's eight-second lifetime.
Tracing both rejected operations confirms they return before the native signal
call. The saved CI output cannot retrospectively establish the child's status.

A bounded, host-controlled native heartbeat pause now reproduces the unchanged
sample without any signal and verifies that the child is alive. The fixture
waits at most three seconds for fresh progress and checks the retained Child for
actual exit before accepting it. A killed-child negative case still fails even
with old heartbeat bytes present. Existing expiry, unrelated-survival and
generation assertions remain; production signal identity code is unchanged.
This was isolated from the unfinished Linux cgroup worktree.

Verification: all 20 macOS platform tests passed, including four child identity
tests; Clippy with warnings denied, formatting and diff checks passed. Agent-spec
1.4 lifecycle passed four scenarios and the five-file boundary (5/5, quality 93%,
zero fail/skip/uncertain). Evidence is in the operator cache at
`codex-protocol/child-progress-tests.log` and `child-progress-lifecycle.json`.
This local result is not a rerun of the failed GitHub macOS job.

### 2026-09-10 — M6 Codex approval protocol seam (ADR-046)

- Added opt-in typed command/file/permission approval events and exact once/deny
  responses while retaining default refusal. Correlation keeps request ID type,
  thread/turn/item and original content; consumed responses cannot be resent.
- Added `hagency-permissions` as the host coordinator over the unchanged schema13
  writer and independent runtime crate. It derives session execution context,
  proves current resource/owner authority, parks before approval and persists
  Applying before a response-byte attempt. No runtime payload supplies host
  identity, workspace lease, owner verdict, reusable grant or application truth.
- Audited official Codex 0.153.4 source at commit 3d2ee51: all three callbacks emit
  resolved before typed response parsing/core submission, including cancellation.
  The adapter therefore records uncertainty and never marks Applied or resumes
  dispatch from flush/resolution/model text. Live native approval remains gated
  on evidence after core application, not merely response delivery.
- Regression fixtures use real SQLite and fake bounded streams. They assert
  Applying in the writer before its first byte; cover allow/deny/profile shapes,
  default refusal, exact numeric/string IDs, stale private/device/lease authority,
  revoked grant becoming deny, DB rollback, 16 pending limit, multiple approvals,
  cancellation, timeout, EOF and durable restart. Early fixture model/opaque-ID
  mistakes were corrected; boxed fixture futures avoid test-stack overflow
  without changing stack settings. No production store rule was loosened.
- Final verification: 197 full native tests and 51 focused runtime/permissions
  tests pass, with zero failures or ignored tests. Clippy, rustfmt and diff
  whitespace checks pass. Task lifecycle has four passing scenarios plus the
  passing explicit boundary, zero skip/uncertain/pending-review; 134 native spec
  selectors resolve. Logs are in the external operator cache as
  `codex-approval-{workspace,tests,clippy,lifecycle-final,inventory}.log`.
- Review-added cancellation tests hold a SQLite write lock, poll attach/consume
  once, release the lock and confirm durable commit without repolling the caller.
  Dropping that future closes the channel; lost consumed decisions emit no bytes,
  cannot resend and reopen as Uncertain. The initial lifecycle boundary failure
  was the root-manifest path spelling; explicit `./Cargo.toml` and `./Cargo.lock`
  now match the already authorized files.

### 2026-09-10 — Codex approval one-response follow-up

- Generic server-request rejection now refuses a request that already emitted its
  typed approval response. Its original pending correlation remains intact until
  upstream resolution; a second typed response is likewise refused.
- A real Connection regression covers allow, rejected generic/typed duplicates,
  and accepted original resolution. Focused approval tests, runtime Clippy,
  rustfmt and task lifecycle pass (four scenarios plus boundary, no skips).
  Evidence: `codex-approval-one-response{,-clippy,-lifecycle}.log` in the external cache.

## 2026-09-10 — Integrated MCP, approval and child-observation checkpoint

Integrated native MCP as 363db02, the macOS fixture correction as 795ae94 and
Codex approval coordination as e5b4239. Full workspace verification passed 220
unique tests, zero failed or ignored, plus the isolated proxy-environment child
check (221 printed passes). Workspace Clippy with warnings denied, formatting
and diff checks passed; all 152 native spec selectors resolve. Integrated
approval and child-identity lifecycles each passed 5/5 with explicit boundaries
and zero fail/skip/uncertain.

Review found a connection-level duplicate response path: generic rejection could
follow a typed approval while correlation was retained. ec80da5 fences that
path without dropping pending resolution identity. Its three focused approval
tests passed; the writer/coordinator cannot resend a consumed approval.

Previous 68af16c passed Windows and Linux Native CI 34519683281, including actual
Windows owned-pipe cancellation/handle and child fixtures. macOS failed the old
80 ms heartbeat-only observation; the corrected fixture is now integrated and
new CI remains required. Node CI 34519683353 passed. The complete earlier
47b6a51 native three-OS and Node runs were green.

The parallel Matrix collector review identified shared negative snapshot loss,
lost positive-observation response fencing, SDK shutdown errors and plaintext
custom SDK values; fixes are still in its isolated worktree. Linux cgroup and
Matrix formatting also remain separate. This is a development checkpoint, not
completion of a migration phase. Original checkout and deployed processes are
unchanged.


### 2026-09-10 — Native Matrix account/device observations and global negative fencing

- Isolated branch `feat/rust-matrix-transport` starts at schema14 commit
  `47b6a51`. Task contract `task-rust-matrix-transport` and accepted ADR-047 govern
  this slice; no original checkout, deployed service, live account or key changed.
- Added `hagency-matrix`: actual pinned HTTPS whoami, bounded sync and authenticated
  full-room state feed the existing host domain observations. It uses protected
  encrypted SDK state/crypto stores and one owned worker; there is no message send,
  approval application or event admission. Exact account/device/registration,
  TLS origin and host room/generation intents remain separate from Palpo tokens.
- Domain schema15 persists exact transport unavailability, atomically retires old
  routes/own grants and retains possible final/notice sends as uncertain. Fresh
  generation is required after failure; stale negative evidence cannot revoke a
  newer incarnation. Migration fixtures preserve schema14 and reject partial or
  missing structure. Shared room failure also retires other Agents' old routes.
- Tests found and closed SDK custom-value plaintext storage and a fixture teardown
  race. Journal encryption is explicit; the pending fixture now writes through
  the owned worker. Interrupted sync retains its full encrypted original response
  and cannot automatically replay. Bounds reject capacity rather than pruning
  received work or dedup receipts. Database rollback and queued timeout tests
  preserve original receipts, keys and exclusive ownership.
- Parent review found three additional boundaries, all closed with regressions:
  shared unsafe snapshots must reach shared-room invalidation, a lost positive
  response must fence the attempted incarnation too, and SDK close errors must
  never report successful shutdown. The close acknowledgement follows runtime
  termination and filesystem lock release.
- Local validation: 225 workspace tests in 45 binaries pass, zero failed/ignored;
  146 native selectors all bind; workspace Clippy `-D warnings`, fmt and diff
  checks pass. Agent-spec 1.4 lifecycle is 8/8 with explicit changed boundaries,
  zero skipped/uncertain. Evidence is under the 2026-09-10 migration cache with
  `matrix-*` prefixes. Existing offline cross-signed crypto fixture also passes.
- Remaining gates are explicit: no live provisioning/key publication/cross-signing,
  Matrix event provenance or sends, pending-SDK inspection recovery, continuous
  retention beyond 64 sync receipts, changing the pinned room set, deployment UX
  or production cutover. If negative persistence itself is unavailable/unknown,
  a future live host must stop using that incarnation; a failed database cannot
  promise immediate retirement. This bounded slice does not complete M5.


### 2026-09-10 — Integrated Matrix collector and three-platform MCP/approval CI

Integrated Matrix transport a2c5621 as d898211, preserving the current Palpo,
permissions and Matrix workspace members without baseline dependency upgrades.
The combined workspace passed 241 unique tests plus the isolated proxy child
check (242 printed passes), zero failed/ignored. Clippy with warnings denied and
all 159 native selector bindings pass. Integrated Matrix lifecycle passes 8/8
with all 31 changed paths bound and zero fail/skip/uncertain. Evidence:
`combined-matrix-mcp-approval-*` and `integrated-matrix-lifecycle.*` in the
2026-09-10 migration cache.

At prior head 911f1f5, Native CI 34522180950 passed on actual Linux, macOS and
Windows runners, and Node CI 34522180924 passed. This closes the prior macOS
child-progress fixture failure and qualifies the MCP/approval integration on
those runners. Matrix collector CI requires the next push; local validation
is not substituted for that result.

Three parallel agents remain active: MCP coordination/delegation/task graphs,
Matrix formatting with JS vectors, and namespace-qualified Linux cgroup cleanup
fixtures. Production execution, Matrix event admission/sends, retained migration
phases and controlled cutover are still open; original/live state is unchanged.

### 2026-09-10 — M6 native Matrix content formatting proof (ADR050)

- Added the isolated `hagency-matrix-format` crate with a bounded serializable
  content DTO. It preserves plaintext, thread/reply/edit relations and existing
  caller-trusted formatted content. Generated Markdown uses the retained Matrix
  tag/attribute/scheme allowlist, with raw HTML parsing disabled and no transport
  or routing authority. No live JavaScript or Matrix path was changed.
- Captured 63 byte-exact JavaScript vectors from pinned markdown-it15.0.1,
  sanitize-html2.17.7 and linkify-it6.1.0, with source/Node-lock hashes and a
  reproducible check mode. Native CI installs locked production dependencies
  with all scripts disabled before executing this pure oracle. A clean external
  cache installation reproduced the check successfully.
- Rejected a candidate parser after reproducing its emphasis source-map panic;
  the pinned selected markdown1.0.0 parser passes that fixture. Adaptations cover
  reference rejection, line breaks, tables, images, safe URL schemes, raw
  punycode, Unicode punctuation and nested-link prevention. Intentional capacity
  and malformed-edit refusals are explicit; broader syntax and actual encrypted
  client-delivery parity remain M6 integration gates.
- Six Rust test groups pass, including 128 deterministic hostile syntax cases;
  the original bound Vitest test, Clippy, rustfmt and diff checks pass. No prior
  Cargo package version was removed or upgraded. Detailed logs are the external
  cache's `matrix-format-*` files. Task lifecycle passes all four scenarios
  plus the explicit boundary, with zero skips or uncertainty. Selector inventory
  resolves every native test binding on this isolated baseline.

## 2026-09-10 — Linux protected cgroup guardian-recovery foundation

Started from `552801b` in a separate worktree. Kernel v6.12 source inspection
confirmed that cgroup.kill is not a close-triggered Job Object equivalent and
that migration checks the control file opener's credentials. Ordinary same-UID
delegation does not provide the required private reassignment boundary. ADR-048
records exact mechanical checks, privileged host obligations and remaining gates.

The optional CgroupRecovery API consumes exact preopened host descriptors and
requires protected full-mount ancestry, an empty domain, equal nonroot UIDs,
NNP, nondumpability and zero capabilities including bounding/ambient sets. Host
SIGCHLD/reaping ownership is checked before using its retained guardian identity
for migration. The same launcher puts that guardian in the boundary before
Prepare/Start; no second workspace launcher or PID-derived kill path was added.
Channel failure or explicit stop uses retained cgroup.kill and bounded recursive
empty-population proof. Unknown outcomes retain explicit uncertainty; observed
absence of live execution is not descendant zombie reaping or canonical task done.

During this slice, the coordinator reported the unrelated macOS child heartbeat
assertion failure. Investigation and its separate fixture correction were
committed independently as `5e8c317`; they are not included in this cgroup batch.
The coordinator also confirmed actual Windows owned-IO CI passed.

The cgroup qualification executable requires explicit host-provisioned FDs and
privileges. Its real native modes cover guardian death with a double-fork detached
descendant, stream closure before stop, failed spawn and full-guarantee refusal.
Missing provisioning exits 78 without a qualification result. No system cgroup,
privileged host configuration, live model or deployment was changed. Actual
protected Linux execution and escape/reassignment fixtures remain open; local
macOS parser/refusal tests and Linux cross-compilation cannot close those gates.

Verification passed: 20 macOS platform tests and six owned-runner integration
tests; all-target Clippy with warnings denied on macOS plus Linux/Windows GNU
cross-targets; formatting and diff checks. Agent-spec 1.4 passed the two limited
scenarios and explicit 12-file boundary (3/3, quality 97%, zero fail/skip/uncertain).
These are admission/regression results, not execution of the provisioned Linux
fixture. Logs are in the operator cache under `codex-protocol/cgroup-*`.
Final source review replaced fdinfo reads with descriptor statx mount identity:
nondumpability can restrict proc fdinfo access and must not be relaxed to work
around it. Known guardian signal failures remain recorded after independent
empty-subtree proof. No push, merge or deployment was performed.

### 2026-09-10 — namespace and disposable CI cgroup qualification follow-up

Continued initial cgroup commit `33751df` in its own isolated worktree. Review
identified that mount root `/` and stat UID0 are namespace-relative. Admission
now verifies current-thread proc source entries, nsfs/type/reserved initial inode,
initial cgroup owner relation and matching nsfs mount identities. Checks repeat
after guardian exec and nondumpability reset. Source-inspected kernel release
families are 6.8/6.12/6.14; unknown versions/identities refuse. ADR-048 records
exact pinned implementation sources and privileged provisioning assumptions.

Added a GitHub-hosted Linux-only root qualifier that creates one exclusive random
cgroup subtree, passes exactly three controls to a nonroot NNP/cap-empty probe,
and retains independent bounded kill/empty observation and identity-checked
removal. Actual cases include guardian death/descendants, stop, failed spawn, full
guarantee refusal, real nested user/cgroup rejection and external cleanup after
both test custodians abort. Helpers have bounded output/deadlines and retained
pidfd cleanup. No cgroups were created or modified locally or on Mini1.

Local checks passed 21 platform tests, four Python admission/collector tests,
and all-target Clippy on macOS plus Linux/Windows GNU cross-targets. The Linux-only
ordinary-subprocess test covers pidfd finally cleanup on timeout/output overflow;
it and all privileged cgroup qualification remain pending real hosted Linux CI.
Production runner enablement, adversarial ptrace/reassignment qualification and
simultaneous-custodian-loss containment remain open. Lifecycle is recorded below.
Agent-spec 1.4 passed all three explicitly limited scenarios plus the 12-file
boundary (4/4, quality 98%, no fail/skip/uncertain). The 110 native spec bindings
were found without missing tests. Lifecycle and run logs are in the operator
cache under `codex-protocol/cgroup-qualification-*`. These results do not include
privileged Linux execution; the coordinator will integrate and run hosted CI.


### 2026-09-10 — Integrated native formatting and qualified-cgroup test path

Integrated formatter e1a7969 as 0e09783 and Linux cgroup preparation/namespace
qualification 33751df +9201ded as 73c8499 +727fdb4. Workspace members, all prior
locked package versions, existing Windows/Matrix/approval changes and concurrent
document history are preserved. An initial local conflict-resolution script
malformed Cargo files; Cargo rejected them before running tests. Reconstructed
all affected files from the parent/incoming Git snapshots, checked the complete
prior package/document sets, and amended the unpushed commit before validation.
The failure log is retained; it is not counted as passing validation.

Combined 727fdb4 passes 249 unique native tests plus one proxy-isolation child
(250 printed passes), with zero failures or ignored tests. Workspace Clippy with
warnings denied, fmt/diff and 166 native selector bindings pass. Formatter
passes 63 actual JS vectors and integrated lifecycle5/5 (16 changed paths).
Cgroup lifecycle4/4 covers the combined15 changed paths; four ordinary Python
refusal/collector tests pass on macOS. These are not privileged containment
proofs. Evidence is retained under `combined-format-cgroup-*`,
`integrated-format-*` and `integrated-cgroup-*` in the migration cache.

The next native CI push installs locked JS oracle dependencies with scripts
disabled and runs the isolated root provisioner only on a disposable hosted
Linux VM. Seven actual fault/refusal cases and retained independent subtree
cleanup must pass there. Kernel/user/cgroup namespace and delegation absence
fail qualification rather than becoming skipped proof. Root fixture cleanup
after simultaneous host/guardian loss does not upgrade the runtime's explicit
Unsupported full POSIX crash-containment guarantee. Live services remain unchanged.

## 2026-09-10 — Native MCP scoped coordination (ADR-051)

Implemented fourteen typed delegation, internal conversation, peer and graph
tools through the existing runner HTTP API. The five assigned-task tools retain
their task scope and 16 KiB request bound; coordination bodies are capped at
32 KiB. Shared response/frame/watchdog limits and host-provisioned capability
headers remain in force. Page projections reject duplicate/out-of-order cursors;
strict response DTOs omit unexpected fields and preserve opaque fractional data.
POST dependency hydration is explicitly read-only for uncertainty handling.

Real rmcp subprocess fixtures against Salvo and canonical SQLite prove exact
mutation replay, pending delegation admission, cross-project/same-Agent wrong
session rejection, membership retirement, creator control, graph readiness,
canonical Done/epoch results, and result reads. Lost graph-result and conversation
responses leave durable commits; identical reconnect retries replay while changed
content or stale authority fails. Scripted responses cover corruption, duplicate
JSON keys, excessive bodies, stalled mutation deadlines, redirects, framing and
resource/node scope mismatches. The unchanged protocol/task/CLI/watchdog suite
also passes. Host notice delivery and dispatch starts are synthetic test fixtures;
no live services or model execution were used.

Focused validation: 15 tests passed, zero failed/ignored, across the hagency lib,
MCP, task-client and new coordination targets. Clippy with warnings denied passed.
All 158 native spec selectors resolve. The task lifecycle passed all six bound
scenarios plus the explicit changed-path boundary (7/7, no fail/skip/uncertain).
Formatting and diff checks passed. Log prefix: `mcp-coordination-` in the external 2026-09-10 migration
cache. No dependency or schema changes. Discovery, file/media tools, Matrix
history, progress hooks, generated MCP configuration, runtime cutover and other
platform acceptance remain separate migration gates.


### 2026-09-10 — Qualify the hosted Linux 6.17 kernel family

At2c7b402, actual Linux native tests and all five Python helper checks passed.
The cgroup qualifier then created its isolated subtree and refused kernel
6.17.0-1022-azure because only6.8/6.12/6.14 had been source-inspected. The failure
is retained in `ci-2c7b402-linux.log`; no containment pass is claimed.

Inspected ten upstream files at Linuxv6.17 commit
e5f0a698b34ed76002dc5cff3804a61c80233a7a, cached with SHA256 manifest in
`linux-6.17-source`. Reserved inode constants now live in a UAPI header, but
initial identity/owner, dynamic range, proc task resolution, credential-bound
migration and recursive kill/fork rules preserve this adapter's assumptions.
Added6.17 to the explicit family gate and its exact observed release vector;
adjacent unqualified families remain refused. ADR048 records pinned source
links. Real hosted positive/refusal qualification still must run after this fix.

The follow-up passes two exact cgroup parser tests, Linux-target platform Clippy
with warnings denied, and lifecycle 4/4 including all four changed paths. The
integrated native inventory resolves 172 selectors. These checks establish
source eligibility and refusal behavior; they do not replace the pending actual
hosted cgroup qualification. Evidence: `cgroup-617-*` in the migration cache.

### 2026-09-10 — Integrated MCP coordination validation

Integrated 8099996 passes 256 unique native workspace tests plus the isolated
proxy child (257 printed passes), with zero failures or ignored tests. Workspace
Clippy with warnings denied passes, and all 172 native selectors resolve. The
MCP coordination lifecycle passes six scenarios plus the explicit 16-path
boundary (7/7), without fail/skip/uncertain. Logs are retained under
`combined-mcp-coordination-*` and `integrated-mcp-coordination-*` in the migration
cache. The native README now describes all 19 implemented tools and retains the
open discovery, file, history, approval and runtime-configuration boundaries.

At 2c7b402, Node CI and native macOS and Windows CI passed. Native Linux tests passed but its
actual cgroup qualifier refused the then-unqualified 6.17 kernel. The new family
check is ready for a separate real CI run. The complete native CI run remains
failed until actual Linux cgroup qualification succeeds.

## 2026-09-10 — Native transcript token normalization (ADR-055)

Added the pure hagency-metering library with explicit byte/line/record/metadata
bounds. Claude UUID deduplication and Codex last cumulative totals agree with 135
synthetic vectors executed against the retained JavaScript parser. Four token
categories remain separate; cache reads do not draw the fresh-token ceiling.
Absent usage remains unknown; malformed lines and incomplete source records stay
visible. Private workspace/model hints establish no attribution or path access.

Parallel review reproduced three gaps before integration: changed Codex cumulative
breakdowns could look consistent, missing whole usage objects escaped diagnostics,
and missing fields could hide arithmetic overflow. The fixes retain last known
components across gaps, count absent expected usage records, and independently
accumulate known Claude lower bounds. Regression fixtures cover each reproduction,
raw numeric spellings, Unicode model ordering, duplicate keys/UUID conflicts,
decreasing totals and capacity exhaustion. Five focused Rust tests and Clippy with
warnings denied pass. The preserved review probe and metering logs are in the
external migration cache. Discovery, persistent ledger, authenticated provenance,
project attribution, quota enforcement and UI integration remain open M7 work.

Final focused validation: 5/5 native tests, 135 unchanged JS oracle cases and
23/23 existing metering Vitest tests passed. All 177 native selectors resolve.
Lifecycle passed 6/6, including all 14 changed paths, with no fail/skip/uncertain.
Cargo added only the local metering package; existing versions remain pinned.

### 2026-09-10 — M6 native progress policy and scoped coalescing (ADR052)

- Added pure `hagency-progress` with fixed verbs, perGroup replacement rules,
  redacted summary DTOs and one immutable host-run accumulator. Receipt, call,
  attempt, input and counter bounds are explicit. Ordering, changed replay,
  time reversal and retired writes fail without adopting old run context.
- Progress attempts throttle before transport outcome; only the accepted frozen
  watermark retires a window. Unknown outcomes remain blocked for inspection;
  newer work and lifetime totals remain. Answer-delivery inspection is a separate
  typed host count/proof, never inferred from tool activity or progress acceptance.
- The oracle executes pure policy plus actual CLI/ACP emitter bodies using fake
  IO/clock/fetch: 275 unchanged cases and 20 documented corrections. These close
  malformed/inherited rule fallback, inherited verbs, repeated/unfiltered ACP
  failures, event-filter bypass and final summaries losing prior window totals.
  Review also found that uncompleted ACP starts looked successful at finish;
  these now remain explicit unresolved attempts. Initial completed/failed status
  is respected, with mixed-outcome and pending-notice/completion-race coverage.
- Final validation: ten focused Rust tests, the 44 original progress-filter
  Vitest tests, Clippy with warnings denied, formatting, diff and JS oracle checks
  pass. Agent-spec1.4 lifecycle passes 5/5 with zero fail/skip/uncertain and 100%
  quality; advisory output-mode/IO/grouping lint messages do not establish live
  IO coverage. All 163 native selectors bind in this isolated base. Cargo adds
  only the new workspace package and changes no prior package version.
- No legacy runtime/hook/Matrix code was edited. Evidence uses external-cache
  `progress-*` logs. This is not operational status, durable outbox, Matrix
  display or full ADR026/M6 parity; actual host attachment remains unimplemented.

### 2026-09-10 — Progress integration and regression findings

Integrated f4cdead retains the formatter, metering and prior workspace packages.
All 181 native selectors resolve; workspace Clippy and all three content/progress/
metering oracles pass. Progress lifecycle passes 5/5 with all 17 integrated paths.
The full workspace regression remains failed: native_guardian_cli_entry received
no leader-exit report within its five-second fixture wait. The archived log does
not establish whether initialization was slow or the guardian failed. A separate
agent is investigating with the existing native process contract. Evidence:
`combined-progress-metering-*` and `integrated-progress-lifecycle.*`.

At 431b2ec, native macOS and Windows CI passed. Linux failed two Matrix room
fixtures waiting for a scripted HTTP request, so cgroup qualification did not run.
The fixture used a three-second wait despite allowing a longer SDK operation
between requests; it also hid early collector errors. A separate reviewed fix
aligns the test wait with the existing budgets and reports early refusal, with
new controlled regressions. Neither failed run is reclassified as passing.


### 2026-09-10 — Matrix room fixture preserves collector diagnostics

Linux CI431b2ec failed two room tests while the scripted peer awaited a request.
Both unchanged tests pass locally. The saved Elapsed message cannot determine
whether collection was still performing SDK work or had already returned an
error. The fixture's three-second request wait was shorter than the ten-second
SDK bootstrap/mutation budget between requests. Its wait now derives from SDK
plus HTTP budgets. The two affected scripts race collector completion and report
an early error directly, instead of concealing it with a later peer timeout.
Production request, SDK and cancellation deadlines are unchanged.

The complete transport integration target passes nine tests, including a real
scripted request separated by a controlled 3.1-second SDK-sized interval and an
actual wrong-account whoami refusal that must surface Identity immediately.
Scoped rustfmt/diff and Clippy pass; ADR-047 parse/lint and the full lifecycle
pass all seven scenarios plus the explicit four-path boundary (8/8), with no
fail/skip/uncertain. Evidence is recorded in matrix-room-fixture-* under the
2026-09-10 migration cache. Fresh Linux execution remains an integration check. The first local new-test run caught a double-slash fixture
URL and was corrected before the final nine-test run. Concurrent ADR-054 intake
work remains uncommitted and is excluded from this fixture commit.

## 2026-09-10 — Host-only owned dispatch integration (ADR053)

Implemented in an isolated worktree from 727fdb4. A new hagency-execution crate
connects exact DomainStore scope to the existing owned child launcher and typed
Codex pipes. Started is durable before child effects; model/effort come from the
frozen provision effect and prompt identity/path fields remain data. Cancellation
retains one worker, owner and negative reconciliation until a private result;
unknown cleanup/replies preserve historical fencing, quarantine and leases.
Completed protocol text does not create canonical Done or Matrix replies.

Fresh macOS fixtures passed six execution tests, two store scope/fence tests,
one real writer queued/committed reply-loss test, six existing task tests and
six existing owned-pipe tests. Actual fixed native subprocesses cover protocol
completion, failed spawn, EOF, wrong thread, unsupported approvals, cancellation,
expiry/revocation and absolute deadline. The lost-receipt coordinator seam only
discards an actual committed start response and is absent in normal builds.
SQLite's existing 100 ms busy timeout stays unchanged; a lock-failure fixture
verifies explicit retained negative retry after lock release. Focused macOS and
Windows GNU all-target Clippy passed. Cross-compilation is not Windows execution;
this new coordinator's Linux/Windows CI remains pending integration.

A separate read-only reviewer found no blocker in the declared slice. ADR053
records one-thread/result bounds, the conservative 60-second library wait/join
allowance, physical-directory and sandbox gaps, commit cancellation checkpoint,
macOS partial-cleanup handling and all service enablement gates. No live model,
credential, production Matrix or cgroup change was made. Final lifecycle evidence
is recorded in the operator cache under codex-protocol/owned-dispatch-*.
Final agent-spec 1.4 lifecycle passed 10/10 (nine scenarios plus the explicit
20-file boundary), quality 94%, with no failed/skipped/uncertain scenarios.
The initial lifecycle's only failure was the known root-file path parser issue;
explicit ./Cargo.toml and ./Cargo.lock corrected it without broadening scope.
All 175 native selectors were present with zero missing bindings. Inherited
project-wide trace warnings remain separate from this bounded M4 qualification.

## 2026-09-10 — Separate guardian CLI exit from database initialization

Investigated f4cdead's combined macOS native_guardian_cli_entry failure in a new
isolated worktree. The unchanged test passed in isolation in 0.70 seconds; eight
bounded native diagnostic launches each reported LeaderExited. Those measurements
showed version exits around 25–27 ms after spawn and fresh init around 80 ms, but
do not establish the original timeout cause. No production guardian/identity or
timeout change was justified by the available evidence.

The guardian entry fixture now uses actual native --version, Unicode cwd, empty
PATH and the same guardian/Job ownership path. It retains the five-second report
limit and exact platform scope assertions, and additionally checks exact compiled
version bytes, empty stderr through EOF and stable repeated terminal observation.
Reads have 256-byte caps and two-second deadlines. The independent real-binary
crash/restart test retains fresh Unicode initialization and explicit token/domain
file assertions. Both CLI tests and macOS/Windows GNU focused Clippy passed.
The temporary measurement fixture was removed; only test/contract/ADR and these
coordination notes changed. Existing guardian lifecycle verification follows.
Final guardian agent-spec 1.4 lifecycle passed 6/6, including all five scenarios
and the five-file boundary, quality 94%, with no fail/skip/uncertain results.
Baseline, bounded diagnostics, CLI tests, both Clippy targets and lifecycle logs
are in the operator cache under guardian-cli-*. This is local macOS execution and
Windows cross-compilation; updated fixture runtime qualification still needs CI.

## 2026-09-10 — Integrated owned execution, progress and metering

At 5a734bf, the full native workspace passes 282 unique tests plus the isolated
proxy-environment child (283 printed passes), with zero failures or ignored
tests. This rerun includes the guardian CLI split and Matrix peer corrections;
earlier failed logs remain retained. All 190 native selectors resolve. Workspace
Clippy passed after execution integration, and formatting/diff checks pass.

Integrated owned-dispatch lifecycle passes 10/10 with all 20 paths, and the Matrix
fixture lifecycle passes 8/8 with all four paths. The combined execution/Matrix
targets pass 22 tests. Logs are retained under `combined-owned-progress-metering-*`,
`integrated-execution-*`, `integrated-owned-dispatch-*` and
`integrated-matrix-fixture-*`. This local result does not replace actual Linux,
Windows or cgroup qualification for the new commit. The next CI run must provide
that evidence. No live service is enabled or replaced.


### 2026-09-10 — Authenticated SDK event intake with durable domain handoff

ADR-054 adds a host-only intake plan over existing current Matrix sessions. Actual
bounded HTTPS sync feeds the owned SDK, retaining the full response and frozen
registration/account/device/room/session tickets in the encrypted SDK journal
before processing. Derived events are persisted before domain mutation. Native
admission still rechecks current shared scope atomically; no domain schema, runner
API, second task store, runtime activation, Matrix sends or live service changed.

Actual offline Olm/Megolm cross-signing and verification prove a human encrypted
DM without @, exact group mentions and thread projection. Unknown key/unverified
identity/forged sender/plaintext trust flags/media/incomplete timelines fail visibly.
Applying interruption survives restart as OutcomeUnknown with exact original raw
and targets; no replay of the SDK's already-consumed token fabricates completion.
Busy and lost domain replies retain handoff without inventing device failure.
Concurrent cancellation, negative room scope and committed-but-lost results recover
only an exact existing historical receipt. Unadmitted retired/conflicting events
remain quarantined rather than retargeted. Changed content cannot reuse a receipt.

Root review found an unchanged initial next_batch could return before persisting
cursor ownership. Fixed and tested through reopen: subsequent collect refreshes
whoami/full state but cannot consume a sync. Restore now checks actual SDK cursor,
identity, original digest and frozen targets. Real encrypted journal consistency
faults and SQLite ACK/finalization rollback are covered. Receipt capacity64 has no
eviction; observation/intake share that ceiling and production remains gated on a
future safe compaction lifecycle. New SDK proof DTOs remain private; public status
exposes only phase/digest/counts. Live publication/query/verification of keys,
automatic Applying/quarantine recovery, history/media and native cutover are open.

Final local checks: complete Matrix package30 tests and verified-ingress target14
pass (44 distinct tests, no ignored/failures), Clippy for Matrix/store all targets
with warnings denied, fmt/diff and172 native bindings pass. Earlier development
failures included fixture API signatures, a valid negative-room generation,
required media fields, SDK signature-upload acknowledgement, and duplicate test
module imports; they were corrected rather than reclassified as passing. Logs
are matrix-intake-* in the 2026-09-10 migration cache. The scoped six-scenario
contract lifecycle and fourteen-path boundary are recorded separately below.

Agent-spec1.4 final lifecycle passes all six ADR-054 scenarios plus the explicit
fourteen-path boundary (7/7, quality100%, no fail/skip/uncertain). Exact run logs
are retained in matrix-intake-lifecycle/ and matrix-intake-lifecycle.log. These
local fixture results do not claim hosted Linux/Windows or live Matrix behavior;
the coordinator will run the integrated checks after the isolated commit.


### 2026-09-10 — M6 native Codex progress attachment (ADR056)

- Added `hagency-progress-runtime`, private-constructor runtime source/receipt
  types and opt-in driver/owned reads. Exact instance/thread/turn and contiguous
  receipts fence foreign, skipped, stale and retired observations. Default Update,
  launch, approval and process cleanup behavior stays available to existing callers.
- Typed tool policy entry shares ACP call tracking without synthesizing ACP JSON.
  Command completion requires explicit completed status and exitCode0; failed and
  unresolved attempts stay separate. File-change evidence is upstream-only.
  Contradicting final snapshots retire the optional projection. Unsupported tool
  kinds remain counted only in fixed diagnostics; no raw payload reaches text.
- Borrowed read cancellation and external active-source close/drop retain the
  runtime/process owner and historical local attempt custody. No Matrix send,
  domain write, canonical task completion or answer-delivery inference was added.
- Focused validation: 66 tests passed across progress, progress-runtime and runtime,
  including two real offline native subprocess cases, plus all 275 unchanged JS
  progress vectors and 20 explicit correction vectors. Focused Clippy with warnings
  denied and formatting pass. Full lifecycle/binding results are recorded below.
- This closes only the typed Codex observation-to-policy seam. Current domain
  authority attachment, durable status custody, private Matrix routing/encryption
  and actual sends remain open. Tests use no model or service. Evidence is stored
  in the external migration cache under `progress-attachment-*`.

Final ADR056 checks: agent-spec1.4 lifecycle passes 6/6 including changed-file
boundaries, with zero failed/skipped/uncertain/pending-review scenarios. All186
native selectors resolve in this isolated base. Advisory lint output does not
establish live adapter or transport coverage. Existing pinned package versions
are unchanged. The repository has no provisioned task-writer in this checkout;
this task contract and external lifecycle logs record the bounded migration work,
not a claim of canonical service-task mutation or production enablement.

Final self-review closed a clock edge: ignored and gated runtime notifications now
advance the same host clock as claims/settlement, without inventing a tool event
or policy receipt. Backdated new claims and settlements cannot bypass a later
quiet event; exact historical settlement replay remains idempotent. The added
actual-stream regression passes with the complete affected66-test suite.

## 2026-09-10 — Integrate authenticated intake and native progress attachment

Integrated Matrix63cbbae as9deb056 and progress7d828b5 as7cdeba6, retaining both
branches' coordination history, all Cargo members and every prior dependency
version. Matrix/verified-ingress44 and progress/runtime66 affected tests pass.
Matrix lifecycle passes7/7 with all14 paths; progress attachment passes6/6 with
all21 paths. Workspace Clippy, formatting,275+20 progress oracle cases and all
201 selector bindings pass. Full integrated native regression passes309 unique
tests plus the isolated proxy child (310 printed), zero failed or ignored.
Evidence is retained in integrated-matrix-intake-*, integrated-progress-attachment-*,
integrated-intake-progress-bindings.* and combined-intake-progress-tests.log.

The completed ae284b9 CI is not green. Node34529853146 and native macOS pass.
Linux's whole workspace passes and actual cgroup cases guardian-death, stop,
failed-spawn and guarantee-refused qualify on6.17.0-1022-azure. Nested-user then
fails exec126 with Permission denied before native admission. No later case is
counted as run. The strict namespace result remains required; a separate agent
is implementing a fixture staging correction without changing runtime checks.

Windows job103047645091 fails six Matrix and twelve Palpo transport tests, plus
the nested proxy child. Explicit OutcomeUnknown appears at writer responses and
several scripts then wait for a request that never arrives. Logs do not establish
the cause or distinguish disk pressure from another worker problem. The pipeline
now runs the unchanged two transport targets with one test thread only after a
Windows full-suite failure. This bounded eight-minute diagnostic preserves the
original failed verdict; it changes no deadlines, correctness assertions or
production concurrency. Its actual Windows outcome is pending the next CI run.
Logs ci-ae284b9-linux.log and ci-ae284b9-windows.log retain both failures.

MCP launch and native file snapshots remain in independent worktrees. In particular,
a successful canonical Done can invalidate the current owned execution epoch
before a final answer is emitted. The unchanged fence remains; read-only reporting
after completion needs its own integration. No production service is enabled.
## 2026-09-10 — Native owned task MCP launch (ADR-057)

- Added typed host-generated Codex task MCP configuration using pinned 0.153.4 source semantics. The exact owned task/capability becomes private child environment; config carries only names and fixed native helper args/tools. Default on-request, workspace-write and network-disabled settings remain unchanged.
- Real owned native pipes now exercise the actual `hagency mcp` helper against a fresh local canonical writer. Heartbeat requires response, readback and successful helper exit. Frozen model input containing foreign task/cwd/model values cannot replace host scope; actual capability is absent from captured config, prompt, receipt and report text.
- The Done fixture exposed a real race: epoch revocation can stop the old runtime before helper acknowledgement/readback/exit. Acceptance now requires durable Done, LostAuthority/Unknown, exact negative fence and a retained dirty lease, while separate atomic stage receipts distinguish observed from unknown delivery. Both completed and interrupted helper stages were observed during development. Renewal and the epoch fingerprint are unchanged; a later reporting phase is still required before the user reply workflow is operational.
- macOS actual pipe/MCP/writer checks pass. The existing macOS whole-tree uncertainty remains fenced; Windows cross-Clippy is compile evidence only. Current focused checks: 51 runtime/execution tests, 6 existing task-client/MCP tests, 3 new integration tests (60 unique); native and Windows GNU Clippy with warnings denied; 194 Rust spec bindings. Scoped agent-spec 1.4 lifecycle passed 5/5 (four scenarios plus the 12-file boundary), with no fail/skip/uncertain result; logs are in the local migration evidence cache.
- No live Codex model, deployed service, production credential, schema change, Matrix reply or runner availability toggle. Protected executable/workspace/config-home provisioning and effective sandbox/tool-inventory qualification remain gates.

## 2026-09-10 — Bounded native workspace file snapshots

ADR-058 adds `hagency-files`: host-opened directory authority, strict portable
relative selections, retained component/file handles, immutable bounded byte
copies and SHA256, plus shared snapshot-count custody through Drop. Real fixtures
cover Unicode, hardlinks, symlinks/FIFO, root/ancestor/leaf replacement, mutation
during copying, size limits and concurrent capacity. The Windows fixture creates
actual symlinks and a junction and refuses unsupported prerequisites visibly.
CONIN$/CONOUT$ and other DOS aliases are rejected portably. A same-size overwrite
with restored modification time documents why copied bytes are not an atomic
source-version guarantee.

All 11 focused tests pass on local macOS. Clippy passes for native macOS and
cross-target Windows GNU/Linux GNU. The first local fixture compilation exposed
an unavailable Apple `rustix::mkfifoat`; the test now creates its real FIFO using
a bounded fixed command. No production command API was added. All 412 pre-existing
locked package identities are unchanged; 12 additions include the new crate and
pinned cap-std/cap-fs-ext/cap-primitives 4.0.3 dependency chain. Logs are under the
operator cache's `file-snapshot-*` prefix. Actual Windows/Linux execution still
requires CI; this slice does not claim protected physical workspace provisioning,
atomic source consistency, persistent staging, live service wiring or Matrix media.

Task lifecycle passes 6/6: five exact native selectors and all 11 explicit changed
paths, with zero failures, skips or uncertain results. It runs against the new
crate directory to avoid rebuilding unrelated workspace packages. Full workspace
binding enumeration hit host disk exhaustion; its failure log is retained, and a
subsequent reduced-artifact attempt was stopped for coordinated cleanup. Only this
worktree's generated target was cleaned. Combined binding verification is pending
on the root integration target; neither interrupted run counts as passing.

### 2026-09-10 — Linux hosted qualifier binary staging follow-up (ADR048)

- ae284b9's Linux log passed four native cases, then unshare could not exec the
  nested-user probe (126/Permission denied). Historical ancestor modes were not
  recorded; they are not asserted as observed fact. Added an independent fresh
  fixture-UID0700 ancestor reproduction before the mandatory native78 case.
- The helper stages two fixed bounded probes in a retained root-owned sealed
  directory under fixed/tmp. Original source/checkout permissions are preserved;
  exact descriptor/inode cleanup refuses changed files/directories. No runtime
  namespace policy, privilege requirement or qualification exit is relaxed.
- Eleven local ordinary-file Python tests pass, including actual compiled native
  execution, copy/mode preservation, byte/type/symlink/refusal bounds, source
  growth, partial write cleanup, replacement survival and strict126 classification.
  Local tests do not execute a privileged namespace or qualify Linux containment.
- Evidence lives under external-cache `ci-staging-*`. Cargo-bound native lifecycle
  and hosted seven-case results are separate; hosted qualification remains pending
  until the coordinator integrates and runs the workflow. An early local fixture
  copied Apple's platform-signed echo and was killed by AMFI; a tiny fixed compiled
  program supplies the portable native-copy fixture without changing code policy.

Final checks: Python11/11, syntax and diff checks pass. Agent-spec1.4 lifecycle
passes4/4: the three unchanged native admission/refusal scenarios plus all six
changed-file boundaries, with zero fail/skip/uncertain. This does not substitute
for the separately run Python suite or pending hosted namespace qualification.
The first lifecycle attempt exited during local disk exhaustion with no usable
result; only generated targets from completed worktrees were cleaned, and the
successful rerun is separately recorded. No source/log deletion, full binding
inventory, local privileged invocation or deployment/push was performed.

## 2026-09-10 — MCP, file snapshot and hosted fixture integration

Integrated ADR057 as949b5d9, ADR058 as213342b and hosted binary staging asb42ff9b.
Every prior locked dependency identity remains present. Only generated Cargo
targets from completed isolated worktrees were cleaned after disk exhaustion;
source trees, commits and failure evidence remain intact.

At b42ff9b, the complete native workspace passes324 unique tests plus the isolated
proxy-environment child (325 printed), with zero failed or ignored. All210 native
selector bindings resolve. Focused file11, owned-MCP3 and runtime/execution/
progress-runtime61 tests pass, as do Python staging11, workspace Clippy with
warnings denied, formatting and diff checks. Integrated lifecycle results are
files6/6 with11 changed paths, MCP5/5 with12 paths and CI staging4/4 with6 paths;
no fail/skip/uncertain result is counted as passing. Broader requirement-trace
diagnostics remain distinct from these bounded contracts. Evidence is retained
under integrated-files-*, integrated-owned-mcp-*, integrated-mcp-progress-*,
integrated-ci-staging-*, combined-files-mcp-staging-tests.log and
integrated-files-mcp-bindings.* in the local migration cache.

The preceding88534b0 native run34531716943 is complete: Windows and macOS pass;
Linux passes the whole workspace but fails hosted nested-user execution with
exit126/Permission denied before native admission. Its newly integrated staging
correction still requires a new hosted run. Windows's default full test step
passes in187 seconds, so the failure-only serial diagnostic is correctly skipped.
This is not a serial/default comparison or proof of the earlier timeout cause.
Both Palpo code and transport tests are unchanged between those failing/passing
runs; Matrix intake code changed. Local writer/custody response timing remains
the first measurement target if failures recur, without widening deadlines.

Node run34531716948 fails one existing service-supervisor test: its healthy-state
assertions pass, then waiting for exactly four child event rows times out after
3000ms. The other4283 tests pass and one is skipped. No root cause is established
from that log; a separate clean worktree is investigating the actual fixture and
supervisor evidence. The failed log is retained and no assertion is relaxed.

ADR059 actual Matrix sender and ADR060 atomic completion with held reply custody
are being implemented in separate worktrees. The current native service still
does not launch Agents, send Matrix messages or replace the deployed application.

## 2026-09-10 — Bounded attachment crypto (ADR061)

Added a host-only hagency-media codec on the pinned Matrix SDK0.18 attachment
primitive and retained workspace snapshots. Encrypted results keep the original
snapshot/file permits, immutable ciphertext and private encryption metadata;
checked decryption validates the complete bounded ciphertext and SDK EOF before
returning plaintext. Static errors and opaque data types keep secrets out of
automatic projections. Size/count admission covers concurrent cloned owners.

Four focused native tests pass, including eight fixed independent Node AES
vectors verified by the actual existing Matrix crypto binding, real retained
files, SDK round trips, malformed/duplicate metadata, corruption/truncation,
capacity and concurrent custody. A regression deliberately demonstrates that
a changed valid key with the same ciphertext hash can produce different bytes:
descriptor provenance must still come from an authenticated encrypted event.
This primitive does not claim sender identity, durable staging or file delivery.

Focused Clippy and formatting pass; the only new locked package identity is
hagency-media. Independent review caught that the Rust CI job disables native
addon install scripts, so the actual Node crypto oracle now runs in the existing
Node test job after its normal npm ci. The Rust jobs consume those checked
fixtures without enabling postinstall scripts. Fresh CI qualification and the
bounded contract lifecycle remain separate from this local evidence.

The preceding79b036c Linux job103059541895 now passes the whole native workspace,
all12 ordinary staging tests and all seven actual cgroup cases on6.17.0-1022-azure:
guardian-death, stop, failed-spawn, guarantee-refused, nested-user, nested-cgroup
and custodians-abort. Every subtree was independently observed empty. This closes
the hosted probe-execution blocker; it does not establish complete simultaneous
backend/guardian crash containment or enable a production runner. The full log
is retained as ci-79b036c-linux.log.

ADR061 final local contract lifecycle passes5/5, including all13 explicit changed
paths, with no fail/skip/uncertain. Descriptor boundary tests include a valid
1024-byte input, its refused1025-byte extension, duplicate fields and padded or
noncanonical base64. Independent review found no further code blocker after the
CI oracle correction. The16 MiB maximum is a deliberate bounded subset of the
legacy20 MiB surface; this does not claim full file-feature parity.
### 2026-09-10 — Legacy supervisor fixture port custody and diagnostics

The original 88534b0 Node failure passed health assertions, then timed out waiting
for exactly four event rows. Its log omitted rows/ports/restarts, so the cause
cannot be assigned retrospectively. Unmodified local supervisor tests pass 8/8.
A controlled real-child same-port run reports healthy at 236 ms but yields only
three ready events, six dashboard restarts and EADDRINUSE after 3237 ms. This proves
the fixture's independently released ephemeral ports were allowed to collide.

The isolated test-only fix keeps both listeners reserved until selection ends
and closes both in finally, including callback failure. Four added regressions
exercise actual bind conflicts/release, real extra stopped events remaining a
failure and bounded redaction. Startup still requires exactly four rows within
3000 ms. Production service code, lease duration, probes and deployments remain
unchanged. The expanded focused file passes 12/12 in 4.12 s; preserved logs use the
external cache's node-supervisor prefix. Parse/lint quality is 100%, all five
scenario bindings exist, and syntax/diff checks pass. Native lifecycle remains
non-passing: despite its requested lint/boundary layers it launched Cargo for a
Node selector and was stopped; the Node-directory attempt then reported missing
Cargo execution. Neither is a passing boundary or behavioral result. Only the
owned generated target is cleaned; source and all original logs are preserved.
## 2026-09-10 — Native final-reply send checkpoints (ADR-033 amendment)

- Added host-only claimed preview and current Sending validation, so the native Matrix sender can match its private transport account before send-start and recheck exact current scope before IO. Neither method performs network IO or changes reply state.
- A journaled exact Delivered observation can now reconcile Sending directly after sender receipt/secret loss. NotSent remains Uncertain-only; immutable body/route, send fence and durable inspection digest still determine acceptance and replay. No runner endpoints or native schema changes.
- Focused reply repository suite covers wrong secrets/fences/IDs, expiry, null-root DM promotion, substituted delivery fields, NotSent refusal and exact positive replay. All 16 focused reply tests and Store Clippy passed. The scoped agent-spec 1.4 lifecycle passed 3/3 with no failed/skipped/uncertain scenarios using the store package as its code root; ADR059 owns actual Matrix sender consumption separately.

## 2026-09-10 — Windows file-custody qualification correction

Native79b036c run34533485134 finishes failed on Windows while Linux and macOS pass;
Node34533485274 passes. Two Windows file tests incorrectly require a retained
directory to be renamed. Actual error32 and the pinned cap-primitives4.0.3
oflags/dir_utils source agree: directory opens deliberately exclude SHARE_DELETE.
The corrected Windows fixtures require that exact denial, verify original bytes
and retained snapshot custody, then release the handles and require the same
rename to succeed. POSIX still exercises actual ancestor/root replacement; the
leaf-file case remains a real rename. Production handle flags are unchanged.
New actual Windows execution remains required; local/cross checks cannot supply it.

The same run has four unrelated DomainStore shutdown OutcomeUnknown failures
(three MCP coordination cases and one runner HTTP case). Their assertions reach
shutdown after their functional checks, at fixture.rs216 and runner.rs183.
The failure-only diagnostic's Matrix9 and Palpo13 transport targets pass serially;
this does not diagnose those different shutdown failures. Logs are retained as
ci-79b036c-windows.log. No shutdown deadline, retry or final-release claim is changed.
### Native owned completion handoff (ADR-060 / schema 016)

Implemented explicit canonical Done plus held final text through the real native
MCP/HTTP boundary. Exact historical receipt replay is isolated from ordinary runner
commands. The retained owned runner stops before final admission; no protocol text
or epoch refresh grants execution, marks Done, or releases unrelated custody.
The writer checks cancellation after queue/lock and rechecks original route,
completed epoch, attempt and finite deadline before publishing stored content.

Focused evidence: the complete store package passed 115 tests; runtime/execution
regressions passed 51; native MCP/task-client tests passed 13. Actual local native
helper finish commits Done and preserves the macOS whole-tree refusal gate. The
new MCP endpoint rejects foreign task/route/capability, conflicting replay and
oversized encoded input; identical post-fence replay returns only its receipt.
Capacity, migration, queue reply loss, cancellation and wrong-scope/route/resource
fixtures retain canonical and custody invariants. Native and cross-target checks
and both focused task lifecycles are recorded with this slice's validation.

Native service remains disabled. Actual Linux/Windows positive completion cleanup
and the complete authenticated Matrix input-to-final-send workflow require the
combined platform integration/CI; ADR-059 owns the final Matrix sender. Task-only
Done and ownerless held content still require an explicit future reporting or
inspection path rather than automatic re-execution.

Final scoped validation: 195 tests across the selected packages and native helper
integration targets (16 core, 115 store, 51 runtime/execution, 13 helper/client).
Native and Windows GNU cross-target all-target Clippy pass with warnings denied;
fmt/diff checks pass. Agent-spec 1.4 package-scoped lifecycles pass all 8 store
scenarios and 3 native integration scenarios plus both boundaries, quality 100%,
with zero fail/skip/uncertain. Cross-compilation is not Windows runtime evidence.
## 2026-09-10 — Bounded shutdown phase diagnostics

- Preserved the original Windows 79b036c MCP/runner teardown failures. Source
  inspection cannot assign them to queueing, SQLite destruction or scheduling.
  Added optional per-job fixed atomic timestamps and an original-verdict snapshot;
  no deadline, retry, Drop-before-ack, ordinary shutdown or authority change.
- Four focused selectors passed: actual normal ownership release, enqueue versus
  queued reply timeout, controlled Drop/ack phase pauses, and independently
  published bounded snapshots. The initial private tests ran in 4.22 seconds and
  actual repository success fixture in 0.13 seconds. Added explicit zero-time and
  partial-order checks; root review corrected a test race where a second shutdown
  can observe either enqueue-closed or reply-closed after the first acknowledgement.
- MCP/runner fixture shutdown still fails on the original error and prints only
  that error plus the static snapshot. Success remains silent. Focused Store
  Clippy and scoped agent-spec lifecycle pass; the lifecycle includes four bound
  behaviors plus boundary with no failures, skips or uncertainty. Validation logs
  are external under the shutdown prefix. No full workspace rebuild was run;
  affected MCP/runner suites and Windows qualification belong to integration.
## 2026-09-10 — Native authenticated Matrix outgoing custody

ADR059 adds actual bounded authenticated notice/final sending to the owned
Matrix collector, consuming the separately tested ADR033 host send checkpoints.
The protected outgoing journal preserves original route/fence/formatted bytes,
SDK key-share/ciphertext writes and accepted HTTP responses. Domain Sending
precedes key sharing; fresh identity/full-state/recipient checks and current
claim validation gate each write, including after the awaited Possible marker.
No claim secret is persisted and uncertain bytes are never automatically resent.

Real local HTTPS fixtures cover plaintext formatted notices/finals and encrypted
DM/group delivery with strict SDK decryption and new group sessions. Additional
faults cover malformed fresh cross-signing/device keys despite cached trust,
retirement during journal persistence, lost HTTP/SDK/domain replies, SQLite
rollback, late retired notice acceptance, nine corrupted protected history
shapes on reopen and 64 actual sends followed by capacity refusal without eviction.
Only exact current notice acceptance activates its task; historical acceptance
records delivery without restoring authority.

The complete Matrix package passes 49 tests (40 unit including 19 outgoing, plus
9 transport), with zero failed/ignored tests. Native package Clippy with warnings
denied passes after correcting two collapsible conditions and boxing the private
notice Source variant. Cargo.lock changes only the existing package's local
formatting/SHA dependency metadata; no locked version changes. Earlier compile,
fixture and Clippy failure logs remain under the external evidence cache's
matrix-outgoing-* prefix. Full workspace/three-platform integration is reserved
for the coordinator; no duplicate broad isolated build or live service ran.

The existing notice/store regression suite passes 14/14. Agent-spec 1.4
lifecycle passes 7/7: six exact selectors and all 18 explicit changed paths,
with zero failures, skips, pending reviews or uncertain results. Live key upload,
missing-session claims, trust establishment, automatic uncertain-send recovery,
receipt compaction, media, production wiring and overall M5 completion remain
explicit gates. No native availability toggle, domain migration or deployment.

ADR-060 review closure: pass the original operation Instant into queued publication
and check it together with cancellation after writer queue/DB lock acquisition.
The new actual queued-expiry test preserves held body/lease even when the separate
persisted completion deadline is later. Three completion worker tests and five
actual helper/MCP tests pass, as does native all-target Clippy. Documentation now
distinguishes decoded body32KiB, native MCP/client encoded32KiB, and direct private
HTTP encoded64KiB limits; no handler or permission limit was relaxed.

## 2026-09-10 — Offline authenticated Matrix to owned native completion

ADR062 adds one test-only integration harness over real local TLS, the owned SDK
collector, canonical DomainStore, normal notice activation/inbox dispatch, actual
owned native app-server pipes, generated native MCP helper and final sender. The
original human event/thread, exact body and separate notice acceptance are checked
throughout. Native Done and upstream unknown output remain distinct; final
publication uses only retained actual owner cleanup and the existing writer fence.

First local run passed all three tests in1.82s. On macOS the positive process flow
commits Done but correctly requires CleanupUnknown, a retained lease and no final
HTTP. Real notice403/lost responses prevent activation and execution; forged
plaintext verification in an encrypted DM is refused before task creation and
currently retires the entire transport. Per-event refusal without disabling
unrelated chat is a separate production follow-up. The Linux/Windows branches require actual final HTTPS, formatted body/original root
and idempotent accepted receipt, pending actual platform CI. No production source,
schema, permission, service availability or live deployment change was made.

Final ADR062 focused validation: three integration tests pass, including exact
HTTP room/transaction paths. Native and Windows GNU cross-target Clippy for the
new integration target pass with warnings denied; fmt/diff checks pass. The
agent-spec1.4 package lifecycle passes4/4 (three scenarios plus all eight explicit
changed paths), quality100%, zero fail/skip/uncertain. Evidence is retained under
the external cache's matrix-owned prefix. Windows cross-compilation is not actual
Windows execution; Linux/Windows final delivery remains for integrated native CI.


## 2026-09-10 — Integrated completion and Matrix delivery checkpoint

Integrated ADR059 authenticated outgoing custody, ADR060 explicit held completion
and its queued-publication deadline correction, shutdown phase diagnostics, and
ADR062 actual local TLS/owned native MCP workflow. The original operation deadline
is checked after the publication writer queue and SQLite lock, so a persisted
completion deadline cannot extend the operation's authority.

The first combined suite found one strict MCP catalog regression: the expected
catalog lacked the newly implemented complete_task_with_reply tool. The expected
catalog now contains all twenty names; schema and negative authority assertions
remain intact. The original failure log is retained as
combined-completion-matrix-tests.log in the external migration evidence cache.
The focused catalog lifecycle passes 7/7 including its explicit changed path.

After that correction, the entire locked native workspace passes 367 unique tests
(368 printed results because the environment-proxy test also runs its child),
78 suites and zero failures or ignored tests. All 243 specification selectors
resolve to actual tests. Workspace all-target Clippy with warnings denied,
rustfmt and diff checks pass. Evidence uses the
combined-completion-matrix-workflow-tests and integrated-completion-matrix-workflow
prefixes. Integrated scoped lifecycles pass for the completion store (9/9),
completion integration (4/4), shutdown diagnostics (5/5), Matrix outgoing (7/7),
queued-deadline store correction (10/10) and MCP correction (4/4), each with all
explicit paths checked and no failed, skipped or uncertain verdicts.

The previous pushed revision 75f47f1 passed actual Linux, macOS and Windows native
CI (34535368811), including Windows retained-root/ancestor handles and media
fixtures. Its Node CI (34535368595) passed 4,288 tests with one existing skip,
including eight native-addon media oracle vectors. Those results qualify that
revision only. New ADR059/060/062 Linux and Windows execution remains a CI gate;
local macOS deliberately retains unknown cleanup and sends no final reply.
Historical Windows shutdown failures still have no established cause.

Three isolated agents continue the usage ledger, private Matrix approval intake
and durable per-event intake refusal. No live services, accounts, credentials,
model execution, release availability or production cutover were changed. This
checkpoint does not complete any migration milestone or the overall migration.

Final checkpoint lifecycle checks pass: ADR062 integration4/4 with all eight
changed paths, ADR056 reference correction6/6 using the workspace code root, and
the coordination/readme checkpoint10/10 with all four explicit paths. The earlier
package-only ADR056 invocation remains recorded as5pass/1skip; it is not counted
as passing. No production code changed after the combined native suite.

## 2026-09-10 — Native host-attributed usage ledger

ADR063/schema017 adds host-only binding and recording for one exact Started native
dispatch/fence. The existing bounded parser creates non-deserializable normalized
observations; raw paths/models never select an Agent. Immutable historical source
identity survives retirement/restart and no live scan, runtime source setter,
provider authentication claim, service hookup or quota authority is introduced.

Per-kind observed high-water growth, UTC daily/monthly buckets, original content-
bound receipts and the checked writer clock commit in one SQLite transaction.
Unknown fields and full parser diagnostics remain visible separately from known
lower bounds. Latest incomplete/regression flags cover cross-snapshot decreases;
historical incomplete coverage stays sticky. Finite source/receipt/period limits
refuse admission without evicting prior evidence. Both period inserts roll back
when a new month needs two rows but only one remains.

The JavaScript oracle produces 16 vectors/80 snapshots against the retained ledger
for known-count high-water, cache separation and observed UTC arithmetic. Native
correction fixtures cover unknown/zero, malformed and duplicate evidence, overflow,
clock reversal, all six row ceilings, foreign and retired source bindings,
reappearance and actual database rollback. Real writer fixtures qualify timestamp
after SQLite lock and before/after-commit response loss without wider deadlines.

Validation before final lifecycle: 133 affected tests passed (128 store, 5 parser),
zero failed/ignored; focused warnings-denied Clippy passed. The first focused run
was 6 passed/1 failed because the fixture used an unqualified Claude model alias;
it now uses the existing qualified `claude-sonnet-5` resource without changing
qualification policy. The initial compile had one temporary-borrow scope error,
fixed by computing queue weight before moving the opaque scope. Those logs remain
in the external `usage-*` cache. Full workspace/three-platform integration is
reserved for the coordinator; source discovery, actual descriptor/process matching,
automatic archival/retention, enforcement and browser/service integration remain
explicit gates.

Final scoped verification: latest correction/capacity assertions pass 7/7, and
agent-spec 1.4 lifecycle passes 9/9 (eight bound selectors, including two real
writer tests, plus all 30 explicit changed paths), with zero failed/skipped/
uncertain/pending-review results. One nonblocking lint heuristic requests file-
output coverage for persistence wording; this slice has no CLI/file formatter.
The first lifecycle correctly failed only because bare `Cargo.lock` was not
recognized as a root path; the contract now declares `./Cargo.lock`. That failure
log is preserved. Final oracle, formatting and whitespace checks pass. Parent
independent review checked source authority, optional arithmetic, latest flags,
all six capacity branches and second-period rollback with no remaining blocker.
No source tests or production deadlines were weakened.


## 2026-09-10 — Operator usage observation projection (ADR067)

Added one authenticated native GET for engagement aggregate usage and selected
UTC day/month observations. One original bounded writer operation reads the
projection; no source restoration or observation write is exposed. Unknown data
and absent periods remain null, incomplete/regression history stays explicit,
and private task/source/workspace/room records never enter the response type.
The capability describes a development read endpoint only; execution, connected
transport, browser console and production parity remain disabled.

Three real Salvo/fresh-writer tests pass, exercising authority and forbidden
headers, absent/partial/regressed/recovered observations, exact public fields,
malformed/duplicate/oversized/out-of-range queries, missing engagements and
missing/closed writers. Focused native Clippy with warnings denied passes. Initial
compile evidence (Salvo generated handler name collided with a pattern binding)
and fixture failure (task/session arguments swapped) are preserved under the
external usage-read prefix; both were corrected without changing production
authority or weakening assertions. Root independently integrated ADR063 schema17
and its lifecycle passes9/9 with all30 paths; its16 JavaScript vectors also pass.

ADR067 strict lifecycle passes4/4 with all15 changed paths. Independent review
found no blocker. The final three tests also cover both supported UTC endpoints
and a percent-encoded duplicate parameter; all pass, with formatting and diff
checks clean. Existing locked package versions remain unchanged.

## 2026-09-10 — Conclusive native Matrix event rejection

ADR065 adds private bounded source dispositions after exact raw-to-actual-SDK
coverage, before any eligible event admission. Rejected and non-target source
tombstones survive cursor completion and cannot be reinterpreted under new plans
or key/trust updates. Genuine identity/room negatives, incomplete timelines, SDK
uncertainty and domain authority failures retain their existing fences. No schema
or service change; ADR064 approval journals remain a separate collector purpose.

Nine focused fixtures first passed in 8.13s: actual mixed bad/good chat, encrypted private
missing-key and trust-change continuation, replay after SDK restart/new plan,
real SQLite receipt rollback, SDK interruption, missing coverage, identity failure,
legacy filtered-history inspection, corruption of candidate/source/phase fields
and real terminal receipts through capacity. Early failures exposed fixture
misuse of Collector.close (correctly retires transport), incomplete key query
responses and SDK cached unverified group-session sender data. Tests now cycle
only the actual SDK owner and use a freshly shared ordinary human outbound group
key for the newly sent message after verification; old receiver state/proofs are
never reset. Temporary diagnostics were removed. The original failure logs remain
in the external matrix-rejection evidence files.

Final local validation: 58 Matrix tests and three amended owned-workflow tests
pass; the nine new tests pass again after unsigned-metadata and missing-ID replay
vectors were added. Native and Windows GNU cross-target Clippy pass with warnings
denied; cross-compilation is not Windows execution. agent-spec 1.4 passes the new
contract 6/6 (five scenarios and the complete 14-path boundary), quality 100%,
with zero fail, skip or uncertain. Evidence uses the matrix-rejection prefix in
the external cache; full integrated OS qualification remains with the parent.
The amended ADR062 contract separately passes 4/4 (three scenarios and its five
changed paths), quality 100%, with zero fail, skip or uncertain.

### 2026-09-10 — ADR064 native private Matrix approval intake

An isolated approval collector now uses actual bounded HTTPS and the owned SDK to
settle existing native owner requests. Its purpose-bound encrypted journal and
cursor cannot be adopted by Agent chat or outgoing transport. Exact registered
bot/whoami and full private owner membership precede frozen native target intake;
verified cross-signed encrypted structured actions feed a transactional current
request/binding/dispatch/task/resource check. Negative approval evidence retires
old shared bindings and grants without rotating the Agent transport.

Every timeline source, including ordinary or refused messages, retains an immutable
wire tombstone. Separate SDK proof and ciphertext digests bind domain receipts.
Changed-source replay quarantines; a new target plan cannot reinterpret an old
rejection. Applying interruptions stay inspectable with the original response
and exact cursor. Lost domain responses and actual SDK SQLite rollback recover
only historical accepted receipts, even after private-device rotation, and cannot
mint new grants. Finite 64-batch/256-source limits reject capacity without eviction.

Local full Matrix verification passes 65 tests (56 unit, including 16 new approval
fixtures, plus 9 transport tests), with zero failures or ignored tests. The 16 store
approval tests pass, including new scope/expiry, stale-negative CAS and grant/source
transaction rollback checks. Clippy with warnings denied passes after one initial
collapsible-if correction; formatting and the task contract parse/lint pass. All
failed compile/fixture/lint logs remain in the external matrix-approval-* evidence
cache. No lockfile version, schema, live account, service or deployment changed.
Agent-spec 1.4 lifecycle passes 6/6, including five exact selectors and all 20
explicit changed paths; no failed, skipped, uncertain or pending-review result.

Native IDs remain approval_ plus 40 hex characters; legacy Robrix/JavaScript uses
32. Card delivery/client compatibility, runtime decision application, live key
provisioning, automatic unknown-SDK recovery, receipt compaction and service cutover
are explicit gates. Parent integration and three-platform CI remain separate from
these actual local tests. The existing outgoing target was reused to avoid another
large SDK build tree; no broad workspace build ran in this isolated worktree.

## 2026-09-10 — Bounded native private media staging

ADR066 adds isolated hagency-media-store storage over actual Snapshot/Encrypted/
CheckedBytes custody. Stable operation IDs bind exact bytes, kind, namespace and
private descriptors in a finite checksummed journal. Retained directory/file
capabilities, one file owner, strict private checks and creation-only Windows SID/
DACL sealing precede private writes. No domain schema, Matrix path or service API
changed; staging does not grant execution, route or delivery authority.

Real interruption fixtures cover intent, partial payload, descriptor, commit and
sync/response boundaries. Unknown tails remain quarantined without truncation;
complete committed frames recover original ciphertext/keys. Actual refused OS
write with zero persisted bytes demonstrates restart absence, not unsent proof.
A future domain operation must be persisted before invoking staging. Parent review
found preflight Media ownership loss; StageFailure now returns unadmitted input
while post-write failures stay held by the original Store, with exact descriptor
regressions for conflict, full/quarantined state and failed private-object checks.

Latest affected local tests pass 23/23 (11 files, 4 codec, 7 staging, 1 existing
private-storage regression). Native and Windows GNU warnings-denied Clippy pass,
including Windows sealing/ACL fixture compilation. Actual Windows runtime/three-
platform qualification is delegated to integrated CI. A first compile needed an
explicit result type; the ownership API refactor briefly left the public signature
unchanged and failed compilation, then was corrected without changing behavior
assertions. Initial unused test imports were removed. All earlier failure logs
remain in the external media-staging-* evidence cache. No full workspace build,
live service, automatic cleanup, hardware power-loss or native cutover claim.

Final agent-spec 1.4 strict lifecycle passes 7/7: six bound selectors (seven
actual staging tests) plus the boundary check for all 13 changed paths, with no
failed, skipped, uncertain or pending-review results. Four nonblocking lint
heuristics remain: two request absence/API constraint coverage and two do not
recognize the explicit local-filesystem verification metadata. These are not
claimed as additional test passes. Added real upstream snapshot-permit assertions
prove returned and retained failure custody also keeps the original resource
owner alive. The final replay selector passes 2/2; both Clippy targets, formatting
and whitespace checks pass after those assertions. All evidence is retained under
the external media-staging-final-* cache logs.

## 2026-09-10 — Windows approval cancellation fixture phase

Retrieved the completed c0afefc Windows job 103078328251 from native run
34539380960. Its sole failed target was hagency-permissions coordinator: seven
passed and native_codex_approval_uncertainty_write_cancel_restart failed at the
unqualified seen assertion. Shutdown and all other original targets passed,
including three ADR062 real owned Matrix tests. No ShutdownSnapshot failure
was present. Later serial Matrix/Palpo transport diagnostics passed, including
an expected should-panic Identity case; they do not supersede the original
approval failure or establish a historical shutdown cause.

The original cancellation timer covered database consumption before write entry.
Its assertion incorrectly excluded the real committed-Applying/no-byte state
already proven by the passed attach/consume lost-response fixture. Exact original
instruction timing is not observable in the log. In an isolated c0afefc worktree,
the four original uncertainty tests also passed locally; that is separate evidence.
The correction gates cancellation on the actual blocked writer with a bounded
phase wait, retains the same 30 ms cancellation interval and all restart/no-retry
assertions, and adds mode labels. No production, authority, deadline or cleanup
code changed. Logs are retained in the external windows-c0afefc evidence files.

Final fixture verification completed before the parallel agent reached its usage
limit: all eight coordinator tests, native and Windows GNU Clippy, and strict
lifecycle3/3 over all five explicit paths pass. The coordinator verified those
retained logs and source diff, then completed the commit. Actual Windows rerun
remains required; cross-compilation and local success do not replace it.


## 2026-09-10 — Combined usage, approval intake and media verification

Integrated ADR063 usage/schema17, ADR067 operator aggregate reads, ADR065
terminal event refusals, ADR064 private owner verdict intake, ADR066 retained
media staging and the Windows approval fixture correction at revision 37a0010.
The locked workspace all-target suite passes 416 unique tests plus one
proxy-environment child (417 printed), 81 suite summaries, with zero failures or
ignored tests. All 272 Rust specification selectors resolve. Workspace all-target
Clippy with warnings denied, rustfmt and whitespace checks pass. Full logs remain
in the external combined-usage-approval-media evidence cache.

The previous pushed c0afefc native run 34539380960 passed Linux and macOS. Its
original Windows job passed all three owned Matrix workflow tests, but failed one
approval cancellation assertion; later transport diagnostics do not replace that
failed result. The correction now synchronizes on actual write entry without
changing production behavior. A fresh Windows run is still required, including
creation-only media journal ACL sealing. Node run 34539380958 passed 4,288 tests
with one existing skip and eight actual native-addon media oracle vectors. Those
remote results qualify c0afefc only.

Native approvals now have authenticated owner intake, while proof of runtime
application remains open. Media staging retains bytes and descriptors but has no
network transfer or file tools. Usage reports preserve uncertainty and do not
enforce quotas. Browser/service wiring, retention, runtime/sandbox qualification
and all inventory parity gates remain open. This is a development checkpoint,
not a completed migration or authorization to change the live installation.

Integrated strict lifecycle checks also pass: usage ledger 9/9 across 30 paths;
usage reads 4/4 across 15; per-event refusal 6/6 across 14 and amended workflow
4/4 across five; approval intake 6/6 across 20; media staging 7/7 across 13;
Windows fixture 3/3 across five. The documentation checkpoint runs the owned
dispatch lifecycle at workspace scope, with all four changed documents explicit.
No fail, skip, uncertain or pending-review verdict was counted as passing.


## 2026-09-10 — Scoped runtime usage observations

ADR069 retains the actual SessionDriver tokenUsage notification as a fixed optional
counter projection with exact source/sequence. The default Update remains Progress.
Total and last remain separate, missing/invalid/future evidence stays explicit,
and the progress attachment consumes receipts without manufacturing tool activity.
Four new actual-stream regressions pass, along with the complete runtime/progress
affected suites and warnings-denied Clippy. No transcript, ledger, quota or service
wiring is claimed; this supplies typed capture for the later attribution adapter.

The full affected runtime/progress run passes59 tests. Final strict lifecycle
passes5/5 across all12 explicit changed paths. Its first attempt recorded one
uncertain scenario because a concurrently compiled media-test metadata import
failed before that selector ran. That fixture compile error was corrected and
the entire lifecycle rerun passed; the original uncertain result is retained.


## 2026-09-10 — Linux media directory sync correction

NativeCI34543863628 at3b5db90 passed macOS but failed all seven Linux media-store
cases at Store::create with OutcomeUnknown. All other original Linux targets
passed. Source tracing found cap-std's retained ambient directory is O_PATH on
Linux; duplicating it cannot make fsync valid. The fix opens fixed relative dot
under that retained directory, checks the same device/inode and unchanged private
permissions, then retains that readable descriptor for directory sync. Actual
flush failure still quarantines; no ambient path or permission fallback is added.

All eight media tests and warnings-denied Clippy pass locally; strict lifecycle
passes3/3 across six paths. The added metadata comparison initially mixed cap-std
and std extension traits and failed compilation; using the cloned std handle
corrected it without weakening the identity assertion. Its real Linux branch
asserts O_PATH EBADF before corrected sync and remains for actual CI. A stopped
local Docker daemon supplies no Linux execution evidence. Windows directory
sync uncertainty and existing macOS behavior are unchanged.

## 2026-09-10 — Bounded native encrypted media download

ADR068 adds MediaDownloader over the existing hardened Matrix HTTPS client and
attachment Codec. Typed MXC components select only a repository path under the
configured origin. Separate binary framing checks require complete HTTP body EOF,
finite actual bytes and descriptor hash validation before checked plaintext.
Clones share active-transfer and retained-result permits; dropped futures and
static errors release only their own buffers. Descriptor provenance, current
dispatch authority, media staging, uploads and room sending remain separate gates.
No SDK or domain/file state is opened; the Linux ADR066 directory-sync correction
is independent and changes no assumption in this transport.

Focused local TLS tests pass6/6. All affected Matrix and codec tests pass84/84
(65 Matrix unit, six media-download integration, nine existing transport and four
codec tests), with no ignored tests. The first focused run correctly refused an
unclean close-delimited TLS EOF: Fake previously dropped TlsStream without sending
close_notify. The approved fixture change performs bounded graceful shutdown and
retains an explicit unclean response; missing chunk terminators, truncated lengths
and integrity failures still fail. Initial manifest inheritance incorrectly assumed
base64 was workspace-shared; pinned existing0.22.1 fixed that check. Clippy found
a complex fixture response tuple, replaced by a named test-only response struct.
Original failures remain in external media-download-* evidence logs.

Final native and Windows GNU warnings-denied Clippy, formatting and whitespace
checks pass. Strict agent-spec1.4 lifecycle passes7/7 (six bound selectors plus
all11 explicit changed paths), with zero failed/skipped/uncertain/pending-review
results. Nonblocking lint heuristics request metadata and absence/JSON-preservation
scenario wording; no heuristic warning is counted as a test pass. The unchanged
JSON path is additionally covered by the complete affected Matrix suite. Parent
independent review found no blocking issue; allocator wording now distinguishes
logical payload caps from allocator rounding and total physical RSS. Actual
Linux/macOS/Windows integrated execution remains the coordinator's CI gate.


## 2026-09-10 — Actual Windows outgoing failures identified

The original Windows job103092158594 at3b5db90 completed with eight outgoing
library failures (Matrix56pass/8fail), while all seven media staging cases and
all eight approval coordinator cases passed, including the corrected write-cancel
phase. One outgoing failure directly exposed claim_final_reply OutcomeUnknown;
two exposed an early collector OutcomeUnknown and five only a missing scripted
HTTP request. The60,000 argument is a lease, not the bounded writer response
deadline. Logs do not prove the source of that delay or a production defect.

The old serial diagnostic reran only Matrix9 and Palpo13 transport tests, all
passing; it missed every failed outgoing library case and cannot reverse the
original result. A new failure-only five-minute step runs native_matrix_outgoing
serially with the SAME workspace/all-target feature selection. The normal full
suite stays fatal, production deadlines/assertions remain unchanged, and existing
transport diagnostics remain. Original and split logs are retained externally.


## 2026-09-10 — Integrated download and usage checkpoint

At5b83591 the locked all-target workspace passes427 unique tests plus one
proxy-environment child (428printed),82 suite summaries and zero failures/ignored.
All287 Rust selectors resolve; full warnings-denied Clippy, formatting and
whitespace checks pass. Integrated ADR068 lifecycle passes7/7 across11paths.
Independent ADR069 review found no blocker and confirmed historical source
validity is not current execution authority. Runtime/progress attachment remains
separate from ledger attribution. The later diagnostic-only CI change parses as
YAML and its scoped lifecycle passes3/3 across four paths.

NativeCI at3b5db90 remains failed on Linux and Windows; macOS and Node passed.
The Linux directory handle correction and new Windows outgoing diagnostics are
ready for fresh actual CI. No safe migration/cutover claim follows from local
success. Evidence remains in the external combined-download-usage logs.

## 2026-09-10 — Typed untrusted runtime usage normalization

ADR071 supplies fixed optional Codex counter DTOs and a pure UsageObservation
constructor without fabricating transcript JSON or introducing runtime/store
dependencies. The actual pinned upstream cache-write fixture normalizes
input100/read40/write60/output10 into fresh0/read40/write60/output10. Reasoning,
last-response and context values stay separate evidence. Invalid values sanitize
to unknown; impossible fresh input remains unknown and contradictions are
diagnosed. Known normalized category overflow returns no observation. Evidence
is versioned, content-bound and always stream-incomplete; it grants no source,
provider, quota or canonical authority. ADR070 owns the separate host adapter.

All ten metering tests pass: five retained parser tests and five new typed runtime
selectors. Coverage includes every retained digest field, all twelve unsafe and
missing counter positions, context limits, checked arithmetic with unknown fields,
upstream context-reset shape, repeated/growing/regressing cumulative snapshots,
and exact unchanged transcript observation JSON. Focused native and Windows GNU
warnings-denied Clippy pass; cross-compilation is not Windows runtime evidence.
Strict agent-spec1.4 lifecycle passes6/6, including all five bound selectors and
eight explicit changed paths, with no skipped uncertain failed or pending-review
verdicts. The crate-scoped code root avoids a redundant full workspace build.
Formatting and whitespace checks pass. Evidence is in external
typed-runtime-usage-* logs; no live runtime was contacted.


## 2026-09-10 — Owned runtime usage reaches the historical ledger

ADR070 binds the exact acknowledged Started scope before native child creation,
then attaches only the private operation's fresh OwnedSession. Every observed
update advances the same source sequence; usage passes through ADR071 without
synthetic transcript JSON. One exact normalized pending tuple survives cancelled
waits and lost responses. Capture failures close new admission, while existing
runner cleanup, canonical completion and delivery still run independently. A
normalization refusal retains the fixed original counter projection; an explicit
retry cannot convert it into an admitted record. Runtime evidence always remains
stream-incomplete and aggregate labels now cover untrusted usage from both forms.

The first focused run failed three new fixture setups because project proof
timestamps were compared with wall-clock time rather than their fixture clock.
Correcting only admission/approval fixture timestamps let all eight initial
capture tests pass. Clippy then found the common test module loaded twice; both
unit modules now share one crate-local fixture import. A later actual native
capacity fixture proves storage refusal leaves protocol completion independent.
All25 affected execution/metering tests and warnings-denied Clippy now pass.
No source/receipt capacity, production timeout or execution assertion was weakened.

Independent review corrected reattachment testing after source sequence advancement
and required the retained normalization rejection. The integrated locked workspace passes441 unique tests plus one proxy child
(442 printed),83 suite summaries and zero failures/ignored. All301 Rust selectors
resolve. Full workspace warnings-denied Clippy, Windows GNU affected Clippy,
formatting and whitespace checks pass. Strict agent-spec1.4 lifecycle passes 10/10, covering all nine bound scenarios
and all20 explicit changed paths, with zero failed/skipped/uncertain/pending-review
verdicts. The fixed metadata/no-authority lint suggestions do not alter those results. No live provider or runtime state
was opened, and no deployment or service cutover is enabled.


At a2f8348, actual native CI34545625733 completed successfully on all three
platforms and Node CI34545625740 passed. Linux printed429 passes (428 unique plus
one proxy child), macOS428 (427 unique plus child), Windows425 (424 unique plus
child); each has82 suite summaries and zero failed/ignored. Platform-specific
selectors account for the different counts. Linux's new retained-directory fsync
regression passed. Windows passed all19 outgoing cases, media8/8, download6/6 and
approval8/8; both failure-only diagnostics were skipped, not additional evidence.
All release builds and Clippy passed. The old3b5db90 Windows failures remain
unexplained. These results concern a2f8348, not the later usage capture changes.

## 2026-09-10 — Bounded encrypted media upload transport

ADR072 adds an actual binary HTTPS POST primitive under the existing hardened
client. A caller-held attempt borrows the exact SDK Encrypted object, retains a
finite slot and moves irreversibly to WritePossible before request polling.
Future drop, cancellation, timeout or invalid responses preserve uncertainty and
the caller's ciphertext/descriptor custody. Accepted is stored only after complete
bounded unambiguous JSON, validated MXC and a final cancellation/deadline check.
Neither an error nor a returned URI establishes room/event/delivery authority or
safe automatic retry. No domain/file-tool/live service or staged-media adapter is
enabled. Existing JSON and download methods remain unchanged.

Five real local TLS selectors pass with actual retained file snapshots and SDK
encryption. They inspect exact ciphertext, bearer destination and absent secret
metadata; exercise valid4096/refused4097 response size, duplicate fields, invalid
MXCs, malformed framing, header limits, truncated/unclean EOF, cancellation,
future drop, header/body/absolute deadlines, terminal no-resend and shared active
plus held-attempt limits. Header-negative fixtures carry otherwise valid URI
bodies. Initial test compilation exposed only a helper-name shadow and unused
import, fixed before executing tests; original log is retained externally.
All 85 affected Matrix tests pass: 65 library, 6 download, 5 upload and 9
transport. Native and Windows GNU all-target Clippy pass with warnings denied;
the first Clippy run requested one equivalent boolean simplification, corrected
without changing policy. Windows cross-check is compilation evidence only;
actual Windows execution remains an integration CI gate. Formatting and diff
checks pass. Strict crate-scoped agent-spec lifecycle passes 6/6 (five bound TLS
selectors plus the explicit eleven-path boundary), with zero failed, skipped,
uncertain or pending-review verdicts. External media-upload-* logs retain the
original failures and successful checks. No full workspace build or live
service was needed.


## 2026-09-10 — Integrated usage capture and upload checkpoint

The isolated migration branch now includes ADR071 typed counters(e45043c),
ADR070 exact owned capture(22503a1) and ADR072 encrypted upload(0f18bdc).
The upload merge had only an append-only progress-note conflict; all parent and
agent lines were retained and checked. Cargo adds one local execution-to-metering
edge and two Matrix test-only edges, with no package/version additions.

At22503a1 the locked workspace passes441 unique tests plus one proxy child,
83 suite summaries and zero failed/ignored. All301 selectors resolve and full
workspace warnings-denied Clippy plus affected Windows GNU Clippy pass. Integrated
ADR071 lifecycle passes6/6 across8paths, separately from ADR070's10/10 across20.
After upload integration, all85 affected Matrix tests pass again from the edited
root tree. The new shared HTTP path is independent of JSON and download behavior;
parent review required and confirmed the final cancellation/deadline check before
Accepted. Workspace warnings-denied Clippy passes, and integrated strict upload lifecycle
passes6/6 across all11 explicit changed paths. The new fixture is not a live
homeserver or an actual Windows runtime qualification; fresh CI remains required.

Uploads still lack durable staged-byte recovery, event authority, file-tool and
service integration. A possible HTTP write is not retry permission. Native owner
approval application, full room/history behavior, physical provisioning/sandbox,
quotas, console and release parity remain open; no M0-M9 milestone is complete.

Integrated upload binding enumeration resolves306 Rust selectors with no missing
tests. The final documentation scope check also passed10/10, including its three
explicit paths; agent-spec executed the bound tests despite the requested
lint/boundary layers. This rerun is recorded separately, not as new coverage.

## 2026-09-10 — Authenticated attachment visibility implementation checkpoint

ADR073 now adds domain schema018: safe metadata and a private SDK manifest
reference commit with exact verified Matrix ingress. New dispatches freeze both
source and projection sequence. Host-only tickets require the current Started
capability and exact route, with a separate revalidation operation for use after
asynchronous IO. Verified task activation copies only existing proven input;
follow-up dispatches retain earlier authorized files without same-room access.

Five focused repository tests pass, covering atomic rollback, finite admission,
changed replay, late projection, follow-up lineage, cross-Agent source conflict,
null-root DM promotion, expiry, revoke, restart and schema17 upgrade. Original
fixture errors are retained externally: wrong revoke signature, missing capacity
fixture foreign-key rows, then omitted SQL column order. They were corrected in
the tests without weakening schema constraints. Store all-target warnings-denied
Clippy passes. Full affected store regressions and strict lifecycle are pending.
No download, runner tool, cache path or production service is enabled. Matrix
authenticated descriptor retention is proceeding separately as ADR074.

ADR073 pre-integration review found that the initial source cutoff included every
already queued session input, rather than only the selected dispatch trigger.
The corrected path starts with a deny-all window; first inbox enqueue freezes
the highest actual selected sequence plus current projection cutoff. Replay never
changes either cutoff. The focused five tests pass again, including a file already
queued after the selected trigger and exclusion of later input/projection.

Before this correction, all139 affected store tests, Windows GNU Clippy and
strict lifecycle6/6 across24 explicit changed paths passed. These results do not
substitute for verification of the correction, which will be rerun in integration.

## 2026-09-10 — Windows worker failure evidence

Native run34549222503 at a856aa5 passed Linux/macOS but failed Windows; Node
run34549222562 passed. The original Windows suite printed442 passes plus two
failures (the pass count includes one proxy child). Both failures were in the
store library: queued completion cancellation returned OutcomeUnknown only at
shutdown, and the outbound concurrency fixture counted two claims. All original
Matrix outgoing/media tests passed; their later successful diagnostics cannot
replace the failed store suite. Original complete logs are retained externally.

ADR075 corrects the concurrency fixture's100ms lease assumption while keeping
its exact one-claim assertion. Its explicit valid120s lease must remain unexpired
through the check; no production deadline changes. A separate actual writer
regression models200ms host submission age and proves an expired100ms claim may
be replaced while its old ticket and duplicate Start both fail. Four affected
worker selectors pass. The historical CI timing is not recorded, so the exact
original timing cause remains an inference, not a reproduced Windows diagnosis.

Queued completion shutdown now reports the existing fixed phase snapshot on the
original error and keeps its two unchanged two-second waits. Its historical
shutdown cause remains unknown. Fresh Windows execution is still required.

ADR075 strict lifecycle passes5/5 across its six explicit changed paths, with
zero failed/skipped/uncertain/pending verdicts. This is local fixture validation,
not an actual Windows rerun or an explanation of the historical shutdown.

## 2026-09-10 — Authenticated encrypted attachment manifests

ADR074's isolated Matrix slice uses root ADR073's host attachment observation and
private ticket APIs. Actual SDK verified m.file/m.image events retain original
ciphertext source and encrypted descriptor in a finite independent journal map
before domain handoff. Domain input contains only validated filename, optional
MIME/declared size and opaque digests. The source-content digest is shared across
receivers; each manifest binds the receiver SDK fingerprint and exact full route.
No intake/history operation downloads media. Existing terminal Unsupported source
receipts remain terminal after the new syntax support.

Safe host lookup checks the current dispatch ticket before and after SDK work.
One total deadline/cancellation bounds lock, open, queue and both domain checks.
The eight-held-result pool survives Owner reopen. A deterministic test promotes
a null-root DM after secret lookup and proves that post-await validation discards
the result. Handles retain private custody; they do not authorize future download
or expose descriptors to runtime/console serialization.

Six new actual SDK/TLS selectors exercise verified file/image metadata, group
mentions and DM wake, exact replay/lost acknowledgement/restart, old refusals,
invalid descriptors/MXCs/metadata, 128 manifests plus refusal, retained handle
capacity, cancellation/deadline, generation/promotion and actual SQLite commit
abort. Full affected Matrix tests pass 91/91 (71 library, 6 download, 5 upload,
9 transport). The SQLite fault fixture initially expected HTTP after the existing
transport fence; its original failure log is retained, and the corrected fixture
requires Generation with no request. Initial Clippy requested a boxed ticket
variant and collapsed condition; fixed without policy changes. Native and Windows
GNU all-target Clippy pass with warnings denied. The updated full-capacity exact
replay passes, as does strict crate-scoped lifecycle: 7/7 (six bound tests plus
fourteen explicit changed paths), zero failed/skipped/uncertain/pending review.
Formatting and whitespace checks pass. Logs remain in external attachment-intake-*
files. Windows cross-compilation is not actual Windows runtime qualification;
integration CI must execute these fixtures on that platform.
No Matrix service, receive cache, MCP file tool or restored upload is enabled.

## 2026-09-10 — Integrated authenticated attachment checkpoint

Root3a9616a integrates ADR073 (a005ba2/21fc7a9 plus selected-trigger correction
7ecdc12), ADR074 (agent1cf5d00) and ADR075 Windows fixture evidence (5050292).
The two documentation conflicts contained independent insertions only; a checked
three-way merge preserved every nonempty line from both parents. The original
checkout and live services remain untouched.

The locked full workspace passes458 unique tests plus one proxy-environment
child (459 printed),84 suite summaries, zero failures and zero ignored. All321
Rust specification selectors resolve without missing tests. Full all-target
warnings-denied Clippy, rustfmt and diff checks pass. Integrated scoped lifecycle
passes ADR0736/6 across27 changed paths and ADR0747/7 across14 paths; ADR075's
separate5/5 across6 paths also passes. Counts overlap; no fail/skip/uncertain
result is included as a pass. External attachment-integrated-* and integrated
lifecycle logs retain the evidence.

Latest completed remote qualification remains a856aa5: Linux448 printed passes,
macOS447 (each includes one proxy child),84 suite summaries and successful
Clippy/release. Windows printed442 passes and two store failures; its release
was skipped after failure. Node passed. The older all-platform green run remains
historical evidence only. Fresh Windows verification is required after ADR075.

Native host file download orchestration is the next bounded slice (ADR076);
SDK manifests and safe metadata do not themselves expose a model-readable file
or a cache path. Durable upload recovery, full file tools, approval application,
physical provisioning/sandbox, complete room/history policy, console/service,
quotas and release parity remain open. No M0-M9 milestone or migration completion
is claimed.

## 2026-09-10 — Exact encrypted staging restoration (ADR077)

The private media Store now restores a distinct RestoredEncrypted from the
original operation and receipt digest. It reuses the committed frame validation
and finite result pool, moving original ciphertext, descriptor, receipt and
namespace without encryption or a second payload copy. Clean recovery and
FileAndDirectorySynced evidence are required. Wrong operation, digest, kind or
namespace cannot replace the original data. Incomplete tails and observed
corruption remain quarantined. Windows unconfirmed directory sync explicitly
refuses this typed restoration while leaving ordinary inspection available.

The actual SDK ciphertext survives close/reopen and source replacement unchanged;
all12 media-store tests pass, including four new contract selectors covering
exact recovery, identity refusal, shared held capacity and actual corrupt or
interrupted journals. A negative-only sync-evidence fixture never manufactures a
positive platform qualification. OS flush evidence does not prove hardware
power-loss durability; handles and capacity remain host-local/per-Store.

There is no restored upload entry point or automatic retry. The original domain
operation, durable WritePossible before HTTP, accepted repository custody and
unknown-write recovery are still required before safe upload restart handling.
File tools, service cutover and migration parity remain open.

Independent review found no blocker. Its suggested complete-frame interrupted
write cases now also pass: same-owner unknown refuses restoration, and actual
clean validated reopen can recover the earlier original receipt under real
sync evidence. Final strict lifecycle passes5/5 across11 explicit changed paths,
zero failed/skipped/uncertain/pending review. Native warnings-denied Clippy and
format/diff checks pass after that addition. Windows GNU Clippy passed the
production slice and initial four tests; actual Windows restoration evidence
remains for integration CI. External media-restoration-* logs and the independent
review preserve the exact qualification boundaries.

## 2026-09-10 — Current-dispatch encrypted attachment receive

ADR076 in an isolated worktree adds Collector::receive_attachment with an opaque
held checked-byte result. The original event determines a sealed current dispatch
ticket and retained SDK manifest. Configured-origin HTTPS download and complete
hash/decryption precede final writer revalidation. No caller chooses a URL, room,
descriptor or cache path. Four early result slots and a lazily shared downloader
remain bounded across SDK Owner reopen. Lookup releases its Owner/busy locks before
GET, allowing actual negative room collection while a download is paused.

Six selectors cover actual verified file/image intake, DM and group-thread task
lineage/follow-up, selected-input visibility, real paused-download revoke/promotion/
negative encryption/expiry, hash and truncated-body failure, cancellation/future
drop, retained capacity and one total deadline. Notice activation inside the
lineage fixture is explicit host domain evidence, not an actual Matrix notice send.
The100ms blocked-Owner test retains its strict bound. The separate actual TLS
deadline test uses normal10s SDK preparation/total with longer HTTP sublimits,
avoiding an assumption that SQLite/SDK work finishes before100ms on loaded CI.

First compile caught two test-future borrow lifetimes, fixed with explicitly owned
futures. Initial runtime5/6 passed; revocation behavior passed but its teardown
called Collector::close after the engagement no longer allowed that mutation.
The fixture now closes its retained SDK and asserts the existing Domain refusal;
production cleanup was unchanged. Corrected focused6/6 and affected Matrix97/97
(77 library,6 download,5 upload,9 transport) pass. Native all-target warnings-denied
Clippy passes. Original logs remain externally under matrix-receive-*; final
deadline fixture and strict lifecycle validation are recorded separately below.
No staging cache, safe path/MCP output or production service is enabled.

Final ADR076 deadline-stage selector passes against the actual held TLS request.
Windows GNU all-target Clippy also passes with warnings denied; this is compile
qualification only, not actual Windows execution. Strict crate-scoped lifecycle
passes7/7 (six exact selectors and ten explicit changed paths), with zero failed,
skipped, uncertain or pending-review verdicts. Formatting and whitespace checks
pass. Full combined tests and three-platform runtime qualification remain the
parent integration/CI step; the local affected result is97/97, with the subsequent
deadline-only fixture refinement covered by focused execution and lifecycle.

## 2026-09-10 — Integrated receive and encrypted recovery checkpoint

Root8a6bb3a adds ADR077; a96fd30 integrates agent ADR0765b7fdef. The progress
conflict contained independent append sections, and resolution preserved every
nonempty line from both parents. Independent review found no production blocker.

Locked full workspace now passes468 unique tests plus one proxy-environment child
(469 printed),84 suite summaries, zero failed/ignored. All331 Rust selectors
resolve without missing tests. Full all-target warnings-denied Clippy, rustfmt
and diff checks pass. Integrated lifecycle checks are recorded below when complete.
Evidence remains in external file-receive-integrated-* logs.

Native34551463344 at preceding ff8be6b is fully green on actual Linux, macOS and
Windows, including original tests, Clippy and release builds. Their printed
counts are460/459/456 respectively, each includes one proxy child and84 summaries.
Both Windows failure-only diagnostics were skipped. Node34551463340 also passes.
Historical a856aa5 Windows shutdown cause remains unproven. ff8be6b does not yet
qualify these new ADR076/077 runtime cases; fresh integration CI is required.

Next work runs in separate clean worktrees: ADR078 plans durable upload domain
custody; ADR079 retains an original encrypted staging commitment before its first
write. Neither turns a possibly written POST into retry authority. No live
service or production Matrix/model connection changed. Migration parity and
M0-M9 completion remain unclaimed.

Final integrated strict lifecycle passes ADR0767/7 across10 explicit changed
paths and ADR0775/5 across11, with zero failed/skipped/uncertain/pending review.
The four new restoration selectors include the reviewed complete-frame unknown
write boundary. Exact external file-receive-integrated-lifecycle and
file-restoration-integrated-lifecycle records preserve these outcomes.

## 2026-09-10 — Original encrypted commitment before staging IO (ADR079)

A clean worktree from a96fd30 adds PreparedEncrypted custody before any journal
mutation. It consumes actual codec Encrypted, computes the original stable frame
digest and retains bounded operation/namespace identity plus the same finite
per-Store result permit used by reads. The host can record this exact commitment
in the domain before calling stage_prepared. No domain or network call is made
by the storage primitive. An intervening append changes the journal chain without
changing the prepared content identity. Replaced source bytes cannot alter the
retained original ciphertext or descriptor.

Prepared staging validates the original Store owner, not only namespace equality,
and rechecks recovery/durability/capacity. Different or reopened owners refuse.
A pre-write refusal returns the exact encrypted Media; a real refused OS write
after admission retains material and OutcomeUnknown in the original Store.
Prepared memory slots do not reserve journal records. Positive preparation and
restoration remain unavailable when Windows directory sync is unconfirmed;
those branches explicitly test refusal, not a successful platform round trip.

All16 local media-store tests pass, including four new contract selectors for
original identity, capacity, owner binding and failure custody. Native
warnings-denied Clippy passed the initial implementation. Final reviewed
reopen-evidence handling and strict lifecycle are recorded below when complete.
Actual Windows execution remains an integration CI obligation.
No upload retry, filesystem path export, runtime tool or service is enabled.

Final strict lifecycle passes5/5 across9 explicit changed paths, with zero
failed/skipped/uncertain/pending review. Final native warnings-denied Clippy,
rustfmt and diff checks pass after the reviewed reopen-sync evidence handling.
External media-preparation-* logs retain exact results. Windows GNU compile and
actual platform runtime checks remain the root integration step.

## 2026-09-10 — Windows media fixture lock ownership (ADR081)

Native34553424733 at25c01ee failed Windows:461 printed passes plus5 failures
across84 original suite summaries. Linux470 and macOS469 printed passes are
green (each platform count includes one proxy child); Node34553424721 passes.
Three new restoration cases fail on fs::read with actual Windows error33 because
a separately opened handle cannot read the exclusively locked journal. Two
existing Matrix transport fixtures time out waiting for their script and obscure
the original Collector outcome. Actual new receive cases passed on Windows.

A bounded test-only helper now reads the live journal through its original
Store file handle and restores the cursor. ADR077 and unpushed ADR079 byte checks
use this owner; independent path reads occur only after owner closure. Exact byte,
corruption, namespace, capacity, returned-material and recovery assertions stay
intact. Production file locks, sharing modes and storage behavior are unchanged.
All16 local media-store tests plus native and Windows GNU warnings-denied Clippy
pass. Actual Windows closure requires the fresh integrated run.

The original Palpo suite in that run passed13/13, while a later serial diagnostic
failed1/13 at custody Store shutdown before reopen. Its OutcomeUnknown cause is
unproven and separate from these media fixtures. ADR080 improves Matrix fixture
early-error visibility; ADR082 adds bounded custody shutdown phase observation.
Their diagnostics must preserve original verdicts and waits. No production
readiness or whole migration completion follows from this fixture repair.

ADR081 strict lifecycle passes8/8 across7 explicit changed paths, zero failed,
skipped, uncertain or pending review. Formatting and diff checks pass; original
Windows runtime failures remain open until the next actual Windows CI.
ADR079 also passed root integration16 tests, native/Windows GNU Clippy and
strict5/5 across9 paths before this fixture correction. Independent preparation
review found no production blocker; byte-level Windows fixture portability was
revealed by the subsequent actual OS run.


## 2026-09-10 — ADR078 durable encrypted upload registry

Implemented core data contracts, opaque host preparation/identity/claim/send
handles and domain schema19 under the existing single writer. Reservation replay
never grants another capture, immutable staging binds before actual IO, and
WritePossible never rearms after timeout, cancel, revoke or restart. Current
checks derive exact Started task/epoch/exclusive workspace and full encrypted
route. Historical acceptance stores only a private receipt commitment and cannot
revive current execution. Public receipts contain no owner/room/workspace/media
secrets. The domain does not verify physical staging by itself; ADR079 and a future
transport owner must supply actual evidence and consume the send grant once.

The contract was parsed/linted before implementation. Initial compile caught
queue-weight opaque temporary borrows; bounded private JSON accounting resolves
them without exposing an authority serializer. Initial repository5/6 passed;
negative-room fixture used the old generation and was correctly refused. The
fixture now uses the existing generation2 invalidation API, production unchanged.
Corrected repository6/6 and actual writer2/2 pass, including lost successful
responses, concurrent claims/begins and lease expiry while queued. Native and
Windows GNU core/store all-target warnings-denied Clippy pass; the Windows result
is compile qualification, not actual Windows execution. Full store regressions
and strict lifecycle evidence follow. Logs are external under upload-* in the
2026-09-10 migration cache. No network, models, service settings or live data changed.

Final ADR078 complete store regression passes148 tests across19 suite summaries,
zero failed or ignored. Strict crate-scoped lifecycle passes8/8 across23 explicit
changed paths, with zero failed/skipped/uncertain/pending-review verdicts. Both
native and Windows GNU all-target core/store Clippy pass with warnings denied;
rustfmt and diff checks pass. Negative SQL epoch/owner/workspace fixtures are
explicitly distinguished from real room/transport/revocation host APIs. Actual
three-platform execution and wider native integration remain the parent CI step.


## 2026-09-10 — Matrix transport fixture error visibility (ADR080)

Windows CI at 25c01ee failed the authenticated HTTPS/restart and bounded
sync/cancellation tests at the fake peer's SDK-plus-HTTP request timeout. The
joined collector result was not visible, so the historical cause remains unknown.
Both identity collections and the bounded sync loop now use the existing
script-first driver. Early collection completion fails with its actual result;
all request assertions, authentication, SDK identity, cancellation and negative
authority checks remain intact. No deadline or production behavior changed.

The accepted contract parsed and linted before code changes. All nine local
transport tests passed, including the actual early-Identity and legal SDK-gap
regressions. Transport-target Clippy with warnings denied, rustfmt and diff checks
passed. Strict agent-spec 1.4 lifecycle passed 5/5 across all five explicit changed
paths, with zero failed, skipped, uncertain or pending-review verdicts. External
matrix-fixture-errors-* logs retain these results. Original Windows CI failure and
independent Palpo shutdown OutcomeUnknown remain failed evidence. Actual Windows
execution is still required; this change does not establish its historical cause.


## 2026-09-10 — ADR082 custody shutdown phase observation

The25c01ee original Palpo target passed13/13; its later serial diagnostic passed
12/13 and failed1/13 at the first Store::shutdown before publication reopen. This uses the custody
worker, not DomainStore. Read-only audit could not determine whether its unchanged
wait expired before pickup, during repository release or around acknowledgement.
Original evidence remains external in windows-25c01ee-original.log and the Palpo
shutdown review; no production cause is inferred from this uninstrumented result.

In a clean72e73cb worktree, parsed/linted the six-path ADR082 contract first. Added
optional per-job reuse of the existing fixed Probe in custody Store. Ordinary
shutdown remains unobserved; original waits, Result, Drop-before-ACK and ownership
ordering stay unchanged. One Palpo fixture failure now prints a static label and
phase snapshot while still failing. Deterministic actual worker gates distinguish
queue, Drop and ACK boundaries without arbitrary sleeps or successful retries.

Focused native store/Palpo tests and Clippy are being recorded externally under
custody-shutdown-*. Complete strict cross-crate lifecycle is explicitly pending
parent integration; a partial crate lifecycle cannot prove the Palpo selector.
No workspace-wide build, live network, service setting or credential changed.

Final focused results: new phase tests3/3, existing worker tests3/3, outbound
custody regressions10/10, and the exact Palpo publication/restart test1/1 pass
(17 distinct tests). One preliminary invocation used a nonexistent outbound
integration target; the retained invocation error was corrected to its actual
library test module. Native and Windows GNU all-target store/Palpo Clippy pass
with warnings denied. Parse/lint, rustfmt and diff checks pass. Complete strict
cross-crate lifecycle remains pending parent integration by explicit coordination;
no partial lifecycle or historical Windows timeout is reported as passing.


## 2026-09-10 — Integrated upload and Windows diagnostic checkpoint

At8fce06e the complete locked native workspace passes483 unique tests plus one
proxy child (484 printed),85 suite summaries, zero failed/ignored. Full all-target
Clippy passes with warnings denied. All357 Rust specification selectors resolve;
formatting and diff checks pass. Logs are external upload-integrated-*.
ADR078 strict root integration passes8/8 with all23 changed paths. ADR082 runs
from the workspace root, passes5/5 with all6 changed paths, and includes both the
actual custody-worker tests and actual Palpo publication/restart selector. No
failed/skipped/uncertain/pending-review verdict is counted as passing. Earlier
ADR0795/5 across9 paths and ADR0818/8 across7 retain their separate results.

Independent read-only ADR078/079 review found no concrete blocker in the accepted
primitive scopes. It traced commit-before-grant, exact staging ownership, current
lease/route checks and historical settlement. It also confirmed the real Matrix
uploader still consumes neither UploadSend nor RestoredEncrypted; integrated upload
qualification is not claimed. ADR083 sealed response custody and ADR084 protected
SDK upload journal are proceeding independently in separate clean worktrees.

Latest pushed25c01ee native34553424733 remains an original Windows failure, with
Linux/macOS and Node34553424721 passing. ADR081 fixes the confirmed test-only
mandatory-lock read bug; ADR080 exposes early transport outcomes; ADR082 preserves
shutdown failure while recording phases. Actual fresh Windows execution remains
required. No live service, account, credential, model call, original checkout or
production cutover was touched. No M0-M9 or whole migration completion is claimed.

### ADR083 — exact encrypted-upload response custody (2026-09-10)

In a clean8fce06e worktree, parsed/linted the eight-path contract before code.
Http::upload now retains actual complete checked response body bytes, body SHA256
and checked MediaId in a sealed host value owned by the existing finite attempt.
The borrowed observed_response getter is historical evidence; the existing
media_id getter remains successful-current-result only. Late cancellation or
timeout preserves possible-write state and the original error. Invalid, partial
or failed-EOF bodies never gain response evidence. No HTTP bounds, journal,
domain/API/service state, descriptor custody or retry semantics changed.

The first focused compilation exposed a test-only use of a nonexistent MediaId
as_str method; corrected to the existing to_mxc projection and preserved the
original compiler log. Exact TLS tests, strict lifecycle and native/Windows GNU
Clippy evidence are recorded externally under upload-response-*. No live server
or credentials are accessed. The separate private journal remains ADR084 work.

Final ADR083 validation: all 6 actual uploader tests pass; strict scoped lifecycle
passes all 5 scenarios plus the eight-path boundary (6/6, no skips or uncertainty).
Native and Windows GNU Matrix all-target Clippy pass with warnings denied;
rustfmt and diff checks pass. Windows cross-compilation is not hosted execution.
The late synchronous final-check branch has no deterministic fixture hook and is
explicitly supported by source-order review, not a race-dependent test or a claim
of durable recovery. Parent review found no blocker in the bounded change.


## 2026-09-10 — Native process-scope progress observation (ADR086)

Original macOS7cf0dc0 CI34556196644 failed process_scope start_stop at its80ms
unrelated-heartbeat sample; the log has no native liveness observation. The
historical cause remains unknown. Root traced the actual owned process identity
and stop paths, then replaced only this test sample with native liveness before
and after fresh heartbeat observation under a strict three-second deadline.
A stopped process fails immediately; alive without progress still times out.
Original owned cancellation, repeated-stop, argv/environment and crash checks stay.

The five-path contract parsed/linted before implementation. Actual pausable native
child coverage demonstrates unchanged short-sample bytes while still alive, a
real no-progress timeout, resumed fresh progress and refusal after actual stop.
The first four process_scope tests pass; final lifecycle and native/Windows GNU
Clippy after strict final-deadline review are recorded externally as
process-progress-*. No production lifecycle, stop deadline or live state changed.


Final ADR086 strict lifecycle passes5/5 across all5 changed paths, with no failed,
skipped, uncertain or pending-review verdicts. All four actual process-scope
selectors ran after the strict final-deadline check was added. Native and Windows
GNU all-target platform Clippy pass with warnings denied; fmt/diff pass. Actual
hosted qualification remains pending; controlled pause/exit evidence does not
identify the original macOS CI schedule or turn its failure into success.

## 2026-09-10 — Protected upload acceptance SDK journal (ADR084)

Created an isolated8fce06e worktree, read the accepted staging/upload boundaries
and external acceptance-journal design, and parsed/linted the bounded contract
before source changes. Incorporated ADR083's committed sealed-response dependency
without editing its files. The SDK now owns a separate encrypted custom journal,
versioned main-journal marker, exact owner-bound one-use reservation and finite
response custody. Historical exact-id lookup restores bounded metadata only.

The first six actual SQLite/SDK and local TLS tests passed. They cover original
raw whitespace/escaping through reopen, lost acknowledgement, stale owner permits,
negative exact identity, cancellation/historical settlement, a real SQLite abort,
corruption/torn bootstrap and capacity independent of main-journal growth. Added
lost reservation/Possible acknowledgements, decoded malformed-route validation,
maximum body capacity and exact historical lookup before final verification.
Production entry points remain deliberately unwired and carry narrow dead-code
expectations only at future-coordinator methods. Final checks are recorded below.

Uncommitted positive bytes remain bounded in the poisoned owner only until close;
no process-global recovery map exists. An original host-retained UploadAttempt may
later resubmit its exact sealed response after validated SDK reopen. This does not
repeat HTTP, recover a lost original attempt after process death, or prove actual
response-to-operation association. Domain-only staging observations in fixtures
qualify the real registry boundary, not physical ADR079 staging or Windows sync.

Final contract verification passed7/7: all six explicit SDK scenarios and the
boundary check covering eight exact paths, with zero failed/skipped/uncertain or
pending-review results. Native and Windows GNU all-target Clippy pass with
warnings denied; rustfmt and diff checks pass. Strict lifecycle's initial added
fence fixture expected Storage although the validator returned Identity; its
failed report is retained externally, and the corrected fixture now asserts that
exact refusal. A real issued601-byte opaque thread root verifies compatibility
with domain SessionBinding syntax while the complete identity remains capped.
Global64 memory custody covers queued/uncommitted copies; each SDK's permanent
ledger separately bounds committed/restored bodies. No global retained-body bound
or native Windows runtime durability result is implied by cross-compilation.

The final full Matrix regression passes104 tests:83 library,6 download,6 upload
and9 transport, with zero failures or ignored tests. External upload-custody-*
logs retain final parse/lint, focused/full test, native/Windows GNU Clippy and
strict lifecycle results. Native Windows execution and a consuming cross-crate
upload coordinator remain separate qualification work.

### ADR085 — historical upload settlement after process loss (2026-09-10)

Read-only audit found restore_upload requires the original random runner secret
and full request, while protected stores retain only their digests. Prior reopen
fixtures kept the capability in memory. Saved exact source trace and SDK discovery
limits in external next-upload-restart-settlement-review.md.

In a clean43fcb8e worktree, parsed/linted the nine-path contract before code. Added
sealed UploadSettlement restored only by exact protected row ID/fence/stage/full
route matching in staged WritePossible/Accepted. It cannot convert to identity,
preparation, claim, send or runner capability. Historical inspect/record recheck
all fields; cancellation and exact receipt replay remain monotonic. No schema,
Cargo, SDK, Matrix, runtime/API, service or current authority gate changed.

Five focused tests pass, including actual writer/settler/replay child processes
with no original capability/request/secret passed across process boundaries.
Domain fixture acceptance is opaque synthetic host data and proves no real SDK
provenance. The first test compilation exposed an integration module path typo;
corrected its explicit path and preserved the original compiler log. Full store,
strict lifecycle and native/Windows GNU Clippy evidence is external under
upload-settlement-*. The consuming SDK coordinator remains separate work.

Parent review found no production blocker and requested checking issued-route
compatibility. That exposed a stricter-than-original 255-byte thread-root check;
replaced it with the existing SessionBinding validation under the command bound.
An actual issued 601-byte opaque thread root now restores, while oversized input
is refused before copying. The five focused tests pass again. The first complete
store run passed 156 tests; final affected validation follows the route correction.

Final ADR085 validation: complete locked store regression suite passes 156 tests
across 19 summaries, including five new settlement tests and three actual child
process phases inside the process fixture. Strict scoped lifecycle passes five
scenarios plus the complete nine-path boundary (6/6, no skips or uncertainty).
Native and Windows GNU store all-target Clippy pass with warnings denied; rustfmt
and diff checks pass. Windows cross-compilation does not replace hosted runtime
qualification. SDK provenance, exact-reference startup discovery and the actual
consuming network coordinator remain unimplemented by this domain-only slice.

## 2026-09-10 — Node launch retry wake cutoff (ADR087)

Original Node CI34556196694/job103129331815 at7cf0dc0 failed only
`backend requeues a wrapper that dies before takePayload without losing its input`
at the30000ms Vitest deadline. The original log and uploaded JSON artifact retain
4287 passed,1 failed,1 skipped. Neither contains an inner phase or dispatch row;
the historical cause remains unknown. No Node runtime source changed from prior
passing25c01ee, which is context rather than proof of a cause.

Read-only trace and an external disposable SQLite reproduction confirmed a
specific scheduler gap: claim at1800000000099 returned null, the retry became due
at1800000000100, and the later future-only wake lookup also returned null. The
row stayed queued with one launch failure and its original input; a later
explicit claim succeeded. No process timing, live runtime or network was needed.

In clean43fcb8e worktree, agent-spec1.4 parsed and linted the10-path contract
before implementation. The new combined claim/wake API reuses the actual claim
transaction's eligibility cutoff. Existing predicates, leases/capabilities,
standalone APIs and generated-build workflow stay intact. The pump consumes the
combined wake; due blocked rows cannot supply an immediate timer spin. The old
real-wrapper test adds fixed-stage/state/numeric failure observation only.

Validation: deterministic clock, blocked/future work and claim compatibility
checks pass3/3; full affected router-core67, actual recovery1 and Claude runtime20
pass88/88 with no skips. Typecheck, normal generated router build comparison,
router boundary, affected ESLint and diff checks pass. Exact contract selectors,
parsed-path boundary and original evidence are retained externally under
node-retry-* and node-7cf0dc0-*. Parse/lint quality is96% with coverage/output
wording advisories; Node's Cargo-only lifecycle cannot verify these scenarios,
so no native lifecycle success is claimed. Parent integration and fresh hosted
Node execution remain separate gates. No live services, credentials, original
checkout, merge or push changed.


## 2026-09-10 — Integrated upload history checkpoint

Root97968d8 includes actual response custody43fcb8e, native progress fixture
a22707f, protected SDK uploads b0553e8, full-process historical settlement9b9078f
and Node claim/wake cutoff97968d8. Independent final source review found no
blocker within these scoped primitives; the consuming transport remains separate.

Complete locked native workspace passes496 independent tests plus one proxy child
(497 printed),85 summaries, zero failed/ignored. This count includes the expected
early-refusal panic selector. All377 Rust specification bindings resolve. Full
all-target warnings-denied Clippy, rustfmt and diff checks pass. Root ADR084 strict
lifecycle passes7/7 with all8 changed paths; ADR085 passes6/6 with all9. No failed,
skipped, uncertain or pending-review scenario is counted as passing. External
settlement-integrated-* and upload-*-integrated-* retain exact outputs.

Latest remote7cf0dc0 native34556196644 failed Windows and macOS; Linux passed.
Original85-summary suites are retained separately: Windows475 printed passes and
6 failures, macOS483 and1, Linux485 and0, all zero ignored. Mandatory-lock media
restoration and original Matrix transport integration now pass Windows. Four
Matrix library failures are DomainStore shutdown, one is intake after its HTTP
script, one is a missing scripted request; historical causes remain unknown.
ADR088 adds original phase observations without timeout changes. ADR086 retains
a real native-liveness/fresh-progress assertion for macOS, not a production fix.

Original Node34556196694 has4287pass1fail1skip. The independently confirmed
claim/wake race now passes its focused regressions. Full local validation exposed
the backend source fingerprint/line-offset inventory drift and separate earlier
Git/version/initialization timeouts. Inventory regeneration is in progress; the
original timeout phases lack complete causality evidence and remain failures.
No live services/models/accounts/credentials or original checkout were modified.
Native file tools, full upload coordination and production migration remain open.

## 2026-09-10 — Exact staged-upload input association (ADR090)

In clean97968d8 worktree, parsed/linted the eight-path contract before code.
Added full private identity/fence association on the original UploadSend and
a borrowed original namespace digest on actual RestoredEncrypted. Neither checks
current permission or reconstructs a grant. Existing current validation, journal
construction, durability evidence and sealed types remain unchanged.

The actual domain regression passes with two same-fence uploads, a replacement
fence, cancellation and independently valid unrelated work. The media fixture
now compares the retained original namespace after source replacement and owner
close alongside unchanged ciphertext/descriptor checks. Final cross-crate strict
lifecycle and Clippy evidence is external under upload-input-*. No production
service, schema or remote communication changed. ADR089 consumes these seams.

Final ADR090 validation: all12 domain file-upload tests and all16 media-store
tests pass, zero failures/ignored. Strict cross-crate lifecycle from the edited
workspace root passes5/5 across all8 changed paths, zero failed/skipped/uncertain
or pending-review. All-target Clippy for both affected crates passes with
warnings denied; rustfmt/diff checks pass. Hosted Windows execution remains
separate; no cross-compilation or new durability claim is made here.

### ADR087 — complete local verification and reviewed inventory correction

The first required verify:ci at5e7e31f failed two remote-autodeploy fixture setup
commands: initial local `git push -u origin stable` and `git commit -m initial
stable`. Kernel totals506passed/2failed; static checks and542Node bindings passed.
Two live-runtime gates skipped on absent HAGENCY_RUNTIME_DIR. The fixture has a
15000ms command budget, but the reporter retained no error code/signal/stderr;
its afterEach removed temporary roots. Local identity is explicitly configured,
origins are unique temporary bare repositories, and read-only config inspection
found no signing/hooks override. The exact subprocess cause remains unknown.

The separate first complete test:ci failed5 tests with4286passes and1platform
skip. The launch-recovery fixture and all8autodeploy tests passed in that run.
Failures were the source inventory mismatch, framework-acp-probe's fake Claude
version probe timeout, and3fake Codex initialize timeouts before MCP approval
scenarios. Those timing causes are not established; no timeout or assertion was
changed, and no repeat success replaces these original failures. Logs/JSON use
node-retry-verify-ci and node-retry-full-test prefixes in the external cache.

The inventory omission is confirmed and corrected under the extended12-path
ADR087 contract, parsed/linted before regeneration (97% with wording advisories).
Read-only actual-vs-reviewed comparison found397 changed leaves: backend hash in
2records plus395 source offsets all minus one. Regenerated through the existing
inventory writer and moved the exact SSE assertion7938→7937. All classifications,
counts, routes, methods and parity gates remain unchanged. Inventory check and
7focused tests pass: the5exact ADR087 scenarios plus2inventory negative-regression
tests;64nonselected tests are excluded, not scenario passes. ESLint/diff checks
and explicit parsed scope comparison pass. The post-correction complete suite
runs separately; original failed verification remains retained. No live runtime,
service, credential, merge, push or production timeout changed.

## 2026-09-10 — Original Windows Matrix failure observation (ADR088)

Read original7cf0dc0 Windows logs and traced all six library failures without
running tests or changing production. Four exact assertions were DomainStore
shutdown after successful Collector close; the attachment failure followed its
completed whoami/sync/state script, and the old-inspector request timeout had an
unprotected prime join. The external windows-7cf0dc0-review.md preserves exact
source/log references and explains why no production timing fix is yet justified.

Created a clean97968d8 worktree and parsed/linted the eight-path contract before
fixture edits. Original domain shutdown calls now use existing shutdown_observed
with fixed failure labels. The attachment loop records only bounded batch index
and fixed HTTP phase; script completion does not claim backend completion. Prime
uses existing common::scripted and an actual wrong-device local HTTP regression
checks early Identity visibility. No production, deadline, CI parallelism, raw
payload output or retry behavior changed. Validation is recorded below.

Final strict agent-spec1.4 lifecycle passes8/8: seven explicit selectors covering
all six original failing cases plus the new early-refusal regression, and all
eight declared file boundaries. There are zero failed/skipped/uncertain or
pending-review verdicts. One full Matrix run passes105 tests (84 library,6
download,6 upload,9 transport), with zero failures or ignored tests. Native
all-target Matrix Clippy with warnings denied, rustfmt and diff checks pass.
External windows-matrix-observation-* logs retain these checks. These are local
fixture validations, not native Windows root-cause proof; original7cf0dc0 remains
failed and its separate diagnostic reruns keep their original verdicts.

ADR088 review followup: the observed attachment branch still joined its script,
which could hide an early returned error behind Fake.next. Revised and
parsed/linted the contract before switching only that branch to existing
common::scripted. A real wrong-device observed intake now preserves Err(Identity)
and the latest fixed HTTP phase. Its focused regression and the existing manifest
bounds selector pass. The full105 run above preceded this followup; revised
strict lifecycle and Clippy results are recorded separately below.

Followup validation: strict lifecycle passes9/9 (eight selectors plus declared
eight-path boundary), with zero failed/skipped/uncertain/pending-review results.
Native all-target Matrix Clippy with warnings denied, rustfmt and diff checks
pass. Focused and lifecycle logs are retained externally with followup names;
there was no repeat full Matrix run or native Windows execution in this followup.
The original six Windows failures remain failed with unresolved inner causes.

Final ADR087 corrected-source verification at7debddc: npm run test:ci exits0 with
4291passed/0failed/1platformskip across293files. The skip is install-macos's
non-macOS refusal branch on this macOS host. Sequential npm run verify:ci also
exits0:543Node selectors resolve and all508kernel/CLI tests pass; the2live-service
gates remain skipped on absent runtime configuration. These results are distinct
from initial5e7e31f verifier and full-suite failures. External final logs/JSON use
node-retry-post-inventory-* and node-retry-final-verification-summary.json.

A separate bounded external observer then used the actual backend/helper with
all5framework commands replaced by disposable local fake binaries, including
hermes-acp. It refused any framework resolving outside the fixture before spawn.
All11observed which/version/ACP-help calls matched fixtures; Claude received only
which/--version, inherited the current PATH without an explicit env override,
and returned its exact fixed version with no error. This establishes the
controlled path behavior, not the executable or scheduling cause of the earlier
failed framework probe. No installed framework CLI or live model was invoked;
no diagnostic modified production source or timeout policy.

## 2026-09-10 — Retain enqueued Matrix invalidations (ADR091)

The ADR089 read-only review traced a separate domain custody defect: call's
receiver-closed guard discarded an already queued negative Matrix observation
when its caller dropped. Created a clean706172d worktree and parsed/linted the
six-path contract before implementation. Only transport and room invalidations
now select a private retained execution mode; ordinary calls keep their existing
cancellation guard, queue and byte permits, deadlines and transaction semantics.

Five focused actual-writer tests pass. Two agents share a real fixture room:
transport retirement affects the original agent's route, while room retirement
removes both routes. Delayed negatives preserve actual newer incarnations.
Dropped positive observations and canonical task creation remain unexecuted.
Both invalidation forms time out with the original OutcomeUnknown, then a
separate query observes their queued mutation after the held writer is released.
No live HTTP provenance or native Windows result is claimed. Strict lifecycle,
full store regression and Clippy verification follow below.

Final ADR091 validation: strict agent-spec1.4 lifecycle passes6/6 (five exact
selectors plus all six declared boundaries), with zero failed/skipped/uncertain
or pending-review verdicts. One full store all-target run passes162/162 across18
test binaries, zero ignored. Native all-target Clippy with warnings denied,
rustfmt and diff checks pass. Evidence and handoff are external under
matrix-invalidation-custody-* and adr091-matrix-invalidation-custody-handoff.md.
No hosted Windows run or original CI outcome was replaced by these local checks.

## 2026-09-10 — Retained staged upload owner (ADR089)

Created an isolated97968d8 worktree and parsed/linted the explicit16-path contract
before implementation. Integrated only parent ADR090 input association APIs as
01b824f. Original domain/media-store code remains parent-owned. Added one Matrix
path dependency and its existing lock entry, with no third-party version changes.

The owner now consumes actual qualified RestoredEncrypted plus the unique matched
Send, repeats constructor association at admission, authenticates current whoami,
records SDK Possible and checks current domain authority immediately before the
single POST. Complete response custody precedes persistence; exact SDK historical
receipts alone settle the domain. A bounded retained negative-identity completion
survives dropped callers. No runtime endpoint, room event or live service changed.

Initial source check exposed one local borrow conflict, and first test compilation
exposed an incorrect SessionBinding import. The first runnable six-case suite
passed3 and failed3 due to fixture generation/retired-close expectations; those
original logs are preserved externally. Corrected next-generation invalidation
and explicit revoked-close outcomes without changing production deadlines or
those domain rules. Review closed failed-constructor roundtrip bypass, current-
credential whoami requirement, unconditional task-not-Done assertion and abandoned
negative fencing. Six focused cases now pass, including an actual fresh process
with only persisted exact selector/config and private journals, malformed/truncated
HTTP, cancellation, release races, capacity and SDK write-failure/revocation recovery.
Latest focused log: staged-upload-tests-fourth.log (6 passed,3.75s). Complete
Matrix regression, strict lifecycle, native and Windows GNU checks follow.
Windows unconfirmed directory sync is an explicit refusal outcome with exact
custody/no-POST assertions and fixed diagnostics, not a positive upload pass.

Final local ADR089 evidence: complete locked Matrix suite passes110 tests
(89 library,6 download,6 upload,9 transport; no failed/ignored), with the fresh
process captured inside one library test rather than counted as another unique
case. Native and Windows GNU all-target Clippy pass with warnings denied; fmt and
diff checks pass. Strict lifecycle passed6 bound scenarios plus the complete
16-path boundary (7/7), without skips or uncertainty. A final wording rerun makes
platform refusal an explicit parsed And step rather than unparsed prose beginning
with Or. All macOS cases exercised the positive qualified staging branches.
Windows cross-compilation does not assert positive Windows execution; actual
unconfirmed-sync branches qualify refusal only. Original failed evidence remains
in staged-upload-check-first.log and staged-upload-tests-first/second.log.

Final independent review additionally found caller-mutable RunnerCapability
Strings could be retained before queue-weight rejection. Synchronous original
identifier/secret bounds now run before both construction and admission; actual
oversized-input/failure-roundtrip tests retain original ciphertext and refuse
without network. Added deterministic cancellation after complete response capture.
The stronger restart fixture aborts the first actual domain acceptance UPDATE
after private SDK Accepted, then drops every original owner/capability and starts
a fresh child. It commits first historical acceptance (replayed=false), followed
by exact replay (true), without HTTP. Six focused tests pass again (3.70s);
final affected suite/Clippy/lifecycle rerun follows these reviewed additions.

Post-review final rerun passes all110 Matrix tests again (89+6+6+9, zero failures
or ignored), and native/Windows GNU all-target warnings-denied Clippy pass.
Only the declared16 Matrix/dependency/contract/coordination paths changed in this
slice. Final exact strict lifecycle and fmt/diff checks are recorded immediately
before the reviewable commit; parent retains integration and hosted CI ownership.

## 2026-09-10 — Combined upload and invalidation integration

Integrated ADR091 as 6e4e7d4 and ADR089 as ed7c060 after root and independent
source review. Resolved only independent documentation append conflicts, retaining
both sides. No production service or live data was changed. The strengthened
upload fixture confirms actual SDK acceptance, a deliberately aborted first
domain acceptance, full original-owner teardown, and first settlement in a fresh
native process without another POST. Capability size bounds and constructor-failure
roundtrips are covered; the late validated-response cancellation case also passes.

At ed7c060, one locked all-target workspace run passes 510 unique tests plus one
proxy child (511 printed), across 85 summaries with zero failed or ignored tests.
Full workspace Clippy with warnings denied, formatting and diff checks pass.
All 400 Rust spec selectors resolve. Integrated ADR091 strict lifecycle passes
6/6 with all six actual changed paths, zero failed/skipped/uncertain/pending.
Integrated ADR089 strict lifecycle also passes 7/7 across all 16 actual changed
paths, with zero failed/skipped/uncertain/pending-review results.
External evidence uses staged-owner-integrated-* and
matrix-invalidation-integrated-lifecycle.*.

Original hosted results at pushed 706172d are now preserved: Node CI 34559411309
passes 4291 tests with one platform skip. Native CI 34559411277 passes Linux and
macOS, while Windows fails four Matrix intake cases. Original printed suite totals
are Linux 501/0, macOS 500/0 and Windows 493/4 passed/failed; all have 85 summaries
and zero ignored. No original Windows shutdown assertion failed. The four failures
retain a prime-sync wait, early collector OutcomeUnknown, completed manifest-batch
HTTP followed by OutcomeUnknown, and an intake receipt OutcomeUnknown. Subsequent
outgoing and transport diagnostics passed but are distinct observations. Original
logs and first-suite extracts use windows/linux/macos-706172d-original*; Node uses
node-706172d-original.log. Root cause of the Windows failures is not yet established.

The next service/MCP file-delivery design is being prepared in an isolated design
worktree. It must join actual entry points, physical root custody, bounded staging,
immutable publication metadata and separately acknowledged encrypted file events.
No new unimplemented selectors or service activation are included in this batch.


### 2026-09-10 — ADR093 retained workspace binding

Host now retains private workspace objects and derives source snapshots from the
same opened roots used by its fixed runtime configuration. Exact original
Started acknowledgement fills one Operation handoff; the sealed binding keeps
the original writer/capability/scope and retires on completion, cancellation,
worker unwind or drop. Report retains root custody through unresolved owned
cleanup. Host maps reject duplicate live directories and canonical nesting; a
smaller file-copy profile refuses excessive configured reads before source IO.

This preserves ADR053/058's trusted stable-ancestor obligation. It does not
qualify hostile same-UID namespace replacement, actual Codex sandbox behavior,
MCP file tools, upload, Matrix file delivery or production activation. Pinned
Codex sandbox canonicalization of cwd/writable roots was traced before rejecting
descriptor aliases as a complete isolation solution. No guardian, Settings or
process ownership policy changed.

Local execution/platform regression: 46 tests passed, including eight new tests
and actual child/guardian fixtures. The two affected hagency CLI targets added
eight passing MCP/Matrix integration checks, for 54 affected tests total. Native
and Windows GNU all-target Clippy passed for execution/platform; Windows is
compile evidence only, with real Windows execution left to integration CI.
Strict workspace-root lifecycle passed 8/8 (seven bound scenarios plus all 17
changed paths), with zero failed/skipped/uncertain/pending-review results. Format
and diff checks passed. This does not claim the separate full knowledge corpus
gate passes. The
first fixture run expected Limit where the file primitive returns Capacity; a
copied SQLite fixture initially used default nonprivate creation and then hit
correct restart fencing. Final fixture precreates a private file and explicitly
restores stale rows only as a negative alternate-writer setup; production
recovery is unchanged. Original failed logs remain external.

ADR094 original Matrix operation observation (2026-09-10): approved scope expanded
from eleven to thirteen allowed paths after preserved fc57d6b original evidence.
The amended contract parsed/linted before outgoing edits. Test-only finite traces
now follow original collector/intake/outgoing tasks and SDK open/queued work,
with separate primary/fencing errors and fixed operation/variant/batch labels.
Two original outgoing shutdown callsites reuse the existing domain shutdown
observer. All original send/refusal/recovery assertions and deadlines remain.

Three actual held-owner regressions pass. One complete native Matrix crate run
passes 115/115 (94 library, 6 HTTP, 6 media upload, 9 transport; zero ignored). Native
all-target Clippy with warnings denied passes. That full run predates only the
final preparation-failure phase label and reuse of the existing shutdown observer
in the new regression. Final strict lifecycle passes 14/14 (thirteen selectors
plus all twelve actual changed paths), with zero failed/skipped/uncertain/pending
scenarios. Final native and Windows GNU all-target Clippy, rustfmt and whitespace
checks pass after those refinements. Cross-compilation is not Windows runtime
qualification; original Windows runs at 706172d and fc57d6b remain failed.


### 2026-09-10 — Retained workspace and original-operation integration

Integrated ADR093 as c3a75ec and ADR094 as bbf19e2 after source review; only
independent coordination append conflicts required resolution. One obsolete
std::fs import in the affected CLI Matrix fixture was removed after the complete
workspace build exposed it. No behavior or assertion changed for that cleanup.

The locked all-target native workspace passes 521 independent tests plus one
proxy child (522 printed), with 85 summaries, zero failures and zero ignored
checks. Full workspace Clippy with warnings denied, rustfmt and diff checks
pass after the import cleanup. All 420 Rust selectors resolve. Final integrated
ADR093 lifecycle passes 8/8 with all 17 paths, and ADR094 passes 14/14 with all
12 paths; both have zero skipped, uncertain or pending-review results. Evidence
is in workspace-observation-integrated-*, original-operation-integrated-lifecycle.*
and retained-workspace-integrated-final-lifecycle.* in the external migration cache.

Latest completed hosted fc57d6b native run34560957072 passes Linux/macOS and fails
Windows. Original totals are Linux512/0, macOS511/0 and Windows502/6 printed
passes/failures, all with 85 summaries and zero ignored. Two Windows failures are
domain shutdown timeouts after successful Collector close; four are early
collector OutcomeUnknown observations without enough internal evidence to name
the cause. Original full logs and test-step extracts are preserved. Later
outgoing/transport diagnostic passes are separate evidence. Node run34560957060
passes4291 tests with one platform skip, plus the required verifier checks.

The separate knowledge gate has417 errors at fc57d6b versus157 at baseline.
The isolated migration-only cleanup reports the260 introduced errors removed,
with independent preservation review and integration still pending. The next
native file-service proposal remains isolated: it needs a real host bootstrap,
fresh compatible dispatch claims and workspace registration before child launch.
No M0–M9 milestone, production cutover or full migration completion is claimed.

## 2026-09-10 — Migration knowledge governance

In the isolated fc57d6b documentation worktree, supplied canonical sections for
61 parsed migration ADRs and frontmatter/sections for the three previously
unparsed ADRs. Preserved all 64 original decision bodies and qualifications.
Added the two requirement documents' missing structure without changing their
requirements. Removed eight decision IDs from seven contracts' satisfies lists,
retaining valid requirement IDs and explicit decision references in prose.
Renumbered only native state ownership to ADR095 and updated its exact references
and task boundaries; legacy execution ADR028 is unchanged.

Agent-spec 1.4 knowledge lint now reports exactly the pre-migration 157 errors,
down from 417: all 260 introduced records are closed, with no new findings and no
baseline findings removed. The corpus gate still exits 2 and remains failing.
All 15 affected/project/foundation contracts parse and lint with exit 0; their
37 warnings and 18 informational findings remain unchanged. All selectors in
the 13 changed task contracts remain identical. The exact Node and Rust binding
catalog checks resolve 543 and 400 selectors with no missing bindings. The first
Node catalog attempt failed on a missing mockup dependency link; that original
failure is preserved, and the corrected run used the existing dependency tree
whose mockup lockfile matches exactly. No runtime tests or lifecycle pass are
claimed for this documentation-only change. Evidence and the actual 85-path
boundary manifest use knowledge-governance-* in the external migration cache.


### 2026-09-10 — Governance integration verification

Integrated a971d65 as94c4ead after root and independent preservation review.
Only an independent progress append conflicted; both histories were retained,
and the intended historical native state-ownership reference moved to ADR095.
Root knowledge lint reproduces exactly the157 baseline error records: zero new
or removed baseline records and all260 introduced records closed. The gate remains
failing with exit2. No source behavior or test selector changed in this cleanup.
Root post-integration binding catalogs resolve543 Node and420 Rust selectors,
with no missing bindings. Whitespace checks pass. The previously completed full
native521-independent-test run and Clippy remain the source validation for this
metadata-only integration; no redundant full runtime suite is claimed here.
## 2026-09-10 — File-delivery domain implementation checkpoint

ADR097 implements schema020 immutable metadata plus original upload reservation in
one transaction and distinct publication claim/begin/current/historical interfaces.
This is a local compiling checkpoint for the dependent ADR098 publisher, not a
completed acceptance or integration claim. Actual service/MCP and SDK qualification
remain separate. Root review corrected association to retained UploadClaim and
required captured-size/stage-length equality. Core/store check passes. The first
focused run had seven passes and one failure in a new fixture's nonexistent SQL
execution_epoch column; its original log is retained as adr097-tests-first.log.
The fixture now targets the actual canonical-task JSON field. Focused and full
validation, strict lifecycle and final review remain pending at this checkpoint.

The post-checkpoint domain review also rejects first capture facts after a recorded
upload staging outcome and checks historical capture/stage length consistency.
The approved borrowed send identity supports retained uncertainty after unique-send
consumption. Tests cover that path, different upload capability/request/fence,
accepted upload cancelled before event publication, and real encoded-route capacity
rollback. The second focused attempt preserved a room fixture Generation failure;
its corrected observation now advances generation. All eight focused selectors
then passed. Both subsequent complete core/store all-target runs pass 186 tests
across 26 summaries with zero failed or ignored. Final native and Windows GNU
all-target warnings-denied Clippy pass. Windows execution/durability and actual
Matrix/service/MCP delivery remain separate unqualified gates.

The first strict lifecycle passes eight selected scenarios plus its explicit
25-path boundary result (9/9). Its separate requirement-trace diagnostic still
reports unexecuted scenarios from other mapped contracts; no full requirement
closure is claimed. Final scoped lifecycle follows the reviewed getter and test
additions immediately before the final delta commit. External evidence uses
adr097-* and preserves both original focused failures separately.

Final strict lifecycle passes again: eight selected scenarios and the 25-path
boundary result, 9/9 with zero failed, skipped, uncertain or pending-review results.
The separate requirement-trace diagnostic remains visible. Knowledge lint still
exits 2 with exactly the 157 pre-migration errors and no additional ADR097 errors.
This completes the domain slice's scoped verification, not the future encrypted
publisher or actual service/MCP acceptance.


### 2026-09-10 — File-delivery domain integration verification

Integrated final ADR097 as 83f425a and 1e6acc4 after reviewing the original
preparation/capture, accepted-upload association and historical settlement seams.
Only the independent progress append conflicted; both histories were retained.
The integration's locked full workspace passes 529 independent tests plus one
proxy child (530 printed), across 86 summaries, with zero failed or ignored tests.
Full workspace all-target warnings-denied Clippy and rustfmt pass. Exact scoped
strict lifecycle passes 9/9: eight file-delivery scenarios and the complete actual
25-path boundary, with zero failed/skipped/uncertain/pending-review results. This
is scoped verification, not closure of other requirements or native file tools.

Latest original hosted evidence remains 1da8f1b: Linux and macOS native jobs and
Node pass; Windows fails six approval fixture cases (513 printed passes and six
failures). Four failures are private SDK open ACK deadlines, one is a private SDK
close ACK deadline, and one exact snapshot places a domain shutdown timeout after
repository destruction began. No specific SQLite/OS cause is established. Original
logs and automatic diagnostics remain separate. ADR099 will add missing original
approval/field-drop observation without changing timeouts or retries. ADR096 actual
bootstrap and ADR098 encrypted file publication remain separate integration work.
## 2026-09-10 — Original publication content association

ADR100 implements the two independent comparisons needed by the ADR098 publisher:
exact immutable domain metadata/capture before historical settlement, and encrypted
descriptor association with the original unchanged frame commitment. It introduces
no current send authority or proof constructor. The exact 13-path contract parsed
and linted before production changes, with quality1.0 and one grouping information.
All four new selectors passed on their first run. The full affected core, store,
media and media-store suites then passed210 tests across29 summaries with zero
failed or ignored. After extending the writer test to fill its actual eight-slot
queue and check Busy budget return, all42 store library tests passed. The complete
frame oracle, original staging records and platform refusal semantics are preserved.
Final Clippy, binding and strict lifecycle evidence follows in external adr100-*
artifacts; actual SDK recipient, service and MCP acceptance remain separate work.

Final native and Windows GNU all-target warnings-denied Clippy pass. Strict lifecycle
passes all four exact selectors, each with one actual passing test, plus the complete
13-path boundary result:5/5, zero failed, skipped, uncertain or pending review. All432
Rust bindings resolve with no missing selector; this catalog is not a full workspace
test run. Knowledge lint still exits2 with the exact157 baseline error records and
no ADR100 additions. The final documentation delta changes no production or test
source. These results close the scoped association contract, without claiming actual
Windows execution/durability or completion of the dependent publisher/service slice.
### 2026-09-10 — ADR096 one-attempt development bootstrap

The actual native `serve --development-driver` now uses the shared Bootstrap,
current-token TLS Collector refresh, an exact compatible writer claim and required
Started-workspace registration before any child launch. The private configuration
selects fixed executable bytes/profile, original roots and Matrix scope; no raw
capability, command, environment or HTTP authority setter is accepted. This remains
one development attempt per service start, with no scheduler or file tool.

The original executable fixture exposed a real macOS stack overflow while moving
a 35,696-byte Report through nested oneshot/async polling. Outer-future boxing alone
still failed. The original result channel now retains one boxed Report and the
bootstrap uses wait_boxed; legacy wait remains available. No deadline or stack
budget increased. The real executable then completed its authenticated Matrix
refresh, fresh claim, registration and actual native MCP task read/heartbeat.

Full affected hagency/execution/store regressions passed 242 unique named tests,
with zero failures/ignored, before four additional compatibility/error-classification
regressions were added; the six final claim-profile tests pass separately. Actual
queue-held and SQLite-lock tests prove owned-claim time sampling after both waits.
Required registration, dropped/late/expired ACK, wrong binding, actual committed
claim-response loss and required Started-response loss exercise the original
writers/owners. Native and Windows GNU warnings-denied Clippy cover all three
crates including CLI tests. Strict cross-crate lifecycle passes all seven scenarios plus the explicit
27-path boundary, with no skipped, uncertain or pending-review verdict.

On macOS, completed protocol still has the existing unknown whole-tree cleanup:
the actual SIGTERM fixture confirms the original service and both writer locks
remain held with outcome_unknown. Linux's same Unix fixture requires observed
whole-tree stop and writer reopen. Windows has no Unix-signal fixture; cross-build
is not Windows runtime evidence. Production capabilities stay false. Logs,
original failures, crash-frame evidence and validation live in the external
2026-09-10 migration cache under bootstrap-*; no live service/model was contacted.

The final host compatibility check also leaves canonical Done follow-ups and
recovery reports queued: current OwnedDispatchScope cannot run them. Actual
verified event/intent/Done/follow-up and Started/reopen/recovery fixtures place
these before compatible work, prove the host selects the later runnable dispatch,
then prove ordinary claim still selects the untouched earlier work. General claim
pre-lock validation and invalid-clock error precedence are preserved.

After the final compatibility changes, all 170 store tests pass. The combined
unique test evidence is 246 names across the earlier full affected run and this
final store run; it is not a claim of one later 246-test invocation. Final native
and Windows GNU Clippy again pass all affected targets. The final stable-source strict lifecycle again passes all seven scenarios plus
the 27-path boundary (8/8), with zero skipped, uncertain or pending-review results;
original checkpoint output is retained separately. No actual Windows bootstrap run has been claimed locally.


### 2026-09-10 — Original approval and repository cleanup observation (ADR099)

Preserved original 1da8f1b native/Node job logs, exact Cargo slices and hashes
externally before subsequent work. Windows remains failed: six approval-library
errors, four direct fixture SDK opens, one direct SDK close and one domain
shutdown. The original shutdown snapshot shows prompt worker pickup followed by
an unfinished repository-drop interval at the unchanged reply timeout; no
backend cause is established. Automatic outgoing and transport diagnostics
remain separate evidence.

In the isolated ADR099 worktree, parsed/linted the nine-path contract before
source edits. Added existing finite traces to original approval bootstrap and
cleanup; the same original spawned observe/close tasks retain their traces.
The optional domain shutdown probe now distinguishes connection destruction
from ownership-file release while normal shutdown keeps its original unobserved
drop path and allocates no probe. New held-owner tests use actual SDK and domain
writer resources, not fabricated phase rings. Validation is recorded below.

Final ADR099 validation: all 17 approval tests passed, including the six original
selectors and the held actual-owner regression. Strict cross-crate lifecycle
passed 13/13 scenarios: 12 explicit tests and the exact nine-path boundary, with
zero failed, skipped, uncertain or pending-review results. Final warnings-denied
Clippy passed for hagency-store/hagency-matrix all targets on native macOS and
Windows GNU; formatting and whitespace checks passed. These compile/check results
are not hosted Windows runtime qualification.

The initial store run preserved four passing actual-worker tests and one failed
existing snapshot-size assertion. The approved four optional timestamps required
an explicit 64-byte increase to its finite cap, from 144 to 208 bytes; the final
strict run includes that original snapshot publication/bound test. The initial
approval compilation also caught a non-Clone HostConfig in the new fixture; the
fixture now borrows its original configuration through retained Inner ownership.
All initial and final validation logs remain external under adr099-*. No original
CI verdict, deadline, retry, workflow or production configuration changed.

### 2026-09-10 — Original encrypted file publication implementation

ADR098 consumes the original accepted UploadOperation plus exact publication claim
and nonclone send. Its finite registry retains the existing media Job/permit before
any await; handles and the owned task hold the Collector separately. Review caught
and removed a self-retaining Arc cycle. Actual private SDK upload history supplies
MXC, and only the SDK composes descriptor-bearing m.file. The existing encrypted
outgoing engine preserves current scope checks and original thread/private route.
A separate complete event receipt precedes domain Delivered. Caller loss and exact
historical recovery cannot repeat encryption or writes.

Eight focused selectors passed after the ADR100 association checkpoint was wired.
Real recipient SDKs decrypt original bytes, Chinese filenames, caption and relation
in DM and thread cases. A separate process records first Delivered after actual SDK
Complete and a failed first domain settlement without original capability/media or
HTTP replay. Unrun/uncertain/cancelled external-owner teardown is checked alongside
future-drop custody. Coherent metadata substitution initially reproduced an erroneous
settlement; full original domain content and frame receipt checks now reject rebuilt
filename/hash/key/receipt substitutions. That original failure and early fixture
compile/setup failures are preserved in external adr098-* logs. A synthetic full
SDK receipt catalog checks admission bounds only; it is not evidence of 64 actual
network deliveries. Full Matrix tests, final Clippy, Windows compilation, exact
lifecycle and independent review remain pending at this checkpoint. Actual FileService
and MCP entry points, Windows positive durability and production execution are open.

### 2026-09-10 — ADR098 encrypted publisher verification

The final publisher uses ADR100's exact original frame and full domain-content
association. Independent review found and closed the final SDK Settle-ACK gap:
a real persisted settled receipt plus already-Delivered acceptance must replay
exactly before the original retained job can be released. Unmatched jobs stay
unknown and block close; both Complete and settled-receipt recovery update the
same retained operation outcome. Negative state reconciliation preserves the
original acknowledgement error and does not manufacture successful delivery.

All123 Matrix tests pass with no failures or ignored cases after the production
fix. Three subsequent test-only extensions each pass their original exact custody,
historical and journal selectors, proving no-attempt/unmatched custody refusal,
Complete recovery outcome update and valid-format settled-digest substitution
refusal. The substituted fixture restores only its saved actual SDK receipt.
Final native and Windows GNU all-target warnings-denied Clippy pass; formatting
and whitespace checks pass. Final strict lifecycle passes all8 actual selectors
plus the complete19-path boundary:9/9, with zero failed, skipped, uncertain or
pending review. Four informational/advisory lint findings remain; quality is1.0.
The knowledge gate still exits2 with exactly157 baseline Error records and no
added or removed Error record. Original failing and final successful test logs,
strict evidence and independent source reviews are preserved outside the repo.

Actual local TLS and recipient SDK fixtures decrypt exact file bytes, metadata
and direct/thread relations. Windows GNU compilation does not prove native
Windows execution: unconfirmed directory sync remains an explicit no-upload
refusal, not positive file delivery qualification. This scoped publisher contract
is complete; FileService/MCP integration and the full migration remain unfinished.

### 2026-09-10 — Integrated publisher and consuming-close review

The combined096/097/098/099/100 workspace at14f0f3e passes564 independent named
tests, one proxy child (565 printed),88 suite summaries and zero failures or
ignored tests. Documentation fd0b459 updates schema20 and explicit one-attempt
development status; it does not claim live replacement or MCP file availability.

A later source review found two existing096 shutdown gaps. Collector close may
consume the SDK owner before returning an error; calling the now-empty wrapper
again cannot prove the original ACK. The Driver now retains the first close
result and never uses a second empty-owner success to release its writer order.
A completed oneshot receive error is also recorded as sticky unknown rather than
left as a pending receiver which could panic on another poll. A timed-out pending
receiver is still retained and may receive its original eventual result. Actual
process-report quarantine and cleanup retry remain before Collector closure.

The four actual bootstrap custody tests pass, including a consuming-API protocol
state fixture and an actual closed oneshot/repeated-close fixture. The protocol
fixture does not claim a real SDK shutdown observation. Final full Clippy,
contract/binding and Windows GNU follow-up results are recorded after they finish.

ADR101 now proceeds in an isolated shared implementation worktree with disjoint
service, adapter and executable proof ownership. Its positive actual-service
acceptance exposed a missing fresh-SDK trust and Olm-session enrollment path:
existing publisher positives use private verified-device fixtures. ADR102 is a
separate prerequisite design; no fixture trust or capability setter will be added
to the application. Positive executable acceptance remains required and unpassed.

The final consuming-close follow-up passes strict lifecycle8/8: all7 existing
bootstrap scenarios execute real tests, plus the exact4-path follow-up boundary;
zero failed, skipped, uncertain or pending review. The custody selector runs4
actual tests. Native full-workspace all-target warnings-denied Clippy, Windows GNU
all-target hagency Clippy, formatting and whitespace checks pass. The Rust catalog
resolves459 bindings and the Node catalog543, with none missing. Independent
review confirms pending timeout receipts and physical process quarantine are
preserved. Final knowledge gating still exits2 with the exact157 baseline Error
records and zero added/removed errors. These scoped follow-up results are separate
from the full564-independent-test integration run above.

### 2026-09-11 — preserve 5a8f7f7 originals and refine ADR094 observations

Original hosted Windows Cargo at 5a8f7f7 failed four intake tests: 559 printed passes,
558 unique passing labels and 4 failures. Two opens last observe StateStoreOpen;
one attachment batch has prompt SDK Start pickup but no completion before the
unchanged deadline; crypto-variant Collector close has no original close trace.
Linux 568 printed/567 unique and macOS 567/566 passed. Node 4291 passed, 1 macOS-only
skip in 293 files; its verifier separately ran 508 tests. Both Windows diagnostics
passed different selectors and do not replace the original failure. Positive
Windows staging remains unqualified. Original metadata, raw slices, all available
artifact ZIPs and a 77-file hash manifest are preserved externally under
5a8f7f7-original-ci-evidence.md.

The isolated ADR094 follow-up retains the same 13 allowed paths and changes 8.
Original SDK Start receives its existing command trace only in test builds;
fixed inner phases retain that original sequence without a new task or attempt.
The original five crypto variants now observe their same Collector close.
A deterministic actual SDK/TLS fixture holds three persist/apply boundaries,
drops callers, queues a distinct read and verifies the original batch after
reopen; a real SQLite trigger refusal keeps OutcomeUnknown and no successful
write completion. The new fixture's first compile failed because HostConfig is
not Clone; it now borrows the original collector configuration. Original compiler
output and the corrected successful one-test run are preserved. All four original
intake selectors, three existing ownership selectors and the new subphase selector
pass locally. All-target Matrix Clippy passes with warnings denied. Strict task
lifecycle passes 15/15: 14 actual selectors each execute one passing test, plus the
exact 8-path boundary; zero failed, skipped, uncertain or pending-review results.
Knowledge gate still exits 2 with exactly 157 baseline Error records and no added
or removed errors. The 13-path allowed list and all 13 earlier selectors are
unchanged; one meaningful selector is added. These are scoped local observations,
not a repair or cause finding for hosted Windows. No fixture or production
deadline, concurrency, retry, result or authority changes.


### ADR101 application checkpoint — real bounded service, positive encryption pending

The isolated file-service implementation now shares the original DomainStore,
Collector and sealed post-Started workspace with Bootstrap. Its fixed worker
retains two jobs across caller loss, serializes source/codec/staging, exposes
current-authorized send_file and exact historical safe status through the real
HTTP/MCP adapters, and refuses unknown replay or incomplete media journals.
Borrowed shutdown retains pending acknowledgements and completed errors; fresh
media journals are created only after actual atomic directory creation.

Actual validation: complete hagency library12 passed,0 failed/ignored;9 new
service tests include an actual encrypted binary POST held while a second
durable admission completes and a third refuses. Truncated response remains
Unknown, the original owner/lock stay retained, and the exact child is reaped
without calling process death a successful settlement. Native hagency all-target
Clippy with warnings denied passed. Separate adapter validation produced16
unique passes across focused Host/runtime and full HTTP/MCP/task-client targets.
Original fixture generation and helper-name compile failures are preserved in
external logs before their corrections. No full ADR101 strict lifecycle or
Windows positive workflow result is claimed.

The actual native file MCP peer now compiles and invokes send_file first, reads
its bounded status and checks canonical task remains in_progress. It has not yet
proved encrypted delivery. Fresh SDK identity/trust/session enrollment is the
accepted ADR102 prerequisite in a separate worktree; actual executable group/DM
recipient decryption and fresh-process first Delivered remain required. This is
an implementation checkpoint, not M0–M9 closure or a production cutover.


### ADR101 independent owner review — task unwind and cancellation follow-up

The independent Bootstrap/FileService owner review found two reproducible
projection defects in checkpoint 745597b: an actual panicked source job kept its
queued/live state, and an actual Delivered domain row with retained cancellation
history was projected as OutcomeUnknown. Each exact regression failed against
the original production files before the fixes; the original failures and one
initial fixture missing-mut compile error remain preserved in external logs.

The worker now keeps the original bounded task-ID-to-Job association through
join, fences that exact original on panic, retains its slot and private custody,
and refuses close as Unknown. An isolated actual child proves unchanged
preparation, no source capture or HTTP, replay/status agreement, retained journal
lock and two Unknown closes; reaped child death is not called settlement. The
projection regression performs real writer reservation, upload/publication
transitions, cancellation and first historical Delivered settlement using the
existing trusted adapter correlation-data boundary. It checks public Delivered
without an error while internal cancellation history remains unchanged. This
fixture creates no SDK or media proof and is not encrypted delivery qualification.

Validation in the edited tree: all 14 application library tests passed with zero
failed or ignored, including all 11 FileService tests; native hagency all-target
Clippy with warnings denied passed. The complete HTTP, MCP and task-client
targets passed all 14 tests with zero failed or ignored. The changes remain inside the existing 38-path
ADR101 boundary. Full ADR101 executable/restart acceptance and Windows positive
durability are not claimed by this follow-up; the separate ADR102 integration
retains those gates.

### Observe actual child progress within its existing deadline — 2026-09-11

The original fed7557 macOS run34573765787/job103181489856 failed only the
runtime owned-child pulse-growth assertion at owned.rs291 (target5passed,
1failed). The parent slept60ms after a live-owner observation, but no child
or filesystem acknowledgement guaranteed a pulse in that scheduling window.
The log does not prove scheduler or disk delay; both remain possible causes.
The complete original log is preserved externally without changing its verdict.

The isolated fixture repair keeps the existing3s absolute observation deadline,
40ms owner wait, required actual pulse growth and stop/cleanup assertions. A new
real gated child cannot write a fourth byte until explicit fixture release;
after release actual growth and retained-owner cleanup are required. Gate and
heartbeat share the existing8s child lifetime. No runtime supervision or CI
behavior changed. The task parsed/linted before source edits; actual custody
3/3 and complete owned target7/7 pass locally, with native and Windows GNU
all-target warnings-denied Clippy passing. Windows cross-build is not execution.

Final strict lifecycle passes3/3 (both actual scenarios and exact five-path
boundary), with zero failed skipped uncertain or pending review. Root's source
review found no blocker. This local validation is distinct from the original
failed hosted run; integration and a new hosted run remain parent-owned.


### 2026-09-11 — FileService integration preserves earlier shutdown regressions

Integrated the reviewed ADR101 service checkpoint and its two independently
reproduced status fixes. The main branch's consuming-close and dropped-oneshot
regression selectors are preserved at their new ownership boundary: the former
now drives actual Bootstrap::close using an explicitly modeled consumed SDK-close
result, while the latter still exercises the actual Driver receiver. This is
protocol-state evidence, not an assertion of real SDK shutdown. The first local
combined library run was15pass/1fail because the relocated fixture's test URL
missed its required canonical slash; the corrected exact selector passed1/0/0.
All-target hagency Clippy with warnings denied passed. Original logs are retained.

The fixed offline file peer now stores only its own validated inherited fixture
context in a bounded private file for an independent historical MCP read. No
production Context serializer or capability setter is added, and the values are
never printed. The actual historical read will be exercised with the combined
ADR102 executable fixture; compilation alone does not establish that acceptance.
A considered stop-on-Unknown polling change was discarded before compilation:
WritePossible can yield transient Unknown while an original live send awaits ACK.
The peer's polling and all production/fixture deadlines remain unchanged.

Original hosted fed7557 CI is now fully preserved: Linux568independent passes;
macOS566passes/1pulse-observation failure; Windows558passes/5approval failures.
Three Windows failures were original2s domain shutdowns inside SQLite Connection
drop, two were original10s SDK opens. Logs identify those boundaries, not their
underlying OS/SQLite cause. Both later Windows diagnostic steps passed separate
selectors. Node passed4291/0fail/1platformskip in293files; its verifier separately
passed508checks. Available Linux/Node artifact ZIPs match GitHub SHA256.
The PR records original failed results separately from diagnostics and fixes.
### 2026-09-11 — Actual native encrypted file delivery and first settlement recovery

The real native serve process now loads the explicit fresh-account enrollment
profile before file readiness and the original dispatch claim. The executable
fixture uses an independent recipient SDK and actual TLS key upload, signature,
claim, binary media upload, encrypted key share and room event exchanges. It does
not seed or open the service SDK to create trust or sessions.

The exact native_file_service_executable test passed both a group thread and DM:
independent decryption recovered the original bytes, Chinese filename, caption,
MIME, size and correct relation; actual status was Delivered while the canonical
task remained in_progress. Each case used one dispatch, five enrollment writes,
one signed claim, one key share and one room event. Production capability flags
remain false.

The exact native_file_service_restart test passed after an actual SQLite trigger
refused the first domain Delivered transaction. The original runtime correctly
became negative/unknown. After killing and reaping that native process, a separate
state-store reader decrypted the original protected journal and independently
confirmed its File Complete record, original ID, route, content and event ACK.
It created no OlmMachine and wrote no authority or journal value. The inspector
closed its pool before restart. After removing the source file and returning 401
for current whoami, a fresh native process recorded the first Delivered for that
original transaction and content digest. No extra dispatch, key write, claim,
media upload or room send occurred. Historical acceptance did not restore current
readiness.

Both exact selectors passed locally on macOS with zero failures/ignored cases.
Original fixture failures are preserved externally: a receipt-read race in the
positive fixture, the invalid assumption that a poisoned domain writer could
finish normal MCP reads, reuse of a create_new stderr path on restart, and an
incorrect acceptance field name. Each was corrected in fixture code, without
changing production deadlines or promoting a negative workflow to delivery.
These checks do not qualify Windows durability or the full ADR101/102 lifecycle;
enrollment negative/custody/restore checks and final integrated validation remain
in progress. No production service or live Matrix account was changed.


### 2026-09-11 — Final enrollment and native file integration checkpoint

All eight actual Matrix enrollment selectors pass on final source. Native and
Windows GNU all-target Matrix Clippy pass with warnings denied. Restoration now
checks exact original request order and required fields, applied ACK shapes,
operator anchors, original recipient curves and bounded before/after session IDs.
Actual caller-loss fixtures retain the same SDK command/result at Prepare, Accept
and Finish boundaries; unknown work never generates a replacement identity or
request. Inspectors use dedicated runtimes, closed and destroyed before another
owner opens the original SDK, to finish scheduled background connection drops.

The complete native file_service target passes all three tests: actual group and
DM delivery; first Delivered recovery with an original-context native MCP read;
and uncertain actual POST/PUT plus a departed-human Direct-room refusal. Old
process teardown now explicitly checks kill/wait. The historical own-ID query
returns Delivered and an unrelated ID refuses, even with current whoami401 and
no source file. Truncated POST/PUT cases remain Unknown with no acceptance or
replay; their actual runtime negative result is never called helper success.
Initial room refusal supersedes queued work without claiming or sending.
All-target hagency Clippy passes with warnings denied.

Prerequisites are explicit: the disposable peer privately captures only its real
inherited context, and the previously validated ADR101 status fixes preserve
Delivered after cancellation. Before that prerequisite was integrated, the new
actual historical query failed with Unknown despite domain Delivered; the failure
is retained. Earlier uncertainty fixture failures used the positive-only waiter
and assumed a refused dispatch remained queued rather than superseded; both
were corrected without changing polling/deadlines or weakening positive checks.
An earlier Matrix reopen test failed without recording its error. Later passes
and the inspector lifetime improvement do not establish that failure's cause.
All original failure logs remain separate from final successful checks.

This is an isolated implementation checkpoint. Final main-branch combined tests,
strict lifecycle/binding checks and native Windows positive staging are still
required. Windows GNU compilation alone is not platform qualification; full
migration and production cutover remain incomplete.


### 2026-09-11 — Combined enrollment regression and inspector close boundary

The original combined workspace run at6e52450 stopped with234 printed passes and
one failure: Matrix library111pass/1fail, native_matrix_enrollment_unknown at the
original reopen assertion. Its matches! assertion hid the fault branch and actual
error. The original partial log and SHA256 are retained externally; unrun targets
are not counted as passing. A diagnostic-only full Matrix library run passed112/0/0,
so it did not reproduce or explain that original failure.

Source review separately confirmed that three crypto-inspector callers closed
the shared pool on their ambient runtime and could proceed before its scheduled
connection drops completed. Their unchanged machine destruction now precedes a
test-only close wrapper that transfers the same shared pool's close into a
dedicated runtime and destroys that runtime before returning. This adds no SDK
owner, reopen, retry or deadline change and preserves the actual close error. It
only joins work scheduled by this close, not unrelated earlier ambient work.
The original assertion now retains fixed phase labels and safe result/error
diagnostics without exposing ledger content. Full Matrix library112/0/0 passes
after this fixture correction; that result does not establish original causality.
Final combined all-target and strict contract checks remain separately required.

The same final source passes native and Windows GNU all-target Matrix Clippy with
warnings denied. The first native Clippy failed on a now-unused CryptoStore trait
import in the existing-identity fixture; only that obsolete import was removed.
The initial warning/failure log remains separate from both final successful checks.


### 2026-09-11 — Combined native file workflow validation at eb06c35

The actual integrated workspace passes593 independent tests (594 printed with
one separately spawned proxy child),90 suite summaries, zero failed and zero
ignored. This run used cargo test --workspace --all-targets --locked
--no-fail-fast. Workspace all-target Clippy, affected native Windows GNU all-target
Clippy and rustfmt pass with warnings denied where applicable. All480 Rust spec
bindings resolve, and all12 cross-language vector checks pass.

Strict ADR101 lifecycle passes9/9: all8 actual scenarios plus all34 actual changed
paths inside its38-path allowance. Strict ADR102 passes11/11: all10 scenarios plus
its exact23 changed paths. Both have zero failed skipped uncertain or pending
review results. The original6e52450 partial failure and diagnostic-only pass remain
separate evidence; later passes do not explain its hidden error. Knowledge lint
still exits2 with exactly157 original Error records, none added or removed.

The isolated Windows directory probe also now has an actual positive original
run34578962296 atdfcad753: ordinary-token private local-NTFS encrypted staging and
a separate process restoring exact original ciphertext descriptor and plaintext.
The five earlier original failed runs remain failed. This probe is not integrated
production support: ADR104 must still qualify the safe production helper, and
this branch's native Windows positive file-service gate remains unpassed.

These results complete the local ADR101/102 development slices, not the retained
M0–M9 migration inventory. New-head hosted CI remains separate; originalfed7557
Windows failures, receive tools, permission application, effective sandbox and
config-home qualification, full room/history/console parity, quotas/retention,
measured budgets and deployment cutover remain open. No live service or account
was changed. Exact command results, failure logs and SHA256 manifests are kept
in the external migration evidence cache.

## ADR106 original SQLite close observation — 2026-09-11

Added a safe per-connection CLOSE-only observation to the original observed
DomainRepository drop. A thread-confined guard holds only the original Probe,
releases its thread-local association on return or unwind, and publishes one
fixed entry timestamp without reading connection data. No raw FFI, SQL logger,
query, checkpoint, wait, retry, ownership change or deadline change was added.
Unobserved shutdown remains free of hook registration and Probe allocation.
The new Option<u64> increases the snapshot ceiling from 208 to 224 bytes.

Seven store shutdown tests pass 7/0/0, including an actual held SQLite callback
through the original caller timeout, retained private lock and separate reopen.
The association test uses actual concurrent and sequential repository closes,
nested/unavailable slots and unwind; it does not manually publish the new marker.
Strict lifecycle passes 13/13 checks: twelve actual bound tests, each with one
executed passing test, plus the complete eight-path boundary. This includes the
five original fed7557 approval failure selectors with unchanged assertions.
Native and Windows GNU all-target store/Matrix Clippy pass with warnings denied.
All evidence is retained externally in adr106-strict.log, its structured run
records, adr106-store-shutdown.log and both adr106 Clippy logs.

The original Windows suite remains failed and its backend cause unproven. The
new phase distinguishes SQLite close entry only; it does not identify Windows
VFS mutex waiting, file I/O or scheduling, nor diagnose SDK initialization.
Positive Windows staging remains unqualified by these checks. The upstream
WAL-reset issue is an open separate dependency assessment with verified original
package/source hashes; this slice enables only the pinned empty trace feature
and leaves Cargo.lock and all dependency versions unchanged.


### 2026-09-11 — Original Linux 85427cb file fixture diagnostics

The original Linux job 103200776066 in run 34579897607 failed
native_file_service_executable and native_file_service_uncertainty at the shared
10.9-second SDK-plus-HTTP fixture wait; native_file_service_restart passed.
The original target result is 1 passed / 2 failed / 0 ignored in 45.87 seconds.
No variant, wait callsite or original child status was printed, and panic cleanup
removed private child stderr with its temporary directory. These failures remain
failed and their cause is unproven. An unchanged isolated macOS run passed all
three tests in 7.77 seconds and does not replace that Linux result.

The bounded follow-up adds static variant/wait/last-route labels, saturating
request counts, elapsed time and safe prior status categories to the same
original child. Before panic cleanup terminates/reaps that PID, it reports actual
try_wait state and only fixed bounded stderr categories from its original retained
read handle, with fallible output that cannot replace a panic. The new refusal
observation uses the existing fifteen-second startup watchdog. It never prints raw
stderr or protocol/private values and leaves production source, the shared Fake
helper, all original waits and process cleanup unchanged. A separate actual-child
regression holds a real TLS whoami response, then observes a distinct real Config
refusal and verifies that cleanup preserves the original panic. No backend fix,
retry, deadline change or scheduling change is claimed.


Final local file_service validation passes 4/4 in 8.18 seconds, including all
three original selectors and the actual-child observation regression. Strict
ADR101 lifecycle passes 10/10: nine scenarios with 15 actual passing test matches
plus the exact seven-path boundary, with zero failed/pending/skipped/uncertain.
All-target hagency native Clippy passes 37.70s and Windows GNU Clippy passes 36.47s
with warnings denied. Formatting and diff checks pass; Cargo files are unchanged.
Windows GNU compilation is not platform qualification. The original Linux
failures, unchanged local diagnostic pass and final observation checks are kept
as separate external logs under linux-85427cb-file-observation-*; exact original
source/log hashes are in linux-85427cb-file-service-original-evidence.json.
### 2026-09-11 — Preserve original platform failures and integrate diagnostics

Original85427cb CI passes macOS593 independent tests, but fails Linux with592
passes/two failures and Windows577 passes/twelve failures. Each platform prints
one additional proxy child result; none ignores a test. Linux's two executable
file fixtures lose their HTTP phase and child status on timeout. Windows includes
unqualified file workflows and original connection-drop timeouts. Separate
outgoing/transport diagnostic passes do not replace those original failures.
Node CI passes4291 tests with one platform skip; its verifier passes508 checks.
All raw logs, exact Cargo slices, line ranges and hashes are preserved externally
under85427cb-original-ci-evidence.md and its manifests.

Integrated ADR106's same-connection close observation passes strict13/13, each
of twelve named selectors executing one passing test, plus its eight-path bound.
Integrated native store/Matrix Clippy passes. The original-child diagnostic
fixture now retains its actual stderr read handle and emits bounded static
categories before panic cleanup; all four executable file tests pass on the
integrated source13d05bf. All493 Rust bindings resolve and rustfmt passes. The
source has not received another full workspace test run since the earlier
eb06c35 acceptance; new hosted CI is required. Neither diagnostic change asserts
the underlying Linux or Windows timeout cause or activates a live service.

### 2026-09-11 — Receive-file selection and cache domain prerequisite

Implemented the accepted ADR105 prerequisite in an isolated tree rebased onto
eb06c35. The36-path allowed boundary includes eleven explicitly approved schema
fixture paths; their historical input versions and old-data refusal checks are
preserved. Atomic inbox selection, current safe attachment discovery, exact host
claim restriction and schema021 original cache facts are implemented. The sink,
receive tools and full incoming executable acceptance remain incomplete.

Final store all-target tests pass189/0/0, app library16/0/0 and actual bootstrap
regressions5/0/0. Native all-target core/store/hagency Clippy passes with warnings
denied. Earlier compile errors in the new mentions fixture and two Clippy findings
are retained externally; a zero-match unqualified exact clock invocation is not
counted as a pass. The actual qualified clock test and final full store run pass.
Node catalog first failed on missing optional native crypto/Next dependencies in
an old external dependency cache. With existing complete dependency links and
cache disabled, actual Node543 and Rust487 bindings resolve with no missing test.
These are catalogs, not claims that all workspace or Node suites executed.

Knowledge lint still exits2 on precisely the unchanged157 baseline error records.
The active contract now gives each of its seven actual tests a separate scenario;
agent-spec otherwise retains only the last Test line in a scenario. Final strict
lifecycle passes8/8: the34 actual changed paths fit the36 allowed paths, and each
of the seven selectors ran exactly one passing test with zero failures or ignored
cases. No skip uncertain or pending result is promoted. Formatting/diff checks
pass. The full fourteen-scenario workflow still has no sink/MCP/executable verdict.

### 2026-09-11 — Retained regular-file identity prerequisite

Added a handle-only regular-file comparison for the upcoming receive sink. It
preserves full Unix device/inode and Windows volume/128-bit file ID, refuses
non-regular objects and failed queries, and never substitutes pathname equality.
Actual tests distinguish duplicate/reopened/hard-linked originals from equal-byte
distinct files and a replacement at the old name; directories refuse in both
argument positions. Identity equality does not grant content, private permission,
link safety, durability or workspace authority.

All10 platform library tests pass locally with zero failures/ignored. Native and
Windows GNU all-target platform Clippy pass with warnings denied, and formatting
passes. Strict lifecycle passes2/2: the real selector and all5 declared paths,
zero failed skipped uncertain or pending review. The task parsed/linted before
source edits; advisory lint warnings remain. Windows cross-compilation does not
prove actual Windows execution, and no receive sink or full workflow is claimed.
### 2026-09-11 — Retain checked receive scope and release completed bytes

In isolated native-received-scope worktree, added the bounded Matrix checked-result
prerequisite from ADR105. Actual encrypted SDK intake and authenticated TLS tests
pass9/9, including six unchanged receive regressions and three new selectors.
The new checks retain six read-only scopes beyond the four-result pool, reject
later task retirement, apply declared and actual HTTP size bounds, and distinguish
the expired original write deadline from a fresh current read-only scope check.
Native and Windows GNU all-target Matrix Clippy pass. All six media-download
transport regressions pass. Strict lifecycle passes4/4: three scenario results
plus the exact seven-path boundary, with no fail skip uncertain or pending result.
The first selector also matches the deadline selector, so its actual2-test output
is preserved and not described as a separate unique scenario. Rustfmt and diff
checks pass. This is not a completed incoming file workflow.
Original Linux and Windows85427cb hosted failures remain separate open evidence.
### ADR104 retained Windows directory sync — local implementation checkpoint

ADR103's original sixth Windows run34578962296 atdfcad753 passed the same-object
local NTFS candidate and independent-process original encrypted restoration.
Its five earlier failures remain failed originals. This slice now derives that
fresh synchronous directory owner inside default media Store opening, through
the existing audited private-storage boundary. It preserves original directory
checks, unsupported directory-unconfirmed inspection and exact preparation and
restoration commitments. A read-only duplicate cannot substitute for the actual
directory acknowledgement. Media-store production remains unsafe-forbid.

The native query owns a fixed Box containing the actual File and both initialized
outputs. Unexpected pending waits for that exact private object. A failed wait or
still-pending completion permanently retains this one allocation and handle on
the parked original worker, with no allocation, formatting, callback, application
exit, unwind or repeat admission. The controlled Windows pre-call gate tests
caller-loss ownership; it is not a claim that native pending was reproduced.

Focused local checks: media library18/18 plus classifier1/1 and store private1/1;
both crates' native and Windows GNU all-target warnings-denied Clippy pass. The
first Windows cross-check caught a test-only unwrap requiring secret-custody Debug;
that assertion now uses a fixed panic message, without adding Debug. Its original
failed compiler log remains external. Strict lifecycle passes all five scenarios plus the explicit seventeen-path
boundary, with no fail/skip/uncertain result. Actual default-Store Windows
qualification is still pending:
the required example now gives Store the original ordinary Dir, and a separate
child must restore the exact earlier bytes/descriptor/commitments. Neither local
checks nor ADR103's candidate-injection success supplies this new platform gate.
No upload, FileService, Matrix, task Done or production cutover is claimed.


## 2026-09-11 — Preserve valid CTR byte equality

The isolated CI closure retains original ADR104 Windows run34582323420's failed
interoperability assertion: ciphertext0x11 can equal plaintext0x11 under CTR.
The test now checks actual ciphertext hash, SDK/native round trips and independent
key/IV metadata. A fixed public Node/OpenSSL equality vector passes through the
actual codec and SDK and still rejects corruption. Production crypto is unchanged.
The full media integration target passes5/5 locally; fmt and warnings-denied
Clippy pass. Original FileService startup failures remain independently open.
See knowledge/context/native-media-ctr-equality.md and the bounded task contract.


## 2026-09-11 — Trace original startup boundaries

Preserved c677ce0 original Native failures and ADR104 storage qualification
separately. Fixed TRACE markers now distinguish runtime/configuration verification
and hashing, store/worker setup, bind/server start and original media creation
refusal. Existing fixture children retain their original capped stderr handle and
project only closed bootstrap/media phase labels. No deadline or outcome changes.
The actual five-test executable FileService target passes locally, including real
configuration refusal and missing-journal refusal observation. Historical Linux
and Windows causes remain unproved; see the bounded startup observation contract
and knowledge/context/native-startup-boundary-observation.md.
### 2026-09-11 — Retained receive workspace sink implementation checkpoint

In isolated native-receive-workspace-sink-v2 tree based on5ece94f plus the separately
qualified ADR104 prerequisite, parsed and linted the exact ten-path ADR105 sink
contract before source edits. Implemented one original retained destination,
bounded write/readback checks, original binding checks and fresh read-only replay.
The application keeps its owner across the borrowed future; no Matrix/media
source dependency or per-request worker was added.

Actual owned execution suite passes22/22 including five new receive tests. Native
all-target Clippy passes. Initial cross-Windows Clippy passes, with a follow-up
creation rights and explicit delete-sharing fix under review. Earlier fixture
runs failed on invalid attachment metadata and a wrong SQL table name; both
original logs are retained in the external2026-09-10 migration cache. No failure
is reinterpreted as passing. This checkpoint awaits the separately bounded exact
ReceiveWrite capability matcher and fresh writer time after SQLite waits, final
checks and strict lifecycle. Real Windows file operations and the incoming
executable workflow remain independent gates.

### 2026-09-11 — Own native receive jobs and original Ready destinations

The root-accepted ADR105 application partition now owns two bounded receive jobs
on one fixed synchronous worker using the existing Shared Collector, DomainStore
and WorkspaceAccess. Reservation precedes GET; unique WritePossible precedes sink
effects. Original jobs retain checked results and prepared sinks through caller
loss or uncertainty. Exact task IDs associate completion and unwind. Ready drops
checked bytes and releases the live slot only after task completion, retaining a
bounded original read-only scope and destination. Every path response revalidates
current authority, immutable domain facts and actual original file readback.

Configuration defaults receive tools off. Bootstrap owns the receive worker and
closes it before the shared Collector. Actual dependency snapshots from the sink
and adapter partitions were used for compilation and hashed externally; they are
excluded from this service commit. The service's two new selectors, all18 app
library tests, five bootstrap regressions and warnings-denied all-target app
Clippy pass. Strict scoped lifecycle passes3/3 with both selectors each executing
one actual test and the fourteen declared paths accepted. The later admission
test extension also checks exact live replay and the two-job bound.

Two initial adapter snapshot compile errors (Salvo handler name collision and an
untyped large JSON integer), initial formatting output and an unqualified exact
selector with zero matches remain externally recorded. The zero-match invocation
is not a passing test. Full encrypted incoming executable acceptance, actual
partial-write worker-unwind evidence and platform qualification remain separate
required integration gates. Original85427cb hosted failures are unchanged.
### 2026-09-11 — Receive authority integration prerequisite

Closed two integration gaps under an exact seven-path contract. The original
workspace writer check now samples time after acquiring its SQLite transaction,
and the unique ReceiveWrite exposes only a complete validated capability match.
Actual clock test passes1/1; received-file integration passes5/5 including the
new grant-association test and four unchanged cases. All-target store Clippy
passes with warnings denied; formatting and diff checks pass. The clock test
uses an independent actual SQLite lock inspection and expiration while the
original command is queued. Grant tests change each identity field and malformed
inputs without reconstructing a grant. Strict lifecycle is recorded separately.
No platform, physical receive workflow or production readiness is implied.

Strict lifecycle passes3/3: the exact seven-path boundary and two separately
bound selectors, each executing one actual passing test. No fail skip uncertain
or pending result is promoted; logs remain in the external migration cache.

### 2026-09-11 — Receive sink partition validation

Integrated the separately tested receive-authority prerequisite99ade8b into this
isolated sink tree, then required its exact grant-capability matcher. Added actual
caller loss while an independent SQLite write lock prevents current validation;
dropping the borrowed future creates no destination and never rearms its owner.
The earlier actual-file caller unwind, mutation, collision and retirement checks
remain separate.

Final execution all-target tests pass33/33: eleven library and twenty-two owned
integration tests, including five new receive selectors. Native and Windows GNU
all-target Clippy pass with warnings denied. Strict agent-spec1.4 lifecycle passes
6/6: all ten sink paths and five independently named behavioral selectors, each
executing exactly one passing test. There are zero failed skipped uncertain or
pending-review lifecycle results. Rustfmt and diff checks pass. Exact outputs are
retained outside Git in the2026-09-10 migration cache under receive-sink-v2-final-*
alongside both earlier failing fixture runs. The separate ADR104 and receive-
authority changes are prerequisites, not additions to the ten-path sink boundary.

This closes the bounded sink partition only. Windows GNU checks are compilation,
not actual Windows execution. The Linux/macOS/Windows incoming executable workflow,
shared service close and live/runtime migration gates remain separate and are not
marked passing by this result. No live service, user checkout or production state
was changed.

### 2026-09-11 — Native receive HTTP, MCP and runtime adapters

Integrated exact sink and service checkpoints plus the independent grant/clock
prerequisite in the isolated adapter tree. Added current runner routes, separate
host receive-tools opt-in, strict closed MCP selection/response validation and
untrusted-content guidance without changing sandbox or execution approval.

Actual MCP child presentation and counted HTTP transport tests pass. The affected
application library18, bootstrap5, MCP4 and task-client8 tests pass; the final
runner target passes11/11 after correcting its fixture to finish the actual
dispatch before expecting retired authentication. Host and typed runtime receive
configuration each pass1/1. All-target app/runtime/execution Clippy passes with
warnings denied. Initial test failures remain external: Ruma accepts a lone$ so
that invalid-selector fixture was wrong; authentication rejects with401, not403;
task-only Done does not itself retire the dispatch. The tests now use actual
invalid input, exact existing authentication status and actual dispatch completion.
Catalog event length now matches the existing backend255-byte validator.

The separate reviewer found no additional concrete custody/concurrency defect in
the service and sink after accounting for exact capability and post-lock clock
checks. Original partial-write/platform and full encrypted incoming executable
acceptance remain separate required gates; these HTTP fixtures do not establish
a real Ready destination. Strict lifecycle and final source hashes follow.

Final adapter strict lifecycle passes6/6: all seventeen changed paths plus five
separate selectors, each running exactly one actual passing test. No failed,
skipped, uncertain or pending-review result remains in this adapter report.
The workspace-scoped lifecycle compiles all packages but executes only its
bound selectors; it is not a full workspace-suite pass.

### 2026-09-11 — Bounded SHA256 development profile

The original 22c4993 Linux job failed four FileService tests while their original
children remained inside configured-executable hashing, with zero HTTP requests.
A version-specific development-profile override compiles only sha2 0.10.9 at
opt-level 3; the caller, full file reads, integrity checks, refusal behavior,
watchdogs, parallelism and release profile remain unchanged. The actual Cargo
library hashes the same preserved local 2,876,408-byte probe in 6/6/6ms versus
108/109/111ms with the original unoptimized library, with the same independently
checked SHA256. This does not establish the original hosted artifact size or
qualify a hosted rerun.

Validation in the isolated 1cb60d7 worktree: media integration 5/5; actual retained
child observation 1/1; formatting and whitespace checks pass. Strict lifecycle
passes 6/6 (boundary plus five scenarios): the configuration filter actually runs
two tests, and each other filter runs one. The first lifecycle failed only its
root Cargo.toml boundary syntax; its five behavioral scenarios passed, and the
corrected explicit `./Cargo.toml` boundary passes in the final lifecycle.

The first whole bootstrap target remains 3 passed / 2 failed. The executable and
custody-shutdown selectors reported post-start protocol `unknown` instead of
`completed`. Their fixture removed its temporary receipts on panic, so that
run's underlying protocol cause is unobserved. Later lifecycle selector success
and the independently passing main-worktree bootstrap run do not replace those
original failures or establish host contention. Original hosted and local logs,
measurements, lifecycle output and hashes remain in the external migration cache.

### 2026-09-11 — Qualify original receive integration evidence

The first main-worktree full Cargo command returned success with 623 independent
passes and one nested proxy child. Subsequent binding inventory found that its
media integration binary omitted the newly committed CTR equality test. The
same target directory had previously built the adapter worktree's older source;
the retained dependency paths are relative and that artifact is newer than the
main source. Its original four-test output, binary hash, source hash, timestamps
and fingerprint are preserved in the external migration cache. This command is
not complete validation of current main-worktree sources. The package-only media
inventory rebuild actually finds all five tests but is not itself test execution.

Invalidate the affected application/media package artifacts before current-source
verification. Do not alternate worktrees on a shared Cargo target and assume
mtime freshness proves source identity. The original full-suite and failing
binding outputs remain intact; a later result must have separate provenance.
An additional workspace inventory was terminated after its first test process
stalled before output; its actual sample shows only dyld entry. That observation
does not establish the cause of separate runtime handshake failures.

### 2026-09-11 — Actual incoming native receive executable partition

Added the accepted nine-path executable partition on the integrated receive
source. The new independent SDK sender publishes its actual signed public keys
to the local TLS homeserver fixture, accepts actual service enrollment writes,
and delivers authenticated Olm/Megolm incoming attachment events. The test does
not open the service SDK store or manufacture inputs, visibility, capabilities or
Started bindings. Real native serve performs the one intake, selected inbox claim
and launch. The offline runtime discovers the two receive tools through actual
native MCP and reads the returned generated file through its real cwd.

The latest complete incoming target passes 4/4 with zero ignored tests. The first
case covers both a DM attachment wake and an unmentioned group attachment selected
as context by the later addressed message in the same thread. Exact replay after
six seconds preserves the original path without another authenticated GET; actual
runtime mutation then refuses without overwrite. An incomplete TLS body produces
Failed without a destination; reopening the service refuses the old Ready path
despite inherited original context and leaves the old bytes unchanged. Each case
counts the actual original GET and persistent records. App all-target Clippy
passes with warnings denied; exact lifecycle and final source evidence follow.

Original failed observations remain outside the tree under the authorized cache:
the initial SDK iterator compile errors; enrollment refusal caused by replacing
the sender's original device self-signature instead of merging SDK signatures;
an observer race after the helper published its real successful byte receipt;
and a silent scripted runtime exceeding its unchanged 1500 ms response deadline
during the six-second replay wait. Bounded commentary events now keep that actual
protocol alive while the original service write deadline ages. Separate pre-helper
launch failures persist as unresolved observations: one complete run was 0/4 and
one diagnostic selector was 0/1 with actual intake/Started but no GET or helper
phase, followed by unchanged-source 1/1 and 4/4 passes. Less concurrent local work
coincided with those passes and does not establish causation. No deadline was
widened and no failure was converted into a pass. Checked original server PID
reaping is separate from the still-unqualified whole-process cleanup outcome.
Installed Codex, all-platform positive filesystem evidence, additional ADR105
lifecycle faults and full production migration remain incomplete.

The application regression selection passes 46 tests (library 18, bootstrap 5,
MCP 4, runner 11 and task client 8). Strict lifecycle first passed 5/5, including
the exact nine paths and four selectors each running one actual test. After
clarifying the contract's integration doubles and grouping, the final lifecycle
is 4/5 and remains non-passing: the truncated-media selector again failed before
helper entry with zero GETs; the other three selectors each passed one test.
All outputs retain zero skipped, uncertain and pending-review scenarios. The
installed agent-spec 1.4.0 also reports four advisory output/metadata warnings
despite the explicit file-output step and Level/Test Double lines; its parsed
scenario metadata omits those lines. No newer verified binary was available.

Added a bounded fixed runtime-entry record before the peer watchdog and handshake
to improve future failure evidence. Its focused selector passes once. A fixed
six-trial direct-binary diagnostic and four-trial Cargo diagnostic also pass, but
do not replace the non-passing final lifecycle. Read-only sampling of ancestry-
verified native children in those Cargo trials observes 112 KiB footprint,
only _dyld_start and no binary images before Rust entry. Those sampled children
belong to the native init/service boundary; no failed runtime helper was sampled,
so this does not establish the cause of its failure. No production timeout or
policy changed. The checkpoint is ready for integration review, not qualification
completion. Logs, strict verdicts, sampled stacks and source manifests remain in
the authorized external receive-executable evidence cache.


### 2026-09-11 — Atomic private-directory creation

Original 22c4993 Windows FileService children reach private_policy_refused after
29 requests, before the media Store opens. Rust 1.95 DirBuilder uses default
security attributes: Windows inherits parent ACLs but assigns the creating
token's default owner. Our strict private check requires TokenUser ownership, so
those sources are incompatible when the token's default owner differs from its
user. The particular original CI SID predicate is unobserved and is not claimed
as measured evidence.

The bounded private-directory task adds create_directory_new: one nonrecursive
atomic create with the existing explicit current-user owner/protected DACL on
Windows, mode 0700 on Unix, and strict validation without recreation. Recovery
uses the actual AlreadyExists error for existing Store::open selection. Existing
paths are never resealed, reowned or repaired. Checker predicates, privileges,
filesystem qualification, deadlines, worker ownership and journal policy stay
unchanged.

Local macOS verification passes the new private creation regression 1/1, original
atomic media freshness/lock test 1/1, and the complete FileService integration
target 5/5. Package-only rebuilt lists verify the actual edited-worktree tests in
the reused target. Windows hagency-store/hagency all-target compilation passes;
Windows store Clippy with warnings denied passes. The Windows regression compares
actual current TokenOwner, default-created owner/private ACL, explicit-created
TokenUser owner/protected DACL, and refusal of existing entries. It changes no
token and emits only fixed boolean facts. Its Windows body is not executed on
macOS: actual hosted Windows assertions and complete workflow qualification
remain required. Original failures and local/cross-compile evidence are preserved
separately in the external migration cache.

Strict lifecycle completes with 4/4 passing results: the exact eight-path boundary
and three selectors, each executing one actual passing test. There are zero failed,
skipped, uncertain or pending-review results. The first selector also launches
unrelated workspace binaries; its sampled pre-Rust dyld delay is preserved in the
external evidence, and the original invocation completed without intervention.
The lifecycle's test-name coverage does not distinguish cfg branches: its Windows
assertion label is not Windows execution evidence. Actual Windows validation stays
pending. Final local hagency/hagency-store Clippy with warnings denied also passes.
## 2026-09-11 — ADR107 first retained native usage slice

Root accepted the browser authority design and exact 31-path boundary before its
source additions (27 original paths, pinned cap-fs-ext manifest/lock amendment,
then `.github/workflows/rust.yml` for the explicit browser lane and
`native/scripts/check-rust-spec-bindings.mjs` for all-feature selector inventory).
Work is isolated
from migration1cb60d7 in `hagency-native-console-usage-20260911`; original dirty
checkout and live services remain untouched. Contract parse/lint completed before
implementation and the amendments; lint warnings remain recorded.

Implemented retained native page/data mode, safe bounded engagement-label and
typed usage reads, separate finite issuer/session authority, native access CLI,
startup nofollow asset snapshots and the Node-free Salvo serving path. Ordinary
Cargo and the mandatory feature-enabled browser lane are separate.

Validation before final refresh presentation: enabled console integration 5/5 passed, including actual
Chromium en/zh/preferences, dynamic engagement creation/selection, missing periods,
explicit failure, raw native API refusal, logout, and the actual executable with
empty runtime PATH. Current JS regression 16/16 passes (native 2, retained usage 6,
refresh 8): the first latest invocation matched only native/refresh 10 tests, and
the correctly named retained usage selector separately executed its six tests.
Initial failures are preserved in the external console-first-check, tests1/2 and
Vitest1 logs: Salvo macro binding names, the intentionally refused macOS `/var`
alias, a route-announcer test locator, async SSR assertion and provider harness
compatibility, and JSON charset handling. No failure was counted as success.
Feature-enabled all-target Hagency Clippy with warnings denied and ordinary
workspace rustfmt passed. Ordinary native console/HTTP/usage regressions passed
10/10. All-feature workspace test inventory found 537 bindings with none missing;
listing compiles tests but is not their execution. An initial inventory invocation
omitted the assigned target and created a local target; it was unqualified evidence,
then removed with Cargo clean after confirming inactivity. Remaining commands use
the external task-local target wrapper.

Final strict lifecycle passed 8/8: the explicit 31-path boundary and seven selectors,
each with exactly one actual passing test and no failed/ignored tests in stdout.
This is a disclosed feature-adapted lifecycle, not unmodified stock agent-spec.
The external wrapper at
`/Users/yuechen/Library/Caches/hagency-rust-migration/2026-09-10/console-tools/run`
has SHA256 `f8fff88eb7382abdf1a5e509f08298414c208d17ae35db64bcc1d6ad3b675e62`.
For each bound selector it invokes the actual Cargo command
`cargo test --locked -p hagency --features native-console-browser -q <selector>`.
The wrapper source, actual argv JSONL and original complete lifecycle stdout are
retained under that external cache; ordinary rustfmt, Clippy and CI commands are
separate, with no feature-adaptation environment. Result files are
`console-lifecycle-final.json` and `console-lifecycle-counts.json`.

The final artifact 6 contains 134 assets / 6,010,899 bytes / largest 239,884 bytes.
Actual final browser screenshots are `console-screenshots-final/console-usage-en.png`
and `console-usage-zh.png`; the earlier unstyled controls remain in the original
`console-screenshots` directory as before evidence. Existing button, field and
split styles now preserve light/dark presentation. The final real Chromium run
also delays a same-selection read, confirms visible observations plus busy status,
aborts one actual transport to confirm explicit stale state, and succeeds on retry.
It preserves actual delayed navigation and logout regressions. Independent source
review found the original three races fixed and no material defect in the final
refresh delta; that review did not rerun tests. Neither local browser evidence nor
the new CI job claims unexecuted platform qualification. M7 and the overall
migration remain open.


### 2026-09-11 — Integrated verification and upload fixture isolation

At b856b47, full workspace/all-feature Clippy and binding inventory passed.
Original full Cargo run recorded633 independent passes plus1 nested pass and
3 failures. Two browser failures referenced a nonexistent Playwright cache binary;
explicit installed Chrome153.0.8010.36 reran both actual selectors successfully.
The third was historical file upload acceptance returning Capacity. Its exact
original binary passed a single-selector diagnostic; source tracing found the
capacity fixture intentionally consuming all64 process-wide permits under a
module-local lock. Its injected exhaustion now runs in a bounded exact-selector
child and fails on missing/failed child verdict or timeout. Production unchanged.

The first focused compile failed because tokio process was not an explicit Matrix
dev feature; that failure is retained. After adding only the development feature,
the whole Matrix library passed115/115,0 ignored. Logs and source/verdict metadata
are under the migration cache as upload-capacity-isolation-*,
console-correct-browser-diagnostic.*, and
file-publication-historical-original-artifact-diagnostic.*.
Original Node verify:ci passed; full Vitest4292 passed/1 failed/1 skipped.
The failing framework-acp-probe file passed3/3 when run separately, so its
original full-run cause remains unresolved. Hosted b856b47 macOS, browser and
Node jobs passed; Linux receive failures expose a separate O_PATH directory-sync
defect, and Windows original CI is still running. No full migration pass/cutover.

Capacity-isolation final qualification: strict lifecycle7/7 (boundary + six
actual scenario selectors),0 failed/skip/uncertain; package all-target warnings-denied
Clippy passed. All115 Matrix library tests passed before the focused lifecycle.
### Native approval fresh-clock prerequisite (2026-09-11)

Root approved an eight-path store partition in a new clean worktree from 087ffa6;
the active contract and accepted ADR043 scope were parsed/linted before source
edits. Production binding, request admission, direct and SDK verdict admission,
one-shot consumption and application observation now sample authority time after
the original SQLite Immediate lock. Deterministic repository timestamp entry
points remain available, and no parked renewal, coordinator, pump, schema,
dependency or sandbox policy changed.

Five focused tests pass: physical transaction exclusion at all six callbacks,
actual bounded queue admission, four lock-delayed admission cases, three delayed
consumption cases, and historical application settlement without current resume.
The existing 100 ms SQLite busy timeout remains unchanged; lock fixtures hold
about 65 ms. A temporary negative control restoring just the old six wrappers
produces the expected three failures: expired binding accepted, expired allow
consumed, expired execution resumed. Exact source was then restored. Two fixture
compile failures, historical provisioning setup failure and an overlong lock
fixture's DatabaseBusy failures remain in the external approval-clock evidence
cache. A first broader parallel regression passed 17/19 approval tests and exposed
fixture setup expiring before queue admission. The final fixture gives setup a
one-second lifetime, then enters the 65 ms contention window before expiry; it
still asserts the actual operation entered while authority was current.

Final affected regressions pass 52 store unit tests, 19 approval tests and eight
permission-coordinator tests (79 total, zero failures or ignored tests). Store
and permissions all-target warnings-denied Clippy, formatting and diff checks
pass. Strict package-scoped agent-spec 1.4 lifecycle passes 6/6: exact eight paths
and five selectors, each executing one actual test, with zero failed, skipped,
uncertain or pending-review results. One advisory file-output lint warning remains.
Final source hashes, exact commands/CWD, all original failures and verification
outputs are retained in the external approval-clock evidence cache. This
establishes no new native runtime application or full M6 parity claim.


### 2026-09-11 — Exact original domain writer CPU observation

The bounded ADR106 extension binds a query-only Windows thread handle to the
actual domain writer immediately before ConnectionDropStarted. GetThreadTimes
baseline and the first snapshot's checked CPU deltas cover that domain-drop
interval, not only post-SQLite-CLOSE work. Native PID/TID identify the writer,
not the caller shown in panic output. A first measurement or unavailable result
is frozen so later snapshots cannot retry or attribute subsequent thread work.
The handle is non-inheritable and closes with its original Probe through RAII;
there is no numeric-TID reopen, privilege change or thread suspension.

Original caller deadlines/results, close order, ownership release, queue and ACK
semantics are unchanged. The observation cannot identify a VFS call or establish
IO, mutex, sleep or descheduling as the original cause. No SQLite policy, VFS
shim, global tracing, payload/path projection or production repair is introduced.
The new field reports unobserved, unsupported, unavailable(stage), or measured
fixed scalars only. Actual maximum in-memory snapshot is 248 bytes under the
256-byte ceiling, and maximum-width complete Debug output is 863 bytes under
2048, including all native states and maximum timestamp/ID/delta widths.

Local store source was rebuilt in the explicitly assigned target. The initial
short --exact selector matched zero tests and is retained but not counted; the
correct full selector runs 1/1. All eight actual domain observation tests pass,
including the original paused-close timeout/lock assertions, callback association,
connection/ownership order, queue and snapshot bounds. The new non-Windows body
proves unsupported reporting only. Windows all-target compilation and Clippy
with warnings denied pass; native Windows execution remains required. Original
22c4993 failures and the source-derived SQLite candidate remain historical
uncertainty, not a fixed or supported backend verdict.

Strict lifecycle completes with 6/6 passing results: the exact 11-file boundary
and five selectors, each running one actual passing test. No failure, skipped,
uncertain or pending-review result remains in that local report. Its test-name
coverage does not distinguish cfg bodies: native Windows accounting assertions
remain unexecuted locally even though their selector has an unsupported-host
implementation. Source review found no additional material defect; no source was
changed after successful validation, only this final evidence prose.


### 2026-09-11 — Correct Linux received-file directory sync descriptor

Read-only review of b856b47 Linux originals traced all four successful-path
workspace sink failures to sync_all on the Root's cap-std O_PATH descriptor.
Pinned cap-primitives dir_utils.rs selects O_PATH on Linux but not macOS.
The actual original CI exposed Io, not raw errno; it remains failed evidence.
The candidate retains a readable '.' handle opened relative to the original
root, checks privacy and same_directory, then syncs that retained handle.
No ambient destination reopening, ignored sync error, or Windows change.
Existing bound sink scenarios cover actual materialization, one-shot refusal,
retirement, mutation and read-only deadlines. Format/diff and independent
source review pass; integrated native and actual Linux qualification are pending.

Integrated27b76d1: store unit54 + approval19 + permissions8 and incoming
executable4 all pass; warnings-denied all-feature Clippy passes. Owned22 cases
recorded21 pass and one mutation failure, Workspace(Retired) on the fifth
independent file before root replacement. That test shared one silent usage-gate
runtime across five filesystem mutation cases; the fixture's native read bound
is1500ms. Each independent mutation now retains its own fresh actual runtime,
with the same fixed limits and unchanged assertions. The exact original failure
is retained; this removes cross-case elapsed-time coupling without extending
any runtime deadline. Final integrated qualification follows this fixture change.

## Runtime cooperative approval control checkpoint

Implemented the approved 18-file runtime-only control-pump contract from b856b47
in an isolated tree. No domain, permissions coordinator, execution host, platform,
Cargo or live-service changes were included. Parent review identified that a
prepared callback must use its fixed response reserve while durable begin waits;
Waiting/Prepared/Sent state now retains that distinction without granting the
reserve to unprepared siblings. New deadline regressions exercise both cases.

All five new session fixtures pass locally, including partial-input custody with
one-byte duplex stdout, a retained pinned grant-batch future, exact opaque usage
order, new callbacks/resolution before first byte, one-shot/foreign/capacity
refusal, accepted-byte cancellation and original finite read/frame/write clocks.
An earlier broad runtime pass covered 55 tests including the actual guardian-owned
child stop fixture; final-source runtime/strict results are recorded below when
available. Clippy passes after boxing the returned observation and using a fixed
array for the one-grant fixture. Adding two large retained sessions to one async
deadline test initially overflowed that fixture's stack; splitting its independent
response-reserve cases into a separate test retained all assertions without
changing any stack limit or production deadline. Original local failure output
is retained in the external migration cache.

Final runtime source passes 56/56 package tests and Clippy with warnings denied.
Strict lifecycle passes 6/6 bounded checks, with no fail/skip/uncertain/pending;
its five selectors execute six actual regressions. The external full lifecycle
JSON preserves its separate wider requirement-trace missing-scenario diagnostic.
Windows GNU all-target compilation passes; Windows execution remains pending.
The domain grant's independent expiry is not extended by runtime response reserve.
Future host admission must align owner cutoff, original grant expiry and response
margin before parking; this runtime-only checkpoint authorizes no transmission.

### Approval router authority domain prerequisite (2026-09-11)

The root approved the ADR043/046 amendment after comparing retained requirements
with the original router decision/resume/write order, then approved this exact
26-path task from clean 99200b0. The active contract was parsed and linted before
source edits. Schema22 adds a separate response ledger without promoting legacy
Applying, Uncertain or Applied history. Current version assertions and historical
migration teardown change only as required by that additional table.

The original repository mints one opaque non-Clone non-serde grant on exact
atomic router authorization and consumption. The retained caller batch becomes
attempted before writer enqueue. A fresh original Immediate transaction checks
every barrier, full capability/private context/task/lease/expiry/grant scope and
the original deadline, then commits response-may-send and resumes only that same
attempt. A timely positive acknowledgement is necessary for send admission.
Owner deny permits baseline continuation after all barriers resolve. Generic
unpark cannot bypass this boundary; genuine parked task mutation still refuses.

A new callback reparks. Its own fresh grant resolves the new barrier; an earlier
never-written frame can use a readonly original-grant/original-deadline check.
The domain's local admission is consumed before awaiting a known local-write
observation receipt, so a lost receipt cannot permit another write check. Known
write evidence survives a later uncertain outcome. Native application evidence
remains independent and never resumes current, expired or historical execution.
The old offline coordinator API still exists but cannot create this authority or
resume an approval-parked attempt. Owned runtime integration stays separate.

Actual domain tests cover exact allow/deny batches, missing/duplicate/foreign
grants, original-instance mismatch, scope and expiry changes, parked mutation,
new callbacks, write-observation receipt loss, queued cancellation, lost and late
begin acknowledgement, schema21 upgrade, reopen and later native Applied. Real
SQLite exclusion proves clocks run inside the original transaction; actual
contention crosses lease/deadline expiry within the unchanged 100 ms busy bound.
Private negative-test acknowledgement delays hold actual committed results; they
do not synthesize success or set proof state. Host write/application observations
in these domain tests are explicit observation doubles, not native wire proof.

Final-source store/permissions all-targets pass 212 tests across 23 targets, with
zero failed or ignored tests. All-target warnings-denied Clippy, formatting and
diff checks pass. Strict package-scoped agent-spec 1.4 lifecycle passes 7/7: the
exact 26 paths and six selectors each executing one actual test, with zero
failed, skipped, uncertain or pending-review results. Two advisory lint warnings
remain for forbidden-scope coverage and inferred file-output coverage. Exact
commands/CWD, source hashes, API/failure-table handoff and all original results
are recorded in the external approval-router evidence cache.
Original fixture compile failures, the revoked-grant legacy expectation failure,
a saved-grant fixture setup failure, and intermediate tuple compile errors remain
in the external approval-router evidence cache. Runtime pumping, finite parked
maintenance, deadline configuration, actual transmission and executable
interactive qualification remain separate gates. No runtime, sandbox, timeout,
dependency or live action changed, and full M6 parity remains incomplete.

## 2026-09-11 — ADR108 retained native resource management slice

Started from clean dfba95b in an isolated worktree after parent review of the exact
31-path manifest/lock order/transition table; NativeUsage was explicitly approved
as path 32 for shared truthful logout. ADR108 and its task contract were parsed
and linted before source changes, including the amendment. No main checkout,
live configuration, services, dependencies or schema changed.

Implemented safe resources/selected budget/roles in the retained console and the
explicit finite native publication ticket. The store command captures original
scope and revision, keeps pending custody, checks actual SQLite/session locks and
changes only publication/derived roles. Default access remains read only. Unknown
and conflict persist through current-state reads; Busy/unknown logout offers a
visible retry without claiming revocation. Full native resource creation/editing,
Agent workflows and remote publication remain separate work.

Preserved original failures: an absent-selector run after a wrong working directory
is unqualified; the actual clock test later ran one passing test. The first clock
fixture incorrectly assumed edit_resource(None) would restore publication; it
now uses the existing explicit publication path for fixture reset. Native refusal
`code` mismatch was caught by actual HTTP, corrected in the browser parser and
rerun. Oversized observation coverage uses two individually admissible rows,
because the store correctly refuses a single command exceeding 64 KiB. Browser
loss-relay missing Fetch Metadata was diagnosed from actual headers and corrected
only in the disclosed fixture relay. No failing or zero-match run is a pass.

Current evidence includes real SQLite CAS/current clock checks, 15 passing JS
resource/usage/refresh tests, native HTTP scope and observations, actual bilingual
publication/dynamic resource/conflict/unknown/Busy logout, and an actual native
executable browser with empty runtime PATH. Feature-enabled all-target Clippy for
hagency and hagency-store passes with warnings denied. The retained asset build 2
contains 136 files / 6,069,255 bytes / largest 239,884 bytes and replays only actual
cached font bytes. Real light-English/dark-Chinese screenshots are retained under
external cache `resource-screenshots-4`. Final lifecycle results follow separately.

All Cargo/Node checks use external `resource-tools/run`, SHA256
`bae1c6b3f0a7593b61295cec6d24ea69d32e2f2a0b56c9385a5e3ef0c8891cb9`,
which forces the allocated prepared-stage-target and records actual argv. Its
explicit HAGENCY_RESOURCE_FEATURE_LIFECYCLE=1 adapter dispatches store selectors to
`cargo test --locked -p hagency-store -q <selector>` and console selectors to
`cargo test --locked -p hagency --features native-console-browser -q <selector>`.
This is adapted lifecycle execution, not unmodified stock agent-spec. Ordinary
rustfmt/Clippy/CI are separate; browser inventory alone is not browser execution.

Final ADR108 qualification passes strict adapted lifecycle 7/7: the exact 32-path
boundary plus six selectors, each with exactly one actual passing test and zero
failed or ignored tests in stdout. The final run uses artifact 4 (136 files /
6,069,438 bytes / largest 239,884 bytes). `resource-lifecycle-final.json` and
`resource-lifecycle-counts.json` retain the original output and inspected counts.
Earlier artifact-3 lifecycle output is retained separately, not overwritten.

The final ordinary console checks pass 8/8; the complete feature-enabled console
suite passes 9/9 including the original usage and both new resource browser/process
cases. The final copy-only resource change passes its five JS tests after the
15/15 combined resource/usage/refresh run. All-feature inventory lists 543 bound
Rust selectors with none missing; this is listing only. Final feature-enabled
all-target Clippy for both changed Rust packages and rustfmt check pass.

Parent visual review accepted the retained layout and requested Chinese resource
catalog/period labels; those now use 资源配置, 本地资源目录, 可用角色 and the existing
localized monthly label. Runtime-proof explanation is in Technical details.
Role keys remain unchanged under the existing dictionary contract. A final Busy
logout wording correction says the console is busy, because generic request
capacity can refuse logout even when no resource mutation is pending. Final
artifact-4 Chromium screenshots are `resource-screenshots-artifact4/console-resources-en.png`
and `console-resources-zh.png`. No production or unexecuted OS qualification is
claimed. Publication remains explicitly local-only pending its separate wire
integration, and resource creation/full editing plus complete M7 remain open.


## 2026-09-11 — Integrated native console/runtime checks and Cargo artifact lint

At d85369d the original full workspace/all-target/all-feature run passed661
independent tests plus1 nested child, with0 failures/ignored; all-feature Clippy
and formatting passed. This run used the actual installed Chrome executable,
Node24.10.0 and retained resource console artifact4. Original full Vitest:
4,298 passed,1 skipped,0 failed across295 files. Neither platform skips nor the
old failed b856 runs are counted as new passing product gates.

Concurrent verify-ci originally failed while ESLint traversed a transient Cargo
`target/debug/deps/rustc*` directory removed by compilation. The global ignore
now excludes root and nested `target` artifacts while native JavaScript sources
remain linted. A real ESLint regression proves both exclusions and rejection of
an undefined identifier in native/scripts; the full identifier test file passes
8/8. This fixes artifact traversal, without suppressing the source rule. The
original failure, source-independent integrated results and corrected checks are
retained under `~/Library/Caches/hagency-rust-migration/2026-09-10/`.

The task contract is parsed/linted and its explicit boundaries are checked by
agent-spec. Its Cargo-only lifecycle cannot execute these Vitest bindings, so
the exact Vitest file and verify-ci run separately; no Cargo result substitutes
for Node acceptance. Full migration and hosted qualification remain unfinished.

Corrected verify-ci completed successfully, including113 kernel/CLI tests.
Agent-spec's Cargo-only lifecycle reports these two Node scenarios as skipped;
that is retained as a non-passing lifecycle result, not relabeled as success.
The exact8-test Vitest run and passing verify-ci supply the actual JS evidence.

### Original FileService failure diagnostic partition (2026-09-11)

Parent approved the exact 11-file observation task from b211301. The bounded
contract was parsed and linted before implementation after reading ADR053 and
ADR101. Report snapshots the same original runner before stop; the existing
authenticated operator status projects closed failure labels and optional
counts. Original fixture failure output adds bounded existing helper-receipt
states, including absent, malformed, oversized, unsupported and preexisting.
No native event, keepalive, timeout, retry, process or domain behavior changed.
The original b856b47 Windows failure and later diagnostic cancellation remain
separate; local tests do not establish its source-derived timeout candidate.

The actual owned timeout/EOF regression, maximum-width operator projection and
complete FileService target pass locally: eight tests, zero failed or ignored.
Warnings-denied all-target Clippy passes for the execution and application
packages. Strict lifecycle passes 5/5 bounded checks, with zero failed, skipped,
uncertain or pending-review results. Its four selectors each execute one actual
test; 92 unrelated zero-test target outputs per selector are not counted as tests.
The full lifecycle JSON retains the separate wider requirement-trace diagnostic
for scenarios outside this task. Original b856b47 evidence and final-source
qualification records remain in the external migration cache; actual Windows
execution of these new diagnostics remains pending.
Windows GNU all-target compilation passes for both changed packages; this is
compile evidence only, not an executed Windows diagnostic or production fix.


## 2026-09-11 — Native resource catalog publication library (ADR109)

The original domain writer now observes one current registration and its complete
qualified public resource catalog. The existing outbound adapter can publish that
catalog automatically using its retained publication lane and exact frozen retry
bytes. Empty catalogs withdraw offers; explicit role withdrawal and fleet-scoped
cross-family qualification remain enforced. DTOs expose public identity/model
fields only, and more than200 resources per role or excess peer fields refuse
without partial publication.

Review found a queue window between the first registration observation and HTTP.
The publisher now checks the original domain registration after custody waits
immediately before HTTP admission. An actual SQLite-lock regression rotates the
registration while the publication is queued and verifies no request reaches the
local HTTPS peer. Already admitted HTTP bytes cannot be recalled by rotation;
retained catalog data never grants execution authority. Cancellation is rechecked
before admitting a fresh freeze after awaited domain work.

Focused tests: initial five tests passed; the added queue regression and other
three HTTPS selectors then passed. Dependency setup initially attempted a missing
workspace rusqlite entry; it now uses the same pinned0.37.0 bundled SQLite as the
store, dev-only, without upgrading any lockfile package. Package-wide, lint and
strict lifecycle results will be recorded after their original runs. Evidence:
`~/Library/Caches/hagency-rust-migration/2026-09-10/catalog-*`. Native executable
service ownership/wiring and full M5 acceptance remain separate and unfinished.


ADR109 final local qualification: affected store/Palpo packages226/226 with
zero failures or ignored tests. Strict lifecycle7/7 passes, including six real
selector tests and the exact change boundary. The original lifecycle preserved
6 passing selectors but failed its Cargo.lock boundary spelling; changing the
contract path from Cargo.lock to ./Cargo.lock corrected the parser recognition
without widening the allowed file. Final Clippy with warnings denied and fmt
pass. Original Clippy's six unused shared fixture-helper warnings were resolved
by a test-module-only annotation; no production lint rule changed. The independent
review's pre-send rotation and pre-freeze cancellation findings are closed.

### Native Palpo service wiring (2026-09-11)

The parent approved the exact 15-path service proposal and its existing-registration
prerequisite. The isolated tree starts at eb80500; bac727f catalog source integrated
as 508220c with both append-only documentation sections retained. The task contract
parsed/linted at 100% before implementation. Only the service layer is changed:
fixed private config, independent CLI flag, original Bootstrap store clones,
one retained adapter task, fixed operator status and non-consuming joined close.
The first executable run had one passed cancellation test and three fixture setup
failures from redundant stderr precreation before create-new open. The next run
passed cancellation/configuration refusal and rejected two fixture resource HTTP
edits because their serialized Resource included the forbidden derived roles
cache. Both original outputs are retained; the fixtures now use the existing
provider input shape and create stderr only once. No validation or deadline was
weakened. Actual hosted/consumer/live qualification is separate.

Final local macOS qualification passed 32 distinct tests: four actual executable/
HTTPS integration tests, 23 application library tests and five original Bootstrap
tests. The strengthened cancellation test holds the original publication and both
poll requests while checking joined service close and preserved unknown custody.
Final strict lifecycle passed all five checks with six actual selector executions;
95/95/94/94 empty Cargo target results are excluded from those counts. Selector
reruns overlap the 32 tests and are not additional distinct tests. Warnings-denied
Clippy, formatting and diff checks passed. All-target Windows cross-compilation
passed; this is compile evidence only, with actual Windows service execution still
pending CI. Both earlier fixture failure logs remain separate from final evidence.


## 2026-09-11 — ADR110 private approval card prerequisite

The clean native private-card worktree starts from492bc59. Its task contract was
parsed/linted before code changes. One original domain transaction now returns
an opaque pending owner-card packet with exact private target, operation digest,
parameters and persisted reusable scope. The owner cutoff cannot exceed request
expiry. Queued revalidation resamples the clock after the original SQLite lock.
Cards use the retained structured v1 fields, retain typed upstream RPC data in
an additive field, and refuse encoded content above48KiB without truncation.

The four focused behavioral tests passed, and the full approvals target passed
27/27 after the final human-readable expiry/display-field changes. No grants,
response admission, SDK sends or public endpoints are created. Actual encrypted
card transport, approval-purpose enrollment, executable integration and the
retained client's32-hex versus native40-hex request IDs remain separate gates.
Warnings-denied all-target store Clippy and fmt/diff checks pass. Strict lifecycle
passes5/5 (four actual behavioral selectors plus the exact11-path boundary);
zero-match Cargo targets are excluded. Tests used only isolated local stores.
The initial four focused tests are retained separately from the final27-test
qualification; neither is a Matrix send or client-interoperability claim.


## 2026-09-11 — ADR113 acknowledged quiet-turn budget

Original hosted492bc59 evidence is preserved: Linux667 independent passes plus1
nested, macOS666+1, Windows665+3 and one failing native_file_service_executable;
browser9/9 and Node4299pass/1skip. Windows's original suite finished before the
25-minute job limit canceled subsequent diagnostic compilation. Its first
retained cause is update/transport/timeout, zero pending RPC/server requests and
upload=true. No diagnostic was substituted for that original failed verdict.

In the clean02ce813-based worktree, a real child acknowledged turn/start, then
remained quiet for2200ms. Before the fix the actual test failed after1.83s with
Unknown instead of Completed, although its original operation budget was10s.
Host had mapped unsolicited event_wait_ms to the1500ms RPC response interval.
It now uses the existing operation duration; original absolute lifetime, RPC and
write deadlines, cancellation, authority checks and sandbox are unchanged.
The finite CI job budget becomes40minutes to retain diagnostics and release
time; no test or runtime deadline is increased.

After the mapping correction, all25 owned tests, the actual unchanged file-service
executable1/1 and low-level transport8/8 passed locally. The quiet deadline test
additionally requires reaching the original5s operation rather than expiring at
the RPC interval. Final owned target25/25 (including the stronger5s elapsed assertion),
warnings-denied execution/runtime Clippy and formatter pass. Strict lifecycle
passes4/4: three actual selectors and the exact9-path boundary, with empty
Cargo targets excluded. Original Windows file-workflow acceptance still requires
hosted requalification. All test runs used the explicit MAIN target from this
clean edited source worktree; no other agent used that target.

### Owned native approval coordinator (2026-09-11)

The parent approved the exact 28-path task from eb80500, including the one-for-one
fixture correction to `src/bin/approval_probe/mod.rs` so Cargo gains no binary.
The task was parsed/linted before implementation and again before that boundary
correction. No task-writer is provisioned in this checkout; these notes are
coordination, not a replacement control-plane task record.

The original OwnedSession now occupies Report before any startup await. A shared
ApprovalHost bounds live and parked owners; the unique OwnedApprovalScope keeps
only the original current lease within the immutable operation/capability/request
bounds. Its writer samples time inside the original IMMEDIATE transaction.
Generic parked task mutation remains refused. Actual supported callbacks and
prepared frames remain owned through request, grant consumption, admission,
recheck and write receipts, including cancellation and unwind. New callbacks can
park during an older pinned receipt; the original unsent response waits for every
current barrier. All returned write facts are retained before any await. Unknown
cleanup keeps capacity occupied, and failure uses the existing single negative
attempt fence without a per-grant cleanup loop.

Only pending owner choices emit a committed ID and the earlier immutable owner
cutoff. Saved grants use a fresh exact callback preparation/admission without a
private notice. A first late choice cannot use the response reserve to prepare or
acquire sending authority. Native write acceptance and resolved callbacks never
manufacture Applied. Usage records retain the existing original one-slot receipt;
no update is read past a refused slot. Pre-stop RuntimeObservation remains an
independent immutable diagnostic, including early control failure and unwind.

Focused qualification currently passes ten actual tests across eight selector
prefixes: actual native allow/deny/reuse, multiple and pending-receipt barriers,
resolution/EOF/retirement/caller-drop/late verdict, six real-effect receipt-loss
and unwind modes, shared capacity, original-scope maintenance, SQLite/queue expiry,
and positive/unknown usage custody. Warnings-denied all-target Clippy passes for
execution, store, runtime and permissions. Complete affected-target and strict
lifecycle qualification is in progress. Earlier failed checks are preserved in
the external migration cache: first compile field/API errors, an unverified
fixture session, missing post-write resolved observation, a locked reopen
negative fixture, initial formatting and Clippy argument/visibility findings.

Private SDK intake here is represented only by explicit domain observation test
doubles. Application selection, encrypted owner cards/intake, actual native MCP
transport acceptance and provider application remain separate parent-owned gates.
No production cutover, actual Windows execution or complete M6 parity is claimed.

Final coordinator qualification: parent approved the 29th path solely for the two
exhaustive bootstrap labels `approval_capacity` and `approval_cancelled`; its
corrected contract was parsed/linted before those two arms were edited. Source
qualification passes 316 distinct affected-package tests (execution44, runtime56,
store208, permissions8) and the existing bootstrap projection test, all with zero
failed or ignored results in the successful package records. The package records
are composed: the final execution/runtime run and final store run follow the
preserved earlier combined runs whose only failures were new fixture deadlines.
The original short bounds are now captured after each distinct fixture's setup,
before that original scope binds and parks; production bounds did not change.

A stronger actual regression exposed and fixed a late review gap: a successful
older begin receipt could permit another typed update after the original usage
write failed. The pre-fix test retained two callbacks instead of one. The owned
coordinator now stops/fences on that failed retained slot, finishes the same
older receipt, and retains its original frame/grant/write facts. Its final
regression and complete execution/runtime targets pass; generic usage behavior
is unchanged. The earlier strict pass remains preserved as earlier, narrower
evidence, not evidence for the final fix.

Final strict lifecycle passes9/9 (29 paths plus eight selectors), zero failed,
skipped, uncertain or pending review results. The selectors execute12 successful
test instances covering11 distinct tests; the maintenance clock case matches two
prefixes. Empty filtered targets are not counted as tests. Final warnings-denied
all-target Clippy and Windows GNU all-target compilation pass for application,
execution, store, runtime and permissions. Cross-compilation does not establish
actual Windows execution. Final artifacts, exact commands, source hashes and the
failure/custody handoff live in the external cache under `owned-approval-*`.
This completes only the accepted host coordinator partition. Full encrypted
private-owner/MCP service qualification and complete M6 remain separate gates.


### 2026-09-11 — ADR111 additional configuration implementation

Parent approved the 34-path proposal from d85369d and then the exact 35th path
native/hagency/tests/console/resources.rs for its accidental `config` substring
privacy assertion. The contract was parsed/linted before source edits and again
before that amendment. An initial unsupported Execution heading was refused and
corrected to Decisions; original refusal text is retained externally.

The original wizard now uses actual native model choices and exact source account
association for additional configurations and profile/ceiling edits. Unsupported
fields remain absent. Separate configuration authority cannot publish existing
rows or call operator APIs. Original session/SQLite gate clocks and configuration
CAS guard each command; unknown creation has no automatic retry. Native first
resource enrollment, credential management and complete M7 remain unimplemented.

Original failures are preserved in external configuration logs: an early inner-doc
ordering compile error, initial handler argument/name errors, an extra fixture
argument, the tagged-unit unknown-field regression, and the original privacy
substring assertion. Only that assertion was narrowed to the exact JSON key;
all private account/preset/credential sentinels remain. Actual Chromium artifact1
and artifact2 passed the new flow; the artifact2 numeric input was still unthemed
because the existing CSS only styles text/untyped inputs and selects. The native
input now uses that text selector with numeric inputMode and strict digit/JSON-safe
validation; final computed-style and screenshot evidence follows with final checks.

The task-local external wrapper configuration-tools/run has SHA256 `8126237db18e83ee16536dad061cae78f2166fbb285987f4358b4b8bb85309f8`.
It forces the allocated prepared-stage-target and records actual argv. Explicit
HAGENCY_CONFIGURATION_FEATURE_LIFECYCLE=1 dispatches the choices selector to
hagency-core, remaining native_resource_configuration selectors to hagency-store,
and console selectors to hagency with native-console-browser. This is adapted
agent-spec lifecycle, not unmodified stock execution. Separate fmt/Clippy/JS and
actual browser/executable results plus final lifecycle counts follow below.

The exact 36th path, mockup/scripts/native-console-resources-browser.mjs, was
approved and parsed/linted before editing. The original full console run was
11/13: both resource browser scripts installed a mutation-response listener after
awaiting Busy logout UI, which could miss the original response after the pinned
100 ms SQLite busy timeout. Each listener now precedes its triggering click;
all original Busy/unknown assertions and timeouts remain unchanged. The original
failure is configuration-console-final.log; corrected configuration-console-final-2.log
passes all 13 actual console tests, including retained usage/resources and the new
configuration browser plus native executable/restart. Authority tests pass 4/4.

Final core qualification passes 2/2, the affected store domain/qualification/
publication/configuration tests pass 6/6, and original configuration clock/caller
loss checks pass 1/1. Retained configuration/resources/usage/refresh JS tests pass
23/23, with the separately named resource-first console selector passing 1/1.
The first JS command's nonexistent dashboard-resource-first path supplied no
evidence; the actual tests/resource-first-console.test.js selector ran separately.
Feature-enabled all-target Clippy for core/store/hagency and rustfmt checks pass.

Final artifact4 has 139 files, 6,105,242 total bytes and largest file 239,884 bytes.
Chromium verifies the native token input has the same dark background as its
themed select and rejects unsafe/exponent token input. Parent visual inspection
accepted configuration-screenshots-4/console-configuration-en.png and the Chinese
counterpart. The full passing rerun also captured those views in
configuration-screenshots-5. Earlier white-input screenshots remain before evidence.

Strict adapted lifecycle passes 8/8: exact 36-path boundary plus seven selectors.
Each selector's captured stdout contains exactly one passing test and no failed or
ignored tests; no fail/skip/uncertain/pending verdict is accepted. Original JSON
and inspected counts are configuration-lifecycle-final.json and
configuration-lifecycle-counts.json in the external cache. Parent's read-only
writer/authority review found no concrete defect in this source checkpoint.

The final all-feature workspace inventory completed with no missing bound selector;
it compiles/lists only and is not additional execution evidence. Original output is
configuration-selector-inventory.log. This isolated slice changes no dependency or
schema and makes no live configuration, provider, login or production cutover change.


## 2026-09-11 — Take over native CI from Codex: approval receipts and file completion guard

- The Codex coordinator and its three sub-agents were stopped at the operator's
  request; their uncommitted approval CI corrections (ADR-116, two task specs)
  were adopted onto this branch from the owned-approval-ci-corrections worktree,
  with one compile fix (`task_id: None` in the projection scope test) and rustfmt.
  All 48 hagency-execution tests pass; the two macOS CI failures
  (`native_owned_approval_usage_successful_control`,
  `native_owned_approval_usage_unknown_slot`) passed five consecutive local runs
  after their fixed sleeps became actual failed-receipt observations.
- Root cause of the Ubuntu and Windows `native_file_service_uncertainty` and
  `recovery::native_file_service_restart` failures: the domain writer completed
  a dispatch whose file delivery was still `write_possible`. macOS only reached
  `outcome_unknown` through its unqualified process-tree census
  (`owned_failure: cleanup_unknown`); Linux observed `whole_tree_stopped`,
  completed the peer's turn and settled `completed`. Both were reproduced
  deterministically in a local arm64 Linux container (rust:1.95.0) and the
  diagnostic status was captured on both platforms.
- ADR-117 and `specs/task-rust-native-file-completion-guard.spec.md` add a
  file-delivery completion guard in the domain writer for every completion
  path. Not delivered and (no recorded failure, or a `write_possible` event or
  upload, or an `outcome_unknown` upload) refuses with `State`; the owned
  operation reports `SettlementUnknown` with a negative observation. New store
  test `native_file_delivery_completion_guard` covers begun, cancelled-after-
  begin, accepted-not-begun and reserved deliveries. The Linux container now
  passes all six file-service executable tests.
- Not addressed here: Windows-only failures at 1baa80d that depend on short
  scheduling windows (`clock::native_approval_consumption_clock_after_lock`,
  `native_file_delivery_worker_lost_and_queued` lock branch,
  `native_matrix_upload_custody_capacity` deadline) and the Windows
  `native_file_service_media_startup_observation` stuck at `enrolling`. Hosted
  Windows runs remain the qualification; the local container is Linux only.
- Two Windows-only fixture budgets were made hosted-tolerant without touching
  runtime deadlines: `native_file_service_media_startup_observation` now bounds
  peer requests (96) and time (20 s) separately instead of spending its loop
  budget on idle polls while the child is still `enrolling`, and the isolated
  `native_matrix_upload_custody_capacity` child process gets 180 s to load the
  unoptimized test executable. Both pass on macOS; the file-service suite passes
  on Linux.
- Hosted run for `58ec3c9`: Node CI passed; native macOS and Ubuntu passed for
  the first time since the file-service tests were added; Windows dropped from
  14 failures to two owned-approval fixtures whose five-second owner window and
  ten-second operation budget expired on the hosted runner
  (`approval_resume` returned RunnerAuthority at verdict time,
  `barriers_pending_receipt` saw ApprovalCancelled). The shared test policies
  now allow a 20 s owner wait and 25 s operation budget; the dedicated 300 ms
  expiry test is unchanged. The console-browser job failed once on a Playwright
  wait for the transient Busy logout state in the resource-configuration flow;
  it passed at 1baa80d and is not caused by these changes.


ADR115 implements the explicit finite retained32/native40 approval v1 schema
amendment in a clean1baa80d worktree. The original Rust writer exported five actual
cards (POSIX, Windows drive, UNC, unknown scope and long preview); fixture bytes
are captured in tests/fixtures/native-approval-wire.json. Export mode is tooling,
not qualification. The normal full approvals target then passed29/29, including
exact corpus comparison. Real retained JS producer/schema tests plus original
bridge approval tests passed19/19 in two files. Store all-target Clippy and fmt
passed. Original logs and commands are wire-interop-*-original.log and
wire-interop-original-verdicts.json in the external migration cache.

The schema preserves legacy body/description/preview limits and keeps native
48KiB encoded packet capacity distinct from schema character limits. Request-only
typed RPC metadata does not enter verdicts. The retained JS backend remains
32-only because it owns no native requests; Robrix parser/click and verified native
intake/transport remain separate gates. Stock strict agent-spec1.4 lifecycle completed nonpassing: two passes (exact11-path boundary and actual Rust corpus) and two Node-selector skips. No skipped or zero-test target counts as pass; actual Vitest19/19 remains separate evidence. The raw output includes the CLI error trailer and is preserved in wire-interop-lifecycle-original.json; wire-interop-lifecycle-exit.json records exit1. This is the documented Cargo-only lifecycle limitation, not a completed client integration gate.

### ADR112 original approval SDK enrollment and private-card delivery

The parent approved the exact28-path proposal before implementation. Clean base
c527314 contains e5fd184 service plus card prerequisite c274d11; append-only docs
from both prerequisites were preserved. The Task Contract parsed and linted
before source edits. The first library check caught two missing matrix_room server
arguments; that original log remains retained. After correcting those callsites,
the library check passed. The first actual crypto run returned Cancelled after a
40m13s cold build; this original failure remains separate. The first full Matrix
package had123 pass/one fixture failure: cancellation before HTTP handoff could
correctly prevent SDK Apply. The fixture now waits for the actual interrupted
Apply result before cancellation. No production deadline or retry changed.
The corrected full Matrix package passes145/145:124 library (including9 new),
6/6/9 integration; empty doc targets are excluded. Original ordinary enrollment,
outgoing crypto and approval intake remain covered. Clippy first reported four
collapsible-if cases and duplicate private helper inclusion; after equivalent
conditional formatting and one test-only helper annotation, all-target
warnings-denied Clippy and fmt/diff checks pass. Exact boundary is28 paths.
Strict lifecycle passes7/7 (exact28-path boundary plus six selectors executing
nine actual tests;96 empty targets per selector excluded). The final formatted
source also passes the complete145-test Matrix package. Requirement-trace warnings
for other mapped project scenarios remain explicit; they do not qualify the
client, executable or full approval workflow. Original failure logs and final
source/evidence hashes are retained in the task cache.
The independent recipient fixture derives state only from real uploads/claims,
with original Agent constants preserved as defaults. Synthetic receipt capacity,
injected shutdown-result failure and actual recipient decryption remain distinct
claims. Persistent64-record capacity does not free on restart; service wiring,
client qualification and ongoing identity/key management remain separate.
- Integrated the two slices Codex committed at 14:25 but never landed:
  approval wire interop (`a396bfe`, ADR-115) and private approval SDK delivery
  (`7387264`). Docs conflicts were merged by keeping both sides. Local gates:
  store approvals, the full matrix and hagency crates, the four new Vitest
  checks, fmt and clippy pass. The wire-interop contract bound two Vitest
  scenarios under a `rust` tag, which both spec-binding checkers would have
  rejected; those scenarios now live in a sibling Node contract (`9e0354a`).
  Rust bindings 628 and Node bindings 547 with none missing.
- The codex/native-receive-service-20260911 worktree's uncommitted checkpoint
  (receive catalog, runner and task-client received modules) is superseded:
  those files already landed through the receive-executable slice
  (`3110f9f`, `1cb60d7`). Nothing from that worktree needs integration.
- Adopted Codex's unfinished managed-account binding slice (ADR-114 part A,
  schema 23) from the feat/native-managed-account-binding-20260911 worktree.
  Its pre-stop diff applied to the current branch with a three-way merge
  without conflicts. Four completions were needed: the driver test fixture
  lacked the new `managed_account` field; the `account` subcommand declared a
  required global `--state-dir`, which clap rejects at debug time
  (`native_account_cli` panicked in clap's debug asserts), so the flag is now
  an optional global that the command refuses without; twelve store
  migration tests still asserted schema 22 after rewinding and now assert 23;
  and the accounts unit tests loaded `tests/common/mod.rs` a second time,
  which clippy's duplicate-module lint denies, so they reuse the worker's
  `clock_fixtures` include instead. All eight bound scenarios
  pass locally: identity, preparation, association, retirement, schema22,
  host consumer, owned current and the offline CLI, plus the bootstrap check.
- Hosted run for `5c8312e`: Node CI, console-browser and macOS green. Ubuntu
  failed once in the new `native_private_approval_fresh_enrollment_and_delivery_after_expired_refusal`;
  it passed three consecutive runs in the local Linux container, so it is a
  hosted-runner timing flake, not a regression. Windows: the verdict diagnostics
  show `native_owned_approval_resume` refused with the request pending, the
  dispatch parked and the card unexpired, so the liveness predicate (dispatch
  lease, capability, resource lease, workspace dirty flag or task epoch) is what
  refuses; the fixture now prints those facts. `native_account_host_consumer`
  hit ERROR_SHARING_VIOLATION renaming a retained namespace directory; on
  Windows the retained handle refuses the rename itself, which the test now
  asserts instead of continuing the Unix replacement flow.
- Hosted run for `3bcf5d6`: Node CI, console-browser, macOS and Ubuntu green.
  Windows: `native_owned_approval_resume` refused again; the liveness facts at
  refusal time were parked, fence 1, one resource lease, workspace clean, task
  epoch 0, capability 60 s ahead and the 5 s dispatch lease 4.9 s ahead, so the
  only time-varying predicate is the lease that `watched()` renews every
  100 ms. The fixture now retries a refused verdict up to five times 200 ms
  apart and reports the attempt count, to distinguish a transient lease lapse
  from a persistent refusal. Windows also returned Timeout instead of
  Transport once in `native_outbound_http_authority_tls_and_redaction`
  (hostname mismatch against `localhost`), left as observed for now.
- Added a manual `Windows probe` workflow (`.github/workflows/windows-probe.yml`)
  that builds selected packages' test targets once on `windows-2025` and runs
  chosen selectors N times with `--nocapture` and backtraces, uploading the
  per-iteration logs. It is diagnostic evidence only and does not gate anything;
  the Native Rust workflow keeps the verdict. Default selectors: the two
  owned-approval fixtures, the cancellation fixture and the Palpo hostname
  mismatch test that failed on hosted Windows.
- Probe evidence (runs 34666824897, 34666903801, 34667077363, 34667172019):
  on hosted Ubuntu the two approval-delivery tests pass ten of ten serially,
  so their full-suite timeouts are CPU contention from the parallel matrix
  crate; the delivery fixture now keeps its own HTTP budgets (connect 2 s,
  headers 2 s, request 4 s, body idle 1 s, SDK 20 s) since no delivery
  scenario expects a timeout. On hosted Windows, serial and isolated, the
  Palpo hostname-mismatch test and the barriers and cancellation fixtures
  pass five of five; `native_owned_approval_resume` fails five of five with
  RunnerAuthority on every retry. Its dumped rows show a pending request, a
  parked dispatch with a live exclusive lease, a clean workspace, epoch 0, a
  consistent route and binding, and a callback cwd of the verbatim
  `\\?\C:\...` form with no reusable scope, which a Once or Deny verdict does
  not need. The refusing predicate is still unidentified.
- Windows root cause found through the probe's writer-side traces: the
  refused verdict is the `Always` choice in the reuse half of
  `native_owned_approval_resume`. The child launched in the verbatim canonical
  root reported `\\?\C:\...` as its callback cwd, the parser refused it as
  designed, no reusable scope existed and the persistent grant was refused.
  ADR-116 is amended: the session and process working directory string is now
  the ordinary projection of the retained root on every platform, with custody
  unchanged on the handle. Temporary debug-only refusal traces remain in the
  store until the hosted probe confirms the fix, then they are removed.
- Probe run 34668960156 confirms the amendment: hosted Windows passed
  `native_owned_approval_resume`, both other approval fixtures and the Palpo
  hostname-mismatch test five of five each; hosted Ubuntu passed the two
  delivery scenarios five of five. The three temporary debug-only writer
  traces are reverted, and the fixture's bounded verdict retry is removed
  again since a refusal is a real defect; its retained-state dump stays.
  The `probe/windows-approval` branch remains as the trigger for future
  probes until the workflow reaches the default branch.
- Full hosted run for the cleaned tree (`6206d2d`): Node CI, macOS, Ubuntu and
  console-browser green; Windows failed eight tests. Four asserted that a
  runner reports the verbatim canonical fixture path as its cwd
  (`native_owned_mcp_real_done_epoch`, `native_owned_mcp_real_heartbeat`,
  `native_receive_executable`, `native_owned_dispatch_real_pipes`) and now
  compare against the shared `hagency_execution::ordinary_launch_path`
  projection. The other four (`native_runner_http_*`) failed the domain
  shutdown with `ReplyTimedOut` at SQLite close, a Windows teardown stall
  already observed at 85427cb before any of today's changes; it stays open.
- Hosted Ubuntu failed `native_owned_runtime_failure_observation` once on the
  run for `c96f76c`: it asserted `write == None` for the completed initialize
  frame, but the transport keeps a fully written frame as unconfirmed until
  its flush is observed (`native_codex_transport_failures_flush_broken_stream_and_eof`
  relies on that), so an injected failure landing first reports the complete
  frame. The assertion now accepts none or a fully written frame and still
  rejects a partial one.
- Hosted run for `c96f76c`: Windows tests passed 720 of 720 across 103
  binaries, the first fully green Windows test step on this branch; the job
  was then cancelled at the 40-minute limit during the release build (clippy
  2 min, spec-binding inventory 7 min, tests 17 min, release build past
  04:24Z). The native job budget is now 60 minutes. Ubuntu's single failure
  was the timing assertion relaxed above; macOS, console-browser and Node CI
  were green.
- Hosted macOS failed `usage::native_owned_usage_real_capture` once on the run
  for `c3b9ea8` with `Locked` on the immediate reopen after an acknowledged
  shutdown. The writer provably drops its repository before acknowledging,
  so the residual lock is a transient release lag; the usage tests now poll
  the reopen for up to two seconds on `Locked` instead of failing on the
  first attempt.
- Hosted run for `c3b9ea8`: the Windows native job completed successfully
  end to end for the first time (49 minutes: tests, media qualification,
  release build and artifact), Ubuntu and console-browser passed, Node CI
  passed. macOS failed only the transient `Locked` reopen fixed in the
  following commit.
- Hosted run for `3899b45`: Node CI, console-browser, Ubuntu and macOS green;
  Windows failed three timing-bound fixtures under the parallel test binary.
  `native_receive_workspace_authority_read_deadline` gave the write one
  second and hosted Windows materialized later than that, so the write window
  is now three seconds while the elapsed-deadline refusal and the fresh
  revalidation stay exactly as asserted. `native_file_delivery_worker_lost_and_queued`
  (`lock=true`) holds an external Immediate transaction for 50 ms against the
  worker's 100 ms busy budget; when the host stretches the fixture's own hold
  past that budget the scenario was never modelled, so the fixture measures
  its hold and rebuilds (bounded to five attempts) instead of judging the
  worker, and a modelled hold still requires exactly `RunnerAuthority`.
  `native_receive_uncertainty` reported `outcome_unknown` with a transport
  timeout at `turn_start` (`response_ms` 1500, the fixture ceiling is 2000):
  the incoming probe was still inside its MCP `tools/call` when the budget
  expired. That one is left unchanged and recorded as a hosted Windows
  process-spawn latency case for the Windows VM to reproduce.
- Hosted run for `a5a8018`: Node CI, console-browser, Ubuntu and macOS green;
  the two widened fixtures passed on Windows. Windows failed two different
  tests of the `owned_matrix` binary in the same four-second window:
  `native_matrix_owned_notice_failure` got `OutcomeUnknown` from the domain
  shutdown at `Workflow::close` (the same teardown stall as the earlier
  `native_runner_http_*` failures) and `native_matrix_owned_complete_workflow`
  saw the task still `InProgress` after `operation.wait()` returned (no
  stdout beyond the assertion; `response_ms` 2000, `operation_ms` 30000).
  Neither test had failed on any earlier hosted run. Both are recorded, not
  changed: the hosted Windows suite now fails a rotating set of two to four
  timing-bound fixtures per run, so the next step is the Windows VM
  reproduction and the shutdown analysis the `dsflash` peer is producing.

## 2026-09-12 — Native transcript attribution search (ADR118)

- Ported the pure attribution chain of `lib/metering/attribute.js` to
  `hagency_metering::attribution` (ADR-118, proposed): workspace precedence
  (lastWorkspacePath → workspacePath → workdir/homeDir for on-demand runners,
  JS-trim), Claude's forward-only `/`→`-` project mapping, per-framework
  search descriptors (claude narrowed/non-recursive, codex flat-keyed by date
  so not narrowed and recursive), session totalling over injected `(file,
  text)` pairs with the exact JavaScript `available: false` reason strings,
  and the fleet summary that keeps shared workspaces ambiguous. No filesystem
  access; per-session totals reuse the ADR-055 `parse_session`. Unknown stays
  `null`, never zero; refused parses count as skipped.
- Gates: `native/scripts/attribution-vectors.mjs` generates the 107-vector
  oracle fixture from the retained JavaScript and its `--check` was added to
  the Rust CI job after `metering-vectors.mjs --check`;
  `cargo fmt/clippy -D warnings` and `cargo test -p hagency-metering --locked`
  (13 tests, including the three new replay tests) all pass locally.
- Probe run 34680169676 on `probe/windows-shutdown` (the four stalled
  `native_runner_http_*` selectors with the teardown sample, ten iterations
  each at eight test threads and again serially): forty of forty passed and no
  shutdown timed out, so the stall is not reproducible from those four tests
  alone. That is consistent with the peer analysis: the failing four were the
  earliest closes of a busy binary, and the trigger is the whole process's
  first seconds under parallel load (or the process-global SQLite SHM mutex
  serializing a slow purge). Probe run 34680568569 repeats every `hagency`
  test target under parallel load four times to reach that state.
- Probe run 34680568569 (every `hagency` test target, eight threads, four
  iterations) reproduced the stall on its first iteration only: four
  `native_runner_http_*` closes timed out with `ReplyTimedOut` and the new
  teardown sample showed `domain.sqlite3-wal` at 375 to 416 KB and
  `domain.sqlite3-shm` at 32 KB still present at the moment of timeout, so
  the writer is stalled before the WAL is removed (inside the close-time
  checkpoint or the unlink retry), not after. The sample now also records
  the main database size and modification age and repeats after 500 ms to
  separate those two. Hosted Windows also failed the new
  `attribution-vectors.mjs --check`: the retained JavaScript computes POSIX
  layout facts with the host path module, so the oracle is POSIX-only; the
  CI step is gated to POSIX hosts and the script refuses on win32 with that
  reason, while the Rust replay tests keep running on every OS.
- Probe run 34681376784 (`probe/windows-shutdown` with the deepened teardown
  sample, every `hagency` target, eight threads, four iterations) did not
  reproduce the close stall; iteration 3 instead failed
  `accounts::native_account_bootstrap` and `native_bootstrap_executable`
  in the shared fake Matrix server, where the next scripted request did
  not arrive within the fixture's SDK-plus-HTTP orchestration budget. Same
  class as the other hosted Windows failures: a fixture bound under
  whole-process load, not a product deadline. Recorded, not changed.
- ADR-120 (accepted): both private stores set
  `SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE` right after open, so a close performs
  no checkpoint and unlinks neither `-wal` nor `-shm`; the next open replays
  them. This removes every mechanism the hosted Windows shutdown stall has
  been placed in (close-time checkpoint and its sync, the refused-delete
  retry loop, the process-global SHM purge) without touching a budget, phase
  or verdict. Spec `task-rust-native-bounded-close-path` binds
  `native_store_close_leaves_wal_for_replay`. Durability stays on
  `synchronous=FULL` at commit. The whole-package Windows probe decides
  whether the stall is gone.
- Probe run 34681480044 (deepened sample, every `hagency` target, eight
  threads, four iterations) failed two of four iterations with fourteen
  distinct tests across the `hagency` library tests (file service, MCP
  coordination, bootstrap driver) and four `mcp_coordination` domain
  shutdowns with the same signature as the runner stalls: SQLite close
  entered, nothing after it, zero writer CPU, four threads of one process.
  The stall is therefore process-wide under load and not tied to one
  fixture, which is what ADR-120 addresses; the runner fixture's deepened
  sample did not fire because its own binary passed in that run.
- Hosted run for `94c1177`: console-browser and macOS green, Windows
  cancelled by the following push, Ubuntu failed once on
  `approvals::native_owned_approval_cancellation` with "notice channel
  closed" (the operation ended before committing an approval request; the
  test prints nothing else). Second occurrence of this selector on hosted
  Ubuntu. Recorded, not changed; the fixture should print the operation
  report when the notice channel closes before it is treated further.
- Operator Windows VM (`54.156.69.166`, Server 2025, 4 vCPU, 16 GB, Defender
  on, MSVC 2022 Build Tools, Rust 1.95.0 MSVC): every `hagency` test target
  at `94c1177` ran four times under eight test threads with no shutdown
  stall (0 of 4 failed), so the package-level probe does not reproduce on
  this VM even though it reproduces one in four on hosted `windows-2025`.
  The CI-equivalent whole-workspace suite is now running there three times
  at the same baseline before the ADR-120 tree is tried.
- Probe run 34682424382 (ADR-120 tree, every `hagency` target, eight
  threads, four iterations): no domain shutdown timed out in any iteration,
  the first hosted whole-package run without a `ReplyTimedOut` snapshot.
  Three of four iterations still failed, dominated by "collector completed
  before its HTTP script: Err(OutcomeUnknown)" from the shared scripted
  fixture (`hagency-matrix/tests/common/mod.rs:148`) across file service,
  owned Matrix and bootstrap tests, plus one fixture orchestration budget
  and one restart recovery miss. That class also appeared before ADR-120
  and is the next analysis target; it is a different producer of
  `OutcomeUnknown` than the domain shutdown.

## 2026-09-12 — Native bounded transcript reader (ADR119)

- Ported `lib/metering/reader.js` to `hagency_metering::reader` (ADR-119,
  proposed): the four ceilings with their defaults (30-day window, 200 files,
  8 MiB per transcript, 20 000 walked entries), `ReaderLimits::from_env`
  parsing explicit strings with the JavaScript `parseInt` positive-integer
  rule, the breadth-first walk that recurses only where the layout says so
  (nested Codex date tree recursive, Claude project directory flat), the
  narrowed-vs-unread age split, newest-first reads under the file ceiling,
  line-boundary truncation with `lastIndexOf`-exact semantics, and
  `bounds_report` with the exact JavaScript wording keeping understatement
  and unknown-ownership claims apart. `meter_fleet` ports the TTL cache
  (force, `computedAt` stamp, `reset`) keyed over fleet identity and home
  directory with an injected clock.
- Gates: `native/scripts/reader-vectors.mjs` records 21 vectors (13 reads,
  5 reports, 3 fleets) from the retained JavaScript over synthetic trees and
  its `--check` was added to the Rust CI job after the attribution line;
  `cargo fmt/clippy -D warnings` and `cargo test -p hagency-metering
  --locked` (18 tests, five new reader tests) pass locally, plus the
  attribution and metering vector checks.
- Operator Windows VM, CI-equivalent whole-workspace suite at `94c1177`,
  iteration 1: four failures and no domain shutdown timeout
  (`native_file_service_shutdown_original_job_unwind` on an orchestration
  `Elapsed`, `approvals::native_owned_approval_barriers`,
  `approval_loss::native_owned_approval_barriers_pending_receipt`,
  `card::native_private_approval_card_clock`). The VM reproduces the
  load-bound fixture class but not the SQLite close stall; the hosted
  runner reproduces both. Iterations 2 and 3 are running.
- Hosted run for `5c20f8c` (ADR-120): console-browser, Ubuntu and macOS
  green; Windows ran the whole suite with no shutdown timeout and failed
  only `approvals::native_owned_approval_barriers` and
  `approval_loss::native_owned_approval_barriers_pending_receipt`, both
  asserting `Protocol::Unknown` where a completed protocol was expected
  after an `ApprovalCancelled` failure at stage `Update`. The same two
  failed on the operator VM's whole-workspace baseline, so they are now
  reproducible off the hosted runner.
- Probe rerun 34682424382 (second ADR-120 sample, every `hagency` target,
  eight threads, four iterations): again no shutdown timeout; one
  iteration failed only `native_matrix_owned_complete_workflow`, now with
  the writer's verdict printed next to the raw row. Two consecutive
  whole-package samples without a `ReplyTimedOut` snapshot, against one to
  four per failing iteration before ADR-120.
- In the second ADR-120 probe sample the one failure,
  `native_matrix_owned_complete_workflow`, was no longer the status
  mismatch but `report.failure == Some(SettlementUnknown)` at
  `owned_matrix.rs:154` (completion custody could not confirm the
  settlement within its bound). Recorded for the next analysis round.
- Operator VM whole-workspace baseline, iteration 2: the only failure was
  `approvals::native_owned_approval_barriers`, no shutdown timeout. That
  selector has now failed on both VM iterations and on the hosted ADR-120
  run, so it is the first Windows defect reproducible on demand off the
  hosted runner; it will be run alone with full output on the VM next.
- The shared scripted fake-server fixture (`hagency-matrix/tests/common`)
  keeps its tight `limits()` for this crate's transport-bound tests, which
  deliberately drive slow headers and deadlines against them, and gains
  `load_limits()` (connect 2 s, headers 2 s, request 4 s, body idle 1 s,
  sdk 20 s, the suite-local precedent, all below the production defaults)
  for the service-level fixtures that share a current-thread runtime with
  the fake peer: the file service admission tests and the owned Matrix
  workflow now run with them, the wait between scripted requests uses
  them, the fake peer's TLS accept has five seconds instead of one, and an
  early collector settlement reports its elapsed time. Fixture
  orchestration only; no product deadline, verdict or assertion changes
  (ADR-080 invariant kept).
- Operator VM whole-workspace baseline at `94c1177`, three iterations: all
  three failed, `approvals::native_owned_approval_barriers` in every one,
  `approval_loss::native_owned_approval_barriers_pending_receipt` and
  `card::native_private_approval_card_clock` in two, `approvals::native_owned_approval_usage`
  and `native_file_service_shutdown_original_job_unwind` once; no shutdown
  timeout on the VM at all. The barrier selector is now being run on the
  VM alone and under the owned binary's own eight-way load with full output.
- The reader replay test imported a Unix-only permissions extension
  unconditionally, so every Windows build of the workspace test targets
  failed to compile at `b1c1634` (caught on the operator VM before the
  hosted run reported). The unreadable-directory vector is now Unix-only
  and skipped elsewhere with a printed reason; the other vectors replay on
  every OS.
- Probe run on `probe/windows-shutdown` at `f9ff420` (ADR-120 plus the
  service-level fixture budgets; every `hagency` test target, eight
  threads, four iterations): four of four iterations passed with no
  failing test, no shutdown timeout and no early collector settlement.
  First fully green whole-package Windows probe on this branch. A rerun
  collects a second sample.
- Probe rerun at `f9ff420`: again four of four iterations green. Eight of
  eight whole-package iterations without a failing test since the fixture
  budgets landed on top of ADR-120.
- Probe run with the `hagency-execution` targets added (eight threads,
  four iterations): two of four iterations failed, no shutdown timeout.
  The approval write-ordering class appeared as expected (`usage`,
  `cancellation`, `barriers_pending_receipt`), and under the extra load
  three `native_runner_http_*` tests and `native_owned_mcp_real_heartbeat`
  failed for reasons recorded in the session log; the runner fixture's
  teardown sample did not fire, so these are not the close stall.

## 2026-09-12 — Stall-witness test harness (ADR-106, harness only)

- Harness-only change, no product source: every fixture shutdown that
  observes a domain or custody close now asserts the `ShutdownOutcome` and
  prints `[shutdown-stall] <outcome>` with the snapshot (runner,
  mcp_coordination, owned_matrix Workflow and RunnerApi, the matrix common
  `shutdown_domain` — which also covers the file-service lib tests that
  include it via `test_common`). A new process-wide test latch
  (`hagency-matrix/tests/common/stall.rs`, first-writer-wins `OnceLock`)
  records the first timed-out shutdown with its snapshot and a two-point
  database teardown sample; a `witness(context)` helper lets later failure
  paths attribute themselves to the recorded stall instead of misreporting
  it (owned_matrix's status assertion now consults it and prints the
  writer's verdict). The approval `notice()` helper no longer fails with a
  bare "notice channel closed": it prints the operation's report and the
  retained-state dump `choose()` pioneered, deadline unchanged. Nothing is
  retried, widened or ignored; every affected test still fails.
- Gates: `cargo fmt --all --check` and
  `cargo clippy -p hagency -p hagency-execution -p hagency-matrix
  --all-targets --locked -- -D warnings` pass. The cargo test gates were
  run but cannot pass in this sandbox: every test binary fails at fixture
  setup with EPERM on the SQLite repository open (`Repository::open` /
  `DomainRepository::open`), including on the unmodified tree (verified by
  stashing all changes and re-running: identical failures), so the failures
  are environmental seatbelt denials, not regressions; per-target `cargo
  check` of every touched test target is green.

## 2026-09-12 — Settlement cause marker for SettlementUnknown

- `Report` now carries `settlement_cause: Option<SettlementCause>` (ADR-060
  amendment, appended today): a bounded `Copy` diagnostic discriminant set by
  `settlement_failure(report, &error)` at the three producer sites in
  `hagency-execution/src/operation.rs` (`observe_owned_completion`,
  `publish_owned_completion` with checkpoint precedence preserved,
  `complete_owned_dispatch`), so a lost settlement names whether the command
  never entered the writer queue (`QueueBusy`), the writer is gone
  (`QueueUnavailable`), the reply timed out (`ReplyTimedOut`), or it was
  refused (`RunnerAuthority`/`State`/`Quarantined`); everything else reports
  `Storage`. `Failure` stays `Copy` and every existing
  `assert_eq!(report.failure, Some(Failure::SettlementUnknown))` compiles
  unchanged. The bootstrap status projection adds `settlement_cause` next to
  `owned_failure`. The marker is diagnostic only — never authority, retry,
  reply or lease input, and carries no error text.
- Tests: `native_settlement_cause_markers_are_distinct` (every mapped
  refusal distinct, `Storage` the fallback, all seven markers pairwise
  distinct) passes; `native_owned_dispatch_real_pipes` and
  `native_matrix_owned_complete_workflow` assert the field is `None` on
  clean completion. A queue-`Busy` test is not reachable from the execution
  crate: the domain writer's queue is a private field, and the parking tests
  in hagency-store reach it only through in-crate `#[path]` includes.
- Spec: two scenarios added to the ADR-060 completion spec binding
  `native_settlement_cause_markers_are_distinct` and
  `native_owned_dispatch_real_pipes`.
- The approval phase trace ran on the operator VM under the owned
  binary's eight-way load: every `ApprovalCancelled` is the
  `ApprovalResolved` primitive on an entry at retained, acknowledged,
  prepared, begun, admitted, checked, without a write-accepted record.
  The fixture probe resolves an approval only after reading its response,
  so the frame was on the wire while the host had not yet recorded the
  receipt. A two-predicate change that merely stops the cancellation was
  trialled earlier and moved the failure into a re-send the transport
  refuses; the corrected design (complete the in-flight write, record its
  acceptance, never re-send) is being written before any product change.
- Hosted run 34689032882 for `63c4ac9`: console-browser, Ubuntu, macOS
  and Windows all green. First run on this branch where every Native Rust
  job passed, including the full Windows suite with no shutdown timeout
  and no approval cancellation. The approval middle case remains
  intermittent under load (reproducible on the operator VM) and its fix
  is in implementation; this run is the baseline for measuring it.

## 2026-09-12 — Console-browser logout-wait failure evidence (diagnostic)

- Diagnostic for the hosted console-browser flake
  (`context-console-browser-flake.md` §4.1 item 1, Node driver only):
  `native-console-resource-configuration-browser.mjs` gains an
  `expectLogoutState` helper. It waits for the same
  `[data-logout-state="<state>"]` selector with the same default bound and, on
  timeout, reports the observed attribute value, the section count, the first
  400 characters of `main`'s text, and — when `HAGENCY_CONSOLE_SCREENSHOTS`
  is set — saves a full-page screenshot named for the expected state, so a
  hosted artifact can tell "never rendered" from "rendered with another
  value" (the product maps only `Error::Busy` to `busy`; everything else
  renders `unknown`). The two bare waits (`busy`, `ended`) now go through it.
  No bound changes, no assertion relaxation — the test demands the same
  states.
- The other browser scripts are unchanged: they share no browser-helper
  module (`mockup/scripts/lib/` holds Matrix helpers only), so per the brief
  the failure-path screenshot stays local to this driver.
- Gates: `node --check` on the script passes; the repository's Node lint is
  skipped (`node_modules` absent offline). The browser test itself runs in
  the hosted console-browser job (Chromium + native console assets), not in
  this sandbox.
- 2026-09-12: Windows intermittents made self-describing. The palpo TLS
  hostname-refusal test now accepts the documented refusal set (`Transport`
  or `Timeout`, both ADR-042 refusals with identical custody semantics; the
  fixture's 300 ms header budget decides which one surfaces on a host whose
  loopback drop is silent) while still requiring that no request reaches the
  peer; the fixture names the handshake outcome. The MCP catalog test reports
  the helper child's `try_wait` state when `tools/list` fails, so a
  `TransportClosed` can be attributed to "helper exited" or "pipe closed
  while alive". Analysis: peer `dsflash` brief 10.
- 2026-09-12: ceiling slice 3 landed red (2d7342cd): its three admission
  tests reused the fixture engagement's agent name "Worker" in the same
  project, which `admit` refuses as a name collision (`Error::Conflict`), and
  the integration chain filtered the test output through a pipeline without
  `pipefail`, so the failure did not stop the push. Each test now admits its
  own agent; the arithmetic assertions are unchanged and pass.
- 2026-09-12: the usage report's new `ceiling` headroom (ADR-123) broke the
  console usage page: the client validates the report against an exact key
  list and threw `invalid_native_response`, so the page never reached its
  ready state and both usage browser drivers timed out on the hosted run.
  The client now requires the headroom object with its exact shape (drawn
  always known; used and remaining null when unmeasured or undeclared) and
  the usage page renders it bilingually next to the summary. Verified with
  the real browser: all 13 console tests pass.
- 2026-09-12: MCP catalog failure attributed on the Windows VM: one of ten
  eight-thread runs failed `tools/list` with `TransportClosed` and the new
  diagnostic reported `helper try_wait=Ok(Some(ExitStatus(1)))`, so the
  helper child exits under load rather than closing its pipe while alive.
  The fixture now reads the exited helper's stderr at the failure point.
  The workflow gains a Windows MCP diagnosis step at eight threads (a serial
  re-run would hide a load-only failure) and keeps console failure
  screenshots as an artifact.
- 2026-09-12 — Reconciled a lost approval write-acceptance record before
  reporting it unknown (ADR-053's reconcile-before-retry rule, ADR-046
  amendment). The acceptance pump now performs exactly one bounded read of
  `approval_response_summary` when the acceptance call fails: `write_accepted`
  true continues on the successful path, a conclusive false reports
  `SettlementUnknown` with the new `AcceptanceUnrecorded` cause, and a refused
  or unanswered read stays `SettlementUnknown` with the read's own cause. The
  frame is never re-sent and the acceptance write is never re-issued. The read
  is ordered on the same single-writer FIFO queue as the write and skips an
  abandoned job whose caller stopped waiting, so a negative answer is
  conclusive; `Fault::WriteAckLost` covers the committed-but-unacknowledged
  case and `Fault::WriteAck` remains the negative control. New tests:
  `native_domain_acceptance_reply_timeout_reconciles` (store) plus
  `native_owned_approval_acceptance_reconcile_accepted` and
  `_unrecorded` (execution), each bound to a `Test:` scenario. The SQLite-backed
  selectors could not run in this sandbox (EPERM opening the private state
  directory); the orchestrator runs them.
