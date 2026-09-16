import { afterAll, beforeAll, beforeEach, expect, test, vi } from 'vitest';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { MatrixClient, CryptoClient } from 'matrix-bot-sdk';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';
import { ApprovalMatrixPacer } from '../lib/approval-matrix-pacer.js';
import { legacyRecord } from './helpers/approval-legacy-fixture.js';

const SECRET = 'synthetic-legacy-reader-secret';
const names = ['success', 'retry', 'stalled', 'crypto', 'stop', 'rotate', 'invalid', 'side', 'missing', 'keys', 'envelope', 'aftercrypto', 'afterbegin', 'rate', 'large', 'registration', 'sidereplay', 'sideencrypted', 'siderotate', 'redirect'];
const records = Object.fromEntries(names.map((name) => {
  const id = `approval_${createHash('sha256').update(name).digest('hex').slice(0, 32)}`;
  return [name, legacyRecord({ id, ownerMxid: '@owner:legacy.test', ownerDmRoomId: `!${name}:legacy.test`,
    projectRoomId: '!project:legacy.test', matrixEventId: name === 'missing' ? null : `$verdict-${name}` })];
}));
let context, matrix, sdk, bridgeModule, selected, handler;
const calls = [];
const sockets = new Set();
let encryptCount, decryptCount, sendCount, originalBotState;

beforeAll(async () => {
  matrix = createServer((req, res) => { calls.push({ method: req.method, url: req.url, authorization: req.headers.authorization }); handler(req, res); });
  matrix.on('connection', socket => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); });
  matrix.listen(0, '127.0.0.1'); await once(matrix, 'listening');
  const matrixUrl = `http://127.0.0.1:${matrix.address().port}`;
  context = await createBackendTestContext('legacy-reader-api-', {
    agents: { worker: { name: 'worker', kind: 'agent' } },
    rawDataFiles: { 'approvals.json': JSON.stringify({ version: 1, bindings: {}, audit: [], requests: Object.fromEntries(Object.values(records).map(r => [r.id, r])) }) },
    env: { MATRIX_BRIDGE_SECRET: SECRET, MATRIX_SERVER_NAME: 'legacy.test', MATRIX_BOT_USERNAME: 'bot',
      MATRIX_HOMESERVER: matrixUrl, HAGENCY_API: 'http://127.0.0.1:1',
      HAGENCY_APPROVAL_DM_MODE: 'required', HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST: '' },
  });
  process.env.HAGENCY_API = `http://127.0.0.1:${context.app.address().port}`;
  bridgeModule = await import('../bridge-matrix.js');
  const state = bridgeModule.bridgeStateForTest();
  originalBotState = { botMxid: state.botMxid, botCredentialGeneration: state.botCredentialGeneration };
  state.botMxid = '@bot:legacy.test'; state.botCredentialGeneration = 'legacy-reader-g1';
  sdk = new MatrixClient(matrixUrl, 'synthetic-reader-token');
});

afterAll(async () => {
  Object.assign(bridgeModule.bridgeStateForTest(), originalBotState);
  for (const socket of sockets) socket.destroy();
  await new Promise(resolve => matrix.close(resolve));
  await context.cleanup();
});

function eventPair(name) {
  const r = records[name];
  const approval = context.internals.approvalStoreForTest.getProjectionRequest(r.id);
  const originalContent = bridgeModule.buildOwnerApprovalRequest(approval);
  const detail = originalContent['com.agentchat.approval'];
  const verdictContent = { msgtype: 'com.agentchat.approval.verdict.v1', body: 'Approve once',
    'com.agentchat.approval': { version: 1, kind: 'verdict', request_id: r.id, agent: r.agent, project: r.project,
      project_room_id: r.projectRoomId, input_digest: r.inputDigest, action: 'approve_once' },
    'm.relates_to': { 'm.in_reply_to': { event_id: `$original-${name}` } } };
  expect(detail).not.toHaveProperty('owner_mxid');
  const wrap = (eventId, sender, content) => ({ event_id: eventId, room_id: r.ownerDmRoomId, sender,
    type: 'm.room.encrypted', content: { algorithm: 'm.megolm.v1.aes-sha2',
      ciphertext: Buffer.from(JSON.stringify({ type: 'm.room.message', content })).toString('base64') } });
  return { [`$verdict-${name}`]: wrap(`$verdict-${name}`, r.ownerMxid, verdictContent),
    [`$original-${name}`]: wrap(`$original-${name}`, '@bot:legacy.test', originalContent) };
}

