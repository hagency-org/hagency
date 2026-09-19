---
kind: decision
id: ADR-014
title: "Provision agent Matrix identities by server capability, never by derived password"
status: Accepted
liveness: auto
tags: [matrix, identity, credentials, authorization, appservice]
---

## Implementation status 2026-08-11 — and a correction to how this record was written

**Accepted does not mean built, and this document could not tell you which was which.** The
Decision below is written in flat present-tense declarative from end to end, so a decision that has
merely been *taken* reads exactly like one that has *shipped*. The table restores the distinction.
It changes no decision's substance: everything below stands as accepted, including the parts nothing
has implemented yet.

| # | decision | status | evidence |
|---|---|---|---|
| A | amendment below — pending invitation, project supplies the server | **built** | `state.pendingInvites` and `pendingInviteKey()` in `bridge-matrix.js`; `createPendingInviteStore` plus its list/upsert/settle API in `backend-v2.js` |
| 1 | the ownership invariant does not change | **built** | true by construction — ADR-002's `(room, agent)` binding from the inviter is untouched, and acceptance now writes it |
| 2 | two provisioning flows, chosen by server capability | **decided, not built** | the repository still contains zero appservice support: no `as_token`, `hs_token`, `sender_localpart` or registration file — the only matches are prose in this ADR's own lineage |
| 3 | derived-password self-registration is to be deleted | **built**, with one named exception | `deriveAgentPassword`, `legacyAgentPassword`, `agentPasswordCandidates` and `tryMatrixLogin` are gone from `bridge-matrix.js`, as are all three env vars and their `.env.example` entries; `ensureAgentAccount` now validates a SUPPLIED token (`agentTokenFromEnv`) and refuses with `AgentCredentialMissingError` rather than minting one. Exception: `matrixRegister` stays — the BOT uses it with its own explicit `MATRIX_BOT_PASSWORD`, which this decision never covered. `tests/agent-credential-supplied.test.js` (20 cases, 10/10 mutants caught) |
| 4 | multiple homeservers; a per-agent credential record | **built** (2026-08-13) | The RECORD: `state.agentTokens[name]` is `{ homeserver, serverName, mxid, accessToken }`, migrated from bare strings on load (`tests/bridge-agent-credential-record.test.js`, 13 cases). The THREADING: all seven token-taking primitives take a REQUIRED `baseUrl` — `requireBaseUrl` throws naming the function rather than defaulting, because a default would let a caller holding an agent's credential send it to this deployment's server. Call sites that resolve an agent by name pass `credential.homeserver`; the two send paths that receive a token instead resolve it with `baseUrlForToken`. `tests/bridge-matrix-http-primitives.test.js`, 30 cases, 14 mutants killed across two rounds |
| 5 | an agent MXID is discovered via `/whoami`, not constructed | **partly built** (2026-08-13) | Auditing all 15 `agentUserId` call sites showed composition is not uniformly wrong: it is CORRECT where the value is a specification rather than an observation — telling an operator which account to create and which token to issue, the expected side of `ensureAgentAccount`'s identity check (where no discovered value exists yet, by definition), and constructing the name of an account to find and remove. The 6 sites that ASSERT an operating agent's identity now use `agentMxid()`, which reads the discovered value from the credential record and falls back to composition only for an agent with no record — which has no token and therefore cannot have acted. Still composed: `humanUserId` (7 sites), whose discovered source is a different one (ADR-002's `owner_mxid` on bindings) |
| 6 | a dead credential is a human-visible state | **built at the record, not the UI** | the retry loop is gone (`AgentCredentialMissingError`), the state is LIVE (`markAgentUnprovisioned`/`clearAgentUnprovisioned`, not a startup snapshot), and it reaches a durable cross-process surface: `unprovisionedAgents` in `data/health/matrix-bridge.json`, which the standalone doctor already reads. Still absent: a dashboard view, and the homeserver per agent (needs decision 4) |
| 7 | flow B requires a dedicated login, stated where an operator reads | **decided, not built** | the rule exists only as prose here; there is no startup check and nothing in `.env.example` says it |
| 8 | the ADR-008 crypto-store defect is a prerequisite | **built** | `lib/matrix-crypto-store-identity.js` now excludes the `bot-sdk.json` placeholder by name, so an empty `{}` store no longer reads as "data without a device identity" |

Each decision below repeats its own status line, because a reader who arrives at one decision
through a link never sees this table.

**Why this is a correction and not a house-style preference.** The sibling audits in this same
round found the identical defect in other Accepted records — ADR-003 stating a guarantee that had an
authorised exception recorded elsewhere, ADR-007 stating a scope that nothing in the code enforces.
This ADR committed the same error and in its strongest form: decision 3 asserted a **completed
deletion** of code that is still live and still on the default startup path, so the record was
evidence for the opposite of the truth. An Accepted decision record is read as a description of the
system by everyone who was not in the room, which makes present-tense declarative for unbuilt work
a correctness defect rather than a matter of tone.

## Superseded in part by ADR-016 (2026-08-13)

**The amendment below is withdrawn on two points; the rest of this record stands.** By operator
ruling, non-federating homeservers are no longer out of scope — they are the **assumed** case, and
federation is an optimization. So the amendment's three-mode table (federated invite / appservice /
project-issued tokens) is withdrawn, as is its closing paragraph deferring non-federating servers,
and the credential moves from `(project, agent)` to `(项目方, agent)` — a project side being one
homeserver plus the credential Hagency holds there. Decisions 4 and 5 below are the ones ADR-016
settles the shape of; decisions 1, 3, 6, 7 and 8 are unaffected.

