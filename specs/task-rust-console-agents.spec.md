spec: task
name: "Serve the native agent roster behind bounded browser authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, agents, browser]
---

## Intent

Bind the agent-roster read-only observation (ADR-126): a `/console/api/agents`
projection of the engagement rows that omits every private field the retained
`GET /api/agents` response spreads, and the `/console/agents/` page that renders
it. One spec per page family, per the house pattern (the usage and engagements
reads live in the usage spec; the alerts page in the ceiling-alert spec). Browser
selectors follow the same gated lane that spec's Decisions block records:
compiled only under the default-off `native-console-browser` feature, listed by
`cargo test --list` under `--all-features` exactly as `native_console_browser`
is bound there, executed only in the enabled lane.

## Constraints

### Must
- Mount `GET /console/api/agents` under the console `authenticate` hoop with no scope: scope facts are a payload, never a gate on reads.
- Serve exactly the seven declared keys with no nested object; `null` means unknown, never zero.
- Derive `last_activity_ms` as the newest `runner_attempts.created_at` among the engagement's dispatches — and a dispatch with no attempt row reports `null`.
- Publish a server-owned `unavailable` list naming every column native has no source for.
- Stage the page through the asset key map, mime allowlist and `/console/agents` alias; the required key `/console/usage/` stays.
- Keep the client's exact-key validator on the roster wire item.
- The ingress roster regression must run the original inline physical factory
  for both account kinds, using offline TLS, an independent owner and the actual
  native process. Read its minted engagement by request id from the original
  store and query the real authenticated console before and after activation.
  No target activation, approval binding or session route may be seeded. This
  test does not establish native startup/fleet wiring or live qualification.

### Must Not
- Do not serve a credential home, workdir, state dir, workspace path, tmux target, pane buffer or token through any key.
- Do not add a new scope, a fourth ticket, or a task/progress/utilisation column the retained roster refuses.
- Do not widen the five-document exception; the page serves as a non-document with no query string.
- Do not gate a whole scenario by OS or by feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency/src/console/agents.rs
- native/hagency/src/console.rs
- native/hagency/src/console/assets.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency/tests/console/agents.rs
- native/hagency/tests/console/real_agent.rs
- native/hagency/tests/inline_factory/mod.rs
- native/hagency/tests/console.rs
- native/hagency/tests/console/browser.rs
- native/hagency/Cargo.toml
- mockup/app/agents/page.jsx
- mockup/components/NativeAgents.jsx
- mockup/lib/native-api.js
- mockup/scripts/build-native-console.mjs
- specs/task-rust-console-agents.spec.md
- knowledge/decisions/adr-126-native-agent-roster.md
- native/README.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed Node runtime and Matrix server changes.
- mockup/app/api/hagency/[...path]/route.js (the allowlist stays).

## Acceptance Criteria

Scenario: The agent roster observation omits every private field
  Test: native_console_agent_roster_observation
  Given the roster read behind the console authenticate hoop with at least one engagement carrying a live session
  When the roster is read through a valid session
  Then every item carries exactly the seven declared keys and no key names a credential home tmux target workspace path or token and no item carries any nested object
  And an engagement with no session row reports last_activity_ms null rather than zero
  And the top-level unavailable list names every column native has no source for

Scenario: A foreign origin cannot read the agent roster
  Test: native_console_agent_roster_refuses_foreign_origin
  Given a valid console session cookie
  When the roster is read with a cross-site sec-fetch-site or a foreign host or origin
  Then the route refuses with console_origin_required and serves no agent item

Scenario: The agent roster page renders under the native browser boundary
  Test: native_console_agent_roster_browser
  Given the native console fixture and the built agent page with the native-console-browser feature whose selectors appear in cargo test --list under --all-features exactly as native_console_browser is bound by the usage console spec
  When real Chromium opens /console/agents without any operator token
  Then the roster reaches data-native-state ready with no external request and no credential value on screen

Scenario: The roster shows an agent whose engagement was minted by the production ingress
  Test: native_console_roster_shows_an_ingress_provisioned_agent
  Given a production intake that admitted a com.hagency.engagement.request.v1 event and a representative verdict that made the engagement effective and bound its session route
  When the operator lists the agent roster through the real GET /console/api/agents route
  Then the roster carries an item whose engagement_id is the engagement the store minted by request_id (never seeded), and the engagements row, the effects row in kind=provision state=complete, and the matrix_session_routes row for that engagement all exist
  Production caller: hagency::console::agents::list

## Out of Scope

The agent lifecycle (start/stop/preset behind a finite scope), a CLI read via an
`/api/native/v1/agents` operator route, the rail row (a retained-file edit), and
every other console page.