function respond(res, status, body) { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(body)); }
function makeBridge() {
  const bridge = Object.create(bridgeModule.MatrixBridge.prototype);
  // These cases target proof/crypto/body deadlines. Shared default200ms
  // admission is exercised separately through the real single-worker fixture.
  bridge._approvalMatrixPacer = new ApprovalMatrixPacer({ gapMs: 0 });
  Object.assign(bridge, { botClient: sdk, botUserId: '@bot:legacy.test', approvalDmMode: 'required', actingSideFor: () => null,
    approvalBotPublisherReady: { client: sdk, mxid: '@bot:legacy.test', credentialGeneration: 'legacy-reader-g1' } });
  return bridge;
}
function row(name) {
  return { ...context.internals.approvalStoreForTest.listDueProjections({ limit: 200 }).find(r => r.request_id === records[name].id),
    approval: context.internals.approvalStoreForTest.getProjectionRequest(records[name].id) };
}

beforeEach(() => {
  calls.length = 0; encryptCount = 0; decryptCount = 0; sendCount = 0;
  bridgeModule.matrixRateLimitGateForTest.reset();
  bridgeModule.bridgeStateForTest().botCredentialGeneration = 'legacy-reader-g1';
  // Exercise the SDK's actual envelope-preserving decrypt implementation, with
  // the Rust cryptographic engine replaced only at the crypto boundary.
  sdk.crypto = { isReady: true,
    decryptRoomEvent: async (event, roomId) => {
      decryptCount += 1;
      return CryptoClient.prototype.decryptRoomEvent.call({ isReady: true, engine: { machine: {
        decryptRoomEvent: async raw => ({ event: Buffer.from(JSON.parse(raw).content.ciphertext, 'base64').toString() }),
      } } }, event, roomId);
    },
    encryptRoomEvent: async (_room, type, content) => { encryptCount += 1;
      return { algorithm: 'm.megolm.v1.aes-sha2', ciphertext: Buffer.from(JSON.stringify({ type, content })).toString('base64') }; },
  };
  handler = async (req, res) => {
    const url = new URL(req.url, 'http://fixture');
    if (req.method === 'GET' && url.pathname.endsWith('/joined_members')) {
      return respond(res, 200, { joined: { '@bot:legacy.test': {}, '@owner:legacy.test': {} } });
    }
    if (req.method === 'GET' && url.pathname.endsWith('/state/m.room.encryption/')) return respond(res, 200, { algorithm: 'm.megolm.v1.aes-sha2' });
    if (req.method === 'GET' && url.pathname.includes('/event/')) {
      const event = eventPair(selected)[decodeURIComponent(url.pathname.split('/').at(-1))];
      return respond(res, event ? 200 : 404, event || { errcode: 'M_NOT_FOUND' });
    }
    if (req.method === 'PUT' && url.pathname.includes('/send/')) {
      sendCount += 1; let body = ''; for await (const chunk of req) body += chunk;
      const stored = row(selected).plan;
      expect(stored.attempt_state).toBe('attempted');
      expect(JSON.parse(body)).toEqual(stored.prepared_payload);
      expect(decodeURIComponent(url.pathname.split('/').at(-1))).toBe(stored.transaction_id);
      return respond(res, 200, { event_id: `$status-${selected}` });
    }
    respond(res, 404, { errcode: 'M_NOT_FOUND' });
  };
});

test('legacy production adapter proves two hops and publishes canonical encrypted status through real store API', async () => {
  selected = 'success';
  const original = JSON.stringify(context.internals.approvalStoreForTest.state.requests);
  const result = await bridgeModule.publishApprovalProjectionWithBridgeForTest(makeBridge(), row(selected));
  expect(result).toEqual({ ok: true, event_id: '$status-success' });
  expect(calls.filter(c => c.url.includes('/event/')).map(c => decodeURIComponent(c.url.split('/').at(-1)))).toEqual(['$verdict-success', '$original-success']);
  expect(decryptCount).toBe(2); expect(encryptCount).toBe(1); expect(sendCount).toBe(1);
  expect(JSON.stringify(context.internals.approvalStoreForTest.state.requests)).toBe(original);
  const evidence = context.internals.approvalStoreForTest.state.legacyOriginalEvidence[records.success.id];
  expect(evidence).toMatchObject({ original_event_id: '$original-success', original_sender: '@bot:legacy.test', historical_credential_generation: null });
  expect(context.internals.approvalStoreForTest.privateRequestPublisher(records.success.id)).toBeNull();
});


