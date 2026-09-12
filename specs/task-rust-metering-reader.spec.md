spec: task
name: "Native bounded transcript reader"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, metering]
---

## Intent

Port the bounded newest-first transcript reader and the cached fleet meter from
`lib/metering/reader.js` without granting file layout authority over Agent
attribution budgets or canonical task state, and without letting a bounded scan
masquerade as a complete measurement.

## Constraints

### Must
- Preserve the modification-time window file-count byte and traversal ceilings with the same defaults.
- Report every bound that bites with the retained JavaScript wording and keep understatement and unknown-ownership claims separate.
- Discover the nested Codex date tree recursively and keep the Claude project directory flat and non-recursive.
- Keep the newest-first order so a bound drops the oldest candidates rather than an arbitrary slice.
- Parse environment overrides with the JavaScript positive-integer rule from explicit strings never the process environment.
- Cache per fleet identity and home directory with an injected clock and stamp every value with its computed time.
- Keep directory names paths and mtimes private untrusted layout hints.

### Must Not
- Do not read real transcripts credentials homeservers or model services in tests.
- Do not absorb a truncation or a dropped candidate into a figure that looks complete.
- Do not turn an out-of-window non-narrowed candidate into this agent's understatement.
- Do not cache a fleet answer across a changed fleet identity.

## Boundaries

### Allowed Changes
- native/hagency-metering/src/reader.rs
- native/hagency-metering/src/lib.rs
- native/hagency-metering/tests/reader.rs
- native/scripts/reader-vectors.mjs
- native/fixtures/reader.json
- knowledge/decisions/adr-119-native-metering-reader.md
- specs/task-rust-metering-reader.spec.md
- .github/workflows/rust.yml
- docs/progress.md

### Forbidden
- Existing JavaScript behavior canonical stores runtime execution Matrix adapters another checkout and live data.

## Acceptance Criteria

Scenario: Reader agrees with the retained JavaScript on synthetic trees
  Test: native_metering_reader_vectors
  Level: unit
  Test Double: synthetic transcript trees rebuilt from a relative fixture description with chosen mtimes
  Given nested Codex and flat Claude layouts Unicode names oversize files and missing roots
  When native reads them with an injected clock
  Then files bounds reports and fleet rows match the recorded oracle
  And the cache stamps and caveats agree field for field

Scenario: Every bound that bites is named never silent
  Test: native_metering_reader_bounds_are_reported
  Level: unit
  Test Double: synthetic in-window out-of-window oversize and ceiling-exceeding trees
  Given a window drop a file ceiling a byte truncation and a traversal ceiling
  When the scan finishes
  Then the exact JavaScript wording names each bite and separates understatement from unknown ownership
  And a complete scan reports nothing at all

Scenario: Layout discovery matches each framework's facts
  Test: native_metering_reader_layout_discovery
  Level: unit
  Test Double: synthetic YYYY/MM/DD date trees and flat project directories
  Given Codex sessions under nested date directories and Claude transcripts beside a nested subdirectory
  When each search is walked
  Then the recursive walk finds every date tree file newest first and the flat walk never descends
  And no sibling workspace's file is opened on another's budget

Scenario: Limits and the fleet cache follow the JavaScript contract
  Test: native_metering_reader_cache
  Level: unit
  Test Double: synthetic fleet over a fresh home with an injected clock
  Given environment limit strings and a fleet metered across TTL force fleet-change and reset calls
  When limits parse and the cache answers
  Then positive integers win and anything else keeps the default and a hit keeps its original stamp while a changed fleet never reads the old answer

Scenario: Environment limit parsing keeps the JavaScript integer rule
  Test: native_metering_reader_limits_from_env
  Level: unit
  Test Double: explicit limit strings without touching the process environment
  Given positive integers zeros negatives fractional strings and garbage
  When ReaderLimits parses them
  Then a positive integer wins with parseInt prefix semantics and every other shape keeps its default

## Out of Scope

The HTTP endpoint usage projections quota enforcement and any filesystem change
remain outside the reader; the reader only reads.
