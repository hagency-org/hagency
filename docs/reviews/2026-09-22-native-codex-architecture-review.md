# Native Codex execution: architectural review of the fundamental gaps

Scope: why the Rust port keeps losing Codex agents in long live runs although
every function has been ported and pinned. Read-only review of the port
(`native/`, `knowledge/decisions`, `docs/design/native-execution-parity.md`)
against the retained product (`backend-v2.js`, `lib/`, `router/src`), by four
independent deep-dives (process custody, approval leg, failure model and
restart, observability and timing), cross-checked against the week's live
findings (2026-09-16 to 2026-09-22). Nothing here is built; every claim is a
file:line in this worktree (`R` = `native/`, `K` = `knowledge/decisions`) or
in the retained tree (`T` = the retained repository root).

## The thesis

The port re-implemented the retained product's mechanisms one by one and
inverted its failure philosophy at four joins. The retained product's rule is:
**a dispatch can fail, a session can be quarantined, the agent lives, other
agents are untouched, and the process always exits.** The port's rule, decided
piecemeal across ADR-036, 053, 060, 096 and code with no ADR, is: **any fact
the host cannot prove ends the agent's worker, marks the fleet failed, and
retains the process.** A week of live runs found symptom after symptom of that
inversion (guardian census refusals, cleanup unknown, lost authority, pump
death, Generation cascades, SIGTERM refused) and each was patched at the
symptom. The 99/101-round soak on `46668c9c` still lost two fleets, one to
each of the two remaining un-diagnosed signatures, and both are instances of
the same design, not new bugs.

## Fundamental gaps, ranked by how much of the observed instability they explain

### G1. The agent *is* its attempt loop; there is no per-dispatch containment

- Rust: `run_continuous` returns on any `Err` of one attempt and on any report
  that is failed, unsettled or not physically stopped
  (`R/hagency/src/bootstrap/driver.rs:302-389`); the thread then only waits
  for `Close` (`:191-207`). Recovery (`awaiting_operator`, ADR-169) exists
  only when the tree is proven stopped and an inspection is recorded
  (`:373-380`), and suspends intake meanwhile.
- Retained: a runner failure settles the *dispatch* as `outcome_unknown`,
  blocks the task, quarantines the *session*, and the agent keeps claiming
  (`T/router/src/store.ts:2623-2660`, `T/backend-v2.js:2735-2756`); the agent
  is marked down only for an unconfirmed cleanup (`T/backend-v2.js:2717-2733`).
