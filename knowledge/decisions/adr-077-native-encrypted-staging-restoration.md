---
kind: decision
id: ADR-077
title: "Restore exact committed encrypted staging without reconstructing source authority"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Committed encrypted staging must be restorable without pretending a generic stored byte object retains original source-snapshot authority.

## Decision

ADR066's generic StagedMedia preserves bytes and kind, but cannot stand in for
ADR061's original Encrypted object, which retains a real source Snapshot.
Introduce a distinct non-serializable RestoredEncrypted from the private Store.
Its only constructor requires the exact original operation and receipt digest,
a validated committed encrypted frame, the matching opened namespace, clean
journal recovery and FileAndDirectorySynced evidence. Descriptor, ciphertext,
receipt, namespace identity and the existing read-result permit move together
without a second copy or an encryption call. Kind and digest failures do not
rebind storage. Pending/incomplete storage remains quarantined for this API.

Windows FileSyncedDirectoryUnconfirmed continues to allow ordinary inspectable
reads but cannot create this qualified restoration. OS flush evidence is not a
hardware power-loss guarantee. The existing per-Store finite read-result pool
covers both generic and typed results. Handles can outlive Store closure, but do
not reconstruct source/ancestor handles, prove a private runtime workspace or
assert current dispatch, SDK or Matrix room authority.

This slice deliberately adds no prepare_restored/send method to the uploader.
Restored bytes are not evidence that an earlier POST was unsent. The next upload
adapter still needs an exact domain operation recorded before staging, durable
WritePossible before HTTP, protected accepted MXC custody and a recovery path
that never resends unknown writes. A missing record or wrong operation is not
permission to re-encrypt, create a replacement operation or upload elsewhere.

## Consequences

Typed restoration requires exact original operation, receipt, namespace and qualified sync evidence. It retains storage custody while making no claim of current dispatch authority or earlier POST absence.

## Alternatives Considered

Reconstructing Encrypted source authority from generic staged bytes would invent lost filesystem custody. Re-encrypting or reuploading because a record is missing would confuse storage recovery with safe resend permission.
