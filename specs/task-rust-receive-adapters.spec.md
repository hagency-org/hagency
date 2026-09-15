spec: task
name: "Expose current scoped receive-file discovery through native HTTP and MCP"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, attachments, mcp]
---

## Intent

Connect accepted ADR105 receive-service operations to the actual authenticated
runner API and native task MCP. This adapter partition leaves SDK, durable cache
and physical sink ownership with the original service; it adds no such authority.

## Constraints

### Must
- Require current runner authentication for both attachment discovery and receive, with no historical cache-path exemption.
- Accept only an event ID for receive and bounded after/limit selectors for discovery.
- Preserve the service's synchronous original-job handoff before waiting and never cancel or retry admitted work on HTTP caller loss.
- Bound request bodies to16KiB receive responses to4096bytes and discovery responses to16KiB and16items.
- Validate closed response fields selected event ordered sequences metadata hashes and generated workspace-relative path before exposing them to MCP.
- Keep private URL descriptor room capability or SDK information out of public responses and errors.
- Present only list_received_files and receive_file under the separate host-provisioned receive-tools option; default and send-only catalogs remain unchanged.
- Reserve the presentation environment marker against arbitrary Host environment injection and preserve actual sandbox and approval configuration.
- Mark metadata and attachment content as untrusted user input in tool descriptions and native runtime guidance.
- Preserve five-second task-client deadlines complete framing authentication and no-redirect/no-retry behavior.

### Must Not
- Do not create another store collector receive queue physical write owner or proof setter inside an adapter.
- Do not treat presentation markers response-shaped fixture data or historical records as current permission to read a real file.
- Do not claim complete incoming runtime workflow or production activation from these adapter checks.

## Boundaries

### Allowed Changes
- native/hagency/src/lib.rs
- native/hagency/src/runner.rs
- native/hagency/src/runner/received.rs
- native/hagency/src/task_client.rs
- native/hagency/src/task_client/received.rs
- native/hagency/src/task_client/transport.rs
- native/hagency/src/mcp.rs
- native/hagency/src/mcp/catalog.rs
- native/hagency/src/mcp/receive_catalog.rs
- native/hagency-execution/src/host.rs
- native/hagency-runtime/src/codex/session/task_mcp.rs
- native/hagency/tests/mcp.rs
- native/hagency/tests/task_client/mcp.rs
- native/hagency/tests/runner.rs
- specs/task-rust-receive-adapters.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Receive service implementation bootstrap and execution sink belong to separate active contracts.
- No dependency schema crypto trust process deadline or default sandbox changes.

## Acceptance Criteria

Scenario: Receive tools require exact separate presentation opt-in
  Level: integration
  Test Double: actual native MCP child stdio with fixed invalid arguments
  Test: native_mcp_receive_presentation
  Given default send-only receive-only and combined helper presentation profiles
  When the helper lists tools or receives malformed selectors
  Then only the selected exact tools appear and untrusted fields refuse without network work
  And invalid environment markers fail without disclosing their contents

Scenario: Closed receive transport rejects unsafe or mismatched responses
  Level: integration
  Test Double: actual native MCP process and scripted loopback HTTP peer
  Test: native_mcp_receive_transport
  Given an original inherited task context and bounded receive or discovery call
  When the peer returns valid malformed mismatched oversized or redirected responses
  Then only closed safe selected data is returned and invalid private data never escapes
  And the same original credential is used without retry or replacement request

Scenario: Both receive routes require current authorization
  Level: integration
  Test Double: actual Salvo router and original canonical domain writer
  Test: native_runner_receive_current_authority
  Given missing invalid expired or current runner credentials
  When discovery or receive reaches the native runner route
  Then current credentials are checked before receive-service access and stale authority cannot use historical access

Scenario: Native runtime carries exact receive tools without permission changes
  Level: integration
  Test Double: actual native Host profile and serialized Codex configuration
  Test: native_receive_tools_host_profile
  Given host-created helper and receive option
  When runtime settings are prepared
  Then the fixed marker cannot be injected through an arbitrary environment and requires the native helper

Scenario: Receive runtime catalog preserves sandbox and tool boundaries
  Level: integration
  Test Double: actual typed Codex thread and turn requests
  Test: native_task_mcp_receive_configuration
  Given default send-only receive-only and combined profiles
  When typed runtime configuration is serialized
  Then exact enabled tools and inherited marker agree and sandbox approval and network defaults remain unchanged

## Out of Scope

Real incoming SDK ingress selected context verified GET original workspace bytes
and MCP receive end-to-end are mandatory separate acceptance. These adapters do
not prove physical cache safety runtime sandbox qualification or migration cutover.
