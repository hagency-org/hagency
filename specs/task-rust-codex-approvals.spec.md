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
- Decline an unanswered callback at its owner bound only after recording a durable host-minted deny, only with that family's own decline, never an accept, and only when the host opted the session in; a failed approval channel still ends the operation.
- Refuse no pinned approval request by its shape, as the retained runner does: every well-formed command, file-change and permissions request reaches the owner, who sees the whole request, and is answered with nothing but that family's exact accept or decline. A request the core cannot turn into an exact reusable rule is offered once or deny only. A malformed request is a protocol fault and still ends the session.
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

Scenario: No approval is refused by its shape
  Test: native_codex_approval_shapes_all_reach_the_owner
  Given an opted session and command, file-change and permissions requests with an unusual kind, a decision menu without accept or without decline, a grant root, or an unusual permission profile
  When each arrives
  Then it reaches the host coordinator as an ordinary approval, nothing is answered on the owner's behalf, and the turn goes on
  And a malformed request still ends the session

Scenario: Whatever was offered or proposed, the answer is an exact accept or decline
  Test: native_codex_approval_mapping
  Given pinned requests carrying alternate decision menus, policy amendments, a network context, a grant root or an unusual permission profile
  When each is parsed and answered
  Then a command or file change is answered accept or decline and never a session-wide or amended decision
  And a permission profile is granted as asked for this turn only, or not at all

Scenario: An unanswered owner wait is declined by the host and the turn goes on
  Test: native_owned_approval_owner_wait_expiry_declines_and_continues
  Given an owned approval callback the owner never answers
  When its owner wait runs out inside the request's own expiry
  Then the expiry is recorded as a durable host-minted deny before one decline frame is written
  And the operation completes instead of ending outcome-unknown

Scenario: The command family declines at expiry with its own frame
  Test: native_owned_approval_owner_wait_expiry_command_family
  Given an unanswered command approval callback
  When its owner wait runs out
  Then the family's own decline is written and the turn goes on

Scenario: An allow first seen after the owner wait is still refused
  Test: native_owned_approval_expired_allow_is_still_refused
  Given a callback the host has expired
  When an owner allow is observed afterwards
  Then no accept is prepared or written and the late verdict is refused

Scenario: An unknown expiry denial sends nothing
  Test: native_owned_approval_expiry_deny_uncertain_sends_nothing
  Given the durable expiry denial whose outcome is lost
  When the coordinator continues
  Then no response byte is written and the operation ends without authority

Scenario: An expired callback may only be declined and the opt-in only moves a read bound
  Test: native_codex_control_expired_callback_may_only_decline
  Given sessions with and without the host's owner-wait expiry opt-in
  When a wait spans the owner bound and an accept or a decline is prepared afterwards
  Then without the opt-in the session times out at the owner bound as before
  And with it the session stays readable, an accept is refused by policy and only a decline is prepared inside the response bound

Scenario: The expiry denial is recorded at most once
  Test: native_owner_approval_owner_wait_expiry_is_at_most_once
  Given a pending owner approval past its owner cutoff
  When the expiry denial is recorded, replayed, raced with an owner verdict and followed by a stale card tap
  Then the replay is idempotent, an already decided row and a premature cutoff are refused, and the stale tap is refused

Scenario: A whole composed service declines an approval its owner never answers
  Test: native_fleet_approval_owner_wait_expiry
  Given the real service composition with the approval bot's own credentials and a card delivered to the owner's encrypted DM
  When the owner sends nothing before the owner wait ends
  Then the runner receives the family's own decline inside the response reserve
  And the approval is durably denied with the host's expiry reason and the owner sent nothing

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
