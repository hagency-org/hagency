import { afterAll, afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import { mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { restoreEnv, snapshotEnv } from './helpers/env.js';

// Satisfies REQ-TSS-TASK-ACTIVATION and ADR-016's appservice reachability contract.
describe('appservice startup drains thread outboxes without a bot', () => {
  let MatrixBridge;
  let runtimeDir;
  let envSnapshot;
  let bridge;

  beforeAll(async () => {
    runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'hagency-botless-router-'));
    envSnapshot = snapshotEnv([
      'HAGENCY_RUNTIME_DIR', 'MATRIX_BRIDGE_SECRET', 'HAGENCY_THREAD_SESSIONS',
      'HAGENCY_APPSERVICE_SYNC_SIDE', 'HAGENCY_APPSERVICE_SYNC_URL',
      'HAGENCY_ROUTER_OUTBOX_POLL_MS',
    ]);
    Object.assign(process.env, {
      HAGENCY_RUNTIME_DIR: runtimeDir,
      MATRIX_BRIDGE_SECRET: 'test-bridge-secret',
      HAGENCY_THREAD_SESSIONS: '1',
      HAGENCY_APPSERVICE_SYNC_SIDE: 'palpo.test',
      HAGENCY_APPSERVICE_SYNC_URL: 'http://palpo.test',
      HAGENCY_ROUTER_OUTBOX_POLL_MS: '250',
    });
    ({ MatrixBridge } = await import(`${pathToFileURL(path.resolve('bridge-matrix.js')).href}?botless-router-start`));
  });

  afterEach(async () => {
    bridge?.stopApprovalProjectionWorker();
    await bridge?._approvalProjectionDrainPromise;
    vi.clearAllTimers();
    bridge = null;
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  afterAll(() => {
    restoreEnv(envSnapshot);
    rmSync(runtimeDir, { recursive: true, force: true });
  });

  test('a pending task acknowledgement and a later reply both leave after bot login fails', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('fetch', async () => new Response('{"ok":true}', { status: 200 }));
    bridge = new MatrixBridge();
    // Replace unrelated startup I/O; start(), both outbox polls, and delivery stay real.
    for (const method of ['replayPendingMatrixDeliveries', 'refreshActingCredentials',
      'pollAgentInvites', 'pollRegistrations', 'startAppserviceIntake', 'connectSSE', 'writeHealthRecord']) {
      bridge[method] = async () => {};
    }
    bridge.startBotSide = async () => { throw new Error('bot password missing'); };
    bridge.getAgentToken = () => null;
    bridge.ensureAgentToken = async () => null;
    const sender = { kind: 'appservice' };
    bridge.agentSenderFor = () => sender;
    const sends = [];
    bridge.sendAsAgentContent = async (...args) => { sends.push(args); return '$sent'; };
    const command = (id) => ({ commandId: id, senderAgentName: 'worker', roomId: '!room:palpo.test',
      threadRootEventId: '$root', body: id, transactionId: id, claimToken: 'claim' });
    const pending = { matrix: command('task-ack'), reply: null };
    const receipts = [];
    const approvalReads = [];
    const unexpected = [];
    const approvalPages = new Map([
      ['/api/approvals/matrix/projections', 'projections'],
      ['/api/approval-bindings/matrix/rooms', 'rooms'],
      ['/api/approval-bindings/matrix/markers', 'markers'],
    ]);
    bridge.callBackendApi = async (method, route, body) => {
      if (method === 'GET' && route === '/api/matrix/direct-agents') return { agents: [] };
      if (method === 'POST' && route === '/api/matrix-work/claim') return { job: null };
      const url = new URL(route, 'http://fixture');
      const field = approvalPages.get(url.pathname);
      if (method === 'GET' && field) {
        expect(url.searchParams.get('limit')).toBe('20');
        expect(body).toBeNull();
        approvalReads.push(url.pathname);
        return { [field]: [] };
      }
      const claim = /^\/api\/router\/(matrix|reply)-outbox\/claim$/.exec(route);
      if (method === 'POST' && claim) {
        const next = pending[claim[1]];
        pending[claim[1]] = null;
        return { command: next };
      }
      if (method === 'POST' && /^\/api\/router\/(matrix|reply)-outbox\/[^/]+\/delivered$/.test(route)) {
        receipts.push({ route, body });
        return { ok: true };
      }
      unexpected.push({ method, route });
      throw new Error(`unexpected fixture route: ${method} ${route}`);
    };

    await bridge.start();
    await bridge._approvalProjectionDrainPromise;
    expect(approvalReads).toEqual([...approvalPages.keys()]);
    expect(unexpected).toEqual([]);
    expect(sends.map((args) => args[2].body)).toEqual(['task-ack']);
    expect(sends[0][0]).toBe(sender);
    expect(sends[0][2]['m.relates_to'].event_id).toBe('$root');
    expect(receipts).toEqual([{ route: '/api/router/matrix-outbox/task-ack/delivered',
      body: { claim_token: 'claim', event_id: '$sent' } }]);
    pending.reply = command('task-result');
    await vi.advanceTimersByTimeAsync(250);
    expect(sends.map((args) => args[2].body)).toEqual(['task-ack', 'task-result']);
    expect(receipts).toEqual([
      { route: '/api/router/matrix-outbox/task-ack/delivered', body: { claim_token: 'claim', event_id: '$sent' } },
      { route: '/api/router/reply-outbox/task-result/delivered', body: { claim_token: 'claim', event_id: '$sent' } },
    ]);
    for (const [index, id] of ['task-ack', 'task-result'].entries()) {
      expect(sends[index][1]).toBe('!room:palpo.test');
      expect(sends[index][2]['m.relates_to'].event_id).toBe('$root');
      expect(sends[index][4]).toEqual({ transactionId: id, throwOnFailure: true });
    }
    // The approval worker remains real and independently polls its three empty
    // canonical pages; those GETs must never enter the router receipt ledger.
    await vi.advanceTimersByTimeAsync(4750);
    await bridge._approvalProjectionDrainPromise;
    expect(approvalReads).toEqual([...approvalPages.keys(), ...approvalPages.keys()]);
    expect(receipts).toHaveLength(2);
    expect(sends).toHaveLength(2);
    expect(unexpected).toEqual([]);
  });
});
