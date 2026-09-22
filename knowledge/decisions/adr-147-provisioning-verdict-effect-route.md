---
kind: decision
id: ADR-147
title: "The provisioning verdict, its effect and the new engagement's route"
status: Decided
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [native, provisioning, ingress, engagement, verdict, matrix]
---

## Context

### Implementation checkpoint 2026-09-16 — intent is not physical fulfillment

The current Matrix verdict path verifies the provider and reserves a pending
provision effect. It does not yet run the complete inline account/home/room/runtime
provisioner required below. Approved intent is therefore not observed Applied,
and it must not synthesize an Active engagement or a derived session route.
The console's real-ingress roster completion test remains failing; the binding
audit also identifies the missing effect-completed and session-route selectors.
This is incomplete implementation, not a change to the accepted product flow.

The exact host claim prerequisite is now implemented as
`DomainRepository::claim_effect_for` and the matching bounded `DomainStore`
method. The actual external-account adoption caller uses it instead of claiming
the lexically first pending effect and filtering afterwards, which could start
an unrelated engagement. Two real SQLite/writer regressions pass exact competing
claims, rollback, exhausted fences, cancellation and old-registration refusal.
Original Started custody reopens Uncertain. The generic cleanup claim remains,
and no effect worker, new schema, synthetic physical receipt or runner setter
is introduced. These claims do not themselves complete provisioning; the full
inline physical implementation and real two-agent qualification remain owed.

The ordinary registration-token account step is now implemented separately as
the Host-only `TokenAccountProvision`. It retains create-only encrypted
original effect/fence/registration custody before a registration POST, uses an
independently random discarded password, and admits an opaque account handle
only after the actual returned token freshly authenticates the matching MXID
and device. Lost responses and partial bootstrap never permit another account
creation; accepted response recovery is whoami-only. Five offline local-TLS /
real-filesystem tests and library/test Clippy pass, including dropped receiver
ownership and the original cumulative deadline. These are account-step
fixtures, not real Palpo account-creation, SDK-enrollment, room/home/runtime,
full inline approval or application-service-profile qualification. The step
is now connected to `approve_provision` through the explicitly configured
Host-only `TokenProvisioningHost` account-stage owner. Native bootstrap reads
the `registration_token_account_step_v1` marker and separate protected token
and wrapping-key files; neither secret enters the driver JSON or console. The
approval refreshes verify-only project authority, then claims the exact effect
inline. Each registration POST and accepted account observation rechecks its
original Started effect/fence/payload, Reserved engagement and registration
through the same DomainStore. The finite non-evicting Host registry retains
opaque observed accounts and unknown jobs through receiver loss and replay.
Account errors retain Unknown; successful account observation leaves Started,
not Complete/Active, until the remaining physical stages are implemented.
This wiring creates no domain completion or route,
and does not close either missing provisioning selector. ADR-016's mandatory
application-service and registration-token deployment profiles both remain
part of the full factory's completion requirements.

The explicit private `registration_token_rooms_enrollment_step_v1` checkpoint
now connects ordinary-account creation to actual agent-created encrypted owner
DMs, representative-authorized project invitations, actual agent joins and
pre-activation SDK enrollment. Fresh full-MXID credentials, project binding and
invitation powers plus original Started/Reserved DomainStore scope are checked
at write boundaries. An owner invitation is not joined membership: the retained
job waits only for the owner's actual joined state within its original finite
budget. Fixed create-only encrypted room records retain every original possible
POST and accepted response; partial/lost attempts never repeat. Completed room
inspection is GET-only, and known room IDs remain historical metadata, not
readiness or caller-supplied physical proof. The original opaque account remains
owned even when later room or SDK setup fails. Plaintext project participants
retain membership/authority checks without invented encryption anchors; encrypted
participants still require their actual peer anchors under ADR-102.

Six original Matrix selectors cover local TLS with separate representative and
independent owner actors, actual recipient decryption in the newly created DM,
caller loss/lost responses, read-only replay, changed scope, owner-join timeout
and torn/partial/foreign/extra fixed custody. The actual executable bootstrap
selector checks the closed marker and separate protected files. These are
offline fixtures, not Palpo provisioning or full factory qualification. Both
deployment profiles, managed home/template and runtime setup, genuine Applied/
Active completion and session routes remain required. Neither missing physical-
completion/session-route selector is withdrawn, and no full migration/release or
production cutover is established by this source checkpoint.

The explicit private `registration_token_home_rooms_enrollment_step_v1`
checkpoint now materializes the original v1 home before account registration.
Its fixed existing private root, declared project roots and native task-client
file retain actual directory/file identities. Original request identity and
actual writer-selected resource configuration determine the home/manifest/entry
documents and supervisor sibling. Both bounded copy and explicit symlink modes
remain supported; unsafe external links, special files, changed roots and exceeded
bounds refuse rather than silently importing another tree. Native task wrappers
invoke the native CLI, contain no runner credential, and cannot assign tasks.

