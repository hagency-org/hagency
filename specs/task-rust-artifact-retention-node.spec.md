spec: task
name: "Rotate retained recovery logs and prune the retained media cache"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, retention, maintenance, node]
---

## Intent

Bind the retained half of ADR129. The retained product appends four jsonl files
with no rotation and leaves `MEDIA_FETCH_CACHE_DIR` unbounded
(`backend-v2.js:2781-2784`; `lib/mcp-server-core.js:145-146`, `:568`). The native half of the same ADR
is bound by the rust-tagged contract
`specs/task-rust-artifact-retention.spec.md`; the spec-binding checkers select a
runtime per contract, so the retained Vitest scenarios need their own Node
contract — a retained selector can never appear in the Cargo inventory, and a
rust-tagged file never reaches the Vitest inventory.

The scenarios below are the negative cases the design requires
(`context-artifact-retention-design-v2.md` §11c) for the retained objects. They
are the only retained scenarios this contract binds; the remaining §11a names in
that design have no binding yet and are recorded as an open gap in the companion
report, not silently bound here.

## Constraints

### Must
- Rotate `messages-archive.jsonl`, `system-info.jsonl`, `audit.jsonl` and
  `message-delivery-events.jsonl` by size in `bin/hagency-maintain`, whole rows
  only, keeping the newest N rotations.
- Keep every rotation readable and included in the readers' search set, because
  two of the four files are read back.
- Keep a pending or unknown-outcome delivery's media entry across a cache sweep.
- Log and retry a failed delete; never refuse a fetch because retention failed.
- Use `path.join`/`path.parse` for every sweep path, never manual separators.

### Must Not
- Do not truncate a read-back jsonl file, and do not drop a partial row.
- Do not rotate on the request path or refuse work when a rotation fails.
- Do not change the retained media response, the delivery-event wire or any store
  schema.
- Do not gate a whole scenario by OS; express a platform difference as a named
  refusal assertion on the other leg.

## Boundaries

### Allowed Changes
- bin/hagency-maintain
- lib/mcp-server-core.js
- backend-v2.js
- install/install-macos.sh
- tests/artifact-retention.test.js
- specs/task-rust-artifact-retention-node.spec.md
- docs/progress.md

### Forbidden
- Live services, live models, deployed state and Matrix server changes.

## Acceptance Criteria

Scenario: A rotated message archive keeps whole rows and its last complete line
  Test: a rotated message archive keeps its last complete line and never splits a JSON row
  Given a message archive whose final write is a torn partial row
  When it rotates
  Then every line in the retained rotation parses as JSON and no partial row was carried across the boundary

Scenario: A rotated-away message is still found by the membership read
  Test: archivedMessageExists still finds a message that lives only in a rotation
  Given an archived message that exists only inside a rotation, with the live file no longer carrying it
  When the membership read answers for that message id
  Then it answers true, so a committed message is not re-persisted through the dispatch gate

Scenario: A cache entry referenced by a pending delivery survives the sweep
  Test: a media cache entry referenced by a pending delivery survives the sweep
  Given a cache entry whose source is named by a delivery that has not reached a terminal receipt
  When the sweep runs with that source protected
  Then the entry is still present and the pending delivery can still read it

Scenario: A cache entry referenced by an unknown-outcome delivery survives the sweep
  Test: a media cache entry referenced by an unknown-outcome delivery survives the sweep
  Given a cache entry whose source is named by a delivery whose outcome is unknown
  When the sweep runs
  Then the entry is retained, because an unknown outcome is evidence and never a prune candidate

Scenario: A failed delete is logged and never refuses a fetch
  Test: a failed media cache delete is logged and does not refuse a subsequent fetch
  Given a cache entry the sweep cannot delete
  When the delete fails and a fetch for that source then arrives
  Then the failure is logged, no fetch is refused because of it, and the entry is retried on the next sweep

## Out of Scope

The native half of ADR129 (bound by `specs/task-rust-artifact-retention.spec.md`);
the store-resident retention slices and the retention tick; the Windows service
wrapper and the macOS `newsyslog` entry, both deferred by ADR129; and every
retained scenario in the design's §11a that this contract does not bind.
