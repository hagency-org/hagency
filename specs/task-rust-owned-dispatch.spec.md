spec: task
name: "Bind one owned native Codex operation to durable dispatch custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runner, custody]
---

## Intent

Connect exact host dispatch authority to the existing owned native child pipes in
one bounded operation, while keeping production workspace and sandbox qualification open.

## Constraints

### Must
- Read exact frozen dispatch inputs, current task, resource leases and provisioned model settings from the domain writer.
- Commit Started with a fresh writer clock before any child creation; never spawn after a lost start response.
- Retain one worker and process owner across caller cancellation and stop the actual owner before any successful dispatch settlement.
- Preserve historical attempt fencing and dirty leases after failed, cancelled, stale or unknown execution.
- Separate upstream completion, physical cleanup, canonical task state and domain settlement.
- Bound execution, cancellation, domain calls, output and synchronous join observations explicitly.
- Restrict offline fixtures to fixed host-owned directories; resource/path equality is not physical directory custody or sandbox proof.

### Must Not
- Do not accept model-supplied task identity, paths, resource selection or permission overrides as authority.
- Do not mark canonical Done or send Matrix replies from upstream completion.
- Do not enable owner approvals without their existing exact application gate.
- Do not enable a production runner, live models, a scheduler or an arbitrary-command HTTP endpoint.
- Do not clear quarantined or dirty resources through negative observations.

## Boundaries

