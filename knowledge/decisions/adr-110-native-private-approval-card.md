---
kind: decision
id: ADR-110
title: Freeze private native approval cards from current original domain truth
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
---

## Context

The approval collector accepts private structured verdicts and the execution
coordinator owns actual pending callbacks. Neither has a coherent packet for
delivering a current request to its owner. Two separate metadata reads could
observe different room or request authority.

## Decision

The original domain writer creates an opaque private card packet in one SQLite
transaction. Pending status, exact owner/bot binding, registration, room, device,
task, lease and request expiry use the clock sampled after the original lock.
The host supplies the original earlier owner cutoff, never a renewed timeout;
it must be future and at most the stored immutable request expiry. This leaves
any prepared-response reserve unavailable as promised owner decision time.

The packet has no public constructor, mutation, Debug or serde projection. Its
host accessors expose only the frozen target and structured private message.
It is metadata, not permission to send or approve. A fresh comparison uses the
same target and complete content and refuses a changed or expired request.
Future encrypted transport must still verify authenticated private membership
and current keys after awaited custody commits and immediately before writes.

The wire content retains com.agentchat.approval.request.v1 and the complete
com.agentchat.approval v1 detail. Agent equals the native engagement ID, project
equals the canonical project ID. Request ID and digest remain unchanged. The
upstream JSON-RPC request ID keeps its number/string type in the additive
upstream_rpc_id field; the retained v1 upstream_request_id stays a display string.
Neither can replace the digest or request ID as authority. The exact stored
operation parameters, scope description, workspace and task remain private.
Once and Deny are always available; Task and Always require a persisted reusable
scope. Display text cannot create a reusable rule. The final encoded JSON is
limited to 48 KiB to leave encryption framing within the existing 60 KiB event
budget. Oversize refuses; input and scope are never silently truncated.

## Consequences

There is no schema change, public endpoint, Matrix send, grant creation or
execution resumption. Existing original-writer ownership and bounded queue
remain. A later packet read cannot reconstruct a lost send. Approval-purpose
SDK enrollment, protected encrypted delivery custody and executable wiring
remain required. Current native forty-hex request IDs differ from the retained
client's thirty-two-hex parser; this work does not claim Robrix interoperability.
ADR115 explicitly aligns the finite peer-schema profiles and records the receiver
upgrade, field-size and scoped-action requirements. It does not replace the
separate actual client, encrypted delivery and executable integration gates.

## Alternatives Considered

Two separately awaited reads lose coherent scope. An ordinary Agent notice
would give the approval bot invented task/session authority. A public DTO would
leak owner room and operation details. None is used.
