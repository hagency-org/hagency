---
kind: requirement
id: REQ-RUST-MIGRATION-EXECUTION
title: Execute the native Hagency migration in an isolated worktree
status: Accepted
---

## Problem

The operator's follow-up, "executre migration in a clean worktree", authorizes
implementation of the Rust migration. This extends REQ-RUST-MIGRATION-PLAN's
earlier documentation-only scope. Use merged master as a pinned baseline and
preserve the original checkout's uncommitted changes and running services.

## Requirements

Use Salvo and shared Rust domain logic. Preserve generic fleet names and current
wire identities. Start with fresh development state; never open live runtime or
crypto stores. Native Windows, macOS and Linux remain release requirements.
Unexecuted platform or crypto checks must remain open gates, not assumed passes.

First implement a runnable native foundation: authenticated loopback HTTP,
validated configuration and protocol values, an exclusively owned durable
repository, bounded work queues, explicit recovery states, and cross-language
canonicalization vectors. Record transaction ownership and phase acceptance.
Continue migrating behavior under bounded task contracts. Existing deployments
may only be replaced after the migration plan's parity and cutover gates.
