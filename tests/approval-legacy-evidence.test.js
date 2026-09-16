import { afterEach, expect, test } from 'vitest';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'fs';
import os from 'os';
import path from 'path';
import { createApprovalStore } from '../lib/approval-store.js';
import { legacyRecord, legacyDisk, legacyId, legacyAttestation, publisherInput, planIdentity } from './helpers/approval-legacy-fixture.js';

const dirs = [];
afterEach(() => { for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true }); });
function fixture(record = legacyRecord()) {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'legacy-original-store-')); dirs.push(dir);
  const file = path.join(dir, 'approvals.json'); writeFileSync(file, JSON.stringify(legacyDisk(record)));
  const store = createApprovalStore(file, { now: () => 1000 });
  const actor = publisherInput(); store.upsertProjectionPublisher({ ...actor, scope: actor.publisher_scope });
  const row = store.listDueProjections()[0];
  return { file, store, row, record, attestation: legacyAttestation(row, record) };
}
function prepare(f) {
  const result = f.store.attestLegacyOriginal(f.record.id, f.attestation);
  const { content } = f.store.legacyOriginalForProjection(f.row.cas_token, f.record.id);
  return { ...publisherInput(), request_id: f.record.id, revision: f.row.revision, channel: 'private_status',
    legacy_evidence_cas: result.evidence.evidence_cas, prepared_event_type: 'm.room.message', prepared_payload: content };
}
function unchanged(f, callback) {
  const state = structuredClone(f.store.state); const bytes = readFileSync(f.file, 'utf8');
  expect(callback).toThrow(); expect(f.store.state).toEqual(state); expect(readFileSync(f.file, 'utf8')).toBe(bytes);
}

test('legacy evidence rejects tuple relation and CAS conflicts without mutation', () => {
  const f = fixture();
  const changes = [
    ['request_id', 'other'], ['revision', 2], ['revision', '1'], ['channel', 'private_request'], ['cas_token', 'wrong'],
    ['candidate_verdict_event_id', '$different'], ['schema_version', 2],
  ];
  for (const [key, value] of changes) unchanged(f, () => f.store.attestLegacyOriginal(legacyId, { ...f.attestation, [key]: value }));
  unchanged(f, () => f.store.attestLegacyOriginal('wrong-path', f.attestation));
  for (const role of ['verdict', 'original']) {
    for (const [key, value] of [
      ['request_id', 'wrong'], ['agent', 'worker2'], ['project', 'p '], ['project_room_id', '!else:test'],
      ['input_digest', 'e'.repeat(64)], ['room_id', '!other:test'], ['version', 2], ['kind', 'status'],
      ['type', 'm.room.encrypted'], ['msgtype', 'com.hafleet.approval.' + (role === 'verdict' ? 'verdict' : 'request') + '.v1'],
      ['edited', true], ['redacted', true], ['event_id', 'not-an-event'], ['sender', 'bot'],
    ]) {
      const input = structuredClone(f.attestation); input[role][key] = value;
      unchanged(f, () => f.store.attestLegacyOriginal(legacyId, input));
    }
  }
  for (const [role, key, value] of [
    ['verdict', 'sender', '@other:test'], ['verdict', 'event_id', '$other'], ['verdict', 'action', 'deny'],
    ['verdict', 'reply_to_event_id', '$other'], ['original', 'event_id', '$owner-verdict'],
    ['original', 'sender', '@otherbot:test'], ['original', 'runtime', 'claude'],
    ['original', 'upstream_request_id', 'wrong'], ['original', 'expires_at', 501], ['original', 'expires_at', Infinity],
  ]) {
    const input = structuredClone(f.attestation); input[role][key] = value;
    unchanged(f, () => f.store.attestLegacyOriginal(legacyId, input));
  }
  f.store.attestLegacyOriginal(legacyId, f.attestation);
  const conflicting = structuredClone(f.attestation);
  conflicting.original.event_id = conflicting.verdict.reply_to_event_id = '$another-original';
  unchanged(f, () => f.store.attestLegacyOriginal(legacyId, conflicting));
});

