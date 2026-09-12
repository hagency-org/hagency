---
kind: decision
id: ADR-086
title: Observe unrelated process liveness and fresh progress independently
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

A fixed heartbeat sleep cannot distinguish an exited unrelated process from a live process whose filesystem progress has not yet been scheduled.

## Decision

At7cf0dc0 native CI34556196644 the original macOS process_scope target failed
native_process_scope_start_stop at line85. It sampled an unrelated process's
heartbeat file, slept80ms and asserted that the file grew. The log records no
native process state, so it cannot distinguish an exited process from delayed
scheduling or file progress. The original failure is retained; a later passing
test does not establish its historical cause.

Replace that test-only sample with a bounded observation using the existing
OwnedProcess::is_leader_running API. Require both current native liveness and a
fresh heartbeat within three seconds, checking liveness again after observing
growth. An observed exit fails immediately. An alive process with no fresh
heartbeat still fails at the deadline, with a distinct fixed diagnostic. This
uses the same observation bound as the existing child-identity fixture and never
signals, reaps or reconstructs process ownership from a numeric PID.

The real process-scope launch, Unicode argument/environment checks, owned stop,
unchanged cancelled-child heartbeat checks, repeated-stop identity assertion,
spawn refusal and crash-containment assertions remain intact. No production
stop timeout, guardian behavior, ownership or cleanup guarantee changes.

A real pausable native child demonstrates that the old80ms sample can be unchanged
while the owned child is alive, without any cancellation signal. The new helper
requires resumed fresh progress. After actually stopping that child, the helper
must reject its exit even though the old heartbeat file remains. This proves the
fixture distinction; it does not reproduce or explain the historical CI schedule.
Existing probe pause gates are reused without changing the probe executable.

Local Cargo tests and native/Windows GNU compilation are separate from actual
hosted macOS/Windows/Linux execution. No historical failure, skipped or uncertain
result becomes passing evidence, and no live service or model is involved.

## Consequences

The fixture separately requires native liveness and fresh bounded progress, preserving observed exit as failure. The controlled distinction does not establish the historical macOS scheduling cause.

## Alternatives Considered

Counting an unchanged heartbeat as proof of death or a stale file as proof of liveness would confuse separate observations. A later passing run cannot replace the original failed assertion.
