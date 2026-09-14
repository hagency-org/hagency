---
kind: decision
id: ADR-146
title: "Production callers and the store surface"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
liveness: auto
tags: [store, wiring, spec-governance, definition-of-done]
---

## Context

The 2026-09-14 integration wiring audit found that the provisioning-ingress gap
was not singular: **nine store flows** (G1–G9) write rows that today are reached
only by tests and fixtures. In a real deployment an admitted engagement never
becomes effective, no session route row is ever inserted for the intake plan's
configured session, account readiness stays unknown, a crashed dispatch is never
reconciled, no workspace id is ever registered, saved-grant revocation has no
route, and operator stops are fenced but never settled. Alongside the gaps, a
second class accumulated silently: store methods **superseded by a wired newer
path** that were never marked or deleted, so the store surface overstates what
production actually drives.

The spec suite named these writes in `Then` lines and proved them with tests —
correctly, per method — but nothing checked that a *production* caller exists.
"Tested" and "wired" diverged without any gate noticing.

**Numbering note.** This decision was allocated "ADR-145" by integration, but
`adr-145-console-readiness-version-strip.md` is committed and referenced from
the console-readiness spec and the migration plan (§14); this decision takes
ADR-146, the next unallocated number.

## Decision

**(a) The production-caller rule.** A spec `Then` line that names a store write
carries a `Production caller:` line naming the production function
(`crate::path::fn`) that reaches the write. A checker
(`native/scripts/check-production-callers.mjs`, owed by
`specs/task-rust-production-callers-check.spec.md`) fails when that caller is
absent from the production call graph, computed with tests, fixtures and the
bootstrap probe stripped. `Production caller: owed (Gn)` is the only exemption:
it marks a tracked gap and is reported, not failed.

**(b) The supersession table.** Every method named in the audit is classified
below as **superseded-by** (the wired replacement, with file:line of the
production caller), **gap** (G-number — the write has no production caller and
no wired replacement), or **delete** (decision owed to the named owner; none
decided here). Verification per method: the bare-name grep

```
grep -rn '\.<name>(\|::<name>(' native --include='*.rs' | grep -v tests/
```

is only the starting enumeration — it under-strips (paths containing `tests/`
only, so `#[cfg(test)]` modules and `#[test]` fns inside production files still
hit; e.g. the literal command returns accounts.rs:1273, inside `mod tests`, as
a false production hit for `approve`) and it collides on bare names (see the
`admit` row). Every row below was therefore judged with the checker spec's
strip: `#[cfg(test)]` items and `#[test]` fns removed, `*/tests/*`,
`native/fixtures/**`, and the probe/fixture binaries
(`hagency/src/bootstrap/driver.rs`,
`hagency-platform/src/bin/hagency-platform-probe.rs`,
`hagency-platform/src/bin/hagency-cgroup-probe.rs`,
`hagency-progress-runtime/src/bin/hagency-progress-probe.rs`,
`hagency-runtime/src/bin/hagency-runtime-probe.rs`,
`hagency-runtime/src/bin/approval_probe/`,
`hagency/tests/fixtures/*.rs` `[[bin]]` peers) excluded before calling any hit
"production". Nothing below is asserted without that check, and bare-name
collisions are resolved per type in the row's note.

### Gaps — store writes with no production caller and no wired replacement

