---
kind: decision
id: ADR-061
title: Bound attachment crypto separately from file and room authority
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Attachment encryption and decryption need finite complete-byte handling while remaining separate from authenticated event and room authority.

## Decision

`hagency-media` supplies the next ADR027 primitive after ADR058 snapshots. It
uses the already pinned `matrix-sdk-crypto` 0.18.0 attachment encryptor/decryptor;
it neither creates a second room crypto store nor implements its own cipher.
The [Matrix v1.16 attachment contract](https://spec.matrix.org/v1.16/client-server-api/#sending-encrypted-attachments)
requires encrypted upload bytes and encryption metadata inside an authenticated
encrypted room event. A ciphertext hash does not authenticate a key or sender.

The pinned implementation uses AES256 CTR with a fresh key and random high half
of the IV, and hashes ciphertext. Its decryptor checks the expected hash only at
EOF. Our wrapper first validates the complete bounded ciphertext hash, then
drains the SDK decryptor fully into private bounded memory; no partially read
plaintext is exposed. Changing only a valid key can still yield different bytes
with a correct ciphertext hash. An explicit regression preserves this limitation:
the future Matrix adapter must provide authenticated descriptor provenance.

The opaque codec owner limits each operation to at most16 MiB and at most8 held
results, default4 MiB and4. Encryption consumes a Snapshot and retains its actual
source/ancestor handles and workspace permit, plus one codec permit, until the
result is dropped. It holds one additional ciphertext buffer. Decryption holds
one plaintext buffer and one codec permit; the caller owns already bounded input.
Limits describe allocations retained by these APIs, not caller copies or global
host admission. There is no generic network/disk reader or unbounded streaming
callback. No automatic serialization or Debug is supplied for private data.
These are bounded subset limits: the legacy file surface permits20 MiB, while
this native primitive currently permits at most16 MiB. Size-limit parity is not
claimed and larger files remain unsupported until separately qualified.

Encryption metadata is at most1024 bytes with unique known fields, v2, oct/A256CTR,
both operations, ext=true, canonical unpadded key/IV/hash encodings, exact decoded
lengths and an initially zero counter. Unknown versions/algorithms or ambiguous
metadata are unsupported, never plaintext fallback. Its explicit private-event
JSON accessor exists only for the trusted encrypted transport, not HTTP/model
projection. The URL and outer message are deliberately absent at this layer.

The SDK encryption constructor documents a panic if its randomness source fails;
no fallback key or plaintext result is provided. The library does not promise
secure erasure of SDK or temporary allocations. Fresh crypto keys are produced
for each encryption call, so retry identity must eventually come from a durable
prepared-media record rather than encrypting the source again.

Fixtures use public fixed test keys and Node AES ciphertext checked by the actual
existing Matrix crypto binding, plus native SDK round trips on actual retained
workspace snapshots. No live model/server, upload, download or delivery receipt
is involved. Persistent staging, file outbox authority and transport are open.

The actual Node-binding oracle runs in the existing Node CI test job after its
normal `npm ci`. The Rust jobs keep their scripts-disabled dependency install
and consume the checked fixture, because that install deliberately omits the
Matrix addon's postinstall download. A warm local binding does not prove a fresh
scripts-disabled CI installation can execute the oracle.

## Consequences

The pinned SDK supplies crypto and checked EOF behavior; descriptors and keys remain private custody. A ciphertext digest alone authenticates neither the key nor sender, and staging and delivery stay separate.

## Alternatives Considered

Implementing another cipher or exposing plaintext before complete hash and EOF checks would weaken the existing SDK boundary. Treating valid decryption as sender authentication would confuse integrity with provenance.