ADR-016 also corrects this record's cost estimate for the deferral, with measurements: agents send
plaintext and need no crypto store, and `matrixRegister` already implements the registration-token
flow. Read ADR-016 before acting on the amendment below.

## Amendment 2026-08-11 — the project dictates access, and an invitation is the input

Two things in the Decision below are corrected, and one clause is added. The original text put
the credential on the **agent** (`{ homeserver, accessToken }` per agent) and had an operator
type a server address. Both are wrong.

**The credential belongs to the project, not the agent.** A homeserver is a property of the
project, so an agent serving three projects on three servers would need three credentials — the
real key is `(project, agent)` and the project is what supplies the server. Dispatch therefore
assigns the homeserver by itself; nothing needs to be chosen twice.

**The server is not typed, it is derived.** A Matrix room id is `!opaque:origin-server`
(`ROOM_ID_RE = /^![^:\s]+:[^\s]+$/`), so `projectRoomId` — already carried on every engagement and
every whitelist entry — names the project's server. What an operator cannot derive is the
server's *API* base URL, which may differ from the server name by `.well-known` delegation; that
is discovery, not data entry.

**And in the normal case there is no credential at all.** The operator's framing settles this: a
project dictates how you join it, the way an open-source project runs a Discord and developers
join it by accepting an invitation. What that analogy establishes is that **your account is yours
and portable** — an invitation authorises entry, it does not issue you an identity. Matrix's
equivalent is exactly federation plus a room invite: the agent keeps its one identity on the
contributor's homeserver, and joining a project is accepting an invite to a room hosted
elsewhere. So:

| mode | when | what is configured |
|---|---|---|
| **federated invite** (normal) | the servers federate and the project does not require local accounts | nothing — only the decision to accept |
| appservice | the project's server will install our registration | `{ apiUrl, asToken, namespace: '@ac_.*' }` on the project |
| project-issued tokens | the project requires local accounts | one token per `(project, agent)` on the project |

Per-`(project, agent)` rather than one token per project, in the last two modes, because
ADR-013's transparency amendment requires the borrower to know **which** agent and model serves
them, and ADR-002 binds ownership per `(room, agent)`. One shared account for all lent agents
collapses both.

**Where the analogy breaks, and it is the point where this product's economics differ:** joining
a Discord costs the joiner nothing. Lending an agent spends tokens. So acceptance stays a
deliberate, audited act by the contributor — never a link click, and never automatic. The shape
is still invitation-driven; only the click becomes an approval.

**What this replaces is broken in both directions today, which is why it is a fix and not a
feature.** `MATRIX_TRUST_MODE` defaults to `audit`, and the invite handler reads
`if (!trust.trusted && MATRIX_TRUST_MODE === 'enforce') continue;`:

