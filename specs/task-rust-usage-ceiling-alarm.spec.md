spec: task
name: "File and resolve ceiling overrun alerts from the drawn figure"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, metering, budget, alerts]
---

## Intent

A resource drawn past its declared ceiling raises a stored, operator-readable
overrun alert; the alert auto-resolves when the draw falls back under. The
figure is the same drawn ceiling admission enforces on, so the alarm and the
decision can never disagree about whether a resource is over.

## Constraints

### Must
- Sweep every resource with a declared finite ceiling in one atomic
  transaction, reading the same `ceiling_report` admission uses (never a
  second arithmetic path).
- Raise strictly on `drawn > ceiling`; treat `drawn <= ceiling` as recovery
  and auto-resolve with `resolved_by = 'system'`.
- Store one open alert per resource dedupe key, with `occurrences` incremented
  on repeat and the same row reopened on a re-over after resolution.
- Carry the retained wording verbatim for summary, runbook, impact and
  recovery_condition with raw numbers, and `detail` as a JSON string capped
  at 4096 bytes.
- Prune resolved rows older than 7 days.
- Pin the sweep state machine to the retained JavaScript through the ceiling
  oracle (`sweeps` vectors, `alertStoreSha256`).

### Must Not
- Do not treat a resource with no declared ceiling as over (unknown is not
  zero).
- Do not report exactly-on-the-ceiling as over.
- Do not let auto-resolve revoke or end engagements, block or permit
  admission, release leases, or authorize retries — an alert is diagnostic,
  never enforcement.
- Do not attach a timer, route or console change in this slice; cadence is the
  caller's concern until slice (b).
- Do not change the draw rule, admission decisions, refusal wording, deadlines
  or engagement state.

## Boundaries

### Allowed Changes
- native/hagency-store/src/migrations/024-ceiling-alerts.sql
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/ceiling_alerts.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/ceiling_alerts.rs
- native/hagency-store/tests/fixtures/ceiling-vectors.json
- native/scripts/ceiling-vectors.mjs
- specs/task-rust-usage-ceiling-alarm.spec.md
- knowledge/decisions/adr-124-native-usage-ceiling-alarm.md
- docs/progress.md
- ./Cargo.lock

## Acceptance Criteria

Scenario: The sweep files a warning with every actionable field
  Test: native_ceiling_alert_sweep_files_warning_with_actionable_fields
  Given a resource committed above a ceiling that was lowered under it
  When the sweep runs
  Then one open alert carries summary, runbook, impact, recovery_condition and the raw detail numbers, and admission still refuses by its own rule

Scenario: Measured fresh spend over the ceiling raises with a tiny commitment
  Test: native_ceiling_alert_measured_over_raises_with_nothing_committed
  Given fresh measured spend above the ceiling with cache reads deliberately huge
  When the sweep runs
  Then the drawn figure is the fresh sum, never the committed-only figure and never the four-kind total

Scenario: Inside and exactly-on raise nothing
  Test: native_ceiling_alert_inside_and_on_boundary_raise_nothing
  Given a resource at or below its ceiling
  When the sweep runs
  Then no alert is filed

Scenario: Recovery resolves without an operator
  Test: native_ceiling_alert_resolves_when_draw_falls_back_under
  Given an open overrun alert and a draw now back under the ceiling
  When the sweep runs
  Then the row is resolved by system

Scenario: Repeats ride one alert, not several
  Test: native_ceiling_alert_dedupes_across_repeated_sweeps
  Given a resource still over its ceiling
  When the sweep runs repeatedly
  Then one open alert remains with occurrences incremented

Scenario: A resource with no ceiling is skipped
  Test: native_ceiling_alert_no_ceiling_resource_is_skipped
  Given a resource with no declared ceiling
  When the sweep runs
  Then it is not reported as over

Scenario: The native sweep matches the retained JavaScript
  Test: native_ceiling_alerts_match_javascript
  Given the regenerated oracle vectors computed by the retained sweep
  When the native sweep replays each seed
  Then raised, updated, resolved and final-row state agree, including the month-rollover unknown-not-zero rule

Scenario: The alerts read requires operator authority
  Test: native_alerts_read_requires_operator_authority
  Given the alerts route behind the operator boundary
  When a request arrives without the token, with a wrong token, with a foreign origin or forwarded header, or with an invalid limit
  Then it is refused exactly as the usage read is, and no other method is admitted

Scenario: The alerts read publishes open ceiling alerts
  Test: native_alerts_read_publishes_open_ceiling_alerts
  Given two seeded overruns swept into open alerts and one resolved alert
  When an operator reads /api/native/v1/alerts
  Then every retained field is present with the parsed detail object and raw numbers, resolved alerts are absent, and the limit is respected newest-first

