spec: task
name: "Activate delegated native tasks through durable thread receipts"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, tasks, delegation]
---

## Intent

Connect canonical task metadata, admitted inputs, thread activation and delegation.
Persist task intent before sending its acknowledgement and permit subsequent human
follow-up without reusing a completed task's authorization epoch.

## Constraints

### Must
- Persist task metadata input bindings intent identity and acknowledgement outbox in one domain transaction.
- Keep task execution inactive until a current content-bound transport receipt activates its thread.
- Use stable transaction identifiers and fenced expiring outbox claims; failed writes and conflicting receipts must not activate tasks.
- Scope delegation to the current started creator capability its admitted inputs and the same project; an explicit parent must be the current task.
- Maintain independent child input processing and never complete a parent from its child's result.
- Attach inputs idempotently and reopen completed tasks only when a fresh original-human thread input is actually started under current authority.
- Preserve task identity and advance authorization epoch on follow-up; keep stale capabilities invalid.

### Must Not
- Do not accept a runtime-supplied owner sender or transport receipt as authority.
- Do not wake completed tasks for another human peer traffic processed input or a different thread.
- Do not launch native processes or send live Matrix messages in tests.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-task-intents.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- docs/**

### Forbidden
- Live state, credentials, deployed JS runtime and Matrix server changes.

## Acceptance Criteria

Scenario: Intent and acknowledgement remain atomic
  Test: native_task_intent_activation
  Given a task definition admitted input and an injected outbox write failure
  When the intent is created and its acknowledgement is delivered
  Then metadata input and intent commit together and only the exact receipt activates execution

Scenario: Transport claims are fenced across restart
  Test: native_task_outbox_recovery
  Given claimed failed delivered or stale acknowledgement attempts
  When claims expire or the owner restarts
  Then retries retain transaction identity and stale or conflicting receipts cannot activate a task

Scenario: Delegation is bound to creator scope
  Test: native_task_delegation_scope
  Given a started creator and tasks on the same or another project
  When delegated tasks and input attachments are requested
  Then only admitted creator inputs and the current parent authorize delegation and child completion leaves the parent unchanged

Scenario: Human follow-up reopens only at execution
  Test: native_task_human_followup
  Given a completed task with fresh stale foreign or mismatched follow-up input
  When fresh work is queued claimed and started
  Then only the original human's unprocessed thread input reopens that task under a new epoch

Scenario: Private API delegates without transport authority
  Test: native_runner_http_delegation
  Given a current started runner and a bounded delegation request
  When the private runner API handles delegation and retries
  Then creator scope and idempotency are enforced while forged delivery and owner assertions are rejected

## Out of Scope

Graph dependency scheduling, final reply delivery, real Matrix provenance, DM
promotion policy and actual runtime processes remain subsequent migration work.