- Decided as a deferral: ADR-036 `:292-298` ("A failed turn still ends the
  attempt as a protocol failure with its owner retained, which ends the agent's
  worker"), reaffirmed ADR-180 `:130-133`; parity doc `:911-927` names the
  retained behaviour and defers it.
- Explains: usage-limit ending an agent (09-20), every failed turn ending an
  agent, `outcome_unknown` fleets, soaks having to run as fresh fleets.
- Classification: **parity change**, already described, never scheduled.

### G2. Unproven cleanup is a terminal, unresolvable in-process state that vetoes shutdown

- The guardian is one-shot: it reports once and exits
  (`R/hagency-platform/src/supervisor/unix.rs:589-609`); the host caches the
  verdict forever (`:405-408`; `R/hagency-runtime/src/owned/session.rs:166-172`;
  `R/hagency-execution/src/operation.rs:392-420`), so `retry_stop` can never
  change it. The proof needs two consecutive error-free whole-system censuses
  inside 2 s during the tree's own teardown, and a single row error, signal
  errno or tracker gap latches failure permanently
  (`R/hagency-platform/src/supervisor/unix/macos.rs:77-83,116-147,153-166`;
  `macos/native.rs:218-224`; `macos/tracking.rs:60-65`).
- The three-fact predicate (`operation.rs:439-441`, ADR-060 `:99-102`) then
  retains the owner (`:383-390`), returns `CleanupUnknown` even after a held
  completion (`:1378-1387,1415-1417`), ends the driver (G1), fails the fleet
  (`R/hagency/src/bootstrap/fleet.rs:45-50,106-129`) and makes SIGTERM
  ineffective: the driver answers `Err(OutcomeUnknown)` at close
  (`driver.rs:194-206`), drain fails (`fleet.rs:463-466`), `serve` never calls
  `stop_graceful` (`R/hagency/src/bootstrap.rs:1551-1559`), and `main.rs:297-303`
  parks on `std::future::pending`. Decided in ADR-096 `:160-165,200-206`
  ("No macOS unknown cleanup is called a complete shutdown"), ADR-053
  `:159-163`.
- Retained: the same receipt is bounded (8 s wait for the guardian's close,
  `T/router/src/runner.ts:368-377`), the guardian exits 125 with the reason on
  stderr (`T/router/src/runner-guardian.ts:139-146`), the consequence is one
  dispatch unknown and one agent fenced (`T/backend-v2.js:2717-2756`), and
  shutdown always completes (`:17386-17416,17622-17624`).
- Explains: `cleanup_unknown` after completed tasks (09-19 ×3, 09-22), every
  "service refused SIGTERM" (09-19, 09-20, 09-22), zombie guardians with exit 1.
- Classification: bounding the receipt and scoping it to the dispatch is
  **parity**; giving up the retained-owner rule (ADR-096) is a **new operator
  decision**, because the retained product never had one — its whole answer is
  "record, fence the agent, exit".

### G3. Authority-first coupling: any store or observation error stops a healthy tree

- In-run, every 100 ms, the lease renewal maps *any* store error to
  `LostAuthority` (`operation.rs:845-855`), including `Busy` from a full writer
  queue or byte budget (`R/hagency-store/src/domain_worker.rs:2677-2681,
  2701-2707`) and the 2 s reply bound (`:2708-2722`); the host then stops the
  tree on purpose (`operation.rs:1325` → guardian `StopCause::Requested`,
  `unix.rs:544`). Warm idle re-qualifies the owner every 100 ms under
  `response_ms` (hard-capped 2 s, `R/hagency-execution/src/host.rs:38`) with a
  synchronous guardian round trip that is answered only between whole-system
  censuses (`warm.rs:599-606,526-528`; `unix.rs:538-543`), and one late reply
  latches the owner unusable (`unix.rs:256-265`). About forty
  `map_err(|_| Failure::LostAuthority)` sites erase the store's own
  distinction between revoked, timed out and busy.
- Retained: no continuous host-side qualification of a running guardian
  exists; the runner lease is 20 min (`T/backend-v2.js:246`), liveness is a
  120 s heartbeat TTL with a 30 s grace (`T/lib/supervisor-lifecycle-manager.js:14`,
  `T/lib/agent-state.js:14`).
- No ADR decides either coupling (ADR-113 only splits `response_ms`; ADR-029
  `:201-204` records the live occurrence).
- Explains: `lost_authority` under load (09-20 offline, memory
  `warm-idle-qualify-is-load-sensitive`), the `stop_cause: requested`
  signature, and the fatal end of the parked approval in G4.
- Classification: **parity change** (bounded, coarse liveness; refusals are
  facts about the store, not about the process).

### G4. The owner-approval leg is structurally complete and operationally fatal

- Path: parse → runtime callback → execution coordinator → store admission →
  service pump → encrypted card → verdict intake → response. Every step has a
  production caller. But the coordinator maps a store refusal of the *request*
  to `LostAuthority` and stops the runner
  (`R/hagency-execution/src/approval/observations.rs:204-221`, `:267-271`),
  discarding the reason; the retained runner treats a park refusal as a
  *decline* and continues (`T/router/src/runner.ts:660-664`).
- The 2026-09-22 loss is exactly that: `server_request: command_approval`,
  dispatch never `parked` (no `park_reason`), `owner_approvals` empty, approval
  room available with owner and bot joined, agent lost 44 s into the round with
  the rig's owner wait at 180 s. The admission refused. The most likely
  refusal, unproven: the host context pins `environment_id: None`
  (`operation.rs:1238`) while the store refuses any request whose
  `environmentId` differs from it (`R/hagency-store/src/domain/approvals.rs:336-354`)
  and the parser admits the field (`R/hagency-runtime/src/codex/approval.rs:146`);
  the retained product folds `environmentId` into its scope key
  (`T/lib/execution-authorization.js:101-107`). No Codex command approval has
  ever been seen to reach an owner through the port.
- Also: default `approval_owner_wait_ms` = 1000 (`R/hagency/src/bootstrap/config.rs:63-65`)
  against the retained 5 min TTL + 60 s; the single approval pump stops on the
  first send or intake error and every agent's next dispatch then fails
  `OutcomeUnknown` (`R/hagency/src/bootstrap/approval.rs:238-248,289-297`;
  `driver.rs:595-601`); the agent-facing MCP approval tools answer "Approval
  host leg is not wired" (`R/hagency/src/mcp.rs:462-479`, ADR-064 PC-C3
  `:192-196`) with no tracking record; one `operation_ms` covers the approval
  wait, so an approval raised late in a turn is a `Deadline`
  (`R/hagency-execution/src/approval/state.rs:232-236`).