test('legacy raw envelope is rejected before decrypt or second hop', async () => {
  selected = 'envelope';
  const previous = handler;
  handler = (req, res) => req.url.includes('/event/')
    ? respond(res, 200, { ...eventPair(selected)[`$verdict-${selected}`], room_id: '!other:legacy.test' })
    : previous(req, res);
  expect(await makeBridge().publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, unresolved: true });
  expect(decryptCount).toBe(0);
  expect(calls.filter(c => c.url.includes('/event/'))).toHaveLength(1);
  expect(context.internals.approvalStoreForTest.state.legacyOriginalEvidence[records[selected].id]).toBeUndefined();
  expect(sendCount).toBe(0);
});

test('legacy missing candidate or unavailable decryption stays unresolved without replaying a verdict', async () => {
  selected = 'missing';
  const bridge = makeBridge(); bridge.onRoomMessage = vi.fn(() => { throw new Error('historical event must not be dispatched'); });
  expect(await bridge.publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, unresolved: true });
  expect(calls).toHaveLength(0);
  selected = 'keys'; sdk.crypto.decryptRoomEvent = async () => { throw new Error('private key diagnostic must not escape'); };
  expect(await bridge.publishLegacyApprovalProjection(row(selected))).toEqual({ ok: false, unresolved: true, error_code: 'legacy_unresolved' });
  expect(calls.filter(c => c.url.includes('/event/'))).toHaveLength(1);
  expect(bridge.onRoomMessage).not.toHaveBeenCalled();
  expect(sendCount).toBe(0);
});

test('legacy uncertain replay skips history and encryption and retains exact transaction bytes', async () => {
  selected = 'retry';
  const previous = handler; const attempts = [];
  handler = async (req, res) => {
    if (req.method !== 'PUT') return previous(req, res);
    let body = ''; for await (const chunk of req) body += chunk;
    attempts.push({ url: req.url, body });
    expect(row(selected).plan.attempt_state).toBe('attempted');
    return respond(res, attempts.length === 1 ? 502 : 200, attempts.length === 1 ? { errcode: 'M_UNKNOWN' } : { event_id: '$retry-receipt' });
  };
  const bridge = makeBridge();
  expect(await bridge.publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, uncertain: true });
  expect(row(selected).plan.attempt_state).toBe('uncertain');
  const prepared = structuredClone(row(selected).plan);
  calls.length = 0; encryptCount = 0; decryptCount = 0;
  expect(await bridge.publishLegacyApprovalProjection(row(selected))).toEqual({ ok: true, event_id: '$retry-receipt' });
  expect(attempts[1]).toEqual(attempts[0]);
  expect(JSON.parse(attempts[1].body)).toEqual(prepared.prepared_payload);
  expect(encryptCount).toBe(0); expect(decryptCount).toBe(0);
  expect(calls.filter(c => c.url.includes('/event/'))).toHaveLength(0);
});

test('legacy event HTTP aborts a real stalled response without another hop', async () => {
  selected = 'stalled'; const previous = handler; let close;
  const closed = new Promise(resolve => { close = resolve; });
  handler = (req, res) => {
    if (!req.url.includes('/event/')) return previous(req, res);
    res.writeHead(200, { 'Content-Type': 'application/json' }); res.write('{"event_id":');
    res.on('close', close);
  };
  const result = await makeBridge().publishLegacyApprovalProjection(row(selected), { httpTimeoutMs: 80, timeoutMs: 1000 });
  expect(result).toEqual({ ok: false, unresolved: true, error_code: 'legacy_http_timeout' });
  await closed;
  expect(calls.filter(c => c.url.includes('/event/'))).toHaveLength(1);
  expect(sendCount).toBe(0);
});

