spec: task
name: "Keep native recovery artifacts and logs private with documented retention"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, retention, artifacts, platform]
---

## Intent

Implement accepted ADR126's native half: name the retention posture native already
has (stderr only, structural media bounds, no eviction path) and prove it, so the
migration plan's cutover step 9 requirement to "keep recovery artifacts private
with documented retention" (`docs/design/hagency-rust-migration-plan.md:489`) has
a bound a reader can point at rather than an
assumption. The retained jsonl rotations and the retained media-cache prune are
designed in the same ADR but are bound by the sibling Node contract
`specs/task-rust-artifact-retention-node.spec.md`, because a retained Vitest
selector can never appear in the Cargo inventory.

## Decisions

Retention on the native side is a **posture with no sweep**: nothing prunes, and
the bound is a refusal at the capacity edge rather than a reclaim. The one thing
this slice adds to production behaviour is none — its additions are documentation,
plus the assertions that pin the posture.

Every scenario below is bound on **every hosted leg**. No scenario is gated by a
file-level OS condition, and where a behaviour genuinely differs by OS the test
names the **refusal** on the other OS instead of disappearing from the leg.

## Constraints

### Must
- Assert the stderr-only logging posture: no file sink is constructed by the native
  service or the MCP helper on any leg.
- Assert the media store's capacity refusal at its declared bound instead of an
  eviction, and that `committed_records`/`occupied_bytes` are observably unchanged
  by the refusal.
- Assert that an unsettled operation survives a reopen: `committed_records()` is
  unchanged and `read(operation)` still succeeds.
- Assert that owned-dispatch fixture artifacts are `TempDir`-owned and self-cleaning,
  and that no production writer creates an `owned-dispatch.*` path.
- Assert that a received-file partial destination survives an unknown outcome.
- Keep every fixture `tempfile`-owned; contact no live service and no live model.
- Compare paths on canonicalized `PathBuf`s or component-wise joins, never on a
  string that assumes one separator.

### Must Not
- Do not add a sweep, prune, eviction or delete path to the media store.
- Do not add a file log sink to the native service or the MCP helper.
- Do not put retention work on the request path or on the domain writer.
- Do not treat a retention failure as a refusal of new work.
- Do not gate a whole test function by OS; express a platform difference as a
  named refusal assertion on the other leg.
- Do not change the media store's declared limits, the workspace file bound or any
  store schema.

## Boundaries

### Allowed Changes
- native/hagency-media-store/src/**
- native/hagency-media-store/tests/**
- native/hagency-execution/src/**
- native/hagency-execution/tests/**
- native/hagency/src/main.rs
- native/hagency/src/mcp/**
- native/hagency/tests/**
- knowledge/decisions/adr-126-native-recovery-artifact-retention.md
- specs/task-rust-artifact-retention.spec.md
- specs/task-rust-artifact-retention-node.spec.md
- docs/progress.md

### Forbidden
- Live services, live models, deployed Node runtime and Matrix server changes.

## Acceptance Criteria

Scenario: The media store refuses at capacity instead of evicting
  Test: native_media_store_capacity_refuses_rather_than_evicts
  Level: integration
  Test Double: real media store on a tempfile-backed private directory at a small injected bound
  Given a media store holding committed records and an operation that would exceed a declared limit
  When the write is attempted
  Then it is refused with the capacity error, every previously committed record is still readable, and the committed record count and occupied byte figures are unchanged by the refusal

Scenario: An unsettled operation survives a reopen
  Test: native_media_store_survives_reopen_with_unsettled_operation
  Level: integration
  Test Double: real media store closed and reopened over the same private directory
  Given a staged operation whose dispatch is not resolved and whose record is committed
  When the store is closed and reopened
  Then the committed record count is unchanged and reading the original operation succeeds, so an unknown-fate artifact is retained rather than reclaimed

Scenario: Owned-dispatch fixture artifacts are temp-owned and self-cleaning
  Test: native_owned_dispatch_fixture_artifacts_are_temp_owned
  Level: integration
  Test Double: the existing owned-dispatch harness over a Unicode tempfile directory
  Given an owned-dispatch fixture run that writes its marker files
  When the run ends and its owner is dropped
  Then every marker lived under the dropped temporary directory, no owned-dispatch path is written anywhere a production state directory reaches, and nothing survives the drop

Scenario: A partial receive destination survives an unknown outcome
  Test: native_receive_partial_destination_survives_unknown_outcome
  Level: integration
  Test Double: real receive workspace over a tempfile-backed private directory
  Given a partially written received-file destination whose operation outcome is unknown
  When the unknown outcome is observed and recovery runs
  Then the partial destination is still present and unmodified, because it is the only evidence that the operation may have run

Scenario: Native logs to stderr and constructs no file sink
  Test: native_logs_to_stderr_with_no_file_sink
  Level: integration
  Test Double: the native service and MCP helper started with a private fresh state directory
  Given a native service and an MCP helper started against a fresh private state directory
  When each emits its startup and refusal diagnostics
  Then every diagnostic arrives on stderr, no log file appears anywhere under the state directory or the runtime root, and on a platform whose log redirection is not yet implemented the run reports the named not-implemented refusal rather than silently writing a file or skipping the assertion

## Out of Scope

Retained-side jsonl rotation and the retained media-cache prune (bound by the
sibling Node contract); the Windows service wrapper's log redirection, deferred to
M8; the macOS installer's retention sentence, an acceptance item recorded in
ADR126 and not implemented here; store-resident retention slices and the retention
tick; terminal parity and full M7/M8 completion.
