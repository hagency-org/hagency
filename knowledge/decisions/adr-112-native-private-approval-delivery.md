---
kind: decision
id: ADR-112
title: "Enroll the original approval SDK and deliver one private encrypted card"
status: Accepted
requirements: [REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-MATRIX-DM-PRIVACY, REQ-RUST-MIGRATION-EXECUTION]
---

## Context

ADR064 owns private verdict intake but cannot enroll the approval bot or send a
card. ADR110 freezes private card metadata without giving it send authority. The
coordinator accepted this exact28-path partition before implementation; the task
was parsed and linted before source edits. ADR059 ordinary routes and ADR102
Agent enrollment cannot be relabeled to create approval-bot authority.

## Decision

HostApprovalConfig::new continues to establish the approval-only purpose and additionally rejects a prepopulated ordinary enrollment profile. Add HostApprovalConfig::with_fresh_account_enrollment(anchors), with the existing canonical ordinary-user fresh-account rules and maximum16 independent peer masters. No profile/name/purpose can come from event content or HTTP. The bot's normal account token and external operator anchor provisioning remain explicit prerequisites.

ApprovalCollector adds:
- enroll_fresh_account(&self, &CancellationToken) -> Result<(), Error>
- send_private_approval_card(&self, Arc<PrivateApprovalCard>, &CancellationToken) -> Result<PrivateApprovalDeliverySummary, Error>
- resume_private_approval_delivery_custody(&self, &CancellationToken) -> Result<PrivateApprovalDeliverySummary, Error> (network-free)
- private_approval_delivery_status(&self) -> Result<PrivateApprovalDeliveryStatus, Error> (bounded counts/closed stages)
- close(&self) replaces consuming close, retaining the original close job/result. Existing callsites remain source compatible.

Summary states are Idle, Accepted and Uncertain, with optional opaque request ID and replayed flag. Accepted means the original Matrix room-send event-ID response was durably retained, never owner approval, read receipt, runtime permission, native application or dispatch resumption. Status omits raw content, room/account/device IDs, keys, event IDs and paths. The public packet is never serialized as an input/proof API.

New sends require this approval collector's explicit fresh-account profile and a qualified original Complete enrollment record. Existing externally seeded reader SDKs can continue intake, but this initial send partition does not invent a production identity-recovery route for them. There is no bootstrap/executable wiring, automatic card discovery or client parser modification in this task.

## Enrollment mechanics and purpose isolation

Use a private explicit EnrollmentPurpose::{Agent, Approval} on the existing SDK command/handle path, checked against the actual owner's persisted purpose. Preserve the existing Agent caller and its matrix_transport checks. An Approval command must use the approval-only handle and approval current-scope checks; an ordinary handle cannot call it. The original config binding already hashes approval-reader-v1 and is included in the protected enrollment Context/marker, so restored Agent enrollment cannot be adopted by an approval SDK.

Factor only the common fixed enrollment protocol into a private scope-selected operation. Approval scope derives the original configured engagements' domain ApprovalRoomAuthority values, exact bot/device and owner+bot room membership; it never calls observe_matrix_transport or constructs an Agent route. Re-derive/compare the frozen authority/users and positively observe current private snapshots before each original external key POST and at completion, after queued SDK custody work. Use the existing exact CAS negative fencing when actual observations fail. Admission scopes and recipient sets cannot grow during an attempt.

The original approval SDK is created only after authenticated whoami/private observations, then immediately retained in the existing owner slot. The same SDK and same one-command queue perform version validation, initial bounded query, one bootstrap generation, original device/OTK+signing+signature uploads, anchored peer signatures, fresh exact query and one signed-session claim. No fixture seeding, new SDK client, seed import, TOFU, impersonation, auth/UIA completion, reset, general outgoing loop or retry is introduced. SDK Prepare also refuses retained approval intake/card mutation custody, preventing enrollment from stepping over an interrupted crypto operation. Reopen validates original Complete identity/session material; nonterminal enrollment never rearms or sends again.

## One original encrypted card attempt

