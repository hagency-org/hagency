---
kind: decision
id: ADR-118
title: Native transcript attribution search
status: Proposed
---

## Context

The ported metering parsers normalize a single transcript; attribution still
lives only in JavaScript (`lib/metering/attribute.js`). Migration plan item M7
("continue transcript discovery ... exact Agent/project attribution") needs
the pure attribution chain in Rust: workspace precedence, per-framework search
descriptors, session totalling and the fleet summary.

## Decision

This ports the pure logic of `lib/metering/attribute.js` into
`hagency_metering::attribution` as the next authorized step of the Rust
migration and ADR-055's parser boundary. Input is an in-memory agent record,
a home directory string, an explicit process working directory and an injected
slice of `(file, text)` sessions. **The module performs no filesystem
access**; traversal, age windows and file budgets stay in the JavaScript
reader until separately specified.

Search descriptors (`dir`, `narrowed`, `recursive`) and every recorded `cwd`
are **private untrusted layout hints, never authenticated Agent, project or
task identity**. Claude's directory mapping replaces `/` with `-` forward
only and is ambiguous backwards, so it narrows a search and never proves a
workspace; the transcript's own `cwd` decides, resolved like `path.resolve`
against the caller-supplied working directory. Workspace precedence is
unchanged: `lastWorkspacePath`, then `workspacePath`, then `workdir`/`homeDir`
only for an `on-demand` runner; every candidate is trimmed and the first
non-empty wins, else the agent is unattributable.

Every case the JavaScript cannot answer returns `available: false` with the
same reason string — unsupported framework (including the per-framework
`UNSUPPORTED` reasons), no recorded workspace, no known transcript location,
and the zero-match reasons that state additively both what was opened and
what a bound stopped the scan from reaching. **A refused, partial or missing
measurement is never replaced by a zero**: unknown fields stay `null`
through all arithmetic (ADR-013's missing-is-unknown rule, continuing
ADR-055's correction of the JavaScript's coercion to zero), and cache reads
stay separate from fresh input. The four token categories remain separate;
per-session totals come from the ported `parse_session`.

Documented divergences from the JavaScript, all corrections in the ADR-055
direction: a session the native parser refuses (duplicate JSON keys,
conflicting message identity, unsafe counters, capacity) counts as skipped
rather than being coerced into a number; a transcript with conflicting
workspace hints is unattributable rather than first-wins; non-string agent
fields read as absent rather than stringified. Synthetic vectors executed by
the retained JavaScript functions cover the shared semantics; native-only
assertions pin the corrected ones. Fleet summaries keep agents sharing a
workspace ambiguous instead of summing or splitting them.

## Consequences

Attribution totals, reasons and ambiguity behavior become testable in Rust
without touching a filesystem, and later ports can move the reader behind the
same injected interface. Anything still reading real transcripts, homeservers
or provider services remains outside this module.

## Alternatives Considered

Coercing refused parses to zero or keeping the JavaScript's first-wins cwd
would match the oracle but manufacture attribution from damaged evidence.
Widening the workspace chain (e.g. `workdir` for every runner) would attribute
another agent's transcripts to this one; misattribution is worse than an
absent figure.
