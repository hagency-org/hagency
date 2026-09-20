---
kind: requirement
id: REQ-THREAD-SCOPED-SESSIONS
title: "Isolate coding agent conversations per Matrix thread"
status: Accepted
liveness: auto
tags: [sessions, matrix, threads, runner, scheduler, approval, runtime]
---

## Problem

One agent runs one long-lived coding runtime conversation, so messages from
different Matrix threads interleave in a single LLM context: constraints
leak between threads, replies target the wrong thread, and one long task
blocks every other topic. The first attempt (ADR-010, prototyped on
`wip/thread-sessions-rewrite`) anchored isolation on per-thread tmux
windows and was rejected: the window is not a controllable truth source —
post-exit shells execute injected text, hibernation races draft input,
resume ids escape capture, and reply anchors depend on model compliance.
Isolation must be owned by the backend, with runtime processes reduced to
disposable workers.

## Requirements

[REQ-TSS-BACKEND-OWNER] Topic-session identity, routing, and isolation MUST derive solely from backend-owned durable state identified by `(stable agent id, room id, scope kind, thread root event id)` with an opaque session id; a main session MUST have no thread root, a thread session MUST have an authenticated Matrix root, and no mutable agent name, delimiter-concatenated key, terminal, window, or runtime-process state may be authoritative.

[REQ-TSS-RUNNER] Each dispatched message MUST execute in a fresh one-shot headless runner whose context is rebuilt from durable artifacts, and message content MUST reach the runner as data — never interpolated into a shell command line or argv.

[REQ-TSS-TASK-CREDENTIAL] Work MUST reach the coding execution layer only through an existing task id whose execution binding has an authenticated thread root and durably confirmed Matrix anchor; a dispatch lacking that active binding MUST be refused, and an agent MUST NOT be able to assign work to a worker through an ordinary message instead of the structured task-creation tool.

[REQ-TSS-TASK-ROOT] A task's thread root MUST be the originating human message's authenticated top-level Matrix event and MUST be stored separately from the agent acknowledgement event returned by Matrix. When the source message is already a thread member, the backend MUST derive the root from its authenticated stored thread relation, retain the original source message as input, and refuse nested or substituted roots. The backend MUST NOT fabricate either event id, and a task session MUST NOT become dispatchable until an idempotent Matrix `m.thread` send rooted at that top-level event has been durably acknowledged.

[REQ-TSS-TASK-IDEMPOTENT] Task creation MUST use a caller-scoped request key plus a canonical payload digest; replaying the same key and digest MUST return the original task, reusing the key with different content MUST fail, supplementary input attachment MUST have its own idempotent operation, and a message MUST be allowed to contribute to more than one task.

[REQ-TSS-TASK-BINDING-UNIQUE] For one stable assignee, a Matrix room/thread root MUST bind to at most one task so later follow-ups resolve unambiguously; several tasks may derive from one source root only when their assignees are distinct, and a second task for the same assignee/root MUST be rejected visibly.

[REQ-TSS-TASK-ACTIVATION] Creating a task intent MUST atomically write the existing durable task record, pending execution binding, immutable task inputs, and Matrix outbox command; acknowledging that command MUST atomically record the thread anchor, activate the task session, and activate its task inputs, while permanent Matrix failure MUST leave those inputs unactivated and report a visible failure.

[REQ-TSS-TASK-SINGLE-TRUTH] Thread sessions MUST extend the existing durable task model and `/api/tasks` contract rather than create a second task authority; a versioned resumable offline-rollback-capable cutover MUST verify imported JSON task counts and canonical digests before SQLite becomes the sole task writer, runtime dual-write or writer switching MUST NOT occur, and legacy tasks without a confirmed execution binding MUST remain non-dispatchable.

[REQ-TSS-EXPLICIT-TASK] An explicit human task command MUST create a task without model judgement and MUST NOT be batched with any other message.

[REQ-TSS-WORKER-MAIN-PROMOTION] A message addressing a coding worker in a room's untreaded main timeline MUST be promoted to a task thread rooted at that message, and work MUST NEVER execute in a worker's main session.

[REQ-TSS-FRONT-DESK-CONTEXT] A main-timeline session's logical identity MUST persist indefinitely while its assembled context MUST remain bounded, rotating to a new context generation once the budget is exceeded rather than growing without limit.

