/* Closed native browser protocol. Credentials exist only in the fragment exchange
 * and HttpOnly cookie; usage facts never enter local/session storage. */
export const NATIVE_MODE = process.env.NEXT_PUBLIC_HAGENCY_NATIVE_CONSOLE === '1';
const ROOT = '/console';
const KINDS = ['input', 'output', 'cacheWrite', 'cacheRead'];
const STATES = ['pending', 'reserved', 'active', 'rejected', 'revoked', 'failed'];
const CLEANUP = ['not_required', 'pending', 'uncertain', 'complete'];
const id = (v) => typeof v === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(v);
const number = (v) => Number.isSafeInteger(v) && v >= 0;
const object = (v, keys) => v !== null && typeof v === 'object' && !Array.isArray(v)
  && Object.keys(v).length === keys.length && keys.every((k) => Object.hasOwn(v, k));
const counts = (v, nullable) => object(v, KINDS) && KINDS.every((k) => number(v[k]) || (nullable && v[k] === null));
const evidence = (v) => v === 'host_attributed_untrusted_usage';
const period = (v) => v === null || (object(v, ['kind', 'key', 'observed_growth', 'known_growth_lower_bound', 'incomplete', 'observations', 'evidence'])
  && ['daily', 'monthly'].includes(v.kind) && typeof v.key === 'string' && v.key.length <= 10
  && counts(v.observed_growth, true) && counts(v.known_growth_lower_bound, false)
  && typeof v.incomplete === 'boolean' && number(v.observations) && evidence(v.evidence));

// Ceiling headroom published with the report (ADR-123): the drawn figure is
// always known; used and remaining are null when nothing was measured or no
// limit is declared. Unknown is rendered as unknown, never as zero.
const headroom = (v) => object(v, ['tokens_drawn', 'tokens_used', 'remaining_tokens']) && number(v.tokens_drawn)
  && (v.tokens_used === null || number(v.tokens_used)) && (v.remaining_tokens === null || number(v.remaining_tokens));

