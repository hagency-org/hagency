# Native handle closeout repair

The Q5 observer returned exit 1 with `window_missed_checkpoint`, but its middle
agent later reported it as still pending. Keep the actual failed case intact.
Add a read-only summary of that middle agent's persisted tool records before
it writes task closeout. The summary describes observed execution results;
it does not prove current process liveness or successful task acceptance.

1. Add deterministic regressions for an observed failed terminal, missing
   terminal, mixed handles, corrupt evidence and a terminal followed by polling.
2. Add `native-handle-summary.mjs`, reading the existing record format with
   bounded, stable, readonly input. Emit compact JSON without command/output
   bodies. Nonzero exit, missing terminal and invalid evidence remain distinct.
3. Distribute the helper with the inner-loop skill. Require the middle to use
   its exact handle directory when closing a managed monitor/observer.
4. Run the focused tests, CLI checks and exact Q5 saved records. Preserve Q5
   as failed; this read-only repair cannot satisfy interrupted recovery.
5. Prepare a separate full case from the original seed. Its lower task must
   spell out the exact standalone tool invocation expected by the unchanged
   observer, with a preflight assertion binding the task text and matcher.
   Preserve every prior goal/checkpoint and retain all full E2E gates.

No additional framework, dependency, sandbox grant, runtime restart or managed
handle polling is part of this code repair. The existing isolated PR worktree
is the edit/test location. User authorization to fix and autonomously retest
persists; no additional design approval is required for this bounded repair.
