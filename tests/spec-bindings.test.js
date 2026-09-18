import { execFileSync } from 'node:child_process';
import { expect, test } from 'vitest';

test('active spec selectors resolve to registered tests', () => {
  const raw = execFileSync(process.execPath, ['scripts/check-spec-bindings.js'], {
    encoding: 'utf8', timeout: 120_000, maxBuffer: 32 * 1024 * 1024,
  });
  const result = JSON.parse(raw);
  expect(result.count).toBeGreaterThan(150);
  expect(result.missing).toEqual([]);
}, 180_000);


test('native contracts use their Cargo catalog and cannot silently lose selectors', async () => {
  const { checkSpecBindings } = await import('../scripts/check-spec-bindings.js');
  const missing = checkSpecBindings([], { runtime: 'rust' });
  expect(missing.count).toBeGreaterThanOrEqual(8);
  expect(missing.missing.some(row => row.selector === 'custody_survives_restart')).toBe(true);
  expect(missing.deferred).toContain('project.spec.md');
  expect(checkSpecBindings([], { runtime: 'node' }).deferred).toContain('task-rust-foundation.spec.md');
  const catalog = missing.missing.map(row => ({ name: `fixture::${row.selector}` }));
  expect(checkSpecBindings(catalog, { runtime: 'rust' }).missing).toEqual([]);
  expect(() => checkSpecBindings([], { runtime: 'typo' })).toThrow('Unknown spec runtime');
});

test('a platform-tagged spec binds only on its own platform', async () => {
  const { checkSpecBindings } = await import('../scripts/check-spec-bindings.js');
  const spec = 'task-rust-macos-descendant-parity.spec.md';
  const selector = 'native_macos_descendant_tracking';
  const on = (platform) => checkSpecBindings([], { runtime: 'rust', platform });
  expect(on('darwin').missing.some(row => row.file === spec && row.selector === selector)).toBe(true);
  expect(on('darwin').deferred).not.toContain(spec);
  for (const platform of ['linux', 'win32']) {
    expect(on(platform).missing.some(row => row.file === spec)).toBe(false);
    expect(on(platform).deferred).toContain(spec);
  }
  // An untagged native spec still has to resolve everywhere.
  expect(on('linux').missing.some(row => row.selector === 'custody_survives_restart')).toBe(true);
  expect(() => checkSpecBindings([], { runtime: 'rust', platform: 'typo' })).toThrow('Unknown spec platform');
});
