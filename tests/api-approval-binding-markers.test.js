import { afterAll, beforeAll, describe, expect, test } from 'vitest';
import request from 'supertest';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';

let context;
const secret = 'marker-bridge-secret';
const bridge = (method, url) => request(context.app)[method](url).set('X-Bridge-Secret', secret);
beforeAll(async () => {
  context = await createBackendTestContext('hafleet-marker-api-', { agents: { worker: { name: 'worker', kind: 'agent' } },
    env: { MATRIX_BRIDGE_SECRET: secret, MATRIX_SERVER_NAME: 'test', MATRIX_BOT_USERNAME: 'bot' } });
  await bridge('put', '/api/approval-bindings').send({ agent: 'worker', project: 'p', project_room_id: '!p:test', owner_mxid: '@owner:test', owner_dm_room_id: '!dm:test' });
});
afterAll(() => context.cleanup());

describe('approval binding marker bridge API', () => {
  test('marker routes are secret-only independently keyed and cursor-stable', async () => {
    expect((await request(context.app).get('/api/approval-bindings/matrix/markers')).status).toBe(403);
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/sync').send({ agent: 'worker', owner_mxid: '@owner:test', approval_room_id: '!dm:test', publisher_mxid: '@bot:test' })).status).toBe(200);
    await bridge('put', '/api/approval-bindings').send({ agent: 'worker', project: 'q', project_room_id: '!q:test', owner_mxid: '@other:test', owner_dm_room_id: '!otherdm:test' });
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/sync').send({ agent: 'worker', owner_mxid: '@other:test', approval_room_id: '!otherdm:test', publisher_mxid: '@otherbot:test' })).status).toBe(200);
    const listed = await bridge('get', '/api/approval-bindings/matrix/markers?limit=1');
    const row = listed.body.markers[0];
    const publisher = row.marker.publisher_mxid;
    expect(row).not.toHaveProperty('request_id');
    const prepared = await bridge('post', '/api/approval-bindings/matrix/markers/prepare').send({ cas_token: row.cas_token,
      approval_room_id: row.approval_room_id, binding_generation: row.binding_generation, marker_channel: row.marker_channel,
      publisher_mxid: publisher, credential_generation: 'g1' });
    expect(prepared.body.plan).toMatchObject({ prepared_event_type: 'com.agentchat.approval.room.v1', state_key: '' });
    const identity = { cas_token: prepared.body.plan.cas_token, approval_room_id: row.approval_room_id,
      binding_generation: row.binding_generation, marker_channel: row.marker_channel,
      publisher_mxid: publisher, credential_generation: 'g1' };
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/begin-send').send(identity)).status).toBe(200);
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/receipt').send({ ...identity, event_id: '$marker' })).status).toBe(200);
    const next = await bridge('get', `/api/approval-bindings/matrix/markers?limit=1&after=${encodeURIComponent(listed.body.next)}`);
    expect(next.status).toBe(200);
    expect(next.body.markers).toHaveLength(1);
    expect((await bridge('post', '/api/approvals/fake/matrix/projections/1/prepare').send({ ...identity })).status).toBe(409);
  });

  test('v2 marker sync requires the pinned publisher context and server-derived manifest', async () => {
    expect((await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'g2',
    })).status).toBe(200);
    const base = {
      agent: 'worker', owner_mxid: '@owner:test', approval_room_id: '!dm:test',
      publisher_mxid: '@bot:test', publisher_scope: 'local_bot',
      credential_kind: 'local_bot', credential_generation: 'g2',
    };
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/sync').send({
      ...base, credential_generation: 'wrong',
    })).status).toBe(409);
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/sync').send({
      ...base, publisher_scope: 'agent:worker:test', publisher_mxid: '@ac_worker:test',
      credential_kind: 'agent_token', credential_generation: 'agent-g1',
    })).status).toBe(409);
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/sync').send({
      ...base,
      project_room_associations: [{ agent: 'worker', project_room_id: '!p:test', active: true }],
    })).status).toBe(400);
    const synced = await bridge('post', '/api/approval-bindings/matrix/markers/sync').send(base);
    expect(synced.status).toBe(200);
    expect(synced.body.marker).toMatchObject({ version: 2, marker_channel: 'room_marker_v2' });
    const listed = await bridge('get', '/api/approval-bindings/matrix/markers?limit=100');
    const row = listed.body.markers.find((item) => item.marker_channel === 'room_marker_v2');
    const prepared = await bridge('post', '/api/approval-bindings/matrix/markers/prepare').send({
      cas_token: row.cas_token, approval_room_id: row.approval_room_id,
      binding_generation: row.binding_generation, marker_channel: row.marker_channel,
      publisher_scope: row.publisher_scope, publisher_mxid: row.marker.publisher_mxid,
      credential_kind: row.credential_kind, credential_generation: row.credential_generation,
    });
    expect(prepared.status).toBe(200);
    const planIdentity = {
      cas_token: prepared.body.plan.cas_token,
      approval_room_id: row.approval_room_id,
      binding_generation: row.binding_generation,
      marker_channel: row.marker_channel,
      publisher_scope: row.publisher_scope,
      publisher_mxid: row.marker.publisher_mxid,
      credential_kind: row.credential_kind,
      credential_generation: row.credential_generation,
    };
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/begin-send')
      .send(planIdentity)).status).toBe(200);
    expect((await bridge('put', '/api/approvals/matrix/publishers').send({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'g3',
    })).status).toBe(200);
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/begin-send')
      .send(planIdentity)).status).toBe(409);
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/receipt').send({
      ...planIdentity,
      event_id: '$v2-api',
    })).status).toBe(200);

    const afterReceipt = await bridge('get', '/api/approval-bindings/matrix/markers?limit=100');
    const retirement = afterReceipt.body.markers.find((item) => (
      item.marker_channel === 'room_marker_v1_retirement'
    ));
    const retirementPrepared = await bridge('post', '/api/approval-bindings/matrix/markers/prepare').send({
      cas_token: retirement.cas_token,
      approval_room_id: retirement.approval_room_id,
      binding_generation: retirement.binding_generation,
      marker_channel: retirement.marker_channel,
      publisher_scope: retirement.publisher_scope,
      publisher_mxid: retirement.publisher_mxid,
      credential_kind: retirement.credential_kind,
      credential_generation: retirement.credential_generation,
    });
    expect(retirementPrepared.status).toBe(200);
    const retirementIdentity = {
      cas_token: retirementPrepared.body.plan.cas_token,
      approval_room_id: retirement.approval_room_id,
      binding_generation: retirement.binding_generation,
      marker_channel: retirement.marker_channel,
      publisher_scope: retirement.publisher_scope,
      publisher_mxid: retirement.publisher_mxid,
      credential_kind: retirement.credential_kind,
      credential_generation: retirement.credential_generation,
    };
    const retirementBegin = await bridge('post', '/api/approval-bindings/matrix/markers/begin-send')
      .send(retirementIdentity);
    expect(retirementBegin.status, JSON.stringify(retirementBegin.body)).toBe(200);
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/receipt').send({
      ...retirementIdentity,
      event_id: '$retirement-api',
    })).status).toBe(200);
    expect((await bridge('post', '/api/approval-bindings/matrix/markers/reconcile-retirements').send({
      approval_room_id: '!dm:test',
      legacy_state_observed_nonempty: true,
      publisher_scope: 'local_bot',
      publisher_mxid: '@bot:test',
      credential_kind: 'local_bot',
      credential_generation: 'g3',
    })).body.reconciliation).toEqual({ examined: 1, queued: 1 });
  });
});
