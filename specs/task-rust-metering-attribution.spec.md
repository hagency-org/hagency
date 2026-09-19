spec: task
name: "Native transcript attribution search"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, metering]
---

## Intent

Port transcript discovery and per-agent token attribution from
`lib/metering/attribute.js` without granting transcript text authority over
Agent attribution budgets or canonical task state.

## Constraints

### Must
- Preserve the workspace precedence chain and the per-framework search layout facts exactly as the retained JavaScript states them.
- Keep search descriptors as private untrusted layout hints with no filesystem access.
- Return an explicit reason for every unanswerable case and never a zero in place of an unknown figure.
- Keep the four token categories separate and cache reads excluded from ceiling arithmetic.
- Execute bounded synthetic vectors against the retained JavaScript attribution functions.
- Refuse rather than coerce sessions the native parser rejects.

### Must Not
- Do not read real transcripts credentials homeservers or model services in tests.
- Do not infer prices project identity quota availability or canonical task completion.
- Do not silently widen the workspace precedence chain or turn missing usage into measured zero.
- Do not sum agents sharing a workspace or invent a division between them.

## Boundaries

### Allowed Changes
- native/hagency-metering/**
- native/scripts/attribution-vectors.mjs
- native/fixtures/attribution.json
- knowledge/decisions/adr-118-native-metering-attribution.md
- specs/task-rust-metering-attribution.spec.md
- .github/workflows/rust.yml
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- Existing JavaScript behavior canonical stores runtime execution Matrix adapters another checkout and live data.

## Acceptance Criteria

Scenario: Attribution agrees with the retained JavaScript functions
  Test: native_metering_attribution_vectors
  Level: unit
  Test Double: synthetic agents workspaces and session texts executed by the real JavaScript attribution functions
  Given deterministic agents frameworks workspaces and injected sessions
  When native attribution answers the same inputs
  Then project directories search descriptors workspace precedence metered rows and fleet summaries match the recorded oracle
  And reasons and totals agree field for field

Scenario: Search layout facts stay pinned per framework
  Test: native_metering_attribution_search_layout
  Level: unit
  Test Double: synthetic framework and workspace identifiers
  Given claude codex unknown frameworks and relative workspaces
  When a search descriptor is requested
  Then claude narrows by project directory without recursion and codex recurses without narrowing
  And unknown or relative inputs return no search rather than a guessed one

Scenario: Unavailable attribution reports a reason and never a zero
  Test: native_metering_attribution_unavailable_never_zero
  Level: unit
  Test Double: synthetic unsupported frameworks missing workspaces foreign transcripts and bound-limited scans
  Given every case attribution cannot answer
  When a row is produced
  Then the exact JavaScript reason string is returned with no totals
  And shared fleets stay ambiguous and nothing attributable stays null

## Out of Scope

Session reading filesystem traversal age windows and file budgets remain in the
JavaScript reader until their native port is specified.
