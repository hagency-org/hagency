spec: task
name: "Preserve settled Matrix send proofs beyond the live receipt cache"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, matrix, custody]
---

## Intent

Remove the permanent stop after 64 delivered messages without dropping settled
send evidence or relaxing uncertain-send recovery. Reuse the private authenticated
receipt trie; live memory remains finite and historical queries stay bounded.

## Constraints

### Must
- Keep at most 64 live outgoing receipts and one original pending attempt.
- Archive only settled receipts, never pending/possible/Complete custody.
- Preserve original kind, intent ID, fence and complete accepted-attempt digest.
- Bind nodes to the exact SDK identity and protected journal root, with existing authenticated-node and 256-branch limits.
- Persist immutable nodes before atomically publishing archive root, cache removal and new Prepared/settled state in the ordinary encrypted journal.
- Roll back cache/root memory on failed publication and retain the poisoned original owner; uncertain receipt loss must require inspection/reopen.
- Query exact historical IDs/fences without loading all history. Prevent reuse across kinds as required by restore's existing ID/fence uniqueness rule.
- Keep final/notice replay factual and non-executing; historical proof grants no current route, task activation, model or HTTP authority.
- Reject a conflicting present domain row before factual final replay; protected SDK history still proves past acceptance after canonical-row retention. Notice replay requires its existing already-Delivered row. Newly claimed/re-admitted rows cannot borrow historical delivery.
- Resolve a retained file-publication job from its exact protected settled receipt and already-Delivered domain acceptance, including after that receipt leaves the live cache.
- Keep original current-send preflight, domain begin, per-write checks, ciphertext/recipient binding and private diagnostics.
- Use actual offline owned SDK/local TLS and domain fixtures; no tests contact external services.

### Must Not
- Do not raise cache limits, evict replay coverage, replay uncertain PUTs, synthesize acceptance, settle first file Delivered from a compact receipt, or clear unknown leases.
- Do not treat a finite-cache rollover proof as physical disk retention, fleet/API parity, sandbox qualification or a successful real soak.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/replies.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/sdk/outgoing.rs
- native/hagency-matrix/src/sdk/sync_history.rs
- native/hagency-matrix/src/outgoing.rs
- native/hagency-matrix/src/outgoing/state.rs
- native/hagency-matrix/src/upload/publication.rs
- native/hagency-matrix/tests/outgoing/**
- native/hagency-matrix/tests/file_publication/**
- knowledge/decisions/adr-059-native-matrix-outgoing.md
- knowledge/decisions/adr-152-native-outgoing-receipt-history.md
- specs/task-rust-matrix-outgoing.spec.md
- docs/progress.md

## Acceptance Criteria

Scenario: More than 64 genuine sends preserve oldest replay after restart
  Test: native_matrix_outgoing_history_rollover
  Level: integration
  Test Double: real owned encrypted SDK/domain SQLite and scripted local TLS peer
  Given 150 separately accepted final sends and their canonical delivery rows
  When settled receipts roll into protected history and the owner restarts
  Then the live cache stays at 64, oldest exact replay sends nothing and new sends remain possible

Scenario: Missing proof or failed root publication cannot rearm a send
  Test: native_matrix_outgoing_history_negative
  Given malformed/missing historical nodes, interrupted cache-root publication or a newly claimed domain row reusing a historical key
  When the original owner queries or prepares a new send
  Then original cache/custody remains protected and no domain begin or HTTP write is authorized

Scenario: Historical file receipt only releases original already-delivered custody
  Test: native_file_publication_historical
  Given an original retained file-publication job and its exact compact settled receipt
  When historical recovery verifies the original already-Delivered domain acceptance
  Then no new network effect or first Delivered write occurs

## Out of Scope

Approval/upload history rollover, physical disk quota/retention, general unknown
effect recovery, live deployment and entire-port qualification remain separate work.
