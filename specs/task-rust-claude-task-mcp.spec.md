spec: task
name: "Bind the native scoped task helper to the original initialized Claude process"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, claude, mcp]
---

## Intent

Implement Claude's missing native task/file tool bridge, including delayed helper
startup for the existing retained task-context ABI. Do not enable production Host
before original authorization, account and platform ownership gates are complete.

## Constraints

### Must
- Keep a shared validated fixed helper descriptor and unchanged Codex wire output.
- Match TS launch policy: auto with a workspace write lease, plan without one;
  preserve Bash(gh *) and Bash(git push *) ask rules and exact maintenance allows.
- Use the model-name grammar from lib/claude-thread-runtime.js.
- Start with strict empty MCP configuration; bind one native stdio helper once
  while the same initialized driver is Ready, before its sole task prompt.
- Verify empty preflight inventory, exact added-server acknowledgement and
  connected exact tool inventory under one original deadline before prompting.
- Keep dispatch credentials in inherited context, never control JSON or argv.
- Require the helper's owned-task profile to enforce the same restricted catalog
  in both tools/list and tools/call, including optional file tool switches.
- Preserve independent current capability/task/fence/lease checks on every call.
- Close/stop original ownership on failure or dropped started binding future.
- Keep missing/lost binding outcomes uncertain, without reconnect or replacement.
- Test with actual native helper pipes and a real loopback domain service only.

### Must Not
- Do not bypass permissions, trust model-supplied completion, add generic MCP
  setters or raw tool callers, import credentials, or run providers in Cargo tests.
- Do not treat connected tools as sandbox, account, owner-grant, process cleanup,
  model execution or full Matrix/Robrix qualification evidence.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/lib.rs
- native/hagency-runtime/src/task_mcp.rs
- native/hagency-runtime/src/codex/session/task_mcp.rs
- native/hagency-runtime/src/claude.rs
- native/hagency-runtime/src/claude/task_mcp.rs
- native/hagency-runtime/src/claude/session.rs
- native/hagency-runtime/src/claude/session/task_mcp.rs
- native/hagency-runtime/src/owned/claude.rs
- native/hagency-runtime/tests/claude_task_mcp.rs
- native/hagency-runtime/examples/claude_task_mcp_probe.rs
- native/hagency/src/main.rs
- native/hagency/src/mcp.rs
- native/hagency/src/mcp/stdio.rs
- native/hagency/tests/owned_mcp.rs
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- native/hagency/tests/fixtures/claude_mcp_peer.rs
- knowledge/decisions/adr-158-native-claude-task-mcp.md
- specs/task-rust-claude-task-mcp.spec.md
- docs/progress.md
- docs/agent-knowledge.md
- docs/plan.md
- docs/design/native-execution-parity.md

## Acceptance Criteria

Scenario: Fixed Claude launch and helper profile grant no broad permissions
  Test: native_claude_task_mcp_profile
  Given task IDs paths optional file tools and injection-shaped inputs
  When fixed launch arguments and server configuration are generated
  Then only exact maintenance allow rules and bounded literal native argv exist

Scenario: Original Ready session binds once before prompting
  Test: native_claude_task_mcp_binding
  Given native streams with empty inventory exact acknowledgements and connected tools
  When one typed helper binds and a prompt is written
  Then all requests correlate and guidance names the exact task without credentials

Scenario: Native Claude launch preserves the working TS permission contract
  Test: native_claude_launch_ts_parity
  Given the TS launch function and configuration/model regression cases
  When write-lease and read-only dispatch arguments are generated
  Then auto and plan are selected respectively and gh and git push retain ask rules
  And invalid model values refuse using the same grammar as the TS implementation

Scenario: Failed uncertain or cancelled binding cannot rearm
  Test: native_claude_task_mcp_refusals
  Given foreign pending extra missing or malformed inventory and interrupted IO
  When binding checks run under the original deadline
  Then the driver closes without task prompt replacement or replay authority

Scenario: Actual native helper maintains only its original task
  Test: native_claude_owned_task_mcp
  Given actual owned Claude-shaped pipes native helper and scoped loopback API
  When direct or retained context binds after initialization
  Then real task read heartbeat and readback succeed broader tools refuse and no task Done or cleanup authority is invented

Scenario: Operator-only discovery diagnostic cannot claim task execution
  Test: native_claude_task_probe_environment
  Given a pure environment constructor and a synthetic non-authoritative context
  When ordinary tests inspect the explicit local diagnostic profile
  Then no provider process is started and no provider credentials are imported

## Decisions

ADR158 extends ADR057 and ADR154–157. This is production adapter implementation
with offline integration evidence, not production Host enablement or a live soak.
An explicitly activated operator-only no-prompt diagnostic may inspect the actual
installed Claude CLI and native helper catalog. Its synthetic context points to
an owned non-serving loopback socket, never a task service. Connected tools are
not evidence of authentication, dispatch, model work, sandbox or whole-tree stop.