test('legacy crypto overrun retains its promise until settlement and starts no later IO', async () => {
  selected = 'crypto'; let entered; let finish;
  const entry = new Promise(resolve => { entered = resolve; });
  sdk.crypto.encryptRoomEvent = async () => { entered(); return new Promise(resolve => { finish = resolve; }); };
  let settled = false;
  const attempt = makeBridge().publishLegacyApprovalProjection(row(selected), { timeoutMs: 200 });
  attempt.then(() => { settled = true; });
  await entry; const countAtCrypto = calls.length;
  await new Promise(resolve => setTimeout(resolve, 230));
  expect(settled).toBe(false);
  expect(calls).toHaveLength(countAtCrypto);
  finish({ algorithm: 'm.megolm.v1.aes-sha2', ciphertext: 'late-owned-crypto' });
  expect(await attempt).toEqual({ ok: false, unresolved: true, error_code: 'legacy_deadline' });
  expect(row(selected).plan).toBeNull();
  expect(calls).toHaveLength(countAtCrypto); expect(sendCount).toBe(0);
});

test('legacy stop and credential rotation between hops prevent later IO', async () => {
  for (const name of ['stop', 'rotate']) {
    selected = name; calls.length = 0;
    const decrypt = sdk.crypto.decryptRoomEvent; let current = true;
    sdk.crypto.decryptRoomEvent = async (...args) => {
      const event = await decrypt(...args);
      if (name === 'stop') current = false;
      else bridgeModule.bridgeStateForTest().botCredentialGeneration = 'rotated';
      return event;
    };
    const result = await makeBridge().publishLegacyApprovalProjection(row(selected), { isCurrent: () => current });
    expect(result).toMatchObject({ ok: false, unresolved: true, error_code: name === 'stop' ? 'legacy_stopped' : 'legacy_publisher_changed' });
    expect(calls.filter(c => c.url.includes('/event/'))).toHaveLength(1);
    expect(sendCount).toBe(0);
    expect(context.internals.approvalStoreForTest.state.legacyOriginalEvidence[records[name].id]).toBeUndefined();
    sdk.crypto.decryptRoomEvent = decrypt;
  }
});

test('legacy token rotation during encryption prevents durable preparation and final PUT', async () => {
  selected = 'aftercrypto'; const original = sdk.accessToken; const encrypt = sdk.crypto.encryptRoomEvent;
  sdk.crypto.encryptRoomEvent = async (...args) => { const bytes = await encrypt(...args); sdk.accessToken = 'rotated-token'; return bytes; };
  try {
    expect(await makeBridge().publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, error_code: 'legacy_publisher_changed' });
    expect(row(selected).plan).toBeNull(); expect(sendCount).toBe(0);
  } finally { sdk.accessToken = original; }
});

test('legacy rotation after a durable begin remains uncertain and prevents final PUT', async () => {
  selected = 'afterbegin'; const fetchActual = globalThis.fetch;
  vi.stubGlobal('fetch', async (...args) => {
    const response = await fetchActual(...args);
    if (String(args[0]).endsWith('/begin-send')) {
      expect(response.status).toBe(200);
      expect(row(selected).plan.attempt_state).toBe('attempted');
      bridgeModule.bridgeStateForTest().botCredentialGeneration = 'after-begin-rotation';
    }
    return response;
  });
  try {
    expect(await makeBridge().publishLegacyApprovalProjection(row(selected))).toEqual({ ok: false, uncertain: true, error_code: 'legacy_publisher_changed' });
    expect(row(selected).plan.attempt_state).toBe('attempted');
    expect(sendCount).toBe(0);
  } finally { vi.unstubAllGlobals(); }
});


test('legacy rate limit does not retry and its body obeys the same byte bound', async () => {
  selected = 'large';
  handler = (_req, res) => respond(res, 429, { retry_after_ms: 60000, padding: 'x'.repeat(270000) });
  expect(await makeBridge().publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, error_code: 'legacy_response_too_large' });
  expect(calls).toHaveLength(1);
  bridgeModule.matrixRateLimitGateForTest.reset(); calls.length = 0; selected = 'rate';
  handler = (_req, res) => respond(res, 429, { retry_after_ms: 60000 });
  expect(await makeBridge().publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, error_code: 'legacy_rate_limited' });
  expect(calls).toHaveLength(1);
  expect(bridgeModule.matrixRateLimitGateForTest.beforeRequest()).toBe(false);
  bridgeModule.matrixRateLimitGateForTest.reset();
});


