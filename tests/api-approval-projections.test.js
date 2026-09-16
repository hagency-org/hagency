import { afterAll, beforeAll, describe, expect, test } from 'vitest';
import request from 'supertest';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';

const AGENT_TOKEN = 'projection-agent-token';
const BRIDGE_SECRET = 'projection-bridge-secret';
let context;
let approvalId;

function bridge(method, url) {
  return request(context.app)[method](url).set('X-Bridge-Secret', BRIDGE_SECRET);
}

beforeAll(async () => {
  context = await createBackendTestContext('hafleet-projection-api-', {
    agents: { worker: { name: 'worker', type: 'agent', kind: 'agent', online: true } },
    agentTokens: { worker: AGENT_TOKEN },
    env: {
      MATRIX_BRIDGE_SECRET: BRIDGE_SECRET,
      HAGENCY_AGENT_TOKEN_MODE: 'hard',
      MATRIX_SERVER_NAME: 'test',
      MATRIX_BOT_USERNAME: 'bot',
    },
  });
  const publisher = await bridge('put', '/api/approvals/matrix/publishers').send({
    scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
    credential_kind: 'local_bot', credential_generation: 'g1',
  });
  expect(publisher.status).toBe(200);
  expect((await bridge('put', '/api/approvals/matrix/publishers').send({
    scope: 'agent:worker:test', agent: 'worker', side_id: 'test', publisher_mxid: '@ac_worker:test',
    homeserver: 'test', credential_kind: 'agent_token', credential_generation: 'agent-g1',
  })).status).toBe(200);
  await bridge('put', '/api/approval-bindings').send({
    agent: 'worker', project: 'p', project_room_id: '!p:test',
    owner_mxid: '@owner:test', owner_dm_room_id: '!dm:test',
  });
  const created = await request(context.app).post('/api/approvals')
    .set('X-Agent-Token', AGENT_TOKEN)
    .send({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'u', tool_name: 'Bash', input_preview: 'pwd' });
  approvalId = created.body.approval.id;
});

afterAll(() => context.cleanup());

