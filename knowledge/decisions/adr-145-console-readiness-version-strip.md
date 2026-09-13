---
kind: decision
id: ADR-145
title: "Console readiness and version strip"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
liveness: auto
tags: [console, readiness, version, observability]
---

## Context

The cutover runbook (the two-host cutover ADR) gates on `/ready`, never `/health` — but the operator's only tools during cutover are curl and the CLI: the native console renders usage, resources, alerts and engagements, and nothing references readiness, version or the store head (the console-parity review's R8: greps → 0). The second-reader review of the v1 design confirmed the architectural call end to end (reading the unauthenticated `GET /ready` from the page is consistent with the readiness contract — **ADR-096's brief-19/21 amendments**, `adr-096:230-271`, not ADR-107 — and with the console's origin rules: the route sits outside the console router, `browser_boundary` is a console-path hoop only) and forced two build-time corrections: the store head cannot come from the manifest build (no SQLite in a node/Next build), and nothing may enter `manifest.json` (`Manifest` is `deny_unknown_fields`, `assets.rs:14`; `Assets::load` refuses `version != 1`, `:116`). This ADR is the v2 design as reviewed.

## Decision

**The strip reads the existing unauthenticated `GET /ready`** (`lib.rs:121`, handler `:238-240`) — never `/health` (200-while-live by contract, the wrong boundary), never a new console-authenticated read (readiness is diagnostic-never-authority for any observer; a session would imply otherwise), and never proxied through `/console/api/*`. The payload is consumed as-is: `{"status","implementation","components[].{name,state"}}` (`lib.rs:335-342`) — **no new wire keys anywhere**; the readiness contract stays frozen as ADR-096's amendments define it.

**Version and store head are build-time constants in a generated module.** `build-native-console.mjs` writes `generated/status-constants.js` (`HAGENCY_NATIVE_VERSION`, `HAGENCY_NATIVE_SCHEMA_HEAD`) into the staged tree before `next build`, so Next bundles it into an existing hashed chunk; the staged tree is the build's `mkdtemp` workspace, so **no repo path for the generated file is committed**. If Next excludes an unbundled generated module, the constants are appended to an existing staged `.js` — same values, same test. The version is parsed from the root `Cargo.toml`'s `[workspace.package]` — the single source the versioned-release ADR fixes; the binary reports the same value via clap. The head is fed as a `--schema-head <n>` argument copied from the migration registry (`domain.rs`'s `version: 25`). **`assets.rs` is untouched by construction** — the file passes the existing `.js` mime whitelist and the manifest gains no field.

**Render rules, precisely.** `status === "ok"` renders **ready**; a 503/`unavailable` renders **not ready** with the failing components' names and words — a failing component **never renders ready**; an unreachable `/ready` renders **unknown**, never ready. The sweep-tick cell renders its **raw word, unstyled by outcome**: `ComponentState::is_ready()` returns true for every `TickOutcome` (`lib.rs:207-210`), so `refused_busy`/`refused_outcome_unknown` are *ready* words and must not be styled not-ready; only the sweep's liveness (`alive`/`stopped`) participates in the colour, exactly as the server's own predicate does. The store-head cell is honest about being the binary's *expected* head, never a live query (the runbook's step 4 reads the live one).

**Placement and scope.** The strip is `mockup/components/NativeStatusStrip.jsx`, rendered as a `PageHead` child (`PageHead.jsx:19-29` already takes children) on every native page — readiness is a fleet fact, and the cutover operator may be on any page when a 503 flips. (There are five native pages but four `Native*` components — `resources/new` renders in native mode through the existing component set — so the strip touches the four components; the fifth page inherits it.) No scope, no route, no session: the same class as the rail. **Restart control stays explicitly out of scope until the service wrapper exists** — the retained proxy's own rule ("nothing here can … start a process") is inherited verbatim.

**The README amendment this ADR carries** is one passage: `native/README.md`'s console-route description gains a line naming the strip (it reads `/ready`, it is not a console route), so the README's route inventory stays true.

**Client validation.** `validateReadiness` checks the exact key set and count of the payload per `native-api.js:10-11` and treats unknown state words as renderable text, not errors — **one vocabulary, the server's**; no second readiness-word enumeration in the client.

## Consequences

Good, because the cutover operator finally sees the gate the runbook polls, on any page, with the server's own words; no Rust change, no contract change, no new authority surface.
Bad, because the version is build-time (a stale asset build renders a stale version until the equality test catches it), and the strip cannot restart anything — by design, until the wrapper exists.

## Alternatives Considered

- Read `/health` — rejected: 200-while-live is the retained contract, not readiness.
- A console-authenticated readiness read — rejected: implies the session matters; the boundary is deliberately unauthenticated.
- Inject into `manifest.json` — rejected: `deny_unknown_fields` + `version != 1` rejection make it a Rust change in disguise.
- Enumerate state words in the client — rejected: a second vocabulary that can drift from the server's.