test('legacy evidence is idempotent after reload without canonical or queue mutation', () => {
  const f = fixture();
  const originalRequests = structuredClone(f.store.state.requests), originalBindings = structuredClone(f.store.state.bindings);
  const originalQueue = structuredClone(f.store.state.projectionOutbox);
  const first = f.store.attestLegacyOriginal(legacyId, f.attestation);
  expect(first.duplicate).toBe(false); expect(first.evidence.historical_credential_generation).toBeNull();
  expect(first.evidence).not.toHaveProperty('credential_generation');
  expect(first.evidence).not.toHaveProperty('description'); expect(first.evidence).not.toHaveProperty('input_preview');
  const persisted = readFileSync(f.file, 'utf8');
  f.store.now = () => 9999;
  expect(f.store.attestLegacyOriginal(legacyId, f.attestation)).toEqual({ ...first, duplicate: true });
  expect(readFileSync(f.file, 'utf8')).toBe(persisted);
  const reloaded = createApprovalStore(f.file);
  expect(reloaded.attestLegacyOriginal(legacyId, f.attestation)).toEqual({ ...first, duplicate: true });
  expect(reloaded.state.requests).toEqual(originalRequests); expect(reloaded.state.bindings).toEqual(originalBindings);
  expect(reloaded.state.projectionOutbox).toEqual(originalQueue); expect(reloaded.privateRequestPublisher(legacyId)).toBeNull();
});

test('legacy evidence rolls back before rename and retains committed degraded state after rename', () => {
  const f = fixture();
  f.store.fsFault = (phase) => { if (phase === 'beforeRename') throw new Error('injected before rename'); };
  unchanged(f, () => f.store.attestLegacyOriginal(legacyId, f.attestation));
  expect(f.store.persistenceHealth().degraded).toBe(false);
  f.store.fsFault = (phase) => { if (phase === 'afterRename') throw new Error('injected after rename'); };
  const accepted = f.store.attestLegacyOriginal(legacyId, f.attestation);
  expect(f.store.persistenceHealth().degraded).toBe(true);
  expect(JSON.parse(readFileSync(f.file)).legacyOriginalEvidence[legacyId]).toEqual(accepted.evidence);
  expect(f.store.legacyOriginalForProjection(f.row.cas_token, legacyId).evidence).toEqual(accepted.evidence);
  expect(() => f.store.attestLegacyOriginal(legacyId, f.attestation)).toThrow(/requires reload/);
  const reloaded = createApprovalStore(f.file);
  expect(reloaded.persistenceHealth().degraded).toBe(false);
  expect(reloaded.attestLegacyOriginal(legacyId, f.attestation).duplicate).toBe(true);
});

test('legacy plaintext status must equal canonical readonly content', () => {
  const f = fixture(); const input = prepare(f);
  for (const change of [
    (p) => { p['com.agentchat.approval'].actions = [{ id: 'approve_once' }]; },
    (p) => { p['com.agentchat.approval'].state = 'approved'; },
    (p) => { p['com.agentchat.approval'].revision = 999; },
    (p) => { p['com.agentchat.approval'].migration_kind = 'native_v2'; },
    (p) => { p['com.agentchat.approval'].publisher_mxid = '@other:test'; },
    (p) => { p['com.agentchat.approval'].input_digest = 'forged'; },
    (p) => { p['m.relates_to']['m.in_reply_to'].event_id = '$different'; },
    (p) => { p.body = 'task succeeded'; },
  ]) {
    const mutated = structuredClone(input); change(mutated.prepared_payload);
    unchanged(f, () => f.store.prepareProjection(f.row.cas_token, mutated));
  }
  unchanged(f, () => f.store.prepareProjection(f.row.cas_token, { ...input, legacy_evidence_cas: 'wrong' }));
  const plan = f.store.prepareProjection(f.row.cas_token, input).plan;
  expect(plan.prepared_payload).toEqual(input.prepared_payload);
  expect(plan.legacy_evidence_cas).toBe(input.legacy_evidence_cas);
  expect(plan.prepared_payload['com.agentchat.approval']).not.toHaveProperty('actions');
});

