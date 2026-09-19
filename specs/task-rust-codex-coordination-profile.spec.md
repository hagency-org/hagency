spec: task
name: "Opt-in coordination tools for owned Codex dispatches"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL]
tags: [active, rust, mcp, coordination]
---

## Intent

Let an owned Codex dispatch call delegation, task comments and the conversation and
peer-message tools its helper already serves, behind an explicit host option, with
every tool that reaches another session approved by the owner per call.

## Constraints

- Off by default. The option adds exactly the eight ADR180 names to Codex enabled_tools and nothing to the environment. Graph tools stay out. The Claude owned profile is unchanged.
- Pre-approve only comment_task, as ADR-021 does. Never pre-approve a tool that reaches another session and never set a server-wide approval mode.
- Grant no authority: every call still needs the current runner capability fence and lease, and caller and assignee must be active engagements of one fleet and project.
- A delegation from a task minted by a verified agent inbox uses that dispatch's waking entry as its omitted root. A root the dispatch cannot see stays refused.

## Allowed changes

- native/hagency-runtime/src/task_mcp.rs
- native/hagency-runtime/src/codex/session/task_mcp.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/factory.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency-store/src/domain/task_intents.rs
- native/hagency-store/tests/received_files.rs
- specs/task-rust-codex-coordination-profile.spec.md
- specs/task-rust-owned-mcp-launch.spec.md
- knowledge/decisions/adr-180-codex-coordination-profile.md
- docs/**

## Scenarios

Scenario: The host option adds exactly the coordination group and pre-approves only comment_task
  Test: native_task_mcp_coordination_configuration
  Given a task helper profile with and without the coordination option
  When the Codex thread configuration is built
  Then enabled_tools gains the eight names only when the option is on and the environment is unchanged
  And comment_task is the only added pre-approval and no graph tool is enabled

Scenario: A Matrix request delegates from its waking entry
  Test: native_agent_inbox_task_delegates_from_its_waking_entry
  Given a started dispatch minted from a verified agent inbox with an older context entry and a waking entry
  When it delegates without naming a root
  Then the new intent is rooted at the waking entry
  And a root the dispatch cannot see is refused
