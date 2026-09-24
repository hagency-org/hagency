spec: task
name: "Observe the original SQLite connection close entry"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, sqlite, diagnostics]
---

## Intent

Distinguish original SQLite close entry within the unresolved fed7557 Windows
connection-destruction failures. Add bounded observation only and retain the
original failed verdicts and all existing deadlines.

## Constraints

### Must
- Use only the pinned safe rusqlite CLOSE event on the same original observed DomainRepository Connection.
- Associate its static callback with one scoped worker-thread-local original Probe whose guard cannot move across threads or retain a repository.
- Publish only the fixed SqliteCloseEntered timestamp and ignore all callback connection data.
- Preserve existing connection then ownership-file destruction including unwind cleanup and acknowledgement after actual destruction.
- Keep normal shutdown free of hook registration slot access and Probe allocation and retain missing fields for generic custody shutdown.
- Retain original timeout results when later cleanup finishes and keep the snapshot within 224 bytes.
- Enable only the existing rusqlite trace feature without changing versions packages lockfile SQLite flags or unsafe policy.
- Preserve the five original Matrix failure selectors and prove the callback on actual owned writer work.

### Must Not
- Do not add database queries checkpoints production waits retries workers deadlines queue changes or test serialization.
- Do not register SQL statement row profile or global log callbacks or read filenames SQL values identities backend payloads or connection addresses.
- Do not overwrite another scoped callback association fabricate completion acknowledge a lost receiver or infer rollback from a missing phase.
- Do not claim the original failure is fixed by diagnostic success or upgrade SQLite or Windows staging durability in this slice.

## Boundaries

### Allowed Changes
- native/hagency-store/Cargo.toml
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/shutdown.rs
- knowledge/decisions/adr-106-native-sqlite-close-observation.md
- specs/task-rust-sqlite-close-observation.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Actual SQLite close entry cannot fabricate completed destruction
  Level: integration
  Test Double: real owned domain writer paused inside its actual SQLite CLOSE callback
  Test: native_domain_shutdown_sqlite_close_entry
  Given a queued original shutdown of an actual WAL repository
  When its real CLOSE callback is held beyond the unchanged caller deadline
  Then the entry timestamp exists but connection finish ownership release and ACK do not and later actual destructor completion cannot change the original uncertainty

Scenario: Callback custody is exact and expires with its scope
  Level: integration
  Test Double: distinct actual repositories with concurrent and sequential observed closes
  Test: native_domain_shutdown_sqlite_close_association
  Given independent original probes and private repository ownership
  When one close is held while another completes and the scoped thread performs later unrelated cleanup
  Then only each original probe receives its own close entry and no retained association upgrades a later normal close

Scenario: Pre-connection uncertainty stays before SQLite entry
  Level: integration
  Test Double: actual writer held before original connection destruction
  Test: native_domain_shutdown_connection_drop
  Given the original connection-drop boundary
  When the unchanged caller deadline expires before SQLite entry
  Then entry and finish remain absent and only release of the same original worker permits a separate real reopen

Scenario: Ownership release still follows actual connection destruction
  Level: integration
  Test Double: actual writer held before ownership-file drop
  Test: native_domain_shutdown_ownership_drop
  Targets: native/hagency-store/src/domain_worker.rs
  Given completed original SQLite entry and connection destruction
  When the ownership-file boundary is held
  Then the original private lock stays retained and no ACK is fabricated after the original timeout

Scenario: Queue uncertainty cannot claim SQLite entry
  Level: integration
  Test Double: actual held domain writer and original finite queue
  Test: native_domain_shutdown_queue
  Given an original enqueue or pre-pickup wait
  When its unchanged deadline expires
  Then missing close phases remain unobserved and no new operation replaces the original result

Scenario: Existing repository and acknowledgement evidence remains intact
  Level: integration
  Test Double: actual writer held at the original drop and acknowledgement boundaries
  Test: native_domain_shutdown_phases
  Given the original shutdown phase tests
  When the new close entry is recorded or remains absent
  Then all prior field ordering ownership and lost-receiver assertions remain true

Scenario: Snapshots remain finite and domain-specific
  Level: unit
  Test Double: existing fixed snapshot publication fixture
  Test: native_domain_shutdown_snapshot
  Given independent shutdown probes and generic custody phases
  When their published snapshots are read
  Then the new domain phase remains absent unless observed and the complete snapshot stays within 224 bytes

Scenario: Original corrupt-journal refusal keeps its real cleanup verdict
  Level: integration
  Test Double: original encrypted approval journal corruption fixture
  Test: native_matrix_approval_bounds_corrupt_encrypted_journal_cannot_resume_authority
  Given the original nine corruption variants
  When their original shutdown succeeds or fails
  Then the unchanged refusal and shutdown result accompany only actual observed close phases

Scenario: Original ciphertext replay retains its exact authority boundary
  Level: integration
  Test Double: original approval replay and real SDK reopen
  Test: native_matrix_approval_bounds_same_ciphertext_cannot_acquire_new_target_authority_after_reopen
  Given the original ciphertext and target-authority assertions
  When original cleanup runs
  Then no observation changes receipt authority or the original result

Scenario: Original framing refusal retains its original SDK setup deadline
  Level: integration
  Test Double: original approval framing and body-limit cases
  Test: native_matrix_approval_bounds_sync_framing_and_body_limits_do_not_advance_crypto_cursor
  Given the original four framing variants
  When their original SDK opens and cleanup runs
  Then setup errors and cursor assertions remain unchanged without claiming the SQLite close marker diagnoses SDK initialization

Scenario: Original journal rollback preserves exact receipt recovery
  Level: integration
  Test Double: original SDK journal abort and historical domain receipt
  Test: native_matrix_approval_recovery_ack_journal_rollback_requires_reopen_and_exact_receipt
  Given the original failed journal acknowledgement and reopened receipt
  When actual cleanup runs
  Then recovery assertions and the original shutdown verdict remain unchanged

Scenario: Original private rotation preserves historical settlement only
  Level: integration
  Test Double: original lost commit and private approval rotation
  Test: native_matrix_approval_recovery_lost_commit_reopens_exact_receipt_after_private_rotation
  Given the original owner identity and historical receipt
  When original setup recovery and cleanup run
  Then no close observation changes authority deadlines or the original result

## Decisions

ADR106 is accepted for the exact eight-path implementation. All twelve bound
selectors remain required. Strict lifecycle uses the complete actual change manifest with --code . because acceptance spans store and Matrix.

## Out of Scope

SDK initialization instrumentation native stack capture SQLite dependency upgrade
production cleanup policy changes hosted workflow reruns Windows staging
qualification and claims of a proven original backend cause.
