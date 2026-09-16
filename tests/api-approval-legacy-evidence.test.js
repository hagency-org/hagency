import { afterAll, beforeAll, expect, test } from 'vitest';
import request from 'supertest';
import { readFileSync } from 'node:fs';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';
import { legacyDisk, legacyId, legacyAttestation, publisherInput } from './helpers/approval-legacy-fixture.js';

let context;
const secret = 'synthetic-legacy-bridge-secret';
const bridge = (method, url) => request(context.app)[method](url).set('X-Bridge-Secret', secret);
beforeAll(async () => {
  context = await createBackendTestContext('hafleet-legacy-evidence-api-', {
    agents: { worker: { name: 'worker', kind: 'agent' } }, agentTokens: { worker: 'synthetic-agent-token' },
    rawDataFiles: { 'approvals.json': JSON.stringify(legacyDisk()) },
    env: { MATRIX_BRIDGE_SECRET: secret, HAGENCY_AGENT_TOKEN_MODE: 'hard', MATRIX_SERVER_NAME: 'test', MATRIX_BOT_USERNAME: 'bot' },
  });
  const actor = publisherInput();
  expect((await bridge('put', '/api/approvals/matrix/publishers').send({ ...actor, scope: actor.publisher_scope })).status).toBe(200);
});
afterAll(() => context?.cleanup());

test('legacy attestation persists only authenticated bridge evidence and returns readonly wire', async () => {
  const row = context.internals.approvalStoreForTest.listDueProjections()[0];
  const path = `/api/approvals/${legacyId}/matrix/legacy-original`;
  const attestation = legacyAttestation(row);
  expect((await request(context.app).post(path).send(attestation)).status).toBe(403);
  const accepted = await bridge('post', path).send(attestation);
  expect(accepted.status).toBe(200);
  expect(accepted.body.evidence).toMatchObject({ original_event_id: '$original-request', original_sender: '@bot:test', historical_credential_generation: null });
  const read = await bridge('get', `${path}?cas_token=${row.cas_token}`);
  expect(read.status).toBe(200);
  expect(read.body.content['com.agentchat.approval']).toMatchObject({ version: 1, kind: 'status', migration_kind: 'legacy_v1', state: 'consumed', decision: 'allow', revision: row.revision });
  expect(read.body.content['m.relates_to']).toEqual({ 'm.in_reply_to': { event_id: '$original-request' } });
  expect(read.body.content['com.agentchat.approval']).not.toHaveProperty('actions');
});

async function isolated(record, env = {}) {
  const c = await createBackendTestContext('hafleet-legacy-api-case-', {
    agents: { worker: { name: 'worker', kind: 'agent' } }, agentTokens: { worker: 'synthetic-agent-token' },
    rawDataFiles: { 'approvals.json': JSON.stringify(legacyDisk(record)) },
    env: { MATRIX_BRIDGE_SECRET: secret, HAGENCY_AGENT_TOKEN_MODE: 'hard', MATRIX_SERVER_NAME: 'test', MATRIX_BOT_USERNAME: 'bot',
      HAGENCY_APPROVAL_DM_MODE: 'required', HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST: '', ...env },
  });
  const api = (method, url) => request(c.app)[method](url).set('X-Bridge-Secret', secret);
  const actor = publisherInput();
  expect((await api('put', '/api/approvals/matrix/publishers').send({ ...actor, scope: actor.publisher_scope })).status).toBe(200);
  const row = c.internals.approvalStoreForTest.listDueProjections()[0];
  return { c, api, row, path: `/api/approvals/${row.request_id}/matrix/legacy-original`, attestation: legacyAttestation(row, record) };
}

