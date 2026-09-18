---
kind: decision
id: ADR-163
title: Distinguish proven stopped runner occupancy from unresolved task custody
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-DASHBOARD-RUNNER-PROJECTION, REQ-THREAD-SCOPED-SESSIONS]
---

The current TS path separates runner occupancy from task outcome. Store
`claimDispatchObserved` counts leased/started/parked rows; backend
`pumpRouterDispatches` removes an original live runner after its cleanup callback
and retains an explicit unavailable marker when that cleanup was not confirmed.
Task/workspace quarantine still refuses conflicting writers. A historical failed
task is not itself evidence of a currently running process.

Native claim instead counts every unresolved dispatch globally, even when the
original worker has recorded an exact stopped-owner receipt. This can exhaust
the ordinary one-runner limit after a known-stopped failure and block unrelated
work. The cap now excludes ONLY unresolved attempts with a matching ADR162
receipt. Leased/started/parked and unknown physical owners still count; the cap
itself is not raised. Matching includes both dispatch ID and fence.

This is occupancy accounting, not recovery permission. The failed attempt's
lease, dirty workspace, quarantined session, stop, inputs and task are unchanged;
the entire existing claim eligibility query remains in force. Same-session or
conflicting-workspace work cannot claim. Reopened stores use the original receipt
without reconstructing it. Old failures lacking that proof remain charged.

The TS source also contains an explicit operator outcome-inspection/resolution
flow (`beginOutcomeInspection` / `resolveOutcomeUnknown`, Dashboard and scoped
AgentOps routes). ADR148's original statement that only orphan homes could be
reconciled is not an accurate description of this checkout. The operator-only
trigger, no automatic replay, and native physical-proof obligations remain;
full Rust parity with all three TS resolutions is still owed.

## Verification (2026-09-16)

The new store regression first failed at the expected independent claim before
the occupancy query changed. It now covers missing/wrong-fence proof, reopen,
same-session/workspace exclusion, unchanged original custody and the unchanged
limit for the next live owner. The execution regression runs an actual failed
owner followed by an actual independent process; missing-process proof cannot
free a slot. Its first assertion incorrectly counted the second process's valid
output against the original failure; that assertion is now dispatch-scoped.

Original test handles returned exit0: execution84, store45, strict all-target
store/execution Clippy, and the current TS router build plus router-core/backend88.
Native build handle72030 also returned exit0; `git diff --check` passed.
The first TS build command used a nonexistent router/package.json; the corrected
root `npm run build:router` succeeded before the 88-test rerun. Logs are retained
as stopped-capacity-* in the private local qualification cache.

Read-only live inspection at2026-09-17T03:40:58Z still shows two completed tasks,
the original unknown file attempt and new task004 queued, with no files or
approvals. This source change is not deployed there: the original attempt lacks
the receipt and would still consume the slot. No service restart, state reset,
new fleet, Claude/Octos execution or live E2E/soak acceptance is implied.
