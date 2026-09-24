---
kind: decision
id: ADR-038
title: Bind Matrix ingress and canonical task activation to current copied session provenance
status: Accepted
---

## Context

Native task activation and follow-up intake need authenticated session provenance that survives copying, retries and changing task epochs.

## Decision

This bounded M3 integration closes the verified-session intake seam recorded in
ADR-033. It connects authenticated host input, canonical task intent and activation,
frozen dispatch, original-human follow-up and final reply intent in offline
repository and HTTP fixtures. It does not implement a Matrix network adapter or
production notice sender, and it leaves M3 and the overall migration open.

The host obtains a typed MatrixIngressScope from a current verified session. Each
observation binds the exact session incarnation, registration, room and device
transport generations, alongside the original full server/room/event/sender and
thread identity. Runtime HTTP has no ingress, scope, wake, target, transport claim
or acknowledgement command. Host observation DTOs do not implement Deserialize.
The existing legacy find_session fence stays in place, and legacy ingestion and
input-attachment commands reject verified sessions rather than granting them the
old caller-selected wake policy.

A current full joined snapshot must include the sending human. Group wake derives
from explicit authenticated full-MXID mentions, never textual @ parsing. Direct
main and direct thread messages from the authorized human need no mention. Known
Agent transports, representatives and approval bots are background input, so their
output cannot create automatic Agent-to-Agent wake loops. Unmentioned discussion
is retained as independent session input. Only supported input kinds can wake.
Mention content and encryption are authenticated host evidence, not authority
asserted by runtime text; the real adapter must establish that evidence.

Schema 12 preserves the initial room-visibility and transport-observation times.
A fresh session uses their maximum as a conservative ingress lower bound; identical
same-generation refresh and restart never move it. Delayed offline messages in the
same established generation remain admissible. origin_ts is only an additional
deny filter: it is not proof of membership or authenticity. The M5 adapter must
establish a consistent authenticated sync/membership boundary, reject ambiguous
backfill and handle clock skew before admission. A new room or device generation
requires renewed evidence and fresh sessions. Schema-11 routes receive no invented
boundary during migration; same-generation refresh cannot upgrade them.

Each per-Agent event receipt pins the immutable original source SID, generation
family, normalized body and authenticated mention/encryption digest. A repeated
event with changed body or scope conflicts. The same event can be admitted to
multiple Agents only through separate exact projections with independent copies;
shared message storage never substitutes for these copies. Retired source receipts
cannot be replayed into fresh sessions, including a null-root DM promoted to a
Group and same-generation main-session retirement. Explicit task copies may move
only through the transactional task-intent path, preserving the original receipt.

VerifiedTaskRequest selects already-admitted eligible input under host authority.
It cannot create a task from an arbitrary sequence or unmentioned background event.
Direct main uses one continuing null-root session and canonical task; its original
human source event and activation ACK event remain distinct provenance fields.
A group main mention creates a task thread rooted at the original human event.
Explicit threads retain the actual authenticated top-level root and need that
root's stored receipt; missing roots and nested-root substitutions fail. A task
thread pins the root's original source SID as its parent, including a thread
resolved before task creation. Retiring that parent retires the derived route.

Canonical task creation, pending intent, source/root copies, request receipt and
ACK notice commit together. Injected write failures roll them all back. The ACK
freezes exact route, source event, content digest and stable transaction ID. A host
acknowledgement must match server, room, sender, device, encryption, transaction and
content plus a valid event ID and live claim. Legacy acknowledgement rejects these
notices. Only that current exact receipt activates task input; it cannot establish
Done. Repeating the exact observed ACK is idempotent, while changed observations
conflict. Room, membership, parent, allocation, registration or device retirement
fences claimed ACKs and input scheduling.

Current verified notice claims are deliberately an offline scheduling proof.
They expose frozen route/content and can reclaim an expired claim. They do not yet
have the final-reply outbox's begin-send/Sending/Uncertain state machine. A live
adapter MUST add durable send custody, immediate current-route validation,
cancellation coordination and uncertain-outcome inspection before wiring these
claims to Matrix. Stable transaction IDs handle ordinary duplicate retries but do
not prevent a delayed private notice being sent after promotion. Rejection of its
later ACK does not undo that privacy leak. This is an explicit M5 transport gate,
not a claim of safe operational notice delivery.

Dispatch freezes exact session-owned input before runtime start. Later input stays
separate; finishing one Agent's dispatch does not acknowledge another Agent's copy.
After Done, only fresh unprocessed input from the original human, with valid mention
or direct policy, can reopen the same task when dispatch actually starts. Enqueue
and claim alone cannot change task truth. Canonical epochs remain authoritative:
Done increments the epoch, reopen increments it again, and a later Done increments
it again. Old capabilities and old final reply intents cannot report the new epoch.
Final intent uses the actual intent-created task and its exact Done/report grant,
with null-root direct routing unchanged.

Limits retained from ADR-033 include first Group admission only to the authorized
project room (or explicit promotion), owner-only encrypted invitation-only DMs,
same-server member policy, and no taskless/front-desk output. Automatic unread
whole-room window selection, media retrieval, generalized invited rooms, real
Matrix authentication/sync/crypto, live notice and reply delivery, model processes,
console cutover and operational end-to-end tests remain separate work. This slice
preserves background session copies and thread task copies; it does not claim a
complete discussion-window bridge. Existing bounds reject further intake at 2,000
pending copies per session, 10,000 task inputs and 100,000 retained request/message
records. Nothing is silently truncated or counted as delivered.

## Consequences

Exact source receipts and frozen dispatch input preserve separate admission, activation and reply intent. Repository fixtures do not qualify a live Matrix adapter or complete discussion-window behavior.

## Alternatives Considered

Allowing runtime HTTP to supply ingress authority or silently attaching later messages would expand a frozen capability. Reusing old Done epochs for follow-up output would misattribute canonical task authority.
