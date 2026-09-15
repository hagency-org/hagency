import { afterEach, expect, it } from 'vitest';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const modulePath = path.resolve('skills/hagency-inner-loop/scripts/native-control.mjs');
const childFixture = pathToFileURL(path.resolve('tests/fixtures/fake-herdr-native-control.mjs')).href;
const roots = [];
const sha = raw => createHash('sha256').update(raw).digest('hex');
const clone = value => structuredClone(value);
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }); });

async function controller() {
  expect(existsSync(modulePath), 'the native control API must be distributed').toBe(true);
  const api = await import(pathToFileURL(modulePath).href);
  expect(typeof api.runControlPlan).toBe('function');
  expect(typeof api.queryNative).toBe('function');
  return api;
}

function fixture(mode = 'normal') {
  const root = realpathSync(mkdtempSync(path.join(tmpdir(), 'hagency-native-control-'))); roots.push(root);
  const project = path.join(root, 'project with spaces'); mkdirSync(project);
  const evidence = path.join(root, 'evidence'); mkdirSync(evidence, { mode: 0o700 });
  const trace = path.join(root, 'trace.jsonl');
  writeFileSync(trace, JSON.stringify({ ts: new Date().toISOString(), direction: 'server_to_client', frame: { seed: true } }) + '\n');
  const calls = path.join(root, 'calls.jsonl'); writeFileSync(calls, '');
  const statePath = path.join(root, 'state.json');
  const frontend = path.join(root, 'pinned frontend'); writeFileSync(frontend, 'pinned frontend\n');
  const row = (pid, ppid) => ({ pid, ppid, pgid: 22, started: `1789220000.${String(pid).padStart(6, '0')}`, state: 'S' });
  const rows = [row(21, 11), row(22, 21), row(23, 22)];
  const identities = {
    frontend: { ...rows[1], cwd: project, argv: [frontend, '--profile-id', 'owned', '--cwd', project,
      '--stdio-command', `octos serve --cwd ${project} --solo`] },
    backend: { ...rows[2], cwd: project, argv: ['octos', 'serve', '--stdio', '--solo', '--cwd', project] },
  };
  for (const role of Object.values(identities)) delete role.state;
  const state = {
    mode, nextId: 0, project, trace, calls, herdrSession: 'owned-session', agent: 'owned-agent',
    paneId: 'w1:pC', nativeSession: 'owned:local:tui#coding', profile: 'owned',
    secret: 'PRIVATE_OBJECTIVE_AND_STDERR_MARKER',
    goal: { profile_id: 'owned', goal_id: 'goal_01', created_at_ms: 1789223633767, objective: 'PRIVATE_OBJECTIVE_AND_STDERR_MARKER', status: 'paused' },
    turns: [{ turn_id: '01a09639-d58e-78e3-a74b-efb7d2ef7432', session_id: 'owned:local:tui#coding', state: 'completed' }],
    rows, loops: [],
    pane: { pane_id: 'w1:pC', shell_pid: 21, foreground_process_group_id: 22,
      foreground_processes: Object.values(identities).map(p => ({ pid: p.pid, cwd: p.cwd, argv: p.argv })) },
    agentInfo: { name: 'owned-agent', agent: 'octoscode', pane_id: 'w1:pC', terminal_id: 'term_owned',
      cwd: project, foreground_cwd: project, agent_status: 'idle' },
  };
  writeFileSync(statePath, JSON.stringify(state));
  const herdr = path.join(root, 'fake herdr');
  writeFileSync(herdr, `#!${process.execPath}\nimport { main } from ${JSON.stringify(childFixture)};\nmain(${JSON.stringify(statePath)}, process.argv.slice(2));\n`, { mode: 0o755 });
  const birth = path.join(root, 'fake births');
  writeFileSync(birth, `#!${process.execPath}\nimport {readFileSync} from 'node:fs';\nconsole.log(JSON.stringify({version:1,processes:JSON.parse(readFileSync(${JSON.stringify(statePath)},'utf8')).rows}));\n`, { mode: 0o755 });
  const binding = {
    version: 1, project, trace, herdr: { path: herdr, sha256: sha(readFileSync(herdr)) },
    birth_tool: { path: birth, sha256: sha(readFileSync(birth)) },
    frontend_binary: { path: frontend, sha256: sha(readFileSync(frontend)) },
    herdr_session: state.herdrSession, lower_agent: state.agent, pane_id: state.paneId,
    terminal_id: 'term_owned', shell_pid: 21, profile: 'owned', native_session: state.nativeSession,
    frontend: identities.frontend, backend: identities.backend,
  };
  const plan = { version: 1, operation_id: randomUUID(), operation: 'inspect', binding,
    expected_goal: { goal_id: state.goal.goal_id, created_at_ms: state.goal.created_at_ms, objective_sha256: sha(state.goal.objective) },
    // Each exchange includes eight real Node launches; allow the supported
    // maximum so host scheduling does not replace the expected domain failure.
    evidence_dir: evidence, query_timeout_ms: 10000 };
  const readState = () => JSON.parse(readFileSync(statePath, 'utf8'));
  const update = fn => { const s = readState(); fn(s); writeFileSync(statePath, JSON.stringify(s)); };
  const sent = () => readFileSync(calls, 'utf8').split('\n').filter(Boolean).map(x => JSON.parse(x).command);
  const writePlan = value => {
    const file = path.join(root, `plan-${randomUUID()}.json`); writeFileSync(file, JSON.stringify(value)); chmodSync(file, 0o444);
    return { file, digest: sha(readFileSync(file)) };
  };
  return { root, trace, evidence, statePath, state, binding, plan, readState, update, sent, writePlan };
}