test('legacy evidence API rejects path CAS public bearer and malformed proof atomically', async () => {
  const f = await isolated();
  try {
    for (const method of ['get', 'post']) {
      expect((await request(f.c.app)[method](f.path).set('X-Agent-Token', 'synthetic-agent-token').send(f.attestation)).status).toBe(403);
      expect((await request(f.c.app)[method](f.path).set('Authorization', 'Bearer synthetic-agent-token').send(f.attestation)).status).toBe(403);
    }
    const store = f.c.internals.approvalStoreForTest;
    const before = JSON.stringify(store.state);
    expect((await f.api('post', f.path.replace(legacyId, 'wrong')).send(f.attestation)).status).toBe(409);
    for (const mutate of [
      (p) => { p.revision += 1; }, (p) => { p.original.input_digest = 'wrong'; },
      (p) => { p.candidate_verdict_event_id = '$wrong'; }, (p) => { p.original.sender = '@other:test'; },
      (p) => { p.private_context.joined = false; }, (p) => { p.private_context.ready = false; },
      (p) => { p.private_context.owner_mxid = '@other:test'; }, (p) => { p.private_context.room_id = '!other:test'; },
      (p) => { p.publisher.credential_generation = 'unknown'; },
    ]) {
      const input = structuredClone(f.attestation); mutate(input);
      expect((await f.api('post', f.path).send(input)).status).toBe(409);
      expect(JSON.stringify(store.state)).toBe(before);
    }
    const bad = { ...f.attestation, raw_event: { body: 'private details' } };
    expect((await f.api('post', f.path).send(bad)).status).toBe(400);
    expect(JSON.stringify(store.state)).toBe(before);
    const accepted = await f.api('post', f.path).send(f.attestation);
    expect(accepted.status).toBe(200);
    expect((await f.api('post', f.path).send(f.attestation)).body).toMatchObject({ duplicate: true, evidence: accepted.body.evidence });
    const agent = await request(f.c.app).get(`/api/approvals/${legacyId}`).set('X-Agent-Token', 'synthetic-agent-token');
    expect(agent.status).toBe(200);
    expect(agent.body.approval).not.toHaveProperty('legacyOriginalEvidence');
    expect(agent.body.approval).not.toHaveProperty('original_sender');
    expect((await f.api('get', `${f.path}?cas_token=wrong`)).status).toBe(409);
  } finally { await f.c.cleanup(); }
});

test('legacy status pins current publisher and rejects rotation while accepting exact attempted receipt', async () => {
  const f = await isolated();
  try {
    const store = f.c.internals.approvalStoreForTest;
    const canonicalBefore = JSON.stringify({ requests: store.state.requests, bindings: store.state.bindings });
    const accepted = await f.api('post', f.path).send(f.attestation); expect(accepted.status).toBe(200);
    const preparePath = `/api/approvals/${legacyId}/matrix/projections/${f.row.revision}`;
    const input = { ...publisherInput(), cas_token: f.row.cas_token, channel: 'private_status',
      legacy_evidence_cas: accepted.body.evidence.evidence_cas, private_context: f.attestation.private_context,
      prepared_event_type: 'm.room.encrypted', prepared_payload: { algorithm: 'm.megolm.v1.aes-sha2', ciphertext: 'exact-first' } };
    const prepared = await f.api('post', `${preparePath}/prepare`).send(input); expect(prepared.status).toBe(200);
    const plan = prepared.body.plan;
    const identity = { cas_token: plan.cas_token, channel: 'private_status', publisher_scope: plan.publisher_scope,
      publisher_mxid: plan.publisher_mxid, room_id: f.row.target_room_id, credential_generation: plan.credential_generation,
      transaction_id: plan.transaction_id, private_context: f.attestation.private_context };
    expect((await f.api('post', `${preparePath}/begin-send`).send({ ...identity, room_id: '!other:test' })).status).toBe(409);
    expect((await f.api('post', `${preparePath}/begin-send`).send(identity)).status).toBe(200);
    expect((await f.api('post', `${preparePath}/retry`).send({ ...identity, error_code: 'timeout' })).status).toBe(200);
    const replay = await f.api('post', `${preparePath}/prepare`).send({ ...input, prepared_payload: { ciphertext: 'loser' } });
    expect(replay.status).toBe(200); expect(replay.body.plan).toEqual({ ...plan, attempt_state: 'uncertain' });
    const rotated = publisherInput({ credential_generation: 'rotated' });
    expect((await f.api('put', '/api/approvals/matrix/publishers').send({ ...rotated, scope: rotated.publisher_scope })).status).toBe(200);
    expect((await f.api('post', `${preparePath}/prepare`).send(input)).status).toBe(409);
    expect((await f.api('post', `${preparePath}/prepare`).send({ ...input, ...rotated })).status).toBe(409);
    for (const operation of ['begin-send', 'retry']) {
      expect((await f.api('post', `${preparePath}/${operation}`).send(identity)).status).toBe(409);
    }
    expect((await f.api('post', `${preparePath}/receipt`).send({ ...identity, event_id: '$late-status' })).status).toBe(200);
    expect((await f.api('post', `${preparePath}/receipt`).send({ ...identity, event_id: '$late-status' })).status).toBe(200);
    expect((await f.api('post', `${preparePath}/receipt`).send({ ...identity, event_id: '$other' })).status).toBe(409);
    expect(f.c.internals.approvalStoreForTest.privateRequestPublisher(legacyId)).toBeNull();
    expect(JSON.stringify({ requests: store.state.requests, bindings: store.state.bindings })).toBe(canonicalBefore);
  } finally { await f.c.cleanup(); }
});