[REQ-TSS-BATCHING] Batching of consecutive front-desk messages MUST be a performance optimization only: each message MUST retain its own id, order, and processing state, and a batch MUST NOT be treated as a single task.

[REQ-TSS-NO-WARM-RUNNER] V1 MUST terminate the model runtime after each dispatch and MUST NOT retain or reuse a warm model process; any later reuse requires a separate accepted contract proving unchanged isolation, approval, capability, and lease boundaries.

[REQ-TSS-ROUTER-ATOMIC] Router state MUST live in a single-writer transactional SQLite store in the backend process, and task intent with Matrix outbox, dispatch claim with capability and required resource leases, and dispatch settlement with capability revocation, lease release, and persistent workspace-dirty state MUST each commit atomically inside that store.

[REQ-TSS-CROSS-SYSTEM] Matrix sends and approval consumption MUST NOT be described as part of a SQLite transaction; each boundary MUST use a durable idempotent outbox or inbox event with a stable key and payload digest, MUST reconcile after restart, and MUST fail closed so an unrecorded Matrix confirmation cannot activate work and an unapplied approval decision cannot deliver allow to a runner.

[REQ-TSS-CLIENT-READ-ONLY] Clients MUST obtain router state only as an authorized, privacy-filtered authoritative snapshot plus a versioned event feed with transactional low/high watermarks, MUST treat events as invalidation signals and re-snapshot on a detectable gap, MUST NOT write router state directly, and MUST perform every mutation through an authenticated backend endpoint appropriate to that actor.

[REQ-TSS-CLIENT-PRIVACY] Router projections MUST NOT expose absolute local paths, secrets, message bodies unnecessary to the view, owner-DM approval content, or actionable approval controls outside their authorized owner scope; V1 router read endpoints MUST remain inside the existing bearer-authenticated local Dashboard boundary until a separate Matrix-client authentication contract is Accepted.

[REQ-TSS-REPLY-ROUTING] Reply addressing MUST be computed by the backend from the dispatch record; the runner-facing protocol MUST NOT carry a model-fillable reply-target field.

[REQ-TSS-INBOX-SCOPE] MCP reads issued by a runner, including `check_inbox`, MUST return only content belonging to the dispatching session.

[REQ-TSS-CONTEXT-ISOLATION] The context assembled for one session's runner MUST NOT contain another session's messages, summaries, or task state.

[REQ-TSS-INPUT-DURABILITY] The router MUST copy normalized immutable session input instead of retaining only references to the JSON message store, MUST ingest idempotently, and MUST reconcile persisted source messages after restart before source retention may delete them.

[REQ-TSS-RUNNER-CAPABILITY] Every runner protocol call that reads input or changes dispatch state MUST present an unguessable short-lived capability bound to runner id, dispatch id, and durable fence generation; only its hash may be stored, and settlement or fencing MUST revoke it.

[REQ-TSS-AT-MOST-ONCE] A dispatch MUST execute at most once after it starts: the backend MUST commit the started transition while reading the immutable payload for one capability-authenticated response, a runner MUST NOT act without receiving that successful response, loss of the response after commit MUST be treated as an ambiguous started dispatch, a dispatch that died before starting MAY be re-leased, and a dispatch that died after starting MUST NEVER be re-executed automatically.

[REQ-TSS-OUTCOME-UNKNOWN] `cancelled_before_start` MUST be the only clean non-completion state; any terminal state of a started dispatch other than completion — crash, lease overrun, approval expiry, or operator cancellation — MUST settle as outcome_unknown with a sanitized notice asking the operator to inspect the workspace, and persistent workspace state MUST carry a may-hold-uncommitted-changes flag independently of the released lease until an authenticated human clears it after inspection. An unanswered owner window that the host declines under REQ-TSS-APPROVAL-TIMEOUT is not a terminal state of the dispatch; approval expiry here means an approval whose own expiry passes without a recorded decision or a delivered response.

[REQ-TSS-RESTART-RECONCILE] On backend start, every dispatch left in the started state whose runner lease is expired or unverifiable MUST become outcome_unknown with the same operator notice, and in-flight work MUST NOT be silently resumed or re-executed.

[REQ-TSS-FENCE] Output from a runner whose dispatch is no longer current MUST be recorded as fenced and MUST NOT settle any dispatch, where "no longer current" is decided by both an identity match and a durable monotonic sequence that MUST survive a backend restart.

