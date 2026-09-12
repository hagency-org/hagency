---
kind: decision
id: ADR-059
title: Send frozen Matrix intents with protected SDK and external outcome custody
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
---

## Context

Sending a frozen Matrix intent crosses domain, SDK and external HTTP outcomes, each of which can lose an acknowledgement independently.

## Decision

The native host may send an existing final-reply or verified task-notice claim
through `hagency-matrix::Collector`. This extends ADR047/054's bounded transport
and owned SDK; it does not enable a service runner, enroll Matrix accounts,
publish device keys, establish trust or complete the migration. Plaintext group
sends and encrypted DM/group sends have real local HTTPS and SDK fixtures.
Encrypted sending requires identities, cross-signing and Olm sessions already
provisioned in the protected SDK store. The fixture-only provisioning code is
absent from production builds.

### Host inputs and exact authority

`send_final(ReplyClaim)`, `send_notice(VerifiedNoticeClaim)` and
`resume_outgoing_custody` are host APIs. They expose no arbitrary HTTP operation,
Matrix address, body, device-proof constructor or mutable credential. Configuration
retains ADR047's fixed origin, token, registration fingerprint/generation, full
MXID, Matrix account/device incarnation and room intents. Palpo machine-token
generation is not Matrix transport generation. Public summaries report only an
opaque intent ID, delivered/uncertain/idle and replay status; they contain no
room IDs, token, event ID, private content, SDK identity or filesystem path.

Final preview reads the current Claimed route and secret before any send. Notice
preview uses its frozen claim and host receipt, then verifies the exact content,
route, transaction and fence against the actual domain begin result. A mismatch
cannot send. The protected outgoing journal freezes BeforeBegin before submitting
the domain begin; domain Sending must commit before any key-share or room PUT.
A lost begin result remains inspect-only, even if nothing reached the network.
The retained journal never stores claim secrets.

Every possible PUT requires fresh authenticated whoami and a complete room-state
snapshot matching the frozen route, including full sender/device, registration,
transport, session and room generations. Direct rooms require exactly the pinned
human and Agent, invite-only membership and Megolm. No m.direct/display-name or
browser assertion establishes privacy. Unsafe snapshots follow ADR047's shared
negative-room path; a negative whoami fences only the exact captured transport.
Full joined membership must match the initially frozen set. Encrypted attempts
also require an identical fresh device-query result before each PUT.

Current Sending claim validation happens both before and after the awaited
WritePossible journal commit, immediately before HTTP. A revocation, promotion,
lease expiry or cancellation during that await cannot use the earlier validation.
HTTP checks cancellation again on entry and uses a biased cancellation branch.
This does not make a remote request atomic with future room/server changes: once
the PUT starts, concurrent remote revocation may race with acceptance. Such work
retains external outcome custody instead of assuming it was not sent.

### Content and cryptographic derivation

ADR050 formats only the frozen domain body. Notice msgtype is m.notice; final
msgtype is m.text. The thread relation is constructed solely from the frozen
route's root, including fallback and in-reply-to. A null root has no relation.
Domain intent digest, exact formatted content SHA256 and exact ciphertext/key
share body SHA256 are distinct values; none substitutes for another.

The inspected pinned matrix-sdk-crypto 0.18.0 implementation is in
`machine/mod.rs`, `identities/manager.rs` and `session_manager/group_sessions/mod.rs`.
The adapter uses `query_keys_for_users`, `mark_request_as_sent`, `get_identity`,
`get_user_devices`, `get_missing_sessions`, `discard_room_key`, `share_room_key`
and `encrypt_room_event_raw`. Low-level requests stay private to the one SDK
worker; no high-level unbounded crypto outgoing loop is started.

A bounded keys/query asks for every current joined user. The response must name
exactly that set, contain no failed server entry and supply at most 64 devices.
Every identity must be verified and every device verified and cross-signed. The
fresh supplied master/self-signing authority fields and device identity,
algorithms, keys and signatures must exactly match SDK-accepted values. This
extra check matters because the SDK can ignore malformed raw cross-signing data
while retaining a cached verified identity. A successful query alone is not
fresh verification. The current owned published device must appear and its
Curve25519/Ed25519 keys must exactly match the retained SDK identity. Missing or
changed recipient proof refuses the send and retires the relevant old scope.

Missing Olm sessions return Unsupported; no key claim/upload/trust request is
sent. The worker explicitly discards the old outbound group session, creates a
fresh session with `CollectStrategy::OnlyTrustedDevices`, verifies the exact
key-share device set (all verified current devices except this same sending
device), and immediately encrypts under that session while retaining exclusive
SDK ownership. Prior room-session recipients are never reused. There is no
plaintext fallback for encrypted rooms and no trust-on-first-use path. Actual
fixtures decrypt the transmitted to-device and room ciphertext with CrossSigned
trust and assert the new session differs from a seeded historical session.

### Journal, response loss and restart

