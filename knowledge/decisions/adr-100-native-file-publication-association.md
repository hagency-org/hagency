---
kind: decision
id: ADR-100
title: "Validate original content before historical file publication settlement"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
---

## Context

ADR097 freezes original file metadata and capture facts alongside a distinct event
locator. ADR098's private publisher must validate recovered SDK content against
those original facts. A journal record and its reconstructed JSON hash can be
internally consistent while its metadata or encryption descriptor no longer agrees
with the independently retained domain row or media receipt. Existing bounded
historical restoration checks the locator, but does not accept content to compare.

## Decision

Add a host-only historical restore_file_delivery_settlement_for_content method to
DomainRepository and its bounded DomainStore wrapper. The method validates locator,
request and captured inputs, reuses exact historical locator restoration, then
compares all immutable request and capture fields against the original row before
returning the existing opaque settlement handle. Unknown locators return None;
changed bounded content returns Conflict. Existing locator mismatch and phase
refusals remain unchanged. The worker validates before queue admission and accounts
for all owned input bytes. Retirement, cancellation or reopen does not prevent
historical settlement, and no preparation, claim, send or current authority is
returned. The consuming publisher must still retain its actual protected complete
SDK acknowledgement before committing Delivered; correlation data is not proof.

Expose encrypted_receipt_matches as a bounded pure media-store comparison taking
the namespace digest, validated OperationId, ciphertext length, actual Descriptor
and original receipt digest. Factor frame identity fields and hashing so preparation,
scan and this comparison use the same byte-for-byte format. The descriptor exposes
its already validated ciphertext SHA256 by borrowed reference solely for this
comparison. Exact descriptor JSON bytes remain committed, including key, IV and
ciphertext hash. The comparison refuses lengths above the existing hard item bound
and never performs IO, stages media, creates a receipt or returns opaque proof.

Matching these data proves only association with a supplied digest. It does not
read ciphertext, authenticate a sender or SDK response, reconstruct source custody,
establish namespace ownership, acknowledge OS sync, prove hardware durability or
authorize a current send. Actual staging and restoration continue to enforce their
independent ownership, clean recovery and platform sync requirements. Windows
directory-unconfirmed evidence remains negative qualification. Historical missing
storage or a matching digest never authorizes replacement encryption or another POST.

## Consequences

The publisher can independently reject coherent metadata and descriptor substitutions
before its first historical Delivered write without adding a fake proof type. The
existing schema and frame identity are unchanged; callers remain responsible for
real protected SDK acceptance and current authority before any new network effect.
Bounded offline tests exercise the original repository, writer, SDK codec and private
storage. Actual recipient, service and MCP acceptance remain separate mandatory work.

## Alternatives Considered

Reconstructing event JSON and checking only its own new hash would not compare the
original metadata. Copying the frame hash algorithm into the publisher could drift
from persisted staging identities. Returning a receipt or verified-proof wrapper
from hashes would overstate a pure comparison. Reading or decrypting ciphertext
inside this helper would add unrelated IO or copied plaintext without establishing
the independently required original source and SDK acknowledgement boundaries.
