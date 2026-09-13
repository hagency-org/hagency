---
kind: decision
id: ADR-124
title: File and resolve ceiling overrun alerts from the drawn figure
status: Proposed
---

## Context

Slices 1–3 ported the ceiling draw, refusal wording and admission rule
(ADR-121/122/123). ADR-123 listed the overrun alarm (gap G5 of the port plan)
as the one remaining gap, deliberately deferred because it needs new
infrastructure — native has no alert table and no periodic sweep at all —
rather than another join. The retained JavaScript sweeps hourly
(`backend-v2.js:9393-9452`, `CEILING_OVERRUN_SWEEP_INTERVAL_MS` at `:9350`),
filing `agent_ceiling_overrun` when `drawn > ceiling` and auto-resolving when
the draw falls back under, with an alert record in `lib/alert-store.js` whose
four actionable fields (owner, runbook, impact, recoveryCondition) are what
keep it a `warning` rather than a silent `info`.

## Decision

Slice (a) of the alarm plan (brief 8) delivers storage, sweep and resolution;
publication and the hourly trigger are slice (b).

**Storage.** One table, `ceiling_alerts` (migration 024), one row per dedupe
key `agent_ceiling_overrun:<resource_id>`. The row carries the retained
wording verbatim (summary/runbook/impact/recovery_condition with raw numbers,
never `compactTokens`), `detail` as a JSON string capped at 4096 bytes,
`occurrences`, `first_seen_ms`/`last_seen_ms`, and `resolved_at_ms`/
`resolved_by` (NULL = open).

**Sweep.** `DomainRepository::sweep_ceiling_overruns(&mut self, now: u64) ->
Result<SweepOutcome, Error>` as one `Immediate` transaction over every
resource with a declared finite ceiling, reading the read-side projection
`usage::ceiling_report` (`resource_ceiling`, ADR-121) — the SAME drawn rule
admission enforces on (`max(reserved, spent)`, unknown falls back to
reserved, `backend-v2.js:14052-14053` cited by both), though a distinct code
path from `budget()`/`resource_budget`. That is exactly the retained split:
Node's sweep reads `ceilingSpendFor` while admission reads `remainingFor`
(`backend-v2.js:9402` vs `:14823`). The two agree by shared rule and the
oracle, not by construction. Strictly
`drawn > ceiling` raises; `drawn <= ceiling` auto-resolves with
`resolved_by = 'system'`; a repeat against an open row increments
`occurrences` (and, like the retained store's dedupe path
`alert-store.js:231-249,254-271`, refreshes summary/detail but never
rewrites the four text fields — only a fresh insert writes them); a re-over
after resolution reopens the same row; resolved rows
older than 7 days are pruned (`ALERT_RESOLVED_TTL_MS` parity). No timer, no
route, no console change: the sweep cadence is the caller's concern until
slice (b), and the async `DomainStore::sweep_ceiling_overruns(now)` wrapper is
the `sweepCeilingOverrunsForTest` shape — tests drive it directly.

**A resource that loses its ceiling.** The sweep skips a resource with no
declared ceiling (`backend-v2.js:9397-9400` `continue`) — correct for raising
(an absent ceiling is unknown, not zero) — which also means auto-resolve is
never reached for it: an alert opened while the ceiling existed, then the
ceiling removed, stays open. Node behaves identically but offers manual
resolve routes on its alert surface; native slices (a)/(b) have no operator
close path, so the row persists until the ceiling is redeclared. This is
parity in mechanism with an asymmetry in escape hatches, the same class as
the episode-model divergence below; an operator close path is future work.

**Oracle.** The `sweeps` vectors are computed by EXECUTING the retained
`lib/alert-store.js` `createAlertStore` (fake clock, in-memory save) — not a
transcription: `ingest`'s `created` flag and `autoResolve`'s return derive
raised/updated/resolved from the store's own outcomes, and the fixture pins
`alertStoreSha256`. The one non-encoded transition is the reopen-window
divergence below (Node mints a new record outside 5 minutes; native reopens
the one row), which native's own replay test pins.

