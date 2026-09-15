# Herdr unseen idle compatibility

The local U preflight produced a completed native turn and Herdr `done`.
Herdr `pane_agent_status` maps detected Idle to `done` when unseen and `idle`
when seen; client-shell projection uses the same distinction. The existing
control gates compare only `idle`, so a no-focus pane cannot resume or manage
its loop despite complete terminal-only native evidence.

1. Reproduce goal resume and loop create/resume/delete rejection using the
   existing child-process fixture with the actual `done` display value.
2. Accept only `idle` or `done` at the existing display checks; retain original
   goal/loop/process identity, fresh terminal-only turns and pre-send checks.
3. Cover active native turns under `done` and reject working, blocked, unknown
   or missing display values. Run the bound native-control suite and CI checks.
4. Record the interpretation in ADR-033 and the operator skill. Preserve all
   frozen U resources and failed runtime artifacts; this source repair does
   not rerun U or establish full E2E acceptance.

No new dependencies, runtime settings, permission changes or live mutations.
Agent-spec lifecycle skips without an executing Node verifier remain skips;
actual Vitest results are separate evidence.
