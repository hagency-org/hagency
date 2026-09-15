import { createHash } from 'node:crypto';
import {
  closeSync,
  constants,
  fstatSync,
  fsyncSync,
  linkSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  realpathSync,
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

import { publishExclusiveJson, readReadonlyJson } from './native-control-evidence.mjs';

const SHA256 = /^[0-9a-f]{64}$/;
const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const PREDECESSORS = [
  'audit',
  'safe_state',
  'observer_binding',
  'observer_claim',
  'observer_ready',
  'watch_manifest',
  'watch_exit',
];
const PLAN_KEYS = ['attempt_id', 'base_control', 'predecessors', 'project', 'recovery_contract', 'trace', 'version'];
const CONTRACT_KEYS = [
  'attempt_id',
  'base_control',
  'goal',
  'predecessor_full_acceptance',
  'predecessors',
  'project',
  'stages',
  'version',
];
const PIN_KEYS = ['path', 'sha256'];
const GOAL_KEYS = ['created_at_ms', 'goal_id', 'objective_sha256'];
const SAFE_TRUE_FIELDS = [
  'goal_paused',
  'loops_empty',
  'all_turns_terminal',
  'observer_only_armed_no_fault',
  'stage34_absent',
];

class PreparationError extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

function fail(code) {
  throw new PreparationError(code);
}

function record(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function exactKeys(value, keys) {
  return record(value)
    && Object.keys(value).sort().join('\0') === [...keys].sort().join('\0');
}

function nonempty(value) {
  return typeof value === 'string' && value.length > 0;
}

function sameFile(left, right) {
  return left.dev === right.dev && left.ino === right.ino;
}

function validTraceIdentity(value) {
  return Array.isArray(value)
    && value.length === 2
    && Number.isSafeInteger(value[0])
    && value[0] >= 0
    && Number.isSafeInteger(value[1])
    && value[1] > 0;
}

function sameTraceIdentity(information, identity) {
  return information.dev === identity[0] && information.ino === identity[1];
}

function sha256(raw) {
  return createHash('sha256').update(raw).digest('hex');
}

function pathComponents(candidate) {
  const parsed = path.parse(candidate);
  const relative = candidate.slice(parsed.root.length);
  const components = relative === '' ? [] : relative.split(path.sep);
  const paths = [];
  let current = parsed.root;
  for (const component of components) {
    current = path.join(current, component);
    paths.push(current);
  }
  return paths;
}

function captureTrustedAncestry(candidate, code) {
  if (typeof candidate !== 'string'
    || !path.isAbsolute(candidate)
    || path.normalize(candidate) !== candidate) fail(code);
  const effectiveUid = typeof process.geteuid === 'function' ? process.geteuid() : undefined;
  if (!Number.isSafeInteger(effectiveUid)) fail(code);
  const candidates = [path.parse(candidate).root, ...pathComponents(candidate)];
  const entries = [];
  try {
    for (const current of candidates) {
      const information = lstatSync(current);
      if (!information.isDirectory()
        || information.isSymbolicLink()
        || ![0, effectiveUid].includes(information.uid)
        || ((information.mode & 0o022) !== 0 && (information.mode & 0o1000) === 0)) fail(code);
      entries.push({ path: current, information });
    }
    if (entries[0].information.uid !== 0 || realpathSync(candidate) !== candidate) fail(code);
    for (let index = 0; index < entries.length - 1; index++) {
      const parent = entries[index].information;
      if ((parent.mode & 0o022) !== 0
        && ![0, effectiveUid].includes(entries[index + 1].information.uid)) fail(code);
    }
  } catch (error) {
    if (error instanceof PreparationError) throw error;
    fail(code);
  }
  return entries;
}

function retainTrustedAncestry(expected, code) {
  const current = captureTrustedAncestry(expected.at(-1).path, code);
  if (current.length !== expected.length
    || current.some((entry, index) => (
      entry.path !== expected[index].path || !sameFile(entry.information, expected[index].information)
    ))) fail(code);
}

function canonicalExisting(candidate, kind, code) {
  if (typeof candidate !== 'string'
    || !path.isAbsolute(candidate)
    || path.normalize(candidate) !== candidate) fail(code);
  let information;
  try {
    for (const component of pathComponents(candidate)) {
      const componentInformation = lstatSync(component);
      if (componentInformation.isSymbolicLink()) fail(code);
      if (component === candidate) information = componentInformation;
    }
    if (realpathSync(candidate) !== candidate) fail(code);
  } catch {
    fail(code);
  }
  if (kind === 'directory' && !information?.isDirectory()) fail(code);
  if (kind === 'file' && !information?.isFile()) fail(code);
  return information;
}

function trustedPrivateDirectory(candidate, code, expected) {
  const information = canonicalExisting(candidate, 'directory', code);
  const effectiveUid = typeof process.geteuid === 'function' ? process.geteuid() : undefined;
  if (!Number.isSafeInteger(effectiveUid)
    || information.uid !== effectiveUid
    || (information.mode & 0o777) !== 0o700
    || (expected && !sameFile(information, expected))) fail(code);
  return information;
}

function within(candidate, parent) {
  return candidate === parent || candidate.startsWith(`${parent}${path.sep}`);
}

function validatePin(pin) {
  if (!exactKeys(pin, PIN_KEYS) || !nonempty(pin.path) || !SHA256.test(pin.sha256)) fail('invalid_lineage');
}

function validatePredecessorPins(pins) {
  if (!exactKeys(pins, PREDECESSORS)) fail('invalid_lineage');
  const paths = new Set();
  for (const name of PREDECESSORS) {
    validatePin(pins[name]);
    if (paths.has(pins[name].path)) fail('invalid_lineage');
    paths.add(pins[name].path);
  }
}

function readPinned(pin) {
  try {
    return readReadonlyJson(pin.path, pin.sha256).value;
  } catch {
    fail('invalid_lineage');
  }
}

function validatePlan(input) {
  let plan;
  try {
    plan = structuredClone(input);
  } catch {
    fail('invalid_plan');
  }
  if (!exactKeys(plan, PLAN_KEYS)
    || plan.version !== 1
    || !UUID_V4.test(plan.attempt_id)) fail('invalid_plan');
  const baseAncestry = captureTrustedAncestry(plan.base_control, 'invalid_control');
  const projectAncestry = captureTrustedAncestry(plan.project, 'invalid_control');
  const baseControl = trustedPrivateDirectory(plan.base_control, 'invalid_control');
  const project = canonicalExisting(plan.project, 'directory', 'invalid_control');
  if (within(plan.base_control, plan.project) || within(plan.project, plan.base_control)) fail('invalid_plan');
  validatePin(plan.recovery_contract);
  validatePredecessorPins(plan.predecessors);
  return { plan, baseAncestry, baseControl, project, projectAncestry };
}

function validateGoalContract(goal) {
  if (!exactKeys(goal, GOAL_KEYS)
    || !nonempty(goal.goal_id)
    || !Number.isSafeInteger(goal.created_at_ms)
    || goal.created_at_ms <= 0
    || !SHA256.test(goal.objective_sha256)) fail('invalid_lineage');
}

function goalFromSafeState(safeState) {
  if (!record(safeState)
    || safeState.full_acceptance !== false
    || SAFE_TRUE_FIELDS.some(field => safeState[field] !== true)
    || !record(safeState.owned_processes_present)
    || !Array.isArray(safeState.observations)
    || !validTraceIdentity(safeState.trace_identity)
    || !Number.isSafeInteger(safeState.trace_prefix_size)
    || safeState.trace_prefix_size <= 0
    || !SHA256.test(safeState.trace_prefix_sha256)) fail('invalid_lineage');
  for (const [key, value] of Object.entries(safeState)) {
    if (key !== 'full_acceptance' && typeof value === 'boolean' && value !== true) fail('invalid_lineage');
  }
  const goalReads = safeState.observations.filter(observation => (
    observation?.request?.frame?.method === 'session/goal/get'
  ));
  if (goalReads.length !== 1) fail('invalid_lineage');
  const observation = goalReads[0];
  const goal = observation?.response?.frame?.result?.goal;
  if (!record(goal)
    || observation.request.frame.id !== observation.response?.frame?.id
    || !nonempty(goal.goal_id)
    || !Number.isSafeInteger(goal.created_at_ms)
    || goal.created_at_ms <= 0
    || goal.status !== 'paused'
    || typeof goal.objective !== 'string') fail('invalid_lineage');
  return goal;
}

function validateHistoricalLineage(plan) {
  const contract = readPinned(plan.recovery_contract);
  if (!exactKeys(contract, CONTRACT_KEYS)
    || contract.version !== 1
    || contract.attempt_id !== plan.attempt_id
    || contract.base_control !== plan.base_control
    || contract.project !== plan.project
    || contract.predecessor_full_acceptance !== false
    || !isDeepStrictEqual(contract.stages, ['interrupt03', 'restart04'])) fail('invalid_lineage');
  validateGoalContract(contract.goal);
  validatePredecessorPins(contract.predecessors);
  if (!isDeepStrictEqual(contract.predecessors, plan.predecessors)) fail('invalid_lineage');

  const evidence = Object.fromEntries(PREDECESSORS.map(name => [name, readPinned(plan.predecessors[name])]));
  const audit = evidence.audit;
  if (!record(audit)
    || audit.old_watch_eligible_for_acceptance !== false
    || audit.full_acceptance !== false
    || audit.native_terminal_exit_code !== 3
    || audit.manifest_path !== plan.predecessors.watch_manifest.path
    || audit.manifest_sha256 !== plan.predecessors.watch_manifest.sha256
    || audit.actual_exit_path !== plan.predecessors.watch_exit.path
    || audit.actual_exit_sha256 !== plan.predecessors.watch_exit.sha256) fail('invalid_lineage');

  const watchManifest = evidence.watch_manifest;
  if (!record(watchManifest)
    || watchManifest.version !== 1
    || !nonempty(watchManifest.job_id)
    || watchManifest.manifest_path !== plan.predecessors.watch_manifest.path) fail('invalid_lineage');
  const watchExit = evidence.watch_exit;
  if (!record(watchExit)
    || watchExit.returncode !== 3
    || watchExit.manifest_unchanged !== true
    || watchExit.manifest_path !== plan.predecessors.watch_manifest.path
    || watchExit.manifest_sha256 !== plan.predecessors.watch_manifest.sha256
    || watchExit.job_id !== watchManifest.job_id) fail('invalid_lineage');

  const safeGoal = goalFromSafeState(evidence.safe_state);
  if (safeGoal.goal_id !== contract.goal.goal_id
    || safeGoal.created_at_ms !== contract.goal.created_at_ms
    || sha256(Buffer.from(safeGoal.objective, 'utf8')) !== contract.goal.objective_sha256) fail('invalid_lineage');

  const binding = evidence.observer_binding;
  if (!record(binding)
    || binding.version !== 1
    || binding.stage !== 'interrupt03'
    || binding.project !== plan.project
    || binding.control !== plan.base_control
    || binding.trace !== plan.trace
    || !validTraceIdentity(binding.trace_identity)
    || !Number.isSafeInteger(binding.trace_offset)
    || binding.trace_offset <= 0
    || !SHA256.test(binding.trace_prefix_sha256)
    || binding.goal_id !== contract.goal.goal_id
    || binding.goal_objective_sha256 !== contract.goal.objective_sha256) fail('invalid_lineage');

  const claim = evidence.observer_claim;
  const ready = evidence.observer_ready;
  if (!record(claim)
    || !record(ready)
    || !Number.isSafeInteger(claim.pid)
    || claim.pid <= 0
    || ready.pid !== claim.pid
    || !Number.isFinite(claim.armed_at)
    || claim.armed_at <= 0
    || ready.armed_at !== claim.armed_at
    || claim.stage !== 'interrupt03'
    || ready.stage !== 'interrupt03'
    || !validTraceIdentity(ready.trace_identity)
    || !SHA256.test(ready.trace_prefix_sha256)
    || !Object.hasOwn(evidence.safe_state.owned_processes_present, String(claim.pid))
    || evidence.safe_state.owned_processes_present[String(claim.pid)] !== null) fail('invalid_lineage');

  return { contract, evidence };
}

function stableFileRead(file, expected, code, afterOpen) {
  let descriptor;
  try {
    const initial = canonicalExisting(file, 'file', code);
    if (expected && !sameFile(initial, expected)) fail(code);
    descriptor = openSync(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const opened = fstatSync(descriptor);
    if (!opened.isFile() || !sameFile(initial, opened)) fail(code);
    afterOpen?.();
    const raw = readFileSync(descriptor);
    const final = fstatSync(descriptor);
    if (!sameFile(opened, final) || final.size !== opened.size || raw.length !== opened.size) fail(code);
    const retainedPath = canonicalExisting(file, 'file', code);
    if (!sameFile(opened, retainedPath)) fail(code);
    return { information: final, raw };
  } catch (error) {
    if (error instanceof PreparationError) throw error;
    fail(code);
  } finally {
    if (descriptor !== undefined) {
      try { closeSync(descriptor); } catch {}
    }
  }
}

function captureTrace(file, afterOpen) {
  const captured = stableFileRead(file, undefined, 'invalid_trace', afterOpen);
  if (captured.raw.length === 0 || captured.raw.at(-1) !== 0x0a) fail('invalid_trace');
  let text;
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(captured.raw);
    const lines = text.slice(0, -1).split('\n');
    if (lines.length === 0 || lines.some(line => line.length === 0)) fail('invalid_trace');
    for (const line of lines) {
      const value = JSON.parse(line);
      if (!record(value)) fail('invalid_trace');
    }
  } catch (error) {
    if (error instanceof PreparationError) throw error;
    fail('invalid_trace');
  }
  return {
    information: captured.information,
    raw: captured.raw,
    sha256: sha256(captured.raw),
  };
}

function validateTraceLineage(plan, lineage, captured) {
  const binding = lineage.evidence.observer_binding;
  const ready = lineage.evidence.observer_ready;
  const safeState = lineage.evidence.safe_state;
  if (binding.trace !== plan.trace
    || !sameTraceIdentity(captured.information, binding.trace_identity)
    || !sameTraceIdentity(captured.information, ready.trace_identity)
    || !sameTraceIdentity(captured.information, safeState.trace_identity)
    || binding.trace_offset > captured.raw.length
    || safeState.trace_prefix_size > captured.raw.length
    || sha256(captured.raw.subarray(0, binding.trace_offset)) !== binding.trace_prefix_sha256
    || ready.trace_prefix_sha256 !== binding.trace_prefix_sha256
    || sha256(captured.raw.subarray(0, safeState.trace_prefix_size)) !== safeState.trace_prefix_sha256) {
    fail('invalid_lineage');
  }
}

function verifyLinkedTrace(source, target, captured) {
  const retainedSource = stableFileRead(source, captured.information, 'trace_changed');
  const retainedLink = stableFileRead(target, captured.information, 'trace_changed');
  if (!sameFile(retainedSource.information, retainedLink.information)
    || retainedSource.raw.length < captured.raw.length
    || retainedLink.raw.length < captured.raw.length
    || !retainedSource.raw.subarray(0, captured.raw.length).equals(captured.raw)
    || !retainedLink.raw.subarray(0, captured.raw.length).equals(captured.raw)) fail('trace_changed');
  const finalSourcePath = canonicalExisting(source, 'file', 'trace_changed');
  const finalLinkPath = canonicalExisting(target, 'file', 'trace_changed');
  if (!sameFile(finalSourcePath, captured.information)
    || !sameFile(finalLinkPath, captured.information)) fail('trace_changed');
}

function syncDirectory(directory, code) {
  let descriptor;
  try {
    descriptor = openSync(directory, constants.O_RDONLY | (constants.O_DIRECTORY ?? 0));
    fsyncSync(descriptor);
  } catch {
    fail(code);
  } finally {
    if (descriptor !== undefined) {
      try { closeSync(descriptor); } catch {}
    }
  }
}

function bounded(status, reason, attemptPath, manifestPath) {
  return {
    version: 1,
    status,
    full_acceptance: false,
    ...(reason ? { reason } : {}),
    ...(attemptPath ? { attempt_path: attemptPath } : {}),
    ...(manifestPath ? { manifest_path: manifestPath } : {}),
  };
}

function retainControlDirectories(plan, directories) {
  retainTrustedAncestry(directories.baseAncestry, 'control_changed');
  retainTrustedAncestry(directories.projectAncestry, 'control_changed');
  trustedPrivateDirectory(plan.base_control, 'control_changed', directories.baseControl);
  const project = canonicalExisting(plan.project, 'directory', 'control_changed');
  if (!sameFile(project, directories.project)) fail('control_changed');
  trustedPrivateDirectory(directories.recoveryRootPath, 'control_changed', directories.recoveryRoot);
  trustedPrivateDirectory(directories.attemptPath, 'control_changed', directories.attempt);
}

export function prepareFaultAttempt(input, testOptions = {}) {
  let plan;
  let attemptPath;
  let manifestPath;
  try {
    if (!record(testOptions)
      || Object.keys(testOptions).some(key => !['afterLink', 'afterTraceOpen', 'link'].includes(key))
      || (testOptions.link !== undefined && typeof testOptions.link !== 'function')
      || (testOptions.afterTraceOpen !== undefined && typeof testOptions.afterTraceOpen !== 'function')
      || (testOptions.afterLink !== undefined && typeof testOptions.afterLink !== 'function')) fail('invalid_plan');
    const validated = validatePlan(input);
    plan = validated.plan;
    const recoveryRoot = path.join(plan.base_control, 'recovery-attempts');
    const candidatePath = path.join(recoveryRoot, plan.attempt_id);
    let recoveryRootExists = false;
    let recoveryRootInformation;
    try {
      lstatSync(recoveryRoot);
      recoveryRootExists = true;
    } catch (error) {
      if (error?.code !== 'ENOENT') fail('attempt_claim_failed');
    }
    if (recoveryRootExists) {
      recoveryRootInformation = trustedPrivateDirectory(recoveryRoot, 'invalid_control');
      try {
        lstatSync(candidatePath);
        return bounded('rejected', 'attempt_already_claimed', candidatePath);
      } catch (error) {
        if (error?.code !== 'ENOENT') fail('attempt_claim_failed');
      }
    }
    const lineage = validateHistoricalLineage(plan);
    const trace = captureTrace(plan.trace, () => testOptions.afterTraceOpen?.({ source: plan.trace }));
    validateTraceLineage(plan, lineage, trace);
    retainTrustedAncestry(validated.baseAncestry, 'invalid_control');
    retainTrustedAncestry(validated.projectAncestry, 'invalid_control');
    trustedPrivateDirectory(plan.base_control, 'invalid_control', validated.baseControl);
    const retainedProject = canonicalExisting(plan.project, 'directory', 'invalid_control');
    if (!sameFile(retainedProject, validated.project)) fail('invalid_control');
    if (recoveryRootInformation) {
      trustedPrivateDirectory(recoveryRoot, 'invalid_control', recoveryRootInformation);
    } else {
      try {
        mkdirSync(recoveryRoot, { mode: 0o700 });
        syncDirectory(plan.base_control, 'attempt_claim_failed');
      } catch (error) {
        if (error?.code !== 'EEXIST') throw error;
      }
      recoveryRootInformation = trustedPrivateDirectory(recoveryRoot, 'invalid_control');
    }
    retainTrustedAncestry(validated.baseAncestry, 'invalid_control');
    retainTrustedAncestry(validated.projectAncestry, 'invalid_control');
    trustedPrivateDirectory(plan.base_control, 'invalid_control', validated.baseControl);
    const projectBeforeClaim = canonicalExisting(plan.project, 'directory', 'invalid_control');
    if (!sameFile(projectBeforeClaim, validated.project)) fail('invalid_control');
    trustedPrivateDirectory(recoveryRoot, 'invalid_control', recoveryRootInformation);
    try {
      mkdirSync(candidatePath, { mode: 0o700 });
    } catch (error) {
      if (error?.code === 'EEXIST') return bounded('rejected', 'attempt_already_claimed', candidatePath);
      fail('attempt_claim_failed');
    }
    attemptPath = candidatePath;
    const attemptInformation = trustedPrivateDirectory(attemptPath, 'control_changed');
    syncDirectory(recoveryRoot, 'attempt_claim_failed');
    const directories = {
      baseAncestry: validated.baseAncestry,
      baseControl: validated.baseControl,
      project: validated.project,
      projectAncestry: validated.projectAncestry,
      recoveryRoot: recoveryRootInformation,
      recoveryRootPath: recoveryRoot,
      attempt: attemptInformation,
      attemptPath,
    };
    retainControlDirectories(plan, directories);

    manifestPath = path.join(attemptPath, 'attempt.json');
    const linkedTrace = path.join(attemptPath, 'trace.jsonl');
    const traceFence = {
      source_path: plan.trace,
      linked_path: linkedTrace,
      device: trace.information.dev,
      inode: trace.information.ino,
      prefix_size: trace.raw.length,
      prefix_sha256: trace.sha256,
    };
    try {
      publishExclusiveJson(manifestPath, {
        version: 1,
        attempt_id: plan.attempt_id,
        status: 'claimed',
        full_acceptance: false,
        predecessor_full_acceptance: false,
        base_control: plan.base_control,
        project: plan.project,
        recovery_contract: plan.recovery_contract,
        predecessors: plan.predecessors,
        goal: lineage.contract.goal,
        stages: lineage.contract.stages,
        trace_fence: traceFence,
      });
    } catch {
      manifestPath = undefined;
      fail('manifest_publication_failed');
    }
    retainControlDirectories(plan, directories);

    try {
      (testOptions.link ?? linkSync)(plan.trace, linkedTrace);
    } catch {
      fail('trace_link_failed');
    }
    try {
      testOptions.afterLink?.({ source: plan.trace, target: linkedTrace });
    } catch {
      fail('trace_changed');
    }
    retainControlDirectories(plan, directories);
    verifyLinkedTrace(plan.trace, linkedTrace, trace);
    retainControlDirectories(plan, directories);
    verifyLinkedTrace(plan.trace, linkedTrace, trace);

    try {
      publishExclusiveJson(path.join(attemptPath, 'prepared.json'), {
        version: 1,
        attempt_id: plan.attempt_id,
        status: 'prepared',
        full_acceptance: false,
        predecessor_full_acceptance: false,
        attempt_path: attemptPath,
        manifest_path: manifestPath,
        trace_path: linkedTrace,
        same_inode: true,
        trace_fence: traceFence,
      });
    } catch {
      fail('prepared_publication_failed');
    }
    retainControlDirectories(plan, directories);
    verifyLinkedTrace(plan.trace, linkedTrace, trace);
    return bounded('prepared', undefined, attemptPath, manifestPath);
  } catch (error) {
    const reason = error instanceof PreparationError ? error.code : 'invalid_plan';
    return bounded('failed', reason, attemptPath, manifestPath);
  }
}

function main(args) {
  let report;
  try {
    if (args.length !== 4
      || args[0] !== '--plan'
      || args[2] !== '--sha256'
      || !SHA256.test(args[3])) fail('invalid_plan');
    const loaded = readReadonlyJson(args[1], args[3]);
    report = prepareFaultAttempt(loaded.value);
  } catch {
    report = bounded('failed', 'invalid_plan');
  }
  process.stdout.write(`${JSON.stringify(report)}\n`);
  process.exitCode = report.status === 'prepared' ? 0 : 1;
}

let invokedAsScript = false;
try {
  invokedAsScript = Boolean(process.argv[1])
    && realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1]);
} catch { /* A missing entry path does not turn a library import into execution. */ }
if (invokedAsScript) {
  main(process.argv.slice(2));
}