| Method | Defined | Production reachability (grep result) | Class |
|---|---|---|---|
| `admit` | domain.rs:1084 | the engagement-minting `DomainRepository::admit` has no production caller; the bare-name grep's outside-store hits are other types — `MediaBudget::admit` (hagency-media/src/lib.rs:122,145) and `control.admit` (hagency-runtime/src/codex/session/driver.rs:480) — not the store method | gap G1 — glm8 wiring in progress |
| `approve` | domain.rs:1145 | none outside tests/fixtures | gap G2 |
| `claim_effect` | domain.rs:1338 | none outside tests/fixtures | gap G2 |
| `observe_effect` | domain.rs:1357 | none outside tests/fixtures | gap G2 |
| `retry_cleanup` | domain.rs:1265 | none outside tests/fixtures | gap G2/G5 (shared) |
| `resolve_verified_matrix_session` | domain/matrix_routes.rs:471 | only the bootstrap probe (hagency/src/bootstrap/driver.rs:393) | gap G3 |
| `begin_account_login` | domain/accounts.rs:785 | none outside tests/fixtures | gap G4 |
| `settle_account_login` | domain/accounts.rs:824 | none outside tests/fixtures | gap G4 |
| `reconcile_dispatches` | domain/execution.rs:1014 | none outside tests/fixtures | gap G5 |
| `recover_dispatch` | domain/execution.rs:1025 | none outside tests/fixtures | gap G5 |
| `shutdown_observed` | worker.rs:128, domain_worker.rs:2526 | none outside tests/fixtures | gap G5 |
| `register_workspace` | domain/execution.rs:670 | only the bootstrap probe (hagency/src/bootstrap/driver.rs:414) | gap G6 |
| `revoke_approval_grant` | domain/approvals.rs:676 | none outside tests/fixtures | gap G7 |
| `settle_conversation_stop` | domain/conversation_lifecycle.rs:367 | none outside tests/fixtures | gap G8 |
| `pending_conversation_stops` | domain/conversation_lifecycle.rs:354 | none outside tests/fixtures | gap G8 |

15 distinct methods, 8 open gaps (G9 resolved as superseded, below).

### Superseded — a wired newer path exists; the old method keeps only test/fixture callers

| Method | Defined | Wired replacement (production caller) |
|---|---|---|
| `claim_dispatch` | domain/execution.rs:730 | `admit_owned_dispatch` (domain/owned_dispatch.rs:302) ← hagency-execution/src/operation.rs:706 |
| `start_dispatch` | domain/execution.rs:867 | `admit_owned_dispatch` ← hagency-execution/src/operation.rs:706 |
| `complete_dispatch` | domain/execution.rs:964 | `admit_owned_dispatch` ← hagency-execution/src/operation.rs:706 |
| `renew_dispatch` | domain/execution.rs:918 | `admit_owned_dispatch` ← hagency-execution/src/operation.rs:706 |
| `park_dispatch` | domain/execution.rs:888 | `admit_owned_dispatch` ← hagency-execution/src/operation.rs:706 |
| `fail_before_start` | domain/execution.rs:939 | `admit_owned_dispatch` ← hagency-execution/src/operation.rs:706 |
| `enqueue_dispatch` | domain/execution.rs:722 | `admit_owned_dispatch` ← hagency-execution/src/operation.rs:706 (remaining caller is the bootstrap probe, hagency/src/bootstrap/driver.rs:416) |
| `ingest_message` | domain/messages.rs:152 | `admit_matrix_event` (domain/verified_ingress.rs:261) ← hagency-matrix/src/intake.rs:333 |
| `create_task_intent` | domain/task_intents.rs:399 | `admit_matrix_event` ← hagency-matrix/src/intake.rs:333 |
| `observe_owner_verdict` | domain/approvals.rs:492 | `admit_approval_verdict` (domain/approvals.rs:964) ← hagency-matrix/src/approval_intake.rs:630 |
| `claim_final_reply` | domain/replies.rs:219 | verified send lane: `begin_final_reply_send` (domain/replies.rs:270) ← hagency-matrix/src/outgoing.rs:253; settlement `reconcile_final_reply` ← hagency-matrix/src/outgoing.rs:536 |
| `observe_final_reply` | domain/replies.rs:290 | `reconcile_final_reply` ← hagency-matrix/src/outgoing.rs:536 |
| `cancel_final_reply` | domain/replies.rs:321 | verified send lane fences on re-`begin_final_reply_send` ← hagency-matrix/src/outgoing.rs:253 |
| `claim_task_notice` | domain/task_intents.rs:548 | verified notice lane: `begin_verified_task_notice_send` (domain/notice_custody.rs:145) ← hagency-matrix/src/outgoing.rs:266 |
| `deliver_task_notice` | domain/task_intents.rs:589 | `validate_verified_task_notice_send` (domain/notice_custody.rs:175) ← hagency-matrix/src/outgoing.rs:438 |
| `fail_task_notice` | domain/task_intents.rs:647 | verified notice lane (notice_custody.rs) ← hagency-matrix/src/outgoing.rs:266,438 |
| `retry_task_notice` | domain/task_intents.rs:688 | verified notice lane (notice_custody.rs) ← hagency-matrix/src/outgoing.rs:266,438 |
| `put_resource` | domain.rs:767 | `configure_resource` ← hagency/src/console/resource_configuration.rs:170; `publish_resource` ← hagency/src/console/resources.rs:331 |
| `register_session` | domain/execution.rs:639 | superseded by the verified route model admitted via `admit_matrix_event` ← hagency-matrix/src/intake.rs:333; its replacement writer is itself gap G3 |
| `resolve_session` | domain/messages.rs:123 | same as `register_session` — superseded by the verified route model; writer is gap G3 |
| `usage_source` | domain/usage/reads.rs:77 | `usage_report` (domain_worker.rs:2982) ← hagency/src/usage.rs:37, hagency/src/console/usage.rs:126 |
| `restore_usage_source` | domain/usage.rs:123 | `usage_report` ← hagency/src/usage.rs:37, hagency/src/console/usage.rs:126 |
| `upload_stage_commitment` | domain/uploads.rs:673 | wired upload lane: `reserve_upload` ← hagency-matrix/src/upload/operation.rs:49; `observe_upload_staged` ← hagency/src/file_service/pipeline.rs:301 |
| `inspect_upload_settlement` | domain/uploads.rs:655 | `record_upload_settlement` (domain/uploads.rs:661) ← hagency-matrix/src/upload/operation.rs:150 |
| `bind_upload_stage` | domain/uploads.rs:683 | `observe_upload_staged` ← hagency/src/file_service/pipeline.rs:301 |
| `cancel_upload` | domain/uploads.rs:736 | `cancel_file_delivery` (hagency/src/file_service) + lane fences ← hagency/src/file_service/pipeline.rs |
| `restore_upload` | domain/uploads.rs:611 | `claim_upload`/`begin_upload` recovery ← hagency/src/file_service/pipeline.rs:324,330 |
| `record_upload_acceptance` | domain/uploads.rs:726 | `record_upload_settlement` ← hagency-matrix/src/upload/operation.rs:150 |
| `create_verified_task_intent` | domain/verified_ingress.rs:454 | **G9 resolved — superseded:** runner-command task lane `delegate_task` (domain/task_intents.rs:411) ← `RunnerCommand::Delegate`, hagency-store/src/domain_worker.rs:2096 ← hagency/src/runner.rs; spec claim checked: no spec row names `verified_task_requests` |
| `attach_task_inputs` | domain/task_intents.rs:466 | **G9:** `send_peer`/`delegate_task` runner-command lane ← hagency-store/src/domain_worker.rs:2078,2096; no spec row names `task_input_receipts` |
| `enqueue_peer_dispatch` | domain/peers.rs:348 | **G9:** `send_peer` (domain/peers.rs:291) ← `RunnerCommand::SendPeer`, hagency-store/src/domain_worker.rs:2078 ← hagency/src/runner.rs:73 |

