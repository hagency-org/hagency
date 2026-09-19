spec: task
name: "Preserve the original owned runner failure and bounded file helper observations"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, diagnostics, custody]
---

## Intent

Disambiguate the remaining original Windows FileService failure using the same
owned runtime report and original offline helper receipts before selecting a fix.
This bounded observation amendment follows ADR-053 "Bind one owned native session
to exact durable dispatch authority" and ADR-101 "Connect one development
FileService to the original Started workspace and Matrix owner".

## Constraints

### Must
- Snapshot the original runner phase typed session failure and first transport cause before stop or reconciliation can discard that owner.
- Retain only closed enums optional original pending counts and original accepted or total write bytes.
- Keep existing protocol cleanup settlement failure verdict and operator status semantics unchanged.
- Project diagnostics only through the existing authenticated operator status and original fixture failure snapshot.
- Observe helper receipt presence and whitelisted status through bounded reads of the original isolated fixture workspace.
- Preserve absent unsupported malformed and read-failed helper observations explicitly.
- Keep the original hosted failure distinct from later diagnostics and local passing fixtures.

### Must Not
- Do not change deadlines retries ordering process custody permissions or SQLite policy.
- Do not emit protocol events keepalive activity raw stderr request IDs payloads credentials paths SQL or private metadata.
- Do not add browser product flow fields a global trace or a replacement runtime helper or attempt.

## Boundaries

### Allowed Changes
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/lib.rs
- native/hagency-execution/tests/owned.rs
- native/hagency/src/bootstrap.rs
- native/hagency/tests/file_service/fixture.rs
- knowledge/decisions/adr-053-native-owned-dispatch.md
- knowledge/decisions/adr-101-native-file-service-integration.md
- knowledge/context/native-file-owned-observation.md
- specs/task-rust-file-owned-observation.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Actual owned failure survives original owner removal
  Test: native_owned_runtime_failure_observation
  Given an actual original offline owned child that stays silent or closes stdout
  When its unchanged runtime operation fails and cleanup stops or retains the owner
  Then its exact original phase session failure and transport cause remain in the report including after proven whole-tree stop removes the owner

Scenario: Operator projection remains bounded and private
  Test: native_bootstrap_runtime_observation_projection
  Given maximum-width diagnostic counts and typed runtime failure variants
  When the existing operator status projects the original report
  Then it exposes only closed labels and optional counts without payloads or authority changes

Scenario: Original helper receipt states are explicit and bounded
  Test: native_file_service_helper_observation_bounds
  Given absent malformed oversized unsupported and known helper receipts
  When the original fixture snapshot reads their bounded projections
  Then each state is distinct and private content never enters the diagnostic output

Scenario: Original executable observation keeps its own failure and custody
  Test: native_file_service_original_observation
  Given an actual service child with its original fixture observation
  When a refusal or missing HTTP observation is inspected
  Then the same process and helper absence remain separate from its original verdict

## Out of Scope

Changing the native execution timing policy fixing the suspected silent helper
interval replaying original hosted jobs live providers and production cutover.
