import { afterEach, expect, test, vi } from 'vitest';
import { fetchStopped, inspectStopped, resolutionBody, resolveStopped, validateInspection, validateStopped } from '../mockup/lib/native-recovery.js';
import { missingKeys, orphanKeys, placeholderMismatches, translate } from '../mockup/lib/i18n.js';
import { renderDashboard } from './helpers/dashboard-render.js';

const task = { id: 'task', session_id: 'session', creator_session_id: null, title: 'Inspect result', description: '', priority: 'p2', granularity: 'task', labels: [], parent_id: null,
  status: 'in_progress', execution_epoch: 0, created_at: 1, updated_at: 2, started_at: 2, completed_at: null, heartbeat_at: null, waiting_reason: null, waiting_until: null };
const inspection = () => ({ inspectionId: 'a'.repeat(64), inspectionToken: 'b'.repeat(64), expiresAt: Date.now() + 900000,
  snapshot: { dispatchId: 'dispatch', fence: 1, receiptDigest: 'c'.repeat(64), observation: { scope: 'd'.repeat(64), workspace: 'work',
    inventory: { profile: 'stopped-content-inventory-v1', root: { platform: 'unix-v1', volume: '0000000000000001', object: Array(16).fill(0) },
      entries: [{ path: '<img src=x onerror=alert(1)>', kind: 'file', bytes: 5, sha256: 'e'.repeat(64), readonly: false }] } }, task: { ...task }, route: null } });
const page = { engagementId: 'agent', dispatches: [{ dispatchId: 'dispatch', sessionId: 'session', taskId: 'task', fence: 1, reason: 'owned_runner_failure', inspectionAvailable: true }], nextAfter: null };
const response = (v, status = 200) => new Response(JSON.stringify(v), { status, headers: { 'Content-Type': 'application/json' } });
afterEach(() => vi.unstubAllGlobals());

test('native recovery protocol binds decisions and results', async () => {
  const i = inspection(); expect(validateInspection(i, 'agent', 'dispatch')).toBe(i);
  for (const action of ['continue', 'accept_completed', 'keep_blocked']) {
    const body = resolutionBody(i, action, 'Reviewed exact original effects', 'Finish remaining work', 'request', 'replacement');
    const input = JSON.parse(body);
    expect(input.original).toBe('dispatch'); expect(input.inspectionToken).toBe(i.inspectionToken);
    expect(input.replacement).toEqual(action === 'continue' ? { id: 'replacement', session_id: 'session', task_id: 'task', resources: [{ id: 'work', exclusive: true }], payload: { instruction: 'Finish remaining work' } } : null);
    const receipt = { requestId: 'request', original: 'dispatch', action, replacement: input.replacement?.id ?? null, resolvedAt: 3, task: { ...task, status: action === 'accept_completed' ? 'done' : action === 'keep_blocked' ? 'blocked' : 'in_progress' } };
    const fetch = vi.fn().mockRejectedValueOnce(new TypeError('lost response')).mockResolvedValueOnce(response(receipt));
    vi.stubGlobal('fetch', fetch);
    await expect(resolveStopped('agent', body, task)).rejects.toThrow('outcome_unknown'); expect(fetch).toHaveBeenCalledTimes(1);
    expect(await resolveStopped('agent', body, task)).toEqual(receipt); expect(fetch).toHaveBeenCalledTimes(2);
    expect(fetch.mock.calls[0][1].body).toBe(fetch.mock.calls[1][1].body);
    expect(fetch.mock.calls[0][0]).toBe('/console/api/agents/agent/resolve-stopped-dispatch');
    expect(fetch.mock.calls[0][1]).toMatchObject({ credentials: 'same-origin', redirect: 'error', cache: 'no-store' });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...receipt, task: { ...receipt.task, id: 'foreign' } })));
    await expect(resolveStopped('agent', body, task)).rejects.toThrow('outcome_unknown');
  }
  expect(() => resolutionBody(i, 'continue', 'review', '', 'request', 'replacement')).toThrow('invalid_selection');
  expect(() => resolutionBody(i, 'keep_blocked', '界'.repeat(1000), '', 'request', 'replacement')).toThrow('invalid_selection');
});

test('native recovery projections reject substitution and unbounded responses', async () => {
  expect(validateStopped(page, 'agent')).toBe(page);
  for (const bad of [{ ...page, engagementId: 'foreign' }, { ...page, secret: 'private' }, { ...page, nextAfter: 'wrong' }, { ...page, dispatches: [...page.dispatches, ...page.dispatches] }]) {
    expect(() => validateStopped(bad, 'agent')).toThrow('invalid_native_response');
  }
  const i = inspection();
  expect(() => validateInspection(i, 'agent', 'foreign')).toThrow();
  for (const change of [(v) => { v.inspectionToken = 'short'; }, (v) => { v.snapshot.observation.inventory.entries[0].secret = 'private'; }, (v) => { v.snapshot.task.extra = true; }]) {
    const v = inspection(); change(v); expect(() => validateInspection(v, 'agent', 'dispatch')).toThrow();
  }
  const large = inspection(); large.snapshot.observation.inventory.entries = Array.from({ length: 100 }, (_, n) => ({ ...large.snapshot.observation.inventory.entries[0], path: `path${n}_` + 'a'.repeat(1024) }));
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response(large)));
  expect((await inspectStopped('agent', 'dispatch')).snapshot.observation.inventory.entries).toHaveLength(100);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ padding: 'x'.repeat(65537) })));
  await expect(fetchStopped('agent')).rejects.toThrow('invalid_native_response');
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ padding: 'x'.repeat(2 * 1024 * 1024 + 1) })));
  await expect(inspectStopped('agent', 'dispatch')).rejects.toThrow('invalid_native_response');
});

test('native recovery controls follow lifecycle scope in both languages', async () => {
  const agent = { name: 'Worker', framework: 'codex', role: 'coding', state: 'active', engagement_id: 'agent', requested_tokens: 1, last_activity_ms: null };
  for (const locale of ['en', 'zh']) {
    for (const allowed of [false, true]) {
      const html = await renderDashboard('mockup/components/NativeAgents.jsx', { locale, data: { phase: 'ready', agents: [agent], permissions: { manageLifecycle: allowed } } });
      expect(html.includes('data-lifecycle-action="review"')).toBe(allowed);
      if (allowed) expect(html).toContain(translate(locale, 'nrec.open'));
      expect(html).not.toContain('inspectionToken'); expect(html).not.toContain('nrec.');
    }
  }
  expect(missingKeys('zh').filter((s) => s.startsWith('nrec.'))).toEqual([]);
  expect(orphanKeys('zh').filter((s) => s.startsWith('nrec.'))).toEqual([]);
  expect(placeholderMismatches('zh').filter((s) => s.key.startsWith('nrec.'))).toEqual([]);
});
