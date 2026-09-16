import { afterEach, describe, expect, test } from 'vitest';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'fs';
import os from 'os';
import path from 'path';
import { ApprovalStoreError, createApprovalStore } from '../lib/approval-store.js';

const fixtureDirs = [];
afterEach(() => {
  for (const dir of fixtureDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

const binding = { agent: 'worker', project: 'p', project_room_id: '!p:test', owner_mxid: '@owner:test', owner_dm_room_id: '!dm:test' };
function setup(options = {}) {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'approval-proj-'));
  fixtureDirs.push(dir);
  const file = path.join(dir, 'store.json');
  const store = createApprovalStore(file, { now: () => 1000, ...options });
  store.upsertBinding(binding);
  return { store, file };
}
function create(store, id = 'up-1', expiresAt) {
  return store.createRequest({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: id, tool_name: 'Bash', input_preview: 'pwd', expires_at: expiresAt }, { routerApprovalId: id });
}
function identity(plan, roomId) {
  return { publisher_scope: plan.publisher_scope, publisher_mxid: plan.publisher_mxid, room_id: roomId, credential_generation: plan.credential_generation, transaction_id: plan.transaction_id };
}
function deliver(store, row, eventId) {
  const plan = store.prepareProjection(row.cas_token, { publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot', credential_generation: 'g1', payload_version: 1, prepared_event_type: 'm.room.message', prepared_payload: { body: row.channel } }).plan;
  store.beginProjectionSend(plan.cas_token, identity(plan, row.target_room_id));
  store.receiptProjection(plan.cas_token, { ...identity(plan, row.target_room_id), event_id: eventId });
}
function drainRevision(store, revision) {
  store.listDueProjections().filter((row) => row.revision === revision).forEach((row, i) => deliver(store, row, `$event${revision}${i}`));
}

describe('approval projection store', () => {
  test('public notice is ineligible until private request has a durable receipt', () => {
    const { store } = setup();
    const request = create(store, 'private-before-public');
    let due = store.listDueProjections({ limit: 20 });
    const privateRow = due.find((row) => row.request_id === request.id && row.channel === 'private_request');
    expect(privateRow).toBeTruthy();
    expect(due.some((row) => row.request_id === request.id && row.channel === 'public_notice')).toBe(false);

    const plan = store.prepareProjection(privateRow.cas_token, { publisher_scope: 'local_bot',
      publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot',
      credential_generation: 'g1', payload_version: 1, prepared_event_type: 'm.room.message',
      prepared_payload: { body: 'private' } }).plan;
    store.beginProjectionSend(plan.cas_token, identity(plan, privateRow.target_room_id));
    store.retryProjection(plan.cas_token, { ...identity(plan, privateRow.target_room_id), retry_at: 2000,
      error_code: 'unavailable' });
    due = store.listDueProjections({ limit: 20 });
    expect(due.some((row) => row.request_id === request.id && row.channel === 'public_notice')).toBe(false);
    expect(store.getRequest(request.id).status).toBe('pending');

    store.receiptProjection(plan.cas_token, { ...identity(plan, privateRow.target_room_id), event_id: '$private' });
    due = store.listDueProjections({ limit: 20 });
    const publicRow = due.find((row) => row.request_id === request.id && row.channel === 'public_notice');
    expect(publicRow).toBeTruthy();
    const publicPlan = store.prepareProjection(publicRow.cas_token, { publisher_scope: 'agent:worker:test',
      publisher_mxid: '@worker:test', homeserver: 'test', credential_kind: 'agent_token',
      credential_generation: 'agent-g1', payload_version: 1, prepared_event_type: 'm.room.message',
      prepared_payload: { body: 'redacted public notice' } }).plan;
    store.beginProjectionSend(publicPlan.cas_token, identity(publicPlan, publicRow.target_room_id));
    store.retryProjection(publicPlan.cas_token, { ...identity(publicPlan, publicRow.target_room_id),
      retry_at: 3000, error_code: 'public_unavailable' });
    expect(store.getRequest(request.id).status).toBe('pending');
  });

  test('prepare rejects noncanonical versions and payloads without mutation', () => {
    const { store, file } = setup();
    create(store);
    const row = store.listDueProjections()[0];
    const valid = {
      publisher_scope: 'local_bot',
      publisher_mxid: '@bot:test',
      homeserver: 'test',
      credential_kind: 'local_bot',
      credential_generation: 'g1',
      prepared_event_type: 'm.room.message',
      prepared_payload: { body: 'test' },
    };
    const diskBefore = readFileSync(file, 'utf8');

    for (const payload_version of ['Infinity', Infinity, 0, -1, 1.5, 2]) {
      expect(() => store.prepareProjection(row.cas_token, { ...valid, payload_version }))
        .toThrowError(/payload_version/);
    }
    for (const prepared_payload of [undefined, null, [], 'text']) {
      expect(() => store.prepareProjection(row.cas_token, { ...valid, prepared_payload }))
        .toThrowError(/prepared_payload/);
    }
    expect(() => store.prepareProjection(row.cas_token, {
      ...valid,
      prepared_payload: { ciphertext: 'fixed', nested: { numeric: 1e400 } },
    })).toThrowError(/numbers must be finite/);
    expect(() => store.prepareProjection(row.cas_token, {
      ...valid,
      prepared_event_type: 'm.reaction',
    })).toThrowError(/prepared_event_type/);

    expect(store.listDueProjections()[0].plan).toBeNull();
    expect(readFileSync(file, 'utf8')).toBe(diskBefore);
  });

  test('prepare preserves every JSON key and encrypted string across reload', () => {
    const { store, file } = setup();
    create(store);
    const row = store.listDueProjections()[0];
    const payload = JSON.parse('{"ciphertext":"a+/=\\\\byte","nested":{"__proto__":{"value":1}}}');
    const plan = store.prepareProjection(row.cas_token, {
      publisher_scope: 'local_bot',
      publisher_mxid: '@bot:test',
      homeserver: 'test',
      credential_kind: 'local_bot',
      credential_generation: 'g1',
      prepared_event_type: 'm.room.encrypted',
      prepared_payload: payload,
    }).plan;
    expect(Object.hasOwn(plan.prepared_payload.nested, '__proto__')).toBe(true);
    expect(plan.prepared_payload).toEqual(payload);

    const reloaded = createApprovalStore(file);
    const persisted = reloaded.listDueProjections()
      .find((item) => item.plan?.cas_token === plan.cas_token).plan.prepared_payload;
    expect(persisted).toEqual(plan.prepared_payload);
    expect(persisted.ciphertext).toBe('a+/=\\byte');
  });

  test('creation and transitions enqueue increasing canonical revisions privately', () => {
    const { store } = setup();
    const request = create(store);
    expect(request).not.toHaveProperty('projection_revision');
    expect(store.getRequest(request.id, { matrix: true })).not.toHaveProperty('projection_revision');
    expect(store.listDueProjections().map((row) => row.channel)).toEqual(['private_request']);
    drainRevision(store, 1);
    const matrix = store.getRequest(request.id, { matrix: true });
    store.submitMatrixVerdict(request.id, { action: 'approve_once', sender_mxid: matrix.owner_mxid, room_id: matrix.owner_dm_room_id, agent: matrix.agent, project: matrix.project, project_room_id: matrix.project_room_id, input_digest: matrix.input_digest });
    expect(store.listDueProjections()).toEqual([expect.objectContaining({ revision: 2, channel: 'private_status', state: 'approved' })]);
    drainRevision(store, 2);
    store.consumeDecision(request.id, 'worker', matrix.input_digest);
    expect(store.listDueProjections()).toEqual([expect.objectContaining({ revision: 3, state: 'consumed' })]);
  });

  test('private status keeps the private request publisher across a binding or credential change', () => {
    const { store, file } = setup();
    const request = create(store);
    const requestRow = store.listDueProjections().find((row) => row.channel === 'private_request');
    const original = store.prepareProjection(requestRow.cas_token, {
      publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'original',
      prepared_event_type: 'm.room.message', prepared_payload: { body: 'request' },
    }).plan;
    store.beginProjectionSend(original.cas_token, identity(original, requestRow.target_room_id));
    store.receiptProjection(original.cas_token, { ...identity(original, requestRow.target_room_id), event_id: '$request' });
    const matrix = store.getRequest(request.id, { matrix: true });
    store.submitMatrixVerdict(request.id, {
      action: 'approve_once', sender_mxid: matrix.owner_mxid, room_id: matrix.owner_dm_room_id,
      agent: matrix.agent, project: matrix.project, project_room_id: matrix.project_room_id,
      input_digest: matrix.input_digest,
    });
    const statusRow = store.listDueProjections().find((row) => row.channel === 'private_status');
    const before = readFileSync(file, 'utf8');
    expect(() => store.prepareProjection(statusRow.cas_token, {
      publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'rotated',
      prepared_event_type: 'm.room.message', prepared_payload: { body: 'approved' },
    })).toThrowError(/private request publisher/);
    expect(readFileSync(file, 'utf8')).toBe(before);
    expect(store.prepareProjection(statusRow.cas_token, {
      publisher_scope: original.publisher_scope, publisher_mxid: original.publisher_mxid,
      homeserver: original.homeserver, credential_kind: original.credential_kind,
      credential_generation: original.credential_generation,
      prepared_event_type: 'm.room.message', prepared_payload: { body: 'approved' },
    }).plan).toMatchObject({ credential_generation: 'original' });
  });

  test('publisher registry rotates transactionally and reloads without exposing a credential', () => {
    const { store, file } = setup();
    const first = store.upsertProjectionPublisher({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'g1',
    });
    expect(first).not.toHaveProperty('credential');
    expect(createApprovalStore(file).projectionPublisher('local_bot')).toMatchObject({ credentialGeneration: 'g1' });
    const before = readFileSync(file, 'utf8');
    store.fsFault = (phase) => { if (phase === 'beforeRename') throw new Error('publisher write failed'); };
    expect(() => store.upsertProjectionPublisher({
      scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
      credential_kind: 'local_bot', credential_generation: 'g2',
    })).toThrowError(/publisher write failed/);
    expect(store.projectionPublisher('local_bot')).toMatchObject({ credentialGeneration: 'g1' });
    expect(readFileSync(file, 'utf8')).toBe(before);
  });

  test('ready plan cannot retry or receipt before durable begin', () => {
    const { store, file } = setup();
    create(store);
    const row = store.listDueProjections()[0];
    const plan = store.prepareProjection(row.cas_token, {
      publisher_scope: 'local_bot',
      publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot',
      credential_generation: 'g1', prepared_event_type: 'm.room.message', prepared_payload: { body: 'x' },
    }).plan;
    const expectedIdentity = identity(plan, row.target_room_id);
    expect(() => store.retryProjection(plan.cas_token, { ...expectedIdentity, retry_at: 2000, error_code: 'early' })).toThrowError(/has not begun/);
    expect(() => store.receiptProjection(plan.cas_token, { ...expectedIdentity, event_id: '$never-begun' })).toThrowError(/receipt mismatch/);
    expect(store.listDueProjections().find((item) => item.plan?.cas_token === plan.cas_token).plan.attempt_state).toBe('ready');
    const diskRow = JSON.parse(readFileSync(file)).projectionOutbox.find((item) => item.planCasToken === plan.cas_token);
    expect(diskRow).toMatchObject({ attemptState: 'ready', nextAttemptAt: 0, eventId: null });
  });

  test('uncertain retry observes deadline and becomes receiptable with the same plan', () => {
    let now = 1000;
    const { store } = setup({ now: () => now });
    create(store);
    const row = store.listDueProjections()[0];
    const plan = store.prepareProjection(row.cas_token, { publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot', credential_generation: 'g1', prepared_event_type: 'm.room.message', prepared_payload: { body: 'x' } }).plan;
    store.beginProjectionSend(plan.cas_token, identity(plan, row.target_room_id));
    store.retryProjection(plan.cas_token, { ...identity(plan, row.target_room_id), retry_at: 2000, error_code: 'timeout' });
    expect(store.listDueProjections().some((item) => item.channel === row.channel)).toBe(false);
    now = 2000;
    const due = store.listDueProjections().find((item) => item.channel === row.channel);
    expect(due.plan.attempt_state).toBe('uncertain');
    expect(store.beginProjectionSend(plan.cas_token, identity(plan, row.target_room_id)).plan.attempt_state).toBe('attempted');
    expect(store.receiptProjection(plan.cas_token, { ...identity(plan, row.target_room_id), event_id: '$same' })).toEqual({ event_id: '$same' });
    expect(store.receiptProjection(plan.cas_token, { ...identity(plan, row.target_room_id), event_id: '$same' })).toEqual({ event_id: '$same' });
  });

  test('full prepared-plan identity is required and immutable', () => {
    const { store } = setup();
    create(store);
    const row = store.listDueProjections()[0];
    const input = { publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot', credential_generation: 'g1', prepared_event_type: 'm.room.message', prepared_payload: { body: 'x' } };
    const winner = store.prepareProjection(row.cas_token, input).plan;
    expect(store.prepareProjection(row.cas_token, { ...input, prepared_payload: { body: 'loser' } }).plan).toEqual(winner);
    store.beginProjectionSend(winner.cas_token, identity(winner, row.target_room_id));
    for (const mismatch of [{ room_id: '!wrong:test' }, { credential_generation: 'wrong' }, { transaction_id: 'wrong' }]) {
      expect(() => store.receiptProjection(winner.cas_token, { ...identity(winner, row.target_room_id), ...mismatch, event_id: '$wrong' })).toThrowError(/identity mismatch/);
    }
    expect(() => store.receiptProjection(winner.cas_token, { ...identity(winner, row.target_room_id), event_id: 'not-event' })).toThrowError(/Matrix event id/);
  });

  test('invalid verdict event id cannot mutate memory before validation', () => {
    const { store, file } = setup();
    const request = create(store);
    const matrix = store.getRequest(request.id, { matrix: true });
    expect(() => store.submitMatrixVerdict(request.id, { action: 'approve_once', sender_mxid: matrix.owner_mxid, room_id: matrix.owner_dm_room_id, agent: matrix.agent, project: matrix.project, project_room_id: matrix.project_room_id, input_digest: matrix.input_digest, event_id: 'x'.repeat(256) })).toThrow(ApprovalStoreError);
    expect(store.getRequest(request.id).status).toBe('pending');
    expect(JSON.parse(readFileSync(file)).requests[request.id].status).toBe('pending');
  });

  test('retry validation failure restores attempted state in memory and on disk', () => {
    const { store, file } = setup();
    create(store);
    const row = store.listDueProjections()[0];
    const plan = store.prepareProjection(row.cas_token, { publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot', credential_generation: 'g1', prepared_event_type: 'm.room.message', prepared_payload: { body: 'x' } }).plan;
    store.beginProjectionSend(plan.cas_token, identity(plan, row.target_room_id));
    expect(() => store.retryProjection(plan.cas_token, { ...identity(plan, row.target_room_id), retry_at: 2000, error_code: 'x'.repeat(129) })).toThrowError(/exceeds 128/);
    expect(store.listDueProjections().find((item) => item.plan?.cas_token === plan.cas_token).plan.attempt_state).toBe('attempted');
    const diskRow = JSON.parse(readFileSync(file)).projectionOutbox.find((item) => item.planCasToken === plan.cas_token);
    expect(diskRow).toMatchObject({ attemptState: 'attempted', nextAttemptAt: 0 });
  });

  test('create conflict rolls back incidental expiry in memory and on disk', () => {
    let now = 1000;
    const { store, file } = setup({ now: () => now });
    const expiring = create(store, 'early', 1100);
    create(store, 'same');
    now = 1200;
    expect(() => store.createRequest({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'same', tool_name: 'Bash' })).toThrowError(/different approval origin/);
    expect(store.state.requests[expiring.id].status).toBe('pending');
    const disk = JSON.parse(readFileSync(file));
    expect(disk.requests[expiring.id].status).toBe('pending');
  });

  test('pre-rename rolls back and post-rename degradation blocks later writes until reload', () => {
    let phase = '';
    const { store, file } = setup({ fsFault: (name) => { if (phase === name) throw new Error(name); } });
    phase = 'beforeRename';
    expect(() => create(store)).toThrowError(/failed to persist/);
    expect(store.listRequests()).toHaveLength(0);
    phase = 'afterRename';
    expect(() => create(store, 'committed')).not.toThrow();
    expect(store.persistenceHealth().degraded).toBe(true);
    expect(() => create(store, 'blocked')).toThrowError(/requires reload/);
    expect(store.listRequests()).toHaveLength(1);
    expect(Object.keys(JSON.parse(readFileSync(file)).requests)).toHaveLength(1);
    expect(createApprovalStore(file).listRequests()).toHaveLength(1);
  });

  test('degraded reads return committed pending state without lazy expiry until reload', () => {
    let now = 1000;
    let phase = '';
    const { store, file } = setup({ now: () => now, fsFault: (name) => { if (phase === name) throw new Error(name); } });
    phase = 'afterRename';
    const request = create(store, 'degraded-read', 1100);
    now = 1200;
    expect(store.getRequest(request.id).status).toBe('pending');
    expect(store.listRequests({ status: 'approved' })).toEqual([]);
    expect(store.state.requests[request.id].status).toBe('pending');
    const reloaded = createApprovalStore(file, { now: () => now });
    expect(reloaded.getRequest(request.id).status).toBe('expired');
  });

  test('healthy lazy expiry rolls back when persistence fails before rename', () => {
    let now = 1000;
    let phase = '';
    const { store, file } = setup({ now: () => now, fsFault: (name) => { if (phase === name) throw new Error(name); } });
    const request = create(store, 'lazy-fault', 1100);
    now = 1200;
    phase = 'beforeRename';
    expect(() => store.getRequest(request.id)).toThrowError(/failed to persist/);
    expect(store.state.requests[request.id].status).toBe('pending');
    expect(JSON.parse(readFileSync(file)).requests[request.id].status).toBe('pending');
  });

  test('expiry sweep reaches expired rows behind a long-lived prefix', () => {
    let now = 1000;
    const { store } = setup({ now: () => now, ttlMs: 10000 });
    create(store, 'long-1', 9000); create(store, 'long-2', 9000); create(store, 'expired', 1100);
    now = 1200;
    expect(store.sweepExpired({ limit: 1 })).toEqual({ scanned: 1, expired: 1 });
    expect(store.listRequests().find((r) => r.upstream_request_id === 'expired').status).toBe('expired');
  });

  test('legacy migration is bounded, resumable, and emits no actionable or null-target work', () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'approval-v1-'));
    fixtureDirs.push(dir);
    const file = path.join(dir, 'store.json');
    const request = (id, status, expiresAt, room = '!dm:test') => ({ id, agent: 'worker', runtime: 'codex', project: 'p', projectRoomId: '!p:test', ownerMxid: '@owner:test', ownerDmRoomId: room, upstreamRequestId: id, inputDigest: `d-${id}`, status, decision: status === 'consumed' ? 'allow' : null, createdAt: 1, expiresAt });
    writeFileSync(file, JSON.stringify({ version: 1, bindings: {}, requests: { a: request('a', 'pending', 500), b: request('b', 'consumed', 5000), c: request('c', 'pending', 5000, null) }, audit: [] }));
    let store = createApprovalStore(file, { now: () => 1000, migrationBatchSize: 1 });
    expect(store.listDueProjections()).toEqual([expect.objectContaining({ request_id: 'a', state: 'expired', migration_kind: 'legacy_v1' })]);
    store = createApprovalStore(file, { now: () => 1000, migrationBatchSize: 1 });
    store.migrateLegacyBatch();
    store = createApprovalStore(file, { now: () => 1000, migrationBatchSize: 1 });
    store.migrateLegacyBatch();
    const due = store.listDueProjections({ limit: 20 });
    expect(due.every((row) => row.channel === 'private_status' && row.target_room_id)).toBe(true);
    expect(due.some((row) => row.state === 'pending' && row.request_id === 'a')).toBe(false);
  });

  test('stale expiry heap entries consume a bounded slot and later overdue work progresses', () => {
    let now = 1000;
    const { store } = setup({ now: () => now, ttlMs: 10000 });
    const stale = create(store, 'stale', 1050);
    create(store, 'overdue', 1100);
    store.denyPending(stale.id, 'fixture');
    now = 1200;
    expect(store.sweepExpired({ limit: 1 })).toEqual({ scanned: 1, expired: 0 });
    expect(store.sweepExpired({ limit: 1 })).toEqual({ scanned: 1, expired: 1 });
  });

  test('migration offset rolls back on pre-rename failure and resumes after reload', () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'approval-migration-fault-'));
    fixtureDirs.push(dir);
    const file = path.join(dir, 'store.json');
    const rows = {};
    for (const id of ['a', 'b', 'c']) rows[id] = { id, agent: 'worker', runtime: 'codex', project: 'p', projectRoomId: '!p:test', ownerMxid: '@owner:test', ownerDmRoomId: '!dm:test', upstreamRequestId: id, inputDigest: id, status: 'pending', createdAt: 1, expiresAt: 5000 };
    writeFileSync(file, JSON.stringify({ version: 1, bindings: {}, requests: rows, audit: [] }));
    let phase = '';
    let store = createApprovalStore(file, { now: () => 1000, migrationBatchSize: 1, fsFault: (name) => { if (phase === name) throw new Error(name); } });
    expect(store.state.migration.offset).toBe(1);
    phase = 'beforeRename';
    expect(() => store.migrateLegacyBatch()).toThrowError(/failed to persist/);
    expect(store.state.migration.offset).toBe(1);
    expect(JSON.parse(readFileSync(file)).migration.offset).toBe(1);
    store = createApprovalStore(file, { now: () => 1000, migrationBatchSize: 1 });
    expect(store.state.migration.offset).toBe(2);
  });

  test('verdict expiry and binding removal keep unmigrated legacy work read-only', () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'approval-legacy-direct-'));
    fixtureDirs.push(dir);
    const file = path.join(dir, 'store.json');
    const row = (id, expiresAt = 5000) => ({ id, agent: 'worker', runtime: 'codex', project: 'p', projectRoomId: '!p:test', ownerMxid: '@owner:test', ownerDmRoomId: '!dm:test', upstreamRequestId: id, inputDigest: id, status: 'pending', createdAt: 1, expiresAt });
    const bindingKey = `worker\0!p:test`;
    writeFileSync(file, JSON.stringify({ version: 1, bindings: { [bindingKey]: { agent: 'worker', project: 'p', projectRoomId: '!p:test', ownerMxid: '@owner:test', ownerDmRoomId: '!dm:test', active: true } }, requests: { a: row('a'), b: row('b'), c: row('c', 500), d: row('d') }, audit: [] }));
    const store = createApprovalStore(file, { now: () => 1000, migrationBatchSize: 1 });
    store.submitMatrixVerdict('b', { action: 'approve_once', sender_mxid: '@owner:test', room_id: '!dm:test', agent: 'worker', project: 'p', project_room_id: '!p:test', input_digest: 'b' });
    expect(store.getRequest('c').status).toBe('expired');
    store.removeBinding('worker', '!p:test');
    const work = store.listDueProjections({ limit: 20 }).filter((item) => ['b', 'c', 'd'].includes(item.request_id));
    expect(work).toEqual(expect.arrayContaining([
      expect.objectContaining({ request_id: 'b', revision: 1, state: 'approved', migration_kind: 'legacy_v1' }),
      expect.objectContaining({ request_id: 'c', revision: 1, state: 'expired', migration_kind: 'legacy_v1' }),
      expect.objectContaining({ request_id: 'd', revision: 1, state: 'denied', migration_kind: 'legacy_v1' }),
    ]));
    expect(work.every((item) => item.channel === 'private_status')).toBe(true);
  });

  test('superseded ready plan cannot begin while an attempted plan can receipt', () => {
    const { store } = setup();
    const readyRequest = create(store, 'ready');
    const readyRow = store.listDueProjections().find((item) => item.request_id === readyRequest.id && item.channel === 'private_request');
    const readyPlan = store.prepareProjection(readyRow.cas_token, { publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot', credential_generation: 'g', prepared_event_type: 'm.room.message', prepared_payload: { body: 'ready' } }).plan;
    store.denyPending(readyRequest.id, 'terminal');
    expect(() => store.beginProjectionSend(readyPlan.cas_token, identity(readyPlan, readyRow.target_room_id))).toThrowError(/superseded/);

    const attemptedRequest = create(store, 'attempted');
    const attemptedRow = store.listDueProjections({ limit: 20 }).find((item) => item.request_id === attemptedRequest.id && item.channel === 'private_request');
    const attemptedPlan = store.prepareProjection(attemptedRow.cas_token, { publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test', credential_kind: 'local_bot', credential_generation: 'g', prepared_event_type: 'm.room.message', prepared_payload: { body: 'attempted' } }).plan;
    store.beginProjectionSend(attemptedPlan.cas_token, identity(attemptedPlan, attemptedRow.target_room_id));
    store.denyPending(attemptedRequest.id, 'terminal');
    expect(store.receiptProjection(attemptedPlan.cas_token, { ...identity(attemptedPlan, attemptedRow.target_room_id), event_id: '$landed' })).toEqual({ event_id: '$landed' });
  });

  test('denied request without binding cannot enqueue a null-target status on consume', () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), 'approval-unbound-'));
    fixtureDirs.push(dir);
    const store = createApprovalStore(path.join(dir, 'store.json'), { now: () => 1000 });
    const denied = store.createRequest({ agent: 'worker', runtime: 'codex', upstream_request_id: 'u', tool_name: 'Bash' });
    store.consumeDecision(denied.id, 'worker');
    expect(store.listDueProjections()).toEqual([]);
  });
});
