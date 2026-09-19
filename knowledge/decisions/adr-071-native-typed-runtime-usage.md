---
kind: decision
id: ADR-071
title: Normalize typed untrusted Codex usage without transcript fabrication
status: Accepted
---

## Context

Typed Codex counters have different cache semantics from legacy transcript observations and require explicit bounded normalization with uncertainty preserved.

## Decision

ADR069 retains actual source-bound runtime observations; ADR063 accepts normalized
usage for separately authenticated host attribution. This pure conversion supplies
the counter seam only. Its public fixed DTOs are untrusted numerical inputs, never
source credentials. Runtime, store and execution do not become dependencies of the
metering crate. Parent ADR070 owns the actual source/dispatch attachment.

Pinned Codex0.153.4 at3d2ee51ca2d5db578f328aa75e20aa22c0197c9a maps
input_tokens_details.cache_write_tokens into cache_write_input_tokens. Its actual
codex-rs/codex-api/src/sse/responses.rs fixture parses input100, cached40, write60,
output10 and total110. Input includes both cache categories. The four ledger kinds
therefore use fresh=input-read-write, read, write and output. Reasoning remains an
output breakdown, never an added category. This intentionally differs from the
retained transcript parser which does not read cache-write and returns zero for it;
that existing parser and its historical oracle are unchanged in this slice.

The version1 typed evidence retains separate total and last breakdowns, optional
context capacity, input projection flags and fixed normalization diagnostics. Each
optional counter accepts at most9,007,199,254,740,991. Unsafe values become unknown
with invalid evidence; absent fields retain missing evidence. Invalid fresh-input
subtraction yields unknown rather than saturation. Independent valid categories
survive. Contradictory total arithmetic or reasoning breakdowns remain visible.
Checked known normalized cumulative category overflow returns Overflow and no
observation; the caller must retain original evidence and report bounded refusal.

The pinned protocol's TokenUsageInfo::fill_to_context_window can replace cumulative
components with zero while setting total to capacity. Core recompute_token_usage
can estimate last.total with zero components. Thus total/last/context never serve
as alternative consumed-token quantities. Observations are independent cumulative
snapshots, not deltas or a sum of snapshots. The durable ledger separately detects
cross-snapshot regression and retains high-water observations.

Every runtime observation has stream_incomplete=true. Exact counters do not prove
all usage events were captured, provider billing, canonical Done or quota capacity.
The digest covers complete sanitized typed evidence under an explicit versioned
domain; no raw metadata or text is retained. Different unsafe raw values that both
sanitize to the same unknown and diagnostics intentionally have equal evidence.
The digest provides content identity only, not authenticity.

An optional skipped-when-absent runtime evidence field leaves existing transcript
observation serialization unchanged. Legacy parse failure and diagnostics behavior
remain intact. No file reading, event admission, storage, live model, HTTP endpoint
or service toggle is introduced.

## Consequences

The pure conversion separates fresh input, cache read, cache write and output using the pinned evidence. Its digest identifies sanitized content only and does not authenticate source or billing.

## Alternatives Considered

Fabricating a transcript record or applying the legacy transcript arithmetic would miscount typed cache fields. Replacing invalid or absent counters with measured zero would conceal incomplete evidence.
