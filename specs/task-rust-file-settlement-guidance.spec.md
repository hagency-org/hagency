spec: task
name: "Guide bounded inspection of nonterminal file delivery outcomes"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, mcp, files]
---

## Intent

A live Codex task reported file failure after inspecting a possible write; the
same original write subsequently settled Delivered. Keep conservative status
semantics and explain the permitted next action at the actual tool response.

## Constraints

- Queued and OutcomeUnknown remain nonterminal; neither is delivery or failure.
- Add fixed MCP text guidance only. Structured receipts, authority, admission,
  process ownership, completion guards and HTTP/Matrix retry behavior stay intact.
- Poll only the same delivery_id, within the original task deadline and authority.
- Never recapture, resend, invent acceptance or extend a deadline to clear unknown.
- Terminal outcomes carry no continued-polling guidance.
- Regression tests contact synthetic local peers only.

## Allowed changes

- native/hagency/src/mcp.rs
- native/hagency/src/mcp/file_catalog.rs
- native/hagency/tests/file_service.rs
- specs/task-rust-file-settlement-guidance.spec.md
- knowledge/decisions/adr-177-file-settlement-guidance.md
- docs/**

## Scenarios

Scenario: An in-flight original write remains unknown until its real acknowledgement
  Test: native_file_service_nonterminal_polling
  Given a real native helper and an upload or encrypted event held by a local TLS peer
  When get_file_delivery reads the original delivery before acknowledgement
  Then its structured status remains outcome_unknown and its guidance permits bounded inspection only
  And releasing the original response produces Delivered without a second write
  And terminal delivery has no continued-polling guidance

Scenario: An incomplete acknowledgement never becomes a successful delivery
  Test: native_file_service_uncertainty
  Given an actual native upload or event whose response is truncated
  When the original helper inspects the unresolved receipt
  Then it remains outcome_unknown with no automatic resend or canonical success
