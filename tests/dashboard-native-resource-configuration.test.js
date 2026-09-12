import { describe, expect, test, vi } from 'vitest';
import { renderDashboard } from './helpers/dashboard-render.js';
import { configurationSelection, configurationView, validateConfiguration, configureResource } from '../mockup/lib/native-api.js';
const resource = { id: `resource_${'b'.repeat(24)}`, framework: 'codex', model: 'gpt-5.6-sol', provider: null, reasoning: 'medium', ceiling: { tokens: null }, published: false, roles: [], revision: 'a'.repeat(64) };
const editor = { resource, choices: [{ model: resource.model, reasoning: resource.reasoning, tier: 'medium', roles: ['Builder'] }], modelTier: 'medium', modelRoles: ['Builder'] };
describe('retained native resource configuration', () => {
  test('closed selected observations preserve unknown ceilings and reject private fields', () => {
    expect(validateConfiguration(editor, resource.id)).toBe(editor);
    expect(editor.resource.ceiling).not.toHaveProperty('period');
    for (const field of ['presetId', 'seatId', 'authHome', 'config']) expect(() => validateConfiguration({ ...editor, resource: { ...resource, [field]: 'private' } }, resource.id)).toThrow();
    expect(() => validateConfiguration({ ...editor, choices: Array(257).fill(editor.choices[0]) }, resource.id)).toThrow();
    expect(configurationView({ pathname: '/console/resources/new/' })).toBe(true);
    expect(configurationSelection({ search: `?resource_id=${resource.id}` })).toEqual({ id: resource.id, mode: 'edit' });
    for (const search of ['?resource_id=a&source_resource_id=b', '?source_resource_id=a&source_resource_id=b', '?unknown=a']) expect(() => configurationSelection({ search })).toThrow();
  });
  test('actual retained wizard presents native account scope in both languages', async () => {
    for (const locale of ['en', 'zh']) {
      const html = await renderDashboard('mockup/app/resources/new/page.jsx', { locale, data: { nativeConsole: true, configurationConsole: true, phase: 'ready', editor, resources: [resource], permissions: { configureResource: true }, editing: false } });
      expect(html).toContain(locale === 'en' ? 'Create another configuration' : '创建另一项配置');
      expect(html).toContain('steps wizard'); expect(html).toContain(resource.model);
      for (const input of ['wz-name', 'wz-rate', 'apiKey', 'authHome']) expect(html).not.toContain(`id="${input}"`);
      expect(html).not.toContain('undefined');
    }
    const empty = await renderDashboard('mockup/app/resources/new/page.jsx', { data: { nativeConsole: true, phase: 'ready', editor: null } });
    expect(empty).toContain('First-resource enrollment'); expect(empty).not.toContain('configuration-source');
  });
  test('unknown creation is retained with inspection and never a success announcement', async () => {
    const html = await renderDashboard('mockup/app/resources/new/page.jsx', { data: { nativeConsole: true, phase: 'access', action: { configuration: true, kind: 'unknown' } } });
    expect(html).toContain('A resource may already have been created'); expect(html).toContain('will not be retried automatically'); expect(html).not.toContain('Local configuration saved.');
  });
  test('create uses public source and exact change unions with one request on lost reply', async () => {
    const changes = { profileChange: { kind: 'preserve' }, ceilingChange: { kind: 'clear' } };
    const fetch = vi.fn(async () => { throw new TypeError('lost original reply'); }); vi.stubGlobal('fetch', fetch);
    await expect(configureResource(resource, true, changes)).rejects.toThrow('outcome_unknown'); expect(fetch).toHaveBeenCalledTimes(1);
    const [path, options] = fetch.mock.calls[0]; expect(path).toBe('/console/api/resources'); expect(options.method).toBe('POST');
    expect(JSON.parse(options.body)).toEqual({ expectedRevision: resource.revision, sourceResourceId: resource.id, ...changes });
    vi.unstubAllGlobals();
  });
});
