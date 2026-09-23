---
kind: decision
id: ADR-181
title: "Every owned attempt leaves evidence a lost agent can be diagnosed from"
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [native, execution, guardian, store, diagnostics]
---

## Context

A week of live two-agent Codex runs lost agents to two signatures the product
could not explain: `cleanup_unknown` after a completed task, and
`lost_authority` with `stop_cause: requested`. The architectural review of
2026-09-22 (`docs/reviews/2026-09-22-native-codex-architecture-review.md`,
gap G6) found why every fix was a guess: the crates that execute a dispatch
(`hagency-execution`, `hagency-runtime`, `hagency-platform`) cannot log; the
only record of a failure is one WARN line with fixed labels
(`bootstrap/driver.rs:704-705`); the guardian's stderr is `/dev/null` by
construction and its exit status is never read (`supervisor/unix.rs:193-194,
269, 285, 448, 471`); the `Stopped` frame has no field saying why a stop after
`LeaderExited` did not prove the tree gone (`supervisor/unix/macos.rs:159-163`);
`Cleanup::Unknown`'s error kind is dropped (`bootstrap.rs:603`); some forty
`map_err(|_| Failure::LostAuthority)` sites erase whether the store said
revoked, timed out or busy; and the store keeps one collapsed word in
`runner_outputs` (`domain/owned_dispatch.rs:581-588`) with no phase
timestamps.

The retained product records, per dispatch: `runner_activity` phases with a
10 s heartbeat (`router/src/activity.ts`), `started_at`/`parked_at`/
`settled_at` on the dispatch row, and a `terminal_reason` carrying the exit
identity and the last 500 characters of the runner's stderr
(`router/src/runner.ts:484`); its guardian writes its refusal to a pipe the
parent keeps (`router/src/runner-guardian.ts:142,151`).

ADR-175 fixed the rule that diagnostics are fixed categories, never free
text lifted from an unsafe snapshot, and ADR-040 made the runtime's stderr
private state with no automatic console or log projection. Both stand; this
record decides where bounded evidence is kept so an operator can read it.

## Decision

1. **A per-attempt event log in the store.** New table
   `runner_attempt_events(dispatch_id, fence, seq, at_ms, phase, detail)`,
   `PRIMARY KEY(dispatch_id, fence, seq)`, `detail` a bounded JSON object of
   fixed keys. Phases, in the order an attempt can visit them: `claimed`,
   `spawn_started`, `spawn_done`, `initialized`, `turn_started`,
   `approval_requested`, `approval_decided`, `parked`, `resumed`,
   `stop_requested`, `stop_reported`, `settled`, `failed`, `lost`. The host
   writes them through the domain worker as best-effort observations in their
   own savepoint: a write that fails never changes the attempt's outcome and
   is counted, not retried. Retention prunes them with the dispatch (ADR-053
   amendment §3 window).
2. **The attempt row carries its clock.** `runner_attempts` gains
   `started_at`, `parked_at`, `last_renew_at`, `settled_at` and
   `terminal_reason`. `terminal_reason` is the retained product's shape:
   `<failure>:<exit identity>:<stderr tail>`, where the failure is the
   uncollapsed execution `Failure` name, the exit identity is the leader's
   exit code or signal as the guardian reported it, and the tail is the last
   512 bytes of the runtime's retained stderr with control characters
   replaced. It is written once, at settlement or failure, into the private
   store only; ADR-040's rule stands for every other projection.
3. **The failure is persisted uncollapsed.** The `failed` event's detail holds
   the `Failure` variant, `settlement`, `settlement_cause`, `protocol`,
   `cleanup` with the `io::ErrorKind` of an `Unknown`, the guardian's
   `stop_cause`, `stop_detail` and the three report booleans, and the runtime
   observation (`stage`, `session_error`, `transport_cause`,
   `server_request`, pending counts) — the same labels the status already
   uses, so nothing new is invented, only kept. The `runner_outputs` word
   stays for compatibility.
