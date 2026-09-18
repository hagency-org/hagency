## 2026-09-12 — Read-only inspection subcommands (brief 22)

- Three `hagency` subcommands — `alerts`, `engagements`, `resources` —
  are thin clients of the operator routes the service already publishes
  (`GET /api/native/v1/{…}`), built like `console-access` (ADR-107's
  ticket pattern): loopback-only `--listen`, the private operator token
  from `--state-dir`, one bounded hyper exchange (5 s, 512 KiB cap),
  `--limit` forwarded (routes refuse out-of-range, never clamp) and
  `--json` passing the route body through verbatim. Table columns are the
  routes' own wire keys — never a derived figure. Read-only by
  construction: nothing creates, verdicts, revokes or resolves.
- The client is `src/inspect.rs`, a lib module beside `console::client`
  (there is no cli module; every `main.rs` arm calls a lib module — the
  brief's "say which" option, stated here and in the ADR).
- Refusals exit distinctly, never a silent 0: 3 unreachable, 4 refused
  (401/403 or an unreadable local credential), 5 invalid, 6
  busy/unavailable — each named on stderr.
- Tests: `native_cli_inspection_matches_operator_routes` (the passthrough
  equals the route body — byte-for-byte for resources/engagements, rows
  parsed for alerts whose `at_ms` is a read clock; table columns asserted
  verbatim) and `native_cli_inspection_exit_codes_name_refusals` (dead
  port → 3, foreign credential dir → 4, `--limit 0` → 5, a 503 responder
  → 6, each named). Spec scenarios with both `Test:` names appended beside
  `native_account_cli`'s; ADR-107 gains the inspection amendment.

## 2026-09-12 — The operator alert close path (brief 25, ADR-124 amendment)

- Ceiling alerts gain operator display-state transitions over the
  four-state subset (open, acknowledged, resolved-terminal, suppressed —
  `assigned` dropped: no agent-token authority natively), from ONE
  server-owned map (`ALERT_STATUSES` + `allowed_transitions` in
  `hagency-store`) that every consumer derives from: the store write, the
  operator route `POST /api/native/v1/alerts/{key}/transition` (the read's
  own bearer+local authority and refusal words), the console route (which
  SERVES each row's legal set as `next` through the shared `console_alert`
  builder), and the page — buttons render only from the served `next`, so
  a client map can never disagree and a refused transition is never a
  button. The retained console's own `NEXT_STATUS` drift (it offers pairs
  its store refuses) is NOT ported.
- Migration 025 (`status`/`note`/`transitioned_*`, head 24→25) with every
  schema-enumerating assertion moved in the same commit; the write is one
  `Immediate` transaction behind the single writer (`bad_transition`,
  `not_found`, bounds; `resolved_by` carries the actor like the retained
  `meta.actor || 'operator'`). Display state only — nothing enforces on it.
- Sweep interaction (brief decision, citation in code): a suppressed row
  rides occurrences and is NEVER reopened by the sweep — the retained store
  reopens only once `suppressUntil` passed (`alert-store.js:245-248`);
  native carries no window (operator release only, the named divergence).
  An acknowledged row auto-resolves on recovery exactly like an open one
  (`resolved_by='system'`, note preserved); a resolved row re-raised
  reopens as a fresh episode with display state reset.
- Client: the validator grows `status` (four-value), `next`, `note`;
  `transitionAlert`; the `Data.jsx` transition mutator; `data-transition`
  buttons with the acknowledged→resolved walk and the terminal row's empty
  set; the read-only notice retitled (display-state · never enforcement).
  The validator has NO unit test of its own (`mockup/` has none for
  `native-api.js`): it is exercised by the browser lane against the real
  server, and its refusal branches (`assigned` rejected in `status` and
  `next`, the note bound) are read-verified only.
- Oracle: `ceiling-vectors.mjs` now EXECUTES the retained
  `lib/alert-store.js` `transition()` (fake clock, sha256-pinned) over the
  five pairs both models share plus the terminal refusal — 6 fixture
  vectors — with a Rust replay test; native's extra pairs are pinned by the
  map test. Driver: the browser pass presses the REAL buttons in both lanes
  (open→acknowledged→resolved, terminal offers nothing), zh assertion
  matched to the actual dictionary string.
- Spec: five scenarios (store map table, sweep interaction, operator route
  authority, console served map, oracle replay). ADR-124 gains the
  close-path amendment.



- E1 (a sensitive bearer): the operator credential header is built exactly
  like `console::client`'s — a `HeaderValue` marked
  `set_sensitive(true)`, extracted into an `authorization()` helper so a
  unit test pins it: the flag is asserted, the rendered header never
  contains the token, and a plain header value fails both assertions.