One finite original job owns the physical copy through its real return even
after caller loss. Protected create-only possible/complete records and held
home/source/project/binary identities prevent partial or foreign rearming.
Fresh resource and original Started/Reserved writer checks follow filesystem
work. Revocation during an actual unfinished 60 MiB copy leaves the physical
result retained but prevents registration, room or SDK effects, including replay.
Five offline home selectors, the native executable profile selector and strict
affected-package Clippy pass. These physical home observations are not a runtime
start, sandbox proof, Applied/Active receipt or derived session route. Both
deployment profiles and the full genuine inline runtime/completion/route path,
real factory qualification, new sustained/recovery soaking and release gates
remain required; neither missing completion/route selector is withdrawn.

The explicit private `appservice_login_home_rooms_enrollment_step_v1`
checkpoint now connects the mandatory AS account kind to the same original
home/room/SDK owner. One private fixed side master authenticates passwordless
registration with inhibited login, then a separate dedicated device login.
The device session is private operational SDK custody, never a canonical agent
credential or substitute for side authority. Full registered sender, virtual
identity and refused outside-namespace identity reads qualify the side master;
fresh original writer checks follow the last HTTP await at write boundaries.

AS-specific encrypted create-only registration/login stages pin the original
profile, origin, credential hash, effect and fence. Receiver loss retains the
same operation/lock; lost responses, partial/foreign/torn custody, unsupported
legacy login and changed credentials never rearm or fall back. Ordinary-account
serialized custody identity and markers remain unchanged. Native bootstrap reads
only separate protected AS/representative/wrapping-key files and a closed public
home/namespace/peer configuration; neither secret enters driver JSON or console.

Four account selectors, two original inline selectors and the native executable
selector pass offline. Independent recipient decryption uses the actual created
account, DM and original claimed SDK session. Revocation or broadened namespace
at a held final downstream read blocks room invitation or the first SDK key POST
despite the device token still authenticating; original possible custody remains.
Full Matrix passes 211 tests, bootstrap passes 13, affected-package strict Clippy
and Windows GNU all-target compilation pass. Compilation is not native Windows
execution. The binding inventory lists 941 selectors and still lacks the two
physical-completion/session-route selectors. No lifecycle qualification is claimed.

This is not either complete deployment profile: native HS-authenticated AS
transactions/user queries and registration-file generation, genuine retained
inline runtime launch/handoff, Applied/Active/derived routes and real fleet/
two-agent factory qualification remain required. Runtime must reuse the existing
fixed owned launcher and preserve its IO/process custody, not become an
initialize-and-stop probe or a second launcher. Source is not deployed; the failed
generation-11 live soak/recovery and five unknown dispatches remain unchanged.

### Implementation checkpoint 2026-09-16 — retained runtime task-context bridge

The existing fixed owned launcher now has a Host-only task-context prerequisite
for a capability minted after genuine future warm initialization. Its explicit
opt-in inherits only a closed private record reference and marker; the default
direct environment path is unchanged. After actual acknowledged Started and
initialize, the same owner retains one create-only bounded context job through
real filesystem return, then binds its fixed typed helper only while Ready and
opens the thread. The actual create descriptor is held before writing. Original
Started marker, capability/fingerprint, deadline/cancellation owner, current
writer authority and file/root/byte custody remain fenced; a later task snapshot
cannot reconstruct a lost start acknowledgement. No new launch path or setter
produces task/approval authority. ADR-057 records the explicit private operational
file extension and its secrecy/verification limits.

Six exact offline selectors execute the original writer, retained caller, driver
and actual owned native MCP child. The parent initializes with no record or
inherited secret; after binding, the real helper reads/heartbeats/readbacks through
the scoped API, exits successfully and never reloads a subsequently invalidated
startup record. Heartbeat remains canonical InProgress with no final reply, and
macOS whole-tree cleanup remains unproven with its lease retained. This is actual
pipe/helper/API evidence, not effective sandbox or model readiness. The selected
57-test Native regression and strict linting pass after the final Started check;
an intervening three-initialize-timeout run remains failed with cause unproven.
The existing real-sandbox gates still reject the checked-in evidence placeholder.

Current dispatch still receives Started **before** its only spawn. Actual
pre-activation inline warm-worker/IO-reactor custody and handoff through that same
owned execution path are not implemented by this bridge. Neither complete factory
profile is exposed, no Applied/Active or derived session route is fabricated, and
the two original physical-completion/session-route selectors remain missing.
The remaining factory must also hand off its original pre-activation enrollment
SDK to normal current-Active scope without resetting or replacing its owner.
Mandatory AS receiver/registration generation, genuine configured fleet/two-agent
and real client/recovery re-soaking remain owed. Source is not deployed; mini3's
75 completed dispatches, 74 Delivered replies and five unknowns remain unchanged.

