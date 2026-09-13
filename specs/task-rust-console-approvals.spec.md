spec: task
name: "Serve the console approval observation with seven keys and no owner detail"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, approvals, read-only]
---

## Intent

Bind PC-C2a and PC-C2b of the private-card plan v5: first a bounded, `SELECT`-named
approval **list read** on the store with its `DomainStore` wrapper and a store test
(C2a, the A5 addition that lands first); then two observation routes and a page
section serving `ApprovalRow` with exactly seven named keys and the withheld sets
stated (C2b) — read-only behind the existing `authenticate` hoop, no new `Scope`.
The delivery route is **not** this spec's to bind early: per the v5 check's V2 its
selector belongs with the route's own commit behind PC-C0, and a test asserting a
refusal for a nonexistent route asserts nothing.

## Constraints

### Must
- Add the C2a list read first: a `SELECT` naming its columns with `after`/`limit` and a hard cap, mirroring `engagements` (`domain.rs:607-615`, 1..=100), its wrapper beside `approval_summary` (`domain_worker.rs:1570`), and a store test.
- Serve `ApprovalRow` — exactly seven keys, `camelCase`, `deny_unknown_fields`: `id`, `state` (one of the seven CHECK words, `013:34`), `choice` (`once|task|always|deny` or null), `reusable_scope` (`scope_key IS NOT NULL`), `expires_at`, `engagement_id` (one join to `approval_contexts`), `project_room_id` (via the engagement join, nullable).
- Withhold, and state in the row's absence: the retained-only set (`owner_mxid`, `owner_dm_room_id`, `tool_name`, `description`, `input_preview`, the card bytes), the native columns deliberately withheld (`description`, the `config`/`application`/`observation` JSON), and the no-native-source fields (`decided_at`, `created_at`, `consumed_at`, `denial_reason`, `decision_event_id`, the agent name).
- Mount `console/approvals.rs` beside the four existing sub-routers under the API hoop; reads carry no scope — a read-only ticket can reach no mutation.
- Render the page section from `state`/`choice` words and a status word for an undelivered card — never a card, never a preview; the five-document exception stays five and `/console/approvals` is a non-document.
- Assert the byte-level negative over the serialized response, not only the key set.

### Must Not
- Do not add a new `Scope`, a mutation route, a verdict route or a detail route.
- Do not serve any owner identity, owner room, tool detail, input preview or card byte through any key or any nested byte — the row has no nested object at all.
- Do not bind the delivery route's selector before the route's own commit behind PC-C0 (V2); when that route lands it serves `PrivateApprovalDeliveryStatus` as-is (five keys, no request id, no room, no body) and needs `lib.rs` and `bootstrap.rs`.
- Do not change `browser_boundary`/`authenticate`/`common_authority`/`same_origin`, the session cap of 4 or the 15-minute lifetime, the 8-permit semaphore, `ApprovalSummary`'s shape, `PrivateApproval`'s doc-comment boundary, `current_approval_bindings`' predicate, or `approval_verdict_receipts`' at-most-once rule.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/
- native/hagency/src/console/approvals.rs
- native/hagency/src/console.rs
- native/hagency/src/console/assets.rs
- native/hagency/tests/console/approvals.rs
- native/hagency/tests/console.rs
- mockup/lib/native-api.js
- mockup/components/NativeApprovals.jsx
- mockup/app/approvals/page.jsx
- mockup/scripts/build-native-console.mjs
- mockup/lib/i18n.js
- specs/task-rust-console-approvals.spec.md
- knowledge/decisions/adr-138-bounded-native-approval-observation.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed state and Matrix server changes.
- native/hagency-matrix/src/approval_delivery.rs (the delivery route's own commit owns it, with lib.rs and bootstrap.rs); native/hagency/src/console/authority.rs; mockup/app/api/**.

## Acceptance Criteria

Scenario: The C2a list read is bounded and pages by the opaque cursor
  Test: native_console_approval_list_read_is_bounded
  Level: integration
  Test Double: real store rows behind the store fixture; no console route
  Given seeded approvals across the cursor boundary
  When the new list read is called through its DomainStore wrapper
  Then it pages by after/limit with the hard cap refusing out-of-range limits
  And every returned row names its SELECT columns with no derived blob

Scenario: The observation projection omits owner identity and tool detail
  Test: native_console_approval_observation
  Level: integration
  Test Double: the console fixture with seeded approvals through the new read
  Given a valid console session and approvals whose withheld fields exist in the store
  When the list and single observation routes are served
  Then every row carries exactly the seven declared keys with no nested object
  And no byte of any response contains an owner mxid, an owner room id, a tool name, an input preview or any card byte
  And the absent never-invented fields appear as absent rather than null-derived guesses

Scenario: A foreign origin cannot observe approvals
  Test: native_console_approval_refuses_foreign_origin
  Level: integration
  Test Double: the console fixture with the non-document page mounted
  Given a valid console session cookie and the /console/approvals non-document page
  When the routes are called with a cross-site sec-fetch-site or a foreign host or origin
  Then each refuses with console_origin_required and serves no approval row

Scenario: An approval with no delivery status shows its state word and never a card
  Test: native_console_approval_undelivered_shows_status_not_a_card
  Level: integration
  Test Double: the console fixture and the built approvals page; the delivery route not yet landed
  Given an undelivered approval rendered on the page
  When the row renders
  Then it shows the state word and a status word and never a card or a preview
  And no property of the page could carry card bytes because no key exists for one

Scenario: A read-only session can observe but cannot decide
  Test: native_console_approval_observation_is_read_only
  Level: integration
  Test Double: the console fixture holding a read-only ticket
  Given a read-only console session
  When the observation routes are called
  Then both serve their rows
  And no mutation verdict or consume route exists on the approvals path to refuse — the absence is asserted, not assumed

## Decisions

The v5 check's V2 ruling is recorded here so it survives the companion: the delivery
route's selector `native_console_approval_delivery_status_refuses_foreign_origin`
is **bound in the delivery route's own commit behind PC-C0**, not in this spec — the
spec names it once in Must Not as forbidden-early. The foreign-origin negative for the
observation routes is carried instead by
`native_console_approval_refuses_foreign_origin` above, which exists in this commit's
world (the routes do).

## Out of Scope

The delivery route (its own commit behind PC-C0, owning `lib.rs` and `bootstrap.rs`),
the MCP approval tool pair (PC-C3), the fail-closed denial (PC-C1), and every other
console page.
