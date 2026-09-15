import { afterEach, expect, it, vi } from 'vitest';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const modulePath = path.resolve('skills/hagency-inner-loop/scripts/run-stage-release.mjs');
const publicationFault = vi.hoisted(() => ({ target: null, delayed: 0 }));
vi.mock('../skills/hagency-inner-loop/scripts/native-control-evidence.mjs', async importOriginal => {
  const actual = await importOriginal();
  return { ...actual, publishExclusiveJson(file, value) {
    const result = actual.publishExclusiveJson(file, value);
    const release = /\/stage0[34]-release\.json$/.test(file);
    const mutationInput = /\/child-input-/.test(file) && ['goal-resume', 'loop-resume'].includes(value.operation);
    if (publicationFault.delayed === 0 && ((publicationFault.target === 'release' && release)
      || (publicationFault.target === 'mutation-input' && mutationInput))) {
      publicationFault.delayed += 1;
      // Model a blocking filesystem flush returning after the observation ages.
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 2100);
    }
    return result;
  } };
});
const fixtures = [];
const fixture = async stage => {
  const { stageFixture } = await import('./fixtures/native-stage-fixture.mjs');
  const f = await stageFixture(stage); fixtures.push(f); return f;
};
afterEach(async () => {
  publicationFault.target = null; publicationFault.delayed = 0;
  await Promise.all(fixtures.splice(0).map(f => f.dispose()));
});

async function coordinator() {
  expect(existsSync(modulePath), 'the reviewed stage coordinator must be distributed').toBe(true);
  const api = await import(pathToFileURL(modulePath).href);
  expect(typeof api.runStageRelease).toBe('function');
  return api.runStageRelease;
}

const json = file => JSON.parse(readFileSync(file, 'utf8'));
const events = f => readFileSync(path.join(f.root, 'stage-events.jsonl'), 'utf8')
  .split('\n').filter(Boolean).map(line => JSON.parse(line));
const mutations = f => f.sent().filter(command => command === '/goal resume' || command.startsWith('/loop resume '));

it('releases03 after ready live identity and protected checks before one goal resume', async () => {
  const f = await fixture('interrupt03');
  const inputPath = path.join(f.root, 'stage-cli-input.json');
  const bytes = JSON.stringify(f.plan);
  writeFileSync(inputPath, bytes, { mode: 0o400 });
  const child = spawnSync(process.execPath, [modulePath, '--plan', inputPath,
    '--sha256', createHash('sha256').update(bytes).digest('hex')], {
    encoding: 'utf8', timeout: f.plan.activation_timeout_ms,
  });
  expect(child.status, child.stdout + child.stderr).toBe(0);
  expect(child.signal).toBeNull();
  expect(child.stderr).toBe('');
  const result = JSON.parse(child.stdout);
  expect(result, JSON.stringify(result)).toMatchObject({ status: 'activated', stage: 'interrupt03', full_acceptance: false });
  expect(mutations(f)).toEqual(['/goal resume']);
  expect(json(f.release)).toMatchObject({ stage: 'interrupt03', activation_id: f.plan.activation_id });
  expect(events(f)).toMatchObject([{ command: '/goal resume', release_exists: true, intent_exists: true, terminal_turns: true }]);
  expect(f.readState().goal.status).toBe('active');
  expect(json(result.evidence_path).children.map(child => child.operation)).toEqual(['inspect', 'goal-resume', 'inspect']);
  expect(JSON.stringify(result)).not.toContain(f.readState().goal.objective);
});

