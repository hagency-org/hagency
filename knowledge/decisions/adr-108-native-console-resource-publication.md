---
kind: decision
id: ADR-108
title: Retained native resource observations and finite catalog publication authority
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

ADR107 retains the original console with native usage. Existing native resources
already provide configurations including withdrawn rows, typed selected pool and
shared-account budgets, seat declarations and role availability. Legacy resource
pages also expect names, daily rate caps, execution policy, detected runtimes,
Agent members and authentication-home facts which native DTOs do not yet provide.
ADR025 keeps Agent definitions project-owned and requires durable explicit withdrawal.

## Decision

Retain /resources and its resource table, ResourceAgents control, preferences,
translations and technical-details pattern. A bounded native data mode shows safe
resource profiles, current native catalog inclusion, selected PoolBudget and
SeatBudget, and typed role observations. Public resource identifiers stay in
technical details; internal preset IDs, account identities, full configurations,
credentials and auth-home paths do not enter browser DTOs. Configured model and
reasoning labels are actual fields, not invented friendly names. Dynamic resource
query selection works for records created after the asset build. Original usage
and resources use full document navigation; this slice serves no Next RSC server.

ResourceAgents uses native-specific Include in native catalog / Withdraw from
native catalog labels. A local ledger/catalog change does not claim remote Palpo
publication. Creation and the complete original wizard follow separate native
contracts for their missing fields and discovery; they are not replaced by this
bounded observation/publication step.

Default tickets and sessions remain read only. The local operator command's
explicit --manage-resource-publication option issues one finite scoped ticket.
Only its exchange creates a management session entry retaining a concrete opaque
ResourcePublicationAccess. Read-only entries own no such access. The original
entry prepares a non-Clone non-Deserialize command binding the exact resource,
configuration revision, desired publication and original request deadline. Browser
payloads cannot supply authority, callbacks or scope assertions. The host-only
store types are concrete and do not introduce a generic authorization callback.

Global ticket/session map locks remain short. Queue admission retains the exact
entry and releases that map. The original store worker obtains SQLite IMMEDIATE,
then tries the per-session mutation gate without waiting; it never reacquires the
global map. The gate remains through revision comparison, publication-only update
and commit. Deadline, original session expiry and immediate console retirement
are checked after SQLite waiting and again before mutation and commit. Unrelated
usage reads and sessions never acquire this gate. Logout tries only its original
entry gate and refuses Busy without claiming revocation; no inverse blocking lock
acquisition is permitted. No deadline is widened or restarted.

Malformed or unscoped requests refuse before admission. Queue/gate contention is
explicit Busy; an obsolete revision is conflict with no overwrite. Only observed
current commit returns success. Caller/reply loss, response timeout or authority
loss after a possible commit remains outcome_unknown. A later read reconciles
current durable publication without attributing it to a lost command. No write is
automatically retried. UI action conflict/unknown stays visible; logout Busy/unknown
has a visible explicit retry and never says access ended. Old data may clear while
server revocation remains unresolved.

## Validation and limits

Use real SQLite waits, actual original-session gate contention, current-clock
checks, private temporary state and real mutation/readback. Browser acceptance
uses retained assets and actual Chromium in English and Chinese, including a
resource created while the page is open and explicit failed actions. The real
native executable serves with an empty runtime PATH. The existing default-off
browser feature and mandatory CI lane remain; missing prerequisites fail when
enabled, while listing tests provides no execution evidence. No live service or
production configuration is changed, and full M7 migration remains open.

---

## Amendment — the account surface (retention MA-S3a)

This adds the **third** management grant, `--manage-account-enrollment`, and a
bounded account observation. Three plans claim "the fourth console scope"
(backlog §5.4); the code has two today, so this is the third, and **whichever of
the three lands first owns the count**. The grant covers reserve+materialize,
retire and enrol — one class of act over the host's credential namespace, where
a grant to mint without one to destroy would be weaker than the offline verb it
mirrors. The `console-access` dispatch becomes 4-way (`main.rs:234-250` is today
3-way); **no `ConsoleAccount` verb is added** — a subcommand layer on top of
that same restructure is redundant, and the offline `account` verb already
surfaces this state on the channel ADR-114 chose.

**Authority asymmetry, stated.** Enrolment binds a concrete non-Clone command
(`AccountEnrollmentAccess`, accounts.rs:688-730). Reserve+materialize and
retire do not — plain `&mut DomainRepository` methods whose only offline guard
is the exclusive owner (bootstrap/accounts.rs:24) — so on the console their
authority is a session-scope boolean. Acceptable because those routes take only
bounded scalars, the account identity is store-generated, and the
credential-binding act is the one that is command-bound. Binding the other two
is a named follow-up.

