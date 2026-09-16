import { afterEach, describe, expect, test } from 'vitest';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createApprovalStore } from '../lib/approval-store.js';

const dirs = [];
afterEach(() => dirs.splice(0).forEach((dir) => rmSync(dir, { recursive: true, force: true })));

function setup(options = {}) {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'approval-shared-marker-'));
  dirs.push(dir);
  const file = path.join(dir, 'store.json');
  const store = createApprovalStore(file, options);
  store.upsertProjectionPublisher({
    scope: 'local_bot', publisher_mxid: '@private-publisher:test', homeserver: 'https://test',
    credential_kind: 'local_bot', credential_generation: 'private-g1',
  });
  return { store, file };
}

const OWNER = '@owner:test';
const ROOM = '!owner-dm:test';
function binding(agent, project, projectRoom, agentJoined = null, owner = OWNER) {
  return {
    agent, project, project_room_id: projectRoom, owner_mxid: owner,
    owner_dm_room_id: ROOM, agent_joined: agentJoined,
  };
}
function seedActualFour(store) {
  store.upsertBinding(binding('oldprobe', 'project1', '!project1:test', null));
  store.upsertBinding(binding('claude', 'project1', '!project1:test', null));
  store.upsertBinding(binding('claude', 'project2', '!project2:test', null));
  store.upsertBinding(binding('codex', 'project2', '!project2:test', true));
}

function syncRoom(store, over = {}) {
  return store.syncBindingMarker({
    agent: 'claude', owner_mxid: OWNER, approval_room_id: ROOM,
    publisher_mxid: '@private-publisher:test', publisher_scope: 'local_bot',
    credential_kind: 'local_bot', credential_generation: 'private-g1', ...over,
  });
}
function markerIdentity(row, plan) {
  return {
    approval_room_id: row.approval_room_id,
    binding_generation: row.binding_generation,
    marker_channel: row.marker_channel,
    publisher_mxid: plan.publisher_mxid,
    credential_generation: plan.credential_generation,
  };
}