// Refresh the exact protected endpoint backed by the current ProjectSideStore;
// neither tokens nor generations are invented by the fixture.
async function installSide(bridge, kind) {
  const sideStore = context.internals.projectSideStoreForTest;
  sideStore.upsertSide({ server_name: 'legacy.test', api_base_url: sdk.homeserverUrl,
    credential: kind === 'appservice'
      ? { kind, asToken: 'synthetic-side-send-token', hsToken: 'different-inbound-token', namespace: '@ac_.*', senderLocalpart: 'historical' }
      : { kind, registrationToken: 'not-a-send-token', representativeToken: 'synthetic-representative-send-token' } });
  sideStore.observeAccess('legacy.test', { state: 'accepted' }); sideStore.setRepresentative('legacy.test', { mxid: '@historical:legacy.test' });
  bridge.approvalBotPublisherReady = null;
  delete bridge.actingSideFor;
  bridge.actingCredentials = new Map();
  bridge.forgetRoomsOnSides = () => {};
  const { default: request } = await import('supertest');
  bridge.backendApiForActing = async () => {
    const response = await request(context.app).get('/api/project-sides/acting-credentials')
      .set('X-Bridge-Secret', SECRET);
    expect(response.status).toBe(200);
    return response.body;
  };
  await bridge.refreshActingCredentials();
  expect([...bridge.actingCredentials.keys()]).toEqual(['legacy.test']);
  expect(bridge.actingSideFor('legacy.test')).toMatchObject({
    side: { active: true, accessState: 'accepted', representative: { mxid: '@historical:legacy.test' } },
    credential: { kind, outboundGeneration: sideStore.credentialFor('legacy.test').outboundGeneration },
  });
  return sideStore;
}
function plaintextSideHandler(previous) {
  return (req, res) => {
    const url = new URL(req.url, 'http://fixture');
    if (url.pathname.endsWith('/joined_members')) return respond(res, 200, { joined: { '@historical:legacy.test': {}, '@owner:legacy.test': {} } });
    if (url.pathname.endsWith('/state/m.room.encryption/')) return respond(res, 404, { errcode: 'M_NOT_FOUND' });
    if (url.pathname.includes('/event/')) {
      const raw = eventPair(selected)[decodeURIComponent(url.pathname.split('/').at(-1))];
      const clear = JSON.parse(Buffer.from(raw.content.ciphertext, 'base64').toString());
      return respond(res, 200, { ...raw, ...clear, sender: raw.sender === '@bot:legacy.test' ? '@historical:legacy.test' : raw.sender });
    }
    return previous(req, res);
  };
}
test.each([['side', 'appservice'], ['registration', 'registrationToken']])('legacy %s uses actual store generation and exact private sender transport', async (name, kind) => {
  selected = name; const bridge = makeBridge(); const sideStore = await installSide(bridge, kind);
  handler = plaintextSideHandler(handler);
  vi.stubEnv('NODE_ENV', 'production'); vi.stubEnv('HAGENCY_APPROVAL_DM_MODE', 'required');
  vi.stubEnv('HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST', '');
  try {
    expect(await bridge.publishLegacyApprovalProjection(row(selected))).toEqual({ ok: true, event_id: `$status-${selected}` });
    expect(encryptCount).toBe(0); expect(decryptCount).toBe(0); expect(sendCount).toBe(1);
    const generation = sideStore.credentialFor('legacy.test').outboundGeneration;
    const stored = context.internals.approvalStoreForTest.state.projectionOutbox.find(r => r.requestId === records[selected].id);
    expect(stored.plan.credential_generation).toBe(generation);
    expect(stored.plan.publisher_mxid).toBe('@historical:legacy.test');
    expect(stored.plan.prepared_payload['com.agentchat.approval']).toMatchObject({ migration_kind: 'legacy_v1', state: 'consumed', decision: 'allow' });
    expect(stored.plan.prepared_payload['com.agentchat.approval']).not.toHaveProperty('actions');
    for (const call of calls) {
      expect(call.authorization).toBe(`Bearer ${kind === 'appservice' ? 'synthetic-side-send-token' : 'synthetic-representative-send-token'}`);
      expect(new URL(call.url, 'http://fixture').searchParams.get('user_id')).toBe(kind === 'appservice' ? '@historical:legacy.test' : null);
    }
  } finally { vi.unstubAllEnvs(); }
});


