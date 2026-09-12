# Rust migration plan review

Reviewed the 575-line draft in `docs/design/hagency-rust-migration-plan.md`
against Hagency `e927e46` plus console cleanup `70312d1`. Draft SHA-256:
`61df2ca26f14cb5be5eb329c9d4bd1a284d2e34a3ae1ed09892eaf2a38dd3fe0`.

The architecture is a reasonable planning baseline. The separation of shared
business rules, OS adapters and external Agent dependencies is sound. The plan
correctly keeps outbound transport, owner approvals, private-room boundaries,
canonical task completion and honest recovery outcomes in scope. I found four
items to tighten before treating the phases as executable implementation tasks.
These are planning gaps, not reproduced defects in a Rust implementation.

## Findings

1. **Medium — Define request latency and work isolation before porting metering.**
   Plan lines 354–377 schedule metering and hardware measurements, but provide no
   requirement for bounded request latency, concurrent scans, queue capacity or
   cancellation of blocking work. Current `backend-v2.js:15509` awaits metering
   while handling `/api/usage`; `lib/metering/reader.js` performs synchronous
   directory/file reads despite its cache and scan bounds. Live console checks
   have already observed usage exceeding the proxy's 8-second timeout.

   A Rust translation could retain this request-path behavior. Add an M1
   concurrency design: background usage collection, timestamped cached reads,
   bounded blocking work and explicit busy/stale/unavailable results. Set workload
   profiles and measurable latency/memory limits in M0/M1, then test an approval
   and heartbeat while a large history scan runs. M8 should confirm those budgets.
   Simply wrapping everything in `spawn_blocking` is insufficient: Tokio documents
   that started blocking jobs cannot be aborted and recommends limiting CPU work.
   [Tokio documentation](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

2. **Medium — Specify the commit boundary across core, task and transport state.**
   Plan lines 254–258 introduce several repositories and transactional updates;
   lines 280–293 require exactly scoped dispatch and recovery. The document does
   not yet identify which records commit together and which cross an inbox/outbox
   boundary. Exclusive ownership does not by itself make separate commits atomic.

   This matters for concrete existing behavior: `lib/engagement-store.js:364`
   persists resource reservations and fulfillment phases, while
   `lib/fleet-outbound-client.js:35` persists an incoming delivery before its
   transport ACK and tracks later processing separately. Specify the owner,
   transaction records, replay key and recovery action for request admission,
   approval/reservation, provisioning, dispatch settlement and result delivery.
   Add crash tests between those commits, including a committed reservation with
   no recorded next action. Resolve this as an M1 exit artifact before M2/M3 use
   the storage interfaces. The plan need not force all stores into one database.

3. **Medium — Bring Windows runner and encrypted Matrix feasibility proofs forward.**
   The plan acknowledges these risks, but the full proofs sit in M4/M5
   (lines 295–329), after substantial storage/domain work can proceed. The current
   guardian explicitly rejects Windows (`router/src/runner-guardian.ts:7`), and
   Matrix identity/crypto continuity is intentionally unproven. These are major
   inputs to the proposed release and schedule.

   Add small M0/M1 proofs using one selected runner on native Windows and one
   encrypted Matrix device with the intended Rust SDK/storage features. Verify
   child ownership, cancellation and sandbox capability; verify restart and
   decryption with the chosen fresh-device or import strategy. Defer the full
   adapters to M4/M5. Job Objects have breakaway and child-creation exceptions,
   so a successful spawn alone cannot establish ownership of all work.
   [Microsoft documentation](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).

4. **Low — Separate phase start dependencies from phase completion dependencies.**
   M7's exit gate at lines 360–363 requires the existing product workflows to pass
   against Rust, but the graph at lines 393–410 gives M7 only M2/M3 prerequisites.
   Owner approval, attachments and Matrix behavior are completed in M5/M6. This
   leaves M7's completion ambiguous even though the final M8 gate depends on both
   branches.

   Show M5/M6 dependencies for M7 integration acceptance, or split M7 into an
   independently testable UI/API-shell milestone and a later integrated workflow
   milestone. Parallel UI development can still begin after M2/M3.

## What the plan already gets right

- Native Windows is a real target with native tests; WSL/tmux are not substitutes.
- Runtime Node removal is distinguished from browser JavaScript, CI tools and
  separately installed Agent executables.
- The plan explicitly catches static-export routing for new Agent IDs; it does
  not assume serving the existing export solves that behavior.
- Matrix crypto formats are not assumed compatible, dual writers are forbidden,
  and rollback after external effects requires reconciliation.
- All 86 relative links resolve. Ten phase ranges sum to 43–73 engineer-weeks.
  Those estimates remain provisional; arithmetic validation is not schedule proof.

No migration implementation or runtime test was performed. The draft itself was
not edited. Recommended next work is a bounded M0 task with the two early proofs,
the state-ownership table and measurable performance acceptance criteria.
