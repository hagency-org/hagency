---
kind: decision
id: ADR-076
title: "Expose checked attachment bytes only within current frozen dispatch authority"
status: Accepted
tags: [rust, matrix, media, privacy]
---

## Context

Receiving attachment bytes requires composition of frozen domain visibility, original verified manifests and bounded authenticated HTTPS decryption.

## Decision

ADR027's next bounded receive slice composes ADR073 domain tickets, ADR074 private
verified manifests and ADR068 authenticated HTTPS decryption inside hagency-matrix.
No new crate, domain schema, server or runtime endpoint is required.

Collector::receive_attachment accepts only the host's current RunnerCapability,
original event ID and cancellation token. It obtains an opaque AttachmentTicket
from the canonical writer, retrieves the exact original authenticated manifest,
downloads through the Collector's configured HTTPS origin and revalidates the same
capability and ticket at the writer clock before returning checked bytes. Caller
input cannot choose a descriptor, URL, room, device, workspace path or verification
assertion. The future MCP adapter must inherit its capability from host context.

The ticket freezes source sequence and per-session projection boundaries, exact
Started dispatch/fence/secret, current session/room/device/registration generations
and proven parent lineage. A queued unrelated upload beyond the actual selected
trigger remains invisible even if already in the inbox at enqueue. Later valid
follow-ups can access earlier attachments in their existing authorized lineage.
Possession of an old ticket or descriptor never creates current authority.

The Collector retains a lazily initialized downloader and four early result slots
across shared access and SDK Owner close/reopen. Lazy initialization preserves
existing loopback-HTTP collector support; receiving still requires HTTPS. The
fixed downloader defaults retain two active transfers, four Codec results and a
four-MiB logical ciphertext cap. The early slot stays occupied through every wait
and through the returned object's lifetime, so full result capacity refuses before
network contact. Active response buffers remain covered by the downloader permit.
These logical payload/object bounds exclude allocator rounding and protocol buffers;
they do not claim a physical-memory or process-global RSS ceiling.

The complete operation uses one configured SDK absolute deadline. It bounds domain
queueing, Owner lock/open, manifest lookup, HTTP and final domain validation.
The downloader's existing request/header/body-idle/connect limits can only narrow
this deadline. Public standalone download semantics remain unchanged. Cancellation
is checked before admission, during waits and immediately before output. Synchronous
bounded SDK hash/decryption cannot be forcibly cancelled; overrun discards the result
after CPU work ends. Future drop releases its owned response/buffers/permits without
detaching a receive job. Already queued read-only domain/SDK inspection work may
finish independently, but its abandoned response cannot expose bytes to the caller.

Manifest lookup releases the Owner mutex and collector busy permit before GET.
Concurrent intake and negative privacy observations can therefore retire the route
while a download is paused. Post-download writer validation refuses returned bytes
after revocation, private-to-group promotion including a null-root DM, negative
room evidence, device generation change, parent retirement or lease expiry. A failed
writer response never implies permission to expose data or retry it automatically.

The provenance boundary is prior actual authenticated Collector observations plus
current domain state. Receive does not fabricate fresh remote membership evidence
or assume the server cannot change before its next observation. It uses the same
configured credential as that Collector; ADR068 alone does not authenticate account
identity. A future requirement for a new server snapshot on every call must retain
the collector's owned negative-observation fencing rather than add a cancellable
raw whoami/state read whose result can be lost without retiring authority.

ReceivedAttachment has a private constructor and no Clone, Debug, Serialize or
Deserialize. It owns actual CheckedBytes, validated filename/MIME/declared-size
metadata and its result permit. The host can borrow bytes and their actual SHA256;
declared size and MIME remain sender data and do not override integrity or limits.
No MXC, key, descriptor, private route or credential is returned. The object is scoped
at return time: revocation cannot erase a trusted holder's memory, and delayed
external export must independently revalidate current authority.

Local fixtures use actual verified Olm/Megolm file/image events, persisted manifests,
selected inbox dispatches, configured-origin TLS and retained SDK media vectors.
Domain task-notice activation in the lineage fixture is an explicit host observation
fixture, not a claim of an actual notice send. Paused GETs prove final refusal after
retirement and concurrent negative collection. Failure, deadline, drop and held-slot
cases prove no partial result or capacity reset. Existing ADR068 framing/MXC tests
remain the transport matrix rather than being redundantly reimplemented here.

This is a host-local byte object, not a cache path or full receive_file implementation.
Protected receive-cache creation, durable staged identity, safe path/output types,
MCP tool approval/context wiring and production activation remain absent. Plain
attachments, arbitrary new groups, taskless sessions, thumbnails, uploads and event
sends are not enabled. GET is read-only; deliberate later retries must revalidate the
same original source, and no partial/plaintext fallback or automatic retry is added.

## Consequences

The collector revalidates the same ticket and capability before returning checked bytes. Persistent cache paths, MCP exposure and full receive_file service behavior remain unimplemented gates.

## Alternatives Considered

Allowing callers to choose a descriptor, room or verification flag would bypass original provenance. Returning partial bytes or accepting only a pre-download authority check would ignore retirement during asynchronous work.
