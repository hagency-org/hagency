spec: task
name: "Collect exact Darwin observer process arguments and cwd"
inherits: project
satisfies: [REQ-INNER-LOOP-MONITOR]
tags: [darwin, herdr, process, recovery]
---

## Intent

Provide the missing PID metadata query for a fault observer outside the lower
Herdr pane, so the recovery coordinator can verify its real argv and cwd with
the existing before/after birth snapshots. Preserve every original E2E gate.

## Constraints

### Must
- Accept only --pid followed by one canonical positive decimal PID within INT_MAX.
- Obtain actual kernel data through Darwin KERN_PROCARGS2, PROC_PIDVNODEPATHINFO and PROC_PIDTBSDINFO using the host SDK declarations.
- Observe target pointer width from PROC_FLAG_LP64, verify BSDINFO before/after, and skip only the exact XNU alignment padding after the executable path.
- Emit one version1 JSON object containing pid, cwd and argv only after fully validating both observations.
- Parse exactly the native argc NUL-terminated arguments, preserving empty arguments after argv0 and token boundaries.
- Reject empty argv0, malformed UTF8, missing terminators, invalid counts, oversized buffers and short syscall results.
- Exclude environment data from parsing, stdout, stderr and artifacts; acknowledge the kernel buffer contains environment bytes in memory.
- Keep successful output compatible with assertProcessIdentity; the caller must still collect and verify before/after births under its own deadline.
- Test only synthetic byte buffers and independently owned local fixture processes, never managed agents or live services.

### Must Not
- Split ps display text, substitute launch plans or executable paths for observed argv/cwd, or fall back after a failed observation.
- Send signals, change runtime state, read business data or replace the existing birth collector.
- Claim atomic process snapshots, cross-platform live acceptance, or full E2E completion.

## Decisions

- Add one small C source under the inner-loop skill native directory; no external libraries or runtime compilation.
- Build only with the host C compiler and SDK; tests compile into their private temporary directory.
- Bound raw argument data to4MiB and argc to16384. Observe a monotonic1.5second internal budget before/after syscalls; a parent timeout remains necessary for a blocked syscall.
- Parse with observed pointer width4 or8: padding=(width-((16+exec_path_bytes_including_NUL)%width))%width. Require those exact padding bytes zero and reject an empty argv0 at that boundary without scanning onward.
- C parser/JSON tests use a separately compiled test driver; no test injection flags or data paths are accepted by the production CLI.
- Non-Darwin production builds exit nonzero with an explicit unsupported_platform category. No Linux or Windows metadata implementation is implied.
- Build command is /usr/bin/clang -std=c11 -O2 -Wall -Wextra -Werror SOURCE -o OUTPUT; distribution requires an explicit prebuilt binary pin in the later coordinator plan.

## Boundaries

### Allowed Changes
- skills/hagency-inner-loop/native/darwin-process-metadata.c
- tests/fixtures/darwin-process-metadata-driver.c
- tests/native-process-metadata.test.js
- skills/hagency-inner-loop/SKILL.md
- specs/task-native-process-metadata.spec.md
- knowledge/decisions/adr-036-native-process-metadata.md
- docs/superpowers/plans/2026-09-13-native-process-metadata.md

### Forbidden
- router/**
- backend-v2.js
- bridge-matrix.js
- Frozen observer/adapter files, live process state, runtime profiles and lower business trees

## Acceptance Criteria

Scenario: Observe exact arguments and cwd of an owned fixture
  Test: reads exact argv tokens and cwd from an independently owned process
  Given a fixture with a distinct argv0 spaces empty arguments Unicode and a private cwd
  When the actual native CLI queries that PID
  Then JSON preserves those exact tokens and cwd without environment fields

Scenario: Reject unsupported command forms and vanished processes
  Test: rejects invalid PID commands and nonexistent processes without metadata
  Given malformed or extra arguments or a missing process
  When the actual CLI runs
  Then it exits nonzero without publishing process metadata

Scenario: Parse complete argument buffers without consuming environment
  Test: parses exactly argc arguments and never serializes the environment suffix
  Given a native-endian buffer with executable padding empty argv tokens and an environment suffix
  When the production parser is exercised through the test-only driver
  Then it preserves exactly the declared arguments and excludes the suffix

Scenario: Reject malformed kernel argument data
  Test: rejects malformed argument buffers and invalid UTF8 before output
  Given bad counts missing terminators overlong encodings surrogates or oversized input
  When parsing runs
  Then it fails without partial JSON or environment disclosure

Scenario: Validate syscall failures and complete cwd data
  Test: rejects failed and short native observations without a fallback
  Given a failed or short syscall or unterminated cwd supplied at compile time in the isolated driver
  When the collector executes
  Then it rejects the observation without stale or fabricated metadata

Scenario: Empty argv0 cannot shift parsing into environment
  Test: rejects an owned process with empty argv0 without exposing its environment
  Given a synthetic owned process with empty argv0 followed by arguments and an environment sentinel
  When the actual CLI queries it using the observed target pointer width
  Then it fails with empty stdout before interpreting the sentinel as a missing argument

## Out of Scope

- Stage release/observer launch, signals, native control mutations and full recovery acceptance.
- Managed-process live permission verification, which remains required during integration.
