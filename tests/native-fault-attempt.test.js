import { afterEach, expect, it } from 'vitest';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import {
  chownSync,
  chmodSync,
  existsSync,
  linkSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const modulePath = path.resolve('skills/hagency-inner-loop/scripts/prepare-native-fault-attempt.mjs');
const roots = [];
const sha256 = raw => createHash('sha256').update(raw).digest('hex');

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

async function preparer() {
  const api = existsSync(modulePath) ? await import(`${pathToFileURL(modulePath).href}?t=${Date.now()}`) : {};
  expect(typeof api.prepareFaultAttempt, 'prepareFaultAttempt(plan, testOptions?) must be exported').toBe('function');
  return api.prepareFaultAttempt;
}

function writeReadonlyJson(file, value) {
  const raw = Buffer.from(`${JSON.stringify(value)}\n`);
  writeFileSync(file, raw, { mode: 0o600 });
  chmodSync(file, 0o444);
  return { path: file, sha256: sha256(raw) };
}

function clone(value) {
  return structuredClone(value);
}

function fixture({ partialTrace = false } = {}) {
  const root = realpathSync(mkdtempSync(path.join(tmpdir(), 'hagency fault attempt with spaces-')));
  roots.push(root);
  const project = path.join(root, 'business project with spaces');
  const baseControl = path.join(root, 'private control with spaces');
  const lineage = path.join(root, 'readonly lineage with spaces');
  mkdirSync(project, { mode: 0o700 });
  mkdirSync(baseControl, { mode: 0o700 });
  mkdirSync(lineage, { mode: 0o700 });

  const objective = 'PRIVATE_OBJECTIVE_MUST_NEVER_APPEAR_IN_PUBLIC_OUTPUT';
  const goal = { goal_id: 'goal_01', created_at_ms: 1789223633767, objective_sha256: sha256(objective) };
  const observerPid = 5035;
  const trace = path.join(root, 'append only trace with spaces.jsonl');
  const traceLines = [
    JSON.stringify({ ts: '2026-09-12T16:20:00.000Z', direction: 'client_to_server', frame: { id: 'old-1' } }),
    JSON.stringify({ ts: '2026-09-12T16:20:00.010Z', direction: 'server_to_client', frame: { id: 'old-1' } }),
    JSON.stringify({ ts: '2026-09-12T16:27:02.500Z', direction: 'server_to_client', frame: { later_append: true } }),
  ];
  const bindingPrefix = Buffer.from(`${traceLines[0]}\n`);
  const safePrefix = Buffer.from(`${traceLines.slice(0, 2).join('\n')}\n`);
  const traceRaw = Buffer.from(traceLines.join('\n') + (partialTrace ? '' : '\n'));
  writeFileSync(trace, traceRaw, { mode: 0o600 });
  const traceStat = lstatSync(trace);
  const traceIdentity = [traceStat.dev, traceStat.ino];

  const paths = Object.fromEntries([
    'audit', 'safe_state', 'observer_binding', 'observer_claim', 'observer_ready', 'watch_manifest', 'watch_exit',
  ].map(name => [name, path.join(lineage, `${name.replaceAll('_', ' ')}.json`)]));
  const jobId = randomUUID();
  const values = {
    watch_manifest: { version: 1, job_id: jobId, manifest_path: paths.watch_manifest, immutable_deadline_at: 1789230247927 },
    observer_binding: {
      version: 1,
      stage: 'interrupt03',
      project,
      control: baseControl,
      trace,
      trace_identity: traceIdentity,
      trace_offset: bindingPrefix.length,
      trace_prefix_sha256: sha256(bindingPrefix),
      goal_id: goal.goal_id,
      goal_objective_sha256: goal.objective_sha256,
      private_detail: 'historical-only',
    },
    observer_claim: { armed_at: 1789229809.6917472, pid: observerPid, stage: 'interrupt03' },
    observer_ready: {
      armed_at: 1789229809.6917472,
      pid: observerPid,
      stage: 'interrupt03',
      trace_identity: traceIdentity,
      trace_prefix_sha256: sha256(bindingPrefix),
    },
    safe_state: {
      full_acceptance: false,
      goal_paused: true,
      loops_empty: true,
      all_turns_terminal: true,
      observer_only_armed_no_fault: true,
      stage34_absent: true,
      trace_identity: traceIdentity,
      trace_prefix_size: safePrefix.length,
      trace_prefix_sha256: sha256(safePrefix),
      owned_processes_present: { [observerPid]: null, 97842: null, 97850: null },
      observations: [{
        request: { frame: { method: 'session/goal/get', id: 'tui-72' } },
        response: { frame: { id: 'tui-72', result: { goal: {
          goal_id: goal.goal_id,
          created_at_ms: goal.created_at_ms,
          status: 'paused',
          objective,
        } } } },
      }],
    },
  };

  const pins = {};
  pins.watch_manifest = writeReadonlyJson(paths.watch_manifest, values.watch_manifest);
  values.watch_exit = {
    returncode: 3,
    manifest_unchanged: true,
    manifest_path: paths.watch_manifest,
    manifest_sha256: pins.watch_manifest.sha256,
    job_id: jobId,
  };
  pins.watch_exit = writeReadonlyJson(paths.watch_exit, values.watch_exit);
  values.audit = {
    scope: 'Q3_entire_actual_failed_watch_lifetime',
    old_watch_eligible_for_acceptance: false,
    full_acceptance: false,
    native_terminal_exit_code: 3,
    manifest_path: paths.watch_manifest,
    manifest_sha256: pins.watch_manifest.sha256,
    actual_exit_path: paths.watch_exit,
    actual_exit_sha256: pins.watch_exit.sha256,
  };
  for (const name of ['audit', 'safe_state', 'observer_binding', 'observer_claim', 'observer_ready']) {
    pins[name] = writeReadonlyJson(paths[name], values[name]);
  }

  const predecessors = Object.fromEntries([
    'audit', 'safe_state', 'observer_binding', 'observer_claim', 'observer_ready', 'watch_manifest', 'watch_exit',
  ].map(name => [name, clone(pins[name])]));
  const attemptId = randomUUID();
  const contractPath = path.join(lineage, 'recovery contract.json');
  let contract = {
    version: 1,
    attempt_id: attemptId,
    base_control: baseControl,
    project,
    goal: clone(goal),
    predecessor_full_acceptance: false,
    stages: ['interrupt03', 'restart04'],
    predecessors: clone(predecessors),
  };
  let contractPin = writeReadonlyJson(contractPath, contract);
  const plan = {
    version: 1,
    attempt_id: attemptId,
    base_control: baseControl,
    project,
    trace,
    recovery_contract: clone(contractPin),
    predecessors: clone(predecessors),
  };
  const attemptPath = path.join(baseControl, 'recovery-attempts', attemptId);

  const rewriteContract = mutate => {
    chmodSync(contractPath, 0o600);
    contract = clone(contract);
    mutate(contract);
    contractPin = writeReadonlyJson(contractPath, contract);
    plan.recovery_contract = clone(contractPin);
  };
  const rewritePredecessor = (name, mutate) => {
    chmodSync(paths[name], 0o600);
    values[name] = clone(values[name]);
    mutate(values[name]);
    pins[name] = writeReadonlyJson(paths[name], values[name]);
    plan.predecessors[name] = clone(pins[name]);
    rewriteContract(next => { next.predecessors[name] = clone(pins[name]); });
  };
  const snapshotLineage = () => Object.fromEntries([
    ...Object.values(paths), contractPath,
  ].map(file => [file, readFileSync(file)]));
  const writePlan = () => writeReadonlyJson(path.join(root, `plan ${randomUUID()}.json`), plan);

  return {
    root, project, baseControl, lineage, objective, goal, observerPid, trace, traceRaw,
    paths, values, pins, plan, attemptPath, contractPath, rewriteContract, rewritePredecessor,
    snapshotLineage, writePlan,
  };
}

function expectLineageUnchanged(before) {
  for (const [file, raw] of Object.entries(before)) expect(readFileSync(file), file).toEqual(raw);
}

it('prepares one readonly fault attempt with the original trace inode', async () => {
  const prepareFaultAttempt = await preparer();
  const f = fixture();
  const before = f.snapshotLineage();

  const report = await prepareFaultAttempt(f.plan);

  expect(report).toEqual({
    version: 1,
    status: 'prepared',
    full_acceptance: false,
    attempt_path: f.attemptPath,
    manifest_path: path.join(f.attemptPath, 'attempt.json'),
  });
  const traceLink = path.join(f.attemptPath, 'trace.jsonl');
  const sourceStat = lstatSync(f.trace);
  const linkStat = lstatSync(traceLink);
  expect([linkStat.dev, linkStat.ino]).toEqual([sourceStat.dev, sourceStat.ino]);
  expect(readFileSync(traceLink)).toEqual(f.traceRaw);
  expectLineageUnchanged(before);

  for (const name of ['attempt.json', 'prepared.json']) {
    expect(lstatSync(path.join(f.attemptPath, name)).mode & 0o777).toBe(0o444);
  }
  for (const directory of [path.dirname(f.attemptPath), f.attemptPath]) {
    const directoryStat = lstatSync(directory);
    expect(directoryStat.mode & 0o777).toBe(0o700);
    expect(directoryStat.uid).toBe(process.geteuid());
  }
  const manifest = JSON.parse(readFileSync(report.manifest_path, 'utf8'));
  const prepared = JSON.parse(readFileSync(path.join(f.attemptPath, 'prepared.json'), 'utf8'));
  expect(manifest).toMatchObject({
    version: 1,
    attempt_id: f.plan.attempt_id,
    status: 'claimed',
    full_acceptance: false,
    predecessor_full_acceptance: false,
    goal: f.goal,
    stages: ['interrupt03', 'restart04'],
    trace_fence: { source_path: f.trace, linked_path: traceLink, prefix_size: f.traceRaw.length, prefix_sha256: sha256(f.traceRaw) },
  });
  expect(manifest.predecessors).toEqual(f.plan.predecessors);
  expect(prepared).toMatchObject({
    version: 1,
    attempt_id: f.plan.attempt_id,
    status: 'prepared',
    full_acceptance: false,
    predecessor_full_acceptance: false,
    same_inode: true,
    trace_path: traceLink,
  });
  expect(JSON.stringify({ report, manifest, prepared })).not.toContain(f.objective);
  expect(manifest).not.toHaveProperty('live_ready');
  expect(prepared).not.toHaveProperty('live_ready', true);
});

it('rejects untrusted control directory ownership or mode before claiming', async () => {
  const prepareFaultAttempt = await preparer();

  const exposedBase = fixture();
  chmodSync(exposedBase.baseControl, 0o710);
  expect(await prepareFaultAttempt(exposedBase.plan)).toEqual({
    version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_control',
  });
  expect(readdirSync(exposedBase.baseControl)).toEqual([]);

  const exposedRecovery = fixture();
  const recoveryRoot = path.dirname(exposedRecovery.attemptPath);
  mkdirSync(recoveryRoot, { mode: 0o750 });
  expect(await prepareFaultAttempt(exposedRecovery.plan)).toEqual({
    version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_control',
  });
  expect(readdirSync(recoveryRoot)).toEqual([]);

  const unsafeAncestor = fixture();
  chmodSync(unsafeAncestor.root, 0o777);
  expect(await prepareFaultAttempt(unsafeAncestor.plan)).toEqual({
    version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_control',
  });
  expect(readdirSync(unsafeAncestor.baseControl)).toEqual([]);

  if (process.geteuid() === 0) {
    const foreignBase = fixture();
    const original = lstatSync(foreignBase.baseControl);
    chownSync(foreignBase.baseControl, original.uid + 1, original.gid);
    expect(await prepareFaultAttempt(foreignBase.plan)).toEqual({
      version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_control',
    });
  }
});

it('rejects base redirection before the first mutation without writing into the project', async () => {
  const prepareFaultAttempt = await preparer();
  const f = fixture();
  const displacedBase = path.join(f.root, 'displaced private control');
  const projectBefore = readdirSync(f.project);
  let hookCalled = false;

  const report = await prepareFaultAttempt(f.plan, {
    afterTraceOpen() {
      hookCalled = true;
      renameSync(f.baseControl, displacedBase);
      symlinkSync(f.project, f.baseControl);
    },
  });

  expect(hookCalled).toBe(true);
  expect(report).toEqual({ version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_control' });
  expect(report).not.toHaveProperty('attempt_path');
  expect(readdirSync(f.project)).toEqual(projectBefore);
  expect(existsSync(path.join(f.project, 'recovery-attempts'))).toBe(false);
  expect(readdirSync(displacedBase)).toEqual([]);
});

it('rejects replacement of an existing recovery root before claiming an attempt', async () => {
  const prepareFaultAttempt = await preparer();
  const f = fixture();
  const recoveryRoot = path.dirname(f.attemptPath);
  const displacedRoot = path.join(f.baseControl, 'displaced existing recovery root');
  mkdirSync(recoveryRoot, { mode: 0o700 });
  let hookCalled = false;

  const report = await prepareFaultAttempt(f.plan, {
    afterTraceOpen() {
      hookCalled = true;
      renameSync(recoveryRoot, displacedRoot);
      mkdirSync(recoveryRoot, { mode: 0o700 });
    },
  });

  expect(hookCalled).toBe(true);
  expect(report).toEqual({ version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_control' });
  expect(report).not.toHaveProperty('attempt_path');
  expect(readdirSync(recoveryRoot)).toEqual([]);
  expect(readdirSync(displacedRoot)).toEqual([]);
});

it('rejects a source pathname replaced after its descriptor is opened before claiming', async () => {
  const prepareFaultAttempt = await preparer();
  const f = fixture();
  const displaced = path.join(f.root, 'displaced original trace.jsonl');
  let hookCalled = false;

  const report = await prepareFaultAttempt(f.plan, {
    afterTraceOpen({ source }) {
      hookCalled = true;
      renameSync(source, displaced);
      writeFileSync(source, f.traceRaw, { mode: 0o600 });
    },
  });

  expect(hookCalled).toBe(true);
  expect(report).toEqual({ version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_trace' });
  expect(existsSync(f.attemptPath)).toBe(false);
  expect(readFileSync(displaced)).toEqual(f.traceRaw);
  expect(readFileSync(f.trace)).toEqual(f.traceRaw);
  expect(lstatSync(displaced).ino).not.toBe(lstatSync(f.trace).ino);
});

it('rejects missing mutable changed or inconsistent predecessor pins before claiming', async () => {
  const prepareFaultAttempt = await preparer();
  const cases = [
    f => { delete f.plan.predecessors.audit; },
    f => { chmodSync(f.paths.audit, 0o644); },
    f => { f.plan.predecessors.audit.sha256 = '0'.repeat(64); },
    f => f.rewritePredecessor('audit', value => { value.old_watch_eligible_for_acceptance = true; }),
    f => f.rewritePredecessor('audit', value => { value.full_acceptance = true; }),
    f => f.rewritePredecessor('audit', value => { value.native_terminal_exit_code = 0; }),
    f => f.rewritePredecessor('audit', value => { value.manifest_path = f.paths.watch_exit; }),
    f => f.rewritePredecessor('watch_exit', value => { value.returncode = 0; }),
    f => f.rewritePredecessor('watch_exit', value => { value.manifest_unchanged = false; }),
    f => f.rewritePredecessor('watch_manifest', value => { value.job_id = randomUUID(); }),
    f => f.rewritePredecessor('safe_state', value => { value.goal_paused = false; }),
    f => f.rewritePredecessor('safe_state', value => { value.owned_processes_present[String(f.observerPid)] = {}; }),
    f => f.rewritePredecessor('safe_state', value => { value.trace_prefix_size++; }),
    f => f.rewritePredecessor('safe_state', value => { value.trace_identity[1]++; }),
    f => f.rewritePredecessor('observer_binding', value => { value.goal_id = 'goal_changed'; }),
    f => f.rewritePredecessor('observer_binding', value => { value.goal_objective_sha256 = 'a'.repeat(64); }),
    f => f.rewritePredecessor('observer_binding', value => { value.trace = path.join(f.root, 'foreign.jsonl'); }),
    f => f.rewritePredecessor('observer_binding', value => { value.trace_offset++; }),
    f => f.rewritePredecessor('observer_claim', value => { value.stage = 'restart04'; }),
    f => f.rewritePredecessor('observer_ready', value => { value.pid++; }),
    f => f.rewritePredecessor('observer_ready', value => { value.trace_identity[0]++; }),
    f => f.rewriteContract(value => { value.goal.created_at_ms++; }),
    f => f.rewriteContract(value => { value.predecessor_full_acceptance = true; }),
    f => f.rewriteContract(value => { value.stages.reverse(); }),
    f => f.rewriteContract(value => { value.extra = true; }),
  ];

  for (const mutate of cases) {
    const f = fixture();
    mutate(f);
    const report = await prepareFaultAttempt(f.plan);
    expect(report.status, JSON.stringify(report)).toBe('failed');
    expect(report).toMatchObject({ version: 1, full_acceptance: false, reason: 'invalid_lineage' });
    expect(report).not.toHaveProperty('attempt_path');
    expect(existsSync(f.attemptPath)).toBe(false);
    expect(JSON.stringify(report)).not.toContain(f.objective);
    expect(readdirSync(f.baseControl)).toEqual([]);
  }
});

it('refuses symlinks partial records cross-device links and changed trace prefixes', async () => {
  const prepareFaultAttempt = await preparer();

  const linkedEvidence = fixture();
  const auditLink = path.join(linkedEvidence.lineage, 'audit symlink.json');
  symlinkSync(linkedEvidence.paths.audit, auditLink);
  linkedEvidence.plan.predecessors.audit.path = auditLink;
  linkedEvidence.rewriteContract(value => { value.predecessors.audit.path = auditLink; });
  expect((await prepareFaultAttempt(linkedEvidence.plan))).toMatchObject({ status: 'failed', reason: 'invalid_lineage' });
  expect(existsSync(linkedEvidence.attemptPath)).toBe(false);

  const linkedTrace = fixture();
  const traceLink = path.join(linkedTrace.root, 'trace symlink.jsonl');
  symlinkSync(linkedTrace.trace, traceLink);
  linkedTrace.plan.trace = traceLink;
  linkedTrace.rewritePredecessor('observer_binding', value => { value.trace = traceLink; });
  expect((await prepareFaultAttempt(linkedTrace.plan))).toMatchObject({ status: 'failed', reason: 'invalid_trace' });
  expect(existsSync(linkedTrace.attemptPath)).toBe(false);

  const partial = fixture({ partialTrace: true });
  expect((await prepareFaultAttempt(partial.plan))).toMatchObject({ status: 'failed', reason: 'invalid_trace' });
  expect(existsSync(partial.attemptPath)).toBe(false);

  const nested = fixture();
  nested.plan.project = nested.baseControl;
  expect((await prepareFaultAttempt(nested.plan))).toMatchObject({ status: 'failed', reason: 'invalid_plan' });
  expect(existsSync(nested.attemptPath)).toBe(false);
});

it('refuses a reused or colliding fault attempt without changing prior evidence', async () => {
  const prepareFaultAttempt = await preparer();
  const f = fixture();
  expect((await prepareFaultAttempt(f.plan)).status).toBe('prepared');
  const retained = Object.fromEntries(readdirSync(f.attemptPath).map(name => [name, readFileSync(path.join(f.attemptPath, name))]));

  const report = await prepareFaultAttempt(f.plan);

  expect(report).toEqual({
    version: 1,
    status: 'rejected',
    full_acceptance: false,
    reason: 'attempt_already_claimed',
    attempt_path: f.attemptPath,
  });
  expect(readdirSync(f.attemptPath).sort()).toEqual(Object.keys(retained).sort());
  for (const [name, raw] of Object.entries(retained)) expect(readFileSync(path.join(f.attemptPath, name))).toEqual(raw);
});

it('retains a claimed attempt on trace target collisions and EXDEV without copying', async () => {
  const prepareFaultAttempt = await preparer();
  for (const mode of ['collision', 'exdev']) {
    const f = fixture();
    const before = f.snapshotLineage();
    const report = await prepareFaultAttempt(f.plan, {
      link(source, target) {
        if (mode === 'collision') writeFileSync(target, 'FOREIGN TRACE TARGET\n', { flag: 'wx' });
        const error = new Error(mode);
        error.code = mode === 'exdev' ? 'EXDEV' : 'EEXIST';
        throw error;
      },
    });

    expect(report).toEqual({
      version: 1,
      status: 'failed',
      full_acceptance: false,
      reason: 'trace_link_failed',
      attempt_path: f.attemptPath,
      manifest_path: path.join(f.attemptPath, 'attempt.json'),
    });
    expect(existsSync(f.attemptPath)).toBe(true);
    expect(existsSync(path.join(f.attemptPath, 'attempt.json'))).toBe(true);
    expect(existsSync(path.join(f.attemptPath, 'prepared.json'))).toBe(false);
    if (mode === 'collision') expect(readFileSync(path.join(f.attemptPath, 'trace.jsonl'), 'utf8')).toBe('FOREIGN TRACE TARGET\n');
    else expect(existsSync(path.join(f.attemptPath, 'trace.jsonl'))).toBe(false);
    expectLineageUnchanged(before);
    expect(readFileSync(f.trace)).toEqual(f.traceRaw);
  }
});

it('detects a changed trace prefix after linking and permanently retains the claim', async () => {
  const prepareFaultAttempt = await preparer();
  const f = fixture();
  const report = await prepareFaultAttempt(f.plan, {
    link: linkSync,
    afterLink({ source }) {
      const changed = Buffer.from(readFileSync(source));
      changed[0] = changed[0] === 0x7b ? 0x5b : 0x7b;
      writeFileSync(source, changed);
    },
  });

  expect(report).toEqual({
    version: 1,
    status: 'failed',
    full_acceptance: false,
    reason: 'trace_changed',
    attempt_path: f.attemptPath,
    manifest_path: path.join(f.attemptPath, 'attempt.json'),
  });
  expect(existsSync(path.join(f.attemptPath, 'trace.jsonl'))).toBe(true);
  expect(existsSync(path.join(f.attemptPath, 'prepared.json'))).toBe(false);
  expect((await prepareFaultAttempt(f.plan)).status).toBe('rejected');
});

it('fails when the claimed attempt parent is replaced during linking', async () => {
  const prepareFaultAttempt = await preparer();
  const f = fixture();
  const recoveryRoot = path.dirname(f.attemptPath);
  const displacedRoot = path.join(f.baseControl, 'displaced recovery attempts');

  const report = await prepareFaultAttempt(f.plan, {
    link(source, target) {
      renameSync(recoveryRoot, displacedRoot);
      mkdirSync(recoveryRoot, { mode: 0o700 });
      mkdirSync(path.dirname(target), { mode: 0o700 });
      linkSync(source, target);
    },
  });

  expect(report).toEqual({
    version: 1,
    status: 'failed',
    full_acceptance: false,
    reason: 'control_changed',
    attempt_path: f.attemptPath,
    manifest_path: path.join(f.attemptPath, 'attempt.json'),
  });
  expect(existsSync(path.join(displacedRoot, f.plan.attempt_id, 'attempt.json'))).toBe(true);
  expect(existsSync(path.join(recoveryRoot, f.plan.attempt_id, 'prepared.json'))).toBe(false);
});

it('fault attempt CLI rejects changed plans and never claims live readiness', async () => {
  await preparer();

  for (const mode of ['changed', 'writable', 'extra']) {
    const f = fixture();
    const pin = f.writePlan();
    if (mode === 'changed') {
      chmodSync(pin.path, 0o600);
      writeFileSync(pin.path, `${JSON.stringify({ ...f.plan, attempt_id: randomUUID() })}\n`);
      chmodSync(pin.path, 0o444);
    }
    if (mode === 'writable') chmodSync(pin.path, 0o644);
    const args = [modulePath, '--plan', pin.path, '--sha256', pin.sha256];
    if (mode === 'extra') args.push('--force');
    const result = spawnSync(process.execPath, args, { encoding: 'utf8', timeout: 10000 });
    expect(result.status, result.stderr).toBe(1);
    expect(JSON.parse(result.stdout)).toEqual({ version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_plan' });
    expect(result.stderr).toBe('');
    expect(existsSync(f.attemptPath)).toBe(false);
    expect(result.stdout).not.toContain(f.objective);
  }

  const f = fixture();
  const pin = f.writePlan();
  const result = spawnSync(process.execPath, [modulePath, '--plan', pin.path, '--sha256', pin.sha256], {
    encoding: 'utf8', timeout: 10000,
  });
  expect(result.status, result.stderr).toBe(0);
  expect(JSON.parse(result.stdout)).toEqual({
    version: 1,
    status: 'prepared',
    full_acceptance: false,
    attempt_path: f.attemptPath,
    manifest_path: path.join(f.attemptPath, 'attempt.json'),
  });
  expect(result.stderr).toBe('');
  expect(result.stdout).not.toContain(f.objective);
});
