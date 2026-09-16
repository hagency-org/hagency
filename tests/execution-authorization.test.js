import { afterEach, expect, test, vi } from 'vitest';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { ApprovalStore } from '../lib/approval-store.js';
import { deriveExecutionAuthorization, normalizeExecutionPolicy } from '../lib/execution-authorization.js';

const roots = [];
afterEach(() => { vi.restoreAllMocks(); roots.splice(0).forEach(root => rmSync(root, { recursive: true, force: true })); });
const command = (changes = {}) => ({ agentId: 'edison-incarnation-1', workspace: '/work/edison', taskId: 'task-1', mayWrite: true,
  method: 'item/commandExecution/requestApproval', params: { threadId: 'thread-1', turnId: 'turn-1', itemId: 'item-1',
    command: 'curl https://aapt.org/report.pdf', cwd: '/work/edison', reason: 'network please', ...changes } });
const binding = { agent: 'edison', project: 'physics', project_room_id: '!project:test', owner_mxid: '@owner:test', owner_dm_room_id: '!owner:test' };
const body = id => ({ agent: 'edison', runtime: 'codex', project_room_id: '!project:test', upstream_request_id: id,
  tool_name: 'app_server_command', description: 'Fetch report', input_preview: 'curl report' });
const verdict = (r, action, overrides = {}) => ({ sender_mxid: r.owner_mxid, room_id: r.owner_dm_room_id,
  agent: r.agent, project: r.project, project_room_id: r.project_room_id, input_digest: r.input_digest,
  action, event_id: `$${r.id}`, ...overrides });

test('approval rollback preserves projection state and pruning waits for delivery without revoking grants', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'approval-retention-')); roots.push(root);
  const filename = path.join(root, 'approvals.json');
  let now = 1000;
  const store = new ApprovalStore(filename, { now: () => now });
  store.upsertBinding(binding);
  const record = store.createRequest(body('original'), { execution: command() });
  let receiptNumber = 0;
  const deliverDue = () => {
    for (let pass = 0; pass < 3; pass++) {
      const due = store.listDueProjections().filter(row => row.request_id === record.id);
      if (!due.length) return;
      for (const row of due) {
        const plan = store.prepareProjection(row.cas_token, {
          publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
          credential_kind: 'local_bot', credential_generation: 'g1', payload_version: 1,
          prepared_event_type: 'm.room.message', prepared_payload: { body: row.channel },
        }).plan;
        const identity = { publisher_scope: plan.publisher_scope, publisher_mxid: plan.publisher_mxid,
          room_id: row.target_room_id, credential_generation: plan.credential_generation,
          transaction_id: plan.transaction_id };
        store.beginProjectionSend(plan.cas_token, identity);
        store.receiptProjection(plan.cas_token, { ...identity, event_id: `$receipt${++receiptNumber}` });
      }
    }
    throw new Error('fixture did not finish the bounded projection drain');
  };
  deliverDue();
  store.submitMatrixVerdict(record.id, verdict(record, 'approve_always'));
  const before = structuredClone(store.state);
  const diskBefore = readFileSync(filename, 'utf8');
  const originalSave = store._save.bind(store);
  store._save = () => { throw new Error('disk full'); };
  expect(() => store.createRequest(body('failed'))).toThrow('disk full');
  expect(store.state).toStrictEqual(before);
  expect(readFileSync(filename, 'utf8')).toBe(diskBefore);
  expect(Object.keys(store.state.requests)).toEqual([record.id]);
  store._save = originalSave;
  now += 8 * 24 * 60 * 60_000;
  const next = store.createRequest(body('next'), { execution: command() });
  expect(store.getRequest(record.id).status).toBe('approved');
  expect(store.listDueProjections().some(row => row.request_id === record.id
    && row.channel === 'private_status')).toBe(true);
  expect(next.status).toBe('approved');
  deliverDue();
  expect(store.state.projectionOutbox.filter(row => row.requestId === record.id)
    .every(row => row.eventId)).toBe(true);
  const afterDelivery = store.createRequest(body('after-delivery'), { execution: command() });
  expect(store.getRequest(record.id)).toBeNull();
  expect(store.consumeDecision(record.id, 'edison', record.input_digest).code).toBe('not_found');
  expect(afterDelivery.status).toBe('approved');
  expect(store.listGrants('edison').filter(grant => grant.active)).toHaveLength(1);
});

