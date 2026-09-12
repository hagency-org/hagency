spec: task
name: "Separate native approval router authorization from response and application evidence"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, approvals, authority, schema]
---

## Intent

Implement the root-approved ADR043/046 amendment as a domain-only prerequisite.
The root reviewed the retained router decision-then-resume-then-write behavior and
approved separating exact durable router authorization, one-shot response
admission/transmission and independent unconfirmed native application. This work
starts from clean 99200b0. Runtime control pumping and execution integration are
separate owners and remain disabled here.

## Constraints

### Must
- Schema22 adds a separate response ledger without promoting old Applying Uncertain or Applied rows into new router authority.
- First exact atomic authorization and consumption creates one opaque non-Clone non-serde response grant bound to the original repository instance capability private context and application descriptor.
- Keep the grant in its caller owner across begin awaits and mark its single begin attempt before enqueue; only a positive acknowledgement admits response sending.
- Fresh post-lock checks validate every current approval barrier and full owner scope expiry task epoch grant and lease before atomically marking response possible and resuming only the same retained attempt.
- Genuine parked state must refuse task mutation; a valid owner deny may authorize baseline continuation after all current barriers resolve.
- Transmission observations and native application observations are independent evidence; neither an unknown transmission nor a later native Applied observation can reauthorize execution.
- Preserve original grants across caller loss and prevent reconstruction after unknown consumption or response admission; restart and historical rows never permit sending or resume.
- Update existing schema-version assertions and shared historical teardown only as required by schema22, preserving their original checks.

### Must Not
- No runtime coordinator pump host controller timeout sandbox Cargo dependency or live-service changes.
- No generic unpark bypass, hidden writer, native application claim from flush or resolution, manual proof setter, or response retry after unknown outcome.

## Boundaries

### Allowed Changes
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain/approvals/responses.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/migrations/022-approval-responses.sql
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/approvals/responses.rs
- native/hagency-store/tests/approvals/responses_worker.rs
- native/hagency-store/tests/common/mod.rs
- native/hagency-store/tests/received_files.rs
- native/hagency-store/tests/owned_completion.rs
- native/hagency-store/tests/usage.rs
- native/hagency-store/tests/conversations.rs
- native/hagency-store/tests/file_delivery.rs
- native/hagency-store/tests/file_uploads.rs
- native/hagency-store/tests/verified_ingress/attachments.rs
- native/hagency-store/tests/verified_ingress/notice_custody.rs
- native/hagency-store/tests/workflows/custody.rs
- native/hagency-store/tests/workflows/mod.rs
- native/hagency-store/tests/replies.rs
- specs/task-rust-approval-router-authority.spec.md
- knowledge/decisions/adr-043-native-owner-approvals.md
- knowledge/decisions/adr-046-codex-approval-adapter.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- No runtime permissions execution HTTP MCP Matrix schema other than the original domain schema22 or operational cutover changes.

## Acceptance Criteria

Rule: exact-router-decision — Only original exact router authority admits a response

Scenario: Original grants admit allow and deny after every current barrier resolves
  Test: native_approval_router_authority
  Given actual current bound approvals and original one-use grants
  When exact allow and deny decisions enter response admission
  Then the same attempt resumes only after every barrier is authorized and parked task mutation refuses

Scenario: Invalid changed or expired authority cannot admit response sending
  Test: native_approval_response_scope
  Given original grants and current owner task lease and reusable grant scope
  When authority changes or expires before response admission
  Then no response is admitted and no generic unpark bypass succeeds

Scenario: Native application evidence remains independent of router authority
  Test: native_approval_response_observations
  Given admitted responses and uncertain or historical application records
  When transmission and native application observations arrive
  Then only their exact evidence changes and later Applied cannot resume parked expired or retired execution

Scenario: Schema21 history and restart never manufacture response ownership
  Test: native_approval_response_recovery
  Given actual schema21 historical approval states and fresh schema22 response attempts
  When upgrade or repository reopen occurs
  Then history remains readable but original response grants cannot be reconstructed or reused

Scenario: Caller loss retains a consumed original grant without repeat admission
  Test: native_approval_response_caller_loss
  Given the original bounded writer and retained response grants
  When consumption or response admission acknowledgement is lost
  Then original attempt state remains distinguishable and no second response authority is returned

Scenario: Response admission uses the original post-lock clock and deadline
  Test: native_approval_response_clock
  Given the physical original SQLite transaction and bounded writer queue
  When contention crosses scope expiry or the original response deadline
  Then response admission refuses without fresh deadlines or new authority

Scenario: A lost acceptance reply leaves the row unwritten when the abandoned job is skipped
  Test: native_domain_acceptance_reply_timeout_reconciles
  Given the acceptance reply bound expires while the single writer cannot run
  When the ordered reconcile read answers after the caller stopped waiting
  Then the abandoned acceptance job is skipped and no accepted row is manufactured

Scenario: The reconcile continues on a record the reply-loss seam hid
  Test: native_owned_approval_acceptance_reconcile_accepted
  Given a written response frame whose acceptance call succeeded but whose reply the seam converted to an unknown
  When the single ordered read reports the committed accepted row
  Then the operation continues on the successful path with one frame and one accepted row

Scenario: The reconcile reports a conclusive absence of the record
  Test: native_owned_approval_acceptance_reconcile_unrecorded
  Given a written response frame whose acceptance write is refused by a held writer
  When the single ordered read answers that no accepted row exists
  Then SettlementUnknown carries the AcceptanceUnrecorded cause with no re-send and no record

## Out of Scope

Owned runtime integration, parked maintenance, protocol control pumping, owner TTL
configuration, actual native response transmission and executable interactive
qualification remain separate accepted partitions. Domain tests prove none of
those effects and do not mark the migration complete.
