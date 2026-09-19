spec: task
name: "Materialize one received attachment under its original Started workspace"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, files, workspace]
---

## Intent

Implement the bounded workspace sink partition of accepted ADR105. The application
retains a non-Clone WorkspaceReceive before any effect. Matrix authentication,
service queues and executable receive integration are separately owned gates.

## Constraints

### Must
- StartedWorkspace prepare_receive borrows the original workspace and capability and consumes only the unique ReceiveWrite with its original absolute deadline without filesystem IO or await.
- Retain the same private Binding and original writer root and retirement; require the opaque write grant to match the complete original capability and never expose a root or cloneable write authority.
- Borrowed materialize marks the attempt before its first await and retains the actual newly created file before any seal or plaintext write; errors unwind and caller loss cannot rearm that owner.
- Use generated .hagency-received-32hex.bin from the receive identity and relative create_new NoFollow nonblocking regular-file checks private current-owner permissions and one link.
- Check original current writer and workspace before and after effects with local retirement checks around bounded write and read chunks.
- Retain and compare the actual original file with its current relative entry using full platform identity then bound actual readback length and SHA256 and require file and directory sync acknowledgements.
- On Unix retain a readable directory handle opened relative to the original root and verify complete directory identity before any file creation; never try to sync a Linux O_PATH descriptor or substitute an ambient reopen.
- Revalidate a completed file read-only using a fresh bounded deadline without renewing write authority or touching its bytes.
- Run real temporary file mutation authority expiry and retained-owner tests and preserve all failures skips uncertain and unsupported platform results.

### Must Not
- Never overwrite truncate rename unlink repair follow a link or reopen an ambient destination.
- Never add Matrix or media dependencies to execution or files or introduce per-request workers or a pathname authority fallback.
- Never describe cross-compilation or OS sync acknowledgement as actual Windows execution or physical power-loss evidence.

## Boundaries

### Allowed Changes
- native/hagency-execution/Cargo.toml
- native/hagency-execution/src/lib.rs
- native/hagency-execution/src/workspace.rs
- native/hagency-execution/src/workspace/received.rs
- native/hagency-execution/tests/owned.rs
- native/hagency-execution/tests/owned/receive.rs
- ./Cargo.lock
- specs/task-rust-receive-workspace-sink.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Original Started workspace retains one verified destination
  Level: integration
  Test Double: actual original child Started receipt writer and private temporary files
  Test: native_receive_workspace_sink_original
  Given a current original Started workspace and unique committed write grant
  When checked bytes materialize once and the retained result is reread
  Then the generated relative destination contains exact private single-link bytes
  And replay is read-only while changed bytes objects links permissions and roots refuse

Scenario: Preparation and failed materialization cannot manufacture another write
  Level: integration
  Test Double: actual original Started receipt occupied destinations and unique writer grants
  Test: native_receive_workspace_once
  Given original current authority and generated occupied or mismatched destinations
  When preparation materialization errors caller loss and a repeated attempt occur
  Then preparation creates nothing and an attempted owner never rearms or modifies an existing entry
  And the actual original destination remains retained across error and worker unwind

Scenario: Expired and retired original bindings deny effects and output
  Level: integration
  Test Double: actual canonical retirement original binding and absolute deadlines
  Test: native_receive_workspace_authority_retirement
  Given current wrong expired and retired original workspace authority
  When writing or read-only result validation checks the captured writer
  Then stale authority cannot create a destination or return a cached result
  And a fresh read-only deadline cannot revive an attempted write

Scenario: Read-only replay refuses changed physical objects
  Level: integration
  Test Double: actual private temporary files and controlled entry content link and root mutations
  Test: native_receive_workspace_sink_mutations
  Given one materialized original destination in its original retained root
  When bytes size link count permissions or the physical root changes
  Then revalidation refuses and repeated materialization cannot overwrite or repair the entry

Scenario: Fresh read-only deadlines retain current original authority
  Level: integration
  Test Double: actual elapsed original deadline and original writer capability expiry
  Test: native_receive_workspace_authority_read_deadline
  Given an original materialized file whose write deadline has expired
  When a fresh read-only deadline validates bytes then the original capability expires
  Then current read-only replay succeeds only before authority expires and never rearms the write

## Decisions

The root agent accepted this exact ten-path partition under ADR105. Reuse the
existing pinned cap-fs-ext and sha2 versions only. ADR104 WindowsDirectorySync is
a separate integrated prerequisite; no unsupported fallback qualifies success.
Parse and lint before code, run the bound tests and existing owned workspace
regressions, warnings-denied native and Windows GNU Clippy, then strict lifecycle
against the complete sink changed-path union. Record platform limits separately.

## Out of Scope

Matrix ingress or download application queues MCP service integration cache
record mutation recovery cleanup production cutover and hostile same-UID control.
