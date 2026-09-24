spec: task
name: "Observe original Matrix collector and SDK operations without changing results"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, diagnostics]
---

## Intent

Identify the original operation and fixed inner phase behind four Windows706172d
and six Windowsfc57d6b failures, plus four Windows5a8f7f7 failures, while retaining
the original result and operation ownership.

## Constraints

### Must
- Keep each finite observation attached to the original collector task SDK open or queued command across caller loss.
- Distinguish queued started returned and caller acknowledgement phases with monotonic elapsed time.
- Preserve original primary errors separately from fencing errors and preserve every returned result.
- Print only fixed operation variant and phase labels bounded batch or event indices and redacted Error variants.
- Keep observation instrumentation and held-stage gates private to test builds.
- Retain all ten original failing selectors and original CI verdicts as separate evidence; the four Windows5a8f7f7 failures repeat four of those selectors.
- Retain the original SDK Start command and sequence on bounded inner phase observations through caller loss.

### Must Not
- Do not change production or fixture deadlines CI parallelism retry behavior queue capacity or service activation.
- Do not use global last-operation state unbounded traces extra attempts or replacement jobs.
- Do not print payloads credentials identifiers paths rooms or raw backend errors.
- Do not infer original backend causes from successful local checks or later diagnostics.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/intake.rs
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/outgoing.rs
- native/hagency-matrix/tests/outgoing/mod.rs
- native/hagency-matrix/src/collector/observation.rs
- native/hagency-matrix/tests/intake/mod.rs
- native/hagency-matrix/tests/intake/attachments.rs
- native/hagency-matrix/tests/operation_observation/mod.rs
- knowledge/decisions/adr-094-native-matrix-original-operation-observation.md
- specs/task-rust-matrix-original-operation-observation.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Crypto prime failures identify the original fixed variant and phase
  Level: integration
  Test Double: actual encrypted SDK and local TLS fixture
  Test: native_matrix_intake_crypto_verified_human_dm_no_mention_and_spoof_refusal
  Given the original five crypto variants and bootstrap collector
  When that original prime intake or Collector close operation fails
  Then its fixed operation variant and last collector or SDK phase accompany the unchanged failure

Scenario: Concurrent cancellation distinguishes prime from the actual intake
  Level: integration
  Test Double: original local HTTP and real negative room mutation
  Test: native_matrix_intake_handoff_concurrent_cancel_and_negative_room_cannot_admit
  Given original bootstrap and event handoff scripts
  When a collector settles before its script
  Then its original operation is distinguishable and negative admission assertions remain unchanged

Scenario: Manifest intake identifies its original bounded handoff phase
  Level: integration
  Test Double: actual encrypted SDK attachment batches and SQLite domain
  Test: native_matrix_attachment_manifest_bounds
  Given the original two sixty-four event batches
  When observed intake fails after a scripted HTTP response
  Then fixed room domain or SDK phase and bounded event index accompany the original result

Scenario: Changed-event cleanup observes the original Collector close
  Level: integration
  Test Double: real changed-event refusal and original SDK owner close
  Test: native_matrix_intake_handoff_changed_event_cannot_reuse_old_receipt
  Given the unchanged historical event conflict and quarantined status
  When original Collector close completes or fails
  Then domain retirement SDK close and acknowledgement remain distinct from later domain shutdown

Scenario: Dropped callers retain observation on actual queued SDK work
  Level: integration
  Test Double: actual SQLite write held before original SDK completion
  Test: native_matrix_operation_observation_queued_owner
  Given an original SDK mutation and queued command behind a held owner boundary
  When callers disappear before completion
  Then each original finite trace records its own queued started and returned phases while actual committed state remains inspectable

Scenario: Original SDK open and close retain their observed ownership boundaries
  Level: integration
  Test Double: actual SDK owner held at fixed open and close boundaries
  Test: native_matrix_operation_observation_owner_lifecycle
  Given an original opening or closing SDK owner
  When its real thread is held at a recorded boundary
  Then the observed phase and retained lock agree and only the original completion releases ownership

Scenario: Primary refusal and failed fencing remain separate original errors
  Level: integration
  Test Double: real wrong-device HTTP response and unavailable domain writer
  Test: native_matrix_operation_observation_primary_fence
  Given actual identity refusal followed by failed authoritative retirement
  When the original owned collector returns uncertainty
  Then its trace retains both the primary Identity error and the separate fencing failure without changing the returned error

Scenario: Outgoing notice distinguishes bootstrap intake and actual send
  Level: integration
  Test Double: original real SDK local TLS and domain fixture
  Test: native_matrix_outgoing_plain_notice_activates_only_after_real_acceptance
  Given the original outgoing operation and assertions
  When the original operation fails or its HTTP script observes early completion
  Then its fixed callsite and SDK phase accompany the unchanged result

Scenario: Lost outgoing writes identify their original bootstrap send and recovery
  Level: integration
  Test Double: original real SDK local TLS and domain fixture
  Test: native_matrix_outgoing_recovery_lost_http_and_begin_do_not_replay
  Given the original outgoing operation and assertions
  When the original operation fails or its HTTP script observes early completion
  Then its fixed callsite and SDK phase accompany the unchanged result

Scenario: Accepted outgoing recovery identifies its original bootstrap send and resume
  Level: integration
  Test Double: original real SDK local TLS and domain fixture
  Test: native_matrix_outgoing_recovery_accepted_response_survives_busy_lost_domain_reply_and_restart
  Given the original outgoing operation and assertions
  When the original operation fails or its HTTP script observes early completion
  Then its fixed callsite and SDK phase accompany the unchanged result

Scenario: Protected outgoing history refusal identifies original bootstrap send and restore
  Level: integration
  Test Double: original real SDK local TLS and domain fixture
  Test: native_matrix_outgoing_recovery_restore_rejects_inconsistent_protected_history
  Given the original outgoing operation and assertions
  When the original operation fails or its HTTP script observes early completion
  Then its fixed callsite and SDK phase accompany the unchanged result

Scenario: Crypto refusal cleanup observes original domain shutdown
  Level: integration
  Test Double: original outgoing fixture and actual domain shutdown observation
  Test: native_matrix_outgoing_crypto_unverified_missing_or_changed_keys_never_fall_back
  Given substantive original assertions and Collector close have completed
  When the same original DomainStore shutdown fails
  Then its fixed shutdown label and existing shutdown phases retain the original error

Scenario: Uncertain outgoing wire cleanup observes original domain shutdown
  Level: integration
  Test Double: original outgoing fixture and actual domain shutdown observation
  Test: native_matrix_outgoing_bounds_wire_failures_retain_possible_writes
  Given substantive original assertions and Collector close have completed
  When the same original DomainStore shutdown fails
  Then its fixed shutdown label and existing shutdown phases retain the original error

Scenario: Original intake subphases retain command custody after caller loss
  Level: integration
  Test Double: actual local TLS bootstrap encrypted SQLite journal and held original SDK command
  Test: native_matrix_operation_observation_intake_subphases
  Given the original intake command at actual persist and SDK apply boundaries
  When its caller drops while another original read waits and a separate real SQLite trigger refuses a write
  Then each bounded phase remains attached to its original command the completed batch survives reopen and the refusal keeps its original error without a false completion

## Out of Scope

Production timing fixes source-of-delay conclusions new public observability
APIs backend payload logging live hosts deployment and reclassification of any
original Windows failure. Missing phases remain unobserved.