test('legacy first plan pins context across revisions and uncertain ciphertext survives reload', () => {
  const f = fixture(legacyRecord({ status: 'approved', consumedAt: null }));
  const input = prepare(f); input.prepared_event_type = 'm.room.encrypted'; input.prepared_payload = { algorithm: 'm.megolm.v1.aes-sha2', ciphertext: 'exact-first-ciphertext' };
  const first = f.store.prepareProjection(f.row.cas_token, input).plan;
  const identity = planIdentity(f.row, first);
  f.store.beginProjectionSend(first.cas_token, identity); f.store.retryProjection(first.cas_token, { ...identity, error_code: 'timeout' });
  const replay = f.store.prepareProjection(f.row.cas_token, { ...input, prepared_payload: { ciphertext: 'must-not-replace' } }).plan;
  expect(replay.prepared_payload).toEqual(first.prepared_payload); expect(replay.transaction_id).toBe(first.transaction_id);
  const reloaded = createApprovalStore(f.file, { now: () => 1000 });
  reloaded.receiptProjection(first.cas_token, { ...identity, event_id: '$readonly-status' });
  reloaded.consumeDecision(legacyId, 'worker', f.record.inputDigest);
  const next = reloaded.listDueProjections()[0];
  const publisher = publisherInput({ credential_generation: 'rotated' });
  reloaded.upsertProjectionPublisher({ ...publisher, scope: publisher.publisher_scope });
  const nextContent = reloaded.legacyOriginalForProjection(next.cas_token, legacyId).content;
  expect(() => reloaded.prepareProjection(next.cas_token, { ...input, ...publisher, revision: next.revision,
    prepared_event_type: 'm.room.message', prepared_payload: nextContent })).toThrow(/first pinned publisher/);
});

test('native rows cannot attest legacy evidence or borrow its status exception', () => {
  const f = fixture();
  f.store.upsertBinding({ agent: 'worker', project: 'p', project_room_id: '!p:test', owner_mxid: '@owner:test', owner_dm_room_id: '!dm:test' });
  const native = f.store.createRequest({ agent: 'worker', runtime: 'codex', project: 'p', project_room_id: '!p:test', upstream_request_id: 'new', tool_name: 'Bash' });
  const row = f.store.listDueProjections().find((r) => r.request_id === native.id);
  unchanged(f, () => f.store.attestLegacyOriginal(native.id, { ...f.attestation, request_id: native.id, cas_token: row.cas_token }));
  expect(f.store.state.legacyOriginalEvidence).toEqual({});
});

test('legacy evidence accepts complete historical namespace pairs and preserves deny', () => {
  const f = fixture(legacyRecord({ decision: 'deny' })); f.attestation.verdict.action = 'deny';
  for (const role of ['verdict', 'original']) {
    f.attestation[role].payload_key = 'com.hafleet.approval';
    f.attestation[role].msgtype = `com.hafleet.approval.${role === 'original' ? 'request' : 'verdict'}.v1`;
  }
  f.store.attestLegacyOriginal(legacyId, f.attestation);
  expect(f.store.legacyOriginalForProjection(f.row.cas_token, legacyId).content['com.agentchat.approval']).toMatchObject({ decision: 'deny', state: 'consumed' });
});

test('legacy missing candidate verdict remains unresolved', () => {
  const f = fixture(legacyRecord({ matrixEventId: undefined }));
  expect(f.store.legacyOriginalForProjection(f.row.cas_token, legacyId)).toEqual({ candidate_verdict_event_id: null, evidence: null, content: null });
  unchanged(f, () => f.store.attestLegacyOriginal(legacyId, f.attestation));
  expect(f.store.listDueProjections()).toHaveLength(1);
});

