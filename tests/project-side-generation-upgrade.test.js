import { afterEach, describe, expect, test } from 'vitest';
import { mkdtempSync, mkdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import request from 'supertest';
import { ProjectSideStore, ProjectSideStoreError } from '../lib/project-side-store.js';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';

const roots = [];
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }); });
const asCredential = () => ({ kind: 'appservice', asToken: 'old-as-secret', hsToken: 'old-hs-secret', url: 'http://127.0.0.1:8009', namespace: '@ac_.*', senderLocalpart: 'hagency' });
const registrationCredential = token => ({ kind: 'registrationToken', registrationToken: 'old-registration-secret', representativeToken: token });
const side = (credential, extra = {}) => ({ id: 'palpo.test', serverName: 'palpo.test', apiBaseUrl: 'http://127.0.0.1:8008', label: 'Existing project side', credential, representative: { mxid: '@hagency:palpo.test', localpart: 'hagency', observedAt: 10 }, active: true, accessState: 'accepted', accessCheckedAt: 10, createdAt: 1, updatedAt: 10, projects: {}, pendingCredential: null, ...extra });
const document = sides => ({ version: 1, sides, audit: [{ type: 'existing', at: 1 }] });
function disk(data) {
  const root = mkdtempSync(path.join(tmpdir(), 'side-generation-upgrade-')); roots.push(root);
  const parent = path.join(root, 'data'); mkdirSync(parent);
  const file = path.join(parent, 'project-sides.json');
  writeFileSync(file, JSON.stringify(data, null, 1) + '\n', { mode: 0o600 });
  return { root, parent, file, bytes: readFileSync(file) };
}

