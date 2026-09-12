---
kind: decision
id: ADR-068
title: "Bound encrypted media download separately from event and dispatch authority"
status: Accepted
tags: [rust, matrix, media, transport]
---

## Context

Encrypted media retrieval needs bounded authenticated-origin transport and complete decryption without acquiring event or dispatch authority from a URL.

## Decision

This ADR027 slice adds host-local MediaDownloader to hagency-matrix, using the
existing hardened HTTPS client and ADR061 Codec. It implements authenticated
repository GET and checked plaintext only. The earlier ADR066 storage partition
is not a Matrix authorization source; this slice opens no staging, SDK or domain
store and does not depend on directory sync behavior.

### API and authority

MediaDownloader::new borrows validated HostConfig and bounded MediaDownloadLimits.
It requires HTTPS, including local test peers, and retains the configured origin,
bearer and TLS roots in the existing Http client. It does not call whoami or assert
that the supplied credential belongs to a particular user/device. No new endpoint,
token, room, impersonation, verified boolean or filesystem setter is provided.

MediaId::new accepts a bounded mxc:// server/media path, not an HTTP URL. Server
and media components are 1..255 ASCII bytes, total at most 517. The server uses
ruma's ServerName parser; explicit percent, slash, backslash, query, fragment,
userinfo, control and whole dot-segment rejection prevents URL normalization.
Media IDs use only ASCII letters/digits/hyphen/underscore. Inspection of pinned
ruma-identifiers-validation0.12.1 found MxcUri validation permits an empty media
component and narrows the slash offset to u8; 250-byte server input can hit its
NonZeroU8 unwrap. The native wrapper validates the bounded components directly
and never invokes that narrowing path. This is an intentional stricter admission
boundary rather than relying on an unchecked OwnedMxcUri.

download borrows MediaId and Descriptor and returns CheckedBytes or a static
MediaDownloadError. The caller retains the descriptor on every outcome. Only
GET /_matrix/client/v1/media/download/{server}/{media} is generated, using encoded
path segments under the configured origin. A remote MXC server is a repository
path component; no DNS/connection/token is sent directly to that media server.
The [Matrix authenticated media endpoint](https://spec.matrix.org/v1.13/client-server-api/#get_matrixclientv1mediadownloadservernamemediaid)
allows redirects; this bounded subset explicitly refuses them. There is no old
unauthenticated fallback, proxy, retry, query token, URL preview or thumbnail.

Correct descriptor syntax and ciphertext hash do not authenticate sender, event,
room, device, workspace or intended plaintext. A different valid key can decrypt
the same hash-checked ciphertext to different bytes. The future collector seam
must derive the private descriptor from an authenticated decrypted event, bind
the event to the dispatch's frozen visible input/privacy floor, and revalidate
current authority after the asynchronous download before exposing or staging it.
None of those proofs can be supplied through this transport's result.

### Bounds, EOF and cancellation

Clones share one active-transfer semaphore and one Codec result pool. Limits
allow 1..16MiB ciphertext per request, 1..4 active transfers and 1..8 retained
results; defaults are 4MiB, two transfers and four results. Admission uses
try_acquire before network, with no queue. An active permit retains the complete
ciphertext buffer through decryption; actual CheckedBytes retain the separate
Codec permit until dropped, even if all downloader handles have gone away.
A full Codec may refuse a new result after its bounded GET. It never releases
an earlier result's permit or creates additional checked buffers. Logical payload
bounds cover at most active*byte_cap ciphertext plus results*byte_cap plaintext.
Allocator rounding/overhead and existing HTTP/TLS/SDK buffers are additional;
these per shared owner limits are not a physical-memory or global RSS guarantee.

The binary reader requests checked Vec capacity with try_reserve_exact, avoiding
geometric growth without assuming the allocator provides exactly that size. It bounds
accepted headers to 64 fields and16KiB, below Hyper1.11.1's own finite parser
buffer. Duplicate Content-Length, Transfer-Encoding, Content-Encoding or
Content-Type, CL plus TE, non-decimal/overflow lengths, nonidentity content
encoding and unsupported transfer encoding are refused. Conflicting malformed
framing may be rejected by Hyper before a HeaderMap is exposed; duplicate valid
fields remain visible to the adapter and are explicitly refused. The existing
JSON perform implementation is unchanged.

Declared size is only an early refusal check. Actual chunks cannot exceed the
byte cap, and decryption begins only at HTTP body EOF. This means the end of the
HTTP-framed body, not arbitrary bytes after a completed Content-Length message.
Close-delimited TLS bodies additionally need a clean TLS EOF; absent close_notify,
truncated declared length and missing chunk terminators fail. Remote filename,
MIME and content-disposition are not trusted as file metadata or authority.
Successful responses must be200; errors expose only static categories/status.

One monotonic absolute deadline spans request headers, body and the post-codec
check, with the configured connect/header/body-idle sublimits. Cancellation is
checked before admission, during each network wait, before and after the Codec.
Bounded synchronous SDK hash/decryption cannot be hard-cancelled; if it overruns,
the plaintext result is dropped and Timeout returned after CPU work finishes.
No separate crypto worker, detached transfer or retry is introduced. Dropping
the polled future drops its response/ciphertext/active permit. Existing bounded
OS DNS jobs may finish later while retaining their own resolver permits.

Incomplete or integrity-failed ciphertext is released, with no partial plaintext
object and no source Snapshot invented. GET creates no upload/send effect, so
these errors do not need an upload uncertainty claim. Callers can explicitly pass
successful CheckedBytes to ADR066; its own durability and custody rules still
apply. This slice does not stage downloaded bytes automatically.

### Verification and limits

Six deterministic selectors drive actual configured-origin TLS and retained
Node/Matrix SDK crypto vectors: exact remote path and bearer, untrusted CA,
redirect/auth/404/410/429 refusal, malformed/oversized framing, exact byte limits,
chunked and close-delimited EOF, unclean TLS EOF, bad hashes, slow headers/bodies,
absolute deadline, cancellation, dropped futures and shared capacity recovery.
The existing Fake now performs bounded stream shutdown after scripted bytes so
valid close-delimited TLS reaches close_notify; explicit unclean responses keep
the negative case. No server/model/credential outside local fixtures is contacted.

Upload, prepared media receipts, attachment event admission, plaintext media,
scoped MCP receive_file, disk persistence and Matrix room sending remain gated.
The legacy file surface supports20MiB while the native codec remains a documented
16MiB subset. Platform runtime behavior is qualified by integrated CI; local
success or Windows cross-compilation alone is not a three-platform runtime proof.

## Consequences

The downloader returns checked plaintext only after complete bounded transport and crypto checks. Whoami identity, manifest provenance, current dispatch scope and persistent receive paths remain separate responsibilities.

## Alternatives Considered

Following redirects or accepting arbitrary origins would expose the configured bearer. Partial plaintext, automatic retry or a silent plaintext fallback would conceal incomplete or unauthenticated media outcomes.
