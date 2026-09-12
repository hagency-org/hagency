spec: task
name: "Bind fresh native credential namespaces to the original managed Host"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, accounts, execution, cli]
---

## Intent

Implement accepted ADR114 part A from 1baa80d within the exact 38 approved paths.
Schema23 is reserved. Part B browser enrollment waits for this qualified API.

## Constraints

### Must
- Create only fresh private native Codex default credential namespaces under explicit state.
- Report effective authentication provider account identity and provider quota as unknown.
- Retain exact original directory proof and one-shot preparation custody before filesystem effects.
- Derive versioned keyed identity from actual namespace facts without aliasing legacy homes.
- Preserve immutable managed association through first resource creation ADR111 cloning and editing.
- Require exact managed account matching and environment consumption in the real Host and owned scope.
- Reject managed resources in the old development-only fixed environment path.
- Preserve original absolute deadlines after SQLite waits and around filesystem IO and commit.
- Fence retirement publication and runtime admission while preserving commitments and cleanup custody.
- Preserve actual schema22 data and record actual nonzero test counts and failure evidence.
- Use only the exclusively assigned prepared-stage-target after the parent releases the Cargo hold.

### Must Not
- Do not inspect copy import or log live credentials or claim subscription authentication or runtime readiness.
- Do not accept browser account identity arbitrary home paths keys endpoints or environment overrides.
- Do not repair overwrite unlink rearm or automatically retry uncertain preparation under a new identity.
- Do not release original IO or process custody merely because a caller or receipt was lost.
- Do not change Part B browser surfaces add crate versions or perform live service or production cutover.

## Boundaries

### Allowed Changes
- ./knowledge/decisions/adr-114-native-managed-account-binding.md
- ./native/README.md
- ./docs/agent-knowledge.md
- ./docs/progress.md
- ./specs/task-rust-native-managed-account-binding.spec.md
- ./Cargo.lock
- ./native/hagency-store/Cargo.toml
- ./native/hagency-platform/src/lib.rs
- ./native/hagency-platform/src/directory_identity.rs
- ./native/hagency-store/src/lib.rs
- ./native/hagency-store/src/domain.rs
- ./native/hagency-store/src/domain_worker.rs
- ./native/hagency-store/src/domain/accounts.rs
- ./native/hagency-store/src/domain/execution.rs
- ./native/hagency-store/src/domain/owned_dispatch.rs
- ./native/hagency-store/src/domain/resource_configuration.rs
- ./native/hagency-store/src/domain/resource_publication.rs
- ./native/hagency-store/src/migrations/023-managed-accounts.sql
- ./native/hagency-store/tests/accounts.rs
- ./native/hagency-store/tests/schema_fixtures.rs
- ./native/hagency-store/tests/approvals.rs
- ./native/hagency-store/tests/verified_ingress/notice_custody.rs
- ./native/hagency-store/tests/verified_ingress/attachments.rs
- ./native/hagency-store/tests/replies.rs
- ./native/hagency-store/tests/workflows/mod.rs
- ./native/hagency-store/tests/workflows/custody.rs
- ./native/hagency-store/tests/conversations.rs
- ./native/hagency-store/tests/usage.rs
- ./native/hagency-store/tests/file_delivery.rs
- ./native/hagency-store/tests/file_uploads.rs
- ./native/hagency-store/tests/owned_completion.rs
- ./native/hagency-store/tests/received_files.rs
- ./native/hagency-store/tests/fixtures/account-identity.json
- ./native/scripts/account-vectors.mjs
- ./native/hagency-execution/src/host.rs
- ./native/hagency-execution/src/operation.rs
- ./native/hagency-execution/tests/owned.rs
- ./native/hagency-execution/tests/owned/accounts.rs
- ./native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- ./native/hagency/src/bootstrap.rs
- ./native/hagency/src/bootstrap/config.rs
- ./native/hagency/src/bootstrap/driver.rs
- ./native/hagency/src/bootstrap/accounts.rs
- ./native/hagency/src/main.rs
- ./native/hagency/tests/cli.rs
- ./native/hagency/tests/bootstrap.rs
- ./native/hagency/tests/bootstrap/fixture.rs
- ./native/hagency/tests/bootstrap/accounts.rs
- ./native/hagency/tests/fixtures/owned_mcp_peer.rs
- ./.github/workflows/rust.yml

## Acceptance Criteria

Scenario: Native namespace identity remains exact
  Test: native_account_identity
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given retained physical namespace identities and pinned keyed vectors
  When aliases distinct roots missing keys and profile changes are observed
  Then exact native identity is preserved without unkeyed fallback or provider authentication claims

Scenario: Preparation retains original filesystem outcomes
  Test: native_account_preparation
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given an original private native state owner
  When preparation encounters collisions IO failure caller loss and uncertain commit
  Then original created namespace custody and preparation identity remain without repair deletion or new-ID retry

Scenario: Resources retain their managed association
  Test: native_account_association
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given an active original managed account handle
  When first creation ADR111 cloning editing and raw JSON bypass are attempted
  Then exact preset account generation and seat associations stay immutable and quota remains unknown

Scenario: Retirement fences account publication and work
  Test: native_account_retirement
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given managed resources and original commitments
  When the account retires before or after an original command
  Then new enrollment publication and launch refuse without refunding commitments or claiming uncertain cleanup

Scenario: Schema twenty two retains its original records
  Test: native_account_schema22
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given actual native schema twenty two records
  When schema twenty three is applied and reopened
  Then old resources seats declarations and commitments retain their exact meanings

Scenario: The real Host consumes only its selected namespace
  Test: native_account_host_consumer
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given two actual private namespaces and hostile ambient environment sentinels
  When a real isolated runtime probe launches through the managed Host
  Then it observes only the matching retained namespace and mismatched or legacy host paths refuse

Scenario: Managed owned authority stays current after waiting
  Test: native_account_owned_current
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given an original managed account and owned dispatch
  When SQLite waiting expiry retirement replacement and caller loss occur
  Then original association checks fence stale work at actual admission boundaries

Scenario: Offline account commands preserve native ownership
  Test: native_account_cli
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given actual native executable initialization and private temporary state
  When account preparation inspection retirement and concurrent owner commands execute
  Then exact safe facts persist across restart and busy never starts a second writer

Scenario: Bootstrap uses the actual managed driver association
  Test: native_account_bootstrap
  Level: integration
  Test Double: actual isolated files SQLite native processes and original handles
  Given the actual native executable and isolated native runtime fixture
  When its fixed development profile selects an original managed binding
  Then actual launch consumes that exact binding with empty runtime PATH and no live authentication

## Decisions

The parent-approved dependency graph amendment adds the existing platform path edge
and pinned sha2 0.10.9 and hmac 0.12.1 edges and promotes pinned cap-std and
cap-fs-ext 4.0.3 to all platforms in hagency-store. No crate version changes.
The full directory identity primitive is an observation and not portable authority.
A private fixed namespace is still subject to the existing trusted ancestor
provisioning requirement and does not isolate malicious same-UID mutation.
Any package selector adapter is disclosed with source hash and actual argv.
Cargo remains held while the host is memory pressured until parent release.

## Out of Scope

Browser enrollment effective provider auth qualification login API-key enrollment
external-home adoption credential rotation full M7 and production migration.