it('correlates an immediate response emitted before prompt delivery returns', async () => {
  const { runControlPlan } = await controller(); const f = fixture();
  const report = await runControlPlan(f.plan);
  expect(report, JSON.stringify(report)).toMatchObject({ status: 'observed', operation: 'inspect', full_acceptance: false, goal_id: 'goal_01' });
  expect(f.sent()).toEqual(['/goal --report', '/loop list --report', '/turn session']);
  const evidence = JSON.parse(readFileSync(report.evidence_path, 'utf8'));
  expect(evidence.observations).toHaveLength(3);
  for (const o of evidence.observations) {
    expect(o.request.frame.id).toBe(o.response.frame.id);
    expect(o.fence.offset).toBeGreaterThan(0);
  }
  expect(JSON.stringify(report)).not.toContain(f.state.secret);
});

it('rejects changed trace history wrong scope duplicate requests and mismatched responses', async () => {
  const { runControlPlan } = await controller();
  for (const mode of ['rewrite_prefix', 'replace_trace', 'wrong_scope', 'duplicate_request', 'wrong_id', 'rpc_error']) {
    // The budget includes eight real Node child launches for identity checks.
    // Keep enough time to reach delivery; assert it was actually sent below.
    const f = fixture(mode);
    const report = await runControlPlan(f.plan);
    expect(report.status, mode).toBe('failed');
    expect(report.full_acceptance).toBe(false);
    expect(f.sent(), mode).toEqual(['/goal --report']);
    expect(JSON.stringify(report)).not.toContain(f.state.secret);
  }
});

it('pauses and resumes only the original goal and retains correlated evidence', async () => {
  const { runControlPlan } = await controller();
  for (const operation of ['goal-pause', 'goal-resume']) {
    const f = fixture(); f.plan.operation = operation;
    if (operation === 'goal-pause') f.update(s => { s.goal.status = 'active'; s.turns[0].state = 'active'; s.agentInfo.agent_status = 'running'; });
    const report = await runControlPlan(f.plan);
    expect(report).toMatchObject({ status: 'applied', goal_id: 'goal_01', full_acceptance: false });
    expect(report.goal_status).toBe(operation === 'goal-resume' ? 'active' : 'paused');
    expect(f.readState().goal.created_at_ms).toBe(f.state.goal.created_at_ms);
    expect(f.sent().filter(x => x === '/goal resume' || x === '/goal pause')).toHaveLength(1);
    expect(readdirSync(path.dirname(report.evidence_path))).toContain('intent.json');
    const evidence = JSON.parse(readFileSync(report.evidence_path, 'utf8'));
    const action = evidence.observations.at(-1);
    expect(action.request.frame.method).toBe('session/goal/set');
    expect(action.request.frame.id).toBe(action.response.frame.id);
    expect(report).not.toHaveProperty('session_idle', true);
  }
});

