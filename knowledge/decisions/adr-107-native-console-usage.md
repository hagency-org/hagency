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
