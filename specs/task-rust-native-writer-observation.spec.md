spec: task
name: "Observe the exact original Windows SQLite writer thread"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, windows, diagnostics]
---

## Intent

Attach native writer identity and bounded CPU accounting to the original observed
domain shutdown, without replacing its verdict or guessing the waiting operation.

## Decisions

Extend accepted ADR106 while retaining ADR099 and ADR088 phase and result meanings.
Original 22c4993 Windows CLOSE entries precede an unobserved backend interval.
Its panic thread IDs name callers, not the domain writers. No production cause
is proven. A query-only owned handle must bind this observation to the actual
writer rather than reopen a potentially reused thread ID.

## Constraints

### Must
- Capture the actual current domain writer before original connection destruction only when that shutdown already has a Probe.
- Retain one query-only owned Windows thread handle and baseline GetThreadTimes reading in that same finite Probe.
- Publish only native process and writer thread IDs and checked kernel/user CPU deltas in the existing snapshot path.
- Freeze the first available snapshot reading including query failure so later work on the same thread cannot change the original native observation.
- Report unobserved unsupported and unavailable states explicitly without changing the shutdown result.
- Preserve original caller-finished timestamps deadlines queue operations connection-before-ownership destruction and acknowledgement after destruction.
- Keep the snapshot Copy and within 256 bytes and keep ordinary unobserved shutdown free of Probe allocation or native observation calls.
- Bound the complete Debug projection to 2048 bytes with maximum-width scalar values and every native state label.
- Retain the original timeout result when the same worker later finishes and report Windows compilation separately from required native execution.

### Must Not
- Do not install global tracing a VFS shim or additional SQLite callbacks queries or configuration.
- Do not add shutdown attempts waits retries workers test serialization thread suspension or privilege changes.
- Do not project paths SQL payloads object names handle addresses or backend text.
- Do not use caller panic thread IDs or reopen a thread by numeric ID for this measurement.
- Do not infer IO mutex sleep or scheduling cause from CPU deltas or qualify original failures as passing.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/shutdown.rs
- native/hagency-store/src/shutdown/native_writer.rs
- native/hagency-store/src/shutdown/native_writer/windows.rs
- native/hagency-store/src/lib.rs
- knowledge/decisions/adr-106-native-sqlite-close-observation.md
- knowledge/context/native-sqlite-writer-observation.md
- specs/task-rust-native-writer-observation.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Native accounting stays bound to the real writer handle
  Test: native_domain_writer_thread_observation
  Level: integration
  Test Double: actual Windows worker thread and query-only native handles or explicit unsupported host
  Given the actual writer captures its current thread before returning its observation
  When another thread snapshots that retained object before and after writer exit
  Then measured native identity remains the original writer with monotonic CPU deltas
  And an actual unavailable native query remains unavailable without invented measurements

Scenario: Original close timeout keeps its exact custody and verdict
  Test: native_domain_shutdown_sqlite_close_entry
  Level: integration
  Test Double: actual owned domain writer held at its original SQLite CLOSE callback
  Given an original shutdown has reached SQLite close
  When the unchanged caller deadline expires before original destruction completes
  Then native observation belongs to the same writer and cannot fabricate connection completion ownership release or acknowledgement
  And later completion does not replace the original timeout result

Scenario: Snapshot growth and missing observations remain bounded
  Test: native_domain_shutdown_snapshot
  Level: unit
  Test Double: existing independent fixed shutdown probes
  Given probes for unrelated and generic custody operations
  When snapshots expose their fixed scalar fields
  Then native writer observations remain absent until captured and the complete snapshot fits within 256 bytes

Scenario: Observation does not alter connection-before-ownership release
  Test: native_domain_shutdown_ownership_drop
  Level: integration
  Test Double: actual owned domain writer held before ownership-file destruction
  Given the original connection has finished destruction
  When the ownership boundary is held beyond the original caller deadline
  Then the original private lock remains held and native accounting adds no acknowledgement or retry authority

Scenario: Queued uncertainty cannot acquire an unrelated writer identity
  Test: native_domain_shutdown_queue
  Level: integration
  Test Double: original bounded queue and held writer
  Given the original shutdown has not reached domain connection destruction
  When its unchanged deadline expires
  Then native writer observation stays unobserved and all original queue and result assertions hold

## Out of Scope

Hosted dispatch process stack capture WCT privilege activation VFS instrumentation
SQLite upgrades production cleanup changes private-directory creation and claims
that CPU accounting identifies the original blocked backend operation.
