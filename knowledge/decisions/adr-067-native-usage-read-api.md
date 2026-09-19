---
kind: decision
id: ADR-067
title: Operator-only aggregate usage observations
status: Accepted
---

## Context

Operators need bounded usage aggregates without exposing private source handles or confusing observed totals with authenticated billing.

## Decision

The native management API adds GET `/api/native/v1/engagements/{id}/usage` behind
its existing exact loopback Host and operator bearer gate. Origin and forwarding
headers remain refused. Responses are no-store. There is no anonymous, runner or
project-side equivalent and no HTTP observation/source mutation.

The optional `at_ms` query selects an observed UTC day and month; omission uses
the writer's current time after queue admission. The entire query is at most128
bytes and contains at most one exact `at_ms` field. Unknown or duplicate fields,
empty/nondecimal/out-of-range values and UTC years outside1970..9999 are refused.
Time selection changes only a read projection, never the ledger's observation
clock or attribution. Unknown engagement IDs return NotFound.

One bounded writer operation reads the engagement and its summary/daily/monthly
periods without interleaving another domain operation. Existing ledger bounds
limit work to128 sources and exact indexed period lookups. The original queue
and response deadlines remain intact; closed or timed-out state cannot become
a successful zero response. This is not an atomic snapshot against unauthorized
external writers or arbitrary database modification.

The typed report contains the engagement ID, selected timestamp, aggregate
summary and optional daily/monthly rows. No source handle, snapshot digest,
transcript, runtime/task identity, room ID, workspace or credential is exported.
Missing sources/periods remain null, latest observations can regress, historical
known high-water values remain explicitly lower bounds, and incomplete flags and
`host_attributed_untrusted_transcript` evidence remain visible. Reads do not
enforce quotas or claim provider-authenticated accounting.

The capability bit describes this development read API only. Browser console
integration, secure capture, continuous retention and production availability
are not inferred from this endpoint.

## Consequences

The loopback management read preserves null periods, lower bounds and incomplete flags. It adds no source mutation, runner endpoint, quota enforcement or production console integration.

## Alternatives Considered

Returning transcripts, workspace paths or source credentials would exceed an aggregate read. Filling missing observations with zero or treating a selected date as a ledger-clock mutation would change the evidence semantics.
