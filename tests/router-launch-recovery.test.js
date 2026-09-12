import { afterAll, beforeAll, describe, expect, onTestFailed, test, vi } from 'vitest';
import path from 'node:path';

import { createBackendTestContext } from './helpers/backend-test-runtime.js';

describe('thread-session runner launch recovery', () => {
  let context;
  let errorSpy;

  beforeAll(async () => {
    errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    context = await createBackendTestContext('hagency-router-launch-recovery-', {
      env: {
        HAGENCY_THREAD_SESSIONS: '1',
        HAGENCY_ROUTER_TASK_CUTOVER: '1',
        HAGENCY_CODEX_RUNNER_BIN: path.join(process.cwd(), 'tests', 'fixtures', 'runner-does-not-exist'),
        HAGENCY_RUNNER_LAUNCH_RETRY_MS: '60000',
      },
      agents: {
        worker: {
          name: 'worker', agentId: 'agent_worker', type: 'codex', role: 'coding',
          kind: 'agent', workdir: process.cwd(), workspaceMode: 'shared', online: true,
        },
      },
      agentTokens: { worker: 'worker-launch-recovery-token' },
    });
  });

  afterAll(() => {
    context.internals.stopRouterPumpForTest?.();
    context.internals.routerStoreForTest?.close();
    context.cleanup();
    errorSpy.mockRestore();
  });

  test('backend requeues a wrapper that dies before takePayload without losing its input', async () => {
    const router = context.internals.routerStoreForTest;
    const ingested = router.ingestMessage({
      messageId: 'launch-root', roomId: '!launch:test', matrixEventId: '$launch-root',
      senderName: 'alice', recipientAgentId: 'agent_worker', recipientAgentName: 'worker',
      normalizedBody: 'execute after the runtime is repaired',
    });
    const intent = router.createTaskIntent({
      requestScope: 'launch-recovery', requestKey: 'launch-task', roomId: '!launch:test',
      threadRootEventId: '$launch-root', rootMessageId: 'launch-root', inputMessageIds: ['launch-root'],
      task: { title: 'Launch recovery', assigneeAgentId: 'agent_worker', assigneeName: 'worker' },
    });
    const command = router.claimMatrixCommand();
    const active = router.recordMatrixDelivery({
      commandId: command.commandId, claimToken: command.claimToken, eventId: '$launch-anchor',
    });
    router.registerWorkspace({
      resourceId: 'launch-workspace', safeLabel: 'launch workspace', backendPath: process.cwd(),
    });
    const queued = router.enqueueDispatch({
      sessionId: active.sessionId, taskId: intent.taskId, framework: 'codex', localServerId: 'local',
      workspaceResourceId: 'launch-workspace', mayWrite: true, payload: {},
    });

    let phase = 'first-launch';
    onTestFailed(() => {
      const observed = router.db.prepare(
        'SELECT state, launch_failures, available_at FROM dispatches WHERE dispatch_id = ?',
      ).get(queued.dispatchId);
      process.stderr.write(`[router-launch-recovery] ${JSON.stringify({
        phase,
        state: observed?.state ?? null,
        launchFailures: observed?.launch_failures ?? null,
        retryDelayMs: observed?.available_at === null || !observed ? null : observed.available_at - Date.now(),
      })}\n`);
    });

    context.internals.scheduleRouterPumpForTest();
    let row;
    // A real wrapper process has to spawn and die before the count moves; on a loaded CI runner
    // that took longer than the 1 s (100 x 10 ms) this loop used to allow, and the test flaked with
    // launch_failures one short. Bounded by wall-clock instead: still a hard cap (LOOP-R1), just one
    // sized for process churn rather than for a quiet laptop.
    const deadline1 = Date.now() + 30_000; // hard cap (LOOP-R1); same reasoning as deadline2 below.
    while (Date.now() < deadline1) {
      row = router.db.prepare(
        'SELECT state, launch_failures, available_at FROM dispatches WHERE dispatch_id = ?',
      ).get(queued.dispatchId);
      if (row?.state === 'queued' && row.launch_failures === 1) break;
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    expect(row).toMatchObject({ state: 'queued', launch_failures: 1 });
    expect(row.available_at).toBeGreaterThan(Date.now());
    expect(router.db.prepare(
      'SELECT message_id FROM dispatch_messages WHERE dispatch_id = ?',
    ).all(queued.dispatchId)).toEqual([{ message_id: 'launch-root' }]);
    expect(router.snapshot().dispatches.find((dispatch) => dispatch.dispatchId === queued.dispatchId))
      .toMatchObject({
        state: 'queued',
        blockedBy: { reason: 'runner_launch_backoff', launchFailures: 1 },
      });
    expect(router.db.prepare(
      'SELECT body FROM notice_outbox WHERE dispatch_id = ?',
    ).get(queued.dispatchId)).toMatchObject({ body: expect.stringContaining('retry automatically') });

    // Simulate a persisted backoff surviving a process restart: startup only
    // kicks the pump once, which must reconstruct the future wake-up from DB.
    phase = 'retry-wake-and-second-launch';
    router.db.prepare(
      'UPDATE dispatches SET available_at = ? WHERE dispatch_id = ?',
    ).run(Date.now() + 100, queued.dispatchId);
    context.internals.scheduleRouterPumpForTest();
    const deadline2 = Date.now() + 30_000; // hard cap (LOOP-R1); 5s flaked on loaded CI runners (#139, #141): requeue → relaunch → second death takes longer there. Success still exits within one 10ms poll.
    while (Date.now() < deadline2) {
      row = router.db.prepare(
        'SELECT state, launch_failures, available_at FROM dispatches WHERE dispatch_id = ?',
      ).get(queued.dispatchId);
      if (row?.launch_failures === 2) break;
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    expect(row).toMatchObject({ state: 'queued', launch_failures: 2 });

    phase = 'stopped-pump';
    context.internals.stopRouterPumpForTest();
    router.db.prepare(
      'UPDATE dispatches SET available_at = ? WHERE dispatch_id = ?',
    ).run(Date.now(), queued.dispatchId);
    context.internals.scheduleRouterPumpForTest();
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(router.db.prepare(
      'SELECT state, launch_failures FROM dispatches WHERE dispatch_id = ?',
    ).get(queued.dispatchId)).toMatchObject({ state: 'queued', launch_failures: 2 });
  });
});
