import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import {
  closeSync, constants, fstatSync, lstatSync, openSync, readFileSync, readlinkSync, readSync, realpathSync,
} from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { isDeepStrictEqual, promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import {
  assertProcessIdentity,
} from './native-control-evidence.mjs';

const execute = promisify(execFile);
const SHA = /^[0-9a-f]{64}$/;
const UUID4_LOWER = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const UUID4_FILE = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const STARTED = /^(?:0|[1-9]\d{0,18})\.\d{6}$/;
const LOOP_ID = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;
const OBSERVER_LIFETIME_SECONDS = 1200;
const MAX_FILE_BYTES = 128 * 1024 * 1024;
const MAX_CHILD_OUTPUT_BYTES = 16 * 1024 * 1024;
const STAGE_NUMBER = { interrupt03: '03', restart04: '04' };
const PREDECESSORS = [
  'audit', 'safe_state', 'observer_binding', 'observer_claim', 'observer_ready', 'watch_manifest', 'watch_exit',
];
const BASE_PLAN_KEYS = [
  'version', 'activation_id', 'stage', 'attempt_manifest', 'prepared', 'observer_binding', 'observer_claim',
  'observer_ready', 'observer', 'metadata_tool', 'frozen_helpers', 'protected', 'binding', 'expected_goal',
  'child_operations', 'prerequisite_pins', 'controller_tools', 'query_timeout_ms', 'activation_timeout_ms',
  'observer_timeout_ms', 'settlement_timeout_ms', 'settlement_interval_ms',
];
const BINDING_KEYS = [
  'version', 'project', 'trace', 'herdr', 'birth_tool', 'frontend_binary', 'herdr_session', 'lower_agent',
  'pane_id', 'terminal_id', 'shell_pid', 'profile', 'native_session', 'frontend', 'backend',
];
const FROZEN_KEYS = [
  'version', 'stage', 'project', 'control', 'instance', 'profile', 'lower_agent', 'herdr_session',
  'native_session', 'pane_id', 'shell_pid', 'goal_id', 'goal_objective_sha256', 'frontend', 'backend',
  'frontend_binary', 'frontend_binary_sha256', 'herdr', 'herdr_sha256', 'birth_tool', 'birth_tool_sha256',
  'trace', 'trace_identity', 'trace_offset', 'trace_prefix_sha256', 'protected',
];

class EvidenceError extends Error {
  constructor(category = 'invalid_native_stage_evidence') { super(category); this.code = category; }
}

function requireEvidence(value, category = 'invalid_native_stage_evidence') {
  if (!value) throw new EvidenceError(category);
}

function record(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    && [Object.prototype, null].includes(Object.getPrototypeOf(value));
}

function exactKeys(value, keys) {
  return record(value) && Object.keys(value).sort().join('\0') === [...keys].sort().join('\0');
}

function publicText(value) {
  return typeof value === 'string' && value.length > 0 && value.length <= 4096
    && !/[\x00-\x1f\x7f]/.test(value);
}

function bindingText(value) {
  return publicText(value) && !value.startsWith('-');
}

function digest(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function sameFile(left, right) {
  return left.dev === right.dev && left.ino === right.ino;
}

function pathParts(candidate) {
  const parsed = path.parse(candidate);
  const remainder = candidate.slice(parsed.root.length);
  const parts = remainder === '' ? [] : remainder.split(path.sep);
  let current = parsed.root;
  return parts.map(part => (current = path.join(current, part)));
}

function canonical(candidate, kind, category = 'invalid_evidence_path') {
  requireEvidence(publicText(candidate) && path.isAbsolute(candidate)
    && path.normalize(candidate) === candidate, category);
  let final;
  try {
    if (candidate === path.parse(candidate).root) final = lstatSync(candidate);
    for (const current of pathParts(candidate)) {
      const information = lstatSync(current);
      requireEvidence(!information.isSymbolicLink(), category);
      if (current === candidate) final = information;
    }
    requireEvidence(realpathSync(candidate) === candidate, category);
  } catch (error) {
    if (error instanceof EvidenceError) throw error;
    throw new EvidenceError(category);
  }
  requireEvidence(kind === 'directory' ? final?.isDirectory() : final?.isFile(), category);
  return final;
}

function absent(candidate, category = 'stage_already_consumed') {
  try { lstatSync(candidate); } catch (error) {
    if (error?.code === 'ENOENT') return;
    throw new EvidenceError(category);
  }
  throw new EvidenceError(category);
}

function observerInstancePath(candidate) {
  const category = 'observer_binding_changed';
  if (process.platform !== 'darwin' || typeof candidate !== 'string' || !candidate.startsWith('/tmp/')) {
    canonical(candidate, 'directory', category);
    return candidate;
  }
  requireEvidence(publicText(candidate) && path.normalize(candidate) === candidate, category);
  try {
    const before = lstatSync('/tmp', { bigint: true });
    const target = readlinkSync('/tmp');
    requireEvidence(before.isSymbolicLink() && before.uid === 0n
      && path.resolve('/', target) === '/private/tmp'
      && realpathSync('/tmp') === '/private/tmp', category);
    const mapped = `/private${candidate}`;
    canonical(mapped, 'directory', category);
    requireEvidence(realpathSync(candidate) === mapped, category);
    const retained = lstatSync('/tmp', { bigint: true });
    requireEvidence(retained.isSymbolicLink() && retained.uid === 0n
      && sameFile(before, retained) && readlinkSync('/tmp') === target, category);
    return mapped;
  } catch (error) {
    if (error instanceof EvidenceError) throw error;
    throw new EvidenceError(category);
  }
}

function validatePin(pin) {
  requireEvidence(exactKeys(pin, ['path', 'sha256']) && typeof pin.path === 'string'
    && SHA.test(pin.sha256), 'invalid_evidence_pin');
}

function readPinnedBytes(pin, { readonly = true, executable = false } = {}) {
  validatePin(pin);
  const initial = canonical(pin.path, 'file', 'changed_evidence_pin');
  const acceptable = information => information.size <= MAX_FILE_BYTES
    && (!readonly || (information.mode & 0o222) === 0)
    && (!executable || (information.mode & 0o111) !== 0);
  requireEvidence(acceptable(initial), 'changed_evidence_pin');
  let descriptor;
  try {
    descriptor = openSync(pin.path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const before = fstatSync(descriptor);
    const raw = readFileSync(descriptor);
    const after = fstatSync(descriptor);
    const retained = canonical(pin.path, 'file', 'changed_evidence_pin');
    requireEvidence(sameFile(initial, before) && sameFile(before, after) && sameFile(after, retained)
      && [before, after, retained].every(acceptable)
      && before.mode === initial.mode && after.mode === before.mode && retained.mode === after.mode
      && before.uid === initial.uid && after.uid === before.uid && retained.uid === after.uid
      && before.gid === initial.gid && after.gid === before.gid && retained.gid === after.gid
      && before.size === after.size && raw.length === after.size
      && before.mtimeMs === after.mtimeMs && digest(raw) === pin.sha256, 'changed_evidence_pin');
    return raw;
  } catch (error) {
    if (error instanceof EvidenceError) throw error;
    throw new EvidenceError('changed_evidence_pin');
  } finally {
    if (descriptor !== undefined) try { closeSync(descriptor); } catch { /* best effort close */ }
  }
}

function readPinnedJson(pin) {
  const raw = readPinnedBytes(pin);
  try { return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(raw)); }
  catch { throw new EvidenceError('changed_evidence_pin'); }
}

function readReadonlyBoundedJson(file, category) {
  const initial = canonical(file, 'file', category);
  requireEvidence((initial.mode & 0o222) === 0 && initial.size <= MAX_FILE_BYTES, category);
  let descriptor;
  try {
    descriptor = openSync(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const before = fstatSync(descriptor), raw = readFileSync(descriptor), after = fstatSync(descriptor);
    requireEvidence(sameFile(initial, before) && sameFile(before, after) && sameFile(after, lstatSync(file))
      && before.size === after.size && after.size <= MAX_FILE_BYTES && raw.length === after.size
      && (after.mode & 0o222) === 0, category);
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(raw));
  } catch (error) {
    if (error instanceof EvidenceError) throw error;
    throw new EvidenceError(category);
  } finally {
    if (descriptor !== undefined) try { closeSync(descriptor); } catch { /* best effort close */ }
  }
}

function within(root, candidate) {
  return candidate.startsWith(`${root}${path.sep}`);
}

function relativeFile(project, relative) {
  requireEvidence(typeof relative === 'string' && relative !== '' && !relative.includes('\\')
    && !path.posix.isAbsolute(relative) && path.posix.normalize(relative) === relative
    && relative.split('/').every(part => part !== '' && part !== '.' && part !== '..'), 'invalid_protected_set');
  const result = path.join(project, ...relative.split('/'));
  requireEvidence(within(project, result), 'invalid_protected_set');
  return result;
}

function validProcess(value) {
  return exactKeys(value, ['pid', 'ppid', 'pgid', 'started', 'cwd', 'argv'])
    && Number.isSafeInteger(value.pid) && value.pid > 1
    && Number.isSafeInteger(value.ppid) && value.ppid >= 0
    && Number.isSafeInteger(value.pgid) && value.pgid >= 0
    && typeof value.started === 'string' && STARTED.test(value.started) && value.started !== '0.000000'
    && typeof value.cwd === 'string' && value.cwd !== ''
    && Array.isArray(value.argv) && value.argv.length > 0 && value.argv.every(token => typeof token === 'string');
}

function validGoal(goal) {
  return exactKeys(goal, ['goal_id', 'created_at_ms', 'objective_sha256'])
    && publicText(goal.goal_id)
    && Number.isSafeInteger(goal.created_at_ms) && goal.created_at_ms > 0
    && SHA.test(goal.objective_sha256);
}

function validLoop(loop) {
  return exactKeys(loop, ['loop_id', 'created_at_ms', 'prompt_sha256', 'mode', 'interval_seconds'])
    && typeof loop.loop_id === 'string' && LOOP_ID.test(loop.loop_id)
    && Number.isSafeInteger(loop.created_at_ms) && loop.created_at_ms > 0
    && SHA.test(loop.prompt_sha256) && loop.mode === 'fixed_interval' && loop.interval_seconds === 60;
}

function validateOperationIds(plan) {
  const interruptKeys = ['initial_inspect', 'goal_resume', 'final_inspect'];
  const restartKeys = [...interruptKeys, 'loop_resume', 'settlement_inspects'];
  const expected = plan.stage === 'restart04' ? restartKeys : interruptKeys;
  requireEvidence(exactKeys(plan.child_operations, expected), 'invalid_child_operations');
  const scalar = expected.filter(key => key !== 'settlement_inspects').map(key => plan.child_operations[key]);
  const settlement = plan.child_operations.settlement_inspects ?? [];
  requireEvidence(Array.isArray(settlement) && (plan.stage !== 'restart04'
    || (settlement.length >= 1 && settlement.length <= 128)), 'invalid_child_operations');
  const all = [plan.activation_id, ...scalar, ...settlement];
  requireEvidence(all.every(value => typeof value === 'string' && UUID4_LOWER.test(value))
    && new Set(all).size === all.length, 'invalid_child_operations');
}

function validateTimeouts(plan) {
  for (const key of ['query_timeout_ms', 'observer_timeout_ms']) {
    requireEvidence(Number.isSafeInteger(plan[key]) && plan[key] >= 100 && plan[key] <= 10_000,
      'invalid_stage_timeout');
  }
  requireEvidence(Number.isSafeInteger(plan.activation_timeout_ms) && plan.activation_timeout_ms >= 5000
    && plan.activation_timeout_ms <= 240_000
    && Number.isSafeInteger(plan.settlement_timeout_ms) && plan.settlement_timeout_ms >= 100
    && plan.settlement_timeout_ms <= 120_000
    && Number.isSafeInteger(plan.settlement_interval_ms) && plan.settlement_interval_ms >= 100
    && plan.settlement_interval_ms <= 5000, 'invalid_stage_timeout');
}

function validateBindingShape(binding) {
  requireEvidence(exactKeys(binding, BINDING_KEYS) && binding.version === 1, 'invalid_native_binding');
  canonical(binding.project, 'directory', 'invalid_native_binding');
  canonical(binding.trace, 'file', 'invalid_native_binding');
  requireEvidence(binding.native_session === `${binding.profile}:local:tui#coding`
    && ['profile', 'lower_agent', 'herdr_session', 'pane_id', 'terminal_id', 'native_session']
      .every(key => bindingText(binding[key]))
    && typeof binding.pane_id === 'string' && /^w[A-Za-z0-9]+:p[A-Za-z0-9]+$/.test(binding.pane_id)
    && typeof binding.terminal_id === 'string' && binding.terminal_id !== ''
    && Number.isSafeInteger(binding.shell_pid) && binding.shell_pid > 1
    && validProcess(binding.frontend) && validProcess(binding.backend), 'invalid_native_binding');
  for (const key of ['herdr', 'birth_tool', 'frontend_binary']) validatePin(binding[key]);
  const front = binding.frontend, back = binding.backend;
  requireEvidence(front.cwd === binding.project && back.cwd === binding.project
    && front.ppid === binding.shell_pid && back.ppid === front.pid && front.pgid === back.pgid
    && new Set([binding.shell_pid, front.pid, back.pid]).size === 3
    && front.argv[0] === binding.frontend_binary.path && path.basename(front.argv[0]) === 'octoscode'
    && path.basename(back.argv[0]) === 'octos', 'invalid_native_binding');
}

function exactFlag(argv, flag, value) {
  const positions = argv.map((token, index) => token === flag ? index : -1).filter(index => index >= 0);
  return positions.length === 1 && (value === undefined || argv[positions[0] + 1] === value);
}

function flagValue(argv, flag) {
  const index = argv.indexOf(flag);
  return index >= 0 && index + 1 < argv.length ? argv[index + 1] : undefined;
}

function readTracePrefix(trace, length, expected) {
  requireEvidence(Number.isSafeInteger(length) && length >= 0 && SHA.test(expected), 'invalid_trace_fence');
  let descriptor;
  try {
    descriptor = openSync(trace, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const information = fstatSync(descriptor);
    requireEvidence(information.size >= length && information.size <= MAX_FILE_BYTES
      && length <= MAX_FILE_BYTES, 'invalid_trace_fence');
    const bytes = Buffer.alloc(length);
    let offset = 0;
    while (offset < length) {
      const count = readSync(descriptor, bytes, offset, length - offset, offset);
      requireEvidence(count > 0, 'invalid_trace_fence');
      offset += count;
    }
    const after = fstatSync(descriptor);
    requireEvidence(sameFile(information, after) && after.size >= length && after.size <= MAX_FILE_BYTES
      && sameFile(after, lstatSync(trace)) && digest(bytes) === expected, 'invalid_trace_fence');
    if (length > 0) {
      const records = new TextDecoder('utf-8', { fatal: true }).decode(bytes).split('\n');
      requireEvidence(records.at(-1) === '', 'invalid_trace_fence');
      for (const line of records.slice(0, -1)) requireEvidence(record(JSON.parse(line)), 'invalid_trace_fence');
    }
  } catch (error) {
    if (error instanceof EvidenceError) throw error;
    throw new EvidenceError('invalid_trace_fence');
  } finally {
    if (descriptor !== undefined) try { closeSync(descriptor); } catch { /* best effort close */ }
  }
}

function protectedShape(plan, frozen) {
  const count = plan.stage === 'interrupt03' ? 2 : 3;
  const expectedAll = new Map();
  const expectedObserver = new Map();
  const protectedPaths = [];
  for (let increment = 1; increment <= count; increment += 1) {
    const number = String(increment).padStart(2, '0');
    const checkpointRelative = `.autonomy/checkpoints/${number}.json`;
    const redGreenRelative = `.autonomy/red-green/${number}.json`;
    const checkpoint = readReadonlyBoundedJson(relativeFile(plan.binding.project, checkpointRelative), 'invalid_protected_set');
    const redGreen = readReadonlyBoundedJson(relativeFile(plan.binding.project, redGreenRelative), 'invalid_protected_set');
    requireEvidence(checkpoint.increment === increment
      && checkpoint.sourceSnapshot === `.autonomy/sources/${number}.js`
      && typeof checkpoint.testOutput === 'string'
      && /^\.autonomy\/test-outputs\/[0-9a-f-]+\.json$/i.test(checkpoint.testOutput)
      && UUID4_FILE.test(path.posix.basename(checkpoint.testOutput, '.json'))
      && redGreen.increment === increment,
    'invalid_protected_set');
    const captures = [redGreen.red_capture_path, redGreen.green_capture_path];
    for (const capture of captures) {
      requireEvidence(typeof capture === 'string' && path.isAbsolute(capture)
        && path.normalize(capture) === capture && within(plan.binding.project, capture), 'invalid_protected_set');
      const relative = path.relative(plan.binding.project, capture).split(path.sep).join('/');
      const match = /^\.hagency-test-evidence\/capture-([0-9a-f-]+)-([0-9a-f]{64})\.json$/i.exec(relative);
      requireEvidence(match && UUID4_FILE.test(match[1]) && plan.protected[relative] === match[2], 'invalid_protected_set');
      protectedPaths.push(relative);
      expectedAll.set(relative, plan.protected[relative]);
    }
    for (const relative of [checkpointRelative, checkpoint.sourceSnapshot, checkpoint.testOutput]) {
      requireEvidence(SHA.test(plan.protected[relative]), 'invalid_protected_set');
      protectedPaths.push(relative);
      expectedAll.set(relative, plan.protected[relative]);
      expectedObserver.set(relative, plan.protected[relative]);
    }
    requireEvidence(SHA.test(plan.protected[redGreenRelative]), 'invalid_protected_set');
    protectedPaths.push(redGreenRelative);
    expectedAll.set(redGreenRelative, plan.protected[redGreenRelative]);
  }
  requireEvidence(protectedPaths.length === 6 * count && new Set(protectedPaths).size === protectedPaths.length
    && expectedAll.size === 6 * count
    && isDeepStrictEqual(Object.fromEntries(expectedAll), plan.protected)
    && isDeepStrictEqual(Object.fromEntries(expectedObserver), frozen.protected), 'invalid_protected_set');
}

function validateLineage(plan, attemptPath) {
  const manifest = readPinnedJson(plan.attempt_manifest);
  const prepared = readPinnedJson(plan.prepared);
  requireEvidence(plan.attempt_manifest.path === path.join(attemptPath, 'attempt.json')
    && plan.prepared.path === path.join(attemptPath, 'prepared.json')
    && manifest.version === 1 && manifest.status === 'claimed' && manifest.full_acceptance === false
    && manifest.predecessor_full_acceptance === false && manifest.attempt_id === path.basename(attemptPath)
    && manifest.project === plan.binding.project
    && manifest.base_control === path.dirname(path.dirname(attemptPath))
    && isDeepStrictEqual(manifest.goal, plan.expected_goal)
    && isDeepStrictEqual(manifest.stages, ['interrupt03', 'restart04'])
    && prepared.version === 1 && prepared.status === 'prepared' && prepared.full_acceptance === false
    && prepared.predecessor_full_acceptance === false && prepared.attempt_id === manifest.attempt_id
    && prepared.attempt_path === attemptPath && prepared.manifest_path === plan.attempt_manifest.path
    && prepared.trace_path === plan.binding.trace && prepared.same_inode === true
    && isDeepStrictEqual(prepared.trace_fence, manifest.trace_fence), 'invalid_attempt_lineage');
  const contract = readPinnedJson(manifest.recovery_contract);
  requireEvidence(contract.version === 1 && contract.attempt_id === manifest.attempt_id
    && contract.base_control === manifest.base_control && contract.project === manifest.project
    && contract.predecessor_full_acceptance === false && isDeepStrictEqual(contract.goal, plan.expected_goal)
    && isDeepStrictEqual(contract.stages, manifest.stages)
    && exactKeys(manifest.predecessors, PREDECESSORS)
    && isDeepStrictEqual(contract.predecessors, manifest.predecessors), 'invalid_attempt_lineage');
  const values = Object.fromEntries(PREDECESSORS.map(name => [name, readPinnedJson(manifest.predecessors[name])]));
  const watch = values.watch_manifest, exit = values.watch_exit, audit = values.audit;
  requireEvidence(watch.version === 1 && typeof watch.job_id === 'string' && watch.job_id !== ''
    && watch.manifest_path === manifest.predecessors.watch_manifest.path
    && Number.isSafeInteger(watch.deadline_at) && watch.deadline_at > 0
    && exit.returncode === 3 && exit.manifest_unchanged === true && exit.job_id === watch.job_id
    && exit.manifest_path === manifest.predecessors.watch_manifest.path
    && exit.manifest_sha256 === manifest.predecessors.watch_manifest.sha256
    && audit.old_watch_eligible_for_acceptance === false && audit.full_acceptance === false
    && audit.native_terminal_exit_code === 3 && audit.manifest_path === exit.manifest_path
    && audit.manifest_sha256 === exit.manifest_sha256
    && Number.isSafeInteger(audit.immutable_deadline_at) && audit.immutable_deadline_at === watch.deadline_at
    && audit.actual_exit_path === manifest.predecessors.watch_exit.path
    && audit.actual_exit_sha256 === manifest.predecessors.watch_exit.sha256, 'invalid_attempt_lineage');
  const safe = values.safe_state, oldBinding = values.observer_binding;
  const goalReads = Array.isArray(safe.observations) ? safe.observations.filter(observation => (
    observation?.request?.frame?.method === 'session/goal/get'
  )) : [];
  const safeGoal = goalReads[0]?.response?.frame?.result?.goal;
  requireEvidence(safe.full_acceptance === false && safe.goal_paused === true && safe.loops_empty === true
    && safe.all_turns_terminal === true && safe.observer_only_armed_no_fault === true && safe.stage34_absent === true
    && oldBinding.version === 1 && oldBinding.stage === 'interrupt03'
    && oldBinding.project === plan.binding.project && oldBinding.control === manifest.base_control
    && oldBinding.trace === manifest.trace_fence.source_path
    && oldBinding.goal_id === plan.expected_goal.goal_id
    && oldBinding.goal_objective_sha256 === plan.expected_goal.objective_sha256
    && isDeepStrictEqual(safe.trace_identity, oldBinding.trace_identity)
    && record(safe.owned_processes_present)
    && Object.hasOwn(safe.owned_processes_present, String(values.observer_claim.pid))
    && safe.owned_processes_present[String(values.observer_claim.pid)] === null
    && goalReads.length === 1
    && goalReads[0].request.frame.id === goalReads[0].response?.frame?.id
    && safeGoal?.goal_id === plan.expected_goal.goal_id
    && safeGoal.created_at_ms === plan.expected_goal.created_at_ms
    && safeGoal.status === 'paused' && typeof safeGoal.objective === 'string'
    && digest(Buffer.from(safeGoal.objective)) === plan.expected_goal.objective_sha256
    && safe.trace_prefix_size >= oldBinding.trace_offset
    && SHA.test(safe.trace_prefix_sha256) && SHA.test(oldBinding.trace_prefix_sha256), 'invalid_attempt_lineage');
  const oldClaim = values.observer_claim, oldReady = values.observer_ready;
  requireEvidence(oldClaim.stage === 'interrupt03' && oldReady.stage === 'interrupt03'
    && oldClaim.pid === oldReady.pid && oldClaim.armed_at === oldReady.armed_at
    && Array.isArray(oldReady.trace_identity) && oldReady.trace_identity.length === 2
    && oldReady.trace_identity.every(Number.isSafeInteger) && SHA.test(oldReady.trace_prefix_sha256),
  'invalid_attempt_lineage');
  return { manifest, prepared, predecessors: values };
}

function validateTrace(plan, manifest, predecessors) {
  const fence = manifest.trace_fence;
  requireEvidence(exactKeys(fence, ['source_path', 'linked_path', 'device', 'inode', 'prefix_size', 'prefix_sha256'])
    && fence.linked_path === plan.binding.trace
    && fence.linked_path === path.join(path.dirname(plan.attempt_manifest.path), 'trace.jsonl')
    && fence.source_path !== fence.linked_path, 'invalid_trace_fence');
  const source = canonical(fence.source_path, 'file', 'invalid_trace_fence');
  const linked = canonical(fence.linked_path, 'file', 'invalid_trace_fence');
  requireEvidence(sameFile(source, linked) && source.dev === fence.device && source.ino === fence.inode
    && isDeepStrictEqual(predecessors.observer_binding.trace_identity, [source.dev, source.ino])
    && isDeepStrictEqual(predecessors.safe_state.trace_identity, [source.dev, source.ino]), 'invalid_trace_fence');
  const current = readReadonlyBoundedJson(plan.observer_binding.path, 'observer_binding_changed');
  requireEvidence(isDeepStrictEqual(current.trace_identity, [source.dev, source.ino])
    && current.trace_offset >= fence.prefix_size, 'invalid_trace_fence');
  readTracePrefix(fence.source_path, predecessors.observer_binding.trace_offset,
    predecessors.observer_binding.trace_prefix_sha256);
  readTracePrefix(fence.source_path, predecessors.safe_state.trace_prefix_size,
    predecessors.safe_state.trace_prefix_sha256);
  readTracePrefix(fence.source_path, fence.prefix_size, fence.prefix_sha256);
  readTracePrefix(plan.binding.trace, current.trace_offset, current.trace_prefix_sha256);
}

function validateObserverFiles(plan, observerRoot) {
  const launch = readPinnedJson(plan.observer_binding);
  const frozen = readReadonlyBoundedJson(path.join(observerRoot, 'binding.json'), 'observer_binding_changed');
  const frozenKeys = plan.stage === 'restart04'
    ? [...FROZEN_KEYS, 'loop_id', 'loop_interval_seconds', 'loop_prompt_sha256'] : FROZEN_KEYS;
  requireEvidence(within(path.dirname(plan.attempt_manifest.path), plan.observer_binding.path)
    && exactKeys(frozen, frozenKeys) && isDeepStrictEqual(launch, frozen)
    && frozen.version === 1 && frozen.stage === plan.stage && frozen.control === path.dirname(plan.attempt_manifest.path)
    && frozen.project === plan.binding.project && frozen.trace === plan.binding.trace
    && observerInstancePath(frozen.instance)
    && frozen.profile === plan.binding.profile && frozen.lower_agent === plan.binding.lower_agent
    && frozen.herdr_session === plan.binding.herdr_session && frozen.native_session === plan.binding.native_session
    && frozen.pane_id === plan.binding.pane_id && frozen.shell_pid === plan.binding.shell_pid
    && isDeepStrictEqual(frozen.frontend, plan.binding.frontend)
    && isDeepStrictEqual(frozen.backend, plan.binding.backend)
    && frozen.frontend_binary === plan.binding.frontend_binary.path
    && frozen.frontend_binary_sha256 === plan.binding.frontend_binary.sha256
    && frozen.herdr === plan.binding.herdr.path && frozen.herdr_sha256 === plan.binding.herdr.sha256
    && frozen.birth_tool === plan.binding.birth_tool.path && frozen.birth_tool_sha256 === plan.binding.birth_tool.sha256
    && frozen.goal_id === plan.expected_goal.goal_id
    && frozen.goal_objective_sha256 === plan.expected_goal.objective_sha256
    && Array.isArray(frozen.trace_identity) && frozen.trace_identity.length === 2
    && frozen.trace_identity.every(Number.isSafeInteger)
    && Number.isSafeInteger(frozen.trace_offset) && frozen.trace_offset >= 0
    && SHA.test(frozen.trace_prefix_sha256), 'observer_binding_changed');
  requireEvidence(exactFlag(frozen.frontend.argv, '--profile-id', frozen.profile)
    && exactFlag(frozen.frontend.argv, '--cwd', frozen.project)
    && exactFlag(frozen.frontend.argv, '--mode', 'protocol')
    && exactFlag(frozen.frontend.argv, '--no-splash')
    && exactFlag(frozen.frontend.argv, '--lang', 'en')
    && exactFlag(frozen.frontend.argv, '--stdio-command')
    && exactFlag(frozen.backend.argv, '--instance-data-dir', frozen.instance)
    && exactFlag(frozen.backend.argv, '--cwd', frozen.project)
    && exactFlag(frozen.backend.argv, '--data-dir') && exactFlag(frozen.backend.argv, '--config')
    && exactFlag(frozen.backend.argv, '--stdio') && exactFlag(frozen.backend.argv, '--solo')
    && exactFlag(frozen.backend.argv, '--no-network') && frozen.backend.argv[1] === 'serve'
    && typeof flagValue(frozen.frontend.argv, '--stdio-command') === 'string'
    && flagValue(frozen.frontend.argv, '--stdio-command').includes(' ')
    && typeof flagValue(frozen.backend.argv, '--data-dir') === 'string'
    && typeof flagValue(frozen.backend.argv, '--config') === 'string',
  'observer_binding_changed');
  canonical(flagValue(frozen.backend.argv, '--data-dir'), 'directory', 'observer_binding_changed');
  canonical(flagValue(frozen.backend.argv, '--config'), 'file', 'observer_binding_changed');
  if (plan.stage === 'restart04') {
    requireEvidence(frozen.loop_id === plan.expected_loop.loop_id
      && frozen.loop_interval_seconds === plan.expected_loop.interval_seconds
      && frozen.loop_prompt_sha256 === plan.expected_loop.prompt_sha256, 'observer_binding_changed');
  }
  const claim = readPinnedJson(plan.observer_claim), ready = readPinnedJson(plan.observer_ready);
  requireEvidence(plan.observer_claim.path === path.join(observerRoot, 'claim.json')
    && plan.observer_ready.path === path.join(observerRoot, 'ready.json')
    && exactKeys(claim, ['stage', 'armed_at', 'pid'])
    && exactKeys(ready, ['stage', 'armed_at', 'pid', 'trace_identity', 'trace_prefix_sha256'])
    && claim.stage === plan.stage && ready.stage === plan.stage && claim.pid === plan.observer.pid
    && ready.pid === claim.pid && ready.armed_at === claim.armed_at
    && isDeepStrictEqual(ready.trace_identity, frozen.trace_identity)
    && ready.trace_prefix_sha256 === frozen.trace_prefix_sha256, 'observer_ready_changed');
  return { claim, frozen };
}

function verifyTools(plan) {
  readPinnedBytes(plan.metadata_tool, { readonly: false, executable: true });
  readPinnedBytes(plan.frozen_helpers.observer);
  readPinnedBytes(plan.frozen_helpers.adapter);
  readPinnedBytes(plan.binding.herdr, { readonly: false, executable: true });
  readPinnedBytes(plan.binding.birth_tool, { readonly: false, executable: true });
  readPinnedBytes(plan.binding.frontend_binary, { readonly: false, executable: true });
  requireEvidence(path.basename(plan.frozen_helpers.observer.path) === 'fault_observer.py'
    && path.basename(plan.frozen_helpers.adapter.path) === 'native_adapter.py'
    && path.dirname(plan.frozen_helpers.observer.path) === path.dirname(plan.frozen_helpers.adapter.path),
  'invalid_frozen_helpers');
  for (const [key, filename] of [['controller', 'native-control.mjs'], ['evidence', 'native-control-evidence.mjs']]) {
    const expected = realpathSync(fileURLToPath(new URL(filename, import.meta.url)));
    requireEvidence(plan.controller_tools[key].path === expected, 'invalid_controller_tools');
    readPinnedBytes(plan.controller_tools[key], { readonly: false });
  }
}

function validateObserverIdentity(plan) {
  requireEvidence(validProcess(plan.observer)
    && plan.observer.argv.length === 6 && path.isAbsolute(plan.observer.argv[0])
    && plan.observer.argv[2] === '--binding' && plan.observer.argv[4] === '--stage'
    && plan.observer.argv[5] === plan.stage, 'invalid_observer_identity');
  canonical(plan.observer.cwd, 'directory', 'invalid_observer_identity');
  const resolveOperand = operand => path.resolve(plan.observer.cwd, operand);
  requireEvidence(resolveOperand(plan.observer.argv[1]) === plan.frozen_helpers.observer.path
    && resolveOperand(plan.observer.argv[3]) === plan.observer_binding.path, 'invalid_observer_identity');
}

function validatePrerequisites(plan) {
  const expected = plan.stage === 'restart04'
    ? ['natural03', 'pre_restart_audits', 'prerequisite_waits', 'blocked_terminal_order', 'tiny_timeout', 'same_backend_readonly_recovery']
    : [];
  requireEvidence(exactKeys(plan.prerequisite_pins, expected), 'invalid_prerequisites');
  for (const name of expected) {
    const value = readPinnedJson(plan.prerequisite_pins[name]);
    requireEvidence(record(value), 'invalid_prerequisites');
  }
}

function trustedAncestry(candidate) {
  const effectiveUid = typeof process.geteuid === 'function' ? process.geteuid() : undefined;
  requireEvidence(Number.isSafeInteger(effectiveUid), 'untrusted_stage_ancestry');
  const candidates = [path.parse(candidate).root, ...pathParts(candidate)];
  const result = candidates.map(current => ({
    path: current,
    information: canonical(current, 'directory', 'untrusted_stage_ancestry'),
  }));
  requireEvidence(result[0].information.uid === 0, 'untrusted_stage_ancestry');
  for (let index = 0; index < result.length; index += 1) {
    const information = result[index].information;
    requireEvidence([0, effectiveUid].includes(information.uid)
      && ((information.mode & 0o022) === 0 || (information.mode & 0o1000) !== 0),
    'untrusted_stage_ancestry');
    if (index > 0 && (result[index - 1].information.mode & 0o022) !== 0) {
      requireEvidence([0, effectiveUid].includes(information.uid), 'untrusted_stage_ancestry');
    }
  }
  return result;
}

function privateDirectory(candidate) {
  const information = canonical(candidate, 'directory', 'untrusted_private_directory');
  const effectiveUid = typeof process.geteuid === 'function' ? process.geteuid() : undefined;
  requireEvidence(information.uid === effectiveUid && (information.mode & 0o777) === 0o700,
    'untrusted_private_directory');
}

function retainDirectories(plan, attemptPath, observerRoot, manifest) {
  const frozen = readPinnedJson(plan.observer_binding);
  const observerParent = path.dirname(observerRoot);
  const baseControl = manifest.base_control;
  const recoveryRoot = path.dirname(attemptPath);
  for (const candidate of [baseControl, recoveryRoot, attemptPath, observerRoot]) privateDirectory(candidate);
  const roots = [...new Set([
    plan.binding.project, baseControl, recoveryRoot, attemptPath,
    observerParent, observerRoot, observerInstancePath(frozen.instance), plan.observer.cwd,
  ])];
  const retained = new Map();
  for (const root of roots) {
    for (const { path: candidate, information } of trustedAncestry(root)) {
      retained.set(candidate, { path: candidate, dev: information.dev, ino: information.ino,
        uid: information.uid, mode: information.mode });
    }
  }
  return [...retained.values()];
}

function recheckDirectories(context) {
  for (const retained of context.directory_identities) {
    const current = canonical(retained.path, 'directory', 'stage_ancestry_changed');
    requireEvidence(current.dev === retained.dev && current.ino === retained.ino
      && current.uid === retained.uid && current.mode === retained.mode, 'stage_ancestry_changed');
  }
}

function validatePlanShape(plan) {
  requireEvidence(record(plan) && ['interrupt03', 'restart04'].includes(plan.stage), 'invalid_stage_plan');
  const keys = plan.stage === 'restart04' ? [...BASE_PLAN_KEYS, 'expected_loop'] : BASE_PLAN_KEYS;
  requireEvidence(exactKeys(plan, keys) && plan.version === 1 && UUID4_LOWER.test(plan.activation_id), 'invalid_stage_plan');
  for (const key of ['attempt_manifest', 'prepared', 'observer_binding', 'observer_claim', 'observer_ready', 'metadata_tool']) {
    validatePin(plan[key]);
  }
  requireEvidence(exactKeys(plan.frozen_helpers, ['observer', 'adapter'])
    && exactKeys(plan.controller_tools, ['controller', 'evidence'])
    && record(plan.protected) && Object.keys(plan.protected).length > 0, 'invalid_stage_plan');
  for (const pin of [...Object.values(plan.frozen_helpers), ...Object.values(plan.controller_tools),
    ...Object.values(plan.prerequisite_pins ?? {})]) validatePin(pin);
  requireEvidence(validGoal(plan.expected_goal)
    && (plan.stage === 'restart04' ? validLoop(plan.expected_loop) : !Object.hasOwn(plan, 'expected_loop')),
  'invalid_expected_state');
  validateOperationIds(plan);
  validateTimeouts(plan);
  validateBindingShape(plan.binding, plan.stage);
  validateObserverIdentity(plan);
}

function staticEvidence(plan, attemptPath, observerRoot) {
  const lineage = validateLineage(plan, attemptPath);
  validateTrace(plan, lineage.manifest, lineage.predecessors);
  const observer = validateObserverFiles(plan, observerRoot);
  verifyTools(plan);
  validatePrerequisites(plan);
  protectedShape(plan, observer.frozen);
  for (const [relative, sha256] of Object.entries(plan.protected)) {
    readPinnedBytes({ path: relativeFile(plan.binding.project, relative), sha256 });
  }
  return { ...observer, lineage };
}

export function validateStagePlan(input) {
  let plan;
  try { plan = structuredClone(input); } catch { throw new EvidenceError('invalid_stage_plan'); }
  validatePlanShape(plan);
  const attemptPath = path.dirname(plan.attempt_manifest.path);
  const activationPath = path.join(attemptPath, `activation-${plan.stage}`);
  const releasePath = path.join(plan.binding.project, '.autonomy', `stage${STAGE_NUMBER[plan.stage]}-release.json`);
  const observerRoot = path.join(attemptPath, 'fault-observer', plan.stage);
  const evidence = staticEvidence(plan, attemptPath, observerRoot);
  const baseControl = evidence.lineage.manifest.base_control;
  requireEvidence(baseControl !== plan.binding.project
    && !within(baseControl, plan.binding.project) && !within(plan.binding.project, baseControl)
    && publicText(path.join(activationPath, 'controls')), 'invalid_attempt_scope');
  return {
    plan,
    attempt_path: attemptPath,
    activation_path: activationPath,
    release_path: releasePath,
    observer_root: observerRoot,
    directory_identities: retainDirectories(plan, attemptPath, observerRoot, evidence.lineage.manifest),
  };
}

function verifyNoConsumedWindow(context, released) {
  const { plan, observer_root: observerRoot, release_path: releasePath } = context;
  const number = STAGE_NUMBER[plan.stage];
  if (!released) {
    for (const name of ['outcome.json', 'action-intent.json']) absent(path.join(observerRoot, name));
    absent(path.join(plan.binding.project, '.autonomy', `started-${number}.json`));
    absent(path.join(plan.binding.project, '.autonomy', 'checkpoints', `${number}.json`));
    absent(releasePath);
  } else {
    let release;
    try { release = readReadonlyBoundedJson(releasePath, 'stage_release_changed'); }
    catch { throw new EvidenceError('stage_release_changed'); }
    requireEvidence(release.version === 1 && release.activation_id === plan.activation_id
      && release.attempt_id === path.basename(context.attempt_path) && release.stage === plan.stage
      && release.goal_id === plan.expected_goal.goal_id && Number.isSafeInteger(release.released_at_ms),
    'stage_release_changed');
  }
}

export function verifyStageEvidence(context, { released = false } = {}) {
  requireEvidence(record(context) && typeof released === 'boolean' && record(context.plan)
    && typeof context.attempt_path === 'string' && typeof context.activation_path === 'string'
    && typeof context.release_path === 'string' && typeof context.observer_root === 'string'
    && Array.isArray(context.directory_identities), 'invalid_stage_context');
  recheckDirectories(context);
  const observer = staticEvidence(context.plan, context.attempt_path, context.observer_root);
  const now = Date.now() / 1000;
  requireEvidence(Number.isFinite(observer.claim.armed_at) && observer.claim.armed_at <= now
    && now - observer.claim.armed_at <= OBSERVER_LIFETIME_SECONDS
    && observer.claim.armed_at + OBSERVER_LIFETIME_SECONDS - now
      >= context.plan.activation_timeout_ms / 1000, 'observer_claim_expired');
  verifyNoConsumedWindow(context, released);
}

function remaining(deadline) {
  const milliseconds = Math.floor(deadline - performance.now());
  requireEvidence(Number.isFinite(deadline) && milliseconds > 0, 'observer_query_timeout');
  return milliseconds;
}

async function command(pin, args, deadline, queryTimeout) {
  readPinnedBytes(pin, { readonly: false, executable: true });
  const timeout = Math.min(queryTimeout, remaining(deadline));
  let result;
  try {
    result = await execute(pin.path, args, {
      encoding: 'utf8', timeout, maxBuffer: MAX_CHILD_OUTPUT_BYTES, killSignal: 'SIGKILL', windowsHide: true,
    });
  } catch {
    throw new EvidenceError('observer_query_failed');
  }
  requireEvidence(result.stderr === '', 'observer_query_failed');
  readPinnedBytes(pin, { readonly: false, executable: true });
  remaining(deadline);
  try { return JSON.parse(result.stdout); }
  catch { throw new EvidenceError('observer_query_failed'); }
}

function selectedSnapshot(snapshot, pid) {
  requireEvidence(exactKeys(snapshot, ['version', 'processes']) && snapshot.version === 1
    && Array.isArray(snapshot.processes), 'observer_identity_changed');
  const rows = snapshot.processes.filter(row => record(row) && row.pid === pid);
  requireEvidence(rows.length === 1, 'observer_identity_changed');
  return { version: 1, processes: rows };
}

export async function observeStageObserver(context, deadline) {
  requireEvidence(record(context) && record(context.plan), 'invalid_stage_context');
  remaining(deadline);
  const startedAt = Date.now();
  const beforeRaw = await command(context.plan.binding.birth_tool, [], deadline, context.plan.query_timeout_ms);
  const metadata = await command(context.plan.metadata_tool,
    ['--pid', String(context.plan.observer.pid)], deadline, context.plan.query_timeout_ms);
  const afterRaw = await command(context.plan.binding.birth_tool, [], deadline, context.plan.query_timeout_ms);
  requireEvidence(exactKeys(metadata, ['version', 'pid', 'cwd', 'argv']) && metadata.version === 1,
    'observer_identity_changed');
  try { assertProcessIdentity(context.plan.observer, { before: beforeRaw, metadata, after: afterRaw }); }
  catch { throw new EvidenceError('observer_identity_changed'); }
  const before = selectedSnapshot(beforeRaw, context.plan.observer.pid);
  const after = selectedSnapshot(afterRaw, context.plan.observer.pid);
  return { started_at_ms: startedAt, observed_at_ms: Date.now(), before, metadata, after };
}