it.each([
  ['missing goal', s => { s.goal = null; }],
  ['foreign goal', s => { s.goal.goal_id = 'different'; }],
  ['changed objective', s => { s.goal.objective += ' changed'; }],
  ['changed creation', s => { s.goal.created_at_ms++; }],
  ['missing turns', s => { s.turns = null; }],
  ['unknown turn', s => { s.turns = [{ turn_id: 'unknown', state: 'mystery' }]; }],
  ['duplicate turn', s => { s.turns.push(clone(s.turns[0])); }],
  ['active turn', s => { s.turns[0].state = 'active'; }],
  ['busy Herdr', s => { s.agentInfo.agent_status = 'running'; }],
])('rejects missing goals invalid turns changed goals and active work before resume: %s', async (_label, mutate) => {
  const { runControlPlan } = await controller();
  const f = fixture(); f.plan.operation = 'goal-resume'; f.update(mutate);
  expect((await runControlPlan(f.plan)).status).toBe('failed');
  expect(f.sent()).not.toContain('/goal resume');
});

it('preserves mutation intent and rejects reuse after delivery timeout', async () => {
  const { runControlPlan } = await controller(); const f = fixture('missing_mutation_response');
  f.plan.operation = 'goal-resume';
  const first = await runControlPlan(f.plan);
  expect(first).toMatchObject({ status: 'outcome_unknown', full_acceptance: false });
  const failedEvidence = JSON.parse(readFileSync(first.evidence_path, 'utf8'));
  expect(failedEvidence.failed_exchange).toBeDefined();
  expect(failedEvidence.failed_exchange.fence.offset).toBeGreaterThan(0);
  expect(failedEvidence.failed_exchange.records.filter(r => r.frame.method === 'session/goal/set')).toHaveLength(1);
  const get = failedEvidence.failed_exchange.records.find(r => r.frame.method === 'session/goal/get');
  expect(failedEvidence.failed_exchange.records.some(r => r.direction === 'server_to_client' && r.frame.id === get.frame.id)).toBe(true);
  const intentPath = path.join(path.dirname(first.evidence_path), 'intent.json');
  const before = readFileSync(intentPath);
  const second = await runControlPlan(f.plan);
  expect(second.status).toBe('rejected');
  expect(f.sent().filter(c => c === '/goal resume')).toHaveLength(1);
  expect(readFileSync(intentPath)).toEqual(before);
  expect(f.readState().goal.status).toBe('active');
});

it('reports changed goal identity after a control as unknown without rollback', async () => {
  const { runControlPlan } = await controller(); const f = fixture('changed_goal_after'); f.plan.operation = 'goal-resume';
  expect((await runControlPlan(f.plan)).status).toBe('outcome_unknown');
  expect(f.sent().filter(c => c === '/goal resume' || c === '/goal pause')).toEqual(['/goal resume']);
});

it('rejects lost process proof before delivery and a replaced process after delivery', async () => {
  const { runControlPlan } = await controller();
  for (const mutate of [s => { s.rows[1].started = '1789220001.000022'; },
    s => { s.pane.foreground_processes[0].argv = s.pane.foreground_processes[0].argv.join(' ').split(' '); },
    s => { delete s.pane.foreground_processes[0].argv; },
    s => { s.agentInfo.terminal_id = 'foreign'; }]) {
    const f = fixture(); f.update(mutate);
    expect((await runControlPlan(f.plan)).status).toBe('failed');
    expect(f.sent()).toEqual([]);
  }
  const f = fixture('replaced_process_after');
  expect((await runControlPlan(f.plan)).status).toBe('failed');
  expect(f.sent()).toEqual(['/goal --report']);
});

it('keeps control evidence outside the business project', async () => {
  const { runControlPlan } = await controller(); const f = fixture();
  const unsafe = path.join(f.binding.project, 'control-evidence'); mkdirSync(unsafe, { mode: 0o700 });
  f.plan.evidence_dir = unsafe;
  expect((await runControlPlan(f.plan)).status).toBe('failed');
  expect(readdirSync(unsafe)).toEqual([]); expect(f.sent()).toEqual([]);
});

it('reports a goal resumed by another actor during control as unknown', async () => {
  const { runControlPlan } = await controller(); const f = fixture('external_resume'); f.plan.operation = 'goal-resume';
  expect((await runControlPlan(f.plan)).status).toBe('outcome_unknown');
  expect(f.sent().filter(c => c === '/goal resume' || c === '/goal pause')).toEqual(['/goal resume']);
});

it('rejects missing and foreign nested goal scope', async () => {
  const { runControlPlan } = await controller();
  for (const mutate of [s => { delete s.goal.profile_id; }, s => { s.goal.profile_id = 'foreign'; },
    s => { s.goal.session_id = 'foreign:local:tui#coding'; }]) {
    const f = fixture(); f.update(mutate);
    expect((await runControlPlan(f.plan)).status).toBe('failed');
    expect(f.sent()).toEqual(['/goal --report']);
  }
});

