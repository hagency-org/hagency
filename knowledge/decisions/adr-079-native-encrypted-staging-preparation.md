---
kind: decision
id: ADR-079
title: "Bind original encrypted staging identity before journal writes"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

A crash after staging but before recording its identity would otherwise force recovery to infer authority from whatever journal content remains.

## Decision

ADR077 requires an independently retained original operation and receipt digest
to restore encrypted storage. ADR066 stage returns that receipt after writing;
a crash between staging and recording its identity cannot safely discover a new
identity from whatever happens to be present. Add PreparedEncrypted as distinct
host custody of an actual SDK Encrypted object before any journal mutation.

Store::prepare_encrypted validates the current private store, clean recovery,
qualified file/directory sync, existing identity/capacity and computes the same
frame identity that stage will commit. It holds the actual ciphertext, original
operation, namespace, digest and a permit from the existing finite result pool.
The digest excludes append offset and chain predecessor: an unrelated intervening
append cannot change the prepared content identity. No raw constructor, Clone,
Debug or serialization is provided. Bounded storage operation/namespace digest
accessors expose identity to the host, never a filesystem path or runtime grant.

Store::stage_prepared consumes custody only in its original Store owner, verifies
the identity and current state again, then uses the existing write and failure
custody path. Same-namespace different owners and reopened Stores do not inherit
that in-memory admission. Pre-write refusal returns original Media; once IO may
have begun the original Store retains custody and uncertainty. Holding a plan
does not reserve journal space or guarantee a later write. Its permit is shared
with generic reads and restored results and remains per Store, not global.

The host must commit the original plan identity to the upload domain before
calling stage_prepared. This storage primitive makes no domain write, captures
no new source, sends no network request and proves no current dispatch authority.
Windows unconfirmed sync refuses preparation explicitly; ordinary staging and
inspection remain separate. Current OS flush evidence does not prove historical
or hardware power-loss durability. Upload WritePossible and unknown POST recovery
remain separate requirements; no missing record authorizes replacement encryption.

## Consequences

PreparedEncrypted retains original ciphertext and a fixed commitment before mutation. Preparation reserves a held result, not journal space or guaranteed future writes, and Windows unconfirmed sync remains refused.

## Alternatives Considered

Discovering a replacement commitment after IO or encrypting again would sever original operation identity. Treating current flush evidence as historical power-loss proof would overstate the storage contract.