test('approval scopes preserve exact command and structured permissions without inferring domains', () => {
  const exact = deriveExecutionAuthorization(command());
  expect(exact.scope.kind).toBe('exact_command');
  expect(exact.scope.description).toContain('Exact command');
  expect(exact.scope.description).toContain('curl https://aapt.org/report.pdf');
  expect(deriveExecutionAuthorization(command({ command: 'curl https://aapt.org/other.pdf' })).scope.key).not.toBe(exact.scope.key);
  expect(deriveExecutionAuthorization(command({ cwd: '/work/other' })).scope.key).not.toBe(exact.scope.key);
  expect(deriveExecutionAuthorization(command({ reason: 'other prose', threadId: 'new-thread' })).scope.key).toBe(exact.scope.key);
  const network = deriveExecutionAuthorization(command({ networkApprovalContext: { host: 'aapt.org', protocol: 'https' } }));
  expect(network.scope).toMatchObject({ kind: 'network_host', description: 'Network host: aapt.org\nProtocol: https' });
  expect(deriveExecutionAuthorization(command({ networkApprovalContext: { host: 'aapt.org.evil.test', protocol: 'https' } })).scope.key).not.toBe(network.scope.key);
  for (const params of [{ kind: 'writeStdin' }, { futureEscalation: true }, { command: null }, { cwd: 'relative' },
    { networkApprovalContext: { host: '*.org', protocol: 'https' } }, { additionalPermissions: { unexpected: true } }]) {
    expect(deriveExecutionAuthorization(command(params))).toBeNull();
  }
  const permissions = { ...command(), method: 'item/permissions/requestApproval', params: {
    cwd: '/work/edison', permissions: { network: { enabled: true }, fileSystem: { read: ['/data/report'] } } } };
  const scope = deriveExecutionAuthorization(permissions);
  expect(scope.scope.description).toContain('all destinations');
  expect(scope.scope.description).toContain('read: /data/report');
  permissions.params.permissions.fileSystem.read = ['/data'];
  expect(deriveExecutionAuthorization(permissions).scope.key).not.toBe(scope.scope.key);
  permissions.params.permissions.fileSystem = { entries: [{ access: 'write', path: { type: 'glob_pattern', pattern: '*' } }] };
  expect(deriveExecutionAuthorization(permissions)).toBeNull();
  expect(deriveExecutionAuthorization({ ...command(), method: 'item/fileChange/requestApproval' })).toBeNull();
  expect(normalizeExecutionPolicy(undefined, 'codex')).toEqual({ yolo: false });
  expect(normalizeExecutionPolicy({ yolo: true }, 'codex')).toEqual({ yolo: true });
  expect(() => normalizeExecutionPolicy({ yolo: 'false' }, 'codex')).toThrow();
  expect(() => normalizeExecutionPolicy({ yolo: true }, 'claude')).toThrow();
});

