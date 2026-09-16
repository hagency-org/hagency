import { afterAll, beforeAll, describe, expect, test, vi } from 'vitest';
import { createServer } from 'node:http';
import { MatrixClient } from 'matrix-bot-sdk';
import request from 'supertest';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';
import { ApprovalMatrixPacer } from '../lib/approval-matrix-pacer.js';
import {
  MatrixBridge, bridgeStateForTest, ourServerNameForTest,
  matrixRateLimitGateForTest,
  approvalProjectionIoForTest, publishApprovalProjectionWithBridgeForTest,
} from '../bridge-matrix.js';

const SECRET = 'adapter-bridge-secret';
const AGENT_TOKEN = 'adapter-agent-token';
const server = ourServerNameForTest().toLowerCase();
let context;
let originalBotState;

function bridgeRequest(method, url, body) {
  const req = request(context.app)[method.toLowerCase()](url).set('X-Bridge-Secret', SECRET);
  return body === undefined ? req : req.send(body);
}

beforeAll(async () => {
  context = await createBackendTestContext('hafleet-projection-adapter-', {
    agents: { worker: { name: 'worker', type: 'agent', kind: 'agent', online: true } },
    agentTokens: { worker: AGENT_TOKEN },
    env: { MATRIX_BRIDGE_SECRET: SECRET, HAGENCY_AGENT_TOKEN_MODE: 'hard',
      MATRIX_SERVER_NAME: server, MATRIX_BOT_USERNAME: 'bot' },
  });
  const state = bridgeStateForTest();
  originalBotState = { botMxid: state.botMxid, botCredentialGeneration: state.botCredentialGeneration };
  state.botMxid = `@bot:${server}`;
  state.botCredentialGeneration = 'adapter-bot-generation';
});

afterAll(async () => {
  const state = bridgeStateForTest();
  state.botMxid = originalBotState.botMxid;
  state.botCredentialGeneration = originalBotState.botCredentialGeneration;
  await context.cleanup();
});

