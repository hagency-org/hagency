spec: task
name: "Native scoped MCP coordination tools"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, mcp, tasks, delegation]
---

## Intent

Expose implemented native delegation conversation peer and graph operations to
managed runners through the bounded MCP helper and existing scoped HTTP API.

## Constraints

### Must
- Preserve the existing five task tools and MCP initialization framing watchdog and uncertainty behavior.
- Advertise only implemented typed operations with exact bounded argument schemas.
- Share one private bounded local HTTP transport and enforce per-operation request and response limits.
- Derive actor capability session project and parent authority from inherited host context and the canonical domain.
- Require explicit stable mutation call IDs that reach the backend idempotency record unchanged.
- Preserve exact creator current membership task Done epoch and node result authority.
- Keep lost mutation responses unknown without automatic retries and permit identical content-bound reconnect retries.
- Exercise real native HTTP routes and an rmcp client with synthetic current runner capabilities.

### Must Not
- Do not accept arbitrary URLs paths methods credentials caller identity or host observations from tool arguments.
- Do not mint report grants or expose host inspection approval or process control APIs.
- Do not advertise unimplemented tools or bypass canonical graph readiness through delegation or peer APIs.
- Do not run live services models deployments or real accounts.

## Boundaries

### Allowed Changes
- native/README.md
- native/hagency/src/task_client.rs
- native/hagency/src/task_client/**
- native/hagency/src/mcp.rs
- native/hagency/src/mcp/**
- native/hagency/tests/**
- specs/task-rust-mcp-coordination.spec.md
- knowledge/decisions/adr-051-native-mcp-coordination.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Canonical store schema host observation APIs Matrix SDK runtime execution production JavaScript another checkout and live credentials.

## Acceptance Criteria

Scenario: MCP advertises a strict implemented coordination catalog
  Test: native_mcp_coordination_catalog
  Level: integration
  Test Double: real rmcp child protocol and native API with strict typed parser fixtures
  Given the existing native MCP initialization protocol and task tools
  When a client lists or calls coordination tools
  Then only implemented typed bounded operations are advertised
  And arbitrary caller capability host observation paths and methods are rejected

Scenario: Delegation uses canonical caller task and backend receipt
  Test: native_mcp_coordination_delegation
  Level: integration
  Test Double: local native API with real SQLite and synthetic capabilities
  Given a current task runner and eligible project worker
  When stable delegation calls are repeated changed or submitted across scope
  Then one canonical child is admitted and exact replay is preserved
  And wrong project stale capability or parent identity cannot acquire authority

Scenario: Internal conversations and peer work remain member and creator scoped
  Test: native_mcp_coordination_conversation
  Level: integration
  Test Double: native HTTP API and rmcp child client
  Given current participants and an internal conversation
  When the creator changes membership peers exchange messages or another session attempts control
  Then exact recipient sessions receive bounded durable input and only the current creator changes scope
  And nonmembers and removed sessions cannot read or send through old authority

Scenario: Graph results retain canonical readiness and exact node scope
  Test: native_mcp_coordination_graph
  Level: integration
  Test Double: native HTTP API and rmcp child client with host-driven dispatch fixtures
  Given canonical graph node tasks and dependencies
  When runners report results read dependencies or cancel work
  Then only current exact completed node epochs produce dependency results
  And wrong nodes premature results graph readiness bypass and noncreator cancellation are refused

Scenario: Lost responses remain uncertain and reconnect retries remain identical
  Test: native_mcp_coordination_recovery
  Level: integration
  Test Double: local forwarding fault fixture with real domain commits
  Given an accepted mutation with a lost or malformed response
  When the MCP helper reconnects and retries the same explicit call ID
  Then original content and backend receipt determine replay without duplicate work
  And changed content or stale actor is refused rather than retried automatically

Scenario: Coordination requests and projections remain bounded
  Test: native_mcp_coordination_bounds
  Level: integration
  Test Double: real rmcp child and scripted bounded local HTTP responses
  Given invalid nested arguments and oversized or delayed local responses
  When the helper performs read or mutation transport
  Then request response frame and deadline limits fail visibly without leaking credentials
  And existing task protocol and watchdog behavior remain enforced

Scenario: Each helper refusal names its own class
  Test: native_mcp_helper_framing_is_named
  Level: integration
  Test Double: the real helper spawned directly on its raw stdio
  Given a partial frame whose peer write half closes before the newline
  When the helper reaches end of input
  Then it exits with the framing code and reports the bound and the observed size

Scenario: A lifecycle refusal keeps its own code
  Test: native_mcp_helper_protocol_is_named
  Level: integration
  Test Double: the real helper spawned directly on its raw stdio
  Given a well-formed frame outside the current MCP lifecycle
  When the helper reads it
  Then it exits with the protocol code and names the refusing check

## Out of Scope

This slice does not complete MCP parity or M3/M6. Agent discovery files Matrix
history progress hooks host recovery runtime dispatch and approval tools remain
unadvertised unless separately implemented. The existing task tools stay exact
to the assigned task; coordination never broadens their mutation scope.