**Divergence from Node's episode model.** The retained store is an id-based
episode log: a re-over within the 5-minute reopen window reopens the same
record, a later re-over mints a new record (`lib/alert-store.js:254-271`).
Native uses **one row per dedupe key**, reopened on any re-over regardless of
the gap. The reason: slice (b) will publish on the dedupe-key resource
dimension, not on individual alert episodes, and there is no alert-ID surface
to justify preserving episodes yet. The contract that matters is unchanged —
one open alert per resource, occurrences riding on it — and the simplification
is recorded here so slice (b) can revisit it if an operator-facing alert-id
surface ever appears.

**What auto-resolve must never do.** An alert is diagnostic, never
enforcement: raising one must not revoke or end engagements, block or permit
admission (admission already refuses by its own rule), release leases,
complete owned dispatches or mark tasks Done (canonical completion and final
reply custody are as out of bounds as admission), or
authorize retries. Auto-resolve flips the row's display state and nothing
else. "Resolved" is a display state on the alert record, full stop.

## Consequences

Bounds: 7-day resolved retention, 4096-byte detail cap, sweep cadence is the
caller's concern until slice (b). No admission, refusal, deadline, lease or
engagement behavior changes; `resource_ceiling`/`CeilingReport` are reused
unmodified. The oracle gains `sweeps` vectors computed by the retained
JavaScript (pinned by `alertStoreSha256` alongside the existing hashes) so the
native sweep and the retained sweep agree on the same seeds. The seven tests
(`tests/ceiling_alerts.rs`) exercise every transition; the SQLite-backed ones
cannot open their fixture in the peer sandbox (cap-std ancestor EPERM) and run
under CI.

## Amendment: slice (b) — publication and the hourly trigger

**Consumer list (this slice).** One consumer ships: the operator read
`GET /api/native/v1/alerts?limit=` (`hagency/src/alerts.rs`, mounted in the
existing `api/native/v1` chain), with exactly the `usage.rs` operator
authority (bearer token plus local management authority) and its refusal
mapping, including `Busy → 503 "busy"` (`native-api.js:56` maps the same
pair client-side, so a 429 would have fallen through to `native_unavailable`).
The response is
`{"at_ms": …, "alerts": [...]}` with snake_case keys; every retained field is
published (dedupe key, resource id, summary, the parsed `detail` object,
runbook, impact, recovery_condition, occurrences, first/last seen, resolved
state), open rows only, newest activity first, limit defaulted to 100 (the
retained `listAlerts` default, `alert-store.js:410`) and refused outside
`1..=200` (`MAX_OPEN_CEILING_ALERTS`). That bound is a DELIBERATE divergence:
the retained route clamps an over-large limit (`Math.min(parseInt(limit) ||
100, 500)`, `alert-store.js:410-412`); native refuses it with
`Error::Invalid`, matching every other bounded read behind this boundary — a
silently-clamped limit hides a client bug, a refusal surfaces it — and the
native cap (200) is tighter than the retained (500) for the same bounded-cost
reason. A corrupt `detail`
string is `Error::Schema` surfaced as `503 alerts_corrupt` — never a silent
null. The route publishes what the sweep wrote; it never re-derives the draw.
The real future consumers on this store read are the retained SSE echo
(`alert_created`/`alert_updated`/`alert_resolved` over `/api/stream`,
`backend-v2.js:1866`) and the console page; neither ships in this slice.

**Cadence and its rationale.** The sweep loop starts in `Bootstrap::serve`,
beside the other background owners, and stops in `close`. The production
period is `CEILING_SWEEP_PERIOD` = 3600 s — the retained hourly cadence and
its rationale (`backend-v2.js:17499-17504`): an overrun is a standing
condition nobody requests, and a tighter loop would only re-file the same
alert. Tests override it via `Bootstrap::with_ceiling_sweep_period` or drive
`start_ceiling_sweep` directly with a short period. The period is carried as
a const plus builder because that is this service's existing pattern for
background-owner tuning: the CLI surface (`main.rs`) passes a fixed default
and no other bound reaches `Bootstrap` as a parsed config value — wiring an
env/file knob here would be the first of its kind, so it is left to a real
config surface if one ever lands. (tokio's `interval` fires the first tick
immediately, so production also sweeps once at startup — an overrun present
at boot is filed promptly; stated rather than discovered.)