test('scoped grants survive restart but never cross task agent project owner or revocation', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-grants-')); roots.push(root);
  const filename = path.join(root, 'approvals.json');
  const activeTasks = new Set(['task-1', 'task-2']);
  let currentAgentId = 'edison-incarnation-1';
  let taskEpoch = 0;
  const options = { getTaskEpoch: () => taskEpoch, isTaskActive: id => activeTasks.has(id), isAgentCurrent: (agent, id) => agent === 'edison' && id === currentAgentId };
  let store = new ApprovalStore(filename, options);
  store.upsertBinding(binding);
  const create = (id, execution = command(), extra = {}) => store.createRequest({ ...body(id), ...extra }, { execution });
  const request = create('one');
  expect(store.getRequest(request.id, { matrix: true }).reusable_scope.description).toContain('Exact command');
  expect(store.submitMatrixVerdict(request.id, verdict(request, 'approve_task', { sender_mxid: '@intruder:test' })).ok).toBe(false);
  expect(store.listGrants('edison')).toHaveLength(0);
  expect(store.submitMatrixVerdict(request.id, verdict(request, 'approve_task')).ok).toBe(true);
  expect(store.consumeDecision(request.id, 'edison', request.input_digest).decision).toBe('allow');
  expect(store.consumeDecision(request.id, 'edison', request.input_digest).code).toBe('consumed');
  store = new ApprovalStore(filename, options);
  expect(create('two').status).toBe('approved');
  expect(create('new-task', { ...command(), taskId: 'task-2' }).status).toBe('pending');
  expect(create('new-command', command({ command: 'curl https://other.test/' })).status).toBe('pending');
  expect(create('new-workspace', { ...command(), workspace: '/work/other' }).status).toBe('pending');
  expect(create('new-environment', command({ environmentId: 'remote' })).status).toBe('pending');
  activeTasks.delete('task-1');
  taskEpoch++;
  expect(create('done-task').status).toBe('pending');
  activeTasks.add('task-1');
  expect(create('reopened-task').status).toBe('pending');
  activeTasks.delete('task-1');
  expect(store.submitMatrixVerdict(create('late-task').id, verdict(store.listRequests().at(-1), 'approve_task')).ok).toBe(false);
  const permanent = create('permanent', { ...command(), taskId: 'task-2' });
  expect(store.submitMatrixVerdict(permanent.id, verdict(permanent, 'approve_always')).ok).toBe(true);
  store = new ApprovalStore(filename, options);
  const granted = create('reuse-permanent');
  expect(granted.status).toBe('approved');
  const grantId = granted.grant_id;
  expect(store.revokeGrant(grantId, 'other-agent', 'operator')).toBeNull();
  store.revokeGrant(grantId, 'edison', 'operator');
  expect(store.consumeDecision(granted.id, 'edison', granted.input_digest).decision).toBe('deny');
  expect(create('after-revoke').status).toBe('pending');
  const second = create('second'); store.submitMatrixVerdict(second.id, verdict(second, 'approve_always'));
  store.upsertBinding({ ...binding, project_room_id: '!other:test' });
  expect(create('other-project', command(), { project_room_id: '!other:test' }).status).toBe('pending');
  store.upsertBinding({ ...binding, owner_mxid: '@new-owner:test' });
  store.upsertBinding(binding);
  expect(create('owner-returned').status).toBe('pending');
  const third = create('third'); store.submitMatrixVerdict(third.id, verdict(third, 'approve_always'));
  store.deactivateBinding('edison', '!project:test'); store.upsertBinding(binding);
  expect(create('reactivated').status).toBe('pending');
  const fourth = create('fourth'); store.submitMatrixVerdict(fourth.id, verdict(fourth, 'approve_always'));
  currentAgentId = 'edison-incarnation-2';
  expect(create('recreated-agent', { ...command(), agentId: currentAgentId }).status).toBe('pending');
  store.removeBinding('edison', '!project:test'); store.upsertBinding(binding);
  expect(create('rebound', { ...command(), agentId: currentAgentId }).status).toBe('pending');
});

test('agent supplied scope cannot grant authority and failed persistence leaves no saved authorization', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'hagency-grants-')); roots.push(root);
  const store = new ApprovalStore(path.join(root, 'approvals.json'), { isTaskActive: () => true });
  store.upsertBinding(binding);
  const forged = store.createRequest({ ...body('forged'), execution: command(), reusable_scope: { description: 'anything' } });
  expect(store.getRequest(forged.id, { matrix: true }).reusable_scope).toBeUndefined();
  expect(store.submitMatrixVerdict(forged.id, verdict(forged, 'approve_always')).ok).toBe(false);
  const valid = store.createRequest(body('valid'), { execution: command() });
  const save = vi.spyOn(store, '_save').mockImplementation(() => { throw new Error('disk failed'); });
  expect(() => store.submitMatrixVerdict(valid.id, verdict(valid, 'approve_always'))).toThrow('disk failed');
  expect(store.listGrants('edison')).toHaveLength(0);
  expect(store.getRequest(valid.id).status).toBe('pending');
  save.mockRestore();
});
