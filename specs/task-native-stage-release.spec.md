spec: task
name: "Release one prearmed native recovery stage without replay"
inherits: project
satisfies: [REQ-INNER-LOOP-MONITOR]
tags: [herdr, recovery, control, evidence]
---

## Intent

Replace the failed ad-hoc Q3 release scripts with a bounded coordinator for
interrupt03 and restart04. Compose the reviewed native controller, process
metadata collector and protected-file primitives. The middle retains every
original natural-turn, watch, approval, fault and final acceptance gate.

## Constraints

### Must
- Accept a canonical readonly SHA256-pinned plan and retain all failed predecessor evidence.
- Verify the prepared attempt/contract/predecessor pins, original trace inode/prefix, exact stage binding and current protected artifacts before release.
- Require the frozen observer's readonly claim/ready/binding from this attempt and current exact PID/PPID/PGID/birth/cwd/argv proof through native metadata and birth snapshots.
- Require the same original goal paused/blocked, complete nonempty terminal-only history, Herdr idle and the expected loop state before activation.
- Claim the fixed attempt/stage activation directory exclusively, fsync intent before the release marker, and retain the claim after every later failure.
- Publish only the stage-specific release marker with no replacement, then send each planned control operation at most once.
- Order restart04 as release, original-loop resume, terminal settlement if a natural audit starts, original-goal resume, then a fresh active-state observation.
- Stop after an unknown or failed postrelease operation without retrying, rollback, loop recreation or compensating mutation.
- Preserve bounded private evidence and publish full_acceptance=false even when activation succeeds.

### Must Not
- Start or replace observers, send Ctrl+C/SIGKILL itself, query another agent's native handle, modify old deadlines or delete guards.
- Write lower business source/tests/checkpoints or synthesize04 facts before they exist.
- Treat pinned review inputs or an active goal/loop as proof that natural audits, interruption, restart or full E2E acceptance passed.

## Decisions

- CLI: node scripts/run-stage-release.mjs --plan FILE --sha256 HEX; library runStageRelease(plan) returns activated/failed/outcome_unknown/rejected with full_acceptance=false.
- The stage plan carries version, activation_id UUIDv4, stage interrupt03/restart04, attempt_manifest pin, prepared pin, observer_binding pin, observer_claim pin, observer_ready pin, observer exact process identity, metadata_tool pin, frozen_helpers pins, protected relative-path hash map, native control binding, expected_goal, optional expected_loop for04, child operation UUIDs, prerequisite_pins and query/settlement budgets.
- Activation and child operation IDs use lowercase canonical UUIDv4 strings, distinct and fixed by the plan. Validate each derived child plan against the pinned controller's input constraints before claiming the stage. The same attempt/stage claim cannot be bypassed with a new activation or child ID.
- Predecessor readonly files must retain their actual pins; prepared and manifest identify the same attempt and trace. Preparation establishes lineage, not live readiness.
- Frozen helper pins identify observer and adapter in the same directory. Observer argv must identify that exact script, binding and stage; actual argv/cwd are queried, never split from ps or inferred from a launch string.
- [platform-specific: macOS] Preserve an instance descendant spelled through the root-owned /tmp system symlink only when it resolves to /private/tmp and the corresponding descendant is canonical with no further symlink. Preserve the original instance string and exact process argv; all other evidence paths retain strict canonical validation. Reject unrelated aliases, descendant symlinks and lexical traversal, including during later evidence rechecks.
- Protected sets contain checkpoint, source snapshot, test-output capture, red-green record and its red/green captures for every prior increment:01/02 for03,01/02/03 for04. The frozen observer binding covers its exact checkpoint/source/test-output subset.
- Stage03 requires no live loop;04 requires the sole original fixed60second loop paused. The current native observations establish eligibility independently of older files.
- Prerequisite pins retain the middle's already reviewed evidence inputs.04 requires named pins for natural03, pre_restart_audits, prerequisite_waits, blocked_terminal_order, tiny_timeout and same_backend_readonly_recovery. This coordinator checks readability/digests/binding, not the semantics of those historical experiments; the original independent acceptance remains mandatory before constructing/authorizing a04 plan and at final audit.
- A bounded settlement loop uses only readonly inspect operations with distinct recorded IDs; it never fires an audit or repeats a mutation. If the loop cannot settle before the fixed budget, activation stays unknown after release.
- The parent deadline must bound subprocess observation and controls. No successful inspection or timeout implies safe restart.
- The plan pins the adjacent native-control.mjs and native-control-evidence.mjs as controller_tools.controller and controller_tools.evidence. Each control is an actual bounded Node child running the reviewed CLI; retain its exit/stdout/stderr in private evidence and require successful exit plus an observed/applied report. A timed-out child is never a verified mutation result.
- activation_timeout_ms is5000..240000, observer_timeout_ms and query_timeout_ms100..10000, settlement_timeout_ms100..120000, settlement_interval_ms100..5000. Fixed child_operations identify initial_inspect, goal_resume, final_inspect and, for04, loop_resume plus1..128 settlement_inspects; all UUIDv4s and activation_id differ. Recheck observer identity/freshness after initial inspect and after04 settlement before the next mutation.

## Boundaries

