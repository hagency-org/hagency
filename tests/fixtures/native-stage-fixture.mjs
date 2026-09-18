import { createHash, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import {
  appendFileSync,
  chmodSync,
  existsSync,
  linkSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { main as fakeHerdrMain } from './fake-herdr-native-control.mjs';

const fixtureModule = pathToFileURL(fileURLToPath(import.meta.url)).href;
const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const terminal = new Set(['completed', 'errored', 'interrupted']);
const sha256 = value => createHash('sha256').update(value).digest('hex');
const clone = value => structuredClone(value);

function pin(file) {
  return { path: file, sha256: sha256(readFileSync(file)) };
}

function readonlyBytes(file, bytes, mode = 0o444) {
  mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  writeFileSync(file, bytes, { mode: 0o600 });
  chmodSync(file, mode);
  return pin(file);
}

function readonlyJson(file, value) {
  return readonlyBytes(file, Buffer.from(`${JSON.stringify(value)}\n`));
}

function executable(file, source) {
  return readonlyBytes(file, Buffer.from(source), 0o555);
}

function startedNow(offset = 0) {
  const milliseconds = Date.now() + offset;
  return `${Math.floor(milliseconds / 1000)}.${String((milliseconds % 1000) * 1000).padStart(6, '0')}`;
}

function stateFrom(file) {
  return JSON.parse(readFileSync(file, 'utf8'));
}

function saveState(file, value) {
  writeFileSync(file, JSON.stringify(value));
}

function isPrompt(args, state, command) {
  return args[0] === '--session' && args[1] === state.herdrSession
    && args[2] === 'agent' && args[3] === 'prompt' && args[4] === state.agent
    && args[5] === command;
}

function prepareAuditState(state, args) {
  if (!isPrompt(args, state, '/turn session') || !state.auditStarted) return;
  state.auditHydrates += 1;
  const active = state.mode === 'never_settles' || state.auditHydrates <= 2;
  const audit = {
    turn_id: state.auditTurnId,
    session_id: state.nativeSession,
    state: active ? 'active' : 'completed',
  };
  state.turns = [...state.baseTurns, audit];
  if (!active) state.auditSettled = true;
}

function recordResumeEvent(state) {
  appendFileSync(state.events, `${JSON.stringify({
    at_ms: Date.now(),
    command: state.lastCommand,
    release_exists: existsSync(state.release),
    intent_exists: existsSync(path.join(state.activation, 'intent.json')),
    goal_state: state.goal.status,
    loop_status: state.loops[0]?.status ?? null,
    terminal_turns: state.turns.length > 0 && state.turns.every(turn => terminal.has(turn.state)),
  })}\n`);
}

export async function fixtureHerdrCommand(statePath, args) {
  let state = stateFrom(statePath);
  if (state.mode === 'final_inspect_failure' && state.postGoalProbeAllowance === 0
    && args[2] === 'agent' && args[3] === 'get') {
    throw new Error('fixture final inspect failure');
  }
  if (state.mode === 'final_inspect_failure' && state.postGoalProbeAllowance > 0
    && ((args[2] === 'agent' && args[3] === 'get') || args[2] === 'pane')) {
    state.postGoalProbeAllowance -= 1;
    saveState(statePath, state);
  }
  if (state.mode === 'goal_preflight_failure' && state.auditSettled
    && isPrompt(args, state, '/goal --report')) {
    state.goal.status = 'active';
  }
  prepareAuditState(state, args);
  saveState(statePath, state);

  const command = args[2] === 'agent' && args[3] === 'prompt' ? args[5] : null;
  const resuming = command === '/goal resume' || command?.startsWith('/loop resume ');
  if (resuming && (!existsSync(state.release)
    || !existsSync(path.join(state.activation, 'intent.json')))) {
    throw new Error('fixture resume without durable release and intent');
  }
  fakeHerdrMain(statePath, args);

  if (resuming) {
    state = stateFrom(statePath);
    state.lastCommand = command;
    if (command.startsWith('/loop resume ')) {
      state.auditStarted = true;
      state.auditHydrates = 0;
      state.auditSettled = false;
    } else if (state.mode === 'final_inspect_failure') {
      state.postGoalProbeAllowance = 2;
    }
    saveState(statePath, state);
    recordResumeEvent(state);
  }
}

export function fixtureBirthCommand(statePath) {
  const state = stateFrom(statePath);
  if (state.mode === 'birth_failure') throw new Error('fixture birth unavailable');
  state.birthCalls += 1;
  const rows = clone(state.rows);
  if (state.mode === 'observer_birth_change' && state.birthCalls >= 2) {
    const observer = rows.find(row => row.pid === state.observerMetadata.pid);
    if (observer) observer.started = '1789220001.000031';
  }
  saveState(statePath, state);
  process.stdout.write(`${JSON.stringify({ version: 1, processes: rows })}\n`);
}

export function fixtureMetadataCommand(statePath, args) {
  const state = stateFrom(statePath);
  const expected = String(state.observerMetadata.pid);
  if (state.mode === 'metadata_failure' || args.length !== 2 || args[0] !== '--pid'
    || args[1] !== expected || !state.rows.some(row => row.pid === state.observerMetadata.pid)) {
    process.stderr.write('{"category":"fixture_metadata_unavailable"}\n');
    process.exitCode = 1;
    return;
  }
  process.stdout.write(`${JSON.stringify(state.observerMetadata)}\n`);
}

function wrapperSource(kind, statePath) {
  const entry = kind === 'herdr'
    ? `await fixtureHerdrCommand(${JSON.stringify(statePath)}, process.argv.slice(2));`
    : kind === 'birth'
      ? `fixtureBirthCommand(${JSON.stringify(statePath)});`
      : `fixtureMetadataCommand(${JSON.stringify(statePath)}, process.argv.slice(2));`;
  const imported = kind === 'herdr' ? 'fixtureHerdrCommand'
    : kind === 'birth' ? 'fixtureBirthCommand' : 'fixtureMetadataCommand';
  return `#!${process.execPath}\nimport { ${imported} } from ${JSON.stringify(fixtureModule)};\n${entry}\n`;
}

function protectedIncrement(project, increment) {
  const number = String(increment).padStart(2, '0');
  const testId = randomUUID();
  const checkpointRelative = `.autonomy/checkpoints/${number}.json`;
  const sourceRelative = `.autonomy/sources/${number}.js`;
  const testRelative = `.autonomy/test-outputs/${testId}.json`;
  const redGreenRelative = `.autonomy/red-green/${number}.json`;
  const source = readonlyBytes(path.join(project, sourceRelative), Buffer.from(`export const increment = ${increment};\n`));
  const testOutput = readonlyJson(path.join(project, testRelative), { version: 1, increment, status: 'passed' });
  const capture = kind => {
    const value = { version: 1, increment, phase: kind, status: kind === 'red' ? 'failed' : 'passed' };
    const raw = Buffer.from(`${JSON.stringify(value)}\n`);
    const digest = sha256(raw);
    const relative = `.hagency-test-evidence/capture-${randomUUID()}-${digest}.json`;
    return { relative, pin: readonlyBytes(path.join(project, relative), raw) };
  };
  const red = capture('red');
  const green = capture('green');
  const checkpoint = readonlyJson(path.join(project, checkpointRelative), {
    increment,
    sourceSnapshot: sourceRelative,
    testOutput: testRelative,
  });
  const redGreen = readonlyJson(path.join(project, redGreenRelative), {
    increment,
    red_capture_path: red.pin.path,
    green_capture_path: green.pin.path,
  });
  return {
    protected: {
      [checkpointRelative]: checkpoint.sha256,
      [sourceRelative]: source.sha256,
      [testRelative]: testOutput.sha256,
      [redGreenRelative]: redGreen.sha256,
      [red.relative]: red.pin.sha256,
      [green.relative]: green.pin.sha256,
    },
    observer: {
      [checkpointRelative]: checkpoint.sha256,
      [sourceRelative]: source.sha256,
      [testRelative]: testOutput.sha256,
    },
  };
}

function registerPin(registry, name, get, set, json = true) {
  registry.set(name, { get, set, json });
}

export function stageFixture(stage = 'interrupt03', { controlPlacement = 'external', instanceSystemAlias = false } = {}) {
  if (!['interrupt03', 'restart04'].includes(stage)) throw new Error('invalid fixture stage');
  if (!['external', 'same', 'inside'].includes(controlPlacement)) throw new Error('invalid control placement');
  const root = realpathSync(mkdtempSync(path.join(instanceSystemAlias ? '/private/tmp' : tmpdir(), 'native-stage-fixture-')));
  const project = path.join(root, 'business project');
  const control = controlPlacement === 'same' ? project
    : controlPlacement === 'inside' ? path.join(project, 'private control') : path.join(root, 'private control');
  const attemptId = randomUUID();
  const attemptPath = path.join(control, 'recovery-attempts', attemptId);
  const observerRoot = path.join(attemptPath, 'fault-observer', stage);
  const activation = path.join(attemptPath, `activation-${stage}`);
  const autonomy = path.join(project, '.autonomy');
  const release = path.join(autonomy, `stage${stage === 'interrupt03' ? '03' : '04'}-release.json`);
  const statePath = path.join(root, 'fixture state.json');
  const trace = path.join(root, 'native trace.jsonl');
  const calls = path.join(root, 'native calls.jsonl');
  const events = path.join(root, 'stage-events.jsonl');
  const tools = path.join(root, 'tools');
  const instance = path.join(instanceSystemAlias ? root.replace(/^\/private\/tmp\//, '/tmp/') : root, 'instance data');
  const backendState = path.join(root, 'backend state');
  const backendConfig = path.join(root, 'backend config.json');
  const middleWorkdir = path.join(root, 'middle workdir');
  const helpers = path.join(root, 'frozen helpers');
  const evidence = path.join(root, 'prerequisites');
  for (const directory of [
    project, control, attemptPath, observerRoot, autonomy, tools, instance, backendState, middleWorkdir, helpers, evidence,
  ]) {
    mkdirSync(directory, { recursive: true, mode: 0o700 });
  }
  writeFileSync(backendConfig, '{}\n', { mode: 0o600 });
  writeFileSync(calls, '');
  writeFileSync(events, '');

  const oldBindingPrefix = Buffer.from(`${JSON.stringify({
    ts: new Date(Date.now() - 5000).toISOString(),
    direction: 'client_to_server',
    frame: { id: 'fixture-old-prefix', method: 'session/goal/get', params: { session_id: 'fixture:local:tui#coding' } },
  })}\n`);
  const safePrefix = Buffer.concat([oldBindingPrefix, Buffer.from(`${JSON.stringify({
    ts: new Date(Date.now() - 4500).toISOString(),
    direction: 'server_to_client',
    frame: { id: 'fixture-old-prefix', result: { session_id: 'fixture:local:tui#coding' } },
  })}\n`)]);
  writeFileSync(trace, safePrefix, { mode: 0o600 });
  const traceInformation = lstatSync(trace);
  const linkedTrace = path.join(attemptPath, 'trace.jsonl');
  linkSync(trace, linkedTrace);

  const objective = 'PRIVATE_FIXTURE_OBJECTIVE_MUST_NOT_ESCAPE';
  const expectedGoal = {
    goal_id: 'goal_01',
    created_at_ms: Date.now() - 60_000,
    objective_sha256: sha256(objective),
  };
  const historicalObserverPid = 50_035;
  const historicalArmedAt = Date.now() / 1000 - 60;
  const jobId = randomUUID();
  const predecessorPaths = Object.fromEntries([
    'audit', 'safe_state', 'observer_binding', 'observer_claim', 'observer_ready', 'watch_manifest', 'watch_exit',
  ].map(name => [name, path.join(evidence, `${name}.json`)]));
  const predecessorValues = {
    watch_manifest: {
      version: 1,
      job_id: jobId,
      manifest_path: predecessorPaths.watch_manifest,
      deadline_at: Date.now() - 30_000,
    },
    observer_binding: {
      version: 1,
      stage: 'interrupt03',
      project,
      control,
      trace,
      trace_identity: [traceInformation.dev, traceInformation.ino],
      trace_offset: oldBindingPrefix.length,
      trace_prefix_sha256: sha256(oldBindingPrefix),
      goal_id: expectedGoal.goal_id,
      goal_objective_sha256: expectedGoal.objective_sha256,
      private_detail: 'historical-only',
    },
    observer_claim: { armed_at: historicalArmedAt, pid: historicalObserverPid, stage: 'interrupt03' },
    observer_ready: {
      armed_at: historicalArmedAt,
      pid: historicalObserverPid,
      stage: 'interrupt03',
      trace_identity: [traceInformation.dev, traceInformation.ino],
      trace_prefix_sha256: sha256(oldBindingPrefix),
    },
    safe_state: {
      full_acceptance: false,
      goal_paused: true,
      loops_empty: true,
      all_turns_terminal: true,
      observer_only_armed_no_fault: true,
      stage34_absent: true,
      trace_identity: [traceInformation.dev, traceInformation.ino],
      trace_prefix_size: safePrefix.length,
      trace_prefix_sha256: sha256(safePrefix),
      owned_processes_present: { [historicalObserverPid]: null, 50_042: null, 50_050: null },
      observations: [{
        request: { frame: { method: 'session/goal/get', id: 'fixture-predecessor-goal' } },
        response: { frame: { id: 'fixture-predecessor-goal', result: { goal: {
          goal_id: expectedGoal.goal_id,
          created_at_ms: expectedGoal.created_at_ms,
          status: 'paused',
          objective,
        } } } },
      }],
    },
  };
  const predecessorPins = {};
  predecessorPins.watch_manifest = readonlyJson(predecessorPaths.watch_manifest, predecessorValues.watch_manifest);
  predecessorValues.watch_exit = {
    returncode: 3,
    manifest_unchanged: true,
    manifest_path: predecessorPaths.watch_manifest,
    manifest_sha256: predecessorPins.watch_manifest.sha256,
    job_id: jobId,
  };
  predecessorPins.watch_exit = readonlyJson(predecessorPaths.watch_exit, predecessorValues.watch_exit);
  predecessorValues.audit = {
    scope: 'Q3_entire_actual_failed_watch_lifetime',
    old_watch_eligible_for_acceptance: false,
    full_acceptance: false,
    native_terminal_exit_code: 3,
    immutable_deadline_at: predecessorValues.watch_manifest.deadline_at,
    manifest_path: predecessorPaths.watch_manifest,
    manifest_sha256: predecessorPins.watch_manifest.sha256,
    actual_exit_path: predecessorPaths.watch_exit,
    actual_exit_sha256: predecessorPins.watch_exit.sha256,
  };
  for (const name of ['audit', 'safe_state', 'observer_binding', 'observer_claim', 'observer_ready']) {
    predecessorPins[name] = readonlyJson(predecessorPaths[name], predecessorValues[name]);
  }
  const predecessors = Object.fromEntries([
    'audit', 'safe_state', 'observer_binding', 'observer_claim', 'observer_ready', 'watch_manifest', 'watch_exit',
  ].map(name => [name, clone(predecessorPins[name])]));
  const contract = readonlyJson(path.join(evidence, 'recovery-contract.json'), {
    version: 1,
    attempt_id: attemptId,
    base_control: control,
    project,
    goal: expectedGoal,
    predecessor_full_acceptance: false,
    stages: ['interrupt03', 'restart04'],
    predecessors,
  });
  const traceFence = {
    source_path: trace,
    linked_path: linkedTrace,
    device: traceInformation.dev,
    inode: traceInformation.ino,
    prefix_size: safePrefix.length,
    prefix_sha256: sha256(safePrefix),
  };
  const attemptManifest = readonlyJson(path.join(attemptPath, 'attempt.json'), {
    version: 1,
    attempt_id: attemptId,
    status: 'claimed',
    full_acceptance: false,
    predecessor_full_acceptance: false,
    base_control: control,
    project,
    recovery_contract: contract,
    predecessors,
    goal: expectedGoal,
    stages: ['interrupt03', 'restart04'],
    trace_fence: traceFence,
  });
  const prepared = readonlyJson(path.join(attemptPath, 'prepared.json'), {
    version: 1,
    attempt_id: attemptId,
    status: 'prepared',
    full_acceptance: false,
    predecessor_full_acceptance: false,
    attempt_path: attemptPath,
    manifest_path: attemptManifest.path,
    trace_path: linkedTrace,
    same_inode: true,
    trace_fence: traceFence,
  });

  appendFileSync(trace, `${JSON.stringify({
    ts: new Date(Date.now() - 4000).toISOString(),
    direction: 'server_to_client',
    frame: { id: 'fixture-old-prefix', result: { session_id: 'fixture:local:tui#coding' } },
  })}\n`);
  const bindingPrefix = readFileSync(trace);
  const priorCount = stage === 'interrupt03' ? 2 : 3;
  const protectedMap = {};
  const observerProtected = {};
  for (let increment = 1; increment <= priorCount; increment += 1) {
    const values = protectedIncrement(project, increment);
    Object.assign(protectedMap, values.protected);
    Object.assign(observerProtected, values.observer);
  }

  const observerScript = path.join(helpers, 'fault_observer.py');
  const adapterScript = path.join(helpers, 'native_adapter.py');
  const observerHelper = readonlyBytes(observerScript, Buffer.from('import sys\nsys.stdin.buffer.read()\n'));
  const adapterHelper = readonlyBytes(adapterScript, Buffer.from('raise SystemExit("fixture adapter is never executed")\n'));
  const launchBindingPath = path.join(attemptPath, `observer launch ${randomUUID()}.json`);
  const python = '/usr/bin/python3';
  const observerArgv = [
    python,
    path.relative(middleWorkdir, observerScript),
    '--binding',
    path.relative(middleWorkdir, launchBindingPath),
    '--stage',
    stage,
  ];
  const observerChild = spawn(python, observerArgv.slice(1), {
    cwd: middleWorkdir,
    env: { PYTHONDONTWRITEBYTECODE: '1' },
    stdio: ['pipe', 'ignore', 'pipe'],
  });
  const observerExit = new Promise((resolve, reject) => {
    observerChild.once('error', reject);
    observerChild.once('exit', (code, signal) => resolve({ code, signal }));
  });
  const observer = {
    pid: observerChild.pid,
    ppid: process.pid,
    pgid: process.pid,
    started: startedNow(),
    cwd: middleWorkdir,
    argv: observerArgv,
  };

  const herdr = readonlyBytes(path.join(tools, 'herdr'), Buffer.from(wrapperSource('herdr', statePath)), 0o700);
  const birthTool = readonlyBytes(path.join(tools, 'birth-tool'), Buffer.from(wrapperSource('birth', statePath)), 0o755);
  const metadataTool = executable(path.join(tools, 'metadata-tool'), wrapperSource('metadata', statePath));
  const frontendBinary = readonlyBytes(path.join(tools, 'octoscode'), Buffer.from('fixture frontend\n'), 0o755);
  const backendBinary = readonlyBytes(path.join(tools, 'octos'), Buffer.from('fixture backend\n'), 0o555);
  const shellPid = 41_001;
  const frontend = { pid: 41_002, ppid: shellPid, pgid: 41_002, started: startedNow(-3000), cwd: project,
    argv: [frontendBinary.path, '--mode', 'protocol', '--profile-id', 'fixture', '--cwd', project,
      '--no-splash', '--lang', 'en', '--stdio-command', `octos serve --stdio --cwd ${project}`] };
  const backend = { pid: 41_003, ppid: frontend.pid, pgid: frontend.pgid, started: startedNow(-2000), cwd: project,
    argv: [backendBinary.path, 'serve', '--stdio', '--solo', '--data-dir', backendState,
      '--instance-data-dir', instance, '--config', backendConfig, '--cwd', project, '--no-network'] };
  const shell = { pid: shellPid, ppid: 1, pgid: shellPid, started: startedNow(-4000), state: 'S' };
  const baseTurns = [
    { turn_id: randomUUID(), session_id: 'fixture:local:tui#coding', state: 'completed' },
    { turn_id: randomUUID(), session_id: 'fixture:local:tui#coding', state: 'completed' },
  ];
  const expectedLoop = {
    loop_id: 'loop_17',
    created_at_ms: Date.now() - 30_000,
    prompt_sha256: sha256('fixture recurring audit'),
    mode: 'fixed_interval',
    interval_seconds: 60,
  };
  const loops = stage === 'restart04' ? [{
    ...expectedLoop,
    session_id: 'fixture:local:tui#coding',
    profile_id: 'fixture',
    prompt: 'fixture recurring audit',
    status: 'paused',
    updated_at_ms: Date.now() - 1000,
    next_run_at_ms: Date.now() + 60_000,
    last_run_at_ms: null,
    expires_at_ms: Date.now() + 86_400_000,
  }] : [];
  const rows = [shell,
    { pid: frontend.pid, ppid: frontend.ppid, pgid: frontend.pgid, started: frontend.started, state: 'S' },
    { pid: backend.pid, ppid: backend.ppid, pgid: backend.pgid, started: backend.started, state: 'S' },
    { pid: observer.pid, ppid: observer.ppid, pgid: observer.pgid, started: observer.started, state: 'S' },
    { pid: 49_999, ppid: 1, pgid: 49_999, started: startedNow(-5000), state: 'S' },
  ];
  const binding = {
    version: 1,
    project,
    trace: linkedTrace,
    herdr_session: 'fixture-session',
    lower_agent: 'fixture-lower',
    pane_id: 'wA:pC',
    terminal_id: 'fixture-terminal',
    profile: 'fixture',
    native_session: 'fixture:local:tui#coding',
    shell_pid: shellPid,
    frontend,
    backend,
    herdr,
    birth_tool: birthTool,
    frontend_binary: frontendBinary,
  };
  const observerBindingValue = {
    version: 1,
    stage,
    project,
    control: attemptPath,
    instance,
    profile: binding.profile,
    lower_agent: binding.lower_agent,
    herdr_session: binding.herdr_session,
    native_session: binding.native_session,
    pane_id: binding.pane_id,
    shell_pid: binding.shell_pid,
    goal_id: expectedGoal.goal_id,
    goal_objective_sha256: expectedGoal.objective_sha256,
    frontend: clone(frontend),
    backend: clone(backend),
    frontend_binary: binding.frontend_binary.path,
    frontend_binary_sha256: binding.frontend_binary.sha256,
    herdr: binding.herdr.path,
    herdr_sha256: binding.herdr.sha256,
    birth_tool: binding.birth_tool.path,
    birth_tool_sha256: binding.birth_tool.sha256,
    trace: linkedTrace,
    trace_identity: [traceInformation.dev, traceInformation.ino],
    trace_offset: bindingPrefix.length,
    trace_prefix_sha256: sha256(bindingPrefix),
    protected: observerProtected,
    ...(stage === 'restart04' ? {
      loop_id: expectedLoop.loop_id,
      loop_interval_seconds: expectedLoop.interval_seconds,
      loop_prompt_sha256: expectedLoop.prompt_sha256,
    } : {}),
  };
  const observerBinding = readonlyJson(launchBindingPath, observerBindingValue);
  readonlyJson(path.join(observerRoot, 'binding.json'), observerBindingValue);
  const armedAt = Date.now() / 1000;
  const observerClaim = readonlyJson(path.join(observerRoot, 'claim.json'), { stage, armed_at: armedAt, pid: observer.pid });
  const observerReady = readonlyJson(path.join(observerRoot, 'ready.json'), {
    stage,
    armed_at: armedAt,
    pid: observer.pid,
    trace_identity: observerBindingValue.trace_identity,
    trace_prefix_sha256: observerBindingValue.trace_prefix_sha256,
  });
  const state = {
    mode: 'success',
    secret: 'PRIVATE_FIXTURE_DETAIL_MUST_NOT_ESCAPE',
    trace,
    calls,
    events,
    release,
    activation,
    nextId: 0,
    herdrSession: binding.herdr_session,
    nativeSession: binding.native_session,
    profile: binding.profile,
    paneId: binding.pane_id,
    agent: binding.lower_agent,
    agentInfo: { name: binding.lower_agent, agent: 'octoscode', pane_id: binding.pane_id,
      terminal_id: binding.terminal_id, cwd: project, foreground_cwd: project, agent_status: 'idle' },
    pane: { pane_id: binding.pane_id, shell_pid: shellPid, foreground_process_group_id: frontend.pgid,
      foreground_processes: [frontend, backend] },
    goal: { ...expectedGoal, objective, status: stage === 'restart04' ? 'blocked' : 'paused',
      profile_id: binding.profile, session_id: binding.native_session },
    loops,
    baseTurns,
    turns: clone(baseTurns),
    rows,
    observerMetadata: { version: 1, pid: observer.pid, cwd: observer.cwd, argv: observer.argv },
    birthCalls: 0,
    auditStarted: false,
    auditHydrates: 0,
    auditSettled: false,
    auditTurnId: randomUUID(),
    postGoalProbeAllowance: -1,
  };
  saveState(statePath, state);

  const prerequisiteNames = stage === 'restart04'
    ? ['natural03', 'pre_restart_audits', 'prerequisite_waits', 'blocked_terminal_order', 'tiny_timeout', 'same_backend_readonly_recovery']
    : [];
  const prerequisitePins = Object.fromEntries(prerequisiteNames.map(name => [name,
    readonlyJson(path.join(evidence, `prerequisite-${name}.json`), { version: 1, name, full_acceptance: false })]));
  const operationIds = Array.from({ length: stage === 'restart04' ? 8 : 3 }, () => randomUUID());
  const childOperations = stage === 'interrupt03' ? {
    initial_inspect: operationIds[0],
    goal_resume: operationIds[1],
    final_inspect: operationIds[2],
  } : {
    initial_inspect: operationIds[0],
    loop_resume: operationIds[1],
    settlement_inspects: operationIds.slice(2, 6),
    goal_resume: operationIds[6],
    final_inspect: operationIds[7],
  };
  const plan = {
    version: 1,
    activation_id: randomUUID(),
    stage,
    attempt_manifest: attemptManifest,
    prepared,
    observer_binding: observerBinding,
    observer_claim: observerClaim,
    observer_ready: observerReady,
    observer,
    metadata_tool: metadataTool,
    frozen_helpers: { observer: observerHelper, adapter: adapterHelper },
    protected: protectedMap,
    binding,
    expected_goal: expectedGoal,
    ...(stage === 'restart04' ? { expected_loop: expectedLoop } : {}),
    child_operations: childOperations,
    prerequisite_pins: prerequisitePins,
    controller_tools: {
      controller: pin(path.join(repositoryRoot, 'skills/hagency-inner-loop/scripts/native-control.mjs')),
      evidence: pin(path.join(repositoryRoot, 'skills/hagency-inner-loop/scripts/native-control-evidence.mjs')),
    },
    query_timeout_ms: 10_000,
    activation_timeout_ms: 120_000,
    observer_timeout_ms: 10_000,
    settlement_timeout_ms: 30_000,
    settlement_interval_ms: 100,
  };

  const registry = new Map();
  for (const key of ['attempt_manifest', 'prepared', 'observer_binding', 'observer_claim', 'observer_ready', 'metadata_tool']) {
    registerPin(registry, key, () => plan[key], value => { plan[key] = value; }, key !== 'metadata_tool');
  }
  for (const key of ['observer', 'adapter']) {
    registerPin(registry, `frozen_helpers.${key}`, () => plan.frozen_helpers[key],
      value => { plan.frozen_helpers[key] = value; }, false);
  }
  for (const [relative] of Object.entries(plan.protected)) {
    registerPin(registry, relative,
      () => ({ path: path.join(project, relative), sha256: plan.protected[relative] }),
      value => { plan.protected[relative] = value.sha256; }, true);
  }
  for (const key of Object.keys(plan.prerequisite_pins)) {
    registerPin(registry, `prerequisite_pins.${key}`, () => plan.prerequisite_pins[key],
      value => { plan.prerequisite_pins[key] = value; }, true);
  }

  const readState = () => stateFrom(statePath);
  const update = fn => {
    const next = readState();
    fn(next);
    saveState(statePath, next);
    return next;
  };
  const sent = () => readFileSync(calls, 'utf8').split('\n').filter(Boolean)
    .map(line => JSON.parse(line).command);
  const rewritePin = (name, fn) => {
    const entry = registry.get(name);
    if (!entry) throw new Error('unknown fixture pin');
    const current = entry.get();
    chmodSync(current.path, 0o600);
    const raw = readFileSync(current.path);
    let value = entry.json ? JSON.parse(raw) : raw;
    const replacement = fn(value);
    if (replacement !== undefined) value = replacement;
    const nextRaw = entry.json
      ? Buffer.from(`${JSON.stringify(value)}\n`)
      : Buffer.isBuffer(value) ? value : Buffer.from(String(value));
    writeFileSync(current.path, nextRaw);
    chmodSync(current.path, name === 'metadata_tool' ? 0o555 : 0o444);
    const next = pin(current.path);
    entry.set(next);
    return next;
  };
  let disposed = false;
  const dispose = async () => {
    if (disposed) return;
    disposed = true;
    observerChild.stdin.end();
    const exited = await observerExit;
    if (exited.code !== 0 || exited.signal !== null) throw new Error('fixture observer did not exit naturally');
    rmSync(root, { recursive: true, force: true });
  };
  return { root, plan, release, activation, observerRoot, statePath, trace, readState, update, sent, rewritePin, dispose };
}
