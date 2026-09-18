spec: task
name: "Prepare a distinct native fault attempt without changing consumed evidence"
inherits: project
satisfies: [REQ-INNER-LOOP-MONITOR]
tags: [herdr, recovery, evidence]
---

## Intent

Prepare a new explicit recovery namespace for the remaining03/04 E2E gates,
retaining the original goal and failed Q3 history. Do not restart a consumed
observer or mutate any live goal, release marker or business artifact.

## Constraints

### Must
- Require a readonly SHA256-pinned plan, recovery contract and all predecessor evidence.
- Validate canonical disjoint project/control paths and an exact new UUIDv4 attempt directory.
- Verify predecessor audit exit3 and ineligible status, matching manifest/exit pins and old claimed observer identity.
- Preserve all old consumed evidence bytes and publish new metadata exclusively as readonly JSON.
- Hard-link the actual append-only trace inode into the new attempt with no replacement, symlinks or copy fallback.
- Verify source/link inode and complete-prefix identity before and after linking.
- Retain a claimed attempt after any subsequent failure and refuse its reuse.
- Keep public output bounded and free of raw predecessor payloads or goal objectives.

### Must Not
- Start observers or native workers, issue commands, signals or approval decisions, or query managed native handles.
- Change frozen Python validators, fault thresholds, old manifests, task state, deadline or lower implementation.
- Treat prepared files or historical reaping evidence as current live readiness or full acceptance.

## Decisions

- Reuse Node22 builtin modules and existing native-control-evidence primitives.
- CLI uses --plan FILE --sha256 HEX; library exports prepareFaultAttempt(plan).
- The plan supplies version1, attempt_id, base_control, project, trace, recovery_contract pin and predecessor pins named audit, safe_state, observer_binding, observer_claim, observer_ready, watch_manifest and watch_exit.
- The readonly recovery contract binds the same attempt_id/base_control/project, original goal ID/creation/objective digest, exact predecessor pins and stages interrupt03/restart04; it explicitly preserves predecessor_full_acceptance=false.
- New path is base_control/recovery-attempts/UUID. attempt.json contains lineage and trace fence; a separate prepared.json confirms successful same-inode linking. Failure after claiming never removes or repurposes this directory.
- No live03 binding is created by this preparer. The coordinator must still establish current goal/turn/process/protected facts and observer readiness.04 data cannot be fabricated before03 and blocked/timeout acceptance exist.

## Boundaries

### Allowed Changes
- skills/hagency-inner-loop/scripts/prepare-native-fault-attempt.mjs
- skills/hagency-inner-loop/scripts/native-control.mjs
- tests/native-fault-attempt.test.js
- skills/hagency-inner-loop/SKILL.md
- bin/hagency-sync-skills
- tests/skill-sync.test.js
- specs/task-native-fault-attempt.spec.md
- knowledge/decisions/adr-035-distinct-fault-attempt.md
- docs/superpowers/plans/2026-09-13-native-fault-attempt.md

### Forbidden
- router/**
- backend-v2.js
- bridge-matrix.js
- Live runtime state, business projects, frozen Python and consumed attempt evidence

## Acceptance Criteria

Scenario: Prepare a distinct same-inode trace namespace
  Test: prepares one readonly fault attempt with the original trace inode
  Given valid pinned failed-predecessor evidence and a new explicit recovery contract
  When the preparer claims the new attempt
  Then new readonly metadata and a same-inode trace link are created while old bytes remain unchanged

Scenario: A claimed attempt cannot be reused
  Test: refuses a reused or colliding fault attempt without changing prior evidence
  Given an existing attempt or conflicting trace target
  When preparation is invoked again
  Then it fails without overwriting or deleting previous evidence

Scenario: Reject invalid lineage before creating an attempt
  Test: rejects missing mutable changed or inconsistent predecessor pins before claiming
  Given absent writable changed successful or mismatched predecessor records
  When the recovery plan is checked
  Then no attempt is created

Scenario: Reject unsafe trace paths and link failures
  Test: refuses symlinks partial records cross-device links and changed trace prefixes
  Given unsafe path components a partial line or a failing or racing hard link
  When trace preparation runs
  Then it retains any claim and fails without copy or symlink fallback

Scenario: Pin CLI input and keep preparation distinct from execution
  Test: fault attempt CLI rejects changed plans and never claims live readiness
  Given a changed or writable plan and a valid readonly plan
  When the actual CLI fixture runs
  Then failures exit nonzero and prepared output keeps full_acceptance false without launching a process

Scenario: Preserve CLI execution through distributed skill links
  Test: linked native control CLIs execute while library imports stay inert
  Given complete Claude and Codex skill directory symlinks
  When the native control or preparer CLI is invoked through a link with invalid arguments
  Then it returns its explicit nonzero JSON failure while importing either library has no side effects

## Out of Scope

- Observer metadata adaptation, stage binding/release, fault signals and live monitoring.
- Full interruption/restart/receipt/Matrix acceptance and global task mutation.
