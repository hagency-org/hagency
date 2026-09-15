spec: task
name: "Serve agent start stop and preset behind one finite lifecycle scope"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, agents, lifecycle, browser]
---

## Intent

Bind CL-S2 (ADR-130, D-SCOPE in force) on top of CL-S1's landed roster read:
one new finite scope, `Scope::AgentLifecycle`, owning the operator's three
lifecycle acts — **start** (an idempotent ensure over the store's state word,
never a process birth), **stop** (fence-and-record via the live set or an
unsettled `dispatch_stops` row; never settle), and **preset-apply** (pointing
at an already-published preset id, bounded) — with the console authority's
existing minting discipline and a never-grant list. The roster's served
permissions carry the controls; a read-only session renders none enabled.

## Constraints

### Must
- Mint `Scope::AgentLifecycle` only through the operator-authenticated `/api/native/v1/console/access` under ADR-107's issuance rules verbatim: one issuance per second, one outstanding ticket at a time, replacement invalidating the preceding, exchanged before the next is minted; the CLI flag `--manage-agent-lifecycle` is mutually exclusive with both existing management flags, declared and asserted pairwise.
- Add exactly one new public store surface — `stop_dispatch_for_agent` on `DomainStore`/`DomainRepository` (`domain_worker.rs` wrapping `conversation_lifecycle.rs`): the read-plus-stop entry that resolves the named engagement's dispatch (live set or unsettled `dispatch_stops` row) and fences it. It is the **only** store surface this slice adds; `fence_dispatch` stays `pub(super)`, and `settle_conversation_stop` remains uncallable from runtime-facing commands.
- Scope the grant to a session's `Grant` — the existing caps govern (at most 4 concurrent sessions, absolute 15-minute lifetime, no rolling expiry); the scope adds no longer-lived credential.
- Make start an at-most-once ensure: refused with a named code when the agent is already live, spawning nothing (ADR-053's fixed launcher rule).
- Make stop idempotent through the resolution predicate — live set (`queued`,`leased`,`started`,`parked`) or an unsettled `dispatch_stops` row, newest by id — fencing only the resolved dispatch, never `retire`'s session cascade, and serving the five-key wire object (`stopped`, `stop_pending`, `dispatch_id`, `fence`, `state`) with refusals in the console's existing `{"ok":false,"code":…}` envelope.
- Make preset-apply bounded: one pending apply at a time, refused for an unknown or unpublished preset id, never inventing or widening a field the store does not hold.
- Render the lifecycle controls only from CL-S1's served `permissions` booleans on the roster; a read-only session renders none enabled.
- Add the scope's user-facing strings to both dictionaries (`en` and `zh` in `mockup/lib/i18n.js`).

### Must Not
- Do not grant through this scope: resource publication or configuration, any account act (enrollment, readiness, credential namespace), dispatch or runner capability, workspace access, Matrix path or content, any child process or argv, or the settlement of any stop — `settle_conversation_stop` stays uncallable from runtime-facing commands.
- Do not widen an existing scope or fold lifecycle into configure (D-SCOPE rejected it); do not change the two existing scopes, their flags' `conflicts_with`, or any existing mutation route.
- Do not change `one_live_session` (`003:19`), the `dispatch_stops` DDL, `fence_dispatch`'s uncertain-vs-determinate split, `approval_verdict_receipts`, or the close path.
- Do not touch `browser_boundary`/`authenticate`/`common_authority`/`same_origin`, the five-document exception, the 8-permit semaphore, session rules, or `assets.rs:157`'s required key.
- Do not gate any scenario by OS or feature in the binding set (the browser scenario rides the `native-console-browser` lane as ever — present in `--all-features --list`, executing only when enabled).

## Boundaries

### Allowed Changes
- native/hagency/src/main.rs
- native/hagency/src/console.rs # the lifecycle router + operator issuer wiring and the scope error map — the scope cannot route or refuse without them
- native/hagency/src/console/client.rs # lifecycle_access() issues the operator CLI ticket — the flag has no issuer without it
- native/hagency/src/console/authority.rs
- native/hagency/src/console/agents.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/domain/conversation_lifecycle.rs
- mockup/app/agents/page.jsx
- mockup/components/NativeAgents.jsx # the lifecycle controls rendered from served permissions — the page has no controls without it
- mockup/components/Data.jsx # the agents load path and permissions slice — the controls cannot read manageLifecycle without it
- mockup/lib/native-api.js # startAgent/stopAgent/applyPreset + the exact-key validator — the controls have no client without it
- mockup/scripts/native-console-browser.mjs # the lifecycle browser lane over the roster walk — the browser scenario has no driver without it
- mockup/lib/i18n.js
- native/hagency/tests/console/agents.rs
- native/hagency/tests/console.rs
- native/hagency/tests/cli.rs
- native/hagency/tests/console/browser.rs
- specs/task-rust-console-agent-lifecycle.spec.md
- knowledge/decisions/adr-130-native-agent-lifecycle-authority.md
- docs/progress.md

### Forbidden
- Live services, live agents, credentials and deployed state.
- native/hagency/src/console/resources.rs; mockup/app/api/**; every other hagency-store file (the two licensed store paths carry exactly one new public entry — `stop_dispatch_for_agent`, F1's fix — and the fence kernel and settlement stay the store's own).

## Acceptance Criteria

Scenario: The agent lifecycle scope gates exactly its three acts
  Test: native_console_agent_lifecycle_is_scoped
  Level: integration
  Test Double: the console fixture holding a read-only ticket and an agent-lifecycle ticket
  Given both sessions against the three lifecycle routes and the neighbouring mutation routes
  When each route is called with each session
  Then the read-only session is refused with agent_lifecycle_scope_required on all three and no engagement row changes
  And the scoped session performs start stop and preset-apply only — it is refused by publication configuration account and every other mutation route with their own scope words

Scenario: The console-access grant issues exactly one lifecycle scope
  Test: native_cli_console_access_issues_agent_lifecycle_scope
  Level: unit
  Test Double: the console-access command line and the native issuer
  Given the console-access command line
  When --manage-agent-lifecycle is passed alone and combined with either existing management flag
  Then alone it issues a ticket that grants the three lifecycle acts and no publication configuration or account act
  And each combination is refused before any ticket is issued
  And issuance honours the one-per-second and one-outstanding rules with replacement invalidating the preceding ticket

Scenario: Start and stop are at-most-once over the store's own state
  Test: native_console_agent_start_stop_is_at_most_once
  Level: integration
  Test Double: an engagement with a dispatch in state started and a real domain writer
  Given an agent whose dispatch is live and then fenced by a first stop
  When start is called twice and stop is called again
  Then a start against an already-live agent refuses with the named already-live word and spawns nothing
  And the second stop resolves the same dispatch id and fence through the unsettled stop row writing no second row and still reports stop_pending
  And stopped stays false because no production path settles

Scenario: Operator recovery resumes an orphaned dispatch exactly once
  Test: native_console_agent_recover_dispatch_recovers_orphan
  Level: integration
  Test Double: a settled orphan dispatch with held lease quarantined session and dirty workspace
  Production caller: hagency::console::agents::recover_dispatch
  Given an engagement whose dispatch was settled to the orphan state with its lease held session quarantined and workspace dirty
  When a read-only session posts recovery and then a lifecycle operator posts recovery with evidence
  Then the read-only post is refused before any row changes
  And the operator post clears the lease clears the quarantine clears the dirty flag supersedes the orphan and enqueues the replacement
  And the recovery row records the operator evidence unchanged

Scenario: Recovery refuses a dispatch already fenced by a stop
  Test: native_console_agent_recover_dispatch_refuses_stopped_dispatch
  Level: integration
  Test Double: a dispatch carrying a dispatch_stops row and a real domain writer
  Given an engagement whose dispatch has an unsettled stop row from the conversation-stop flow
  When a lifecycle operator posts recover-dispatch for that dispatch
  Then the route refuses with the named not-recoverable word and no lease quarantine dirty or recovery row changes

Scenario: Preset-apply is bounded to one pending apply over published presets
  Test: native_console_agent_preset_apply_is_bounded
  Level: integration
  Test Double: a fixture with one published preset and one unpublished id
  Given an agent and the two preset ids
  When preset-apply is called for each and then twice in a row for the published one
  Then the unpublished id refuses with a named not-published word and no row is written
  And the second concurrent apply refuses with a named one-pending word
  And a completed apply points only at the preset id never widening a field

Scenario: The roster page renders lifecycle controls only from served permissions
  Test: native_console_agent_lifecycle_browser
  Level: integration
  Test Double: real Chromium over a fresh native fixture with the built roster page, receiving no operator token; browser-lane
  Given the native-console-browser feature whose selector appears in cargo test --list under --all-features exactly as native_console_browser is bound by the usage console spec
  When the roster renders under a read-only session and under an agent-lifecycle session
  Then the read-only roster shows no enabled lifecycle control
  And the scoped roster shows the controls enabled from the served permissions booleans
  And no external request leaves the page

## Decisions

**Start's already-live refusal word is `agent_already_live` (HTTP 409).** The
route maps `hagency_store::Error::State` to it (`console/agents.rs`), so the
start scenario binds to that exact word; the refusal happens before any spawn —
start is an ensure over the store's own state, never a process birth.

**Preset-apply has no completion path in this slice.** The apply is a pointer
over an already-published preset id, and `begin_lifecycle_apply` refuses a
second apply while one is pending (`agent_lifecycle_apply_pending`, HTTP 409);
nothing in this slice completes it, so the scenario's third clause narrows to
the pointer's shape — the response is exactly `{ok, presetId}` — never a widened
field. Completion is a later slice.

**The flag's pairwise exclusivity is declared AND asserted.**
`--manage-agent-lifecycle` declares `conflicts_with_all` against both
management flags, and the CLI selector asserts each combination is refused
before any ticket issues — a declaration is not a test.

**The browser lane is the roster walk's own driver.**
`native_console_agent_lifecycle_browser` rides
`mockup/scripts/native-console-browser.mjs` (now licensed in Allowed Changes),
adding a lifecycle lane that mints no ticket and asserts the controls appear
only when the served `permissions.manageLifecycle` boolean is true.

**Stop's widening to `retire`'s session cascade is a later slice.** The route
fences only the resolved dispatch — the one the named engagement resolves —
and does not reuse `retire`'s session-keyed walk, which also closes child
conversations (`conversation_lifecycle.rs:125-150`). Widening is a deliberate
future decision with its own review, not a default; the cost (sibling
dispatches keep running) is accepted now.

**One new public store entry, named.** `stop_dispatch_for_agent`
(`DomainStore` wrapper in `domain_worker.rs`, over the domain file) is the
slice's entire store surface (F1's fix): the selector's resolution and the
fence travel through it, and nothing else on the store becomes reachable from
the console crate.

**The CLI selector's grant-exclusivity and issuance clauses are pinned where they live (r1 F2).**
The scenario's "grants the three lifecycle acts and no publication configuration or
account act" is asserted by `native_console_agent_lifecycle_is_scoped`'s
neighbouring-refusal half (the lifecycle session is refused by the publication,
configuration and account mutations with their own scope words), and the
one-per-second / one-outstanding / replacement-invalidates rules are asserted by
`native_console_finite_clock`. The CLI selector itself pins what only the CLI can:
the flag combinations refuse before issuance, and the issued ticket's URL page and
shape. A reader taking the CLI selector alone as the pin for the grant's exclusivity
would over-credit it; this entry names where each clause lives.

## Out of Scope

CL-S1's roster read itself (landed), the stop-widening to `retire`'s session
cascade (a later deliberate slice), the host settlement path (the host's own),
the agent-lifecycle CLI read (no operator route exists), and every other
console page.
