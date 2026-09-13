spec: task
name: "Serve the MCP approval tool pair bound to the assigned task"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, mcp, approvals, runner]
---

## Intent

Bind PC-C3 of the private-card plan v6: a runner-facing pair of catalog tools —
`get_approval` (a read keyed by the assigned task, not an approval id) and
`consume_approval` (a mutation with `call_id`, at-most-once through the receipt
and the state machine) — over a four-key `ApprovalView` projection that omits
the owner room, the DM room id and the tool detail. Every MCP call stays bound
to the session's task (`mcp.rs:291-293`). This is a **native decision, not a
port**: the retained intake is an HTTP pair (`GET/POST /api/approvals/:id`,
`backend-v2.js:10911/10967`, driven by `lib/runtime-approval-client.js`), named
here as the divergence so no reader looks for a retained tool to match.

## Constraints

### Must
- Derive `get_approval`'s subject by the by-task lookup — task → session → live dispatch → `approval_contexts(dispatch_id, fence)` → `owner_approvals(context_id)` — pinned deterministically (the live dispatch's current fence; the newest approval at that fence), because `approval_context_dispatch` is non-unique (`013:30`) and `owner_approvals` has no unique `context_id` (`:32`).
- Catalog the tools within the existing bounds: `get_approval` takes `id` only (the task id, 1..=128, no `call_id`); `consume_approval` takes `id` + `call_id` (1..=512); `additionalProperties:false`; the annotation sets per the design (read: readOnly/idempotent; mutation: destructive/idempotent).
- Enforce at-most-once through BOTH gates: the `call_id` receipt (same `call_id` + identical content returns the stored response; a differing digest is `Error::Conflict`, `execution.rs:447-461`) and the approval state machine with the plan's named refusal words — `already_consumed` for `applying`/`applied`, `not_consumable` for the other settled states — replacing the generic `Error::RunnerAuthority` at `approvals.rs:880-882`.
- Serve `ApprovalView` — exactly four keys, `id, state, reusable_scope, choice`, from `ApprovalSummary` (`approvals.rs:97-102`) — and nothing else.
- Bind every call to the session's task (`mcp.rs:291-293`); a task that is not the session's own refuses.
- Assert the byte-level negative over the serialized tool response, with the two value classes stated: the escaped-prone values over the decoded strings, the metacharacter-free withheld names over the raw bytes.

### Must Not
- Do not accept an approval id, an owner mxid, a room id, another task, a `choice` input (the decision is the owner's, via `observe_owner_verdict`), or an `action` key (`mcp.rs:347`).
- Do not serve `owner_mxid`, `owner_dm_room_id`, `tool_name`, `description`, `input_preview`, the retained `reusable_scope` object, or the native-withheld columns (`digest`, `grant_id`, `scope_key`, `expires_at`, the `config`/`application`/`observation` JSON) through any key or any byte.
- Do not change ADR-110's no-packet-reconstruction rule or the frozen card validator and `MAX_CARD`; the catalog's existing tool set, schema bounds and `annotations` shape (`catalog.rs:52`); `mcp.rs`'s task binding, `call_id` shape check, mutation gate, or `FRAME_LIMIT = 32*1024`; the `call_id` receipt path and its cap; `approval_verdict_receipts` and `current_approval_bindings`' predicate; `ApprovalSummary`'s shape and `PrivateApproval`'s doc boundary.
- Do not add a console route or a new `Scope`.

## Boundaries

### Allowed Changes
- native/hagency/src/mcp/catalog.rs
- native/hagency/src/mcp.rs
- native/hagency/src/mcp/**
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/
- native/hagency/tests/mcp.rs
- native/hagency/tests/mcp/**
- specs/task-rust-mcp-approval-tools.spec.md
- knowledge/decisions/adr-064-native-matrix-approval-intake.md
- knowledge/decisions/adr-110-native-private-approval-card.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed state and Matrix server changes.
- native/hagency/src/console.rs; the card validator and `MAX_CARD`; the catalog's existing tools' schemas.

## Acceptance Criteria

Scenario: The two approval tools are catalogued within their bounds
  Test: native_mcp_approval_tools_are_catalogued_and_bounded
  Level: integration
  Test Double: the MCP stdio helper over a fixture session
  Given the catalog listing for a bound session
  When the two new tools are listed and invoked with their exact input shapes
  Then each accepts only its declared keys with additionalProperties false and the design's annotations
  And neither accepts an approval id, an owner mxid, a room id, another task, a choice or an action key

Scenario: A consumed approval cannot be consumed twice
  Test: native_mcp_approval_consume_is_at_most_once
  Level: integration
  Test Double: a fixture dispatch with one approval at the live fence
  Given an approval whose consume has succeeded under one call_id
  When consume_approval is called again with the same call_id and identical content
  Then the stored response is returned unchanged
  And a differing content under the same call_id refuses with the store's conflict word
  And a fresh call_id against a consumed approval refuses with the named word already_consumed and never re-applies

Scenario: The approval projection omits the owner room and the tool detail
  Test: native_mcp_approval_projection_omits_owner_room_and_tool_detail
  Level: integration
  Test Double: a fixture approval whose withheld fields exist in the store
  Given an approval served through get_approval
  When the tool response is serialized
  Then it carries exactly the four ApprovalView keys
  And the decoded string values contain no owner mxid, owner room id, tool name or input preview text and the raw bytes contain no withheld field name

Scenario: An approval cannot be read or consumed for another task
  Test: native_mcp_approval_is_bound_to_the_assigned_task
  Level: integration
  Test Double: two fixture sessions with distinct tasks and one approval each
  Given a session bound to one task and an approval belonging to another task's derivation
  When get_approval or consume_approval names the other task
  Then both refuse with the session-binding word and no approval row changes

Scenario: An approval that failed to deliver is observed as denied
  Test: native_mcp_approval_failed_delivery_observes_the_denial
  Level: integration
  Test Double: the C1 denial leg over a fixture failed delivery
  Given an approval whose private delivery failed and C1's denial leg has run
  When get_approval serves it
  Then under the deny assumption it reports state decided with choice deny
  And under leave-pending it reports state pending with choice null — the scenario asserts whichever D-PC-FC decides, and binds with C1's denial leg, not before

## Decisions

**C3 is independent of PC-C0** (v6's V5 correction): four scenarios bind at C3
without the collector; the fifth (`…failed_delivery_observes_the_denial`) binds
with **C1's denial leg only**. C3's lane slot after C2a/C2b is an ordering
choice over shared files, not a dependency. The two store tests (the by-task
lookup; the new refusal words) are C3's own new store work and land with it.

## Out of Scope

The console approval observation (PC-C2, its own spec), the delivery status
route (behind PC-C0), the fail-closed denial policy itself (D-PC-FC, C1's to
decide), and any retained HTTP intake port.
