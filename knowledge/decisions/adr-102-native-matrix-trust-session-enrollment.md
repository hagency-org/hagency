---
kind: decision
id: ADR-102
title: "Enroll one fresh native Matrix identity and its original recipient sessions"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [rust, matrix, crypto, enrollment, custody]
---

## Context

ADR098 publishes encrypted files only when the existing SDK already has its
original cross-signing private identity, verified recipient identities and usable
Olm sessions. Its positive library fixtures explicitly prepare those prerequisites
inside cfg(test). A fresh native serve process cannot call those fixtures. ADR101
therefore cannot yet prove its required first send_file call and real recipient
delivery. Refusal of an unverified recipient is correct negative evidence and
does not satisfy that positive executable gate.

The pinned matrix-sdk-crypto 0.18.0 supplies real enrollment and session APIs.
bootstrap_cross_signing(false) generates and saves a private identity if its local
private identity is empty; false alone does not protect a remote existing identity.
OtherUserIdentity::verify requires the original own user-signing private key.
pin_current_master_key records continuity but does not make is_verified true.
Applying a keys/claim response can return Ok after skipping a missing, unknown or
badly signed recipient key. Neither Ok nor get_missing_sessions returning None
proves all intended recipients have sessions.

This prerequisite was accepted after coordinator and independent review. Its executable selectors remain required. No
enrollment, service activation or positive Windows durability result is implemented
or established by this document. The local pinned SDK source and precise future
interface/path manifest are recorded in the external ADR102 review artifact.

## Decision

### Explicit fresh account scope

Add an optional closed matrix.crypto_enrollment object to the existing private
development-driver.json. Its only profile is fresh_own_account_v1. peer_masters is
an array of objects containing a complete user_id and one canonical unpadded
base64 Ed25519 master_key. Accept at most 16 distinct other users, at most 16 KiB
encoded configuration and no unknown or duplicate fields. Anchors are obtained
by the operator outside the Matrix key-query channel. All current joined peer
users in the configured encrypted rooms must have anchors; an anchor does not
admit a user into a room. Devices remain bounded to 64 across the frozen user set.
An absent profile preserves existing behavior. The model, MCP and HTTP file
requests cannot supply, amend or enable this configuration.

Use only the configured ordinary user access token and exact account/device from
actual whoami. Application-service tokens, impersonation, UIA credentials, signing
seed import, key reset, SAS/QR, secret-storage recovery and automatic key rotation
are outside this profile. A normal account token and exclusive host provisioning
are operator prerequisites, not properties inferred from token syntax. The code
never appends an impersonation parameter or an auth field.

