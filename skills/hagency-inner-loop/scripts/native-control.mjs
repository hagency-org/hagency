import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { closeSync, constants, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { setTimeout as delay } from 'node:timers/promises';
import { isDeepStrictEqual, promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { assertProcessIdentity, publishExclusiveJson, readReadonlyJson } from './native-control-evidence.mjs';

const execute = promisify(execFile);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const SHA = /^[0-9a-f]{64}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const OPERATION_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const GOAL_STATES = new Set(['active', 'paused', 'blocked', 'budget_limited', 'complete']);
const TURN_STATES = new Set(['active', 'interrupting', 'completed', 'errored', 'interrupted']);
const TERMINAL = new Set(['completed', 'errored', 'interrupted']);
// Herdr displays detected Idle as done until the pane is seen.
const HERDR_IDLE = new Set(['idle', 'done']);
const OPERATIONS = new Set(['inspect', 'goal-pause', 'goal-resume', 'loop-create', 'loop-pause', 'loop-resume', 'loop-delete']);
const LOOP_ID = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;
const MAX_FILE_BYTES = 128 * 1024 * 1024;
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = value => typeof value === 'string' && value.length > 0 && value.length <= 4096 && !/[\x00-\x1f\x7f]/.test(value);
const pid = value => Number.isSafeInteger(value) && value > 1;
const sameFile = (a, b) => a.dev === b.dev && a.ino === b.ino;
function requireProof(condition, code) { if (!condition) throw new ControlError(code); }
class ControlError extends Error { constructor(code) { super(code); this.code = code; } }

// No path normalization here: silently resolving a supplied symlink would erase
// the distinction the binding is intended to prove.
function canonical(candidate, kind) {
  requireProof(text(candidate) && path.isAbsolute(candidate) && path.normalize(candidate) === candidate, 'invalid_path');
  let component = path.parse(candidate).root;
  for (const part of candidate.slice(component.length).split('/').filter(Boolean)) {
    component = path.join(component, part);
    requireProof(!lstatSync(component).isSymbolicLink(), 'symlink_path');
  }
  const info = lstatSync(candidate);
  requireProof(realpathSync(candidate) === candidate && (kind === 'directory' ? info.isDirectory() : info.isFile()), 'invalid_path');
  return info;
}

function pinnedFile(pin, executable = false) {
  requireProof(record(pin) && SHA.test(pin.sha256), 'invalid_binary_pin');
  const initial = canonical(pin.path, 'file');
  requireProof(initial.size <= MAX_FILE_BYTES && (!executable || (initial.mode & 0o111) !== 0), 'invalid_binary_pin');
  const fd = openSync(pin.path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = fstatSync(fd);
    requireProof(sameFile(initial, before) && before.size <= MAX_FILE_BYTES, 'binary_changed');
    const raw = readFileSync(fd);
    const after = fstatSync(fd);
    requireProof(sameFile(before, after) && after.size === before.size && raw.length === before.size
      && after.mtimeMs === before.mtimeMs && sha(raw) === pin.sha256
      && sameFile(after, canonical(pin.path, 'file')), 'binary_changed');
  } finally { closeSync(fd); }
}

function validateBinding(b) {
  requireProof(record(b) && b.version === 1, 'invalid_binding');
  canonical(b.project, 'directory'); canonical(b.trace, 'file');
  for (const key of ['herdr_session', 'lower_agent', 'pane_id', 'terminal_id', 'profile', 'native_session']) {
    requireProof(text(b[key]) && !b[key].startsWith('-'), 'invalid_binding');
  }
  requireProof(b.native_session === `${b.profile}:local:tui#coding` && pid(b.shell_pid)
    && record(b.frontend) && record(b.backend)
    && b.frontend.cwd === b.project && b.backend.cwd === b.project
    && b.frontend.ppid === b.shell_pid && b.backend.ppid === b.frontend.pid
    && b.backend.pgid === b.frontend.pgid
    && new Set([b.shell_pid, b.frontend.pid, b.backend.pid]).size === 3
    && Array.isArray(b.frontend.argv) && b.frontend.argv[0] === b.frontend_binary?.path, 'invalid_binding');
  pinnedFile(b.herdr, true); pinnedFile(b.birth_tool, true); pinnedFile(b.frontend_binary);
}

function timeout(value) {
  requireProof(Number.isSafeInteger(value) && value >= 100 && value <= 10000, 'invalid_timeout');
  return value;
}

async function command(pin, args, deadline) {
  pinnedFile(pin, true);
  const remaining = Math.floor(deadline - performance.now());
  requireProof(remaining > 0, 'query_timeout');
  try {
    const result = await execute(pin.path, args, {
      timeout: remaining, maxBuffer: 16 * 1024 * 1024, encoding: 'utf8', killSignal: 'SIGKILL', windowsHide: true,
    });
    pinnedFile(pin, true);
    return JSON.parse(result.stdout);
  } catch (error) {
    if (error instanceof ControlError) throw error;
    throw new ControlError('command_failed');
  }
}

async function processes(b, deadline) {
  validateBinding(b);
  const before = await command(b.birth_tool, [], deadline);
  const a = await command(b.herdr, ['--session', b.herdr_session, 'agent', 'get', b.lower_agent], deadline);
  const p = await command(b.herdr, ['--session', b.herdr_session, 'pane', 'process-info', '--pane', b.pane_id], deadline);
  const after = await command(b.birth_tool, [], deadline);
  const agent = a?.result?.agent, pane = p?.result?.process_info;
  requireProof(record(agent) && record(pane) && agent.name === b.lower_agent && agent.agent === 'octoscode'
    && agent.pane_id === b.pane_id && agent.terminal_id === b.terminal_id
    && agent.cwd === b.project && agent.foreground_cwd === b.project
    && text(agent.agent_status) && pane.pane_id === b.pane_id && pane.shell_pid === b.shell_pid
    && pane.foreground_process_group_id === b.frontend.pgid && Array.isArray(pane.foreground_processes), 'process_scope_changed');
  const seen = new Set();
  for (const row of pane.foreground_processes) {
    requireProof(record(row) && pid(row.pid) && !seen.has(row.pid), 'invalid_process_metadata'); seen.add(row.pid);
  }
  for (const role of ['frontend', 'backend']) {
    const matches = pane.foreground_processes.filter(row => row.pid === b[role].pid);
    requireProof(matches.length === 1, 'missing_process');
    assertProcessIdentity(b[role], { before, metadata: matches[0], after });
  }
  const left = before.processes.find(row => row.pid === b.shell_pid), right = after.processes.find(row => row.pid === b.shell_pid);
  requireProof(left && right && left.state !== 'Z' && right.state !== 'Z'
    && ['pid', 'ppid', 'pgid', 'started'].every(key => left[key] === right[key]), 'shell_changed');
  // Retain only the selected owned facts, not the host-wide process snapshot.
  return { frontend: b.frontend, backend: b.backend, shell: right, agent_status: agent.agent_status,
    pane_id: b.pane_id, terminal_id: b.terminal_id, observed_at_ms: Date.now() };
}

function traceBytes(file) {
  const initial = canonical(file, 'file');
  requireProof(initial.size <= MAX_FILE_BYTES, 'trace_too_large');
  const fd = openSync(file, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const opened = fstatSync(fd);
    requireProof(sameFile(initial, opened) && opened.size <= MAX_FILE_BYTES, 'trace_changed');
    const bytes = readFileSync(fd);
    const final = fstatSync(fd);
    requireProof(sameFile(opened, final) && sameFile(final, canonical(file, 'file'))
      && final.size >= opened.size && bytes.length >= opened.size && bytes.length <= MAX_FILE_BYTES, 'trace_changed');
    return { bytes, dev: opened.dev, ino: opened.ino };
  } finally { closeSync(fd); }
}

function makeFence(file) {
  const { bytes, dev, ino } = traceBytes(file);
  requireProof(bytes.length === 0 || bytes.at(-1) === 10, 'trace_partial_record');
  return { dev, ino, offset: bytes.length, prefix_sha256: sha(bytes), at_ms: Date.now() };
}

function freshRecords(file, fence) {
  const current = traceBytes(file);
  requireProof(sameFile(current, fence) && current.bytes.length >= fence.offset
    && sha(current.bytes.subarray(0, fence.offset)) === fence.prefix_sha256, 'trace_history_changed');
  const end = current.bytes.lastIndexOf(10);
  if (end < fence.offset) return [];
  const lines = new TextDecoder('utf-8', { fatal: true }).decode(current.bytes.subarray(fence.offset, end)).split('\n');
  return lines.map(line => {
    const row = JSON.parse(line);
    requireProof(record(row) && record(row.frame) && ['client_to_server', 'server_to_client'].includes(row.direction), 'invalid_trace_record');
    return row;
  });
}

function correlate(rows, expected, fence) {
  const requests = rows.map((row, index) => ({ row, index })).filter(({ row }) =>
    row.direction === 'client_to_server' && row.frame.method === expected.method);
  requireProof(requests.length <= 1, 'ambiguous_request');
  if (!requests.length) return null;
  const { row: request, index } = requests[0];
  const id = request.frame.id;
  requireProof((text(id) || Number.isSafeInteger(id)) && request.frame.jsonrpc === '2.0'
    && isDeepStrictEqual(request.frame.params, expected.params), 'request_mismatch');
  requireProof(rows.filter(row => row.direction === 'client_to_server' && row.frame.id === id).length === 1, 'ambiguous_request');
  const replies = rows.map((row, position) => ({ row, position })).filter(({ row }) => row.direction === 'server_to_client' && row.frame.id === id);
  requireProof(replies.length <= 1, 'ambiguous_response');
  if (!replies.length) return null;
  const { row: response, position } = replies[0];
  requireProof(position > index && response.frame.jsonrpc === '2.0' && !Object.hasOwn(response.frame, 'error')
    && record(response.frame.result), 'rpc_failed');
  const requestTime = Date.parse(request.ts), responseTime = Date.parse(response.ts);
  requireProof(Number.isFinite(requestTime) && Number.isFinite(responseTime)
    && requestTime >= fence.at_ms && responseTime >= requestTime && responseTime <= Date.now(), 'trace_timestamp_invalid');
  const result = response.frame.result;
  requireProof(result.session_id === expected.params.session_id
    && (!Object.hasOwn(expected.params, 'profile_id') || result.profile_id === expected.params.profile_id), 'response_scope_changed');
  return { fence, request, response };
}

function queryDescription(b, method) {
  const scope = { session_id: b.native_session, profile_id: b.profile };
  if (method === 'session/goal/get') return { command: '/goal --report', expected: [{ method, params: scope }] };
  if (method === 'loop/list') return { command: '/loop list --report', expected: [{ method, params: scope }] };
  if (method === 'session/hydrate') return { command: '/turn session', expected: [{ method, params: { session_id: b.native_session, include: ['turns'] } }] };
  throw new ControlError('unsupported_query');
}

async function exchange(b, description, timeoutMs, beforeSend) {
  const deadline = performance.now() + timeout(timeoutMs);
  const before = await processes(b, deadline);
  const fence = makeFence(b.trace); // Must precede both intent publication and delivery.
  try {
    if (beforeSend) beforeSend(fence, before);
    await command(b.herdr, ['--session', b.herdr_session, 'agent', 'prompt', b.lower_agent, description.command], deadline);
    while (performance.now() < deadline) {
      const rows = freshRecords(b.trace, fence);
      const observations = description.expected.map(expected => correlate(rows, expected, fence));
      if (observations.every(Boolean)) {
        for (let i = 1; i < observations.length; i++) {
          requireProof(rows.indexOf(observations[i].request) > rows.indexOf(observations[i - 1].response), 'control_rpc_order_changed');
        }
        const after = await processes(b, deadline);
        // Process inspection can take time; prove no conflicting append arrived meanwhile.
        const finalRows = freshRecords(b.trace, fence);
        const retained = description.expected.map(expected => correlate(finalRows, expected, fence));
        requireProof(isDeepStrictEqual(retained, observations), 'trace_changed');
        return { observations, before, after };
      }
      await delay(Math.min(20, Math.max(1, deadline - performance.now())));
    }
    throw new ControlError('query_timeout');
  } catch (error) {
    const failure = error instanceof ControlError ? error : new ControlError('invalid_or_unavailable_evidence');
    const attempt = { fence, before, command: description.command, records: [], trace_integrity: 'unavailable' };
    try {
      const rows = freshRecords(b.trace, fence);
      const requests = rows.filter(row => row.direction === 'client_to_server'
        && description.expected.some(expected => row.frame.method === expected.method));
      const ids = new Set(requests.map(row => row.frame.id));
      attempt.records = rows.filter(row => requests.includes(row)
        || (row.direction === 'server_to_client' && ids.has(row.frame.id)));
      attempt.trace_integrity = 'preserved';
    } catch { attempt.trace_integrity = 'invalid_or_unavailable'; }
    failure.attempt = attempt;
    throw failure;
  }
}

export async function queryNative(binding, method, { timeout_ms = 2000 } = {}) {
  return exchange(binding, queryDescription(binding, method), timeout_ms);
}

function expectedGoal(value) {
  requireProof(record(value) && text(value.goal_id) && Number.isSafeInteger(value.created_at_ms)
    && value.created_at_ms > 0 && SHA.test(value.objective_sha256), 'invalid_expected_goal');
}

function goal(result, expected) {
  const actual = result.goal;
  requireProof(record(actual) && actual.goal_id === expected.goal_id && actual.created_at_ms === expected.created_at_ms
    && typeof actual.objective === 'string' && sha(actual.objective) === expected.objective_sha256
    && GOAL_STATES.has(actual.status), 'goal_changed');
  requireProof(actual.profile_id === result.profile_id
    && (!Object.hasOwn(actual, 'session_id') || actual.session_id === result.session_id), 'goal_scope_changed');
  return actual;
}

function turns(result) {
  requireProof(Array.isArray(result.turns), 'turns_missing');
  const ids = new Set();
  for (const turn of result.turns) {
    requireProof(record(turn) && UUID.test(turn.turn_id) && !ids.has(turn.turn_id) && TURN_STATES.has(turn.state)
      && (!Object.hasOwn(turn, 'session_id') || turn.session_id === result.session_id), 'invalid_turns');
    ids.add(turn.turn_id);
  }
  return result.turns;
}

function loops(result) {
  requireProof(Array.isArray(result.loops), 'loops_missing');
  const ids = new Set();
  for (const loop of result.loops) {
    requireProof(record(loop) && text(loop.loop_id) && !ids.has(loop.loop_id)
      && loop.session_id === result.session_id && loop.profile_id === result.profile_id
      && ['active', 'paused'].includes(loop.status), 'invalid_loops');
    ids.add(loop.loop_id);
  }
  return result.loops;
}

function loopTemplate(value) {
  requireProof(record(value) && text(value.prompt) && value.prompt.trim().length > 0 && value.prompt.trim() === value.prompt
    // Rust's Unicode trim also strips U+0085, which JS trim preserves.
    && !/^\p{White_Space}|\p{White_Space}$/u.test(value.prompt)
    && SHA.test(value.prompt_sha256) && sha(value.prompt) === value.prompt_sha256
    && value.mode === 'fixed_interval' && value.interval_seconds === 60, 'invalid_loop_template');
}

function expectedLoop(value) {
  requireProof(record(value) && typeof value.loop_id === 'string' && LOOP_ID.test(value.loop_id)
    && Number.isSafeInteger(value.created_at_ms) && value.created_at_ms > 0
    && SHA.test(value.prompt_sha256) && value.mode === 'fixed_interval' && value.interval_seconds === 60, 'invalid_expected_loop');
}

function boundLoop(actual, binding, expected) {
  requireProof(record(actual) && actual.loop_id === expected.loop_id && actual.created_at_ms === expected.created_at_ms
    && actual.session_id === binding.native_session && actual.profile_id === binding.profile
    && typeof actual.prompt === 'string' && sha(actual.prompt) === expected.prompt_sha256
    && actual.mode === expected.mode && actual.interval_seconds === expected.interval_seconds
    && ['active', 'paused', 'deleted'].includes(actual.status), 'loop_identity_changed');
  return actual;
}

function soleLoop(list, binding, expected) {
  requireProof(list.length === 1, 'loop_not_unique');
  return boundLoop(list[0], binding, expected);
}

function syncDirectory(directory) {
  const fd = openSync(directory, constants.O_RDONLY | constants.O_DIRECTORY);
  try { fsyncSync(fd); } finally { closeSync(fd); }
}

/** Narrow controller: observed/applied describe one operation, never full E2E. */
export async function runControlPlan(input) {
  let operation, operationId, directory, failedExchange, mutationIntent = false;
  const observations = [], processObservations = [];
  const result = status => ({ version: 1, status, full_acceptance: false,
    ...(operation ? { operation, operation_id: operationId } : {}) });
  const finish = report => {
    if (!directory) return report;
    const file = path.join(directory, 'evidence.json');
    try {
      publishExclusiveJson(file, { version: 1, report, observations, processes: processObservations,
        ...(failedExchange ? { failed_exchange: failedExchange } : {}) });
      return { ...report, evidence_path: file };
    } catch {
      return { ...result(mutationIntent ? 'outcome_unknown' : 'failed'), reason: 'evidence_publication_failed' };
    }
  };
  try {
    const plan = structuredClone(input);
    requireProof(record(plan) && plan.version === 1 && OPERATION_ID.test(plan.operation_id)
      && OPERATIONS.has(plan.operation), 'invalid_plan');
    operation = plan.operation; operationId = plan.operation_id;
    expectedGoal(plan.expected_goal); timeout(plan.query_timeout_ms);
    const isLoopOperation = operation.startsWith('loop-'), creatingLoop = operation === 'loop-create';
    if (creatingLoop) loopTemplate(plan.loop_template);
    else if (isLoopOperation) expectedLoop(plan.expected_loop);
    requireProof(record(plan.binding), 'invalid_binding');
    canonical(plan.binding.project, 'directory');
    const evidenceRoot = canonical(plan.evidence_dir, 'directory');
    requireProof((evidenceRoot.mode & 0o077) === 0, 'evidence_directory_not_private');
    requireProof(plan.evidence_dir !== plan.binding.project
      && !plan.evidence_dir.startsWith(`${plan.binding.project}${path.sep}`), 'evidence_inside_business_project');
    const candidate = path.join(plan.evidence_dir, operationId);
    try { mkdirSync(candidate, { mode: 0o700 }); }
    catch (error) { if (error.code === 'EEXIST') return { ...result('rejected'), reason: 'operation_already_claimed' }; throw error; }
    directory = candidate; syncDirectory(plan.evidence_dir);
    publishExclusiveJson(path.join(directory, 'plan.json'), plan);
    validateBinding(plan.binding);
    const retain = observed => {
      publishExclusiveJson(path.join(directory, `observation-${processObservations.length + 1}.json`), observed);
      observations.push(...observed.observations); processObservations.push({ before: observed.before, after: observed.after });
      return observed.observations.at(-1).response.frame.result;
    };
    const read = async method => retain(await queryNative(plan.binding, method, { timeout_ms: plan.query_timeout_ms }));
    const act = async (description, needsIdle, details = {}) => exchange(plan.binding, description, plan.query_timeout_ms, (fence, before) => {
      if (needsIdle) requireProof(HERDR_IDLE.has(before.agent_status), isLoopOperation ? 'loop_not_ready' : 'resume_not_ready');
      // Set before publication: an fsync/read-back error may leave a durable intent.
      mutationIntent = true;
      publishExclusiveJson(path.join(directory, 'intent.json'), { version: 1, operation_id: operationId,
        operation, expected_goal: plan.expected_goal, ...details, fence, created_at_ms: Date.now() });
    });
    const current = goal(await read('session/goal/get'), plan.expected_goal);
    const currentLoops = loops(await read('loop/list'));
    let originalLoop;
    if (creatingLoop) requireProof(currentLoops.length === 0, 'loop_set_not_empty');
    else if (isLoopOperation) originalLoop = soleLoop(currentLoops, plan.binding, plan.expected_loop);
    const currentTurns = turns(await read('session/hydrate'));
    if (operation === 'inspect') return finish({ ...result('observed'), goal_id: current.goal_id,
      goal_status: current.status, loop_count: currentLoops.length, turn_count: currentTurns.length });
    if (isLoopOperation) {
      const b = plan.binding, action = operation.slice('loop-'.length), needsIdle = action !== 'pause';
      if (needsIdle) {
        requireProof(['paused', 'blocked'].includes(current.status) && currentTurns.length > 0
          && currentTurns.every(turn => TERMINAL.has(turn.state))
          && HERDR_IDLE.has(processObservations.at(-1).after.agent_status), 'loop_not_ready');
      } else requireProof(current.status !== 'complete', 'loop_not_ready');
      if (action === 'resume') requireProof(originalLoop.status === 'paused', 'loop_not_ready');
      const template = plan.loop_template, scope = { session_id: b.native_session, profile_id: b.profile };
      const params = creatingLoop
        ? { ...scope, prompt: template.prompt, mode: template.mode, interval_seconds: template.interval_seconds }
        : { session_id: b.native_session, loop_id: originalLoop.loop_id };
      const description = { command: creatingLoop ? `/loop every 60s ${template.prompt}` : `/loop ${action} ${originalLoop.loop_id}`,
        expected: [{ method: `loop/${action}`, params }] };
      const observed = await act(description, needsIdle, creatingLoop
        ? { loop_template: template } : { expected_loop: plan.expected_loop });
      const response = retain(observed);
      const status = { create: 'active', pause: 'paused', resume: 'active', delete: 'deleted' }[action];
      requireProof(response.ok === true && response.status === status && record(response.loop)
        && response.loop_id === response.loop.loop_id, 'loop_control_response_changed');
      let expected = plan.expected_loop;
      if (creatingLoop) {
        requireProof(response.created === true && record(response.fire) && response.fire.queued === false
          && response.fire.reason === 'waiting_for_schedule', 'loop_creation_unproven');
        expected = { loop_id: response.loop_id, created_at_ms: response.loop.created_at_ms,
          prompt_sha256: template.prompt_sha256, mode: template.mode, interval_seconds: template.interval_seconds };
        expectedLoop(expected);
        requireProof(expected.created_at_ms >= observed.observations[0].fence.at_ms
          && expected.created_at_ms <= observed.after.observed_at_ms
          && response.loop.next_run_at_ms === expected.created_at_ms + 60000, 'loop_creation_unproven');
      }
      const changed = boundLoop(response.loop, b, expected);
      requireProof(changed.status === status && (action !== 'delete' || response.deleted === true), 'loop_control_response_changed');
      const observedGoal = goal(await read('session/goal/get'), plan.expected_goal);
      if (needsIdle) requireProof(observedGoal.status === current.status, 'goal_status_changed');
      const listed = loops(await read('loop/list'));
      if (action === 'delete') requireProof(listed.length === 0, 'loop_deletion_unproven');
      else requireProof(soleLoop(listed, b, expected).status === status, 'loop_status_changed');
      return finish({ ...result('applied'), goal_id: current.goal_id, loop_id: changed.loop_id,
        loop_created_at_ms: changed.created_at_ms, loop_status: status });
    }
    const resuming = operation === 'goal-resume';
    if (resuming) {
      requireProof(['paused', 'blocked'].includes(current.status) && currentTurns.length > 0
        && currentTurns.every(turn => TERMINAL.has(turn.state))
        && HERDR_IDLE.has(processObservations.at(-1).after.agent_status), 'resume_not_ready');
    } else requireProof(['active', 'paused', 'blocked', 'budget_limited'].includes(current.status), 'pause_not_ready');
    const b = plan.binding, status = resuming ? 'active' : 'paused';
    const scope = { session_id: b.native_session, profile_id: b.profile };
    const description = { command: resuming ? '/goal resume' : '/goal pause', expected: [
      { method: 'session/goal/get', params: scope },
      { method: 'session/goal/set', params: { ...scope, objective: current.objective, status, transition_actor: 'user' } },
    ] };
    const observed = await act(description, resuming);
    retain(observed);
    const freshGoal = goal(observed.observations[0].response.frame.result, plan.expected_goal);
    requireProof((resuming ? ['paused', 'blocked'] : ['active', 'paused', 'blocked', 'budget_limited']).includes(freshGoal.status),
      'goal_eligibility_changed');
    const finalGoal = goal(observed.observations[1].response.frame.result, plan.expected_goal);
    requireProof(finalGoal.status === status, 'goal_status_changed');
    return finish({ ...result('applied'), goal_id: finalGoal.goal_id, goal_status: finalGoal.status });
  } catch (error) {
    failedExchange = error?.attempt;
    return finish({ ...result(mutationIntent ? 'outcome_unknown' : 'failed'),
      reason: error instanceof ControlError ? error.code : 'invalid_or_unavailable_evidence' });
  }
}

async function main(args) {
  let report;
  try {
    requireProof(args.length === 4 && args[0] === '--plan' && args[2] === '--sha256'
      && SHA.test(args[3]), 'invalid_cli_arguments');
    const loaded = readReadonlyJson(args[1], args[3]);
    report = await runControlPlan(loaded.value);
  } catch { report = { version: 1, status: 'failed', full_acceptance: false, reason: 'invalid_plan' }; }
  process.stdout.write(`${JSON.stringify(report)}\n`);
  process.exitCode = ['observed', 'applied'].includes(report.status) ? 0 : 1;
}

let invokedAsScript = false;
try {
  invokedAsScript = Boolean(process.argv[1])
    && realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1]);
} catch { /* A missing entry path does not turn a library import into execution. */ }
if (invokedAsScript) await main(process.argv.slice(2));
