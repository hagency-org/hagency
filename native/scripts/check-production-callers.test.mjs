// Tests for check-production-callers.mjs (ADR-146). Fixture trees, no repo
// dependency: checkProductionCallers accepts { root, read }.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import {
  stripTestItems,
  extractFns,
  parseSpecLines,
  parseAdrGaps,
  checkProductionCallers,
} from './check-production-callers.mjs';

test('stripTestItems removes cfg(test) modules and test fns, keeps production fns', () => {
  const src = [
    'pub fn approve(&self) { let x = 1; }',
    '',
    '#[cfg(test)]',
    'mod tests {',
    '    use super::*;',
    '    #[test]',
    '    fn approve_works() {',
    '        assert!(true);',
    '    }',
    '}',
    '',
    'pub fn observe(&self) {',
    '    self.approve();',
    '}',
  ].join('\n');
  const out = stripTestItems(src);
  assert.match(out, /pub fn approve/);
  assert.match(out, /pub fn observe/);
  assert.doesNotMatch(out, /approve_works/);
  assert.doesNotMatch(out, /mod tests/);
  const fns = extractFns(out).map((f) => f.name);
  assert.deepEqual(fns.sort(), ['approve', 'observe']);
});

test('stripTestItems removes a bare #[test] fn and #[tokio::test] fn', () => {
  const src = '#[test]\nfn unit() { assert_eq!(1, 1); }\n#[tokio::test]\nasync fn atest() { assert!(true); }\npub fn real() {}\n';
  const out = stripTestItems(src);
  assert.doesNotMatch(out, /\bunit\b/);
  assert.doesNotMatch(out, /\batest\b/);
  assert.match(out, /pub fn real/);
});

test('parseSpecLines classifies wired and owed Production caller lines', () => {
  const spec = [
    '  Production caller: hagency::bootstrap::accounts::run',
    '  Production caller: owed (G2)',
  ].join('\n');
  const lines = parseSpecLines(spec, 's.spec.md');
  assert.equal(lines.length, 2);
  assert.deepEqual(lines[0].parsed, { kind: 'caller', full: 'hagency::bootstrap::accounts::run', name: 'run' });
  assert.deepEqual(lines[1].parsed, { kind: 'owed', gap: 'G2' });
});

test('parseAdrGaps reads the gap ids from the ADR table', () => {
  const adr = '| gap G1 — owner |\n| gap G2 |\n**8 open gaps (G1–G8)**\n';
  const gaps = parseAdrGaps(adr);
  assert.ok(gaps.has('G1') && gaps.has('G2') && gaps.has('G8'));
});

function makeFixture({ rust, specs, adr }) {
  const root = mkdtempSync(path.join(tmpdir(), 'pc-'));
  mkdirSync(path.join(root, 'specs'), { recursive: true });
  mkdirSync(path.join(root, 'knowledge/decisions'), { recursive: true });
  for (const [rel, content] of Object.entries(rust)) {
    mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    writeFileSync(path.join(root, rel), content);
  }
  writeFileSync(path.join(root, 'specs', 'fixture.spec.md'), specs);
  writeFileSync(path.join(root, 'knowledge/decisions', 'adr-146-production-callers-and-store-surface.md'), adr);
  const files = Object.keys(rust);
  return { root, files, read: (rel) => (files.includes(rel) ? rust[rel] : readFileSync(path.join(root, rel), 'utf8')) };
}

const MAIN = 'native/hagency/src/main.rs';

test('native_production_callers_wired: a wired caller resolves from a root', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() {}\n',
      'native/hagency/src/bootstrap/accounts.rs': 'pub fn run() { helper(); }\nfn helper() {}\n',
    },
    specs: '  Production caller: hagency::bootstrap::accounts::run\n',
    adr: '| gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok);
  assert.equal(result.wired, 1);
  assert.equal(result.missing.length, 0);
});

test('native_production_callers_missing: a test-only caller is absent and fails', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() {}\n',
      // The only "caller" of approve lives inside a stripped cfg(test) module.
      'native/hagency-store/src/domain.rs': 'pub fn approve(&self) {}\n#[cfg(test)]\nmod tests {\n  fn t() { let db = 1; db_approve(); }\n  fn db_approve() { approve(); }\n  fn approve() {}\n}\n',
    },
    specs: '  Production caller: hagency_store::domain::approve\n',
    adr: '| gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.equal(ok, false);
  assert.equal(result.missing.length, 1);
  assert.match(result.missing[0].caller, /approve$/);
});

test('native_production_callers_owed: a resolvable owed id is reported, never failed', () => {
  const { root, read, files } = makeFixture({
    rust: { [MAIN]: 'fn main() {}\n' },
    specs: '  Production caller: owed (G7)\n',
    adr: '| `revoke_approval_grant` | gap G7 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok);
  assert.equal(result.owed.length, 1);
  assert.match(result.owed[0], /G7/);
});

test('native_production_callers_unknown_gap: an unknown owed id fails the checker', () => {
  const { root, read, files } = makeFixture({
    rust: { [MAIN]: 'fn main() {}\n' },
    specs: '  Production caller: owed (G42)\n',
    adr: '| gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.equal(ok, false);
  assert.equal(result.unknownGaps.length, 1);
  assert.equal(result.unknownGaps[0].gap, 'G42');
});
