spec: task
name: "Fault injection proves custody outcomes are never silent successes"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, fault-injection, custody, store, outbound]
---

## Intent

Bind the M8 item-4 half the audit named: storage and network fault
injection — disk-full on the store, partial upload and server rejection on
the outbound adapter — with the custody outcome **named for each**: refused,
fenced or uncertain, never a silent success. ADR-045 (notice custody) and
ADR-091 (negative-observation custody) already decide those outcomes; this
spec adds no decision, it binds the injections that prove them.

## Constraints

### Must
- Inject disk-full on the store portably: SQLite's `max_page_count` pragma (or an equivalent engine-level limit) — never a real tmpfs or filesystem fill; the limit is set, the write is attempted, the refusal is observed, and the limit is restored.
- Assert the disk-full outcome: the store **refuses** the write with its named capacity/refusal word and no partial row survives — never a silent success and never a torn write.
- Inject a partial upload on the outbound adapter against the shared fake peer: a transfer interrupted mid-body, so the outcome cannot be proven delivered.
- Assert the partial-upload outcome: the send is **uncertain** (ADR-045's word — unstarted work may requeue, a possible send stays uncertain), and no runtime gains transport authority from it.
- Inject a server rejection on the outbound adapter: the fake peer serves a definitive rejection response.
- Assert the rejection outcome: the send is **refused** with the rejection's own named cause, never retried silently, never reported delivered.

### Must Not
- Do not use real filesystem fills, tmpfs mounts or disk partitioning — the injection is engine-level and portable.
- Do not weaken or rename any existing custody selector; the existing negative-custody scenarios keep their words.
- Do not inject faults into live services, real homeservers or the production host.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/tests/ (the disk-full injection)
- native/hagency-matrix/tests/ (the outbound injections against the fake peer)
- native/hagency/tests/
- native/scripts/
- specs/task-rust-native-fault-injection.spec.md
- docs/progress.md

### Forbidden
- Live services, live homeservers, credentials, deployed state.
- native/hagency-store/src/** (the store's refusal behavior is asserted, not changed); native/hagency-matrix/src/**.

## Acceptance Criteria

Scenario: The store refuses a write under injected disk-full
  Owed Selector: native_store_refuses_on_disk_full (parked — the name is owed by the implementing slice and binds only when it lands; no Test: line here yet)
  Level: integration
  Test Double: SQLite max_page_count set to the current page count, the write attempted, the limit restored
  Given a live store at its current size with the disk-full limit injected
  When a write is attempted past the limit
  Then the store refuses with its named capacity word and no partial row survives
  And the limit is restored afterwards — never a real filesystem fill

Scenario: A partial upload on the outbound adapter stays uncertain
  Owed Selector: native_outbound_partial_upload_is_uncertain (parked — binds with this slice; no Test: line here yet)
  Level: integration
  Test Double: the shared fake peer with a transfer interrupted mid-body
  Given an outbound upload interrupted after bytes have crossed
  When the adapter observes the interruption
  Then the send is uncertain — unstarted work may requeue and a possible send stays uncertain
  And no runtime gains transport authority from the uncertain outcome

Scenario: A server rejection on the outbound adapter is refused by name
  Owed Selector: native_outbound_server_rejection_is_refused (parked — binds with this slice; no Test: line here yet)
  Level: integration
  Test Double: the shared fake peer serving a definitive rejection response
  Given an outbound send the peer definitively rejects
  When the rejection is observed
  Then the send is refused with the rejection's own named cause
  And it is never retried silently and never reported delivered

## Decisions

**Injection is engine-level, always.** Disk-full is `max_page_count`, not a
filesystem fill: the fault must be portable to every CI lane and must not
depend on the runner's disk layout. The custody outcomes themselves are
ADR-045/091's and are unchanged — this suite proves them under injection.

## Out of Scope

Network outage/reconnect drills (M8's dedicated lane), process-death and
lease-expiry scenarios (already bound by the dispatch specs), and any
change to the store or adapter code.