### Implementation checkpoint 2026-09-16 — actual retained pre-activation owner

The fixed launcher now has a genuinely consumed Host-only `WarmRuntime`, not an
initialize-and-stop or unused warm probe. Its private writer-produced provision
scope, actual materialized home, current account/registration/profile and sticky
producer claim admit one original worker. That worker retains the initialized
OwnedSession and its current-thread IO reactor during finite Ready/no-helper idle,
then accepts one real dispatch through the original owned execution/finalization.
Actual acknowledged Started and required workspace acknowledgment still precede
task-context/helper/thread/turn IO. The initial and dispatch deadlines are absolute;
normal/active drivers cannot be extended and no connection/settings reset occurs.
Original live capacity transfers across phases, with unknown late startup held.
ADR-053 records the explicit cold-start exception and shared-capacity correction.

Eight bound offline selectors exercise actual writer scope/readiness/refusals,
typed idle lifetime, native managed-account ownership, caller loss, late-spawn
capacity, lost workspace acknowledgment and real native MCP heartbeat/readback/exit.
Two simultaneous initialized owners occupy the original two-slot budget; the
acknowledged dispatch consumes its existing slot and uses the same observed PID
with exactly one initialize. An actual held initialize survives a dropped ready
wait without replacement. These are real offline pipes/helper/API/filesystem
observations, not live model or factory qualification. Test fixture activation is
explicitly synthetic and does not prove the product's own Applied/Active result.

Neither complete factory profile exposes this prerequisite yet. The original
inline home/account/rooms/enrollment owner must now retain and consume it, and
the original enrollment SDK still needs actual current-Active scope handoff
without reset or replacement. Genuine physical Applied/Active and derived routes,
mandatory AS receiver/registration generation, configured roster/fleet/two-agent,
effective sandbox and reliable real Palpo/Robrix recovery re-soaking remain owed.
Neither original missing physical-completion/session-route selector is withdrawn.
This source is not deployed and does not repair or relabel the failed generation-11
soak, its five unknowns, full M0–M9/parity/release or production-cutover gates.

### Implementation checkpoint 2026-09-16 — original SDK Active handoff

The opaque account's original successful pre-activation enrollment job now owns
its immutable original provision scope as well as its original Collector/SDK.
`active_collector` admits a retained, forward-only current-Active handoff rather
than exporting credentials/configuration or opening another owner. A separate
read-only writer validator requires the original acknowledged effect's exact
fence/payload to be Complete, the engagement Active and registration current;
the existing Started/Reserved validator is not broadened. The same SDK must
already contain its original protected Agent Complete. Ordinary collection and
Complete verification plus fresh actual whoami, project owner/binding/invitation
powers, encrypted joined owner+agent Direct privacy and AS authority then execute
on that original queue/owner. A final original writer read follows the last await.

The original busy permit, absolute SDK budget and finite retained job survive
caller loss. Active success may refresh only the same original Complete/owner;
failed, running or closed work cannot rearm or return to pre-activation. Failed
post-collect verification fences only the exact original positive transport.
No signing upload, session claim, identity/config/store replacement, Applied
receipt or derived route is generated by the handoff.

Five bound offline selectors cover the exact writer phases, same actual SDK
channel/Collector, unchanged public signing material and Complete, no repeated
uploads/claims, negative fencing, premature/rotated/revoked/unsafe/closed-owner
refusals and actual held TLS reads through waiter loss and canonical revocation.
Actual AS home/account/rooms/enrollment also hands off the same SDK owner while
unchanged namespace authority is freshly observed. Revoked or broadened side
authority refuses despite the dedicated device still authenticating; no AS
registration/login, room, signing upload or session claim repeats.
Fixture Applied is explicitly not physical factory fulfillment. ADR-102 records
the preserved identity/session/custody contract. Full original inline home/account/
rooms/SDK/warm-runtime owner consumption, fresh physical current-owner proof and
genuine Applied/Active/derived routes for both complete profiles remain to be
implemented and qualified. Mandatory native AS receiver/registration generation,
configured roster/fleet/two-agent, sandbox, completion/recovery reliability and
sustained real mini3 Palpo/Robrix re-soaking remain owed. Neither original missing
factory selector is withdrawn; source is not deployed and failed Gen11/five
unknown dispatches and full M0–M9/parity/release/cutover remain unchanged.

### Fresh physical warm owner prerequisite (2026-09-16)