export function validateReport(v, selected) {
  const s = v?.summary;
  if (!object(v, ['engagement_id', 'at_ms', 'summary', 'daily', 'monthly', 'ceiling']) || v.engagement_id !== selected || !number(v.at_ms)
    || !headroom(v.ceiling)
    || !object(s, ['sources', 'latest_counts', 'known_high_water_lower_bound', 'latest_incomplete_sources', 'historically_incomplete_sources', 'regression_observations', 'evidence'])
    || !['sources', 'latest_incomplete_sources', 'historically_incomplete_sources', 'regression_observations'].every((k) => number(s[k]))
    || !(s.latest_counts === null || counts(s.latest_counts, true))
    || !(s.known_high_water_lower_bound === null || counts(s.known_high_water_lower_bound, false))
    || !evidence(s.evidence) || !period(v.daily) || !period(v.monthly)) throw new Error('invalid_native_response');
  return v;
}
export function validateEngagements(v) {
  if (!object(v, ['engagements', 'next_after']) || !Array.isArray(v.engagements) || v.engagements.length > 16
    || !(v.next_after === null || id(v.next_after)) || v.engagements.some((e) => !object(e, ['id', 'agentName', 'projectName', 'role', 'state', 'cleanup'])
      || !id(e.id) || typeof e.agentName !== 'string' || e.agentName.length > 128
      || !(e.projectName === null || (typeof e.projectName === 'string' && e.projectName.length <= 256))
      || typeof e.role !== 'string' || e.role.length > 128 || !STATES.includes(e.state) || !CLEANUP.includes(e.cleanup))) throw new Error('invalid_native_response');
  return v;
}
async function request(path, options = {}) {
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), 5000);
  try {
    const response = await fetch(`${ROOT}${path}`, { ...options, credentials: 'same-origin', cache: 'no-store', redirect: 'error', signal: abort.signal });
    if (!response.headers.get('content-type')?.startsWith('application/json')) throw new Error('invalid_native_response');
    const reader = response.body.getReader();
    const chunks = []; let size = 0;
    for (;;) {
      const { done, value } = await reader.read(); if (done) break;
      size += value.length;
      if (size > 64 * 1024) { await reader.cancel(); throw new Error('invalid_native_response'); }
      chunks.push(value);
    }
    const bytes = new Uint8Array(size); let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    const value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
    if (!response.ok) {
      if (value?.code === 'console_busy' && response.status === 429) throw new Error('busy');
      const known = { busy: 503, outcome_unknown: 504, resource_revision_conflict: 409, resource_publication_scope_required: 403, resource_configuration_scope_required: 403, resource_in_use: 409, invalid_resource_command: 400 };
      if (known[value?.code] === response.status) throw new Error(value.code);
      throw new Error(response.status === 401 ? 'console_access_required' : (response.status === 404 ? 'not_found' : 'native_unavailable'));
    }
    return value;
  } catch (error) {
    if (['console_access_required', 'not_found', 'invalid_native_response', 'invalid_selection', 'busy', 'outcome_unknown', 'resource_revision_conflict', 'resource_publication_scope_required', 'resource_configuration_scope_required', 'resource_in_use', 'invalid_resource_command'].includes(error.message)) throw error;
    if (options.method === 'DELETE') throw new Error('logout_unknown');
    if (['POST', 'PATCH'].includes(options.method) && path.startsWith('/api/resources')) throw new Error('outcome_unknown');
    throw new Error('native_unavailable');
  } finally { clearTimeout(timer); }
}
export function resourceView(location) { return /^\/console\/resources\/?$/.test(location.pathname); }
export function selection(location, field = 'engagement_id') {
  const query = new URLSearchParams(location.search);
  if ([...query.keys()].some((k) => k !== field) || query.getAll(field).length > 1) throw new Error('invalid_selection');
  const value = query.get(field);
  if (value !== null && !id(value)) throw new Error('invalid_selection');
  return value;
}
export async function exchangeAccess(location, history, previousLogout = Promise.resolve()) {
  const fragment = location.hash;
  if (!fragment) return;
  history.replaceState(history.state, '', `${location.pathname}${location.search}`);
  if (!/^#access=[a-f0-9]{64}$/.test(fragment)) throw new Error('console_access_required');
  await previousLogout;
  await request('/session', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ ticket: fragment.slice(8) }) });
}
export async function fetchNative(selected, after = '') {
  if ((selected !== null && !id(selected)) || (after && !id(after))) throw new Error('invalid_selection');
  const list = validateEngagements(await request(`/api/engagements?limit=16${after ? `&after=${after}` : ''}`));
  const chosen = selected ?? list.engagements[0]?.id ?? null;
  const report = chosen === null ? null : validateReport(await request(`/api/engagements/${chosen}/usage`), chosen);
  return { ...list, selected: chosen, report };
}
export async function logoutNative() { await request('/session', { method: 'DELETE' }); }

const revision = (v) => typeof v === 'string' && /^[a-f0-9]{64}$/.test(v);
const text = (v, max) => typeof v === 'string' && v.length <= max;
const optionalText = (v, max) => v === null || text(v, max);
const periodFields = (v) => !Object.hasOwn(v, 'period') || v.period === null || text(v.period, 64 * 1024);
const ceiling = (v) => v === null || (v && Object.keys(v).every((k) => ['tokens', 'period'].includes(k)) && (v.tokens === null || number(v.tokens)) && periodFields(v));
const validResource = (r) => !(!object(r, ['id', 'framework', 'model', 'provider', 'reasoning', 'ceiling', 'published', 'roles', 'revision'])
      || !id(r.id) || !text(r.framework, 64) || !text(r.model, 256) || !optionalText(r.provider, 128) || !optionalText(r.reasoning, 128)
      || !ceiling(r.ceiling) || typeof r.published !== 'boolean' || !Array.isArray(r.roles) || r.roles.length > 64 || !r.roles.every((v) => text(v, 64)) || !revision(r.revision));
