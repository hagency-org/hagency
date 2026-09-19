---
kind: decision
id: ADR-132
title: "Bounded native project-side observation"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
liveness: auto
tags: [console, matrix, project-sides, read-only, privacy]
---

## Context

A project side is one homeserver, one credential and one representative
(ADR-016 decision 1: "The id IS the server name, so one side per homeserver
is structural rather than validated"), and the retained console renders them
on the 项目 card (`mockup/app/projects/page.jsx:141-166`) from the store's
allow-list projection `publicSide` (`lib/project-side-store.js:198-282`) —
deliberately a projection, not a strip: "a deletion has to be remembered
every time a field is added and a projection cannot forget" (`:190-193`).
The retained proxy refuses `POST /api/project-sides/:id/registration`
outright because it answers with the registration YAML "which carries an
`as_token` and an `hs_token` in plaintext — the only time either is
readable" (`route.js:208-219`), and admits only the list, one-side and
budget reads (`:102`, `:115`, `:127`). Native has **no project-side model
and no credential anywhere**: grep for `as_token|hs_token|asToken|hsToken`
under `native/` returns nothing. What native has is the fleet registration:
`registrations(fleet_id,generation,config)` and
`projects(fleet_id,id,generation,room_id,owner_mxid,owner_room_id)`
(`domain.sql:1-15`), with `reception_room_id` a real `Registration` field
(`hagency-core/src/authority.rs:22`).

## Decision

**Serve `GET /console/api/project-sides`** — a new
`console/project_sides.rs` behind the existing `authenticate` hoop with
**no scope** (scope facts are payload, never a gate on reads — the same
class as `engagements` and the roster, ADR-126). It reads through a **new
store list read `project_sides()`** — a bounded `SELECT`-named
`LEFT JOIN registrations→projects` added to `domain.rs` with the
`DomainStore` wrapper and `lib.rs` re-export — because **no list read exists
today**; this slice owns that read and its store test. This makes the slice
two commits of work (store read first, route second), not one.

**The wire item carries exactly six keys**: `id`, `representative`,
`generation`, `reception_room_id`, `registered`, `projects` — with
`projects[]` exactly `{id, room_id}`, bounded to 64 per side (the store
bounds `registrations` at 1024, `domain.rs:467-472`). `registered` is the
honest form of retained's `active`: *the row's generation column agrees with
the generation its own config carries* — the two stored copies of the same
fact, checked rather than assumed (drift is visible, not assumed away) — not
a credential claim, not an access verdict.
`owner_mxid`/`owner_room_id` are **withheld**: the owner's DM room is
non-public (ADR-112), and the retained projection does not serve them
either. Room-id-class data that *is* operator-facing (`reception_room_id`,
`projects[].room_id`) is served, matching the retained projection's own
class split.

**No credential can reach the wire in any byte.** The item has no credential
key at all; a server-owned `unavailable` list names every retained field
native has no source for (`label`, `api_base_url`, `credential_kind`,
`has_credential`, `awaiting_install`, `sender_localpart`, `appservice_url`,
`namespace`, `access_state`, `access_detail`, `allocated_tokens`,
`project_name`, `owner`). The byte-level test is a **forward guard**: it
seeds a registration whose config carries a credential-shaped value and
asserts the serialized body contains none of it — it passes trivially today
(native has no credential column) and fails the moment a future field
carries one in. A green run is **not** proof of present safety, and this ADR
says so.

**The page** is a new `/console/project-sides/` (deliberately separate from
the retained `/projects`, whose invites/whitelist/contributions have no
native source — folding a branch in would hide most of that page), staged by
`build-native-console.mjs` with the mime/key-map/alias edits of ADR-126's
pattern and the required-key rule unchanged. The five-document exception
stays five; the page serves as a non-document with no query string.

**Deferred, named:** the rail row and any CLI read (no
`/api/native/v1/project-sides` operator route exists; same follow-up class
as the roster's).

## Consequences

Good, because the side list exists natively as a projection that cannot
forget a field, the byte-level guard arms against the credential model that
will one day exist, and the withheld owner channels are named.
Bad, because most of the retained card's fields are `unavailable` until
their sources exist, `registered` is weaker than an access verdict, and the
slice carries a store read that lengthens it beyond one commit.

## Alternatives Considered

- Port `publicSide` wholesale — impossible: most fields have no native
  source; inventing them is the silent-zero failure.
- Derive `hasCredential: false` because native holds none — rejected: a
  claim about a record that does not exist reads as "unverified" rather than
  "unmodelled".
- Serve a native branch inside the retained `/projects` page — rejected:
  that page is mostly sources native does not have.