- On the **default**, an agent joins any room anyone invites it to, but `markRoomTrusted` and
  `upsertRoomAgentBinding` are skipped — so the agent is present, the project can send
  `!request`, and every approval then fails `owner_binding_missing`. Joined and unengageable: a
  dead end that looks like presence.
- Under **`enforce`**, the invite is silently skipped. `untrusted_inviter` occurs exactly once in
  the repository — the line that produces it. There is no record, no API and no pending state, so
  the contributor never learns they were invited.
- And `MATRIX_TRUSTED_INVITER_MXIDS` is an env var read at module load, absent from the backend
  and the console. So "join a project" today means: learn an MXID out of band, edit `.env`,
  restart the bridge, and hope the invite is re-polled. The invitation carries no weight; the
  authorisation happens in a text editor.

**The decision:** an invite from an inviter that is not already trusted is recorded as a
**pending invitation** — room, derived server, inviter, and which agent was invited — and the
agent does **not** join until the contributor accepts. Accepting joins the room, marks it
trusted, and writes the `(room, agent)` ownership binding from the inviter, which is ADR-002's
mechanism unchanged: **accepting an invitation is how ownership is established.**

Accepting does **not** whitelist the project. The whitelist decides who may skip approval
(ADR-013 decision 4), which is a stronger and separate statement than "this agent may be in this
project". An earlier draft of this amendment had acceptance produce the whitelist entry too;
that was wrong.

The project surface is therefore read-mostly — the projects we have been invited into, their
server, their inviter, which agents are in them, and their trust state — with credential fields
appearing only in the two exceptional modes. `MATRIX_TRUSTED_INVITER_MXIDS` degrades from a
prerequisite to an optional bootstrap accelerator.

**A correction to decision 4 below.** "The bot keeps one homeserver … rooms hosted elsewhere are
reachable through federation" is true **only where the servers federate**. Where they do not, the
bot cannot see the project's room at all, so project-issued-token mode without federation needs a
project-level **bot** credential as well — and with it a second crypto store, a second device and
a second E2EE state machine, doubling the ADR-008 surface. Non-federating homeservers are
therefore **out of scope** until a real project requires one, and that limitation is stated to
the operator rather than discovered.

## Context

An agent needs a Matrix identity to participate in a project room. **When this ADR was written**,
`bridge-matrix.js` created one itself: the username was composed as `@ac_<agent>:<our server>`, and
the password was **derived** — `sha256(MATRIX_AGENT_PASSWORD_SECRET + ':' + agentName)`. On startup
the bridge tried the cached access token, fell back to logging in with the derived password, and
fell back again to *registering* the account, which needed `MATRIX_REG_TOKEN` or open registration
on the homeserver.

*(Past tense as of 2026-08-11: decision 3 is built and that mechanism is gone. The description is
kept because the three properties below are the entire argument for this ADR, and they are only
legible against the model they condemn.)*

Three properties of that model, each verified against the code rather than assumed:

- **The master secret cannot be rotated.** Every agent password is derived from it, so changing
  it invalidates every existing account's password at once. The bridge would then fail to log in
  and try to register usernames that are already taken. `MATRIX_ALLOW_LEGACY_AGENT_PASSWORD`
  exists as a migration path from an older *template* scheme, so this class of problem has been
  hit before; rotating the secret itself has no path at all.
- **The credentials it produces are not revocable.** Revoking every access token achieves
  nothing, because the password can be re-derived and used to log in again. Compromise of
  `.env` is therefore permanent control of every agent identity, recoverable only by renaming
  the agents.
- **It requires account-creation privilege on the homeserver.** That is a far larger grant than
  the ability to act as a handful of existing accounts, and no third-party project will give it
  to an external bridge. So the model **structurally cannot** onboard an agent onto a homeserver
  we do not administer — which is exactly what the contribution-console persona (ADR-013) will
  meet.

`Robrix2`'s own onboarding documentation already distinguishes the two paths that exist in
practice:

> Octos(AppService):注册在服务器上的应用服务 — 由服务器托管、可以管理自己名下的一批账号
> Octos(Direct)／Hermes／OpenClaw:以「Matrix 好友」形式直接添加 — 就是一个普通 Matrix 账号背后的机器人

