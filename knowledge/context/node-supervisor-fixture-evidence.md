---
kind: context
id: CTX-NODE-SUPERVISOR-FIXTURE-EVIDENCE
title: "Local supervisor fixture port custody and startup evidence"
status: Observed
tags: [node, supervisor, regression, ci]
---

On 2026-09-10, Node CI run 34531716948/job 103053818992 at 88534b0
failed only the supervisor startup event wait: the healthy assertions passed,
then `rows.length === 4` did not become true within 3000 ms. The overall result
was 1 failed/4283 passed/1 skipped. The log did not contain event rows, child logs,
selected ports or restart counts. Its historical cause remains unknown.

The fixture selected and released the backend's ephemeral port before selecting
the dashboard's. The kernel was therefore allowed to return the same port twice.
A controlled offline reproduction supplied that same port to both real children.
The actual supervisor reported `ok:true` after 236 ms, with dashboard restart 1.
After 3237 ms there were exactly three ready events, dashboard restart 6 and an
`EADDRINUSE` in its child log. The dashboard TCP probe could reach the backend
while its own child failed to listen. This demonstrates a concrete fixture
defect and the observed failure shape, not attribution of the old CI failure.

The fixture now retains both listeners through selection and closes all of them
in `finally`. Real competing binds prove both remain reserved simultaneously;
both become available after success and after a controlled callback failure.
There remains a handoff interval after release during which another process
could acquire a port. No production listener/health/lease behavior was changed.

The original four-row and 3000 ms conditions remain. On failure the test emits a
maximum 16-row projection, total/omitted counts, fixed four service restart/exit
fields, and recognized error codes from at most 2048 bytes of each child log.
It never emits raw logs, environment, arbitrary event fields or workspace paths.
A fifth real stopped event still fails; stopped/restarted events are not filtered
away. Malformed/unreadable event logs produce a bounded generic diagnostic.

Each fixture still appends one short JSON line per ready/stopped event. The old
failure contains no truncated JSON or append error evidence; append atomicity
has not been identified as the cause and that mechanism remains unchanged.
The existing source starts services sequentially by their configured probes;
process probes can observe wrappers before their children record readiness.

Evidence is retained outside the repository under the operator's migration cache
for 2026-09-10: `ci-88534b0-node-failed.log`, `node-supervisor-baseline.log`,
`node-supervisor-port-repro.mjs`/`.log`, and `node-supervisor-fixed.log`.
Unmodified local baseline 8/8 passed in 3.55 s; the expanded focused suite 12/12
passed in 4.12 s. These are local checks, not a replacement CI verdict. Task
parse/lint pass with quality 100%; all five scenario selectors match the actual
12-test Vitest inventory. Syntax and whitespace checks pass.

Agent-spec 1.4 lifecycle remains non-passing: the root invocation with
`--layers lint,boundary` nevertheless launched Cargo for the Node selector and
was stopped (exit 143); targeting the actual `tests` directory exited with
`failed to run cargo test: No such file or directory`. Both attempts are retained.
No boundary/lifecycle pass or Node behavioral skip is fabricated. Only this
worktree's generated native target is cleaned after confirming its compilers
stopped; no source, running service or other worktree is touched.
