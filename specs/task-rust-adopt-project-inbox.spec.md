spec: task
name: "Adopt an authenticated project inbox beside the owner DM"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, provisioning, matrix]
---

## Intent

Let the existing external-account adoption command bind the request's project
room as a second inbox, enabling native group/DM qualification with local runners.

## Constraints

- Optional project_inbox names only session, workspace and room generation.
  Its room always comes from the verified request; privacy is always Group.
- Re-read project membership using the authenticated agent token. Require the
  agent and owner joined before any domain mutation. Caller JSON is not proof.
- Distinct DM/project sessions and workspace identities; no existing route is
  silently aliased to a new requested session name.
- Preserve existing request, admission, effect, route and mention/privacy gates.
- No new HTTP mutation endpoint, model authorization, credential copying or
  SDK store reset. Offline fixtures never access live services.
- Keep Matrix response collection bounded even without Content-Length.

## Allowed changes

- native/hagency/src/bootstrap/provision.rs
- native/hagency/src/bootstrap/provision/**
- specs/task-rust-adopt-project-inbox.spec.md
- knowledge/decisions/adr-172-adopt-project-inbox.md
- docs/**

## Scenarios

Scenario: Authenticated adoption binds separate project and owner routes
  Test: native_adopt_project_inbox
  Given a verified project request and authenticated agent membership
  When optional project inbox adoption runs
  Then both distinct routes and workspaces exist with their observed privacy
  And exact replay creates no additional session

Scenario: Missing project authority or aliased identities refuse adoption
  Test: native_adopt_project_inbox_refusals
  Given absent agent/owner membership or invalid inbox identities
  When adoption runs
  Then invalid input creates no domain authority
  And a new name cannot silently reuse an existing route

Scenario: Unknown-length Matrix bodies stop at the byte limit
  Test: native_adopt_matrix_chunked_bound
  Given a chunked response larger than one MiB whose EOF is withheld
  When the adoption client reads the response
  Then it refuses at the byte limit before EOF or the request deadline
