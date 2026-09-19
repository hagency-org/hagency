spec: task
name: "Qualify successful media through two original configured factory agents"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, fleet, media, executable, offline]
---

## Objective

Exercise successful per-agent file delivery through the actual configured native
service, original factories, native MCP, authenticated media POST and encrypted
room event. Missing-source refusal is not positive media qualification. This is
one prerequisite of full bidirectional Palpo/Robrix fleet validation, not a
replacement for real server/client files, actual model/approval or sustained soaking.

## Constraints

- Extend the genuine same-project two-agent executable fixture. Preserve both
  registration-token and AS-login profiles, two concurrent Started helpers and
  two rounds per agent. Do not seed target domain authority or media receipts.
- Each actual native helper writes different task-bound binary bytes under the
  SAME relative filename in its own original workspace, then calls send_file
  and observes its exact delivery before canonical completion.
- Owner inputs are independently encrypted file events, admitted through the
  original SDK/intake. Each helper lists its actual visible attachments, refuses
  the other agent's exact event, receives its own through authenticated media
  GET and includes those verified bytes in its task-bound output. No attachment
  authority, downloaded destination or Ready receipt may be seeded by the test.
- Require one upload and one encrypted file event per original task. Authenticate
  each with only its physically returned account token; reject cross-agent room,
  media or metadata association. No plaintext file event or plaintext upload.
- The independent recipient's original SDK decrypts the room event. Its upstream
  attachment decoder verifies the uploaded bytes using only that event's actual
  encryption descriptor. Assert exact task-bound bytes, filename, caption, size,
  sender and private DM relation, plus canonical file/dispatch/session binding.
- Successful media must not itself mark the canonical task Done. Only the real
  completion tool and original cleanup may publish the final text reply.
- Preserve all original limits, scoped authority, encryption, custody and strict
  Linux whole-tree qualification. No new dependencies, formatter, live-service
  Cargo tests, permission changes, SDK reset, deployment, commit or PR.

## Allowed changes

- native/hagency/tests/configured_fleet.rs
- native/hagency/tests/configured_fleet/**
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- native/hagency/src/bootstrap/**
- native/hagency/src/file_service/**
- native/hagency/src/file_service.rs
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: Two original configured agents deliver isolated encrypted files repeatedly
  Test: native_configured_fleet_media_two_agents
  Given both genuine factories serve one project through the actual executable
  When each native helper sends task-bound binary bytes from the same relative path
  Then independent recipient SDKs verify distinct exact uploaded bytes and metadata
  And each helper receives its own encrypted owner file and cannot receive the other agent's
  And each original task has one canonical delivered file before its final reply
  And subsequent rounds use original factory custody without re-enrollment
  And readiness and shutdown retain no live leases or unknown operations