### Allowed Changes
- native/README.md
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-execution/**
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/owned_dispatch.rs
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- native/hagency-platform/src/lib.rs
- native/hagency/src/runner.rs
- knowledge/decisions/adr-053-native-owned-dispatch.md
- knowledge/decisions/adr-139-native-codex-launch-surface.md
- knowledge/decisions/adr-140-real-codex-sandbox-qualification.md
- specs/task-rust-owned-dispatch.spec.md
- docs/**

## Acceptance Criteria

Scenario: Frozen authority precedes child creation
  Test: native_owned_dispatch_start_scope
  Given a claimed dispatch with a current task and exact frozen inputs
  When the host validates and commits its start
  Then changed or stale scope is refused before workspace code executes

Scenario: Native completion remains distinct from canonical completion
  Test: native_owned_dispatch_real_pipes
  Level: integration
  Test Double: offline native app-server child on fixed host-owned directory
  Given a fresh DomainStore and an actual owned native child
  When the exact turn completes and its owner is stopped
  Then canonical Done is not inferred and uncertain platform cleanup keeps leases quarantined

Scenario: A bounded reply expiry names its side of the writer queue
  Test: native_domain_unknown_reports_dequeue
  Given a parked earlier job with a command still queued and then a command the writer begins
  When the two-second reply bound expires in each position
  Then the dequeue attribution distinguishes running from queued and grants no retry or completion authority

Scenario: Cancellation and lost replies retain custody
  Test: native_owned_dispatch_cancel_and_unknown
  Level: integration
  Test Double: offline native child and explicit expiry/revocation
  Given cancellation expiry or revocation
  When the host operation reconciles its exact historical attempt
  Then no cancelled operation releases a dirty lease or infers canonical completion

Scenario: Lost writer receipts preserve exact durable custody
  Test: native_owned_dispatch_queue_reply_loss
  Level: integration
  Test Double: bounded private reply-channel gates around the actual domain writer
  Given a start command queued or already durably committed
  When its response receiver expires before the gate opens
  Then the unstarted command is skipped and the committed start is fenced without replay

Scenario: Lost committed start receipt never launches a native child
  Test: native_owned_dispatch_lost_receipt_never_spawns
  Level: integration
  Test Double: library-test-only discard of the actual committed start response
  Given a real start transaction and lost response
  When the owned coordinator performs normal reconciliation
  Then no child is spawned and the actual started attempt retains its lease

Scenario: Negative evidence cannot change a replacement attempt
  Test: native_owned_dispatch_negative_historical_fence
  Given an expired or revoked historical capability
  When the host reports an execution failure
  Then only that authenticated attempt can be fenced and a replacement is unchanged

Scenario: Admission and protocol failures cannot complete work
  Test: native_owned_dispatch_admission_and_protocol_failure
  Level: integration
  Test Double: actual native offline child using fixed host paths
  Given wrong workspace scope failed spawn EOF wrong thread or a server approval request
  When the operation terminates
  Then known unstarted work is released and all started failures remain quarantined

Scenario: Database lock failure retains a bounded negative retry
  Test: native_owned_dispatch_database_lock_refuses_before_spawn
  Level: integration
  Test Double: external test-only SQLite writer lock
  Given the existing database busy timeout
  When admission and negative reconciliation cannot acquire the writer lock
  Then no child runs and an explicit retained retry can reconcile the clean unstarted lease

Scenario: Absolute operation deadline stops silent work
  Test: native_owned_dispatch_deadline_stops_and_fences
  Level: integration
  Test Double: actual silent native offline child
  Given a finite absolute host deadline and no terminal protocol response
  When the deadline expires
  Then retained process stop precedes negative fencing and no clean completion is inferred

Scenario: A spawn that outlives the operation budget is abandoned, fenced and never orphaned
  Test: native_owned_dispatch_spawn_outliving_budget_is_fenced
  Level: integration
  Test Double: an unconditional Host flag + builder (with_guardian_prepare_stall), default off, no production caller — its child stalls before exec, budget derived from HAGENCY_OPERATION_BUDGET_MS; no production path sleeps
  Given a finite absolute host deadline and a spawn handshake that does not complete within it
  When the bounded await of the spawn expires at the deadline and the JoinHandle is dropped
  Then the operation returns within the budget plus the checkpoint cadence
  And the attempt is fenced outcome_unknown exactly as today's SpawnFailed path (Negative(Fenced), quarantined, dirty workspace)
  And a late child is stopped and reaped — delivered through the SpawnCustody try-send to the operation's teardown, with the process-group Drop as the backstop — no orphan and no second dispatch shares its resources
  And Failure::SpawnFailed with Cleanup::Unknown{TimedOut} is reported for the uncertain start, never a plain Deadline

Scenario: The Codex argv is exactly the app-server subcommand
  Test: native_codex_argv_is_app_server_only
  Level: integration
  Given a configured host with a validated executable and workspace
  When the host prepares an owned launch
  Then the spawn argv is exactly one argument app-server
  And no sandbox approval or directory flag ever appears on argv because policy travels in the typed initialize request

Scenario: The bare argv still yields stdio on the pinned Codex CLI
  Test: native_codex_stdio_flag_matches_pinned_cli
  Level: integration
  Test Double: the recorded help excerpt at native/fixtures/codex-cli/app-server-help-0.153.4.excerpt.txt
  Given the fixture excerpt captured from the spec-pinned Codex 0.153.4 binary
  When its captured_version header and transport assertions are validated offline
  Then the excerpt records stdio as the --listen default with --stdio an exact synonym
  And a captured_version other than the pinned 0.153.4 or a transport default that is not stdio fails the test never skips

Scenario: A real app-server write inside the sandbox succeeds
  Test: native_codex_real_app_server_sandbox_write_inside
  Level: integration
  Test Double: tracked evidence file written by the operator-run codex_qualify example
  Given the qualification evidence file at its documented path with a pinned codex version
  When the CI test validates its shape pin and freshness on any hosted leg
  Then the write_inside verdict is recorded as passing with a log path and commit under test
  And a missing stale or non-passing evidence file fails the test never skips

Scenario: A real app-server write outside the sandbox is refused
  Test: native_codex_real_app_server_sandbox_refuses_outside
  Level: integration
  Test Double: tracked evidence file written by the operator-run codex_qualify example
  Given the same evidence file and its refuses_outside verdict
  When the CI test validates the refusal record and the pinned codex version
  Then the verdict shows the app-server sandbox error with no file created outside
  And a missing stale or mismatched pin fails the test never skips

Scenario: The typed sandbox policy echoes back through the offline peer
  Test: native_codex_probe_sandbox_policy_echo
  Level: integration
  Test Double: offline app-server fixture peer replaying an initialize response
  Given a typed initialize request with the default workspace-write and on-request approval policy
  When the peer echoes its observed sandbox and approval configuration back
  Then the echo matches the requested policy exactly or the mismatch is named
  And no echo is treated as evidence of effective OS sandboxing

## Decisions

**The existing deadline scenario keeps its verdict.**
`native_owned_dispatch_deadline_stops_and_fences` keeps asserting `Deadline`
for the launched-in-time case — the marker exists, the child ran, and the
budget expired over its own work — and must **not** be widened to accept
`SpawnFailed`. The new scenario above separates the two failure shapes
exactly so the original stays sharp: widening it would hide genuine
`SpawnFailed` classification regressions (the glm5 report's verdict).

## Out of Scope

Live model execution, physical directory provisioning, actual sandbox efficacy,
MCP launch configuration, owner approval application, Matrix delivery, terminal
parity, warm reuse, a global scheduler and full M4 completion.
