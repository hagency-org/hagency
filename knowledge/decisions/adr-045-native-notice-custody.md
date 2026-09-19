---
kind: decision
id: ADR-045
title: Verified task notices commit send-start before transport
status: Accepted
---

## Context

A claimed task notice can already have reached Matrix when its claim expires, so scheduling and send-start need distinct durable states.

## Decision

Verified task acknowledgements previously returned a complete route at claim and
accepted delivery directly from that claim. Claim expiry could replay an already
sent private acknowledgement. Schema 14 adds a one-shot Sending state, attempt
fence, immutable task epoch and source event, cancellation bit and inspection
receipts to that same notice row. There is no second task or outbox owner.

Claim is scheduling only. The host must begin the send under its exact secret,
lease, current route and task epoch before using the frozen snapshot. A lost
begin response is uncertain. Restart or expiry requeues only unstarted claims;
Sending becomes Uncertain. A possible send cannot be claimed again until an
authenticated host inspector proves delivery or proves no send was accepted.
NotSent requires actual evidence; timeout and a stable Matrix transaction ID are
insufficient. Inspection uses the exact attempt fence and a durable receipt.

Promotion, transport/registration changes and explicit cancellation retire the
old send authority. Late delivery can be retained as observed history, including
an event that escaped cancellation, without activating cancelled or retired
canonical work. Canonical task activation, Matrix acceptance, process start and
Done remain separate. Legacy notice methods refuse verified notices.

Old development rows have no task-epoch/send-start proof. Migration preserves
delivered history, fences unsent verified notices, and conservatively retains
previously claimed verified rows as uncertain. It never adopts current task or
room state as proof for old output. This is a migration of isolated development
stores, not an import of live Node data.

These durable transitions do not themselves make a Matrix send private. The
actual adapter must coordinate cancellation, current membership and encryption
recipients immediately before external IO, and inspect accepted-but-lost sends.
Cancellation cannot retract an event already accepted by a homeserver. No live
adapter is enabled here; actual recipient/sync/crypto qualification remains open.

## Consequences

Sending becomes uncertain after owner loss and requires exact authenticated inspection before any new grant. Cancellation cannot retract an event a homeserver already accepted.

## Alternatives Considered

Acknowledging delivery directly from a claim or requeueing every expired claim could repeat a private notice. Timeout and transaction-ID stability are insufficient evidence for a NotSent transition.