[REQ-TSS-DELIVERY-EFFECT] A started dispatch MUST receive a structured wrapper acknowledgement that the verified child process accepted the complete payload within a measured bounded interval; first model output MUST NOT be used as that evidence, and absent acknowledgement MUST settle outcome_unknown as stalled delivery rather than waiting for lease expiry.

[REQ-TSS-AUTHORITY-SEPARATION] Dispatch state MUST be moved only by the structured runner protocol; model-authored text MUST be display-only and MUST NOT be able to change dispatch state, session state, or routing.

[REQ-TSS-PRE-WRITE-VERIFY] Before any input is written to an interactive runner process, the backend MUST verify that the intended runtime still owns that process's input channel, and MUST refuse the write otherwise.

[REQ-TSS-RUNNER-LIFETIME] Each runtime process tree MUST remain owned by a guardian bound to backend liveness; graceful shutdown MUST stop ingress, terminate and await all live runners before exit, and loss of the backend process MUST terminate the guarded runtime tree rather than leave an orphan modifying a quarantined workspace.

[REQ-TSS-WORKSPACE-LEASE] A runner that may write files MUST hold its working directory's exclusive lease for its entire life including while parked on an approval, two writing runners MUST NEVER hold the same directory concurrently, and sessions holding no workspace lease MUST NOT be charged against the writer budget.

[REQ-TSS-WORKSPACE-MODE] Each agent MUST declare `workspace_mode` as `shared` or `worktree` with `shared` as the default; under `shared` all sessions contend for one directory lease, and under `worktree` each thread session MUST own a git worktree on its own branch, created on first dispatch and initialized by the agent's declared bootstrap command, so that writing topics hold independent leases.

[REQ-TSS-BOOTSTRAP-BOUNDARY] Runner workspace paths, modes, worktree roots, and bootstrap argv MUST require operator authority; worktree preparation MUST run outside the backend event loop in a helper whose environment excludes backend, Matrix, Dashboard, agent, and provider credentials, and a failed bootstrap that dirtied the checkout MUST remain quarantined even if its configured argv later changes.

[REQ-TSS-WORKTREE-LIFECYCLE] Worktrees MUST NEVER be auto-merged, auto-deleted, or discarded while dirty, and removing a worktree MUST NOT delete its branch; a session evicted while its worktree holds uncommitted changes MUST retain that worktree and report it for a human to resolve.

[REQ-TSS-NON-GIT-RESOURCES] A worktree MUST NOT be treated as isolating anything git does not track; a dispatch requiring exclusive use of a port, database, or other machine resource MUST take a separate named lease for it.

[REQ-TSS-APPROVAL-PARK] A runner blocked on an owner approval MUST stay alive and MUST keep its workspace lease; any writing work contending for that same lease MUST queue and MUST receive a visible waiting-for-approval status in its own thread, and the parked dispatch MUST be cancellable by the operator.

[REQ-TSS-PARKED-CAP] Live and parked runners MUST each have a finite host-wide cap, the parked cap MUST remain below the live cap so non-parked work retains capacity, and when the parked cap is full a new approval request MUST fail closed before entering parked state and MUST NOT release or transfer its workspace lease to admit another writer.

[REQ-TSS-APPROVAL-TIMEOUT] When the approval channel fails, the runner MUST be terminated and the dispatch MUST settle under the at-most-once rule rather than as a clean denial. When instead the owner's decision window runs out unanswered while the channel is healthy and the request's own expiry is still ahead, the host MUST record a durable host-minted denial before any response byte and MUST answer the runtime with that request family's own decline, so the turn continues and can report that approval was not granted in time; an allow first observed after the owner window MUST still be refused, a later tap on the expired card MUST be refused, and if the denial cannot be recorded or the decline cannot be delivered inside the request's expiry the first sentence applies. (Expiry half amended 2026-09-19 by operator decision, ADR-046; the channel-failure half is unchanged.)

[REQ-TSS-POST-EXIT-INERT] After a runner exits, delivering further input for its session MUST NOT execute anything: pending messages MUST remain data until a new runner is dispatched.

[REQ-TSS-COORDINATOR-DIGEST] A coordinator session's rebuilt context MUST begin with a backend-generated digest of the agent's active topics and their states, independent of the model requesting it.