it.each(['absent ready', 'changed birth', 'changed argv', 'dead observer'])(
  'rejects absent stale foreign dead or changed observer proof before release: %s', async mode => {
    const runStageRelease = await coordinator(), f = await fixture('interrupt03');
    if (mode === 'absent ready') f.plan.observer_ready.path += '.absent';
    if (mode === 'changed birth') f.update(s => { s.rows.find(row => row.pid === f.plan.observer.pid).started = '1789220001.000031'; });
    if (mode === 'changed argv') f.update(s => { s.observerMetadata.argv.push('foreign'); });
    if (mode === 'dead observer') f.update(s => { s.rows = s.rows.filter(row => row.pid !== f.plan.observer.pid); });
    const result = await runStageRelease(f.plan);
    expect(result.status, mode).toBe('failed');
    expect(existsSync(f.release)).toBe(false);
    expect(mutations(f)).toEqual([]);
  });

it('retains collisions and refuses replay under different activation IDs', async () => {
  const runStageRelease = await coordinator(), f = await fixture('interrupt03');
  const first = await runStageRelease(f.plan);
  expect(first.status, JSON.stringify(first)).toBe('activated');
  const oldRelease = readFileSync(f.release), oldResult = readFileSync(first.evidence_path);
  const again = structuredClone(f.plan);
  again.activation_id = randomUUID();
  for (const key of Object.keys(again.child_operations)) again.child_operations[key] = randomUUID();
  expect((await runStageRelease(again)).status).toBe('rejected');
  expect(readFileSync(f.release)).toEqual(oldRelease);
  expect(readFileSync(first.evidence_path)).toEqual(oldResult);
  expect(mutations(f)).toEqual(['/goal resume']);
});

it('retains a foreign release collision without sending a native mutation', async () => {
  const runStageRelease = await coordinator(), f = await fixture('interrupt03');
  writeFileSync(f.release, 'foreign immutable release\n', { mode: 0o444 });
  const result = await runStageRelease(f.plan);
  expect(['failed', 'rejected']).toContain(result.status);
  expect(readFileSync(f.release, 'utf8')).toBe('foreign immutable release\n');
  expect(mutations(f)).toEqual([]);
});

it('preserves release and intent after a failed or uncertain goal resume', async () => {
  const runStageRelease = await coordinator(), f = await fixture('interrupt03');
  f.update(s => { s.mode = 'missing_mutation_response'; });
  const result = await runStageRelease(f.plan);
  expect(result.status, JSON.stringify(result)).toBe('outcome_unknown');
  expect(mutations(f)).toEqual(['/goal resume']);
  expect(existsSync(f.release)).toBe(true);
  expect(existsSync(path.join(f.activation, 'intent.json'))).toBe(true);
  const children = json(result.evidence_path).children;
  expect(children.map(child => child.operation)).toEqual(['inspect', 'goal-resume']);
  expect(children.at(-1).report.status).toBe('outcome_unknown');
  expect((await runStageRelease(f.plan)).status).toBe('rejected');
  expect(mutations(f)).toHaveLength(1);
});

it('releases04 resumes the original loop settles its audit and resumes the original goal', async () => {
  const runStageRelease = await coordinator(), f = await fixture('restart04');
  const result = await runStageRelease(f.plan);
  expect(result, JSON.stringify(result)).toMatchObject({ status: 'activated', stage: 'restart04', full_acceptance: false });
  expect(mutations(f)).toEqual([`/loop resume ${f.plan.expected_loop.loop_id}`, '/goal resume']);
  expect(events(f)).toMatchObject([
    { command: `/loop resume ${f.plan.expected_loop.loop_id}`, release_exists: true, intent_exists: true, terminal_turns: true },
    { command: '/goal resume', release_exists: true, intent_exists: true, terminal_turns: true, loop_status: 'active' },
  ]);
  const childOperations = json(result.evidence_path).children.map(child => child.operation);
  expect(childOperations.slice(0, 2)).toEqual(['inspect', 'loop-resume']);
  expect(childOperations.slice(2, -2).length).toBeGreaterThanOrEqual(2);
  expect(childOperations.slice(2, -2).every(operation => operation === 'inspect')).toBe(true);
  expect(childOperations.slice(-2)).toEqual(['goal-resume', 'inspect']);
  expect(f.readState().goal.status).toBe('active');
  expect(f.readState().loops).toHaveLength(1);
  expect(f.readState().loops[0]).toMatchObject({ loop_id: f.plan.expected_loop.loop_id, status: 'active' });
});