test('legacy API accepts current project-side plaintext and rejects encrypted or stale side context', async () => {
  const f = await isolated(undefined, { NODE_ENV: 'production', HAGENCY_APPROVAL_DM_MODE: 'required', HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST: '' });
  try {
    const sideStore = f.c.internals.projectSideStoreForTest;
    sideStore.upsertSide({ server_name: 'test', api_base_url: 'http://127.0.0.1:1',
      credential: { kind: 'appservice', asToken: 'synthetic-as-v1', hsToken: 'synthetic-hs', namespace: '@ac_.*', senderLocalpart: 'historical' } });
    sideStore.observeAccess('test', { state: 'accepted' }); sideStore.setRepresentative('test', { mxid: '@historical:test' });
    const actor = { publisher_scope: 'side-representative:test', publisher_mxid: '@historical:test', homeserver: 'test',
      credential_kind: 'appservice', credential_generation: sideStore.credentialFor('test').outboundGeneration };
    expect((await f.api('put', '/api/approvals/matrix/publishers').send({ ...actor, scope: actor.publisher_scope, side_id: 'test' })).status).toBe(200);
    f.attestation.original.sender = actor.publisher_mxid; f.attestation.publisher = actor;
    const plaintext = structuredClone(f.attestation); plaintext.private_context.encrypted = false;
    const accepted = await f.api('post', f.path).send(plaintext); expect(accepted.status).toBe(200);
    const status = (await f.api('get', `${f.path}?cas_token=${f.row.cas_token}`)).body.content;
    const preparePath = `/api/approvals/${legacyId}/matrix/projections/${f.row.revision}`;
    const input = { ...actor, cas_token: f.row.cas_token, channel: 'private_status',
      legacy_evidence_cas: accepted.body.evidence.evidence_cas, private_context: plaintext.private_context,
      prepared_event_type: 'm.room.message', prepared_payload: status };
    const prepared = await f.api('post', `${preparePath}/prepare`).send(input); expect(prepared.status).toBe(200);
    const encrypted = { ...input, private_context: f.attestation.private_context,
      prepared_event_type: 'm.room.encrypted', prepared_payload: { ciphertext: 'one' } };
    expect((await f.api('post', `${preparePath}/prepare`).send(encrypted)).status).toBe(409);
    // Rotate the real ProjectSideStore token without refreshing the publisher registry.
    sideStore.setCredential('test', { kind: 'appservice', asToken: 'synthetic-as-v2', hsToken: 'synthetic-hs', namespace: '@ac_.*', senderLocalpart: 'historical' });
    sideStore.observeAccess('test', { state: 'accepted' });
    expect((await f.api('post', f.path).send(plaintext)).status).toBe(409);
    expect((await f.api('post', `${preparePath}/prepare`).send(input)).status).toBe(409);
    const plan = prepared.body.plan;
    const identity = { cas_token: plan.cas_token, channel: 'private_status', publisher_scope: plan.publisher_scope,
      publisher_mxid: plan.publisher_mxid, room_id: f.row.target_room_id, credential_generation: plan.credential_generation,
      transaction_id: plan.transaction_id, private_context: f.attestation.private_context };
    expect((await f.api('post', `${preparePath}/begin-send`).send(identity)).status).toBe(409);
  } finally { await f.c.cleanup(); }
});

test('legacy API permits only the existing explicit plaintext-test policy', async () => {
  const f = await isolated(undefined, { HAGENCY_APPROVAL_DM_MODE: 'plaintext-test', HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST: '1', NODE_ENV: 'test' });
  try {
    f.attestation.private_context.encrypted = false;
    const accepted = await f.api('post', f.path).send(f.attestation); expect(accepted.status).toBe(200);
    const read = await f.api('get', `${f.path}?cas_token=${f.row.cas_token}`);
    const input = { ...publisherInput(), channel: 'private_status', cas_token: f.row.cas_token,
      legacy_evidence_cas: accepted.body.evidence.evidence_cas, private_context: f.attestation.private_context,
      prepared_event_type: 'm.room.message', prepared_payload: read.body.content };
    const planPath = `/api/approvals/${legacyId}/matrix/projections/1/prepare`;
    process.env.NODE_ENV = 'production';
    expect((await f.api('post', planPath).send(input)).status).toBe(409);
    process.env.NODE_ENV = 'test';
    const prepared = await f.api('post', planPath).send(input); expect(prepared.status).toBe(200);
    expect(prepared.body.plan.prepared_payload).toEqual(read.body.content);
  } finally { await f.c.cleanup(); }
});

