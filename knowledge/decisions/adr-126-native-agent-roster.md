---
kind: decision
id: ADR-126
title: "Bounded native agent roster observation"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
liveness: auto
tags: [console, agents, read-only, privacy]
---

## Context

The native console serves five pages and nine API route templates, and the
retained console's agent roster (`/workforce`,
`mockup/app/workforce/page.jsx` — the roster the page's own header names:
"nothing had it for all of them side by side, which is exactly what a roster
is", `:24-26`) has no native counterpart: grep for `agents` under
`native/hagency/src/console/` returns nothing. The retained query is
`GET /api/agents` (`backend-v2.js:11695`) → `serializeAgent`
(`:6822-6913`), whose response spreads the whole agent record and therefore
carries `homeDir` (`:6890`), `workdir`/`stateDir` (`:6891-6892`),
`workspacePath`/`lastWorkspacePath` (`:6902-6905`) and `tmux` — private data
a console token can read today, because the proxy allowlist admits
`/^agents$/` (`route.js:36`). Native has no agents table: its model is
registration → resource → engagement → session → dispatch, so a roster row
is a derivation from the engagements read (`domain.rs:607`) and the resource
reads, not a port. The retained roster's own refusal set — no work item, no
assignment state, no progress, no queue, no lease, no task count, no
utilisation (`workforce/page.jsx:59-79`) — is adopted wholesale.

## Decision

**Serve `GET /console/api/agents`** — a new `console/agents.rs` beside the
existing reads, mounted under the API sub-router's `authenticate` hoop with
**no scope**. Scope facts are a payload, not a gate: the existing pure reads
never gate on scopes (only mutations refuse without one — publication's
`can_publish`, configuration's `can_configure`), and where the resources read
*computes* scope facts it does so to publish a `permissions` object in its
200 body (`console/resources.rs:162-175`), never to refuse. The roster is
the same read class as `engagements`: a bounded observation the session
already proves the operator may see. No new `Scope`, no fourth ticket.

**The wire item carries exactly seven keys**: `name`, `framework`, `role`,
`state`, `engagement_id`, `requested_tokens`, `last_activity_ms` — all
nullable except `name`; `null` is "unknown", never zero and never invented.
`state` reuses the engagement state enum already fixed in
`native-api.js:6` (pending/reserved/active/rejected/revoked/failed) — no
second state map can drift. Bounds mirror the existing validators:
`text(name,128)`, `text(framework,64)`, `text(role,128)`, `id(engagement_id)`,
safe-integer `requested_tokens`/`last_activity_ms`. `last_activity_ms` is the
newest clock among the engagement's dispatch rows (`runner_attempts.created_at`)
— **"last dispatch activity", not "last seen"**: native has no heartbeat
model, and this ADR says so rather than implying parity.

**A server-owned `unavailable` list** names every retained roster column
native has no source for (`consumed`, `last_seen`, `online`, `tmux`, `pane`,
`credential_home`, `workspace_path`, `seat`): the page renders whatever the
server names, so a future source turns a column on by removing its name —
the same server-owned discipline as the alert transition map.

**No private field can reach the wire.** There is no key for a credential
home, workdir, state dir, workspace path, tmux target, pane buffer or token;
the item has no nested object at all (every key is scalar), so nothing can
hide inside one; and the exact-key client validator (`native-api.js:10-11`,
key set *and* count) fails the whole read if the server adds a key — a
future `tmux` or `homeDir` cannot arrive silently.

**The page** is `/console/agents/` (a new `agents/page.jsx` rendering
`NativeAgents.jsx`), added to the staged build and the asset key map with the
required-key rule unchanged (`assets.rs:157` still demands `/console/usage/`).
The five-document exception stays five: the page serves as a non-document
with **no query string** (`console.rs:357-358`), which a roster needs none of.

**Deferred, named:** a CLI read needs an `/api/native/v1/agents` operator
route and its own projection discussion — a follow-up, not this slice; the
rail's native-mode row list is a retained-file edit, also out.

## Consequences

Good, because the roster finally exists natively with a projection that
cannot forget a field (allow-list, exact-key, no nesting), and the honest
gaps are named by the server rather than papered over.
Bad, because the roster is weaker than the retained one (no condition,
consumed or seat columns) until their sources exist, and the derivation is
engagement-keyed: an agent with no engagement row is invisible.

## Alternatives Considered

- Port `serializeAgent` and strip fields — rejected: a redaction list is the
  failure mode (a deletion has to be remembered every time a field is added).
- Add task/progress columns from `canonical_tasks` — rejected: the retained
  roster refuses them on principle (`:62-64`); native should not widen what
  the retained surface deliberately narrowed.
- Gate the read behind a scope — rejected: no existing read gates; scope
  proliferation for an observation is not the model.
