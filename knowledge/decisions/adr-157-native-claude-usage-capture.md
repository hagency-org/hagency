---
kind: decision
id: ADR-157
title: Retain source-bound Claude usage as incomplete cumulative snapshots
status: Accepted
---

## Context

ADR154–156 provide actual native Claude pipes and callback control, but lack the
opaque usage provenance and private ledger attachment already present for Codex.
The local three-runner requirement does not authorize bypassing those boundaries.

## Decision

Use the documented Agent SDK stream semantics as numerical evidence, not billing
authority. Reference, checked 2026-09-16:
https://code.claude.com/docs/en/agent-sdk/cost-tracking
Assistant frames repeat usage across content blocks sharing message.message.id.
Only deduplicated main-loop input/cache counts are accumulated; per-step output
is a placeholder. Result usage excludes subagents. Result modelUsage includes
reported models across the call. This driver accepts one prompt and no resets.
An error result can carry usage but grants no successful task or cleanup claim.

The original driver mints immutable observations after protocol/session checks
and before returning mutable payloads. Its private source combines instance
identity and the validated session ID; no public constructor or deserializer
can recreate it. Attach only immediately after system/init (sequence1). All
subsequent messages, including permission/control events, advance sequence.
Successful private control returns or stdin flushes do not mint stream events.
Close, failure and Drop retire retained source clones. Result leaves the source
live for immediate capture, never a reusable conversation or a process-stop proof.

Bound admission to16384 messages,1024 main-loop step IDs of at most512 bytes,
and64 model entries. Exact counters are optional integers at most2^53-1. Keep
known subtotals separately so missing fields cannot hide overflow. Conflicting
deduplicated input/cache evidence, missing step identity, or projection capacity
invalidates capture permanently without changing an otherwise accepted protocol
message. Counters missing or malformed remain unknown with fixed flags. Ignore
subagent assistant frames; never add them to main-loop snapshots. A present but
malformed/empty modelUsage cannot silently fall back to a stronger claim: retain
unknown reported-model evidence. Only absent/null modelUsage uses explicitly
main-loop result usage. Result snapshots replace accumulated step snapshots.
No model names, message IDs, raw metadata, billing estimates or private text enter
the metering observation. Unknown fields are discarded with fixed diagnostics.

Pure typed normalization has a separate versioned Claude evidence field, skipped
when absent so existing Codex/transcript serialization is unchanged. Claude input
already excludes cache categories; do not apply Codex cache subtraction. Output
is forcibly unknown for step coverage even if a caller supplies it. Every runtime
observation is incomplete. Digest means sanitized content identity, not authenticity.

The private UsageRun admits family-specific sources only under the acknowledged
Started resource family, retains ordering and retirement guards, and uses the
same single pending tuple and content-bound writer receipts as ADR070. Final
usage must enter that slot before capture closes. Retry uses only the retained
tuple, never a new source/event. No source restoration permits fresh attachment.

## Consequences

Offline evidence separately covers real native pipes, exact driver provenance,
pure arithmetic and actual domain persistence. Production Host still refuses
Claude; this is not managed account readiness, live provider qualification,
Mac descendant containment, Matrix owner approval or sustained soaking.

## Alternatives Considered

Summing assistant and result totals double counts. Deduplicating by outer UUID
misses repeated content blocks. Treating result usage as all-agent usage loses
subagent consumption. Parsing fabricated transcripts loses source custody and
mixes legacy arithmetic. Opening Host admission now would skip unrelated gates.
