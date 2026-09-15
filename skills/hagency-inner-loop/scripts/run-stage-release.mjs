import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { closeSync, constants, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { setTimeout as delay } from 'node:timers/promises';
import { isDeepStrictEqual, promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { publishExclusiveJson, readReadonlyJson } from './native-control-evidence.mjs';

const execute = promisify(execFile);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const terminal = new Set(['completed', 'errored', 'interrupted']);
const sameFile = (a, b) => a.dev === b.dev && a.ino === b.ino;
class StageError extends Error { constructor(code) { super(code); this.code = code; } }
function requireProof(value, reason) { if (!value) throw new StageError(reason); }
function remaining(deadline) {
  const value = Math.floor(deadline - performance.now());
  requireProof(value > 0, 'activation_deadline');
  return value;
}
function exists(file) {
  try { lstatSync(file); return true; }
  catch (error) { if (error.code === 'ENOENT') return false; throw error; }
}
function syncDirectory(directory) {
  const fd = openSync(directory, constants.O_RDONLY | constants.O_DIRECTORY);
  try { fsyncSync(fd); } finally { closeSync(fd); }
}
function verifyControllerTools(plan) {
  for (const [key, name] of [['controller', 'native-control.mjs'], ['evidence', 'native-control-evidence.mjs']]) {
    const pin = plan.controller_tools[key];
    const expected = realpathSync(fileURLToPath(new URL(name, import.meta.url)));
    requireProof(pin.path === expected && realpathSync(pin.path) === pin.path, 'controller_path_changed');
    const initial = lstatSync(pin.path);
    requireProof(initial.isFile() && !initial.isSymbolicLink() && initial.size <= 1024 * 1024, 'controller_changed');
    const fd = openSync(pin.path, constants.O_RDONLY | constants.O_NOFOLLOW);
    try {
      const before = fstatSync(fd), raw = readFileSync(fd), after = fstatSync(fd);
      requireProof(sameFile(initial, before) && sameFile(before, after)
        && sameFile(after, lstatSync(pin.path)) && raw.length === before.size && after.size === before.size
        && before.mtimeMs === after.mtimeMs && sha(raw) === pin.sha256, 'controller_changed');
    } finally { closeSync(fd); }
  }
}

// Inspect evidence is produced by the pinned controller, not a status label
// supplied by the caller. Keep loop identity and terminal eligibility separate.
function inspectedState(plan, child, goalStatus, loopStatus) {
  const evidence = readReadonlyJson(child.report.evidence_path).value;
  requireProof(evidence.version === 1 && evidence.report.status === 'observed'
    && evidence.report.operation_id === child.operation_id
    && Array.isArray(evidence.observations) && evidence.observations.length === 3
    && Array.isArray(evidence.processes) && evidence.processes.length === 3, 'native_inspection_missing');
  const methods = ['session/goal/get', 'loop/list', 'session/hydrate'];
  const results = evidence.observations.map((observation, index) => {
    const request = observation.request?.frame, response = observation.response?.frame;
    requireProof(request?.method === methods[index] && request.id === response?.id
      && response.result?.session_id === plan.binding.native_session
      && (index === 2 || response.result.profile_id === plan.binding.profile), 'native_inspection_scope');
    return response.result;
  });
  const goal = results[0].goal, expected = plan.expected_goal;
  requireProof(goal?.goal_id === expected.goal_id && goal.created_at_ms === expected.created_at_ms
    && typeof goal.objective === 'string' && sha(goal.objective) === expected.objective_sha256
    && goal.profile_id === plan.binding.profile
    && (!Object.hasOwn(goal, 'session_id') || goal.session_id === plan.binding.native_session)
    && (goalStatus === 'active' ? goal.status === 'active' : ['paused', 'blocked'].includes(goal.status)), 'goal_state_changed');
  const loops = results[1].loops;
  requireProof(Array.isArray(loops), 'loop_state_missing');
  if (loopStatus === 'none') requireProof(loops.length === 0, 'unexpected_loop');
  else {
    const loop = loops[0], bound = plan.expected_loop;
    requireProof(loops.length === 1 && loop.loop_id === bound.loop_id && loop.created_at_ms === bound.created_at_ms
      && loop.session_id === plan.binding.native_session && loop.profile_id === plan.binding.profile
      && typeof loop.prompt === 'string' && sha(loop.prompt) === bound.prompt_sha256
      && loop.mode === 'fixed_interval' && loop.interval_seconds === 60 && loop.status === loopStatus, 'loop_state_changed');
  }
  const turns = results[2].turns;
  requireProof(Array.isArray(turns) && turns.length > 0, 'terminal_history_missing');
  const ids = new Set();
  for (const turn of turns) {
    requireProof(typeof turn.turn_id === 'string' && !ids.has(turn.turn_id)
      && (terminal.has(turn.state) || ['active', 'interrupting'].includes(turn.state)), 'turn_history_invalid');
    ids.add(turn.turn_id);
  }
  const after = evidence.processes.at(-1)?.after;
  requireProof(after && Number.isSafeInteger(after.observed_at_ms)
    && after.observed_at_ms >= child.started_at_ms && after.observed_at_ms <= child.settled_at_ms,
  'native_inspection_time');
  return { terminal: turns.every(turn => terminal.has(turn.state)) && after.agent_status === 'idle' };
}

/** One consumed attempt/stage claim, never an E2E acceptance verdict. */
export async function runStageRelease(input) {
  let context, plan, activationInfo, controlsInfo, controlsPath, deadline;
  let claimed = false, releaseAttempted = false;
  const children = [], observations = [], directories = new Map();
  const report = (status, reason) => ({ version: 1, status, full_acceptance: false,
    ...(plan ? { activation_id: plan.activation_id, stage: plan.stage } : {}),
    ...(reason ? { reason } : {}) });
  const retainActivation = () => {
    for (const [directory, retained] of directories) {
      const current = lstatSync(directory);
      requireProof(current.isDirectory() && !current.isSymbolicLink() && sameFile(current, retained)
        && current.uid === retained.uid && current.mode === retained.mode, 'stage_ancestry_changed');
    }
    const current = lstatSync(context.activation_path);
    requireProof(current.isDirectory() && !current.isSymbolicLink() && sameFile(current, activationInfo)
      && current.uid === process.geteuid() && (current.mode & 0o777) === 0o700
      && realpathSync(context.activation_path) === context.activation_path, 'activation_directory_changed');
    if (controlsInfo) {
      const controls = lstatSync(controlsPath);
      requireProof(controls.isDirectory() && !controls.isSymbolicLink() && sameFile(controls, controlsInfo)
        && controls.uid === process.geteuid() && (controls.mode & 0o777) === 0o700,
      'activation_directory_changed');
    }
  };
  const finish = result => {
    if (!claimed) return result;
    try {
      retainActivation();
      const evidencePath = path.join(context.activation_path, 'result.json');
      publishExclusiveJson(evidencePath, { version: 1, report: result, children, observations,
        release_attempted: releaseAttempted, full_acceptance: false });
      return { ...result, evidence_path: evidencePath };
    } catch {
      return report(releaseAttempted ? 'outcome_unknown' : 'failed', 'stage_evidence_publication_failed');
    }
  };
  try {
    const { validateStagePlan, verifyStageEvidence, observeStageObserver } = await import('./native-stage-evidence.mjs');
    context = validateStagePlan(input); plan = context.plan;
    deadline = performance.now() + plan.activation_timeout_ms;
    // A completed/failed stage remains consumed after its live inputs change.
    if (exists(context.activation_path)) return report('rejected', 'stage_already_claimed');
    verifyStageEvidence(context);
    for (let current of [context.attempt_path, path.dirname(context.release_path)]) {
      for (;;) {
        const information = lstatSync(current);
        requireProof(information.isDirectory() && !information.isSymbolicLink(), 'stage_ancestry_changed');
        directories.set(current, information);
        if (path.dirname(current) === current) break;
        current = path.dirname(current);
      }
    }
    try { mkdirSync(context.activation_path, { mode: 0o700 }); }
    catch (error) { if (error.code === 'EEXIST') return report('rejected', 'stage_already_claimed'); throw error; }
    claimed = true; activationInfo = lstatSync(context.activation_path);
    syncDirectory(context.attempt_path); retainActivation();
    publishExclusiveJson(path.join(context.activation_path, 'plan.json'), plan);
    controlsPath = path.join(context.activation_path, 'controls');
    mkdirSync(controlsPath, { mode: 0o700 }); controlsInfo = lstatSync(controlsPath);
    syncDirectory(context.activation_path);

    let latestObserver;
    const native = async (operation, id, childDeadline = deadline) => {
      retainActivation(); verifyControllerTools(plan);
      remaining(childDeadline);
      const childPlan = { version: 1, operation_id: id, operation, binding: plan.binding,
        expected_goal: plan.expected_goal, evidence_dir: controlsPath, query_timeout_ms: plan.query_timeout_ms,
        ...(operation.startsWith('loop-') ? { expected_loop: plan.expected_loop } : {}) };
      const inputPath = path.join(context.activation_path, `child-input-${id}.json`);
      const pinned = publishExclusiveJson(inputPath, childPlan);
      // Parent dispatch gate: publication and tool hashing can outlive the
      // observation. This does not make downstream native RPCs atomic.
      if (operation !== 'inspect') fresh(latestObserver);
      const child = { operation, operation_id: id, started_at_ms: Date.now(), exit_code: null, signal: null };
      let stdout = '', stderr = '';
      try {
        const output = await execute(process.execPath, [plan.controller_tools.controller.path,
          '--plan', inputPath, '--sha256', pinned.sha256], {
          timeout: remaining(childDeadline), maxBuffer: 1024 * 1024, encoding: 'utf8', killSignal: 'SIGKILL', windowsHide: true,
        });
        stdout = output.stdout; stderr = output.stderr; child.exit_code = 0;
      } catch (error) {
        stdout = typeof error.stdout === 'string' ? error.stdout : '';
        stderr = typeof error.stderr === 'string' ? error.stderr : '';
        child.exit_code = Number.isInteger(error.code) ? error.code : null;
        child.signal = error.signal ?? null;
        child.killed = error.killed === true;
      }
      child.settled_at_ms = Date.now();
      try { child.report = JSON.parse(stdout); } catch { child.report = null; }
      children.push(child);
      retainActivation();
      publishExclusiveJson(path.join(context.activation_path, `child-exit-${id}.json`), { ...child, stdout, stderr });
      verifyControllerTools(plan);
      remaining(childDeadline);
      requireProof(child.exit_code === 0 && child.signal === null && stderr === ''
        && child.report?.status === (operation === 'inspect' ? 'observed' : 'applied')
        && child.report.operation === operation && child.report.operation_id === id
        && child.report.full_acceptance === false
        && child.report.evidence_path === path.join(controlsPath, id, 'evidence.json'), 'native_control_not_verified');
      const recordedPlan = readReadonlyJson(path.join(controlsPath, id, 'plan.json')).value;
      requireProof(isDeepStrictEqual(recordedPlan, childPlan), 'native_child_plan_changed');
      return child;
    };
    const observe = async name => {
      remaining(deadline); retainActivation();
      verifyStageEvidence(context, { released: releaseAttempted });
      const evidence = await observeStageObserver(context, Math.min(deadline, performance.now() + plan.observer_timeout_ms));
      retainActivation();
      const observed = publishExclusiveJson(path.join(context.activation_path, name), evidence);
      observations.push({ path: path.join(context.activation_path, name), sha256: observed.sha256 });
      latestObserver = evidence;
      return evidence;
    };
    const fresh = observed => {
      remaining(deadline);
      const now = Date.now();
      requireProof(Number.isFinite(observed.started_at_ms) && Number.isFinite(observed.observed_at_ms)
        && observed.observed_at_ms >= observed.started_at_ms && now >= observed.observed_at_ms
        && now - observed.observed_at_ms <= 2000, 'observer_not_fresh');
    };
    const initial = await native('inspect', plan.child_operations.initial_inspect);
    const initialState = inspectedState(plan, initial, 'paused', plan.stage === 'interrupt03' ? 'none' : 'paused');
    requireProof(initialState.terminal, 'native_not_idle');
    const observerBeforeRelease = await observe('observer-before-release.json');
    verifyStageEvidence(context); fresh(observerBeforeRelease); retainActivation();
    publishExclusiveJson(path.join(context.activation_path, 'intent.json'), { version: 1,
      activation_id: plan.activation_id, stage: plan.stage, created_at_ms: Date.now(),
      expected_goal: plan.expected_goal, child_operations: plan.child_operations,
      ...(plan.expected_loop ? { expected_loop: plan.expected_loop } : {}),
      observer_evidence: observations.at(-1), release_path: context.release_path, full_acceptance: false });
    verifyStageEvidence(context); fresh(observerBeforeRelease); retainActivation();
    // Publication can become visible even when its final fsync/read-back fails.
    releaseAttempted = true;
    publishExclusiveJson(context.release_path, { version: 1, activation_id: plan.activation_id,
      attempt_id: path.basename(context.attempt_path), stage: plan.stage,
      goal_id: plan.expected_goal.goal_id, released_at_ms: Date.now() });

    if (plan.stage === 'restart04') {
      await native('loop-resume', plan.child_operations.loop_resume);
      const settleDeadline = Math.min(deadline, performance.now() + plan.settlement_timeout_ms);
      let settled = false;
      for (const id of plan.child_operations.settlement_inspects) {
        const state = inspectedState(plan, await native('inspect', id, settleDeadline), 'paused', 'active');
        if (state.terminal) { settled = true; break; }
        await delay(Math.min(plan.settlement_interval_ms, remaining(settleDeadline)));
      }
      requireProof(settled, 'loop_settlement_unverified');
      const observerBeforeGoal = await observe('observer-before-goal.json');
      verifyStageEvidence(context, { released: true }); fresh(observerBeforeGoal);
    }
    await native('goal-resume', plan.child_operations.goal_resume);
    const final = await native('inspect', plan.child_operations.final_inspect);
    inspectedState(plan, final, 'active', plan.stage === 'interrupt03' ? 'none' : 'active');
    return finish(report('activated'));
  } catch (error) {
    return finish(report(releaseAttempted ? 'outcome_unknown' : 'failed',
      error instanceof StageError ? error.code : 'invalid_or_unavailable_stage_evidence'));
  }
}

async function main(args) {
  let report;
  try {
    if (args.length !== 4 || args[0] !== '--plan' || args[2] !== '--sha256'
      || !/^[0-9a-f]{64}$/.test(args[3])) throw new Error('invalid_cli');
    const plan = readReadonlyJson(args[1], args[3]).value;
    report = await runStageRelease(plan);
  } catch {
    report = { version: 1, status: 'failed', reason: 'invalid_plan', full_acceptance: false };
  }
  process.stdout.write(`${JSON.stringify(report)}\n`);
  process.exitCode = report.status === 'activated' ? 0 : 1;
}

let invokedAsScript = false;
try {
  invokedAsScript = Boolean(process.argv[1])
    && realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1]);
} catch { /* Missing entry paths do not turn library imports into execution. */ }
if (invokedAsScript) await main(process.argv.slice(2));
