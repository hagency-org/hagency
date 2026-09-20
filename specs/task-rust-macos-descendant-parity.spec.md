spec: task
name: "Port the working TS descendant-stop path to native macOS"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, platform, only-macos]
---

## Intent

Unblock the existing local Codex message-to-reply path by porting the behavior of
router/src/runner-guardian.ts and owned-process-tree.ts, using native lifetime
identities and their existing process regression cases. Do not substitute leader
exit for descendant observation or claim kernel crash containment.

## Constraints

### Must
- Start the actual executable suspended, bind its native identity and baseline
  census before allowing user code to run, without a shell or extra inherited fd.
- Track descendant ancestry continuously and retain it after reparenting.
- Signal only original lifetime identities; PID metadata alone grants no authority.
- Stop and inspect detached descendants while preserving unrelated processes.
- Keep lost inspection, unclassified new ancestry, capacity and deadline outcomes
  explicitly unknown. No fresh census repairs an earlier tracking gap.
- Start the leader in its own session. When ancestry stalls, classify a newcomer
  only by a process it shares that same census with, in its session and then in
  its group; a scope with no classified member, with conflicting members, or
  that the kernel refused to name classifies nothing.
- Unrelated process churn on the host must not end any guardian's observation,
  including churn that detaches into its own process group.
- When ancestry, session and group evidence all stall, classify a newcomer as
  unrelated only when its resource and jetsam coalitions both differ from the
  leader's. Coalition evidence is negative only: it never classifies a process
  as owned, and a zero or unreadable coalition on either side classifies nothing.
- Bound the remembered birth map so a guardian's lifetime does not depend on the
  host's process creation rate, and never forget an owned birth.
- Report a fixed stop cause and refusal category to the host and to the operator
  status, beside the pinned cleanup words and never in place of them.
- Preserve Linux subreaper and Windows Job behavior and existing bounded waits.
- Run actual local native process fixtures and then the local runtime path.

### Must Not
- Do not contact models/Palpo from ordinary tests, change sandbox defaults,
  claim a task completed from process exit, or hide existing qualification failures.
- Do not treat unsupported kqueue NOTE_TRACK as a working native facility.
- Do not classify a newcomer no evidence reaches; that refusal stays fatal and
  sticky, and no later census repairs it. A newcomer in the leader's own
  coalition with no ancestry, session or group evidence is such a newcomer.

## Boundaries

