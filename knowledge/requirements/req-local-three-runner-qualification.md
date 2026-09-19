---
kind: requirement
id: REQ-LOCAL-THREE-RUNNER-QUALIFICATION
title: Qualify local Codex Claude Code and OctosCode against mini3 Palpo and Robrix
status: Accepted
---

## Source

Operator instruction on 2026-09-16: "use local codex not mini3's", followed by
"use local codex, claude code and octoscode". This refines the execution topology
of REQ-RUST-MIGRATION-EXECUTION and its active end-to-end/soak objective; it does
not narrow the migration's parity or safety gates.

## Requirements

- Run the coding tools on the operator's local Mac: Codex, Claude Code and
  OctosCode backed by the local real Octos runtime. mini3 supplies Palpo; Robrix
  remains the real Matrix client. Do not silently replace local execution with
  mini3-hosted or container-hosted model runners.
- Use the installed local tools and pin their actual versions/executable facts.
  Keep operator credentials with their provider tools. No token copying,
  credential export, shared arbitrary coordinator HOME or fabricated managed
  account readiness is authorized by this topology choice.
- Complete native Rust production adapters and their actual dispatch, permission,
  scoped task/file tools, completion, usage and process ownership integration for
  each runner. A successful standalone CLI or a retained JavaScript bridge is not
  proof of the Rust port working.
- Preserve default runner sandbox policy and private owner approval. A missing
  native macOS descendant-stop proof remains unproven; do not relabel leader exit
  as whole-tree cleanup to enable a passing local soak.
- Distinguish the OctosCode client from its Octos execution backend. Explicitly
  exercise real protocol mode; mock mode or a version/help command never counts
  as an executed task. Verify installed protocol behavior rather than assuming
  the current source checkout matches an older installed binary.
- Qualify the existing shared-room/separate-DM, encryption, file transfer,
  attribution, approval, recovery and sustained-soak requirements on the new
  topology. Report evidence per runner and preserve prior gen10/gen11 results as
  historical remote-run evidence, not local three-runner acceptance.
- Preserve existing live services, user checkouts and SDK stores. The previously
  prepared fresh mini3 account is unused, not authenticated or a cleanup proof;
  do not continue asking the operator to sign in there.

## Current evidence and open work

The 2026-09-16 read-only inventory found local Codex0.154.0, Claude Code2.1.270,
OctosCode0.3.0-rc.9 and its Octos2.0.3-rc.11 backend. Provider-owned status commands
reported Codex/Claude signed in, with only exit/boolean summaries retained. No
model request, credential read/export by Hagency, or real local soak was run.

Native `hagency-runtime` currently exports only Codex and owned IO. The Host
explicitly rejects Claude and only admits Codex/OpenAI; model qualification also
admits only Claude/Codex families. The retained Node Octos ACP adapter is a useful
behavior reference, not a native implementation. Native macOS whole-tree cleanup
is still refused. These are real completion gaps, not missing login on mini3.