**The browser DTO is a new `AccountRow`, and all five routes serialize it —
never `AccountChoice`.** `AccountChoice` is `Serialize` (accounts.rs:202-212)
and re-exported (lib.rs:108), one line from a browser response; it also carries
`authentication`/`quota` that would read as a readiness answer and would break
the exact-key validator when MA-S3b lands. Only the enrolment mutation takes
`expected_revision`.

**The unknown window is wider than the offline verb's** (2 s reply bound,
domain_worker.rs:2393, vs a 5 s preparation deadline, accounts.rs:525), and
`materialize_account`'s `'uncertain'`-before-`mkdir` ordering leaves the row
inspectable either way. The printed link lands on `/console/accounts/` (a third
branch of client.rs:118-126), and the page stays **outside** the five-document
exception (a non-document with no query string).

---

## Amendment — the catalogue's derived fillability, and what stays absent (G5)

ADR-108's observation route already serves the six-role catalogue beside the
resource list (`console/resources.rs:144-152`), and the native resources page
already renders it as its own section (`NativeResources.jsx:75`). The
console-parity inventory's G5 row read "STILL OPEN — native resources is a
different model"; on this revision the *read* is served, so this amendment
records what was actually missing, which is smaller and narrower.

**Added: three derived keys on each role row — `families`, `fillable`,
`overTier` — over ONE set.** `role_publications` (`domain.rs:343-356`) already
walks the published resources to compute `available` via `Resource::qualifies`
(`project.rs:168-173`), which is
`published ∧ provisionable() ∧ ceiling.tokens.is_some() ∧ qualification::qualifies`.
The amendment derives all three new values over **that same set** and no other.
`qualification::resources_for_role` (`:192-210`) is deliberately NOT used: its
filter omits both `published` and `provisionable()`, so a resource could count
toward `fillable` and not toward `available` — two answers for one question, the
drift `route.js:121-125` refuses. Cross-referenced with ADR-111, whose preset
scope owns the write side this amendment deliberately does not touch.

**`families` is the model family, not the framework.** The retained catalogue's
cross-family rule counts *model families* — `mockup/lib/derive.js:91` builds
`families` from `r.match.family`, and `:100` requires two of them for a
cross-family role (`lib/matrix-agent.js:26-27`). `qualification::model()`
returns exactly that (`(tier, family)`, `qualification.rs:143-156`), so
`families` is `model(&r.profile()).1` over the qualifying set, sorted for a
stable wire. A framework list would have made the native verdict disagree with
the retained page for the same deployment.

**Not added, and why — the ADRs already named these absent.**
- **`name`.** `Resource` has no name field (`project.rs:127-139`); ADR-111:55
  says a preset "must own friendly names rather than a second browser metadata
  store". The page keeps its derived label (`NativeResources.jsx:9`).
- **`rateCapPerDay`.** `Ceiling` is `{tokens, period}` only
  (`allocation.rs:72-76`); ADR-108 and ADR-111 name daily rate caps absent
  pending canonical persistence (`native/README.md:716-719` states the same).
  A `null` would read as "no cap chosen" when the truth is "no such column
  exists", so no key is added and the page renders a reasoned blank — the
  retained page's own device (`resources/page.jsx:119-121`).
- **`apiBaseUrl`, `apiKeySet`, `extraArgs`.** Forbidden by ADR-111:38 and
  `native/README.md:716-717`.
- **`usedBy`.** Depends on the agent roster (backlog CL-S1); named as a
  dependency, not silently dropped.

**Read-only.** No route, no scope, no mutation is added. Publishing a role
remains an operator act (`resources.rs:22`) and the browser keeps no
offer-terms form: the retained catalogue's `count`/`budgetCapPerEngagement`/
`rateCap` write (`capability/page.jsx:288-325`) needs the canonical persistence
ADR-111 names.

**One-key-set contract change, one commit.** `RoleRow` is `deny_unknown_fields`
(`console/resources.rs:57-59`), so the three new keys make the
`serde_json::from_value::<RoleRow>` at `:148` hard-fail until `RoleRow` gains
them, and the client's exact-key conjunction (`native-api.js:10-11` at
`:176-178`) must move with it. Server keys, `RoleRow` and the validator are
therefore one commit; the failure mode is a schema error (a 503), never a
silent widening.
