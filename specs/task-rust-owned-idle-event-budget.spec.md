spec: task
name: "Keep owned turn notification waits inside the operation budget"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, runtime, deadlines, windows]
---

## Intent

Correct the owned Host mapping that gives unsolicited turn notifications the
short RPC response timeout. A tool may remain protocol-silent after turn/start
has replied while still operating within the original finite execution budget.
The original492bc59 Windows file workflow failed with an observed transport
timeout, no pending RPCs and a completed upload. This source defect is tested
independently; that original Windows failure is preserved pending requalification.

## Constraints

### Must
- Keep the existing operation and RPC/write limits and their finite validation unchanged.
- Bound unsolicited event waits by the existing original operation duration; original lifetime and host cancellation still cut them off without renewal.
- Keep pending RPC deadlines, write deadlines, partial frame bounds, authority checks and actual process cleanup enforced.
- Reproduce the premature timeout with an actual quiet owned child before changing production mapping; use no synthetic protocol keepalive.
- Preserve hosted original failures and native skips and qualify the unchanged real file-service executable workflow separately.
- Give the native CI job finite room for the observed near25-minute full check plus subsequent diagnostic and release work; never hide failed steps.

### Must Not
- No larger production operation or response limit, disabled watchdog, automatic retry, new runtime capability or sandbox relaxation.
- No real model, production state or live homeserver testing; no claim that local success qualifies Windows.

## Boundaries

### Allowed Changes
- native/hagency-execution/src/host.rs
- native/hagency-execution/tests/owned.rs
- native/hagency-execution/tests/owned/idle.rs
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- .github/workflows/rust.yml
- specs/task-rust-owned-idle-event-budget.spec.md
- knowledge/decisions/adr-113-native-owned-idle-event-budget.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- No transport session protocol permission approval coordinator Matrix storage or source-root changes.

## Acceptance Criteria

Scenario: A quiet owned turn may finish inside its original operation budget
  Test: native_owned_turn_quiet_notification_wait
  Given a real child which has acknowledged turn start and sends no protocol event for longer than the RPC response limit
  When it emits its terminal answer within the original operation budget
  Then the answer is observed without a transport timeout and actual cleanup remains qualified separately

Scenario: Quiet turns remain bounded by cancellation and the operation deadline
  Test: native_owned_turn_quiet_limits
  Given a real acknowledged turn which stays silent
  When cancellation or the original operation deadline occurs
  Then execution stops and its original attempt remains negative without invented output

Scenario: Pending RPC timeout retains its original diagnostic cause
  Test: native_owned_runtime_failure_observation
  Given an actual runner that never replies to initialize
  When the original response deadline expires
  Then the response remains unknown and the original timeout and pending request count are retained

## Out of Scope

Changing low-level transport timeout semantics, model performance policy or
production qualification. CI timeout increase affects only the hosted job, not
test or runtime deadlines. The current failed hosted run remains failed.
