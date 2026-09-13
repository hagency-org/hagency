---
kind: decision
id: ADR-144
title: "Two-agent integration acceptance: one shared room and separate DMs"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [rust, matrix, fixtures, acceptance, acceptance-evidence]
---

## Context

The migration plan's M8 item 3 requires "Run end-to-end resource request, approval,
delegation, files, usage and revocation on dedicated fixtures/accounts. **Test two
Agents sharing a room and separate DMs.**"
(`docs/design/hagency-rust-migration-plan.md:378-379`), under an exit gate that admits
no untested cell (`:387-388`). The backlog's §4 finds this requirement "planned
nowhere" and calls it the single largest omission. Every native integration harness
on this tree is single-engagement: `Fixture::new()`
(`native/hagency-matrix/tests/common/mod.rs:60-93`) admits and approves one request
and binds its identity to that one engagement (`:85`), and no two-engagement fixture
exists anywhere under `native/*/tests` (grep → none; 95 call sites use the single
form). So "two Agents" is a fixture gap before it is an assertion gap.

The retained product has the oracle:
`tests/matrix-direct-chat.test.js:186` ("ordinary invitations support two agents
with independent room bindings and mention routing") drives two agent clients into
one room, asserts only the addressed agent's client sends, and refuses a
private-root-threaded reply and an ambiguous sender with nothing sent. The
mechanisms native composes all exist: the real-TLS fake peer
(`hagency-palpo/tests/common/mod.rs:168`), the owned-dispatch full workflow
(`native/hagency/tests/owned_matrix.rs:11`), DM and group ciphertext in both
directions for one engagement (`hagency-matrix/tests/outgoing/mod.rs:451`), the
store's cross-engagement isolation analogue (`conversations.rs:196`), and the
DM-only approval-room rule (`approval_intake.rs:19-30`).

## Decision

**The acceptance is two slices, split by what an in-process fixture can honestly
prove.**

**MA-M8a — the two-engagement acceptance, in the default test targets.** A
`Fixture::new_pair()` beside `new()` (never a change to `new()` or its 95
dependents): two admitted engagements on two resources whose seats differ, two
transport identities, one shared project room both may address, one direct room
for the owner per engagement. **Five tests** assert the retained oracle's four
properties plus its two refusals: (1) both agents deliver into the shared room,
each charged to its own engagement, and a threaded reply to the other engagement's
private root is refused with no send, and an ambiguous sender is refused;
(2) a DM reaches only its own engagement's direct room and no request carries the
shared room id — **in each direction**: the agent→owner DM is addressed to that
engagement's direct room only, and the owner→agent DM reaches its own
engagement's inbox only, never the other engagement's rows — the shared-room
half is not asserted here, because property (5) already pins that no DM body
is visible in the shared room's ciphertexts, so the per-direction clause
asserts only per-engagement DM isolation; (3) a task handed across the room
settles on its own engagement
with the usage ledger observing the spend there and not on the other; (4) a
message for one engagement is never readable from the other; (5) no DM body is
visible in the shared room's ciphertexts. The shared room is a **delivery** room,
never an approval room — `HostApprovalConfig::new` admits only
`RoomPrivacy::Direct` rooms (`approval_intake.rs:19-30`), and the fixture must not
widen the approval set to make a test pass. No new crate, no migration, no route,
no schema change, no homeserver; runs in the existing `native` matrix job.

**MA-M8b — the real-homeserver qualification, operator-run, evidence-recorded**
(the RUN-A/ADR-140 class). A runbook under `docs/` plus an operator-run example
against a real Palpo homeserver and a real owner client, asserting the three
claims in-process fixtures cannot carry: a foreign homeserver's membership/PL
admission, E2EE against a second real device with real key upload/claim, and the
approval round-trip through a real owner client. The evidence record lives
**crate-side** — the class `adr-140-real-codex-sandbox-qualification.md`
defines on this branch (an always-present test validating a tracked record;
note ADR-075 keeps its own evidence inline in prose): `native/hagency/qualification/
two-agent.json` — a tracked file naming the pinned artifact and version, the
environment, the three verdicts and what remains unproven, in the ADR-075 record
shape (the run id, the exact failures, the verdict). The always-present test
`native_two_agent_qualification_records_its_evidence` validates that record — it
exists, names the artifact and version, carries a verdict per claim — and
**fails when the record is missing or partial, never skips**. A missing record is
a red gate, not an untested cell.

**What this does not claim.** Not Windows or macOS coverage (the plan's item 2;
Windows is paused per `adr-136-windows-paused-release-target.md`), not item 4's outage/recovery classes, not item 5's
on-hardware budgets — separate M8 items, named so this record is not read as
M8's completion.

## Consequences

Good, because the plan's largest omission becomes an executable, reproducible
acceptance in the default targets plus three honestly-labelled qualification
claims, and the negatives are tests, not prose: a leak across engagements and a
DM body in the room each fail a named assertion.
Bad, because MA-M8b cannot run in CI today — the plan's exit gate still cannot
be marked green from a hosted run, and the record says which cells are
qualification-only.

## Alternatives Considered

- One slice covering all four claims in process — rejected: it would have to fake
  a foreign homeserver's admission and a second device's keys, exactly the
  conflation the evidence-record class exists to prevent.
- Extend the single-engagement tests instead of adding a pair fixture — rejected:
  a second identity changes construction, not assertions; the pair belongs beside
  `new()`.
- Carry MA-M8b in a new hosted job now — deferred: a real homeserver in CI is a
  service-lane decision (SR-*), not this slice's.

## Amendment 2026-09-13 — MA-M8b's evidence lands as a file plus an always-present test

The qualification record is a tracked evidence file at
`native/hagency/qualification/two-agent.json`, validated by
`native_two_agent_qualification_records_its_evidence`
(`native/hagency/tests/qualification.rs`) on EVERY leg — ungated, failing with a
named reason when the file is absent, partial, stale or carries no verdict for
any claim (ADR-140's evidence class; `qualification/codex-sandbox.json` is the
exemplar). Until an operator run is recorded, the file is an honest placeholder
and the test's refusal IS its state.

**How the operator runs it.** Follow
`docs/design/two-agent-qualification-runbook.md` against a real Palpo homeserver
and a real owner client: qualify the three claims (foreign homeserver
membership/PL admission, E2EE to a second real device, the approval round-trip
through a real owner client), rewrite the record in the same commit as any
launch-path change — schema `hagency-two-agent-qualification-v1`, pinned native
version and full commit hash, timestamp, both engagements, the shared room as
delivery-only, the DM in BOTH directions verified as plaintext, two usage rows,
one passing verdict per claim naming its evidence, a non-empty `unproven` list,
and every homeserver/room identity as a sha256 digest, never a name — then run
`cargo test -p hagency --test qualification`.

Hosted runners skip exactly this one selector BY NAME in
`.github/workflows/rust.yml`, which carries the codex qualification skip on the
same line — the hosted skip list is
`-- --skip native_codex_real_app_server --skip native_two_agent_qualification_records_its_evidence`.
The codex skip is the ADR-140 slice's own, landed in its own commit
("ci(native): hosted runners skip the operator-run qualification tests by
name, never inside a test") with its companion "every qualification selector
is present on every hosted leg" — both commit subjects, not ADR-140 amendment
sections. The skip is never inside the test: hosted runners have no real
homeserver and will never have real evidence. A green validating test proves
the record's shape and verdicts, not that the qualification ran.
