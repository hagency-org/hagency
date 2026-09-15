spec: task
name: "Register the fleet: the project-side registration the provisioning ingress requires"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, registration, onboarding, bootstrap, console, parity]
---

## Intent

Give the port the ability to write its own fleet registration. The native store
already owns the write — `DomainRepository::register`
(`native/hagency-store/src/domain.rs:730-766`) is the **sole** writer of the
`registrations` table outside tests — but its facade
(`native/hagency-store/src/domain_worker.rs:2841-2842`) has no production caller.
A fresh deployment therefore has no `registrations` row, and the provisioning
ingress cannot start: bootstrap reads that row fail-closed at config load
(`native/hagency/src/bootstrap/config.rs:305-318`, via
`provisioning_registration_for_engagement`,
`native/hagency-store/src/domain/verified_ingress.rs:198-214`, which joins the
engagement on `fleet_id` **and** `generation`), and the intake's verdict path
needs the representative mxid from the same record
(`native/hagency-matrix/src/intake.rs:299-306`). Nothing else in the port writes
the table.

The retained product records the same act as a project side:
`projectSideStore.upsertSide` (`lib/project-side-store.js:369-440`), reached
through `POST /api/project-sides` (`backend-v2.js:9861-9877`) and the
registration-file path (`POST /api/project-sides/:id/registration-file`,
`:10030`). This slice wires the port's `register` to its production callers.

This is **parity**, not new behaviour: the retained product already exposes the
act, and the port already models the row — only the caller is missing.

## Bootstrap-time, route-driven, or both? **BOTH — and each gets its own scenarios.**

The port needs both, for different reasons:

- **Bootstrap-time is load-bearing.** `native/hagency/src/bootstrap/config.rs:305-318`
  refuses to start a host whose engagement names no registration (or a
  registration with no reception room) — the comment says so at `:300-304`:
  "refuses to start rather than observing nothing and silently dropping the
  provisioning ingress". So the row must exist **before** `serve`, written by the
  same trusted local layer as the other pre-serve bootstrap steps
  (`register_workspace` at `native/hagency/src/bootstrap.rs:871-875`, the
  managed-account binding at `:876-892`). The established port precedent for a
  pre-serve store write exposed to an operator is a **CLI subcommand**, not a
  route: the G4 account bootstrap dispatches `hagency account …` from
  `native/hagency/src/main.rs:202-206` into
  `hagency::bootstrap::accounts::run` (`native/hagency/src/bootstrap/accounts.rs:40`).
- **Route-driven is the retained shape.** The retained product registers through
  `POST /api/project-sides` (`backend-v2.js:9861`), an operator route. Parity
  therefore also wants an authenticated route, mounted on the console API surface
  the port already has (`native/hagency/src/console.rs:70-80`, where
  `project_sides::router()` is mounted at `:75` — today that router is read-only:
  `Router::with_path("project-sides").get(list)`,
  `native/hagency/src/console/project_sides.rs:21`).

Both are in scope, as two scenarios, because they are different acts with
different authorities: a bootstrap CLI write (trusted local operator, no HTTP),
and an operator route write (console session + bearer). A port with only the
route cannot start a fresh host that has never been reached over HTTP; a port
with only the CLI loses the retained operator surface.

## Constraints

### Must

- Reach the write through `DomainRepository::register`
  (`native/hagency-store/src/domain.rs:730`) and its facade
  (`domain_worker.rs:2841`) — the sole writer of `registrations`. No second
  `INSERT INTO registrations`.
- Validate the record before writing (`registration.validate()`,
  `native/hagency-core/src/authority.rs:36-58`): `fleet_id` is `hf_` plus 32 hex,
  `generation` is `> 0` and `<= JSON_SAFE_MAX`, `server_name` parses as a Matrix
  server name, `reception_room_id` is a room id on that server,
  `representative_mxid` and `approval_bot_mxid` are user ids on that server, the
  representative mxid is exactly `@<fleet_id>_representative:<server_name>`, and
  the two mxids differ.
- Keep the store's generation fence (`domain.rs:754-756`): a registration whose
  `generation` is **less than or equal to** the stored one is refused with a
  generation error, and no row is written.
- Keep the store's idempotency (`domain.rs:749-753`): a re-registration whose
  content is **equal** to the stored record is a no-op that succeeds.
- Keep the rotation's reconciles (`domain.rs:762-763`): a genuine generation
  advance runs the graphs and matrix-route reconciles in the same transaction,
  because rotation fences execution immediately (the comment at `:757-758`).
- Bound the table (`domain.rs:735-741`, `bounded_row(…, "registrations",
  "fleet_id", …, 1024)`).
- Authenticate the **route** caller through the console session and bearer the
  other console routes use (`native/hagency/src/console.rs:70-80`), and
  re-check before responding (the console's recheck-after-write pattern,
  `native/hagency/src/console/agents.rs:236-244`).
- Report the missing prerequisite honestly at bootstrap: a serve whose engagement
  names no registration stays the named fail-closed refusal it already is
  (`bootstrap/config.rs:300-318`) — this slice adds the writer, it does not
  soften that refusal.

### Must Not

