---
kind: decision
id: ADR-081
title: "Inspect locked media fixtures through their original owner handle"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Windows correctly refused fixture path reads that opened another handle while the original media journal remained exclusively locked.

## Decision

Native CI34553424733 at25c01ee exposed three ADR077 fixture failures on actual
Windows: error33 while fs::read opened a second handle to the exclusively locked
journal. Production staging was not the failing operation. ADR079's not-yet-pushed
fixtures use the same invalid independent-read pattern and need the same correction.

Add a test-only bounded journal snapshot helper that reads through the existing
Store file handle, restores its cursor and keeps the original lock throughout.
Use it whenever the Store is alive; independent path reads remain only after
every journal owner has closed. Byte comparisons, corruption/recovery checks,
capacity, actual platform sync assertions and existing lock semantics stay intact.
No production unlock, new sharing permission, retry or relaxed assertion is added.

The original Windows run remains failed. Its separate Matrix fixture timeouts and
later Palpo diagnostic shutdown failure have different evidence and are not fixed
by this change. Actual Windows rerun is required; macOS tests and Windows Clippy
alone do not establish runtime closure.

## Consequences

Test inspection uses the bounded original owner handle and restores its cursor without unlocking. Separate Matrix and shutdown failures retain their own unresolved evidence and qualification requirements.

## Alternatives Considered

Adding sharing permission or releasing the production lock for inspection would weaken ownership. Retrying the independent path read would not correct the fixture's invalid handle assumption.
