---
kind: decision
id: ADR-055
title: Native bounded transcript token normalization
status: Accepted
---

## Context

Transcript token observations contain missing, duplicate and contradictory untrusted records that must remain visible during native normalization.

## Decision

This implements the parser boundary of the authorized Rust migration and
ADR-013's token-only, missing-is-unknown and separate-cache rules. Input is an
untrusted in-memory JSONL snapshot. No filesystem or provider request is made.
Transcript paths and models are private source hints, never authenticated Agent,
project or task identities. The service does not consume these reports yet.

Claude sums distinct message UUIDs; missing UUIDs remain visibly undeduplicable.
An identical UUID with changed normalized usage is refused. Codex uses the last
cumulative record, reports decreases and arithmetic disagreement, and never
sums repeated last-token deltas. Reasoning remains a breakdown of output.
Same-total changed breakdowns and decreasing cumulative components are separately
marked inconsistent, even when the scalar total still agrees with input/output.
Cache reads remain separate and do not count toward a fresh-token ceiling.

Each token field is optional. Absent or null fields stay unknown, including
through aggregate arithmetic. An empty transcript has no observed usage. Valid
nonnegative JSON integer literals from zero through JavaScript's maximum safe integer are
accepted; coercions, decimal/exponent spellings, negative values and overflowing totals are
refused. This deliberately corrects legacy coercion and missing-to-zero behavior
under the existing unknown-never-zero requirement. Malformed JSON lines remain
skippable but are counted as incomplete evidence. Duplicate JSON keys are refused.
Out-of-range JSON numbers fail the entire observation. The last comparable known
Codex component survives missing-field gaps, so missing data cannot reset a
later decreasing cumulative observation.

Bounds are 8 MiB per snapshot, 16,384 lines, 64 KiB per line, JSON depth 32 and
4,096 values per record, 4,096 distinct Claude UUIDs, 64 model hints and 16
workspace hints. UUID/model strings are at most 256 bytes and workspace hints
4 KiB. Capacity exhaustion fails the entire observation; no partial total is
returned as a complete measurement. A report records malformed lines, missing
fields or expected whole usage records, conflicting workspace hints, undeduplicable messages and inconsistent
Codex arithmetic. These diagnostics cannot be dropped by a future ledger caller.
Known Claude lower bounds accumulate independently of optional-field completeness;
a missing later field cannot hide overflow in already observed token volume.
The line byte bound and JSON decoder recursion guard limit allocation during
decoding. The smaller depth/node limits validate the materialized record; they
are not advertised as pre-allocation or total RSS limits.

Synthetic vectors execute the current JavaScript source for unchanged valid
semantics. Native correction fixtures separately cover missing values, invalid
numbers, duplicate keys/identities and contradictory evidence. No live transcript
is used. Discovery, current runner stream usage, persisted ledger identity,
engagement attribution, quota enforcement and browser projections remain open.

## Consequences

Bounded parsing preserves unknown fields and inconsistency diagnostics instead of manufacturing complete totals. Attribution, quota enforcement and provider-authenticated billing remain outside the parser.

## Alternatives Considered

Summing repeated Codex cumulative records or counting cache reads as fresh input would inflate usage. Treating missing values as zero or returning a truncated complete-looking total would erase uncertainty.
