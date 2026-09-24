---
kind: decision
id: ADR-069
title: Retain exact native session usage counters before policy attribution
status: Accepted
---

## Context

The typed Codex runtime previously discarded scoped token counters that downstream usage attribution needs to inspect without reconstructing transcript JSON.

## Decision

The pinned Codex0.153.4 protocol exposes thread/tokenUsage/updated with threadId,
turnId and tokenUsage total/last breakdowns plus modelContextWindow. Source:
codex-rs/app-server-protocol/src/protocol/v2/thread.rs lines1834–1918 at commit
3d2ee51ca2d5db578f328aa75e20aa22c0197c9a. Runtime previously accepted the scoped
notification as Progress and discarded its counters in the optional observation.

Project it only after the existing actual SessionDriver state validates scope.
The default Update remains Progress. An opaque UsageEvidence in the existing
ObservationKind retains total and last counters separately, under that exact
source instance/thread/turn and sequence. No caller can mint an Observation or
replace its source. Clone retains historical evidence; source retirement remains
visible and a future ledger adapter must not mistake it for current authority.

Counter fields preserve absence and null as unknown and reject negative, floating,
string, boolean or values above9,007,199,254,740,991. Missing cache-write input is
unknown here even though the upstream Rust serde type defaults that field to zero;
this projection records received evidence rather than creating it. Fixed flags
expose missing, invalid and unsupported fields. Existing frame/JSON/event limits
precede projection and its result has constant size. Unknown raw values and fields
are discarded; raw source metadata is not retained or printed. Metric shape errors
produce uncertain metrics without changing the runtime's accepted Progress event.
Wrong thread/turn or lifecycle still fails through the original state machine.

These are upstream counters, not normalized token totals. Input may include cached
input; total and last are not additive; context-window capacity is not consumption.
No arithmetic relationship is inferred here, and complete fields can still be
contradictory. Downstream normalization must preserve those contradictions and
bind the exact owned execution before using ADR063. No fake transcript JSON is
constructed. Runtime has no metering/store dependency or quota side effect.

The existing progress attachment records usage receipts for sequence continuity
but emits no tool status or completion from them. This extends its exhaustive
ObservationKind match only. Actual stream fixtures prove optional values,
uncertainty, same-text different-driver identity, retirement, sequence gaps and
quiet progress. This is the typed capture seam, not durable usage ingestion,
provider-authenticated billing or completed M7 parity.

## Consequences

Opaque evidence retains optional total and last counters under the original session source and sequence. Counters remain untrusted observations, and their presence proves neither complete capture nor quota authority.

## Alternatives Considered

Adding total and last counters or treating context-window capacity as consumption would invent arithmetic. Fabricating transcript input or deriving authority from textual IDs would lose the original runtime evidence boundary.
