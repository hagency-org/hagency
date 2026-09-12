import { afterEach, describe, expect, test } from 'vitest';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';

import { createRouterTaskStore, migrateLegacyTasks, openRouter, WorktreeManager } from '../router/dist/index.js';

const roots = [];

function makeRouter(options = {}) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-router-'));
  roots.push(root);
  let now = options.start ?? 1_800_000_000_000;
  const router = openRouter({
    dbPath: path.join(root, 'router.db'),
    now: options.now ?? (() => now),
    eventRetention: options.eventRetention ?? 100,
  });
  return {
    root,
    router,
    tick(ms = 1) { now += ms; },
  };
}

function ingest(router, {
  id,
  agentId = 'agent-a-id',
  agentName = 'agent-a',
  room = '!room:test',
  event = `$${id}`,
  root = null,
  body = `body:${id}`,
  explicitTask = false,
} = {}) {
  return router.ingestMessage({
    messageId: id,
    roomId: room,
    matrixEventId: event,
    threadRootEventId: root,
    senderMxid: '@alex:test',
    senderName: 'alex',
    recipientAgentId: agentId,
    recipientAgentName: agentName,
    normalizedBody: body,
    explicitTask,
  });
}

function createAndActivate(router, {
  rootMessageId = 'm-root',
  eventId = '$m-root',
  agentId = 'agent-a-id',
  agentName = 'agent-a',
  requestKey = 'create:1',
  body = `task input ${requestKey}`,
} = {}) {
  const ingested = ingest(router, {
    id: rootMessageId,
    event: eventId,
    agentId,
    agentName,
    body,
  });
  expect(ingested.ok).toBe(true);
  const intent = router.createTaskIntent({
    requestScope: 'test',
    requestKey,
    roomId: '!room:test',
    threadRootEventId: eventId,
    rootMessageId,
    inputMessageIds: [rootMessageId],
    task: {
      title: `Task ${requestKey}`,
      assigneeAgentId: agentId,
      assigneeName: agentName,
    },
  });
  expect(intent.ok).toBe(true);
  const command = router.claimMatrixCommand();
  expect(command).not.toBeNull();
  const activated = router.recordMatrixDelivery({
    commandId: command.commandId,
    claimToken: command.claimToken,
    eventId: `$anchor-${requestKey}`,
  });
  expect(activated.ok).toBe(true);
  return { ingested, intent, command, activated };
}

function enqueueWriter(router, activated, resourceId = 'workspace:agent-a') {
  router.registerWorkspace({
    resourceId,
    safeLabel: 'agent-a workspace',
    backendPath: '/private/backend/path/that-must-not-leak',
  });
  return router.enqueueDispatch({
    sessionId: activated.sessionId,
    taskId: activated.taskId,
    framework: 'codex',
    localServerId: 'local',
    workspaceResourceId: resourceId,
    mayWrite: true,
    payload: { prompt: 'work on this task' },
  });
}

