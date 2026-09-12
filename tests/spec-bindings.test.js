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