test('legacy API persists evidence atomically and reports committed degraded health', async () => {
  const f = await isolated();
  try {
    const store = f.c.internals.approvalStoreForTest;
    const frames = []; const client = { write: (frame) => frames.push(frame) };
    f.c.internals.sseAdapterForTest.clients.add(client);
    try {
      const before = JSON.stringify(store.state);
      store.fsFault = (phase) => { if (phase === 'beforeRename') throw new Error('evidence pre-rename failure'); };
      const failed = await f.api('post', f.path).send(f.attestation);
      expect(failed.status).toBe(503); expect(JSON.stringify(store.state)).toBe(before);
      expect(frames).toEqual([]);
      store.fsFault = (phase) => { if (phase === 'afterRename') throw new Error('evidence post-rename failure'); };
      const committed = await f.api('post', f.path).send(f.attestation);
      expect(committed.status).toBe(200);
      expect(committed.body.persistence).toMatchObject({ committed: true, degraded: true });
      expect((await f.api('get', `${f.path}?cas_token=${f.row.cas_token}`)).body.evidence).toEqual(committed.body.evidence);
      expect(store.state.requests[legacyId].status).toBe('consumed');
      expect(store.state.projectionOutbox).toHaveLength(1); expect(frames).toEqual([]);
    } finally { f.c.internals.sseAdapterForTest.clients.delete(client); }
  } finally { await f.c.cleanup(); }
});

test('legacy API rejects two currently valid private contexts for the same original sender', async () => {
  const f = await isolated();
  try {
    const side = f.c.internals.projectSideStoreForTest;
    side.upsertSide({ server_name: 'test', api_base_url: 'http://127.0.0.1:1',
      credential: { kind: 'appservice', asToken: 'synthetic-same-bot', hsToken: 'synthetic-hs', namespace: '@ac_.*', senderLocalpart: 'bot' } });
    side.observeAccess('test', { state: 'accepted' }); side.setRepresentative('test', { mxid: '@bot:test' });
    const actor = { publisher_scope: 'side-representative:test', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'appservice', credential_generation: side.credentialFor('test').outboundGeneration };
    expect((await f.api('put', '/api/approvals/matrix/publishers').send({ ...actor, scope: actor.publisher_scope, side_id: 'test' })).status).toBe(200);
    const before = JSON.stringify(f.c.internals.approvalStoreForTest.state);
    expect((await f.api('post', f.path).send(f.attestation)).status).toBe(409);
    expect((await f.api('post', f.path).send({ ...f.attestation, publisher: actor })).status).toBe(409);
    expect(JSON.stringify(f.c.internals.approvalStoreForTest.state)).toBe(before);
  } finally { await f.c.cleanup(); }
});

test('legacy API requires explicit CAS and rejects absent publisher objects without internal errors', async () => {
  const f = await isolated();
  try {
    expect((await f.api('get', f.path)).status).toBe(409);
    for (const publisher of [undefined, null, [], 'local_bot']) {
      expect((await f.api('post', f.path).send({ ...f.attestation, publisher })).status).toBe(409);
    }
  } finally { await f.c.cleanup(); }
});

test('legacy plaintext API rejects padded event type without mutation under explicit test policy', async () => {
  const f = await isolated(undefined, { HAGENCY_APPROVAL_DM_MODE: 'plaintext-test', HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST: '1', NODE_ENV: 'test' });
  try {
    f.attestation.private_context.encrypted = false;
    const accepted = await f.api('post', f.path).send(f.attestation); expect(accepted.status).toBe(200);
    const read = await f.api('get', `${f.path}?cas_token=${f.row.cas_token}`);
    const store = f.c.internals.approvalStoreForTest;
    const state = JSON.stringify(store.state);
    const disk = readFileSync(store.filePath, 'utf8');
    for (const forged of [true, false]) {
      const content = structuredClone(read.body.content);
      if (forged) {
        content['com.agentchat.approval'].state = 'approved';
        content['com.agentchat.approval'].actions = [{ id: 'approve_once' }];
      }
      const input = { ...publisherInput(), channel: 'private_status', cas_token: f.row.cas_token,
        legacy_evidence_cas: accepted.body.evidence.evidence_cas, private_context: f.attestation.private_context,
        prepared_event_type: ' m.room.message ', prepared_payload: content };
      expect((await f.api('post', `/api/approvals/${legacyId}/matrix/projections/1/prepare`).send(input)).status).toBe(400);
      expect(JSON.stringify(store.state)).toBe(state); expect(readFileSync(store.filePath, 'utf8')).toBe(disk);
    }
  } finally { await f.c.cleanup(); }
});
