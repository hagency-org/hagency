---
kind: decision
id: ADR-121
title: Native read-side resource ceiling draw
status: Proposed
---

## Context

Migration plan item 8 leaves quota enforcement open. The retained JavaScript
decides a ceiling by two competing figures — commitments to active engagements
and measured fresh spend for the ceiling's period — combined as
`max(reserved, spent)` with an unmeasured period falling back to the
commitment figure (`backend-v2.js:14012-14060`, `remainingFor`). A ceiling is
drawn down by fresh tokens only (input, output, cacheWrite; operator ruling
2026-08-12 pinned by `tests/ceiling-draws-fresh-tokens.test.js`); cache reads
are measured and shown but never draw.

The native side has the evidence (ADR-063 usage ledger, per-engagement period
buckets) and the commitment arithmetic (ADR-025 `allocation::resource_budget`),
but nothing joins them: native admission checks `ceiling − committed` only and
its refusal variant names neither the binding draw nor the preset to raise.

## Decision

Slice one of the ceiling-enforcement port adds a read-side projection only:
`DomainRepository::resource_ceiling(resource_id, at)` returns a
`CeilingReport` mirroring the retained JavaScript `ceilingSpendFor` object —
`reserved` (SQLite `SUM(tokens)` over `reserved`/`active` engagements of the
resource, aggregated like `budget()`), `spent` (fresh-kind sum of the current
period's per-engagement `known_growth` lower bounds), `consumed` (the display
sum over all four kinds), `ceiling_tokens`, `preset_name`, `spend_period_key`,
and `drawn = spent.map_or(reserved, max)`.

The period granularity comes from the resource's ceiling declaration, monthly
unless `daily`. An absent period bucket is `None` — unknown, never zero — so
the commitment figure stands alone; that is the difference between "nobody
measured this period" and "nothing was consumed". All sums are bounded by
`JSON_SAFE_MAX` and fail closed with `Error::Capacity`.

Evidence stays untrusted: the figures are lower bounds over host-attributed
observations, the report carries `UsageEvidence::HostAttributedUntrustedUsage`,
and this slice changes no allocation, admission or publication decision.

## Consequences

The oracle is `native/scripts/ceiling-vectors.mjs`: the retained
`lib/metering/ledger.js` computes `spent`/`consumed` from the same observation
sets (including the BigLittle lockout numbers) and the two retained
`remainingFor` lines are mirrored verbatim for the combination; both source
files are pinned by sha256 so a drifted oracle fails `--check`. The Rust
replay tests are the four selectors in
`specs/task-rust-usage-ceiling-draw.spec.md`.

Later slices (separately specified) fold `drawn` into admission, split the
`no_ceiling`/`over_commit` refusal variants with the binding-draw wording, and
publish headroom; this slice only computes and reports the draw.

## Amendment: the console consumer (brief 18 — publish headroom)

The deferred "publish headroom" follow-through named above. The resources
page's budget read now carries a `draw` object: `committed` (reserved +
active commitments), `measured`/`consumed` (the current period's fresh and
display lower bounds — null when unmeasured, never zero), `drawn`
(`max(reserved, spent)` with the unknown-fallback, the same figures this
ADR's draw and ADR-124's alarm publish — one `resource_headroom(id, at)`
store read returns the commitments budget AND the draw together in ONE
writer job, so the page can never mix figures from two reads), `period`,
`ceilingTokens` and `remainingBeforeCeiling` (null when no ceiling is
declared), and `binding` — the binding draw named the way ADR-122's
over-commit refusal names it (`engagement-store.js:82-83`: `measured >
committed ? "measured spend" : "committed allocations"`), null when the
measurement is unknown because then nothing competes for the ceiling.

Client `validateBudget` grew the same object in the same commit (exact eight
keys, both directions, unknown arms null). The page renders each figure in a
`data-headroom` cell with the explicit unknown word for nulls. Statement
time is the read's clock (the retained budget read has no clock parameter);
the route's no-query rule is unchanged.

## Amendment: the headroom review (brief 20, E1/E3/E4)

**E1 — one committed figure, two predicates, one invariant.** The budget's
`pool.committed` and the draw's `committed` are computed by different SQL —
`budget()` folds `preset_id=? OR seat_id=?` grouped by preset/seat, while
`ceiling_report` folds bare `resource_id=?`. They select the same engagement
set **only because** `resource_id = public_resource_id(preset_id)` is
injective (`project.rs:67-69`: a truncated hash of the preset id). That is
the rule: the two figures are equal by that invariant, never by accident,
and a future non-injective mapping must re-derive one from the other rather
than let two different "committed" numbers share a panel. Pinned by the
shared-seat console test: a second preset on the same seat holding an
approved engagement keeps `draw.committed == pool.committed == 100` on the
first pool while `seat.committed` is deliberately the larger cross-preset
figure (350) — the seat roll-up is the one place the predicates are
*supposed* to differ.

**E3 — the always-zero `reserved` is off the console wire.** The console
route runs `budget()` with `exclude_engagement_id: None`, and the core
assigns `reserved` only inside that arm (`allocation.rs:155-159`), so the
top-level key was a constant 0 beside two meaningful commitment figures. It
is removed from the console wire and from `validateBudget`'s exact key list
in the same commit; the page renders one committed cell (the budget panel's
`pool.committed`, which equals `draw.committed` per E1).

**E4 — the operator route keeps its five-key shape.**
`/api/native/v1/resources/{id}/budget` (the operator API, ADR-067's read
surface) still returns the plain commitments budget without `draw`: its
consumer is machine-to-machine operator tooling that already treats the
budget as allocation facts, and adding measured-evidence figures there is a
separate decision about that route's contract. The console route carries
`draw` because its consumer is a human deciding whether a resource is close
to its ceiling. One concept, two shapes, two consumers — stated here so the
asymmetry is a decision, not a drift.