### Allowed Changes
- native/hagency-platform/src/lib.rs
- native/hagency-platform/src/supervisor.rs
- native/hagency-platform/src/supervisor/windows.rs
- native/hagency/src/bootstrap.rs
- native/hagency-platform/src/unix.rs
- native/hagency-platform/src/supervisor/unix.rs
- native/hagency-platform/src/supervisor/unix/macos.rs
- native/hagency-platform/src/supervisor/unix/macos/**
- native/hagency-platform/src/bin/hagency-platform-probe.rs
- native/hagency-platform/tests/guardian.rs
- native/hagency-platform/tests/descendants.rs
- native/**/tests/**
- native/hagency-runtime/src/bin/claude_probe/**
- native/hagency-runtime/src/codex/session.rs
- native/hagency-runtime/src/codex/session/state.rs
- native/hagency-runtime/src/codex/session/driver.rs
- native/hagency-runtime/src/codex/session/task_mcp.rs
- native/hagency-runtime/src/codex/session/hooks.rs
- knowledge/decisions/adr-036-native-codex-session.md
- native/hagency-execution/qualification/source_digests.rs
- native/hagency-execution/qualification/codex-sandbox-0.154.0.json
- knowledge/decisions/adr-029-native-process-scopes.md
- specs/task-rust-macos-descendant-parity.spec.md
- docs/progress.md
- docs/agent-knowledge.md
- docs/design/native-execution-parity.md

## Acceptance Criteria

Scenario: Native tracking preserves ancestry and refuses missing evidence
  Test: native_macos_descendant_tracking
  Given baseline foreign processes original root and later descendant observations
  When parents exit or identities change or a census loses ancestry
  Then only proven descendants are owned and uncertainty remains sticky

Scenario: Native launch establishes identity before work and seals descriptors
  Test: native_guardian_start_stop
  Given explicit Unicode argv cwd and inherited foreign descriptors
  When the original guardian starts and stops work
  Then exact arguments arrive with no guardian channel and unrelated work survives

Scenario: Detached and reparented tools stop with their original owner
  Test: native_macos_descendant_stop
  Given the TS still-parented and already-reparented descendant cases
  When the native owner cancels its process tree
  Then all owned progress stops and the unrelated process remains alive

Scenario: Missing ancestry cannot produce a positive stop receipt
  Test: native_macos_descendant_unknown
  Given a deliberately lost discovery observation
  When original known processes are stopped
  Then leader exit remains separate from unproven descendant cleanup

Scenario: Group membership classifies a newcomer whose parent no census saw
  Test: native_macos_group_evidence_classifies_unseen_parent
  Given newcomers with unseen parents in a foreign group, the owned group and an unknown group
  When the tracker updates from one census
  Then the first is unrelated, the second is owned and live, and the third still refuses
  And evidence absent from that census or conflicting within a group classifies nothing

Scenario: Unrelated churn does not cost an idle owner its observation or stop proof
  Test: native_macos_unrelated_churn_keeps_descendant_proof
  Given an idle supervised leader observed every 100 ms
  When unrelated processes keep surviving parents that exit before any census
  Then every observation stays positive and the stop receipt proves the whole tree

Scenario: An owned survivor of an unseen parent is stopped with its tree
  Test: native_macos_unseen_parent_in_owned_group_is_stopped
  Given a leader whose subshell forks a survivor and exits at once
  When the owner observes and then stops the tree
  Then observation stays positive, the receipt proves the whole tree and the survivor is gone

Scenario: Session evidence classifies a survivor left alone in its process group
  Test: native_macos_session_evidence_classifies_detached_group
  Given newcomers with unseen parents alone in a group, inside the leader's session and inside a session of their own
  When the tracker updates from one census
  Then the first is unrelated, the second is owned and live, and the third still refuses
  And a session the kernel refused to name classifies nothing

Scenario: Detached unrelated churn does not cost an idle owner its observation
  Test: native_macos_detached_unrelated_churn_keeps_descendant_proof
  Given an idle supervised leader and a subshell that leads its own process group
  When that subshell forks a survivor and exits before any census sees it
  Then every observation stays positive and the stop receipt proves the whole tree

Scenario: The same churn without a new process group isolates the evidence
  Test: native_macos_attached_unrelated_churn_control
  Given the identical survivor shape with no process group change
  When the owner observes and then stops the tree
  Then the receipt proves the whole tree, so only the changed scope differs

Scenario: One unrelated orphan stops no guardian on the host
  Test: native_macos_one_detached_foreign_orphan_stops_no_guardian
  Given two supervised leaders observed beside each other under one host
  When a single unrelated subshell leaves a detached survivor behind
  Then neither owner loses its observation and both receipts prove their whole tree

Scenario: A survivor in a session of its own still refuses and names its cause
  Test: native_macos_unseen_session_orphan_refuses_and_names_its_cause
  Given an unrelated middle that opens its own session and exits before its survivor is born
  When the owner observes the census that first contains that survivor
  Then observation ends with ObservationFailure and detail AncestryUnconfirmed
  And the leader is reaped, every signal is accepted, and the whole-tree receipt stays false

Scenario: A daemon started elsewhere through an unseen parent does not stop a guardian
  Test: native_macos_coalition_evidence_is_negative_only
  Given a newcomer alone in a session of its own whose parent no census saw
  When its resource and jetsam coalitions both differ from the leader's
  Then it is unrelated, observation continues and nothing becomes owned
  And the same newcomer in the leader's coalition, with an unreadable coalition, with only one id differing, or under a leader whose coalition is unknown still refuses

Scenario: The platform facts coalition evidence rests on still hold
  Test: native_macos_coalition_survives_setsid_and_differs_from_launchd
  Given this process, a child that opens its own session, and launchd
  When their coalitions are read
  Then the child keeps this process's coalition and launchd's differs in both ids

Scenario: Remembered births stay bounded without losing needed ancestry
  Test: native_macos_known_births_are_pruned_without_losing_ancestry
  Given unrelated churn far past the prune threshold under one tracker
  When repeated censuses replace the whole unrelated population
  Then the map stays bounded, an owned birth is never forgotten
  And a parent seen only in the previous census still classifies the orphan it left

Scenario: The host records why a guardian stopped observing
  Test: native_bootstrap_runtime_observation_projection
  Given an observed cleanup report carrying a stop cause and a refusal category
  When the operator status is projected
  Then stop_cause names a fixed category beside the unchanged cleanup word
  And the bounded operator projection still fits its existing limit

Scenario: Local operator configuration cannot widen the requested thread policy
  Test: native_codex_session_settings
  Given provider-owned login and existing operator sandbox settings
  When the native host opens or resumes the same scoped thread
  Then it explicitly disables workspace network access and extra writable roots
  And reported policy widening still fails before a prompt is sent

Scenario: Local Codex hook notices preserve their exact thread and turn
  Test: native_codex_session_hook_notices
  Given the installed 0.154.0 synchronous hook notification schema
  When notices race a thread or turn response or contain substituted scope
  Then bounded matching notices are diagnostic only and foreign scope fails

## Decisions

Extend ADR029 with the TS behavior baseline and native macOS implementation.
Process observations are not a filesystem/network sandbox or a guarantee against
guardian death. The real local Palpo/Robrix run remains required after this link.

2026-09-18: ADR029's session-scoped group evidence amendment. The live two-agent
fleet lost a warm Codex runtime's authority before Started because any process on
the host with an unseen parent stopped the owned tree. The refusal now applies
only to a newcomer that neither ancestry nor its process group classifies.

2026-09-19: ADR029's session evidence amendment. The 2026-09-18 amendment framed
the residual case as an owned descendant; the identical unrelated row is equally
unclassifiable and far more common, and three live occurrences were exactly that,
stopping both agents' trees 2-6 ms apart from one host process exit. Session
evidence is tried before group evidence, the remembered birth map is pruned, and
the guardian's stop cause now reaches the operator status. The session-of-its-own
case stays fatal; kqueue NOTE_TRACK remains the only complete answer and remains
unused.

2026-09-20: ADR029's coalition evidence amendment. The first soak on the amended
tracker died in round one of `observation_failure:ancestry_unconfirmed`: an hourly
launchd updater starts daemons through a middle that opens its own session and is
gone before any census. Operator decision: add coalition evidence, negative only.
A process in the service's own coalition with no other evidence still refuses.

