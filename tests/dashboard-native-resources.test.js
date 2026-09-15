import { describe, expect, test, vi } from 'vitest';
import { renderDashboard } from './helpers/dashboard-render.js';
import { validateResources, validateBudget, selection, resourceView, publishResource, logoutNative } from '../mockup/lib/native-api.js';
const resource = { id: 'new_resource', framework: 'codex', model: 'gpt-5.6-sol', provider: null, reasoning: 'medium', ceiling: null, published: true, roles: [], revision: 'a'.repeat(64) };
const roles = ['Architect','Builder','Reviewer','Tester','Writer','Researcher'].map((role) => ({ role, explicitPublication: null, available: false, crossFamily: false, defaultTier: 'medium' }));
const rows = { resources: [resource], roles, next_after: null, permissions: { publishResource: false, configureResource: false } };
const budget = { scope: 'resource', pool: { ceiling: null, period: null, committed: 0, remaining: null }, seat: { quota: null, period: null, committed: 0, remaining: null, status: 'undeclared' }, reserved: 0, remainingTokens: null };
describe('native resource publication controls', () => {
  test('closed native resource and budget observations retain missing values', () => {
    expect(validateResources(rows)).toBe(rows); expect(validateBudget(budget)).toBe(budget);
    for (const field of ['seatId', 'authHome', 'presetId', 'config']) expect(() => validateResources({ ...rows, resources: [{ ...resource, [field]: 'private' }] })).toThrow();
    expect(() => validateBudget({ ...budget, remainingTokens: NaN })).toThrow();
    expect(selection({ search: '?resource_id=new_resource' }, 'resource_id')).toBe('new_resource');
    expect(() => selection({ search: '?resource_id=a&resource_id=b' }, 'resource_id')).toThrow();
    expect(resourceView({ pathname: '/console/resources/' })).toBe(true);
    expect(validateResources({ ...rows, resources: [{ ...resource, ceiling: { tokens: null } }] }).resources[0].ceiling).not.toHaveProperty('period');
  });
  test('retained bilingual page shows native facts and future management gaps', async () => {
    for (const locale of ['en', 'zh']) {
      const html = await renderDashboard('mockup/app/resources/page.jsx', { locale, data: { nativeConsole: true, phase: 'ready', ...rows, budget, selected: resource.id } });
      expect(html).toContain(locale === 'en' ? 'Withdraw from native catalog' : '从本地资源目录撤下');
      expect(html).toContain('disabled');
      expect(html).toContain(locale === 'en' ? 'Unknown' : '未知');
      expect(html).toContain(locale === 'en' ? 'Delivery to Palpo has not been verified' : '尚未验证向 Palpo');
      expect(html).not.toContain('NaN');
    }
  });
  test('unresolved logout has a visible retry without claiming success', async () => {
    for (const status of ['busy', 'unknown']) {
      const html = await renderDashboard('mockup/app/resources/page.jsx', { data: { nativeConsole: true, phase: 'access', logoutStatus: status } });
      expect(html).toContain('Retry ending access'); expect(html).toContain('Server revocation is unresolved'); expect(html).not.toContain('Console access ended');
    }
  });
  test('action conflict and unknown remain explicit during reconciliation', async () => {
    for (const kind of ['conflict', 'unknown']) {
      const html = await renderDashboard('mockup/app/resources/page.jsx', { data: { nativeConsole: true, phase: 'ready', ...rows, budget, selected: resource.id, action: { kind, label: 'codex' } } });
      expect(html).toContain(`data-resource-action="${kind}"`); expect(html).toContain('Read current state');
    }
  });
  test('transport loss and original busy responses do not become mutation success', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('actual transport unavailable'); }));
    await expect(publishResource(resource, false)).rejects.toThrow('outcome_unknown');
    await expect(logoutNative()).rejects.toThrow('logout_unknown');
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ ok: false, code: 'console_busy' }), { status: 429, headers: { 'content-type': 'application/json' } })));
    await expect(logoutNative()).rejects.toThrow('busy');
    vi.unstubAllGlobals();
  });
});
