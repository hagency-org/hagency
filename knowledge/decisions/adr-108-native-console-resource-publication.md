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
