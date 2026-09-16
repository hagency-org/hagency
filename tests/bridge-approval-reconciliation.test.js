import { afterEach, describe, expect, test, vi } from 'vitest';
import { EventEmitter } from 'node:events';
import {
  MatrixBridge, approvalProjectionIoForTest, publishApprovalProjectionForTest,
} from '../bridge-matrix.js';

function worker(overrides = {}) {
  const bridge = Object.assign(Object.create(MatrixBridge.prototype), {
    approvalProjectionIntervalMs: 5_000,
    _approvalProjectionTimer: null,
    _approvalProjectionDrainPromise: null,
    _approvalProjectionWakeQueued: false,
    _approvalProjectionCursor: null,
    _approvalProjectionStopped: false,
    _approvalProjectionEpoch: 0,
    callBackendApi: vi.fn(),
    publishApprovalProjectionRow: vi.fn(async () => ({ ok: true })),
    ...overrides,
  });
  const requestApi = bridge.callBackendApi;
  bridge.callBackendApi = vi.fn((method, url, ...args) => {
    if (url.startsWith('/api/approval-bindings/matrix/rooms?')) return Promise.resolve({ rooms: [] });
    if (url.startsWith('/api/approval-bindings/matrix/markers?')) return Promise.resolve({ markers: [] });
    return requestApi(method, url, ...args);
  });
  return bridge;
}

afterEach(() => vi.useRealTimers());