it('CLI pins readonly plans and returns bounded outcomes without private data', async () => {
  await controller();
  for (const failure of ['digest', 'writable', 'delivery']) {
    const f = fixture(failure === 'delivery' ? 'delivery_error' : 'normal'); const p = f.writePlan(f.plan);
    if (failure === 'writable') chmodSync(p.file, 0o644);
    const result = spawnSync(process.execPath, [modulePath, '--plan', p.file, '--sha256', failure === 'digest' ? 'a'.repeat(64) : p.digest], { encoding: 'utf8', timeout: 40000 });
    expect(result.status).toBe(1);
    const report = JSON.parse(result.stdout.trim()); expect(report.full_acceptance).toBe(false);
    expect(result.stdout + result.stderr).not.toContain(f.state.secret);
    if (failure !== 'delivery') expect(f.sent()).toEqual([]);
  }
  const f = fixture(); const p = f.writePlan(f.plan);
  const result = spawnSync(process.execPath, [modulePath, '--plan', p.file, '--sha256', p.digest], { encoding: 'utf8', timeout: 40000 });
  expect(result.status).toBe(0); expect(JSON.parse(result.stdout).status).toBe('observed');
  expect(result.stderr).toBe('');
});

const LOOP_PROMPT = 'PRIVATE_LOOP_PROMPT_MARKER: audit current tests without implementation';
function configureLoop(f, operation) {
  f.plan.operation = operation;
  const template = { prompt: LOOP_PROMPT, prompt_sha256: sha(LOOP_PROMPT), mode: 'fixed_interval', interval_seconds: 60 };
  if (operation === 'loop-create') f.plan.loop_template = template;
  else {
    const loop = { session_id: f.state.nativeSession, profile_id: f.state.profile, loop_id: 'loop_17',
      prompt: LOOP_PROMPT, mode: 'fixed_interval', interval_seconds: 60, status: 'paused',
      created_at_ms: 1789224000000, updated_at_ms: 1789224000000, next_run_at_ms: 1789224060000,
      last_run_at_ms: null, expires_at_ms: 1789828800000 };
    f.update(s => { s.loops = [loop]; });
    f.plan.expected_loop = { loop_id: loop.loop_id, created_at_ms: loop.created_at_ms,
      prompt_sha256: template.prompt_sha256, mode: loop.mode, interval_seconds: loop.interval_seconds };
  }
  return f;
}
const loopMutations = f => f.sent().filter(c => /^\/loop (every|pause|resume|delete) /.test(c));

const idleOperations = ['goal-resume', 'loop-create', 'loop-resume', 'loop-delete'];
function idleControlFixture(operation) {
  const f = fixture();
  if (operation.startsWith('loop-')) configureLoop(f, operation);
  else f.plan.operation = operation;
  return f;
}
const idleMutations = f => f.sent().filter(c => c === '/goal resume' || /^\/loop (every|resume|delete) /.test(c));

it.each(idleOperations)('accepts unseen Herdr done only with terminal native turns: %s', async operation => {
  const { runControlPlan } = await controller(); const f = idleControlFixture(operation);
  f.update(s => { s.agentInfo.agent_status = 'done'; });
  const report = await runControlPlan(f.plan);
  expect(report, JSON.stringify(report)).toMatchObject({ status: 'applied', operation, full_acceptance: false });
  expect(idleMutations(f)).toHaveLength(1);
  const evidence = JSON.parse(readFileSync(report.evidence_path, 'utf8'));
  const hydrated = evidence.observations.find(o => o.request.frame.method === 'session/hydrate');
  expect(hydrated.response.frame.result.turns).toEqual(f.state.turns);
  expect(report).not.toHaveProperty('session_idle', true);
});

it.each(idleOperations)('rejects active native turns even when Herdr reports done: %s', async operation => {
  const { runControlPlan } = await controller(); const f = idleControlFixture(operation);
  f.update(s => { s.agentInfo.agent_status = 'done'; s.turns[0].state = 'active'; });
  expect(await runControlPlan(f.plan)).toMatchObject({ status: 'failed', full_acceptance: false });
  expect(idleMutations(f)).toEqual([]);
});