The original warm worker now qualifies its retained physical leader over the
same guardian channel (or Windows process handle), with fresh current writer/
home/account reads after physical IO. Ready waits retain their exact inspection
receiver and absolute response/idle bound through waiter loss; an expired
buffered positive cannot become readiness. Private guardian nonces correlate
fresh observations, unknown observation cannot rearm, and only an exact late
reply can be drained before original cleanup. Linux's optional independent
cgroup cleanup proof is not replaced by a peer's Stopped report. Idle maintenance
and dispatch handoff also check the original owner, with a consumed dispatch
closure retained outside the cancellable wait for its original finalization.
The first Ready failure stays sticky through later cancellation/teardown.

These are factory prerequisites, not physical fulfillment, model/effective-
sandbox or complete-profile qualification. The full inline factory must still
consume the original home/account/rooms/SDK/runtime, write genuine Applied/Active
and derive session routes. Both profiles, native AS receiver/registration
generation, configured roster/fleet/two-agent, completion/recovery reliability and
real mini3 Palpo/Robrix re-soaking remain required. No missing factory selector
is withdrawn or supplied with fixture activation; source is not deployed.

### Original provisioning-ingress context

The inline factory runtime contract now explicitly authorizes the concrete
Host-only Matrix -> execution bridge described in ADR-053. Original physical
home/account/room/SDK observations, fresh original warm-owner qualification and
exact scoped writer activation must precede route creation. The acknowledgment
is required, not reconstructed from Complete after receipt loss. The same
enrolled SDK supplies current Active observations and the actual created
owner-DM ID selects the null-root session_{engagement_id}; known room identity
alone is not authority. This path cannot expose a complete deployment profile
before its AS receiver/registration-generation and native fleet gates exist.
No existing checkpoint marker is redefined and no target fixture activation
satisfies the two original factory selectors.

The configured original approval Collector is also a required concrete factory
capability. Its fixed bot/registration/endpoint must match and it validates the
exact writer-produced warm scope on its own original DomainStore before launch;
a foreign writer cannot contribute approval authority. After the received Active
ACK, that same Collector authenticates the target's protected private approval
room before any session route is published. No target approval binding is seeded,
no foreign project room is rebound and native approvals stay enabled. Failure
there retains exact Active history and negative runtime custody, not backward
Unknown or a dispatchable agent. This observation does not implement the still
required dynamic native fleet/approval pump or full deployment-profile gates.
An admitted finite factory shutdown also retains the coordinator's original
busy permit on its owned closure job through waiter loss. It drains all retained
agents despite an individual failure, retaining each original result and reporting
aggregate OutcomeUnknown for a partially failed drain. Known coordinator Busy
before admission remains effect-free. Admitted shutdown closes every retained
agent's dispatch admission before awaiting drains; SDK Busy retains its original
negative custody without admitting new work. Joining owners never certifies whole-tree
cleanup or releases unknown capacity.
First-dispatch admission is spent before its ownership-worker task is queued.
That task owns the exact warm handoff and any failed WarmRuntime or unreceived
Operation's synchronous join. Losing the queued wait cannot select another
capability or rearm the original owner, and no async caller performs that join.

Sequential dispatch extends that same admission boundary with an original-runtime
ticket and ADR053's private completion witness. The first initialized owner is
never replaced after failure. Only positively settled and fully stopped prior
work, whose original result and worker join were observed, permits a distinct
fresh Started operation using the same retained Host/account/workspace and SDK.
Pending or unknown work and mutable public reports cannot open admission. The
single warm task record is not reused for a later task; the existing ordinary
direct helper binding supplies that task's actual capability after Started.

The factory's same private observation now retains the activated engagement in
the original approval collector under ADR064/112. Its original producing scope
is checked before and after authenticated IO; only successful admission extends
the bounded membership. The original bot's existing Complete SDK, private rooms
and anchors remain fixed. Card delivery, enrollment, verdict intake and shutdown
freeze that membership under the same busy permit. This closes rejection of a
new factory agent solely because it was absent at bot startup; it does not add
native fleet scheduling or a service approval-verdict pump.

The factory membership observer now participates in ADR064's bounded host service
turn on that SAME configured original approval collector. Waiting does not grant
membership or relax its existing busy/job/current-scope checks; actual observation
still follows the original Active acknowledgment, and original warm readiness is
rechecked before publishing a route. A cancelled/lost observer retains its owned
work independently of the scheduling guard. This prevents routine contention with
the service approval pump from immediately failing a newly activated agent, without
permitting re-entry past unknown custody or claiming configured fleet readiness.

The provisioning ingress (task-rust-provisioning-ingress.spec.md, landing) admits
an engagement from a `com.hagency.engagement.request.v1` event observed in the
pre-project reception room, verified by `verify_request` and minted by
`DomainRepository::admit` (`domain.rs:1084`). Admission leaves the engagement
`pending` — minted but neither **effective** (the provider's approval observed,
`approve` written, the provision effect claimed and observed complete) nor
**routable** (a `matrix_session_routes` row). The next slice closes both.