- Do not change `register`, the generation fence, the `registrations` schema, or
  the graph/route reconciles; this slice wires the callers, it does not move the
  write.
- Do not add a **Matrix** ingress for registration: unlike the provisioning
  request, registration is a trusted local/operator act, not an observed event.
- Do not fold registration into the engagement-lifecycle slices
  (`specs/task-rust-engagement-refuse.spec.md`,
  `specs/task-rust-engagement-retire.spec.md`); it is a separate parity item.
- Do not name a `Test:` selector for a test that does not exist. Every scenario
  below carries `Owed Selector:` until its test lands.
- Do not invent a gap id. The owed form is `owed (G11)`, allocated centrally for
  this slice.

## Production-caller gate

The gate (`native/scripts/check-production-callers.mjs`, a wired CI step at
`.github/workflows/rust.yml:124`) resolves a concrete `crate::path::fn`
against the production call graph. Both callers landed 2026-09-15 and every
scenario's `Production caller:` line now names one of them:

- Bootstrap-time: `hagency::bootstrap::registration::run` — the module
  mirroring `native/hagency/src/bootstrap/accounts.rs`, dispatched from a
  subcommand in `native/hagency/src/main.rs` beside `Command::Account`,
  calling the facade `DomainStore::register` (`domain_worker.rs:2841`).
- Route-driven: `hagency::console::project_sides::save` — the POST handler
  on the existing `project_sides` router, gated by the shared
  `Scope::AgentLifecycle` check, calling the same facade.

Both write the **same store method**; the two route scenarios differ in
authority, not in the write.

## Representation divergences (UNRESOLVED — decision owed, not decided here)

1. **The key differs: fleet id vs server name.** The port keys `registrations` by
   `fleet_id` (`native/hagency-store/src/domain.sql`: `fleet_id TEXT PRIMARY
   KEY`). The retained product keys a side by its **server name**, and says the
   id *is* the server name on purpose: "a generated id would let two records
   claim one homeserver, and the invariant that makes this design coherent is
   that a homeserver has exactly one credential and one representative"
   (`lib/project-side-store.js:365-368`, `:375`). The port's `Registration`
   carries both a `fleet_id` and a `server_name` (`authority.rs:19-21`) with no
   store-level uniqueness on the server name. **This spec assumes the port keeps
   its fleet-id key** and does not assert a one-server-one-registration
   invariant. **Unresolved.**
2. **The generation fence is the port's, not the retained product's.** The
   retained `upsertSide` has no generation or fence concept — it overwrites the
   record and carries unspecified fields forward
   (`lib/project-side-store.js:374-417`). The port refuses a non-advancing
   generation (`domain.rs:754-756`) and treats an advance as a rotation that
   fences execution immediately (`:757-758`). **This spec asserts the port's
   fence** because it is the port's existing contract; the rotate-and-fence
   behaviour is stated as a divergence from retained, **not** as parity.
   **Unresolved** whether retained needs an equivalent.
3. **The record's shape is narrower in the port.** The retained side carries
   `credential`, `apiBaseUrl`, `label`, `representative`, `active`,
   `allocatedTokens`, `projects` and access state
   (`lib/project-side-store.js:374-417`). The port's `Registration` is exactly
   six fields — `fleet_id`, `generation`, `server_name`, `reception_room_id`,
   `representative_mxid`, `approval_bot_mxid` (`authority.rs:18-25`). **This spec
   asserts only the six-field record**; the retained richness (credential
   issuance, per-side allocation, projects) is **not** covered, and slipping any
   of it in would be new behaviour needing its own decision. **Unresolved.**
