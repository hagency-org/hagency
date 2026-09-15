spec: task
name: "Correlate Herdr native controls with exact process and trace evidence"
inherits: project
satisfies: [REQ-INNER-LOOP-MONITOR]
tags: [herdr, evidence, recovery]
---

## Intent

Replace the repeated Q3 control-script errors with reusable, tested middle-agent
tools. Inspect an assigned Octoscode session and pause or resume its existing
goal through the authorized Herdr session, retaining the actual request and
response association without replaying an uncertain control.

## Constraints

### Must
- Compare exact PID, PPID, process group, birth string, cwd and argv tokens across fresh observations; reject missing evidence.
- Preserve existing files and failed attempts; publish evidence exclusively as readonly regular files.
- Capture trace inode, prefix and offset before sending a command; match its exact method, parameters and response ID.
- Validate the original goal ID, creation time and objective digest before and after a mutation.
- Preserve a durable action intent before sending a mutation; a reused operation ID must never send another command.
- Report completed tool observations separately from full E2E acceptance.
- Run deterministic Vitest regressions using local temporary files and child-process fixtures, without live services.

### Must Not
- Split a command-line string to infer argv, accept PID substrings, ignore missing proof or treat mutable process state as birth identity.
- Overwrite a checkpoint, receipt, prior evidence file or old action intent.
- Retry a dispatched command after a timeout or uncertain response.
- Change Matrix permissions, native sandbox policy, runtime deadlines, existing frozen E2E inputs or task status.

## Decisions

- [JS-only] Node22 ESM and built-in filesystem, crypto and child-process modules; no new dependencies.
- [macOS-only] The first live process adapter uses the existing pinned Darwin birth collector plus Herdr's tokenized foreground process metadata. Other operating systems require an explicit adapter and are not reported as verified.
- The library separates evidence/process primitives from native request correlation and the bounded CLI.
- CLI plans are readonly files pinned by a required SHA256 argument. Supported operations are inspect, goal-pause and goal-resume; shell commands and arbitrary native prompts are not accepted.
- Goal resume requires the exact goal to be paused or blocked, a complete terminal-only turn array and Herdr's detected idle state, displayed as idle when seen or done when unseen. Loop creation, resume and deletion use the same idle display check. Neither display value proves task completion. Goal pause may stop future work while an existing turn settles; its response never claims session idle.
- Each mutation uses a fresh operation ID and an exclusive evidence directory. Missing or conflicting responses after intent publication yield outcome_unknown and retain evidence.
- CLI stdout is one bounded JSON result without raw goal objectives, nonces, argv or subprocess stderr. Failures return nonzero and full_acceptance remains false.

## Boundaries

### Allowed Changes
- skills/hagency-inner-loop/scripts/native-control-evidence.mjs
- skills/hagency-inner-loop/scripts/native-control.mjs
- skills/hagency-inner-loop/SKILL.md
- tests/inner-native-control-evidence.test.js
- tests/inner-native-control.test.js
- tests/fixtures/fake-herdr-native-control.mjs
- tests/skill-sync.test.js
- bin/hagency-sync-skills
- specs/task-inner-native-control.spec.md
- knowledge/decisions/adr-033-native-control-evidence.md
- docs/superpowers/plans/2026-09-13-native-control-evidence.md