describe('approval projection request worker', () => {
  test('one page selects at most two requests and rotates the opaque cursor past blocked work', async () => {
    const pages = [
      [{ request_id: 'a', target_room_id: '!a:test', cursor: 'opaque-a1' }, { request_id: 'a', target_room_id: '!a:test', cursor: 'opaque-a2' },
        { request_id: 'b', target_room_id: '!b:test', cursor: 'opaque-b' }],
      [{ request_id: 'c', target_room_id: '!c:test', cursor: 'opaque-c' }],
    ];
    let page = 0;
    let active = 0;
    let maximum = 0;
    const seen = [];
    const bridge = worker({
      callBackendApi: vi.fn(async (_method, url) => {
        if (page === 1) expect(url).toContain(`after=${encodeURIComponent('opaque-b')}`);
        return { projections: pages[page++] || [] };
      }),
      publishApprovalProjectionRow: vi.fn(async row => {
        active += 1;
        maximum = Math.max(maximum, active);
        seen.push(row.request_id);
        await Promise.resolve();
        active -= 1;
        if (row.request_id === 'a') throw new Error('publisher unavailable');
        return { ok: true };
      }),
    });
    await bridge.drainApprovalProjectionsOnce();
    expect(seen).toEqual(['a', 'b']);
    expect(maximum).toBe(2);
    expect(bridge._approvalProjectionCursor).toBe('opaque-b');
    await bridge.drainApprovalProjectionsOnce();
    expect(seen).toEqual(['a', 'b', 'c']);
  });

  test('a full page progresses behind two persistently unavailable requests with concurrency two', async () => {
    const rows = Array.from({ length: 20 }, (_, index) => ({
      request_id: `request-${String(index).padStart(2, '0')}`, target_room_id: `!room-${index}:test`, cursor: `cursor-${index}`,
    }));
    let active = 0;
    let maximum = 0;
    const attempted = [];
    const bridge = worker({
      callBackendApi: vi.fn(async () => ({ projections: rows })),
      publishApprovalProjectionRow: vi.fn(async row => {
        active += 1;
        maximum = Math.max(maximum, active);
        attempted.push(row.request_id);
        await Promise.resolve();
        active -= 1;
        if (row.request_id === 'request-00' || row.request_id === 'request-01') throw new Error('blocked');
        return { ok: true };
      }),
    });
    await bridge.drainApprovalProjectionsOnce();
    expect(attempted).toEqual(rows.map(row => row.request_id));
    expect(maximum).toBe(2);
    expect(bridge._approvalProjectionCursor).toBe('cursor-19');
  });

  test.each(['prepare', 'begin-send'])('stop during %s prevents the later Matrix send', async (pausedStage) => {
    let release;
    let reached;
    const paused = new Promise(resolve => { release = resolve; });
    const atStage = new Promise(resolve => { reached = resolve; });
    const sender = { kind: 'appservice', agentName: 'worker', agentUserId: '@ac_worker:test',
      side: { side: { serverName: 'test' } }, credential: { outboundGeneration: 'g1' } };
    const plan = { cas_token: 'plan-cas', publisher_scope: 'agent:worker:test',
      publisher_mxid: '@ac_worker:test', homeserver: 'test', credential_kind: 'appservice',
      credential_generation: 'g1', prepared_event_type: 'm.room.message',
      prepared_payload: { body: 'fixed' }, transaction_id: 'final-txn' };
    const bridge = worker({ _approvalProjectionEpoch: 1,
      agentSenderFor: () => sender,
      sendAsAgentContent: vi.fn(async () => '$event'),
      callBackendApi: vi.fn(async (_method, url) => {
        if (url.endsWith(`/${pausedStage}`)) {
          reached();
          await paused;
        }
        if (url.endsWith('/prepare')) return { plan };
        if (url.endsWith('/begin-send')) return { plan: { ...plan, attempt_state: 'attempted' } };
        return { ok: true };
      }),
    });
    const row = { request_id: 'approval_stop', revision: 1, channel: 'public_notice', state: 'pending',
      target_room_id: '!project:test', cas_token: 'row-cas', publisher_scope: 'agent:worker:test',
      approval: { agent: 'worker', project: 'p' } };
    const resultPromise = publishApprovalProjectionForTest(row, approvalProjectionIoForTest(bridge, 1));
    await atStage;
    bridge.stopApprovalProjectionWorker();
    release();
    if (pausedStage === 'prepare') await expect(resultPromise).rejects.toThrow(/worker stopped/);
    else await expect(resultPromise).resolves.toMatchObject({ ok: false, uncertain: true });
    expect(bridge.sendAsAgentContent).not.toHaveBeenCalled();
  });

  test('stop after a started send preserves exact receipt handling', async () => {
    const calls = [];
    const bridge = worker({ _approvalProjectionEpoch: 1,
      agentSenderFor: () => ({ kind: 'appservice', agentName: 'worker', agentUserId: '@ac_worker:test',
        side: { side: { serverName: 'test' } }, credential: { outboundGeneration: 'g1' } }),
      sendAsAgentContent: vi.fn(async () => {
        bridge.stopApprovalProjectionWorker();
        return '$already-started';
      }),
      callBackendApi: vi.fn(async (_method, url) => {
        calls.push(url);
        const plan = { cas_token: 'plan-cas', publisher_scope: 'agent:worker:test',
          publisher_mxid: '@ac_worker:test', homeserver: 'test', credential_kind: 'appservice',
          credential_generation: 'g1', prepared_event_type: 'm.room.message',
          prepared_payload: { body: 'fixed' }, transaction_id: 'final-txn' };
        if (url.endsWith('/prepare')) return { plan };
        if (url.endsWith('/begin-send')) return { plan: { ...plan, attempt_state: 'attempted' } };
        return { ok: true };
      }),
    });
    const row = { request_id: 'approval_started', revision: 1, channel: 'public_notice', state: 'pending',
      target_room_id: '!project:test', cas_token: 'row-cas', publisher_scope: 'agent:worker:test',
      approval: { agent: 'worker', project: 'p' } };
    await expect(publishApprovalProjectionForTest(row, approvalProjectionIoForTest(bridge, 1)))
      .resolves.toEqual({ ok: true, event_id: '$already-started' });
    expect(calls.some(url => url.endsWith('/receipt'))).toBe(true);
    expect(calls.some(url => url.endsWith('/retry'))).toBe(false);
  });

  test('duplicate wakes coalesce into one bounded follow-up pass', async () => {
    let release;
    const firstPage = new Promise(resolve => { release = resolve; });
    const bridge = worker({ callBackendApi: vi.fn()
      .mockImplementationOnce(() => firstPage)
      .mockResolvedValueOnce({ projections: [] }) });
    const first = bridge.wakeApprovalProjectionWorker();
    bridge.wakeApprovalProjectionWorker();
    bridge.wakeApprovalProjectionWorker();
    release({ projections: [] });
    await first;
    expect(bridge.callBackendApi).toHaveBeenCalledTimes(6);
  });

  test('timer convergence is nonoverlapping and stop prevents new work', async () => {
    vi.useFakeTimers();
    let release;
    const pending = new Promise(resolve => { release = resolve; });
    const bridge = worker({ _approvalProjectionStopped: true,
      callBackendApi: vi.fn().mockImplementationOnce(() => pending).mockResolvedValue({ projections: [] }) });
    const started = bridge.startApprovalProjectionWorker();
    await vi.advanceTimersByTimeAsync(15_000);
    expect(bridge.callBackendApi).toHaveBeenCalledTimes(1);
    bridge.stopApprovalProjectionWorker();
    release({ projections: [] });
    await started;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(bridge.callBackendApi).toHaveBeenCalledTimes(1);
    expect(bridge._approvalProjectionTimer).toBeNull();
  });

  test('approval events and reconnect only wake the canonical worker', async () => {
    vi.useFakeTimers();
    const streams = [];
    const bridge = worker({
      _eventSourceFactory: vi.fn(() => {
        const stream = new EventEmitter();
        stream.close = vi.fn();
        streams.push(stream);
        return stream;
      }),
      wakeApprovalProjectionWorker: vi.fn(async () => ({})),
    });
    bridge.connectSSE();
    expect(bridge.wakeApprovalProjectionWorker).toHaveBeenCalledTimes(1);
    streams[0].emit('approval_requested', JSON.stringify({ request_id: 'approval_a' }));
    streams[0].emit('approval_changed', JSON.stringify({ request_id: 'approval_a', revision: 2 }));
    await Promise.resolve();
    expect(bridge.wakeApprovalProjectionWorker).toHaveBeenCalledTimes(3);
    streams[0].emit('error', new Error('closed'));
    await vi.advanceTimersByTimeAsync(5_000);
    expect(streams).toHaveLength(2);
    expect(bridge.wakeApprovalProjectionWorker).toHaveBeenCalledTimes(4);
  });

  test('legacy event handler queues without direct delivery or denial', async () => {
    const bridge = worker({ wakeApprovalProjectionWorker: vi.fn(async () => ({})) });
    await expect(bridge.onApprovalRequested({ request_id: 'approval_a' }))
      .resolves.toEqual({ ok: true, requestId: 'approval_a', queued: true });
    expect(bridge.wakeApprovalProjectionWorker).toHaveBeenCalledOnce();
    expect(bridge.callBackendApi).not.toHaveBeenCalled();
  });
});


test('different channels for one selected request are retained and serialized', async () => {
  const rows = ['private_status', 'public_notice'].map((channel, i) => ({ request_id: 'same', revision: 2, channel,
    cas_token: `cas-${i}`, cursor: `cursor-${i}`, target_room_id: `!room-${i}:test` }));
  let active = 0; let maximum = 0; const seen = [];
  const bridge = worker({ callBackendApi: vi.fn(async () => ({ projections: rows })),
    publishApprovalProjectionRow: vi.fn(async row => {
      active++; maximum = Math.max(maximum, active); seen.push(row.channel); await Promise.resolve(); active--; return { ok: true };
    }) });
  await bridge.drainApprovalProjectionsOnce();
  expect(seen).toEqual(['private_status', 'public_notice']); expect(maximum).toBe(1);
  expect(bridge._approvalProjectionCursor).toBe('cursor-1');
});