function claimAndStart(router, dispatchId, runnerId = 'runner-1') {
  const claim = router.claimDispatch({
    runnerId,
    leaseMs: 60_000,
    capabilityTtlMs: 60_000,
    maxLiveRunners: 8,
  });
  expect(claim?.ok).toBe(true);
  expect(claim.dispatchId).toBe(dispatchId);
  const capability = {
    dispatchId: claim.dispatchId,
    runnerId: claim.runnerId,
    fenceGeneration: claim.fenceGeneration,
    capability: claim.capability,
  };
  const started = router.takePayload(capability);
  expect(started.ok).toBe(true);
  return { claim, capability, started };
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

describe('completed Matrix task continuation', () => {
  function completed({ settle = true } = {}) {
    const f = makeRouter();
    const { activated } = createAndActivate(f.router);
    const queued = enqueueWriter(f.router, activated);
    const first = claimAndStart(f.router, queued.dispatchId);
    const tasks = createRouterTaskStore(f.router);
    tasks.transitionTask(activated.taskId, 'done');
    const previousCompletedAt = tasks.getTask(activated.taskId).completed_at;
    if (settle) f.router.settleAndRelease({ ...first.capability, outcome: 'completed', output: { text: 'JavaScript tests passed' } });
    f.tick();
    return { ...f, activated, first, tasks, previousCompletedAt };
  }
  function followup(f, overrides = {}) {
    const input = { messageId: 'python-followup', roomId: '!room:test', matrixEventId: '$python-followup',
      threadRootEventId: '$m-root', senderMxid: '@alex:test', senderName: 'alex',
      recipientAgentId: 'agent-a-id', recipientAgentName: 'agent-a', normalizedBody: 'Rewrite as Python', ...overrides };
    expect(f.router.ingestMessage(input).ok).toBe(true);
    expect(f.router.attachTaskInputs({ taskId: f.activated.taskId, requestScope: 'matrix-thread:agent-a-id',
      requestKey: input.matrixEventId ?? input.messageId, messageIds: [input.messageId] }).ok).toBe(true);
    const queued = enqueueWriter(f.router, f.activated);
    expect(queued).toMatchObject({ ok: true, state: 'queued' });
    return queued;
  }
  const claim = router => router.claimDispatch({ runnerId: 'followup-runner', leaseMs: 60000, capabilityTtlMs: 60000, maxLiveRunners: 8 });

  test('completed thread follow-up reopens the same task and never replays consumed work', () => {
    const f = completed(), next = followup(f);
    const run = claimAndStart(f.router, next.dispatchId, 'followup-runner');
    expect(run.started).toMatchObject({ taskId: f.activated.taskId, sessionId: f.activated.sessionId });
    expect(run.started.inbox.map(m => m.body)).toEqual(['Rewrite as Python']);
    expect(run.started.context.messages.map(m => m.body)).toContain('JavaScript tests passed');
    expect(f.tasks.getTask(f.activated.taskId)).toMatchObject({ status: 'in_progress', completed_at: null });
    const reopenEvents = () => f.router.eventsAfter(0).events.filter(e => e.kind === 'task.reopened_for_followup');
    expect(reopenEvents()).toHaveLength(1);
    expect(reopenEvents()[0].payload).toMatchObject({ taskId: f.activated.taskId,
      dispatchId: next.dispatchId, messageId: 'python-followup', previousCompletedAt: f.previousCompletedAt });
    f.tasks.transitionTask(f.activated.taskId, 'done');
    expect(f.router.settleAndRelease({ ...run.capability, outcome: 'completed', output: { text: 'Python tests passed' } }).ok).toBe(true);
    expect(enqueueWriter(f.router, f.activated)).toMatchObject({ ok: true, dispatchId: next.dispatchId, state: 'completed' });
    expect(claim(f.router)).toBeNull();
    expect(f.tasks.getTask(f.activated.taskId).status).toBe('done');
    expect(reopenEvents()).toHaveLength(1);
    expect(f.router.snapshot().dispatches.filter(d => d.state === 'completed')).toHaveLength(2);
    expect(f.router.snapshot().tasks).toHaveLength(1);
    f.router.close();
  });

  test('completed thread follow-up survives restart with the original queued dispatch', () => {
    const f = completed(), next = followup(f);
    f.router.close();
    const reopened = openRouter({ dbPath: path.join(f.root, 'router.db') });
    reopened.reconcileOnStart();
    const run = claimAndStart(reopened, next.dispatchId, 'restarted-runner');
    expect(run.started.inbox.map(m => m.messageId)).toEqual(['python-followup']);
    expect(run.started.sessionId).toBe(f.activated.sessionId);
    expect(reopened.db.prepare('SELECT COUNT(*) count FROM tasks').get().count).toBe(1);
    expect(reopened.snapshot().dispatches.find(d => d.dispatchId === f.first.claim.dispatchId).state).toBe('completed');
    reopened.close();
  });

  test('completed thread follow-up waits for the previous runner and workspace gates', () => {
    const f = completed({ settle: false }), next = followup(f);
    expect(claim(f.router)).toBeNull();
    expect(f.tasks.getTask(f.activated.taskId).status).toBe('done');
    f.router.settleAndRelease({ ...f.first.capability, outcome: 'completed' });
    // Ordinary resource availability gates still apply to this fresh work.
    f.router.db.prepare("UPDATE resources SET dirty = 1 WHERE resource_id = 'workspace:agent-a'").run();
    expect(claim(f.router)).toBeNull();
    expect(f.tasks.getTask(f.activated.taskId).status).toBe('done');
    f.router.db.prepare("UPDATE resources SET dirty = 0 WHERE resource_id = 'workspace:agent-a'").run();
    expect(claimAndStart(f.router, next.dispatchId, 'unblocked-runner').started.inbox).toHaveLength(1);
    f.router.close();
    const uncertain = completed({ settle: false });
    followup(uncertain);
    uncertain.router.settleAndRelease({ ...uncertain.first.capability, outcome: 'outcome_unknown' });
    expect(claim(uncertain.router)).toBeNull();
    expect(uncertain.tasks.getTask(uncertain.activated.taskId).status).toBe('done');
    uncertain.router.close();
    const blocked = completed();
    followup(blocked);
    blocked.router.db.prepare("UPDATE tasks SET status = 'blocked' WHERE task_id = ?").run(blocked.activated.taskId);
    expect(claim(blocked.router)).toBeNull();
    expect(blocked.tasks.getTask(blocked.activated.taskId).status).toBe('blocked');
    blocked.router.close();
  });

  test('completed thread follow-up rejects foreign peer processed and mismatched inputs', () => {
    for (const variant of ['foreign', 'peer', 'processed', 'room', 'thread', 'inactive', 'unattached']) {
      const f = completed();
      followup(f, variant === 'foreign' ? { senderMxid: '@other:test' }
        : variant === 'peer' ? { senderMxid: null, matrixEventId: null, senderName: 'another-agent' } : {});
      // Corrupt scoped fixtures exercise the scheduler's independent recheck.
      if (variant === 'processed') f.router.db.prepare('UPDATE session_messages SET processed_at = 42 WHERE message_id = ?').run('python-followup');
      if (variant === 'room') f.router.db.prepare('UPDATE router_messages SET room_id = ? WHERE message_id = ?').run('!foreign:test', 'python-followup');
      if (variant === 'thread') f.router.db.prepare('UPDATE router_messages SET thread_root_event_id = ? WHERE message_id = ?').run('$foreign', 'python-followup');
      if (variant === 'inactive') f.router.db.prepare("UPDATE task_bindings SET activation_state = 'closed' WHERE task_id = ?").run(f.activated.taskId);
      if (variant === 'unattached') f.router.db.prepare('DELETE FROM task_inputs WHERE message_id = ?').run('python-followup');
      expect(claim(f.router), variant).toBeNull(); expect(claim(f.router), variant).toBeNull();
      expect(f.tasks.getTask(f.activated.taskId)).toMatchObject({ status: 'done', completed_at: f.previousCompletedAt });
      expect(f.router.db.prepare("SELECT COUNT(*) count FROM notice_outbox WHERE dedupe_key LIKE 'completed_task_followup:%'").get().count, variant).toBe(1);
      f.router.close();
    }
  });
});

describe('router identity and durable task activation', () => {
  test('delegation from a threaded human follow-up uses the original Matrix root', () => {
    const { router, tick } = makeRouter();
    const parent = createAndActivate(router);
    const first = enqueueWriter(router, parent.activated, 'parent-workspace');
    const firstRun = claimAndStart(router, first.dispatchId);
    router.settleAndRelease({ ...firstRun.capability, outcome: 'completed' });
    tick();
    ingest(router, { id: 'human-follow-up', event: '$human-follow-up', root: '$m-root', body: 'Delegate the README now' });
    router.attachTaskInputs({ taskId: parent.intent.taskId, requestScope: 'fixture', requestKey: 'follow-up', messageIds: ['human-follow-up'] });
    const next = enqueueWriter(router, parent.activated, 'parent-workspace');
    const current = claimAndStart(router, next.dispatchId, 'follow-up-runner');
    const child = router.createTaskFromDispatch({ ...current.capability, toolCallId: 'child-from-follow-up',
      inputMessageIds: [], task: { title: 'Write README', assigneeAgentId: 'agent-b-id',
        assigneeName: 'agent-b', parentId: parent.intent.taskId } });
    expect(child.ok).toBe(true);
    const command = router.claimMatrixCommand();
    expect(command.threadRootEventId).toBe('$m-root');
    // A strict Matrix server rejects m.thread whose target has rel_type.
    const matrixEvents = {
      '$m-root': { content: { body: 'Original task' } },
      '$human-follow-up': { content: { 'm.relates_to': { rel_type: 'm.thread', event_id: '$m-root' } } },
    };
    expect(matrixEvents[command.threadRootEventId].content['m.relates_to']?.rel_type).toBeUndefined();
    const activated = router.recordMatrixDelivery({ commandId: command.commandId,
      claimToken: command.claimToken, eventId: '$child-shared-thread-anchor' });
    expect(activated).toMatchObject({ ok: true, threadRootEventId: '$m-root', threadAnchorEventId: '$child-shared-thread-anchor' });
    expect(activated.sessionId).not.toBe(parent.activated.sessionId);
    expect(router.db.prepare('SELECT message_id, role FROM task_inputs WHERE task_id = ?').all(child.taskId))
      .toEqual([{ message_id: 'human-follow-up', role: 'root' }]);
    const childQueued = enqueueWriter(router, activated, 'child-workspace');
    const childRun = claimAndStart(router, childQueued.dispatchId, 'child-runner');
    expect(childRun.started.inbox).toContainEqual(expect.objectContaining({ messageId: 'human-follow-up', body: 'Delegate the README now' }));
    router.close();
  });

  test('task intents refuse nested or substituted Matrix thread roots', () => {
    const { router } = makeRouter();
    createAndActivate(router);
    ingest(router, { id: 'follow-up', event: '$follow-up', root: '$m-root' });
    const input = { requestScope: 'fixture', requestKey: 'invalid-root', roomId: '!room:test',
      rootMessageId: 'follow-up', inputMessageIds: ['follow-up'], task: { title: 'Invalid nested task',
        assigneeAgentId: 'agent-b-id', assigneeName: 'agent-b' } };
    expect(router.createTaskIntent({ ...input, threadRootEventId: '$follow-up' })).toMatchObject({ ok: false, code: 'bad_request' });
    expect(router.createTaskIntent({ ...input, threadRootEventId: '$unrelated' })).toMatchObject({ ok: false, code: 'bad_request' });
    ingest(router, { id: 'already-nested', event: '$already-nested', root: '$follow-up' });
    expect(router.createTaskIntent({ ...input, rootMessageId: 'already-nested', inputMessageIds: ['already-nested'],
      threadRootEventId: '$follow-up' })).toMatchObject({ ok: false, code: 'bad_request' });
    expect(router.claimMatrixCommand()).toBeNull();
    expect(router.snapshot().tasks).toHaveLength(1);
    router.close();
  });

  test('parent settlement preserves shared task input processing in the child session', () => {
    for (const outcome of ['completed', 'outcome_unknown', 'operator_cancel']) {
      const { router } = makeRouter();
      const parent = createAndActivate(router);
      const queued = enqueueWriter(router, parent.activated, 'parent-workspace');
      const { capability } = claimAndStart(router, queued.dispatchId);
      const child = router.createTaskFromDispatch({ ...capability, toolCallId: 'delegate',
        rootMessageId: 'm-root', inputMessageIds: ['m-root'], task: { title: 'Child task',
          assigneeAgentId: 'agent-b-id', assigneeName: 'agent-b', parentId: parent.intent.taskId } });
      expect(child.ok).toBe(true);
      const command = router.claimMatrixCommand();
      const activated = router.recordMatrixDelivery({ commandId: command.commandId,
        claimToken: command.claimToken, eventId: '$child-anchor' });
      expect(activated.ok).toBe(true);
      const settled = outcome === 'operator_cancel'
        ? router.markOutcomeUnknown(queued.dispatchId, 'operator cancellation')
        : router.settleAndRelease({ ...capability, outcome });
      expect(settled.ok).toBe(true);
      expect(router.db.prepare('SELECT processed_at FROM session_messages WHERE session_id = ? AND message_id = ?')
        .get(parent.activated.sessionId, 'm-root').processed_at).not.toBeNull();
      expect(router.db.prepare('SELECT processed_at FROM session_messages WHERE session_id = ? AND message_id = ?')
        .get(activated.sessionId, 'm-root').processed_at).toBeNull();
      const childQueued = enqueueWriter(router, activated, 'child-workspace');
      expect(childQueued).toMatchObject({ ok: true, state: 'queued' });
      const started = claimAndStart(router, childQueued.dispatchId, 'child-runner');
      expect(started.started.inbox.map((message) => message.messageId)).toContain('m-root');
      router.close();
    }
  });

  test('test_same_matrix_event_projects_to_distinct_sessions_per_agent_id', () => {
    const { router } = makeRouter();
    const first = ingest(router, {
      id: 'shared-event', agentId: 'agent-a', agentName: 'alpha',
      room: '!shared:test', event: '$shared', root: '$root', body: 'same immutable Matrix input',
    });
    const second = ingest(router, {
      id: 'shared-event', agentId: 'agent-b', agentName: 'beta',
      room: '!shared:test', event: '$shared', root: '$root', body: 'same immutable Matrix input',
    });
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    expect(first.session.sessionId).not.toBe(second.session.sessionId);
    expect(router.assembleContext(first.session.sessionId).messages).toHaveLength(1);
    expect(router.assembleContext(second.session.sessionId).messages).toHaveLength(1);
  });

  test('test_task_json_migration_is_resumable_and_api_compatible', () => {
    const { router } = makeRouter();
    const legacy = [{
      id: 'task_legacy', title: 'Legacy', description: 'kept', status: 'created',
      priority: 'p2', granularity: 'task', assignee: 'agent-a', created_by: 'alex',
      created_at: '2026-08-01T00:00:00.000Z', updated_at: '2026-08-01T00:00:00.000Z',
      started_at: null, completed_at: null, heartbeat_at: null, waiting_reason: null,
      waiting_until: null, parent_id: null, labels: ['old'], health: null, comments: [],
    }];
    const first = migrateLegacyTasks(router, legacy);
    const replay = migrateLegacyTasks(router, legacy);
    const taskStore = createRouterTaskStore(router);
    expect(first).toMatchObject({ sourceCount: 1, importedCount: 1, replayed: false });
    expect(replay).toMatchObject({ sourceCount: 1, importedCount: 1, replayed: true });
    expect(taskStore.getTask('task_legacy')).toMatchObject({ title: 'Legacy', labels: ['old'] });
    const updated = taskStore.updateTask('task_legacy', { priority: 'p1' });
    expect(updated.priority).toBe('p1');
    expect(taskStore.listTasks({ priority: 'p1' })).toHaveLength(1);
    router.close();
  });

  test('legacy task migration inserts parents before children regardless of source order', () => {
    const { router } = makeRouter();
    const base = {
      description: '', status: 'created', priority: 'p2', granularity: 'task',
      assignee: 'agent-a', created_by: 'alex',
      created_at: '2026-08-01T00:00:00.000Z', updated_at: '2026-08-01T00:00:00.000Z',
      started_at: null, completed_at: null, heartbeat_at: null, waiting_reason: null,
      waiting_until: null, labels: [], health: null, comments: [],
    };
    const result = migrateLegacyTasks(router, [
      { ...base, id: 'child', title: 'Child', parent_id: 'parent' },
      { ...base, id: 'parent', title: 'Parent', parent_id: null },
    ]);
    expect(result).toMatchObject({ sourceCount: 2, importedCount: 2 });
    expect(createRouterTaskStore(router).getTask('child')).toMatchObject({ parent_id: 'parent' });
    router.close();
  });

  test('test_legacy_task_without_thread_binding_is_not_dispatchable', () => {
    const { router } = makeRouter();
    migrateLegacyTasks(router, [{
      id: 'legacy', title: 'Legacy', description: '', status: 'created', priority: 'p2',
      granularity: 'task', assignee: 'agent-a', created_by: null,
      created_at: '2026-08-01T00:00:00.000Z', updated_at: '2026-08-01T00:00:00.000Z',
      started_at: null, completed_at: null, heartbeat_at: null, waiting_reason: null,
      waiting_until: null, parent_id: null, labels: [], health: null, comments: [],
    }]);
    const session = router.resolveSession({ agentId: 'agent-a-id', agentName: 'agent-a', roomId: '!r', threadRootEventId: '$r' });
    const result = router.enqueueDispatch({
      sessionId: session.sessionId, taskId: 'legacy', framework: 'codex',
      localServerId: 'local', workspaceResourceId: 'ws', mayWrite: true, payload: {},
    });
    expect(result).toMatchObject({ ok: false, code: 'missing_task_binding' });
    router.close();
  });

  test('test_same_matrix_thread_has_distinct_sessions_per_agent_id', () => {
    const { router } = makeRouter();
    const a = router.resolveSession({ agentId: 'a-id', agentName: 'same', roomId: '!r', threadRootEventId: '$root' });
    const b = router.resolveSession({ agentId: 'b-id', agentName: 'same', roomId: '!r', threadRootEventId: '$root' });
    expect(a.sessionId).not.toBe(b.sessionId);
    expect(a.agentId).not.toBe(b.agentId);
    router.close();
  });

  test('test_partial_unique_index_allows_one_main_session_per_agent_room', () => {
    const { router } = makeRouter();
    const first = router.resolveSession({ agentId: 'a-id', agentName: 'a', roomId: '!r' });
    const second = router.resolveSession({ agentId: 'a-id', agentName: 'renamed', roomId: '!r', threadRootEventId: null });
    expect(second.sessionId).toBe(first.sessionId);
    expect(router.db.prepare("SELECT COUNT(*) count FROM sessions WHERE scope_kind='main'").get().count).toBe(1);
    router.close();
  });

  test('test_task_thread_command_distinguishes_root_and_anchor', () => {
    const { router } = makeRouter();
    const { command, activated } = createAndActivate(router);
    expect(command.threadRootEventId).toBe('$m-root');
    expect(activated.threadRootEventId).toBe('$m-root');
    expect(activated.threadAnchorEventId).toBe('$anchor-create:1');
    expect(activated.threadAnchorEventId).not.toBe(activated.threadRootEventId);
    router.close();
  });

  test('test_task_idempotency_key_rejects_payload_change', () => {
    const { router } = makeRouter();
    ingest(router, { id: 'm1', event: '$m1' });
    const base = {
      requestScope: 'dispatch:d1', requestKey: 'tool:1', roomId: '!room:test',
      threadRootEventId: '$m1', rootMessageId: 'm1', inputMessageIds: ['m1'],
      task: { title: 'First', assigneeAgentId: 'agent-a-id', assigneeName: 'agent-a' },
    };
    const first = router.createTaskIntent(base);
    const replay = router.createTaskIntent(base);
    const conflict = router.createTaskIntent({ ...base, task: { ...base.task, title: 'Changed' } });
    expect(first.ok && first.replayed).toBe(false);
    expect(replay.ok && replay.replayed).toBe(true);
    expect(conflict).toMatchObject({ ok: false, code: 'idempotency_conflict' });
    router.close();
  });

  test('test_task_input_attachment_has_independent_idempotency', () => {
    const { router } = makeRouter();
    const task = createAndActivate(router, {
      rootMessageId: 'attach-root', eventId: '$attach-root', requestKey: 'attach-root',
    });
    ingest(router, {
      id: 'attach-later', event: '$attach-later', root: '$attach-root', body: 'later task detail',
    });
    const input = {
      taskId: task.activated.taskId,
      requestScope: 'matrix-thread:agent-a-id',
      requestKey: '$attach-later',
      messageIds: ['attach-later'],
    };
    expect(router.attachTaskInputs(input)).toMatchObject({ ok: true, replayed: false, attached: 1 });
    expect(router.attachTaskInputs(input)).toMatchObject({ ok: true, replayed: true, attached: 1 });
    expect(router.db.prepare('SELECT COUNT(*) count FROM task_inputs WHERE task_id = ? AND message_id = ?')
      .get(task.activated.taskId, 'attach-later').count).toBe(1);
    expect(router.findActiveTaskBinding('agent-a-id', '!room:test', '$attach-root'))
      .toMatchObject({ taskId: task.activated.taskId, threadRootEventId: '$attach-root' });
    router.close();
  });

  test('test_task_intent_rolls_back_with_matrix_outbox_write_failure', () => {
    const { router } = makeRouter();
    ingest(router, { id: 'm1', event: '$m1' });
    router.db.exec("CREATE TRIGGER reject_outbox BEFORE INSERT ON matrix_outbox BEGIN SELECT RAISE(ABORT, 'forced'); END");
    expect(() => router.createTaskIntent({
      requestScope: 'test', requestKey: 'fail', roomId: '!room:test',
      threadRootEventId: '$m1', rootMessageId: 'm1', inputMessageIds: ['m1'],
      task: { title: 'Must roll back', assigneeAgentId: 'agent-a-id', assigneeName: 'agent-a' },
    })).toThrow();
    expect(router.db.prepare('SELECT COUNT(*) count FROM tasks').get().count).toBe(0);
    expect(router.db.prepare('SELECT COUNT(*) count FROM task_inputs').get().count).toBe(0);
    router.close();
  });

  test('test_matrix_outbox_replay_reuses_transaction_id', () => {
    const { router, tick } = makeRouter();
    ingest(router, { id: 'm1', event: '$m1' });
    const intent = router.createTaskIntent({
      requestScope: 'test', requestKey: 'replay', roomId: '!room:test',
      threadRootEventId: '$m1', rootMessageId: 'm1', inputMessageIds: ['m1'],
      task: { title: 'Replay', assigneeAgentId: 'agent-a-id', assigneeName: 'agent-a' },
    });
    const first = router.claimMatrixCommand(1_000);
    tick(1_001);
    const second = router.claimMatrixCommand(1_000);
    expect(second.transactionId).toBe(first.transactionId);
    expect(second.commandId).toBe(intent.commandId);
    const activated = router.recordMatrixDelivery({ commandId: second.commandId, claimToken: second.claimToken, eventId: '$anchor' });
    const replay = router.recordMatrixDelivery({ commandId: second.commandId, claimToken: second.claimToken, eventId: '$anchor' });
    expect(activated.ok && activated.replayed).toBe(false);
    expect(replay.ok && replay.replayed).toBe(true);
    router.close();
  });

  test('test_task_activation_fails_closed_on_thread_send_failure', () => {
    const { router } = makeRouter();
    ingest(router, { id: 'm1', event: '$m1' });
    const intent = router.createTaskIntent({
      requestScope: 'test', requestKey: 'failure', roomId: '!room:test',
      threadRootEventId: '$m1', rootMessageId: 'm1', inputMessageIds: ['m1'],
      task: { title: 'Failure', assigneeAgentId: 'agent-a-id', assigneeName: 'agent-a' },
    });
    const command = router.claimMatrixCommand();
    const result = router.recordMatrixFailure({ commandId: command.commandId, claimToken: command.claimToken, errorCode: 'M_FORBIDDEN' });
    expect(result).toMatchObject({ ok: true, activationState: 'thread_delivery_failed' });
    expect(router.db.prepare('SELECT activated_at FROM task_inputs WHERE task_id=?').get(intent.taskId).activated_at).toBeNull();
    expect(router.claimReplyCommand()).toMatchObject({
      dispatchId: null,
      threadRootEventId: '$m1',
      body: expect.stringContaining('No coding work was started'),
    });
    router.close();
  });

  test('test_context_rebuild_uses_copied_immutable_router_input', () => {
    const { router } = makeRouter();
    const source = { body: 'immutable normalized source input' };
    const ingested = ingest(router, { id: 'copied-input', body: source.body });
    source.body = 'source row was mutated or retained elsewhere';
    const context = router.assembleContext(ingested.session.sessionId);
    expect(context.messages).toEqual([
      expect.objectContaining({ messageId: 'copied-input', body: 'immutable normalized source input' }),
    ]);
    expect(JSON.stringify(context)).not.toContain(source.body);
    router.close();
  });
});

describe('router dispatch, capability, isolation, and recovery', () => {
  test('test_front_desk_batch_creates_independently_keyed_tasks', () => {
    const { root, router } = makeRouter();
    const session = ingest(router, { id: 'front-root', event: '$front-root', body: 'create two tasks' }).session;
    ingest(router, { id: 'front-extra', event: '$front-extra', body: 'shared constraint' });
    router.registerWorkspace({ resourceId: 'front-desk-create', safeLabel: 'front desk', backendPath: root });
    const queued = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk-create', payload: { prompt: 'coordinate' },
    });
    const { capability } = claimAndStart(router, queued.dispatchId, 'coordinator-runner');
    const first = router.createTaskFromDispatch({
      ...capability, toolCallId: 'call-1', rootMessageId: 'front-root', inputMessageIds: ['front-extra'],
      task: { title: 'First', assigneeAgentId: 'worker-1', assigneeName: 'worker-one' },
    });
    const second = router.createTaskFromDispatch({
      ...capability, toolCallId: 'call-2', rootMessageId: 'front-root', inputMessageIds: ['front-extra'],
      task: { title: 'Second', assigneeAgentId: 'worker-2', assigneeName: 'worker-two' },
    });
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    expect(first.taskId).not.toBe(second.taskId);
    expect(router.createTaskFromDispatch({
      ...capability, toolCallId: 'call-3', rootMessageId: 'front-root', inputMessageIds: ['front-extra'],
      task: { title: 'Conflicting', assigneeAgentId: 'worker-1', assigneeName: 'worker-one' },
    })).toMatchObject({
      ok: false,
      code: 'bad_request',
      message: expect.stringContaining('already has a task bound'),
    });
    expect(router.createTaskFromDispatch({
      ...capability, toolCallId: 'call-1', rootMessageId: 'front-root', inputMessageIds: ['front-extra'],
      task: { title: 'First', assigneeAgentId: 'worker-1', assigneeName: 'worker-one' },
    })).toMatchObject({ ok: true, replayed: true, taskId: first.taskId });
  });

  test('messages arriving before claim extend only the queued dispatch batch', () => {
    const { root, router } = makeRouter();
    const session = ingest(router, { id: 'batch-1', body: 'first' }).session;
    router.registerWorkspace({ resourceId: 'front-desk', safeLabel: 'front desk', backendPath: root });
    const first = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: { prompt: 'first' },
    });
    ingest(router, { id: 'batch-2', body: 'second' });
    const second = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: { prompt: 'second' },
    });
    expect(second.dispatchId).toBe(first.dispatchId);
    const claim = router.claimDispatch({ runnerId: 'r1', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8 });
    const started = router.takePayload(claim);
    expect(started.inbox.map((message) => message.messageId)).toEqual(['batch-1', 'batch-2']);
  });

  test('test_explicit_task_command_is_never_batched', () => {
    const { root, router } = makeRouter();
    const session = ingest(router, { id: 'quiet-window', body: 'ordinary front-desk input' }).session;
    router.registerWorkspace({ resourceId: 'front-desk', safeLabel: 'front desk', backendPath: root });
    const queued = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: {},
    });
    ingest(router, {
      id: 'explicit-task', event: '$explicit-task', body: '/task @worker fix the isolated bug', explicitTask: true,
    });
    expect(router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: {},
    })).toMatchObject({ ok: true, dispatchId: queued.dispatchId });
    expect(router.db.prepare('SELECT message_id FROM dispatch_messages WHERE dispatch_id = ? ORDER BY message_id')
      .all(queued.dispatchId)).toEqual([{ message_id: 'quiet-window' }]);
    expect(router.createTaskIntent({
      requestScope: 'matrix-explicit:agent-a-id', requestKey: '$explicit-task',
      roomId: '!room:test', threadRootEventId: '$explicit-task',
      rootMessageId: 'explicit-task', inputMessageIds: ['explicit-task'],
      task: { title: 'Fix isolated bug', assigneeAgentId: 'worker-id', assigneeName: 'worker' },
    })).toMatchObject({ ok: true, replayed: false, activationState: 'pending_thread' });
    expect(router.claimMatrixCommand()).toMatchObject({ threadRootEventId: '$explicit-task' });
    router.close();
  });

  test('messages arriving after a dispatch starts form a separate queued batch', () => {
    const { root, router } = makeRouter();
    const session = ingest(router, { id: 'started-batch-1', body: 'first' }).session;
    router.registerWorkspace({ resourceId: 'front-desk', safeLabel: 'front desk', backendPath: root });
    const first = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: { prompt: 'first' },
    });
    const running = claimAndStart(router, first.dispatchId);
    ingest(router, { id: 'started-batch-2', body: 'second' });
    const second = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: { prompt: 'second' },
    });
    expect(second.dispatchId).not.toBe(first.dispatchId);
    expect(running.started.inbox.map((message) => message.messageId)).toEqual(['started-batch-1']);
    expect(router.checkInbox(running.capability).map((message) => message.messageId)).toEqual(['started-batch-1']);
    expect(router.db.prepare(
      'SELECT message_id FROM dispatch_messages WHERE dispatch_id = ? ORDER BY message_id',
    ).all(second.dispatchId)).toEqual([{ message_id: 'started-batch-2' }]);
    router.close();
  });

  test('one session is single-flight and the next runner receives prior agent output as context', () => {
    const { root, router } = makeRouter();
    const session = ingest(router, { id: 'fifo-1', body: 'first user turn' }).session;
    router.registerWorkspace({ resourceId: 'front-desk', safeLabel: 'front desk', backendPath: root });
    const first = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: {},
    });
    const firstRun = claimAndStart(router, first.dispatchId, 'fifo-runner-1');
    expect(firstRun.started).not.toHaveProperty('replyRoute');
    ingest(router, { id: 'fifo-2', body: 'second user turn' });
    const second = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: {},
    });
    expect(router.claimDispatch({
      runnerId: 'must-wait', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toBeNull();
    expect(router.settleAndRelease({
      ...firstRun.capability, outcome: 'completed', output: { text: 'first agent answer' },
    })).toMatchObject({ ok: true, state: 'completed' });
    const secondRun = claimAndStart(router, second.dispatchId, 'fifo-runner-2');
    const bodies = secondRun.started.context.messages.map((message) => message.body);
    expect(bodies).toContain('first user turn');
    expect(bodies).toContain('first agent answer');
    expect(bodies).toContain('second user turn');
    router.close();
  });

  test('context rotation preserves an early standing agreement within the rebuild budget', () => {
    const { root, router } = makeRouter();
    const first = ingest(router, {
      id: 'agreement-0', body: 'Standing agreement: always use the blue deployment lane.',
    });
    router.db.prepare('UPDATE session_messages SET processed_at = 1').run();
    for (let index = 1; index <= 18; index += 1) {
      ingest(router, { id: `agreement-${index}`, body: `filler-${index} ${'x'.repeat(180)}` });
      if (index < 18) {
        router.db.prepare('UPDATE session_messages SET processed_at = 1 WHERE message_id = ?')
          .run(`agreement-${index}`);
      }
    }
    router.registerWorkspace({ resourceId: 'front-desk', safeLabel: 'front desk', backendPath: root });
    const queued = router.enqueueDispatch({
      sessionId: first.session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: { rebuildTokenBudget: 500 },
    });
    const run = claimAndStart(router, queued.dispatchId, 'agreement-runner');
    expect(run.started.context.contextGeneration).toBeGreaterThan(1);
    expect(run.started.context.rollingSummary).toContain('always use the blue deployment lane');
    expect(JSON.stringify(run.started.context).length).toBeLessThan(2_500);
    router.close();
  });

  test('context rebuild hard-caps a single oversized current input without losing durable access', () => {
    const { root, router } = makeRouter();
    const first = ingest(router, {
      id: 'oversized-agreement', body: 'Standing agreement: preserve the green release lane.',
    });
    router.db.prepare('UPDATE session_messages SET processed_at = 1').run();
    const oversizedBody = `current request ${'\\"quoted\\"\n'.repeat(6_000)}`;
    ingest(router, { id: 'oversized-current', body: oversizedBody });
    router.registerWorkspace({ resourceId: 'front-desk', safeLabel: 'front desk', backendPath: root });
    const queued = router.enqueueDispatch({
      sessionId: first.session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk', payload: { rebuildTokenBudget: 500 },
    });
    const run = claimAndStart(router, queued.dispatchId, 'oversized-runner');
    expect(JSON.stringify(run.started.context).length).toBeLessThanOrEqual(2_000);
    expect(run.started.context.rollingSummary).toContain('preserve the green release lane');
    expect(run.started.context.messages.at(-1)?.body).toContain('truncated to rebuild budget');
    expect(router.checkInbox(run.capability).at(-1)?.body).toBe(oversizedBody.trim());
    router.close();
  });

  test('test_dispatch_without_active_task_binding_is_refused', () => {
    const { router } = makeRouter();
    const session = router.resolveSession({ agentId: 'a', agentName: 'a', roomId: '!r', threadRootEventId: '$root' });
    const result = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      mayWrite: true, workspaceResourceId: 'ws:a', payload: { prompt: 'work' },
    });
    expect(result).toMatchObject({ ok: false, code: 'missing_task_credential' });
    router.close();
  });

  test('test_session_context_contains_only_own_thread', () => {
    const { router } = makeRouter();
    const a = createAndActivate(router, {
      rootMessageId: 'context-a', eventId: '$context-a', requestKey: 'context-a', body: 'ONLY-A-MARKER',
    });
    const b = createAndActivate(router, {
      rootMessageId: 'context-b', eventId: '$context-b', requestKey: 'context-b', body: 'ONLY-B-MARKER',
    });
    const qa = enqueueWriter(router, a.activated, 'context-workspace-a');
    const qb = enqueueWriter(router, b.activated, 'context-workspace-b');
    const contextA = claimAndStart(router, qa.dispatchId, 'context-runner-a').started.context;
    const contextB = claimAndStart(router, qb.dispatchId, 'context-runner-b').started.context;
    expect(JSON.stringify(contextA)).toContain('ONLY-A-MARKER');
    expect(JSON.stringify(contextA)).not.toContain('ONLY-B-MARKER');
    expect(JSON.stringify(contextB)).toContain('ONLY-B-MARKER');
    expect(JSON.stringify(contextB)).not.toContain('ONLY-A-MARKER');
    router.close();
  });

  test('test_runner_capability_is_bound_hashed_and_revoked', () => {
    const { router } = makeRouter();
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    const { claim, capability } = claimAndStart(router, queued.dispatchId);
    expect(JSON.stringify(router.db.prepare('SELECT * FROM runner_capabilities').get())).not.toContain(claim.capability);
    expect(router.checkInbox({ ...capability, dispatchId: 'other' })).toMatchObject({ ok: false });
    const settled = router.settleAndRelease({ ...capability, outcome: 'completed', output: { text: 'done' } });
    expect(settled).toMatchObject({ ok: true, state: 'completed' });
    expect(router.checkInbox(capability)).toMatchObject({ ok: false, code: 'invalid_capability' });
    router.close();
  });

  test('test_dispatch_claim_rolls_back_when_any_resource_is_unavailable', () => {
    const { router } = makeRouter();
    const one = createAndActivate(router, { rootMessageId: 'm1', eventId: '$m1', requestKey: 'one' });
    const two = createAndActivate(router, { rootMessageId: 'm2', eventId: '$m2', requestKey: 'two' });
    const q1 = enqueueWriter(router, one.activated, 'workspace:one');
    router.ensureResource?.('port:3000', 'named');
    router.db.prepare("UPDATE dispatches SET named_resources_json='[\"port:3000\"]' WHERE dispatch_id=?").run(q1.dispatchId);
    const q2 = enqueueWriter(router, two.activated, 'workspace:two');
    router.db.prepare("UPDATE dispatches SET named_resources_json='[\"port:3000\"]' WHERE dispatch_id=?").run(q2.dispatchId);
    const first = router.claimDispatch({ runnerId: 'r1', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8 });
    const second = router.claimDispatch({ runnerId: 'r2', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8 });
    expect(first.ok).toBe(true);
    expect(second).toBeNull();
    expect(router.db.prepare('SELECT state FROM dispatches WHERE dispatch_id=?').get(q2.dispatchId).state).toBe('queued');
    expect(router.db.prepare('SELECT COUNT(*) count FROM runner_capabilities WHERE dispatch_id=?').get(q2.dispatchId).count).toBe(0);
    router.close();
  });

  test('test_lost_started_response_becomes_outcome_unknown_without_retry', () => {
    const { router } = makeRouter();
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    claimAndStart(router, queued.dispatchId);
    const report = router.reconcileOnStart();
    expect(report.outcomeUnknown).toBe(1);
    expect(router.db.prepare('SELECT state FROM dispatches WHERE dispatch_id=?').get(queued.dispatchId).state).toBe('outcome_unknown');
    expect(router.claimDispatch({ runnerId: 'r2', leaseMs: 1_000, capabilityTtlMs: 1_000, maxLiveRunners: 8 })).toBeNull();
    expect(router.claimReplyCommand()).toMatchObject({
      dispatchId: queued.dispatchId,
      threadRootEventId: activated.threadRootEventId,
      body: expect.stringContaining('will not be run again automatically'),
    });
    router.close();
  });

  test('test_leased_but_unstarted_dispatch_is_requeued', () => {
    const { router } = makeRouter();
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    router.claimDispatch({ runnerId: 'r1', leaseMs: 1_000, capabilityTtlMs: 1_000, maxLiveRunners: 8 });
    const report = router.reconcileOnStart();
    expect(report.requeued).toBe(1);
    expect(router.db.prepare('SELECT state FROM dispatches WHERE dispatch_id=?').get(queued.dispatchId).state).toBe('queued');
    router.close();
  });

  test('launch failure requeue persists a retry delay and can be leased again without resending input', () => {
    const { router, tick } = makeRouter();
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    const first = router.claimDispatch({
      runnerId: 'launch-fails', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    });
    expect(router.requeueBeforeStart(first, 5_000, 'executable unavailable')).toMatchObject({
      ok: true, state: 'queued', retryAt: 1_800_000_005_000,
    });
    expect(router.nextQueuedDispatchAt()).toBe(1_800_000_005_000);
    expect(router.claimDispatch({
      runnerId: 'too-early', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toBeNull();
    expect(router.db.prepare(
      'SELECT state, launch_failures, last_launch_error FROM dispatches WHERE dispatch_id = ?',
    ).get(queued.dispatchId)).toMatchObject({
      state: 'queued', launch_failures: 1, last_launch_error: 'executable unavailable',
    });
    expect(router.snapshot().dispatches.find((row) => row.dispatchId === queued.dispatchId))
      .toMatchObject({
        state: 'queued',
        blockedBy: {
          reason: 'runner_launch_backoff', retryAt: 1_800_000_005_000, launchFailures: 1,
        },
      });
    expect(router.claimReplyCommand()).toMatchObject({
      dispatchId: queued.dispatchId,
      body: expect.stringContaining('no input was lost'),
    });
    tick(5_000);
    expect(router.claimDispatch({
      runnerId: 'recovered', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toMatchObject({ ok: true, dispatchId: queued.dispatchId, runnerId: 'recovered' });
    router.close();
  });

  test('claim wake retains a retry that becomes due between claim and wake lookup', () => {
    let clock = 1_800_000_000_000;
    let crossAt = null;
    const { router } = makeRouter({ now: () => {
      const observed = clock;
      if (crossAt !== null) clock = crossAt;
      return observed;
    } });
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    const args = { runnerId: 'first', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8 };
    const first = router.claimDispatch(args);
    const retry = router.requeueBeforeStart(first, 100, 'fixture executable unavailable');
    expect(retry.ok).toBe(true);
    clock = retry.retryAt - 1;
    crossAt = retry.retryAt;

    const observed = router.claimDispatchWithWake({ ...args, runnerId: 'before-due' });
    expect(clock).toBe(retry.retryAt);
    expect(observed).toEqual({ claim: null, nextAvailableAt: retry.retryAt });
    // A new standalone future lookup has already lost this now-due retry.
    expect(router.nextQueuedDispatchAt()).toBeNull();
    expect(router.db.prepare(
      'SELECT state, launch_failures FROM dispatches WHERE dispatch_id = ?',
    ).get(queued.dispatchId)).toEqual({ state: 'queued', launch_failures: 1 });
    expect(router.db.prepare(
      'SELECT message_id FROM dispatch_messages WHERE dispatch_id = ?',
    ).all(queued.dispatchId)).toEqual([{ message_id: 'm-root' }]);
    expect(router.claimDispatchWithWake({ ...args, runnerId: 'after-wake' }))
      .toMatchObject({ claim: { ok: true, dispatchId: queued.dispatchId }, nextAvailableAt: null });
    router.close();
  });

  test('claim wake excludes already-due blocked work and retains future retries', () => {
    const { router } = makeRouter();
    const future = createAndActivate(router);
    const queued = enqueueWriter(router, future.activated);
    const args = { runnerId: 'first', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8 };
    const retry = router.requeueBeforeStart(router.claimDispatch(args), 100, 'fixture executable unavailable');
    expect(retry.ok).toBe(true);
    const blocked = createAndActivate(router, {
      rootMessageId: 'blocked-root', eventId: '$blocked-root', requestKey: 'blocked-task',
    });
    const blockedQueued = enqueueWriter(router, blocked.activated);
    router.db.prepare("UPDATE tasks SET status = 'blocked' WHERE task_id = ?").run(blocked.intent.taskId);
    router.db.prepare('UPDATE dispatches SET available_at = ? WHERE dispatch_id = ?')
      .run(1_800_000_000_000, blockedQueued.dispatchId);
    expect(router.claimDispatchWithWake({ ...args, runnerId: 'blocked-plus-future' }))
      .toEqual({ claim: null, nextAvailableAt: retry.retryAt });
    expect(router.cancelBeforeStart(queued.dispatchId).ok).toBe(true);
    expect(router.claimDispatchWithWake({ ...args, runnerId: 'blocked-only' }))
      .toEqual({ claim: null, nextAvailableAt: null });
    expect(router.db.prepare('SELECT state FROM dispatches WHERE dispatch_id = ?')
      .get(blockedQueued.dispatchId)).toEqual({ state: 'queued' });
    router.close();
  });

  test('claim wake preserves successful and refused claim results', () => {
    const { router } = makeRouter();
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    const args = { runnerId: 'first', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 1 };
    expect(router.claimDispatchWithWake(args))
      .toMatchObject({ claim: { ok: true, dispatchId: queued.dispatchId }, nextAvailableAt: null });
    const standalone = router.claimDispatch({ ...args, runnerId: 'capacity-refused' });
    expect(standalone).toMatchObject({ ok: false, code: 'live_runner_cap' });
    expect(router.claimDispatchWithWake({ ...args, runnerId: 'capacity-refused' }))
      .toEqual({ claim: standalone, nextAvailableAt: null });
    expect(router.nextQueuedDispatchAt()).toBeNull();
    router.close();
  });

  test('test_outcome_unknown_releases_lease_and_preserves_resource_dirty', () => {
    const { router } = makeRouter();
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    const { capability } = claimAndStart(router, queued.dispatchId);
    router.settleAndRelease({ ...capability, outcome: 'outcome_unknown', reason: 'runner_crash' });
    expect(router.db.prepare('SELECT COUNT(*) count FROM resource_leases').get().count).toBe(0);
    const workspace = router.inspectWorkspace('workspace:agent-a');
    expect(workspace).toMatchObject({ dirty: true, dirtyReason: 'runner_crash' });
    router.close();
  });

  test('outcome_unknown quarantines its session and workspace until inspected recovery creates a new dispatch', () => {
    const { router } = makeRouter();
    const { activated } = createAndActivate(router, {
      rootMessageId: 'recover-root', eventId: '$recover-root', requestKey: 'recover-task',
    });
    const original = enqueueWriter(router, activated, 'workspace:recover');
    const { capability } = claimAndStart(router, original.dispatchId, 'runner-that-dies');
    expect(router.settleAndRelease({
      ...capability,
      outcome: 'outcome_unknown',
      reason: 'runner_crash_after_edit',
    })).toMatchObject({ ok: true, state: 'outcome_unknown', workspaceDirty: true });

    const later = ingest(router, {
      id: 'recover-followup', event: '$recover-followup', root: '$recover-root',
      body: 'Please preserve the partial edit and finish the remaining tests.',
    });
    expect(later.ok).toBe(true);
    expect(router.attachTaskInputs({
      taskId: activated.taskId,
      requestScope: 'recover-followup',
      requestKey: '$recover-followup',
      messageIds: ['recover-followup'],
    })).toMatchObject({ ok: true, attached: 1 });
    const replacement = enqueueWriter(router, activated, 'workspace:recover');
    expect(replacement).toMatchObject({ ok: true, state: 'queued' });
    expect(replacement.dispatchId).not.toBe(original.dispatchId);
    expect(router.claimDispatch({
      runnerId: 'must-stay-quarantined', leaseMs: 60_000,
      capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toBeNull();
    expect(router.snapshot().dispatches.find((row) => row.dispatchId === replacement.dispatchId)?.blockedBy)
      .toMatchObject({ reason: 'outcome_unknown_inspection', dispatchId: original.dispatchId });
    expect(router.clearWorkspaceDirty('workspace:recover'))
      .toMatchObject({ ok: false, code: 'inspection_required' });

    const inspection = router.beginOutcomeInspection(original.dispatchId);
    expect(inspection).toMatchObject({
      ok: true,
      dispatchId: original.dispatchId,
      resource: { resourceId: 'workspace:recover', dirtyGeneration: 1 },
    });
    expect(JSON.stringify(inspection)).not.toContain('/private/backend/path');
    expect(router.resolveOutcomeUnknown({
      dispatchId: original.dispatchId,
      inspectionId: inspection.inspectionId,
      inspectionToken: 'wrong-token',
      requestId: 'recover-resolution-1',
      action: 'continue',
      operatorNote: 'Inspected git status and diff; the partial edits are coherent.',
      recoveryInstruction: 'Continue from the inspected partial edits and run the remaining tests.',
    })).toMatchObject({ ok: false, code: 'inspection_required' });

    const resolved = router.resolveOutcomeUnknown({
      dispatchId: original.dispatchId,
      inspectionId: inspection.inspectionId,
      inspectionToken: inspection.inspectionToken,
      requestId: 'recover-resolution-1',
      action: 'continue',
      operatorNote: 'Inspected git status and diff; the partial edits are coherent.',
      recoveryInstruction: 'Continue from the inspected partial edits and run the remaining tests.',
    });
    expect(resolved).toMatchObject({
      ok: true,
      replayed: false,
      dispatchId: original.dispatchId,
      replacementDispatchId: replacement.dispatchId,
    });
    expect(router.resolveOutcomeUnknown({
      dispatchId: original.dispatchId,
      inspectionId: inspection.inspectionId,
      inspectionToken: inspection.inspectionToken,
      requestId: 'recover-resolution-1',
      action: 'continue',
      operatorNote: 'Inspected git status and diff; the partial edits are coherent.',
      recoveryInstruction: 'Continue from the inspected partial edits and run the remaining tests.',
    })).toMatchObject({ ok: true, replayed: true, replacementDispatchId: replacement.dispatchId });

    expect(router.inspectWorkspace('workspace:recover')).toMatchObject({
      dirty: false,
      dirtyGeneration: 1,
      quarantinedByDispatchId: null,
    });
    expect(router.snapshot().dispatches.find((row) => row.dispatchId === original.dispatchId))
      .toMatchObject({ state: 'outcome_unknown', resolutionAction: 'continue', replacementDispatchId: replacement.dispatchId });
    expect(router.db.prepare('SELECT payload_json FROM dispatches WHERE dispatch_id = ?')
      .get(replacement.dispatchId).payload_json).not.toContain('work on this task');
    expect(router.snapshot().attention.some((row) => row.dispatchId === original.dispatchId)).toBe(false);
    const recoveryClaim = router.claimDispatch({
      runnerId: 'new-recovery-runner', leaseMs: 60_000,
      capabilityTtlMs: 60_000, maxLiveRunners: 8,
    });
    expect(recoveryClaim).toMatchObject({ ok: true, dispatchId: replacement.dispatchId });
    const recoveryPayload = router.takePayload({
      dispatchId: recoveryClaim.dispatchId,
      runnerId: recoveryClaim.runnerId,
      fenceGeneration: recoveryClaim.fenceGeneration,
      capability: recoveryClaim.capability,
    });
    expect(recoveryPayload).toMatchObject({ ok: true });
    expect(JSON.stringify(recoveryPayload)).toContain('Continue from the inspected partial edits');
    expect(router.db.prepare('SELECT state FROM dispatches WHERE dispatch_id = ?').get(original.dispatchId).state)
      .toBe('outcome_unknown');
    router.close();
  });

  test('outcome recovery rejects a stale inspection generation and supports explicit non-retry resolutions', () => {
    const { router } = makeRouter();
    const first = createAndActivate(router, {
      rootMessageId: 'stale-inspection-root', eventId: '$stale-inspection-root', requestKey: 'stale-inspection',
    });
    const original = enqueueWriter(router, first.activated, 'workspace:stale-inspection');
    const { capability } = claimAndStart(router, original.dispatchId, 'stale-inspection-runner');
    router.settleAndRelease({ ...capability, outcome: 'outcome_unknown', reason: 'runner_crash' });
    const stale = router.beginOutcomeInspection(original.dispatchId);
    expect(stale.ok).toBe(true);
    router.db.prepare(
      "UPDATE resources SET dirty_generation = dirty_generation + 1 WHERE resource_id = 'workspace:stale-inspection'",
    ).run();
    expect(router.resolveOutcomeUnknown({
      dispatchId: original.dispatchId,
      inspectionId: stale.inspectionId,
      inspectionToken: stale.inspectionToken,
      requestId: 'stale-resolution',
      action: 'accept_completed',
      operatorNote: 'This inspection is stale and must not clear newer state.',
    })).toMatchObject({ ok: false, code: 'workspace_quarantined' });

    const current = router.beginOutcomeInspection(original.dispatchId);
    expect(current).toMatchObject({ ok: true, resource: { dirtyGeneration: 2 } });
    expect(router.resolveOutcomeUnknown({
      dispatchId: original.dispatchId,
      inspectionId: current.inspectionId,
      inspectionToken: current.inspectionToken,
      requestId: 'current-resolution',
      action: 'accept_completed',
      operatorNote: 'Inspected the newer workspace state and accepted it as the completed result.',
    })).toMatchObject({ ok: true, action: 'accept_completed', replacementDispatchId: null });
    expect(router.db.prepare('SELECT status FROM tasks WHERE task_id = ?').get(first.activated.taskId).status).toBe('done');
    expect(router.claimDispatch({
      runnerId: 'must-not-retry-accepted', leaseMs: 60_000,
      capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toBeNull();
    router.close();
  });

  test('expired outcome inspection tokens leave the session and workspace quarantined', () => {
    const { router, tick } = makeRouter();
    const task = createAndActivate(router, {
      rootMessageId: 'expired-inspection-root', eventId: '$expired-inspection-root', requestKey: 'expired-inspection',
    });
    const original = enqueueWriter(router, task.activated, 'workspace:expired-inspection');
    const { capability } = claimAndStart(router, original.dispatchId, 'expired-inspection-runner');
    router.settleAndRelease({ ...capability, outcome: 'outcome_unknown', reason: 'runner_crash' });
    const inspection = router.beginOutcomeInspection(original.dispatchId, 60_000);
    expect(inspection.ok).toBe(true);
    tick(60_001);
    expect(router.resolveOutcomeUnknown({
      dispatchId: original.dispatchId,
      inspectionId: inspection.inspectionId,
      inspectionToken: inspection.inspectionToken,
      requestId: 'expired-resolution',
      action: 'continue',
      operatorNote: 'This note cannot authorize an expired inspection.',
      recoveryInstruction: 'Do not run because the inspection expired.',
    })).toMatchObject({ ok: false, code: 'inspection_expired' });
    expect(router.inspectWorkspace('workspace:expired-inspection')).toMatchObject({
      dirty: true,
      quarantinedByDispatchId: original.dispatchId,
    });
    expect(router.snapshot().attention.some((row) => row.dispatchId === original.dispatchId)).toBe(true);
    expect(router.claimDispatch({
      runnerId: 'expired-must-not-run', leaseMs: 60_000,
      capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toBeNull();
    router.close();
  });

  test('test_post_exit_message_with_shell_metachars_executes_nothing', () => {
    const { router, root } = makeRouter();
    const marker = path.join(root, 'must-not-exist');
    const result = ingest(router, { id: 'shell', event: '$shell', body: `; touch ${marker}` });
    expect(result.ok).toBe(true);
    expect(existsSync(marker)).toBe(false);
    expect(router.db.prepare('SELECT COUNT(*) count FROM dispatches').get().count).toBe(0);
    router.close();
  });

  test('test_check_inbox_requires_capability_and_scopes_session', () => {
    const { router } = makeRouter();
    const first = createAndActivate(router, { rootMessageId: 'a', eventId: '$a', requestKey: 'a' });
    ingest(router, { id: 'a2', event: '$a2', root: '$a', body: 'A-INBOX' });
    expect(router.attachTaskInputs({
      taskId: first.activated.taskId,
      requestScope: 'test-inbox-a',
      requestKey: '$a2',
      messageIds: ['a2'],
    })).toMatchObject({ ok: true, attached: 1 });
    const second = createAndActivate(router, { rootMessageId: 'b', eventId: '$b', requestKey: 'b' });
    ingest(router, { id: 'b2', event: '$b2', root: '$b', body: 'B-INBOX' });
    expect(router.attachTaskInputs({
      taskId: second.activated.taskId,
      requestScope: 'test-inbox-b',
      requestKey: '$b2',
      messageIds: ['b2'],
    })).toMatchObject({ ok: true, attached: 1 });
    const queued = enqueueWriter(router, first.activated);
    const { capability } = claimAndStart(router, queued.dispatchId);
    const inbox = router.checkInbox(capability);
    expect(JSON.stringify(inbox)).toContain('A-INBOX');
    expect(JSON.stringify(inbox)).not.toContain('B-INBOX');
    router.close();
  });

  test('test_remote_and_octos_dispatches_fail_visibly', () => {
    const { router } = makeRouter();
    const main = router.resolveSession({ agentId: 'a', agentName: 'a', roomId: '!r' });
    const octos = router.enqueueDispatch({ sessionId: main.sessionId, framework: 'octos', localServerId: 'local', payload: {} });
    const remote = router.enqueueDispatch({ sessionId: main.sessionId, framework: 'codex', serverId: 'remote', localServerId: 'local', payload: {} });
    expect(octos).toMatchObject({ ok: false, code: 'unsupported_framework' });
    expect(remote).toMatchObject({ ok: false, code: 'remote_runner_unsupported' });
    router.close();
  });
});

describe('approval parking and read projection', () => {
  test('approval retries preserve one-use decisions and exact runtime request bindings', () => {
    const { router } = makeRouter();
    try {
      const { activated } = createAndActivate(router);
      const queued = enqueueWriter(router, activated);
      const { capability } = claimAndStart(router, queued.dispatchId);
      const first = { ...capability, approvalId: 'retry-0', operationDigest: 'same-input',
        upstreamThreadId: 'thread', upstreamTurnId: 'turn', upstreamItemId: 'mcp:0', upstreamRequestId: '0', maxParkedRunners: 1 };
      const second = { ...first, approvalId: 'retry-1', upstreamItemId: 'mcp:1', upstreamRequestId: '1' };
      expect(router.parkForApproval(first)).toEqual({ ok: true, replayed: false });
      expect(router.parkForApproval(first)).toEqual({ ok: true, replayed: true });
      expect(router.parkForApproval({ ...first, upstreamRequestId: 'changed' })).toMatchObject({ ok: false, code: 'approval_mismatch' });
      const denied = { approvalId: first.approvalId, dispatchId: queued.dispatchId, operationDigest: first.operationDigest,
        decisionEventId: 'denied-0', decision: 'deny' };
      expect(router.recordApprovalDecision(denied)).toEqual({ ok: true, replayed: false });
      expect(router.recordApprovalDecision({ ...denied, decisionEventId: 'changed-verdict', decision: 'allow' })).toMatchObject({ ok: false, code: 'approval_mismatch' });
      expect(router.resumeAfterApproval(first)).toEqual({ ok: true, decision: 'deny' });
      expect(router.parkForApproval({ ...first, approvalId: 'different-id-same-runtime-request' })).toMatchObject({ ok: false, code: 'approval_mismatch' });
      expect(router.parkForApproval(second)).toEqual({ ok: true, replayed: false });
      expect(router.recordApprovalDecision(denied)).toEqual({ ok: true, replayed: true });
      expect(router.reconcileApprovalDecision(denied)).toEqual({ ok: true, replayed: true, deliverable: false });
      expect(router.reconcileApprovalDecision({ ...denied, decisionEventId: 'late-allow', decision: 'allow' })).toMatchObject({ ok: false, code: 'approval_mismatch' });
      expect(router.resumeAfterApproval(first)).toMatchObject({ ok: false, code: 'approval_mismatch' });
      expect(router.readApprovalDecision(second)).toEqual({ ok: true, decision: null });
      expect(router.db.prepare('SELECT state FROM dispatches WHERE dispatch_id=?').get(queued.dispatchId).state).toBe('parked');
      expect(router.recordApprovalDecision({ ...denied, approvalId: second.approvalId, decisionEventId: 'allowed-1', decision: 'allow' })).toEqual({ ok: true, replayed: false });
      expect(router.resumeAfterApproval(second)).toEqual({ ok: true, decision: 'allow' });
      expect(router.db.prepare('SELECT decision, resumed_at FROM approval_waits ORDER BY rowid').all()).toEqual([
        { decision: 'deny', resumed_at: expect.any(Number) }, { decision: 'allow', resumed_at: expect.any(Number) },
      ]);
      expect(router.db.prepare('SELECT COUNT(*) count FROM approval_inbox').get().count).toBe(2);
    } finally { router.close(); }
  });

  test('approval request migration preserves historical decisions and the current parked wait', () => {
    const { root, router } = makeRouter();
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    const { capability } = claimAndStart(router, queued.dispatchId);
    const first = { ...capability, approvalId: 'legacy-0', operationDigest: 'legacy-input-0',
      upstreamThreadId: 'thread', upstreamTurnId: 'turn', upstreamItemId: 'mcp:0', upstreamRequestId: '0', maxParkedRunners: 1 };
    const second = { ...first, approvalId: 'legacy-1', operationDigest: 'legacy-input-1', upstreamItemId: 'mcp:1', upstreamRequestId: '1' };
    router.parkForApproval(first);
    router.recordApprovalDecision({ approvalId: first.approvalId, dispatchId: queued.dispatchId,
      operationDigest: first.operationDigest, decisionEventId: 'legacy-deny', decision: 'deny' });
    router.resumeAfterApproval(first);
    router.parkForApproval(second);
    // Frozen pre-v9 schema: the migration must accept a database created by the
    // earlier release, independently of the current implementation's schema.
    const legacySchema = `CREATE TABLE approval_waits_legacy (
      approval_id TEXT PRIMARY KEY,
      dispatch_id TEXT NOT NULL REFERENCES dispatches(dispatch_id) ON DELETE CASCADE,
      operation_digest TEXT NOT NULL,
      upstream_thread_id TEXT, upstream_turn_id TEXT, upstream_item_id TEXT, upstream_request_id TEXT,
      created_at INTEGER NOT NULL, resolved_at INTEGER, decision TEXT,
      UNIQUE(dispatch_id, operation_digest)
    );`;
    const columns = 'approval_id, dispatch_id, operation_digest, upstream_thread_id, upstream_turn_id, upstream_item_id, upstream_request_id, created_at, resolved_at, decision';
    // Build a real pre-migration table in this owned fixture; do not edit live state.
    router.db.exec(`${legacySchema}
      INSERT INTO approval_waits_legacy SELECT ${columns} FROM approval_waits;
      DROP TABLE approval_waits;
      ALTER TABLE approval_waits_legacy RENAME TO approval_waits;
      DELETE FROM router_schema_migrations WHERE version >= 9;`);
    router.close();
    const reopened = openRouter({ dbPath: path.join(root, 'router.db'), now: () => 1_800_000_000_000 });
    try {
      expect(reopened.db.prepare('SELECT approval_id, decision, resumed_at FROM approval_waits ORDER BY rowid').all()).toEqual([
        { approval_id: 'legacy-0', decision: 'deny', resumed_at: expect.any(Number) },
        { approval_id: 'legacy-1', decision: null, resumed_at: null },
      ]);
      expect(reopened.db.prepare('SELECT decision_event_id, decision FROM approval_inbox').all()).toEqual([{ decision_event_id: 'legacy-deny', decision: 'deny' }]);
      expect(reopened.resumeAfterApproval(first)).toMatchObject({ ok: false, code: 'approval_mismatch' });
      expect(reopened.recordApprovalDecision({ approvalId: second.approvalId, dispatchId: queued.dispatchId,
        operationDigest: second.operationDigest, decisionEventId: 'legacy-new-deny', decision: 'deny' })).toMatchObject({ ok: true });
      expect(reopened.resumeAfterApproval(second)).toEqual({ ok: true, decision: 'deny' });
      expect(reopened.parkForApproval({ ...first, approvalId: 'fresh-2', upstreamItemId: 'mcp:2', upstreamRequestId: '2' })).toMatchObject({ ok: true, replayed: false });
      expect(reopened.db.pragma('foreign_key_check')).toEqual([]);
    } finally { reopened.close(); }
  });

  test('test_approval_decision_event_reconciles_idempotently', () => {
    const { router } = makeRouter();
    const { activated } = createAndActivate(router);
    const queued = enqueueWriter(router, activated);
    const { capability } = claimAndStart(router, queued.dispatchId);
    const parked = router.parkForApproval({
      ...capability,
      approvalId: 'approval-1',
      operationDigest: 'a'.repeat(64),
      upstreamThreadId: 'thread-1',
      upstreamTurnId: 'turn-1',
      upstreamItemId: 'item-1',
      upstreamRequestId: 'request-1',
      maxParkedRunners: 4,
    });
    expect(parked.ok).toBe(true);
    const event = {
      decisionEventId: 'decision-1', approvalId: 'approval-1', dispatchId: queued.dispatchId,
      operationDigest: 'a'.repeat(64), decision: 'allow',
    };
    expect(router.recordApprovalDecision(event)).toMatchObject({ ok: true, replayed: false });
    expect(router.recordApprovalDecision(event)).toMatchObject({ ok: true, replayed: true });
    expect(router.readApprovalDecision({ ...capability, approvalId: 'approval-1', operationDigest: 'a'.repeat(64) })).toMatchObject({ ok: true, decision: 'allow' });
    expect(router.recordApprovalDecision({ ...event, decisionEventId: 'decision-2', dispatchId: 'other' })).toMatchObject({ ok: false });
    router.close();
  });

  test('test_parked_runner_cap_denies_new_park_request', () => {
    const { router } = makeRouter();
    const first = createAndActivate(router, { rootMessageId: 'a', eventId: '$a', requestKey: 'a' });
    const queued = enqueueWriter(router, first.activated);
    const { capability } = claimAndStart(router, queued.dispatchId);
    const denied = router.parkForApproval({
      ...capability, approvalId: 'a', operationDigest: 'b'.repeat(64), maxParkedRunners: 0,
    });
    expect(denied).toMatchObject({ ok: false, code: 'parked_runner_cap' });
    expect(router.db.prepare('SELECT COUNT(*) count FROM resource_leases').get().count).toBe(1);
    expect(router.db.prepare('SELECT state FROM dispatches WHERE dispatch_id=?').get(queued.dispatchId).state).toBe('started');
    router.close();
  });

  test('test_shared_mode_parked_approval_queues_other_writer', () => {
    const { router } = makeRouter();
    const firstTask = createAndActivate(router, {
      rootMessageId: 'wait-root-1', eventId: '$wait-root-1', requestKey: 'wait-1',
    });
    const secondTask = createAndActivate(router, {
      rootMessageId: 'wait-root-2', eventId: '$wait-root-2', requestKey: 'wait-2',
    });
    const first = enqueueWriter(router, firstTask.activated, 'shared-wait-workspace');
    const firstRun = claimAndStart(router, first.dispatchId, 'wait-runner-1');
    expect(router.parkForApproval({
      ...firstRun.capability,
      approvalId: 'wait-approval', operationDigest: 'wait-operation', maxParkedRunners: 4,
    })).toMatchObject({ ok: true });
    const second = enqueueWriter(router, secondTask.activated, 'shared-wait-workspace');
    expect(router.claimDispatch({
      runnerId: 'wait-runner-2', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toBeNull();
    expect(router.claimReplyCommand()).toMatchObject({
      dispatchId: second.dispatchId,
      threadRootEventId: '$wait-root-2',
      body: expect.stringContaining('awaiting owner approval'),
    });
    router.close();
  });

  test('test_worktree_mode_approval_wait_does_not_block_another_topic', () => {
    const { router } = makeRouter();
    const firstTask = createAndActivate(router, {
      rootMessageId: 'worktree-wait-1', eventId: '$worktree-wait-1', requestKey: 'worktree-wait-1',
    });
    const secondTask = createAndActivate(router, {
      rootMessageId: 'worktree-wait-2', eventId: '$worktree-wait-2', requestKey: 'worktree-wait-2',
    });
    router.registerWorkspace({ resourceId: 'worktree-one', safeLabel: 'worktree one', backendPath: '/tmp' });
    router.registerWorkspace({ resourceId: 'worktree-two', safeLabel: 'worktree two', backendPath: '/tmp' });
    const first = router.enqueueDispatch({
      sessionId: firstTask.activated.sessionId, taskId: firstTask.activated.taskId,
      framework: 'codex', localServerId: 'local', workspaceMode: 'worktree',
      workspaceResourceId: 'worktree-one', mayWrite: true, payload: {},
    });
    const firstRun = claimAndStart(router, first.dispatchId, 'worktree-wait-runner-1');
    router.parkForApproval({
      ...firstRun.capability,
      approvalId: 'worktree-wait-approval', operationDigest: 'worktree-wait-operation', maxParkedRunners: 4,
    });
    const second = router.enqueueDispatch({
      sessionId: secondTask.activated.sessionId, taskId: secondTask.activated.taskId,
      framework: 'codex', localServerId: 'local', workspaceMode: 'worktree',
      workspaceResourceId: 'worktree-two', mayWrite: true, payload: {},
    });
    expect(router.claimDispatch({
      runnerId: 'worktree-wait-runner-2', leaseMs: 60_000,
      capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toMatchObject({ ok: true, dispatchId: second.dispatchId });
    router.close();
  });

  test('test_nonwriting_topic_flows_while_an_approval_holds_the_writer_lease', () => {
    const { root, router } = makeRouter();
    const task = createAndActivate(router, {
      rootMessageId: 'writer-wait', eventId: '$writer-wait', requestKey: 'writer-wait',
    });
    const writer = enqueueWriter(router, task.activated, 'writer-wait-workspace');
    const writerRun = claimAndStart(router, writer.dispatchId, 'writer-wait-runner');
    router.parkForApproval({
      ...writerRun.capability,
      approvalId: 'writer-wait-approval', operationDigest: 'writer-wait-operation', maxParkedRunners: 4,
    });
    const main = ingest(router, {
      id: 'front-desk-during-approval', agentId: 'agent-a-id', agentName: 'agent-a',
      body: 'read-only coordination',
    }).session;
    router.registerWorkspace({ resourceId: 'front-desk-readonly', safeLabel: 'front desk', backendPath: root });
    const readonly = router.enqueueDispatch({
      sessionId: main.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'front-desk-readonly', mayWrite: false, payload: {},
    });
    expect(router.claimDispatch({
      runnerId: 'front-desk-runner', leaseMs: 60_000,
      capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toMatchObject({ ok: true, dispatchId: readonly.dispatchId });
    router.close();
  });

  test('test_parked_cap_reserves_live_slot_for_non_writing_dispatch', () => {
    const { root, router } = makeRouter();
    const task = createAndActivate(router, {
      rootMessageId: 'parked-cap-writer', eventId: '$parked-cap-writer', requestKey: 'parked-cap-writer',
    });
    const writer = enqueueWriter(router, task.activated, 'parked-cap-workspace');
    const writerRun = claimAndStart(router, writer.dispatchId, 'parked-cap-runner');
    expect(router.parkForApproval({
      ...writerRun.capability,
      approvalId: 'parked-cap-approval', operationDigest: 'parked-cap-operation', maxParkedRunners: 1,
    })).toMatchObject({ ok: true });
    const main = ingest(router, {
      id: 'parked-cap-front-desk', agentId: 'coordinator-id', agentName: 'coordinator',
      body: 'continue read-only coordination',
    }).session;
    router.registerWorkspace({ resourceId: 'parked-cap-front-desk', safeLabel: 'front desk', backendPath: root });
    const readonly = router.enqueueDispatch({
      sessionId: main.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'parked-cap-front-desk', mayWrite: false, payload: {},
    });
    expect(router.claimDispatch({
      runnerId: 'reserved-live-slot', leaseMs: 60_000,
      capabilityTtlMs: 60_000, maxLiveRunners: 2,
    })).toMatchObject({ ok: true, dispatchId: readonly.dispatchId });
    expect(router.db.prepare('SELECT state FROM dispatches WHERE dispatch_id = ?')
      .get(writer.dispatchId).state).toBe('parked');
    router.close();
  });

  test('test_worktrees_still_contend_for_named_port_lease', () => {
    const { router } = makeRouter();
    const firstTask = createAndActivate(router, {
      rootMessageId: 'port-task-a', eventId: '$port-task-a', requestKey: 'port-task-a',
    });
    const secondTask = createAndActivate(router, {
      rootMessageId: 'port-task-b', eventId: '$port-task-b', requestKey: 'port-task-b',
    });
    router.registerWorkspace({ resourceId: 'port-worktree-a', safeLabel: 'worktree a', backendPath: '/tmp' });
    router.registerWorkspace({ resourceId: 'port-worktree-b', safeLabel: 'worktree b', backendPath: '/tmp' });
    const first = router.enqueueDispatch({
      sessionId: firstTask.activated.sessionId, taskId: firstTask.activated.taskId,
      framework: 'codex', localServerId: 'local', workspaceMode: 'worktree',
      workspaceResourceId: 'port-worktree-a', namedResourceIds: ['port:4173'], mayWrite: true, payload: {},
    });
    const second = router.enqueueDispatch({
      sessionId: secondTask.activated.sessionId, taskId: secondTask.activated.taskId,
      framework: 'codex', localServerId: 'local', workspaceMode: 'worktree',
      workspaceResourceId: 'port-worktree-b', namedResourceIds: ['port:4173'], mayWrite: true, payload: {},
    });
    const firstClaim = router.claimDispatch({
      runnerId: 'port-runner-a', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    });
    expect(firstClaim).toMatchObject({ ok: true, dispatchId: first.dispatchId });
    expect(router.claimDispatch({
      runnerId: 'port-runner-b', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    })).toBeNull();
    expect(router.snapshot().dispatches.find((row) => row.dispatchId === second.dispatchId)?.blockedBy)
      .toMatchObject({ reason: 'resource_lease', resourceLabel: 'port:4173' });
    router.close();
  });

  test('test_restart_reconciles_started_dispatch_to_outcome_unknown', () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-router-restart-'));
    roots.push(root);
    const dbPath = path.join(root, 'router.db');
    let router = openRouter({ dbPath });
    const task = createAndActivate(router, {
      rootMessageId: 'restart-task', eventId: '$restart-task', requestKey: 'restart-task',
    });
    const queued = enqueueWriter(router, task.activated, 'restart-workspace');
    claimAndStart(router, queued.dispatchId, 'restart-runner');
    router.close();
    router = openRouter({ dbPath });
    expect(router.reconcileOnStart()).toMatchObject({ outcomeUnknown: 1 });
    expect(router.snapshot().dispatches.find((row) => row.dispatchId === queued.dispatchId))
      .toMatchObject({ state: 'outcome_unknown', terminalReason: 'backend_restart_unverifiable_runner' });
    expect(router.claimReplyCommand()).toMatchObject({
      dispatchId: queued.dispatchId, body: expect.stringMatching(/inspect the workspace/i),
    });
    expect(router.claimDispatch({
      runnerId: 'restart-must-not-run', leaseMs: 1_000, capabilityTtlMs: 1_000, maxLiveRunners: 8,
    })).toBeNull();
    router.close();
  });

  test('test_fencing_generation_survives_restart', () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-router-fence-'));
    roots.push(root);
    const dbPath = path.join(root, 'router.db');
    let now = 1_800_000_000_000;
    let router = openRouter({ dbPath, now: () => now });
    const session = ingest(router, { id: 'fence-input' }).session;
    router.registerWorkspace({ resourceId: 'fence-workspace', safeLabel: 'fence workspace', backendPath: root });
    const queued = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'claude', localServerId: 'local',
      workspaceResourceId: 'fence-workspace', mayWrite: false, payload: {},
    });
    const oldClaim = router.claimDispatch({
      runnerId: 'old-runner', leaseMs: 1_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    });
    expect(oldClaim).toMatchObject({ ok: true, fenceGeneration: 1 });
    router.close();
    now += 1_001;
    router = openRouter({ dbPath, now: () => now });
    expect(router.reconcileOnStart()).toMatchObject({ requeued: 1 });
    const newClaim = router.claimDispatch({
      runnerId: 'new-runner', leaseMs: 60_000, capabilityTtlMs: 60_000, maxLiveRunners: 8,
    });
    expect(newClaim).toMatchObject({ ok: true, dispatchId: queued.dispatchId, fenceGeneration: 2 });
    expect(router.settleAndRelease({
      dispatchId: oldClaim.dispatchId, runnerId: oldClaim.runnerId,
      fenceGeneration: oldClaim.fenceGeneration, capability: oldClaim.capability,
      outcome: 'completed', output: { text: 'late stale output' },
    })).toMatchObject({ ok: false });
    expect(router.db.prepare('SELECT state, fence_generation, fenced_output_json FROM dispatches WHERE dispatch_id = ?')
      .get(queued.dispatchId)).toMatchObject({
        state: 'leased', fence_generation: 2, fenced_output_json: JSON.stringify({ text: 'late stale output' }),
      });
    router.close();
  });

  test('test_router_snapshot_excludes_paths_and_owner_private_approval_data', () => {
    const { router } = makeRouter();
    router.registerWorkspace({
      resourceId: 'ws', safeLabel: 'safe workspace', backendPath: '/secret/absolute/workspace', branchName: 'hagency/task',
    });
    router.db.prepare("UPDATE resources SET dirty=1, dirty_reason='inspect required' WHERE resource_id='ws'").run();
    const text = JSON.stringify(router.snapshot());
    expect(text).toContain('safe workspace');
    expect(text).not.toContain('/secret/absolute/workspace');
    expect(text).not.toContain('normalized_body');
    expect(text).not.toContain('operation_digest');
    router.close();
  });

  test('test_router_event_gap_uses_transactional_low_watermark', () => {
    const { router } = makeRouter({ eventRetention: 100 });
    for (let i = 0; i < 130; i += 1) {
      router.resolveSession({ agentId: `a${i}`, agentName: `a${i}`, roomId: '!r' });
    }
    const snapshot = router.snapshot();
    expect(snapshot.lowWatermark).toBeGreaterThan(1);
    expect(router.eventsAfter(1)).toMatchObject({ gap: true });
    router.close();
  });
});

describe('conservative worktree lifecycle', () => {
  test('test_worktree_mode_runs_two_threads_in_distinct_worktrees', () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-worktree-'));
    roots.push(root);
    const repo = path.join(root, 'repo');
    const worktreesDir = path.join(root, 'worktrees');
    execFileSync('git', ['init', repo]);
    execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repo });
    execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repo });
    writeFileSync(path.join(repo, 'README.md'), 'base\n');
    execFileSync('git', ['add', 'README.md'], { cwd: repo });
    execFileSync('git', ['commit', '-m', 'base'], { cwd: repo });
    const manager = new WorktreeManager();
    const a = manager.ensure({ repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$thread-a' });
    const b = manager.ensure({ repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$thread-b' });
    expect(a.path).not.toBe(b.path);
    expect(a.branch).not.toBe(b.branch);
    expect(manager.ensure({ repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$thread-a' })).toMatchObject({ path: a.path, created: false });
  });

  test('failed worktree bootstrap remains fail-closed until a successful bootstrap is recorded', () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-worktree-bootstrap-'));
    roots.push(root);
    const repo = path.join(root, 'repo');
    const worktreesDir = path.join(root, 'worktrees');
    execFileSync('git', ['init', repo]);
    execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repo });
    execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repo });
    writeFileSync(path.join(repo, 'README.md'), 'base\n');
    execFileSync('git', ['add', 'README.md'], { cwd: repo });
    execFileSync('git', ['commit', '-m', 'base'], { cwd: repo });
    const manager = new WorktreeManager();
    const base = { repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$bootstrap' };
    const failing = { ...base, bootstrap: [process.execPath, '-e', 'process.exit(17)'] };
    expect(() => manager.ensure(failing)).toThrow();
    expect(() => manager.ensure(failing)).toThrow();
    expect(manager.ensure({
      ...base,
      bootstrap: [process.execPath, '-e', 'process.exit(0)'],
    })).toMatchObject({ created: false });
    expect(manager.ensure({
      ...base,
      bootstrap: [process.execPath, '-e', 'process.exit(0)'],
    })).toMatchObject({ created: false });
  });

  test('changed bootstrap cannot bypass a dirty failed-bootstrap quarantine', () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-worktree-bootstrap-dirty-'));
    roots.push(root);
    const repo = path.join(root, 'repo');
    const worktreesDir = path.join(root, 'worktrees');
    execFileSync('git', ['init', repo]);
    execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repo });
    execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repo });
    writeFileSync(path.join(repo, 'README.md'), 'base\n');
    execFileSync('git', ['add', 'README.md'], { cwd: repo });
    execFileSync('git', ['commit', '-m', 'base'], { cwd: repo });
    const manager = new WorktreeManager();
    const base = { repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$dirty-bootstrap' };
    expect(() => manager.ensure({
      ...base,
      bootstrap: [process.execPath, '-e', "require('fs').writeFileSync('partial','dirty');process.exit(17)"],
    })).toThrow();
    expect(() => manager.ensure({
      ...base,
      bootstrap: [process.execPath, '-e', 'process.exit(0)'],
    })).toThrow(/dirty workspace; operator repair is required/);
  });

  test('worktree bootstrap does not inherit backend credentials', async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-worktree-bootstrap-env-'));
    roots.push(root);
    const repo = path.join(root, 'repo');
    const worktreesDir = path.join(root, 'worktrees');
    execFileSync('git', ['init', repo]);
    execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repo });
    execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repo });
    writeFileSync(path.join(repo, 'README.md'), 'base\n');
    execFileSync('git', ['add', 'README.md'], { cwd: repo });
    execFileSync('git', ['commit', '-m', 'base'], { cwd: repo });
    const previous = process.env.API_TOKEN;
    process.env.API_TOKEN = 'must-not-reach-bootstrap';
    try {
      const manager = new WorktreeManager();
      const environmentProbe = [
        "const fs=require('fs');const cp=require('child_process');",
        "let parent='';const proc='/proc/'+process.ppid+'/environ';",
        "if(fs.existsSync(proc))parent=fs.readFileSync(proc).toString();",
        "else parent=cp.execFileSync('ps',['eww','-p',String(process.ppid),'-o','command='],{encoding:'utf8'});",
        "if(!parent)throw new Error('parent environment probe was empty');",
        "const leaked=Boolean(process.env.API_TOKEN)||parent.includes('must-not-reach-bootstrap');",
        "fs.writeFileSync('bootstrap-env',leaked?'leaked':'clean');",
      ].join('');
      const info = await manager.ensureAsync({
        repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$bootstrap-env',
        bootstrap: [process.execPath, '-e', environmentProbe],
      });
      expect(readFileSync(path.join(info.path, 'bootstrap-env'), 'utf8')).toBe('clean');
    } finally {
      if (previous === undefined) delete process.env.API_TOKEN;
      else process.env.API_TOKEN = previous;
    }
  });

  test('async worktree preparation does not block the backend event loop', async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-worktree-bootstrap-async-'));
    roots.push(root);
    const repo = path.join(root, 'repo');
    const worktreesDir = path.join(root, 'worktrees');
    execFileSync('git', ['init', repo]);
    execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repo });
    execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repo });
    writeFileSync(path.join(repo, 'README.md'), 'base\n');
    execFileSync('git', ['add', 'README.md'], { cwd: repo });
    execFileSync('git', ['commit', '-m', 'base'], { cwd: repo });
    const manager = new WorktreeManager();
    let timerFired = false;
    const timer = new Promise((resolve) => setTimeout(() => {
      timerFired = true;
      resolve();
    }, 20));
    const preparation = manager.ensureAsync({
      repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$async-bootstrap',
      bootstrap: [process.execPath, '-e', 'const end=Date.now()+250;while(Date.now()<end){}'],
    });
    await timer;
    expect(timerFired).toBe(true);
    await expect(preparation).resolves.toMatchObject({ created: true });
  });

  test('recreated worktree cannot reuse bootstrap success from a removed checkout', () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-worktree-recreate-'));
    roots.push(root);
    const repo = path.join(root, 'repo');
    const worktreesDir = path.join(root, 'worktrees');
    execFileSync('git', ['init', repo]);
    execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repo });
    execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repo });
    writeFileSync(path.join(repo, 'README.md'), 'base\n');
    execFileSync('git', ['add', 'README.md'], { cwd: repo });
    execFileSync('git', ['commit', '-m', 'base'], { cwd: repo });
    const manager = new WorktreeManager();
    const spec = {
      repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$recreate',
      bootstrap: [process.execPath, '-e', "require('fs').writeFileSync('bootstrap-output','ready')"],
    };
    const first = manager.ensure(spec);
    expect(existsSync(path.join(first.path, 'bootstrap-output'))).toBe(true);
    manager.remove(spec, true);
    const second = manager.ensure(spec);
    expect(existsSync(path.join(second.path, 'bootstrap-output'))).toBe(true);
  });

  test('worktree resource identity includes repository and worktree root', () => {
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-worktree-identity-'));
    roots.push(root);
    const makeRepo = (name) => {
      const repo = path.join(root, name);
      execFileSync('git', ['init', repo]);
      execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repo });
      execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repo });
      writeFileSync(path.join(repo, 'README.md'), `${name}\n`);
      execFileSync('git', ['add', 'README.md'], { cwd: repo });
      execFileSync('git', ['commit', '-m', 'base'], { cwd: repo });
      return repo;
    };
    const manager = new WorktreeManager();
    const a = manager.ensure({
      repositoryPath: makeRepo('repo-a'), worktreesDir: path.join(root, 'worktrees-a'),
      agentId: 'agent', threadRootEventId: '$same-thread',
    });
    const b = manager.ensure({
      repositoryPath: makeRepo('repo-b'), worktreesDir: path.join(root, 'worktrees-b'),
      agentId: 'agent', threadRootEventId: '$same-thread',
    });
    expect(a.resourceId).not.toBe(b.resourceId);
  });

  test('registered workspace resource cannot be redirected to another backend path', () => {
    const { root, router } = makeRouter();
    const first = path.join(root, 'first');
    const second = path.join(root, 'second');
    router.registerWorkspace({ resourceId: 'immutable-workspace', safeLabel: 'first', backendPath: first });
    expect(() => router.registerWorkspace({
      resourceId: 'immutable-workspace', safeLabel: 'second', backendPath: second,
    })).toThrow(/cannot change its backend path/);
    expect(router.db.prepare(
      'SELECT backend_path FROM resources WHERE resource_id = ?',
    ).get('immutable-workspace')).toEqual({ backend_path: first });
    router.close();
  });

  test('test_dirty_worktree_retained_on_session_eviction', () => {
    const { router } = makeRouter();
    const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-worktree-dirty-'));
    roots.push(root);
    const repo = path.join(root, 'repo');
    const worktreesDir = path.join(root, 'worktrees');
    execFileSync('git', ['init', repo]);
    execFileSync('git', ['config', 'user.email', 'test@example.com'], { cwd: repo });
    execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repo });
    writeFileSync(path.join(repo, 'README.md'), 'base\n');
    execFileSync('git', ['add', 'README.md'], { cwd: repo });
    execFileSync('git', ['commit', '-m', 'base'], { cwd: repo });
    const manager = new WorktreeManager();
    const spec = { repositoryPath: repo, worktreesDir, agentId: 'agent', threadRootEventId: '$dirty' };
    const info = manager.ensure(spec);
    writeFileSync(path.join(info.path, 'dirty.txt'), 'keep me\n');
    expect(manager.inspect(spec).dirty).toBe(true);

    const { activated } = createAndActivate(router, {
      rootMessageId: 'dirty-task-root', eventId: '$dirty', requestKey: 'dirty-worktree',
    });
    router.registerWorkspace({
      resourceId: info.resourceId,
      safeLabel: info.safeLabel,
      backendPath: info.path,
      branchName: info.branch,
    });
    const queued = router.enqueueDispatch({
      sessionId: activated.sessionId,
      taskId: activated.taskId,
      framework: 'codex',
      localServerId: 'local',
      workspaceMode: 'worktree',
      workspaceResourceId: info.resourceId,
      mayWrite: true,
      payload: {},
    });
    const { capability } = claimAndStart(router, queued.dispatchId, 'dirty-runner');
    expect(router.settleAndRelease({
      ...capability,
      outcome: 'outcome_unknown',
      reason: 'session_eviction_requested',
    })).toMatchObject({ ok: true, state: 'outcome_unknown', workspaceDirty: true });

    expect(() => manager.remove(spec)).toThrow(/dirty worktree requires explicit force/);
    expect(existsSync(info.path)).toBe(true);
    expect(execFileSync('git', ['show-ref', '--verify', `refs/heads/${info.branch}`], { cwd: repo, encoding: 'utf8' })).toContain(info.branch);
    const safeProjection = router.inspectWorkspace(info.resourceId);
    expect(safeProjection).toMatchObject({
      resourceId: info.resourceId,
      safeLabel: info.safeLabel,
      branchName: info.branch,
      dirty: true,
      dirtyReason: 'session_eviction_requested',
    });
    expect(JSON.stringify(safeProjection)).not.toContain(info.path);
    router.close();
  });

  // Regression: a dispatch without the workspace lease must not be launched
  // into a write-capable runtime. Its writes would be neither serialized
  // against other runners nor recorded as workspace dirt, so the launcher has
  // to be able to read the lease decision (REQ-TSS-WORKSPACE-LEASE).
  test('launch descriptor reports write authority so the sandbox can follow the lease', () => {
    const { root, router } = makeRouter();
    const session = ingest(router, { id: 'authority-1', body: 'front desk question' }).session;
    router.registerWorkspace({ resourceId: 'authority-ws', safeLabel: 'shared', backendPath: root });
    const queued = router.enqueueDispatch({
      sessionId: session.sessionId, framework: 'codex', localServerId: 'local',
      workspaceResourceId: 'authority-ws', mayWrite: false, payload: { prompt: 'just asking' },
    });
    const { capability } = claimAndStart(router, queued.dispatchId, 'runner-readonly');
    expect(router.getLaunchDescriptor(capability).mayWrite).toBe(false);

    const activated = createAndActivate(router, {
      rootMessageId: 'authority-writer', eventId: '$authority-writer',
      requestKey: 'authority-writer', body: 'do the work',
    });
    const writing = enqueueWriter(router, activated.activated, 'authority-writer-ws');
    const writer = claimAndStart(router, writing.dispatchId, 'runner-writer');
    expect(router.getLaunchDescriptor(writer.capability).mayWrite).toBe(true);
    router.close();
  });

});