Hagency implements **neither**. It implements a third thing that needs server privilege like an
AppService while producing per-account credentials like a Direct agent, and makes them
non-revocable. It is dominated by both. The repository contains **zero** AppService support: no
`as_token`, no `hs_token`, no `sender_localpart`, no registration file (the only occurrences of
"appservice" are for *ignoring* other people's appservice bots).

## Decision

**1. The ownership model is the invariant, and it does not change.** The owner of an agent in a
room is the human whose authenticated full MXID invited that exact agent account into that
trusted room (ADR-002). Robrix2 requires the same thing and for the same reason:

> **你本人逐个邀请自己的 @ac_\<agent\> 木偶账号**
> 让 bridge 代替人类创建/邀请项目成员,不能证明"谁邀请 Agent,谁是 owner"

Credential provisioning and ownership are therefore **independent problems**. Changing how an
agent obtains its identity does not touch how its owner is established, which is what makes this
decision cheaper than it looks.

*Status — **built**, in the sense that nothing had to change for it to hold. The pending-invite work
in the amendment above preserved it rather than replacing it: acceptance is what writes the
`(room, agent)` binding, and the MXID it binds is still the inviter's.*

**2. Two provisioning flows, chosen by what the target homeserver supports.**

| | flow A — AppService | flow B — Direct token |
|---|---|---|
| when | the homeserver will install a registration (typically our own) | a third-party homeserver that will not |
| who creates the account | the homeserver, from a claimed namespace | a human, once per agent |
| what Hagency holds | one `as_token` for the whole namespace | one access token per agent |
| server admin needed | yes, to install the registration | no — an ordinary user account |
| revocable | yes, by removing the registration | yes, per agent |

Flow A claims the namespace `@ac_.*`. The existing `MATRIX_AGENT_PREFIX` default (`ac_`) is
already namespace-shaped, so this is a formalisation of the current naming rather than a change
to it.

*Status — **decided, not built**. Neither flow exists. Flow A has no support of any kind: no
`as_token`, `hs_token`, `sender_localpart` or registration file anywhere in the repository. Flow B
has an INTAKE as of 2026-08-11 — `MATRIX_AGENT_TOKEN_<AGENT>` accepts a human-supplied token and
`ensureAgentAccount` validates and adopts it (decision 3) — but an intake is not the flow: flow B
also requires the per-agent homeserver of decision 4, the discovered MXID of decision 5 and the
dedicated-login rule of decision 7, none of which exist. The table in the amendment above adds a
third mode, **federated invite**, which is the one that is built; it needs no credential, so it does
not satisfy this decision, it postpones it.*

**3. Derived-password self-registration is to be deleted, without a compatibility flag.** Not
deprecated behind a switch: a switch would be used, and it is the only one of the three that
produces a credential nobody can revoke. `MATRIX_AGENT_PASSWORD_SECRET` and
`MATRIX_ALLOW_LEGACY_AGENT_PASSWORD` go with it.

*Status — **built** (2026-08-11), and one prediction in the original wording was wrong.*

*Deleted from `bridge-matrix.js`: `deriveAgentPassword`, `legacyAgentPassword`,
`agentPasswordCandidates`, the three env vars, their startup warnings, their `.env.example`
entries, and the README rows in both languages. `tryMatrixLogin` went too — a multi-candidate
password login has no remaining caller once there are no candidates to iterate.*

*Corrected: this decision also claimed "the `matrixRegister` path" would go, and it has NOT. That
path is what the BOT uses, at `ensureBotAccount`, with an explicit operator-set
`MATRIX_BOT_PASSWORD` inside a login→register→backoff loop. The bot is a single account whose
password an operator chose, so none of the three condemned properties applies to it: it is
rotatable (change the variable, change the password), revocable, and its registration is a
one-time act on the homeserver the operator runs. The decision named a symbol when it meant a
mechanism, and deleting `matrixRegister` would have broken bot startup for no security gain.*

*What replaced it: `ensureAgentAccount` takes the stored token, then `MATRIX_AGENT_TOKEN_<AGENT>`,
validates the candidate against `/whoami`, and ADOPTS it into `state.agentTokens` only after the
homeserver accepts — so a typo in `.env` never displaces a working credential. With neither
available it throws `AgentCredentialMissingError` (`needsProvisioning: true`), naming both the
exact variable to set and the exact MXID to create. It refuses rather than returning null so no
caller can send with `Bearer undefined`, which fails as an empty result rather than as an error.*

*The distinction this hinges on: a REJECTED credential (401/403) versus an UNREACHABLE homeserver.
Only the first is a verdict on the token. `isMatrixAuthFailure` therefore answers false for
everything it does not recognise, because misreading an outage as mass revocation would send an
operator to reissue tokens by hand for a fleet whose credentials were fine.*

*Not built here: persisting the discovered MXID. `/whoami` now returns it on every adoption, which
makes decision 5 nearly free — but decision 5 is still open, and settling it as a side effect of a
credential change would decide it without deciding it.*

*Adversarial review of the first version (2026-08-11) found six real defects, all now fixed with
tests and 22 mutants killed. Two were introduced by this change: the credential was validated but
its IDENTITY was not, so a token pasted under the wrong agent's variable was adopted and used to
send as its real owner (and to rename that owner's profile); and `ensureAgentAccount` began throwing
on SSE paths that awaited it bare inside a synchronous try, turning an EXPECTED standing condition
into an unhandled rejection — fatal on modern Node. Three were pre-existing paths whose severity
this change escalated from recoverable to permanent, because they lose credentials that can no
longer be re-minted: `loadState` treated every read failure as "start empty" and startup then
persisted that over the file; both pruning loops delete every token absent from a roster, and a 200
carrying a non-array normalizes to an empty roster; and the variable-name mangling is not injective,
so `octos-agent` and `octos_agent` read the same variable. The sixth: the stale-account cleanup
could make a LIVE agent self-leave a room, dormant under the derived password (which needed an unset
secret) and re-armed by stored tokens, which every live agent has.*

*The pattern worth keeping: removing a self-healing fallback does not just delete a feature, it
reclassifies every path that relied on it. Paths that were merely inefficient became destructive,
and paths where a throw was almost impossible became paths where a throw is routine. Neither class
announces itself in the diff.*

*Migration: nothing breaks on deploy. Tokens already in `bridge-state.json` keep working, since the
`/whoami` check is unchanged. What stops happening silently is REPLACEMENT — a missing or dead
credential used to be re-minted from the master secret, and now startup logs
`[agent-credential] NEEDS PROVISIONING` per agent plus one summary line listing every inert agent.
That is the point rather than a regression: re-minting is precisely what made the credential
unrevocable.*

**4. Hagency may connect agents to MULTIPLE homeservers; the bot stays on one.** The operator's
ruling is that agents must be registerable against different Matrix servers, not a single one.
The split that makes this affordable:

- **The bot keeps one homeserver** — the contributor's own. It is the E2EE sender, the
  authorization service, and the holder of the single crypto store. Rooms hosted elsewhere are
  reachable through **federation**, so a bot on one server does not need an account on another.
- **Each agent carries its own credential record** — `{ homeserver, accessToken }` — and every
  agent-side API call uses that record's base URL rather than the global one.

Measured, so the scope is a number rather than a feeling — and re-measured, because the first figure
recorded here was already stale. As of 2026-08-11 (`a52c3ea` and `a43896a` both) `bridge-matrix.js`
contains **44** `HOMESERVER` references, not 42, and the original 19/23 split does not survive contact
with them. Classified by which access token authenticates the call:

- **18 are agent-side** — agent `whoami`, display name, the avatar retry, the agent invite poll and
  room scan, agent joins and leaves, DM/SPY room creation, `sendAsAgentContent` — and must become
  per-credential.
- **16 are bot-side or global** — the constant itself, `MATRIX_SERVER_NAME`, the bot client, the
  media URL builders, the bot's own sync, invite, kick and `createRoom` — and stay as they are.
- **10 take whichever token their caller holds**: `matrixLogin`, `matrixRegister` (two sites),
  `getUserId`, `getMatrixAccessTokenSession`, `uploadMedia`, `setUserAvatar`, `setRoomAvatar`'s
  `token || state.botToken`, and the two bot-then-agent retry ladders in `_ensureHumanInviteOrFail`
  and `_upgradeLegacyDmRoom`. These cannot be assigned to a side at all: they need the base URL
  passed in with the token, which is a slightly larger change than "make the agent ones
  per-credential".

That third bucket is the substantive part of the correction; the drift from 42 to 44 is only the
reminder that a reference count in a decision record is a snapshot. Treat the total as scope, not as
an invariant — it moved by one inside an uncommitted working tree while this paragraph was being
written.

*Status — **partly built** (2026-08-13), and the half that is built is the one this decision leads
with.*

*`state.agentTokens[name]` is now `{ homeserver, serverName, mxid, accessToken }`. Bare strings are
migrated on load — pointed at `MATRIX_HOMESERVER`, which is not a guess, since under the model this
replaces there was nowhere else a stored token could have come from. `serverName` is carried BESIDE
`homeserver` rather than derived from it: they answer different questions, `.well-known` delegation is
allowed to make them disagree, and the one consumer that needs the name compares it against a ROOM's
origin server. `mxid` is recorded where the homeserver has just reported it and is left null
otherwise, which does not settle decision 5 — `agentUserId()` still composes.*

*The threading is done too. All seven token-taking primitives — `matrixLogin`, `matrixRegister`,
`getUserId`, `getMatrixAccessTokenSession`, `uploadMedia`, `setUserAvatar`, `setRoomAvatar`, plus
`setDisplayName` which shares their shape — take a `baseUrl`, and it is REQUIRED. `requireBaseUrl`
throws naming the function instead of defaulting to `HOMESERVER`, because a default is precisely the
failure this decision exists to prevent: a caller holding an agent's credential silently sending it to
this deployment's own server. JavaScript cannot enforce a required parameter, so the throw is the
enforcement.*

*Where the base URL comes from, per caller, because that is the substance:*

- *call sites that resolve an agent BY NAME now use `getAgentCredential()` / `agentCredential()` and
  pass `credential.homeserver` — room membership add and remove, group room creation, and both avatar
  paths;*
- *the two send paths that receive a TOKEN rather than a name — `sendAsAgentContent` and
  `sendAttachmentAsAgent`, the choke points for every outbound agent message — resolve it with
  `baseUrlForToken`, which finds the token's own record and falls back to `HOMESERVER` for the bot's,
  whose server this decision's split says stays ours;*
- *the bot's own login, registration, session check and avatar pass `HOMESERVER` explicitly.*

***THE REFERENCE COUNT WENT UP, from 44 to 55, and that is the intended outcome rather than a
regression.*** *The constant is no longer read INSIDE the primitives, where it was invisible and the
same for every call; it is now written at the call sites, where it is a visible per-call decision. The
honest measure of what remains is not the total but the twelve places that still choose it — nine
explicit `, HOMESERVER)` arguments and three `|| HOMESERVER` fallbacks — each of which is a
grep-able statement that "this call is the home server's" and can be revisited when a project side
needs it. `lib/bot-commands.js` still holds 7 references and its own `MATRIX_SERVER_NAME` derivation,
and was never classified by this decision.*

*One defect was closed by the record rather than by the threading, and it is the reason this was worth
doing before the mechanical part. `setRoomAvatar`'s retry ladder iterated every value in
`state.agentTokens` and sent each to `HOMESERVER`. Under one homeserver that is a sane fallback — the
bot may lack the power level to set a room avatar while an agent in the room has it. Under ADR-016 that
map spans homeservers, so the loop offered every project side's access token to whichever server the
call was aimed at: a credential disclosure across project sides, caused by a cosmetic feature, arriving
the moment a second project side exists rather than when anyone edited the function. The ladder now
skips any credential whose `serverName` is not the room's origin, and fails closed when the room id has
no server part.*

*The amendment above moves the credential's owner from the agent to the `(project, agent)` pair, and
ADR-016 decision 1 moves it again to the project side. The record built here is the bridge-local shape
that makes either expressible; converging the two stores is not done.*

**5. An agent's MXID becomes DISCOVERED, not constructed.** Today it is composed as
`@${AGENT_PREFIX}${name}:${MATRIX_SERVER_NAME}`. With a credential from a foreign server that
composition is simply wrong, so the identity must come from `/whoami` on the agent's own
homeserver and be stored beside the credential.

This is load-bearing rather than tidy. The owner-derivation expression in the agent invite poll —
`inviteState.find(e => e.type === 'm.room.member' && e.state_key === '@' + AGENT_PREFIX + agentName +
':' + MATRIX_SERVER_NAME)?.sender` in `bridge-matrix.js`, cited by its text rather than by a line
number per ADR-007's symbol-first convention, and because the original citation here,
`bridge-matrix.js:3932`, had already drifted past two hundred lines — derives the owner by searching
the invite state for a **constructed** MXID. For an agent on any other homeserver that filter never
matches, so no binding is written, and every approval for that agent is later denied with
`owner_binding_missing` — a silent authorization failure rather than an error. That line is also the
one the ADR-002 audit found untested: no fixture ever makes *this* `state_key` filter reject
anything. (`tests/join-backfill.test.js` does have a rejecting fixture for a `state_key` filter, but
it is the backfill selector's bot-membership check, a different filter in a different function; it is
easy to mistake for coverage of this one.) The untested filter and the multi-homeserver breakage are
the same defect seen from two directions.

*Status — **decided, not built**. `agentUserId()` still composes `@${AGENT_PREFIX}${name}:${MATRIX_SERVER_NAME}`
through `makeUserId()`, the filter above still matches that construction, and no agent MXID is stored
anywhere beside a credential because there is no credential record to store it beside (decision 4).*

**6. A dead credential is a human-visible state, not a retry loop.** Flow A's `as_token` does not
expire. Flow B's access token can be revoked, expired by server policy, or invalidated by
deleting its device — and unlike a derived password, **nothing can re-mint it**. So a failed
`/whoami` on an agent credential must surface as an explicit "this agent's credential is invalid
and a human must re-issue it" state on the operator's surfaces, carrying which agent and which
homeserver. It must not be absorbed into a background retry: the symptom would otherwise be an
agent that has silently stopped speaking.

*Status — **partly built** (2026-08-11). The half this decision leads with is done: the
self-healing retry it forbids no longer exists, because the thing it retried into — a
derived-password re-login — was deleted with decision 3. `ensureAgentAccount` now raises
`AgentCredentialMissingError` carrying `needsProvisioning: true`, `agentName`, the variable to set
and the MXID to create; startup logs one `[agent-credential] NEEDS PROVISIONING` line per agent and
a summary naming every agent that cannot speak.*

*Then completed further after adversarial review, which made the fair objection that a log line is
not a surface: it scrolls, and an operator who missed startup cannot ask which agents are inert. The
list is now written to `data/health/matrix-bridge.json` as `unprovisionedAgents` — a durable
cross-process record the standalone doctor already reads — and it is maintained LIVE rather than
snapshotted at startup, because a snapshot goes stale in both directions (an agent provisioned later
stays listed; one revoked at runtime never appears) and a confidently wrong record is worse than
none. The field is named `unprovisionedAgents` and not `agentsMissingCredential` because the health
writer's redaction guard refuses any key matching /credential/; a test pins that so a future rename
fails loudly instead of silently disabling the record.*

*Still NOT built: a dashboard view, and WHICH HOMESERVER — that needs decision 4.*

**7. Flow B requires a DEDICATED login, and this must be stated where an operator will read it.**
An access token identifies a *device*. If an operator pastes a token from their own Element
session to save time, the bridge and a human client share one device and advance the same
Olm/Megolm state from two stores — which can break decryption for both, durably. ADR-008 already
binds the crypto store to the token's device, so the machinery to detect a change exists; what
does not exist is the rule.

*Status — **decided, not built**. The rule is still stated only here, which is the one place the
decision itself says is not enough. `.env.example` documents the Matrix variables without it, and
there is no startup check that would refuse or warn about a shared device. Nothing about this waits
on flow B: it is a paragraph in operator-facing documentation plus, optionally, one check.*

**8. Sequencing: the ADR-008 crypto-store defect is a prerequisite, not a parallel task.**
`lib/matrix-crypto-store-identity.js` treats a store containing only the placeholder
`bot-sdk.json` (`{}`, which matrix-bot-sdk writes the instant the storage provider is
constructed, before any device id exists) as "contains data but has no device identity" and
refuses to start — permanently, and with no documented recovery path. Reproduced across five
store shapes. Today that window is widest on first deployment; under flow B, re-issuing a token
means a new device, which means the archive path runs routinely, so the defect moves from rare to
ordinary. It must be fixed before flow B ships.

*Status — **built**. `lib/matrix-crypto-store-identity.js` now excludes `bot-sdk.json` by name when
deciding whether a store "contains data", so a directory holding only that `{}` placeholder is
treated as empty and start-up proceeds. The prerequisite is therefore discharged; it is the only
part of this ADR whose implementation unblocks the rest.*

## Consequences

These are the consequences the decisions above would have, not outcomes anyone has observed. Per the
status table, decisions 1, 3 and 8 and the amendment are built and decision 6 is partly built, so of
what follows, only the parts that turn on a supplied credential have actually happened.

Good, because the credential a compromise yields becomes **revocable**, which the derived
password never was; because onboarding onto a homeserver we do not administer becomes possible at
all; because flow A removes per-agent credentials entirely for the common case; and because
`ensureAgentAccount`'s login/register/derive machinery disappears rather than being maintained.

Good, because AppService is unusually cheap here. The hard part of appservice bridges is E2EE for
puppet accounts, and Hagency's agents send **plaintext** — `sendAsAgentContent` is a raw
`PUT /rooms/{id}/send/m.room.message/{txn}` with no crypto path, and the only crypto store is the
bot's. Flow A therefore does not inherit the problem that makes mautrix-style bridges complex.

Bad, because flow B loses self-healing. A derived password could always re-login; a pasted token
cannot be renewed, so credential expiry becomes an operator task. Decision 6 exists to make that
visible rather than mysterious, and it is a real ongoing cost, not a one-off.

Bad, because flow B is manual per agent. For the fleet size this product targets — a contributor
lending a handful of agents — that is acceptable; it would not be for fleets of fifty, and flow A
is the answer there.

Bad, because agents on different homeservers can only share a room where those servers federate.
Where they do not, an agent from one simply cannot serve a project on the other, and no amount of
credential work changes that. This is a property of Matrix, and it should be stated to an operator
rather than discovered.

## Alternatives Considered

- **Keep derived-password self-registration and add BYO alongside it:** rejected. Keeping it
  means keeping the one path that requires account-creation privilege *and* yields
  non-revocable credentials. A flag left in place is a flag someone uses.
- **AppService only:** rejected because it still needs a server admin to install a registration,
  so it does not solve the third-party homeserver case — the case that motivated this decision.
- **Rotate `MATRIX_AGENT_PASSWORD_SECRET` on a schedule instead:** rejected because there is no
  rotation path. Every derived password changes at once and the usernames are already taken.
- **Give every agent an account on the bot's homeserver and rely on federation only:** rejected
  because it re-introduces the account-creation privilege for the one case where we may not have
  it, and because a project may require the agent to be a local account on its own server.
- **Per-agent homeserver for the bot as well:** rejected as unnecessary. Federation already lets
  one bot serve rooms hosted elsewhere, and a second bot identity would mean a second crypto
  store, a second device, and a second E2EE state machine — the ADR-008 surface doubled for no
  gain.

## Amendment 2026-09-14 — the engagement-derived MXID and device (ADR-147)

The provisioning intake derives the newly effective engagement's Matrix identity
from the engagement id: the sender localpart **`@{engagement_id}`** and the device
**`DEVICE_{engagement_id}`**. This adopts the retained product's behaviour of
deriving a new agent's identity deterministically from the engagement — the
retained agent name is `mx_{sideId…}_{role}_{sha256(id)[:12]}` and its localpart
`{agentPrefix}{agentName}` (`backend-v2.js:14756`, `:14791`) — but deviates in the
key: the native rule binds the localpart and device directly to the engagement id
rather than to an intermediate agent name, because the native store has no
separate agent-name surface and its `UNIQUE(server_name, sender_mxid)` constraint
forbids reusing the host's own sender. Deriving both from the engagement id gives
each newly effective engagement a distinct, stable, restart-safe sender and device
that satisfy that constraint. The rationale and evidence are recorded in ADR-147
(d).
