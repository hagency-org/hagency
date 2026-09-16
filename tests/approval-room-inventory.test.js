import { afterAll, afterEach, beforeAll, describe, expect, test } from 'vitest';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import request from 'supertest';
import { createApprovalStore } from '../lib/approval-store.js';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';

const dirs = [];
afterEach(() => dirs.splice(0).forEach(dir => rmSync(dir, { recursive: true, force: true })));
function setup() {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'approval-room-inventory-'));
  dirs.push(dir);
  const file = path.join(dir, 'approvals.json');
  return { file, store: createApprovalStore(file) };
}
function binding(agent, project, room = '!shared:test', owner = '@owner:test') {
  return { agent, project, project_room_id: `!${project}:test`, owner_mxid: owner,
    owner_dm_room_id: room, agent_joined: null };
}
function synchronize(store, candidate) {
  store.upsertProjectionPublisher({ scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
    credential_kind: 'local_bot', credential_generation: 'g1' });
  return store.syncBindingMarker({ agent: candidate.agent, owner_mxid: candidate.owner_mxid,
    approval_room_id: candidate.approval_room_id, publisher_mxid: '@bot:test', publisher_scope: 'local_bot',
    credential_kind: 'local_bot', credential_generation: 'g1' });
}
function receiptDueMarkers(store, prefix) {
  for (let index = 0; index < 8; index += 1) {
    const row = store.listDueMarkers()[0];
    if (!row) return;
    const identity = { approval_room_id: row.approval_room_id, binding_generation: row.binding_generation,
      marker_channel: row.marker_channel, publisher_mxid: '@bot:test', publisher_scope: 'local_bot',
      credential_kind: 'local_bot', credential_generation: 'g1' };
    const { plan } = store.prepareMarker(row.cas_token, identity);
    store.beginMarkerSend(plan.cas_token, identity);
    store.receiptMarker(plan.cas_token, { ...identity, event_id: `$${prefix}-${index}` });
  }
  throw new Error('fixture marker drain did not converge');
}

describe('approval room reconciliation inventory', () => {
  test('shared bindings produce one read-only room candidate', () => {
    const { store, file } = setup();
    for (const [agent, project] of [['oldprobe', 'p1'], ['claude', 'p1'], ['claude', 'p2'], ['codex', 'p2']]) {
      store.upsertBinding(binding(agent, project));
    }
    const before = readFileSync(file, 'utf8');
    expect(store.listMarkerRooms()).toEqual([{
      approval_room_id: '!shared:test', owner_mxid: '@owner:test', agent: 'claude',
      synchronization_allowed: true, conflict: false, cursor: expect.any(String),
    }]);
    expect(readFileSync(file, 'utf8')).toBe(before);
    expect(store.listBindings()).toHaveLength(4);
    expect(store.listBindings().every(item => item.agentJoined === null)).toBe(true);
    const manifest = synchronize(store, store.listMarkerRooms()[0]);
    expect(manifest.project_room_associations).toEqual([
      { agent: 'claude', project_room_id: '!p1:test', active: true },
      { agent: 'claude', project_room_id: '!p2:test', active: true },
      { agent: 'codex', project_room_id: '!p2:test', active: true },
      { agent: 'oldprobe', project_room_id: '!p1:test', active: true },
    ]);
  });

  test('inactive and retained marker-only rooms remain discoverable', () => {
    const { store } = setup();
    store.upsertBinding(binding('worker', 'p', '!inactive:test'));
    store.deactivateBinding('worker', '!p:test', 'test withdrawal');
    // Historical rows need not have pending outbox work or a surviving active binding.
    store.state.markerScopes.old = { approvalRoomId: '!legacy:test', ownerMxid: '@owner:test' };
    store.state.markerRoomScopes['!v2:test'] = { approvalRoomId: '!v2:test', ownerMxid: '@owner:test' };
    store.state.markerOutbox.push({ approvalRoomId: '!history:test', eventId: '$receipted' });
    store.state.retiredMarkerRooms['!retired:test'] = { generation: 1 };
    expect(store.listDueMarkers()).toEqual([]);
    const before = structuredClone(store.state);
    const rooms = store.listMarkerRooms();
    expect(rooms.map(item => item.approval_room_id)).toEqual([
      '!history:test', '!inactive:test', '!legacy:test', '!retired:test', '!v2:test',
    ]);
    expect(rooms.every(item => item.agent === null && !item.synchronization_allowed)).toBe(true);
    expect(store.listBindings()).toEqual([]);
    expect(store.state).toEqual(before);
  });

  test('conflicting current or retained owners have no synchronization candidate', () => {
    const { store, file } = setup();
    store.upsertBinding(binding('first', 'p1'));
    store.upsertBinding(binding('second', 'p2', '!shared:test', '@other:test'));
    store.upsertBinding(binding('third', 'p3', '!retained:test'));
    store.state.markerRoomScopes['!retained:test'] = {
      approvalRoomId: '!retained:test', ownerMxid: '@old-owner:test',
    };
    const before = readFileSync(file, 'utf8');
    expect(store.listMarkerRooms().every(item => item.conflict
      && item.owner_mxid === null && item.agent === null && !item.synchronization_allowed)).toBe(true);
    expect(readFileSync(file, 'utf8')).toBe(before);
  });

  test('receipted and deactivated shared room remains inventoried after reload', () => {
    const { store, file } = setup();
    const tuples = [['oldprobe', 'p1'], ['claude', 'p1'], ['claude', 'p2'], ['codex', 'p2']];
    for (const [agent, project] of tuples) store.upsertBinding(binding(agent, project));
    synchronize(store, store.listMarkerRooms()[0]);
    receiptDueMarkers(store, 'initial');
    for (const [agent, project] of tuples) store.deactivateBinding(agent, `!${project}:test`, 'withdrawn');
    receiptDueMarkers(store, 'deactivated');
    const loaded = createApprovalStore(file);
    expect(loaded.listDueMarkers()).toEqual([]);
    expect(loaded.listBindings()).toEqual([]);
    expect(loaded.listBindings({ includeInactive: true })).toHaveLength(4);
    const before = readFileSync(file, 'utf8');
    expect(loaded.listMarkerRooms()).toEqual([{
      approval_room_id: '!shared:test', owner_mxid: '@owner:test', agent: null,
      synchronization_allowed: false, conflict: false, cursor: expect.any(String),
    }]);
    expect(readFileSync(file, 'utf8')).toBe(before);
  });

  test('room pages use codepoint cursors and preserve late insertion on wrap', () => {
    const { store } = setup();
    for (const room of ['!z:test', '!a:test', '!Z:test', '!A:test']) {
      store.upsertBinding(binding('worker', room.slice(1, 2), room));
    }
    const first = store.listMarkerRooms({ limit: 2 });
    expect(first.map(item => item.approval_room_id)).toEqual(['!A:test', '!Z:test']);
    // The old anchor is a value, not an index into a mutable due queue.
    for (const [key, item] of Object.entries(store.state.bindings)) {
      if (item.ownerDmRoomId === '!Z:test') delete store.state.bindings[key];
    }
    store.upsertBinding(binding('new', 'new', '!B:test'));
    const next = store.listMarkerRooms({ limit: 2, after: first[1].cursor });
    expect(next.map(item => item.approval_room_id)).toEqual(['!a:test', '!z:test']);
    expect(store.listMarkerRooms({ limit: 2, after: next[1].cursor })).toEqual([]);
    expect(store.listMarkerRooms({ limit: 2 }).map(item => item.approval_room_id)).toEqual(['!A:test', '!B:test']);
  });

  test('invalid inventory cursors and limits fail without mutation', () => {
    const { store, file } = setup();
    store.upsertBinding(binding('worker', 'p'));
    const before = readFileSync(file, 'utf8');
    for (const after of ['garbage', Buffer.from('[]').toString('base64url'), 'a'.repeat(2049)]) {
      expect(() => store.listMarkerRooms({ after })).toThrow(/cursor/);
    }
    for (const limit of [0, -1, 1.5, NaN, Infinity]) {
      expect(() => store.listMarkerRooms({ limit })).toThrow(/limit/);
    }
    expect(store.listMarkerRooms({ limit: 201 })).toHaveLength(1);
    expect(readFileSync(file, 'utf8')).toBe(before);
    for (let index = 0; index < 205; index += 1) store.state.retiredMarkerRooms[`!cap${index}:test`] = {};
    expect(store.listMarkerRooms()).toHaveLength(20);
    expect(store.listMarkerRooms({ limit: 999 })).toHaveLength(200);
  });
});

