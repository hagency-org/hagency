---
kind: decision
id: ADR-063
title: Durable host-attributed untrusted token observations
status: Accepted
---

## Context

Persistent token observations need exact host attribution and stable high-water accounting despite missing fields, retries and changing observation periods.

## Decision

This bounded migration ports the high-water ledger policy under ADR-013 and
ADR-055. A host-attributed transcript is not provider-authenticated measurement.
No usage row changes allocation, execution permission or quota eligibility.

One source represents the fresh upstream session belonging to one exact native
dispatch/fence. Initial binding requires the private successful Started scope,
exact capability and current writer projection. Agent incarnation, registration,
project, resource and framework come from that scope and existing domain records.
Transcript path, cwd, model hints and supplied names never choose attribution.
The current native launcher starts a fresh upstream session per dispatch; resumed
cross-dispatch or multiple independent cumulative sources require a later explicit
identity contract and are not silently treated as independent usage here.

The source handle is opaque, non-deserializable and host-only. Existing bindings
can be restored by the host after restart; later observations retain historical
attribution after Done or revocation without renewing execution. Restoration is
internal evidence, not proof that arbitrary file bytes came from that execution.
Descriptor/process association, secure capture and discovery remain adapter gates.

Normalization happens before the bounded writer queue. A private snapshot value
is produced only by the existing parser and retains its full diagnostics, optional
counts and static parse failure. Its content digest binds the supplied snapshot;
raw text, workspace and model strings are not persisted by the ledger. Wrong
framework is refused against the bound resource. Parse failure is an idempotent
unknown observation, not a successful zero-token measurement.

Every source retains per-kind optional historical high-water marks, optional latest
counts, full latest diagnostics, receipt history and sticky historical incomplete
evidence. Latest incomplete and regression flags are stored separately from parser
diagnostics because an otherwise clean snapshot can regress across observations. Per-kind decreases record regression while preserving the earlier mark.
Latest values may regress and are labelled separately. Aggregation exposes optional
latest totals and known historical lower bounds, with explicit incomplete-source
and history indicators. All known arithmetic is checked even when another field
is unknown; SQL SUM coercion is not used. Cache reads remain separate from fresh
input, output and cache writes; no monetary interpretation exists.

Record identity is source plus a stable host call ID, content-bound to the exact
normalized snapshot and digest. An identical retry returns its original receipt
before new clock or capacity admission, while conflicting reuse fails. The writer
chooses one checked UTC time after its queue and immediate SQLite lock, refuses
clock reversal, and commits source high-water growth, daily/monthly growth and the
receipt together. Periods describe when growth was observed, never provider billing
time. Explicit zero evidence differs from an absent period. Unknown period delta
components retain known lower bounds without becoming measured zeros.

The first slice has hard finite source, receipt and period ceilings, including
per-source and per-engagement bounds. Admitted identity/high-water and receipt rows
are never evicted. Source disappearance, log pruning or a copied/reappearing log
cannot create a replacement source for the same execution. Source capacity refusal
does not prevent an existing source's permitted growth or exact replay; receipt or
period capacity refusal preserves the entire prior transaction. A full ledger is
explicitly unavailable for new admission, not silently safe for unlimited lifetime.
Automatic archival, period cutoffs and pruning are deferred until there is useful
display detail to discard without losing evidence.

This intentionally corrects legacy ledger coercion/missing-to-zero, repeated loss
of retired source keys, and non-atomic persistence behavior. Positive repeated
high-water and observed UTC growth arithmetic is checked against the actual retained
JavaScript source. Native correction cases are labelled separately from exact parity.
Migration creates no observations for historical tasks or pre-existing source paths.

Exact first-slice bounds are 4,096 source identities globally and 128 per
engagement; 32,768 receipts globally and 256 per source; 32,768 period rows globally
and 512 per engagement across daily and monthly together. Each host call ID is at
most 256 bytes. All observed kinds, known subtotals and aggregates stay within
9,007,199,254,740,991. UTC keys accept only years 1970 through 9999. The existing
single-writer queue remains at most 128 commands and 8 MiB of weighted normalized
commands, with the unchanged two-second response deadline. Timeout is outcome
unknown; the caller must retain the original source/call/content identity. It
never proves rollback and does not justify choosing a new source.

Normalization retains ADR055's 8 MiB snapshot, 64 KiB line, 16,384 line and
bounded identity/JSON shape contracts. A snapshot beyond the outer 8 MiB limit is
rejected before hashing and cannot create a ledger observation; in-bound parser
failure is retained as a fixed failure category plus digest and unknown counts.
Successful parsing retains every Diagnostics field. Raw text and parser model/
workspace hints are discarded before queueing. The ledger is not a provider
recording or cryptographic identity attestation, and its digest is not a secret.

The 16 reproducible JavaScript vectors contain 80 positive known-count snapshots,
including leap-day/month boundaries, duplicates and mixed per-kind regression/
growth. Separately labelled native correction fixtures preserve unknown fields,
explicit zero observations, malformed/duplicate input evidence, changed call-ID
refusal, overflow and sticky incomplete history. Compared with legacy behavior,
an explicit zero observation now creates a period with zero evidence; mere absence
still returns no period. Retention refuses rather than deleting a source key and
then counting its reappearance again. Persistence failure rolls back source,
periods, clock and receipt together instead of leaving mutated in-memory usage.

Repository tests cover all six row ceilings, per-engagement known-overflow despite
unknown latest fields, exact foreign/current/retired source authority, restart,
reappearance and migrations. Capacity fixtures seed bounded historical table
shapes in one private transaction; those synthetic rows do not claim execution
attribution. A new-month case leaves only one free row, proves daily insertion is
rolled back when monthly admission fails, and preserves existing-period growth.
Actual writer fixtures hold SQLite's immediate lock to qualify observation time
and separately withhold acknowledgement before/after real commit. Identical retry
records exactly once under the original unchanged response deadline. No live
provider, transcript scan, transport send, service toggle or quota enforcement is
implemented by this slice.

## Consequences

The single writer commits source, periods and receipts together while preserving untrusted evidence and known lower bounds. Ledger data does not authorize execution, quota eligibility or provider billing.

## Alternatives Considered

Attributing usage from transcript paths or supplied names would trust source hints as identity. Evicting source keys and recounting their reappearance would violate durable deduplication.
