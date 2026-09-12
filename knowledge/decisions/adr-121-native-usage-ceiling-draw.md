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
