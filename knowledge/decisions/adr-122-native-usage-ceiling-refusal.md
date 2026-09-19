---
kind: decision
id: ADR-122
title: Ceiling refusals name the binding draw
status: Proposed
---

## Context

ADR-121 added the read-side ceiling draw. When metering began drawing ceilings
down, the retained JavaScript found that "allocating 50000 would exceed the 0
left on BigLittle" was true and useless: two numbers compete for the ceiling —
commitments and measured fresh spend — and which one is binding is not
inferable from the refusal. The JavaScript fixed this by naming it
(`lib/engagement-store.js:58-103`, `overCommitMessage`) and by splitting the
refusal identity at its call sites: `no_ceiling` for unknown capacity (`:618`),
`over_commit` with the wording (`:638`).

The native store, meanwhile, returns one `Error::InsufficientCapacity` at the
approve sites (`domain.rs:717-719`) for all three of: unknown capacity, a
known ceiling exceeded, and the shared-seat pool quota — a native-only concept
the JavaScript does not model.

## Decision

Slice 2 of the ceiling-enforcement port moves the wording and the identity,
and nothing else.

**Wording.** `hagency_core::ceiling::over_commit_message(agent, alloc,
remaining, ctx: Option<&SpendContext>)` is a byte-faithful port of the
retained `overCommitMessage`: `compactTokens` (1.0M-precision millions,
rounded thousands), the plain head when no context is supplied, the ceiling
sentence, both binding-draw sentences (naming commitments-only when nothing
was measured), the conditional cache-read note that appears only when
consumption exceeds the fresh draw, and the preset-or-agent remedy. The
wording stays pinned to the retained JavaScript by the ceiling oracle: message
vectors computed by importing `overCommitMessage` itself live in
`ceiling-vectors.json` with the source sha256-pinned.

**Identity.** `Error::NoCeiling` ("cannot allocate against an agent with no
declared ceiling", the JavaScript wording) for unknown capacity;
`Error::OverCommit { message }` carrying the produced wording for a known
ceiling exceeded by the allocation. The shared-seat quota binding keeps the
existing `Error::InsufficientCapacity` — the resource-pool refusal this
identity predates the ceiling split for and the only one of the three the
JavaScript does not model.

**Decision unchanged.** This slice changes only refusal identity and wording.
The admission decision (which `remaining` is compared, and against what)
still excludes measured spend; slice 3 folds the ADR-121 draw
(`max(reserved, spent)`) into the decision and supplies the measured fields
of the `SpendContext`. A refusal grants no retry, reply, lease or completion
authority.

## Consequences

Tests that matched `Error::InsufficientCapacity` at the approve sites are
re-pointed: `tests/domain.rs:130` (racing approvals, pool ceiling binding) and
`tests/resource_configuration.rs:120` (zero ceiling, declared seat) now expect
`OverCommit`; `tests/domain.rs:150` (`approve_c`, shared seat binding) keeps
`InsufficientCapacity`. The three core wording tests replay the oracle fixture
and run anywhere; the store-level distinction test needs a repository open and
runs in CI where the sandbox permits.