it.each(['working', 'blocked', 'unknown', undefined])('rejects non-idle Herdr display states before native controls: %s', async status => {
  const { runControlPlan } = await controller(); const f = idleControlFixture('goal-resume');
  f.update(s => { s.agentInfo.agent_status = status; });
  expect(await runControlPlan(f.plan)).toMatchObject({ status: 'failed', full_acceptance: false });
  expect(idleMutations(f)).toEqual([]);
});

it('creates one fixed interval loop and verifies the fresh scoped list', async () => {
  const { runControlPlan } = await controller(); const f = configureLoop(fixture(), 'loop-create');
  const report = await runControlPlan(f.plan);
  expect(report, JSON.stringify(report)).toMatchObject({ status: 'applied', loop_id: 'loop_17', loop_status: 'active', full_acceptance: false });
  expect(f.readState().loops).toHaveLength(1);
  expect(loopMutations(f)).toEqual([`/loop every 60s ${LOOP_PROMPT}`]);
  const proof = JSON.parse(readFileSync(report.evidence_path, 'utf8'));
  const created = proof.observations.find(o => o.request.frame.method === 'loop/create');
  expect(created.request.frame.params).toEqual({ session_id: f.state.nativeSession, profile_id: f.state.profile,
    prompt: LOOP_PROMPT, mode: 'fixed_interval', interval_seconds: 60 });
  expect(proof.observations.at(-1).request.frame.method).toBe('loop/list');
  expect(proof.observations.at(-1).response.frame.result.loops[0].created_at_ms).toBe(created.response.frame.result.loop.created_at_ms);
  expect(report).not.toHaveProperty('natural_ticks_verified', true);
  expect(JSON.stringify(report)).not.toContain(LOOP_PROMPT);
});

it('pauses resumes and deletes only the original loop identity', async () => {
  const { runControlPlan } = await controller();
  for (const action of ['pause', 'resume', 'delete']) {
    const f = configureLoop(fixture(), `loop-${action}`);
    if (action === 'pause') f.update(s => { s.loops[0].status = 'active'; });
    const report = await runControlPlan(f.plan);
    expect(report, JSON.stringify(report)).toMatchObject({ status: 'applied', loop_id: 'loop_17', full_acceptance: false });
    expect(loopMutations(f)).toEqual([`/loop ${action} loop_17`]);
    expect(f.readState().loops[0].created_at_ms).toBe(f.plan.expected_loop.created_at_ms);
    const proof = JSON.parse(readFileSync(report.evidence_path, 'utf8'));
    const mutation = proof.observations.find(o => o.request.frame.method === `loop/${action}`);
    expect(mutation.request.frame.params).toEqual({ session_id: f.state.nativeSession, loop_id: 'loop_17' });
    expect(mutation.response.frame.result).not.toHaveProperty('profile_id');
    const listed = proof.observations.at(-1).response.frame.result.loops;
    expect(listed).toHaveLength(action === 'delete' ? 0 : 1);
    expect(report.loop_status).toBe({ pause: 'paused', resume: 'active', delete: 'deleted' }[action]);
  }
});

it.each([
  ['wrong interval', f => { f.plan.loop_template.interval_seconds = 120; }],
  ['multiline prompt', f => { f.plan.loop_template.prompt += '\n/goal clear'; }],
  ['wrong digest', f => { f.plan.loop_template.prompt_sha256 = 'a'.repeat(64); }],
  ['active goal', f => { f.update(s => { s.goal.status = 'active'; }); }],
  ['active turn', f => { f.update(s => { s.turns[0].state = 'active'; }); }],
  ['busy Herdr', f => { f.update(s => { s.agentInfo.agent_status = 'running'; }); }],
  ['existing loop', f => { configureLoop(f, 'loop-resume'); f.plan.operation = 'loop-create'; }],
])('rejects invalid loop templates and unavailable creation preconditions: %s', async (_label, mutate) => {
  const { runControlPlan } = await controller();
  const f = configureLoop(fixture(), 'loop-create'); mutate(f);
  const report = await runControlPlan(f.plan);
  expect(report.status).toBe('failed');
  expect(['invalid_loop_template', 'loop_not_ready', 'loop_set_not_empty']).toContain(report.reason);
  expect(loopMutations(f)).toEqual([]);
});