### Allowed Changes
- skills/hagency-inner-loop/scripts/run-stage-release.mjs
- skills/hagency-inner-loop/scripts/native-stage-evidence.mjs
- tests/native-stage-release.test.js
- tests/native-stage-evidence.test.js
- tests/fixtures/native-stage-fixture.mjs
- skills/hagency-inner-loop/SKILL.md
- bin/hagency-sync-skills
- tests/skill-sync.test.js
- specs/task-native-stage-release.spec.md
- knowledge/decisions/adr-037-native-stage-release.md
- docs/superpowers/plans/2026-09-13-native-stage-release.md

### Forbidden
- router/**
- backend-v2.js
- bridge-matrix.js
- Frozen Python, old attempts, business implementation and live runtime state during source preparation

## Acceptance Criteria

Scenario: Preserve the macOS system instance alias
  Test: accepts the macOS system instance alias without rewriting native argv
  Given an owned instance descendant uses the verified macOS system temporary alias
  When the stage plan is validated and rechecked
  Then the original instance spelling and complete argv remain unchanged
  And no stage release or native control is sent by validation

Scenario: Reject other instance aliases and traversal
  Test: rejects other instance aliases and lexical traversal
  Test: rejects an unsafe descendant inside the macOS instance alias
  Given an instance uses an unrelated alias a descendant symlink or lexical traversal
  When the stage plan is validated
  Then validation fails before a stage claim or release

Scenario: Recheck the macOS instance alias target
  Test: rejects a replaced macOS instance descendant during evidence recheck
  Given a validated macOS alias descendant is replaced by another symlink
  When stage evidence is rechecked
  Then validation fails before any native mutation

Scenario: Reject unsafe system alias metadata
  Test: rejects unsafe macOS system alias metadata
  Given the system alias has a wrong owner type indirect target or changed identity
  When the instance alias is validated
  Then validation fails without creating a stage claim or release

Scenario: Keep the alias exception platform scoped
  Test: does not enable the system alias exception outside macOS
  Given an instance spelling traverses the system temporary symlink
  When validation runs with a non macOS platform
  Then the canonical path rule rejects the symlink

Scenario: Preserve exact arguments and canonical evidence paths
  Test: does not rewrite native argv when accepting the macOS instance alias
  Given the native instance argument or a canonical evidence path is changed
  When the stage plan is validated
  Then the unchanged identity and path rules reject the plan
  Test: does not extend the macOS instance alias exception to evidence paths

Scenario: Release03 only after actual observer readiness
  Test: releases03 after ready live identity and protected checks before one goal resume
  Given a prepared new attempt and the original paused goal with a live exact observer
  When the coordinator activates interrupt03 once
  Then durable intent and release precede exactly one goal resume and the original goal is observed active

Scenario: Missing or mismatched readiness sends no control
  Test: rejects absent stale foreign dead or changed observer proof before release
  Given missing ready evidence a foreign attempt changed birth or mismatched token array
  When stage activation is requested
  Then neither release nor a native mutation is published

Scenario: Preserve release collisions and consumed attempts
  Test: retains collisions and refuses replay under different activation IDs
  Given an existing release marker or a previously claimed stage activation
  When activation is requested again
  Then previous bytes remain unchanged and no additional control is sent

Scenario: Postrelease uncertainty remains consumed
  Test: preserves release and intent after a failed or uncertain goal resume
  Given a published03 release followed by missing or rejected control evidence
  When the control finishes without a verified applied result
  Then the result remains unknown with no rollback or retry

Scenario: Restart activation respects original loop and goal order
  Test: releases04 resumes the original loop settles its audit and resumes the original goal
  Given real pinned04 prerequisites and the original blocked goal and paused loop
  When restart04 activation runs
  Then release precedes loop resume and terminal settlement precedes the one goal resume

Scenario: Restart partial activation cannot replay
  Test: stops after loop or goal uncertainty and preserves the consumed04 activation
  Given loop uncertainty goal preflight failure or an unknown postactivation observation
  When restart04 activation runs
  Then only the reached operations are sent once and all later mutations are withheld

Scenario: Reject changed lineage and protected data
  Test: rejects changed attempt trace helper or prior increment evidence before release
  Given changed pins trace identity or missing protected artifacts
  When the coordinator checks a plan
  Then it publishes no release and sends no native mutation

Scenario: Refuse mutation dispatch after publication ages observer proof
  Test: retains consumed interrupt03 without dispatch when release publication outlives observer freshness
  Given a released stage whose filesystem publication outlives the two second observer freshness window
  When the coordinator reaches the next mutating child dispatch
  Then the release and intent remain and the result is unknown without sending a mutation

Scenario: Invalid CLI arguments produce a failing process exit
  Test: rejects invalid CLI forms with bounded JSON and a failing process exit
  Given missing invalid or unknown CLI arguments
  When run-stage-release executes
  Then stdout contains bounded failed JSON with full_acceptance false stderr is empty and the process exits one

Scenario: Refuse an incomplete distributed stage package
  Test: skill sync refuses missing stage resources before modifying client links
  Given a missing coordinator evidence module or metadata collector source
  When hagency-sync-skills runs in check or synchronization mode
  Then it exits one with the missing resource on stderr and preserves client links

## Out of Scope

- Declaring historical prerequisite semantics valid from a pass label or digest alone.
- Performing the observer's fault action, natural business work or final Matrix acceptance.