1. Acquire the existing shared busy permit and retain a finite original job record plus the original Arc<Card> and immutable monotonic deadline before the first awaited custody/domain/SDK operation. Request ID is deduplicated across active and retained SDK history; a changed cutoff/content/target for the same ID is a conflict.
2. Read the approval-specific delivery journal. Historical exact accepted receipt returns historical Accepted without HTTP. Any retained incomplete attempt remains inspect-only. Otherwise verify that the current packet matches configured engagement, exact full owner/bot, room, SDK device, registration and room/binding generations. Call the original DomainStore check; refresh authenticated whoami and complete private room state using the existing approval path, then check the exact card again.
3. Persist a separate approval delivery Start containing the full frozen target, original cutoff, exact48KiB-bounded card content, content/attempt digests, original SDK identity and one deterministic purpose-separated Matrix transaction ID. It has no fake ReplyRoute/session/fence and no owner decision grant. Once this attempt is durable, replacement/reconstruction is refused even if later work fails before any PUT.
4. Query exactly the owner+bot keys through the original SDK; persist the response before crypto mutation. Reuse a private SDK leaf factored from ordinary outgoing encryption: unchanged keys::accept exact fresh authority checks, original usable Olm sessions, discard prior outbound group session, OnlyTrustedDevices, exact key-share recipient set and raw m.room.message encryption. Ordinary outgoing keeps its purpose guard and typed route checks. The new path cannot select plaintext fallback, arbitrary content, arbitrary event type or recipients.
5. Each original key-share/room PUT first persists indexed WritePossible. Reserve the original SDK acceptance queue slot before HTTP, refresh the current private snapshot and require the identical fresh verified key-query bytes, then perform the fresh original-domain card check after all awaited custody work immediately before HTTP. Check original cancellation/deadline on entry. No later SDK await occurs between that last domain check and HTTP admission.
6. Move an actual completed HTTP response into the reserved original SDK acceptance command synchronously before another cancellable await. Persist response before SDK to-device acknowledgement. A later cancellation/expiry prevents the next write but does not discard a known original response. Matrix event-ID acceptance becomes Complete, then a content-bound receipt. No domain permission/grant/application record changes.

Remote snapshots, the local domain writer, SDK persistence and HTTP are not one transaction. A room/key/domain change after final observation may race bytes already admitted. The implementation must retain that outcome rather than claim to recall an admitted request. Enrollment key publication does not by itself send private card content; the separate card path repeats its own stricter current checks.

## Journal, lifecycle and finite limits

Use separate approval_delivery / approval_delivery_receipts fields in the original StoreCipher journal, not ordinary outgoing fields or another database/client. Ordinary-purpose restore rejects any such fields. Approval restore retains the existing Agent outgoing/intake rejection and explicitly rejects foreign Agent upload/attachment markers alongside the new ledger. It validates new identity/purpose, full target fields, cutoff<=domain expiry, content/action/target agreement, hashes, query users, exact writes/order/index, reply shapes and receipt uniqueness. Restored metadata never reconstructs an Arc<Card> or current send permission.

Phases: Prepared, QueryPrepared, CryptoApplying, Ready, WritePossible, ResponseStored, Complete, Quarantined. Any nonterminal restored attempt is Uncertain and network-free, including prepared-never-written attempts. Complete may be compressed into its original receipt after rotation/expiry without new checks that would discard known history; no network or domain grant is involved. The same request cannot acquire a new transaction ID/body after an unknown outcome. Failed persistence poisons the live delivery owner; memory-only receipt data is never counted as durable success.

Bounds: one active job per existing collector semaphore; one card attempt;64 terminal card receipts with no eviction; exactly2 joined users and at most64 verified devices;48KiB card content, the existing60KiB encrypted room-wire body cap,256KiB query,4KiB accepted response, at most16 to-device writes plus one encrypted room write;1MiB serialized attempt and the existing16MiB shared encrypted journal cap. Start command<=64KiB; response/query validation occurs before queueing. Encryption expansion can make a card below the metadata48KiB cap exceed the unchanged60KiB room-wire cap; that case refuses without truncation or HTTP. Crypto derivation/encoding that exceeds the existing actual-attempt cap likewise refuses before any secret-bearing PUT. Receipt/cipher envelope capacity is checked before admitting new work. The64 persistent receipts with no eviction are a development capacity limit: restart does not reclaim them, and this partition is not an indefinitely running production approval service. No larger storage/deadline limits or unbounded raw snapshots are introduced.

