spec: task
name: "Bind Windows overlapped stdio to atomic Job Object runner custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runner, windows]
---

## Intent

Replace the unsupported Windows owned-pipe path with bounded local overlapped
IO and exact handle inheritance on the existing atomic job launcher.

## Constraints

### Must
- Assign the existing kill-on-close job before any child code executes.
- Inherit exactly three child pipe handles and no host pipe or job handle.
- Restrict named pipes to local self-connected endpoints with owner access and unpredictable names.
- Bound retained pipe buffers and report writes only after underlying completion.
- Disconnect and cancel pending IO when a stream is dropped without a detached blocking reader.
- Retain one private completion reactor for process lifetime without exposing arbitrary task spawning.
- Retain job ownership through cancellation failure and incomplete termination reports.
- Preserve unknown protocol and termination observations without task or lease settlement.
- Require real Windows CI before claiming Windows runtime validation.

### Must Not
- Do not add another child launcher or weaken the existing POSIX guardian guarantees.
- Do not contact live models or enable native server execution.
- Do not interpret protocol completion as effective sandbox proof or canonical task completion.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency-platform/**
- native/hagency-runtime/**
- knowledge/decisions/adr-044-native-windows-runner-io.md
- specs/task-rust-windows-runner-io.spec.md
- docs/**

## Acceptance Criteria

Scenario: Host runner lifecycle keeps exact process custody
  Test: native_owned_runner_lifecycle
  Level: integration
  Test Double: offline native app server through real child pipes
  Given an owned native child and one bounded host session
  When thread turn and output frames traverse the pipe
  Then output completion remains distinct from the platform termination report

Scenario: Failed admission never substitutes an unowned child
  Test: native_owned_runner_admission
  Given invalid launch settings or a failed native executable
  When the owned launcher refuses startup
  Then the caller receives a failure or uncertain observation

Scenario: IO cancellation and EOF stop through retained ownership
  Test: native_owned_runner_failures
  Level: integration
  Test Double: actual pipe pressure silence and EOF
  Given a child with blocked input or noisy output
  When the host cancels an operation or its deadline expires
  Then protocol state becomes inert and termination keeps its actual platform guarantee

Scenario: Stdio closure does not invent process completion
  Test: native_owned_runner_custody
  Level: integration
  Test Double: actual native child and descendant activity
  Given live work whose pipes are closed
  When the host stops its retained process scope
  Then descendants are checked according to the supported platform guarantee

## Decisions

These shared selectors run on each native OS and replace the former Windows
refusal placeholders. Windows-only handle, job, cancellation and owner-loss
fixtures require the Windows CI host; cross-compilation is not their execution.

## Out of Scope

Real models, actual sandbox qualification, canonical task or lease settlement,
approval authority, POSIX guardian-death recovery, terminal adapters and full M4.
