---
kind: decision
id: ADR-119
title: Native bounded transcript reader
status: Proposed
---

## Context

ADR-118 moved attribution into Rust over injected session text, leaving the
bounded filesystem reader and the cached fleet meter in JavaScript
(`lib/metering/reader.js`). Migration plan item M7 needs that reader: Codex
files transcripts by date with no cwd in the path, so answering "what did this
agent use" means opening candidates until one matches, and a developer machine
holds hundreds of sessions with single transcripts running to megabytes. An
unbounded scan would make a usage request cost seconds and grow with history.

## Decision

This ports `lib/metering/reader.js` into `hagency_metering::reader` as the
next authorized step of the Rust migration, continuing ADR-055's parser and
ADR-118's attribution boundaries. **This is the one metering module that
touches a filesystem**, through `std::fs` only, with the clock injected and
environment overrides parsed by the caller from explicit strings
(`ReaderLimits::from_env`, the JavaScript `parseInt` positive-integer rule,
never the process environment inside a scan).

The contract is unchanged: bounded, newest first, and every bound that bites
is reported, never absorbed. Four ceilings keep their defaults — the 30-day
modification-time window, 200 candidate files, 8 MiB per transcript and
20,000 walked directory entries. Byte truncation cuts at a line boundary with
`lastIndexOf`-exact semantics, because a half-line is unparseable and a
silently skipped record looks the same as a smaller transcript. The walk is
breadth-first and descends only when the framework's layout says so: the
nested Codex `YYYY/MM/DD` tree is walked recursively, the Claude project
directory stays flat — the flat-list bug that hid 4,313,968 measured tokens
behind "no transcripts found" is pinned by test. Directories, file names and
mtimes are **private untrusted layout hints, never authenticated Agent,
project or task identity**; the transcript's own `cwd` still decides, per
ADR-118.

Two claims stay separate in `bounds_report`, exactly as in JavaScript: an
out-of-window drop in a *narrowed* (Claude) search is that agent's own older
spend, so the figure understates; in a *non-narrowed* (Codex) search the
dropped candidate's workspace was never read and can belong to anyone, so it
is reported as unread, never as an understatement. `meter_fleet` keeps the
time-based cache (60-second default TTL) keyed over fleet identity — which
agents exist, their framework and their transcript workspace, plus the home
directory — with `force`, a `computedAt` stamp on every value, and a `reset`
method. A changed fleet never reads the old answer; a heartbeat always hits
the cache.

## Consequences

The full chain — discover, read, parse, attribute, summarize — now runs in
Rust with the same reasons, bounds and ambiguity behavior, testable against
the retained JavaScript oracle on deterministic synthetic trees. The HTTP
endpoint, usage projections and quota enforcement remain outside the crate.

## Alternatives Considered

Pruning the Codex tree by directory *name* would be cheaper but drops the one
transcript most likely to be live — a session started before the window and
still being appended has an old directory and a current mtime; the file's own
mtime decides, so every candidate is stat'd. Absorbing a truncation into a
complete-looking total, or merging the two drop claims into one caveat, would
put a false statement on every figure the moment a scan recurses.
