---
kind: decision
id: ADR-083
title: "Retain exact checked upload response body under the original attempt"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Private upload acceptance needs the exact checked HTTP response bytes, because a parsed media URI cannot reconstruct their original representation.

## Decision

ADR072 already permits one bounded authenticated-origin POST of actual codec
Encrypted bytes and refuses to rearm any attempt once a write is possible. Its
successful result retained only the checked MediaId. A future private acceptance
journal needs the actual response body: reserializing a media URI cannot recover
original whitespace, escapes or a digest of the bytes actually observed.

Http::upload now constructs UploadResponse only after the unchanged HTTP 200,
header, size, true EOF, strict JSON and exact single content_uri validation. It
retains the exact complete response body of at most 4096 bytes, its SHA256 and the
checked MediaId. This digest covers body bytes, not HTTP headers, status, TLS
records or a reserialized JSON value. Validated status and framing remain
transport observations; SHA256 is content identity, not sender authentication.

UploadResponse is defined in the private HTTP module and reexported as a sealed
host type. It has no public raw constructor, Clone, Debug, Serialize or
Deserialize implementation. Its only getters borrow body(), body_sha256() and
media_id(). The response lives inside UploadAttempt under the existing finite
attempt permit, shared across uploader clones. No separately extractable result
can release that permit while retaining its body. Existing active-request,
ciphertext and response limits remain unchanged; these are logical data bounds,
not exact physical allocator or TLS-library memory bounds.

UploadAttempt::observed_response() returns a borrowed historical observation.
The transport result moves into the original attempt before the final current
cancellation/deadline check, with no await in between. If that last check refuses,
the original Cancelled or Timeout result remains, state stays WritePossible and
media_id() still returns None. A fully validated body already observed need not
be destroyed because current success was refused. By contrast, malformed JSON,
invalid MXC, partial body, failed EOF or pre-response cancellation never acquires
response evidence. On success media_id() remains the existing convenience API.
Resend stays Terminal in either case.

A future private journal may copy directly from this sealed borrowed evidence
only after reserving its own finite record and queue capacity, and must retain
that owned copy if persistence becomes uncertain. This API does not provide a
response extraction or ownership-transfer operation and does not bind a result
to a domain upload, SDK identity, Matrix route or authority generation. The
coordinator must establish those bindings before POST. No received URI grants
room delivery, dispatch revival, current permission or safe retry authority.
Dropping the original attempt still loses in-memory outcome custody; no durable
recovery, upload idempotence or absence-of-remote-effect claim is added.

Actual local TLS fixtures compare compact, whitespace-varied and escaped JSON
with equivalent checked MXCs but distinct exact body digests. Existing bounded
response vectors now assert no evidence for invalid or incomplete input and
retain an exact 4096-byte valid body. Cancellation, held-result capacity, original
ciphertext/key isolation, status refusal and nonrearmable state are rechecked.
The compile-time negative-trait assertion guards against accidental printable or
serializable response types. There is no deterministic fixture hook between the
private final synchronous projection and acceptance check; that narrow late
refusal branch is preserved by source ordering rather than a timing-racy test.

This slice adds no persistence, SDK journal, restored-media POST, domain adapter,
file-event sending, model tool or service activation. Native tests and Windows
GNU compilation are separate from actual hosted Windows runtime qualification.

## Consequences

The response retains bounded body bytes, their digest and checked MediaId under the original attempt. It adds neither persistence nor association with an arbitrary domain upload.

## Alternatives Considered

Reserializing the URI would lose whitespace and escape identity. Exposing printable or serializable response evidence would expand private custody into public data without authorization.
