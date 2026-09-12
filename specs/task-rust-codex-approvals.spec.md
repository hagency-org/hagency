spec: task
name: "Connect typed Codex approvals to durable host authority"
inherits: project
satisfies: [REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, approval]
---

## Intent

Provide an opt-in offline Codex approval coordinator with exact request-specific
responses and durable Applying before bytes, while preserving uncertainty when
upstream provides no proof of effective permission application.

## Constraints

### Must
- Retain the default unsupported approval path unless the host attaches the coordinator.
- Bind parsed requests and responses to exact connection request ID thread turn item and original payload.
- Derive host context from the validated session and current repository capability and workspace lease.
- Use the existing core and store authorization implementation without another policy engine.
- Consume the durable decision before sending a typed allow or deny response and never send it twice.
- Treat upstream resolution cancellation EOF timeout and write completion as insufficient proof of effective application.
- Keep pending data bounded and reject malformed unsupported or substituted requests without granting authority.
- Cover real durable store admission and fake-stream protocol exchanges without live models.

### Must Not
- Do not deserialize host identity workspace ownership approval authority or verdicts from runtime input.
- Do not emit session-wide approval policy amendments or YOLO.
- Do not synthesize Applied from flush resolution notifications model text or item completion.
- Do not change platform launchers OwnedSession or live transport cutover.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/codex*
- native/hagency-runtime/src/codex/**
- native/hagency-runtime/tests/**
- native/hagency-permissions/**
- ./Cargo.toml
- ./Cargo.lock
- specs/task-rust-codex-approvals.spec.md
- knowledge/decisions/adr-046-codex-approval-adapter.md
- docs/**

### Forbidden
- Platform and owned process implementations or independent schema14 work.
- Credentials deployed services live models and original dirty checkouts.

## Acceptance Criteria

Scenario: Only bounded exact Codex approval requests admit typed responses
  Test: native_codex_approval_mapping
  Given pinned Codex command file and permission request schemas
  When malformed unsupported duplicate and substituted identities arrive
  Then only request-specific once or deny shapes are emitted with exact correlation

Scenario: Default sessions refuse approval and opted sessions preserve exact scope
  Test: native_codex_approval_session
  Given initialized scoped sessions and fake transport streams
  When request and resolution messages arrive
  Then opt-in is explicit and stale or duplicate request responses fail closed

Scenario: Durable authority precedes every native response byte
  Test: native_codex_approval_coordinator
  Given current verified owner authority canonical tasks and exclusive workspace custody
  When requests are admitted and owner choices consumed
  Then the dispatch parks and Applying persists before a correlated typed response can be written

Scenario: Missing application evidence never creates success or replay authority
  Test: native_codex_approval_uncertainty
  Given consumed approvals and interrupted or resolved protocol requests
  When writes fail futures cancel responses resolve or the store restarts
  Then uncertainty remains durable and neither duplicate application nor dispatch resume is permitted

## Out of Scope

No Matrix cards live models or native runner cutover. Codex 0.153.4 emits
serverRequest/resolved before response parsing or core operation submission and
also on cancellation, so effective application remains a separate inspection gate.
This slice deliberately does not unblock canonical dispatch from protocol evidence.
Taskless paths multi-environment runners session policy changes and arbitrary
filesystem patterns remain unsupported.