- E2 (cell-level table tests): the resources table is compared cell by
  cell against the fetched route body (every column, every row, with the
  renderer's own null→"-" rule), the header is EXACT per column in order
  (`contains("id")` would accept `resource_id`), and the line count equals
  the route's rows plus one; the engagements prefix check became the same
  exact-header assertion with the row-count equality.
- E3 (the documented contract made true): the local `--limit 0`
  short-circuit is GONE — the CLI forwards every limit verbatim and the
  route refuses (never clamps); the test now exercises the forwarded path
  with both `0` and `101` against the live route. (A million-limit guard I
  briefly added was self-caught and removed — it was exactly the local
  bounding E3 forbids.)
- E4 (an honest missing-route exit): 404 is its own class — `Error::Missing`,
  exit **7**, "route not present" — never folded into "invalid inspection
  request"; a unit test pins Missing≠Invalid, and the refusal test adds a
  real 404 responder asserting exit 7 with the name. The review's §4.3
  suggestion is also in: no refusal path may echo the operator token
  (asserted on stderr in the invalid-class cases).



- F2 (the retained health contract): `/health` is again unauthenticated,
  **200 whenever the process is live**, readiness as body detail (names and
  state words only — the stricter native body stays); a new `GET /ready`
  beside it carries the 503-when-any-component-not-ready semantics with the
  same body, for uptime monitoring. No consumer changes: the supervisor's
  dependency wait, the CLI startup probe and the local service supervisor
  all keep their 200. Both boundaries stay diagnostic, never authority.
- F1 (a refused tick is a live loop): the sweep's ready predicate is
  liveness alone, never the tick's outcome — `refused_busy`,
  `refused_outcome_unknown`, `refused` and `swept` are ALL ready (the
  loop's own warn says it waits for the next tick); the words stay on the
  wire for diagnosis. Pinned by `native_health_readiness_refused_tick_is_ready`
  (the loop's own busy-refusal word injected on the real channel type with a
  genuinely-live task; both boundaries answer 200).
- F3 (one vocabulary): a `pub enum ComponentState` (+ `TickOutcome`) from
  which BOTH the wire words and the ready predicate derive; the wiring-bug
  arm now reports `not_started` (not ready). Pinned by
  `native_health_readiness_enumerates_every_state` (every variant's word
  and ready answer).
- The executable browser lane now seeds and passes the SAME measured pool
  as the plain lane (`private_usage_pool`, idempotent id from the preset
  hash), so both lanes prove the same headroom cells — fixing the
  orchestrator's real-browser failure
  (`browser::native_console_resources_executable`:
  `selectOption: options[0]: expected object, got undefined`).
- Spec: five scenarios (the three rewritten for the split + the two new
  tests). ADR-096: the brief-21 amendment, superseding the brief-19
  503-on-`/health` statement twice over.

## 2026-09-12 — Headroom review edits E1–E4 (review of 6eae4834)

- E1 (two predicates, one invariant): the shared-seat console test pins that
  `draw.committed` (bare `resource_id=?`) and `pool.committed`
  (`preset_id=?` grouped) agree across a second preset sharing the pool's
  seat (both 100) while `seat.committed` is deliberately the larger
  cross-preset figure (350) — equal by the injectivity of
  `public_resource_id` (`project.rs:67-69`), stated in ADR-121, never by
  accident.
- E2 (a name that lies): the budget route's clock fault now refuses as
  `console_unavailable` (503 via `Error::Unavailable`) instead of `busy` —
  `Busy` names worker saturation and invites a retry; a failed clock is the
  console's own unavailability and is not in the client's retryable set.
- E3 (always-zero key off the wire): the console budget wire no longer
  emits the top-level `reserved` (a constant 0 — `budget()` runs without an
  exclusion and the core assigns `reserved` only inside that arm,
  `allocation.rs:155-159`); `validateBudget`'s exact key list drops it in
  the same commit; the page's headroom panel drops its duplicate committed
  cell (the budget panel's `pool.committed` carries the equal figure), and
  the `nr.headroom.committed` key left both dictionaries (parity 11/11).
- E4 (two shapes, two consumers, stated): ADR-121 records that the operator
  route `/api/native/v1/resources/{id}/budget` keeps its five-key
  commitments shape while the console route carries `draw` — an
  asymmetry made a decision, not drift.
- Spec: the observations scenario now names the shared-seat fixture, the
  omitted reserved key, and the clock refusal.

## 2026-09-12 — Health/readiness by component (brief 19, ADR-096 amendment)

- `/health` (unauthenticated, the retained route's boundary) now answers a
  readiness rollup instead of the foundation-era constant: components
  `domain_writer`, `custody_store` (both `writer_open()` synchronous probes
  — no writer job, no lock), `ceiling_sweep` (liveness via the SHARED task
  handle, started in `Bootstrap::serve` before the router snapshot so the
  served app carries it from the first request) and
  `ceiling_sweep_last_tick` (reported, never readiness-failing alone), plus
  the optional `development_driver`/`palpo_transport` owners via
  `StatusHandle::state()` state words. 200 when every state is a ready word;
  503 with the full component list otherwise — never a silent 200. `stage`
  is gone. States are words, never counts: stricter than the retained route,
  which publishes agent/server counts (`backend-v2.js:7585-7618`).
- Readiness is diagnostic, never authority: nothing may read it to retry,
  release or complete anything (stated in the probes' doc comments, the
  handler and the ADR amendment).
- Tests `native_health_readiness_ready`, `_names_stopped_sweep`,
  `_names_closed_domain_writer` wire the app exactly as `Bootstrap::serve`
  does (writers + shared sweep handle + tick channel); the stopped case
  cancels the loop and proves the healthy components stay named, the closed
  case shuts the domain writer with the sweep `disabled` (a ready word) so
  the 503 is honest about exactly one component. Spec scenarios appended to
  the foundation spec beside `bounded_work_keeps_health_responsive`.

## 2026-09-12 — Resources headroom section (brief 18, ADR-121 amendment)

- The budget route now answers ONE combined read: new store method
  `resource_headroom(id, at)` returns the commitments budget AND the
  ceiling draw together in a single writer job (a page must never render
  figures from two different reads); the console route serializes the budget
  plus a `draw` object — committed, measured/consumed (null when unmeasured,
  never zero), drawn (`max(reserved, spent)`, the same figures the alarm and
  admission publish), period, ceilingTokens, remainingBeforeCeiling (null
  when no ceiling), and `binding` named the way ADR-122's refusal names it
  (`engagement-store.js:82-83`: measured > committed ? "measured spend" :
  "committed allocations"; null when unknown because nothing competes).
  Statement time is the clock; the no-query rule is unchanged.
- `validateBudget` grew the object in the same commit (exact eight keys,
  both directions, unknown arms null) — import-verified: both binding words
  accepted, all-null arms accepted, missing/extra key and wrong binding
  rejected.
- The page: `HeadroomPart` inside the budget panel renders eight
  `data-headroom` cells, unknowns as the explicit unknown word; twelve new
  `nr.headroom.*` i18n keys in both dictionaries (parity 12/12).
- Tests: `native_console_resource_observations` extended with both arms —
  the unmeasured resource (every unknown null, never zero, binding null) and
  the fixture's measured usage pool (drawn == max(committed, measured), the
  refusal's binding wording, remaining = ceiling − drawn). The resources
  browser passes the measured pool's id and the driver asserts the
  measured-vs-unknown cells render (figures for the measured pool; the
  unknown word, never zero, for the unmeasured one).
- Spec scenarios updated (observations + browser); ADR-121 gains the
  console-consumer amendment — the "publish headroom" follow-through its
  closing paragraph deferred to exactly this slice.

## 2026-09-12 — Settlement precedence: a verdict outranks the completion path (ADR-060 amendment)

`hagency-execution`'s completion block in `operation.rs` ran for every drive
result except `Err(Cancelled | Deadline | UnsupportedApproval)` — a set that
included `Err(SettlementUnknown)`. With a held completion reference it first
asserted `canonical_status = Some(TaskState::Done)` and then either returned
`Err(Failure::CleanupUnknown)` on the macOS retained-owner case (replacing the
settlement verdict) or published the completion and returned `Ok(())` on Linux
(swallowing `SettlementUnknown` into a published reply and an
operator-invisible success). Both inverted ADR-060's "a negative/unknown
cleanup path never publishes" and ADR-053's "canonical status is observed,
never promoted to Done".

The guard is now `drive.is_ok()`: completion custody is consulted only when
the drive actually succeeded, and every other result is surfaced unchanged by
`drive?`. `canonical_status = Some(TaskState::Done)` moved **after** the
cleanup gate, and the macOS retained-owner bail records
`report.settlement = Settlement::Unknown` **beside** the verdict (with
`Report.cleanup` and `settlement_cause`) rather than instead of it. The rule is
independent of the candidate in-flight/quiet-path lineage and lands on
upstream `feat/rust-migration`.

Paired change: `native_owned_approval_acceptance_reconcile_unrecorded` no
longer races the host with a 600 ms sleep. The test now holds the
`BEGIN IMMEDIATE` write lock until the operation has produced its verdict,
bounded by a 6 s `tokio::time::timeout` so a premise that no longer holds
fails as a timeout instead of passing by waiting longer; in WAL mode the
ordered reconcile read answers while the lock is still held, making the
refused acceptance and the conclusive negative read deterministic for either
ordering of the probe's `turn/completed`. The two existing scenarios now also
pin the rule directly: `..._reconcile_unrecorded` asserts the failed drive
neither asserts an unobserved Done nor converts the verdict into
`CanonicalReplyReady`, and `..._reconcile_accepted` (the negative control) pins
platform-aware outcomes — `CanonicalReplyReady` on Linux, `Unknown` beside
`CleanupUnknown` with no asserted Done on macOS.

## 2026-09-12 — Engagements review edits E3/E4 (review of 0077cb08)

- E3 (pagination untested): the console fixture now seeds THREE engagements
  (UsageWorker approved+effective, AlertWorker on its own pool carrying the
  E4 astral name, PageWorker admitted with a distinct agent name), so
  `?limit=1` yields two content pages plus an empty third. The test asserts
  page one's single row, a non-null opaque `next_after` equal to the
  last-served id (the cursor is BOUND to the store's server-assigned
  engagement id, compared lexically in `WHERE id>?1 ORDER BY id`; it carries
  NO authority — no session, no resource, and every page re-authorizes
  through the console session), then walks to exhaustion asserting each
  content page serves a NEW engagement and the final page is empty with a
  null cursor. The browser pass pages IN-PAGE (Next until disabled, then
  First page) — each page reaching the ready state proves
  `validateEngagements` accepted it.
- E4 (bound unit mismatch): the client now counts projectName in Unicode
  SCALAR VALUES (`[...s].length <= 255`), aligned with the server's
  verifier bound `trim().chars().take(255)` (authority.rs:286, Unicode
  scalars). Chosen over UTF-16 alignment because the scalar bound is
  enforced at verification — upstream of the wire — so changing it would
  alter what requests verify, not just the read; and the retained JS bound
  (`backend-v2.js:8061` `slice(0,255)`, which counts UTF-16 units) is an
  implementation accident of `slice`, not a designed rule. The fixture
  carries the discriminating name: 260 astral characters → 255 scalars = 510
  UTF-16 units, exceeding the old client bound but never the server's. The
  node unit verified the validator accepts it (and rejects 256); the Rust
  test asserts the wire carries exactly 255 chars / 510 UTF-16 units.

## 2026-09-12 — Console engagements page, read-only (ADR-107 amendment)

- Loader + router first (the alerts-slice lesson, in the same commit as the
  page): `assets.rs` mime allowlist entry for `engagements/index.html` plus
  its `/console/engagements/` key mapping; `console.rs` document set gains
  the engagements document with a NO-QUERY rule — a paginated triage list
  whose selection happens in-page, unlike usage's per-entity
  `engagement_id` selection. Note: the alerts entries in these two files are
  absent on this branch — the orchestrator wired them integration-side; this
  commit adds only the engagements entries.
- Server: the read already existed (`GET /console/api/engagements`,
  `console/usage.rs`, ADR-107's selector); the wire grew one key —
  `requestedTokens` (raw number, never compacted) — on the server `Label`
  and in `validateEngagements`' seven-key exact list in the SAME commit (the
  binding exact-key lesson; import-verified both directions).
- Client: `NativeEngagements.jsx` rides the existing `fetchNative` load (no
  new Data.jsx branch): state strip (six native states), state/agent
  filters, the tokens column, `next_after` pagination, read-only note, no
  mutating buttons (create/verdict/revoke stay with the project side — the
  retained page's route reasons have no native counterpart). Page branches
  on `NATIVE_MODE`; rail enables `engagements`; build script stages the page
  at all four sites; three i18n keys in both dictionaries (parity 3/3).
- Tests: `tests/console/engagements.rs` — `native_console_engagements_read`
  (authority matrix incl. `sec-fetch-site: none` and `forwarded`, query
  refusals at 0/17/bad/encoded/repeated/foreign, the exact seven-key wire
  shape, pagination) and `native_console_engagements_document` (document
  served from the staged fixture, no-query rule, origin rule intact on
  non-documents). The fixture's `assets()` now stages the engagements
  document so the loader allowlist is exercised. Browser: an engagements
  pass in `native-console-browser.mjs` (ready state, the seeded engagement
  row, the read-only note, no mutating buttons). Spec scenarios with both
  `Test:` names; ADR-107 gains the engagements-consumer amendment.

## 2026-09-12 — Console alerts review edits E1–E4 (review of 63ef17cf)

- E1 (the write/read contradiction): the store read now PARSES `detail` when
  it can and falls back to the raw string (`read_detail`,
  `Value::String(raw)`) — never `Error::Schema`. The retained
  `truncatePayload` slices the JSON STRING (`alert-store.js:61-64`), so an
  over-long row legitimately holds invalid JSON and the retained consumer
  passes it through as text (`mapAlert`, `mockup/lib/api.js:203`); the old
  unconditional parse made one truncated row 503 the whole read and left the
  client's string arm dead. Pinned end to end by
  `native_console_alerts_publish_truncated_detail` (store read AND console
  route, wire string verbatim, derived pair intact); the write side stays
  pinned by the `detail_json` unit. The read-side fallback was the
  reviewer's preferred option (a); option (b) would have reverted the
  truncation.
- E2 (one alert type, known cliff): `validateAlerts`'s comment and ADR-124
  now record WHY every row is `warning`/`open` (the route derives both; the
  migration has no severity/status column; exactly one alert type exists),
  and that the hard equality checks make a future non-warning row REFUSE the
  whole read rather than silently relabel it — a second alert type needs a
  migration + route + validator change in the same slice.
- E3 (test strength): `native_console_alerts_read` now pins the derived pair
  on the wire in every accepted read (so a route emitting `info` fails there,
  not only in the publication test) and covers the two missing authority
  arms — `sec-fetch-site: none` and a `forwarded` header — alongside the
  existing matrix.
- E4 (naming): the four "brief 13" labels replaced with durable references
  (ADR + progress headings cite the console-consumer slice by commit
  `63ef17cf` and the brief-12 port plan; the browser-script and fixture
  comments likewise). The reviewer's suggested number (15) contradicts the
  orchestrator's own brief numbering (the console slice IS brief 13's
  implementable; this review brief is 14), so the ordinal was dropped
  entirely rather than renumbered — noted in the report.

## 2026-09-12 — Console alerts read (the console consumer slice, 63ef17cf; ADR-124 amendment)

- Server: `hagency/src/console/alerts.rs` — `GET /console/api/alerts?limit=`
  mirroring `console/usage.rs` exactly (recheck after the store answers, the
  console `failed()` mapping, `Busy → 503 "busy"`, statement-time `at_ms`,
  no-store from the boundary), mounted beside `usage::router()`. Wire item is
  the store's `CeilingAlert` plus DERIVED `severity: "warning"` and
  `status: "open"` (never stored; the read is open-rows-only by
  construction). Limit defaulted 100, refused outside 1..=200.
- Client: `mockup/lib/native-api.js` gains `validateAlerts` (exact-key, both
  directions — thirteen keys per alert; `detail` object-or-truncated-string,
  the retained truncatePayload rule) + `fetchAlerts` + `alertsView`;
  `components/Data.jsx` routes the alerts view through its native load;
  `components/NativeAlerts.jsx` renders one open-count strip, agent filter,
  the table + detail panel with the four actionable fields — NO transition
  buttons (no operator close path exists; controls that would 404 lie),
  delete stays refused; `app/alerts/page.jsx` branches on `NATIVE_MODE`;
  rail enables alerts; `build-native-console.mjs` stages the page (3 sites);
  five new i18n keys in both dictionaries (parity verified 29/29 `al.` keys).
  Polling rides `Data.jsx`'s existing 15 s refresh; no new timer; no SSE.
- Console fixture seeds one overrun through the store (commit 100 under a
  generous ceiling on its OWN pool/seat, ceiling lowered to 50, swept at
  2000) so every consumer — HTTP tests, the browser pass, the executable
  lane — sees one open alert; the usage pool is untouched so
  `new_engagement` still admits.
- Tests: `tests/console/alerts.rs` — `native_console_alerts_read` (authority
  matrix as the usage console test, limit refusal at 0/201/bad/repeated,
  default + cap boundary publish) and
  `native_console_alerts_fixture_publishes_open_alerts` (every wire field,
  derived fields, parsed detail figures, resolved absent after restore).
  Browser: the native-console pass asserts the alerts page renders the
  seeded summary, runbook, read-only note, refused delete, and reaches
  `data-native-state="ready"` in both lanes.
- Spec scenarios with both `Test:` names appended; ADR-124 gains the console
  consumer amendment (derived fields, no actions, validator contract,
  15 s cadence, SSE not in slice).

## 2026-09-12 — Slice (b) review edits E1–E8 (publication review of 51694e20)

- E1: `Busy → 503 "busy"` confirmed in place (route + `native-api.js:56`
  client map agree; the test never asserted 429). The ADR's stale
  "intentional divergence: 429" sentence is deleted; the amendment now notes
  the client mapping as the reason a 429 would have broken the mockup.
- E2: kept 200/refuse as a DOCUMENTED deliberate divergence — the retained
  cap is 500 at `alert-store.js:410` and it CLAMPS
  (`Math.min(parseInt(limit) || 100, 500)`); the invented "retained
  pagination cap" citation is gone from the constant doc, the read doc, the
  route comment and the ADR, each now stating clamp-vs-refuse and why
  (consistency with every other bounded read; a clamped limit hides client
  bugs; 200 tighter than 500 for bounded cost).
- E3: newest-first is now real — t0 sweep opens both rows (the tie the
  review flagged), t1 resolves pool_b only, t2 reopens it, then the read
  asserts pool_b first with `last_seen_ms == t2` and pool_a at t0; `?limit=1`
  returns pool_b exactly.
- E4: the busy-survival test now refuses with the store's OWN `Error::Busy` —
  capacity-1 `DomainStore` plus a held SQLite write lock plus two queued
  `put_resource` jobs saturate the mpsc so the tick observes
  `Refused("busy")` exactly (bounded 3 s retry window ×5, exact assertion so
  a catch-all `failed` would time out and fail); `OutcomeUnknown` needs a
  parked-writer seam that is test-private to `hagency-store`, recorded in
  the ADR as the store-side suite's arm.
- E5: grep confirms no `u64::MAX` ceiling remains in
  `native/hagency/tests/alerts*` (the fixture has used the JSON-safe
  `GENEROUS` since slice (b)).
- E6: future consumers are now the real retained pair — the SSE echo
  (`backend-v2.js:1866`) and the console page; "Matrix delivery" dropped (no
  retained counterpart).
- E7/E8: ADR gains "Abort-vs-commit on shutdown" (skipped-or-committed,
  never torn; a dequeued job commits whole, so abort means no NEW effect
  beyond the tick's atomic unit) and the loop's doc comment states it; the
  period stays const+builder, documented as the service's existing pattern
  for background-owner tuning (no config value reaches `Bootstrap` today);
  the immediate-first-tick behaviour is stated.
- Store tests untouched per the brief; the orchestrator's three fixes there
  (name collision, Reserved-state `register_session`, scoped-request
  verifier) are noted for the `native/hagency/tests/alerts/` fixture to
  follow — its one engagement per pool is admitted, approved and effective
  through `admit`/`approve` only, so none of the three failure modes apply.

## 2026-09-12 — Alarm review edits B1–B7 + slice (b) Busy correction

- B1: the remaining eleven `user_version == 23` head assertions (approvals,
  conversations, file_delivery, file_uploads, owned_completion,
  received_files, replies, verified_ingress/attachments,
  verified_ingress/notice_custody, workflows/mod, workflows/custody) moved to
  24 — the schema-head bump is now complete; post-edit grep finds no other.
- B2: the sweep doc comment and ADR now state the honest fact — the sweep
  reads the read-side projection `usage::ceiling_report` (ADR-121), the SAME
  drawn rule admission enforces on but a distinct code path from
  `budget()`/`resource_budget`, exactly the retained split (Node's sweep reads
  `ceilingSpendFor`, admission reads `remainingFor`); agreement is by shared
  rule and oracle, not by construction.
- B3: reopen/dedupe UPDATEs no longer write `runbook` (only a fresh insert
  writes the four text fields, matching `alert-store.js:231-271`); the dedupe
  test plants a sentinel runbook across two more sweeps and asserts it
  survives.
- B4: `detail_json` truncates at 4096 the way the retained `truncatePayload`
  does (encode then slice, `alert-store.js:61-64`) instead of aborting with
  `Error::Capacity`; the column CHECK stays as the last line of defence, and a
  unit pins the truncation (valid sweep inputs cannot reach the bound, so it
  is defence-in-depth at the owning function).
- B5: `native_ceiling_alert_prunes_resolved_rows_after_seven_days` — old
  resolved row pruned (`pruned == 1`), young resolved kept, open never
  pruned, all through the fixture's own store wrapper.
- §3.4/B7: `native_ceiling_alert_lost_ceiling_keeps_alert_open` pins the
  parity — a ceiling-less resource is skipped for raise AND resolve, the alert
  stays open exactly as Node's `continue` leaves it (`backend-v2.js:9397-9400`)
  — and ADR-124 records the no-operator-close-path asymmetry.
- B6: the oracle's sweep vectors are now computed by EXECUTING the retained
  `createAlertStore` (fake clock, in-memory save; ingest `created` flag and
  `autoResolve` returns derive the counters) rather than by a local mirror;
  regenerated fixture is byte-identical in expectations, `--check` green ×3;
  ADR notes the execution and the one non-encoded transition (reopen window).
- B7: ADR must-never now names completion (owned dispatch, canonical Done,
  final reply custody) beside admission/leases/retries.
- Slice (b) correction: the alerts route maps `Busy → 503 "busy"` exactly as
  `usage.rs` does (the brief's 429 was the orchestrator's error); recorded in
  ADR-124's amendment section.
- Store-test CI defect fixed in-passing: the nine `u64::MAX` fixture ceilings
  in `tests/ceiling_alerts.rs` (which `Tokens` rejects above JSON_SAFE_MAX)
  replaced with the JSON-safe `GENEROUS` constant — same class as B1, would
  have failed every alarm test in CI once the EPERM lifted.

## 2026-09-12 — Ceiling alarm slice (b): publish open alerts, sweep hourly (ADR-124 amendment)

- Store read `DomainRepository::open_ceiling_alerts(limit)` (and the
  `DomainStore` wrapper, exports `CeilingAlert`/`MAX_OPEN_CEILING_ALERTS`):
  open rows only, `last_seen_ms` desc, limit defaulted 100 by the route and
  refused at 0/>200 (`Error::Invalid`, the retained listAlerts pagination
  cap); `detail` is PARSED on read — a corrupt row is `Error::Schema`, never
  a silent null. Every retained field is carried.
- Route `GET /api/native/v1/alerts?limit=` (`hagency/src/alerts.rs`, mounted
  in the `api/native/v1` chain): exactly the `usage.rs` operator authority
  (bearer + local authority, same refusal mapping) with one recorded
  divergence — `Busy` → `429` per the brief's explicit instruction where
  `usage.rs` uses `503`. Response `{"at_ms":…,"alerts":[…]}`, snake_case,
  no-store; publishes what the sweep wrote, never re-derives the draw.
- Trigger: `start_ceiling_sweep(domain, shutdown, period)` in `bootstrap.rs`,
  spawned from `Bootstrap::serve` beside the other background owners and
  stopped in `close`. Production period `CEILING_SWEEP_PERIOD` = 3600 s (the
  retained hourly cadence and rationale); tests override via
  `with_ceiling_sweep_period` or drive the starter directly. On
  `Busy`/`OutcomeUnknown`/failed acquisition the tick logs the refusal code
  with the `[ceiling]` prefix and waits for the next tick — no in-line
  retry, never blocking admission traffic. Observation hook:
  `tokio::sync::watch` of `CeilingSweepTick` (`Swept`/`Refused`); no
  sleep-based polling in tests. The `Options` field was deliberately avoided
  (a builder instead) so no existing `Options` literal outside this slice
  needed touching.
- Tests (`native/hagency/tests/alerts.rs`): the authority matrix applied to
  the new route, the publication read (every retained field, parsed detail,
  resolved absent, limit respected), and the sweep loop (short period, tick
  observed on the watch, writer held busy via a second connection's
  `BEGIN IMMEDIATE` → refusal survived, recovery sweep after release).
  Known latent defect flagged for the orchestrator, outside this brief's
  ownership: brief 8's `tests/ceiling_alerts.rs` uses nine `u64::MAX`
  ceilings, which `Tokens` refuses above JSON_SAFE_MAX — masked in the peer
  sandbox by the EPERM, it will fail in CI unless lowered.

## 2026-09-12 — Ceiling alarm slice (a): file and resolve overrun alerts (ADR-124)

- Migration `024-ceiling-alerts.sql`: one row per dedupe key
  `agent_ceiling_overrun:<resource_id>` with the retained wording columns,
  `detail` JSON-string ≤4096, `occurrences`, resolved-by; registered with its
  `LIMIT 0` verification query.
- `DomainRepository::sweep_ceiling_overruns(now)`: one `Immediate`
  transaction over every resource with a declared finite ceiling, reading the
  same `ceiling_report` admission uses. Strictly `drawn > ceiling` raises;
  `<=` auto-resolves (`resolved_by='system'`); repeats increment
  `occurrences`; a re-over after resolution reopens the same row (the
  one-row-per-key divergence from Node's episode model, argued in the ADR);
  resolved rows pruned after 7 days. `SweepOutcome` counts returned; async
  `DomainStore` wrapper added (tests drive it directly — no timer in this
  slice). An alert is diagnostic, never enforcement.
- Oracle: `ceiling-vectors.mjs` gained the alert state machine (ingest-dedupe,
  auto-resolve, reopen) with `alertStoreSha256` pinned; six seeds including
  the 2-phase month-rollover unknown-not-zero case; fixture regenerated,
  `--check` green ×3.
- Tests (`tests/ceiling_alerts.rs`, the seven named scenarios): all compile;
  the SQLite-backed ones hit the known sandbox EPERM at fixture open and are
  CI's to run.

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
- 2026-09-12: three ceiling-alert store tests, never executable in the
  peer sandbox, failed at fixture setup on first execution: the seeded
  engagement was only Reserved (session registration is refused until the
  approval effect is applied, as the usage fixture does), a second
  engagement reused the live agent name (admission refuses the collision),
  and the oracle replay asked for zero tokens where the retained seed
  commits nothing (a scoped request never asks for zero). All seven pass.

- 2026-09-12 — Named each `hagency mcp` helper refusal with its own exit code
  (ADR-051 amendment). A hosted load failure was reported as `Error: Protocol`
  with exit 1, which eleven return sites produced indistinguishably. `Framing`
  (exit 70) now covers the bounded stdio refusals — EOF with a partial frame,
  frame over `FRAME_LIMIT`, response over `OUTPUT_LIMIT` — and reports the bound
  and observed size; `Protocol` (exit 71) keeps the lifecycle/schema refusals,
  each carrying a `&'static str` tag naming its check; `Io`/`Context` are 72/73
  and the stdio watchdog's 74 is unchanged. `main`'s `Mcp` arm now exits with
  `Error::exit_code()` instead of Rust's default handler. New tests
  `native_mcp_helper_framing_is_named` and `native_mcp_helper_protocol_is_named`
  drive the real helper on its raw stdio (the fixture's new `raw_helper`, since
  `Client::new` hands stdin to `serve`), and `exit_evidence` now prints what the
  exit code means so a hosted failure is readable without stderr. No deadline
  and no limit value changes.

- 2026-09-12 — Corrected the MCP helper attribution after review (ADR-051). The
  amendment's claim that the old `?` path "printed the same message" was false:
  Rust's `Termination` prints the `Debug` form, so the old path emitted exactly
  `Error: Protocol` — the bare variant — which is the captured hosted evidence,
  and why its stderr named no check. The ADR now states the real counts (21
  `Error::Protocol` constructions at HEAD across `mcp.rs`/`json.rs`/`stdio.rs`,
  15 on the pre-`tools/list` path, five reclassified to `Framing`), records the
  `Framing`/`Protocol` boundary judgement, names the `101 Panic` exit in
  `exit_code_name`, and records one residual: a **failed** `output.flush()` on
  the new exit path skips the second flush the old drop would have attempted, in
  the same broken-pipe case only. The commit message for `01545c8` keeps the old
  wording; this entry and the ADR are the correction. The exit codes are
  documented as diagnostic only, carrying no authority.
- 2026-09-13: the console alerts slice staged `alerts/index.html` into the
  built assets but the Rust loader allowlists page documents by path and
  keys them to console routes, so every console fixture refused the built
  set (`Error::Assets`) and all six browser tests failed locally. The
  loader and the document router now know the alerts document (no
  selection query). All 16 console tests pass with the real browser.
- 2026-09-13: hosted Ubuntu failed `native_approval_consumption_clock_after_lock`
  ("operation must enter before expiry"): the `after_lock` fixture sleeps
  until 60 ms before a 1 s deadline, takes the writer lock and asserts the
  clock has not expired, a window a loaded runner can miss; the same
  scheduling-window class was recorded earlier for Windows. The fixture
  now reports an unmodelled window (lock taken too late, or held into the
  operation's unchanged 100 ms busy budget) and each scenario rebuilds
  itself a bounded number of times instead of judging it. No product bound
  changed.
- 2026-09-13: the precedence rule gated the completion path on a successful
  drive and moved the Done behind the cleanup gate, so the retained
  helper-finish flows (no terminal Codex turn; completion through the held
  row) lost their Done on every host. Now only the settlement verdict skips
  the path (with cancel, deadline and unsupported approval as before), and a
  held completion reference, the store's committed finish read back through
  custody, sets Done at block entry as before. Execution-crate gates now
  include the hagency workflow targets.
- 2026-09-13: after the landing review of the harness commits and the
  completion-path fix: the reconcile-accepted fixture's comment now says what
  the two platform branches actually differ in (which cleanup gate refuses, not
  custody), and ADR-060 states the invariant the exclusion list relies on: every
  drive outcome outside the four verdict classes consults custody, and
  publication still requires the store's own held completion row.
- 2026-09-13: integrating the alert close path (operator transitions on ceiling
  alerts, migration 025) surfaced two defects its author's sandbox could not
  run: every recovery fixture that rewinds below 25 replayed the migration's
  ADD COLUMN over an already-upgraded table (fixed by rebuilding the table in
  the innermost shared removal helper and in the four direct-drop fixtures,
  the store's existing convention for ADD COLUMN migrations), and the sweep
  test drove `suppressed` straight to `acknowledged`, which the map refuses;
  the operator reopens first.
- 2026-09-13: the real-browser pass for the alert transitions asserted the
  served map right after the click while the page was still in its ready
  state, and expected a terminal notice for a resolved row that the open-alerts
  read no longer lists. The driver now waits for the pressed control to leave
  the DOM (bounded) and asserts the no-open-alerts state after resolve.
- 2026-09-13: operator decision: Windows is not a release target for now (no
  coding agent runs on Windows). The hosted windows-2025 lane keeps running for
  its diagnostics but no longer blocks the run; the approval product lineage
  lands on the Ubuntu and macOS verdicts, and the Windows VM work is paused.

## 2026-09-12 — The console alert transition is scoped and session-authored (brief 28)

- F1: `POST /console/api/alerts/{key}/transition` now requires the
  CONFIGURE scope (`Authority::can_configure` — the same finite-ticket
  scope the resource configuration writes require), refused with the
  console's existing missing-scope word
  (`resource_configuration_scope_required`) BEFORE any body parse or
  store read. The `sec-fetch-site`/`origin` mutation guards were already
  inherited: the route's `authenticate` hoop calls `current`, which
  enforces `same_origin(req, depot, method != GET)` (console.rs) — no new
  guard needed.
- F3: the actor is the SESSION's identity, fixed by the server
  (`SESSION_ACTOR = "console"` — the session carries no display name, so
  provenance names the boundary). The body's `actor` field is removed;
  the exact-key parse (`deny_unknown_fields`) refuses a body carrying one
  as `invalid_console_request`. The operator bearer route keeps its
  bounded optional `actor` (ADR-124 amendment, named in the ADR note).
- F2/F4: all three "thirteen keys" claims corrected to fifteen
  (`ConsoleAlert` struct comment, `native-api.js` `ALERT_KEYS` comment,
  ADR-124 validator contract); the NativeAlerts header comment and the
  ADR "no actions" prose corrected to describe the served-map buttons.
- The list read now serves `permissions.configureResource` (the same key
  the resources read serves); the page hides the triage buttons without
  it and shows a notice naming the configuration management link — the
  same shape as the resources page's missing-`can_configure` notice.
  Tests: `native_console_alert_transition_requires_scope` (read-only
  refused with the named word, store row and provenance asserted
  unchanged; scoped session succeeds) and
  `native_console_alert_transition_actor_is_the_session` (actor body
  refused by the exact-key rule; `transitioned_by` asserted the session
  identity); spec scenarios bound in the alarm spec; the browser lane
  gains the read-only arm (buttons absent, notice rendered) and mints a
  scoped link for the unchanged walk.
- 2026-09-13: the console transition authority fix carried a browser-lane
  change its author's sandbox could not run: the harness minted the scoped
  ticket before the browser exchanged the read-only one, and the authority
  keeps one outstanding ticket, so the read-only session never existed. The
  in-process lane now mints the scoped link only when the driver asks for it
  after the read-only walk; the executable lane has no scoped walk; the
  post-exchange wait targets the resources page, which carries no ready marker.
- 2026-09-14: the cutover-blocking provisioning gap is closed end to end and
  landed. A request in the pre-project reception room now reaches
  `DomainRepository::admit` through the production intake — the discriminator
  is matched during batch derivation, before target resolution, so the request
  is classified as a provisioning candidate instead of being dropped as
  not-for-us — and the representative's verdict then makes the engagement
  effective and routable: the verdict is admitted, the provision effect is
  claimed and observed complete in the same handoff, and the provisioned
  engagement's Matrix route is bound. Before this, every agent in every native
  test had been admitted by a test helper, so a native deployment could not
  provision an agent through its own ingress at all.
- 2026-09-14: that gap turned out to be one of nine. A production-wiring audit
  of the whole tree — strip `#[cfg(test)]` items and `#[test]` fns, exclude
  test, fixture, example and probe trees, then walk the store's own call graph
  from each production root — found nine flows whose store writes were reached
  only by tests. Seven now have an evidenced production caller in the tree; five
  of those were recorded closed in the classification table earlier, and the
  provisioning pair's rows landed at 83e49557. One
  flow was superseded, and one remains: operator recovery and resume, which has
  a decision record but no code yet. The audit's method is now enforced rather
  than remembered: a spec `Production caller:` line must resolve in the
  production call graph, and `native/scripts/check-production-callers.mjs`
  fails the build on a caller that is absent, ambiguous or unresolved, with a
  tracked gap id required for anything still owed.
- 2026-09-14: the checker earned its keep by being given real input. Run
  against the tree it was written for, it reported the audit's own defining gap
  as wired, stripped a production bootstrap file as a probe, missed router
  handler edges, could not type a receiver, and did not strip comments. Each
  was a real defect, each was fixed, and the fixture suite grew to cover them.
  A checker that only ever sees clean input cannot be trusted to refuse dirty
  input.
- 2026-09-14: four intermittent tests were fixed by handshake rather than by
  widening anything — a media deadline and two no-resend scenarios, all three
  landed before this entry, and the turn-end untransmitted arm, whose fix landed
  at 83e49557 alongside the table rows closing G2 and G3 — plus the CLI console
  readiness poll, which
  treated a reset connection during startup as a failure instead of retrying
  until its own deadline. The rule applied throughout: every wait is derived
  from the client's own limits rather than written as a literal, and an
  ordering is proven by a marker, a lock or an observed diagnostics mark, never
  by a sleep. No deadline, budget or gate was widened to make a test pass.
- 2026-09-14: two corrections to earlier claims in this log, both recorded
  rather than quietly fixed. The recovery gap was first written down as landed
  and then softened to an artifact trail; neither was right, and the real gap
  is operator recovery and resume — lease, quarantine, dirty flag, supersession,
  replacement and the recovery record. Separately, a claim that four production
  call sites existed for the shutdown observation was wrong: all four sit
  inside a test module, and the read is a snapshot that writes nothing.
- 2026-09-14 evening: the provisioning pair is recorded closed in the wiring
  audit's table, with the callers named: the representative's verdict is
  admitted, the provision effect is claimed and observed complete in the same
  handoff, and the provisioned engagement's Matrix route is bound, all four
  inside the production intake's approval path.
- 2026-09-14 evening: the console can show an agent that the port provisioned
  itself. A fake peer delivers the engagement request and the approval into the
  reception room, the production intake provisions the agent, and the console
  roster read serves it. The test asserts store rows rather than the roster
  alone — the minted engagement id read back by request id and never
  re-derived, the provision effect observed complete, exactly one session route
  row — so a roster item cannot satisfy it on its own. This is the first
  end-to-end evidence that the native port can take an agent from request to
  operator-visible, and it is the proof the ingress work existed to make
  possible.
- 2026-09-14 evening: a completeness sweep of the wiring audit found seven more
  store writes it had missed or misclassified, and named the mechanism of the
  miss: each grep hit was classified by the FILE it landed in rather than by
  the ITEM enclosing it, so calls inside test modules whose ranges open far
  above them read as production callers. One write was absent from the table
  entirely. The audit's method now requires classifying by the enclosing
  item's range, resolving path and module includes, and excluding a facade's
  own body, which is the only caller of most store writes and is never
  evidence of production reachability.
- 2026-09-14 evening: two specification corrections, both in the direction of
  claiming less. A scenario's Then demanded an outcome its own test asserts the
  opposite of, and now states what the test can actually refute. And a
  tolerated failure variant that the product cannot produce in that scenario
  was removed from the accepted set: the arm requires an entry that is not in
  flight, while the scenario's entry is in flight by its own Given. A tolerated
  outcome that can never occur is indistinguishable from a live one and would
  have swallowed any regression that began producing it.
- 2026-09-14 evening: two cutover-blocking gaps are now documented with specs
  rather than discovered later. An operator can neither refuse a pending
  engagement request nor retire an active one: the two terminal states are
  written in a single store function that nothing in production reaches. And
  the approval delivery path cancels a send it should have completed, because
  its deadline is anchored on the owner's decision window and the job cancels
  the token it handed to its own send, so a budget overrun is reported as a
  cancellation rather than a timeout. The retained product performs all three
  acts through operator routes and retries cleanup only when an operator asks,
  so an automatic retry here would be new behaviour rather than parity. Spec
  slices for refusal, retirement with cleanup retry, and project-side
  registration are written with every selector owed; the delivery defect and
  two representation divergences have decision records in progress.
- 2026-09-15: an operator can recover an orphaned dispatch. This was the last
  wiring gap standing between the port and cutover: the store has always known
  how to release the lease, clear the quarantine and the dirty flag, supersede
  the attempt, enqueue its replacement and record what it did, but nothing in
  production could ask it to, so a dispatch whose agent crashed stayed
  quarantined for ever. A console route now reaches that path, behind the same
  agent-lifecycle authority the roster and the start, stop and preset routes
  already use. No scope was invented for it.
  Nothing automatic reaches it, and that is deliberate rather than incidental:
  an attempt whose outcome is unknown grants no authority to retry, reply, take
  a lease or declare completion, so recovery is an operator's act or it is
  nothing. The review searched for a sweep, a timer and a spawn rather than
  accepting the claim. Recovery also refuses outright when the dispatch was
  stopped through the conversation-stop path, which keeps the two clearers of
  that state mutually exclusive by construction.
  The proof is an inversion: the test seeds the opposite of every value it then
  asserts, so it cannot pass unless the production path ran. The store's own
  diff is empty, so none of the evidence recovery writes was weakened to make
  the wiring fit.
- 2026-09-15: two decisions land with it. One settles who recovers and resumes
  an orphaned dispatch, and why automatic resumption is refused. The other
  settles that the port keeps two terminal states where the retained product
  has one, because a refusal declines a request before provisioning while a
  retirement ends a provisioned engagement, and four consumers branch on the
  difference; and that the effects table with its fence governs retirement,
  with the retire driver owing an operator-triggered claim, observe and record
  rather than a sweeper the retained product never had.

- 2026-09-15: an operator can refuse and retire an engagement, and the retire
  half of that gap is closed. Two console routes now reach the store's `revoke`
  and its cleanup retry, behind the same agent-lifecycle authority the recovery
  route already uses; no scope was invented. The refusal half stays open and is
  named as the remainder rather than quietly folded in, because `reject` still
  has no production caller and saying otherwise would have made the record
  describe a world that does not exist.
- 2026-09-15: the late-output gap is closed. A production route reaches
  `record_late_output`, and it deliberately sits outside the Check hoop, because
  Check authorises only dispatches in `started` and would therefore refuse the
  very arrival the fence rule says must be recorded. What authenticates it
  instead is the store's own attempt-row check — exact runner id and secret,
  constant-time, no clock — so nothing was loosened to make the route fit. The
  32 KiB output bound, the 128-row per-attempt cap and `accepted=0` are
  unchanged, which is what keeps the evidence fenced and settling nothing. The
  review that accepted it began by observing that no store file was touched at
  all, and that fact carried half the verdict.
- 2026-09-15: a dead surface was removed rather than wired. `create_coordinator_task`
  had no production caller and its behaviour was already delivered by the wired
  delegation lane, so it was deleted — write, facade and every call site — under
  a hard rule that no test function may be removed. The tests that exercised it
  were rewritten against the surviving path with their properties named one by
  one, and the audit record now says deleted instead of carrying a gap id it was
  never going to close.
- 2026-09-15: the audit record's prose caught up with its tables, twice. Nine
  corrections landed across the decision records for operator recovery, approval
  delivery and terminal states, and a further pass found five more passages in
  the wiring audit itself that still described closed gaps in the present tense.
  The recurring defect is always the same shape: a correction applied in one
  place while the paragraph beside it keeps the pre-correction world. Every
  citation was checked against source before acceptance, and two proposed
  corrections were refused because the correction was itself wrong.
- 2026-09-15: a hosted intermittent in the approval-contention test was fixed at
  its root rather than by widening anything. The fixture admits an attempt when
  fifteen milliseconds remain, then spends five of them proving the production
  operation is still blocked; on a starved runner that probe can consume the rest
  of the window. Two of the function's three exits already treated that as a
  window it failed to model and asked the caller to rebuild; the third asserted
  instead. It now rebuilds like its neighbours. No bound, deadline or budget was
  lengthened, and no assertion about the product was changed — the authority
  rejection and every row-count check still run, on attempts that actually
  modelled the window.

- 2026-09-15: the wiring audit's open set is empty. G11 — project-side
  registration — was the last allocated gap, and with it closed every id (G1–G12,
  including G5a and both halves of G10) is closed, superseded or deleted. The
  production-callers checker reads `count 43, wired 43, owed []`.

  **G11.** `DomainRepository::register` (`domain.rs:730`) now has two production
  callers through the existing facade `DomainStore::register`
  (`domain_worker.rs:2842`): `hagency::bootstrap::registration::run`, the CLI
  `Registration` subcommand (`main.rs:215` → `bootstrap/registration.rs:29` →
  `:58`), and `hagency::console::project_sides::save`, the `POST /api/project-sides`
  route behind the shared `Scope::AgentLifecycle` check (`project_sides.rs:101-122`).
  Neither caller is test-enclosed.

  That closure cost one regression and its correction. The new POST route broke
  `project_sides::native_console_project_side_refuses_foreign_origin`, whose final
  clause asserted `NOT_FOUND | METHOD_NOT_ALLOWED` — encoding "this surface has no
  mutation route" as its proof that a foreign origin cannot mutate it. G11
  obsoleted the invariant, not the guard: the shared hoop applies
  `common_authority` and `same_origin(mutation=true)` to POST before any handler,
  so the route already refused all six forged-header shapes. The clause was
  replaced with those six cases asserted directly against the POST route — strictly
  stronger than what it replaced. Whole-target console: 56 passed / 0 failed, twice.

  **The Windows lane had been dead before its test phase, for a non-Windows
  reason.** `corpus-retention-vectors.mjs --check` byte-compared its fixture raw
  while every sibling oracle normalises; that fixture sits in
  `hagency-store/tests/fixtures/`, outside the `/native/fixtures/*.json text eol=lf`
  pin, so a Windows checkout gets CRLF and the compare fails. Because that step
  precedes the test phase, every later step was skipped. Fixed by normalising the
  comparison rather than guarding the step off on Windows — a guard would have
  disabled a working check to hide a portability bug. The same class was then swept:
  `approval_vectors.rs` hashed unpinned root sources raw (fixed at the hash site),
  and `native/**/tests/fixtures/*.json` is now pinned `text eol=lf`, verified to
  rewrite nothing (all eight matched files already `i/lf` in the index).

  **Two fixture fuses removed, neither widened.** The `usage-gate` probe
  self-expired after 5 s against a 25 s operation budget while its test never
  writes `usage-release`, so the fixture retired the workspace binding out from
  under the test it was meant to observe; `usage_gate_hold` now ends on the release
  file or the host's stdin close. `await_release` carried the same shape one layer
  deeper and now ends on the custody ceiling. In both, the 25 s budget, every
  assertion and the dev+ino root check are unchanged.

  **Prose corrections.** ADR-095's ingress-absent amendment marked superseded by
  the G1 closure, dated framing preserved; ADR-148's recovery prose and ADR-150's
  retire prose read as landed; and ADR-146's six stale open-gap sentences —
  including two that claimed the `reject` refusal half "remains open" while the
  row above recorded it closed — now state the true open set.

- 2026-09-15: the cutover record corrected, and three gaps recorded against it.
  ADR-135 (two-host cutover runbook) carried two steps that had gone stale and
  would misfire on a healthy cutover. Step 4's schema fence checks `user_version`
  is "exactly 25" with "026 the next free migration"; the store now carries
  022-033, so an operator following it literally sees 33, reads "stop — the old
  runtime must not be started against it", and aborts a correct cutover. Step 3's
  rollback says to "restore the state dir from the step-1 copy", but step 1 takes
  no copy — and such a copy would be unsound anyway, since `database.rs:118` sets
  `journal_mode=WAL` and `:114-117` sets `SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE`, and
  no backup or `VACUUM INTO` path exists in production code.

  Three conditions the runbook does not carry are now recorded in it: a
  matrix-configured profile cannot bootstrap on fresh state (the `engagements`
  half of `Prepared::load`'s precondition has exactly one production writer,
  `domain.rs:1138` inside `admit()`, reachable only through provisioning ingress
  on a running serve — this runbook's own profile configures no matrix and is
  unaffected); `operator.token` is written once under `O_EXCL` with no rotation
  path, so losing it forfeits every authority surface of that state dir; and the
  ADR-140/ADR-144 qualification evidence gate is enforced on no path, being red
  locally against a checked-in placeholder and skipped by name in CI.

  Three things were checked and are NOT gaps: the absent importer is the
  runbook's deliberate fresh-install posture, rollback is specified (window R1
  opens at step 3 and closes at the end of step 5, after which snapshot-restore is
  refused), and dual-running is rejected by policy — the narrower true finding is
  that the policy has no mechanical enforcement, since the only exclusive lock is
  intra-native.

  The file_service parallel-race is instrumented, not fixed: the probe now stamps
  its protocol stages and the status probe names its failure arm, so the next
  firing can separate pre-main exec starvation from the port-TOCTOU and
  fake-server-starvation candidates. A 3-of-6 reproduction was observed under the
  isolated `--test file_service` shape; whole-package runs pass and do not
  discriminate.

- 2026-09-15: the Rust-migration review removed two lifecycle success claims
  that had no transition behind them. `start` previously returned success for
  every non-active engagement without re-arming a dispatch, and preset-apply
  stored a session-local pointer that changed no engagement, budget, account or
  provision effect and could never complete. Both compatibility routes now
  validate lifecycle authority and fail closed with named 501 words; the grant
  carries no pending pseudo-state, and the roster exposes only stop, whose fence
  is a real durable domain write. ADR-130, the task spec, integration tests and
  browser assertions now describe that same smaller truth.

- 2026-09-15: the migration branch's deterministic CI gaps are closed locally.
  Approval-vector source extraction normalises CRLF before matching its literal
  anchors, the migration inventory classifies the native installer and launchd
  registration, dashboard fixtures follow the current resource and usage wire
  schemas, and the production-caller tests register with whichever Node harness
  invoked them so the Vitest JSON catalog is not polluted by TAP. That catalog
  also exposed five artifact-retention selectors whose implementation ADR-129
  explicitly deferred; they are now recorded as owed instead of being presented
  as passing tests that do not exist.

- 2026-09-16: real qualification on mini3 Palpo and the Robrix2 headless
  client remains **incomplete**, not an entire-port or soak success. In the
  isolated qualification worktree and private fresh native state, the real
  Robrix device sent three owner-DM messages as `m.room.encrypted` (the server
  events have no plaintext body). The Rust SDK verified and decrypted those
  messages, and canonical inbox selection created three tasks/owned attempts.
  No final reply was delivered and no soak interval has started.

  The real server ignores the requested room filter, so collect now validates
  the complete bounded sync before narrowing room sections, preserving global
  crypto data. Room-event bounds count actual event sections, not nested
  power-level keys. SDK replay custody keys exact `(cursor, digest)` receipts,
  since a server can change key-count metadata without changing its cursor.
  A verified peer inbound key share legitimately adds an Olm session after
  enrollment; restore now requires all protected original session IDs to remain
  present, while retaining exact recipient/identity checks and the existing
  per-peer session ceiling. An independent SDK crypto fixture proves this
  restore path without claims, identity reset or another key publication.

  The pinned Linux Codex 0.154.0 stream exposed optional int64 `emittedAtMs`,
  a global remote-control notice, thread-scoped MCP startup without turn IDs,
  and global account rate-limit snapshots. Sanitized schema-digest fixtures
  cover their narrow compatibility handling. Timestamp metadata cannot extend
  deadlines; notices cannot approve, complete or supply canonical usage.
  Failed/cancelled canonical task-writer startup remains a named refusal.
  Initial protocol-envelope and turn-start malformed failures are corrected;
  the third live attempt reaches update and refuses an unsupported callback,
  with whole-tree cleanup observed. Fixed callback-shape diagnostics have
  been added to the original owned report, without callback text or secrets.

  Local checks: protocol/session/transport 48 tests passed before the additional
  fixed-label test (which also passes), Matrix package 170 passed, Palpo service
  4 passed, runtime-status projection passed, and all-target runtime/service
  Clippy passed before the fixed-label amendment. These offline checks do not
  substitute for real model/tool completion, encrypted reply rendering, bounded
  sustained sync retention, restart/recovery, effective sandbox qualification,
  lifecycle/API parity or a measured soak. Operator adoption is a fresh-state
  bridge, not automatic fleet provisioning. Those integration gates remain owed.

- 2026-09-16 follow-up: real mini3 Palpo / Robrix2 owner-DM roundtrips now
  work for the narrow canonical task/reply path. Two initial requests and six
  subsequent soak pulses each reached an owned real-model attempt, canonical
  task completion, observed whole-tree cleanup, server-accepted final reply,
  and exact reply text in the real Robrix headless client. The initial request
  and reply server events were checked as encrypted, with no plaintext body.
  The same encrypted DM/device route was used by the six soak pulses; their
  operator observations bind each accepted reply event ID and rendered frame.
  There are eight delivered final replies, eight completed dispatches, and
  four older failed qualification dispatches retained as outcome-unknown.

  This is **not a successful soak or entire-port qualification**. The requested
  one-hour operator soak stopped after 375.775 seconds (6m15.775s), with six
  pulses passed and the seventh pending. Native status refused refresh with
  Matrix capacity at the 64-entry completed-sync receipt limit; transport
  generation 8 is unavailable. No protected SDK state was cleared to continue.

  The local successor uses immutable authenticated replay/source history to
  retain older proofs while keeping 64 live receipts. Focused rollover/reopen,
  terminal-source, and corruption/root-publication rollback tests pass. An
  initial all-target Matrix Clippy check passed, but the broader package run
  exposed a pre-apply limited-timeline regression that dropped quarantined raw
  custody. The correction now retains Prepared custody before source lookup
  and quarantines refusal before SDK mutation. Its broader recheck, deployment,
  and a fresh measured soak are still owed; the failed live run remains failed.

  The native task-MCP launch now carries the legacy router's exact per-tool
  approval override for the four canonical task tools only (ADR-057); this
  does not grant shell/file permissions or answer native MCP elicitation.
  Historical outgoing custody inspection is again attempted even when current
  refresh is fenced, without granting current readiness or another HTTP write.

  A separate real Linux Codex 0.153.4 operator sandbox probe failed both
  verdicts on UnsupportedEvent before observing a write tool. Whole-tree
  cleanup was observed, but neither target file was created, no tool attempt
  or ungranted approval was observed, and absence alone is not a refusal proof.
  Effective sandbox qualification remains red. Automatic fleet provisioning,
  full lifecycle/API parity, real owner-approval/media/restart/recovery paths,
  other bounded receipt histories, and production qualification are not
  established by the successful owner-DM roundtrips.

- 2026-09-16 continuation: the corrected protected sync-history implementation
  passes all 175 Matrix package tests (152 unit and 23 integration tests),
  all-target service/Matrix/runtime Clippy, and `git diff --check`. The
  fixed-category notification diagnostic passes its offline regression, and
  the read-only encrypted-custody operator example passes two private-state,
  authentication/bounds and redaction checks. `agent-spec` is unavailable in
  this environment; its parse/lint checks were not run.

  The history implementation was deployed to mini3 without clearing the SDK
  identity or protected state. Generation 9 then retained the seventh failed
  soak request in known SDK-derived domain-refusal quarantine. The new explicit
  `intake-refuse-stale-session` command (ADR-151) proved the exact pre-session
  boundary and terminally rejected one unacknowledged candidate, without any
  effect retry. Read-only inspection shows no pending observation/batch and
  64 live receipts with protected archive present. The eight completed replies
  and four older uncertain dispatches remained unchanged. This is negative
  settlement of a synthetic request, not its successful execution or a repair
  of the failed soak verdict.

  Fresh authenticated adoption established generation 10 on the same encrypted
  account/device state. A new 3600-second real Robrix2 headless / mini3 Palpo
  owner-DM soak started at 2026-09-16 08:48:41 UTC, with task/reply pulses every
  120 seconds and health checks every 30 seconds. Its first pulse completed,
  was durably delivered and rendered in Robrix, and both request/reply server
  events are `m.room.encrypted` with no plaintext body. There are now nine
  completed dispatches/delivered replies and the original four uncertain
  attempts. This fresh run is **in progress**, not a passing hour. Its scope
  is sustained sync and owner-DM replies; 30 pulses do not qualify the still
  bounded outgoing-receipt history or entire-port parity.

  The real Codex 0.153.4 sandbox operator omitted `CODEX_HOME`, unlike production.
  Forwarding the prepared private namespace fixed authentication. On the
  isolated Linux qualification container with namespace prerequisites enabled,
  the inside-workspace probe now observes an actual tool write, completed
  upstream turn, file creation and whole-tree cleanup. The outside probe still
  completes without a tool/approval attempt: no outside file appears, but that
  is **not** sandbox-refusal proof. The combined gate remains failed and the
  native soak container's default kernel setup is not qualified by that separate
  probe. Full fleet/API/lifecycle, owner approval, media, recovery, further
  receipt rollover and production qualification remain owed.

- 2026-09-16 continuation, observed through 09:21:30 UTC: the same generation-10
  real mini3 Palpo / actual Robrix2 headless owner-DM soak remains running.
  Seventeen task/reply pulses have durably delivered and rendered, the latest
  at elapsed 1956.059 seconds (32m36.059s), with a 15.198-second response.
  Independent server inspection of the latest twenty DM events shows
  `m.room.encrypted` and no plaintext body; the captured pulse-14 frame was
  visually inspected and contains the exact reply. Read-only encrypted journal
  inspection shows 64 live sync receipts, 64 intake receipts, an archive root,
  no pending observation and no intake batch. The native container has zero
  restarts. Its original four outcome-unknown dispatches remain retained.
  This is an in-progress hour, not a passing soak or entire-port qualification.
  The earlier 375.775-second failed run remains failed. This client is Robrix2
  headless, not a separately qualified upstream Robrix desktop build.

  ADR-152 and the outgoing-history task spec now define a local successor to
  the permanent 64-settled-send stop. Settled final/notice/file receipts retain
  their exact ID, fence, kind and accepted complete-attempt digest in the private
  authenticated trie. The hot cache remains at 64; immutable nodes precede the
  atomic encrypted root/cache/Prepared-or-Settle publication. Failed root
  publication restores original cache/root memory and poisons the original
  owner, without beginning or retrying a send. Exact historical queries do not
  load a lifetime list or grant current task/route/HTTP authority.

  Historical final replay checks for a conflicting present domain row: a newly
  claimed row with the same ID/fence cannot borrow old SDK delivery. An absent
  retained canonical row does not erase protected past acceptance or create a
  replacement row. The absence counterfactual temporarily renames the fixture
  row and its references in deferred-constraint transactions, restores them and
  verifies FK integrity; it is not a runtime pruning/cascade qualification.
  Notice replay requires its already-Delivered domain row. Retained file jobs
  resolve only from their exact archived receipt and original already-Delivered
  domain acceptance, never from a compact receipt as first Delivered authority.

  The full Matrix package recheck passes 176 tests (153 unit and 23 integration),
  following correction of that fixture's foreign-key violation. Both focused
  outgoing-history tests pass: 150 actual local-TLS accepted final sends plus
  restart/oldest replay/new-send continuation, and root-publication/missing-node/
  fresh-claim refusal. Additional exact/different-fence assertions pass their
  focused recheck. Store reply integration tests pass all 17 cases, all-target
  service/Matrix Clippy passes, and `git diff --check` passes. Historical file
  recovery uses an actual accepted file and retained media job; its fixture
  moves that settled receipt out of cache, not 64 real file sends. These tests
  remain offline; operator runs alone contact Palpo/Robrix. `agent-spec` remains
  unavailable, so no parse/lint result is claimed.

  Outgoing-history code is not deployed to the current soak service; its binary
  remains unchanged to preserve the measured run. A single resource snapshot
  (236.5 MiB container memory, 9.24% CPU) is not an embedded-budget or memory-soak
  verdict. Live outgoing rollover, approval/upload history, physical retention,
  automatic fleet provisioning, lifecycle/API parity (start/preset still 501),
  real owner approvals/media/restart/recovery, effective sandbox enforcement and
  production/cutover qualification remain owed. No SDK reset, commit, PR or
  production cutover was performed.

- 2026-09-16 continuation, terminal observation at 09:48:43 UTC: the same
  generation-10 mini3 Palpo / actual Robrix2 headless owner-DM soak exits 0 with
  `soak_passed`, elapsed 3602.040 seconds and 30 passed task/reply pulses. Each
  pulse required an already-Delivered domain reply and the exact reply rendered
  by the real client before capturing its frame. Pulse 30 completed in 17.242
  seconds; its captured frame was visually inspected. Independent homeserver
  inspection binds its exact reply event ID and shows all twenty latest events
  are `m.room.encrypted` with no plaintext body. The native container has zero
  restarts and read-only protected-journal inspection shows 64 hot sync receipts,
  64 intake receipts, an archive root and no pending observation or batch. The
  final domain snapshot has 38 completed dispatches and 38 Delivered replies in
  total; the original four outcome-unknown dispatches remain unchanged and
  generation 10 remains available. No service restart, SDK reset or source
  rebuild interrupted this measured run. The original 375.775-second failed
  soak remains failed. This passing hour qualifies only encrypted owner-DM
  task/reply and sustained sync, not upstream Robrix desktop, outgoing rollover,
  two-agent operation or entire-port parity.

  Runtime command evidence now retains a bounded, private exact command/cwd
  binding. Changed active/completed bindings or contradictory supplied terminal
  fields invalidate it. The operator probe requires one matching intended
  command under the original cwd, no unrelated/repeated tool, exact inside file
  bytes and whole-tree cleanup. Outside qualification requires an actual
  matching failed command or matching ungranted command approval, not merely
  no outside file. Three focused offline runtime/probe witness selectors and
  all-target runtime/execution/progress Clippy pass; `git diff --check` passes.
  The LF-normalized compiled fingerprint covers sixteen selected critical
  sources, not the entire build tree. ADR-140 and the new command-witness task
  contract record these requirements.

  Real operator runs against Codex 0.153.4 and 0.154.0 each pass the exact inside
  write with one observed command, but both direct outside and supplementary
  outside-symlink probes observe zero command/tool items and no approval. Their
  outside verdicts remain failed: no outside file is not enforcement proof.
  The symlink stays bound to the physical outside target before/after the run
  and does not replace or repair the original gate. These separate isolated
  probe containers enabled namespace prerequisites; they do not qualify the
  native soak container's default kernel setup. No approval was granted.

  The offline all-target workspace run with `--no-fail-fast` finishes exit 101,
  with two failed targets: console roster provisioning remains pending rather
  than physically complete, and real two-agent qualification is still an
  explicitly failing placeholder. Only the two real Codex app-server evidence
  selectors were excluded by the command-line filter; no placeholder or
  no-attempt verdict was relabeled passing. ADR-147 requires synchronous inline
  physical provisioning, not a separate effect worker or identities/routes
  synthesized from approved intent. That implementation remains owed. Start
  and preset still return 501; real owner approvals, media, recovery, usage/
  quota, retention and effective sandbox enforcement remain unqualified. The
  outgoing-history changes are still local, not deployed to the completed soak
  binary. No commit, PR or production cutover was performed.

- 2026-09-16 continuation, observation at 10:12 UTC: generation 10 was cleanly
  stopped after its terminal passing hour and fenced unavailable. The outgoing
  history and command-witness source was built on mini3, and actual existing
  account adoption observed a fresh generation-11 receipt and session before
  starting the new native service. The original SDK identity/store was retained;
  no account/device replacement, SDK reset or widening of default container
  kernel permissions was performed. A second real Palpo / Robrix2 headless
  hour soak started at 09:52:43 UTC with 30-second pulses and is still running,
  not yet a passing hour. Its read-only domain snapshot has 72 completed
  dispatches, 72 Delivered replies, one started dispatch and the same four
  original outcome-unknown dispatches. The native container is running with
  zero restarts and generation 11 available.

  Read-only authenticated inspection now proves real outgoing cache rollover:
  the same oldest delivered final receipt was present in the 62-entry hot
  cache and absent from the archive; after further real sends it is absent
  from the bounded 64-entry hot cache and present in the authenticated archive
  (ten visited nodes). There is no pending outgoing or intake observation at
  that snapshot. This is actual native SDK custody on mini3, not a synthetic
  success receipt. Pulse 33's real Robrix frame was visually inspected, and
  independent server inspection reports twenty encrypted latest events with
  no plaintext body. Ongoing evidence is in
  `soak-20260916T0954-gen11-outgoing/observations.jsonl` under the private
  operator cache; this does not qualify media/approval history or retention.

  A production adoption bug is fixed locally: Host-only `claim_effect_for`
  selects the exact named eligible effect inside the original immediate
  transaction instead of globally claiming a different pending effect and
  filtering afterwards. Two real DomainStore tests cover concurrent targeted
  claiming, reopen-to-Uncertain, transaction rollback, exhausted fences,
  invalid identifiers, revocation and registration-generation changes.
  All six domain tests and store/native all-target warnings-denied Clippy pass.
  This fix is not deployed to the already-running measured generation-11
  binary. It is a provisioning prerequisite, not the missing physical factory.

  The private `matrix_custody` operator example adds bounded read-only exact
  settled-receipt lookup without exporting receipt IDs, digests or contents.
  Five offline tests pass, including corruption/identity/conflict refusals,
  the full 256-branch authenticated path and repeated-bit refusal; example
  Clippy passes with warnings denied. Those encrypted diagnostic fixtures are
  not real SDK sends. Spec binding inventory lists 901 selectors and remains
  failed on the same two missing physical-provisioning/route selectors.
  ADR-147's implementation checkpoint and the ingress spec retain the
  synchronous physical provisioning requirement; no separate effect worker,
  fabricated Active state or premature session route was introduced.
  Entire-port, fleet provisioning, two-agent, lifecycle/API, approval/media,
  usage/quota, retention, recovery and sandbox qualification remain owed.
  No commit, PR or cutover was performed.

- 2026-09-16 continuation, terminal observation at 10:12:54 UTC: the same
  generation-11 dense real-client soak exits 1 with `soak_failed` after
  1210.918 seconds and 36 passed pulses. Pulse 37's original dispatch is
  completed, but its canonical task remains InProgress and it has no final
  reply or owned completion receipt. Its sole retained output is upstream
  assistant text exactly equal to the requested ACK; there are zero
  task-operation receipts. The platform correctly did not turn that ordinary
  final answer into Done or a Matrix reply. The 64-entry outgoing rollover
  proof remains valid, but this is not a passing hour.

  A separately sent real Robrix follow-up attempted recovery of that same
  missing canonical completion. It did not succeed: the operation reached its
  configured 30000-ms deadline, with protocol Unknown, whole-tree cleanup
  observed stopped, negative settlement and no task-operation/owned-completion
  receipts. The fifth outcome-unknown dispatch is retained; the original four
  have not been cleared. Capabilities now explicitly report outcome_unknown,
  owned_attempt/deadline. No reply was fabricated, no original failed run was
  relabeled, no automatic restart/SDK reset/privilege change was performed.
  Real sustained/recovery qualification is therefore still incomplete.

- 2026-09-16 continuation, account-step checkpoint: implemented the Host-only
  ordinary registration-token primitive `TokenAccountProvision` under
  `task-rust-token-account-provision.spec.md` (satisfies
  REQ-RUST-MIGRATION-EXECUTION). Its original context binds registration,
  effect/fence/payload, desired engagement-scoped account/device, origin and
  credential profile. Actual owner-private create-only encrypted custody and
  an exclusive file lock precede the registration POST. The token-only UIA
  requests retain the same independently random password/device/session;
  the password is discarded and never persisted. Registration requests have
  no representative bearer header, and only the actually returned token may
  authenticate fresh whoami. Substituted account/device, unsupported profile,
  corrupt/missing custody and conflicting context fail closed. Lost responses
  never permit another registration; accepted-response reopen is GET-only.

  The accepted owner survives a dropped receiver and retains its lock through
  network and accepted blocking writes. A typed opaque account handle may
  consume its private credential into a fixed ordinary HostConfig; it exports
  no token and creates no canonical Active state, room/crypto authority, route
  or runtime-launch receipt. Existing Host origin validation and bounded HTTP
  construction were shared without changing collector transport policy.
  The already-used getrandom 0.3.4 dependency supplies independent entropy.

  Five local TLS / real protected-filesystem account-step tests pass (0.78 s),
  covering accepted registration, actual returned-token whoami, no-password
  persistence, inspect-only reopen, context/corruption refusal, dropped
  receiver/concurrent ownership, actual admitted-POST lost-response refusal,
  unsafe profile/identity/guest/framing/bounds and cumulative original deadline.
  The all-target offline Matrix package finishes exit 0: 153 library tests and
  28 integration tests pass, none filtered. All-target Matrix Clippy finishes
  exit 0 with warnings denied; `git diff --check` passes. `agent-spec` remains
  unavailable, so no parse/lint result is claimed.

  The all-feature offline spec-binding inventory exits 1 with 906 selectors
  and the same two missing `native_provisioning_effect_completed` and
  `native_provisioning_session_route` selectors. Listing/metadata coverage is
  not passing behavior or physical provisioning evidence.

  ADR-147's checkpoint explicitly retains the unfinished inline factory. The
  new primitive is local source, not deployed to the generation-11 service and
  not yet wired into approval fulfillment. These account-step fixtures do not
  qualify actual Palpo account creation, appservice provisioning, managed
  agent-home/template creation, SDK enrollment, owner DM/project membership,
  runtime launch or real two-agent operation. Both mandatory ADR-016 deployment
  profiles and the two ingress completion/route selectors remain required.
  The latest live snapshot still has 75 completed dispatches, 74 Delivered
  replies and five retained unknown dispatches; generation 11 is available
  and its native container running with zero restarts. The second soak and its
  separate recovery remain failed. No commit, PR, SDK reset or cutover occurred.

- 2026-09-16 continuation, inline account wiring: the previous user-facing turn
  was a status restatement, classified no progress rather than a verified wait.
  Revalidated the current isolated source and actual mini3 service; read-only
  live SQL still shows 75 completed dispatches, 74 Delivered replies and five
  unknown dispatches. The container is running with zero restarts. The original
  failed dense soak and its failed separate recovery were not restarted/cleared.

  Implemented `task-rust-inline-provision-account.spec.md` (satisfies
  REQ-RUST-MIGRATION-EXECUTION). An explicit private
  `registration_token_account_step_v1` bootstrap marker attaches the Host-only
  `TokenProvisioningHost` to the ordinary Collector. The registration credential
  and wrapping key are separate protected files; no secret enters driver JSON,
  console DTOs or runtime input. After verified representative approval and
  canonical approve, that same accepted owned intake job claims exactly its
  provision effect and performs real registration/returned-token whoami inline.
  No separate effect worker or callback accepting fabricated physical proof was
  introduced.

  The verify-only project snapshot is refreshed before this physical step's
  approval rather than authorizing it from the request's old cache. The original
  DomainStore checks the actual Started effect/fence/payload, Reserved engagement
  and current registration inside its own SQLite Immediate transaction before
  each registration POST and before admitting the resulting account observation.
  Sixteen non-evicting Host job slots retain opaque observed accounts and unknown
  jobs; outer intake-receiver loss does not discard the accepted owner. Replay
  acknowledges only the historical account step, and a fresh Host cannot reclaim
  Started/Uncertain work. Account errors retain Unknown; account success leaves
  Started rather than fabricating Complete/Active or a session route.

  Eight new offline local-TLS / actual Collector/DomainStore/private-filesystem
  inline tests pass, covering observed registration, verdict replay, admitted
  lost response and fresh-Host refusal, private profile/forged-sender refusal,
  actual revocation during UIA, dropped outer receiver/concurrent Busy, changed
  fresh project authority and sixteen-slot capacity. The current all-target
  Matrix package exits 0: 161 library plus 28 integration tests (189 total),
  none ignored or filtered. Full domain target exits 0 with seven tests,
  including the new current-scope predicate with actual changed fence/payload
  refusal. Native bootstrap's complete target exits 0 with ten tests; the new
  private-profile selector covers eight actual executable configuration cases.
  All-target Matrix/store/native Clippy exits 0 with warnings denied, and
  whitespace validation passes. The initial native bootstrap compile failure
  (an unclosed delimiter during reception-branch cleanup) and initial capacity
  fixture unused-mut warning remain in their original external logs, not relabeled.

  The all-feature offline binding inventory exits 1 with 916 selectors and the
  same two missing physical-completion/session-route selectors. This is metadata,
  not passing execution or full provisioning proof. `agent-spec` is still absent;
  no parse/lint/lifecycle result is claimed. ADR-147 and agent knowledge record the
  wired account-stage boundary without changing the accepted full inline product.

  This new source is not deployed and these fixtures are not actual Palpo account
  creation. Both ADR-016 profiles, managed home/template, actual new owner DM and
  project membership, pre-activation SDK enrollment, runtime/fleet and two-agent
  operation remain unfinished. Canonical-completion reliability, real sustained
  recovery, approvals/media, lifecycle/API parity, sandbox, effective usage/quota,
  retention, full M0–M9/release gates and upstream Robrix desktop qualification
  remain owed. The safe next factory work is physical rooms/home and SDK enrollment
  under the original pre-activation effect, followed by runtime and genuine domain
  completion/routes; a successful account alone does not close that work. No
  commit, PR, live restart, SDK reset or production cutover was performed.

- 2026-09-16 continuation, original pre-activation SDK: the previous turn was a
  status restatement, classified no progress. Revalidated the isolated source
  and actual mini3 container/SQL before implementation. The service remains
  running with zero restarts, 75 completed dispatches, 74 Delivered replies and
  five unknown dispatches; the original failed soak/recovery were not rerun.

  Implemented `task-rust-provision-account-enrollment.spec.md`, satisfying the
  migration/session/privacy requirements without shrinking ADR-147's factory.
  The original opaque inline account retains its actual writer/claim/registration
  and can enroll the same Agent-purpose SDK while the effect is Started and the
  engagement Reserved. Its original validated request fixes the project Group
  and distinct owner+agent encrypted DM. Actual token/room/project/privacy and
  anchor checks precede SDK creation and every key write; original DomainStore
  validation follows the last actual room-read await. Normal Agent enrollment
  still refuses Reserved authority. No Applied/Active/transport/session route
  is fabricated.

  One immutable account job retains the Collector, SDK and result after caller
  loss. Success replay revalidates without uploads/claims, changed profiles
  refuse, unknown/failed jobs cannot rearm, and a Weak teardown assertion proves
  no settled account/owner Arc cycle. SDK-custody closure is separately owned,
  retains its original acknowledgement, never reopens the job, and is explicitly
  not a canonical transport/route/lifecycle cleanup receipt.

  Five new selectors pass using actual inline approval/registration through
  local TLS plus an independent actual recipient SDK. They cover original
  signing uploads and session claim, independent room-key/event decryption and
  protected Complete reopen, ten configuration/current-room refusal variants,
  same-owner read-only replay and changed-profile/current-scope refusal, dropped
  outer caller and admitted lost-upload response, and actual revocation while
  the final Direct state GET is held after SDK Preparing/Possible. The latter
  refuses before the actual first key POST. The existing standalone account
  integration test also confirms that no inline claim means no SDK enrollment.

  Full current all-target Matrix package exits 0 with 166 library plus 28
  integration tests (194 total), none ignored or filtered. Library runtime is
  150.89 seconds; the longer media/custody tests were polled to terminal result,
  not restarted. All-target Matrix/store/native Clippy exits 0 with warnings
  denied; whitespace validation passes. The initial test-only Debug diagnostic
  and borrow-lifetime compile failures, the first three failing execution
  assertions (Reserved transport read/closure and caller-loss polling), and
  initial duplicate-fixture-module Clippy refusal remain in their separate
  external logs. Corrections did not add Debug to private clients, weaken
  authority, retry key writes, raise deadlines or suppress the lint.

  The offline all-feature binding inventory resolves 921 selectors but remains
  failed with the same two
  missing physical-completion/session-route selectors. It only lists compiled
  tests, not executable qualification. `agent-spec` remains absent; no parse,
  lint or lifecycle result is claimed. Native bootstrap's account-only marker
  still does not call the full physical factory, and this source is not deployed.
  Both ADR-016 profiles, actual new room creation/join, managed home/template,
  runtime/fleet/two-agent operation, genuine effect completion/routes, completion
  reliability and sustained recovery/soaking, approval/media/lifecycle/API parity,
  effective sandbox/budgets/quota/retention, platform/upstream desktop and full
  M0–M9/release gates remain owed. Next connect actual room/home observations to
  this original pre-activation SDK owner, then runtime and canonical completion.
  No commit, PR, SDK reset, live restart or production cutover occurred.

- 2026-09-16 continuation, original physical room setup: the previous turn was
  a user-requested status restatement, classified no progress. Revalidated the
  isolated worktree, preserved original checkout dirt and actual mini3 service.
  The interrupted test-module patch had not applied and no such test process or
  log existed; no running observation was discarded or restarted. Completed the
  source checkpoint under `task-rust-inline-agent-rooms.spec.md` instead of
  treating the earlier unexecuted patch as evidence.

  The original ordinary account creates a new encrypted owner DM itself; a
  separately authenticated representative invites it to the exact bound project,
  and the actual returned agent credential joins. Fresh membership observations
  remain separate from each HTTP acknowledgement. The actual owner joins via
  an independent TLS client in the positive fixture; production never joins as
  the owner. The retained job polls actual owner membership, not an invitation,
  before connecting known room IDs to the original pre-activation SDK operation.
  Fixed encrypted create-only room custody pins original claim/request/origin/
  credentials/key before every POST. Caller drop retains the original owner;
  unknown/torn/partial responses stay spent. Complete replay is read-only, and
  known DM metadata survives later failures without claiming readiness.

  The inline Host retains the opaque actual account before entering these later
  stages. Native bootstrap adds only the explicit closed rooms/enrollment marker
  plus a separate protected representative-token file and public peer anchors.
  Account-only/default profiles keep their existing boundary. Corrected both
  normal and pre-activation enrollment recipient selection to require anchors
  for encrypted-room participants, not plaintext project representatives. Actual
  plaintext project membership/binding and invitation authority remain checked.

  Six new Matrix selectors execute actual original approval/account/room/SDK
  protocol through local TLS and protected files. They cover independent actual
  owner join and recipient decryption in the created DM; nine credential/privacy/
  membership/invitation/join refusals; finite owner-join timeout; original retained
  caller-loss and lost create/invite/join responses; changed-plan and GET-only
  replay; original engagement revocation while the last project GET is held; and
  seven torn/wrong-stage/missing/extra-record reopen refusals before HTTP. Effects
  remain Started/Reserved on success and unknown on failures, with no Active
  transport, fabricated runtime/physical receipt or derived session route.

  Final full Matrix all-target package exits 0: 172 library plus 28 integration
  tests, 200 total, none ignored/filtered; library runtime 159.07 seconds. Its
  longer custody/media tests were polled through original session 67125 to actual
  exit, not restarted. Native bootstrap's complete target exits 0 with 11 tests,
  including 18 actual rooms-profile configuration cases. All-target Matrix/store/
  native Clippy exits 0 with warnings denied, and whitespace validation passes.
  Initial missing-type/disabled HTTP helper compile refusals and two failed
  bootstrap runs remain in separate external logs. The latter used an invalid
  arbitrary Ed25519 point; a real independent recipient supplies valid public
  configuration now, with explicit invalid-point and padded-anchor negatives.
  No production validation or default deadline was weakened to make them pass.

  The offline all-feature binding inventory exits 1 with 928 selectors and the
  same missing physical-completion/session-route selectors. Listing is not
  execution or complete qualification. `agent-spec` is absent; no parse/lint/
  lifecycle result is claimed. This source is not deployed and no fresh Palpo
  room/account or new soak result is claimed. Final actual mini3 inspection shows
  the service running with zero restarts, 75 completed dispatches, 74 Delivered
  replies and five unknown dispatches. Failed generation-11 soak/recovery remains
  untouched; the earlier passing hour proves only encrypted owner-DM replies in
  actual Robrix2 headless, not upstream Robrix desktop or whole-port readiness.

  The full goal remains active: both ADR-016 profiles, managed home/template,
  runtime/fleet/two-agent physical provisioning, genuine completion/routes,
  completion reliability, sustained recovery and new live soaking, approvals/
  media/lifecycle/API parity, effective sandbox/budgets/quota/retention, platform/
  upstream desktop qualification and full M0–M9/release gates are still owed.
  Next bind genuine home/runtime observations to this original account/room/SDK
  owner, then complete the domain and derive routes from actual fulfillment.
  No formatter, new dependency, commit, PR, SDK reset, deployment, live restart
  or production cutover occurred.

- 2026-09-16 continuation, original native home setup: the previous goal turn
  was a status answer, classified no progress. Revalidated isolated worktree
  feat/rust-migration at 67e0e84, preserved original checkout dirt, and observed
  the original Matrix/bootstrap processes to terminal exit 0 (204 and 12 tests).
  The prior strict-lint log contained an actual eight-argument helper refusal;
  it was not assumed green. Replaced those inputs with one private owned Creation
  context, preserving the same blocking job, fixed deadline and cancellation
  signal. Strict all-target Matrix/store/native Clippy now exits 0 without a lint
  allow, formatter, dependency or authority change.

  Implemented task-rust-inline-agent-home.spec.md: native physical v1 homes,
  immutable manifest, retained four framework entry templates, supervisor sibling,
  coordination/project mappings and native task wrapper before the first account
  registration. Agent identity/owner/project derive from the original request;
  framework/model/provider/reasoning come from actual qualified writer resource
  configuration. Manifest task is null and no runner secret is written into the
  home or wrapper. Fixed home/source/binary objects are retained; separate SDK/
  credential state cannot be nested in copied projects or managed homes.

  Both explicit bounded copy and symlink modes are preserved. Copy has finite
  per-file/aggregate/entry/depth bounds and keeps safe relative links and owner
  executable bits. Unsafe links/special files, exceeded limits, changed objects,
  missing mappings and existing/partial custody refuse before network effects.
  Protected create-only possible custody precedes physical creation; unknown work
  never repairs or reuses an unrelated home. The actual result remains owned
  through late failure, and fresh resource plus original Started/Reserved writer
  checks follow the real filesystem await. No second launcher or Applied/Active
  receipt/session route is introduced by this prerequisite.

  Five exact home selectors pass, with actual original approval/account/room/SDK
  flow and independent encrypted-DM recipient. Coverage includes both project
  ownership modes and source replacement, seven partial/unsafe/limit refusals,
  original retained caller loss during a 60 MiB copy, and actual writer revocation
  during an unfinished 60 MiB copy. The last leaves completed physical custody
  retained but no account/room/SDK write or route, including read-only replay.
  Its initial failure was the positive-only account teardown incorrectly used
  for a deliberately absent account; assertions had passed and teardown was
  corrected without weakening them. The failed log is preserved.

  Post-fix bootstrap's complete target exits 0 with 12 tests, including twelve
  closed home-profile configuration cases. The fifth home selector was added
  after the full post-fix 204-test package, so a fresh full current Matrix run was
  observed through original session 2001 to exit 0: 177 library plus 28 integration
  tests, 205 total, none ignored/filtered; library runtime 160.04 seconds.
  Installed-target Windows GNU all-target
  Matrix/store/native check was separately observed through original session 58345
  to exit 0. Compilation is not native Windows home filesystem/platform conformance.
  Final offline binding audit exits 1 with 934
  selectors and the same two genuine completion/session-route selectors missing.
  Its earlier overlapping snapshot also lacked the then-new home-scope selector;
  the final source-consistent inventory resolves it. agent-spec is absent; no
  parse/lint/lifecycle result is claimed.

  Actual mini3 read-only inspection still shows running=true, zero restarts,
  75 completed dispatches, 74 Delivered replies and five retained unknowns. This
  source is not deployed and no new Palpo factory/soak result is claimed. Failed
  generation-11 soak and recovery remain untouched. Next complete genuine runtime
  setup through the existing owned launcher, then physical Applied/Active and
  derived routes; both deployment profiles and actual fleet/two-agent, completion
  reliability, sustained recovery/re-soaking, approval/media/lifecycle/API parity,
  effective sandbox/budgets/quota/retention, native platforms/upstream desktop and
  full M0–M9/release gates remain owed. The full goal remains active. No formatter,
  new dependency, commit, PR, SDK reset, deployment, live restart or cutover occurred.

- 2026-09-16 continuation, source checkpoint observed by 12:37:55 UTC: the
  previous status check yielded terminal evidence that the four AS account
  selectors pass, making the missing original inline integration the next action.
  This continuation implements its actual fixture wiring and native profile gate,
  not a claim that the full migration or failed live soak is repaired.

  The fixed private AS credential now travels through the original approved
  request/home/account/room/SDK owner. Sensitive-header passwordless registration
  inhibits login; actual accepted creation precedes a separately fenced dedicated
  device login. Its session remains private operational SDK custody, not an agent
  credential or replacement side master. Fixed encrypted AS stages preserve
  original claim/profile/origin/credential binding, while normal custody identity
  and default markers remain unchanged. No lost/partial attempt retries or falls
  back, and receiver loss retains the original operation and lock.

  Added original inline AS fixture coverage, with strict actual HTTP assertions
  for both privileged POSTs after physical home completion. The positive uses an
  independently joined owner and decrypts in the agent-created encrypted DM through
  its actual claimed session. Three held-read negatives revoke the master before
  invitation or first SDK key upload, or broaden the namespace before that upload.
  Independent TLS whoami still authenticates the original device token. No later
  room/key POST or route is admitted, and a closed-original SDK ledger retains
  first Possible/no response. Failed owners cannot rearm on replay.

  Native bootstrap adds 22 closed-profile cases: two valid AS configurations and
  unchanged default reach only the original observer refresh, deliberately refused
  by wrong identity; 19 malformed/missing/wrong/ordinary/aliased/public inputs stop
  before HTTP/model work. Separate protected AS/representative/key files keep
  credentials outside driver JSON/console; no ordinary-token fallback qualifies.
  The initial inline pair exits 0 through session 44256 (2/2, 3.73 seconds).
  Bootstrap's exact new selector exits 0 through session 56201 (8.51 seconds).

  Source-consistent full Matrix exits 0 through original session 80812: 179 library
  plus 32 integration tests, 211 total, none ignored/filtered; library 178.03 seconds.
  Full bootstrap exits 0 through session 54664 with 13 tests (14.02 seconds).
  Affected Matrix/store/native all-target warnings-denied Clippy exits 0 through
  session 68030; Windows GNU all-target check exits 0 through session 5648.
  Compilation is not native Windows filesystem/runtime conformance. The binding
  inventory exits 1 through session 34659 with 941 selectors and the same two
  genuine physical completion/session-route selectors missing. Listing is not
  execution; agent-spec is absent and no parse/lint/lifecycle is claimed.
  All original records are retained in the external migration cache under
  `appservice-*-20260916.log`; the earlier compile failure is not relabelled.

  Actual read-only mini3 inspection remains running=true, zero restarts, 75
  completed dispatches, 74 Delivered replies and five retained unknowns. New source
  is not deployed; generation-11's failed soak/recovery remains untouched. This
  checkpoint does not implement the mandatory HS-authenticated AS transaction/user
  receiver or registration-file generation, nor either complete deployment profile.
  Genuine original inline retained runtime/handoff through the same owned launcher,
  Applied/Active/derived routes and configured real-ingress roster remain required.
  The current launcher needs Active dispatch scope and retains reactor-bound Unix
  pipes; initialize-and-stop or a second launcher cannot substitute for that work.
  Real fleet/two-agent factory, completion reliability/recovery/re-soaking, approval/
  media/lifecycle/API parity, effective sandbox/budgets/quota/retention, native
  platforms/upstream desktop and full M0–M9/release gates remain owed. The full goal
  stays active. No formatter, dependency, commit, PR, SDK reset, deploy/restart or
  cutover occurred; original hagency and Robrix2 source changes are preserved.

- 2026-09-16 continuation, retained task-context bridge checkpoint: the preceding
  goal turn was status-only, not implementation progress. Revalidated the actual
  unfinished changes and wired the same original owned execution path instead of
  treating account/SDK observations as full runtime readiness.

  Host's explicit opt-in now inherits only a closed fixed private reference and
  context marker; default direct capability/task ABI stays unchanged. After real
  acknowledged Started and actual initialize, the same owner awaits one retained
  create-only context job through filesystem return, binds the fixed typed helper
  once while Ready, then opens the thread. There is no second launcher, raw secret
  in protocol/receipt, generic policy setter, premature task or fake Applied/Active.
  Original IO reactor, app-server-only argv, scope, sandbox and cleanup/Done/reply
  settlement are preserved. Actual create descriptor is retained before writing;
  exact held root/file/bytes, original deadline and cancellation owner are pinned.
  Current DomainStore authority checks precede/follow the bounded physical work.
  Caller loss retains the actual original job; Busy/unknown/failed binding cannot
  rearm, and completed replay inspects only the same original custody.

  Review tightened the sealed writer Started marker to crate-internal validation.
  A current InProgress task is not an acknowledged Started receipt. The initial
  new refusal test incorrectly tried to reconstruct its scope through a getter
  which already refuses Started; its terminal failure is preserved. The corrected
  assertion proves that refusal without changing or bypassing the private marker.
  Exact post-fix Store test exits 0 through session 27619 with all three selectors.
  Runtime late helper exits 0 through session 59635; native closed startup grammar
  exits 0 through session 41385. Actual owned native MCP selector exits 0 through
  session 26771: initialize with absent record/reference-only parent, then original
  helper API read/heartbeat/readback/successful exit, including cached context after
  its startup file changes. This is an offline app-server peer plus real native
  MCP/DomainStore, not a live model or effective-sandbox/factory readiness proof.

  Source-consistent full Store/Runtime exits 0 through original session 45173:
  402 printed tests, 45 summaries, zero failed/ignored/filtered. Selected complete
  Native lib/bootstrap/owned_mcp/task_client targets exit 0 through session 49934
  with 30+13+6+8 = 57 tests, no failures or ignored/filtered tests. Earlier Native
  session 39539 remains failed: three actual initialize transport timeouts during
  an overlapping build/test run; cause is unproven. No deadline was enlarged or
  test serialized to force passing. Final four-package all-target warnings-denied
  Clippy exits 0 through session 21966; Windows GNU all-target compilation exits 0
  through session 8978. Compilation is not native Windows runtime conformance.

  The broader three-package attempt exits 101 through session 16428: Execution's
  75 ordinary tests pass, then the two real-sandbox evidence selectors reject the
  checked-in placeholder. They remain failed, not skipped; no live Cargo test or
  operator evidence rewrite was attempted. Separate current Store/Runtime run
  observes packages Cargo did not reach. Final offline binding inventory exits 1
  through session 66586 with 947 selectors and the same two genuine physical-
  completion/session-route selectors missing. Inventory is not execution;
  agent-spec is absent and no parse/lint/lifecycle qualification is claimed.
  Original terminal records remain in external `retained-context-*-20260916.log`.
  ADR-057 explicitly records the new private operational file extension; it is
  plaintext checked-private dispatch custody, not canonical agent credentials,
  an AS side master, encrypted SDK state, persistent grant or hostile-runtime secrecy.

  Next integrate actual pre-activation warm worker/reactor ownership through this
  same launcher and handoff, not initialize-and-stop or an unused warm handle.
  Transport lifetime currently starts at construction; Host uses original
  operation_ms as its lifetime, so warm idle cannot simply outlive it and later
  claim readiness. Its typed lifecycle must preserve original dispatch deadlines.
  Original enrollment SDK also needs current-Active scope handoff without reset
  or replacement before final Applied/Active/derived routes and configured ingress
  roster can be qualified. Mandatory native AS receiver/registration generation,
  both complete profiles, real fleet/two-agent, reliable completion/recovery and
  sustained real client re-soaking plus full M0–M9/parity/release gates remain owed.
  Read-only mini3 remains running=true, restarts=0, 75 completed dispatches,
  74 Delivered replies and five retained unknowns. Failed generation-11 soak and
  recovery remain untouched; this source is not deployed and full goal stays active.
  No formatter, dependency, commit, PR, SDK reset, deploy/restart or cutover occurred;
  original hagency dirty changes and Robrix2's existing headless build are preserved.

- 2026-09-16 continuation, actual retained pre-activation runtime: the prior
  status-only turn was no implementation progress; live read-only state and the
  original Gen10 passing/Gen11 failing terminal records were revalidated. The
  active eight-selector task now implements genuine warm owner consumption:
  one fixed native session/process/current-thread reactor waits pre-activation,
  then transfers its original worker/report into actual acknowledged owned dispatch.
  Original start/workspace ack/usage/private context/native helper/thread/turn and
  cleanup/settlement/negative finalization remain on that same execution path.
  No probe-only readiness, second launch/reactor, adoption or cold fallback.

  Original producer/fence/resource/account/registration scope and actual home/root
  are checked fresh. Its finite sticky producer registry prevents duplicate,
  foreign/reopened or failed warming. Typed pre-initialize opt-in allows only
  Ready/no-helper idle and one original absolute-deadline dispatch; idle helper/
  thread requests and normal/active extension/rearm refuse. Live capacity is held
  before possible spawn and transferred once. Genuine scope validation writes no
  Active/task/route authority; fixture activation is explicitly not factory proof.

  Initial check 56551 exits 0. Store 43538 exits 101: fixture wrongly expected raw
  unenriched roles and permitted reserved-profile editing. Correct actual normalized
  frozen-role and refused-public-write/corruption fixtures pass all three Store
  selectors through 89772. That combined handle ends 101 because the separate idle
  fixture wrongly expected request ID 2 rather than original sequence ID 1; corrected
  exact Runtime 66881 exits 0 without resetting connection or deadline. Initial
  Native 91998 exits 101 on incorrect existing API arguments/types. Native 67026
  and diagnostic 27815 remain failed: unstarted expiry actually Superseded, and
  approval-enabled startup lacked its required observed private approval room.
  The real existing guard refused binding at TurnStart; fixed fixture submits the
  actual Host observation, no verdict/grant or bypass. Native 44026 exits 0 (3 tests).

  Late-spawn capacity negative control 56038 exits 101: old shared deadline/cancel
  cleanup could free an uncertain live slot. Fix keeps possible late startup held
  until observed full stop, with unchanged bounds and known pre-child releases.
  Native 75415 exits 0 (4 tests). Initial Clippy 43466 exits 101 on shared startup's
  eight arguments; grouped private NativeStartup/Controls fixes it without lint
  suppression. Source-consistent final Native 58854 exits 0: 30+13+6+8+4 = 61 tests,
  five summaries, zero failed/ignored/filtered. Same PID/one initialize spans actual
  idle past initial IO lifetime and Started/workspace-ack/native MCP heartbeat,
  including two held live slots and the actual associated managed account.

  Full Store/Runtime 62853 exits 0: 406 printed tests, 44 all-target summaries, zero
  failed/ignored/filtered. Final warnings-denied four-package all-target Clippy
  73013 exits 0. Full Execution 43945 and grouped source 23221 both exit 101:
  76 pass and the two original real-sandbox placeholder evidence gates fail;
  none ignored/filtered. Lint/build briefly overlapped Store/Runtime; its terminal
  result was passing, not abandoned/restarted. All original logs are retained in
  external warm-owner-*-20260916.log. No live Cargo test/model, increased deadline,
  forced serialization or passing lifecycle claim; agent-spec remains unavailable.

  ADR053 records the explicit pre-activation scope exception and capacity correction;
  ADR147 records source progress, not completed factory. Next implement the full
  original inline home/account/rooms/enrollment/warm runtime owner and same SDK's
  current-Active handoff for both complete profiles, then genuine Applied/Active/
  derived routes and configured roster/fleet/two-agent qualification. Mandatory AS
  receiver/registration generation, actual sandbox, completion/recovery reliability,
  sustained real Palpo/Robrix re-soaking and full M0–M9/parity/release remain owed.
  Warm ready is initial IO evidence, not perpetual liveness or model qualification;
  completion still needs fresh physical/current-owner proof. Neither original
  missing provisioning selector is withdrawn or given synthetic authority.

  Read-only mini3 again has running=true/restarts=0, 75 completed dispatches,
  74 Delivered replies and five unknowns. Failed Gen11 soak/recovery remains failed;
  these changes were not deployed or re-soaked. Original hagency dirty entries and
  Robrix2/headless build are preserved; no dependency, formatter, commit/PR, reset,
  service restart or cutover occurred. Full goal remains active.

  Final grouped-source Windows GNU all-target check 36527 exits 0: compile only,
  not native Windows execution. Final offline all-feature/all-target binding
  inventory 79315 exits 1 with 955 selectors and the same two missing full-factory
  selectors (native_provisioning_effect_completed/native_provisioning_session_route).
  All eight warm-task selectors are registered, not listing-qualified as execution.
  Agent-spec is absent, no parse/lint/lifecycle claim. All known task handles are
  terminal/closed; tracked source diff-check and new source whitespace checks are
  clean. The full goal is not complete or blocked by this source checkpoint.

- 2026-09-16 continuation, original enrollment SDK Active consumption: the prior
  user-status turn was no implementation progress; it revalidated original
  terminal Gen10/Gen11 records. The new five-selector active handoff contract
  now consumes the original opaque account's successful enrolled Collector/SDK,
  without configuration/credential export, second owner/store, fresh identity,
  signing upload/session claim or reset. Its separate read-only writer validator
  requires the original acknowledged Started effect's exact fence/payload to now
  be actually Complete, engagement Active and registration current. Existing
  Started/Reserved enrollment stays strict; no Applied or route is synthesized.

  Same original SDK queue/owner, protected Agent Complete, busy permit and absolute
  admission deadline span collection/Complete verification/current writer checks.
  Actual whoami/project binding/owner/powers/Direct privacy/recipient inventory and
  AS namespace authority are freshly read. Lost waiter retains original job/result;
  failed/running/closed jobs cannot rearm or move backwards. Post-collect failure
  fences only its actual original positive transport. Actual Weak SDK channel
  identity and settled Weak teardown prove same owner and no job/owner Arc cycle.
  Original AS home/account/rooms/SDK also hands off this same owner; revoked or
  broadened side master refuses despite the private dedicated device still live.
  AS registration/login, room, signing and claim effects stay exactly original.

  Initial51809 exits101 (production used cfg-only convenience method); fixed
  existing Agent-purpose Handle stays on the original SDK queue. Initial80946
  exits101 (assertions needed absent private Debug); fixed equality/cancel checks
  add no Debug projection. Corrected5814 and final Weak teardown5336 exit0,
  three actual Matrix selectors. Both-kind81365 exits0, four Matrix selectors,
 179 filtered/zero failed or ignored. Store85957 exits0, one selector/no filters.
  The first full invocation misspells --no-fail-fast and exits1 before execution;
  original log retained. Correct full46889 exits0:552 printed tests/45 summaries,
  zero failed/ignored/filtered (Matrix214/6;Store338/39). That combined run precedes
  test-only Weak/AS extensions; final current-source Matrix all-target follows
  separately. Store source remains identical. Actual original still-live process
  sampling showed recipient crypto work; its same full handle later exited0,
  not abandoned/restarted after a long observation. No forced serialization or
  deadline enlargement. Distinct sdk-active-*-20260916 logs remain external.

  Three-package warnings-denied all-target Clippy14292/65168/25000 exits0;
 25000 includes final both-kind fixture source. Windows GNU all-target checks
 39351/63968 exit0:compile only, not native execution/release (Windows paused).
  First all-feature binding inventory41333 exits1:959 selectors, same two missing
  native_provisioning_effect_completed/native_provisioning_session_route. Final
  both-kind inventory43078 exits1:960 selectors, same two missing factory selectors.
  Final current-source Matrix58074 retained log is terminal:214 passed/one failed,
  zero ignored/filtered, one failed target. The original-deadline account test
  returns OutcomeUnknown after two registration requests before its strict peer
  script receives the third expected request; panic is token_provision.rs:68.
  Its 750ms budget includes two scripted 300ms response delays before that request;
  the precise intervening cause remains unproved. No deadline increase, forced
  serialization, test change or rerun erases the failure. Final Matrix stays red
  despite the earlier passing full run. The handle is unavailable to poll; the
  authoritative retained terminal log supplies the result, not an observation
  timeout. No listing-qualified execution or unavailable agent-spec lifecycle.

  Readonly mini3 still running=true/restarts=0,75 completed dispatches,74 Delivered
  replies,five unknowns. No deploy/re-soak. Existing developerInstructions already
  explicitly require canonical completion; do not invent missing guidance as the
  failed Gen11 root cause or fabricate Done from ordinary ACK. Genuine full inline
  home/account/rooms/enrollment/warm owner consumption/fresh physical proof,
  Applied/Active/derived routes for both profiles and mandatory native AS receiver/
  registration generation, configured roster/fleet/two-agent, actual sandbox,
  completion/recovery reliability, sustained real client re-soaking and full
  M0–M9/API/parity/budgets/quota/retention/platform/release/cutover remain owed.
  No original factory selector withdrawn or given synthetic authority, new marker,
  dependency, formatter, commit/PR, reset/deploy/restart/cutover or original source
  edits. Original hagency dirty entries/Robrix2 headless artifact preserved; full
  goal remains active, not complete or blocked by this source checkpoint.

- 2026-09-16 continuation, fresh original warm owner: the previous goal turn
  observed/recorded the final Matrix failure, progress but not full qualification.
  Six-selector task-rust-warm-current-owner.spec.md now requires fresh physical
  observation rather than cached initialize or absence of Stopped. Unix correlates
  actual guardian/retained Scope observations by strictly increasing private nonce;
  Windows checks its original process handle. Unknown cannot rearm; only an exact
  late reply drains before original cleanup. Linux independent cgroup cleanup is
  not replaced by peer reports. No PID authority/new launcher/configuration export.

  Same worker/reactor/session owns a bounded Ready inspection through caller loss
  and its original absolute response/idle deadline. Final writer/home/account
  checks follow physical IO; expired buffered positives cannot become readiness.
  Idle/handoff also qualify; consumed dispatch closures leave the cancellable wait
  before inspection awaits, retaining original negative finalization on failure.
  First failure stays sticky through later cancellation/teardown. No Applied/task/
  approval/route write, default sandbox change, second initialize or cold fallback.

  Initial12532 exits101 (cleanup arm type), fixed without teardown changes.
  Guardian55014 exits0:8 printed selected tests. Native36905 exits0:5 before
  new custody coverage. Native94357 exits101:peer acknowledgment metadata absent
  when read; actual host flush is not peer-read proof. Fixed fixture waits for
  completed JSON under its existing4-second bound. Corrected44532 exits0:6 tests.
  Later guardian42891 exits101 (unit2 pass;guardian1 pass/5 fail startup/channel/
  child-exit bounds); native28913 exits101 (1 pass/5 fail startup/initialize bounds).
  Exact cause remains unproved. Diagnostic59495/12414 exit0:6 each at unchanged
  original budgets, not a clearance of failures or proof of load qualification.
  Settled unknown19282 exits0:1 private-protocol selector/five negative variants,
  explicitly scripted not factory proof. Sticky19570 exits0:1 original-result test.
  Actual native owner-exit/held-writer waiter-loss/expiry selectors pass separately.

  Full four-package all-target48832 exits101:380 printed passes/11 failures,
  52 summaries,zero ignored/filtered. Six red targets:console roster pending
  provision, two-agent placeholder, received-file4 initialize timeouts, release
  cutover missing4 template inputs/poisoned mutex, execution2 sandbox placeholders,
  platform fatal-truncation exit101 vs125 (cause unproved). Its warm target passes6;
  final sticky/late-wrong extensions have separate current checks. Strict all-target
  Clippy19412/18545/26368 exits0;26368 covers settled source. Windows GNU34971/90930
  exit0:compile only,not native execution/release; Windows paused. Inventory62617
  exits1:966 bindings,two original missing factory selectors plus sticky introduced
  after library compilation. Settled inventory6905 exits1:966 bindings and only
  the two original factory selectors missing. All6 current-owner selectors are
  registered and separately executed above, not listing-qualified. All known
  task handles terminal/closed; selected diff-check/new-source whitespace clean.
  Agent-spec absent,no parse/lint/lifecycle claim.

  Readonly mini3 still running=true/restarts=0,75 completed dispatches,74 Delivered
  replies,5 unknowns. No deploy/reset/restart/re-soak or original source edits.
  Original dirty hagency entries/Robrix2 artifact unchanged. Genuine full inline
  original home/account/rooms/SDK/warm consumer, Applied/Active/derived routes for
  both profiles, native AS receiver/registration generation, configured roster/
  fleet/two-agent,effective sandbox,completion/recovery and real sustained client
  soaking remain owed. Upgrade source-copy/fingerprint must include the4 actual
  shared templates, not substituted bytes. Full M0–M9/parity/budgets/quota/retention/
  platform/release/cutover remain open. No factory selector withdrawal,new marker,
  dependency,formatter,commit/PR or cutover; full goal remains active. Next connect
  genuine original factory consumption/activation/routes, not a substitute profile.

- 2026-09-16 continuation, genuine original factory: the previous status-only
  turn was no progress. task-rust-inline-factory-runtime.spec.md/ADR053/147 now
  connect the original physical home/account/rooms/enrolled SDK to one fixed
  Host-only WarmHostPlan/FactoryRuntime and the same initialized original owner.
  The explicit internal Matrix -> execution dependency is approved; no external
  crate/version, launcher, exported secret/config, synthetic target activation,
  changed deployment/checkpoint marker, cold fallback or changed budgets.
  Original worker activation performs fresh physical/current checks and exact
  original writer transaction/ACK before Active SDK handoff and actual created
  encrypted owner-DM/null-root session/workdir routing. Lost ACK cannot authorize
  replay/dispatch. First dispatch consumes the same initialized owner exactly once.
  Store provision_runtime4 passes separately, admission-only not physical proof.

  Initial offline Native4 failures were traced to the independent owner join
  client's wrong placeholder origin, fixed to the actual endpoint at unchanged
  bounds. Then3 passed/first dispatch LostAuthority at TurnStart; ordinary owned
  scope sampler stayed current. Separate approval binding was missing and copied
  fixture room had another project. The product now requires its original fixed
  approval Collector, validates same writer scope before launch, and authenticates
  target binding after exact ACK before routing. Fixture seeds only the unrelated
  external bootstrap on its preexisting typed managed project; no target rows.
  Foreign writer refuses before initialize; failed post-ACK membership retains
  Active history/no route. Final Native10 passes in9.23s,zero ignored/filtered,
  including both account kinds' activation/routing/first real native helper use,
  privacy refusals, foreign writer, intake/shutdown waiter loss and both Busy
  paths. Coordinator closure retains original permit/jobs through caller loss,
  closes each dispatch admission before awaits and drains all original owners;
  partial aggregate failure stays Unknown with individual custody retained.
  PoisonError<MutexGuard> Send compile failure is fixed by mapping before await.
  No forged Done/whole-tree proof/unknown lease release; macOS remains conservative.

  Upgrade build copy and fingerprint now include original role JSON plus all4
  shared entry templates, no substituted bytes. New unit covers each template's
  copy/invalidation. Initial release9 passes including actual local N/N+1 state
  continuity/rollback; later exact-current-N restore also covers child/staging
  failure, not an old same-version backup. Current settled five-package all-target
  no-fail-fast75472 remains running and will have separate final evidence.
  Agent-spec absent,no parse/lint/lifecycle claim; selected whitespace checks clean.
  No formatter/commit/PR/original source edit/reset/deploy/restart/re-soak/cutover.
  Native configured fleet/roster/two-agent/approval pump, both complete ADR016
  deployment profiles/AS receiver/registration generation, real effective sandbox,
  canonical completion/recovery/re-soaking and full M0–M9/API/parity/budgets/quota/
  retention/platform/release/cutover remain owed. Full goal remains active.

- 2026-09-16 continuation: previous status-only turn was no progress. Resolved
  prior unobserved dispatch-owner test:11 pass; five-package strict Clippy passes.
  Prior full75472 actually ended884 printed passes/four failures/zero ignored.
  It is no longer running; its red roster/two-agent/two-sandbox evidence remains
  recorded separately from later checks.

  The real authenticated console roster regression now runs the original inline
  physical factory for both account kinds, reads the target minted by ingress,
  verifies Applied/Active and its actual DM route, and preserves exact seven-key
  privacy. Removed the old injected approval-room/unconfigured-factory fixture.
  Native startup/fleet integration is still required.

  Fixed actual factory-agent approval rejection: the original bot now retains
  scope-validated new membership under its existing busy permit and64-member cap.
  All observation/intake/enrollment/send/close consumers use that same bounded
  membership. An opaque original writer scope and successful post-ACK private
  observations are required; no public setter, SDK replacement, grant or fallback.
  New red regression reproduced Config after actual enrollment; positive tests
  now enroll the bot before agent creation, preserve its keys/claims, deliver an
  exact encrypted card to the independent owner for both account kinds, preserve
  pending permission, and cover original caller loss. A held private read plus
  canonical revocation refuses membership/routing. The test checks canonical
  Cancelled/fence+1 plus unchanged Applied digest, not an incorrect expectation
  that end() leaves the effect state Complete.

  Final source:Matrix216 pass; console56 plus factory13 pass; five-package
  all-target Clippy passes with warnings denied. Existing stable Windows GNU
  all-target check passes (compile only). Binding inventory lists984 Rust
  selectors with none missing (listing only). Separate earlier Native83 includes
  actual local release9 and warm6 passes. One intermediate console run hit
  OutcomeUnknown during provisioning; cause unproven, original failed log retained.
  Later settled runs pass at unchanged budgets without test-thread overrides.
  All task handles terminal; diff/new-source whitespace checks clean.
  agent-spec is absent; no lifecycle/parse/lint claim.

  Readonly mini3:running,zero restarts,75 completed/74 Delivered/five unknown.
  No deploy/restart/reset/re-soak, formatter, dependency addition, commit/PR or
  cutover. Original hagency and Robrix2 changes are preserved. Actual configured
  fleet/repeated dispatch/approval-verdict pumping, full profiles/AS receiver and
  registration generation, effective sandbox, canonical completion/recovery,
  real two-agent and sustained Palpo/Robrix soaking plus all M0–M9/parity/release
  requirements remain unfinished. Full goal stays active.

- 2026-09-16 native approval round trip: the preceding status-only goal turn was
  no progress. Current source confirmed that PC-C0 sent cards but neither enrolled
  the approval SDK nor polled owner verdicts. Added the bounded roundtrip task
  contract and ADR064/112 amendments, then reproduced the fresh executable gap:
  no enrollment writes, card or verdict poll. No live state was changed.

  Startup now drives the original separate bot's observe/enroll path before any
  driver launch while continuing to poll its real HTTP server. Existing Complete
  enrollment is reused without key/session regeneration. This is Matrix crypto
  enrollment, not ADR114 provider login. The original single-consumer run notices
  feed at most64 pending IDs into the same ApprovalCollector. The serial250ms
  polling cadence preserves all existing authority/custody/capacity/deadline
  checks; a refusal or unsettled batch stops the pump without automatic retry.
  Bootstrap retains the original approval owner across failed close observations.

  Actual native child tests start without approval SDK/binding, drive real TLS
  key publication and independent-owner crypto, decrypt the card, return exact
  encrypted approve-once/deny verdicts and observe the matching actual callback
  in the pinned offline probe. Plaintext first leaves the request pending; only
  the subsequent encrypted verdict admits a receipt. Wrong actual owner anchor
  prevents dispatch. A held enrollment response proves HTTP remains responsive.
  The older pre-enrolled test preserves five writes/one claim. A real missing
  domain-authority close failure proves the same Arc and database locks remain
  across two failed closes; it does not claim positive SDK shutdown evidence.

  Final approval selection passes1 unit plus5 executable tests. The prior broader
  native run passed16 bootstrap and56 console but failed in a factory diagnostic
  that read nullable terminal deadlines as bools. Preserving Option<bool> in that
  read-only diagnostic lets the actual report be observed; all13 factory tests
  then pass, without product changes or increased budgets. Separate release9 and
  warm6 pass. Matrix216 plus runtime69 pass (285 total,zero failed/ignored).
  Five-package all-target warnings-denied Clippy passes. Windows GNU all-target
  native compilation passes; it is not native Windows execution. Production
  caller checker unit tests24 pass. Final all-feature Rust binding inventory
  lists988 selectors with none missing (listing only, not test execution).
  All task handles are terminal; tracked/new-spec whitespace checks are clean.

  Logs are native-approval-roundtrip-* under the existing private live evidence
  root. Initial fixture failures (raw crypto API spelling, JSON-encoded choice,
  invalid literal public anchor) remain in their own logs; final fixtures use
  the pinned raw API, decode the stored JSON enum and generate a real public
  anchor. Atomic fixture receipt publication avoids partial-file observations.
  agent-spec is absent; no parse/lint/lifecycle claim. No formatter, dependency
  addition, commit/PR, deploy/restart/reset/re-soak or cutover. Original hagency's
  six dirty entries and Robrix2's headless artifact are preserved.

  This closes the configured single-run service approval wiring gap, not fleet
  composition, real Robrix approval clicks or full port readiness. Complete
  profiles/AS receiver/registration generation, repeated/multi-agent dispatch and
  workspace ownership, effective sandbox, reliable canonical completion/recovery,
  real two-agent and sustained Palpo/Robrix qualification, all M0–M9/API/parity/
  budgets/quota/retention/platform/release/cutover requirements remain owed.
  Gen10's narrow hour and Gen11's failed cycle37 are unchanged. Full goal active.

- 2026-09-16 factory sequential ownership and Linux physical qualification:
  removed the permanent first-dispatch limit under the new sequential task
  contract. Admission is spent in the original runtime before blocking enqueue.
  Only the original operation's private successful settlement/full-stop/usage
  observation, received result and joined worker admit distinct later work.
  Public Report mutation, pending work, failed handoff and caller loss cannot
  manufacture continuation. Subsequent Started launches retain the same Host,
  writer, physical roots/account, approval budget and enrolled Matrix SDK, with
  direct fresh task/helper binding instead of reusing the first warm context.

  Actual Linux testing exposed a separate preexisting factory blocker: home
  copying fsynced cap-std's O_PATH descriptor, failing after the real file copy
  and before account registration. Task-context publication had the same bug.
  Added a bounded directory-sync contract and reused the account store's existing
  readable same-object directory adapter internally. No sync error is ignored,
  pathname reopened, partial home repaired, authority fabricated or budget raised.
  The Linux unit observes the original EBADF, positive real sync, namespace
  replacement isolation and refusal when the retained directory becomes public.

  Post-fix isolated Linux: all15 factory tests pass, including two distinct
  physical dispatches for BOTH account kinds; helper readback names task1 then
  task2, only one warm context exists, original SDK writes/claims do not repeat,
  and actual whole-tree cleanup releases leases. These are heartbeat/readback
  tasks, NOT canonical Done/final-reply or real model/sandbox qualification.
  Separate Linux sync1, task-context3 and account11 tests pass. All containers
  have network disabled and synthetic state only; live state/credentials are
  never mounted. Source hashes match the local worktree. The existing offline
  rust:1.94-bookworm image used installed Rust1.94.1 (workspace minimum1.94),
  not the local pinned1.95 compiler; this is explicit Linux execution evidence.

  Post-fix macOS: bootstrap16 and console56 pass; a simultaneous-build factory
  run had10 passes/five provisioning failures (Domain/OutcomeUnknown), retained
  in its original log with cause unproven. With unrelated build checks finished,
  an unchanged default-concurrency run passes factory15, owned-MCP6 and warm6.
  macOS factory continuation remains conservative refusal without full cleanup
  proof. Sync1/context3/account11 pass. Four-package all-target strict Clippy
  and Windows GNU all-target check pass (Windows compile only). Final995 Rust
  spec bindings resolve with none missing; listing is not execution. Earlier
  pre-directory-fix library225, execution39 and release9 passes remain separate.

  Logs use factory-sequential-* under the existing private evidence root. Initial
  Linux source-copy missing static include, parallel-link OOM, and15/15 factory
  failures are retained, not called passes. Build jobs were reduced to1, not
  test concurrency or deadlines. A task-created partial build-cache volume filled
  the20GB Docker disk; removed only that verified unused disposable copy, then
  used a host-disk target directory. The original cache remains; disk restored
  to2.7GB free. Live container remained running,zero restarts/no OOM; readonly
  state remains75 completed/74 delivered/five unknown. Original hagency's six
  dirty entries and Robrix2's headless artifact are unchanged. All handles are
  terminal. No formatter, dependency, commit/PR, deploy/restart/reset/re-soak.
  agent-spec is absent; no parse/lint/lifecycle claim.

  This is concrete progress, not full port readiness. Configured native fleet
  startup/scheduling and multi-workspace integration, complete profiles/AS
  receiver/registration generation, real sandbox/two-agent evidence, canonical
  completion/recovery reliability and sustained real Palpo/Robrix qualification,
  remaining M0–M9/parity/budgets/quota/retention/platform/release/cutover gates
  remain. Gen10's narrow one-hour pass and Gen11's failed cycle37 are unchanged.

- 2026-09-16 fleet dispatch isolation and stop-custody correction:
  replaced the bootstrap's single workspace slot with at most16 original Started
  bindings. Exact full-capability selection retains one entry before the writer
  wait; exact release retires held guards without affecting other entries. No
  source-root setter, replacement writer or lock held across await is introduced.
  Three real Started-operation regressions cover distinct roots with identical
  filenames, foreign capabilities, individual/global retirement and capacity.

  Native final delivery now authenticates its original completed dispatch/fence
  against the historical attempt and claims only that dispatch's current pending
  intent. It cannot consume another agent's older reply or restore execution.
  Two-agent store regressions cover all capability fields, unpublished completion,
  expired execution, retired routes, original claim renewal and uncertain-send
  refusal; actual native MCP/Matrix fixtures use the same scoped API.

  Further review found the old global stop sweep unsafe, not merely incomplete:
  a real original-operation regression returned one pre-child failure while
  another original Started operation was held, and the production finish path
  settled BOTH stops (expected two pending, observed zero). Removed that sweep;
  negative scopes retain pending rows, dirty roots and leases. Original successful
  completion publication/delivery and explicit operator recovery are unchanged.
  The former direct-store-string fixture proved no physical inspection. G8 is
  explicitly reopened in ADR130/146 and its positive exact stopped-owner/workspace
  inspection selector remains owed; the real negative test replaces the invalid
  positive claim. This is not completion of automatic stop/recovery behavior.

  Current verified local results: store owned-completion8/replies17; native
  library34, bootstrap16, owned-Matrix3 and owned-MCP6; strict all-target store/
  hagency Clippy; Windows GNU all-target compilation (not execution). Production
  caller checker44/43 wired/one owed G8, no missing; checker units24. All-feature
  Rust spec inventory1000 with no missing selectors is listing only. agent-spec
  remains absent, so no parse/lint/lifecycle result is claimed. Initial fixture
  compile errors (private completion receipt fields/revoke signature) are retained.
  The earlier workspace fixture's max-live1 failure was corrected to17 to test
  the16-entry registry; production limits and operation deadlines were unchanged.

  Isolated Linux store25 pass. Its large library test link was OOM-killed even
  with build-jobs1; the failed container/log remain. Rebuilding only that test
  binary with GNU ld --no-keep-memory succeeds with the same test profile/debug
  assertions and all34 actual tests pass. No test filter, serialization or deadline
  change. Seven changed-source digests match the isolated mini3 snapshot. Linux
  executable integration now passes bootstrap17, owned-Matrix3 and owned-MCP6,
  including actual whole-tree cleanup, scoped final intent claim and SDK send.
  Local console56 and the full hagency-store package regression also pass with
  no failed/ignored tests. All original handles are terminal. Logs are fleet-isolation-* and
  fleet-stop-custody-* under the existing private live evidence root.

  No deployment, restart, reset, new soak, dependency, formatter, commit/PR or
  cutover. Read-only inspection confirms the live service still runs with zero
  restarts/no OOM. Original hagency's six dirty entries and Robrix2's headless
  artifact remain unchanged. Gen10's one-hour narrow pass and Gen11's failed
  cycle37 are unchanged; latest source is not yet live-qualified. Full configured
  fleet startup/scheduling, per-agent file routing, approval multiplexing, exact
  stop recovery, complete profiles/AS receiver, real sandbox/two-agent/client
  approval and sustained soaking, plus all remaining M0–M9/parity/budget/quota/
  retention/platform/release/cutover obligations remain. Full goal stays active.

  Next actual service wiring still needs to construct the configured warm factory
  with the original approval collector (Shared is currently constructed first),
  consume original successful ProvisionedAgent handles into recurring per-agent
  drivers, route file/receive work to each original enrolled SDK, multiplex approval
  receivers through the single approval SDK, and close retained factory owners.
  No full-profile config path currently calls with_warm_runtime. The old driver's
  global max-live1 cannot be mistaken for tested multi-agent scheduling. G8 needs
  exact inspected original stop custody, never restoration of the removed sweep.

- 2026-09-16 fleet approval service composition:
  the pump now fairly multiplexes up to16 original operation notice receivers
  through its existing one-slot handoff, instead of draining one until closure.
  Closing one source preserves the rest. Original pending-ID/card/journal bounds
  remain unchanged. Due verdict polling has priority, with cadence reset after
  actual IO so a slow poll cannot starve new cards. The original approval collector
  supplies a bounded host scheduling turn shared by pump initialization/send/
  intake/close and genuine factory membership observation. It never replaces
  original SDK/domain busy permits, sticky jobs, current checks or unknown custody.

  Further inspection found the driver's old try_send could discard a simultaneous
  agent's receiver even after multiplexing. It now reserves handoff capacity
  BEFORE claiming any task, waits cancellably without task/workspace authority,
  and transfers the original receiver synchronously after Operation creation.
  No waiting after Started can consume its workspace-ACK budget. An actual
  driver/Matrix regression holds capacity and proves zero claims on full/cancelled/
  closed paths; releasing capacity admits only its original next claim. Six new
  scheduling/custody selectors cover idle/fair/closed receivers, finite capacity,
  dropped waits, exact scheduling timeout, held original TLS through caller loss,
  and pre-claim handoff. Pure channel/virtual-time results are not crypto proofs;
  existing executable encrypted-owner callbacks and physical factory tests remain.

  Final local results: full Matrix219, native lib37/bootstrap16/inline-factory15/
  owned-MCP6/warm-runtime6 (80), strict all-target execution/Matrix/hagency Clippy,
  and Windows GNU all-target compilation. Windows remains compile-only. A prior
  full Matrix run failed the750ms account deadline fixture before its expected
  third request; isolated unchanged execution passed. Its three300ms delays now
  use controlled Tokio time with actual TLS/encrypted files and a real3s watchdog,
  preserving all750ms production/deadline/custody semantics. The first clocked
  fixture also exposed zero-delay peer timers stalling frozen time; explicit
  zero-delay writes now need no timer, with positive delays/gates unchanged.
  Both initial failures/logs remain; this is not a physical-latency qualification.

  Linux before the final pre-claim change passed native36/bootstrap17/factory15/
  owned-MCP6/warm6 (80) on actual1.94.1; final-source Linux validation is ongoing.
  The isolated source/cache uses no network or live credentials/state. No deploy,
  restart, reset, re-soak, formatter, dependency, commit/PR or cutover. Native
  configured factory/fleet startup, recurring per-agent original runtimes/SDKs,
  file/receive routing, exact G8 recovery, full profiles/AS receiver, effective
  sandbox, real two-agent/client approvals, canonical completion/recovery reliability
  and renewed sustained Palpo/Robrix soaking plus remaining full parity gates stay
  required. Gen10's narrow hour and Gen11's failed cycle37 are not changed.

  The broader Linux Matrix library run then terminated180 passed/7 failed: every
  failure was in nested home/AS provisioning, before registration. Isolating
  native_provisioning_inline_home reproduced it, and a new actual nested-copy
  regression failed with Io(EBADF). The copier was calling chmod on cap-std's
  O_PATH child-directory descriptor. It now creates each child with private0700
  mode atomically, retains/checks its handle and syncs each real nested directory
  rather than repeatedly syncing only the copy root. Windows default builder and
  private-parent checks are unchanged; no sync/permission error is suppressed.
  The new direct fixture passes locally, as do all14 original physical home/room/
  AS selectors. Linux direct/physical verification and final store/lint/compile
  regression are underway; the prior180/7 result remains failed. The failed chain
  never reached its queued token tests or final native library build.

  Final nested-copy source: Linux's direct regression passes after its actual
  EBADF red result; all14 physical home/room/AS selectors and the3 new approval
  service-turn selectors pass. Matrix integration32 (approval vectors/media
  download/media upload/token accounts/transport) passes on Linux. The first
  integration attempt failed two oracle tests because the isolated snapshot
  omitted retained bridge/route/schema files; copying those exact source files
  fixed the qualification environment without regenerating/reblessing vectors.
  That failed chain/log also remains. The final native Linux library was freshly
  rebuilt with the observed JSON artifact, and its37 plus executable integrations
  are currently running; no earlier cached executable is substituted.

  Final local post-copy results are full store343 across39 nonempty targets,
  native80, and the14 physical Matrix selectors. Strict all-target store/execution/
  Matrix/hagency Clippy and Windows GNU compilation pass. Spec binding listing1015
  has no missing selectors; caller inventory retains44/43 wired/one owed G8.
  agent-spec and the provisioned task-writer remain absent, so no lifecycle or
  canonical runtime-task-state result is claimed. Eleven changed Rust source
  hashes match the mini3 snapshot. Actual Linux compiler is1.94.1. The broad
  Matrix180/7 run remains failed; targeted corrected passes are not a new full
  Matrix run. Live service is still running with zero restarts/no OOM; original
  hagency's six dirty entries and Robrix2's headless artifact are preserved.

  Final native Linux job is now terminal exit0: lib37/bootstrap17/inline-factory15/
  owned-MCP6/warm-runtime6 (81) all pass on the current source, including original
  encrypted approval callbacks, handoff backpressure and both account-kind
  sequential dispatches with real Linux whole-tree cleanup. All task handles are
  terminal. Logs are fleet-approval-* under the existing private evidence root.
  No real-soak result changed. Next implementation is configured native factory/
  fleet startup and recurring original-agent service composition, including
  per-agent file/receive routing and exact shutdown/stop custody; the approval
  service prerequisite does not itself enable that fleet. Full goal stays active.

- 2026-09-16 configured original-agent service composition:
  the prior status-only turn was no implementation progress. This continuation
  adds explicit continuous `factory_service` composition under
  `inline_factory_service_checkpoint_v1`, requiring a home/rooms provisioner,
  configured approval bot, finite warm idle bound and actual intake session plan.
  Existing checkpoint markers retain their behavior without this additional field;
  neither complete ADR016 deployment profile is advertised. Bootstrap constructs
  the approval collector once and attaches the SAME original collector and shared
  eight-live/two-parked ApprovalHost to its original configured warm provisioner.

  The finite service discovers only successful original retained jobs and takes
  each non-cloneable ProvisionedAgent once, independently of the coordinator's
  later intake. Recurring drivers call that original agent's dispatch path; the
  first warm process is consumed and follow-up admission retains the private
  completion witness. No ordinary replacement Host, SDK reset or Active-row
  reconstruction. Each agent gets its own workspace and file/receive owners;
  root and factory claims share max-live8 only in explicit fleet mode. Factory
  unmanaged HOME/CODEX_HOME now use its actual managed home instead of inheriting
  a coordinator's shared directory; managed-account preparation still overrides
  these with its own original private account custody.

  File routes use a bounded full-original-attempt writer read to select the
  correct backend, including historical reads after workspace release. This
  does not restore execution or file authority; original current/historical
  backend checks still execute. File limits/tools are fixed before warm launch,
  with private per-agent media namespaces and original enrolled SDK sharing.
  Shutdown quiesces all agents before draining, continues through other owners
  after an individual failure, retains partial/unknown owners and closes factory
  custody before the coordinator/writer. Readiness exposes a stopped/failed or
  unstarted fleet even when the coordinator is healthy. Discovery cannot restart
  after its original run has been consumed.

  New offline selectors cover closed configured startup, original factory
  discovery, service-driven sequential claims/native MCP, historical two-engagement
  authentication and actual workspace/file-worker shutdown isolation. The real
  helper exercises current received-file listing and send/get of a deliberately
  missing source through the original fleet backends. That proves routing and
  source refusal, NOT a successful upload/download. Both actual account kinds
  run; Mac retains unknown whole-tree cleanup and does not run the second task.
  This is not a simultaneous two-agent or fully configured executable/client
  provisioning-to-reply proof. Those remain required, together with AS receiver/
  registration generation, completion/recovery, G8, effective sandbox, approvals,
  media delivery and sustained real Palpo/Robrix soaking and full remaining parity.

  Pre-final local native84 and strict four-package Clippy pass. The earlier
  fixture failures remain: invalid synthetic Agent name, test-future borrow
  lifetime, incorrectly expected positive Mac close, mismatched snapshot limit,
  OsString/path-alias comparison, and a lint refusal. Linux actual1.94.1 passes
  store routing3 and bootstrap18. Its broad factory run passes16/fails1: the
  recurring service completed both tasks, but the fixture compared the first PID
  against a latest-stage record overwritten by the second runtime. It now captures
  the first actual PID during execution before subsequent authenticated refresh.
  The initial Docker invocation also failed before Cargo because a login shell
  removed its PATH; both failed containers/logs remain. Current-source Linux
  integration and exact-artifact library qualification are running with network
  disabled, isolated source/target and no live credentials/state mounts.

  Caller inspection initially lost the common startup path in its simple
  same-file associated-function resolution. The actual common OS-owner startup
  is now a module-private helper, retaining identical runtime/control ownership;
  caller audit again resolves44/43 wired with only original G8 owed. No checker
  weakening, new test authority or false automatic stop settlement. agent-spec
  and task-writer remain absent; no lifecycle/canonical-task-state claim.
  No live deployment, reset, restart, new soak, formatter, dependency, commit/PR
  or cutover. Gen10's narrow passing hour and Gen11's failed cycle37 remain
  unchanged. Latest source is not live-qualified; full goal remains active.

- 2026-09-16 configured-service final offline observation: the original final
  Linux library owner is terminal exit0, no OOM, with38/38 passing. Together with
  bootstrap18, inline-factory17, owned-MCP6 and warm-runtime6 this is85 native
  tests on actual1.94.1. Local final native84, full store344, strict four-package
  all-target Clippy and Windows GNU compilation pass. All16 changed Rust hashes
  matched the isolated Linux snapshot. The spec binding inventory lists1020 with
  no missing selectors (listing is not execution); production caller audit
  resolves44/43 wired with original G8 still owed, no missing/ambiguous/unresolved.
  Original failed runs remain failed. These results precede the new configured
  executable two-agent fixture and establish no live deployment or new soak.

## 2026-09-16 configured executable and shared project-room generations

- Added `task-rust-fleet-executable.spec.md` and an actual `serve --agent-driver`
  integration fixture. Only an unrelated coordinator is preexisting fixture
  state. Each of the two targets is genuinely admitted from authenticated
  reception request/verdict, physically materialized, registered, joined,
  crypto-enrolled and initialized by the configured service. Independent owner
  encryption enters actual SDK/intake; no target tasks or Started rows are seeded.
  The two native helpers must both hold actual Started work before either is
  released to complete through MCP/API. Exact canonical IDs bind Done, final
  replies and independent recipient decryption. A second round per agent checks
  recurring original factory custody without re-registration or key upload.

- The first full fixture exposed a real same-project bug: the second physical
  join changes membership while both original Collectors remain pinned to room
  generation1. Intake correctly returns Conflict/Generation and fences custody.
  This failed run remains in `fleet-executable-linux5.log`; it was not a pass or
  a reason to put the agents into separate projects. Earlier fixture failures
  (module path/type inference, missing separate approval CA, and observing Active
  before its final approval binding) remain in logs1–4. The peer now respects a
  zero timeline limit and waits for original routes before timestamping input.

- ADR153 and `task-rust-factory-room-generations.spec.md` define the fix: one
  original warm-factory scheduling guard spans prior-read, authenticated full
  project-state GET, writer CAS and recheck across its original Collectors.
  Only changed joined membership advances the available Group generation;
  encryption/invite/privacy/owner/registration policy changes refuse. The exact
  existing validator and retirement transaction remain shared with ordinary
  observations. Old group routes stay retired; independent DMs stay unchanged.
  Missing/stale/unavailable scope cannot be rearmed, and original SDK/credential
  custody is neither reset nor reconstructed. The coordinator now shares that
  same guard when it watches the project. General group-session rebinding and
  negative-scope recovery remain separate required work.

- Initial actual Linux1.94.1 run6 passes the original same-project two-agent
  executable test for BOTH account kinds: four task/reply exchanges per profile,
  eight total, two real in-flight native helpers, original first PIDs, later
  factory-owned launches, zero unknown dispatches/leases and clean service exit.
  Its original SDK enrollments occur once per agent. This is offline local TLS
  and an actual native MCP fixture peer, NOT real Palpo/Robrix/model/sandbox proof.
  The test explicitly skips positive qualification off Linux; compilation or
  that skip is not native macOS/Windows cleanup evidence. Existing local native
  helper/factory regression29 passed before the room fix. New store authority
  tests2 and all reply tests19 pass; the complete final store suite passes346.
  Initial affected-package strict Clippy passes. Final expanded coordinator/
  readiness, Matrix/native regression and cross-platform checks remain running.

- No live deployment, service restart, SDK reset, unknown cleanup, new soak,
  formatter, dependency, commit, PR or production cutover. The real-client gen10
  narrow passing hour and gen11 failed cycle37/recovery remain unchanged. Full
  AS/profile, real sandbox/approvals/media, G8/restart recovery, completion
  reliability, sustained Palpo/Robrix soaking and all migration parity/release
  requirements remain part of the active goal.

### Final observation for the configured executable / ADR153 revision

- Original Linux run8 ended exit0, no OOM: bootstrap18, configured executable1,
  inline factory17, owned MCP6 and warm runtime6 all pass (48 selectors). The
  configured selector takes19.24sec and covers BOTH account kinds, two agents in
  one project plus its observing coordinator, eight actual encrypted task/reply
  exchanges, real simultaneous Started helpers, original runtime custody,
  readiness200 and clean shutdown with no unknowns/leases. All ten implementation
  and configured-fixture source hashes match the isolated Linux snapshot.
- Run7 remains failed: its second-round helper diagnostic still named round1.
  Canonical completion legitimately retires the original process before its
  local ACK/exit file is guaranteed. The final test instead checks the exact
  task/dispatch/fence/owned-completion/reply chain and independent recipient body.
  It does not retry the task or accept a stale file. Project generations count
  changed authenticated snapshots, not individual physical joins: the test
  checks current availability and the complete exact member set, allowing both
  joins to appear in a single snapshot.
- The broad local Matrix lib run ends186passed/1failed in737.55sec, not green.
  Its unsafe-encryption refusal finished while the independent owner's original
  join was still pending. The fixture awaited that actor without serving its
  HTTP request, causing the existing4sec timeout. The fixture now serves ONLY
  that original human actor while awaiting its actual termination; the intake
  refusal and deadline are unchanged. All six physical-room tests subsequently
  pass. All five Matrix integration targets pass32; the full187-test lib has
  not been rerun after this fixture-only fix. Preserve the failed broad log.
- Final native local lib38 passes; full store346 passes. Four affected packages'
  strict all-target Clippy and Windows GNU all-target compilation pass for the
  production revision. The later Matrix fixture-only Clippy and Windows checks
  also pass, separately logged. Windows compilation is not execution. The offline all-feature Rust
  selector inventory lists1024 with no missing selectors; it is not test
  execution. Production callers remain44/43wired, G8 still owed. agent-spec and
  the provisioned task-writer are unavailable; no lifecycle/task-state success
  is claimed. Whitespace checking passes; no formatter was run.
- Latest source remains undeployed. None of this is a new live soak: real Palpo
  plus headless Robrix2 gen10 passed30cycles/3602.04sec, then gen11 passed36cycles
  before cycle37 failed, followed by failed recovery. Entire-port, real model/
  sandbox/owner approvals/media, G8 and Active restart/rebinding, native platform
  runtime qualification and sustained real-client fleet soaking remain owed.
  No SDK reset, unknown repair, service restart, commit, PR or cutover occurred.

## 2026-09-16 configured two-agent bidirectional media qualification

- Previous goal turn is progress: ADR153 and actual executable two-agent tests
  changed implementation and qualification evidence. Rechecked source, accepted
  migration constraints and the old gen11 record. Its missing completion was
  genuinely zero task-operation calls despite final assistant text; preserving
  InProgress was correct. No inferred Done or automatic unknown retry was added.
- New `task-rust-fleet-media-executable.spec.md` binds successful per-agent media
  through the same original configured service. This replaces no existing gate:
  the earlier missing-source/empty-list test was negative coverage, not success.
  Both registration-token and AS-login runs still physically provision two
  agents in one project, retain the coordinator and original warm processes,
  observe both Started scopes before either proceeds, and run twice per agent.
- An independent owner SDK encrypts actual binary attachment inputs. Each native
  helper lists its original visible attachments, refuses the other agent's
  exact event, then downloads its own through the actual scoped MCP/API and
  authenticated Matrix media GET. It reads the generated original-workspace
  destination and writes task-bound output containing those received bytes to
  the SAME relative filename in both agents' separate roots.
- Actual send_file, upload and encrypted m.file publication follow. The recipient
  SDK decrypts the event and the upstream Matrix AttachmentDecryptor checks the
  exact uploaded ciphertext using only its received descriptor. Tests assert
  filename/caption/size/full sender/private relation, exact per-task bytes and
  canonical delivered-file/dispatch binding. One GET and one POST per task;
  foreign receive attempts never reach HTTP. The task is explicitly still
  InProgress after media delivery, then real completion publishes its final
  reply. No target attachment/Ready/file/task/claim/completion rows are seeded.
- Linux offline run1 exits0 for original executable and outgoing-media selectors
  (41.61sec). Run2 exits0 for the final bidirectional selector (21.59sec): eight
  receives, eight encrypted sends, eight final replies across both account kinds,
  readiness200 and clean shutdown with zero unknowns/leases. Both original Docker
  jobs are terminal/noOOM. Four source hashes match the isolated mini3 snapshot.
  Native helper/factory local regression29 passes on the outgoing revision.
  Final native all-target strict Clippy and Windows GNU compile pass;1025 Rust
  selectors list with none missing. Listing/cross-compile are not execution.
- The original full Matrix187 rerun now ends exit0:187passed, zero failed/ignored,
  731.01sec. The final Linux31-selector regression also ends exit0: configured2,
  inline factory17, owned MCP6 and warm runtime6 all pass. These final observations
  supersede pending status, not the original broad186/1 failed run's verdict.
  Logs: `fleet-media-*` and `fleet-room-matrix-final-full.log` in the existing
  private evidence cache. agent-spec/task-writer remain unavailable; no lifecycle
  or canonical task-state result is invented. No formatter or dependency change.
- Only offline fixture/spec/document changes this turn. No deployment, new live
  account/room, live-state write, Robrix restart, SDK reset, commit, PR or cutover.
  The new evidence is NOT real Palpo/Robrix media, actual model/owner approval,
  effective sandbox or sustained soak. Full profiles, G8, restart/rebinding,
  canonical completion reliability and all migration parity/release gates remain
  owed. The next real fleet qualification must use the latest source and preserve
  old gen10/gen11 evidence; the existing running live instance remains untouched.

## 2026-09-16 fresh live-fleet authentication prerequisite

- Previous goal turn was no progress: it clarified the real gen10 pass and gen11
  failure but changed no implementation or qualification state. Revalidated the
  actual running mini3 service (running, zero restarts), actual Robrix automation
  process (startup true, no exit), and the current inline factory source.
- The factory preserves per-agent physical homes. A real selected managed
  resource instead supplies its exact retained provider namespace through the
  existing account association; copying the coordinator's credentials or sharing
  its arbitrary runtime HOME is not the intended authentication path. ADR114
  requires a fresh operator-observed provider login, not credential-file inference.
- Prepared one new private qualification state and one managed account on mini3
  through the actual native init/account-prepare commands, with network disabled.
  Existing live state, SDK stores and failed-soak evidence were not modified.
  No new Matrix account/room or target Agent was created. Read-only readiness
  observation currently finds zero login receipts, so authentication is unknown.
- Added the fixed opt-in `account login --device-auth` route under
  `task-rust-managed-device-login.spec.md`. Plain login is unchanged. Original
  retained namespace, environment clearing, attempt-before-spawn and exit-derived
  observed/refused/uncertain settlement remain intact. There is no arbitrary
  option passthrough, service-driven login, credential import or API-key route.
- Actual isolated CLI/provider tests pass4 locally and pass4 in mini3 Linux with
  Docker network disabled. The new selector exercises all three exit classes;
  exact provider argv and hostile ambient environment refusal are asserted.
  The original Linux container exits0 without OOM. Three edited Rust source
  hashes match the Linux snapshot. Strict native all-target Clippy and Windows
  GNU all-target compilation pass.1027 spec bindings resolve with none missing;
  listing and cross-compilation are not runtime qualification. No formatter ran.
- Copied only the qualified native/Codex executable artifacts into the fresh
  private fleet root, pinned their SHA256 digests, and verified the copied
  native binary's login help. The provider's pinned offline help and local
  upstream source confirm the fixed headless flag. No provider login was started.
  The operator-only local `fleet-device-login.sh` refuses noninteractive use,
  preserves the exact state mount, uses no Docker log collection, and asks for
  provider consent in the operator's own terminal/browser. User sign-in was
  requested; no credential or device code is requested in chat.
- Evidence lives under `fleet-device-login-*` in the existing private cache;
  remote root and account ID are in its operator login script, not in tracked
  qualification claims. The runbook now includes the managed authentication
  prerequisite. agent-spec and task-writer remain unavailable: no lifecycle
  success or canonical Done is asserted. No commit, PR, live-service restart,
  production cutover, new soak, or whole-port completion is claimed. Next live
  action is to verify the operator's actual login receipt and enroll its bound
  resource before creating the separate real two-agent fleet. All broader
  migration, sandbox, recovery, client approval and sustained-soak gates remain.

## 2026-09-16 operator selects local Codex Claude Code and OctosCode

- The preceding connection-help turn was no progress. Rechecked the exact fresh
  remote account: zero login observations and no operator-login container. The
  user then explicitly changed execution to local Codex, Claude Code and
  OctosCode. mini3 remains the Palpo server; Robrix remains the real client.
- Verified installed local tools: Codex0.154.0, Claude Code2.1.270,
  OctosCode0.3.0-rc.9 (02e6816) and Octos2.0.3-rc.11 (02e773ea). Provider-owned
  status commands return exit0 and signed-in booleans for Codex/Claude. Raw
  provider identity/output was not printed; no credential file was imported or
  copied. These checks do not prove actual model execution or managed readiness.
- OctosCode is the UI Protocol client, while the installed Octos exposes ACP and
  server execution. The local client has no default config file. Its installed
  help still calls it mock-backed while the newer source checkout defaults to
  protocol: do not infer the installed transport from that checkout or treat a
  mock as the requested runner. Backend authentication remains unverified.
- Found concrete native port gaps: the Host rejects Claude, the runtime crate
  exports only Codex/owned IO, model qualification lacks the Octos family, and
  macOS whole-tree stop remains conservatively false. Native completion release
  depends on that stronger proof. Existing Node ACP support is reference behavior,
  not permission to call a Node-backed workaround the completed Rust port.
- Recorded REQ-LOCAL-THREE-RUNNER-QUALIFICATION and updated the working plan and
  runbook. Disabled the previously supplied remote-login helper; it now returns
  a fixed refusal without SSH or provider launch. Its unused state and original
  running remote fleet remain preserved. No remote login, model call, new soak,
  commit, PR or full parity claim. Next work is native local authentication and
  actual runner adapters/ownership under the requested topology, not another
  request for mini3 sign-in. Full original migration scope remains active.

## 2026-09-16 native local Claude stream protocol prerequisite

- Previous turn is progress: it verified the local tools, recorded the requested
  topology and disabled the obsolete remote-login helper. Continued the actual
  native Claude port under ADR154 and task-rust-claude-stream-protocol, not the
  retained Node adapter or a mock standing in for the requested local runner.
- Added a native Claude JSONL codec with typed permission/control/cancel and
  session-scoped event envelopes, fixed auto-mode/stdin callback arguments and
  bounded initialize/user/interrupt input. It has no permission-allow, process
  launch, credential or canonical-completion API. Payloads deliberately have no
  Debug implementation; upstream error text is not exposed as a diagnostic.
- Moved the existing unique-key JSON reader to a private shared module without
  changing its depth bound. Both protocols reject duplicate keys, including
  escaped duplicates, and excess nesting. Claude pull decoding returns one frame
  per call, handles fragmented UTF8/CRLF, caps lines at1MiB and partial age at10s,
  and irreversibly closes on malformed input, overflow, backwards time or expiry.
  Prompt input is capped at64KiB; model options cannot inject a new CLI flag.
- Three new bound protocol tests and the existing10 Codex tests pass. The full
  runtime suite passes72 tests with zero failures/ignored. A final expanded
  three-test rerun adds every accepted event kind and exact CRLF capacity edges.
  Runtime and native consumer all-target strict Clippy pass; Windows GNU native
  all-target compilation passes. These are offline checks, not local model or
  whole-tree cleanup qualification. No formatter or dependency change occurred.
- Separately ran one no-prompt initialization against the actual local Claude
  Code2.1.270 with auto mode, empty explicit MCP configuration, ambient settings
  disabled and no session persistence. It returned one matching successful
  control response and its leader exited0 without a signal. No prompt/model task
  was sent. The independent diagnostic did not use native owned Host custody;
  its sanitized private record explicitly says whole-tree cleanup unproven and
  no native-host/model/Matrix qualification. Raw provider output was not retained.
- Original live service, Robrix and the original checkout remain untouched.
  agent-spec/task-writer are still absent. Final runtime all-target strict Clippy
  and the original full selector inventory both exit0;1030 bindings resolve with
  none missing. Listing is not execution and no lifecycle result is invented.
  Production Host intentionally still refuses Claude. Next is the
  actual retained transport/session and owner-approval/task-tool integration;
  Octos, local authentication binding, macOS cleanup, full parity and live
  three-runner soaking all remain open. No commit, PR or completion claim.

## 2026-09-16 native Claude retained IO and owned-session prerequisite

- The prior turn's codec and independent installed-CLI initialize observation
  were progress, not enabled native Claude execution. Added ADR155 and the
  bound task-rust-claude-owned-session contract for the next native layer.
- Implemented one disposable Claude session: exact initialize correlation, one
  flushed prompt, first system/init session binding, matching subsequent events,
  bounded permission/cancellation observations and an explicit ResultObserved
  phase. Duplicate init, changed identities, unsolicited controls, reused request
  IDs and known buffered pre-prompt events refuse. No permission response or
  canonical completion authority is introduced; production Host still refuses
  Claude before workspace/process work.
- A single async owner pumps stdin/stdout/stderr under write, event, lifetime
  and partial-frame deadlines. Queues are capped at16 frames/2MiB wire bytes;
  stderr keeps a private16KiB tail. Partial writes and full-but-unflushed writes
  retain diagnostic byte counts on failure. Any polled operation's cancellation
  permanently closes its streams; no detached reader or automatic retry exists.
  The Rust concurrency/lifecycle guidance informed those explicit custody rules.
- OwnedClaudeSession uses the existing native guardian and actual async platform
  pipes. Failure/cancellation/drop requests bounded stop from the original owner.
  A result alone deliberately keeps process custody: the offline peer remains
  alive after its result and after stdin closure, so explicit-stop and Drop tests
  have a real pulse witness. Platform cleanup is returned unchanged; macOS
  whole-tree proof remains false, never upgraded from leader exit.
- Full offline runtime tests pass77 with zero failures/ignored. The final five
  new session/owned tests also pass after adding stalled/failed flush, prompt
  cancellation, buffered pre-prompt refusal and a live Drop witness. Runtime and
  native consumer all-target warnings-denied Clippy pass. Windows GNU native
  consumer all-target compilation passes; this is compilation, not Windows
  runtime or containment qualification. Private logs use `local-claude-owned-*`
  in the existing live-evidence cache. No formatter or dependency change occurred.
- agent-spec/task-writer remain unavailable; no lifecycle or canonical Done is
  invented. No live model, new Palpo/Robrix soak, remote model login, credential
  import, service restart, commit or PR occurred in this slice. Next is actual
  Claude approval/task-MCP/usage and execution-Host integration, plus the still
  open local authentication, Octos, macOS containment and full migration gates.
- Final full selector inventory exits0 with1035 bindings and none missing;
  listing is not test execution. All original check handles reached terminal
  exit0, and `git diff --check` is clean. No background test job remains.

## 2026-09-16 native Claude one-shot permission control and live deny observation

- Previous turn is progress: ADR155 connected the codec to bounded native IO
  and retained process custody. ADR156 now adds the missing permission-response
  mechanics; the ADR005 amendment distinguishes this official Agent SDK stdio
  boundary from the unchanged retained tmux channel. Neither is owner authority.
- Added explicit control enablement, privately retained original tool input,
  single-use/source-bound prepared frames and fixed allow-original-input or
  deny-and-interrupt responses. No caller input substitution, permission updates,
  JSON verdict constructor, bypass or production Claude admission was added.
  Original durable grant consumption/rechecks remain the execution Host's duty.
- Receive timestamps survive delayed host consumption. Owner/response deadlines
  derive from those timestamps inside the original lifetime; prepared write
  deadlines and idle read deadlines do not renew on control/message returns.
  Callback IDs are capped at16 and input at64KiB each. Cancelled/sent identities
  remain tombstones. Original byte buffers/offsets survive message returns;
  ordinary read/control calls cannot advance a suspended response writer.
- A separately pinned host future remains alive while original IO is pumped.
  Partial stdout frames fence the first response byte, and observed cancellation
  or result barriers prevent later response continuation. Partial writes remain
  explicit uncertainty. Rust concurrency/lifecycle guidance informed these
  no-detached-reader, retained-frame and started-future-drop rules. The original
  guardian stops on errors/cancellation; macOS cleanup semantics are unchanged.
- The offline suite initially passed82 runtime tests. Added an operator-only
  diagnostic with a pure environment unit test; final all-target runtime passes83
  with zero failed/ignored. Coverage includes exact original input despite caller
  mutation, cross-session/repeated responses, fixed clocks, delayed receipt,
  pre-byte/partial-write barriers, capacity, future drops and actual owned-pipe
  allow/deny/cancellation. Strict runtime+native consumer all-target Clippy and
  Windows GNU all-target compilation pass. Cross-compilation is not runtime proof.
- The explicit live diagnostic is separate from Cargo tests and production Host:
  local installed Claude, fresh private empty cwd, safe/restricted mode, Bash-only
  availability with a Bash ask rule, empty MCP, no persistence, a fixed harmless
  prompt, deny-only replies and finite time/cost bounds. It uses provider-owned
  operator authentication without reading/copying credentials, not a fabricated
  Hagency account binding. Raw provider frames/text are never printed or stored.
- Diagnostic1 failed initialize before a prompt. No-prompt structural inspection
  found optional pending_permission_requests/pending_user_dialog_requests arrays
  in the installed success envelope. The native codec now permits only empty
  arrays, with regression coverage for nonempty/malformed queues. It still rejects
  conflicting error/success fields, duplicate keys and foreign request identities.
- Diagnostic2 initialized and observed auto mode but returned an error result with
  zero permissions. Read-only provider status isolated the environment issue:
  HOME/PATH alone reports logged out, while adding USER restores the existing
  claude.ai login; LOGNAME/SHELL/TMPDIR/CF metadata separately do not. Forwarding
  USER is OS identity preservation in this diagnostic, not token/API-key adoption.
- Diagnostic3 through the actual native owner reached one real permission request
  and flushed one deny, zero allows. It then observed an error result and exited1.
  Preserve this negative outcome: it exercises the local callback transport but
  is not a successful coding task, runtime-application ACK, canonical Done or soak.
  Leader exit/signals were observed; whole_tree_stopped=false remains explicit.
  All three original diagnostic records and private workspaces are retained.
- Rechecked installed Claude2.1.270 at its original local version path; SHA256
  a506b6d970a4cf44f6abdb53a81ddcd5d3b0ce042a95c502fe9d1f946bdb8807.
  Safe evidence is under `local-claude-control-*` in the existing private cache.
  No mini3 model execution/login, live-service restart, Robrix source change,
  credential import, formatter, dependency change, commit or PR occurred.
- The full goal remains active. Next is connecting Claude to original durable
  approval grant/usage/task-MCP/execution Host custody, not simply deleting its
  UnsupportedRunner refusal. Local managed authentication, Octos, native macOS
  containment, actual Matrix/Robrix successful tasks and sustained three-runner
  soaking plus the broader M0–M9 migration gates remain unproved.
- Final selector inventory exits0 with1041 bindings and none missing; listing is
  not execution. All original build/test/inventory and live-diagnostic handles
  reached terminal states. The three live diagnostics remain exit1; offline final
  checks exit0. `git diff --check` is clean. agent-spec/task-writer remain absent,
  so no lifecycle success or canonical task-state write is claimed.

## 2026-09-16 native Claude source-bound usage and durable capture

- Previous turn is progress: ADR156 reached a real local deny callback while
  preserving its error result and incomplete process cleanup. ADR157 now adds
  the next missing native integration: immutable usage observations, typed
  normalization and the existing private dispatch ledger attachment. No live
  provider, Palpo, Robrix or mini3 execution was started in this slice.
- Original driver identity plus validated session ID binds each observation.
  Attachment is allowed immediately after system/init (sequence1); all later
  ordinary/control/prepared-write message reads advance that same sequence.
  Payload mutation cannot change evidence. Close, error, cancelled read and Drop
  retire retained sources. Result usage can be recorded but is not process stop.
- Official Claude cost-tracking documentation distinguishes nested message IDs,
  placeholder per-step output, main-loop result usage and reported-model totals.
  Main-loop input/cache evidence deduplicates nested IDs, not outer UUIDs, and
  excludes subagent assistant frames. Final aggregates replace partial snapshots.
  Empty/malformed model maps stay unknown, never fallback or measured zero.
- Fixed bounds cover16384 observations,1024 step IDs (512 bytes each) and64 models.
  Known subtotals remain checked even after a missing counter. Conflicts and
  capacity/overflow invalidate capture; independent invalid fields remain unknown
  with diagnostics. No model names, message IDs, text, paths or estimated billing
  enter normalized usage evidence. Codex/transcript serialization is unchanged.
- Private UsageRun now accepts only the native Codex/Claude source adapters and
  checks the acknowledged Started resource family. The original single pending
  source/call/content tuple survives cancelled writer waits, including after a
  real commit. Result usage enters that slot before capture closes; replay after
  process stop does not reopen it. Rust concurrency/lifecycle guidance informed
  this retained-source and cancellation-safe receipt design.
- Native offline guardian pipes plus the actual domain writer prove duplicate
  suppression, family/source/sequence/retirement guards, pending-write refusal,
  persistent high-water totals, restart inspection and exact lost-response replay.
  The error result still leaves the canonical task InProgress. The first focused
  execution test exposed an unqualified fixture model; it now uses the same
  accepted offline model profile as existing store usage tests, not a readiness
  override. Initial Clippy flagged a large temporary enum; borrowed evidence
  removes that unnecessary copy without new per-event heap allocation.
- Full runtime tests pass87, including the four new runtime selectors. The new
  typed-normalization and owned-ledger selectors pass. Strict affected-package
  plus native-consumer Clippy passes; Windows GNU all-target compilation passes,
  which is not Windows runtime evidence. The first broader test command stopped
  at the two existing checked-in-placeholder Codex sandbox qualification failures;
  the final four-package no-fail-fast run finished530 passed,2 failed,0 ignored.
  Only native_codex_real_app_server_sandbox_write_inside and
  native_codex_real_app_server_sandbox_refuses_outside fail; their qualification
  records remain placeholders. All six new contract selectors passed. Preserve
  exit101 rather than calling the broader suite green. Final Clippy and the
  Windows runtime recheck also pass after the cancellation-retirement assertion.
  Evidence: local-claude-usage-{full-tests,final-clippy,windows,final-windows}.log
  in the existing private cache. The sequential full selector listing reached
  its actual300s command deadline and exited1/ETIMEDOUT, not a passing inventory;
  the separate bounded four-worker compiled-harness listing finished exit0:
  143 executables list1156 tests, resolving1047 spec bindings with none missing.
  This is listing, not execution; its parallel-bindings JSON/log retain that
  distinction. All original check handles are terminal. `git diff --check` is
  clean. agent-spec/task-writer remain absent; no lifecycle or task-state success
  is claimed from documentation writes.
- Goal remains active. Production Host still refuses Claude until original
  private grants, scoped task/file MCP, account and platform custody integration
  are complete. Octos/OctosCode, sound macOS descendant stop, successful real local
  tasks and sustained three-runner Palpo/Robrix soaking plus M0–M9 parity remain.
  No credential copying, mini3 login, formatter, dependency change, commit or PR.

## 2026-09-16 native Claude task bridge and execution-path parity pivot

- Recovered the previous owned-helper test's successful terminal log after its
  original handle was gone; reran current sources rather than assuming stale
  output covered later fixes. Previous implementation work is progress; the
  intervening Palpo-address answer added no implementation evidence.
- ADR158 shares the validated native task descriptor without changing the prior
  Codex configuration. Claude binds its helper once on the original initialized
  process, after actual task-context acknowledgement and before the sole prompt.
  Empty inventory, exact added-server ACK and connected exact tool inventory
  share one deadline. Cancellation/error closes the original operation and stops
  its retained owner; no reconfiguration/replacement or raw control API exists.
- `hagency mcp --owned-task-profile` restricts both tools/list and tools/call to
  maintenance plus explicitly enabled file pairs. Current capability/task/fence
  checks stay in the API. Normal MCP's wider catalog remains unchanged. This
  limited profile is not yet the whole TS coordination surface.
- Actual offline native guardian pipes, helper executable and domain API cover
  direct/retained context, exact optional catalogs, heartbeat/readback, foreign
  task refusal, hidden-tool refusal and missing-helper failure. Plain model result
  leaves canonical InProgress, no final reply, and the workspace lease retained.
- Two explicitly activated diagnostics used installed local Claude2.1.270 and
  the actual Rust helper. Both initialized and bound the exact catalog, exit0.
  The second used the final read-only launch profile. Neither sent a model prompt,
  tool call or API connection; their synthetic context points to an owned
  non-serving loopback socket and supplies no task/account authority. Leader exit
  and accepted stop signals were observed; whole_tree_stopped=false is retained.
  Records: local-claude-mcp-live-catalog{,-parity}.json in the private cache.
  Private workspaces claude-native-catalog.HxNm5x and
  claude-native-catalog-parity.6y6MU6 remain retained. No provider token was read,
  copied or written into argv/config/output. Same-user/environment isolation is
  not established by inherited context or a successful MCP connection.
- Operator clarified the working order: map each working TS path to Rust, port
  missing behavior using its implementation/tests, then run the complete path
  through the same Palpo/Robrix and fix observed differences. Make it work, make
  it right, make it fast. Added docs/design/native-execution-parity.md and updated
  docs/plan.md to organize work by complete execution paths, not adapter proofs.
- Direct TS comparison found native launch-policy differences. Ported the same
  write-lease auto/read-only plan selection, protected Bash(gh *) / Bash(git push *)
  ask rules and bounded model grammar. The existing TS configuration/dispatch/
  guardian suites pass47 tests after rebuilding router/dist. Native launch-parity
  tests exercise those same choices; this is not production Host enablement.
- Verification: runtime initially91, final92 tests pass, zero failed/ignored.
  MCP/task-client integration21 passes; final focused helper/owned suite10 passes.
  Initial strict Clippy reported adjacent independent if statements in test
  code; line separation fixes them, and final runtime/native-consumer all-target
  Clippy passes. No formatter was run. Windows GNU all-target check passed before
  the final launch-parity edit; it is not Windows runtime or final-source proof.
  Workspace compiled-harness listing before the added parity selector resolved
  1052 bindings with none missing (145 executables/1161 names). Final current
  ADR158-only listing resolves all6 selectors. Listing is not test execution.
  Earlier full-workspace qualification failures are unchanged, not silently green.
- TS's five actual process-guardian tests pass on this Mac, including detached
  descendants, a foreign-process survivor and inspection-loss refusal. Source
  tracing identifies the native local-path gap: Scope hardcodes false on
  non-Linux POSIX and the supervisor rejects positive macOS stop reports. Next
  port this missing behavior using native identities and the TS regression cases
  to unblock the existing Codex message-to-reply path. Do not flip the boolean or
  weaken unknown recovery. Then connect Claude to shared Host/driver/approvals,
  port real Octos/OctosCode, and run identical live paths and sustained soaking.
- All original check handles reached terminal states. agent-spec/task-writer
  remain absent; no lifecycle or canonical task-state success is claimed. Full
  local three-runner execution/soak and broader migration remain incomplete.
  No credential copying, remote runner login, service reset, dependency change,
  formatter, commit or PR occurred.

## 2026-09-16 native macOS stop and observed local Codex compatibility

- Ported the TS guardian's sampled descendant behavior with native suspended
  spawn, an original-process baseline, retained ancestry across reparenting,
  birth/version-checked signals, bounded stop and two clean post-root censuses.
  Actual parented, detached/reparented and early-leader cases stop; a foreign
  process keeps advancing. Lost observation remains sticky Unknown. This is not
  adversarial crash containment; the separate fast-double-fork/crash profiles
  remain unsupported rather than being relabelled positive.
- The first concurrent census exposed a concrete false refusal for launchd's
  unrelated new children. Native original-parent PID version distinguishes a
  direct launchd child from an adopted/executed unknown orphan. Retained birth
  identities still decide owned descendants. Temporary diagnostics were removed.
- Real installed local Codex0.154.0 then exposed inherited thread network policy
  before any prompt. Pin the requested network=false and extra roots=[] in the
  thread config as well as the existing turn policy; do not weaken echo checks.
  Next the real CLI emitted synchronous hook notices. Added bounded schema/scope
  validation and diagnostic-only notices, including startup races. Foreign
  thread/turn, async hooks and unknown event families remain refused.
- The final real run performed one matching inside-workspace command, verified
  the created contents and proved observed-tree cleanup. Outside and symlink
  probes completed with zero tool attempts: no file creation is NOT sandbox
  denial evidence. Their pass fields stay false and the qualification exits1.
  Earlier policy and notification failures remain retained. The new0.154.0 record
  does not replace the older pinned placeholder gates or qualify a whole task.
- Platform/runtime122 offline tests pass. Downstream execution initially exposed
  four obsolete macOS-Unknown expectations and one missing approval notice; after
  updating only supported-scope expectations, the full76-test rerun passed. The
  isolated missing-notice failure has no established cause; its original log is
  retained, not dismissed as flakiness. Native guardian CLI entry also passes.
  Strict platform/runtime/execution Clippy passes. Palpo's versions endpoint and
  the existing Robrix started/not-exited status respond; no new Matrix task was
  sent and health checks do not prove end-to-end delivery.

## 2026-09-16 explicit local Codex provider binding (ADR159)

- Traced TS runnerEnv against native bootstrap: TS carries provider context;
  native previously selected only a fresh runtime home or a managed namespace.
  Added the explicit private local_codex/provider_owned_codex_v1 profile and
  retained LocalCodex owner. It binds one unmanaged preset/seat to existing
  provider directories; no credential file read/copy, fabricated readiness,
  global login change, remote runner or managed-account fallback is involved.
- Original directory ownership/write permissions and identity are checked at
  admission, before spawn and through the same operation. Retained objects stay
  with unresolved process custody. Provider read/search permissions are allowed;
  group/other write permission, replacement and workspace-containing-provider
  layouts refuse without permission repair. OS search path is captured once.
  Only fixed provider paths/search path are added; API keys and coordinator
  secrets are refused. Managed readiness and default sandbox remain unchanged.
- Host selection narrows to the same immutable preset/seat before leasing, and
  consumption independently checks it. Foreign work stays queued without an
  attempt. Unrestricted profiles avoid the additional resource-match SQL query.
  Factory/warm combinations and Windows local provider binding remain refused.
- Actual offline native CLI/guardian/helper tests verify the selected separate
  provider directories and exclusion of ambient provider/coordinator secrets.
  Replacement during execution fences the original task; model completion alone
  still leaves canonical InProgress. No managed account/login rows are created.
- Execution regression78 passes; store library/owned-claim/owned-dispatch81
  passes; bootstrap19 passes. The first bootstrap run was18/1: its old macOS
  shutdown assertion expected Unknown despite observed whole_tree_stopped. The
  final run now exercises successful native shutdown and released DB owners.
  Initial Clippy caught a decimal Unix permission literal in a fixture; fixed
  to explicit octal, then strict execution/store/native all-target Clippy passes.
  Final path-capture refinement is rechecked by the focused binding selectors.
  Final focused selectors and strict Clippy v2 pass; both active contracts'
  10 selectors resolve to executed passing tests (not agent-spec lifecycle).
  All original check handles are terminal and git diff --check passes. Rust
  lifecycle guidance informed retaining provider-directory custody with the
  original runner and stopping it when the observed binding changes.
  Already-locked rustix1.1.4 became a direct execution dependency; no version
  upgrade occurred. No formatter, commit or PR. Original failures remain in
  local-codex-binding-* logs under the existing private qualification cache.
- agent-spec/task-writer remain absent; documentation is not lifecycle/canonical
  task-state success. The active goal remains incomplete. Next create a separate
  local Matrix qualification identity/device/state and run this native local
  Codex path through the same Palpo and real Robrix; do not share the running
  remote SDK store or duplicate its inbox consumer. Then join Claude/Octos and
  run shared-room/DM/files/approval/recovery and sustained three-runner soaking.

## 2026-09-16 real local Codex path and MCP approval parity (ADR160)

- Created separate qualification Matrix accounts/rooms on the same mini3 Palpo,
  using the existing exact owner master anchor. Kept local provider credentials
  with Codex and left the original remote services/SDK stores untouched. The
  private cache is local-native-palpo.JyL9O9 under the existing qualification root.
- Actual Robrix UI → encrypted Palpo DM → native intake/claim → local Codex →
  canonical Done → one encrypted reply → actual Robrix render passed twice.
  The second task ran a shell command, wrote exact workspace bytes and read them
  back. Original dispatches ended completed with no lease/unknown rows at that
  checkpoint. Claim-to-delivered baselines:14114 ms and22553 ms. This is neither
  three-runner qualification nor sustained soak.
- The file task exposed native `mcpServer/elicitation/request` as malformed.
  No file/owner request was admitted. Original process report proved observed
  whole-tree stop but retained negative settlement and canonical InProgress.
  Traced and ported the existing TS correlateMcp behavior against its tests and
  installed Codex0.154.0 generated schema: exact active item/server/arguments,
  empty form only, one consumption, no stale/ambiguous/repeated candidate and no
  late response after completion. Keep wire parameters unchanged; a separate
  host envelope binds the real item/tool/arguments into the existing owner grant.
  No reusable MCP scope or file-tool autoapproval was added. Runtime diagnostics
  now retain the fixed server-request label on the controlled read path too.
- Explicit approval_owner_wait_ms replaces the hardcoded1000 ms only when
  configured and must fit the original operation plus response reserve. Existing
  profiles retain their default. The30-second execution ceiling is still a
  long-running-task parity gap. The Rust resource skill guided boxing just the
  larger singly-owned MCP payload after Clippy flagged enum growth; OpenAI Docs
  guided the elicitation wire response, not domain authority.
- TS approval reference12 passes. Rust runtime95 passes serially; focused MCP4,
  owned coordinator1 (Once/Deny, reusable-choice refusal), startup-budget1 pass.
  Execution80 non-qualification checks pass; existing pinned qualification is
  1 pass/2 failures, unchanged placeholders. Initial full parallel runtime had
  one uncertain process-start failure; isolated and serial reruns pass, cause
  not established. Strict runtime/execution/native all-target Clippy passes.
- Saved before-mcp-fix.json with original status/custody. Original native service
  handle87788 exited0 after bounded shutdown. Patched restart handle68264 exited1
  because shutdown had fenced generation1. Explicit generation2 re-observation
  restored approval startup, but ordinary intake reports Domain and retains the
  old generation1 session route. New diagnostic service handle11130 remains live
  at127.0.0.1:19330 with driver unavailable, not an active successful agent.
- The actual scoped operator recover-dispatch endpoint returned409: this is a
  stop-fenced failure, not an orphan. dispatch_stops.reason=owned_runner_failure
  has no settlement evidence. Current finish_attempt explicitly leaves this join
  unwired; generic recovery must not become a second custody clearer. Original
  task/lease/quarantine/effects remain untouched. No fabricated proof, direct DB
  clear, new-task bypass, copied credential or replacement SDK store was used.
  after-mcp-port-recovery-refusal.json records current status. File approval,
  delivery and recovery therefore remain unqualified despite the adapter tests.
- Next bounded work: join exact stopped-failure/workspace-inspection custody and
  native restart route generations, then repeat the original file/approval path.
  Continue Claude Host join, real Octos adapter, remaining tools and shared-room/
  DM/recovery/soak scenarios. agent-spec/task-writer remain absent; no lifecycle,
  canonical task success, formatter, commit or PR is claimed for this source work.

## 2026-09-16 Codex-first execution budget parity (ADR161)

- Operator clarified sequencing: finish real Codex E2E and soak first; Claude Code
  second; OctosCode last. No Claude/Octos implementation was changed in this slice.
- The final ADR160 runtime rerun completed95 tests and the owned MCP selector
  completed1; the accidental intermediate test assertion edit was corrected.
  Original rerun logs remain retained. Those two handles were no longer present
  after context compaction; their complete logs were checked and no corresponding
  cargo/test process remained. This slice's subsequent checks have observed
  original terminal tool receipts, not inferred process success.
- Ported the retained TS twenty-minute allowance into explicit ordinary native
  operation configuration, the owned approval context and compatible host claim.
  Capability lifetime is max(60000, operation_ms+30000); generic claims remain
  capped at five minutes. Initial leases remain60 seconds, renewals5 seconds,
  and capability expiry is never renewed. RPC/write/SQLite/receipt waits,
  cancellation cadence, sandbox and all uncertain outcomes remain unchanged.
- Explicit owner waits plus response reserve may fit the existing ten-minute
  durable approval ceiling and original operation; startup's1000 ms default is
  unchanged. Pre-activation warm initialization remains separately capped at30
  seconds, with a regression against accidentally widening it via shared Limits.
  Configured factory admission remains outside this ordinary local-Codex slice.
- Actual offline owned subprocess ran quiet for31 seconds within a45-second
  operation and completed/settled after original whole-tree cleanup. A separately
  configured twenty-minute operation cancelled promptly. Real owned MCP pipes
  accepted a private owner decision with a60-second configured owner window.
  These tests do not claim a twenty-minute real-model run or live owner UI pass.
- Verification: full runtime95 and execution lib/owned80 pass serially. Store
  approval/claim suites44, startup owner-wait1, final limit1 and warm-runtime6
  pass. Store tests include host-only long claims, fixed original expiry, lease
  loss, revocation, overflow and unrenewable context bounds. Final strict Clippy
  across runtime/core/store/execution/native all targets passes. Original handles
 9036,72870,31511,85295,89272,7467,23607,67100,78030,78317 all returned exit0.
  Logs are budget-* under the original private local-native-palpo.JyL9O9 cache.
  Existing pinned sandbox qualification failures were not rerun or reclassified.
- Read-only live inspection at2026-09-17T02:50:19Z still shows two completed tasks
  and delivered replies, the original file task InProgress/outcome_unknown, no
  files or approvals, and one retained lease/unresolved attempt. Diagnostic
  service11130 remains live but agent execution is unavailable (refresh/domain).
  No live config, SDK store, task/fence, file or credential was replaced or reset.
- Next: finish the exact stopped-failure recovery and current Matrix route
  startup join; then exercise the fixed MCP/file path, owner buttons, received
  files and Codex soak through actual Robrix. Preserve the original failure.
  Rust lifecycle/concurrency guidance kept the long execution deadline separate
  from short leases and cancellation. No UI/i18n change, formatter, commit or PR.
  agent-spec/task-writer are still absent; no lifecycle/canonical task completion
  is claimed. Full Codex E2E and the active migration goal remain incomplete.

### Codex original-owner inspection receipt (ADR162, 2026-09-16)

- Original failed native owner now captures a bounded, no-follow, two-pass
  content inventory after observed full process cleanup and exact negative
  fencing. Schema34 retains one immutable dispatch/fence receipt. Recording it
  does not release leases, settle stops, replay inputs or change task outcomes.
  Lost acknowledgement retains the original payload for explicit retry.
- Regression tests exposed and fixed absolute symlink-content handling and a
  mismatch between canonical stored dispatch JSON and struct field ordering.
  Added real-process/hash/reopen/refusal tests, schema33 upgrade preservation,
  historical capability and real writer pre/post-commit receipt-loss tests.
- Original handles39582 (full store352),69384 (execution83),84931 (strict
  all-target store/execution Clippy),77550 (native build) returned exit0.
  Logs are stopped-inspection-* in the original private qualification cache.
  No formatter, commit, PR, Claude or Octos change. No UI/i18n source change.
- Diagnostic service11130 exited0 after targeted SIGINT. Backed up domain DB
  and configuration to before-codex-next; authenticated native provision command
 56967 exited0 adopting local_codex_dm_next/workspace_next at generation3.
  Same Palpo/account/device/Robrix DM and original SDK stores; separate physical
  workspace. This enables a NEW MCP/file qualification, not recovery of the old
  failed task. The original failed attempt, inputs and exclusive lease survive.
- Live follow-up: service65257 (PID47778) started with the tested build and
  original SDK stores. Authenticated generation3 session accepts Robrix's new
  task004, dispatch matrix_dispatch_3ec277ea31ae0330251db762975da32b, fence0,
  canonical Created. It remains QUEUED: ordinary bootstrap fixes max_live=1,
  and the original unresolved attempt consumes that slot globally. No new model,
  file, approval or reply was produced. Re-adopting a separate session alone is
  not sufficient, and the queued task is not E2E evidence. Do not raise the cap
  or clear the old stop as a qualification shortcut. Original stopped-proof loss
  still prevents the old task's legitimate recovery. Further independent live
  qualification requires an isolated fleet/state decision, not just new IDs in
  this domain. The live service is retained for inspection, not being retried.

### Codex stopped-runner occupancy (ADR163, 2026-09-16)

- Mapped current TS operator inspection/resolution and corrected ADR148's stale
  orphan-homes-only reference. Full native continue/accept/keep-blocked parity
  remains open; this slice does not invent or claim an operator recovery route.
- Reproduced the global-capacity bug before fixing the production claim query:
  exact proven-stopped owners no longer occupy a process slot, while their
  task/workspace custody remains held. Unknown physical owners still count.
  Added reopen/wrong-fence/conflicting-work tests and actual failed-then-successful
  independent owned processes at the unchanged one-runner limit.
- Original handles97398 (execution84),36369 (store45),17224 (strict Clippy) and
  55699 (correct root TS build, then router-core/backend88) returned exit0.
  Initial store regression failed as expected before the fix. Initial process
  test's global accepted-output assertion was corrected to scope the original
  dispatch. Earlier nonexistent test targets and wrong TS build path were
  corrected; no unexecuted test was counted as passing. Logs: stopped-capacity-*.
  Native build72030 returned exit0 and `git diff --check` passed.
- Read-only live inspection at2026-09-17T03:40:58Z: two Done/delivered tasks,
  original file attempt InProgress/outcome_unknown, new task004 Created/queued,
  one retained lease, no files or approvals. Original service65257 remains live.
  No deployment/restart would fix that legacy missing proof, so none was made.
  Pending isolated-fleet choice remains unanswered; no new accounts/rooms were
  created. Codex E2E/soak is incomplete; Claude Code and OctosCode remain untouched.
- Rust lifecycle/concurrency guidance kept physical occupancy distinct from
  task custody. No UI/i18n source changes, formatting, commits or PRs. The task
  contract is recorded; unavailable agent-spec/task-writer lifecycle tooling
  still prevents any canonical lifecycle completion claim.

### Codex recovery route target binding (2026-09-16)

- Rechecked actual original service65257 (live) and the retained failed dispatch:
  outcome_unknown/fence1, owned_runner_failure, no stop evidence or settled_at,
  zero original inspection receipts, one lease. Existing orphan recovery and
  the unexposed host-only stop-settlement kernel cannot legitimately unblock it.
  No new fleet/accounts/rooms, live recovery mutation or deployment was made.
- Found and fixed a separate concrete recovery bug: agents/{id}/recover-dispatch
  ignored id after shape validation. Regression handle45955 exited101 with
  actual200 instead of expected404 through another existing agent's URL.
  The route now carries id through the bounded writer, and the shared recovery
  transaction checks dispatch/session ownership before any custody mutation.
  No new schema, endpoint, recovery action or per-agent scope was introduced.
- Original handle63399 returned exit0: task-store7 and full console57 tests,
  including wrong/missing agent refusal with unchanged custody, successful owner
  recovery and the existing stopped-dispatch/read-only refusals. Strict all-target
  store/native Clippy handle86709 returned exit0. `git diff --check` passes.
- The default production-callers checker exited1 before analysis because its
  git-index file enumeration still includes the already-deleted
  native/hagency-runtime/src/codex/json.rs. Its source is unchanged in this slice;
  do not count the unavailable gate as passed or recreate the deleted module.
- Lifecycle/recovery skill guidance kept target validation in the same write
  transaction and preserved stopped-task custody. No UI/i18n code, formatter,
  commits or PRs. Codex E2E and soaking remain incomplete; Claude and Octos remain
  untouched. The isolated qualification-fleet approval question remains pending.
- Final original handle12345 returned exit0: full store354 across40 suite
  summaries, followed by a successful native build. Together with console57,
  this slice has411 passing tests; the earlier focused7 are included, not added
  again. Source diff whitespace check passes. Logs are recovery-agent-* in the
  original private qualification cache. No live service restart was performed.


### Codex receipt-bound stopped continuation (ADR164, 2026-09-17)

- Reviewed the latest feat/rust-migration worktree and preserved its existing
  uncommitted migration work. Native Codex text/command successes are already
  recorded; files, private approvals, recovery/restart and soak remain open.
  Claude execution integration and Octos remain later work under the existing
  Codex-first order. No Robrix code or live service/state was changed.
- Added lifecycle-scoped original-receipt inspection and explicit continuation
  through the native console. Agent, original fence and immutable host digest
  bind the request. The shared stop-settlement kernel, replacement enqueue,
  frozen-input transfer and replay receipt commit together. The old attempt
  remains outcome_unknown; no task outcome or processed input is fabricated.
- Ported a distinct nonempty instruction and content-bound replay, including
  reopen and equivalent finite-number payload representations. Missing receipt,
  mismatched identity/bindings, stale scope, completed work, another active or
  unknown owner and unresolved receive/upload/delivery custody refuse atomically.
  Ordinary orphan recovery still refuses all stop-fenced attempts.
- The console regression first returned404 for the absent route. Later targeted
  regressions exposed missing active-owner and pending-receive guards and use of
  signed-DTO numeric hashing for dispatch payloads; each now passes after its fix.
  The real-process test drives a failed original owner, retained receipt,
  explicit continuation, same-workspace replacement and successful completion
  at the unchanged one-runner limit. The original failure remains observable.
- No receipt is retrofitted into the old live file failure, so it remains blocked.
  No new fleet/account/room, deployment, live approval or E2E/soak claim. This
  slice does not implement TS accept_completed, keep_blocked or expiring
  inspection tokens. API-only changes add no UI text or translation keys.
- Four new contract bindings resolve against compiled test inventories. The
  default production-callers gate still fails on its indexed deleted
  codex/json.rs; explicit current-worktree enumeration passes44 wired callers
  and one declared owed G8. agent-spec parse/lint commands are unavailable
  (executable absent); no lifecycle pass is claimed. No formatter, commit or PR.
- Verification: full store356 and console58 passed on the final verification
  pass; the final actual-process continuation selector also passed. Owned45
  passed serially; its earlier parallel run had44 pass and a failure in the
  existing long-lifetime test's elapsed-time assertion. The cause of that early
  exit is unproven and is not erased by the serial pass. Rebuilt TS router
  core/backend88 passed. Strict all-target Clippy for store/execution/native
  passed. Focused refusal coverage includes18 differing invalid states in one
  table-driven selector; those cases are not counted as18 additional tests.
  Logs: /tmp/hagency-continuation-{store-final,console-final,owned,owned-serial,
  process-final,router,clippy-final}.log. The full store/console and focused
  final checks include the new tests; repeated runs are not additive counts.
- Native `cargo build -p hagency` completed successfully; final diff whitespace
  check passed. No binary was deployed or service restarted. Build log:
  /tmp/hagency-continuation-build.log. The bounded ADR164 slice is ready for user
  testing; overall Rust migration and live Codex qualification remain incomplete.


### Native operator outcome resolution and current-worktree gate (ADR165, 2026-09-17)

- Continued after the operator pointed out that stopping at ADR164 was premature.
  Implemented schema35 inspection credentials and lifecycle-scoped console
  inspection/resolve routes for continue, accept_completed and keep_blocked.
  Preserved the explicit compatibility continuation route and all prior dirty work.
- Inspection requires the original host receipt, matching owned scope, retained
  leases and dirty workspace, unfinished task, valid current route, no competing
  owner, and no pending/unknown media effects. Random credentials persist only as
  hashes, expire in1..60min, bind the current snapshot, and are capacity-bounded.
  The actual writer reads time after acquiring the SQLite transaction.
- Resolution and its request receipt are atomic. Continue reuses the shared
  recovery transaction kernel and moves authenticated inputs; accept-completed
  records canonical Done exactly once; keep-blocked queues nothing. Neither
  terminal action fabricates runner output, input processing, graph completion
  or Matrix delivery. All preserve original outcome_unknown. Graph terminal
  decisions explicitly remain outside this slice. Exact replay survives expiry,
  reopen and equivalent numeric payload representations; changed content conflicts.
- Added expiry/binding/state/media/custody refusal tests, schema34-to35 preservation,
  bounded private issuance, HTTP scope/target tests and all-three-action actual
  subprocess tests. A final-receipt SQL fault proves that settlement, canonical
  task events, replacement/input transfer and token consumption all roll back.
  Existing schema-head assertions were advanced to35 without rewriting historical
  migrations or manufacturing proof for the earlier live failed attempt.
- Fixed the production-caller checker's source discovery to include non-ignored
  untracked files and exclude indexed deleted files. Its normal invocation now
  passes45 wired callers and one tracked G8, with25 checker tests. This does not
  change reachability or permit fixture callers to count as production.
- Initial new actual-process test failed with SpawnFailed before the intended
  protocol failure. Cause unresolved. Retained original fixed startup diagnostics
  in Report and test failure output; no deadlines/automatic retries changed.
  A diagnostic rerun and10 bounded repeats passed, followed by the final full
  owned46 pass. Earlier ADR164 long-lifetime failure remains recorded despite
  current parallel passes. No intermittent failure is relabeled as diagnosed.
- Verification: full store360 (40 suite summaries) passed; the final five focused
  outcome tests add one new rollback selector beyond that run (361 distinct store
  tests covered). Full console59 and final owned46 passed. Strict all-target
  Clippy for store/execution/native and `cargo build --locked -p hagency` passed.
  Seven new Rust spec bindings resolve against compiled test inventories.
  agent-spec parse/lint cannot run because the executable is absent; no lifecycle
  pass claimed. Final source whitespace gate passes. Logs are /tmp/hagency-outcome-*
  and /tmp/hagency-callers-{tests,check}.log; repeated tests are not additive counts.
- Updated the execution map and API testing procedure. I18n checked: no browser
  labels/layout changed; the canonical blocked reason preserves the TS operator
  wording. Operator UI, graph terminal resolution, real Codex files/private approval,
  recovery/restart and soak remain open. Separate-fleet approval is still pending.
  No Robrix edit, live restart, additional account/room, external message, provider
  invocation, formatter, commit or PR was performed in this follow-on.


## 2026-09-17 — authorized isolated fleet, ADR166/167, real Codex file/restart

The pending fleet choice is resolved: the operator authorized isolated testing.
Actual qualification exposed and fixed ADR166 approval cursor/body identity,
Robrix native 40-hex approval parsing, and ADR167 Palpo blurhash upload metadata.
A new independent file task now passes owner-button approval, native upload and
encrypted delivery, canonical Done, whole-tree cleanup, and actual Robrix final
reply. Independent authenticated download/decryption proves exact 34-byte content.
Robrix's native Save dialog blocks the headless harness; this is still unqualified.
Clean shutdown followed by explicit generation 2/fresh-session provisioning and
SDK reopen passes a new workspace task/reply, with no duplicate file effect.

The first new fleet's file002 upload remains WritePossible without acceptance;
PID 20841 intentionally retains its original owner after incomplete close. Preserve
it and every historical failure; a separate response never settles an old upload.
The successful instance is local-native-palpo-upload-fixed-20260917T184629Z, serves
on 19431, and the real Robrix test UI/retained owner profile uses automation 19401.
No original remote service or legacy dispatch rows changed. Full details/evidence
and remaining gates are in docs/design/native-execution-parity.md. No cutover.

Final validation: Matrix lib 190, media upload 7, Robrix parser 38; strict Clippy for
Matrix/native, native build and headless Robrix build pass. agent-spec remains
absent despite parse/lint attempts. No formatter, commit or PR. Receive files,
media-unknown recovery, seamless restart and sustained soak still precede Claude,
then Octos. Individual live successes do not complete migration qualification.


## 2026-09-17 — receive-file, continuous recovery, console and bounded live run

Receive001 passes the native encrypted attachment path with the original owner's
exclusive SDK profile, actual Robrix approval, exact37 bytes, canonical Done and
rendered reply. The native file picker remains unqualified. A subsequent repeated
run failed unsupported_event before its first marker. Original whole-tree-stop
proof permitted ADR165 continuation, but the old worker had already exited its
claim loop. The queued replacement never ran and was later superseded during
explicit fresh-session provisioning; preserve the failure and its distinct result.

ADR168 retains fixed refused-notification diagnostics. ADR169 keeps a stopped
continuous worker until exact durable operator resolution, then resumes ordinary
claims. Offline real-process recovery, store16/bootstrap20/bootstrap-lib18, strict
Clippy, locked build and production-caller checks pass. The original diagnostic
cannot be recovered retroactively. Separate unknown-upload owner PID20841 stays.

The updated isolated fleet then completed12 tasks over1,018.689 seconds: canonical
Done, completed dispatch, exact file bytes, encrypted delivered reply and no lease
left behind for each. The per-pulse UI substring check was insufficient because it
also matched owner prompts; a later independent visible whole-message audit proves
all12 replies, with seven actual Robrix snapshots/captures. This bounded owner-DM
run does not replace group isolation or full sustained qualification.

ADR170 adds native stopped-work discovery/inspection and three operator decisions.
The UI keeps private credentials in memory, requires notes and a distinct continue
instruction, and retains identical requests for manual retry after response loss.
English/Chinese coverage is included. New API projection and focused UI5 pass;
static Next build and actual Chromium commit/lost-ACK/explicit-replay pass. Full
console60 passes serially. Preserve the earlier parallel59/1 real-agent startup
failure and isolated pass; cause is unknown. Strict browser-feature Clippy and
locked native build pass. The isolated service is on generation4/session004 for
live recovery testing with the original SDK stores. No formatter, commit or PR;
agent-spec/task-writer remain unavailable and no lifecycle pass is claimed.


Live ADR169/170 continuation now passes: actual console inspect/note/continue,
same service PID93924, local Codex replacement, exact workspace bytes, canonical
Done, encrypted final reply visibly rendered in Robrix and zero leases. Original
unknown dispatch remains. The intended timeout stopped early on unsupported_event;
a separate pinned-binary probe then identified terminal_interaction in13.272s
with observed whole-tree cleanup. ADR171 adds that scoped progress event after an
offline red regression. Neither test diagnoses the first soak's lost category.


ADR171 final: full runtime96 (session43 included), strict browser-feature
runtime/native Clippy and locked binary build pass. The same original-owner
pinned-binary probe now completes in25.938s with whole-tree cleanup. The updated
isolated service then passes actual Robrix long-command/polling regression in
58.2s: exact35/34-byte files, canonical Done, completed dispatch, zero leases and
visibly rendered encrypted final reply. Active PID15469 serves generation5/
session005 on19431 with final console assets; retained PID20841 is untouched.
Final caller and whitespace checks pass. Notes distinguish all completed gates
from open multi-agent, sustained, sandbox-denial, media/restart and native-picker
qualification. Claude and Octos remain next in the operator's ordered migration.


## 2026-09-17 — actual deadline and same-service UI continuation

Timeout003 on the unchanged generation5/session005 service wrote its before-file
then ran a360-second command against the300000ms host operation budget. The
original dispatch `matrix_dispatch_539908d9247376eea355f0ba64d4aa75` stopped at
300043ms from claim to original stopped-inventory receipt, with owned_failure
`deadline`, cancellation recorded and whole-tree cleanup proved. The exact
before-file remains; the after-file does not exist. Its three-entry original
inventory also includes the earlier terminal regression files.

Actual Chromium displayed the stopped dispatch and hashes in English and Chinese,
with no horizontal document overflow. After an explicit note and new instruction,
the same PID15469 claimed `continuation_c8dd3eea-d4e8-499d-86ba-6732b49f1bb8`.
Local Codex wrote/read `timeout-003-proof.txt` with SHA256
`4c64602c7191bbe6ff0f2fbc9015792e3ab2bdf8cb74343a58409c57abf05ce6`, completed the
canonical task, released all leases and delivered the encrypted final reply
`ISO_CODEX_20260917_TIMEOUT3_OK`, independently matched as an exact visible Robrix
message. The original dispatch remains outcome_unknown; no restart was needed.

Private evidence under the successful isolated root is
`timeout-003-{stop-evidence,inspection-evidence,resolution.result,completion-evidence}.json`,
its original intent, actual English/Chinese console captures and the Robrix
`scene_timeout003_complete.png`. This closes the actual operation-deadline and
explicit UI continuation case. It grants no unknown-media settlement or seamless
restart; original PID20841 and all historical failures remain preserved.


## 2026-09-17 — project adoption and local Codex factory integration

ADR172 adds optional project_inbox to existing-account adoption. It authenticates
project membership under the agent token as well as the observer, binds distinct
verified project/DM routes and refuses session aliases. Chunked Matrix bodies
stop at the existing byte ceiling before EOF. Focused3 and regression library42,
bootstrap20 and CLI8 pass. Initial fixture queries used nonexistent view columns/
tables; corrected to actual route/workspace schema before passing. Strict Clippy
found duplicate fixture modules; reuse the existing library fixture module.

ADR173 removes the local-provider factory exclusion and carries the original
retained provider directories into per-agent warm Hosts. Provision scope must
match the configured unmanaged Codex/OpenAI resource; original directory checks
cover initialization, idle, activation and handoff. Warm startup remains capped
at30seconds with its own absolute deadline; active dispatch/owner waits use the
existing operation limits. The active handoff still checks approval fit.

The actual native configured-service fixture passes two local-profile agents,
two simultaneous task rounds, independent encrypted file receive/send/approval
and final reply attribution. No managed account/login state or auth-file copy is
created. All eight warm selectors pass across the suite and focused correction:
seven initially passed, and the budget test then passed after its fixture used
equal2s phase/RPC durations so the earlier original phase deadline is exercised.
The prior3s phase/2s RPC fixture correctly hit Protocol at the earlier RPC timeout;
no production deadline was enlarged. The first configured test had a misspelled
configuration key, so its failure is not a behavioral red regression. Strict
all-target browser-feature native/execution Clippy passes. Caller check passes.
Ordinary tests used only local peers and synthetic provider files.

The authorized new live fleet root is local-native-palpo-fleet-20260917T212730Z.
Its initial registration encountered an explicit429; retain the result and retry
only after the scripted cooldown. Existing live PID15469 and unknown-upload owner
PID20841 remain unchanged. No live multi-agent pass, formatter, commit, PR or
agent-spec lifecycle success is claimed. Factory group-session intake remains a
separate missing join after its current per-agent DM route.


## 2026-09-17 — Palpo rate-limit interruption and bounded read recovery

The new isolated root3 created three accounts/four rooms after its explicit
registration429 cooldown, adopted the authenticated project/DM routes and started
PID95426 on19432 with ADR173. At21:33:15.721 UTC intake returned Remote(429);
root2 refresh independently returned Remote(429) at21:33:15.857. Both continuous
drivers stopped, transports became unavailable and their DM scopes advanced to
unavailable generation2. The exact failing HTTP routes were not retained; a later
sync200 does not identify them. The target1 request was sent after this failure,
so no target engagement or model task was admitted. No live two-agent pass.

Original read-only counts: root2 completed18/unknown3/superseded1, leases0; root3
only its adopted coordinator, completed provision1, dispatches0/leases0. Both
services closed cleanly (root2 PID15469/session98580 and root3 PID95426/session26702,
exit0); original credentials/SDK stores/workspaces/evidence remain. Explicit new
transport generations6/2, fresh session/workspace IDs and restored DM generation3
are staged. Original unknown media PID20841 is never closed or retried.

ADR174 bounds HTTP JSON GET429 retries to four complete attempts under one
original absolute request deadline. Valid delay hints use the larger delay;
missing hints start at1second with bounded exponential backoff. Invalid hints,
exhaustion or insufficient remaining time return429. Cancellation interrupts
waits. POST/PUT/binary upload/download and unknown transport never retry. Actual
TLS3 pass, including unchanged URL/authentication, hint/deadline/count/cancel
bounds and no mutation retry. Broader tests are running; no live recovery pass
is yet claimed. Earlier configured-service regressions both passed on this Mac
when explicitly selecting the historical Linux-gated tests, and final bootstrap
library42/bootstrap20/CLI8 passed. No formatter, commit or PR.


## 2026-09-17 — bounded read recovery and actual two-agent DM qualification

Final Matrix validation passes224 tests: library193 (including rate-limit3),
transport9, token-provision9, upload7 and download6. The serial library run took
733.80s; all original test handles completed. Strict all-target native/Matrix/
execution Clippy with browser features and locked native build pass. Initial
commands used nonexistent feature/test-target names; those command failures are
retained and not test failures or passes. No ordinary tests contacted live services.

Both isolated services were re-adopted under fresh transport/session authority,
with their original SDK stores and a restored DM generation3 after unavailable2.
They run the same retained `bin/hagency-adr174`, SHA256
`58e215add042a2b150221dd14df42413bbb604ba42471a0472c102af05a8282b`.
Root2 is PID17433, port19431, transport6/session006; root3 is PID18059, port19432,
transport2/session002. Both report ready. Previous failed instances and their
clean-close receipts remain. The original unknown-upload PID20841 stays retained.

Root3's real native factory created two agents in the same project, each with its
own encrypted DM, SDK owner, original local Codex process, home and workspace.
The retained target1 request was processed after restart without resending it.
The actual owner joined both new DMs, and actual Robrix submitted two tasks per
agent. Four dispatches completed with canonical Done, exact different bytes at
the same relative isolation-marker.txt path, encrypted delivered replies bound to
the correct sender/DM and zero leases. Round2 observed both original tasks Started
concurrently. The authenticated unencrypted project history reaches m.room.create
and contains none of the private markers; no group task was submitted.

The initial UI audit clicked sidebar coordinates but captured the other selected
room and failed. Preserve this harness failure; its two completed tasks were not
resent. Stable named tabs, whole-message equality and actual selected-room captures
then establish all four final replies. Snapshots also contain hidden-tab widgets,
so their coordinates alone are insufficient. The round1 resumed observation took
6.18s and round2 52.08s; the former is not original task latency and neither is a
throughput claim. Evidence is `two-dm-completion-evidence.json`, per-round receipts,
original/resumed scripts and logs, actual Robrix frames and
`project-private-marker-audit-complete.json`. The first short-page audit alone did
not prove history exhaustion; the final audit explicitly reaches room creation.

These results qualify the bounded two-agent private-DM task path. Factory
shared-room task intake, multi-agent file/approval interaction, sustained soak,
positive sandbox-denial witnesses, unknown-media recovery, seamless restart and
native picker UI remain. Codex remains first, then Claude, then Octos. No formatter,
commit, PR, production cutover or unavailable agent-spec lifecycle pass.


2026-09-17 ADR175 diagnostic follow-up: root3's first live two-agent file
qualification failed after the four passing DM tasks. Agent2 dispatch
`matrix_dispatch_f725a71c4da3f308c39ad469a2df2e44` stopped with protocol failure
before creating its requested file. Agent1 dispatch
`matrix_dispatch_bcbaade1ffed388adfb12cb21682e0bd` reached actual Robrix approval;
Approve once was visually inspected and accepted. Its upload
`upload_36f8ed2571d3485baa560d10f96d532c` is accepted, but delivery
`file_4007b768d804c87280d643183d9732bd` is write_possible/publication_refused.
Read-only protected-journal inspection shows accepted key sharing, followed by
an encrypted room write without a retained response. This does not prove server
acceptance or rejection. Both task outcomes and their two leases remain unknown;
PID18059 and its SDK/file owners are retained, and neither operation is resent.
Evidence: root3 `file-001-failure-original-evidence.json`, original UI intent/log,
actual approval frames and `file-001-readonly-custody/summary.jsonl`.

The factory capabilities projection omitted its original workers' existing
bounded statuses. ADR175 exposes each registered engagement/status only through
the authenticated operator endpoint and records fixed runner/publication failure
categories at the original failure. Arbitrary Matrix snapshot text is excluded.
This is diagnosis, not recovery or acceptance of either historical failure.
Native library44 and actual local-Codex configured-fleet1 pass, with the two
historical Linux-gated selectors still ignored in this run (their prior explicit
Mac runs remain separate evidence). Strict all-target native Clippy passes.
No formatter, commit, PR or unavailable agent-spec lifecycle claim.

Root2's session006 restart now also passes actual Robrix task execution:
`matrix_dispatch_f750eeaac3ed6c706715c937314bf546` completed with canonical Done,
exact restart-006.txt bytes and visibly rendered encrypted final reply. Evidence:
`restart-006-completion-evidence.json` and selected-room frame. This qualifies
explicit fresh-generation recovery, not seamless old-session resumption.

A separate authorized diagnostic fleet is being prepared under private cache
`local-native-palpo-fleet-diagnostics-20260917T221627Z`, planned port19433, while
all original services remain preserved. Its approval registration received an
explicit429; the original result is retained and only the bounded documented
post-refusal cooldown may permit another registration request. No live new-fleet
success is claimed yet. Multi-agent files/approval isolation, shared-room task
intake and remaining Codex gates are still open before Claude and Octos.


2026-09-17 ADR176 shared Matrix request pacing: the separate ADR175 diagnostic
fleet (`local-native-palpo-fleet-diagnostics-20260917T221627Z`, port19433,
PID62001) admitted target1 but later received a coordinator intake Remote429;
target2's provision effect remains uncertain. No model dispatch occurred. The
original protected snapshots and effect remain in that state directory. SIGTERM
closed this original service with observed exit0; no failed effect is replayed.
This is an operational quota failure, not proof of either earlier Codex/file
failure's exact cause.

Add optional `matrix_request_interval_ms` (10..1000) to the trusted driver config.
One endpoint-bound opaque scheduling owner travels through cloned Matrix Limits
for coordinator, approval bot, account/room provisioning, created agents and
both media transports. Waiting shares the original deadline and cancellation;
all JSON/binary requests and allowed GET retries are paced. POST/PUT/unknown
writes remain single-attempt. Absent config preserves existing cadence, and
other processes on the same IP remain outside this process-local scheduler.
No UI text changed. See ADR176 and its task contract.

Focused TLS pacing3 pass; native library45/configured-local-fleet1 pass, with the
two historical Linux-gated fleet selectors ignored in this run. Strict native/
Matrix all-target Clippy and locked build pass. The first focused test harness
incorrectly acquired the shared FIFO mutex to read its next slot, which observed
a later request's reservation and produced two false timing failures. Preserve
that output; the corrected tests measure actual received attempts against the
original monotonic schedule, with no production timing relaxation. A broader
Matrix run finished with117 passes and the same two original harness failures;
the corrected focused run has three passes. These are separate runs, not a
single green119-test suite. Production caller audit and diff check pass. No formatter,
commit, PR or unavailable agent-spec lifecycle claim.

Root2 session006's completed single-agent qualification closed cleanly (PID17433,
exit0) to reduce test traffic; its SDK stores and all earlier evidence remain.
Original unknown owners PID20841 and PID18059 are still retained. Fresh isolated
paced fleet `local-native-palpo-paced-20260917T223343Z` now runs on port19434,
PID83315, binary `bin/hagency-adr176`, SHA256 `429874dad4333c6d9c275adbb4223ac26f13c41d9d190493b5f534323b52a0d5`, at250ms per Matrix request
shared across the whole fleet. Native provisioning and live two-agent tests are
in progress. No paced live success, group-task parity or full migration completion
is claimed yet. Continue Codex before Claude before Octos.


2026-09-17 paced fleet follow-up: both target agents became Active on root5
`local-native-palpo-paced-20260917T223343Z`, PID83315/port19434. Robrix's sidebar
omitted the second already-joined room; its actual Join with a link / Go to room
flow opened the exact room without replacing the original client SDK. The first
UI-selection attempt sent no task. The subsequent two-task run completed agent1
but agent2 failed before Operation handoff: its private status is worker, without
protocol/cleanup observations. The unstarted lease expired back to Queued; no
model result or stop proof is inferred. Keep this original owner and queued input.
The running old binary cannot recover the underlying error; ADR175's handoff
amendment records it on future attempts without changing admission or retry.

Agent1 then passed a fresh actual Robrix file request in the same paced service.
Dispatch matrix_dispatch_1c1493e2960577dec1005af24b77c6ec completed canonical Done;
actual private Approve once was accepted, upload_ff4d95056f4189564709fd967eefbba5
is accepted, file_c0291e5b451309235fbd13f4aaec0182 is delivered, and the selected
Robrix DM visibly renders the39-byte attachment and exact final reply. A read-only
extraction of its protected descriptor followed by authenticated owner download
and independent AES-CTR decryption verifies exact requested bytes, SHA256
bcda919d05397a782afb3a0cb409875849259b6e26b5d9182a4026d0b0019f96. No SDK reopening,
file resend or native picker claim. Evidence: root5 file-001-agent-1-completion-
evidence.json, file-001-media-byte-evidence.json and actual selected-room frames.
The Python crypto import failed before any request; Node's standard crypto
completed the independent verification. This is one-agent file evidence, not
passing two-agent approval isolation. Root5 now has2 completed,1 queued, leases0.

A separate disposable /bin/sleep guardian audit observed1127 live-owner responses
over120 seconds and an original Requested stop with whole_tree_stopped. It does
not reproduce or explain root5's earlier handoff failure. The audit's original
process closed; no live fleet owner was signalled. Offline handoff regression,
45 library tests, configured positive fleet test, strict Clippy, build and caller
audit pass. No formatter, commit, PR or unavailable agent-spec lifecycle claim.

A fresh authorized fleet with the handoff diagnostic build is being provisioned
under `local-native-palpo-handoff-20260917T225847Z`, planned port19435. Its explicit
approval-registration429 is retained; the documented cooldown precedes another
request. Existing unknown owners PID20841/PID18059/PID83315 remain retained.
Full two-agent Codex, group task intake, sandbox witnesses, unknown-media recovery,
sustained multi-agent soak and picker UI remain; continue Codex, then Claude,
then Octos. The Rust migration is not complete.


23:04UTC operational update: root6 coordinator exhausted GET429 retries during
target1 provisioning; target1's effect remains uncertain and target2's request
received an explicit429. No runner dispatch existed. Original PID20970 closed
with observed exit0; state/SDK/effect snapshots and close receipt remain. Its
owner-join watcher ended after joining only target1, not a two-agent success.
Rather than add another polling process, the authorized root5 fleet created a
separate target3 under its existing250ms process-wide cadence. Target2 and its
Queued task remain untouched; this is a new participant, not recovery of target2.

Root5's selected pair (NativePacedCodex1 and NativePacedCodex3) now passes four
actual Robrix DM tasks in two rounds, labeled1 and3 by the private harness. Both
rounds observed concurrent Started tasks. Each has canonical Done, separate exact
workspace bytes at the same relative path, correct encrypted sender/DM final
replies and zero leases. The selected-room captures visibly show all four final
messages. Evidence: pair13-two-dm-completion-evidence.json and pair13-dm-round-*
receipts. The global fleet remains degraded by preserved target2; participant
status checks do not claim a green global /ready. Concurrent approve/deny file
qualification for this exact pair is now in progress.

2026-09-17 ADR177 file-settlement guidance: the pair's actual file002 decisions
were Approve once for agent3 and Deny for agent1. A new card shifted the UI after
the harness captured it; its original intent and a correction record preserve
the actual participant identities. Agent1 completed with the exact DENIED reply,
with no upload or delivery admission. Agent3's original upload was accepted and
file_5494bd6a099001fd47e5e3b315ea13d4 settled Delivered, but its final reply claimed
FAILED from an earlier outcome_unknown inspection. This is not a fully passing
file workflow. pair13-file-002-observed-results.json records the original final
messages, receipts and decisions; no completion-evidence pass is invented.

Possible writes deliberately project OutcomeUnknown while their original owner
may still progress. Preserve that status and single-attempt semantics. ADR177
adds fixed model-facing text to nonterminal MCP results and the catalog: inspect
the same delivery_id about once per second within the original deadline and
authority; never resend or recapture, stop at terminal/authority loss/expiry, and
report unresolved if still unknown at expiry. Structured receipts stay identical.
No scheduler, wait loop, completion authority or UI/i18n strings changed.

Focused held-upload/held-event regression passes; combined native library45,
file-service7 and MCP6 pass. Strict all-target native Clippy with browser feature,
production caller audit and diff check pass. Live model compliance still needs a
fresh task using the new binary. Preserve unknown owners20841/18059/83315 and
target2's original Queued input. No formatter, commit, PR or agent-spec claim.

ADR177 live follow-up: root2's completed session006 had cleanly closed. The first
session007 adoption refused at room because the stored unavailable room was
already generation4. Preserve that description and refusal. Fresh authenticated
observation restored generation5 at transport7, new session/workspace007; no old
route or task was revived. PID56393 used the ADR177 binary SHA256
156f6da48fb35a76bd58db75c558b9d5abc7f30267395d55cc4d45827f5886ac and one shared
1000ms Matrix request cadence.

Actual Robrix file004 passed: dispatch matrix_dispatch_019adab87a4908539083b03cddf6081c,
approval_d0510c13e7a70163e673e4b1f08c65bc11ec69f5 Approve once, one accepted upload,
file_21bba09562001d7af79116f18c7c7c9e Delivered, canonical Done, zero leases and
exact encrypted final ISO_CODEX_SETTLEMENT_004_OK. Actual selected-room capture
visibly renders the31-byte attachment and final. Independent authenticated owner
download/AES-CTR decryption verifies exact bytes, SHA256
cf6636ef773426e85533dee74b6c8f8ec72a6980ba1ce7b5c8bdbbebb6055f1f. No provider
intermediate tool transcript was retained, so do not claim a live witnessed
unknown-to-delivered polling sequence. The deterministic held-response test
covers that boundary. Evidence: root2 file-004-completion-evidence.json,
file-004-media-byte-evidence.json and native-session007-close.receipt.json.
Original session007 SIGTERM/wait exits0 after capture; SDK state is preserved.

For root5 pair file002, independent media verification confirms agent3's exact
34 bytes, SHA25636b0003e2ab84775b8c5f859d5580b17cb6af39be417c6646b81ab212de65e5b.
Actual selected-room captures preserve agent1 DENIED and agent3's incorrect
FAILED final beside its delivered attachment. A read-only project history audit
reaches m.room.create in19 events, with no private qualification markers or
approval identities. This verifies privacy of those messages, not group task
execution. The earlier incorrect final remains a failed workflow; later success
does not erase it. The full Rust migration remains incomplete.

2026-09-18 ADR178 factory project inbox: the original factory collector now binds
main-room project sessions from its own authenticated current generation. A
separate session per transport/room generation preserves the original runtime,
SDK, private DM and per-agent workspace. Refreshing room metadata retains the
original profile's account/provider/resource restrictions. Default claims still
require encryption; an explicit Group project option also checks the writer's
exact registered project. Exact mentions execute only on the addressed agent.
Outgoing preflight rechecks the captured generation before sending. Automatic
thread discovery is still outside this slice; no UI/i18n text changed.

Offline evidence: Matrix197, owned-claim12, native library45 pass. The configured
project test observes unaddressed inputs creating no work, two correctly addressed
project replies, then two encrypted private replies with no project leakage.
All5 configured fleet tests now pass together, including repeated media and both
account provisioning paths. Two older tests had obsolete non-Linux cleanup
expectations; they now require actual successful cleanup on macOS as accepted by
ADR029, and both pass. Prior newly enabled fleet runs failed with peer EOF; one
isolated run refused initial intake with Domain. Subsequent isolated/full runs
passed. Keep that intermittency unresolved; new synthetic-only failure output
records protocol method names and receipt stages. No deadline or production guard
was relaxed. Strict Clippy/browser-feature build and caller audit pass.

Fresh authorized root7 is local-native-palpo-project-20260917T235232Z, planned
port19436. Pinned dirty-source ADR178 binary SHA256
e0a1326191b0905a5160942c4cbf2413a87247e08391a390f1b70e41a3dad47b; base commit
67e0e84abcf2676b0097de6f3f456cec908be5fb. Setup uses1s operator request spacing and
will use1s shared native Matrix pacing. Initial approval-account registration
received an explicit429, retained before cooldown. Live project qualification
has not passed. Unknown owners20841/18059/83315 remain untouched. No formatter,
commit, PR, unavailable agent-spec lifecycle or full-migration completion claim.


2026-09-18 root7 startup refusal: registration cooldown completed and all three
accounts/four rooms were created. Two adoption attempts returned Matrix before
any local domain rows; independent native read probes then authenticated and
verified every observation, and a fresh adoption succeeded. Do not assign a
specific historical HTTP cause to the earlier generic errors. PID30514 started
ADR178 with1s pacing, but fresh approval enrollment cancelled at its original20s
SDK deadline; the process observed exit1. No dispatch/task/lease exists. Target1
was posted before successful startup and was never admitted locally. Preserve
all requests, failed receipts and original SDK; do not replay it as a task.

ADR179 exposes optional matrix_sdk_timeout_ms within the existing10..60000ms
limits, selected only before startup. Default20s, HTTP deadlines and runtime/task
budgets stay unchanged. A separate real configured local-TLS test reproduces the
20s paced enrollment cancellation; its longer-budget half first fails because the
old binary rejects the new field. The new host configuration is now under test.
Fresh local-only root8 is local-native-palpo-budget-20260918T002128Z, planned
port19437,60s original SDK budget plus1s pacing. No live success is claimed.


ADR179 validation: paced startup retains the real20s cancellation and proves
separate60s original enrollment with exactly five writes, no tasks and clean
shutdown (53.11s). Configuration tests2, library46, CLI8, configured fleet5 pass.
Bootstrap19/20 passed concurrently; plaintext-refusal roundtrip reached its
original command-approval timeout, then passed alone without any guard/budget
change. Retain this intermittency alongside the earlier configured-fleet timing
failures. Strict Clippy, locked browser-feature build, caller audit and diff pass.
No formatter, commit, PR or unavailable agent-spec lifecycle claim.

Authorized root8 setup and adoption succeeded after an explicit registration429
cooldown. Original PID49126/port19437 now runs the60s SDK budget with1s shared
pacing; pinned binary95dfbb4ad58c8f59357ac90195b2fd464c9f4d233396489bb8acbf2f444ab6e6
from dirty source at base67e0e84abcf2676b0097de6f3f456cec908be5fb. Wait for actual
startup and coordinator polling before posting target provisioning requests.
Root7's SDK and failed pre-start target1 request remain preserved; root7 exited1.
Unknown owners20841/18059/83315 remain untouched. Live project qualification and
full migration are not yet complete.


ADR179 live startup passed on root8: approval enrollment completed, coordinator
enrolled and polled, then both target requests were admitted (target2 only after
its explicit429 cooldown). Both original target engagements became Active and
the owner joined their distinct DMs. This proves the new startup budget on Palpo,
not a passing two-agent runtime. Agent1 en_0fdf6515c686af36df17b56c549f007c stopped
before any task with generic OutcomeUnknown; the existing fixed diagnostic does
not identify that pre-operation stage. Agent2 en_36af0975ac6f207e9a6f51ab7b592b6b
created its project session at generation3 and polled it.

Authenticated owner HTTP with explicit m.mentions posted unaddressed project001.
Agent2 admitted it with wake0 and zero tasks; the actual Robrix selected project
capture shows the message. An exact mention then selected task
matrix_task_97c70b9793e0e8a070dbf81d227242d3 on that original project session, but
its warm runtime lost authority before Started. Dispatch became Superseded,
canonical task stays Created, replies0, leases0; report protocol=not_started,
cleanup=unknown, initialization transport=host_closed. This is a failed execution,
not a successful project reply. Private project-001-unaddressed-evidence.json and
project-001-refusal-evidence.json preserve both outcomes. No model/file task was
replayed and the first agent was not revived. Original PID49126 remains retained;
its original first Codex child still existed at the read-only process census.

Current investigation is the retained runtime/guardian boundary. A private
read-only probe runs the original macOS process census/tracking logic against
its own lifetime; it neither signals processes nor opens live SDK/domain owners.
No historical root cause is inferred from a later probe. Full Rust migration,
two-agent live project replies, sustained qualification and Claude/Octos remain.


Read-only macOS census follow-up: the original unmodified native census/tracking
logic completed1768 samples over60.026s without a failure. It does not explain
root8's historical lost authority. A later churn harness used an unavailable
/bin/true path and exited126 before producing a qualified probe result; no
remaining read-only probe process was found. No production guardian semantics
were changed. The final selected Robrix project capture visibly contains the
unaddressed message and exact addressed request, with no fabricated reply.
Root8 remains retained on PID49126; live two-agent execution is still failing.


2026-09-18 (Claude, operator-directed: Codex end to end first). Root8's lost
authority is explained and reproduced offline. It is not the runtime/guardian
census logic failing against its own lifetime; it is any process on the host
whose parent exits between two censuses. The macOS guardian takes a whole-system
census about every 25 ms, the tracker refuses an unexplained ancestry, and the
guardian then stops the owned tree with ObservationFailure. The warm idle loop's
next `qualify_ready_owner` reads that as LostAuthority, which is sticky, and a
later dispatch gets it as its negative finalization: exactly root8's tuple
(lost_authority, not_started, initialize, cancelled, host_closed, cleanup
unknown), 240 ms after the claim (attempt 00:34:50.417Z, warning 00:34:50.657Z).
Root8's copied domain store rules the other candidates out: registration and all
three engagement generations are 1, all engagements active, all provision effects
complete at fence 1. Agent1's pre-task generic OutcomeUnknown is the same event
with no dispatch queued. In a fleet each agent's own short-lived commands have
this shape for every other agent's guardian, and so does a concurrent cargo run.
The earlier 60-second read-only census passed because nothing churned, and the
churn harness never ran (/bin/true does not exist on macOS; it is /usr/bin/true).

Offline reproduction against the real supervisor: an idle `leaf` owner passes 48
observations at 100 ms; with `sh -c '(sleep 0.4 &); exit 0'` running beside it,
the second observation fails and the stop receipt loses whole_tree_stopped.

Fix, ADR029 session-scoped group evidence amendment: the leader starts with
POSIX_SPAWN_SETSID, and when ancestry stalls the tracker classifies a newcomer by
a process it shares a group with in the same census. Sound because setpgid never
crosses a session and XNU's fork allocation skips a PID that still names a live
group or session (checked in kern_fork.c). A group with no classified member or
with conflicting members classifies nothing and the refusal stays. New tests:
`native_macos_group_evidence_classifies_unseen_parent` (unit),
`native_macos_unrelated_churn_keeps_descendant_proof` and
`native_macos_unseen_parent_in_owned_group_is_stopped` (real supervisor; new
`unseen-middle` probe mode). Both integration tests fail on the previous tracker
and pass 3/3 with the change; the whole hagency-platform package passes 34 and
strict clippy is clean. No live service, account or retained owner was touched;
root8 was read only through a copy of its domain store. The live two-agent
project-room run on Palpo/Robrix has not been repeated with this change.


2026-09-18 (Claude) live two-agent runs with the ADR029 group-evidence tracker,
fresh isolated instances on the existing Palpo, local provider-owned Codex.

Instance groupfix (port 19438, binary from cde89a0c). The second agent idled ten
minutes beside concurrent cargo builds and kept its warm runtime; its addressed
project-room task was claimed and reached `running`, which root8 never did. The
tracker change holds live, and the later stop proved `whole_tree_stopped`.

Correction to the earlier entry: root8's FIRST agent (generic OutcomeUnknown
before any task) was not the guardian. It is a second defect, reproduced here at
the same moment: when the second agent joins the shared project, ADR153 advances
the room generation, the first agent's already-resolved `project_<id>_1_2` plan
is retired by its own intake, selection returns RunnerAuthority, and the driver
mapped that silently to OutcomeUnknown. Only the last agent to join a shared
project survived. Fixed in the driver (ADR178 amendment): a plan the fresh
resolution no longer names is superseded, not refused.

Instance inboxfix (port 19439, with that change): the driver logged the
superseded `_1_2` plan as the second agent's account joined, the first agent
resolved `project_<id>_1_3` as a current route and stayed receiving. Validated
live only; a deterministic offline regression test is owed. The coordinator then
ended with `refresh`/`timeout` on Matrix intake while provisioning the second
agent, which stayed `reserved`. Adoption needed five attempts (`Error: Matrix`).
Palpo's limiter keys on the connecting IP, and every client behind the Caddy
proxy shares the Docker gateway address, so one bucket serves Robrix, every
retained service and the operator scripts. `rc_registration` and `rc_message`
are config, and `per_second = 0` disables one (hoops.rs); not changed here.

The groupfix task itself failed as `protocol` / `unsupported_event` /
`refused_notification: thread_status`. Cause: the operator's Codex account has
exhausted its usage limit (provider message: try again 2026-09-24 09:40). The
captured 0.154.0 order is `thread/status/changed{systemError}`, then
`error{usageLimitExceeded, willRetry:false}`, then `turn/completed{failed}`; the
session died on the first and never read the cause. ADR036 amendment: accept
`systemError` during a running turn as a status only, so the existing arms end
the turn as Failed. New selector
`native_codex_session_outcomes_system_error_precedes_its_cause`.

No live Codex task can complete until the provider limit is lifted. Both
instances were closed cleanly with SIGTERM after their evidence was copied; no
earlier retained owner was touched.

Hosted run for cde89a0c: Ubuntu failed the Rust bindings check because
task-rust-macos-descendant-parity binds macOS-only selectors (three original,
three new) that no Linux inventory contains; this spec had never been pushed.
The checker now defers a spec tagged `macos`, `linux` or `windows` on other
platforms, and the hosted job on that platform still resolves it. macOS failed
`native_configured_fleet_media_two_agents` and `..._executable_two_agents`
("timed out during both original native helpers are in flight": one handoff
`deadline`, one `peer_unavailable`, both with `whole_tree_stopped`). These
fixtures had never run hosted and pass locally 6/6; unresolved, matching the
intermittency ADR178 already records. Windows is paused.
