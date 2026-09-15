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
(`hagency-platform/src/bin/hagency-platform-probe.rs`,
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
| `admit` | domain.rs:1084 | the engagement-minting `DomainRepository::admit` is now called from production | **closed G1** (2026-09-14): hagency-matrix/src/intake.rs:275 |
| `approve` | domain.rs:1145 | none outside tests/fixtures | gap G2 |
| `claim_effect` | domain.rs:1338 | none outside tests/fixtures | gap G2 |
| `observe_effect` | domain.rs:1357 | none outside tests/fixtures | gap G2 |
| `retry_cleanup` | domain.rs:1265 | none outside tests/fixtures | gap G2/G5 (shared) |
| `resolve_verified_matrix_session` | domain/matrix_routes.rs:471 | only bootstrap driver setup code (hagency/src/bootstrap/driver.rs:393 — see the amendment: that file is production, and its calls are bootstrap configuration, not the runtime route insert) | gap G3 |
| `begin_account_login` | domain/accounts.rs:785 | now called from production | **closed G4** (2026-09-14): hagency/src/bootstrap/accounts.rs:64 |
| `settle_account_login` | domain/accounts.rs:824 | now called from production | **closed G4** (2026-09-14): hagency/src/bootstrap/accounts.rs:100 |
| `reconcile_dispatches` | domain/execution.rs:1014 | none outside tests/fixtures | gap G5a (operator recovery and resume) |
| `recover_dispatch` | domain/execution.rs:1025 | none outside tests/fixtures | gap G5a (operator recovery and resume) |
| `shutdown_observed` | worker.rs:128, domain_worker.rs:2526 | read-only snapshot — writes nothing (worker.rs:128-132); all call sites are inside `#[cfg(test)]` modules (worker.rs:374 covers :391/:410/:472/:493) | immaterial — not a gap |
| `register_workspace` | domain/execution.rs:670 | now called from production | **closed G6** (2026-09-14): hagency/src/bootstrap.rs:873 (receive-inbox plan workspace before the first claim; the older driver.rs:414 note was setup-code observation, superseded) |
| `revoke_approval_grant` | domain/approvals.rs:676 | now called from production | **closed G7** (2026-09-14): hagency/src/console/approvals.rs:139 (console route) |
| `settle_conversation_stop` | domain/conversation_lifecycle.rs:367 | now called from production | **closed G8** (2026-09-14): hagency/src/bootstrap/driver.rs:367 |
| `pending_conversation_stops` | domain/conversation_lifecycle.rs:354 | now called from production | **closed G8** (2026-09-14): hagency/src/bootstrap/driver.rs:358 |

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
| `enqueue_dispatch` | domain/execution.rs:722 | `admit_owned_dispatch` ← hagency-execution/src/operation.rs:706 (remaining caller is bootstrap driver setup, hagency/src/bootstrap/driver.rs:416 — same amendment) |
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

## Amendment 2026-09-14 — `bootstrap/driver.rs` is production

The original strip list misclassified `native/hagency/src/bootstrap/driver.rs`
as a probe. It is the production host driver (`Driver::start`, `run`,
`settle_pending_stops`, the claim at driver.rs:257); only its `#[cfg(test)]
mod tests` is test code, and the checker's cfg(test) stripping already removes
that. The file is therefore **not stripped**; its fns participate in the
production call graph like any other. The gap rows above that cited it as
"the bootstrap probe" keep their classification: the calls there are
bootstrap-time configuration (fixture-shaped development bootstrap), not the
runtime route/session insert the intake plan needs — G3 and G6 stay gaps on
that reasoning, now stated correctly. (Checker defect found by its first real
input, reported by integration + glm9.)

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

## Amendment 2026-09-14 (second) — audit re-run on the landed head (88e05d5f)

The audit was re-run after ~40 commits. Method unchanged (strip `#[cfg(test)]`
items and `#[test]` fns; exclude tests/, native/fixtures/, examples/, probe
bins; every finding grep-confirmed per the original evidence rule).

**Closed gaps** (production caller now reaches the write):

| Gap | Method | Production caller (evidence) |
|---|---|---|
| G1 | `admit` | `hagency-matrix/src/intake.rs:275` (`self.domain.admit(verified, msg.origin_ts)` in the provisioning ingress) |
| G4 | `begin_account_login` | `hagency/src/bootstrap/accounts.rs:64` |
| G4 | `settle_account_login` | `hagency/src/bootstrap/accounts.rs:100` |
| G6 | `register_workspace` | `hagency/src/bootstrap.rs:873` (receive-inbox plan workspace registered before the first claim) |
| G7 | `revoke_approval_grant` | `hagency/src/console/approvals.rs:139` (console route) |
| G8 | `pending_conversation_stops` | `hagency/src/bootstrap/driver.rs:358` |
| G8 | `settle_conversation_stop` | `hagency/src/bootstrap/driver.rs:367` |

**Correction to the brief (twice revised)**: the crash-reconciliation
BEHAVIOUR is production-reached, and the duplicate-effect clause of the DoD is
satisfied by it: the driver's claim (`hagency/src/bootstrap/driver.rs:257`,
also :573, :637) → `claim_owned_clock` (`execution.rs:765`) → `claim_clock`
(:783) → `expire(&tx, now)` (:810), called before the candidate select;
`expire` (:228) selects every row in ('leased','started','parked') whose
`lease_until` or `capability_until` has elapsed and drives it through `lose`
(:189-203) to `outcome_unknown`, quarantining the session. An expired-lease
reclaim therefore exists at `execution.rs:810`; the earlier clause "no
expired-lease reclaim or crash reconciliation exists in any production path"
is dropped as wrong. `shutdown_observed` is a read-only snapshot
(`worker.rs:128-132` — it writes nothing, only delegates to
`shutdown_tracked` and snapshots a probe), and all its call sites are inside
`#[cfg(test)]` modules (the marker at `worker.rs:374` covers :391/:410/:472/
:493) — so its absence is immaterial to the gap.

