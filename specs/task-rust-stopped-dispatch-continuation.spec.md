spec: task
name: "Explicit receipt-bound continuation after an owned runner failure"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, recovery, console, custody]
---

## Intent

Port the retained TypeScript operator's continue-with-new-instruction behavior
for native failed owners that retained an ADR162 stop receipt. Expose the exact
historical inventory for lifecycle operators and atomically settle that stop
and enqueue a distinct instruction. Preserve the original unknown outcome.

## Constraints

- Both inspection and continuation require the console lifecycle scope and bind
  the addressed agent inside the serialized repository operation.
- Continuation requires the exact original dispatch/fence/receipt digest,
  owned_runner_failure reason, unfinished task, unchanged task/session/resources,
  and a distinct nonempty instruction. Other stop reasons, missing historical
  evidence, stale routes and conflicting custody refuse without mutation. Pending/unknown workspace
  receives, uploads or file delivery cannot be cleared by process-stop proof.
- The operator note records review of workspace and external effects; a stored
  inventory is historical evidence, not a claim of semantic replay safety.
- Reuse the existing stop-settlement kernel inside the recovery transaction.
  Transfer frozen inputs to the replacement; never mark them processed merely
  because the operator continued. Preserve the original task and failed attempt.
- Exact continuation replay returns success even after restart; changed content
  conflicts. Use the existing finite-number payload canonicalization, so equivalent
  numeric representations replay identically and fractional payloads remain valid. No automatic retries, new schema, runner authority, live mutations,
  additional accounts/rooms or service restart are part of this slice.
- Existing orphan recovery continues to refuse every stop-fenced attempt.

## Allowed changes

- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain/conversation_lifecycle.rs
- native/hagency-store/src/domain/stopped_inspection.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/owned_completion.rs
- native/hagency/src/console/agents.rs
- native/hagency/tests/console/agents.rs
- native/hagency-execution/tests/owned/inspection.rs
- knowledge/decisions/adr-164-native-stopped-dispatch-continuation.md
- this spec
- docs/**

## Scenarios

Scenario: Receipt-bound continuation is atomic and replayable
  Test: native_stopped_dispatch_continuation
  Given a started owner failed and retained its original stop inventory
  When a lifecycle operator reviews it and requests a distinct instruction
  Then the stop settles and a replacement becomes claimable with the same task
  And the original unknown outcome is preserved and exact replay survives reopen

Scenario: Invalid or conflicting continuation preserves custody
  Test: native_stopped_dispatch_continuation_refusals
  Given an unresolved stopped dispatch
  When its receipt fence reason task resources instruction or agent do not match
  Then the continuation refuses without clearing custody or enqueuing work

Scenario: Console routes enforce operator scope and exact addressed identity
  Test: native_console_stopped_dispatch_continuation
  Given an original failed owner with a retained stop receipt
  When console inspection and continuation run through the production routes
  Then read-only and foreign-agent requests refuse and the lifecycle owner can continue
  Production caller: hagency::console::agents::continue_stopped_dispatch

Scenario: A real stopped process can be followed by a distinct owned attempt
  Test: native_owned_stopped_continuation_real_process
  Given an actual failed process with observed full cleanup and content inventory
  When its reviewed receipt is continued explicitly
  Then the replacement runs under the original one-runner limit and completes

## Out of scope

TS accept_completed and keep_blocked resolutions, time-limited inspection tokens,
retrofitting missing original-owner proof, UI controls, live recovery and soak.