- Classification: refuse-as-decline, `environmentId` scoping and independent
  approval clocks are **parity**; the wait default is config.

### G5. Fleet and process single points of failure, and a persisted Matrix fence

- A fresh agent's admission failure ends `serve` (`fleet.rs:439` `?` →
  `bootstrap.rs:1534-1550`); the approval pump is one receiver
  (`bootstrap.rs:1516-1521`); one lost agent makes `/ready` 503
  (`fleet.rs:48-50,111-114`; `R/hagency/src/lib.rs:409-417`). None of these is
  decided in an ADR. Negative Matrix evidence is persisted as
  `matrix_transports.available=0`, survives restart, and only an operator
  generation rotation recovers it (`R/hagency-matrix/src/collector.rs:167-174,
  274-335`; `R/hagency-store/src/domain/matrix_routes.rs:172-227`; ADR-047
  `:284-286`); one worker's Transport took the others with `Generation`
  within 240 ms (parity doc `:717-718`).
- Retained: unbounded exponential backoff, no persisted fence
  (`T/lib/appservice-sync.js:463-522`); a single agent's account failure is
  skipped (`T/bridge-matrix.js:4284-4296`); no global failed flag.
- Explains: fleets dying at 6–16 min on a bursty endpoint (09-19), the
  fresh-fleet soak discipline, the pump death that took three agents (09-20).
- Classification: isolation is **parity**; a resumable or auto-rotated fence
  is a **new decision** (parity doc `:731-733` says so).

### G6. The product cannot explain a lost agent

- `hagency-execution`, `hagency-runtime`, `hagency-platform` and `hagency-store`
  have no `tracing` dependency; all 63 log sites live in `bootstrap*`, and the
  only line per failure is one WARN with a JSON of fixed labels
  (`driver.rs:704-705`; `bootstrap.rs:415-466`). Guardian stderr is
  `/dev/null` by construction (`unix.rs:193-194`, admitted at `:534-535,
  603-605`), its exit status is never read (`:269,285,448,471`), and the
  `Stopped` frame carries no reason for a false `whole_tree_stopped` after
  `LeaderExited` (`macos.rs:159-163`). `Cleanup::Unknown`'s `io::ErrorKind` is
  dropped (`bootstrap.rs:603`). The store keeps one collapsed word in
  `runner_outputs` (`R/hagency-store/src/domain/owned_dispatch.rs:581-588`),
  no phase timestamps, no runtime exit status, no stderr tail.
- Retained: `terminal_reason` with exit identity and the last 500 chars of
  stderr (`T/router/src/runner.ts:484`), `runner_activity` phases with a 10 s
  heartbeat (`T/router/src/activity.ts:1-16`), guardian stderr piped to the
  parent (`runner-guardian.ts:84,142,151`).
- Explains why every one of the week's fixes was trial and error: the two
  remaining signatures have no recorded cause, and the only evidence ever
  obtained came from throwaway `eprintln!` builds.
- Classification: **parity** for the persisted evidence; a `tracing`
  dependency in the executing crates is a small new decision.

