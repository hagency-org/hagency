import { afterAll, beforeAll, describe, expect, test } from 'vitest';
import Ajv2020 from 'ajv/dist/2020.js';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { restoreEnv, snapshotEnv } from './helpers/env.js';

const read = (file) => JSON.parse(readFileSync(new URL(file, import.meta.url), 'utf8'));
const corpus = read('./fixtures/native-approval-wire.json');
const ajv = new Ajv2020({ allErrors: true, strict: false });
const request = ajv.compile(read('../schemas/approval/owner-request-v1.schema.json'));
const verdict = ajv.compile(read('../schemas/approval/owner-verdict-v1.schema.json'));
const key = 'com.agentchat.approval';
const detail = (content) => content[key];
const copy = (value) => structuredClone(value);
const native = (name = 'posix') => copy(corpus.cards.find((row) => row.name === name).content);
const check = (validate, content) => expect(validate(content), JSON.stringify(validate.errors)).toBe(true);

// The schema describes syntax. This separate aggregate check documents the
// original native producer's byte boundary; it cannot confer send authority.
const nativeBytesFit = (content) => Buffer.byteLength(JSON.stringify(content), 'utf8') <= 48 * 1024;
function response(content, action) {
  const original = detail(content);
  return {
    msgtype: 'com.agentchat.approval.verdict.v1', body: `Approval: ${action}`,
    [key]: {
      version: 1, kind: 'verdict', agent: original.agent, project: original.project,
      project_room_id: original.project_room_id, request_id: original.request_id,
      input_digest: original.input_digest, action,
    },
  };
}

describe('native approval wire profiles', () => {
  let directory, environment, buildOwnerApprovalRequest, parseApprovalVerdictEvent;
  beforeAll(async () => {
    directory = mkdtempSync(path.join(os.tmpdir(), 'hagency-wire-interop-'));
    environment = snapshotEnv(['HAGENCY_RUNTIME_DIR']);
    process.env.HAGENCY_RUNTIME_DIR = directory;
    ({ buildOwnerApprovalRequest, parseApprovalVerdictEvent } = await import('../bridge-matrix.js'));
  });
  afterAll(() => {
    restoreEnv(environment);
    rmSync(directory, { recursive: true, force: true });
  });

  test('native approval wire profiles preserve actual producer packets and finite retained scopes', () => {
    expect(corpus.version).toBe(1);
    expect(corpus.producer).toBe('native_approval_wire_corpus');
    expect(corpus.cards.map((row) => row.name)).toEqual(['posix', 'drive', 'unc', 'unknown', 'long_preview']);
    expect(detail(native('long_preview')).input_preview.length).toBeGreaterThan(8192);
    expect(detail(native()).upstream_rpc_id).toBe(0);
    expect(detail(native('drive')).upstream_rpc_id).toBe(Number.MAX_SAFE_INTEGER);
    expect(detail(native('unc')).upstream_rpc_id).toBe('001');

    const legacy = {
      id: `approval_${'a'.repeat(32)}`, agent: 'legacy_agent', project: 'legacy_project',
      project_room_id: '!project:example.test', upstream_request_id: 'opaque',
      input_digest: 'b'.repeat(64), runtime: 'codex', tool_name: 'app_server_command',
      description: 'Original private request', input_preview: '{"command":"echo allowed"}',
      expires_at: 11000,
    };
    const retained = [undefined, null, 'task_a'].map((task) => buildOwnerApprovalRequest({
      ...legacy, ...(task !== undefined ? { reusable_scope: {
        description: 'Execute exactly echo allowed', workspace: '/work/a', task_id: task,
      } } : {}),
    }));
    expect(retained.map((content) => detail(content).actions.length)).toEqual([2, 3, 4]);
    for (const content of [...corpus.cards.map((row) => row.content), ...retained]) {
      check(request, content);
      expect(nativeBytesFit(content)).toBe(true);
      for (const { id } of detail(content).actions) {
        const wire = response(content, id);
        check(verdict, wire);
        expect(detail(wire)).not.toHaveProperty('upstream_rpc_id');
        expect(detail(wire)).not.toHaveProperty('upstream_request_id');
        const parsed = parseApprovalVerdictEvent('!private:example.test', {
          sender: '@owner:example.test', event_id: '$source', content: wire,
        });
        if (/^approval_[0-9a-f]{32}$/.test(detail(content).request_id)) {
          expect(parsed).toMatchObject({
            sender_mxid: '@owner:example.test', event_id: '$source',
            request_id: detail(content).request_id, input_digest: detail(content).input_digest,
            agent: detail(content).agent, project: detail(content).project,
            project_room_id: detail(content).project_room_id, action: id,
          });
        } else {
          // The retained JS backend owns only its32-hex requests. The native
          // verifier and Robrix client are separate executable integration gates.
          expect(parsed).toBeNull();
        }
      }
    }
  });
});

