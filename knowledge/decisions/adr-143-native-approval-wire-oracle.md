---
kind: decision
id: ADR-143
title: Executable retained approval and origin oracles
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
---

## Context

The native approval wire and the native console origin hoop were built to match
retained behaviour that nothing executable compared them against. ADR-110 and
ADR-115 record the packet and profile decisions; ADR-107 records the console
authority model; the retained producer itself (`bridge-matrix.js`) and the
console proxy (`mockup/app/api/hagency/[...path]/route.js`) were read, never
replayed. A retained rule that drifted would leave the native counterpart pinned
to a memory of it, and the drift would surface only as a live interoperability
failure.

The `ceiling-vectors.mjs` precedent already answers this shape for the metering
domain: import the retained module, execute it over fixed inputs, pin the
retained source by sha256, and fail `--check` on drift. This decision extends
that precedent to the approval wire and the client-origin rule.

## Decision

`native/scripts/approval-vectors.mjs` EXECUTES the retained producer over a
fixed table and writes `native/hagency-matrix/tests/fixtures/approval-vectors.json`;
`--check` re-derives and fails on drift. Four row families:

- **requests** — `buildOwnerApprovalRequest` (:2600-2646), including the action
  list's scope dependence: `approve_task` requires a task id, `approve_always`
  requires any reusable scope, and neither appears otherwise.
- **notices** — `buildPublicApprovalNotice` (:2578-2598), the redacted
  ADR-003 public surface, with and without a thread root.
- **verdicts** — `parseApprovalVerdictEvent` (:2648-2689), the accept/refuse
  matrix: two complete shapes accepted (current `com.agentchat.*` and legacy
  `com.hagency.*`), nothing mixed, and every identity/digest/action bound
  refused on violation.
- **origin** — `sameOriginWrite` (:379-389) over the approval-relevant subset
  of request headers. The exhaustive console-origin table is ADR-107's
  amendment (CL-S4′), not this one. The native counterpart's absent-origin
  refusal on a mutation is part of request authentication, not a scope
  denial: the console's session hoop answers **401 `console_access_required`**
  (`console.rs` `current`/`same_origin`), while 403 remains reserved for a
  missing resource scope.

Both retained sources are pinned by sha256 inside the fixture. A changed
allowlist, action list, verdict bound or origin rule fails the oracle check
rather than silently re-blessing a different answer.

The fixture lives in `hagency-matrix/tests/fixtures/` because that crate owns
the native counterpart: `approval_delivery/state.rs`'s `Frozen::validate`
(:96-124) is the packet validator the request rows are compared against, and the
verdict intake is that crate's. Placing it beside `hagency-store`'s
`ceiling-vectors.json` would put the fixture in a crate that does not own the
rule.

Three differences between the retained and native rules are recorded in the
fixture's `nativeNotes` and asserted, not left implicit:

1. **Request-id length.** The retained verdict parser admits exactly thirty-two
   lowercase hex characters (`:2676`); native admits thirty-two **or** forty
   (ADR-115). The row `forty-hex-request-id` therefore refuses retained and must
   be accepted natively — the one row where the two sides deliberately disagree.
2. **Packet kind.** A public status notice carries `kind: "status"` under the
   same event key as the request (`:2586-2592`). `Frozen::validate` refuses any
   non-request msgtype, any kind other than `request`, and any content object
   that is not exactly three keys, so a status notice cannot pass it unmodified.
   This is why the public-notice slice cannot reuse the private-card custody
   path unchanged; that conflict is C1's open item, and this oracle records the
   constraint rather than resolving it.
3. **Status schema.** `schemas/approval/public-status-v1.schema.json` already
   describes the notice shape (`kind: "status"`, `state: "waiting_for_owner"`,
   body at most 512 bytes). No native encoder emits it yet; the schema is the
   contract a future slice binds.

The oracle executes retained source, never modifies it, and adds no test-only
export. `bridge-matrix.js` cannot be imported in a bare checkout — its
transitive `matrix-bot-sdk` dependency is absent — so the three functions and
the six constants they need are lifted by literal line anchor into a
`new Function` body. Every anchor is asserted, so a moved block fails loudly. A
future environment with dependencies installed may switch to a direct import
without changing the fixture.

## Consequences

Good, because a retained wire or origin change can no longer pass unnoticed
behind a native rule that was written from a reading of it. Good, because the
three divergences are now stated where a reviewer meets them rather than
inferred from two codebases.

Bad, because the oracle executes lifted source text rather than an imported
module, so a refactor that moves those functions changes the extraction without
changing behaviour — the anchor assertions turn that into a loud failure, but it
is a maintenance cost the import-based precedents do not carry. Bad, because the
oracle compares shapes and verdicts, not encrypted transport or owner clicks;
it is not an interoperability claim.

## Alternatives Considered

Transcribing the expected packets into the fixture by hand: rejected, because a
transcription is a second implementation and the drift it hides is exactly the
drift the oracle exists to catch.

Installing `node_modules` so `bridge-matrix.js` imports directly: rejected for
this slice, because it would make the oracle depend on the dependency tree of a
crate the migration is replacing, and the CI job that runs these checks installs
only production dependencies for the JS oracles.

Comparing only the request packet and skipping the verdict parser: rejected,
because the parser's accept/refuse boundaries — the thirty-two hex bound, the
no-mixed-shapes rule, the action allowlist — are where an interoperability
failure actually lands.

## Amendment 2026-09-14 — the provisioning verdict wire kind (ADR-147)

The native approval wire surface gains a second, distinct verdict kind for
provisioning: **`com.hagency.engagement.approval.v1`**, fields
**`{requestId, decision}`**, accepted **only from the fleet's representative
sender**. It carries the provider's engagement verdict — the approval that makes a
minted engagement effective — and is wholly distinct from the owner-approval v1
profiles this ADR already carries and from the retained execution-verdict kind
`com.hagency.approval.verdict.v1` (`bridge-matrix.js:219-220`), which carries a
running agent's tool-call verdicts, not engagement provisioning. The retained
product has no such wire kind — its provider verdict arrives over HTTP
(`POST /api/engagements/:id/verdict`, `backend-v2.js:15160`) — so this kind is a
native decision; the rationale and evidence are recorded in ADR-147 (a). A verdict
from any sender other than the fleet's representative is refused before `approve`.
