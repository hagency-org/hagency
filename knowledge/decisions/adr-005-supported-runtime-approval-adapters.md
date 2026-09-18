---
kind: decision
id: ADR-005
title: "Use only supported runtime approval adapters"
status: Accepted
liveness: auto
tags: [approval, claude, codex, runtime]
---

## Context

Claude Code exposes permission relay through its MCP channel extension. Codex
now exposes a stable `PermissionRequest` command hook that can allow, deny, or
decline a pending native approval. Both are supported runtime boundaries, but
their transport and lifecycle differ.

## Decision

Claude agents use `notifications/claude/channel/permission_request` and return
`notifications/claude/channel/permission` with the original upstream request
id. Because Claude auto mode may hard-deny an external side effect without
opening a permission dialog, the launcher also installs content-scoped `ask`
rules for protected external VCS operations in the agent-local Claude settings.
This keeps the sandbox and auto-mode baseline while ensuring those operations
open a native prompt that the supported channel can relay. Codex agents use a
synchronous `PermissionRequest` hook that submits the
request to hagency, waits for one server-authorized verdict, and writes the
documented `hookSpecificOutput` decision to stdout.

Managed agents are background tmux processes, so a native prompt is not a safe
fallback: it is an invisible blocking state that requires local terminal
intervention. The launcher therefore verifies every required adapter before it
creates tmux. Claude must expose the configured auto-mode channel flags and
agent-local protected-operation ask rules. Codex must meet the minimum supported
version and report the exact session hook through App Server `hooks/list`.

Codex hook trust is persisted only after one explicit local confirmation for
the exact `currentHash` using App Server `config/batchWrite`. The hook command
contains the hook script SHA-256 digest, and its timeout is derived from the
approval TTL plus a transport margin, so code or lifetime changes invalidate
the old configuration hash. Agent-chat never uses the hook-trust bypass and
never writes Codex trust state directly.

Codex project-directory trust is a separate prerequisite handled before App
Server starts. Its updater is idempotent without optional Python TOML modules:
it keeps one exact project table, repairs only byte-equivalent duplicate
tables left by an older launcher, and rejects conflicting duplicates. This
repair never changes hook trust state.

Managed Claude authentication is also explicit. A per-agent runtime-profile API
key is exported when configured; otherwise the launch environment removes an
ambient `ANTHROPIC_API_KEY` before Claude starts. This prevents the operator
shell from silently overriding a valid authenticated subscription with an
unrelated or stale key.

At request time both adapters consume a server-authorized terminal result
before delivering it. A missing, timed-out, malformed, or failed adapter
produces an explicit deny with a diagnostic; it never becomes an implicit allow
and never falls back to a hidden native prompt. The managed launch wrapper also
replaces the tmux pane shell, so exiting a runtime closes the pane instead of
leaving an interactive shell that can reopen Claude or Codex without the
required adapter flags.

The trusted Codex hook handles one narrow non-recursive exception. Exact
hagency MCP coordination and task-control tool names are allowed locally
because they are the runtime's authenticated control plane, not coding
capabilities. Text-only `send_message` and `post` calls are included so a
workflow can report completion without asking permission to ask permission.
Those two calls are not exempt when they contain local file attachments.
Unrelated MCP namespaces, shell commands, patches, and unknown tools continue
through owner approval. The hook verifies its content-bound digest before
applying this policy.

## Consequences

Good, because remote approval no longer depends on terminal keystroke injection
or a home-grown runtime protocol.

Bad, because Claude custom channels still require their research-preview
development allowlist flag, and Codex hook definitions must be explicitly
trusted by Codex before they run. Initial or changed Codex hooks therefore
require an interactive local launch once before later unattended restarts.

Good, because reading an hagency inbox or posting a text workflow result no
longer creates a recursive approval loop. Local file disclosure and coding
operations remain owner-gated.

## Alternatives Considered

- Keep Codex local-only after the official hook became available: rejected
  because it would leave the two supported runtimes with unnecessarily
  different owner workflows.
- Inject yes/no into the terminal: rejected because terminal focus is not an
  authenticated or replay-safe permission protocol.

## Native noninteractive Claude profile (ADR156)

The native stream-json profile may use the official Agent SDK canUseTool stdio
control callback instead of the retained tmux MCP channel. Configure exactly one
approval transport for a process. This is a supported callback transport, not an
additional verdict authority: original private owner/domain custody, current
grant checks, auto-mode baseline and protected-operation ask rules still apply.
Allow preserves the exact original tool input and never installs permission
updates; cancelled requests get no later response. Production native Claude
admission remains refused until the full coordinator and effective launch policy
are integrated and qualified. See ADR156 for the bounded one-shot wire mechanics.