31 superseded methods. Each carries the one-line supersession note above;
deletion is a separate decision, not made here.

### Delete — decision owed

| Method | Defined | Note |
|---|---|---|
| `transfer_inputs` | domain/peers.rs:131, domain/messages.rs:106 | `pub(super)` helper of the superseded peer-dispatch path; delete together with `enqueue_peer_dispatch` when glm5 confirms the G9 supersession |

Counts: **31 superseded / 15 gap methods across 8 open gaps (G1–G8) / 0
deleted (1 deletion decision owed)**.

**(c) Definition of done.** The migration plan's §12 gains one line making this
a gate, not an audit: a store write is not "done" when a test reaches it but
when a production caller does (see the §12 bullet added with this ADR).

## Consequences

- The nine-flow audit becomes a standing gate: new store writes cannot merge
  behind test-only reachability once the owed checker lands.
- G1–G8 stay owned as scheduled (glm8 G1–G3, glm7 G4, glm9 G5/G6/G8, glm5 G7);
  G9 is closed as superseded, with glm5 owning the deletion decision for
  `enqueue_peer_dispatch` + `transfer_inputs` and the supersession notes above.
- `register_session`/`resolve_session` are superseded *by design* even though
  their replacement writer is itself gap G3 — the table records both facts
  rather than letting the legacy pair look like the live path.
- Specs carrying owed scenarios use `Owed Selector:` lines (never `Test:`), so
  the Rust binding checker neither passes nor demands them until wiring lands.
