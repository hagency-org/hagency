---
kind: decision
id: ADR-107
title: Retained usage console with bounded native browser authority
status: Accepted
---

## Context

M7 retains the existing browser components while moving deployed server behavior
to Salvo. ADR067 already supplies typed engagement usage observations; the legacy
fleet usage DTO has different semantics and cannot be synthesized from them.
The operator API deliberately refuses browser authority. This first read-only
slice does not claim complete console, Agent routing, operations or M7 parity.

## Decision

Opt-in native console assets serve the retained layout, usage page, preferences
and English/Chinese strings at `/console/usage/`. Only that document and manifest
listed `_next/static` JS, CSS and fonts are served. A private, host-selected asset
directory is opened without following links. Startup retains actual bounded
nofollow file snapshots and verifies all manifest hashes, lengths, count and
total before admission. Limits are 128 KiB manifest, 512 assets, 4 MiB per asset
and 32 MiB total content. Invalid or oversized builds are refused, never trimmed
silently. HTTP paths never reopen files. Static bytes contain no credentials.

An existing operator-authenticated, browser-closed POST
`/api/native/v1/console/access` issues one random 256-bit ticket valid for 120
seconds. Issuance is limited to one per second and one outstanding ticket;
replacement invalidates the preceding ticket. A native `console-access` command
reads the private operator token locally and prints only the limited access URL.
No automatic browser launch, token logging, redirects or retries occur. The
ticket is in the fragment and is removed from browser history before exchange.

Same-origin POST `/console/session` consumes the ticket once and returns a random
session cookie. At most four sessions exist, each with an absolute 15 minute
lifetime. Hashes are compared in constant time; secrets are not serialized into
assets or persisted in script-accessible browser storage. The host-only cookie is HttpOnly,
SameSite=Strict and scoped to `/console`. This profile is local loopback HTTP,
not remote deployment or TLS qualification. DELETE on the same path revokes the
current session. Session state is retired before native shutdown and rechecked
after awaited reads. There is no rolling expiry, browser-issued ticket or write
authority beyond exchange/logout.

Document navigation accepts the exact loopback Host even when navigation starts
outside the origin. API authority separately requires exact same-origin fetch
metadata, exact Origin for mutations (absent or exact for GET), no forwarding
headers and one bounded valid cookie. Exchange bodies are closed JSON, at most
256 bytes and two seconds. Eight concurrent console requests bound admission.
All underlying native routes preserve their existing browser-header refusal.

The facade only reads bounded engagement labels and existing UsageReport values
from the original DomainStore. The list query is closed `after`/`limit`, at most
16 rows plus a continuation cursor. Labels expose id, agentName, projectName,
role, state and cleanup, never rooms, runtime identity or credentials. The usage
query retains ADR067's timestamp and failure semantics. Missing, incomplete,
regressed and lower-bound observations remain explicit. No requested-token to
allocated-token conversion, fleet sum, billing claim or quota inference occurs.

The retained DataProvider has a separate native mode with unknown initial state,
no fixture fallback and no legacy proxy fetches. Runtime engagement selection
uses `/console/usage/?engagement_id=...`, independent of build-time Agent names.
The rail retains preferences and identifies unsupported native navigation. Other
retained pages and dynamically provisioned Agent detail URLs remain later slices.

## Validation

Actual issuer/exchange/replay/capacity/expiry/logout/post-await retirement and
existing raw API refusals require regression tests. File mutations, links,
manifest bounds and unknown paths are exercised against real private files.
Chromium must render actual fresh-writer usage in both languages, preserve
preferences, select an engagement absent from the static build, and show unknown
and refusal states. A real native executable must serve built assets with Node
absent from PATH. Node is permitted for build and browser-test tooling only.
No live service, production state or external account is used.

## Amendment: the console engagements consumer (read-only)

The engagements list read `GET /console/api/engagements` already existed for
the usage page's selector (this ADR's original scope); the engagements PAGE
is the new consumer. It is READ-ONLY triage: state strip, state/agent
filters, the tokens column, pagination via `next_after` — no create, verdict,
revoke or whitelist control, because those mutate enforcement and need their
own reviewed decision; buttons that would 404 lie (the same rule as the
alerts page). The retained page's route reasons (notWhitelisted / overOffer
/ overCeiling) have no native counterpart — the whitelist is the retained
fleet model's admission surface; the native analogue of "awaiting my
decision" is the engagement STATE column.

**The wire grew one key in the same commit as its client** (the binding
exact-key lesson): `requestedTokens` — the raw requested allocation as a
number, never compacted — on both the server `Label` and
`validateEngagements`' seven-key exact list. A stale validator on either
side refuses the whole read rather than rendering half a page.

**The document rule differs from usage by design:** `/console/engagements/`
takes NO query. Usage is a per-entity view (its `engagement_id` selection
survives refresh); engagements is a paginated list whose selection happens
in-page, so any query string is an invalid console request. The loader
allowlist (`assets.rs` mime + the `engagements/index.html →
/console/engagements/` key mapping), the document route rule (`console.rs`)
and the build staging (`build-native-console.mjs`) land in the same commit
as the page — the loader lesson from the alerts slice, applied.

## Amendment: read-only inspection subcommands (brief 22)

Three `hagency` subcommands — `alerts`, `engagements`, `resources` — are thin clients of the
operator routes this migration already publishes (`GET /api/native/v1/{alerts,engagements,resources}`),
built exactly like the `console-access` command above this paragraph: loopback-only `--listen`, the
private operator token read from `--state-dir`, one bounded hyper exchange (5 s deadline, 512 KiB
reply cap), `--limit` forwarded (the routes refuse out-of-range values, never clamp) and `--json`
printing the route's body verbatim. The table mode's columns are the routes' own wire keys — never
a derived figure the route does not publish (alerts: `dedupe_key resource_id occurrences resolved
summary` plus the envelope's `at_ms` clock; engagements: `id agentName projectId role
requestedTokens state`; resources: `id framework model tier ceiling`). Read-only by construction:
no subcommand creates, verdicts, revokes or resolves anything.

Refusals exit distinctly, never a silent 0: **3** unreachable (connect/handshake/timeout), **4**
refused (401/403, or an unreadable local operator credential), **5** invalid request (bad flags or
a route 400), **6** busy/unavailable (route 503 or any other server state) — each named on stderr.
The client lives in `src/inspect.rs` as a lib module beside `console::client` (there is no cli
module; every `main.rs` arm calls a lib module), and `main.rs` only wires the three arms.
