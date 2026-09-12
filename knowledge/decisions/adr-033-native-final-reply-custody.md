---
kind: decision
id: ADR-033
title: Freeze Matrix privacy before session execution and separate final intent from delivery
status: Accepted
---

## Context

Canonical Done, final content admission, transport send-start and authenticated external acceptance are distinct facts with different privacy and recovery requirements.

## Decision

The M3 domain proof separates four facts: a canonical task reached Done, its
current runtime submitted final content, a host transport started a send, and an
authenticated host observed the exact external event. None implies the others.
This implements a bounded foundation for REQ-RUST-MIGRATION-EXECUTION,
REQ-THREAD-SCOPED-SESSIONS and REQ-MATRIX-DM-PRIVACY; it is not Matrix feature parity
or proof that a live client can deliver a message.

Schema 11 adds host-observed Matrix account/device generations and full room
snapshots. A snapshot binds the exact server, project, owner, registration,
explicit Group/Direct classification, joined members, invitation policy and
encryption requirement. Registration and transport generations accompany each
observation. Same-generation changes fail. Per-engagement membership evidence
prevents one account's observation from silently admitting another account or a
rotated device. A later full group snapshot fences absent agents even when another
agent reports it. Group is never inferred from a member count.

Unsafe but otherwise scoped observations invalidate an existing room durably:
missing owner/agent, a third DM member, loss of invitation-only access or encryption,
and incompatible privacy all retire previous routes. The host-only invalidation
command covers missing or unknown room evidence. The future adapter must record
negative observations before allowing further sends, including failed refreshes;
ignoring a failed fetch is not permission to keep sending. Restoration requires
a newer generation, renewed membership evidence and a fresh session. Host-only
observation types do not implement Deserialize and have no runner HTTP endpoint.

Each fresh Matrix session freezes its full route and a local session incarnation.
Legacy schema-10 sessions retain generation zero; migration does not reconstruct
privacy from current state or backfill an old SID. Internal conversation sessions
remain internal. The session uniqueness index retains the kind discriminator and
fresh internal-member incarnation policy from schema 9. Retirement reuses the
existing internal-conversation lifecycle, including descendant closure and
unresolved process custody. Changing room, owner, allocation, device or registration
cannot retarget the frozen route. Main sessions with a null thread root receive
exactly the same protection as thread sessions. After a DM is promoted to Group,
new work requires a fresh group session and old private output cannot begin a send.

A runtime submits only a bounded call ID and UTF-8 body, at most 32 KiB. Current
started dispatch authority must bind the exact canonical Done task/epoch or an
inspected report grant for that epoch. Taskless and legacy/internal sessions fail
closed. One immutable intent exists per task epoch; per-dispatch call receipts
bind the full frozen route, epoch and content digest. Repeating a call with changed
content conflicts. Final intent creation never changes the canonical task or
pretends to acknowledge external delivery. Runtime receipts contain only intent
ID, task ID, epoch, state and replay marker; no room, owner, device or body.

The host claims a current Pending intent with an expiring secret and monotonic
fence. Only a second transactional begin-send exposes the frozen body/route and
stable Matrix transaction ID, and it first persists Sending. A claim lost before
begin-send can return to Pending. Once Sending, timeout, restart, cancellation or
scope retirement leaves Uncertain because an external send may already exist.
The host must not repeat begin-send after losing its response. A transport adapter
must coordinate cancellation and obey the returned encryption requirement; this
slice cannot atomically fence a remote homeserver after external I/O has begun.
It preserves that uncertainty instead of claiming that a message was never sent.

Observed delivery must match transaction, content digest, full server and room,
sender, device and required encryption, plus a valid Matrix event ID. A matching
acknowledgement can be replayed; substitutions conflict. Expired/retired send
claims cannot acknowledge. A separate host inspector can establish Delivered for
an old uncertain route, recording what actually happened, or supply bounded proof
that no send was accepted. A timeout alone is not such proof. NotSent permits retry
only while the original route/task epoch is current and cancellation was never
requested. The cancellation bit survives restart and inspection. Inspection
receipts bind the exact result to the intent/fence, so a lost inspection response
can be retried without creating another transport attempt. Retry keeps the same
Matrix transaction ID and immutable body.

Repository rows and per-session pending work have fixed admission bounds. Commands
pass through the existing single writer, its bounded queue and execution-time
clock. Independent migration verification statements prepare before schema commit
and on reopen. No transport account/device strings, runtime text or inspection
result can mint project authority or execution permission.

This slice intentionally has these limits:

- A first Group observation accepts only the already authorized project room;
  promotion of a known DM is supported. Arbitrary newly invited group-room
  authorization remains to implement.
- Direct conversations currently require the project's exact owner, an encrypted
  invitation-only room and the explicit owner/agent pair. Other authorized humans,
  federated members and generalized DM invitation policy remain to implement.
- Taskless/front-desk final output is unsupported, including its null-root legacy
  path. Null-root canonical task tests do not establish taskless feature parity.
- Existing Matrix task-intent intake still creates legacy session bindings. A real
  authenticated adapter must connect canonical input/anchor admission to the new
  verified-session creation path before production use. In particular,
  messages::find_session currently refuses any scope that already has a verified
  matrix_generation greater than zero. Existing ingestion/task-intent commands
  therefore cannot target these fresh sessions. Tests manually create and finish
  canonical tasks; they do not prove verified Matrix ingress through final reply.
  The next integration slice must add verified-route-aware admission/scheduling
  without upgrading legacy IDs or trusting caller-supplied transport assertions.
- Matrix authentication, sync, encryption, SDK send/transaction inspection,
  immutable file/media delivery, console projections, process cancellation and
  live end-to-end testing are not implemented here. No model or live Matrix
  service was used or changed. M3 and the overall native migration remain open.

#### 2026-09-10: Native sender admission and journal reconciliation

The host can preview only an exact current Claimed reply to match its private
account/room collector before committing Sending. Preview does not change state
or authorize network IO. Before each transport write, a separate host-only check
requires the exact secret/fence, unexpired Sending claim and current frozen route.
These are local authorization checkpoints, not an atomic lock on Matrix.

A host inspector may reconcile a still-Sending row directly as Delivered when a
journaled authenticated response matches the immutable transaction, digest,
server/room, sender/device, encryption and current send fence. Losing the claim
secret does not erase that delivery evidence. Existing durable inspection digests
keep retries idempotent. NotSent remains restricted to Uncertain and cannot turn
an in-flight send into a resend permission. No runner/public endpoint receives
preview, validation or inspection authority.

## Consequences

Frozen routes and one-shot send transitions preserve exact historical delivery evidence. A retry or accepted response cannot silently adopt a changed room, device or task epoch.

## Alternatives Considered

Inferring delivery from Done or from a stable Matrix transaction ID would confuse intent with external acceptance. Reusing current room state for old output would discard the frozen private route.
