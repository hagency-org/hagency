---
kind: decision
id: ADR-074
title: Retain authenticated encrypted attachment metadata independently of intake batches
status: Accepted
---

## Context

Encrypted attachment descriptors must survive intake completion while retaining the original SDK-verified event and sender association.

## Decision

This Matrix-only ADR027 slice extends the actual SDK intake gate to encrypted
m.file and m.image. Root ADR073 owns canonical attachment metadata and frozen
per-session projection visibility. This slice owns private transport manifests,
not dispatch authorization, media download, cache paths or event delivery.

Manifests are derived only from verified SDK Decrypted events with matching full
sender MXID, known device, Megolm session and no forwarder. Plain attachments,
mixed URL/file fallback, malformed descriptors and unsupported representations
remain terminal refusals. Existing Unsupported receipts remain terminal after
upgrade; newly supported syntax cannot resurrect previously refused sources.

The actual SDK identity is a pair of public key base64 strings. A domain-separated
SHA256 fingerprint satisfies the core 64 lowercase hex identity contract without
changing SDK persistence identity. Each manifest binds the exact frozen route,
original immutable encrypted source digest and bounded decrypted content/proof.
Private descriptors and repository URLs stay in the existing encrypted journal;
domain observations carry only validated filename/MIME/declared-size and digests.

An independent bounded manifest map is committed with Derived state before any
domain admission. Completing a Batch discards temporary data but retains its
manifests. Lost domain/ACK results recover exact receipts, never manufacture new
current authority. A host lookup must name the exact original manifest identity
and scope and passes current domain checks before and after asynchronous lookup.
Opaque handles retain finite capacity and offer borrowed descriptor/media identity
to trusted host code only; they are not serializable or runtime constructors.
The eight-result pool belongs to the Collector, so an SDK Owner close/reopen
cannot forget still-held results. One absolute configured SDK deadline and
cancellation cover owner lock/open, both domain ticket checks and the SDK queue.
No endpoint is contacted by lookup. A returned handle remains private readable
custody after later revocation; it never substitutes for receive authorization.

Hard limits are 128 retained manifests, 16 KiB per private record, eight held
lookup results and the existing 16 MiB encrypted journal bound. No eviction,
automatic history download or unbounded retry queue is introduced. Capacity
refusal preserves prior records and pending uncertainty. A future downloader must
independently authorize the current dispatch and frozen visibility before and
after network/crypto work; possession of a manifest does not supply that proof.

Filename metadata is strictly validated using the core 255-byte policy instead
of silently rewriting invalid source filenames. MIME is optional bounded data;
declared size is an untrusted observation, not a download limit or proof of bytes.
The native 16 KiB manifest limit includes original encrypted source and decrypted
content. Oversized or malformed per-event manifests are terminal refusals; total
manifest exhaustion preserves the complete pending batch in quarantine. No image
preview or thumbnail retrieval is added. Plaintext attachment destinations remain
unsupported in this bounded encrypted slice rather than silently falling back.

When the Derived plus manifest journal write fails, the current owner refuses
secret lookup and retains explicit Applying uncertainty, with original raw input.
An actual SQLite abort proves no domain admission and no manifest after validated
reopen; a lost positive write response would instead require inspecting what the
journal actually committed. Missing records never imply media transfer absence.

## Consequences

Private manifests remain independent of batch lifetime and canonical safe metadata. Capacity or journal uncertainty never fabricates a download result or revives a previously rejected source.

## Alternatives Considered

Plain URL fallback or caller-supplied descriptors would bypass verified encrypted provenance. Looking up secrets from an unacknowledged journal write would expose memory-only evidence as durable custody.