Scenario: The sweep loop runs on its period and survives a busy writer
  Test: native_alert_sweep_runs_hourly_and_survives_busy
  Given the sweep loop on a short injected period and an open alert
  When one tick is observed, the writer is held busy so a tick is refused, and the hold is released
  Then the loop keeps running and the tick after release sweeps again, awaited on the observation hook without sleeping

Scenario: Resolved alerts are pruned after seven days
  Test: native_ceiling_alert_prunes_resolved_rows_after_seven_days
  Given a resolved row older than the retention window, a younger resolved row, and an open row
  When the sweep runs past the cutoff
  Then the old resolved row is deleted with pruned asserted, the young resolved row is kept, and the open row is never pruned

Scenario: A resource that loses its ceiling keeps its alert open
  Test: native_ceiling_alert_lost_ceiling_keeps_alert_open
  Given an open overrun alert whose resource then loses its declared ceiling
  When the sweep runs
  Then the resource is skipped as unknown for raise and resolve alike and the alert stays open, matching the retained sweep

Scenario: An over-long detail truncates instead of aborting the sweep
  Test: native_ceiling_alert_detail_truncates_like_retained_store
  Given a detail JSON string past the 4096-byte cap
  When it is composed
  Then it is sliced to the cap the way the retained truncatePayload does and the sweep continues

Scenario: The console alerts read requires a browser session
  Test: native_console_alerts_read
  Given the console alerts route behind the console authenticate hoop
  When a request arrives without a session, with a forged or duplicated cookie, with foreign host/origin/forwarded headers, or with a foreign, zero, above-cap or repeated limit
  Then it is refused exactly as the usage console reads are, and the default and bounded limits publish the seeded open alert

Scenario: The console publishes the fixture's open ceiling alert
  Test: native_console_alerts_fixture_publishes_open_alerts
  Given the fixture's commit-then-lower overrun swept once before the store starts
  When a sessioned console read runs
  Then every wire field is present (derived severity warning and status open, the parsed detail figures, the four actionable fields, occurrences, first/last seen) and a resolved alert is absent after the ceiling is restored

Scenario: A truncated detail publishes as text instead of failing the read
  Test: native_console_alerts_publish_truncated_detail
  Given an open alert row whose detail was sliced to the cap under the retained truncatePayload rule and is no longer valid JSON
  When the store read and the console route publish it
  Then the detail appears as the raw string on the wire with the derived severity and status, never Error::Schema, so one truncated row cannot blind the operator to every good one

Scenario: Operator transitions follow one server-owned map
  Test: native_ceiling_alert_transitions_follow_one_legal_map
  Given one open overrun alert seeded by the sweep over the four display states open acknowledged resolved and suppressed
  When every from-to pair is applied through the store
  Then the seven legal pairs of the one map apply with the actor note and transitioned provenance asserted resolved only on resolve and every illegal pair refuses bad_transition writing nothing

Scenario: The sweep respects operator display state
  Test: native_ceiling_sweep_respects_operator_status
  Given a suppressed row and an acknowledged row under the ceiling rules
  When the sweep re-overruns and then recovers
  Then a suppressed row rides occurrences and is never reopened by the sweep and an acknowledged row auto-resolves like an open one with the operator note preserved and a resolved row re-raised reopens as a fresh episode with display state reset

Scenario: The operator transition route keeps the read's authority
  Test: native_ceiling_alert_transition_route_authority
  Given the operator bearer boundary and one open alert
  When the transition route is called anonymous forged with a foreign header on an unknown key with an unknown state word and through one legal walk to terminal
  Then refusals are forty-one forty-three forty-four and four hundred with the store's own words and the legal walk returns the row itself with provenance per hop

Scenario: The console transition serves the map it enforces
  Test: native_console_alert_transition
  Given the console session and the seeded open alert
  When the list is read and transitions are posted through the console route
  Then every row carries next from the one server-owned map acknowledged narrows it resolved empties it the illegal pair refuses bad_transition and the unknown key is a named not-found

Scenario: Operator transitions match the retained store oracle
  Test: native_ceiling_alert_transitions_match_javascript
  Given the fixture's transition vectors computed by executing the retained alert store over the pairs both models share plus the terminal refusal
  When the native store replays each walk
  Then every status resolution actor and occurrence count matches the oracle exactly

Scenario: Migration 025 upgrades populated 024 rows
  Test: native_ceiling_alert_schema_upgrade
  Given a live store rewound to the 024 table shape rebuilt verbatim from migration 024 carrying one open and one resolved row in 024 columns only
  When the repository reopens twice and replays migration 025
  Then user_version is 25 the resolved row backfills to resolved with an empty next the open row serves open with the full three-way next the note is absent on both and the sweep still rides occurrences on the upgraded open row
