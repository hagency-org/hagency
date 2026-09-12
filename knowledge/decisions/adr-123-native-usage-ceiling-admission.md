---
kind: decision
id: ADR-123
title: Admission uses the drawn ceiling and publishes headroom
status: Proposed
---

## Context

ADR-121 computes the read-side draw; ADR-122 names refusals. Until this
decision, the native approve path still compared the request against
commitments alone, ignoring measured fresh spend entirely, and clients saw no
headroom figure — the same two gaps the retained JavaScript closed with
`remainingFor` (`backend-v2.js:14036-14060`) and by publishing
`remainingTokens`/`tokensDrawn` on the usage rows.

The retained tests pin the behavior: a 10M ceiling with 13.6M consumed of
which only 681k is fresh must still approve 1M (cache reads never draw);
10M fresh must refuse it; and after approval the published headroom is the
figure the decision itself used.

## Decision

Slice 3 of the ceiling-enforcement port.

**Admission.** `DomainRepository::approve` folds the ADR-121 report into the
headroom, mirroring `remainingFor` verbatim: `drawn = max(reserved, spent)`
with unknown measurement falling back to `reserved` (never zero);
`by_ceiling = ceiling.saturating_sub(drawn)`; the admitted figure is the
minimum of the non-null limits (ceiling after draw, declared seat quota,
pool), and a seat-period mismatch nulls the whole figure
(`backend-v2.js:14057`) rather than falling back to the pool. The engagement
being decided is excluded from its own commitment sum exactly like the
retained `decide()` call (`excludeEngagementId: id`); approve is the operator
verdict path, so the auto-join seat tightening does not apply. The previously
hardcoded `exclude_engagement_id: None` / `for_auto_join: false` in `budget()`
are now parameters (`resource_budget` keeps the old public shape).

**Refusal context.** The slice-2 `SpendContext` now carries `spent`,
`consumed` and `spend_period_key` from the same report, so `Error::OverCommit`
names the binding draw, the period key and the cache-read discrepancy exactly
as the JavaScript message does.

**Publication.** `UsageReport` gains `ceiling: UsageCeiling { tokens_drawn,
tokens_used, remaining_tokens }` — the authority's own figures, computed from
the same report and budget the decision uses (no exclusion: publication
states current headroom including the engagement's own commitment, mirroring
the retained `remainingFor(agent)` reads). Every existing key is unchanged.

**Oracle.** `ceiling-vectors.mjs` records the end-to-end admission
expectations from the retained tests: lockout (approve 1M; remaining after
approval 9.0M), fresh exhaustion (refuse; 0 left). The fixture stays
sha256-pinned to the retained ledger, backend and engagement store.

## Consequences

Admission semantics change only where measurement exists: an unmeasured
resource behaves exactly as before (commitments-only). The ceiling overrun
alarm (gap G5 in the port plan) is deliberately **not** implemented here; it
needs new alert infrastructure and is a listed follow-up.

Tests: `native_ceiling_admission_uses_drawn_not_consumed`,
`native_ceiling_admission_refuses_fresh_exhaustion`,
`native_ceiling_publishes_headroom_after_approval` (store-level; need a
repository open). The usage API projection test extends its exact-key list
with `ceiling`.
