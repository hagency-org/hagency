spec: task
name: "Report persisted native handle terminals before task closeout"
inherits: project
satisfies: [REQ-INNER-LOOP-MONITOR]
tags: [agents, evidence, cli]
---

## Intent

Prevent a middle agent from reporting a failed native execution as pending
after its actual terminal return has been saved. Summarize that agent's local
execution records without controlling the process or claiming E2E acceptance.

## Constraints

### Must
- Bind the initial resumable return and later polling requests to one handle.
- Reject missing, corrupt, writable or changed record files.
- Keep a missing terminal distinct from current process liveness.
- Do not print command bodies or raw output.

## Decisions

- [JS-only] Use Node.js built-ins and deterministic Vitest tests.
- CLI input is --records DIR --session-id ID; outputs always keep full_acceptance false.
- Exit 0 means observed exit zero, 1 observed nonzero, 2 terminal not observed, 3 invalid evidence.

## Boundaries

### Allowed Changes
- skills/hagency-inner-loop/scripts/native-handle-summary.mjs
- skills/hagency-inner-loop/SKILL.md
- bin/hagency-sync-skills
- tests/native-handle-summary.test.js
- tests/skill-sync.test.js
- specs/task-native-handle-summary.spec.md
- docs/superpowers/plans/2026-09-13-native-handle-closeout.md

### Forbidden
- Do not poll a live handle or mutate existing evidence.
- Do not change lower business source, checkpoints or goals.

## Acceptance Criteria

Scenario: An observed observer failure remains terminal
  Test: reports the persisted failed observer terminal
  Given a resumable return and later exit 1
  When the records are summarized
  Then the result is exited with code 1 and full_acceptance false

Scenario: Missing terminal does not assert liveness
  Test: reports missing terminal without claiming a running process
  Given only nonterminal polling returns
  When the records are summarized
  Then the result is terminal_not_observed

Scenario: Omitted poll input preserves the native default
  Test: accepts omitted empty input and refuses actual input
  Given a polling request omits chars
  When the terminal is summarized
  Then it accepts the default empty input and rejects nonempty input

Scenario: Arbitrary diagnostic output remains private
  Test: does not expose arbitrary reason strings from private output
  Given a terminal output contains a private reason string
  When the records are summarized
  Then only an explicitly public observer code can appear in the summary

Scenario: Contradictory histories are rejected
  Test: rejects mixed handles and returns after terminal
  Given a wrong handle or polling after a terminal return
  When the records are summarized
  Then invalid evidence is reported

Scenario: Corrupt and writable files are rejected
  Test: rejects corrupt writable and symlinked records
  Given an invalid local evidence file
  When the CLI reads the directory
  Then it exits 3 without private output

Scenario: CLI reports actual failed execution
  Test: CLI emits a compact failed terminal summary
  Given valid readonly records with exit 1
  When the CLI receives the records directory and exact handle
  Then it exits 1 and emits only the structured summary

Scenario: Distribution includes the reader
  Test: skill sync refuses missing native handle summary before modifying client links
  Given a source checkout missing the summary resource
  When skill synchronization runs
  Then it refuses before changing client links

## Out of Scope

- Authenticating records produced by arbitrary same-user writers.
- Process liveness, output interpretation as task acceptance or polling cadence acceptance.
- Repairing a previous missed fault window or rewriting failed results.
