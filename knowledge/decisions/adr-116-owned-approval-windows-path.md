---
kind: decision
id: ADR-116
title: Project approval metadata from original canonical Windows workspace custody
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
---

## Context

Original Windows CI at 1baa80d reports TurnStart/LostAuthority before the first
approval notice. The retained workspace is an actual canonical directory; Rust
Windows canonical paths use a verbatim disk namespace that ADR039's generic
untrusted lexical parser deliberately refuses. Source inspection identifies this
incompatibility; actual corrected Windows execution remains a hosted gate.

## Decision

The retained execution Root privately projects its canonical disk or UNC prefix
and normal components into ordinary approval metadata. It retains the original
directory handle and canonical path for launch, readback and identity checks.
No public string constructor or generic untrusted path-parser relaxation is added.
Device namespaces and components with ambiguous ordinary Windows aliases refuse.

Only the host context workspace is projected. Native callback params, digest and
display preview stay original. Ordinary disk/UNC callback cwd can produce the
existing reusable command scope; verbatim or otherwise unrepresentable callback
cwd remains without reusable scope, with Once/Deny available under all existing
owner and deadline checks. A response never gains broader permission from this
projection. Scope text is not filesystem or native application proof.

## Consequences

No schema, deadline, sandbox, callback protocol or process ownership changes.
Actual Windows canonical drive execution is required. Lexical UNC and domain
binding tests do not establish a live UNC share or production availability.
Implementation and qualification are governed by the exact seven-path contract
specs/task-rust-owned-approval-windows-path.spec.md and are currently pending.

## Amendment (2026-09-12)

Projecting only the host context workspace was insufficient. A child launched
in the verbatim canonical root reports that verbatim form as its callback
`cwd`, the untrusted parser refuses it by design, and so no owner verdict on
Windows could ever carry a reusable scope: hosted Windows refused every
`Always` verdict in `native_owned_approval_resume`, five of five probe
iterations, with `reusable choice without scope key`. The session and process
working directory string is now the same ordinary projection of the retained
root (`Root::approval_path`), on every platform. Directory custody is unchanged:
the retained handle and its identity checks remain the authority, and the
projection still refuses device namespaces and ambiguous aliases. Callback
parameters stay byte-exact as reported by the runner.
