spec: task
name: "Materialize the original inline agent home before Matrix effects"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, provisioning, workspace, custody]
---

## Objective

Port the retained v1 home/manifest/entry documents/project mappings/supervisor
workspace into the original inline ordinary-account factory. Bind actual homes
to its original Started effect and resource, before registration/rooms/SDK.
This is a physical prerequisite for the full runtime and genuine completion/
routes, not a substitute for either deployment profile or full M0–M9 gates.

## Constraints

- Derive agent identity/name/owner/project only from the original validated
  request; select framework/model/provider/reasoning from its actual resource.
- Only Host configuration selects existing private home root, canonical project
  roots and native task-client binary. Retain directory objects and detect
  replacement/alias/nesting; never import credentials from workdir or docs.
  The recorded home binding names the binary's path, not its length or mtime
  (those are checked in-process only), so a home reopens after an in-place
  upgrade (operator decision 2026-09-22, task rust-factory-agent-reattach).
- Both explicit copy and symlink project modes remain supported. Bound copies
  by total bytes/entries/depth/per-file, preserve safe in-tree relative links,
  and refuse external links/special files rather than following them.
- Reserve original create-only possible custody before home creation. Partial/
  existing/unknown homes are never overwritten, repaired or resumed. Completed
  original inspection is read-only; changed scope/configuration must refuse.
- One finite retained job owns physical work after caller loss. Non-cancellable
  filesystem work keeps its ownership through actual return; checkpoints bound
  calls and never promote an expired result to readiness.
- Use retained v1 templates, adapting canonical-task references to native scoped
  task/MCP authority. Manifest task is null; do not create another canonical task
  or runtime-profile truth file. Native task wrapper has no Node dependency or
  secret/runner capability in its bytes and cannot mint assignment authority.
- Wire the stage before original account registration with a separate explicit
  registration_token_home_rooms_enrollment_step_v1 marker. Existing profiles
  remain unchanged. No second launcher, Applied/Active receipt or fabricated
  runtime/session route is introduced by a successful home.
- Cargo tests stay offline/local TLS/private temporary filesystem only. No
  formatter, new dependency, deployment, reset, commit or PR in this slice.

## Allowed changes

- native/hagency-store/src/agent_home.rs
- native/hagency-store/src/lib.rs
- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/tests/intake/provisioning.rs
- native/hagency-matrix/tests/provision_rooms/mod.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/bootstrap.rs
- specs/task-rust-inline-agent-home.spec.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- docs/agent-knowledge.md
- docs/progress.md

## Scenarios

Scenario: Original inline factory materializes home before account and rooms
  Test: native_provisioning_inline_home
  Level: integration
  Test Double: real original Collector/writer/files and local TLS recipient SDK
  Given a fixed project mapping and original provider approval
  When its inline factory creates a home and then registers joins and enrolls
  Then actual v1 artifacts and selected resource profile belong to that original engagement, with no Active/route fabricated

Scenario: Both project modes preserve ownership and safe link behavior
  Test: native_provisioning_inline_home_projects
  Given explicit copy and symlink modes with nested ordinary files and safe links
  When actual home materialization runs
  Then copy writes stay in the managed copy and symlink writes affect only the declared original project

Scenario: Partial homes unsafe sources and changed scope refuse before registration
  Test: native_provisioning_inline_home_refusals
  Given a partial/conflicting home, missing mapping, replaced source, unsafe link or exceeded bound
  When the original factory reaches physical setup
  Then no account/room/SDK network write occurs and unknown custody cannot rearm

Scenario: Native configuration exposes only the explicit closed home profile
  Test: native_bootstrap_token_home_profile
  Given the home marker and fixed non-secret paths with separate protected Matrix credentials
  When actual executable configuration prepares
  Then only the valid fixed plan attaches and defaults/account/rooms-only remain unchanged

Scenario: Original physical copy stays owned after the outer caller drops
  Test: native_provisioning_inline_home_custody
  Given an actual admitted original possible home with an unfinished bounded copy
  When the outer intake waiter is aborted
  Then the same retained original job finishes its home/account/rooms/SDK without restarting physical work

Scenario: Revocation during the physical copy prevents subsequent account effects
  Test: native_provisioning_inline_home_scope_change
  Given the original possible home exists and its bounded physical copy is unfinished
  When the real writer revokes that original engagement before the copy returns
  Then the physical result remains retained but no account/room/SDK write or Active/session route is published, including on replay