Enrollment preserves the existing fixed profile and limits.sdk absolute operation budget. Card IO uses one original deadline no later than both owner cutoff and existing45s outgoing-operation budget; clock/read/control/queue transitions never restart it. HTTP and SDK command limits remain unchanged. Cancellation stops future HTTP but accepted SDK custody is awaited or reported Unknown according to its original limits, never silently replayed.

A small retained job registry on ApprovalCollector owns the original new enrollment/send/close task handle and result before awaiting it. Caller loss does not create a replacement. Panic/abandonment leaves explicit Unknown and blocks new mutations. A positive original SDK Read proving no pending card attempt permits a known refusal before Start to leave the collector usable (including an already-expired packet). The job marks itself blocking synchronously before submitting Start; lost open/read/Start acknowledgments and later crypto/HTTP failures remain blocking. Historical inspection never clears an unknown original mutation. Read-only historical inspection can remain available after the original worker is idle. Busy close does not consume the collector; admitted close freezes its original result and never repeats Owner::close or interprets a removed owner as a new success. Existing intake/observe methods check terminal-close/unknown job state and share the same semaphore. The original private SDK lock and observed shutdown result remain prerequisites to successful close.

## Qualification

Use a new actual local TLS fixture with the existing independent recipient OlmMachine helper parameterized for the approval bot/device. Defaults preserve every existing Agent/file fixture. It may provision only the recipient and its external public master anchor; the service SDK starts fresh and is populated solely through production SDK commands and actual uploaded/claimed HTTPS protocol bytes. Recipient-side verification of the captured original bot public identity is explicit fixture operator provisioning, not service trust injection.

Six bound scenario families (new selectors prefixed native_private_approval_):
1. fresh_enrollment_and_delivery: actual fresh bot uploads/claim, exact private card to-device share and room ciphertext decrypted by the independent owner, exact request/cutoff/actions/scope, duplicate delivery historical-only; an expired packet refused before Start leaves an empty journal and permits a distinct valid packet.
2. enrollment_refusals: wrong external anchor/device/signature, unsupported versions/existing own identity/UIA, bad/missing signed claims; actual interrupted enrollment command/request phases and reopen never regenerate keys/claims. Purpose/config inheritance and cross-purpose command/restore refusals.
3. current_private_authority: wrong whoami/device/owner/room, promoted or unencrypted room, stale registration/binding, retired task/dispatch and expiry all refuse. Held actual SDK WritePossible acknowledgment plus real original-domain mutation proves the fresh post-custody check prevents a PUT.
4. send_cancellation_and_loss: actual held/truncated key-share/room responses, caller drop before accepted SDK response settlement, cancellation/cutoff after one acceptance stops subsequent writes; original bytes/status retained and no rearm. A canceled/quarantined original approval intake cannot be overwritten by enrollment/delivery.
5. historical_restart_and_capacity: original Complete and incomplete journal reopen with no network, changed card cutoff/body conflict, malformed protected phase/digest/recipient/order/ACK/purpose records, receipt cap/no eviction and actual encoded limits.
6. retained_close: actual SDK shutdown/private lock held across caller loss, repeated close observes the same result, failed/unknown close never becomes success through an empty owner slot; original intake cleanup semantics remain covered.

Run all original approval intake, ordinary enrollment and outgoing/file crypto package tests after the shared leaf/purpose change. Run warnings-denied all-target Matrix Clippy, formatter, boundary diff and strict Task Contract lifecycle. Use only the assigned idle original-observation target, explicit CARGO_TARGET_DIR and CARGO_INCREMENTAL=0, verifying source paths/new actual selectors before counting results. Cross-compilation, if run, is compile evidence only; actual Windows/Linux execution belongs to later hosted CI. No public/client/live workflow completion is inferred.


Local qualification on the final formatted source:145/145 Matrix package tests,
including9 new actual behavioral functions; strict lifecycle7/7 with six selector
families and one exact28-path boundary; all-target warnings-denied Clippy and
fmt/diff pass. Zero-match Cargo targets are excluded. The context artifact and
external source/evidence manifest retain original failures and explicit fault
model limitations. Other mapped requirement/client/executable scenarios and
hosted Windows/Linux execution remain separate qualification gates.

## Consequences

Only explicit fresh approval accounts qualify for this initial sender. Historical
Matrix acceptance does not grant a verdict or runtime application. Service wiring,
client compatibility, identity recovery and general ongoing key management remain
separate. Already-admitted HTTP bytes cannot be recalled after later changes.