describe('shared approval room marker v2 contract', () => {
  test('four bindings across three agents and two projects form one room manifest', () => {
    const { store } = setup();
    seedActualFour(store);
    const result = syncRoom(store);
    expect(result).toMatchObject({ approval_room_id: ROOM, marker_channel: 'room_marker_v2' });
    expect(result).toMatchObject({
      version: 2, binding_generation: 1, publisher_mxid: '@private-publisher:test', owner_mxid: OWNER,
      project_room_associations: [
        { agent: 'claude', project_room_id: '!project1:test', active: true },
        { agent: 'claude', project_room_id: '!project2:test', active: true },
        { agent: 'codex', project_room_id: '!project2:test', active: true },
        { agent: 'oldprobe', project_room_id: '!project1:test', active: true },
      ],
    });
  });

  test('room manifest rejects mixed owner or private publisher without partial state', () => {
    const { store, file } = setup();
    seedActualFour(store);
    syncRoom(store);
    const before = JSON.parse(readFileSync(file, 'utf8'));
    expect(() => syncRoom(store, {
      agent: 'codex', publisher_mxid: '@other-publisher:test',
    })).toThrow(/publisher context mismatch/i);
    const beforeConflict = JSON.parse(readFileSync(file, 'utf8'));
    store.upsertBinding(binding('beta', 'project3', '!project3:test', null, '@other-owner:test'));
    expect(store.listBindings({ agent: 'beta' })).toHaveLength(1);
    expect(() => syncRoom(store)).toThrow(/common.*owner/i);
    const after = JSON.parse(readFileSync(file, 'utf8'));
    expect(Object.keys(after.bindings)).toHaveLength(Object.keys(before.bindings).length + 1);
    expect(after.requests).toEqual(before.requests);
    expect(after.markerRoomScopes).toEqual(beforeConflict.markerRoomScopes);
    expect(after.markerOutbox).toHaveLength(beforeConflict.markerOutbox.length);
    expect(after.markerOutbox.every((row) => row.superseded)).toBe(true);
  });

  test('duplicate tuples and overflow reject while distinct agents in one project remain valid', () => {
    const { store, file } = setup();
    store.upsertBinding(binding('alpha', 'shared', '!shared:test'));
    store.upsertBinding(binding('beta', 'shared', '!shared:test'));
    expect(() => syncRoom(store, {
      agent: 'alpha', project_room_associations: [
        { agent: 'alpha', project_room_id: '!shared:test', active: true },
        { agent: 'alpha', project_room_id: '!shared:test', active: true },
      ],
    })).toThrow(/caller|association|duplicate/i);
    expect(() => syncRoom(store, {
      agent: 'alpha',
      project_room_associations: [
        { agent: 'alpha', project_room_id: '!shared:test', active: true },
      ],
    })).toThrow(/caller|association/i);
    expect(syncRoom(store, { agent: 'alpha' }).project_room_associations).toEqual([
      { agent: 'alpha', project_room_id: '!shared:test', active: true },
      { agent: 'beta', project_room_id: '!shared:test', active: true },
    ]);
    for (let i = 0; i < 62; i += 1) {
      store.upsertBinding(binding(`agent${i}`, `project${i}`, `!room${i}:test`));
    }
    store.upsertBinding(binding('overflow', 'overflow', '!overflow:test'));
    expect(store.listBindings({ agent: 'overflow' })).toHaveLength(1);
    const before = readFileSync(file, 'utf8');
    expect(() => syncRoom(store, { agent: 'alpha' })).toThrow(/64|limit|overflow/);
    expect(readFileSync(file, 'utf8')).toBe(before);
  });

  test('v1 scopes migrate above room high-water across batches restart mutation and rollback', () => {
    const { store, file } = setup();
    seedActualFour(store);
    store.upsertBinding({ ...binding('zeta', 'project-z', '!project-z:test'), owner_dm_room_id: '!z-room:test' });
    store.state.markerScopes = {
      'oldprobe\0!owner-dm:test': { agent: 'oldprobe', approvalRoomId: ROOM, ownerMxid: OWNER, publisherMxid: '@private-publisher:test', generation: 4, associations: [] },
      'zeta\0!z-room:test': { agent: 'zeta', approvalRoomId: '!z-room:test', ownerMxid: OWNER, publisherMxid: '@private-publisher:test', generation: 7, associations: [] },
    };
    store.state.markerOutbox = [];
    store._save();
    const firstBatch = store.migrateMarkerRoomsV2({ limit: 1 });
    expect(firstBatch).toMatchObject({ examined: 1, complete: false, cursor: ROOM });
    store.upsertBinding({ ...binding('beta', 'project-z', '!project-z:test'), owner_dm_room_id: '!z-room:test' });
    const reloaded = createApprovalStore(file);
    reloaded.fsFault = (phase) => { if (phase === 'beforeRename') throw new Error('migration fault'); };
    const before = readFileSync(file, 'utf8');
    expect(() => reloaded.migrateMarkerRoomsV2({ limit: 1 })).toThrow(/persist|migration fault/);
    expect(readFileSync(file, 'utf8')).toBe(before);
    reloaded.fsFault = () => {};
    expect(reloaded.migrateMarkerRoomsV2({ limit: 1 })).toMatchObject({ examined: 1, complete: true });
    const v2Rows = reloaded.listDueMarkers().filter((row) => row.marker_channel === 'room_marker_v2');
    expect(v2Rows).toHaveLength(2);
    expect(v2Rows.find((row) => row.approval_room_id === '!z-room:test').binding_generation).toBeGreaterThan(7);
  });

  test('exact inactive room migration validates its publisher and rolls back without moving the global cursor', () => {
    const { store, file } = setup();
    store.state.markerScopes = Object.fromEntries(['!A:test', '!B:test'].map((room, i) => [
      `worker\0${room}`, { agent: 'worker', approvalRoomId: room, ownerMxid: OWNER,
        publisherMxid: '@private-publisher:test', generation: i + 3,
        associations: [{ project_room_id: `!project-${i}:test`, active: false }] },
    ]));
    store.state.markerRoomMigration = { cursor: '!previous:test', complete: false };
    store._save();
    const identity = { approval_room_id: '!B:test', limit: 1, publisher_scope: 'local_bot',
      publisher_mxid: '@private-publisher:test', credential_kind: 'local_bot', credential_generation: 'private-g1' };
    const before = readFileSync(file, 'utf8');
    const memory = JSON.stringify(store.state);
    for (const invalid of [
      { approval_room_id: null }, { approval_room_id: 'not-a-room' },
      { publisher_scope: 'side-representative:test' }, { publisher_mxid: '@other:test' },
      { credential_kind: 'appservice' }, { credential_generation: 'stale' },
    ]) {
      expect(() => store.migrateMarkerRoomsV2({ ...identity, ...invalid })).toThrow();
      expect(JSON.stringify(store.state)).toBe(memory);
      expect(readFileSync(file, 'utf8')).toBe(before);
    }
    store.fsFault = phase => { if (phase === 'beforeRename') throw new Error('exact migration write failed'); };
    expect(() => store.migrateMarkerRoomsV2(identity)).toThrow(/persist|exact migration/);
    expect(JSON.stringify(store.state)).toBe(memory);
    expect(readFileSync(file, 'utf8')).toBe(before);
    store.fsFault = () => {};
    expect(store.migrateMarkerRoomsV2(identity)).toEqual({ examined: 1, complete: true, cursor: null });
    expect(store.state.markerRoomMigration).toEqual({ cursor: '!previous:test', complete: false });
    expect(store.listDueMarkers()).toHaveLength(1);
    expect(store.listDueMarkers()[0]).toMatchObject({ approval_room_id: '!B:test', marker_channel: 'room_marker_v2' });
    expect(store.state.markerRoomScopes).not.toHaveProperty('!A:test');
    expect(store.state.bindings).toEqual({});
    const reloaded = createApprovalStore(file);
    expect(reloaded.state.markerRoomMigration).toEqual({ cursor: '!previous:test', complete: false });
    expect(reloaded.listDueMarkers()).toEqual(store.listDueMarkers());
  });

  test('v2 receipt queues fixed retirement and preserves old attempted receipt semantics', () => {
    const { store, file } = setup();
    seedActualFour(store);
    syncRoom(store);
    const v2 = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const plan = store.prepareMarker(v2.cas_token, {
      approval_room_id: ROOM, binding_generation: v2.binding_generation, marker_channel: v2.marker_channel,
      publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1',
    }).plan;
    expect(plan).toMatchObject({
      publisher_scope: 'local_bot', credential_kind: 'local_bot',
      credential_generation: 'private-g1', prepared_event_type: 'com.agentchat.approval.room.v2',
      state_key: '',
    });
    const identity = { approval_room_id: ROOM, binding_generation: v2.binding_generation,
      marker_channel: v2.marker_channel, publisher_mxid: plan.publisher_mxid,
      credential_generation: plan.credential_generation };
    store.beginMarkerSend(plan.cas_token, identity);
    store.receiptMarker(plan.cas_token, { ...identity, event_id: '$v2' });
    const reloaded = createApprovalStore(file);
    const retirement = reloaded.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v1_retirement');
    const retirementPlan = reloaded.prepareMarker(retirement.cas_token, {
      approval_room_id: ROOM, binding_generation: retirement.binding_generation,
      marker_channel: retirement.marker_channel, publisher_mxid: plan.publisher_mxid,
      credential_generation: plan.credential_generation,
    }).plan;
    expect(retirementPlan).toMatchObject({
      prepared_payload: {}, prepared_event_type: 'com.agentchat.approval.room.v1', state_key: '',
    });
    reloaded.beginMarkerSend(retirementPlan.cas_token, markerIdentity(retirement, retirementPlan));
    reloaded.receiptMarker(retirementPlan.cas_token, {
      ...markerIdentity(retirement, retirementPlan), event_id: '$retired-v1',
    });
    expect(() => reloaded.syncBindingMarker({
      agent: 'claude', owner_mxid: OWNER, approval_room_id: ROOM,
      publisher_mxid: '@private-publisher:test',
    })).toThrow(/retired/);
  });

  test('equal replay stable cursor and due channels survive completion', () => {
    const { store } = setup();
    for (const [agent, room] of [['upper', '!A:test'], ['lower', '!a:test'], ['accent', '!Á:test']]) {
      store.upsertBinding({ ...binding(agent, agent, `!${agent}:test`), owner_dm_room_id: room });
      store.syncBindingMarker({
        agent, owner_mxid: OWNER, approval_room_id: room,
        publisher_mxid: '@private-publisher:test', publisher_scope: 'local_bot',
        credential_kind: 'local_bot', credential_generation: 'private-g1',
      });
    }
    const replay = store.syncBindingMarker({
      agent: 'upper', owner_mxid: OWNER, approval_room_id: '!A:test',
      publisher_mxid: '@private-publisher:test', publisher_scope: 'local_bot',
      credential_kind: 'local_bot', credential_generation: 'private-g1',
    });
    expect(replay.binding_generation).toBe(1);

    const page = store.listDueMarkers({ limit: 2 });
    expect(page.map((row) => row.approval_room_id)).toEqual(['!A:test', '!a:test']);
    const anchor = page[1];
    const plan = store.prepareMarker(anchor.cas_token, {
      approval_room_id: anchor.approval_room_id, binding_generation: anchor.binding_generation,
      marker_channel: anchor.marker_channel, publisher_mxid: '@private-publisher:test',
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(plan.cas_token, markerIdentity(anchor, plan));
    store.receiptMarker(plan.cas_token, { ...markerIdentity(anchor, plan), event_id: '$anchor' });

    const next = store.listDueMarkers({ limit: 2, after: anchor.cursor });
    expect(next.map((row) => row.approval_room_id)).toEqual(['!Á:test']);
  });

  test('marker synchronization rolls back its aggregate without rolling back canonical bindings', () => {
    const { store, file } = setup();
    seedActualFour(store);
    syncRoom(store);
    store.upsertBinding(binding('beta', 'project3', '!project3:test'));
    store.upsertProjectionPublisher({
      scope: 'local_bot', publisher_mxid: '@private-publisher:test', homeserver: 'https://test',
      credential_kind: 'local_bot', credential_generation: 'private-g2',
    });
    const before = readFileSync(file, 'utf8');
    const bindingsBefore = store.listBindings({ agent: 'beta' });
    store.fsFault = (phase) => {
      if (phase === 'beforeRename') throw new Error('sync rename fault');
    };

    expect(() => syncRoom(store, { credential_generation: 'private-g2' }))
      .toThrow(/persist|sync rename fault/);
    expect(readFileSync(file, 'utf8')).toBe(before);
    expect(store.listBindings({ agent: 'beta' })).toEqual(bindingsBefore);
  });


  test('v2 supersedes old ready work but preserves exact attempted receipt', () => {
    const { store } = setup();
    seedActualFour(store);
    const legacy = {
      agent: 'claude', approvalRoomId: ROOM, ownerMxid: OWNER,
      publisherMxid: '@private-publisher:test', generation: 0, associations: [],
    };
    store.state.markerScopes[`claude\0${ROOM}`] = legacy;
    store._advanceMarker(legacy, store._markerAssociations(legacy));
    store._save();
    const oldReadyRow = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker');
    const oldReadyPlan = store.prepareMarker(oldReadyRow.cas_token, {
      ...markerIdentity(oldReadyRow, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store._advanceMarker(legacy, [
      ...legacy.associations,
      { project_room_id: '!retired:test', active: false },
    ]);
    store._save();
    expect(() => store.beginMarkerSend(oldReadyPlan.cas_token, markerIdentity(oldReadyRow, oldReadyPlan)))
      .toThrow(/superseded/);

    const oldAttemptRow = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker');
    const oldAttemptPlan = store.prepareMarker(oldAttemptRow.cas_token, {
      ...markerIdentity(oldAttemptRow, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(oldAttemptPlan.cas_token, markerIdentity(oldAttemptRow, oldAttemptPlan));
    syncRoom(store);
    expect(store.receiptMarker(oldAttemptPlan.cas_token, {
      ...markerIdentity(oldAttemptRow, oldAttemptPlan), event_id: '$old-attempted',
    })).toEqual({ event_id: '$old-attempted' });
  });


  test('post-rename migration commit reloads durable cursor and aggregate before recovery', () => {
    const { store, file } = setup();
    store.upsertBinding(binding('claude', 'project1', '!project1:test'));
    store.state.markerScopes = {
      [`claude\0${ROOM}`]: {
        agent: 'claude', approvalRoomId: ROOM, ownerMxid: OWNER,
        publisherMxid: '@private-publisher:test', generation: 3,
        associations: [{ project_room_id: '!project1:test', active: true }],
      },
    };
    store._save();
    store.fsFault = (phase) => {
      if (phase === 'afterRename') throw new Error('directory sync fault');
    };

    expect(store.migrateMarkerRoomsV2({ limit: 1 })).toMatchObject({ examined: 1, complete: true });
    expect(store.persistenceHealth()).toMatchObject({ degraded: true });
    expect(() => store.migrateMarkerRoomsV2({ limit: 1 })).toThrow(/reload|durability/);

    const reloaded = createApprovalStore(file);
    expect(reloaded.persistenceHealth()).toMatchObject({ degraded: false });
    expect(reloaded.state.markerRoomMigration).toMatchObject({ cursor: ROOM, complete: true });
    expect(reloaded.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2'))
      .toMatchObject({ approval_room_id: ROOM, binding_generation: 4 });
    reloaded.upsertBinding({
      ...binding('alpha', 'early', '!early:test'), owner_dm_room_id: '!A:test',
    });
    reloaded.state.markerScopes[`alpha\0!A:test`] = {
      agent: 'alpha', approvalRoomId: '!A:test', ownerMxid: OWNER,
      publisherMxid: '@private-publisher:test', generation: 2,
      associations: [{ project_room_id: '!early:test', active: true }],
    };
    reloaded._save();
    expect(reloaded.migrateMarkerRoomsV2({ limit: 1 })).toMatchObject({
      examined: 1, complete: true, cursor: '!A:test',
    });
    expect(reloaded.listDueMarkers()).toContainEqual(expect.objectContaining({
      approval_room_id: '!A:test', marker_channel: 'room_marker_v2',
    }));
  });


  test('failed v1 retirement remains pending reconciliation', () => {
    let now = 1000;
    const { store } = setup({ now: () => now });
    seedActualFour(store);
    syncRoom(store);
    const v2 = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const v2Plan = store.prepareMarker(v2.cas_token, {
      ...markerIdentity(v2, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(v2Plan.cas_token, markerIdentity(v2, v2Plan));
    store.receiptMarker(v2Plan.cas_token, { ...markerIdentity(v2, v2Plan), event_id: '$v2' });
    const retirement = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v1_retirement');
    const plan = store.prepareMarker(retirement.cas_token, {
      ...markerIdentity(retirement, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(plan.cas_token, markerIdentity(retirement, plan));
    store.retryMarker(plan.cas_token, {
      ...markerIdentity(retirement, plan), retry_at: 2000, error_code: 'transport_unknown',
    });
    expect(store.listDueMarkers()).not.toContainEqual(expect.objectContaining({
      marker_channel: 'room_marker_v1_retirement',
    }));
    now = 2000;
    expect(store.listDueMarkers()).toContainEqual(expect.objectContaining({
      marker_channel: 'room_marker_v1_retirement',
      plan: expect.objectContaining({ attempt_state: 'uncertain' }),
    }));
  });

  test('new room agent supersedes stale ready plan and appears in the next manifest', () => {
    const { store } = setup();
    seedActualFour(store);
    syncRoom(store);
    const old = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const plan = store.prepareMarker(old.cas_token, {
      ...markerIdentity(old, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.upsertBinding(binding('beta', 'project3', '!project3:test'));
    const current = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    expect(current.marker.project_room_associations).toContainEqual({
      agent: 'beta', project_room_id: '!project3:test', active: true,
    });
    expect(() => store.beginMarkerSend(plan.cas_token, markerIdentity(old, plan))).toThrow(/superseded/);
  });

  test('owner conflict persists binding but blocks stale room plan from beginning', () => {
    const { store } = setup();
    seedActualFour(store);
    syncRoom(store);
    const old = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const plan = store.prepareMarker(old.cas_token, {
      ...markerIdentity(old, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.upsertBinding(binding('beta', 'project3', '!project3:test', null, '@other-owner:test'));
    expect(store.listBindings({ agent: 'beta' })).toHaveLength(1);
    expect(() => syncRoom(store)).toThrow(/common.*owner/);
    expect(() => store.beginMarkerSend(plan.cas_token, markerIdentity(old, plan))).toThrow(/superseded/);
  });

  test('publisher rotation blocks ready work but keeps exact attempted receipt admissible', () => {
    const readyStore = setup().store;
    seedActualFour(readyStore);
    syncRoom(readyStore);
    const readyRow = readyStore.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const readyPlan = readyStore.prepareMarker(readyRow.cas_token, {
      ...markerIdentity(readyRow, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    readyStore.upsertProjectionPublisher({
      scope: 'local_bot', publisher_mxid: '@private-publisher:test', homeserver: 'https://test',
      credential_kind: 'local_bot', credential_generation: 'private-g2',
    });
    expect(() => readyStore.beginMarkerSend(readyPlan.cas_token, markerIdentity(readyRow, readyPlan)))
      .toThrow(/no longer current/);

    const attemptedStore = setup().store;
    seedActualFour(attemptedStore);
    syncRoom(attemptedStore);
    const attemptedRow = attemptedStore.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const attemptedPlan = attemptedStore.prepareMarker(attemptedRow.cas_token, {
      ...markerIdentity(attemptedRow, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    attemptedStore.beginMarkerSend(attemptedPlan.cas_token, markerIdentity(attemptedRow, attemptedPlan));
    attemptedStore.upsertProjectionPublisher({
      scope: 'local_bot', publisher_mxid: '@private-publisher:test', homeserver: 'https://test',
      credential_kind: 'local_bot', credential_generation: 'private-g2',
    });
    expect(attemptedStore.receiptMarker(attemptedPlan.cas_token, {
      ...markerIdentity(attemptedRow, attemptedPlan), event_id: '$late-exact',
    })).toEqual({ event_id: '$late-exact' });
  });

  test('each accepted v2 revision retains eligible retirement reconciliation work', () => {
    const { store } = setup();
    seedActualFour(store);
    syncRoom(store);
    const first = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const firstPlan = store.prepareMarker(first.cas_token, {
      ...markerIdentity(first, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(firstPlan.cas_token, markerIdentity(first, firstPlan));
    store.receiptMarker(firstPlan.cas_token, { ...markerIdentity(first, firstPlan), event_id: '$v2-first' });
    store.upsertBinding(binding('claude', 'project3', '!project3:test'));
    const second = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const secondPlan = store.prepareMarker(second.cas_token, {
      ...markerIdentity(second, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(secondPlan.cas_token, markerIdentity(second, secondPlan));
    store.receiptMarker(secondPlan.cas_token, { ...markerIdentity(second, secondPlan), event_id: '$v2-second' });
    const retirements = store.state.markerOutbox.filter((row) => row.markerChannel === 'room_marker_v1_retirement');
    expect(retirements).toHaveLength(2);
    expect(retirements.filter((row) => !row.superseded && !row.eventId)).toHaveLength(1);
  });

  test('late v1 receipt queues one bounded retirement reconciliation', () => {
    const { store } = setup();
    seedActualFour(store);
    const legacy = {
      agent: 'claude', approvalRoomId: ROOM, ownerMxid: OWNER,
      publisherMxid: '@private-publisher:test', generation: 0, associations: [],
    };
    store.state.markerScopes[`claude\0${ROOM}`] = legacy;
    store._advanceMarker(legacy, store._markerAssociations(legacy));
    store._save();
    const old = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker');
    const oldPlan = store.prepareMarker(old.cas_token, {
      ...markerIdentity(old, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(oldPlan.cas_token, markerIdentity(old, oldPlan));

    syncRoom(store);
    const v2 = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v2');
    const v2Plan = store.prepareMarker(v2.cas_token, {
      ...markerIdentity(v2, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(v2Plan.cas_token, markerIdentity(v2, v2Plan));
    store.receiptMarker(v2Plan.cas_token, { ...markerIdentity(v2, v2Plan), event_id: '$v2-before-retire' });
    const retirement = store.listDueMarkers().find((row) => row.marker_channel === 'room_marker_v1_retirement');
    const retirementPlan = store.prepareMarker(retirement.cas_token, {
      ...markerIdentity(retirement, { publisher_mxid: '@private-publisher:test', credential_generation: 'private-g1' }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(retirementPlan.cas_token, markerIdentity(retirement, retirementPlan));
    store.receiptMarker(retirementPlan.cas_token, {
      ...markerIdentity(retirement, retirementPlan), event_id: '$retirement-first',
    });
    store.receiptMarker(oldPlan.cas_token, {
      ...markerIdentity(old, oldPlan), event_id: '$late-v1',
    });

    expect(store.reconcileMarkerRetirements({
      approval_room_id: ROOM,
      limit: 1,
    })).toEqual({ examined: 1, queued: 1 });
    expect(store.listDueMarkers()).toContainEqual(expect.objectContaining({
      marker_channel: 'room_marker_v1_retirement', marker: {},
    }));
    expect(store.reconcileMarkerRetirements({
      approval_room_id: ROOM,
      limit: 1,
    })).toEqual({ examined: 1, queued: 0 });
  });

  test('conflicting binding blocks uncertain resend but preserves its exact receipt', () => {
    const { store } = setup();
    seedActualFour(store);
    syncRoom(store);
    const row = store.listDueMarkers().find((item) => item.marker_channel === 'room_marker_v2');
    const plan = store.prepareMarker(row.cas_token, {
      ...markerIdentity(row, {
        publisher_mxid: '@private-publisher:test',
        credential_generation: 'private-g1',
      }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(plan.cas_token, markerIdentity(row, plan));
    store.retryMarker(plan.cas_token, {
      ...markerIdentity(row, plan),
      retry_at: 0,
      error_code: 'unknown',
    });

    store.upsertBinding(binding('beta', 'project3', '!project3:test', null, '@other-owner:test'));

    expect(store.listDueMarkers()).not.toContainEqual(expect.objectContaining({
      cas_token: row.cas_token,
    }));
    expect(() => store.beginMarkerSend(plan.cas_token, markerIdentity(row, plan)))
      .toThrow(/superseded/);
    expect(() => store.retryMarker(plan.cas_token, markerIdentity(row, plan)))
      .toThrow(/superseded/);
    expect(store.receiptMarker(plan.cas_token, {
      ...markerIdentity(row, plan),
      event_id: '$late-conflicted-receipt',
    })).toEqual({ event_id: '$late-conflicted-receipt' });
  });

  test('removing a conflicting binding creates fresh work after invalidation', () => {
    const { store } = setup();
    seedActualFour(store);
    syncRoom(store);
    const old = store.listDueMarkers().find((item) => item.marker_channel === 'room_marker_v2');
    store.prepareMarker(old.cas_token, {
      ...markerIdentity(old, {
        publisher_mxid: '@private-publisher:test',
        credential_generation: 'private-g1',
      }),
      credential_generation: 'private-g1',
    });
    store.upsertBinding(binding('beta', 'project3', '!project3:test', null, '@other-owner:test'));
    expect(store.removeBinding('beta', '!project3:test')).not.toBeNull();

    const synced = syncRoom(store);
    expect(synced.binding_generation).toBeGreaterThan(old.binding_generation);
    expect(store.listDueMarkers()).toContainEqual(expect.objectContaining({
      approval_room_id: ROOM,
      binding_generation: synced.binding_generation,
      marker_channel: 'room_marker_v2',
    }));
  });

  test('publisher rotation replaces an unusable pending retirement', () => {
    const { store } = setup();
    seedActualFour(store);
    syncRoom(store);
    const first = store.listDueMarkers().find((item) => item.marker_channel === 'room_marker_v2');
    const firstPlan = store.prepareMarker(first.cas_token, {
      ...markerIdentity(first, {
        publisher_mxid: '@private-publisher:test',
        credential_generation: 'private-g1',
      }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(firstPlan.cas_token, markerIdentity(first, firstPlan));
    store.receiptMarker(firstPlan.cas_token, {
      ...markerIdentity(first, firstPlan),
      event_id: '$v2-g1',
    });
    store.upsertProjectionPublisher({
      scope: 'local_bot',
      publisher_mxid: '@private-publisher:test',
      homeserver: 'https://test',
      credential_kind: 'local_bot',
      credential_generation: 'private-g2',
    });
    syncRoom(store, { credential_generation: 'private-g2' });
    const second = store.listDueMarkers().find((item) => item.marker_channel === 'room_marker_v2');
    const secondPlan = store.prepareMarker(second.cas_token, {
      ...markerIdentity(second, {
        publisher_mxid: '@private-publisher:test',
        credential_generation: 'private-g2',
      }),
      credential_generation: 'private-g2',
    }).plan;
    store.beginMarkerSend(secondPlan.cas_token, markerIdentity(second, secondPlan));
    store.receiptMarker(secondPlan.cas_token, {
      ...markerIdentity(second, secondPlan),
      event_id: '$v2-g2',
    });

    const retirements = store.state.markerOutbox.filter((row) => (
      row.markerChannel === 'room_marker_v1_retirement'
    ));
    expect(retirements.find((row) => row.credentialGeneration === 'private-g1')?.superseded)
      .toBe(true);
    expect(retirements).toContainEqual(expect.objectContaining({
      credentialGeneration: 'private-g2',
      superseded: false,
      eventId: null,
    }));
  });

  test('observed legacy state requeues retirement without fabricating a legacy receipt', () => {
    const { store } = setup();
    seedActualFour(store);
    syncRoom(store);
    const v2 = store.listDueMarkers().find((item) => item.marker_channel === 'room_marker_v2');
    const v2Plan = store.prepareMarker(v2.cas_token, {
      ...markerIdentity(v2, {
        publisher_mxid: '@private-publisher:test',
        credential_generation: 'private-g1',
      }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(v2Plan.cas_token, markerIdentity(v2, v2Plan));
    store.receiptMarker(v2Plan.cas_token, {
      ...markerIdentity(v2, v2Plan),
      event_id: '$v2-before-observation',
    });
    const retirement = store.listDueMarkers().find((item) => (
      item.marker_channel === 'room_marker_v1_retirement'
    ));
    const retirementPlan = store.prepareMarker(retirement.cas_token, {
      ...markerIdentity(retirement, {
        publisher_mxid: '@private-publisher:test',
        credential_generation: 'private-g1',
      }),
      credential_generation: 'private-g1',
    }).plan;
    store.beginMarkerSend(retirementPlan.cas_token, markerIdentity(retirement, retirementPlan));
    store.receiptMarker(retirementPlan.cas_token, {
      ...markerIdentity(retirement, retirementPlan),
      event_id: '$retired-before-lost-v1-result',
    });

    const legacyReceipts = store.state.markerOutbox.filter((row) => (
      row.markerChannel === 'room_marker' && row.eventId
    ));
    expect(legacyReceipts).toHaveLength(0);
    expect(store.reconcileMarkerRetirements({
      approval_room_id: ROOM,
      legacy_state_observed_nonempty: true,
      limit: 1,
    })).toEqual({ examined: 1, queued: 1 });
    expect(store.listDueMarkers()).toContainEqual(expect.objectContaining({
      approval_room_id: ROOM,
      marker_channel: 'room_marker_v1_retirement',
      marker: {},
    }));
  });

});