4. **The port's write authenticates nothing.** The retained route is behind
   `requireBearer` (`backend-v2.js:9861`). The store's `register` takes no
   capability and performs no authority check — it is a trusted local writer
   (`domain.rs:730-731` validates the record's *shape* only). **This spec assumes
   authority lives entirely in the calling layer** (the CLI's operator token via
   `hagency init`; the route's console session), as the other bootstrap writers
   do. **Unresolved** whether the port wants a capability check on the store
   method itself.

"Unresolved" means: not settled by this document, and not assigned an ADR number
here. They are recorded so a reader can decide whether any needs its own record.

## Acceptance Criteria

Scenario: A fresh host registers its fleet before serve
  Owed Selector: native_registration_bootstrap_writes_the_fleet_row (owed — no test yet; no Test: line here)
  Given an initialized private state whose engagement names fleet hf_<32 hex> at generation 1 and no registrations row for it
  When the operator runs the bootstrap registration command with a valid six-field registration
  Then exactly one registrations row exists for that fleet at that generation, and a subsequent serve reads it instead of taking the fail-closed refusal
  Production caller: hagency::bootstrap::registration::run
  Retained: POST /api/project-sides (backend-v2.js:9861-9877) -> projectSideStore.upsertSide (lib/project-side-store.js:369-440): the create branch (existing null) writes the record and audits 'side_created' (:433-437, :438)

Scenario: An operator registers the fleet through the console route
  Owed Selector: native_registration_route_writes_the_fleet_row (owed — no test yet; no Test: line here)
  Given an authenticated console operator session and a valid six-field registration
  When the operator submits it through the console project-side registration route
  Then exactly one registrations row exists for that fleet and the response reports the saved record, never the operator token
  Production caller: hagency::console::project_sides::save
  Retained: POST /api/project-sides (backend-v2.js:9861-9877) answers { ok: true, side } (:9873) through respondProjectSideError (:9874, map :9105-9117)

Scenario: A re-registration with identical content is idempotent
  Owed Selector: native_registration_identical_reregistration_is_a_noop (owed — no test yet; no Test: line here)
  Given a registrations row for a fleet at a generation
  When the identical registration (same fleet, generation and all six fields) is registered again
  Then the call succeeds, the row is unchanged, and no second row is written
  Production caller: hagency::bootstrap::registration::run
  Retained: lib/project-side-store.js:371, :374-417 — upsertSide on an existing key re-writes the same keyed record and audits 'side_updated' rather than creating a second side

Scenario: A stale generation is refused and the stored row is unchanged
  Owed Selector: native_registration_stale_generation_is_refused (owed — no test yet; no Test: line here)
  Given a registrations row for a fleet at generation N
  When a registration for the same fleet arrives at generation N (or any generation below it)
  Then the store returns a generation error, the stored row still describes generation N, and no second row is written
  Production caller: hagency::bootstrap::registration::run
  Retained: NO counterpart — the retained upsertSide has no generation fence (lib/project-side-store.js:374-417 overwrites); this is the port's own contract (domain.rs:754-756), asserted here as a divergence, not as parity

Scenario: A generation advance rotates the registration and reconciles
  Owed Selector: native_registration_generation_advance_reconciles (owed — no test yet; no Test: line here)
  Given a registrations row for a fleet at generation N
  When a registration for the same fleet arrives at generation N+1 with valid content
  Then the row describes generation N+1, the graphs and matrix-route reconciles have run in the same transaction, and the previous allocations stay observable rather than erased
  Production caller: hagency::bootstrap::registration::run
  Retained: NO counterpart — rotation and its reconciles are the port's (domain.rs:757-763); the comment at :757-758 states the reason (rotation fences execution immediately, rotation cannot erase spend)

Scenario: An invalid registration is refused before any write
  Owed Selector: native_registration_refuses_an_invalid_record (owed — no test yet; no Test: line here)
  Given a registration that violates the record's own shape — a malformed fleet id, generation zero, a server name the mxids and room do not share, a representative mxid that is not the fleet's own derived name, or two identical operator mxids
  When it is registered through either caller
  Then the store returns an input error and no registrations row is written
  Production caller: hagency::bootstrap::registration::run
  Retained: lib/project-side-store.js:370 serverName(...) and :377 text(...) validate the side's own fields before the record is built; respondProjectSideError maps bad_request to 400 (backend-v2.js:9113-9116)

Scenario: A registration naming an unknown engagement's fleet cannot satisfy bootstrap
  Owed Selector: native_registration_must_match_the_hosts_engagement (owed — no test yet; no Test: line here)
  Given a registrations row written for a fleet and generation that no engagement on the host names
  When the host serves
  Then the registration read refuses fail-closed with the named configuration failure and the host does not start, exactly as before this slice
  Production caller: hagency::bootstrap::registration::run
  Retained: the retained product has no equivalent precondition — a side is usable once its credential verifies (backend-v2.js:9917 POST /api/project-sides/:id/verify); the port's engagement-joined read is its own (verified_ingress.rs:206-207 joins on fleet_id AND generation)

Scenario: The registration route refuses an unauthenticated caller
  Owed Selector: native_registration_route_requires_operator_authority (owed — no test yet; no Test: line here)
  Given no console session and no bearer
  When the console project-side registration route is called
  Then it is refused before any store work and no registrations row is written
  Production caller: hagency::console::project_sides::save
  Retained: POST /api/project-sides is behind requireBearer (backend-v2.js:9861)

## Out of Scope (named explicitly)

- **The credential and verification surface.** The retained side's
  `credential`/`apiBaseUrl` write (`backend-v2.js:9880` PUT
  `/api/project-sides/:id/credential`), its `verify` route (`:9917`) and its
  registration-**file** issuance (`:10030`) are **not** in this slice: the port's
  `Registration` has no credential field (`authority.rs:18-25`), so wiring them
  would be new behaviour, not parity (divergence 3).
- **Per-side allocation and projects.** `setAllocation`
  (`lib/project-side-store.js:454`) and the nested `projects` map (`:401-416`)
  have no counterpart in the six-field record; **not** in this slice.
- **The `create_canonical_task` gap.** The other ADR-146 unowned row (the
  receive-inbox plan's task minter, `domain.rs:679`) is a separate parity item,
  out of scope here.
- **The engagement lifecycle.** Refusal and retirement are the separate slices
  `specs/task-rust-engagement-refuse.spec.md` and
  `specs/task-rust-engagement-retire.spec.md`.
- **Any softening of the fail-closed bootstrap refusal** — this slice adds the
  writer; the pre-serve prerequisite stays a named refusal.
- Any automatic retry, sweeper or timer.