**Abort-vs-commit on shutdown.** `close()` aborts the loop handle BEFORE the
domain writer shuts down. A tick aborted at its await point is skipped or
committed, never torn: a job the writer has not dequeued is dropped whole
(`reply.is_closed()` skips the operation, `domain_worker.rs:2379`); a job
already dequeued runs its single `Immediate` transaction to `commit()`. So
"abort" means no NEW effect beyond the current tick's atomic unit — a sweep
may commit shortly after its caller is gone, which is harmless because the
rows are diagnostic, idempotent by dedupe key, and independent of the reply.

**Refusal-on-tick rule.** On `Busy` or `OutcomeUnknown` the tick logs the
refusal code with the `[ceiling]` prefix and waits for the next tick — never
an in-line retry, never blocking admission traffic: the sweep is idempotent
by dedupe key, so a missed tick is harmless. The same rule covers a failed
SQLite acquisition. The `Busy` arm is pinned by the loop test (a saturated
capacity-1 writer queue yields the store's own mpsc `try_send` refusal,
observed as `Refused("busy")` on the watch); the `OutcomeUnknown` arm needs a
writer parked past the 2 s reply bound, whose only seam is test-private to
`hagency-store`, so that arm is exercised by the store-side suite rather than
this loop test. The loop exposes a `tokio::sync::watch` of the last
`CeilingSweepTick` (`Swept(outcome)` / `Refused(code)`) so tests await
transitions without sleep-based polling.

## Amendment: the console consumer (the console alerts read slice, 63ef17cf; planned in the brief-12 port plan)