The builder implemented that second half on four assumptions that no ADR or spec
decides. They are product decisions, and this ADR decides them, each against the
retained product (the JS/TS sources at the repo root) and the native ADRs.

### Placement (review addendum)

The review of the slice asks that each decision be recorded where the owning ADR
already governs that surface, amending an existing ADR rather than creating a new
one when one owns the surface. The placements are: (a) to the approval wire
surface, **ADR-143**; (c) the session-key derivation, **ADR-095**; (d) the
MXID/device shape, **ADR-014**; and (b) is confirmed-by-existing-decision under
**ADR-022**. This ADR records the four decisions together as the slice's single
decision of record and names that placement for each; the per-ADR amendment text
is applied to ADR-143, ADR-095 and ADR-014 in their own files. (The review's cited
evidence lines — ADR-095's 2026-09-14 amendment and the intake `provision()` body
— were on the slice's own branch, not the head this was written against; the
amendments are now landed here and this ADR cites the retained product's source
directly.)

## Decision

### (a) The provider's verdict carrier — placed with ADR-143 (the approval wire surface)

**Decision: the retained product has no equivalent; the native product decides
`com.hagency.engagement.approval.v1` carrying `{requestId, decision}`, accepted
only from the fleet's representative — because the native provisioning ingress is
Matrix-only and no provisioning-verdict wire kind exists.**

