---
kind: decision
id: ADR-090
title: "Associate actual upload claims and retained staging commitments"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Two different live uploads can share a fence number, so claim validation alone cannot associate a separate send handle with its original operation.

## Decision

ADR078's current validation accepts a sealed UploadClaim and checks that claim's
row. A consuming coordinator also holds a separate UploadSend. Before consuming
the send into private SDK custody it must prove the retained claim refers to the
same upload; two live uploads can have equal fence numbers. Fence equality alone
would let a valid claim for upload B stand in for upload A during revalidation.

UploadSend::matches_claim compares its complete private UploadIdentity (id,
request commitment and capability commitment) and exact fence with the claim.
It exposes no secret, reconstructs no capability and performs no database or
network operation. It proves association only. The consuming owner must retain
that exact claim and run existing current validation after the final private
SDK await before its one possible POST. Matching remains true after retirement,
while current validation refuses; neither method atomically locks remote state.

ADR077 RestoredEncrypted already retains its original HostNamespace privately.
A borrowed namespace_digest accessor allows the consuming owner to compare it
with the domain StageCommitment without passing an unrelated caller-created
namespace or reconstructing Encrypted. The original qualified restoration path
is the only constructor. Keys, bytes and permits retain their original ownership;
no serialization, Clone, raw constructor or durability upgrade is added.

Tests use actual domain-issued claims and sends, including same-fence different
uploads, an expired earlier fence, and retirement after exact matching. Existing
real media restoration proves the namespace survives owner close and source
replacement alongside exact ciphertext and descriptor. Windows unconfirmed
directory sync still refuses typed restoration; that refusal is not a positive
Windows upload qualification. HTTP coordination remains separate ADR089 work.

## Consequences

Complete private identity and stage comparisons establish association without minting authority. The consuming owner must still retain the same claim and perform current writer checks.

## Alternatives Considered

Comparing fence numbers alone would let another upload's claim substitute during validation. Reconstructing encrypted source authority or upgrading unconfirmed directory sync would exceed the retained staging evidence.