describe('project-side generation upgrade', () => {
  test('old appservice generation is durably added without changing authority', () => {
    const before = document({ 'palpo.test': side(asCredential()) });
    const { file } = disk(before);
    const store = new ProjectSideStore(file);
    const generation = store.credentialFor('palpo.test').outboundGeneration;
    expect(generation).toMatch(/^[0-9a-f]{32}$/);
    const saved = JSON.parse(readFileSync(file, 'utf8'));
    expect(saved.sides['palpo.test'].credential.outboundGeneration).toBe(generation);
    delete saved.sides['palpo.test'].credential.outboundGeneration;
    expect(saved).toEqual(before);
    expect(statSync(file).mode & 0o777).toBe(0o600);
    const bytes = readFileSync(file);
    expect(new ProjectSideStore(file).credentialFor('palpo.test').outboundGeneration).toBe(generation);
    expect(readFileSync(file)).toEqual(bytes);
  });

  test('old representative and staged generations preserve pending and inactive state', () => {
    const before = document({
      'palpo.test': side(registrationCredential('old-representative-secret'), { active: false, pendingCredential: asCredential(), pendingIssuedAt: 15 }),
      'unminted.test': side(registrationCredential(null), { id: 'unminted.test', serverName: 'unminted.test', accessState: 'unverified', representative: null }),
    });
    const { file } = disk(before); const store = new ProjectSideStore(file);
    const active = store.credentialFor('palpo.test').outboundGeneration;
    const staged = store.pendingCredentialFor('palpo.test').outboundGeneration;
    expect(active).toMatch(/^[0-9a-f]{32}$/); expect(staged).toMatch(/^[0-9a-f]{32}$/); expect(active).not.toBe(staged);
    const saved = JSON.parse(readFileSync(file, 'utf8'));
    delete saved.sides['palpo.test'].credential.outboundGeneration;
    delete saved.sides['palpo.test'].pendingCredential.outboundGeneration;
    expect(saved).toEqual(before);
    expect(store.credentialFor('unminted.test').outboundGeneration).toBeUndefined();
    const reload = new ProjectSideStore(file);
    expect(reload.credentialFor('palpo.test').outboundGeneration).toBe(active);
    expect(reload.pendingCredentialFor('palpo.test').outboundGeneration).toBe(staged);
  });

  test('current and empty stores do not write during construction', () => {
    class NoWrite extends ProjectSideStore { _save() { throw new Error('unexpected migration write'); } }
    for (const sides of [{}, { 'palpo.test': side({ ...asCredential(), outboundGeneration: 'existing-generation' }) }, { 'palpo.test': side(registrationCredential(null)) }]) {
      const { file, bytes } = disk(document(sides));
      expect(() => new NoWrite(file)).not.toThrow(); expect(readFileSync(file)).toEqual(bytes);
    }
  });

  test('migrated generation survives equal credentials and changes on actual token rotation', () => {
    const { file } = disk(document({ 'palpo.test': side(asCredential()) }));
    const store = new ProjectSideStore(file); const generation = store.credentialFor('palpo.test').outboundGeneration;
    expect(generation).toMatch(/^[0-9a-f]{32}$/);
    store.setCredential('palpo.test', asCredential());
    expect(store.credentialFor('palpo.test').outboundGeneration).toBe(generation);
    store.setCredential('palpo.test', { ...asCredential(), asToken: 'actually-rotated-secret' });
    expect(store.credentialFor('palpo.test').outboundGeneration).not.toBe(generation);
  });

  test('failed migration persistence aborts construction and preserves original credentials', () => {
    const { root, parent, file, bytes } = disk(document({ 'palpo.test': side(asCredential()) }));
    const retained = path.join(root, 'retained');
    class ParentRemovedAfterRead extends ProjectSideStore {
      _load() { const parsed = super._load(); renameSync(parent, retained); writeFileSync(parent, 'blocks directory creation'); return parsed; }
    }
    expect(() => new ParentRemovedAfterRead(file)).toThrow();
    expect(readFileSync(path.join(retained, 'project-sides.json'))).toEqual(bytes);
  });

  test('malformed existing generation fails closed without rewriting the file', () => {
    for (const value of [42, {}, '', ' '.repeat(3)]) {
      const { file, bytes } = disk(document({ 'palpo.test': side({ ...asCredential(), outboundGeneration: value }) }));
      expect(() => new ProjectSideStore(file)).toThrow(ProjectSideStoreError);
      expect(readFileSync(file)).toEqual(bytes);
    }
  });

  test.each(['appservice', 'registrationToken'])('protected acting API exposes durable upgraded generation while public projection stays private (%s)', async kind => {
    const credential = kind === 'appservice' ? asCredential() : registrationCredential('old-representative-secret');
    const original = document({ 'palpo.test': side(credential) });
    const context = await createBackendTestContext('side-generation-api-', {
      rawDataFiles: { 'project-sides.json': JSON.stringify(original) },
      env: { MATRIX_BRIDGE_SECRET: 'generation-api-test-secret', MATRIX_BOT_USERNAME: '', MATRIX_SERVER_NAME: 'botless.invalid' },
    });
    try {
      const acting = await request(context.app).get('/api/project-sides/acting-credentials').set('X-Bridge-Secret', 'generation-api-test-secret');
      expect(acting.status).toBe(200);
      expect(acting.body.sides).toHaveLength(1);
      const generation = acting.body.sides[0].outboundGeneration;
      expect(generation).toMatch(/^[0-9a-f]{32}$/);
      const persisted = JSON.parse(readFileSync(path.join(context.runtimeDir, 'data/project-sides.json'), 'utf8'));
      expect(persisted.sides['palpo.test'].credential.outboundGeneration).toBe(generation);
      const publicResult = await request(context.app).get('/api/project-sides');
      expect(publicResult.status).toBe(200);
      const publicText = JSON.stringify(publicResult.body);
      for (const secret of ['old-as-secret', 'old-hs-secret', 'old-registration-secret', 'old-representative-secret', generation]) expect(publicText).not.toContain(secret);
      const unauthorized = await request(context.app).get('/api/project-sides/acting-credentials');
      expect(unauthorized.status).toBe(403);
    } finally { await context.cleanup(); }
  });
});
