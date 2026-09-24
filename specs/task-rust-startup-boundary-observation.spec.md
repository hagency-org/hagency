spec: task
name: "Observe original native startup and media creation boundaries"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, startup, observation]
---

## Intent

Narrow the original c677ce0 Linux pre-ready startup gap and independent Windows
FileService startup refusal without changing either original result or policy.

## Decisions

Retain accepted [ADR-101](../knowledge/decisions/adr-101-native-file-service-integration.md)
and its original-child observation boundaries. The original four Linux children
are running with empty stderr, no first HTTP request and no ready log. The three
Windows workflow children are ready after29requests but FileService startup is
unavailable. Neither original explains its underlying operation or backend cause.

## Constraints

### Must
- Add only fixed TRACE messages at original runtime, configuration hash, store, worker and server boundaries.
- Keep media creation and storage refusal phases separate from bootstrap progress.
- Project fixed phase labels from the original child's existing retained stderr handle within its existing8193byte read limit.
- Preserve original errors, ownership, private validation, atomic freshness and all current deadlines.
- Enable only the new trace target in actual disposable fixture child commands.
- Validate actual child progress, original refusal and retained-handle association without live external services.

### Must Not
- Do not log paths, credentials, identifiers, backend text, SQL or file contents.
- Do not add a worker, timer, retry, timeout extension, phase authority or a successful fallback.
- Do not claim historical causes from new diagnostic success or unobserved phases.
- Do not alter crypto, native filesystem policy or FileService runtime authority.

## Boundaries

### Allowed Changes
- native/hagency/src/main.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/src/file_service/recovery.rs
- native/hagency/tests/file_service/fixture.rs
- specs/task-rust-startup-boundary-observation.spec.md
- knowledge/context/native-startup-boundary-observation.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Actual startup phases belong to the original retained process output
  Test: native_file_service_original_observation
  Level: integration
  Test Double: actual disposable service child and scripted local TLS peer with original stderr handle
  Given a real service child reaches its original initial authenticated request
  When its existing original observation is read
  Then fixed startup phases show the actual serving boundary
  And a separately refused child retains its own earlier configuration phase after its stderr path is replaced
  And original child errors and panic cleanup remain unchanged

Scenario: Actual media refusal is separate from successful server startup
  Test: native_file_service_media_startup_observation
  Level: integration
  Test Double: actual disposable service and local TLS peer with an existing empty private media directory
  Given an existing private media directory has no original journal
  When the actual service performs original Matrix setup and FileService initialization
  Then the original outcome remains unknown with no journal repair or task attempt
  And its original stderr reports serving separately from the actual store refusal

Scenario: Media creation preserves original atomic freshness and custody
  Test: native_file_service_shutdown_media_creation_requires_atomic_fresh_directory
  Given an existing directory and a separately fresh private media directory
  When original recovery open and retained-lock checks run
  Then existing missing journals remain refused and original successful creation remains exclusive
  And no trace changes the original result or journal creation policy

Scenario: Original executable file workflow remains independently required
  Test: native_file_service_executable
  Level: integration
  Test Double: actual native service and model protocol child with local Matrix peer
  Given the original group and direct native file tasks
  When the existing executable workflow runs with fixed trace enabled
  Then its original file delivery assertions and current authority remain unchanged

## Out of Scope

Production deadline or filesystem fixes, inferred historical CPU/IO/SQLite causes,
Windows directory-sync policy, service deployment, hosted reruns and migration cutover.
