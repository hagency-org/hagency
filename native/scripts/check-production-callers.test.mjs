// Tests for check-production-callers.mjs (ADR-146). Fixture trees, no repo
// dependency: checkProductionCallers accepts { root, read, files }.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import {
  isStripped,
  stripTestItems,
  extractFns,
  extractCalls,
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

test('isStripped covers tests, examples, fixtures and probe bins', () => {
  assert.equal(isStripped('native/hagency-execution/examples/codex_qualify.rs'), true);
  assert.equal(isStripped('native/hagency-store/tests/replies.rs'), true);
  assert.equal(isStripped('native/hagency/src/domain/tests.rs'), true);
  assert.equal(isStripped('native/fixtures/peer.rs'), true);
  assert.equal(isStripped('native/hagency/src/bootstrap/driver.rs'), false); // production host driver (amendment: only its cfg(test) mod strips)
  assert.equal(isStripped('native/hagency-runtime/src/bin/approval_probe/mod.rs'), true);
  assert.equal(isStripped('native/hagency/src/console.rs'), false);
  assert.equal(isStripped('native/hagency-store/src/domain.rs'), false);
});

test('extractCalls treats handler and spawn arguments as edges', () => {
  const body = 'Router::with_path("/x").get(list_agents).post(create_agent).hoop(guard); tokio::spawn(sweep_loop); call_site(other);';
  const calls = extractCalls(body);
  for (const name of ['list_agents', 'create_agent', 'guard', 'sweep_loop', 'call_site']) {
    assert.ok(calls.includes(name), `expected edge to ${name}`);
  }
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

test('parseAdrGaps reads only the gap table rows, including slash-shared ids', () => {
  const adr = [
    '| `approve` | domain.rs:1145 | none | gap G2 |',
    '| `retry_cleanup` | domain.rs:1265 | none | gap G2/G5 (shared) |',
    '',
    'The gaps table has 8 open gaps (G1–G8); G9 resolved as superseded.', // prose: not a row
  ].join('\n');
  const gaps = parseAdrGaps(adr);
  assert.ok(gaps.has('G2') && gaps.has('G5'));
  assert.equal(gaps.has('G9'), false);
  assert.equal(gaps.has('G1'), false); // range mention in prose is not a row
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

test('native_production_callers_wired: a full-path caller reached by an unambiguous chain is wired', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() { bootstrap::accounts::run(); }\n',
      'native/hagency/src/bootstrap/accounts.rs': 'pub fn run() { helper(); }\nfn helper() {}\n',
    },
    specs: '  Production caller: hagency::bootstrap::accounts::run\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok, JSON.stringify(result));
  assert.equal(result.wired, 1);
  assert.equal(result.missing.length, 0);
  assert.equal(result.ambiguous.length, 0);
});

test('native_production_callers_missing: an unreachable full path is missing and fails', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() {}\n',
      // Integration probe: approve exists in domain.rs but nothing in the
      // production graph calls it (the test-module call is stripped).
      'native/hagency-store/src/domain.rs': [
        'impl DomainRepository {',
        '  pub fn approve(&self) {}',
        '}',
        '#[cfg(test)]',
        'mod tests {',
        '  fn t() { approve(); }',
        '}',
      ].join('\n'),
    },
    specs: '  Production caller: hagency_store::domain::DomainRepository::approve\n',
    adr: '| `approve` | gap G2 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.equal(ok, false);
  assert.equal(result.missing.length, 1);
  assert.match(result.missing[0].caller, /approve$/);
  assert.match(result.missing[0].definition, /domain\.rs::approve$/);
});