Before any key write, obtain the actual configured-origin client/versions result
and require an explicit supported version in v1.11 through v1.17. Unknown-only,
older, malformed or unavailable version evidence refuses. The corresponding
standard requires UIA for a regular client to change existing signing keys; its
exceptions permit an absent identity or exactly unchanged keys. Thus a competing
different identity after the last query must be refused by a conforming server
when no auth is supplied. The v1.17 application-service exemption is excluded.
This guarantee depends on server conformance and ordinary-user provisioning; a
query followed by a write is not a conditional update supplied by the SDK.
HTTP401/UIA, unsupported responses and partial signature failures remain explicit
refusal, without completing a challenge or repeating an upload. The normative
source is the [Matrix device-signing upload specification](https://spec.matrix.org/v1.17/client-server-api/#post_matrixclientv3keysdevice_signingupload).

### Original generation and protocol sequence

The same Collector and SDK owner used by the driver own this operation. After
actual collect and the existing one historical outgoing-resume pass, an enabled
profile must complete enrollment/session validation before file-worker readiness,
compatible claim, Started handoff or runtime launch. Failed refresh may still use
the existing historical settlement boundary; it never enrolls or enables launch.
No application file tool, raw response setter or separate SDK client is added.

The first pass validates actual whoami, configured room state and exact current
domain generations, then performs a bounded SDK-originated keys/query. The own
remote master, self-signing and user-signing entries must all be absent; the own
remote device map must be empty. The SDK must have its original device account
but no public or private cross-signing identity and no enrollment marker. Any
existing remote identity, mismatching device, incomplete local private material
or unexplained local identity refuses. Existing identity enrollment/recovery is
a separate future gate, even if the server advertises a plausible public master.

Persist the enrollment marker and Preparing state before calling
bootstrap_cross_signing(false). The SDK then generates and durably stores the
original three private signing keys and its verified public own identity once.
The private keys remain exclusively in the original encrypted SDK crypto store;
they are never exported into a configuration, task, journal response, MCP result
or test injection. A failure or lost acknowledgement after Preparing is Unknown,
including when no private identity can subsequently be loaded. It never generates
another identity and never repairs or removes the marker automatically.

Capture the returned original bootstrap requests in the protected enrollment
ledger before another cancellable handoff. Preserve the returned keys-upload
transaction ID. Send the SDK's device/OTK upload first, its cross-signing upload
second and its own signature upload third. Fresh mode requires the actual device
key and signed one-time-key upload, not an empty fabricated acknowledgement.
Typed successful responses are applied through mark_request_as_sent; signing and
signature requests without an SDK-provided transaction ID receive a single local
correlation ID when the original immutable request is retained. They are not
regenerated to obtain another ID. No general outgoing_requests loop is introduced
or mixed with upload_device_keys.

Before signing peers, the actual initial query's supplied identity and device
fields must exactly equal the identities and devices that the SDK accepted.
Validate full user IDs, usage, algorithms, key IDs, public-key bytes and signatures;
reject device/cross-signing key-ID collisions. Every peer master must equal its
operator anchor. A malformed fresh entry cannot fall back to a cached identity.
Only then call OtherUserIdentity::verify for each frozen anchored peer. Persist
each original returned request and send it once, in a fixed user order; never
merge server-supplied replacement keys into the request. There are at most 16 peer
signature uploads. All signature acknowledgement failures must be empty.

Perform an actual new SDK keys/query after upload acknowledgements. Reuse the
unchanged keys::accept verified-identity, cross-signed-device and exact-fresh-field
checks. In addition compare every own public signing key to the original generated
identity and every peer master to its anchor. This is the trust prerequisite for
sessions; a local verification mutation or upload status alone is insufficient.
Current whoami, configured room membership/privacy and expected domain generations
are rechecked before each external key write and at the final readiness boundary.
New negative observations use the existing retained fencing path.

### Actual signed sessions

Use get_missing_sessions once for the frozen verified recipient users. Preserve
its exact SDK transaction ID and immutable request. Reject any unrelated queued
user/device, unsupported algorithm, excessive request or failure-cache omission
that leaves an original recipient without a stored session. The request may omit
only devices whose exact original Curve25519 key already has a persisted session.
Only one claim request may be in flight in the SDK owner.

Send the real keys/claim request once. Require an empty failures map and exactly
one signed_curve25519 key for each requested user/device, without missing or extra
identities. Persist the actual transport-derived bounded response before applying
it to the SDK. The SDK verifies the signed one-time key against the stored device
key and creates the real outbound Olm session. Since it can skip bad device keys
without an overall error, read the actual persistent CryptoStore session records
for every original recipient curve after application. Require a usable matching
session for each, and on fresh claims prove the new session belongs to that exact
request's device key. Before claiming, retain the original session-ID set for each
curve and require every requested device to have no prior session; afterwards
record its actual new persisted session ID. Complete reopen must match these
original associations. None from get_missing_sessions is not a substitute.

Existing outgoing encryption keeps OnlyTrustedDevices, fresh keys::accept,
exact recipient-set equality, forced fresh room keys and current-domain checks
after the last SDK await. Enrollment does not create a file Send or a dispatch
capability. New users, changed masters/devices or missing later sessions refuse;
there is no opportunistic per-file enrollment, OTK replenishment or claim loop.

### Finite custody and recovery

Use the existing one SDK thread, one-entry command queue and one Collector busy
permit. The Collector retains one owned operation before its first await. Caller
drop cannot discard original requests, responses, negative fencing or the queue
permit. No request spawns a second owner, and no process-global body map is added.
Inner retains only a small Job/state object without an Arc back to Inner. The
separate operation handle and finite running task retain Inner; Job retains no
JoinHandle that owns the same task. A Weak teardown test detects a settled owner
cycle. The operation's absolute deadline is Instant::now() + config.limits.sdk,
captured once at admission; the current default is 20 seconds, not a new setting.
All original HTTP/SDK sub-deadlines and retry policy remain unchanged. A timeout
remains its original error, while completed queued work may be inspected later.

Add one separate StoreCipher-protected finite custom record with a marker in the
existing SDK journal. Bind it to the original SDK binding/device identity and
canonical enrollment profile plus all original public identities. Retain at most
20 write records: one device/OTK upload, one signing-key upload, one own signature
upload, up to 16 peer signature uploads and one session claim. Each immutable
request and each retained transport-derived response contributes at most 64 KiB
to the serialized ledger, including its JSON string escaping or other embedding.
Each of at most two key-query snapshots contributes at most 256 KiB. These fields
therefore consume at most 3 MiB; all metadata, keys, brackets and separators are
separately limited to 64 KiB. The complete plaintext record is capped at 4 MiB and
its encrypted envelope at 24 MiB. Before Preparing, checked arithmetic reserves
every maximum field contribution plus the maximum metadata and cipher envelope;
failure refuses before generation. Per-field serialization is checked before
retention, so escaping cannot evade the bound. These are logical encoded bounds;
the existing HTTP receive buffer and parsed-object overhead are separate bounded
allocations. Query replacement/revalidation uses the same two reserved snapshot
slots, not an accumulating history.

Reserve the worst-case remaining record and response capacity before generating
identity or starting a POST. Acquire command/operation custody before copying
request or response bytes. A failed persistence retains the original bounded
in-memory value in the poisoned SDK owner until explicit close. No unbounded
public snapshot, cloned raw map or private-key projection is returned.

For each original write, record Prepared, then WritePossible before polling HTTP,
then its actual accepted response, then Applying before SDK mutation and Applied
after the SDK acknowledgement and required durable postcondition. Original
request generation has its own Preparing phase because it changes SDK state.
A lost HTTP response, marker, persistence acknowledgement or SDK apply result
never reconstructs a live request. Unknown cannot rearm, even when the remote
endpoint describes equal requests as idempotent. A complete historical response
retained after cancellation is evidence only, not permission to continue writes.

On reopen, missing/torn marker pairs, corruption, profile/binding substitution,
partial private identity or any nonterminal phase fail closed without key writes.
There is no automatic reconciliation or request replay in this slice. A Complete
record can be validated against the original private/public SDK identity and real
persisted sessions, followed by fresh current observations; it cannot generate
new keys or claim another session. Explicit owner close or process death can lose
uncommitted bytes. Durable SDK/domain outgoing settlement remains independently
available through its existing historical-only boundary; it does not require a
new enrollment or confer current readiness.

### Executable proof

Tests must start a real fresh SDK through authenticated Collector/serve paths.
An independent recipient OlmMachine owns its own real signing/device/one-time
keys. The local TLS server returns protocol responses derived from those keys,
checks actual service uploads and applies signature uploads to its query results.
It never calls a production SDK trust/session setter or preloads the service DB.
The service's operator anchor is provisioned before launch from the independently
created recipient public master. Its real claimed session must decrypt the actual
encrypted to-device share and room event, and the recipient must decrypt exact
file ciphertext/metadata under the existing ADR101 executable selector.

Recipient-side sender enrollment, if strict receive verification is asserted,
must be documented separately as explicit fixture operator provisioning of the
original service public identity. It cannot make the service trust server keys.
Refusal tests include wrong anchors, valid-looking unsigned/changed device keys,
missing or bad signed claims, partial200 responses, existing own identities,
unsupported versions, a concurrent identity causing UIA and all bounded original
request-loss phases. Held real SDK/database/HTTP boundaries prove caller loss,
unknown retention and unchanged deadlines. Reopen proves reuse of original keys
without upload or claim replay. ADR101's original executable and first-Delivered
restart selectors remain required; a fresh child must establish actual historical
SDK Complete independently before the first domain Delivered without HTTP replay.

## Consequences

Fresh opt-in accounts gain a narrowly supported path to the existing verified
encryption policy. Powerful preexisting signing seeds are not a setup prerequisite.
An account already enrolled elsewhere, an unsupported server, new recipient keys
or any ambiguous enrollment must stop for a separate operator recovery workflow.
This deliberately does not promise unattended general Matrix account management.

The protocol and SDK persistence steps are not one atomic transaction. The
protected Preparing/Applying markers make that gap unavailable after ambiguity;
they do not manufacture rollback or repair. Real file delivery and historical
settlement remain distinct from enrollment, upload acceptance and canonical Done.
Existing Windows directory-sync uncertainty remains an explicit positive workflow
blocker; this prerequisite does not upgrade platform evidence or alter deadlines.

## Alternatives Considered

Importing all three existing cross-signing seeds is supported by the SDK after
exact public-identity comparison, but requires powerful operator secrets and does
not satisfy a fresh-account default. Full SAS/QR verification and interactive
authentication are larger separate enrollment workflows. Local device trust
setters, continuity pinning alone and trusting server-provided master keys would
bypass the required verified-recipient policy. Repeating bootstrap(false) or a
claim after an unknown result could replace original ownership. Reusing positive
cfg(test) SDK preparation in the native executable would conceal the prerequisite.


### 2026-09-11 — Final enrollment and native file integration checkpoint

All eight actual Matrix enrollment selectors pass on final source. Native and
Windows GNU all-target Matrix Clippy pass with warnings denied. Restoration now
checks exact original request order and required fields, applied ACK shapes,
operator anchors, original recipient curves and bounded before/after session IDs.
Actual caller-loss fixtures retain the same SDK command/result at Prepare, Accept
and Finish boundaries; unknown work never generates a replacement identity or
request. Inspectors use dedicated runtimes, closed and destroyed before another
owner opens the original SDK, to finish scheduled background connection drops.

The complete native file_service target passes all three tests: actual group and
DM delivery; first Delivered recovery with an original-context native MCP read;
and uncertain actual POST/PUT plus a departed-human Direct-room refusal. Old
process teardown now explicitly checks kill/wait. The historical own-ID query
returns Delivered and an unrelated ID refuses, even with current whoami401 and
no source file. Truncated POST/PUT cases remain Unknown with no acceptance or
replay; their actual runtime negative result is never called helper success.
Initial room refusal supersedes queued work without claiming or sending.
All-target hagency Clippy passes with warnings denied.

Prerequisites are explicit: the disposable peer privately captures only its real
inherited context, and the previously validated ADR101 status fixes preserve
Delivered after cancellation. Before that prerequisite was integrated, the new
actual historical query failed with Unknown despite domain Delivered; the failure
is retained. Earlier uncertainty fixture failures used the positive-only waiter
and assumed a refused dispatch remained queued rather than superseded; both
were corrected without changing polling/deadlines or weakening positive checks.
An earlier Matrix reopen test failed without recording its error. Later passes
and the inspector lifetime improvement do not establish that failure's cause.
All original failure logs remain separate from final successful checks.

This is an isolated implementation checkpoint. Final main-branch combined tests,
strict lifecycle/binding checks and native Windows positive staging are still
required. Windows GNU compilation alone is not platform qualification; full
migration and production cutover remain incomplete.

### ADR112 approval-purpose enrollment amendment

The original fixed enrollment protocol is shared behind an explicit checked
Agent/Approval command purpose. Agent callers retain their ordinary transport
observations. Approval callers use exact configured approval room authorities,
owner+bot snapshots and existing negative CAS fencing. The approval purpose is
already part of the protected config binding and enrollment marker; it cannot
adopt an Agent ledger. Original requests, response handoff, Preparing/Applying
ambiguity, anchors, session postconditions and finite budgets remain unchanged.
No identity recovery or automatic replacement is added. The new sender requires
its own Complete record; test-only seeded approval readers remain intake-only.