it.each(['missing_loop_response', 'never_settles', 'goal_preflight_failure', 'final_inspect_failure'])(
  'stops after loop or goal uncertainty and preserves the consumed04 activation: %s', async mode => {
    const runStageRelease = await coordinator(), f = await fixture('restart04');
    f.update(s => { s.mode = mode; });
    if (mode === 'never_settles') {
      f.plan.child_operations.settlement_inspects = f.plan.child_operations.settlement_inspects.slice(0, 1);
    }
    const result = await runStageRelease(f.plan);
    expect(result.status, JSON.stringify(result)).toBe('outcome_unknown');
    const expected = [`/loop resume ${f.plan.expected_loop.loop_id}`];
    if (mode === 'final_inspect_failure') expected.push('/goal resume');
    expect(mutations(f)).toEqual(expected);
    if (mode === 'never_settles') {
      const children = json(result.evidence_path).children;
      const observed = children.at(-1);
      expect(observed).toMatchObject({ operation: 'inspect', exit_code: 0, signal: null, report: { status: 'observed' } });
      const hydrate = json(observed.report.evidence_path).observations.find(row => row.request.frame.method === 'session/hydrate');
      expect(hydrate.response.frame.result.turns.some(turn => turn.state === 'active')).toBe(true);
    }
    expect(existsSync(f.release)).toBe(true);
    expect((await runStageRelease(f.plan)).status).toBe('rejected');
    expect(mutations(f)).toEqual(expected);
  });

it.each([
  ['interrupt03', 'release'], ['interrupt03', 'mutation-input'],
  ['restart04', 'release'], ['restart04', 'mutation-input'],
])('retains consumed %s without dispatch when %s publication outlives observer freshness', async (stage, target) => {
  const runStageRelease = await coordinator(), f = await fixture(stage);
  publicationFault.target = target;
  const result = await runStageRelease(f.plan);
  expect(publicationFault.delayed).toBe(1);
  expect(result, JSON.stringify(result)).toMatchObject({ status: 'outcome_unknown', reason: 'observer_not_fresh' });
  expect(existsSync(f.release)).toBe(true);
  expect(mutations(f)).toEqual([]);
  expect((await runStageRelease(f.plan)).status).toBe('rejected');
});

it.each(['attempt', 'trace', 'helper', 'protected'])(
  'rejects changed attempt trace helper or prior increment evidence before release: %s', async mode => {
    const runStageRelease = await coordinator(), f = await fixture('interrupt03');
    const file = mode === 'attempt' ? f.plan.attempt_manifest.path
      : mode === 'trace' ? f.trace
        : mode === 'helper' ? f.plan.frozen_helpers.observer.path
          : path.join(f.plan.binding.project, Object.keys(f.plan.protected)[0]);
    const permissions = mode === 'trace' ? 0o600 : 0o444;
    chmodSync(file, 0o600); writeFileSync(file, '{}\n'); chmodSync(file, permissions);
    const result = await runStageRelease(f.plan);
    expect(result.status).toBe('failed');
    expect(existsSync(f.release)).toBe(false);
    expect(mutations(f)).toEqual([]);
  });


it('rejects invalid CLI forms with bounded JSON and a failing process exit', () => {
  for (const args of [[], ['--plan'], ['--force'], ['--plan', '/absent', '--sha256', 'invalid']]) {
    const result = spawnSync(process.execPath, [modulePath, ...args], { encoding: 'utf8', timeout: 3000 });
    expect(result.status, JSON.stringify(args)).toBe(1);
    expect(result.stderr).toBe('');
    expect(JSON.parse(result.stdout)).toMatchObject({ status: 'failed', full_acceptance: false });
  }
});
