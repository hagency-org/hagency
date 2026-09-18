---
kind: decision
id: ADR-158
title: Bind one scoped native task MCP helper after original Claude initialization
status: Accepted
---

## Context

The native Claude session has owned pipes, one-shot permissions and typed usage,
but no task-maintenance bridge. A retained prestarted process cannot receive a
future dispatch secret by changing its inherited environment. ADR057 already
defines a private create-only context reference and the native helper startup ABI.

## Decision

Use the official SDK0.3.270 wire boundary matching installed CLI2.1.270:
https://unpkg.com/@anthropic-ai/claude-agent-sdk@0.3.270/sdk.d.ts
and https://code.claude.com/docs/en/agent-sdk/mcp . The SDK defines mcp_status and
mcp_set_servers, an added/removed/errors acknowledgement and per-server connected
status/tool inventory. No SDK dependency or JavaScript bridge is introduced.

The fixed launch profile keeps stdio permissions, an empty strict MCP
configuration, empty filesystem setting sources, hooks disabled, no slash-command
skills, Chrome or session persistence. Only the four existing native maintenance
tool names receive explicit allow rules; no wildcard, Bash, optional file tool or
server-wide allow is added. This does not prove effective sandbox or containment.
Follow the existing TS launch contract, not a new permission policy:
backend-v2.js::claudeThreadSessionArgs selects auto for a write-lease dispatch and
plan otherwise. lib/claude-thread-runtime.js::prepareClaudeThreadRuntime installs
Bash(gh *) and Bash(git push *) ask rules; its claudeThreadModel accepts only a
1–64 character ASCII alphanumeric-first model with dots, underscores or hyphens.
Native argument generation preserves all three behaviors, with regression cases
linked to tests/claude-thread-runtime.test.js. The baseline public arguments
function still defaults to auto; the task profile requires explicit may_write.

One typed helper can bind only while the original initialized driver is Ready.
Under one unchanged deadline: require empty server inventory; send exactly one
stdio server configuration; require exactly that server added, none removed and
no connection errors; require connected status with the exact expected tool set.
No retry, reconnect, reconfiguration or arbitrary server map is exposed. The
original operation guard closes the session and asks its retained owner to stop
on errors, uncertain IO or cancelled waits. A received acknowledgement alone is
not the final connected observation. Private protocol text is never diagnostic.

The descriptor shares validation, maintenance names and task guidance with Codex,
whose emitted configuration and policy remain unchanged. Claude's helper command
is the fixed native executable plus mcp --owned-task-profile. Inherited context
contains the existing direct capability or retained private reference; neither
appears in MCP configuration, prompt, argv or receipts. Optional file flags are
presentation switches, not file or route authority. The native profile filters
both advertisement and invocation to the same four task tools and enabled file
pairs. Current task/capability/fence/lease checks still occur in the real API.
Normal native MCP retains its existing wider coordination catalog unchanged.

After successful binding, bounded fixed guidance prefixes the original prompt.
It identifies the host-selected task and original completion/file semantics but
grants no authority. Oversized combined input refuses before any prompt bytes.
Received messages after that use the existing original source/usage sequence.

## Consequences

Offline native guardian pipes plus the actual native MCP executable and loopback
domain service prove context handoff and task maintenance. They do not prove a
real Claude tool choice, provider login, owner decision or Matrix delivery. Full
Host/approval/account/platform integration and local three-runner qualification
remain required. Production Claude UnsupportedRunner refusal stays intact.

An explicit operator-only example checks actual local CLI compatibility without
a model prompt or tool invocation. It supplies a synthetic, non-authoritative
context pointing to a held non-serving loopback socket, uses a fresh private
empty cwd and bounded original owner, and prints only fixed status/cleanup facts.
It does not qualify an account, dispatch, model task or soak. Its ordinary Cargo
test covers pure environment construction only. No provider credentials are
read or exported. Inherited process context is not isolation from Bash or other
same-user processes; full production environment/sandbox qualification remains.

## Alternatives Considered

Launching the helper before a retained context exists fails startup. Passing the
secret in protocol JSON or model arguments loses private context custody. A broad
MCP allow or relying only on tools/list filtering enlarges the existing profile.