describe('approval projection production request adapter', () => {
  test.each([
    ['appservice', { kind: 'appservice', hsToken: 'hs', asToken: 'as', senderLocalpart: 'hagency',
      namespace: '@ac_.*', url: null, outboundGeneration: 'appservice-generation' }],
    ['registrationToken', { kind: 'registrationToken', registrationToken: 'registration-secret',
      representativeToken: 'representative-secret', outboundGeneration: 'registration-generation' }],
  ])('actual protected endpoint refreshes an accepted %s actor with its generation', async (_kind, credential) => {
    const isolated = await createBackendTestContext(`hafleet-acting-${_kind}-`, {
      env: { MATRIX_BRIDGE_SECRET: SECRET },
      rawRuntimeFiles: { 'data/project-sides.json': JSON.stringify({ version: 1, sides: { [server]: {
        id: server, serverName: server, apiBaseUrl: 'http://127.0.0.1:8008', active: true,
        accessState: 'accepted', createdAt: 1, updatedAt: 1, projects: {}, credential,
        representative: { mxid: `@hagency:${server}`, localpart: 'hagency', observedAt: 1 },
      } }, audit: [] }) },
    });
    try {
      const bridge = Object.assign(Object.create(MatrixBridge.prototype), {
        actingCredentials: new Map(), forgetRoomsOnSides: () => {},
        backendApiForActing: async () => (await request(isolated.app)
          .get('/api/project-sides/acting-credentials').set('X-Bridge-Secret', SECRET)).body,
      });
      await bridge.refreshActingCredentials();
      expect(bridge.actingSideFor(server)).toMatchObject({
        side: { active: true, accessState: 'accepted', representative: { mxid: `@hagency:${server}` } },
        credential: { kind: _kind, outboundGeneration: expect.any(String) },
      });
    } finally {
      await isolated.cleanup();
    }
  });

  test('public agent and botless private representative resolve as distinct canonical actors', async () => {
    const side = { side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: 'https://side.invalid' },
      credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
        outboundGeneration: 'side-generation' } };
    const publicSender = { kind: 'appservice', ...side, agentUserId: `@ac_worker:${server}`, agentName: 'worker' };
    const io = approvalProjectionIoForTest({ actingSideFor: () => side, agentSenderFor: () => publicSender });
    const approval = { agent: 'worker', project: 'adapter' };
    await expect(io.resolveActor({ channel: 'public_notice', target_room_id: `!project:${server}`, approval }))
      .resolves.toMatchObject({ scope: `agent:worker:${server}`, publisher_mxid: `@ac_worker:${server}` });
    await expect(io.resolveActor({ channel: 'private_request', publisher_scope: `side-representative:${server}`,
      target_room_id: `!owner:${server}`, approval }))
      .resolves.toMatchObject({ scope: `side-representative:${server}`, publisher_mxid: `@hagency:${server}` });
  });

  test('side plaintext policy requires a positively absent encryption state', async () => {
    const side = { side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: 'https://side.invalid' },
      credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
        outboundGeneration: 'side-generation' } };
    const io = approvalProjectionIoForTest({ actingSideFor: () => side });
    const row = { request_id: 'approval_side', revision: 1, channel: 'private_request',
      publisher_scope: `side-representative:${server}`, state: 'pending',
      migration_kind: 'native_v2', target_room_id: `!owner:${server}`,
      approval: { agent: 'worker', project: 'adapter', project_room_id: `!project:${server}`,
        owner_mxid: `@owner:${server}`, input_digest: 'b'.repeat(64), expires_at: Date.now() + 1000 } };
    const actor = await io.resolveActor(row);
    vi.stubGlobal('fetch', vi.fn(async () => ({ status: 404, ok: false,
      clone() { return this; }, json: async () => ({ errcode: 'M_NOT_FOUND' }) })));
    await expect(io.prepareContent(row, actor)).resolves.toMatchObject({ event_type: 'm.room.message' });
    expect(fetch.mock.calls[0][0]).toContain(`user_id=${encodeURIComponent(`@hagency:${server}`)}`);
    expect(fetch.mock.calls[0][1].headers.Authorization).toBe('Bearer secret');
    vi.stubGlobal('fetch', vi.fn(async () => ({ status: 404, ok: false,
      clone() { return this; }, json: async () => ({ errcode: 'M_UNRECOGNIZED' }) })));
    await expect(io.prepareContent(row, actor)).rejects.toThrow(/absence was not confirmed/);
    vi.stubGlobal('fetch', vi.fn(async () => ({ status: 200, ok: true,
      clone() { return this; }, json: async () => ({ algorithm: 'm.megolm.v1.aes-sha2' }) })));
    await expect(io.prepareContent(row, actor)).rejects.toThrow(/encrypted project-side/);
  });

  test('registration representative security uses its verified token without masquerade', async () => {
    const side = { side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: 'https://side.invalid',
      representative: { mxid: `@representative:${server}` } },
    credential: { kind: 'registrationToken', representativeToken: 'representative-secret',
      representativeMxid: `@representative:${server}`, outboundGeneration: 'registration-generation' } };
    const io = approvalProjectionIoForTest({ actingSideFor: () => side });
    const row = { request_id: 'approval_registration', revision: 1, channel: 'private_request',
      publisher_scope: `side-representative:${server}`, state: 'pending',
      migration_kind: 'native_v2', target_room_id: `!owner:${server}`,
      approval: { agent: 'worker', project: 'adapter', expires_at: Date.now() + 1000 } };
    vi.stubGlobal('fetch', vi.fn(async () => ({ status: 404, ok: false,
      clone() { return this; }, json: async () => ({ errcode: 'M_NOT_FOUND' }) })));
    await expect(io.prepareContent(row, await io.resolveActor(row))).resolves.toBeTruthy();
    expect(fetch.mock.calls[0][0]).not.toContain('user_id=');
    expect(fetch.mock.calls[0][1].headers.Authorization).toBe('Bearer representative-secret');
  });

  test('side security lookup aborts a real stalled socket within its owned deadline', async () => {
    const socket = createServer((_req, _res) => {});
    await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve));
    const address = socket.address();
    const side = { side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: `http://127.0.0.1:${address.port}` },
      credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
        outboundGeneration: 'side-generation' } };
    const bridge = { actingSideFor: () => side, approvalProjectionSecurityTimeoutMs: 25 };
    const io = approvalProjectionIoForTest(bridge);
    const row = { request_id: 'approval_timeout', revision: 1, channel: 'private_request',
      publisher_scope: `side-representative:${server}`, state: 'pending',
      migration_kind: 'native_v2', target_room_id: `!owner:${server}`,
      approval: { agent: 'worker', project: 'adapter', expires_at: Date.now() + 1000 } };
    vi.unstubAllGlobals();
    try {
      await expect(io.prepareContent(row, await io.resolveActor(row))).rejects.toMatchObject({ name: 'AbortError' });
    } finally {
      await new Promise(resolve => socket.close(resolve));
    }
  });

  test.each([
    ['fragmented valid body', 404, 20, false, true],
    ['delayed absence body', 404, 700, false, false],
    ['continuous partial body', 404, 700, true, false],
    ['delayed rate limit body', 429, 700, false, false],
    ['unconsumed encrypted body', 200, 700, true, false],
    ['unconsumed server error body', 503, 700, true, false],
  ])('side security bounds the complete response: %s', async (_name, status, delay, trickle, accepted) => {
    let requests = 0;
    let closed = false;
    const timers = [];
    const socket = createServer((_req, res) => {
      requests += 1;
      res.once('close', () => { closed = true; });
      res.writeHead(status, { 'Content-Type': 'application/json' });
      res.flushHeaders();
      res.write('{');
      if (trickle) timers.push(setInterval(() => res.write(' '), 10));
      timers.push(setTimeout(() => res.end(status === 429
        ? '"errcode":"M_LIMIT_EXCEEDED","retry_after_ms":1}'
        : '"errcode":"M_NOT_FOUND"}'), delay));
    });
    await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve));
    const side = { side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: `http://127.0.0.1:${socket.address().port}` },
      credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
        outboundGeneration: 'side-generation' } };
    const io = approvalProjectionIoForTest({ actingSideFor: () => side,
      approvalProjectionSecurityTimeoutMs: accepted ? 1000 : 100 });
    const row = { request_id: 'approval_body_deadline', revision: 1, channel: 'private_request',
      publisher_scope: `side-representative:${server}`,
      state: 'pending', migration_kind: 'native_v2', target_room_id: `!owner:${server}`,
      approval: { agent: 'worker', project: 'adapter', expires_at: Date.now() + 1000 } };
    vi.unstubAllGlobals();
    matrixRateLimitGateForTest.reset();
    try {
      const actor = await io.resolveActor(row);
      const started = performance.now();
      if (accepted) {
        await expect(io.prepareContent(row, actor)).resolves.toMatchObject({ event_type: 'm.room.message' });
      } else {
        await expect(io.prepareContent(row, actor)).rejects.toThrow();
        expect(performance.now() - started).toBeLessThan(500);
      }
      // Observe server-side closure before fixture cleanup can close the socket.
      await vi.waitFor(() => expect(closed).toBe(true), { timeout: 400, interval: 10 });
      expect(requests).toBe(1);
    } finally {
      for (const timer of timers) clearTimeout(timer);
      socket.closeAllConnections();
      await new Promise(resolve => socket.close(resolve));
      matrixRateLimitGateForTest.reset();
    }
  });

  test('local private preparation fails closed when crypto readiness disappears', async () => {
    const bridge = { botClient: { crypto: null }, botUserId: `@bot:${server}`, approvalDmMode: 'encrypted',
      actingSideFor: () => null, ensureApprovalDmSecurity: vi.fn(async () => {}) };
    bridge.approvalBotPublisherReady = { client: bridge.botClient, mxid: bridge.botUserId,
      credentialGeneration: 'adapter-bot-generation' };
    const io = approvalProjectionIoForTest(bridge);
    const row = { channel: 'private_request', publisher_scope: 'local_bot', target_room_id: `!owner:${server}`,
      approval: { id: 'approval_x', agent: 'worker', project: 'adapter', expires_at: Date.now() + 1000 } };
    const actor = await io.resolveActor(row);
    await expect(io.prepareContent(row, actor)).rejects.toThrow(/encryption is unavailable/);
  });

  test('saved bot fields alone are not publisher readiness and selected status state wins', async () => {
    const client = { crypto: {} };
    const bridge = { botClient: client, botUserId: `@bot:${server}`, actingSideFor: () => null,
      approvalDmMode: 'plaintext-test', ensureApprovalDmSecurity: vi.fn(async () => {}) };
    let io = approvalProjectionIoForTest(bridge);
    const row = { request_id: 'approval_0123456789abcdef0123456789abcdef', revision: 2, channel: 'private_status', state: 'approved',
      migration_kind: 'native_v2', target_room_id: `!owner:${server}`,
      publisher: { scope: 'local_bot' },
      approval: { id: 'approval_0123456789abcdef0123456789abcdef', agent: 'worker', project: 'adapter',
        project_room_id: `!project:${server}`, owner_mxid: `@owner:${server}`,
        input_digest: 'a'.repeat(64), status: 'consumed', decision: 'allow' } };
    await expect(io.resolveActor(row)).resolves.toBeNull();
    bridge.approvalBotPublisherReady = { client, mxid: bridge.botUserId,
      credentialGeneration: 'adapter-bot-generation' };
    io = approvalProjectionIoForTest(bridge);
    const actor = await io.resolveActor(row);
    const prepared = await io.prepareContent(row, actor);
    expect(prepared.content['com.agentchat.approval']).toMatchObject({
      version: 1, request_id: row.request_id, revision: 2, state: 'approved', decision: 'allow',
      migration_kind: 'native_v2', agent: 'worker', project: 'adapter',
      project_room_id: `!project:${server}`, owner_mxid: `@owner:${server}`,
      publisher_mxid: `@bot:${server}`, input_digest: 'a'.repeat(64),
    });
  });

  test('bot readiness installation binds the exact verified client and clears on replacement failure', () => {
    const verified = { crypto: {} };
    const bridge = { botClient: verified, botUserId: `@bot:${server}`, approvalBotPublisherReady: null,
      clearApprovalBotPublisherReady: MatrixBridge.prototype.clearApprovalBotPublisherReady };
    MatrixBridge.prototype.installApprovalBotPublisherReady.call(
      bridge, verified, bridge.botUserId, 'adapter-bot-generation',
    );
    expect(bridge.approvalBotPublisherReady).toEqual({ client: verified, mxid: bridge.botUserId,
      credentialGeneration: 'adapter-bot-generation' });
    bridge.botClient = { crypto: {} };
    expect(() => MatrixBridge.prototype.installApprovalBotPublisherReady.call(
      bridge, verified, bridge.botUserId, 'adapter-bot-generation',
    )).toThrow(/verified.*context/);
    expect(bridge.approvalBotPublisherReady).toBeNull();
  });

  test('stored local plaintext is security-checked and refused before raw replay', async () => {
    const client = { crypto: {}, doRequest: vi.fn() };
    const bridge = { botClient: client, botUserId: `@bot:${server}`, approvalDmMode: 'encrypted',
      actingSideFor: () => null, ensureApprovalDmSecurity: vi.fn(async () => {}) };
    bridge.approvalBotPublisherReady = { client, mxid: bridge.botUserId,
      credentialGeneration: 'adapter-bot-generation' };
    const io = approvalProjectionIoForTest(bridge);
    const row = { request_id: 'approval_plain', revision: 1, channel: 'private_request', publisher_scope: 'local_bot',
      target_room_id: `!owner:${server}`, approval: { agent: 'worker' } };
    const actor = await io.resolveActor(row);
    await expect(io.send({ prepared_event_type: 'm.room.message', prepared_payload: { body: 'old' },
      transaction_id: 'stored_plain' }, actor, row)).rejects.toThrow(/stored plaintext/);
    expect(bridge.ensureApprovalDmSecurity).toHaveBeenCalledOnce();
    expect(client.doRequest).not.toHaveBeenCalled();
  });

  test('real store/API prepares encrypted bytes once, begins before exact raw PUT, and receipts', async () => {
    await bridgeRequest('put', '/api/approval-bindings', {
      agent: 'worker', project: 'adapter', project_room_id: `!project:${server}`,
      owner_mxid: `@owner:${server}`, owner_dm_room_id: `!owner:${server}`,
    });
    const created = await request(context.app).post('/api/approvals').set('X-Agent-Token', AGENT_TOKEN).send({
      agent: 'worker', runtime: 'codex', project: 'adapter', project_room_id: `!project:${server}`,
      upstream_request_id: 'adapter-native', tool_name: 'Bash', input_preview: 'pwd',
    });
    expect(created.status).toBe(201);
    const due = await bridgeRequest('get', '/api/approvals/matrix/projections?limit=20');
    const row = due.body.projections.find(item => item.request_id === created.body.approval.id
      && item.channel === 'private_request');
    const order = [];
    const encryptRoomEvent = vi.fn(async (_room, _type, content) => {
      order.push('encrypt');
      return { algorithm: 'm.megolm.v1.aes-sha2', ciphertext: JSON.stringify(content) };
    });
    let matrixAttempts = 0;
    const doRequest = vi.fn(async (method, path, _query, content) => {
      if (method === 'GET') {
        expect(path).toContain('/state/m.room.encryption/');
        return { algorithm: 'm.megolm.v1.aes-sha2' };
      }
      order.push('matrix');
      matrixAttempts += 1;
      expect(method).toBe('PUT');
      expect(path).toContain('/send/m.room.encrypted/');
      expect(content).toEqual(expect.objectContaining({ ciphertext: expect.any(String) }));
      const stored = (await bridgeRequest('get', '/api/approvals/matrix/projections?limit=20')).body.projections
        .find(item => item.request_id === row.request_id && item.channel === row.channel);
      expect(stored.plan.attempt_state).toBe('attempted');
      expect(path.endsWith(`/${stored.plan.transaction_id}`)).toBe(true);
      expect(content).toEqual(stored.plan.prepared_payload);
      const canonical = JSON.parse(content.ciphertext)['com.agentchat.approval'];
      expect(canonical).toMatchObject({ version: 1, request_id: row.request_id, revision: 1,
        state: 'pending', migration_kind: 'native_v2', owner_mxid: `@owner:${server}`,
        publisher_mxid: `@bot:${server}`, project_room_id: `!project:${server}`,
        input_digest: expect.stringMatching(/^[a-f0-9]{64}$/) });
      if (matrixAttempts === 1) throw Object.assign(new Error('connection reset'), { code: 'timeout' });
      return { event_id: '$adapter-event' };
    });
    const bridge = {
      _approvalMatrixPacer: new ApprovalMatrixPacer({ gapMs: 0 }),
      botClient: {
        getRoomStateEvent: vi.fn(async () => ({ algorithm: 'm.megolm.v1.aes-sha2' })),
        crypto: { onRoomEvent: vi.fn(async () => {}), isRoomEncrypted: vi.fn(async () => true), encryptRoomEvent },
        doRequest,
      },
      botUserId: `@bot:${server}`,
      approvalDmMode: 'encrypted',
      // Both contexts exist. The protected due row's backend-owned scope must select local_bot.
      actingSideFor: () => ({ side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: 'https://side.invalid' },
        credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'side-secret',
          outboundGeneration: 'side-generation' } }),
      ensureApprovalDmEncrypted: MatrixBridge.prototype.ensureApprovalDmEncrypted,
      ensureApprovalDmSecurity: MatrixBridge.prototype.ensureApprovalDmSecurity,
      callBackendApi: async (method, url, body) => {
        order.push(url.endsWith('/prepare') ? 'prepare' : url.endsWith('/begin-send') ? 'begin'
          : url.endsWith('/receipt') ? 'receipt' : 'api');
        const response = await bridgeRequest(method, url, body);
        if (response.status >= 400) throw new Error(`backend ${response.status}: ${JSON.stringify(response.body)}`);
        return response.body;
      },
    };
    bridge.approvalBotPublisherReady = { client: bridge.botClient, mxid: bridge.botUserId,
      credentialGeneration: 'adapter-bot-generation' };
    const first = await publishApprovalProjectionWithBridgeForTest(bridge, row);
    expect(first).toMatchObject({ ok: false, uncertain: true });
    expect(encryptRoomEvent).toHaveBeenCalledTimes(1);
    expect(order.indexOf('prepare')).toBeLessThan(order.indexOf('begin'));
    expect(order.indexOf('begin')).toBeLessThan(order.indexOf('matrix'));
    const retryRows = (await bridgeRequest('get', '/api/approvals/matrix/projections?limit=20')).body.projections;
    const retryRow = retryRows.find(item => item.request_id === row.request_id && item.channel === row.channel);
    expect(retryRow.plan.attempt_state).toBe('uncertain');
    bridge.botClient.crypto.encryptRoomEvent = vi.fn(async () => { throw new Error('must not re-encrypt'); });
    const result = await publishApprovalProjectionWithBridgeForTest(bridge, retryRow);
    expect(result).toEqual({ ok: true, event_id: '$adapter-event' });
    expect(encryptRoomEvent).toHaveBeenCalledTimes(1);
    const sends = doRequest.mock.calls.filter(([method]) => method === 'PUT');
    expect(sends).toHaveLength(2);
    expect(doRequest.mock.calls.filter(([method]) => method === 'GET')).toHaveLength(3);
    expect(sends[1][3]).toEqual(sends[0][3]);
    expect(sends[1][1]).toBe(sends[0][1]);
    expect(order.lastIndexOf('matrix')).toBeLessThan(order.indexOf('receipt'));
    const after = await bridgeRequest('get', '/api/approvals/matrix/projections?limit=20');
    expect(after.body.projections.some(item => item.request_id === row.request_id
      && item.channel === row.channel)).toBe(false);
  });

  test('production worker drains a real canonical row through the adapter', async () => {
    vi.unstubAllGlobals();
    const matrixCalls = [];
    const socket = createServer(async (req, res) => {
      let raw = ''; for await (const chunk of req) raw += chunk;
      matrixCalls.push({ method: req.method, path: req.url, body: raw ? JSON.parse(raw) : null });
      const missing = req.method === 'GET' && !req.url.endsWith('/joined_members');
      res.writeHead(missing ? 404 : 200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(missing ? { errcode: 'M_NOT_FOUND' }
        : req.method === 'PUT' ? { event_id: '$worker-event' }
          : { joined: { [`@owner:${server}`]: {}, [`@bot:${server}`]: {} } }));
    });
    await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve));
    const matrixUrl = `http://127.0.0.1:${socket.address().port}`;
    const isolated = await createBackendTestContext('hafleet-worker-isolated-', {
      agents: { worker: { name: 'worker', type: 'agent', kind: 'agent', online: true } },
      agentTokens: { worker: AGENT_TOKEN },
      env: { MATRIX_BRIDGE_SECRET: SECRET, HAGENCY_AGENT_TOKEN_MODE: 'hard',
        MATRIX_SERVER_NAME: server, MATRIX_BOT_USERNAME: 'bot', MATRIX_HOMESERVER: matrixUrl },
    });
    const api = (method, url, body) => {
      const pending = request(isolated.app)[method.toLowerCase()](url).set('X-Bridge-Secret', SECRET);
      return body === undefined ? pending : pending.send(body);
    };
    let bridge;
    const warnings = vi.spyOn(console, 'warn');
    try {
      await api('put', '/api/approval-bindings', {
        agent: 'worker', project: 'adapter-worker', project_room_id: `!worker-project:${server}`,
        owner_mxid: `@owner:${server}`, owner_dm_room_id: `!worker-owner:${server}`,
      });
      const created = await request(isolated.app).post('/api/approvals').set('X-Agent-Token', AGENT_TOKEN).send({
        agent: 'worker', runtime: 'codex', project: 'adapter-worker', project_room_id: `!worker-project:${server}`,
        upstream_request_id: 'adapter-worker-native', tool_name: 'Bash', input_preview: 'pwd',
      });
      expect(created.status).toBe(201);
      vi.resetModules();
      const owned = await import('../bridge-matrix.js');
      const client = new MatrixClient(matrixUrl, 'owned-worker-test-token');
      client.crypto = {};
      const send = vi.spyOn(client, 'doRequest');
      const state = owned.bridgeStateForTest();
      state.botMxid = `@bot:${server}`; state.botCredentialGeneration = 'isolated-worker-generation';
      bridge = new owned.MatrixBridge();
      bridge.botClient = client; bridge.botUserId = state.botMxid; bridge.approvalDmMode = 'plaintext-test';
      bridge.installApprovalBotPublisherReady(client, state.botMxid, state.botCredentialGeneration);
      bridge._approvalProjectionStopped = false; bridge._approvalProjectionEpoch = 1;
      bridge.callBackendApi = async (method, url, body) => {
        const response = await api(method, url, body);
        if (response.status >= 400) throw new Error(`backend ${response.status}`);
        return response.body;
      };
      const ownerCheck = vi.spyOn(bridge, 'warnIfOwnerCannotSeeApprovalRoom');
      warnings.mockClear();
      const result = await bridge.drainApprovalProjectionsOnce();
      expect(result.selected).toBe(2); // This request plus its inventoried owner room, no other test's rows.
      expect(result.settled).toHaveLength(2);
      for (const outcome of result.settled) {
        expect(outcome.status).toBe('fulfilled');
        expect(outcome.value).not.toHaveProperty('unresolved', true);
        expect(outcome.value).not.toHaveProperty('error');
      }
      const inventory = result.settled.find(outcome => Object.hasOwn(outcome.value || {}, 'synchronization')).value;
      expect(inventory.synchronization).toMatchObject({ ok: true, marker: { approval_room_id: `!worker-owner:${server}` } });
      expect(inventory.reconciliation).toEqual({ observed_nonempty: false, reconciliation: null });
      expect(send).toHaveBeenCalled();
      expect(ownerCheck).toHaveBeenCalledWith(expect.objectContaining({ owner_mxid: `@owner:${server}` }), server,
        expect.objectContaining({ request: expect.any(Function), fetchImpl: expect.any(Function) }));
      expect(matrixCalls.filter(call => call.path.endsWith('/joined_members'))).toHaveLength(1);
      expect(matrixCalls.filter(call => call.method === 'PUT')).toHaveLength(1);
      expect(warnings).not.toHaveBeenCalled();
      const remaining = (await api('get', '/api/approvals/matrix/projections?limit=200')).body.projections;
      expect(remaining.some(row => row.request_id === created.body.approval.id && row.channel === 'private_request')).toBe(false);
    } finally {
      bridge?.stopApprovalProjectionWorker(); warnings.mockRestore();
      socket.closeAllConnections(); await new Promise(resolve => socket.close(resolve));
      await isolated.cleanup(); vi.unstubAllGlobals();
    }
  });

  test('failed private publication keeps public notice ineligible and decision pending', async () => {
    const isolated = await createBackendTestContext('hafleet-private-order-', {
      agents: { worker: { name: 'worker', type: 'agent', kind: 'agent', online: true } },
      agentTokens: { worker: AGENT_TOKEN },
      env: { MATRIX_BRIDGE_SECRET: SECRET, HAGENCY_AGENT_TOKEN_MODE: 'hard',
        MATRIX_SERVER_NAME: server, MATRIX_BOT_USERNAME: 'bot' },
    });
    const api = (method, url, body) => {
      const pending = request(isolated.app)[method.toLowerCase()](url).set('X-Bridge-Secret', SECRET);
      return body === undefined ? pending : pending.send(body);
    };
    try {
      await api('put', '/api/approval-bindings', { agent: 'worker', project: 'ordered',
        project_room_id: `!ordered:${server}`, owner_mxid: `@owner:${server}`,
        owner_dm_room_id: `!ordered-owner:${server}` });
      const created = await request(isolated.app).post('/api/approvals').set('X-Agent-Token', AGENT_TOKEN).send({
        agent: 'worker', runtime: 'codex', project: 'ordered', project_room_id: `!ordered:${server}`,
        upstream_request_id: 'ordered-private', tool_name: 'Bash', input_preview: 'secret command',
      });
      const bridge = Object.assign(Object.create(MatrixBridge.prototype), {
        botClient: { crypto: {} }, botUserId: `@bot:${server}`, approvalDmMode: 'required',
        approvalBotPublisherReady: { client: null, mxid: `@bot:${server}`,
          credentialGeneration: 'adapter-bot-generation' },
        actingSideFor: () => null,
        ensureApprovalDmSecurity: vi.fn(async () => { throw new Error('private unavailable'); }),
        _approvalProjectionStopped: false, _approvalProjectionEpoch: 1, _approvalProjectionCursor: null,
        callBackendApi: async (method, url, body) => {
          const response = await api(method, url, body);
          if (response.status >= 400) throw new Error(`backend ${response.status}`);
          return response.body;
        },
      });
      bridge.approvalBotPublisherReady.client = bridge.botClient;
      const publicSend = vi.spyOn(bridge, 'sendAsAgentContent');
      for (let tick = 0; tick < 4; tick += 1) await bridge.drainApprovalProjectionsOnce();
      expect(publicSend).not.toHaveBeenCalled();
      const current = await request(isolated.app).get(`/api/approvals/${created.body.approval.id}`)
        .set('X-Agent-Token', AGENT_TOKEN);
      expect(current.body.approval.status).toBe('pending');
    } finally {
      await isolated.cleanup();
    }
  });

  test('side security lookup refuses a 302 without contacting the redirect target', async () => {
    let redirected = 0;
    const target = createServer((_req, res) => { redirected += 1; res.end('{}'); });
    await new Promise(resolve => target.listen(0, '127.0.0.1', resolve));
    const source = createServer((_req, res) => {
      res.writeHead(302, { Location: `http://127.0.0.1:${target.address().port}/stolen` });
      res.end();
    });
    await new Promise(resolve => source.listen(0, '127.0.0.1', resolve));
    const side = { side: { serverName: server, active: true, accessState: 'accepted',
      apiBaseUrl: `http://127.0.0.1:${source.address().port}` },
    credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
      outboundGeneration: 'side-generation' } };
    const row = { request_id: 'approval_security_redirect', revision: 1, channel: 'private_request',
      publisher_scope: `side-representative:${server}`, target_room_id: `!owner:${server}`,
      state: 'pending', migration_kind: 'native_v2', approval: { agent: 'worker', project: 'adapter' } };
    try {
      const io = approvalProjectionIoForTest({ actingSideFor: () => side });
      await expect(io.prepareContent(row, await io.resolveActor(row))).rejects.toThrow();
      expect(redirected).toBe(0);
    } finally {
      source.closeAllConnections(); target.closeAllConnections();
      await Promise.all([new Promise(resolve => source.close(resolve)), new Promise(resolve => target.close(resolve))]);
    }
  });

  test('private final PUT refuses a 307 without exposing its body to the redirect target', async () => {
    let redirected = 0;
    let leaked = '';
    const target = createServer((req, res) => {
      redirected += 1;
      req.on('data', chunk => { leaked += chunk; });
      req.on('end', () => res.end('{"event_id":"$bad"}'));
    });
    await new Promise(resolve => target.listen(0, '127.0.0.1', resolve));
    const source = createServer((_req, res) => {
      res.writeHead(307, { Location: `http://127.0.0.1:${target.address().port}/stolen` });
      res.end();
    });
    await new Promise(resolve => source.listen(0, '127.0.0.1', resolve));
    const side = { side: { serverName: server, active: true, accessState: 'accepted',
      apiBaseUrl: `http://127.0.0.1:${source.address().port}` },
    credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
      outboundGeneration: 'side-generation', namespace: '@ac_.*' } };
    const row = { request_id: 'approval_send_redirect', revision: 1, channel: 'private_request',
      publisher_scope: `side-representative:${server}`, target_room_id: `!owner:${server}`,
      state: 'pending', migration_kind: 'native_v2', approval: { agent: 'worker', project: 'adapter' } };
    try {
      const bridge = { actingSideFor: () => side, approvalProjectionSendTimeoutMs: 100 };
      const io = approvalProjectionIoForTest(bridge);
      const actor = await io.resolveActor(row);
      await expect(io.send({ publisher_mxid: actor.publisher_mxid,
        credential_generation: actor.credential_generation, prepared_event_type: 'm.room.encrypted',
        prepared_payload: { ciphertext: 'private-secret' }, transaction_id: 'redirect-final' }, actor, row))
        .rejects.toThrow();
      expect(redirected).toBe(0);
      expect(leaked).toBe('');
    } finally {
      source.closeAllConnections(); target.closeAllConnections();
      await Promise.all([new Promise(resolve => source.close(resolve)), new Promise(resolve => target.close(resolve))]);
    }
  });

  test('side security 429 performs one bounded request and defers publication', async () => {
    const side = { side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: 'https://side.invalid' },
      credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
        outboundGeneration: 'side-generation' } };
    const io = approvalProjectionIoForTest({ actingSideFor: () => side });
    const row = { request_id: 'approval_limited', revision: 1, channel: 'private_request',
      publisher_scope: `side-representative:${server}`, state: 'pending',
      migration_kind: 'native_v2', target_room_id: `!owner:${server}`,
      approval: { agent: 'worker', project: 'adapter', expires_at: Date.now() + 1000 } };
    vi.stubGlobal('fetch', vi.fn(async () => ({ status: 429, ok: false,
      clone: () => ({ json: async () => ({ errcode: 'M_LIMIT_EXCEEDED', retry_after_ms: 1 }) }) })));
    await expect(io.prepareContent(row, await io.resolveActor(row))).rejects.toThrow(/rate limited/);
    expect(fetch).toHaveBeenCalledOnce();
    vi.unstubAllGlobals();
    matrixRateLimitGateForTest.reset();
  });

  test.each(['side-representative', 'public-agent'])(
    '%s final PUT aborts a stalled response body within the owned deadline', async (kind) => {
      vi.unstubAllGlobals();
      matrixRateLimitGateForTest.reset();
      let puts = 0;
      const socket = createServer((req, res) => {
        if (req.url.includes('/state/m.room.encryption/')) {
          res.writeHead(404, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify({ errcode: 'M_NOT_FOUND' }));
          return;
        }
        puts += 1;
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.flushHeaders();
      });
      await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve));
      const address = socket.address();
      const side = { side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: `http://127.0.0.1:${address.port}` },
        credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
          outboundGeneration: 'side-generation', namespace: '@ac_.*' } };
      const sender = { kind: 'appservice', ...side, agentUserId: `@ac_worker:${server}`, agentName: 'worker' };
      const bridge = Object.assign(Object.create(MatrixBridge.prototype), {
        approvalProjectionSendTimeoutMs: 25,
        // Isolate the response-body timeout; default admission has its own
        // real cross-category and queued-deadline integration tests.
        _approvalMatrixPacer: new ApprovalMatrixPacer({ gapMs: 0 }),
        actingSideFor: () => side,
        agentSenderFor: () => sender,
        isKnownAgentMxid: () => true,
        postWarning: vi.fn(),
        endAgentWork: vi.fn(),
      });
      const row = { request_id: 'approval_stalled', revision: 1,
        channel: kind === 'side-representative' ? 'private_request' : 'public_notice',
        publisher_scope: kind === 'side-representative' ? `side-representative:${server}` : `agent:worker:${server}`,
        target_room_id: `!room:${server}`, state: 'pending', migration_kind: 'native_v2',
        approval: { agent: 'worker', project: 'adapter', project_room_id: `!room:${server}` } };
      const actor = await approvalProjectionIoForTest(bridge).resolveActor(row);
      const plan = { publisher_mxid: actor.publisher_mxid,
        credential_generation: actor.credential_generation, prepared_event_type: 'm.room.message',
        prepared_payload: { body: 'fixed' }, transaction_id: 'final-stalled' };
      try {
        await expect(approvalProjectionIoForTest(bridge).send(plan, actor, row))
          .rejects.toThrow(/abort/i);
        expect(puts).toBe(1);
      } finally {
        socket.closeAllConnections();
        await new Promise(resolve => socket.close(resolve));
      }
    },
  );

  test.each(['side-representative', 'public-agent'])(
    '%s final PUT accepts a complete response before cleanup aborts the stream', async (kind) => {
      vi.unstubAllGlobals();
      matrixRateLimitGateForTest.reset();
      const socket = createServer((req, res) => {
        if (req.url.includes('/state/m.room.encryption/')) {
          res.writeHead(404, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify({ errcode: 'M_NOT_FOUND' }));
          return;
        }
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ event_id: '$complete' }));
      });
      await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve));
      const address = socket.address();
      const side = { side: { serverName: server, active: true, accessState: 'accepted', apiBaseUrl: `http://127.0.0.1:${address.port}` },
        credential: { kind: 'appservice', senderLocalpart: 'hagency', asToken: 'secret',
          outboundGeneration: 'side-generation', namespace: '@ac_.*' } };
      const sender = { kind: 'appservice', ...side, agentUserId: `@ac_worker:${server}`, agentName: 'worker' };
      const bridge = Object.assign(Object.create(MatrixBridge.prototype), {
        approvalProjectionSendTimeoutMs: 100,
        _approvalMatrixPacer: new ApprovalMatrixPacer({ gapMs: 0 }),
        actingSideFor: () => side,
        agentSenderFor: () => sender,
        isKnownAgentMxid: () => true,
        postWarning: vi.fn(), endAgentWork: vi.fn(), recentMatrixEvents: new Map(),
      });
      const row = { request_id: 'approval_complete', revision: 1,
        channel: kind === 'side-representative' ? 'private_request' : 'public_notice',
        publisher_scope: kind === 'side-representative' ? `side-representative:${server}` : `agent:worker:${server}`,
        target_room_id: `!room:${server}`, state: 'pending', migration_kind: 'native_v2',
        approval: { agent: 'worker', project: 'adapter', project_room_id: `!room:${server}` } };
      const io = approvalProjectionIoForTest(bridge);
      const actor = await io.resolveActor(row);
      try {
        await expect(io.send({ publisher_mxid: actor.publisher_mxid,
          credential_generation: actor.credential_generation, prepared_event_type: 'm.room.message',
          prepared_payload: { body: 'fixed' }, transaction_id: 'final-complete' }, actor, row))
          .resolves.toBe('$complete');
      } finally {
        socket.closeAllConnections();
        await new Promise(resolve => socket.close(resolve));
      }
    },
  );
});