describe('approval room inventory API', () => {
  let context;
  const secret = 'inventory-bridge-secret';
  const api = (method, url) => request(context.app)[method](url).set('X-Bridge-Secret', secret);
  beforeAll(async () => {
    context = await createBackendTestContext('inventory-api-', {
      agents: { worker: { name: 'worker', kind: 'agent' } },
      env: { MATRIX_BRIDGE_SECRET: secret, API_TOKEN: 'inventory-operator' },
    });
    for (let index = 0; index < 23; index += 1) {
      expect((await api('put', '/api/approval-bindings').send(binding('worker', `p${index}`, `!room${index}:test`))).status).toBe(200);
    }
  });
  afterAll(() => context?.cleanup());
  test('inventory API is bridge-secret only bounded and read-only', async () => {
    const endpoint = '/api/approval-bindings/matrix/rooms';
    const file = path.join(context.runtimeDir, 'data', 'approvals.json');
    const before = readFileSync(file, 'utf8');
    expect((await request(context.app).get(endpoint)).status).toBe(403);
    expect((await request(context.app).get(endpoint).set('Authorization', 'Bearer inventory-operator')).status).toBe(403);
    const first = await api('get', endpoint);
    expect(first.status).toBe(200);
    expect(first.body.rooms).toHaveLength(20);
    expect(first.body.next).toBe(first.body.rooms[19].cursor);
    const next = await api('get', `${endpoint}?after=${encodeURIComponent(first.body.next)}`);
    expect(next.body.rooms).toHaveLength(3);
    expect(next.body.next).toBeNull();
    expect(new Set([...first.body.rooms, ...next.body.rooms].map(item => item.approval_room_id)).size).toBe(23);
    expect(Object.keys(first.body.rooms[0]).sort()).toEqual([
      'agent', 'approval_room_id', 'conflict', 'cursor', 'owner_mxid', 'synchronization_allowed',
    ]);
    expect((await api('get', `${endpoint}?after=invalid`)).status).toBe(400);
    expect((await api('get', '/api/approval-bindings')).body.bindings).toHaveLength(23);
    expect(readFileSync(file, 'utf8')).toBe(before);
  });
});