it.each([
  ['missing', s => { s.loops = []; }],
  ['duplicate', s => { s.loops.push(clone(s.loops[0])); }],
  ['foreign ID', s => { s.loops[0].loop_id = 'foreign'; }],
  ['changed creation', s => { s.loops[0].created_at_ms++; }],
  ['changed prompt', s => { s.loops[0].prompt += ' changed'; }],
  ['changed schedule', s => { s.loops[0].interval_seconds = 120; }],
  ['foreign profile', s => { s.loops[0].profile_id = 'foreign'; }],
  ['foreign session', s => { s.loops[0].session_id = 'foreign'; }],
])('rejects changed missing and ambiguous loop identities before control: %s', async (_label, mutate) => {
  const { runControlPlan } = await controller();
  const f = configureLoop(fixture(), 'loop-resume'); f.update(mutate);
  const report = await runControlPlan(f.plan);
  expect(report.status).toBe('failed');
  expect(['invalid_loops', 'loop_not_unique', 'loop_identity_changed']).toContain(report.reason);
  expect(f.sent()).toEqual(['/goal --report', '/loop list --report']);
  expect(loopMutations(f)).toEqual([]);
});

it('retains a delivered loop create without response and refuses reuse', async () => {
  const { runControlPlan } = await controller(); const f = configureLoop(fixture('missing_loop_response'), 'loop-create');
  const first = await runControlPlan(f.plan);
  expect(first, JSON.stringify(first)).toMatchObject({ status: 'outcome_unknown', full_acceptance: false });
  const evidence = JSON.parse(readFileSync(first.evidence_path, 'utf8'));
  expect(evidence.failed_exchange.records.some(r => r.frame.method === 'loop/create')).toBe(true);
  expect(f.readState().loops).toHaveLength(1);
  const intentPath = path.join(path.dirname(first.evidence_path), 'intent.json'), intent = readFileSync(intentPath);
  expect((await runControlPlan(f.plan)).status).toBe('rejected');
  expect(readFileSync(intentPath)).toEqual(intent); expect(loopMutations(f)).toHaveLength(1);
});

it('rejects conflicting loop responses and a deleted loop still listed', async () => {
  const { runControlPlan } = await controller();
  for (const mode of ['foreign_loop_response', 'keep_deleted_list']) {
    const f = configureLoop(fixture(mode), 'loop-delete');
    const report = await runControlPlan(f.plan);
    expect(report, mode).toMatchObject({ status: 'outcome_unknown', full_acceptance: false });
    expect(loopMutations(f)).toEqual(['/loop delete loop_17']);
  }
});

it('pauses a loop during active work without claiming idle', async () => {
  const { runControlPlan } = await controller(); const f = configureLoop(fixture(), 'loop-pause');
  f.update(s => { s.goal.status = 'active'; s.loops[0].status = 'active'; s.turns[0].state = 'active'; s.agentInfo.agent_status = 'running'; });
  const report = await runControlPlan(f.plan);
  expect(report).toMatchObject({ status: 'applied', loop_id: 'loop_17', loop_status: 'paused', full_acceptance: false });
  expect(report).not.toHaveProperty('session_idle', true);
  expect(f.readState().turns[0].state).toBe('active');
});

it.each([
  ['leading ASCII', ` ${LOOP_PROMPT}`],
  ['trailing ASCII', `${LOOP_PROMPT} `],
  ['leading NEL', `\u0085${LOOP_PROMPT}`],
  ['trailing NEL', `${LOOP_PROMPT}\u0085`],
])('rejects loop prompt whitespace that the native parser would trim: %s', async (_label, prompt) => {
  const { runControlPlan } = await controller();
  const f = configureLoop(fixture(), 'loop-create');
  f.plan.loop_template.prompt = prompt; f.plan.loop_template.prompt_sha256 = sha(prompt);
  expect(await runControlPlan(f.plan)).toMatchObject({ status: 'failed', reason: 'invalid_loop_template' });
  expect(f.sent()).toEqual([]);
});

it('deletes the original active loop from an idle paused goal without claiming turn cancellation', async () => {
  const { runControlPlan } = await controller(); const f = configureLoop(fixture(), 'loop-delete');
  f.update(s => { s.loops[0].status = 'active'; });
  const report = await runControlPlan(f.plan);
  expect(report).toMatchObject({ status: 'applied', loop_id: 'loop_17', loop_status: 'deleted', full_acceptance: false });
  expect(loopMutations(f)).toEqual(['/loop delete loop_17']);
  expect(report).not.toHaveProperty('session_idle', true);
  expect(report).not.toHaveProperty('turns_cancelled', true);
});