Placement. ADR-095 decides the admission *chain* ("owner message → intake admit →
provider approval → effect observed") and names exactly one wire kind,
`com.hagency.engagement.request.v1`, but never says how the provider verdict
arrives. ADR-143 owns the native approval **wire** surface but knows only the
owner-approval v1 profiles. A new wire *kind* is a wire-surface decision, so it
belongs to **ADR-143**; this ADR records the decision and ADR-143's amendment
carries the kind, its fields and its sender rule.

Evidence. The retained product's provider verdict arrives over **HTTP**, not
Matrix: `POST /api/engagements/:id/verdict` (`backend-v2.js:15160`) calls
`engagementStore.decide({engagementId, approve, …})`
(`lib/engagement-store.js:593`). No `com.hagency.engagement.approval.*` event kind
exists in the retained source — the only retained verdict event is
`com.hagency.approval.verdict.v1` (`bridge-matrix.js:219-220`), and that carries
**execution** approvals (owner tool-call verdicts for a running agent), not
engagement provisioning. The retained console is a REST surface; the native
provisioning-ingress spec expressly forbids adding one ("Do not add a console
create-agent HTTP route — the ingress is Matrix intake, not a REST endpoint"). So
the verdict must arrive on the same pre-project reception room the request did, as
a new versioned event kind mirroring `com.hagency.engagement.request.v1`
(`lib/fleet-protocol.js:4`).

The kind is **`com.hagency.engagement.approval.v1`**, fields
**`{requestId, decision}`**, and it is **accepted only from the fleet's
representative sender** — the same authority the request's `verify_request`
checks — so a verdict from any other sender is refused before `approve`.
`requestId` binds the verdict to the request's own idempotency key.

### (b) Inline synchronous provision — confirmed by existing decision (ADR-022)

**Decision: confirmed by existing decision, not decided anew. ADR-022's title is
"…provisions agents on approval", and the retained behaviour is inline synchronous
provisioning with no effect worker; adopt it.**

Evidence. The retained product provisions inline inside the verdict request:
`fulfillEngagement` (`backend-v2.js:14617`, body `fulfillEngagementOnce` at
`:14635`) provisions the agent home
(`provision-v1-agent-home.js`), mints the Matrix identity, binds the owner, admits
the agent to the project room, and launches it — all **inside** the verdict
handler, awaiting each step, with no background effect worker. ADR-022
(resource-first agent allocation) is built on exactly this retained inline
`createAgent`. The native store already models the effect as a claim/observe pair
rather than a queued job: `claim_effect` (`domain.rs:1338`) takes the pending
provision effect when the engagement is `reserved`, and `observe_effect`
(`domain.rs:1357`) completes it (`Applied` → `Active`). The intake handoff claims
the effect and observes it complete inline, in the same handoff — matching the
retained inline shape. No separate effect worker is introduced.

### (c) The session id derivation — placed with ADR-095 (native state ownership)

**Decision: the retained product has no equivalent; the native product decides the
intake derives the session id deterministically as `session_{engagement_id}` —
because native sessions are engagement-scoped, not room/thread-reused.**

Placement. The session id keys the engagement's later route resolution (G3), so
its derivation is a native state-ownership decision and belongs to **ADR-095**;
this ADR records it and ADR-095's amendment carries the key rule.

Evidence. The retained product has no engagement-derived session id at all: its
agent localpart is `{agentPrefix}{agentName}` (`backend-v2.js:14791`), derived
from the *agent name*, and its sessions are room/thread-scoped conversations, not
per-engagement. Native, by contrast, keys a session to its engagement: the
store's `SessionBinding` (`hagency-core/src/tasks.rs:54`) carries an
`engagement_id`, and the engagement's session row is registered under that
binding (`register_session`, `domain/execution.rs:639`). No prior decision scopes
native sessions to engagements — ADR-095's amendment (this commit) decides it
here. The intake mints the session for the
engagement it just made effective, and a deterministic `session_{engagement_id}`
keeps that derivation idempotent across restart and replay: re-observing the same
admission derives the same session id rather than minting a second row.

### (d) The MXID/device shape — placed with ADR-014 (agent Matrix identity provisioning)

**Decision: the retained product is the source; adopt the retained derivation,
keyed on the engagement id — `@…{engagement_id}` for the sender localpart and
`DEVICE_{engagement_id}` for the device — because the store's
`UNIQUE(server_name, sender_mxid)` forbids reusing the host's own sender.**

Placement. The MXID/device shape is the engagement's transport authority forever
after, so it is an agent-identity-provisioning decision and belongs to **ADR-014**;
this ADR records it and ADR-014's amendment carries the shape.

Evidence. The retained product derives a new agent's stable identity from a
deterministic, engagement-bound value: `fulfillEngagement` builds the agent name
as `mx_{sideId…}_{role}_{sha256(id)[:12]}` — a sha256 over the engagement id
(`backend-v2.js:14756`) — and derives the sender localpart as
`{agentPrefix}{agentName}` (`backend-v2.js:14791`). The native rule adopts the
retained "derive the identity deterministically from the engagement" behaviour
exactly in kind; it deviates only in the *key*, binding the localpart and device
directly to the engagement id rather than to an intermediate agent name, because
the native store has no separate agent-name surface and the
`UNIQUE(server_name, sender_mxid)` constraint requires a sender distinct from the
host's own. Deriving both from the engagement id gives each newly effective
engagement a distinct, stable, restart-safe sender and device that satisfy that
constraint without colliding with the host or any other engagement.

## Consequences (what the tests must observe)

- **(a) verdict kind.** Approving a minted engagement from `requestId` writes the
  `approve` verdict and moves the engagement out of `pending`; a verdict event
  from a sender other than the fleet's representative is refused **before**
  `approve` (no `decisions`/`approve` row); a verdict naming an unknown or
  already-decided `requestId` is refused (`NotFound`/`State`), never a second
  verdict write.
- **(b) inline effect.** After the verdict's handoff returns, the provision
  effect is observed `Applied` and the engagement is `Active` — with no pending
  `effects` row left for a worker; a failed observation leaves the effect
  recoverable (uncertain), not silently `Active`.
- **(c) session id.** A `matrix_session_routes` row exists whose session id is
  exactly `session_{engagement_id}`; re-admission/replay derives the same id and
  creates no second row.
- **(d) transport sender/device.** The new engagement's transport sender and
  device are derived from its engagement id and differ from the host's sender
  (satisfying `UNIQUE(server_name, sender_mxid)`); a second route reusing the
  host's sender is refused by the constraint.

## Alternatives Considered

- *Add a console/REST verdict route like the retained product.* Rejected: the
  provisioning-ingress spec expressly forbids a console create-agent HTTP route;
  the native ingress is Matrix intake, so the verdict must ride the wire.
- *Reuse `com.hagency.approval.verdict.v1` for the provisioning verdict.*
  Rejected: that kind carries execution approvals bound to a running agent's
  tool-call context; overloading it for engagement provisioning would conflate
  two authorities.
- *Introduce an effect worker to claim/complete the provision effect
  asynchronously.* Rejected: ADR-022 already decides inline provisioning, and the
  store's claim/observe pair already models the effect without a worker; one adds
  a failure surface (queue, retry, ordering) the slice does not need.
- *Derive the session id or sender from the room/thread or a random value.*
  Rejected: room/thread derivation is not engagement-stable (a native session is
  keyed to its engagement by `SessionBinding`, `hagency-core/src/tasks.rs:54`),
  and a random id is not restart-idempotent.

## Retained directory synchronization (2026-09-16)

Linux offline factory qualification reached the real copied project, then
refused before account registration: cap-std retains an O_PATH directory
descriptor, which cannot be fsynced. The same invalid synchronization existed
after first task-context publication. Both paths now share the managed account
store's existing directory-sync adapter: open dot relative to the retained
object, check private permissions and exact directory identity, then sync the
usable descriptor. Windows uses the existing WindowsDirectorySync adapter.
Account behavior is unchanged. Errors remain failures; no ambient reopen,
partial-home repair, deadline extension or synthesized readiness is allowed.
The Linux regression also observes the old descriptor's actual EBADF and
checks that replacing its old pathname cannot retarget the retained object.
This physical fix does not qualify a live deployment or the entire port.

## Account deadline fixture correction (2026-09-16)

A full Matrix regression run ended the750ms account-step fixture Unknown before
its script's expected third request. Its two300ms response delays leave only150ms
for real filesystem/TLS overhead; that run did not measure the exact failing
boundary. The unchanged test passed alone,
so the original full-run failure is retained rather than labeled a product pass.
That selector now explicitly controls Tokio time for its three300ms delays while
keeping real transport, encrypted filesystem, original ownership and inspect-only
reopen. A real3s watchdog bounds orchestration. The local peer skips timers only
for explicitly zero-delay writes; positive delays and held gates are unchanged.
The750ms operation and all production limits/code are unchanged. This verifies
the cumulative deadline and accepted-response custody, not physical latency or
live account creation. The full local219-test Matrix rerun passes.

## Nested copied-home creation and durability (2026-09-16)

The Linux whole-Matrix library run passed180 tests and failed seven original home/
AS scenarios. A direct new nested-copy regression reproduces the actual EBADF:
cap-std's O_PATH directory handle supports identity inspection, not fchmod. The
earlier flat factory source did not exercise this nested branch. Copied directories
now request mode0700 during capability-relative creation, then check the original
handle, instead of chmod after default-mode creation. Windows keeps its existing
default builder/inherited private-parent behavior and handle check. No error is
suppressed and no partial home is repaired or retried.

The copier also now synchronizes each actual recursive target directory, not only
the top-level target for every recursion. Original readable same-object sync,
file fsync, private checks and byte/depth/entry/deadline limits remain unchanged.
The direct fixture checks exact nested bytes and modes and, on Linux, observes
the old chmod's actual EBADF. This is physical file/home proof, not runtime,
sandbox or live fleet qualification; the failed whole run remains failed.

## Configured original-agent service composition (2026-09-16)

The closed continuous driver may explicitly opt into
`inline_factory_service_checkpoint_v1` alongside an existing home/rooms account
checkpoint. Without that additional composition field, existing markers retain
their current semantics. This is not either complete ADR016 deployment profile;
AS receiver/registration generation and all full-profile/live gates remain owed.
The composition must attach the original configured approval collector and shared
eight-live/two-parked runtime budget, consume actual successful original factory
agents exactly once, and drive recurring tasks through those owners rather than
ordinary replacement Hosts. Each agent retains independent workspace/file/receive
owners and its enrolled SDK. Unmanaged HOME comes from its materialized home.
Bounded historical attempt lookup may select a backend but grants no execution,
file capture, upload, reply or cleanup authority. Global shutdown freezes admission,
drains all admitted agents despite individual failure and retains exact unknown
custody before closing factory/coordinator SDKs and the original writer.

## Related decisions

- **ADR-143** (native approval wire oracle) — owns the wire surface; its amendment
  carries the new verdict kind (a).
- **ADR-095** (native state ownership) — owns the admission chain and the
  session-key derivation (c).
- **ADR-014** (agent Matrix identity provisioning) — owns the MXID/device shape
  (d).
- **ADR-022** (resource-first agent allocation) — already decides inline
  provisioning; (b) is confirmed by it.
- specs/task-r...[credential-redacted].spec.md — the ingress this ADR's second
  half completes.

## Amendment: a restart brings back the agents this factory completed (operator decision, 2026-09-22)

**Reverses**, for a provision this inline factory completed: the sentence above
"consume actual successful original factory agents exactly once" as the ONLY
way an agent enters the fleet; in `task-rust-configured-fleet-service` "Do not
reconstruct credentials, account, SDK, home, runtime or readiness" and "never
derive owners from Active rows"; and in `task-rust-token-account-provision` "A
valid retained successful response may be reopened only to inspect the same
credential with fresh whoami". Each keeps its meaning for provisioning; the
reversal is a second, read-only way in, after a restart.

**What was wrong.** Measured live on 2026-09-20: after any restart, clean or
not, the fleet listed one worker instead of three and reported healthy. The
two factory agents existed only as in-memory jobs of the process that made
them. Everything they need had survived: the token in encrypted account
custody, the SDK store (whose binding excludes the token and the generations
by design), the rooms custody, the home, and the domain rows (engagement
Active, effect Complete with this factory's receipt, transport, rooms,
session, workspace, approval binding). The retained product brings its agents
back at startup: stored credential first, whoami, never register again; an
agent that cannot come back is skipped and shown, never fatal.

**Decision.** At fleet start, before discovery, the service brings back each
engagement whose provision effect is Complete with this factory's receipt and
whose engagement is Active (`inline_factory_engagements`). For each one:

1. The store rebuilds the ORIGINAL claimed scope from the Complete row and
   proves it by recomputing the receipt digest the original completion stored
   (`reattach_provision_scope`). An account another path adopted, a revoked
   engagement or a changed payload yields no scope.
2. The home is reopened without a write; its recorded binding and manifest
   must agree with a binding recomputed now (`ManagedHomePlan::reopen`).
3. The account custody is read in re-attach mode: empty custody refuses before
   any request, the one register/login path refuses, and the stored token is
   confirmed by one GET whoami.
4. The rooms custody is replayed; create, invite and join refuse.
5. The enrolled SDK store is opened, never bootstrapped; an incomplete
   enrollment ledger is refused rather than enrolled (`Jobs::reattach_enrolled`).
6. The runtime is re-attached with no warm child and no retained task context:
   the agent's next task launches as an ordinary follow-up
   (`WarmHostPlan::reattach_runtime`).
7. The approval membership is re-admitted; workspace registration and session
   resolution are idempotent.

Nothing claims, completes, activates, registers, uploads keys or rotates a
generation. A provision this process already owns is left to ordinary
discovery, which still takes each new agent exactly once.

**One agent's failure is that agent's only.** It is registered in the fleet
as `not_attached`, with the Matrix cause where there is one; the fleet is not
failed, readiness is unchanged, and its queued work waits. A fenced generation
still refuses, as before.

**Unchanged.** A restarted host cannot claim a Started or Uncertain effect
again. Genuine negative Matrix evidence during a re-attach fences as it does
everywhere else.

**Known limits, both fail closed:** a home's binding covers the task-client
binary's length and modification time, so a home does not reopen after a
binary upgrade; a scope that requires a provider-managed account is not
re-attached yet.

Pinned by `native_configured_fleet_reattaches_after_restart` (an agent comes
back and runs its next task through a follow-up; a tampered home leaves it
`not_attached` with the fleet running and its work queued), and by the store
and transport pins named in `task-rust-factory-agent-reattach`.

## Amendment: the owner's join has no deadline (operator decision, 2026-09-22)

**Reverses** the sentence above "the retained job waits only for the owner's
actual joined state within its original finite budget", and in
`task-rust-inline-agent-rooms` "Timeout is unknown, never readiness", for the
owner's join specifically.

**What was wrong.** The wait for a human to click "join" was bounded by the
same budget as a single homeserver request, hard-capped at 60 seconds. When
the owner took longer, the provision was marked uncertain (never claimable
again, its rooms custody never resumable) and, because provisioning runs
inline in the coordinator's intake, the refusal ended the coordinator for
good. The retained product never waited for the owner at all; its DMs were
plaintext. Here the DM is encrypted and the agent's keys can only be shared
once the owner is in the room, so the wait is real, but it is a wait for a
person, not a protocol timeout.

**Decision** (three operator answers: no deadline; the agent is not active
until the owner joins; status only, no reminder).

1. When the DM exists, the owner is invited and the agent has joined, and the
   owner has not joined within the attempt's own budget, the attempt ends as
   `AwaitingOwner`. That is not a failure: no room is retired, the effect is
   not observed unknown, it stays Started, and the rooms custody (every POST
   accepted, no `complete` yet) resumes GET-only. Only the job that observed
   the wait may resume it: on disk, a wait and a completed custody whose
   record was lost look the same, and the latter stays unknown, as does a
   wait a restart interrupted.
2. The intake treats `AwaitingOwner` as success for the approval it just
   handled. On every later turn, before it reads the room, it gives each
   waiting provision one more look: a resumed attempt polls once, within two
   seconds, and hands the wait back. Once the owner has joined, that turn
   finishes the rooms, the enrollment and the factory exactly as the first
   attempt would have, creating, inviting and joining nothing again.
3. The fleet publishes each waiting provision as an `awaiting_owner` row with
   the wall-clock millisecond it began waiting. The row is replaced by the
   agent when it is admitted and dropped if the provision stops waiting
   without admission. Nothing reads it to decide anything; the fleet is not
   failed and readiness is unchanged.
4. No reminder is sent. The status is the signal.

**Unchanged.** A lost or torn POST is still unknown and never repeated. The
SDK budget (ADR-179) is untouched: each look is one bounded read. The agent
is not Active and has no session until the owner has joined.

**Known limit.** A restart during the wait leaves that provision Started,
which a restarted host cannot claim (task-rust-inline-provision-effect-claim);
it is the operator's, as any Started effect is today.

Pinned by the tail of `native_provisioning_inline_rooms_refusals` (the first
attempt runs out, the effect stays Started, a turn with the owner absent looks
once, the turn after the join finishes rooms and enrollment with no new POST)
and by `native_provisioning_waits_for_the_owner_without_a_deadline` (the
fixture's owner joins only after the first attempt's whole budget; the fleet
shows `awaiting_owner` with its start, coordinator turns look again, the agent
takes the row's place and runs its first task).