4. **Lost authority names its site and cause.** `Failure::LostAuthority`
   becomes `LostAuthority { site, cause }`: `site` is a fixed label of the
   check that failed (`lease_renew`, `dispatch_check`, `account_check`,
   `local_codex_check`, `task_mcp_bind`, `warm_root`, `warm_scope`,
   `warm_provision`, `warm_qualify`, `warm_ready`, `warm_activate`,
   `warm_dispatch`, `command_channel`, `approval_bind`, `approval_expiry`,
   `approval_maintain`, `approval_request`, `approval_response`,
   `approval_begin`, `approval_check`, `approval_application`,
   `factory_account`, `host_claim`), `cause` the store's own word
   (`revoked`, `generation`, `quarantined`, `state`, `busy`, `unavailable`,
   `timed_out`, `not_found`, `conflict`, `locked`, `io`, `other`). The status
   gains `authority_site` and `authority_cause` beside the unchanged
   `owned_failure = lost_authority`. This changes no verdict: every site still
   fails; it only stops discarding the reason.
5. **The guardian is heard.** (a) The host reads the guardian child's exit
   status after the `Stopped` frame or on channel loss and records it with
   `stop_reported`. (b) `Reply::Stopped` gains `refusal`, a fixed category
   naming why a stop after `LeaderExited` did not prove the tree gone —
   `census_error`, `tracker_gap`, `signal_error`, `live_descendants`,
   `root_unreaped`, `none` — and a bounded list (at most eight) of the rows
   that kept it live: pid, parent pid, and the executable's file name only.
   (c) The guardian's stderr is a pipe to the host, sealed CLOEXEC so the
   work never inherits it (the work's own stdio is dup2'd separately); the
   host keeps the last 4 KiB and records it with `stop_reported`, control
   characters replaced. A guardian with no host pipe writes nothing and
   behaves as today. A refusal or a stderr line never authorizes anything and
   never strengthens `StopReport` (ADR-029 rule, unchanged).
6. **Lease loss names its writer.** When `expire()` or `lose()` settles a
   started or parked dispatch, the `lost` event records which writer call did
   it (`claim`, `reconcile`, `restart`), `lease_until`, `now` and
   `last_renew_at`.
7. **The executing crates log.** `hagency-execution`, `hagency-runtime` and
   `hagency-platform` (host side) gain a `tracing` dependency and emit one
   INFO line per phase transition with dispatch id, fence, engagement and
   elapsed milliseconds, and a WARN with the same fixed labels as the event
   at every failure. The guardian process installs no subscriber; its pipe is
   its log. The service's subscriber and `RUST_LOG` are unchanged.

## Consequences

Good, because a lost agent can be diagnosed from a copy of the store and the
service log alone: which check failed, what the store said, when each phase
happened, what the guardian saw and how it exited, what the runtime wrote
last. The two open signatures of the review become answerable from their
next occurrence without a private build.

Good, because no verdict changes: this record adds evidence beside every
existing rule (ADR-029, 040, 053, 060, 096, 175) and authorizes nothing.

Neutral, and said so it is not read as a baseline: the verdicts this evidence
now names are themselves owed to later slices of the closing order
(`docs/reviews/2026-09-23-adr-failure-model-consistency.md`). That a lease
renewal answered `busy`, `timed_out` or `unavailable` is recorded as lost
authority today; whether it should end the attempt at all is F11/G3. That an
`approval_request` refusal stops the runner is F4/G4. That unproven cleanup
retains the owner is F2/G2. This record makes those facts visible; it does not
decide them.

Bad, because the store grows by one row per phase per attempt and the guardian
gains a pipe; both are bounded (event count per attempt, 4 KiB tail, eight
rows) and pruned with the dispatch.

Bad, because `terminal_reason` puts up to 512 bytes of runtime stderr into the
private store, which ADR-040 kept out of every projection; the store is
operator-private and the retained product keeps 500 characters in the same
place, so parity is the reason and the bound is the limit.

## Alternatives Considered

- *Keep patching with private `eprintln!` builds.* Rejected: it found nothing
  in three days on the guardian signature and cannot run on a host that is
  not the developer's.
- *Log everything at DEBUG and rely on `RUST_LOG`.* Rejected alone: logs are
  not persisted with the attempt, the guardian has no subscriber, and the
  store is what an operator reads through the console.
- *Free-text reasons.* Rejected: ADR-175's fixed-category rule stands; the
  only bounded text admitted is the stderr tail, exactly as the retained
  product keeps it.
