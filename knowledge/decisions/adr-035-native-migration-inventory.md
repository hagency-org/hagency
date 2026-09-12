---
kind: decision
id: ADR-035
title: Keep migration inventory source-derived and separate from parity evidence
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Migration planning needs reproducible source coverage without mistaking a route inventory for implemented native parity.

## Decision

M0 uses an explicit policy and a deterministic source snapshot. The legacy behavior
baseline remains `5dbef22dc5ad4e0bb1a886538406ec91a5893f9b`; this expanded inventory
records source at `a41ab10ca0c5c27e870d331f868757c80d760846`. Each inspected source has
a SHA-256 digest. Checks compare the current tracked corpus with the reviewed
fixture, so CI needs neither the old Git object nor network access. Intentional
source drift requires reviewing the policy and regenerating the fixture; this is
not an automatic declaration that a new route has been ported.

Espree 11.2.0 is a direct, pinned development dependency. Parsing inspects source
syntax and never imports application entrypoints. HTTP registrations preserve
methods, arguments, handler names and exact source locations. Defaulted named
installer parameters are resolved against named direct or member call sites.
Unknown methods, unresolved arguments, missing installers, unsupported custom
branches and unclassified raw listeners fail the inventory operation.

The supported detector recognizes `app`, `router`, `this.app`, `this.router`,
Express factory variables and straightforward aliases; literal/computed method
names; literal, static template, array and regex path expressions; and chained
`route(...).get(...).put(...)` registration. Regex Express paths require an explicit
ownership extension before the checked inventory accepts them. MCP inspection
recognizes `server.tool` and `server.registerTool`. Custom dispatcher inspection
covers the reviewed modules' method/path equality, path prefixes and literal regex
matches. Next's exported methods and READS/WRITES arrays are catalogued separately.

This is not a whole-program call graph or a general JavaScript evaluator. Arbitrary
receiver aliasing, reflection, generated code, indirect/higher-order installers,
dynamic imports and external SDK registration internals remain detection limits.
Installer matching uses a reviewed name/call-site correspondence, not a proof of
lexical binding across modules. Reviewed dispatcher links record the source calls
and target modules separately. Corpus hash changes force review, but do not by
themselves prove that an unsupported new registration style was understood. New
styles require detector fixtures or an explicit unresolved coverage gate; never
silently call them exhaustive. Generated workspace wrappers and router TypeScript
behavior remain part of the outstanding M0 behavior traceability work.

Every helper candidate has an explicit file-level role, owner, phase, disposition
and validation gate. New or stale helper entries fail. Shell and service files are
not parsed as JavaScript: their source hashes and literal path mentions provide
navigation evidence, not a shell execution graph. In particular, deployed
watchers and dual-use audit/build helpers must not be mislabeled as build-only.
The old Node implementation can remain on build machines only after installed
native commands stop invoking it. The inventory executes none of these helpers.

All rows remain `parity-unverified`. Native kernel coverage recorded in other
contracts is valuable, but an inventory row requires its complete adapter,
authority, error, platform and workflow acceptance before becoming release parity.
The inventory cannot close release gates. Exact supported runtime versions,
embedded hardware/libc and measured budgets, complete event/schema/feature
traceability, terminal behavior, packaging and controlled cutover stay open.

## Consequences

Hashes, locations and reviewed classifications expose source drift without importing application entrypoints. Every inventory row remains parity-unverified until its complete workflow and platform gates are satisfied.

## Alternatives Considered

Importing helpers to discover behavior could execute runtime side effects. Automatically marking regenerated inventory rows as ported would replace adapter and workflow evidence with source navigation data.