export function validateResources(value) {
  if (!object(value, ['resources', 'roles', 'next_after', 'permissions']) || !Array.isArray(value.resources) || value.resources.length > 16
    || !(value.next_after === null || id(value.next_after)) || !object(value.permissions, ['publishResource', 'configureResource']) || typeof value.permissions.publishResource !== 'boolean' || typeof value.permissions.configureResource !== 'boolean'
    || value.resources.some((r) => !validResource(r))
    || !Array.isArray(value.roles) || value.roles.length !== 6 || value.roles.some((r) => !object(r, ['role', 'explicitPublication', 'available', 'crossFamily', 'defaultTier'])
      || !text(r.role, 64) || !(r.explicitPublication === null || typeof r.explicitPublication === 'boolean') || typeof r.available !== 'boolean' || typeof r.crossFamily !== 'boolean'
      || !(r.defaultTier === null || ['lightweight', 'medium', 'strong'].includes(r.defaultTier)))) throw new Error('invalid_native_response');
  return value;
}
export function validateBudget(v) {
  const amount = (n) => n === null || number(n);
  if (!object(v, ['scope', 'pool', 'seat', 'reserved', 'remainingTokens']) || v.scope !== 'resource' || !number(v.reserved) || !amount(v.remainingTokens)
    || !object(v.pool, ['ceiling', 'period', 'committed', 'remaining']) || !amount(v.pool.ceiling) || !optionalText(v.pool.period, 64 * 1024) || !number(v.pool.committed) || !amount(v.pool.remaining)
    || !object(v.seat, ['quota', 'period', 'committed', 'remaining', 'status']) || !amount(v.seat.quota) || !optionalText(v.seat.period, 64 * 1024) || !number(v.seat.committed) || !amount(v.seat.remaining)
    || !['undeclared', 'declared', 'period_mismatch'].includes(v.seat.status)) throw new Error('invalid_native_response');
  return v;
}
export async function fetchResources(selected, after = '') {
  if ((selected !== null && !id(selected)) || (after && !id(after))) throw new Error('invalid_selection');
  const list = validateResources(await request(`/api/resources?limit=16${after ? `&after=${after}` : ''}`));
  const chosen = selected ?? list.resources[0]?.id ?? null;
  const budget = chosen === null ? null : validateBudget(await request(`/api/resources/${chosen}/budget`));
  return { ...list, selected: chosen, budget, resourceConsole: true };
}
export async function publishResource(resource, published) {
  const value = await request(`/api/resources/${resource.id}/publication`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ expectedRevision: resource.revision, published }) });
  if (!object(value, ['resourceId', 'published', 'revision']) || value.resourceId !== resource.id || value.published !== published || !revision(value.revision)) throw new Error('outcome_unknown');
  return value;
}

export function configurationView(location) { return /^\/console\/resources\/new\/?$/.test(location.pathname); }
export function configurationSelection(location) {
  const query = new URLSearchParams(location.search);
  const edit = query.has('resource_id');
  return { mode: edit ? 'edit' : 'create', id: selection(location, edit ? 'resource_id' : 'source_resource_id') };
}
export function validateConfiguration(value, selected) {
  if (!object(value, ['resource', 'choices', 'modelTier', 'modelRoles']) || value.resource?.id !== selected
    || !Array.isArray(value.choices) || value.choices.length > 256
    || value.choices.some((c) => !object(c, ['model', 'reasoning', 'tier', 'roles']) || !text(c.model, 256) || !optionalText(c.reasoning, 128) || !['lightweight', 'medium', 'strong'].includes(c.tier) || !Array.isArray(c.roles) || c.roles.length > 6 || !c.roles.every((r) => text(r, 64)))
    || !(value.modelTier === null || ['lightweight', 'medium', 'strong'].includes(value.modelTier)) || !Array.isArray(value.modelRoles) || value.modelRoles.length > 6 || !value.modelRoles.every((r) => text(r, 64))) throw new Error('invalid_native_response');
  if (!validResource(value.resource)) throw new Error('invalid_native_response');
  return value;
}
export async function fetchConfiguration(entry, after = '') {
  const value = await fetchResources(entry.id, after);
  const editor = value.selected === null ? null : validateConfiguration(await request(`/api/resources/${value.selected}/configuration`), value.selected);
  return { ...value, editor, configurationConsole: true, editing: entry.mode === 'edit' };
}
export async function configureResource(resource, create, changes) {
  const payload = { expectedRevision: resource.revision, ...changes };
  if (create) payload.sourceResourceId = resource.id;
  const value = await request(create ? '/api/resources' : `/api/resources/${resource.id}/configuration`, { method: create ? 'POST' : 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) });
  if (!object(value, ['resourceId', 'revision', 'published']) || !/^resource_[a-f0-9]{24}$/.test(value.resourceId) || !revision(value.revision) || typeof value.published !== 'boolean' || (!create && value.resourceId !== resource.id) || (create && (!value.published || value.resourceId === resource.id))) throw new Error('outcome_unknown');
  return value;
}
