---
kind: decision
id: ADR-057
title: "Host-generated native task MCP configuration for one owned dispatch"
status: Accepted
---

# ADR-057: Host-generated native task MCP configuration for one owned dispatch

Status: accepted for the bounded offline integration; service availability remains false.

## Context

An owned native dispatch needs fixed MCP helper configuration derived from its exact host context without exposing private launch values to tool arguments.

## Decision

`hagency-execution::Host::with_task_helper` accepts only a host-selected absolute native
executable and a literal, nonzero loopback socket address. The executable must currently
be a file. The existing protected fixed-directory/executable provisioning precondition
still applies: this is not a race-proof filesystem custody check. Neither this builder,
`TaskMcp`, nor `Host` has Deserialize, Serialize or Debug. There is no HTTP launcher.

After the existing exact dispatch scope is read, `Host::prepare` derives the task ID from
that scope and the private capability from the operation's validated claim. It refuses
prepopulated reserved context names case-insensitively, ambiguous SystemRoot entries,
malformed paths and non-loopback/IPv6 scoped addresses before Started. It validates the
final launch envelope before committing Started. The existing durable-start receipt,
owned guardian/Job launcher and one-worker cancellation custody are unchanged.

Typed runtime settings emit one fixed `mcp_servers.hagency_task_writer` entry with the
native command, arguments `["mcp"]`, exact workspace cwd, serial tool calls, five-second
startup/tool bounds and the three task tools `get_task`, `update_task_execution`, and
`transition_task`. `env_vars` contains only the names `HAGENCY_RUNNER_API_ADDR`,
`HAGENCY_RUNNER_CAPABILITY`, and `HAGENCY_TASK_ID`. Values live only in the private owned
launch environment. Fixed developer instructions identify the assigned task without its
capability. There is no arbitrary helper argv, model-selected task/cwd/endpoint, generic
configuration setter, tool auto-approval override, or permission grant.

The inherited launch environment is available to the trusted runtime and its MCP child.
The generated shell policy has `inherit="none"`, default exclusions enabled and an
explicit `experimental_use_profile=false` field. This does not qualify shell startup files. A host-supplied absolute SystemRoot can be copied as a fixed explicit
shell value for Windows socket/system behavior. This prevents ordinary ambient shell
inheritance of these fresh variables under the pinned implementation; it is **not**
hostile-runtime, OS process-inspection, plugin, hook or sandbox confidentiality. Execution
still requests `on-request`, `workspace-write`, user review and network access false.
Actual sandbox enforcement and owner approval application remain separate gates.

## Pinned upstream evidence

Source was inspected from installed Codex **0.153.4**, tag `rust-v0.153.4`, commit
`3d2ee51ca2d5db578f328aa75e20aa22c0197c9a`. These are implementation-bound semantics,
not a claim about arbitrary future versions or a fake peer's echo:

- [ThreadStartParams](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L62)
  defines optional JSON configuration and `developerInstructions` alongside typed policy.