What has no production path at all is RECOVERY AND RESUME. `lose` settles the
orphan to `outcome_unknown` and quarantines the session — satisfying the
duplicate-effect half of the DoD — but performs none of what `recover_dispatch`
(`execution.rs:1025`) additionally does: `DELETE FROM resource_leases` (:1102),
clearing `quarantined=0` (:1107), clearing `dirty=0` (:1110), the supersede
write (:1112), the replacement enqueue (:1115), and the recovery records
(`INSERT INTO dispatch_recoveries` :1118, `INSERT INTO dispatch_recovery_reports`
:1120). Without them the orphan stays in `unresolved_dispatches`, keeps
counting against the live cap the claim query checks (`execution.rs:812`), and
the session stays quarantined — permanently, with no production path to
resume. An orphaned dispatch is currently settled but never resumed. That gap
is **G5a (operator recovery and resume)**, open. G2
(`approve`/`claim_effect`/`observe_effect`/`retry_cleanup`) confirmed still
unreached (under review); G3 (`resolve_verified_matrix_session`) still has
only the bootstrap driver setup call (`driver.rs:432`) — the first amendment's
reasoning stands.

**New rows** (unreached writes the original audit did not list):

| Method | Defined | Finding | Class |
|---|---|---|---|
| `create_coordinator_task` | domain/execution.rs:693 | facade domain_worker.rs:2206 has no production caller; no coordinator-task creation path is wired | **new gap** (owner unassigned) |
| `record_late_output` | domain/execution.rs:982 | facade domain_worker.rs:2308 has no production caller; no late-output recording path is wired | **new gap** (owner unassigned) |
| `enqueue_inbox_dispatch` | domain/messages.rs:283 | **superseded-by** `select_receive_inbox` → `select_receive` (messages.rs:647, which performs the `enqueue_inbox` write at :703/:741) ← `hagency/src/bootstrap/inbox.rs:31`; the direct facade is a stale variant | superseded |

Not findings: `mutate_task` is wired through `RunnerCommand::Mutate`
(`hagency/src/runner.rs:416`); the remaining candidate names are read APIs
(`runner_*`, `retention_status`, `canonical_task`, `effect`,
`delivery_denial_reason`, `is_admitted`, …), internal helpers (`counts_*`,
`grow`, `period_keys`, `zero`, `outbound_at`, `queue_remaining`, the
`pub(super)` `enqueue_inbox`), or rows already classified above.

**Checker reconciliation**: `check-production-callers.mjs` on this head reads
exit 0, count 14, wired 11, owed G2/G2/G3 — no discrepancy. The checker only
sees spec-named `Production caller:` lines, and no spec line names the G5a
recovery-artifact writes or the two new gaps, so neither tool contradicts the
other; the asymmetry (audit covers the whole store surface, checker covers
spec-named writes) is the intended division of labour.
