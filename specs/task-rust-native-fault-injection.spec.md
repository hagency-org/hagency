spec: task
name: "Fault injection proves custody outcomes are never silent successes"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, fault-injection, custody, store, outbound]
---

## Intent

Bind the M8 item-4 half the audit named: storage and network fault
injection — disk-full on the store, partial upload and server rejection on
the outbound adapter — with the custody outcome **named in the store's own
words** for each (`unknown`, `rejected`, or a clean engine failure), never a
silent success. The outcomes are decided by the records that own them:
ADR-037 (outbound custody — started claims become `unknown`; a lost or
rejected response retains it), ADR-078 (file-upload custody —
`mark_upload_uncertain` retains the original fence) and ADR-083
(upload-response custody — partial body, failed EOF never acquires
authority). This spec adds no decision, it binds the injections that prove
those records' outcomes.

## Constraints

### Must
- Inject disk-full on the store portably: SQLite's `max_page_count` pragma (or an equivalent engine-level limit) — never a real tmpfs or filesystem fill; the limit is set, the write is attempted, the failure is observed, and the limit is restored.
- Assert the disk-full outcome as the store yields it today: the write returns `Error::Sqlite` carrying the engine's database-full/disk message, never swallowed, no partial row survives, the store remains readable and writable within the limit afterwards — never a silent success and never a torn write.
- Drive the real outbound adapter against the shared fake peer for a partial upload: the fake peer closes after a partial body or a failed EOF, so the outcome cannot be proven delivered.
- Assert the partial-upload outcome: the adapter observes the interruption and issues `Unknown` (via `mark_upload_uncertain`'s production caller), the store records the publication state **`unknown`** with the original fence retained, and no runtime gains transport authority from it.
- Drive the real outbound adapter against the shared fake peer for a server rejection: the fake peer answers 4xx/5xx with a definitive cause.
- Assert the rejection outcome: the adapter issues `PublicationResponse::Rejected` carrying the server's cause, the store records the publication state **`rejected`**, and nothing is re-sent under the same id — never retried silently, never reported delivered.

### Must Not
- Do not use real filesystem fills, tmpfs mounts or disk partitioning — the injection is engine-level and portable.
- Do not weaken or rename any existing custody selector; the existing custody scenarios keep their words.
- Do not inject faults into live services, real homeservers or the production host.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/tests/ (the disk-full injection)
- native/hagency-store/src/repository.rs (one documented unconditional test seam: `open_with_page_limit(path, max_pages)`)
- native/hagency-store/src/domain.rs (the same seam, exposing it on the domain constructor)
- native/hagency-palpo/src/adapter.rs (the outbound adapter issuing `Command::Publication` with `Rejected`/`Unknown`)
- native/hagency-palpo/tests/
- native/hagency-matrix/src/outgoing.rs, native/hagency-matrix/src/sdk/outgoing.rs (the file-publication adapter)
- native/hagency-matrix/src/upload/operation.rs (the upload path's `mark_upload_uncertain` production caller)
- native/hagency-matrix/tests/
- native/hagency/tests/
- native/scripts/
- specs/task-rust-native-fault-injection.spec.md
- docs/progress.md

### Forbidden
- Live services, live homeservers, credentials, deployed state.
- native/hagency-store/src/** except the one licensed seam above (the store's behavior is otherwise asserted, not changed — this includes not introducing a named capacity error); native/hagency-matrix/src/** except the two adapter/upload paths licensed above; native/hagency-palpo/src/** except `adapter.rs`.

## Acceptance Criteria

Scenario: The store fails closed on an injected disk-full write
  Test: native_store_fails_closed_on_disk_full
  Level: integration
  Test Double: SQLite max_page_count set to the current page count, the write attempted, the limit restored
  Given a live store at its current size with the disk-full limit injected
  When a write is attempted past the limit
  Then the write returns Error::Sqlite carrying the engine's database-full/disk message and no partial row survives
  And the store remains readable and writable within the limit afterwards
  And the limit is restored afterwards — never a real filesystem fill

Scenario: A partial upload on the outbound adapter is recorded unknown
  Test: native_outbound_partial_upload_is_recorded_unknown
  Level: integration
  Test Double: the real outbound adapter driven against the shared fake peer, which closes after a partial body or a failed EOF
  Given the real adapter with an upload the fake peer interrupts mid-body or ends with a failed EOF
  When the adapter observes the interruption
  Then the adapter issues Unknown — mark_upload_uncertain's production caller — and the publication state reads unknown with the original fence retained
  And no runtime gains transport authority from the unknown outcome

Scenario: A server rejection on the outbound adapter is recorded rejected
  Test: native_outbound_server_rejection_is_recorded_rejected
  Level: integration
  Test Double: the real outbound adapter driven against the shared fake peer, which answers 4xx/5xx with a definitive cause
  Given the real adapter with a send the fake peer rejects with a 4xx/5xx cause
  When the adapter observes the rejection
  Then the adapter issues PublicationResponse::Rejected carrying the server's cause and the publication state reads rejected
  And nothing is re-sent under the same id — never retried silently and never reported delivered
  And a 409 whose code is sequence_conflict or stale_lease stays retryable and does not finalize to rejected — every other 4xx/5xx finalizes to rejected

## Decisions

**The disk-full word is the engine's, and a named capacity error is a
follow-on, not this slice.** `max_page_count` exhaustion surfaces from
rusqlite as `Error::Sqlite` with the engine's message — and a named
capacity word would need a store change this slice does not license. If a
dedicated `Capacity` error word is the right product surface for an honest
disk-full refusal, that is a separate store slice with its own review;
this spec asserts the failure as it exists today.

**The one licensed store seam, documented and unconditional.** The
integration test cannot reach the writer connection otherwise
(`Repository.db` and `DomainRepository.db` are `pub(crate)` with no public
pragma surface, and a `#[cfg(test)]` window is invisible to the crate's
integration tests) — so exactly one seam is licensed: an unconditional
constructor variant `open_with_page_limit(path, max_pages)` that applies
`PRAGMA max_page_count` on the writer connection, default path unchanged,
no production caller, the same posture ADR-053's bounded-spawn amendment
records for its stall double. Everything else under `src/**` stays
forbidden.

**Injection is engine-level, always.** Disk-full is `max_page_count`, not a
filesystem fill: the fault must be portable to every CI lane and must not
depend on the runner's disk layout.

**The 409 retryable exception, preserved.** The existing custody test
`native_outbound_http_publication_frozen_restart_and_rotation` pins that a
409 carrying `code: sequence_conflict` (and the `stale_lease` shape already
distinguished in `adapter.rs:147`) is a **retryable** custody conflict, not
a finalized rejection — the frozen publication is re-sent and then succeeds.
So "every other 4xx/5xx finalizes to `rejected`" is the rule, and those two
codes are the named exception that stays retryable and never writes
`rejected`.

## Out of Scope

Network outage/reconnect drills (M8's dedicated lane), process-death and
lease-expiry scenarios (already bound by the dispatch specs), any change to
the store or adapter code, and a named capacity error (the follow-on above).
