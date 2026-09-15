spec: task
name: "Connect one owned native child to bounded Codex session IO"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runner, ownership]
---

## Intent

Hand one-use native child pipes from the existing guardian-owned launcher to the
bounded Codex session and verify it with a real offline native fixture.

## Constraints

### Must
- Retain the existing prepare and start boundary and exclusive leader identity through final stop.
- Transfer exactly three private child pipe endpoints and seal every unrelated inherited descriptor.
- Reject malformed ancillary messages wrong descriptor counts and truncation before work starts.
- Close received descriptors on admission failure and apply close-on-exec before any child spawn.
- Bind protocol IO and cancellation to the retained supervisor rather than numeric process IDs.
- Stop the owned scope after protocol termination failure timeout or a dropped operation future.
- Preserve protocol outcome and exact cleanup report as distinct observations.
- Keep POSIX crash-containment refusal and macOS incomplete descendant reporting unchanged.
- Refuse the Windows piped path until cancellable IO is implemented without changing its existing atomic job launch.
- Exercise actual initialize thread start turn start streamed output and completion with an offline native executable.

### Must Not
- Do not launch models enable server execution add command HTTP endpoints or infer effective sandbox permissions.
- Do not complete canonical tasks release leases or infer input acknowledgement from model text.
- Do not create a second launcher or use blocking IO wrappers as proof of bounded asynchronous cancellation.
- Do not edit live services credentials or the original dirty checkout.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency-platform/**
- native/hagency-runtime/**
- specs/task-rust-owned-runner-io.spec.md
- knowledge/decisions/adr-040-native-owned-runner-io.md
- docs/**

## Acceptance Criteria

Scenario: Owned pipes carry one complete offline Codex lifecycle
  Test: native_owned_runner_lifecycle
  Level: integration
  Test Double: offline native app server over actual Unix child pipes with Windows refusal only
  Given a native fixture launched through the retained guardian scope
  When initialize thread start turn start and streamed output traverse child pipes
  Then protocol completion and the observed termination guarantee remain separate

Scenario: Admission and platform guarantees fail before unsafe execution
  Test: native_owned_runner_admission
  Level: integration
  Test Double: offline native executable with invalid launch and unsupported platform requests
  Given an invalid launch failed spawn or unsupported piped platform
  When the host attempts to create the owned session
  Then no child is silently admitted outside the existing ownership boundary

Scenario: IO failure cancellation and noisy output remain bounded
  Test: native_owned_runner_failures
  Level: integration
  Test Double: actual Unix pipe pressure EOF and cancellation with Windows refusal only
  Given a child that closes output blocks input or floods stderr
  When IO fails times out or its host drops a running operation future
  Then retained ownership stops the scope and reports unknown protocol or termination observations

Scenario: Closing protocol streams is distinct from stopping work
  Test: native_owned_runner_custody
  Level: integration
  Test Double: actual Unix child and descendant activity with Windows refusal only
  Given a fixture that keeps running after its IO closes and may have descendants
  When the retained owner explicitly stops the scope
  Then owned work stops without a claim beyond the platform termination guarantee

## Decisions

The lifecycle, IO-failure and descendant scenarios use actual child pipes on
Unix. Their Windows selectors verify explicit Unsupported refusal only. They
do not count as Windows runner behavior or migration parity. Platform ancillary
unit tests separately exercise exact rights ownership and malformed transfers.

## Out of Scope

Windows cancellable piped IO, POSIX guardian-death recovery, effective sandbox
qualification, real models, authenticated dispatch/input acknowledgement,
approval decisions, durable task and lease settlement, and full M4 parity.
