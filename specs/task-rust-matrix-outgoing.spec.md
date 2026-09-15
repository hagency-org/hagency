spec: task
name: "Native authenticated Matrix outgoing custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, matrix, custody]
---

## Intent

Send existing frozen notice and final intents through actual authenticated bounded
HTTPS and owned SDK crypto without losing external outcomes or changing authority.

## Constraints

### Must
- Accept only host-owned configured identity and existing domain claims without external route or crypto proof setters.
- Verify exact authenticated whoami and full current room state before possible writes and persist genuine negative evidence.
- Commit domain Sending before content or key-share HTTP and preserve unknown outcomes after cancellation timeout or lost responses.
- Freeze domain digest separately from exact formatted content and ciphertext digests with stable transaction IDs.
- Persist every accepted HTTP response before domain acknowledgement and recover exact historical acceptance without resending or activating retired tasks.
- Require fresh exact verified recipient device keys including the owned published device before encrypted writes.
- Create a fresh outbound group key per attempt and prohibit reuse by historical recipients or plaintext fallback in encrypted rooms.
- Bound retained journal requests bytes receipts SDK ownership HTTP framing and operation duration without dropping unresolved custody.

### Must Not
- Do not add live server account model deployment or runtime cutover wiring.
- Do not automatically publish keys claim missing Olm sessions establish trust or bootstrap cross-signing.
- Do not persist claim secrets infer NotSent from timeout or silently replay uncertain bytes.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency-matrix/**
- native/hagency-core/src/replies.rs
- native/hagency-core/src/ingress.rs
- native/hagency-store/src/domain/replies.rs
- native/hagency-store/src/domain/notice_custody.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/verified_ingress.rs
- native/hagency-store/tests/replies.rs
- specs/task-rust-matrix-outgoing.spec.md
- knowledge/decisions/adr-059-native-matrix-outgoing.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Domain migrations unrelated production services credential changes live rooms or another worktree.

## Acceptance Criteria

Scenario: Real authenticated plaintext group sends settle exact intents
  Test: native_matrix_outgoing_plain
  Given a current verified group route and frozen notice or final intent
  When an owned collector starts and performs a real local HTTPS send
  Then exact formatted content stable transaction ID and observed event ID settle the original intent
  And notice task activation follows actual acceptance

Scenario: Authenticated encryption uses only the current verified devices
  Test: native_matrix_outgoing_crypto
  Given provisioned real SDK identities and Olm sessions with fresh device query responses
  When a DM or encrypted group message is sent
  Then fresh SDK key shares and ciphertext reach only the exact verified current devices
  And intended recipients decrypt the actual wire content without plaintext fallback

Scenario: Unsafe identities rooms and recipient changes fence writes
  Test: native_matrix_outgoing_scope
  Given a changed identity room generation promotion membership or recipient key
  When preflight or a subsequent write recheck sees the change
  Then old sends cannot transmit or retarget and authenticated negative observations retire old authority

Scenario: Lost acceptance and cancellation retain durable custody
  Test: native_matrix_outgoing_recovery
  Given a possible send accepted response or SDK update interrupted at a controlled boundary
  When callers cancel responses are lost or the owner restarts
  Then exact frozen custody is retained and accepted responses settle without resending
  And uncertain writes remain inspectable rather than becoming NotSent

Scenario: Late historical acceptance cannot restore current authority
  Test: native_matrix_outgoing_domain
  Given exact persisted acceptance with no surviving claim secret or a retired scope
  When the host reconciles the original fence
  Then a narrow content-bound inspection records delivery without activating retired tasks
  And changed receipts or another attempt fence are refused

Scenario: Outgoing work and wire parsing remain finite
  Test: native_matrix_outgoing_bounds
  Given malformed duplicate-key oversized slow or redirected responses and finite journal capacity
  When writes and recovery compete for ownership
  Then accepted work is bounded preserved and never replaced by new work
  And no credentials paths or private content appear in diagnostics

## Out of Scope

Live key upload missing-session claims cross-signing trust establishment production
enrollment live service wiring Matrix media arbitrary room policy automated unknown
write replay and overall migration cutover remain separate qualification gates.
