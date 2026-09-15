---
kind: requirement
id: REQ-RUST-MIGRATION-PLAN
title: Document a native Rust Hagency migration using Salvo on three desktop operating systems
status: Accepted
---

## Problem

The operator requests a detailed migration plan for replacing Hagency's deployed
JavaScript/TypeScript runtime with Rust, selects Salvo for HTTP, and requires
native Windows, Linux and macOS support. Most business rules should have one
shared implementation with platform adapters for OS-dependent behavior.

## Requirements

Document the current module inventory, target architecture, phase dependencies,
deliverables, test gates, platform differences, external runtime dependencies,
state transition/cutover strategy, rollback limits, effort and unresolved choices.
Keep required parity separate from deliberately reduced first-milestone scope.

This request authorizes documentation only. The plan must distinguish operator
requirements from proposed design choices and must not claim that Rust support
already exists. It must preserve accepted authorization, privacy, task and
outbound-transport requirements. No implementation, deployment, credential
change, service restart, Git branch change, commit or merge is part of this task.

Browser JavaScript, build-time tooling, dependency languages, CPU architectures
and the supported external Agent runtimes must be addressed explicitly rather
than silently treating a Rust HTTP server as a complete runtime migration.