[REQ-TSS-REBUILD-BUDGET] Context rebuild MUST respect a configured token budget, and an agreement stated in an earlier turn of a session MUST remain recoverable in later rebuilt turns of that session.

[REQ-TSS-APPROVAL-NEUTRAL] The runner model MUST NOT weaken REQ-OWNER-UI-APPROVAL: approval adapters remain mandatory in runners, adapter startup failure aborts the dispatch, Claude MUST use its supported MCP approval channel, and Codex MUST use App Server's server-initiated approval requests bound to the current thread, turn, item, dispatch, and operation digest; the thread-session path MUST NOT rely on `codex exec` surfacing an unattended prompt or mutate persistent hook trust per dispatch.

[REQ-TSS-APPROVAL-RECONCILE] A consumed approval MUST carry a stable decision-event id and dispatch binding, MUST be applied idempotently to the router before allow reaches the runner, and MUST remain replayable to reconciliation after a failure between the approval store write and router application without authorizing a different dispatch or operation.

[REQ-TSS-OCTOS-EXCLUDED] Octos agents MUST be rejected visibly from thread-session dispatch under this model; the incompatibility (long-lived approval-proxy chain vs disposable runners) is a scoping decision, not silent degradation.

[REQ-TSS-REMOTE-EXCLUDED] V1 thread-session dispatch MUST reject an agent registered to a remote server with `remote_runner_unsupported` and MUST NOT silently fall through to legacy tmux delivery; remote execution requires a separate accepted authenticated runner protocol.

[REQ-TSS-BUILD-CONTRACT] Router production code MUST be strict TypeScript compiled to checked-in ordinary ESM, MUST use a pinned non-experimental SQLite adapter rather than `node:sqlite`, MUST expose only `router/dist/index.js`, and CI MUST reject type errors, internal imports, prohibited type escape hatches, and stale build output.

## Scenarios

Scenario: Several front-desk requests become separately rooted tasks
  Given three consecutive main-timeline messages to a coordinator where the third supplements the first
  When the coordinator creates one task from the first and third and another from the second
  Then each task is rooted at its own originating human message
  And each task's later work, approvals, and follow-ups stay in its own thread

Scenario: One assignee cannot own two tasks on one Matrix root
  Given one task already binds an assignee to a room and thread root
  When another task is created for the same assignee and root with a different request key
  Then creation is refused visibly
  And later thread follow-ups still resolve to the original task only

Scenario: Work cannot reach a worker without a task credential
  Given a coordinator attempting to assign work to a worker through an ordinary message
  When the backend evaluates the dispatch
  Then the dispatch is refused for lacking a task id and thread root

Scenario: A task whose thread reply fails to send never becomes dispatchable
  Given a task-creation intent whose Matrix thread reply cannot be sent
  When activation is attempted
  Then the task session does not become dispatchable
  And its task inputs remain unactivated with the failure reported in the originating timeline

Scenario: Two agents never collide on one Matrix thread
  Given two agents addressed in the same room and Matrix thread
  When the backend resolves their topic sessions
  Then each stable agent id resolves to a different session id

Scenario: Lost Matrix acknowledgement does not duplicate the thread anchor
  Given Matrix accepted a task-thread acknowledgement but the bridge lost the backend acknowledgement
  When startup reconciliation replays the Matrix outbox command
  Then the same Matrix transaction id resolves to the original event
  And the task activates exactly once

Scenario: Two threads drive one agent without contamination
  Given messages in two Matrix threads of one room addressed to the same agent
  When both threads request work
  Then each dispatch runs with a context containing only its own thread's history

Scenario: Under shared mode an approval wait blocks other writing topics, visibly
  Given an agent in shared workspace mode with a dispatch parked awaiting owner approval
  When a message arrives for a different thread of the same agent that also needs to write
  Then the second dispatch stays queued and its own thread receives a waiting-for-approval status

Scenario: Under worktree mode an approval wait does not block another topic
  Given an agent in worktree workspace mode with a dispatch parked awaiting owner approval in its own worktree
  When a message arrives for a different thread of the same agent that needs to write
  Then the second dispatch runs in its own worktree while the first remains parked

Scenario: A dirty worktree survives session eviction
  Given a thread session whose worktree holds uncommitted changes
  When the session is evicted
  Then the worktree and its branch are retained
  And the uncommitted state is reported for a human to resolve