- [Thread processor](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server/src/request_processors/thread_processor.rs#L1318)
  passes request configuration and typed overrides to the config loader.
  [Config manager](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/app-server/src/config_manager.rs#L219)
  converts JSON values into TOML overrides;
  [dotted path handling](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/config/src/overrides.rs#L9)
  inserts the generated server and shell-policy keys.
- [Raw MCP configuration](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/config/src/mcp_types.rs#L325)
  defines command, args, env_vars, cwd, required/enabled, tool allowlist and timeouts.
  `required=true` is an upstream startup requirement, not proof of successful readiness.
- [MCP environment selection](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/rmcp-client/src/utils.rs#L16)
  combines default allowed variables, explicitly named local env_vars and configured env.
  [Native stdio launcher](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/rmcp-client/src/stdio_server_launcher.rs#L258)
  clears the child environment, inserts that selection, applies cwd/args and creates a
  separate Unix process group. The actual test helper follows that group behavior;
  outer guardian/Job ownership still controls cleanup.
- [Shell populate_env](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/protocol/src/shell_environment.rs#L90)
  starts from an empty map for inherit=None, then applies explicit policy values.
  [Shell policy fields](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/config/src/shell_environment_policy.rs)
  define the exact spellings and profile flag.

Config layers deep-merge tables. This change does **not** establish an exclusive MCP
inventory or disable pre-existing servers, lower-priority per-tool approval settings,
hooks, profiles or workspace configuration. Production must first qualify a protected,
trusted Codex home/configuration and effective tool inventory; service availability stays
false until that and the prior physical-directory/sandbox/platform gates are resolved.

## Canonical completion and retained authority

The existing canonical Done transition increments `execution_epoch`. The frozen owned
fingerprint includes that epoch. This change deliberately does not accept a new epoch or
turn Done into renewed execution, lease-release, final-reply, inspection or retry authority.

After actual owner stop and before negative reconciliation, an MCP-enabled operation
attempts one existing fresh-clock `RunnerCommand::Task` read. The exact task/session must
match. This is an observation-only status in the private report; failed read is None.
It adds at most the existing two-second domain response wait to the conservative 60-second
combined library wait allowance (30-second operation plus bounded startup/stop/retry/drop
and domain receipts). OS scheduling and stalled kernel syscalls are not hard real-time
bounded. Report and unresolved owner/reconciliation custody remain unchanged.

The heartbeat fixture completes an upstream turn while canonical state remains
InProgress. The Done fixture performs a real native helper transition and attempts readback and
helper exit, leaving the disposable runtime without terminal output. The authority check
may stop that runtime immediately after the commit and before its tool response or exit
is observed. Separate atomic stage receipts identify acknowledgement, readback and exit;
absent receipts remain unknown. No test gate delays renewal to force these stages through.
Fresh scope renewal rejects the epoch change, stops the owner and records
`LostAuthority / Protocol::Unknown / canonical Done / Negative(Fenced)`. The dirty workspace
lease remains held and no accepted runner output or final Matrix reply is produced.
This is **not a completed user reply workflow**. A later writer-authorized reporting phase
must distinguish execution revocation from final reporting; simply renewing the Done epoch
would permit the existing runtime to keep calling tools and is not this decision.

## Verification boundary

The separate `hagency-owned-mcp-probe` test binary is not the default CLI (`default-run`
remains `hagency`) and is not in the CI artifact upload allowlist. The actual native fixture consumes generated configuration, creates the real `hagency mcp`
child through stdio, and sends initialize, task read, task heartbeat/Done and readback
against a fresh canonical DomainStore through a local Runner API. The heartbeat path closes helper stdin,
requires successful child exit and empty bounded stderr, then records only a task/exit
receipt. Done may be interrupted by the real epoch fence before that observation. The outer native runtime remains alive until its retained owner stops it.
Requests/receipt/report text are checked for absence of the private capability. Frozen
input deliberately contains impostor task/model/cwd values; those do not set launch identity.

The disposable test peer has capped 64 KiB frames, a single retained bounded-channel
watchdog with a 15-second hard process-exit failure, two-second helper-exit polling and a
1025-byte diagnostic read limit. It does not create detached daemon cleanup workers.
A heartbeat timeout or interrupted exchange fails its mandatory receipt assertion.
For Done, the writer commit, exact negative fence and dirty lease are mandatory; helper
acknowledgement/readback/exit are separately observed or unknown. Missing stage receipts
never count as a successful exchange. Initial runs observed both completed and interrupted
Done exchanges, exposing why task commit cannot guarantee response delivery.

Local macOS execution only qualifies the actual native pipe/MCP/writer exchange. Its
whole-tree cleanup remains unproven and heartbeat settlement is fenced with its lease
retained. Linux and Windows successful settlement branches require actual hosted runs;
cross-compilation alone is not runtime qualification. Existing POSIX crash-containment
flags and all migration availability gates are unchanged. No model, external Matrix
server, production credential or deployed service is used.

## Consequences

Generated configuration preserves existing task, capability and policy checks. Environment forwarding is not hostile-runtime confidentiality, and helper acknowledgement remains separate from canonical Done and cleanup.

## Alternatives Considered

Allowing model-selected executable, cwd or endpoint would break the original dispatch association. Treating required=true or echoed policy settings as successful readiness or sandbox proof would exceed the pinned evidence.