### Forbidden
- router/**
- backend-v2.js
- bridge-matrix.js
- Live runtime credentials, source projects, checkpoints and frozen E2E records

## Acceptance Criteria

Scenario: Real argument boundaries and mutable process states survive inspection
  Test: preserves exact argv tokens while allowing mutable process state changes
  Given a bound process with a spaced stdio-command argument and unchanged birth
  When fresh observations change running state to sleeping state
  Then its exact argument array and immutable identity still match

Scenario: Missing or replaced identities fail before action
  Test: rejects missing duplicate replaced and mismatched process identities
  Given missing metadata duplicate PIDs changed birth cwd arguments or parent
  When identity validation runs
  Then it rejects the observation without a substring or optional-proof fallback

Scenario: Relative captures resolve without escaping the project
  Test: validates relative readonly UUID captures and rejects unsafe paths
  Given a relative checkpoint capture and malformed absolute traversing or symlink paths
  When the capture path is resolved
  Then only the regular readonly UUID capture inside the canonical project is accepted

Scenario: Existing evidence cannot be overwritten
  Test: publishes readonly JSON exclusively and preserves collisions and symlinks
  Given an existing evidence file or symbolic link
  When another publication targets that path
  Then publication fails and original bytes remain unchanged

Scenario: Immediate responses are correlated from a pre-send fence
  Test: correlates an immediate response emitted before prompt delivery returns
  Given a local Herdr child fixture that appends its response before exiting
  When a readonly native query is sent
  Then the helper finds its request and matching response from the earlier fence

Scenario: Ambiguous or rewritten traces fail
  Test: rejects changed trace history wrong scope duplicate requests and mismatched responses
  Given a replaced trace rewritten prefix wrong session duplicate request or RPC error
  When the helper observes a native request
  Then the observation fails without accepting unrelated data or resending

Scenario: Existing goal identity remains unchanged through control
  Test: pauses and resumes only the original goal and retains correlated evidence
  Given a bound goal with its original creation time and objective digest
  When one pause or eligible resume is sent
  Then its actual same-ID response is retained and full_acceptance is false

Scenario: Resume rejects unknown or active work
  Test: rejects missing goals invalid turns changed goals and active work before resume
  Given a missing or different goal malformed turns or an active native turn
  When resume is requested
  Then no resume command is delivered

Scenario: Unseen idle panes support existing goal and loop controls
  Test: accepts unseen Herdr done only with terminal native turns
  Given the exact paused goal and complete terminal-only turns with Herdr done
  When goal resume or loop creation resume or deletion is requested
  Then exactly one bound mutation is delivered and full acceptance remains false

Scenario: An unseen display cannot hide an active native turn
  Test: rejects active native turns even when Herdr reports done
  Given Herdr done and a fresh active native turn
  When goal resume or loop creation resume or deletion is requested
  Then no mutation is delivered

Scenario: Other display states do not establish idle
  Test: rejects non-idle Herdr display states before native controls
  Given terminal native turns with a working blocked unknown or missing Herdr state
  When goal resume is requested
  Then no mutation is delivered

Scenario: An uncertain mutation is never replayed
  Test: preserves mutation intent and rejects reuse after delivery timeout
  Given a local fixture that records a delivered control but withholds the response
  When the bounded observation times out and the same operation is requested again
  Then the first result is outcome_unknown and the second attempt sends no control

Scenario: CLI rejects changed plans and keeps diagnostics bounded
  Test: CLI pins readonly plans and returns bounded outcomes without private data
  Given a changed or writable plan and a fixture error containing a private marker
  When the actual CLI runs
  Then it returns nonzero without exposing the marker or sending a control

Scenario: Incomplete skill distribution fails
  Test: skill sync refuses missing native control resources before modifying client links
  Given a source skill missing either native control module
  When synchronization or check runs
  Then it refuses before modifying existing client links

Scenario: Live transport requires complete process proof
  Test: rejects lost process proof before delivery and a replaced process after delivery
  Given missing argument metadata changed births or a foreign terminal identity
  When the controller inspects the bound native session
  Then it stops without continuing commands after the first failed identity proof

Scenario: Evidence never writes into the business project
  Test: keeps control evidence outside the business project
  Given an evidence directory nested inside the lower business project
  When the controller receives a plan
  Then it rejects before creating an operation directory or sending a native command

Scenario: A concurrent resume is not claimed as this operation's success
  Test: reports a goal resumed by another actor during control as unknown
  Given the goal changes from paused to active before the control's internal get
  When the actual get and set responses are correlated
  Then the controller retains an unknown outcome without rollback or retry

Scenario: Nested goal scope cannot contradict the selected session
  Test: rejects missing and foreign nested goal scope
  Given a missing or foreign goal profile or conflicting optional goal session
  When the scoped goal response is validated
  Then the controller rejects it without continuing native commands

## Out of Scope

- Completing the full live goal, loop, fault, receipt or Matrix acceptance with unit tests.
- Granting owner approval or impersonating the lower implementation agent.
- Reusing Q3's consumed observer claim, old watch handle or expired manifest.