test('legacy uncertain side plaintext replay rechecks security and reuses exact stored bytes', async () => {
  selected = 'sidereplay';
  const bridge = makeBridge();
  await installSide(bridge, 'appservice');
  const base = plaintextSideHandler(handler);
  const attempts = [];
  handler = async (req, res) => {
    if (req.method !== 'PUT') return base(req, res);
    let body = ''; for await (const chunk of req) body += chunk;
    attempts.push({ url: req.url, body });
    return respond(res, attempts.length === 1 ? 502 : 200,
      attempts.length === 1 ? { errcode: 'M_UNKNOWN' } : { event_id: '$side-replay' });
  };
  vi.stubEnv('NODE_ENV', 'production'); vi.stubEnv('HAGENCY_APPROVAL_DM_MODE', 'required');
  vi.stubEnv('HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST', '');
  try {
    expect(await bridge.publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, uncertain: true });
    const stored = structuredClone(row(selected).plan);
    expect(stored.prepared_event_type).toBe('m.room.message');
    calls.length = 0;
    expect(await bridge.publishLegacyApprovalProjection(row(selected))).toEqual({ ok: true, event_id: '$side-replay' });
    expect(calls.filter(call => call.url.includes('/state/m.room.encryption/'))).toHaveLength(1);
    expect(calls.filter(call => call.url.includes('/event/'))).toHaveLength(0);
    expect(attempts[1]).toEqual(attempts[0]);
    expect(JSON.parse(attempts[1].body)).toEqual(stored.prepared_payload);
  } finally { vi.unstubAllEnvs(); }
});

test('legacy side encrypted or rotated contexts cannot downgrade or continue proof reads', async () => {
  selected = 'sideencrypted'; const bridge = makeBridge(); const sides = await installSide(bridge, 'appservice');
  const previous = handler;
  handler = (req, res) => req.url.includes('/joined_members')
    ? respond(res, 200, { joined: { '@historical:legacy.test': {}, '@owner:legacy.test': {} } }) : previous(req, res);
  expect(await bridge.publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, error_code: 'legacy_private_crypto_unavailable' });
  expect(calls.filter(c => c.url.includes('/event/'))).toHaveLength(0);
  selected = 'siderotate'; calls.length = 0; const plain = plaintextSideHandler(handler);
  handler = async (req, res) => {
    if (req.url.includes('/event/')) {
      sides.setCredential('legacy.test', { ...sides.credentialFor('legacy.test'), asToken: 'rotated-side-token' });
      await bridge.refreshActingCredentials();
    }
    return plain(req, res);
  };
  vi.stubEnv('NODE_ENV', 'production'); vi.stubEnv('HAGENCY_APPROVAL_DM_MODE', 'required');
  vi.stubEnv('HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST', '');
  try {
    expect(await bridge.publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, error_code: 'legacy_publisher_changed' });
    expect(calls.filter(c => c.url.includes('/event/'))).toHaveLength(1); expect(sendCount).toBe(0);
  } finally { vi.unstubAllEnvs(); }
});


test('legacy exact event reads never follow an HTTP redirect to another event', async () => {
  selected = 'redirect'; const previous = handler;
  handler = (req, res) => {
    if (req.url.endsWith(encodeURIComponent('$verdict-redirect'))) {
      res.writeHead(302, { Location: req.url.replace(encodeURIComponent('$verdict-redirect'), encodeURIComponent('$original-redirect')) });
      return res.end();
    }
    return previous(req, res);
  };
  expect(await makeBridge().publishLegacyApprovalProjection(row(selected))).toMatchObject({ ok: false, unresolved: true });
  expect(calls.filter(c => c.url.includes('/event/'))).toHaveLength(1);
  expect(sendCount).toBe(0);
});