**Consumer.** The native console's alerts page ships as a READ-ONLY triage
surface: `hagency/src/console/alerts.rs` (`GET /console/api/alerts?limit=`,
mirroring `usage.rs` exactly — recheck after the store answers, the console
`failed()` mapping, `Busy → 503 "busy"`, statement-time `at_ms`, no-store from
the boundary) mounted beside `usage::router()`; client
`mockup/lib/native-api.js` `validateAlerts` (exact-key, both directions) +
`fetchAlerts` + `alertsView`; `components/NativeAlerts.jsx` behind
`app/alerts/page.jsx`'s native branch; rail enabled; staged by
`build-native-console.mjs`. Polling rides `Data.jsx`'s existing 15 s refresh
(the retained console's own cadence, `Data.jsx:308`) — no new timer, no SSE:
the native console has no SSE channel and the retained console never used one
(it is a curl/API consumer; the retained SSE echo, `backend-v2.js:1866`,
remains future work on the same store read).

**Derived fields.** `severity: "warning"` and `status: "open"` are derived at
the console handler, never stored: every `agent_ceiling_overrun` ingest is a
warning (`backend-v2.js:9422-9447`), and the store read is open-rows-only by
construction. This is why the native page has one open-count strip and no
status filter where the retained page has five.

**No actions.** The native store has no operator close path (the lost-ceiling
asymmetry above), so the console renders no transition buttons and delete
stays refused — controls that would 404 lie. An operator close path, if ever
added, is its own reviewed slice with its own authority; until then the
console shows the four actionable fields and the raw figures, and resolution
is what the sweep does when the draw recovers.

**Validator contract.** The wire item carries exactly thirteen keys; the
client validator's exact-key list must match or the page never reaches ready
(a stale server or client fails loudly instead of rendering half a page).
`detail` is the parsed payload object OR a truncated JSON string (the
retained `truncatePayload` rule ported at the store): the validator accepts
the union and the page renders the string arm as text.

**Truncated detail must publish, not 503 (console review E1).** The retained
rule slices the JSON STRING (`alert-store.js:61-64`), so an over-long row
legitimately holds invalid JSON, and the retained consumer passes it through
as text (`mapAlert`, `mockup/lib/api.js:203`). The store read therefore
PARSES when it can and falls back to the raw string (`Value::String`)
otherwise — never `Error::Schema`. One truncated row must not blind the
operator to every good row, and the client's string arm is exactly the live
payload for that case. Pinned end to end by
`native_console_alerts_publish_truncated_detail` (store read AND console
route) and the write side by the truncation unit at `detail_json`.

**One alert type, and a known cliff (console review E2).** Every console row
renders `warning`/`open` because exactly one alert type exists natively
(`agent_ceiling_overrun`): the route DERIVES both fields and the migration
has no severity/status column. The client's hard equality checks
(`severity === 'warning'`, `status === 'open'`) make a future non-warning
row REFUSE the whole read (`invalid_native_response`) rather than misrender
it as a warning. That is the intended failure mode: a second alert type
requires a migration (a severity column), a route change, and a validator +
page change, in that order, in the SAME slice — the exact-key list is
load-bearing for semantics, not just shape.

## Amendment: the operator close path — one server-owned transition map (brief 25)

The alarm slices left the lifecycle at auto-resolve only; this amendment adds the operator display-state
transitions the brief-24 design specified, over the **four-state subset** the ceiling alert honestly
carries: `open`, `acknowledged`, `resolved` (terminal), `suppressed`.

**One map, owned by the server.** The legal-transition table exists exactly once
(`hagency-store`'s `ALERT_STATUSES` + `allowed_transitions`), and every consumer derives from it: the
store write, the operator route, the console route (which SERVES each row's legal set as `next`), and
the page — buttons render only from the served `next`, so a client-side map can never disagree with the
server's and a transition the server refuses is never offered as a control. **The retained console's
drift is not ported**: its `NEXT_STATUS` (`mockup/app/alerts/page.jsx:31-37`) offers
`acknowledged→suppressed` and `assigned→suppressed`, which the retained store itself refuses
(`lib/alert-store.js:8-14`), and hides `suppressed→assigned`, which it allows — one concept, two
disagreeing client maps. Native takes the opposite rule: the map is served, never re-declared.

**The state subset and its named divergences.** `assigned` is dropped — it would require an assignee
column and the retained agent-token authority (`backend-v2.js:16104-16113`), which the native boundary
(operator bearer + console session) does not have. Native ADDS `acknowledged→suppressed` and
`suppressed→resolved` (the retained console's intent, made legal by the one map). Suppression carries
NO window natively — it is operator-released only (`suppressed→open`); the retained 24h expiry
(`ALERT_SUPPRESS_DEFAULT_MS`) is a clock feature the native sweep honestly lacks, so the retained
re-over behavior (`alert-store.js:245-248`: a suppressed row reopens on a new occurrence only once its
`suppressUntil` has passed, staying suppressed inside it) becomes the simpler native rule: the sweep
NEVER reopens a suppressed row — occurrences ride, the operator releases. A second re-raise divergence:
the retained store reopens a resolved row only inside `ALERT_REOPEN_WINDOW_MS` (5 min,
`lib/alert-store.js:258-265`) and otherwise files a new alert, while native always reopens the same
dedupe row as a fresh episode. An acknowledged (or
suppressed) row that recovers auto-resolves exactly like an open one (`resolved_by='system'`), matching
the retained `autoResolve` (`alert-store.js:341`: any non-resolved status). The client validator
(`mockup/lib/native-api.js`) keeps its own `ALERT_STATUSES` — it must refuse an unknown state rather
than misrender — which mirrors `hagency_store::ALERT_STATUSES` and must move in the same commit as the
store's list.

**Display state only.** A transition mutates the alert's render columns (`status`, `note` ≤2048
operator text, `transitioned_at_ms`/`transitioned_by`, and `resolved_at_ms`/`resolved_by` on the
terminal hop, the actor like the retained `meta.actor || 'operator'`) and nothing else — no admission,
lease, engagement or retry consults them (the standing rule of this ADR). Notes carry operator text
only; the read's existing private-value checks cover them. The migration is 025 (head 24→25) with every
schema-enumerating assertion moved in the same commit; the store write is one `Immediate` transaction
behind the single writer, refusing `bad_transition`/`not_found`/bounds like every other write.

**Oracle.** The transition vectors in `ceiling-vectors.mjs` are EXECUTED by the retained
`lib/alert-store.js` (fake clock, sha256-pinned, like the sweep vectors), covering the five pairs legal
in both models plus the shared terminal refusal; native's additional pairs are pinned by its own
store test. `tests/alert-store.test.js:59-78`'s suppressed-stays-suppressed half is encoded; the
window-expiry half is the named non-goal above.
