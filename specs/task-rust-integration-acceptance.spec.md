spec: task
name: "Prove two-agent integration acceptance in process with one shared room and separate DMs"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, acceptance, fixtures]
---

## Intent

Bind MA-M8a of ADR-144: the migration plan's M8 item 3 second sentence — "Test two
Agents sharing a room and separate DMs"
(`docs/design/hagency-rust-migration-plan.md:378-379`) — as an in-process acceptance
over the real-TLS fake peer, using a new two-engagement fixture beside the
single-engagement `Fixture::new()` that every existing harness depends on. One spec
per subject, with its own boundary; the qualification half is the sibling spec
`specs/task-rust-integration-qualification.spec.md`.

## Constraints

### Must
- Add `Fixture::new_pair()` beside `new()` with two admitted engagements on two resources whose seats differ, two transport identities, one shared project room and one direct room per engagement.
- Assert the four properties and two refusals of the retained oracle (`tests/matrix-direct-chat.test.js:186`): independent delivery per engagement, DM reach limited to its own direct room, handoff settling with usage on the right engagement, and no cross-engagement readability; refused private-root-threaded reply, refused ambiguous sender.
- Keep the shared room a delivery room only; the approval intake admits `RoomPrivacy::Direct` rooms only.
- Run in the default test targets on every hosted leg; no OS gating, no feature gating, no skip.
- Keep every fixture tempfile-owned and offline; no live homeserver, no credentials.

### Must Not
- Do not change `Fixture::new()` or any test built on it.
- Do not widen the DM-only approval-room rule or the `#[cfg(test)]` fault seams.
- Do not fake a foreign homeserver's admission decision or a second device's real keys — those are the qualification half.
- Do not add a crate, migration, route or schema change.

## Boundaries

### Allowed Changes
- native/hagency-matrix/tests/common/mod.rs
- native/hagency-matrix/tests/common/pair.rs
- native/hagency-matrix/tests/fixtures/**
- native/hagency-matrix/tests/**
- native/hagency/tests/**
- specs/task-r...[credential-redacted].spec.md
- knowledge/decisions/adr-144-two-agent-integration-acceptance.md
- docs/progress.md

The SR-2/ADR-133 launchd harness (`native/hagency/tests/service_launchd.rs`)
is touched in the acceptance-commits lineage for the XML-comment-stripping
defect review r3 named (F11/F12); the path is already covered above.

### Forbidden
- Live services, live models, credentials and deployed state.
- native/hagency-matrix/src/approval_intake.rs; the fake peer's TLS trust material.

## Acceptance Criteria

Scenario: Two agents deliver into one shared room, each charged to its own engagement
  Test: native_two_agents_share_one_room_with_independent_delivery
  Level: integration
  Test Double: the real TLS fake peer and the two-engagement fixture; no real homeserver
  Given two admitted engagements on two resources whose seats differ and one shared project room
  When each agent sends a final reply to the shared room
  Then both sends reach the peer with the shared room id and each is charged to its own engagement
  And a threaded reply to the other engagement's private root is refused with no send
  And a send whose agent identity is ambiguous is refused

Scenario: A DM from one agent never reaches the other engagement or the room
  Test: native_two_agent_dm_reaches_only_its_own_engagement
  Level: integration
  Test Double: the real TLS fake peer and the two-engagement fixture
  Given the shared room and a direct room for the owner
  When one agent sends a private reply
  Then the ciphertext is addressed to the direct room only
  And no request carries the shared room id
  And the other engagement's rows are unchanged
  And the owner's DM back to that agent reaches its own engagement's inbox only
  And the owner's DM to the other agent never appears in the first engagement's rows

Scenario: A task handed across the room settles and attributes usage to one engagement
  Test: native_two_agent_task_handoff_observes_usage_on_the_right_engagement
  Level: integration
  Test Double: the owned-dispatch harness over the two-engagement fixture
  Given a task created for one of the two engagements from a shared-room message
  When the owned dispatch runs to a settled completion
  Then the task settles on its own engagement
  And the usage ledger observes the spend on that engagement and not on the other

Scenario: A message for one engagement is never readable from the other
  Test: native_two_agent_message_never_crosses_engagements
  Level: integration
  Test Double: the two-engagement fixture with an active conversation per engagement
  Given two engagements with an active conversation each
  When one engagement's inbox is read
  Then only its own messages appear and the other's sequence and body are absent

Scenario: No DM body is visible in the shared room
  Test: native_two_agent_dm_content_is_absent_from_the_room
  Level: integration
  Test Double: the two-engagement fixture with an enciphered DM body and the room's recorded sends
  Given an enciphered DM body and the room's recorded sends
  When the room's ciphertexts are inspected
  Then none decrypts to the DM body

## Out of Scope

The real-homeserver qualification (the sibling qualification spec), the outage and
recovery classes of M8 item 4, on-hardware budgets of item 5, Windows and macOS
platform coverage, and any production crate change.
