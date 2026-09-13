# approval-loss verdicts

Brief 27's original subject: the five `hagency-execution --lib` approval-loss
scenarios diverging on probe e5d85643 (run 34774943034), root-caused from the
CI observations (`transport_cause`, pending counts, trace). Local execution is
impossible in this sandbox (`DomainRepository::open` → EPERM), so each verdict
below is pinned to the code path the observation names. No assert was weakened;
only product/harness behavior changed.

## approval_loss::native_owned_approval_turn_end_untransmitted

- ubuntu: `Some(ApprovalCancelled)` — `native/hagency-execution/src/approval/control.rs:367`
  (pre-fix): the recheck pump re-mapped the turn-end rule's quiet verdict
  back into `ApprovalCancelled`. The rule itself
  (`native/hagency-execution/src/approval/observations.rs:154-164`) already
  classified a host-side `HostClosed` with zero accepted bytes as quiet
  `Ok(true)`; the pump's `if checked.terminal?` discarded that. Same
  re-mapping existed at the authorize pump (`control.rs:209`) and the begin
  pump (`control.rs:279`).
- Fix: all three pumps now honor the rule's verdict — `return Ok(())` on a
  terminal the rule classified quietly. `ApprovalCancelled` still propagates
  via `?` when the rule's cancel arm itself fired (non-in-flight unwritten
  entry, `observations.rs:126-131`).
- macOS: "untransmitted arm never stamped; trace:" (EMPTY) —
  `native/hagency-execution/src/approval/diagnostics.rs:116-119` documents
  the cause: every owned fixture shared the dispatch id `"dispatch"`, so a
  parallel test's `reset()` wiped this test's journal between stamp and
  assert (the caveat deferred "for the product branch"). Fix: unique
  per-scenario dispatch keys —
  `native/hagency-execution/tests/support/approval_loss.rs:61` (fixture
  takes `dispatch_key`), all 14 call sites.

## approval_loss::native_owned_approval_turn_end_midwrite_uncertain

- ubuntu: `failure: None`, observation `pending_server_requests: Some(1)`,
  `transport_cause: Some(HostClosed)`; macOS: same shape with
  `Some(CleanupUnknown)` (POSIX structurally reports
  `whole_tree_stopped: false`, `native/hagency-platform/src/unix.rs:117-119`,
  so the cleanup gate surfaces there — accepted by the brief).
- Cause: `native/hagency-execution/src/approval/observations.rs:62-68`
  (pre-fix) retired EVERY written entry at turn end
  (`turn-ended-ignored-written`, `continue`) regardless of `resolved`. The
  midwrite probe exits with the frame on the wire, unread and never resolved
  — the pending server request in the observation — so the turn end
  completed silently over a transmitted frame of unknown receipt.
- Fix: `observations.rs:62-82` — written entries are split on `resolved`:
  resolved frames stay the quiet `turn-ended-ignored-written` arm (the
  in-flight-resolution scenario's subject); unresolved written entries stamp
  `turn-ended-unwritten` + `turn-ended-in-flight-uncertain` and the verdict
  arm (`observations.rs:132-166`) routes them to `SettlementUnknown` via a
  `transmitted` check covering `e.write.is_some()` (the termination snapshot
  has no `unconfirmed_write` once the write completed).

## approval_loss::native_owned_approval_in_flight_resolution_completes_write

- macOS: `Some(CleanupUnknown)` — the same POSIX whole-tree cleanup shape as
  above, surfacing at `native/hagency-execution/src/operation.rs:972`
  because the drive completed while the run was still alive. Not a
  classification defect; the `stopped()` custody gate stays as designed
  (42140c03/f3e6098e deliberately keep `whole_tree_stopped` on every
  custody/settlement path).

## approval_loss::native_owned_approval_peer_gone_before_first_byte

- ubuntu: `RunnerAuthority` from `choose()` —
  `native/hagency-execution/tests/support/approval_loss.rs:276` unwraps
  `observe_owner_verdict`; the store refusal sites are
  `native/hagency-store/src/domain/approvals.rs:611-632` (live/binding/route
  checks in `live()`, `approvals.rs:98-111`). The exact conjunct that
  refused is not pinned from this sandbox; treated as a verdict-intake race
  symptom (probe exits immediately after its callback in the
  `owned-approval-eof` mode). Not patched — no assert weakened, no product
  change without a pinned cause.

## Gate evidence (local)

- `cargo check -p hagency-execution --tests` — clean.
- `cargo fmt -p hagency-execution` — applied.
- `cargo clippy -p hagency-execution --tests` — clean.
- Scenario execution: CI-only (sandbox EPERM on the store fixture).
