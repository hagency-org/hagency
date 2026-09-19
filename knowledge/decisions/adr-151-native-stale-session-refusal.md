---
kind: decision
id: ADR-151
title: "Explicit local operator refusal of conclusively pre-session Matrix custody"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
---

## Context

The failed six-minute qualification soak left its seventh synthetic ACK request
on the real server. Re-adoption used a new session/transport generation without
resetting protected SDK state. Actual SDK output then retained that request,
but the new frozen domain scope refused it. Generic admission refusals remain
quarantined under ADR-054/065; their error alone cannot decide why they failed.

## Decision

An explicit local `intake-refuse-stale-session` operator command may settle only
an exact known SDK-derived quarantined batch whose unacknowledged candidates each
have a fresh opaque negative domain proof: exact original stored session/scope,
complete observation digest, no canonical source receipt, and source timestamp
strictly older than that session's persisted ingress boundary. An old route can
establish refusal only, never grant current authority. No model/effect is run.

The owned SDK converts only those candidates into terminal StaleSession source
decisions, retains raw and SDK digest coverage plus already-acknowledged events,
persists the negative ledger first, and uses the ordinary completed receipt.
Identical repeated requests can inspect protected settled negative evidence;
commit loss remains explicit uncertainty. Missing/changed authority, current
timestamps, canonical commits, coverage ambiguity and Prepared/Applying cannot
be cleared by this operation. It neither repairs the source nor emits an ACK.
A human must send a fresh event for new execution.

This is a named successor to ADR-065's deferred typed writer-refusal contract,
not an automatic startup or sweep path. It is not ADR-148 dispatch recovery:
no lease, dirty flag, quarantined runner or orphaned dispatch is cleared or
resumed. The existing native local CLI carries private operator authority for
this separate negative-only SDK custody act; runners/browser text cannot call it.

## Consequences

Known pre-session input can be conclusively refused without deleting SDK state
or silently retargeting it. The failed soak and its rejected seventh request do
not become successful execution or qualification evidence. General recovery,
effective sandbox, ongoing bounded retention and full port parity remain gates.