describe('approval projection bridge API', () => {
  test('committed create verdict and consume transitions emit increasing redacted wakes', async () => {
    const frames = [];
    const client = { write: (frame) => frames.push(frame) };
    context.internals.sseAdapterForTest.clients.add(client);
    try {
      const created = await request(context.app).post('/api/approvals')
        .set('X-Agent-Token', AGENT_TOKEN)
        .send({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'u-wakes', tool_name: 'Bash', input_preview: 'pwd' });
      expect(created.status).toBe(201);
      const id = created.body.approval.id;
      const matrix = (await bridge('get', `/api/approvals/${id}/matrix`)).body.approval;
      const verdict = await bridge('post', `/api/approvals/${id}/verdict`).send({
        action: 'approve_once', sender_mxid: matrix.owner_mxid, room_id: matrix.owner_dm_room_id,
        agent: matrix.agent, project: matrix.project, project_room_id: matrix.project_room_id,
        input_digest: matrix.input_digest,
      });
      expect(verdict.status).toBe(200);
      const consumed = await request(context.app).post(`/api/approvals/${id}/consume`)
        .set('X-Agent-Token', AGENT_TOKEN)
        .send({ agent: 'worker', input_digest: matrix.input_digest });
      expect(consumed.status).toBe(200);
      const changes = frames.filter((frame) => frame.startsWith('event: approval_changed'))
        .map((frame) => JSON.parse(frame.split('\ndata: ')[1]))
        .filter((payload) => payload.request_id === id);
      expect(changes).toEqual([
        { request_id: id, revision: 1 },
        { request_id: id, revision: 2 },
        { request_id: id, revision: 3 },
      ]);
    } finally {
      context.internals.sseAdapterForTest.clients.delete(client);
    }
  });

  test('bounded maintenance emits redacted wake hints for durable due work', () => {
    const frames = [];
    const client = { write: (frame) => frames.push(frame) };
    context.internals.sseAdapterForTest.clients.add(client);
    try {
      const result = context.internals.runApprovalProjectionMaintenanceForTest();
      expect(result).toMatchObject({ skipped: false });
      expect(result.wakes).toBeGreaterThan(0);
      const wakeFrames = frames.filter((frame) => frame.startsWith('event: approval_changed'));
      expect(wakeFrames.length).toBeGreaterThan(0);
      for (const frame of wakeFrames) {
        const payload = JSON.parse(frame.split('\ndata: ')[1]);
        expect(payload).toEqual({ request_id: expect.any(String), revision: expect.any(Number) });
      }
    } finally {
      context.internals.sseAdapterForTest.clients.delete(client);
    }
  });

  test('projection listing is bridge-secret only and keeps metadata out of agent response', async () => {
    expect((await request(context.app).get('/api/approvals/matrix/projections')).status).toBe(403);
    expect((await request(context.app).get('/api/approvals/matrix/projections').set('X-Agent-Token', AGENT_TOKEN)).status).toBe(403);
    const listed = await bridge('get', '/api/approvals/matrix/projections?limit=200');
    expect(listed.status).toBe(200);
    expect(listed.body.projections).toContainEqual(expect.objectContaining({ request_id: approvalId, revision: 1 }));
    expect(listed.body.projections.find((item) => item.request_id === approvalId
      && item.channel === 'private_request')).toMatchObject({ publisher_scope: 'local_bot' });
    const createdSecond = await request(context.app).post('/api/approvals')
      .set('X-Agent-Token', AGENT_TOKEN)
      .send({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'u-page-2', tool_name: 'Bash', input_preview: 'pwd' });
    expect(createdSecond.status).toBe(201);
    const firstPage = await bridge('get', '/api/approvals/matrix/projections?limit=1');
    const secondPage = await bridge('get', `/api/approvals/matrix/projections?limit=1&after=${encodeURIComponent(firstPage.body.next)}`);
    expect(secondPage.status).toBe(200);
    expect(secondPage.body.projections).toHaveLength(1);
    expect(secondPage.body.projections[0].cursor).not.toBe(firstPage.body.projections[0].cursor);
    expect((await bridge('get', '/api/approvals/matrix/projections?after=not-a-cursor')).status).toBe(400);
    const agentView = await request(context.app).get(`/api/approvals/${approvalId}`).set('X-Agent-Token', AGENT_TOKEN);
    expect(agentView.body.approval).not.toHaveProperty('projection_revision');
    expect(agentView.body.approval).not.toHaveProperty('plan');
  });

  test('page cursor survives receipt of its anchor row', async () => {
    const first = await bridge('get', '/api/approvals/matrix/projections?limit=200');
    expect(first.status).toBe(200);
    const row = first.body.projections.find((item) => item.channel === 'private_request' && !item.plan);
    const actor = row.channel === 'public_notice'
      ? { publisher_scope: 'agent:worker:test', publisher_mxid: '@ac_worker:test', credential_kind: 'agent_token', credential_generation: 'agent-g1' }
      : { publisher_scope: 'local_bot', publisher_mxid: '@bot:test', credential_kind: 'local_bot', credential_generation: 'g1' };
    const prepared = await bridge('post', `/api/approvals/${row.request_id}/matrix/projections/${row.revision}/prepare`).send({
      cas_token: row.cas_token, channel: row.channel, ...actor, homeserver: 'test', payload_version: 1,
      prepared_event_type: 'm.room.message', prepared_payload: { body: 'cursor' },
    });
    const plan = prepared.body.plan;
    const identity = { cas_token: plan.cas_token, channel: row.channel, publisher_scope: plan.publisher_scope,
      publisher_mxid: plan.publisher_mxid,
      room_id: row.target_room_id, credential_generation: plan.credential_generation, transaction_id: plan.transaction_id };
    expect((await bridge('post', `/api/approvals/${row.request_id}/matrix/projections/${row.revision}/begin-send`).send(identity)).status).toBe(200);
    expect((await bridge('post', `/api/approvals/${row.request_id}/matrix/projections/${row.revision}/receipt`).send({ ...identity, event_id: '$cursor-anchor' })).status).toBe(200);
    const next = await bridge('get', `/api/approvals/matrix/projections?limit=1&after=${encodeURIComponent(row.cursor)}`);
    expect(next.status).toBe(200);
    expect(next.body.projections).toHaveLength(1);
    expect(next.body.projections[0].cursor).not.toBe(row.cursor);
  });

  test('listing expires first and returns one coherent committed projection', async () => {
    const store = context.internals.approvalStoreForTest;
    const originalNow = store.now;
    try {
      const created = await request(context.app).post('/api/approvals')
        .set('X-Agent-Token', AGENT_TOKEN)
        .send({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'u-expiry-list', tool_name: 'Bash' });
      store.now = () => created.body.approval.expires_at + 1;
      const listed = await bridge('get', '/api/approvals/matrix/projections?limit=200');
      const row = listed.body.projections.find((item) => item.request_id === created.body.approval.id);
      expect(row).toMatchObject({ revision: 2, state: 'expired', approval: { status: 'expired' } });
    } finally {
      store.now = originalNow;
    }
  });

  test('maintenance contains persistence failure and a later tick recovers', async () => {
    const store = context.internals.approvalStoreForTest;
    const originalNow = store.now;
    const originalFault = store.fsFault;
    const frames = [];
    const client = { write: (frame) => frames.push(frame) };
    context.internals.sseAdapterForTest.clients.add(client);
    try {
      const created = await request(context.app).post('/api/approvals')
        .set('X-Agent-Token', AGENT_TOKEN)
        .send({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'u-maintenance-retry', tool_name: 'Bash' });
      store.now = () => created.body.approval.expires_at + 1;
      store.fsFault = (phase) => { if (phase === 'beforeRename') throw new Error('fixture maintenance failure'); };
      expect(context.internals.runApprovalProjectionMaintenanceForTest()).toMatchObject({ ok: false, error_code: 'persistence_failed' });
      expect(store.getProjectionRevision(created.body.approval.id)).toBe(1);
      expect(frames.filter((frame) => frame.startsWith('event: approval_changed'))).toHaveLength(1);
      store.fsFault = () => {};
      expect(context.internals.runApprovalProjectionMaintenanceForTest()).toMatchObject({ ok: true, expiry: { expired: 1 } });
      expect(store.getProjectionRevision(created.body.approval.id)).toBe(2);
    } finally {
      store.now = originalNow;
      store.fsFault = originalFault;
      context.internals.sseAdapterForTest.clients.delete(client);
    }
  });

  test('prepare begin retry and receipt preserve exact route and plan CAS', async () => {
    const created = await request(context.app).post('/api/approvals')
      .set('X-Agent-Token', AGENT_TOKEN)
      .send({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'u-plan-flow', tool_name: 'Bash' });
    const requestId = created.body.approval.id;
    const listed = await bridge('get', '/api/approvals/matrix/projections?limit=20');
    const row = listed.body.projections.find((projection) => projection.request_id === requestId);
    const prepareInput = {
      cas_token: row.cas_token, publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'g1', payload_version: 1,
      channel: row.channel, prepared_event_type: 'm.room.message', prepared_payload: { body: 'approval' },
    };
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/2/prepare`).send(prepareInput)).status).toBe(409);
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/prepare`).send({ ...prepareInput, channel: 'private_status' })).status).toBe(409);
    const prepared = await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/prepare`).send(prepareInput);
    expect(prepared.status).toBe(200);
    const plan = prepared.body.plan;
    const identity = { cas_token: plan.cas_token, channel: row.channel, publisher_scope: plan.publisher_scope, publisher_mxid: plan.publisher_mxid, room_id: row.target_room_id, credential_generation: plan.credential_generation, transaction_id: plan.transaction_id };
    expect((await bridge('post', `/api/approvals/wrong/matrix/projections/1/begin-send`).send(identity)).status).toBe(409);
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/begin-send`).send({ ...identity, channel: 'private_status' })).status).toBe(409);
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/begin-send`).send(identity)).body.plan.attempt_state).toBe('attempted');
    const retried = await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/retry`).send({ ...identity, retry_at: Date.now(), error_code: 'timeout' });
    expect(retried.body.attempt_state).toBe('uncertain');
    const receipt = await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/receipt`).send({ ...identity, event_id: '$projection' });
    expect(receipt.body).toMatchObject({ ok: true, event_id: '$projection' });
  });

  test('publisher generation is authoritative for prepare begin and retry but not an exact late receipt', async () => {
    const created = await request(context.app).post('/api/approvals')
      .set('X-Agent-Token', AGENT_TOKEN)
      .send({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'u-generation', tool_name: 'Bash' });
    const requestId = created.body.approval.id;
    const listed = await bridge('get', '/api/approvals/matrix/projections?limit=200');
    const row = listed.body.projections.find((projection) => projection.request_id === requestId);
    const proposed = {
      cas_token: row.cas_token, channel: row.channel, publisher_scope: 'local_bot',
      publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot',
      credential_generation: 'caller-selected', payload_version: 1,
      prepared_event_type: 'm.room.message', prepared_payload: { body: 'approval' },
    };
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/prepare`).send(proposed)).status).toBe(409);
    const prepared = await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/prepare`)
      .send({ ...proposed, credential_generation: 'g1' });
    expect(prepared.status).toBe(200);
    const plan = prepared.body.plan;
    const identity = {
      cas_token: plan.cas_token, channel: row.channel, publisher_scope: plan.publisher_scope,
      publisher_mxid: plan.publisher_mxid, room_id: row.target_room_id,
      credential_generation: plan.credential_generation, transaction_id: plan.transaction_id,
    };
    await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'g2',
    });
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/begin-send`).send(identity)).status).toBe(409);

    await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'g1',
    });
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/begin-send`).send(identity)).status).toBe(200);
    await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'g2',
    });
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/retry`).send({ ...identity, error_code: 'timeout' })).status).toBe(409);
    expect((await bridge('post', `/api/approvals/${requestId}/matrix/projections/1/receipt`).send({ ...identity, event_id: '$late' })).status).toBe(200);
  });

  test('private status retains its original actor but cannot send after that credential rotates', async () => {
    await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'status-g1',
    });
    const created = await request(context.app).post('/api/approvals').set('X-Agent-Token', AGENT_TOKEN).send({
      agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test',
      upstream_request_id: 'u-private-status-generation', tool_name: 'Bash',
    });
    const requestRow = (await bridge('get', '/api/approvals/matrix/projections?limit=200')).body.projections
      .find((item) => item.request_id === created.body.approval.id && item.channel === 'private_request');
    const preparedRequest = await bridge('post', `/api/approvals/${requestRow.request_id}/matrix/projections/1/prepare`).send({
      cas_token: requestRow.cas_token, channel: requestRow.channel, publisher_scope: 'local_bot',
      publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot',
      credential_generation: 'status-g1', prepared_event_type: 'm.room.message', prepared_payload: { body: 'request' },
    });
    const requestPlan = preparedRequest.body.plan;
    const requestIdentity = { cas_token: requestPlan.cas_token, channel: requestRow.channel,
      publisher_scope: requestPlan.publisher_scope, publisher_mxid: requestPlan.publisher_mxid,
      room_id: requestRow.target_room_id, credential_generation: requestPlan.credential_generation,
      transaction_id: requestPlan.transaction_id };
    await bridge('post', `/api/approvals/${requestRow.request_id}/matrix/projections/1/begin-send`).send(requestIdentity);
    await bridge('post', `/api/approvals/${requestRow.request_id}/matrix/projections/1/receipt`).send({ ...requestIdentity, event_id: '$private-request' });
    const approval = (await bridge('get', `/api/approvals/${requestRow.request_id}/matrix`)).body.approval;
    await bridge('post', `/api/approvals/${requestRow.request_id}/verdict`).send({
      action: 'approve_once', sender_mxid: approval.owner_mxid, room_id: approval.owner_dm_room_id,
      agent: approval.agent, project: approval.project, project_room_id: approval.project_room_id,
      input_digest: approval.input_digest,
    });
    const statusRow = (await bridge('get', '/api/approvals/matrix/projections?limit=200')).body.projections
      .find((item) => item.request_id === requestRow.request_id && item.channel === 'private_status');
    expect(statusRow.publisher).toMatchObject({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'status-g1',
    });
    await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'status-g2',
    });
    const statusPrepare = await bridge('post', `/api/approvals/${statusRow.request_id}/matrix/projections/${statusRow.revision}/prepare`).send({
      cas_token: statusRow.cas_token, channel: statusRow.channel, publisher_scope: requestPlan.publisher_scope,
      publisher_mxid: requestPlan.publisher_mxid, homeserver: requestPlan.homeserver,
      credential_kind: requestPlan.credential_kind, credential_generation: requestPlan.credential_generation,
      prepared_event_type: 'm.room.message', prepared_payload: { body: 'approved' },
    });
    expect(statusPrepare.status).toBe(409);
  });

  test('public notice publisher is the registered agent identity rather than a representative', async () => {
    const created = await request(context.app).post('/api/approvals')
      .set('X-Agent-Token', AGENT_TOKEN)
      .send({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'u-public-publisher', tool_name: 'Bash' });
    await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'public-private-g1',
    });
    const privateRow = (await bridge('get', '/api/approvals/matrix/projections?limit=200')).body.projections
      .find((item) => item.request_id === created.body.approval.id && item.channel === 'private_request');
    const privatePrepared = await bridge('post', `/api/approvals/${privateRow.request_id}/matrix/projections/${privateRow.revision}/prepare`).send({
      cas_token: privateRow.cas_token, channel: privateRow.channel, publisher_scope: 'local_bot',
      publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot',
      credential_generation: 'public-private-g1', prepared_event_type: 'm.room.message',
      prepared_payload: { body: 'private request' },
    });
    const privateIdentity = { cas_token: privatePrepared.body.plan.cas_token, channel: privateRow.channel,
      publisher_scope: 'local_bot', publisher_mxid: '@bot:test', room_id: privateRow.target_room_id,
      credential_generation: 'public-private-g1', transaction_id: privatePrepared.body.plan.transaction_id };
    await bridge('post', `/api/approvals/${privateRow.request_id}/matrix/projections/${privateRow.revision}/begin-send`).send(privateIdentity);
    await bridge('post', `/api/approvals/${privateRow.request_id}/matrix/projections/${privateRow.revision}/receipt`)
      .send({ ...privateIdentity, event_id: '$public-private-receipt' });
    const listed = await bridge('get', '/api/approvals/matrix/projections?limit=200');
    const row = listed.body.projections.find((item) => item.request_id === created.body.approval.id && item.channel === 'public_notice');
    expect((await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'agent:worker:test', agent: 'other', side_id: 'test', publisher_mxid: '@ac_worker:test',
      homeserver: 'test', credential_kind: 'agent_token', credential_generation: 'agent-g1',
    })).status).toBe(400);
    expect((await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'agent:worker:test', agent: 'worker', side_id: 'test', publisher_mxid: '@bot:test',
      homeserver: 'test', credential_kind: 'agent_token', credential_generation: 'agent-g1',
    })).status).toBe(409);
    expect((await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'agent:worker:test', agent: 'worker', side_id: 'test', publisher_mxid: '@ac_worker:test',
      homeserver: 'test', credential_kind: 'agent_token', credential_generation: 'agent-g1',
    })).status).toBe(200);
    const prepared = await bridge('post', `/api/approvals/${row.request_id}/matrix/projections/${row.revision}/prepare`).send({
      cas_token: row.cas_token, channel: row.channel, publisher_scope: 'agent:worker:test',
      publisher_mxid: '@ac_worker:test', homeserver: 'test', credential_kind: 'agent_token',
      credential_generation: 'agent-g1', prepared_event_type: 'm.room.message', prepared_payload: { body: 'notice' },
    });
    expect(prepared.status).toBe(200);
  });

  test('botless same-server private delivery resolves to the verified side representative', async () => {
    const botless = await createBackendTestContext('hafleet-projection-botless-', {
      agents: { worker: { name: 'worker', type: 'agent', kind: 'agent', online: true } },
      agentTokens: { worker: AGENT_TOKEN },
      env: {
        MATRIX_BRIDGE_SECRET: BRIDGE_SECRET, HAGENCY_AGENT_TOKEN_MODE: 'hard',
        MATRIX_SERVER_NAME: 'same.test', MATRIX_BOT_USERNAME: '',
      },
    });
    try {
      const sideStore = botless.internals.projectSideStoreForTest;
      sideStore.upsertSide({
        server_name: 'same.test', api_base_url: 'http://127.0.0.1:8008',
        credential: { kind: 'appservice', asToken: 'as-private', hsToken: 'hs-private', namespace: '@ac_.*', senderLocalpart: 'hagency' },
      });
      sideStore.observeAccess('same.test', { state: 'accepted' });
      sideStore.setRepresentative('same.test', { mxid: '@hagency:same.test' });
      const sideCredential = sideStore.credentialFor('same.test');
      const api = (method, url) => request(botless.app)[method](url).set('X-Bridge-Secret', BRIDGE_SECRET);
      await api('put', '/api/approval-bindings').send({
        agent: 'worker', project: 'p', project_room_id: '!p:same.test',
        owner_mxid: '@owner:same.test', owner_dm_room_id: '!dm:same.test',
      });
      const created = await request(botless.app).post('/api/approvals').set('X-Agent-Token', AGENT_TOKEN).send({
        agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:same.test',
        upstream_request_id: 'u-botless-same', tool_name: 'Bash',
      });
      const listed = await api('get', '/api/approvals/matrix/projections?limit=20');
      const row = listed.body.projections.find((item) => item.request_id === created.body.approval.id && item.channel === 'private_request');
      expect(row.publisher_scope).toBe('side-representative:same.test');
      expect((await api('put', '/api/approvals/matrix/publishers').send({
        scope: 'local_bot', publisher_mxid: '@bot:same.test', homeserver: 'same.test',
        credential_kind: 'local_bot', credential_generation: 'wrong',
      })).status).toBe(409);
      expect((await api('put', '/api/approvals/matrix/publishers').send({
        scope: 'side-representative:same.test', side_id: 'same.test',
        publisher_mxid: '@hagency:same.test', homeserver: 'same.test', credential_kind: 'appservice',
        credential_generation: sideCredential.outboundGeneration,
      })).status).toBe(200);
      const prepared = await api('post', `/api/approvals/${row.request_id}/matrix/projections/1/prepare`).send({
        cas_token: row.cas_token, channel: row.channel, publisher_scope: 'side-representative:same.test',
        publisher_mxid: '@hagency:same.test', homeserver: 'same.test', credential_kind: 'appservice',
        credential_generation: sideCredential.outboundGeneration, prepared_event_type: 'm.room.message',
        prepared_payload: { body: 'private' },
      });
      expect(prepared.status).toBe(200);
      sideStore.setCredential('same.test', {
        kind: 'appservice', asToken: 'as-rotated', hsToken: 'hs-private', namespace: '@ac_.*', senderLocalpart: 'hagency',
      });
      sideStore.observeAccess('same.test', { state: 'accepted' });
      const plan = prepared.body.plan;
      expect((await api('post', `/api/approvals/${row.request_id}/matrix/projections/1/begin-send`).send({
        cas_token: plan.cas_token, channel: row.channel, publisher_scope: plan.publisher_scope,
        publisher_mxid: plan.publisher_mxid, room_id: row.target_room_id,
        credential_generation: plan.credential_generation, transaction_id: plan.transaction_id,
      })).status).toBe(409);
    } finally {
      botless.cleanup();
    }
  });
});
