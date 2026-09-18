/* Private lifecycle protocol. The inspection credential never leaves memory
 * except in the exact POST body; response loss cannot create another decision. */
import { nativeRequest } from './native-api.js';

const keys = (v, names) => v !== null && typeof v === 'object' && !Array.isArray(v)
  && Object.keys(v).length === names.length && names.every((name) => Object.hasOwn(v, name));
const id = (v) => typeof v === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(v);
const hash = (v) => typeof v === 'string' && /^[a-f0-9]{64}$/.test(v);
const number = (v) => Number.isSafeInteger(v) && v >= 0;
const text = (v, max) => typeof v === 'string' && new TextEncoder().encode(v).length <= max;
const invalid = () => { throw new Error('invalid_native_response'); };
const TASK_KEYS = ['id', 'session_id', 'creator_session_id', 'title', 'description', 'priority', 'granularity', 'labels', 'parent_id', 'status', 'execution_epoch', 'created_at', 'updated_at', 'started_at', 'completed_at', 'heartbeat_at', 'waiting_reason', 'waiting_until'];
function task(v) {
  return keys(v, TASK_KEYS) && id(v.id) && id(v.session_id) && text(v.title, 4096) && text(v.description, 65536)
    && [v.creator_session_id, v.parent_id].every((s) => s === null || id(s))
    && ['created', 'accepted', 'in_progress', 'blocked', 'done'].includes(v.status)
    && ['execution_epoch', 'created_at', 'updated_at'].every((k) => number(v[k]))
    && ['started_at', 'completed_at', 'heartbeat_at'].every((k) => v[k] === null || number(v[k]))
    && ['waiting_reason', 'waiting_until'].every((k) => v[k] === null || text(v[k], 8192))
    && text(v.priority, 32) && text(v.granularity, 32) && Array.isArray(v.labels) && v.labels.length <= 64 && v.labels.every((s) => text(s, 256));
}
function entry(v) {
  if (!text(v?.path, 4096)) return false;
  if (v.kind === 'file') return keys(v, ['path', 'kind', 'bytes', 'sha256', 'readonly']) && number(v.bytes) && hash(v.sha256) && typeof v.readonly === 'boolean';
  if (v.kind === 'directory') return keys(v, ['path', 'kind', 'readonly']) && typeof v.readonly === 'boolean';
  return v.kind === 'symlink' && keys(v, ['path', 'kind', 'target']) && text(v.target, 4096);
}
export function validateStopped(v, agent, after = '') {
  if (!keys(v, ['engagementId', 'dispatches', 'nextAfter']) || v.engagementId !== agent
    || !Array.isArray(v.dispatches) || v.dispatches.length > 16 || !(v.nextAfter === null || id(v.nextAfter))) invalid();
  let previous = after;
  for (const row of v.dispatches) {
    if (!keys(row, ['dispatchId', 'sessionId', 'taskId', 'fence', 'reason', 'inspectionAvailable'])
      || !id(row.dispatchId) || row.dispatchId <= previous || !id(row.sessionId) || !(row.taskId === null || id(row.taskId))
      || !number(row.fence) || row.fence === 0 || !text(row.reason, 256) || typeof row.inspectionAvailable !== 'boolean') invalid();
    previous = row.dispatchId;
  }
  if (v.nextAfter !== null && (v.dispatches.length !== 16 || v.nextAfter !== previous)) invalid();
  return v;
}
export function validateInspection(v, agent, original) {
  const s = v?.snapshot, o = s?.observation, inventory = o?.inventory;
  if (!keys(v, ['inspectionId', 'inspectionToken', 'expiresAt', 'snapshot']) || !hash(v.inspectionId) || !hash(v.inspectionToken) || !number(v.expiresAt)
    || !keys(s, ['dispatchId', 'fence', 'receiptDigest', 'observation', 'task', 'route']) || s.dispatchId !== original || !number(s.fence) || s.fence === 0 || !hash(s.receiptDigest)
    || !keys(o, ['scope', 'workspace', 'inventory']) || !hash(o.scope) || !id(o.workspace) || !task(s.task)
    || !keys(inventory, ['profile', 'root', 'entries']) || inventory.profile !== 'stopped-content-inventory-v1'
    || !keys(inventory.root, ['platform', 'volume', 'object']) || !text(inventory.root.platform, 64) || !text(inventory.root.volume, 64)
    || !Array.isArray(inventory.root.object) || inventory.root.object.length !== 16 || !inventory.root.object.every((n) => number(n) && n < 256)
    || !Array.isArray(inventory.entries) || inventory.entries.length > 1024 || !inventory.entries.every(entry)) invalid();
  if (s.route !== null) {
    if (!text(s.route, 16384)) invalid();
    let route; try { route = JSON.parse(s.route); } catch { invalid(); }
    if (!keys(route, ['session_id', 'session_generation', 'engagement_id', 'fleet_id', 'project_id', 'registration_generation', 'server_name', 'room_id', 'room_generation', 'sender_mxid', 'device_id', 'transport_generation', 'owner_mxid', 'privacy', 'encrypted', 'thread_root'])
      || route.session_id !== s.task.session_id || route.engagement_id !== agent || typeof route.encrypted !== 'boolean' || !text(route.room_id, 4096)) invalid();
  }
  return v;
}
export async function fetchStopped(agent, after = '') {
  if (!id(agent) || (after && !id(after))) throw new Error('invalid_selection');
  return validateStopped(await nativeRequest(`/api/agents/${agent}/stopped-dispatches${after ? `?after=${after}` : ''}`), agent, after);
}
export async function inspectStopped(agent, original) {
  if (!id(agent) || !id(original)) throw new Error('invalid_selection');
  return validateInspection(await nativeRequest(`/api/agents/${agent}/stopped-dispatches/${original}/inspect`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}',
  }, 2 * 1024 * 1024), agent, original);
}
export function resolutionBody(inspection, action, note, instruction, requestId, replacementId) {
  if (!['continue', 'accept_completed', 'keep_blocked'].includes(action) || !id(requestId) || !note.trim() || !text(note, 2000)
    || (action === 'continue' && (!id(replacementId) || !instruction.trim() || !text(instruction, 8192)))) throw new Error('invalid_selection');
  const s = inspection.snapshot;
  const body = JSON.stringify({ original: s.dispatchId, requestId, inspectionId: inspection.inspectionId, inspectionToken: inspection.inspectionToken,
    action, operatorNote: note, replacement: action === 'continue' ? { id: replacementId, session_id: s.task.session_id, task_id: s.task.id,
      resources: [{ id: s.observation.workspace, exclusive: true }], payload: { instruction } } : null });
  if (!text(body, 16 * 1024)) throw new Error('invalid_selection');
  return body;
}
export async function resolveStopped(agent, frozenBody, expectedTask) {
  if (!id(agent)) throw new Error('invalid_selection');
  const input = JSON.parse(frozenBody);
  let v;
  try {
    v = await nativeRequest(`/api/agents/${agent}/resolve-stopped-dispatch`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: frozenBody });
  } catch (error) {
    if (error.message === 'invalid_native_response') throw new Error('outcome_unknown');
    throw error;
  }
  if (!keys(v, ['requestId', 'original', 'action', 'replacement', 'resolvedAt', 'task']) || v.requestId !== input.requestId || v.original !== input.original || v.action !== input.action
    || v.replacement !== (input.replacement?.id ?? null) || !number(v.resolvedAt) || !task(v.task)
    || v.task.id !== expectedTask.id || v.task.session_id !== expectedTask.session_id) throw new Error('outcome_unknown');
  return v;
}
