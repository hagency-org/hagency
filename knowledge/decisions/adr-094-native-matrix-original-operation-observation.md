---
kind: decision
id: ADR-094
title: "Observe the original collector and SDK operation through caller loss"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Original Windows run at 706172d reports 493 printed passes and four Matrix library
failures. ADR088 identifies a crypto prime waiting for sync and the second
manifest batch after its HTTP script completes. Another early OutcomeUnknown
has no label distinguishing prime from main intake; changed-event cleanup fails
at Collector::close, not DomainStore shutdown. These are operation boundaries,
not proof of SQLite, scheduler, SDK or directory-durability causes. Full and
isolated original logs, hashes and source trace are preserved externally under
windows-706172d-*; later automatic diagnostics did not rerun these four cases.

Fresh Windows run at fc57d6b also failed six outgoing library tests. Two fail at original
DomainStore shutdown after Collector close and substantive assertions completed;
four common scripted-driver panics do not distinguish bootstrap, notice intake
or actual send. Their separate original full and first-suite logs are preserved
as native-fc57d6b-windows-full.log and native-fc57d6b-windows-original-cargo.log.
Later serial outgoing and transport diagnostics passed and do not fix this run.

The original Windows run at 5a8f7f7 reports 559 printed passes and four intake
failures. Two original SDK openings last observe StateStoreOpen before their
unchanged ten-second ready-ACK deadline. The second attachment batch picks up
SDK Start promptly but does not return before the same deadline; its HTTP script
then expires while fencing remains pending. The crypto-variant fixture fails at
original Collector close without an attached close trace. Original logs, exact
slices, artifact hashes and failure snapshots are preserved externally in
5a8f7f7-original-ci-evidence.md. The six earlier approval failures at 1da8f1b
passed this later original run; this does not identify or repair their causes.

## Decision

Add private test-only observation to the original work. A fixture opens one
finite trace with a static call-site/variant and optional bounded batch number.
Task-local scope carries that exact trace into existing owned collector tasks;
SDK open and each observed queued command capture its own original trace and
command sequence. No global latest-operation pointer can misattribute a late
completion to the next fixture operation. No new worker, attempt, queue or retry
is introduced. Unobserved production execution compiles observation out.

A fixed recent-event ring records monotonic relative time, fixed phases, command
kind/sequence, bounded event index and redacted Matrix Error variants. Separate
first primary and fencing errors survive ring turnover. Queue admission, worker
pickup, operation return and caller acknowledgement are distinct; pickup may
race ahead of the caller's admission observation. Missing phases never mean no
execution, rollback or ownership release. SDK close separately marks stores,
runtime and lock release before its original acknowledgement. The original
result is never replaced by the snapshot or a later observed completion.

The four intake and six outgoing original fixtures add static operation/variant
scopes around their same work; shared outgoing bootstrap and notice intake use
the same finite observation. The two identified outgoing shutdown calls use the
existing original DomainStore shutdown observer with fixed cleanup labels.
Manifest retains its bounded batch labels. Failure or panic prints
the fixed snapshot. No payload, token, room, event ID, path or backend error text
enters the trace. Existing request scripts, deadlines, original error assertions,
negative authority and cleanup semantics remain unchanged.

For the 5a8f7f7 follow-up, wrap each original crypto-variant Collector close in
its same fixed variant scope. Pass the already-retained SDK Start command trace
into its inner implementation only in test builds. Fixed labels surround the
actual file checks, batch construction, journal writes, SDK sync application,
derivation and quarantine paths. Successful completion labels occur only after
the original operation succeeds. The same command kind and sequence accompany
each label even after its caller disappears; no new operation or trace is
substituted. The existing finite ring and separately retained first errors stay
unchanged. Missing labels remain unobserved, including after ring turnover.

## Consequences

Deterministic tests hold actual SDK/SQLite work or owner lifecycle boundaries,
drop original callers, release the same owner and inspect actual persisted state
and lock availability. They do not merely populate a ring by hand. Another real
wrong-device collector response and unavailable domain writer prove primary
Identity and failed fencing remain separate while the original result remains
OutcomeUnknown. The held gates exist only in these tests. Native local checks
and Windows compilation are separate from a future original hosted run; neither
repairs the already failed 706172d, fc57d6b, 1da8f1b or 5a8f7f7 suite. No timing
or production fix is claimed. The additional deterministic regression holds
actual intake persist/apply boundaries, drops the caller, queues a distinct
read and verifies the original derived batch after reopening the SDK. A real
SQLite trigger refusal preserves OutcomeUnknown and has no successful write
completion label. Neither test establishes the hosted Windows backend cause.

## Alternatives Considered

A global last-operation snapshot could attribute a late SDK completion to a new
caller. A new diagnostic attempt would not observe the original failed work.
Longer deadlines or serial execution would change the measured behavior. None
of these alternatives supplies the required original-operation evidence.