Scenario: A non-writing topic keeps flowing during an approval wait
  Given a dispatch parked awaiting owner approval and holding the workspace lease
  When a message arrives for a session that requests no workspace write
  Then that session is served while the first remains parked

Scenario: A dead runner never causes silent re-execution
  Given a runner killed mid-dispatch after producing side effects
  When its lease expires
  Then the dispatch settles as outcome_unknown with a notice asking the operator to inspect the workspace and is not re-run

Scenario: A runner that died before taking its payload is safely re-leased
  Given a dispatch leased to a runner that died before the backend recorded it as started
  When the lease expires
  Then the dispatch returns to the queue and is dispatched again without operator involvement

Scenario: A runner capability cannot cross dispatches
  Given a capability bound to runner r1, dispatch d1, and fence generation g1
  When it is presented for another dispatch or after d1 settles
  Then the request is refused and no payload or inbox content is returned

Scenario: Approval consumption survives a cross-store failure without allowing work
  Given the approval store consumed an allow decision but router application failed
  When the runner polls and the backend later restarts
  Then no allow is delivered before router application
  And reconciliation applies the same decision event only to its bound dispatch

Scenario: Remote agents fail visibly in v1
  Given an agent registered to a remote server
  When thread-session dispatch is requested
  Then it is refused with remote_runner_unsupported
  And no legacy tmux delivery is attempted

## Dependencies

- REQ-MATRIX-THREAD-CONTINUITY
- REQ-OWNER-UI-APPROVAL

## Source Trace

- decision: ADR-011
- proposal: LEP-002
- specs/task-thread-scoped-agent-sessions.spec.md
- Rejected predecessor: ADR-010 / LEP-001 (`wip/thread-sessions-rewrite` prototype)
- Pattern sources: borrow-proj/fluent (one-shot ephemeral workers, durable-artifact context rebuild), borrow-proj/buzz (per-conversation FIFO), borrow-proj/orca (dispatch fencing, lease-shaped settlement)

## Implementation Parameters and Gates

- Defaults are `AGENTCHAT_REBUILD_TOKEN_BUDGET=12000` tokens (excluding fixed
  system/project instructions and split across pinned constraints, rolling
  summary, and recent messages), `AGENTCHAT_RUNNER_LEASE_MS=1200000`,
  `AGENTCHAT_MAX_LIVE_RUNNERS=8`, and
  `AGENTCHAT_MAX_PARKED_RUNNERS=4`. All caps MUST remain finite and the parked
  cap lower than the live cap if later measurement changes them.
- Codex adapter gate: two `codex exec --json` probes on Codex 0.146.0 did not
  invoke the configured PermissionRequest hook and performed no protected
  action. A real `codex app-server --stdio` probe in a read-only sandbox then
  emitted `item/commandExecution/requestApproval` with thread, turn, and item
  identity; the client accepted it, the command executed, and the turn
  completed. The implementation therefore MUST use the App Server protocol,
  MUST validate all three upstream identities against the dispatch, and MUST
  fail closed on an unknown or resolved request id.
- Per-dispatch capability is passed only through the runner environment or
  the authenticated local adapter connection. It MUST NOT be written into
  persistent Codex configuration or model-visible context.
- Storage/build gate: install the pinned `better-sqlite3` production dependency
  and `typescript`, `@types/node`, and `@types/better-sqlite3` development
  dependencies in a clean Node 22 checkout, run a WAL transaction/restart
  probe, and prove `npm ci` plus the remote-package build remain deterministic.
  Do not substitute experimental `node:sqlite` or a JSON fallback if the spike
  fails.
- Runner acknowledgement gate: three measured launches produced first-event
  maxima of 4562 ms for Codex and 22134 ms for Claude. The implementation
  default is therefore `AGENTCHAT_RUNNER_ACK_MS=60000`; the wrapper still MUST
  acknowledge verified child-stdin acceptance rather than wait for model text.
- Rebuild continuity is scored by a scripted three-turn probe where turn 3
  must cite the turn-1 agreement verbatim or by unambiguous reference. It runs a
  real model, not just assert on the assembler's output — the rejected
  prototype passed every bookkeeping test while delivering no isolation.
  Because model output varies, the gate runs five times and requires at least
  four passes whenever the assembler changes
  rather than on every commit, so it does not become a flaky blocker people
  learn to bypass.
- The coordinator digest is at most 1000 tokens and is rebuilt per dispatch
  from live backend state.