test('native_production_callers_ambiguous: a homonym collision reached only through ambiguous edges fails', () => {
  const { root, read, files } = makeFixture({
    rust: {
      // Integration probe: `admit` collides with MediaBudget::admit and
      // control.admit; the bare `.admit(` edge binds all three.
      [MAIN]: 'fn main() { run(); }\nfn run() { self.admit(); }\n',
      'native/hagency-store/src/domain.rs': 'impl DomainRepository {\n  pub fn admit(&self) {}\n}\n',
      'native/hagency-media/src/lib.rs': 'impl MediaBudget {\n  pub fn admit(&self) {}\n}\n',
      'native/hagency-runtime/src/codex/session/driver.rs': 'impl Control {\n  pub fn admit(&self) {}\n}\n',
    },
    specs: '  Production caller: hagency_store::domain::DomainRepository::admit\n',
    adr: '| `admit` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.equal(ok, false);
  assert.equal(result.missing.length, 0);
  assert.equal(result.ambiguous.length, 1);
  assert.match(result.ambiguous[0].definition, /domain\.rs::admit$/);
  assert.ok(result.ambiguous[0].chain.length > 0);
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
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.equal(ok, false);
  assert.equal(result.unknownGaps.length, 1);
  assert.equal(result.unknownGaps[0].gap, 'G42');
});

test('an unresolvable full path is reported unresolved and fails', () => {
  const { root, read, files } = makeFixture({
    rust: { [MAIN]: 'fn main() {}\n' },
    specs: '  Production caller: hagency_store::domain::DomainRepository::no_such_fn\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.equal(ok, false);
  assert.equal(result.unresolved.length, 1);
  assert.match(result.unresolved[0].reason, /no module file for hagency_store::domain/);
});

test('a production file that also carries mod tests: production fns reachable, test fns stripped', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() { bootstrap::driver::settle_pending_stops(); }\n',
      'native/hagency/src/bootstrap/driver.rs': [
        'pub fn settle_pending_stops() { helper(); }',
        'fn helper() {}',
        '#[cfg(test)]',
        'mod tests {',
        '  fn only_test() { assert!(true); }',
        '}',
      ].join('\n'),
    },
    specs: [
      '  Production caller: hagency::bootstrap::driver::settle_pending_stops',
      '  Production caller: hagency::bootstrap::driver::only_test',
    ].join('\n') + '\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.equal(ok, false); // the mod tests fn is stripped, so its line is unresolved
  assert.equal(result.wired, 1);
  assert.equal(result.unresolved.length, 1);
  assert.match(result.unresolved[0].caller, /only_test$/);
});

test('router chain: call-expression args, identifier handlers, unknown-receiver methods all produce edges', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: [
        'fn main() { run(); }',
        'fn run() {',
        '  let mut server = Server::new();',
        '  server.try_serve(make_app().router());',
        '}',
      ].join('\n'),
      'native/hagency/src/app.rs': [
        'pub struct App;',
        'impl App {',
        '  pub fn router(self) -> Router { console::router() }',
        '}',
        'pub fn make_app() -> App { App }',
      ].join('\n'),
      'native/hagency/src/console.rs': [
        'pub fn router() -> Router {',
        '  Router::new().push(approvals::router())',
        '}',
        'pub mod approvals;',
      ].join('\n'),
      'native/hagency/src/console/approvals.rs': [
        'pub fn router() -> Router {',
        '  Router::with_path("grants/{id}").delete(revoke_grant)',
        '}',
        'async fn revoke_grant() {}',
      ].join('\n'),
    },
    specs: '  Production caller: hagency::console::approvals::revoke_grant\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok, JSON.stringify(result));
  assert.equal(result.wired, 1);
  assert.equal(result.missing.length, 0);
});

test('receiver typing: fn parameter type resolves recv.method to the impl method', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() { let c = get_collector(); drive(&c); }\nfn get_collector() -> Collector { Collector }\n',
      'native/hagency/src/driver.rs': 'use crate::Collector;\npub fn drive(collector: &Collector) { collector.intake(); }\n',
      'native/hagency/src/collector.rs': 'pub struct Collector;\nimpl Collector {\n  pub fn intake(&self) {}\n}\n',
    },
    specs: '  Production caller: hagency::collector::Collector::intake\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok, JSON.stringify(result));
  assert.equal(result.wired, 1);
});

test('receiver typing: struct field type via let x = self.field (cross-file struct)', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() { let c = get_collector(); c.serve(); }\nfn get_collector() -> Collector { Collector }\n',
      // The struct field (inner: Arc<Inner>) lives in a different file than
      // the impl method that clones it.
      'native/hagency/src/collector.rs': 'pub struct Collector {\n  pub inner: Arc<Inner>,\n}\npub struct Inner;\n',
      'native/hagency/src/intake.rs': 'use crate::collector::{Collector, Inner};\nimpl Collector {\n  pub fn serve(&self) { let inner = self.inner.clone(); inner.handoff(); }\n}\nimpl Inner {\n  pub fn handoff(&self) {}\n}\n',
    },
    specs: '  Production caller: hagency::intake::Inner::handoff\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok, JSON.stringify(result));
  assert.equal(result.wired, 1);
});

test('receiver typing: let annotation resolves recv.method', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() { let c: Collector = build(); c.intake(); }\nfn build() -> Collector { Collector }\n',
      'native/hagency/src/collector.rs': 'pub struct Collector;\nimpl Collector {\n  pub fn intake(&self) {}\n}\n',
    },
    specs: '  Production caller: hagency::collector::Collector::intake\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok, JSON.stringify(result));
  assert.equal(result.wired, 1);
});

test('receiver typing: Self inside impl resolves self.method to the impl method', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() { let i = get_inner(); i.start(); }\nfn get_inner() -> Inner { Inner }\n',
      'native/hagency/src/intake.rs': 'pub struct Inner;\nimpl Inner {\n  pub fn start(&self) { self.handoff(); }\n  fn handoff(&self) { self.provision(); }\n  fn provision(&self) {}\n}\n',
    },
    specs: '  Production caller: hagency::intake::Inner::provision\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok, JSON.stringify(result));
  assert.equal(result.wired, 1);
});

test('receiver typing: constructor result (let x = T::new()) resolves recv.method', () => {
  const { root, read, files } = makeFixture({
    rust: {
      [MAIN]: 'fn main() { run(); }\nfn run() { let c = Collector::new(); c.intake(); }\n',
      'native/hagency/src/collector.rs': 'pub struct Collector;\nimpl Collector {\n  pub fn new() -> Collector { Collector }\n  pub fn intake(&self) {}\n}\n',
    },
    specs: '  Production caller: hagency::collector::Collector::intake\n',
    adr: '| `x` | gap G1 |\n',
  });
  const { result, ok } = checkProductionCallers({ root, read, files });
  assert.ok(ok, JSON.stringify(result));
  assert.equal(result.wired, 1);
});
