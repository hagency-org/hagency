spec: task
name: "Native bounded transcript token normalization"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, metering]
---

## Intent

Port Claude and Codex transcript token normalization without granting transcript
text authority over Agent attribution budgets or canonical task state.

## Constraints

### Must
- Preserve four separate token categories and exclude cache reads from ceiling arithmetic.
- Deduplicate Claude message identities and use Codex cumulative totals without adding reasoning twice.
- Execute bounded synthetic vectors against the retained JavaScript parser.
- Keep absent usage unknown and expose malformed incomplete ambiguous and nonmonotonic observations.
- Reject unsafe counters conflicting duplicate identities and capacity exhaustion explicitly.
- Keep parser metadata private untrusted hints with no filesystem access or current task authority.

### Must Not
- Do not read real transcripts credentials homeservers or model services in tests.
- Do not infer prices project attribution physical path identity quota availability or canonical task completion.
- Do not silently truncate input or turn missing usage into measured zero.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-metering/**
- native/scripts/metering-vectors.mjs
- native/fixtures/metering.json
- native/README.md
- .github/workflows/rust.yml
- knowledge/decisions/adr-055-native-metering-parsers.md
- specs/task-rust-metering-parsers.spec.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- Existing JavaScript behavior canonical stores runtime execution Matrix adapters another checkout and live data.

## Acceptance Criteria

Scenario: Normalized usage agrees with the retained JavaScript parser
  Test: native_metering_legacy_vectors
  Level: unit
  Test Double: synthetic JSONL inputs executed by the real JavaScript parser
  Given valid bounded Claude and Codex transcripts
  When native normalization reads their records
  Then categories cumulative totals turns and deduplication match the recorded oracle
  And reasoning and cache reads are not added twice

Scenario: Missing and damaged usage remains explicitly incomplete
  Test: native_metering_incomplete_observations
  Level: unit
  Test Double: synthetic malformed absent and ambiguous transcript records
  Given absent fields malformed lines missing message identities or changing workspaces
  When native normalization reports observations
  Then unknown values remain absent and incomplete evidence is visible
  And no transcript metadata becomes authenticated Agent or project attribution

Scenario: Duplicate and contradictory records cannot invent valid accounting
  Test: native_metering_conflicting_observations
  Level: unit
  Test Double: synthetic duplicate keys message identities and contradictory cumulative totals
  Given conflicting duplicates decreasing totals or impossible cache and reasoning breakdowns
  When native normalization evaluates the transcript
  Then unsafe duplicate observations fail or contradictory arithmetic is explicitly reported
  And a decrease is never converted into a fresh delta or trusted whole session total

Scenario: Parsing memory and arithmetic have explicit limits
  Test: native_metering_bounds
  Level: unit
  Test Double: generated bounded transcript and numeric edge cases
  Given excessive input lines nested JSON metadata identities or counters
  When the parser reaches a declared bound
  Then it returns an explicit error without a silently truncated measurement

Scenario: Reported volume differs from fresh token ceiling arithmetic
  Test: native_metering_ceiling_categories
  Level: unit
  Test Double: exact synthetic token category values and missing measurements
  Given independently observed input output cache writes and cache reads
  When display and ceiling arithmetic are requested
  Then display includes cache reads while ceiling arithmetic excludes them
  And an absent required category remains unknown rather than zero

## Out of Scope

Filesystem transcript discovery identity provenance session ledger persistent
deduplication engagement attribution quota enforcement runner live usage events
and console wiring require separate integrated contracts. This library does not
complete M7 or establish that recorded provider figures are authenticated.