describe('native approval wire refusals', () => {
  test('native approval wire refusals enforce exact IDs and closed metadata on both packets', () => {
    for (const id of [
      ...[0, 31, 33, 39, 41, 64].map((size) => `approval_${'a'.repeat(size)}`),
      `approval_${'A'.repeat(40)}`, `approval_${'ａ'.repeat(40)}`,
      `approval_${'a'.repeat(40)} `, ` approval_${'a'.repeat(32)}`, `request_${'a'.repeat(40)}`,
    ]) {
      const card = native(); detail(card).request_id = id;
      expect(request(card), id).toBe(false);
      expect(verdict(response(card, 'approve_once')), id).toBe(false);
    }
    for (const field of ['owner_mxid', 'owner_dm_room_id', 'allow_all', 'connection_id']) {
      const card = native(); detail(card)[field] = 'forged'; expect(request(card), field).toBe(false);
    }
    for (const field of ['upstream_rpc_id', 'upstream_request_id', 'reusable_scope', 'actions']) {
      const wire = response(native(), 'approve_once'); detail(wire)[field] = detail(native())[field];
      expect(verdict(wire), field).toBe(false);
    }
    for (const value of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1, '', 'a'.repeat(256), null, {}, true]) {
      const card = native(); detail(card).upstream_rpc_id = value; expect(request(card)).toBe(false);
    }
    const missing = native(); delete detail(missing).upstream_rpc_id; expect(request(missing)).toBe(false);
    const action = response(native(), 'allow_everything'); expect(verdict(action)).toBe(false);
  });

  test('native approval wire refusals require exact reusable scope and canonical action order', () => {
    for (const mutate of [
      (d) => { delete d.reusable_scope; },
      (d) => { d.reusable_scope.description = ''; },
      (d) => { d.reusable_scope.task_id = null; },
      (d) => { delete d.reusable_scope.kind; },
      (d) => { d.reusable_scope.kind = 'all_operations'; },
      (d) => { d.reusable_scope.extra = true; },
      (d) => { d.actions.reverse(); },
      (d) => { d.actions.splice(1, 0, d.actions[1]); },
      (d) => { d.actions[1].id = 'approve_future'; },
      (d) => { d.actions[1].style = 'primary'; },
      (d) => { d.actions[0].unsafe = true; },
      (d) => { d.actions.splice(2, 1); },
    ]) {
      const card = native(); mutate(detail(card)); expect(request(card)).toBe(false);
    }
    for (const workspace of ['relative', 'C:relative', '\\server', '\\\\server', '\\\\?\\C:\\work', '\\\\.\\pipe\\name', '/work\0hidden']) {
      const card = native(); detail(card).reusable_scope.workspace = workspace;
      expect(request(card), workspace).toBe(false);
    }
  });

  test('native approval wire refusals keep legacy field ceilings and native encoded budget distinct', () => {
    for (const [field, limit] of [['body', 16384], ['description', 4096], ['input_preview', 8192]]) {
      const card = native('unknown'); detail(card).request_id = `approval_${'a'.repeat(32)}`;
      const target = field === 'body' ? card : detail(card);
      target[field] = 'a'.repeat(limit); check(request, card);
      target[field] += 'a'; expect(request(card), field).toBe(false);
    }
    for (const field of ['body', 'description', 'input_preview']) {
      const card = native('unknown'); const target = field === 'body' ? card : detail(card);
      target[field] = 'a'.repeat(49153); expect(request(card), field).toBe(false);
    }
    const combined = native('unknown'); combined.body = '中'.repeat(10000);
    detail(combined).input_preview = '中'.repeat(10000);
    check(request, combined); // Character-valid does not satisfy encoded transport bytes.
    expect(nativeBytesFit(combined)).toBe(false);
    for (const row of corpus.cards) expect(nativeBytesFit(row.content)).toBe(true);
  });
});
