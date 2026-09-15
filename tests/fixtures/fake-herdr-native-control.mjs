import { appendFileSync, readFileSync, renameSync, writeFileSync } from 'node:fs';

// This local child fixture deliberately appends replies before prompt delivery
// returns. It never launches a worker, contacts a service or sends a signal.
export function main(statePath, args) {
  const s = JSON.parse(readFileSync(statePath, 'utf8'));
  const output = result => process.stdout.write(JSON.stringify({ result }) + '\n');
  const record = (direction, frame) => appendFileSync(s.trace,
    JSON.stringify({ ts: new Date().toISOString(), direction, frame }) + '\n');
  const rpc = (method, params, result, { omit = false } = {}) => {
    const id = `fixture-${++s.nextId}`;
    if (s.mode === 'wrong_scope') params = { ...params, session_id: 'foreign:local:tui#coding' };
    const request = { jsonrpc: '2.0', id, method, params };
    record('client_to_server', request);
    if (s.mode === 'duplicate_request') record('client_to_server', { ...request, id: `${id}-duplicate` });
    if (!omit) record('server_to_client', s.mode === 'rpc_error'
      ? { jsonrpc: '2.0', id, error: { code: -1, message: s.secret } }
      : { jsonrpc: '2.0', id: s.mode === 'wrong_id' ? `${id}-wrong` : id, result });
  };
  const save = () => writeFileSync(statePath, JSON.stringify(s));
  if (args[0] !== '--session' || args[1] !== s.herdrSession) throw new Error('fixture session mismatch');
  const rest = args.slice(2);
  if (rest.join(' ') === `pane process-info --pane ${s.paneId}`) {
    output({ process_info: s.pane }); return;
  }
  if (rest.join(' ') === `agent get ${s.agent}`) {
    output({ agent: s.agentInfo }); return;
  }
  if (rest[0] !== 'agent' || rest[1] !== 'prompt' || rest[2] !== s.agent || rest.length !== 4) {
    throw new Error('fixture command mismatch');
  }
  const command = rest[3];
  appendFileSync(s.calls, JSON.stringify({ command }) + '\n');
  if (s.mode === 'delivery_error') { process.stderr.write(s.secret); process.exitCode = 2; return; }
  if (s.mode === 'replace_trace') { renameSync(s.trace, `${s.trace}.old`); writeFileSync(s.trace, ''); }
  if (s.mode === 'rewrite_prefix') {
    const old = readFileSync(s.trace, 'utf8');
    writeFileSync(s.trace, old.replace('seed', 'SEED'));
  }
  const scope = { session_id: s.nativeSession, profile_id: s.profile };
  if (command === '/goal --report') {
    rpc('session/goal/get', scope, { ...scope, goal: s.goal });
  } else if (command === '/loop list --report') {
    const visible = s.loops.filter(loop => loop.status !== 'deleted');
    if (s.mode === 'keep_deleted_list') visible.push(...s.loops.filter(loop => loop.status === 'deleted').map(loop => ({ ...loop, status: 'paused' })));
    rpc('loop/list', scope, { ...scope, loops: visible });
  } else if (command === '/turn session') {
    rpc('session/hydrate', { session_id: s.nativeSession, include: ['turns'] },
      { session_id: s.nativeSession, turns: s.turns });
  } else if (command === '/goal resume' || command === '/goal pause') {
    if (s.mode === 'external_resume') s.goal.status = 'active';
    rpc('session/goal/get', scope, { ...scope, goal: s.goal });
    const status = command === '/goal resume' ? 'active' : 'paused';
    const params = { ...scope, objective: s.goal.objective, status, transition_actor: 'user' };
    s.goal = { ...s.goal, status };
    if (s.mode === 'changed_goal_after') s.goal.goal_id = 'foreign_goal';
    rpc('session/goal/set', params, { ...scope, goal: s.goal }, { omit: s.mode === 'missing_mutation_response' });
  } else if (command.startsWith('/loop every 60s ')) {
    // R5 is Rust: str::trim follows Unicode White_Space, including U+0085.
    const prompt = command.slice('/loop every 60s '.length).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, ''), now = Date.now();
    const loop = { ...scope, loop_id: 'loop_17', prompt, mode: 'fixed_interval', interval_seconds: 60,
      status: 'active', created_at_ms: now, updated_at_ms: now, next_run_at_ms: now + 60000,
      last_run_at_ms: null, expires_at_ms: now + 7 * 86400000 };
    s.loops.push(loop);
    const result = { ...scope, loop_id: loop.loop_id, loop, ok: true, status: 'active', created: true,
      fire: { queued: false, reason: 'waiting_for_schedule' } };
    rpc('loop/create', { ...scope, prompt, mode: 'fixed_interval', interval_seconds: 60 }, result,
      { omit: s.mode === 'missing_loop_response' });
  } else if (/^\/loop (pause|resume|delete) loop_17$/.test(command)) {
    const action = command.split(' ')[1];
    const loop = s.loops.find(loop => loop.loop_id === 'loop_17');
    if (!loop) throw new Error('fixture loop missing');
    loop.status = { pause: 'paused', resume: 'active', delete: 'deleted' }[action]; loop.updated_at_ms = Date.now();
    const retained = { ...loop };
    if (s.mode === 'foreign_loop_response') retained.loop_id = 'loop_foreign';
    const result = { session_id: s.nativeSession, loop_id: loop.loop_id, loop: retained, ok: true, status: loop.status };
    if (action === 'delete') Object.assign(result, { deleted: true, reaped_cron_job_ids: [] });
    rpc(`loop/${action}`, { session_id: s.nativeSession, loop_id: loop.loop_id }, result,
      { omit: s.mode === 'missing_loop_response' });
  } else throw new Error('fixture native command not supported');
  if (s.mode === 'replaced_process_after') s.rows[1].started = '1789220001.000022';
  save(); output({ delivered: true });
}