The outgoing attempt and receipts extend the existing encrypted StoreCipher
journal in the SDK state store; there is no new schema or second domain store.
The original private binding, external wrapping key, public identity fingerprint
and exclusive owner lock remain mandatory. Existing-store recovery never creates
new identity keys. The account/device/registration binding cannot adopt another
registration's custody. Journal types are private, have no Debug projection and
cannot be submitted through an HTTP/runtime API.

The outgoing phases are:

| Phase | Meaning and recovery |
| --- | --- |
| BeforeBegin | Frozen intent; begin may or may not have committed. Inspect-only. |
| Ready / QueryPrepared | Original intent or prepared bounded query; never automatically resumed into HTTP. |
| CryptoApplying | Exact query response persisted before SDK mutation; interrupted mutation is unknown. |
| Quarantined | Explicit unsupported/malformed crypto derivation; retained for inspection. |
| WritePossible | Exact indexed PUT bytes frozen before IO; any subsequent failure is unknown. |
| ResponseStored | Key-share HTTP response persisted before SDK request acknowledgement; interrupted SDK update is unknown. |
| Complete | Exact room event-ID response persisted, with every earlier key-share acceptance present. May settle only that historical domain fence. |
| Settled receipt | Content-bound attempt digest, original ID/kind/fence; no new send authority. |

An actual HTTP success is stored before domain reconciliation. A domain Busy or
lost reply leaves Complete intact, does not invalidate a healthy transport and
can settle after restart without the claim secret or another HTTP call. Narrow
host reconciliation accepts Delivered while domain state is Sending as well as
Uncertain. NotSent remains Uncertain-only and is never synthesized by this
adapter. Late authenticated acceptance is recorded historically; it cannot
activate a retired task. A task notice activates only after exact acceptance in
current scope. Changed inspection contents or another fence retain the domain's
existing conflict checks.

A failed journal write poisons the live SDK outgoing owner: memory-only response
state cannot later be mistaken for durable acceptance. Reopen loads the last
committed journal. An accepted worker command retains its owner after response
channel loss or timeout. Resume is network-free: Complete settles, other retained
phases report Uncertain, and no pending attempt reports Idle. Uncertain writes
are never automatically resent, even with a stable Matrix transaction ID.

Restore independently validates SDK identity, all route generations, exact
thread relation, content/query/wire digests, whole serialized bounds, phase and
index consistency, exactly one final room write, prior key-share responses and
response shapes. Complete cannot omit an earlier acceptance. Duplicate settled
receipts or overlap with a retained attempt are rejected. Protected encryption
is not a reason to accept an inconsistent old journal.

### Finite resources and remaining gates

The collector has one active network job and one SDK owner. The worker accepts
one bounded queued command alongside one executing command. Start is capped at
512 KiB before queueing, query input at 256 KiB and accept input at 4 KiB. One
retained attempt is capped at 1 MiB of fully serialized JSON; formatted/room
content is capped at 60 KiB, a key-share body at 256 KiB, and writes at 17 (at most
16 shares plus one room message). There are at most 16 joined users, 64 devices
and 64 settled outgoing receipts. Capacity refuses new work without eviction,
including after restart. The existing shared journal envelope cap is 16 MiB,
including encrypted JSON expansion and intake custody. These are finite ingress
and journal budgets, not exact SDK heap/SQLite physical disk quotas.

Existing HTTP bounds apply to real GET/POST/PUT: authenticated pinned origin,
HTTPS CA/hostname verification, no environment proxy/redirect/implicit retry,
bounded headers/body/JSON depth, duplicate decoded-key and coalesced-document
refusal, finite DNS jobs and per-request/idle deadlines. A 45-second operation
timer cancels further IO and then awaits already accepted bounded SDK custody;
it is not a promise to interrupt OS SQLite/SDK cleanup within 45 seconds. All
errors remain fixed codes and omit private source data.

Live account/key publication, missing-session claims, trust/bootstrap/verification,
continuous membership/key tracking, encrypted-history import, automatic recovery
of uncertain SDK/PUT outcomes, persistent receipt compaction, Matrix media and
production host/service wiring remain explicit gates. The 64-receipt hard stop
must be addressed before continuous operation; history is never silently dropped.
Actual Windows/Linux execution is CI qualification, not inferred from macOS tests.
This slice neither enables native availability nor claims live UX/cutover parity.

## Consequences

Protected send history preserves possible writes and exact accepted responses without granting automatic resend. Live enrollment, trust lifecycle, receipt compaction and service wiring remain open.

## Alternatives Considered

Retrying an uncertain PUT or deriving a new recipient route from current state would abandon original send custody. Plaintext fallback for failed encrypted recipient checks would violate the frozen privacy boundary.

### ADR112 shared crypto leaf

The exact SDK encryption sequence is now a private leaf shared with the separate
approval-card sender. It borrows the same exclusively owned OlmMachine and keeps
fresh signed-device checks, original sessions, a new outbound Megolm session and
exact trusted key-share recipients. Ordinary outgoing retains its existing purpose,
route, domain begin and per-write checks. The leaf neither constructs authority
nor admits HTTP; approval cards never fabricate an Agent ReplyRoute or notice.
The independent test peer may name the approval bot while its original Agent
sender/device defaults remain unchanged.
