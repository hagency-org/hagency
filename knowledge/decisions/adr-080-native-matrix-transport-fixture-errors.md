---
kind: decision
id: ADR-080
title: Preserve early Matrix collector errors in scripted transport fixtures
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Scripted Matrix tests could hide an early collector failure behind a later wait for an HTTP request that would never occur.

## Decision

Native Windows CI at 25c01ee failed the authenticated HTTPS/restart and bounded
sync/cancellation transport tests while waiting for a scripted HTTP request.
Those tests joined collection and the whole HTTP script, so an early collector
failure could remain hidden until the script's existing SDK-plus-HTTP timeout.
The original log does not establish the earlier collector result or its cause.

Use the existing common::scripted driver for both identity collections and the
bounded sync loop. It polls script completion first when both futures are ready,
then retains and returns the actual collector result. If collection completes
before its HTTP script, it fails immediately with the actual bounded result.
It neither retries collection nor turns an early refusal into expected success.
The existing real wrong-account fixture demonstrates the early Identity error;
the real HTTP SDK-gap fixture preserves a legal interval between requests.

No HTTP, SDK, fake-peer, shutdown or production deadline changes. Keep all
request/response, authentication, restart identity, cancellation and negative
authority assertions. This is evidence visibility, not proof that the unknown
historical Windows cause has been repaired. Original CI failure and independent
Palpo shutdown OutcomeUnknown remain failed evidence; later runs are separate.

## Consequences

The fixture reports the original early result while preserving legal request gaps and all deadlines. It improves visibility without establishing or repairing the historical Windows cause.

## Alternatives Considered

Joining indefinitely with the unfinished HTTP script obscures the collector stage. Retrying collection or accepting early refusal as success would replace the assertion this diagnostic must preserve.
