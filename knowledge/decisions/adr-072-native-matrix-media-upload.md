---
kind: decision
id: ADR-072
title: Retain possible encrypted upload writes under bounded host custody
status: Accepted
---

## Context

An encrypted upload POST can become externally possible before its response arrives, so the attempt must retain nonrearmable custody and original ciphertext.

## Decision

ADR068 qualifies configured-origin encrypted download. This bounded sibling uses
pinned ruma-client-api0.24.0 media/create_content.rs's authenticated
POST /_matrix/media/v3/upload, with application/octet-stream and no filename
query. The configured HTTPS homeserver alone receives the bearer. Uploads neither
authenticate a returned URI as event/room authority nor send room content.

The caller supplies an actual borrowed ADR061 Encrypted object. Its private
constructor guarantees this API receives codec ciphertext rather than arbitrary
plaintext; the original descriptor and snapshot custody remain with the caller
through cancellation and errors. No descriptor is transmitted. ADR066 staged
objects and their distinct durability outcomes remain outside this first adapter.

Cloned uploaders share finite held-attempt and active-request permits. Held slots
include prepared attempts, uncertain writes and accepted receipts. Admission
precedes a bounded ciphertext copy for the HTTP body. Logical byte and slot caps
do not assert exact allocator or TLS physical memory consumption. No detached
worker or unbounded wait queue is introduced.

An attempt starts Prepared and records WritePossible before request polling.
Cancellation, future drop, deadline, malformed response and any transport failure
after that boundary leave inspectable uncertainty; even a failure before the
socket writes is conservatively unknown once execution may have begun. Only a
complete bounded HTTP200 JSON body with exactly one valid content_uri permits
Accepted. Duplicate fields, unsupported fields, unsafe MXC components, contradictory
framing or incomplete EOF refuse acceptance. A returned URI is stored only as a
bounded repository identity. Neither success nor error re-arms the same attempt.

POST upload has no stable idempotency key. The library performs no automatic retry,
redirect or proxy fallback, and a second newly created attempt is not a recovery
proof for an uncertain first request. A caller dropping an unknown attempt loses
its local outcome marker. This is not a durable upload journal or restart recovery
contract; a future adapter must bind its domain operation and storage durability
before network effects. HTTP errors provide observed static reasons, not proof
that the repository contains no copy.

The new binary path shares the existing hardened HTTP client while leaving its
JSON methods and download behavior unchanged. Request payload caps are1–16MiB,
active permits1–4, held attempts1–8 and response bodies at most4096bytes; headers
retain the existing64field/16KiB accepted bound and strict encoding/framing rules.
One absolute request deadline includes upload and response, with bounded header
and body waits. Full body EOF precedes strict JSON and existing MXC component
validation. CPU work and allocator overhead remain bounded but not hard realtime.

Tests use real local TLS, a real retained file snapshot and actual SDK encryption.
No live provider, homeserver, credential, room send, file tool, service toggle,
source restoration or production availability is introduced.

## Consequences

Shared finite permits cover prepared, active and uncertain attempts. A returned media URI does not establish event authority or room delivery, and staging restoration remains outside this initial adapter.

## Alternatives Considered

Automatically retrying after timeout or accepting arbitrary plaintext input would break the original attempt boundary. Sending a descriptor or following another origin would expose material the configured upload does not require.
