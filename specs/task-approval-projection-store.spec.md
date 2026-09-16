spec: task
name: "Durable canonical approval projection store"
status: accepted
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
---


## Intent

Project every canonical approval state durably and idempotently to Matrix while preserving HAFleet's native authorization, A3 trusted origin, private/public separation, and one-time verdict semantics.

## Decisions

- HAFleet `fc0906c3bd54275b09fb17f4abbe850b1743134e` plus the coordinator's eventual published Phase A runner/loopback/dashboard integration.
- Do not copy Phase A commits into this task.
- Legacy load performs one disclosed O(N) JSON parse and constructs persisted migration/expiry indexes. Each migration tick examines at most its configured candidate count, and expiry selection pops at most its configured number of heap heads with O(log N) heap work per pop. Full-state transaction snapshot cloning/JSON serialization, historical outbox duplicate/transition scans, and due-row filter/sort still scale with stored history; B1.1 does not claim the whole tick is O(limit log N). Enqueued and returned projection counts remain bounded by their explicit batch/limit.

## Boundaries

### Allowed Changes
- lib/approval-store.js
- backend-v2.js
- tests/api-approval-projections.test.js
- tests/approval-store-projection.test.js
- tests/approval-store.test.js
- tests/approval-thread-notice.test.js
- specs/task-approval-projection-store.spec.md
- specs/task-approval-projection-api.spec.md
- specs/task-approval-binding-marker-store.spec.md
- tests/approval-binding-marker.test.js
- tests/api-approval-binding-markers.test.js
- knowledge/requirements/req-approval-canonical-projection.md

### Forbidden

- Runtime/provider approval adapters, global authentication, agent tokens, Matrix verdict authorization, A3 `router_approval_id` provenance, public notice private fields, global hooks, service configuration, dependency manifests, Robrix source, live services, Matrix rooms, or provider calls.

## Acceptance Criteria

Scenario: Persistence failure respects the atomic rename commit point
  Test: pre-rename rolls back and post-rename degradation blocks later writes until reload
Given a request and outbox at revision N
When create, verdict, expiry, denial, consumption, plan, receipt, retry, or migration persistence fails before atomic rename
Then disk and every affected in-memory record, index, row, counter, receipt, and cursor remain at revision N, no SSE is emitted, and a retry from revision N is allowed
When rename succeeds but directory fsync, defensive chmod, or a later health step fails
Then disk and memory both retain revision N+1, the result is explicitly committed with degraded durability health, and no caller retries or duplicates the transition.

Scenario: Every accepted canonical transition has one projection revision
  Test: creation and transitions enqueue increasing canonical revisions privately
Given a native request or later accepted state transition
When its single store transaction commits
Then the canonical record and required outbox rows are durable together with one increasing revision
And rejected/replayed transitions add no revision or row.

Scenario: Send plan preparation and begin-send are durable CAS operations
  Test: full prepared-plan identity is required and immutable
Given an unprepared canonical projection row and two concurrent bridge preparations
When both propose publisher context and encrypted bytes
Then target room and channel come from the row, the backend bounds payload bytes, derives the stable safe transaction ID, and atomically stores one immutable winning plan
And both callers receive that same plan while the losing ciphertext is never sent
When exact `begin-send` succeeds or its response is lost
Then durable state is attempted/uncertain before Matrix I/O and retry is allowed only with the identical publisher, credential generation, event type, payload, and transaction ID.

Scenario: Receipt is exact compare-and-set bookkeeping
  Test: uncertain retry observes deadline and becomes receiptable with the same plan
Given attempted work pinned to a request, revision, channel, room, publisher, credential generation, prepared payload, and transaction ID
When a stale or mismatched receipt arrives
Then it is rejected without dropping or redirecting work
And an exact receipt records only its Matrix event ID without changing approval state.

Scenario: v1 migration emits zero actionable reposts
  Test: legacy migration is bounded, resumable, and emits no actionable or null-target work
Given pending, terminal, and consumed v1 records under positive, absent, undecryptable, or multiple history evidence
When migration runs twice and after restart
Then it preserves authorization/digest/decision fields and emits zero new actionable request or public notice events
And only bounded, resumable, read-only current-state work is eligible.

Scenario: Startup work is bounded
  Test: migration offset rolls back on pre-rename failure and resumes after reload
Given more legacy records than one migration/drain batch
When the process starts
Then it does not scan, enqueue, or publish the entire history synchronously
And a persisted cursor plus bounded timer batches eventually resume without duplicates.

## Out of Scope

Run the four exact selectors and combined command in `phase-b1-hafleet-implementation-plan.md`. Run repository contract parse/lint/boundary checks, artifact freshness, advisory baseline, and one final `verify:ci` on the coordinator's combined Phase A base. Treat Node lifecycle skips as skips and report them honestly.