test('legacy normalized evidence rejects raw bodies secret fields and oversized senders', () => {
  const f = fixture();
  for (const alter of [
    (p) => { p.original.content = { body: 'private command' }; },
    (p) => { p.access_token = 'must-not-be-accepted'; },
    (p) => { p.original.sender = '@' + 'a'.repeat(256) + ':test'; p.publisher.publisher_mxid = p.original.sender; },
    (p) => { p.original['m.new_content'] = { body: 'edited' }; },
    (p) => { p.original.extra = 'x'.repeat(16 * 1024); },
  ]) {
    const input = structuredClone(f.attestation); alter(input);
    unchanged(f, () => f.store.attestLegacyOriginal(legacyId, input));
  }
});

test('legacy canonical plaintext accepts JSON key reordering', () => {
  const f = fixture(); const input = prepare(f);
  input.prepared_payload = Object.fromEntries(Object.entries(input.prepared_payload).reverse());
  input.prepared_payload['com.agentchat.approval'] = Object.fromEntries(Object.entries(input.prepared_payload['com.agentchat.approval']).reverse());
  const plan = f.store.prepareProjection(f.row.cas_token, input).plan;
  expect(plan.prepared_payload).toEqual(input.prepared_payload);
});

test('legacy store send operations independently reject rotated registry but retain receipts', () => {
  const f = fixture(); const input = prepare(f); const plan = f.store.prepareProjection(f.row.cas_token, input).plan;
  const identity = planIdentity(f.row, plan);
  f.store.beginProjectionSend(plan.cas_token, identity);
  f.store.upsertProjectionPublisher({ scope: 'local_bot', ...publisherInput({ credential_generation: 'g2' }) });
  unchanged(f, () => f.store.beginProjectionSend(plan.cas_token, identity));
  unchanged(f, () => f.store.retryProjection(plan.cas_token, { ...identity, error_code: 'timeout' }));
  expect(f.store.receiptProjection(plan.cas_token, { ...identity, event_id: '$exact-old-receipt' })).toEqual({ event_id: '$exact-old-receipt' });
});

test('legacy store refuses unavailable private observation before persisting evidence', () => {
  const f = fixture();
  for (const field of ['joined', 'ready']) {
    const input = structuredClone(f.attestation); input.private_context[field] = false;
    unchanged(f, () => f.store.attestLegacyOriginal(legacyId, input));
  }
  for (const [field, value] of [['room_id', '!else:test'], ['owner_mxid', '@other:test'], ['encrypted', 'yes']]) {
    const input = structuredClone(f.attestation); input.private_context[field] = value;
    unchanged(f, () => f.store.attestLegacyOriginal(legacyId, input));
  }
});

test('legacy evidence reads require an explicit exact row or plan CAS', () => {
  const f = fixture();
  for (const token of [undefined, null, '', [], [f.row.cas_token], f.row.cas_token + 'x']) {
    expect(() => f.store.legacyOriginalForProjection(token, legacyId)).toThrow(/legacy projection/);
  }
  expect(f.store.legacyOriginalForProjection(f.row.cas_token, legacyId).candidate_verdict_event_id).toBe('$owner-verdict');
});

test('legacy exact event type prevents normalization bypass of readonly content', () => {
  const f = fixture(); const input = prepare(f);
  for (const prepared_event_type of [' m.room.message ', '\tm.room.message', 'm.room.message\n', ' m.room.encrypted ']) {
    // Both canonical and forged contents reject: legacy event types are intentionally exact.
    for (const forged of [true, false]) {
      const proposed = structuredClone(input); proposed.prepared_event_type = prepared_event_type;
      if (forged) {
        proposed.prepared_payload['com.agentchat.approval'].state = 'approved';
        proposed.prepared_payload['com.agentchat.approval'].actions = [{ id: 'approve_once' }];
      }
      unchanged(f, () => f.store.prepareProjection(f.row.cas_token, proposed));
    }
  }
});
