import { afterEach, expect, test, vi } from 'vitest';
import { createServer } from 'node:http';
import { once, EventEmitter } from 'node:events';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { MatrixClient, CryptoClient } from 'matrix-bot-sdk';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';
import { legacyRecord } from './helpers/approval-legacy-fixture.js';
import { ApprovalMatrixPacer } from '../lib/approval-matrix-pacer.js';

const SECRET = 'single-worker-synthetic-bridge';
const TOKEN = 'single-worker-synthetic-agent';
const OWNER = '@owner:test';
const BOT = '@bot:test';
let f;
const respond = (res, status, body) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(body)); };
const gate = () => { let release; let entered; return { wait: new Promise(r => { release = r; }),
  reached: new Promise(r => { entered = r; }), release: (...args) => release(...args), enter: () => entered() }; };
const requestId = name => `approval_${createHash('sha256').update(name).digest('hex').slice(0, 32)}`;

async function fixture({ legacy = false, disk = null, pacing = false } = {}) {
  const originalListeners = new Map(['exit', 'SIGINT', 'SIGTERM'].map(event => [event, new Set(process.rawListeners(event))]));
  vi.resetModules();
  const calls = []; const apiCalls = []; const sockets = new Set(); const historical = new Map();
  const hooks = { matrix: null, backend: null, backendAfter: null, crypto: null };
  const releases = [];
  let store; let serial = 0; let eventSerial = 0;
  const matrix = createServer(async (req, res) => {
    try {
      let raw = ''; for await (const chunk of req) raw += chunk;
      const url = new URL(req.url, 'http://fixture');
      const pieces = url.pathname.split('/').map(decodeURIComponent);
      const room = pieces[pieces.indexOf('rooms') + 1];
      const call = { at: performance.now(), method: req.method, path: url.pathname, room, body: raw ? JSON.parse(raw) : null,
        eventType: pieces[pieces.indexOf('state') + 1], query: url.search };
      calls.push(call);
      if (hooks.matrix && await hooks.matrix(call, req, res)) return;
      if (url.pathname.endsWith('/joined_members')) return respond(res, 200, { joined: { [BOT]: {}, [OWNER]: {} } });
      if (url.pathname.includes('/event/')) {
        const event = historical.get(pieces.at(-1));
        return respond(res, event ? 200 : 404, event || { errcode: 'M_NOT_FOUND' });
      }
      if (req.method === 'GET' && pieces.includes('state')) {
        if (pieces.includes('m.room.encryption')) return respond(res, 200, { algorithm: 'm.megolm.v1.aes-sha2' });
        return respond(res, 404, { errcode: 'M_NOT_FOUND' });
      }
      if (req.method === 'PUT') {
        if (pieces.includes('send')) {
          const row = store.state.projectionOutbox.find(r => r.plan?.transaction_id === pieces.at(-1));
          expect(row?.attemptState).toBe('attempted'); expect(call.body).toEqual(row.plan.prepared_payload);
        } else if (pieces.includes('state')) {
          const row = store.state.markerOutbox.find(r => r.approvalRoomId === room && r.attemptState === 'attempted'
            && r.plan?.prepared_event_type === call.eventType && !r.eventId);
          expect(row).toBeTruthy(); expect(call.body).toEqual(row.plan.prepared_payload);
        }
        return respond(res, 200, { event_id: `$worker-${++eventSerial}` });
      }
      respond(res, 404, { errcode: 'M_NOT_FOUND' });
    } catch (error) { calls.push({ fixtureError: error }); respond(res, 500, { errcode: 'M_FIXTURE_ERROR' }); }
  });
  matrix.on('connection', socket => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); });
  matrix.listen(0, '127.0.0.1'); await once(matrix, 'listening');
  const matrixUrl = `http://127.0.0.1:${matrix.address().port}`;
  const legacyRecords = ['good', 'missing'].map(name => legacyRecord({ id: requestId(name),
    ownerMxid: OWNER, ownerDmRoomId: `!legacy-${name}:test`, projectRoomId: '!p:test',
    matrixEventId: name === 'good' ? '$verdict-good' : null }));
  const context = await createBackendTestContext('single-worker-', {
    agents: Object.fromEntries(['worker', 'claude', 'codex', 'oldprobe'].map(name => [name, { name, kind: 'agent' }])),
    agentTokens: { worker: TOKEN },
    ...(legacy || disk ? { rawDataFiles: { 'approvals.json': disk || JSON.stringify({ version: 1, bindings: {}, audit: [],
      requests: Object.fromEntries(legacyRecords.map(r => [r.id, r])) }) } } : {}),
    env: { MATRIX_BRIDGE_SECRET: SECRET, MATRIX_SERVER_NAME: 'test', MATRIX_BOT_USERNAME: 'bot',
      MATRIX_HOMESERVER: matrixUrl, HAGENCY_API: 'http://127.0.0.1:1', HAGENCY_AGENT_TOKEN_MODE: 'hard',
      HAGENCY_APPROVAL_DM_MODE: 'required', HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST: '' },
  });
  store = context.internals.approvalStoreForTest;
  const backend = createServer(async (req, res) => {
    // Proxy to the actual Express listener; intercept only owned HTTP failure/delay boundaries.
    let raw = ''; for await (const chunk of req) raw += chunk;
    const call = { method: req.method, path: req.url, body: raw ? JSON.parse(raw) : null };
    apiCalls.push(call);
    if (hooks.backend && await hooks.backend(call, req, res)) return;
    const response = await fetch(`http://127.0.0.1:${context.app.address().port}${req.url}`, {
      method: req.method, headers: { 'X-Bridge-Secret': req.headers['x-bridge-secret'] || '',
        'X-Agent-Token': req.headers['x-agent-token'] || '', 'Content-Type': 'application/json' },
      ...(raw ? { body: raw } : {}),
    });
    const result = await response.text();
    if (hooks.backendAfter) await hooks.backendAfter(call, response.status, result);
    res.writeHead(response.status, { 'Content-Type': 'application/json' }); res.end(result);
  });
  backend.on('connection', socket => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); });
  backend.listen(0, '127.0.0.1'); await once(backend, 'listening');
  const backendUrl = `http://127.0.0.1:${backend.address().port}`;
  process.env.HAGENCY_API = backendUrl;
  const mod = await import('../bridge-matrix.js');
  const client = new MatrixClient(matrixUrl, 'owned-bot-token');
  client.crypto = { isReady: true, onRoomEvent: async () => {}, isRoomEncrypted: async () => true,
    encryptRoomEvent: async (room, type, content) => {
      if (hooks.crypto) await hooks.crypto(room, content);
      return { algorithm: 'm.megolm.v1.aes-sha2', ciphertext: Buffer.from(JSON.stringify({ type, content })).toString('base64') };
    },
    decryptRoomEvent: (event, room) => CryptoClient.prototype.decryptRoomEvent.call({ isReady: true,
      engine: { machine: { decryptRoomEvent: async raw => ({ event: Buffer.from(JSON.parse(raw).content.ciphertext, 'base64').toString() }) } } }, event, room),
  };
  const streams = [];
  const bridge = new mod.MatrixBridge({ approvalProjectionIntervalMs: 60_000, eventSourceFactory: () => {
    const stream = new EventEmitter(); stream.close = () => {}; streams.push(stream); return stream;
  } });
  // Existing worker/transport tests isolate their own clocks and body budgets.
  // Dedicated pacing cases leave the production default admission owner intact.
  if (!pacing) bridge._approvalMatrixPacer = new ApprovalMatrixPacer({ gapMs: 0 });
  const state = mod.bridgeStateForTest(); state.botMxid = BOT; state.botCredentialGeneration = 'worker-bot-g1';
  state.agentTokens.worker = { accessToken: 'owned-agent-token', mxid: '@ac_worker:test', homeserver: matrixUrl, credentialGeneration: 'worker-agent-g1' };
  bridge.botClient = client; bridge.botUserId = BOT; bridge.installApprovalBotPublisherReady(client, BOT, 'worker-bot-g1');
  bridge.knownAgents.add('worker'); bridge.knownAgentIndex.set('worker', 'worker');
  bridge._approvalProjectionStopped = false; bridge._approvalProjectionEpoch = 1;
  if (legacy) {
    const r = legacyRecords[0]; const approval = store.getProjectionRequest(r.id);
    const original = mod.buildOwnerApprovalRequest({ ...approval, reusable_scope: null });
    const verdict = { msgtype: 'com.agentchat.approval.verdict.v1', body: 'Approve once',
      'com.agentchat.approval': { version: 1, kind: 'verdict', request_id: r.id, agent: r.agent, project: r.project,
        project_room_id: r.projectRoomId, input_digest: r.inputDigest, action: 'approve_once' },
      'm.relates_to': { 'm.in_reply_to': { event_id: '$original-good' } } };
    for (const [id, sender, content] of [['$verdict-good', OWNER, verdict], ['$original-good', BOT, original]]) {
      historical.set(id, { event_id: id, sender, room_id: r.ownerDmRoomId, type: 'm.room.encrypted',
        content: { ciphertext: Buffer.from(JSON.stringify({ type: 'm.room.message', content })).toString('base64') } });
    }
  }
  const api = async (method, path, body, agent = false) => {
    const response = await fetch(`${backendUrl}${path}`, { method,
      headers: { 'Content-Type': 'application/json', [agent ? 'X-Agent-Token' : 'X-Bridge-Secret']: agent ? TOKEN : SECRET },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    const result = await response.json(); expect(response.status, JSON.stringify(result)).toBeLessThan(300); return result;
  };
  const bind = (agent, project, room = '!shared:test') => api('PUT', '/api/approval-bindings', {
    agent, project, project_room_id: `!${project}:test`, owner_mxid: OWNER, owner_dm_room_id: room, agent_joined: null });
  return { context, mod, bridge, client, state, store, calls, apiCalls, hooks, historical, streams, api, bind,
    hold() { const g = gate(); releases.push(g.release); return g; },
    async native(project = `p${++serial}`, room = `!native-${project}:test`) {
      await bind('worker', project, room);
      return (await api('POST', '/api/approvals', { agent: 'worker', runtime: 'codex', project,
        project_room_id: `!${project}:test`, upstream_request_id: `up-${project}`, tool_name: 'Bash', input_preview: 'PRIVATE_SENTINEL' }, true)).approval;
    },
    async passes(count = 6) { for (let i = 0; i < count; i++) await bridge.wakeApprovalProjectionWorker(); },
    async cleanup() {
      bridge.stopApprovalProjectionWorker(); for (const release of releases) release();
      await bridge._approvalProjectionDrainPromise;
      await bridge._approvalMatrixPacer?.tail;
      for (const socket of sockets) socket.destroy();
      await Promise.all([new Promise(r => matrix.close(r)), new Promise(r => backend.close(r))]);
      await context.cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals();
      for (const [event, original] of originalListeners) {
        for (const listener of process.rawListeners(event)) if (!original.has(listener)) process.removeListener(event, listener);
      }
      expect(calls.filter(c => c.fixtureError).map(c => c.fixtureError.message)).toEqual([]);
    },
  };
}
afterEach(async () => { if (f) { const owned = f; f = null; await owned.cleanup(); } });

test('approval HTTP admission shares the default gap across real native legacy and marker adapters', async () => {
  f = await fixture({ legacy: true, pacing: true });
  const native = await f.native('paced');
  await f.bind('claude', 'paced-marker', '!paced-marker:test');
  await f.passes(3);
  expect(f.store.state.projectionOutbox.some(row => row.requestId === native.id && row.channel === 'private_request' && row.eventId)).toBe(true);
  expect(f.store.state.projectionOutbox.some(row => row.requestId === requestId('good') && row.eventId)).toBe(true);
  expect(f.store.state.markerOutbox.some(row => row.approvalRoomId === '!paced-marker:test' && row.markerChannel === 'room_marker_v2' && row.eventId)).toBe(true);
  expect(f.calls.some(call => call.path.includes('/event/'))).toBe(true);
  expect(f.calls.some(call => call.path.includes('/send/'))).toBe(true);
  expect(f.calls.some(call => call.method === 'PUT' && call.path.includes('/state/'))).toBe(true);
  const gaps = f.calls.slice(1).map((call, index) => call.at - f.calls[index].at);
  // Arrival time may differ slightly from admission time due to socket scheduling.
  expect(Math.min(...gaps)).toBeGreaterThanOrEqual(160);
}, 20_000);

test.each(['native', 'legacy', 'marker'].flatMap(kind => ['stop', 'rotation', 'deadline'].map(cause => ({ kind, cause }))))(
  'approval queued stop rotation and deadline prevent new Matrix I/O ($kind/$cause)', async ({ kind, cause }) => {
    f = await fixture({ legacy: kind === 'legacy', pacing: true });
    let row;
    if (kind === 'marker') {
      await f.bind('claude', 'queued', '!queued:test');
      await f.bridge.syncApprovalRoomMarker({ agent: 'claude', owner_mxid: OWNER, approval_room_id: '!queued:test' });
      row = (await f.api('GET', '/api/approval-bindings/matrix/markers?limit=20')).markers[0];
    } else {
      const id = kind === 'native' ? (await f.native('queued')).id : requestId('good');
      row = (await f.api('GET', '/api/approvals/matrix/projections?limit=20')).projections
        .find(item => item.request_id === id && item.channel === (kind === 'native' ? 'private_request' : 'private_status'));
    }
    expect(row).toBeTruthy();
    await f.bridge.reconcileObservedLegacyMarker({ approval_room_id: '!prime:test' });
    expect(f.calls).toHaveLength(1);
    const waiting = f.hold(); const originalWait = f.bridge._approvalMatrixPacer.wait;
    f.bridge._approvalMatrixPacer.wait = (...args) => { waiting.enter(); return originalWait(...args); };
    if (cause === 'deadline') {
      f.bridge.approvalProjectionSendTimeoutMs = 30;
      f.bridge.approvalMarkerMatrixTimeoutMs = 30;
    }
    const options = { isCurrent: () => !f.bridge._approvalProjectionStopped,
      ...(cause === 'deadline' ? { httpTimeoutMs: 30 } : {}) };
    const pending = (kind === 'native' ? f.bridge.publishApprovalProjectionRow(row, f.bridge._approvalProjectionEpoch)
      : kind === 'legacy' ? f.bridge.publishLegacyApprovalProjection(row, options)
        : f.bridge.publishApprovalMarker(row, options)).catch(error => ({ error }));
    await waiting.reached;
    if (cause === 'stop') f.bridge.stopApprovalProjectionWorker();
    if (cause === 'rotation') f.state.botCredentialGeneration = 'changed-while-queued';
    const result = await pending;
    expect(result.ok).not.toBe(true);
    expect(f.calls).toHaveLength(1);
    expect(f.store.state.projectionOutbox.some(item => item.eventId)).toBe(false);
    expect(f.store.state.markerOutbox.some(item => item.eventId)).toBe(false);
  }, 10_000,
);

test('approval pacing honors a real429 before any later category starts', async () => {
  f = await fixture({ legacy: true, pacing: true }); const native = await f.native('limited');
  await f.bind('claude', 'limited-marker', '!limited-marker:test');
  await f.bridge.syncApprovalRoomMarker({ agent: 'claude', owner_mxid: OWNER, approval_room_id: '!limited-marker:test' });
  f.hooks.matrix = (_call, _req, res) => { respond(res, 429, { errcode: 'M_LIMIT_EXCEEDED', retry_after_ms: 5000 }); return true; };
  await expect(f.bridge.reconcileObservedLegacyMarker({ approval_room_id: '!prime:test' })).rejects.toThrow(/rate limited/);
  const remaining = f.mod.matrixRateLimitGateForTest.cooldownRemainingMs();
  const requests = (await f.api('GET', '/api/approvals/matrix/projections?limit=20')).projections;
  const marker = (await f.api('GET', '/api/approval-bindings/matrix/markers?limit=20')).markers[0];
  await f.bridge.publishApprovalProjectionRow(requests.find(row => row.request_id === native.id && row.channel === 'private_request'), 1).catch(() => {});
  await f.bridge.publishLegacyApprovalProjection(requests.find(row => row.request_id === requestId('good')));
  await f.bridge.publishApprovalMarker(marker);
  expect(f.calls).toHaveLength(1);
  expect(f.mod.matrixRateLimitGateForTest.cooldownRemainingMs()).toBeGreaterThan(4000);
  expect(f.mod.matrixRateLimitGateForTest.cooldownRemainingMs()).toBeLessThanOrEqual(remaining);
  expect(f.store.state.projectionOutbox.some(row => row.eventId)).toBe(false);
  expect(f.store.state.markerOutbox.some(row => row.eventId)).toBe(false);
});

test('approval side security send and marker PUT share actual HTTP admission', async () => {
  f = await fixture({ pacing: true });
  const sides = f.context.internals.projectSideStoreForTest;
  sides.upsertSide({ server_name: 'side.test', api_base_url: f.client.homeserverUrl,
    credential: { kind: 'appservice', asToken: 'side-token', hsToken: 'side-hs-token', senderLocalpart: 'representative', namespace: '@ac_.*' } });
  sides.setRepresentative('side.test', { mxid: '@representative:side.test' });
  sides.observeAccess('side.test', { state: 'accepted' });
  await f.bridge.refreshActingCredentials();
  f.hooks.matrix = (call, _req, res) => {
    if (call.method === 'GET' && call.path.includes('/state/m.room.encryption')) {
      respond(res, 404, { errcode: 'M_NOT_FOUND' }); return true;
    }
    return false;
  };
  const native = await f.native('side-paced', '!native:side.test');
  await f.bind('claude', 'side-marker', '!marker:side.test');
  await f.bridge.syncApprovalRoomMarker({ agent: 'claude', owner_mxid: OWNER, approval_room_id: '!marker:side.test' });
  const row = (await f.api('GET', '/api/approvals/matrix/projections?limit=20')).projections
    .find(item => item.request_id === native.id && item.channel === 'private_request');
  const marker = (await f.api('GET', '/api/approval-bindings/matrix/markers?limit=20')).markers[0];
  const results = await Promise.all([f.bridge.publishApprovalProjectionRow(row, 1), f.bridge.publishApprovalMarker(marker)]);
  expect(results.every(result => result.ok)).toBe(true);
  expect(f.calls.some(call => call.method === 'PUT' && call.path.includes('/send/'))).toBe(true);
  expect(f.calls.some(call => call.method === 'PUT' && call.path.includes('/state/'))).toBe(true);
  expect(f.calls.every(call => new URLSearchParams(call.query).get('user_id') === '@representative:side.test')).toBe(true);
  expect(Math.min(...f.calls.slice(1).map((call, index) => call.at - f.calls[index].at))).toBeGreaterThanOrEqual(160);
});

test('approval owner observation skips unavailable publisher after exact receipt', async () => {
  f = await fixture({ pacing: true }); const created = await f.native('receipt-rotation');
  const row = (await f.api('GET', '/api/approvals/matrix/projections?limit=20')).projections
    .find(item => item.request_id === created.id && item.channel === 'private_request');
  f.hooks.backendAfter = async (call, status) => {
    if (call.path.endsWith('/receipt')) { expect(status).toBe(200); f.state.botCredentialGeneration = 'rotated-after-receipt'; }
  };
  expect(await f.bridge.publishApprovalProjectionRow(row, 1)).toMatchObject({ ok: true });
  expect(f.calls.some(call => call.path.endsWith('/joined_members'))).toBe(false);
  expect(f.store.state.projectionOutbox.find(item => item.requestId === created.id && item.channel === 'private_request').eventId).toBeTruthy();
});

test('single worker publishes four shared bindings as v2 before distinct v1 retirement', async () => {
  f = await fixture();
  for (const [agent, project] of [['oldprobe', 'p1'], ['claude', 'p1'], ['claude', 'p2'], ['codex', 'p2']]) await f.bind(agent, project);
  const before = JSON.stringify(f.store.state.bindings);
  await f.passes();
  const writes = f.calls.filter(c => c.method === 'PUT' && c.path.includes('/state/'));
  expect(writes.map(c => c.eventType)).toEqual(['com.agentchat.approval.room.v2', 'com.agentchat.approval.room.v1']);
  expect(writes[0].body.project_room_associations).toHaveLength(4); expect(writes[1].body).toEqual({});
  expect(JSON.stringify(f.store.state.bindings)).toBe(before);
  expect(f.store.state.markerOutbox.filter(r => r.eventId)).toHaveLength(2);
});

test('single worker routes positive and unresolved legacy rows without actionable resend', async () => {
  f = await fixture({ legacy: true });
  const before = JSON.stringify(f.store.state.requests);
  await f.passes();
  const sends = f.calls.filter(c => c.method === 'PUT' && c.path.includes('/send/'));
  expect(sends).toHaveLength(1);
  const clear = JSON.parse(Buffer.from(sends[0].body.ciphertext, 'base64').toString()).content;
  expect(clear['com.agentchat.approval']).toMatchObject({ migration_kind: 'legacy_v1', state: 'consumed' });
  expect(clear['com.agentchat.approval']).not.toHaveProperty('actions');
  expect(JSON.stringify(f.store.state.requests)).toBe(before);
  expect(f.store.state.projectionOutbox.every(r => r.channel === 'private_status')).toBe(true);
});


test('single worker stop inside prepare crypto and state begin blocks later Matrix IO', async () => {
  for (const stage of ['prepare', 'begin-send']) {
    f = await fixture(); await f.bind('worker', 'stop');
    await f.bridge.syncApprovalRoomMarker({ agent: 'worker', owner_mxid: OWNER, approval_room_id: '!shared:test' });
    f.calls.length = 0;
    f.hooks.backendAfter = async (call, status) => {
      if (call.path === `/api/approval-bindings/matrix/markers/${stage}`) {
        expect(status).toBe(200); f.bridge.stopApprovalProjectionWorker();
      }
    };
    await f.bridge.wakeApprovalProjectionWorker();
    expect(f.calls.filter(c => c.method === 'PUT')).toHaveLength(0);
    await f.cleanup(); f = null;
  }
  f = await fixture({ legacy: true });
  const held = f.hold(); f.hooks.crypto = async () => { held.enter(); await held.wait; };
  const pending = f.bridge.wakeApprovalProjectionWorker(); let settled = false;
  pending.then(() => { settled = true; }); await held.reached;
  f.bridge.stopApprovalProjectionWorker(); await new Promise(r => setTimeout(r, 30));
  expect(settled).toBe(false);
  expect(f.calls.filter(c => c.method === 'PUT')).toHaveLength(0);
  held.release(); await pending;
  expect(f.apiCalls.some(c => c.path.endsWith('/prepare'))).toBe(false);
  await f.cleanup(); f = await fixture({ legacy: true });
  f.hooks.matrix = async call => {
    if (call.path.includes('/event/')) f.bridge.stopApprovalProjectionWorker();
    return false;
  };
  await f.bridge.wakeApprovalProjectionWorker();
  expect(f.calls.filter(c => c.path.includes('/event/'))).toHaveLength(1);
  expect(f.calls.filter(c => c.method === 'PUT')).toHaveLength(0);
  expect(f.apiCalls.some(c => c.method === 'POST' && c.path.endsWith('/legacy-original'))).toBe(false);
});

test('single worker records exact native legacy and marker receipts after stop', async () => {
  for (const kind of ['native', 'legacy', 'marker']) {
    f = await fixture({ legacy: kind === 'legacy' });
    if (kind === 'native') await f.native('receipt');
    if (kind === 'marker') {
      await f.bind('worker', 'receipt');
      await f.bridge.syncApprovalRoomMarker({ agent: 'worker', owner_mxid: OWNER, approval_room_id: '!shared:test' });
    }
    f.hooks.matrix = async (call, _req, res) => {
      if (call.method !== 'PUT') return false;
      f.bridge.stopApprovalProjectionWorker();
      respond(res, 200, { event_id: `$known-${kind}` }); return true;
    };
    await f.bridge.wakeApprovalProjectionWorker();
    const receipts = f.apiCalls.filter(c => c.path.endsWith('/receipt'));
    expect(receipts, kind).toHaveLength(1); expect(receipts[0].body.event_id).toBe(`$known-${kind}`);
    expect(f.calls.filter(c => c.method === 'PUT')).toHaveLength(1);
    const persisted = JSON.parse(readFileSync(`${f.context.runtimeDir}/data/approvals.json`, 'utf8'));
    expect([...(persisted.projectionOutbox || []), ...(persisted.markerOutbox || [])].some(r => r.eventId === `$known-${kind}`)).toBe(true);
    await f.cleanup(); f = null;
  }
});

test('single worker refuses v1 sends and isolates exact room migration failures', async () => {
  f = await fixture();
  for (const [name, publisher] of [['a', '@unavailable:test'], ['b', BOT]]) {
    await f.bind('worker', name, `!${name}:test`);
    await f.api('POST', '/api/approval-bindings/matrix/markers/sync', { agent: 'worker', owner_mxid: OWNER,
      approval_room_id: `!${name}:test`, publisher_mxid: publisher });
    await f.api('DELETE', `/api/approval-bindings/worker/${encodeURIComponent(`!${name}:test`)}`);
  }
  const invalid = structuredClone(Object.values(f.store.state.markerScopes).find(s => s.approvalRoomId === '!a:test'));
  const bindings = JSON.stringify(f.store.state.bindings);
  await f.passes(8);
  const writes = f.calls.filter(c => c.method === 'PUT');
  expect(writes.some(c => c.room === '!b:test' && c.eventType === 'com.agentchat.approval.room.v2')).toBe(true);
  expect(writes.filter(c => c.eventType === 'com.agentchat.approval.room.v1').every(c => JSON.stringify(c.body) === '{}')).toBe(true);
  expect(writes.some(c => c.room === '!a:test')).toBe(false);
  expect(Object.values(f.store.state.markerScopes).find(s => s.approvalRoomId === '!a:test')).toEqual(invalid);
  expect(JSON.stringify(f.store.state.bindings)).toBe(bindings);
});


test('single worker holds two global slots across native legacy inventory and marker jobs', async () => {
  f = await fixture({ legacy: true }); await f.native('slots', '!native:test');
  await f.bind('claude', 'marker', '!marker:test');
  await f.bridge.syncApprovalRoomMarker({ agent: 'claude', owner_mxid: OWNER, approval_room_id: '!marker:test' });
  f.calls.length = 0;
  const gates = [f.hold(), f.hold()]; const entered = new Set();
  f.hooks.matrix = async call => {
    if (!entered.has(call.room) && entered.size < 2) {
      const g = gates[entered.size]; entered.add(call.room); g.enter(); await g.wait;
    }
    return false;
  };
  const run = f.bridge.wakeApprovalProjectionWorker();
  expect(f.bridge.wakeApprovalProjectionWorker()).toBe(run);
  expect(f.bridge.wakeApprovalProjectionWorker()).toBe(run);
  await Promise.all(gates.map(g => g.reached));
  await new Promise(r => setTimeout(r, 25));
  expect(new Set(f.calls.map(c => c.room)).size).toBe(2);
  for (const g of gates) g.release();
  await run; await f.passes(4);
  expect(f.store.state.projectionOutbox.some(r => r.migrationKind === 'legacy_v1' && r.eventId)).toBe(true);
  expect(f.store.state.projectionOutbox.some(r => r.migrationKind === 'native_v2' && r.eventId)).toBe(true);
  expect(f.store.state.markerOutbox.some(r => r.eventId)).toBe(true);
  expect(f.calls.some(c => c.method === 'GET' && c.eventType === 'com.agentchat.approval.room.v1')).toBe(true);
  await f.cleanup(); f = await fixture();
  await f.native('crypto-slot', '!native:test');
  await f.bind('claude', 'room-slot', '!other:test'); await f.bind('codex', 'third-slot', '!third:test');
  const crypto = f.hold(); const observation = f.hold();
  f.hooks.crypto = async () => { crypto.enter(); await crypto.wait; };
  f.hooks.matrix = async call => {
    if (call.room === '!other:test') { observation.enter(); await observation.wait; }
    return false;
  };
  const owned = f.bridge.wakeApprovalProjectionWorker();
  await Promise.all([crypto.reached, observation.reached]);
  await new Promise(r => setTimeout(r, 25));
  expect(f.calls.some(c => c.room === '!third:test')).toBe(false);
  crypto.release(); observation.release(); await owned;
  expect(f.calls.some(c => c.room === '!third:test')).toBe(true);
});

test('single worker serializes shared rooms and request identities without skipping page rows', async () => {
  f = await fixture(); await f.native('shared-one', '!shared:test'); await f.native('shared-two', '!shared:test');
  await f.native('other', '!other:test');
  const held = f.hold(); let first = true;
  f.hooks.matrix = async call => {
    if (call.room === '!shared:test' && first) { first = false; held.enter(); await held.wait; }
    return false;
  };
  const run = f.bridge.wakeApprovalProjectionWorker(); await held.reached;
  await expect.poll(() => f.calls.some(c => c.room === '!other:test'), { timeout: 2000 }).toBe(true);
  expect(f.calls.filter(c => c.room === '!shared:test')).toHaveLength(1);
  held.release(); await run; await f.passes(8);
  const privateRows = f.store.state.projectionOutbox.filter(r => r.channel === 'private_request');
  expect(privateRows).toHaveLength(3); expect(privateRows.every(r => r.eventId)).toBe(true);
  const publicRows = f.store.state.projectionOutbox.filter(r => r.channel === 'public_notice');
  expect(publicRows.every(r => r.eventId)).toBe(true);
  await f.cleanup(); f = await fixture();
  const approval = await f.native('same-request'); await f.bridge.wakeApprovalProjectionWorker();
  await f.api('POST', `/api/approvals/${approval.id}/verdict`, {
    action: 'approve_once', sender_mxid: OWNER, room_id: approval.owner_dm_room_id,
    agent: approval.agent, project: approval.project, project_room_id: approval.project_room_id,
    input_digest: approval.input_digest,
  });
  await f.api('POST', `/api/approvals/${approval.id}/consume`, { agent: 'worker', input_digest: approval.input_digest }, true);
  const due = f.store.listDueProjections().filter(r => r.request_id === approval.id);
  // The real API releases only the earliest unreceipted revision.
  expect(due.map(r => [r.revision, r.channel])).toEqual([[2, 'private_status']]);
  f.bridge._approvalProjectionCursor = null;
  await f.bridge.wakeApprovalProjectionWorker();
  expect(f.store.listDueProjections().filter(r => r.request_id === approval.id).map(r => r.revision)).toEqual([3]);
  await f.passes(4);
  const statuses = f.store.state.projectionOutbox.filter(r => r.requestId === approval.id && r.channel === 'private_status');
  expect(statuses).toHaveLength(2); expect(statuses.every(r => r.eventId)).toBe(true);
});

test('single worker independent opaque cursors reach later work and wrap after insertion', async () => {
  f = await fixture();
  for (let i = 0; i < 25; i++) await f.native(`page-${String(i).padStart(2, '0')}`, `!room-${String(i).padStart(2, '0')}:test`);
  const blocked = new Set(f.store.listDueProjections({ limit: 2 }).map(r => r.target_room_id));
  f.hooks.matrix = async (call, _req, res) => {
    if (blocked.has(call.room)) { respond(res, 503, { errcode: 'M_UNAVAILABLE' }); return true; }
    return false;
  };
  await f.bridge.wakeApprovalProjectionWorker();
  const requestCursor = f.bridge._approvalProjectionCursor;
  const roomCursor = f.bridge._approvalRoomCursor;
  expect(requestCursor).toBeTruthy(); expect(roomCursor).toBeTruthy();
  let fail = true;
  f.hooks.backend = async (call, _req, res) => {
    if (fail && call.path.startsWith('/api/approval-bindings/matrix/rooms?')) {
      fail = false; respond(res, 503, { error: 'fixture page unavailable' }); return true;
    }
    return false;
  };
  await f.bridge.wakeApprovalProjectionWorker();
  expect(f.bridge._approvalRoomCursor).toBe(roomCursor);
  expect(f.bridge._approvalProjectionCursor).not.toBe(requestCursor);
  await f.bind('claude', 'inserted', '!000-inserted:test');
  await f.passes(12);
  expect(f.calls.some(c => c.room === '!000-inserted:test' && c.method === 'PUT')).toBe(true);
  expect(f.store.state.projectionOutbox.filter(r => r.channel === 'private_request' && r.eventId)).toHaveLength(23);
  const roomReads = f.apiCalls.filter(c => c.path.startsWith('/api/approval-bindings/matrix/rooms?'));
  expect(roomReads.some(c => new URL(c.path, 'http://fixture').searchParams.get('after') === roomCursor)).toBe(true);
  expect(f.calls.some(c => c.room === '!room-24:test')).toBe(true);
}, 20000);

test('single worker startup timer SSE and reconnect converge through actual adapters', async () => {
  f = await fixture(); const first = await f.native('startup');
  let timerCallback;
  const originalInterval = globalThis.setInterval;
  vi.spyOn(globalThis, 'setInterval').mockImplementation((callback, ms, ...args) => {
    if (ms === 60000) timerCallback = callback;
    return originalInterval(callback, ms, ...args);
  });
  await f.bridge.startApprovalProjectionWorker();
  const timer = f.bridge._approvalProjectionTimer;
  f.bridge.startApprovalProjectionWorker(); expect(f.bridge._approvalProjectionTimer).toBe(timer);
  expect(f.store.state.projectionOutbox.some(r => r.requestId === first.id && r.eventId)).toBe(true);
  const missed = await f.native('missed'); timerCallback(); await f.bridge._approvalProjectionDrainPromise;
  await f.passes(3);
  expect(f.store.state.projectionOutbox.some(r => r.requestId === missed.id && r.eventId)).toBe(true);
  f.bridge.connectSSE(); await f.bridge._approvalProjectionDrainPromise;
  f.streams[0].emit('approval_requested', JSON.stringify({ request_id: missed.id }));
  await f.bridge._approvalProjectionDrainPromise;
  const originalTimeout = globalThis.setTimeout;
  vi.spyOn(globalThis, 'setTimeout').mockImplementation((callback, ms, ...args) => originalTimeout(callback, ms === 5000 ? 0 : ms, ...args));
  f.streams[0].emit('error', new Error('owned stream closed'));
  await expect.poll(() => f.streams.length, { timeout: 2000 }).toBe(2);
  await f.bridge._approvalProjectionDrainPromise;
  expect(f.store.state.requests[first.id].status).toBe('pending');
  expect(f.apiCalls.some(c => c.path.includes('/matrix/rooms?'))).toBe(true);
});

test('single worker native private failure preserves ciphertext and gates redacted public notice', async () => {
  f = await fixture(); const approval = await f.native('private-failure'); const attempts = [];
  f.hooks.matrix = async (call, _req, res) => {
    if (call.method === 'PUT' && call.path.includes('/send/') && call.room === approval.owner_dm_room_id) {
      attempts.push({ path: call.path, body: call.body });
      if (attempts.length === 1) { respond(res, 502, { errcode: 'M_UNKNOWN' }); return true; }
    }
    return false;
  };
  await f.bridge.wakeApprovalProjectionWorker();
  expect(f.calls.some(c => c.method === 'PUT' && c.room === approval.project_room_id)).toBe(false);
  await f.passes(6);
  expect(attempts).toHaveLength(2); expect(attempts[1]).toEqual(attempts[0]);
  const publicCalls = f.calls.filter(c => c.method === 'PUT' && c.room === approval.project_room_id);
  expect(publicCalls).toHaveLength(1); expect(JSON.stringify(publicCalls[0].body)).not.toContain('PRIVATE_SENTINEL');
  expect(publicCalls[0].body['com.agentchat.approval']).not.toHaveProperty('actions');
  expect(f.store.state.requests[approval.id].status).toBe('pending');
});

test('single worker reconciles a late v1 write beyond room page twenty after reload', async () => {
  f = await fixture();
  for (let i = 0; i < 23; i++) {
    const p = `retained-${String(i).padStart(2, '0')}`; await f.bind('worker', p, `!${p}:test`);
    await f.bridge.syncApprovalRoomMarker({ agent: 'worker', owner_mxid: OWNER, approval_room_id: `!${p}:test` });
  }
  await f.passes(12);
  for (let i = 0; i < 23; i++) f.store.deactivateBinding('worker', `!retained-${String(i).padStart(2, '0')}:test`, 'owned test');
  await f.passes(12);
  expect(f.store.listDueMarkers()).toEqual([]);
  const disk = readFileSync(`${f.context.runtimeDir}/data/approvals.json`, 'utf8');
  await f.cleanup(); f = await fixture({ disk });
  let observed = false;
  f.hooks.matrix = async (call, _req, res) => {
    if (!observed && call.room === '!retained-22:test' && call.method === 'GET' && call.eventType === 'com.agentchat.approval.room.v1') {
      observed = true; respond(res, 200, { version: 1, binding_generation: 8, publisher_mxid: '@oldbot:test', owner_mxid: OWNER,
        agent: 'worker', project_room_associations: [{ project_room_id: '!retained-22:test', active: false }] }); return true;
    }
    return false;
  };
  await f.passes(8);
  expect(observed).toBe(true);
  const writes = f.calls.filter(c => c.method === 'PUT');
  expect(writes).toHaveLength(1); expect(writes[0]).toMatchObject({ room: '!retained-22:test', eventType: 'com.agentchat.approval.room.v1', body: {} });
  expect(f.store.listBindings()).toEqual([]);
}, 20000);

test('single worker preserves uncertain sends and stop recovery without false receipts', async () => {
  f = await fixture(); await f.bind('worker', 'unknown');
  await f.bridge.syncApprovalRoomMarker({ agent: 'worker', owner_mxid: OWNER, approval_room_id: '!shared:test' });
  f.bridge.approvalMarkerMatrixTimeoutMs = 60;
  f.hooks.matrix = async (call, _req, res) => {
    if (call.method !== 'PUT') return false;
    f.bridge.stopApprovalProjectionWorker(); res.writeHead(200, { 'Content-Type': 'application/json' }); res.write('{"event_id":'); return true;
  };
  await f.bridge.wakeApprovalProjectionWorker();
  expect(f.apiCalls.filter(c => c.path.endsWith('/receipt'))).toHaveLength(0);
  expect(f.store.state.markerOutbox.find(r => r.plan)?.attemptState).toBe('attempted');
  f.hooks.matrix = null; f.bridge._approvalProjectionStopped = false;
  await f.passes(6);
  expect(f.store.listDueMarkers()).toEqual([]);
  expect(f.store.state.projectionOutbox).toEqual([]);
  await f.cleanup(); f = await fixture({ legacy: true });
  let receiptFailures = 0;
  f.hooks.backend = async (call, _req, res) => {
    if (call.path.endsWith('/receipt')) {
      receiptFailures++; respond(res, 503, { error: 'owned receipt failure' }); return true;
    }
    return false;
  };
  await f.bridge.wakeApprovalProjectionWorker();
  expect(receiptFailures).toBe(1);
  const row = f.store.state.projectionOutbox.find(r => r.plan);
  expect(row.eventId).toBeFalsy();
  expect(row.plan).toBeTruthy();
  const first = f.calls.find(c => c.method === 'PUT');
  expect(first).toBeTruthy();
  f.bridge.stopApprovalProjectionWorker(); f.hooks.backend = null;
  f.bridge._approvalProjectionStopped = false;
  f.store.now = () => Date.now() + 60_000;
  await f.passes();
  const sends = f.calls.filter(c => c.method === 'PUT');
  expect(sends).toHaveLength(2);
  expect(sends[1].path).toBe(first.path); expect(sends[1].body).toEqual(first.body);
  expect(f.store.state.projectionOutbox.filter(r => r.eventId)).toHaveLength(1);
  expect(f.store.state.projectionOutbox.every(r => r.channel === 'private_status')).toBe(true);
});
