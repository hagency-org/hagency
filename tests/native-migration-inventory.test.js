import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { buildInventory, checkInventory, repositorySources } from '../native/scripts/inventory.mjs';
import { customBranches, helperCandidate, proxySurface, registrations, reviewedLinks } from '../native/scripts/inventory-source.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const policy = JSON.parse(readFileSync(new URL('../native/fixtures/inventory-policy.json', import.meta.url), 'utf8'));
const fixture = JSON.parse(readFileSync(new URL('../native/fixtures/legacy-inventory.json', import.meta.url), 'utf8'));

describe('native migration source inventory', () => {
  it('native_inventory_routes_resolve_registration', () => {
    const sources = new Map([
      ['app.js', String.raw`import web from 'express';
const app = web(); const alias = app;
// app.get('/comment-only', handler);
const documentation = "app.post('/string-only', handler)";
app.get('/literal', bearer, handler);
alias['post']('/computed-property', handler);
app.route('/chained').get(handler).put(handler);
app.get(['/array/a', '/array/b'], handler);
app.get(/^\/regex\/(.+)$/, handler);
app.use((req, res, next) => next());
sse.installRoute(app, '/events');
sse.installRoute(app);
installRoute(app, '/direct-events');
server.tool('task_done', 'Explicit completion', {}, handler);
throw new Error('Source inspection must never execute this module');`],
      ['sse.js', `function installRoute(app, route = '/default-events') {
  app.get(route, handler);
}`],
    ]);
    const result = registrations(sources);
    expect(result.routes.map(route => route.paths)).toEqual([
      ['/literal'], ['/computed-property'], ['/chained'], ['/chained'],
      ['/array/a', '/array/b'], [{ regex: String.raw`^\/regex\/(.+)$`, flags: '' }], ['/'],
      ['/events', '/default-events', '/direct-events'],
    ]);
    expect(result.routes[0]).toMatchObject({ source: 'app.js', line: 5, method: 'GET', handlers: ['bearer', 'handler'] });
    expect(result.routes.at(-1).installed_at).toHaveLength(3);
    expect(result.routes.at(-1).default_paths).toEqual(['/default-events']);
    expect(result.tools[0].name).toBe('task_done');
    expect(reviewedLinks(result.parsed, [{ source: 'app.js', callee: 'installRoute', target: 'sse.js' }])[0].sites).toHaveLength(1);
    expect(() => reviewedLinks(result.parsed, [{ source: 'app.js', callee: 'removedInstaller', target: 'sse.js' }])).toThrow(/Stale reviewed dispatcher link/);
    expect(result.routes.every(route => /^[a-f0-9]{64}$/.test(route.sha256))).toBe(true);
    for (const source of [
      "app.post(routeFromConfig, handler)",
      "app[methodFromConfig]('/ambiguous', handler)",
      "function installRoute(app, route = '/fallback') { app.get(route, handler); } instance.installRoute(app, runtimePath);",
      "server.tool(toolFromConfig, handler)",
    ]) expect(() => registrations(new Map([['unknown.js', source]]))).toThrow(/Unresolved/);
    expect(() => registrations(new Map([['bad.js', 'const invalid = ;']]))).toThrow(/cannot parse bad.js/);
    expect(() => registrations(new Map([['indirect.js', "function installRoute(app, route = '/fallback') { app.get(route, handler); } const invoke = installRoute; invoke(app, '/hidden');"]]))).toThrow(/Uncalled dynamic route installer/);

    const custom = registrations(new Map([['custom.js', String.raw`function handle({method, path, body}) {
if (/^safe$/.test(body)) validate();
const match = /^\/items\/(.+)$/.exec(path || '');
if (method === 'GET' && match) return read();
if (path?.startsWith('/delegate/')) return downstream();
if (/^\/users\//.test(path) && method === 'PUT') return write();
}`]]));
    const branches = customBranches('custom.js', custom.parsed);
    expect(branches).toHaveLength(3);
    expect(branches[0]).toMatchObject({ line: 4, methods: ['GET'], paths: [{ regex: String.raw`^\/items\/(.+)$`, flags: '' }] });
    expect(branches[1].paths).toEqual([{ prefix: '/delegate/' }]);
    expect(branches[2].methods).toEqual(['PUT']);
    const unresolved = registrations(new Map([['unknown.js', "if (method === 'GET' && customPredicate()) dispatch();"]]));
    expect(() => customBranches('unknown.js', unresolved.parsed)).toThrow(/Unresolved dispatcher branch/);
    const proxy = registrations(new Map([['proxy.js', `const READS = [/^agents$/];
const WRITES = [{method:'POST',re:/^agents$/}];
export const GET = handler; export const POST = handler;`]]));
    expect(proxySurface('proxy.js', proxy.parsed)).toMatchObject({
      methods: [{ method: 'GET' }, { method: 'POST' }],
      allowlist: [{ method: 'GET', pattern: { pattern: '^agents$', flags: '' } }, { method: 'POST', pattern: { pattern: '^agents$', flags: '' } }],
    });
  });

  it('native_inventory_classification_rejects_drift', () => {
    const all = repositorySources(root);
    const missing = structuredClone(policy);
    missing.helper_groups[0].files.shift();
    expect(() => buildInventory(all, missing)).toThrow(/Unclassified helper/);
    const stale = structuredClone(policy);
    stale.helper_groups[0].files.push('bin/removed-helper');
    expect(() => buildInventory(all, stale)).toThrow(/Stale helper classifications/);
    const duplicate = structuredClone(policy);
    duplicate.helper_groups[1].files.push(duplicate.helper_groups[0].files[0]);
    expect(() => buildInventory(all, duplicate)).toThrow(/Duplicate helper classification/);
    const added = new Map(all).set('scripts/new-installed-helper.mjs', '#!/usr/bin/env node\nthrow new Error("never execute");');
    expect(() => buildInventory(added, policy)).toThrow(/Unclassified helper/);
    expect(helperCandidate('lib/new-hook.js', '#!/usr/bin/env node\n')).toBe(true);
    expect(helperCandidate('remote/scripts/new-helper.js', '// helper')).toBe(true);
    expect(helperCandidate('docs/example.js', '#!/usr/bin/env node\n')).toBe(false);
    const changed = structuredClone(fixture);
    changed.http_registrations[0].paths = ['/unreviewed'];
    expect(() => checkInventory(changed, fixture)).toThrow(/inventory drift/);
    const changedSource = new Map(all).set('lib/backend/sse-adapter.js', `${all.get('lib/backend/sse-adapter.js')}\n// Syntax-neutral source drift still needs review.\n`);
    expect(() => checkInventory(buildInventory(changedSource, policy), fixture)).toThrow(/inventory drift/);
    const unknownApi = new Map(all).set('lib/new-listener.js', 'http.createServer(handler);');
    expect(() => buildInventory(unknownApi, policy)).toThrow(/Unclassified HTTP listener/);
    const closed = structuredClone(policy);
    closed.remaining_gates[0].status = 'complete';
    expect(() => buildInventory(all, closed)).toThrow(/cannot close release gates/);
  });

  it('native_inventory_reproduces_current_sources', () => {
    const actual = buildInventory(repositorySources(root), policy);
    expect(() => checkInventory(actual, fixture)).not.toThrow();
    expect(actual.entries).toHaveLength(138);
    expect(actual.http_registrations.filter(item => item.kind === 'route')).toHaveLength(199);
    expect(actual.http_registrations.filter(item => item.kind === 'middleware')).toHaveLength(3);
    expect(actual.custom_dispatch.reduce((count, dispatcher) => count + dispatcher.branches.length, 0)).toBe(12);
    expect(actual.http_listeners).toHaveLength(2);
    expect(actual.cli_commands).toHaveLength(32);
    expect(actual.mcp_tools).toHaveLength(32);
    expect(new Set(actual.mcp_tools.map(tool => tool.name)).size).toBe(16);
    expect(actual.console_proxy.allowlist).toHaveLength(61);
    const sse = actual.http_registrations.find(item => item.source === 'lib/backend/sse-adapter.js');
    expect(sse.paths).toEqual(['/api/stream']);
    expect(sse.installed_at[0]).toMatchObject({ source: 'backend-v2.js', line: 7937 });
    expect(actual.entries.find(item => item.path === 'scripts/hagency-stable-autodeploy.sh').role).toBe('install');
    expect(actual.entries.find(item => item.path === 'scripts/audit-deps.sh').role).toBe('operator-and-build');
    expect(actual.entries.every(item => item.migration.native_status === 'parity-unverified')).toBe(true);
    expect(actual.http_registrations.every(item => item.migration.native_status === 'parity-unverified')).toBe(true);
    expect(actual.remaining_gates.every(item => item.status === 'open')).toBe(true);
    // The checker constructs data and compares it. It must not rewrite the
    // reviewed file as a side effect of validation or execute any application.
    expect(JSON.parse(readFileSync(new URL('../native/fixtures/legacy-inventory.json', import.meta.url), 'utf8'))).toEqual(fixture);
  });
});
