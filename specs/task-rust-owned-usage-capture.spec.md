spec: task
name: "Capture owned native usage into the exact historical ledger source"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-CONTRIBUTION-CONSOLE]
tags: [active, rust, metering, custody]
---

## Intent

Attribute actual fresh owned-session counters without admitting runtime-supplied
identity, double-counting retries or turning missing observations into zero.

## Constraints

### Must
- Bind the exact acknowledged Started scope before spawning a native child.
- Attach only the private owned operation's fresh thread and actual driver instance.
- Observe all source receipts with contiguous sequence and permanently fence gaps retirement invalidation or foreign sources.
- Preserve one exact pending normalized observation before awaiting the bounded writer and retain it across cancellation or lost responses.
- Retry only the original source call and content without reopening admission or changing UTC growth.
- Keep missing stream evidence explicit and separate capture status from canonical Done cleanup delivery and quota authority.
- Keep normalization outside the writer and use fixed typed counter inputs instead of fabricated transcript text.

### Must Not
- Do not expose a generic resumed-source attachment or restore a historical source onto another process.
- Do not change source receipt capacity discard dedup identities or infer a successful zero observation.
- Do not enable live providers Matrix services HTTP source setters or production runner availability.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency-execution/Cargo.toml
- native/hagency-execution/src/lib.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/usage.rs
- native/hagency-execution/tests/owned.rs
- native/hagency-execution/tests/owned/usage.rs
- native/hagency-execution/tests/support/usage.rs
- native/hagency-execution/tests/support/reply_loss.rs
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- native/hagency-store/src/domain/usage/types.rs
- native/hagency-store/src/domain/usage/reads.rs
- native/hagency/tests/usage.rs
- knowledge/decisions/adr-070-native-owned-usage-capture.md
- specs/task-rust-owned-usage-capture.spec.md
- docs/plan.md
- docs/progress.md
- docs/agent-knowledge.md
- native/README.md

## Acceptance Criteria

Scenario: Actual owned counters acquire immutable attribution before execution
  Test: native_owned_usage_real_capture
  Given a fresh domain and actual offline native app-server pipes
  When Started binds a source before a fresh thread emits cumulative usage
  Then only that execution's ledger grows and repeated or decreasing counters retain exact historical evidence

Scenario: Missing observations do not imply zero usage
  Test: native_owned_usage_missing_and_binding_refusal
  Given a native runner without usage or a full source ledger
  When an owned operation attempts to run
  Then absence remains unknown and refused attribution never launches a child

Scenario: Exact driver sequence prevents foreign or resumed capture
  Test: native_owned_usage_source_guards
  Given actual distinct driver instances with equal textual IDs skipped reads or retirement
  When the private capture sees changed or missing source receipts
  Then it permanently refuses new evidence and exposes no resumed or restored-source constructor

Scenario: Lost acknowledged usage binding never starts a child
  Test: native_owned_usage_lost_binding_never_spawns
  Given a real Started transaction and committed usage source
  When a library-only fixture discards its actual binding response
  Then no child is launched and historical unknown source evidence remains without receipts

Scenario: Counter normalization failure preserves raw bounded evidence
  Test: native_owned_usage_normalization_refusal
  Given actual cumulative categories that exceed the supported combined bound
  When normalization refuses the observation
  Then capture retains its fixed original projection and static failure while ordinary runtime completion remains separate

Scenario: Pending observation custody survives cancellation and lost responses
  Test: native_owned_usage_pending_retry
  Given one exact captured observation and the actual domain writer
  When a wait is cancelled or an acknowledgement is lost before or after commit
  Then the same tuple remains available and an identical retry grows the ledger at most once

Scenario: Restart and capacity preserve historical usage independently of execution
  Test: native_owned_usage_restart_and_capacity
  Given recorded observations an immutable source and exhausted receipt capacity
  When execution ends and durable history is inspected after restart
  Then previous growth and uncertainty remain while new source attachment and quota decisions remain unavailable

Scenario: Operator aggregate evidence covers both observation forms
  Test: native_usage_api
  Given an authenticated operator aggregate request
  When the usage summary includes historical sources
  Then it labels untrusted usage without exposing source identifiers or observation details

Scenario: Actual writer queue and lock retain receipt and clock semantics
  Test: native_usage_worker
  Given the existing bounded writer with controlled queue lock and acknowledgement loss
  When observation commands wait or outlive their caller response
  Then only the original identity can replay its receipt and period growth uses writer lock time

## Out of Scope

Provider-authenticated billing quotas automatic transcript capture resumed sessions
durable pending spool retention pruning runtime approval application live services
production workspace provisioning effective sandbox and release cutover parity.
