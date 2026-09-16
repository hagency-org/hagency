import { afterEach, describe, expect, test } from 'vitest';
import { mkdtempSync, readFileSync, rmSync } from 'fs';
import os from 'os';
import path from 'path';
import { createApprovalStore } from '../lib/approval-store.js';

const dirs = [];
afterEach(() => dirs.splice(0).forEach((dir) => rmSync(dir, { recursive: true, force: true })));
function setup(options = {}) {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'approval-marker-')); dirs.push(dir);
  const file = path.join(dir, 'store.json');
  return { store: createApprovalStore(file, options), file };
}
const binding = (project, room, dm = '!dm:test') => ({ agent: 'worker', project, project_room_id: room, owner_mxid: '@owner:test', owner_dm_room_id: dm });
const sync = (store, publisher = '@bot:test', room = '!dm:test') => store.syncBindingMarker({ agent: 'worker', owner_mxid: '@owner:test', approval_room_id: room, publisher_mxid: publisher });
const markerIdentity = (row, plan) => ({ approval_room_id: row.approval_room_id, binding_generation: row.binding_generation,
  marker_channel: row.marker_channel, publisher_mxid: plan.publisher_mxid, credential_generation: plan.credential_generation });

describe('approval binding marker store', () => {
  test('marker generations track old room new room duplicate and publisher changes', () => {
    const { store } = setup();
    store.upsertBinding(binding('a', '!a:test')); store.upsertBinding(binding('b', '!b:test'));
    expect(sync(store).binding_generation).toBe(1);
    expect(sync(store).binding_generation).toBe(1);
    store.upsertBinding(binding('a', '!a:test', '!new:test'));
    expect(store.listDueMarkers().find((row) => row.approval_room_id === '!dm:test')).toMatchObject({
      binding_generation: 2,
      marker: { project_room_associations: [{ project_room_id: '!a:test', active: false }, { project_room_id: '!b:test', active: true }] },
    });
    expect(sync(store, '@newbot:test').binding_generation).toBe(3);
    expect(sync(store, '@newbot:test', '!new:test')).toMatchObject({ binding_generation: 1, approval_room_id: '!new:test' });
  });

  test('marker overflow and persistence faults roll back memory and disk', () => {
    const { store, file } = setup();
    for (let i = 0; i < 64; i += 1) store.upsertBinding(binding(`p${i}`, `!p${i}:test`));
    sync(store); const before = readFileSync(file, 'utf8');
    expect(() => store.upsertBinding(binding('overflow', '!overflow:test'))).toThrow(/limit/);
    expect(readFileSync(file, 'utf8')).toBe(before);
    expect(store.listBindings()).toHaveLength(64);
    store.fsFault = (phase) => { if (phase === 'beforeRename') throw new Error('fixture'); };
    expect(() => sync(store, '@changed:test')).toThrow(/persist/);
    expect(readFileSync(file, 'utf8')).toBe(before);
  });

  test('superseded ready markers cannot begin while attempted markers can receipt after reload', () => {
    const { store, file } = setup(); store.upsertBinding(binding('a', '!a:test')); sync(store);
    const row = store.listDueMarkers()[0];
    const plan = store.prepareMarker(row.cas_token, { ...markerIdentity(row, { publisher_mxid: '@bot:test', credential_generation: 'g1' }), credential_generation: 'g1' }).plan;
    store.beginMarkerSend(plan.cas_token, markerIdentity(row, plan));
    sync(store, '@next:test');
    const reloaded = createApprovalStore(file);
    expect(reloaded.receiptMarker(plan.cas_token, { ...markerIdentity(row, plan), event_id: '$old-attempt' })).toEqual({ event_id: '$old-attempt' });
    const latest = reloaded.listDueMarkers().find((item) => item.binding_generation === 2);
    const latestPlan = reloaded.prepareMarker(latest.cas_token, { approval_room_id: latest.approval_room_id,
      binding_generation: latest.binding_generation, marker_channel: latest.marker_channel,
      publisher_mxid: '@next:test', credential_generation: 'g2' }).plan;
    reloaded.upsertBinding(binding('b', '!b:test'));
    expect(() => reloaded.beginMarkerSend(latestPlan.cas_token, markerIdentity(latest, latestPlan))).toThrow(/superseded/);
  });

  test('marker payload namespace contains no approval request identity', () => {
    const { store } = setup(); store.upsertBinding(binding('a', '!a:test')); sync(store);
    const row = store.listDueMarkers()[0];
    expect(row.marker).toEqual({ version: 1, binding_generation: 1, publisher_mxid: '@bot:test', owner_mxid: '@owner:test',
      agent: 'worker', project_room_associations: [{ project_room_id: '!a:test', active: true }] });
    const serialized = JSON.stringify(row);
    expect(serialized).not.toMatch(/request_id|upstream|input_digest|tool_name/);
  });

  test('marker preparation is first-writer-wins and ready work cannot retry or receipt', () => {
    const { store, file } = setup(); store.upsertBinding(binding('a', '!a:test')); sync(store);
    const row = store.listDueMarkers()[0];
    const first = store.prepareMarker(row.cas_token, { approval_room_id: row.approval_room_id,
      binding_generation: row.binding_generation, marker_channel: row.marker_channel,
      publisher_mxid: '@bot:test', credential_generation: 'g1' }).plan;
    const second = store.prepareMarker(row.cas_token, { approval_room_id: row.approval_room_id,
      binding_generation: row.binding_generation, marker_channel: row.marker_channel,
      publisher_mxid: '@other:test', credential_generation: 'g2' }).plan;
    expect(second).toEqual(first);
    const exact = markerIdentity(row, first); const before = readFileSync(file, 'utf8');
    expect(() => store.retryMarker(first.cas_token, { ...exact, retry_at: 2000 })).toThrow(/has not begun/);
    expect(() => store.receiptMarker(first.cas_token, { ...exact, event_id: '$early' })).toThrow(/receipt mismatch/);
    expect(readFileSync(file, 'utf8')).toBe(before);
  });

  test('one Matrix room rejects a second agent marker scope without mutation', () => {
    const { store, file } = setup();
    store.upsertBinding(binding('a', '!a:test')); sync(store);
    store.upsertBinding({ agent: 'beta', project: 'b', project_room_id: '!b:test', owner_mxid: '@owner:test', owner_dm_room_id: '!dm:test' });
    const before = readFileSync(file, 'utf8');
    expect(() => store.syncBindingMarker({ agent: 'beta', owner_mxid: '@owner:test', approval_room_id: '!dm:test', publisher_mxid: '@bot:test' }))
      .toThrow(/another agent scope/);
    expect(readFileSync(file, 'utf8')).toBe(before);
    expect(store.listDueMarkers()).toHaveLength(1);
  });

  test('marker cursor uses the same code-point order as marker sorting', () => {
    const { store } = setup();
    store.upsertBinding(binding('a', '!a-project:test', '!a:test')); sync(store, '@bot:test', '!a:test');
    store.upsertBinding(binding('b', '!b-project:test', '!B:test')); sync(store, '@bot:test', '!B:test');
    const first = store.listDueMarkers({ limit: 1 });
    expect(first).toHaveLength(1);
    const second = store.listDueMarkers({ limit: 1, after: first[0].cursor });
    expect(second).toHaveLength(1);
    expect(second[0].approval_room_id).not.toBe(first[0].approval_room_id);
  });
});