## The week's failures, mapped

| Seen live | Gap |
|---|---|
| Guardian refused any unplaceable newcomer; every agent on the host died once an hour (09-18/20, fixed by "follow TS" in ADR-029) | G2 design family (proof over the whole system) |
| Both agents `cleanup unknown` within 2 ms; zombie guardians exit 1; SIGTERM refused (09-19 ×3) | G2 + G6 |
| `cleanup_unknown` after a completed task at round 2; SIGTERM refused (09-22) | G2 + G6 |
| Connect timeouts end a worker for good; `Generation` cascade; fresh-fleet soaks (09-19) | G5 |
| Pump stopped by one host deny; three agents `outcome_unknown` (09-20) | G4 + G5 |
| Usage limit surfaced as `session_error: scope` and ended the agent (09-20) | G1 |
| `lost_authority` before Started after minutes of warm idle (09-18/20) | G3 |
| Command approval refused → session closed (09-19, patched to decline at the adapter) | G4 |
| `lost_authority` 44 s into a round with a parked command approval that never reached the owner (09-22) | G4 + G3 + G6 |
| Restart chain, recover-dispatch, Waiting, follow-ups (all proven this week) | not gaps: the operator resolution path works; it is invoked far too often |

## What closing them requires, and in which order

Order matters: without G6 every further fix is guesswork, and G1/G5 make each
remaining defect fleet-fatal. Each item names the decision the operator owns.

1. **Evidence first (G6).** Persist per-attempt phase events with timestamps,
   the full failure status (uncollapsed failure, stop cause/detail/report,
   cleanup error kind), `LostAuthority` provenance (site + store error), the
   guardian's exit status and refusal rows, and the runtime's exit status and
   stderr tail; give the executing crates `tracing`. Decision: none beyond
   approving the schema; this is what the retained product already records.
2. **Containment (G1 + G5).** A failed or unknown attempt ends the dispatch
   and quarantines the session; the agent's loop continues. Admission, pump
   and readiness stop being fleet-fatal. Decision: parity, per ADR-036's own
   deferral; readiness semantics are a small choice.
3. **Bounded, re-observable custody and a shutdown that completes (G2).** Bound
   the cleanup receipt, record it, fence the dispatch and the agent, and let
   the process exit. Decision: **reverse ADR-096's retained-owner rule** — the
   store already holds the quarantine, so the in-memory owner adds nothing
   durable. This is the one place the port must give up a rule of its own.
4. **Decouple authority from liveness (G3).** Lease renewal on a coarse clock
   through a path that cannot report `Busy` as revocation; no 100 ms
   qualification of an idle runtime; a late guardian reply is a retry, not a
   latch. Decision: parity.
5. **Approval leg (G4).** Refuse-as-decline with the reason recorded;
   `environmentId` scoped as the retained product does (prove first with one
   captured request); independent approval clock; per-request pump failure;
   wire the MCP approval leg or record it as owed; raise the wait default.
   Decision: parity plus one config default.
6. **Matrix fence (G5, second half).** Retry with backoff before fencing;
   whether a fence may be resumed or a generation rotated automatically is the
   operator's decision recorded in ADR-047/174.

Only after 1–5 does another 100-round soak measure anything; a soak on the
present design measures how often the host's own proofs fail, which is what it
has been measuring all week.

## Could not determine (merged from the four reviews)

- Which stop-phase sub-cause fired in the observed `cleanup_unknown` runs
  (census error, tracker gap, signal errno, lingering descendant, unreapable
  root, 3 s host timeout): the frame has no field for it.
- Which store refusal admitted today's command approval refusal; whether Codex
  0.154 sends a non-null `environmentId` on `item/commandExecution/requestApproval`
  (one captured request settles it).
- Whether a second SIGTERM is swallowed after the one-shot handler
  (`main.rs:285-296`).
- Whether whole-system census duration on the operator's hosts exceeds
  `response_ms` under load (needs a measurement the product does not take).
- The retained product's Matrix client HTTP bounds and any in-run lease
  re-check that could abort a healthy runner (none found).
