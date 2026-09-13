spec: task
name: "Native Rust foundation with durable custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-PALPO-OUTBOUND]
tags: [active, rust, persistence, security]
---

## Intent

Start the migration in an isolated checkout with a runnable native Salvo service
and durable, bounded command custody. Preserve the deployed JS runtime during
development and make unfinished migration boundaries visible.

## Constraints

### Must
- Require explicit fresh state and operator credentials; bind only to loopback initially.
- Keep database work on a dedicated bounded worker and reject overload explicitly.
- Commit inbox payload and receipt together before acknowledging custody.
- Bind retries to their complete content and recover committed data after restart.
- Reject corrupt or newer schemas and concurrent state owners.
- Match the pinned JavaScript canonicalization for supported signed DTO values.
- Report unsupported runtime, transport and crypto capabilities honestly.

### Must Not
- Do not read live credentials or runtime stores or restart deployed services.
- Do not equate durable receipt with approval, provisioning, or external execution.
- Do not publish incomplete APIs as wire-compatible production replacements.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- ./rust-toolchain.toml
- ./.gitattributes
- native/**
- .github/workflows/rust.yml
- scripts/check-spec-bindings.js
- tests/spec-bindings.test.js
- ./.gitignore
- specs/project.spec.md
- specs/task-rust-foundation.spec.md
- knowledge/**
- docs/**

### Forbidden
- Existing JS/TS implementation, generated workspace entry files, live state and website files.

## Acceptance Criteria

Scenario: Custody survives restart without admitting work
  Test: custody_survives_restart
  Given a validated delivery and exclusive fresh state
  When native custody is acknowledged and the repository is reopened
  Then its payload and receipt remain committed and it remains unprocessed

Scenario: Retries cannot change committed content
  Test: retries_are_content_bound
  Given a committed request identifier
  When concurrent retries contain equal or different payloads
  Then equal retries return the original receipt and different content is refused

Scenario: State has one compatible owner
  Test: state_ownership_and_schema_fail_closed
  Given a repository that is locked corrupt or newer than supported
  When a service attempts to open it
  Then startup fails without rewriting the state

Scenario: HTTP authority and limits fail closed
  Test: http_auth_and_limits
  Given absent incorrect or browser-origin operator authority
  When a protected HTTP request is submitted
  Then it is refused before any state mutation and large bodies are rejected

Scenario: Private files reject broader local access
  Test: private_storage_rejects_public_access
  Given a state credential whose permissions allow another local user to read it
  When the native credential reader opens it
  Then it refuses the credential without reading its contents

Scenario: Background work does not stall control requests
  Test: bounded_work_keeps_health_responsive
  Given a stalled database worker and a full bounded queue
  When health and additional custody requests arrive
  Then health responds promptly and excess work returns a busy response

Scenario: Health and ready report the component rollup
  Test: native_health_readiness_ready
  Given a serving app wired the way Bootstrap serve wires it with both writers open and the ceiling sweep loop attached
  When the unauthenticated health and ready boundaries are read
  Then both answer two hundred with every component named by state word and no private counts and readiness never requires a sweep to have run

Scenario: A refused sweep tick never fails readiness
  Test: native_health_readiness_refused_tick_is_ready
  Given a live sweep task whose last tick was refused busy the word the loop publishes under worker saturation
  When the health and ready boundaries are read
  Then both answer two hundred naming the loop alive and the refusal on the wire for diagnosis because a refused tick is a live loop waiting for the next tick

Scenario: Every readiness vocabulary word is enumerated
  Test: native_health_readiness_enumerates_every_state
  Given the one component state enum from which the wire words and the ready predicate both derive
  When every variant is scored
  Then each word and ready answer is pinned so the word set and the predicate can never disagree and tick outcomes are always ready

Scenario: Ready names a stopped sweep and health stays live
  Test: native_health_readiness_names_stopped_sweep
  Given the ceiling sweep loop cancelled and finished while both writers remain open
  When the health and ready boundaries are read
  Then health answers two hundred with the stopped sweep named in the body while ready answers five oh three with the same component list and the healthy components stay named and open

Scenario: Ready names a closed domain writer and health stays live
  Test: native_health_readiness_names_closed_domain_writer
  Given the domain writer shut down while the custody store remains open
  When the health and ready boundaries are read
  Then health answers two hundred with the closed writer named in the body while ready answers five oh three never a silent two hundred

Scenario: Signing bytes preserve existing wire semantics
  Test: canonical_vectors_match_javascript
  Given sanitized Unicode null integer and property-order vectors
  When the Rust canonical encoder runs
  Then its bytes and digests match the pinned JavaScript implementation

Scenario: Fresh Rust crypto state retains its device and room key
  Test: crypto_device_survives_restart
  Given a fresh encrypted SDK store and a fixture room
  When its native device encrypts a message and reopens the store
  Then the same device decrypts the message and a different device identity is refused

Scenario: Incomplete migration capabilities are explicit
  Test: unimplemented_capabilities_are_explicit
  Given the native foundation service
  When its authenticated capabilities are read
  Then Agent execution transport and production API parity remain unavailable

Scenario: A supervisor-less service fixture starts and reports ready
  Test: native_service_unit_starts_and_reports_ready
  Level: integration
  Test Double: fixture harness spawning the serve binary and polling its routes in an ungated test file
  Given a fresh private state directory and the serve command the systemd unit renders
  When the harness starts the binary and polls health and ready with #[cfg(target_os = "linux")] inside the body
  Then on Linux ready answers 200 only after every component reports a ready word with the existing vocabulary
  And on the other hosted OSes the same named test asserts the documented not-a-Linux-service refusal never skipping

Scenario: A supervisor-less service fixture stops cleanly inside the budget
  Test: native_service_unit_stops_cleanly_within_timeout_budget
  Level: integration
  Test Double: fixture harness sending SIGTERM and observing the existing drain sequence
  Given a running serve process with open writer channels on Linux
  When the harness sends SIGTERM within the unit's TimeoutStopSec budget
  Then ready flips to 503 naming the stopped components while health keeps 200
  And the bounded writer drains and both stores close per ADR-120 leaving WAL files for replay
  And the process exits zero before the budget expires or parks on an unknown close without exiting zero
  And on the other hosted OSes the named test asserts the not-a-Linux-service refusal never skipping

Scenario: Pending state survives a service restart
  Test: native_service_unit_restart_preserves_pending_state
  Level: integration
  Test Double: fixture harness seeding an outcome-unknown row then restarting the serve binary
  Given a seeded pending or outcome-unknown custody row and a running service on Linux
  When the harness stops the service and starts it again on the same state directory
  Then the pending row is still reported pending or unknown and never resolved or dropped
  And the store stays locked to every other process between the two runs
  And on the other hosted OSes the named test asserts the not-a-Linux-service refusal never skipping

Scenario: The rendered launchd agent carries the required keys
  Test: native_launchd_agent_starts_and_reports_ready
  Level: integration
  Test Double: fixture harness rendering the plist template and spawning the serve command it names in an ungated test file
  Given the launchd plist template rendered with explicit install and state placeholders
  When the harness checks its keys with #[cfg(target_os = "macos")] inside the body and runs the serve command the wrapper execs
  Then on macOS RunAtLoad KeepAlive true ThrottleInterval the log paths and the fixed loopback listen are present
  And ready answers 200 only after every component reports a ready word
  And on the other hosted OSes the same named test asserts the documented not-a-macOS-agent refusal never skipping

Scenario: The launchd stop path drains and exits inside the budget
  Test: native_launchd_agent_stops_cleanly
  Level: integration
  Test Double: fixture harness sending SIGTERM to the serve command under the plist wrapper
  Given a running native service started through the launchd wrapper on macOS
  When the harness sends SIGTERM the existing token drain handles
  Then ready flips to 503 naming stopped components while health keeps 200
  And the writer drains and stores close leaving WAL files before exit zero or a parked unknown close
  And the deliberate stop is launchctl bootout not kill by pid because KeepAlive true restarts a killed process
  And on the other hosted OSes the named test asserts the not-a-macOS-agent refusal never skipping

Scenario: The launchd restart preserves pending state
  Test: native_launchd_restart_preserves_pending_state
  Level: integration
  Test Double: fixture harness seeding an outcome-unknown row then re-running the wrapper
  Given a seeded pending or outcome-unknown custody row and a first run through the wrapper on macOS
  When the wrapper is stopped and run again on the same state directory
  Then the pending row is still reported pending or unknown and never resolved or dropped
  And no rotation is applied to the log files because none is configured
  And on the other hosted OSes the named test asserts the not-a-macOS-agent refusal never skipping

Scenario: The binary reports the workspace version
  Test: native_binary_version_matches_workspace
  Level: integration
  Test Double: the built serve binary run with --version beside the workspace manifest
  Given the workspace version in the root Cargo.toml and a freshly built native binary
  When the binary is invoked with --version
  Then the printed version equals the workspace [workspace.package] version exactly
  And no other version source such as package.json is consulted

Scenario: The release tree contains no Node entry point
  Test: native_release_entrypoints_scan_finds_no_node
  Level: integration
  Test Double: a staged release tree fixture built like the workflow packages it
  Given a staged native release tree containing the binary its units and its checksums
  When the scan looks for package.json node_modules directories javascript entry files and node references in shipped wrappers
  Then the scan finds none and passes
  And any finding fails the scan never skips

Scenario: The runbook dry-run proves version identity on a temp state directory
  Test: native_cutover_dryrun_version_identity
  Level: integration
  Test Double: the built binary run with --version against a fresh temp state directory
  Given a temp state directory initialized by hagency init and the runbook's step 0 precondition
  When the binary's --version output is compared with the workspace version and the artifact naming rule
  Then the reported version equals the workspace [workspace.package] version exactly
  And a mismatch fails the dry-run before any service start

Scenario: The runbook dry-run gates on ready and proves the stop contract
  Test: native_cutover_dryrun_ready_gate_and_stop_contract
  Level: integration
  Test Double: fixture harness starting serve on a temp state directory sending SIGTERM and polling both routes
  Given the runbook's step 3 and step 6 on a temp state directory with the service started
  When /ready is polled to ok before SIGTERM is sent within the stop budget
  Then the gate observed ready ok and never relied on health which is 200 while live
  And after SIGTERM ready flips to 503 naming stopped components while health keeps 200
  And the process exits zero or parks on an unknown close inside the budget without a false success

Scenario: The runbook dry-run preserves pending rows across the stop-start pair
  Test: native_cutover_dryrun_pending_preserved_across_restart
  Level: integration
  Test Double: fixture harness seeding an outcome-unknown row then performing the runbook's stop and start pair
  Given the runbook's steps 6 and 7 on a temp state directory with a seeded pending or outcome-unknown row
  When the service is stopped and restarted on the same state directory
  Then every pending or unknown row before the stop is still reported pending or unknown after the start
  And no row is resolved dropped or marked done by the stop-start pair

## Out of Scope

The spec-binding build tool may distinguish native Cargo contracts from Vitest
contracts. Both catalogs remain mandatory in their corresponding CI jobs.

Production cutover, compatibility aliases, real Agent execution and automatic
approval. Later task contracts port these behaviors without relaxing the project
invariants. An isolated foundation is not a parity release.
