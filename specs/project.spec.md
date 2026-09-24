spec: project
name: "hagency project contract"
tags: [project, security, matrix, agent-runtime]
---

## Intent

Keep hagency's local coding-agent runtime, Matrix bridge, and control plane
safe by default and mechanically verifiable. Durable project truth belongs in
the agent-spec knowledge layer, while each bounded change is implemented from
an executable task contract.

## Constraints

### Must
- User-visible, protocol, authorization, and sandbox changes must have deterministic regression coverage: Vitest for JS/TS, Cargo tests for Rust, and shared behavior vectors at protocol boundaries.
- Durable architecture and security decisions must be recorded under `knowledge/` with stable identifiers.
- Active implementation work must be linked to accepted requirements through `satisfies`.
- Matrix authorization must use the authenticated event sender's complete MXID, never a display name or localpart alone.
- Missing ownership or authorization data must fail closed.

### Must Not
- Plain chat text must not become an execution approval.
- Public project rooms must not expose private approval details or actionable approval controls.
- Direct agent-to-human messages must not be delivered into public or group rooms.
- Tests must not contact Palpo, Claude, Codex, GitHub, or another live external service.

## Decisions

- Existing runtime: Node.js 22 or newer, ESM modules, and Vitest. The authorized native migration uses Rust 2024, Salvo and Cargo tests in an isolated worktree. Existing privacy, authorization and sandbox requirements apply unchanged; development coexistence does not authorize production cutover.
- Specifications live in `specs/`; machine-consumable truth lives in `knowledge/`; explanatory documents live in `docs/`.
- Coding agents launch sandboxed by default: Claude uses `--permission-mode auto`; Codex uses `workspace-write` with `on-request` approval.
- Matrix project rooms use mention-only wake behavior by default.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- ./rust-toolchain.toml
- ./.gitattributes
- native/**
- bin/**
- remote/**
- scripts/**
- services/**
- install/**
- deploy/**
- src/**
- schemas/**
- .github/**
- ./.env.example
- *.service
- *.plist
- *.sh
- AGENTS.md
- CLAUDE.md
- .agent-spec/**
- knowledge/**
- specs/**
- docs/**
- skills/**
- lib/**
- router/**
- tests/**
- mockup/**
- bin/hagency-up
- bin/hagency-up-v1
- bin/hagency-down
- README.md
- README.zh-CN.md
- OPERATIONS.md
- ROADMAP-remote.md
- CHANGELOG.md
- ./NOTICE
- .github/workflows/ci.yml
- bin/hagency-sync-skills
- tsconfig*.json
- scripts/check-router-build.sh
- scripts/run-suite-tests.mjs
- scripts/write-v1-agent-task.js
- scripts/probe-task-maintenance.mjs
- scripts/check-architecture-boundaries.js
- scripts/architecture-boundaries.json
- scripts/verify-ci.sh
- remote/lib/mcp-server-core.js
- remote/lib/push-relay-core.js
- remote/mcp-server.js
- remote/push-relay.js
- install*.sh
- uninstall*.sh
- backend-v2.js
- bridge-matrix.js
- server.js
- ./package.json
- ./package-lock.json
- ./.gitignore

### Forbidden
- Do not commit runtime data, Matrix access tokens, API tokens, or local `.env` files.
- Do not weaken the default coding-agent sandbox or Matrix trust mode to make a test pass.

## Acceptance Criteria

Scenario: Default coding-agent sandbox remains enforced
  Test: Claude defaults to auto-mode
  Given an agent launch command is generated for Claude or Codex
  When the launch policy applies runtime defaults
  Then Claude uses auto permission mode
  And Codex uses workspace-write sandboxing with on-request approval
  Test: Codex Level 2 maps to workspace-write plus on-request

Scenario: Project rooms remain mention gated by default
  Test: matrix default wake is mention-only unless legacy auto is explicitly enabled
  Given no legacy wake override is configured
  When a project room receives an unaddressed human message
  Then the message wakes no coding agent

Scenario: Missing Matrix authorization fails closed
  Test: unknown room with untrusted inviter is untrusted
  Given a Matrix room has no trusted inviter provenance
  When the room invites the bridge bot
  Then the bridge rejects the invitation

## Out of Scope

- Replacing Claude Code or Codex's native sandbox implementation.
- Treating the Robrix2 client as an authorization authority.
