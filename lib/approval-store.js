import { createHash, randomBytes } from 'crypto';
import {
  chmodSync,
  closeSync,
  existsSync,
  fsyncSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from 'fs';
import path from 'path';
import { deriveExecutionAuthorization, SCOPED_APPROVAL_ACTIONS } from './execution-authorization.js';
import { isDeepStrictEqual } from 'node:util';

const STORE_VERSION = 2;
const PROJECTION_PAYLOAD_MAX_BYTES = 64 * 1024;
const MARKER_ASSOCIATION_LIMIT = 64;
const MARKER_EVENT_TYPE = 'com.agentchat.approval.room.v1';
const MARKER_V2_EVENT_TYPE = 'com.agentchat.approval.room.v2';
const MARKER_V2_CHANNEL = 'room_marker_v2';
const MARKER_RETIREMENT_CHANNEL = 'room_marker_v1_retirement';
const MAX_RETRY_DELAY_MS = 5 * 60 * 1000;
const EVENT_ID_RE = /^\$[^\s]{1,254}$/;
const DEFAULT_TTL_MS = 5 * 60 * 1000;
const DEFAULT_AUDIT_LIMIT = 2000;
const MXID_RE = /^@[^:\s]+:[^\s]+$/;
const ROOM_ID_RE = /^![^:\s]+:[^\s]+$/;
const TERMINAL_STATES = new Set(['approved', 'denied', 'expired', 'consumed']);

export class ApprovalStoreError extends Error {
  constructor(code, message) {
    super(message);
    this.name = 'ApprovalStoreError';
    this.code = code;
  }
}

function text(value, field, max = 512) {
  const normalized = typeof value === 'string' ? value.trim() : '';
  if (!normalized || normalized.length > max) {
    throw new ApprovalStoreError('bad_request', `${field} must be 1..${max} characters`);
  }
  return normalized;
}

function optionalText(value, max = 4096) {
  if (value === null || value === undefined) return '';
  const normalized = typeof value === 'string' ? value.trim() : '';
  if (normalized.length > max) {
    throw new ApprovalStoreError('bad_request', `text exceeds ${max} characters`);
  }
  return normalized;
}

function canonicalMatrixContent(value, seen = new WeakSet()) {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) {
      throw new ApprovalStoreError('bad_request', 'prepared_payload numbers must be finite');
    }
    return value;
  }
  if (!value || typeof value !== 'object') {
    throw new ApprovalStoreError('bad_request', 'prepared_payload must contain only JSON values');
  }
  if (seen.has(value)) throw new ApprovalStoreError('bad_request', 'prepared_payload must not be cyclic');
  seen.add(value);
  if (Array.isArray(value)) {
    const result = value.map((item) => canonicalMatrixContent(item, seen));
    seen.delete(value);
    return result;
  }
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    throw new ApprovalStoreError('bad_request', 'prepared_payload must contain plain JSON objects');
  }
  const result = Object.fromEntries(
    Object.entries(value).map(([key, item]) => [key, canonicalMatrixContent(item, seen)]),
  );
  seen.delete(value);
  return result;
}

function fullMxid(value, field = 'owner_mxid') {
  const normalized = text(value, field, 255);
  if (!MXID_RE.test(normalized)) {
    throw new ApprovalStoreError('bad_request', `${field} must be a full Matrix MXID`);
  }
  return normalized;
}

function roomId(value, field) {
  const normalized = text(value, field, 255);
  if (!ROOM_ID_RE.test(normalized)) {
    throw new ApprovalStoreError('bad_request', `${field} must be a full Matrix room id`);
  }
  return normalized;
}

function bindingKey(agent, projectRoomId) {
  return `${agent}\u0000${projectRoomId}`;
}

function compareText(left, right) {
  return left < right ? -1 : (left > right ? 1 : 0);
}

function digestRequest(input) {
  const canonical = JSON.stringify({
    agent: input.agent,
    runtime: input.runtime,
    project: input.project,
    project_room_id: input.projectRoomId,
    owner_mxid: input.ownerMxid,
    owner_dm_room_id: input.ownerDmRoomId,
    upstream_request_id: input.upstreamRequestId,
    tool_name: input.toolName,
    description: input.description,
    input_preview: input.inputPreview,
    ...(input.authorization ? { authorization: input.authorization, binding_authority: input.bindingAuthority } : {}),
    router_approval_id: input.routerApprovalId || null,
  });
  return createHash('sha256').update(canonical).digest('hex');
}

function stableDecisionEventId(record) {
  if (record.decisionEventId) return record.decisionEventId;
  const material = JSON.stringify({
    request_id: record.id,
    decision: record.decision === 'allow' ? 'allow' : 'deny',
    decided_at: record.decidedAt || record.expiresAt || record.createdAt,
    input_digest: record.inputDigest || null,
  });
  return `approval_decision_${createHash('sha256').update(material).digest('hex')}`;
}

function publicRecord(record) {
  if (!record) return null;
  return {
    id: record.id,
    agent: record.agent,
    runtime: record.runtime,
    project: record.project,
    project_room_id: record.projectRoomId,
    owner_mxid: record.ownerMxid,
    owner_dm_room_id: record.ownerDmRoomId,
    upstream_request_id: record.upstreamRequestId,
    router_approval_id: record.routerApprovalId || null,
    input_digest: record.inputDigest,
    status: record.status,
    decision: record.decision || null,
    decision_event_id: record.decisionEventId || (record.decision ? stableDecisionEventId(record) : null),
    denial_reason: record.denialReason || null,
    created_at: record.createdAt,
    expires_at: record.expiresAt,
    decided_at: record.decidedAt || null,
    consumed_at: record.consumedAt || null,
    ...(record.authorizationAction ? { authorization_action: record.authorizationAction } : {}),
    ...(record.grantId ? { grant_id: record.grantId } : {}),
  };
}

function matrixRecord(record) {
  const safe = publicRecord(record);
  if (!safe) return null;
  return {
    ...safe,
    tool_name: record.toolName,
    description: record.description,
    input_preview: record.inputPreview,
    ...(record.authorization ? { reusable_scope: {
      description: record.authorization.scope.description,
      workspace: record.authorization.workspace,
      task_id: record.authorization.taskId,
    } } : {}),
  };
}

export class ApprovalStore {
  constructor(filePath, options = {}) {
    this.filePath = path.resolve(filePath);
    this.now = typeof options.now === 'function' ? options.now : () => Date.now();
    this.ttlMs = Number.isFinite(options.ttlMs) && options.ttlMs > 0
      ? Math.floor(options.ttlMs)
      : DEFAULT_TTL_MS;
    this.fsFault = typeof options.fsFault === 'function' ? options.fsFault : () => {};
    this.migrationBatchSize = Number.isFinite(options.migrationBatchSize) ? Math.max(1, Math.floor(options.migrationBatchSize)) : 100;
    this._persistenceHealth = { degraded: false, error: null };
    this.auditLimit = Number.isFinite(options.auditLimit) && options.auditLimit > 0
      ? Math.floor(options.auditLimit)
      : DEFAULT_AUDIT_LIMIT;
    this.isTaskActive = typeof options.isTaskActive === 'function' ? options.isTaskActive : () => false;
    this.getTaskEpoch = typeof options.getTaskEpoch === 'function' ? options.getTaskEpoch : () => 0;
    this.isAgentCurrent = typeof options.isAgentCurrent === 'function' ? options.isAgentCurrent : () => true;
    this.state = this._load();
    this._persistedState = structuredClone(this.state);
    this._initializeIndexes();
    this._migrateLegacy();
    if (this._migrationChanged) { this._save(); this._migrationChanged = false; }
    this._installMutationGuards();
  }

  _installMutationGuards() {
    const names = [
      'observeBindingMembership', 'upsertBinding', 'deactivateBinding', 'removeBinding',
      'createRequest', 'submitMatrixVerdict', 'denyPending', 'consumeDecision',
      'sweepExpired', 'migrateLegacyBatch', 'prepareProjection', 'beginProjectionSend',
      'receiptProjection', 'retryProjection',
      'syncBindingMarker', 'prepareMarker', 'beginMarkerSend', 'receiptMarker', 'retryMarker',
      'migrateMarkerRoomsV2', 'reconcileMarkerRetirements',
      'upsertProjectionPublisher', 'attestLegacyOriginal',
    ];
    for (const name of names) {
      const mutate = this[name].bind(this);
      this[name] = (...args) => this._runMutation(() => mutate(...args));
    }
    for (const name of ['getRequest', 'listRequests']) {
      const read = this[name].bind(this);
      this[name] = (...args) => this._runLazyRead(() => read(...args));
    }
  }

  _runLazyRead(read) {
    const before = structuredClone(this.state);
    try {
      return read();
    } catch (error) {
      this.state = before;
      throw error;
    }
  }

  _runMutation(mutate) {
    if (this._persistenceHealth.degraded) {
      throw new ApprovalStoreError(
        'persistence_degraded',
        'approval store requires reload after a post-commit durability failure',
      );
    }
    const before = structuredClone(this.state);
    try {
      return mutate();
    } catch (error) {
      this.state = before;
      throw error;
    }
  }

  _load() {
    if (!existsSync(this.filePath)) {
      return {
        version: STORE_VERSION,
        bindings: {},
        requests: {},
        grants: {},
        audit: [],
        projectionOutbox: [],
        migration: { pendingIds: [], offset: 0 },
        expiryIndex: [],
        markerScopes: {},
        markerOutbox: [],
        markerRoomScopes: {},
        markerRoomMigration: { cursor: null, complete: false },
        retiredMarkerRooms: {},
        markerReceiptSequence: 0,
        projectionPublishers: {},
        legacyOriginalEvidence: {},
      };
    }
    try {
      const parsed = JSON.parse(readFileSync(this.filePath, 'utf8'));
      return {
        version: Number(parsed?.version) || 1,
        bindings: parsed?.bindings && typeof parsed.bindings === 'object' ? parsed.bindings : {},
        requests: parsed?.requests && typeof parsed.requests === 'object' ? parsed.requests : {},
        grants: parsed?.grants && typeof parsed.grants === 'object' && !Array.isArray(parsed.grants) ? parsed.grants : {},
        audit: Array.isArray(parsed?.audit) ? parsed.audit.slice(-this.auditLimit) : [],
        projectionOutbox: Array.isArray(parsed?.projectionOutbox) ? parsed.projectionOutbox : [],
        migration: parsed?.migration && typeof parsed.migration === 'object' ? parsed.migration : null,
        expiryIndex: Array.isArray(parsed?.expiryIndex) ? parsed.expiryIndex : null,
        markerScopes: parsed?.markerScopes && typeof parsed.markerScopes === 'object' ? parsed.markerScopes : {},
        markerOutbox: Array.isArray(parsed?.markerOutbox) ? parsed.markerOutbox : [],
        markerRoomScopes: parsed?.markerRoomScopes && typeof parsed.markerRoomScopes === 'object'
          ? parsed.markerRoomScopes : {},
        markerRoomMigration: parsed?.markerRoomMigration && typeof parsed.markerRoomMigration === 'object'
          ? parsed.markerRoomMigration : { cursor: null, complete: false },
        retiredMarkerRooms: parsed?.retiredMarkerRooms && typeof parsed.retiredMarkerRooms === 'object'
          ? parsed.retiredMarkerRooms : {},
        markerReceiptSequence: Number.isSafeInteger(parsed?.markerReceiptSequence)
          ? parsed.markerReceiptSequence : 0,
        legacyOriginalEvidence: parsed?.legacyOriginalEvidence && typeof parsed.legacyOriginalEvidence === 'object'
          && !Array.isArray(parsed.legacyOriginalEvidence) ? parsed.legacyOriginalEvidence : {},
        projectionPublishers: parsed?.projectionPublishers && typeof parsed.projectionPublishers === 'object'
          ? parsed.projectionPublishers : {},
      };
    } catch (error) {
      throw new ApprovalStoreError('persistence_failed', `failed to load approval store: ${error.message}`);
    }
  }

  _save() {
    const before = structuredClone(this._persistedState || this.state);
    if (this._persistenceHealth.degraded) {
      this.state = before;
      throw new ApprovalStoreError('persistence_degraded', 'approval store requires reload after a post-commit durability failure');
    }
    const dir = path.dirname(this.filePath);
    const tmp = `${this.filePath}.${process.pid}.${randomBytes(6).toString('hex')}.tmp`;
    const bytes = JSON.stringify(this.state, null, 2) + '\n';
    let fd = null;
    let committed = false;
    try {
      mkdirSync(dir, { recursive: true, mode: 0o700 });
      writeFileSync(tmp, bytes, { mode: 0o600 });
      chmodSync(tmp, 0o600);
      fd = openSync(tmp, 'r'); fsyncSync(fd); closeSync(fd); fd = null;
      this.fsFault('beforeRename');
      renameSync(tmp, this.filePath);
      committed = true;
      this._persistedState = structuredClone(this.state);
      this.fsFault('afterRename');
      const dirFd = openSync(dir, 'r');
      try { fsyncSync(dirFd); } finally { closeSync(dirFd); }
      this._persistenceHealth = { degraded: false, error: null };
      return { committed: true, durabilityError: null };
    } catch (error) {
      if (fd !== null) try { closeSync(fd); } catch {}
      if (!committed) {
        this.state = before;
        throw new ApprovalStoreError('persistence_failed', `failed to persist approval store: ${error.message}`);
      }
      this._persistenceHealth = { degraded: true, error: String(error.message || error) };
      return { committed: true, durabilityError: this._persistenceHealth.error };
    }
  }

  persistenceHealth() { return { ...this._persistenceHealth }; }

  getProjectionRevision(id) {
    return Number(this.state.requests[id]?.projectionRevision) || null;
  }

  getProjectionRequest(id) {
    const record = this.state.requests[id];
    return record ? matrixRecord(record) : null;
  }

  _initializeIndexes() {
    let changed = false;
    if (!this.state.markerScopes || typeof this.state.markerScopes !== 'object') { this.state.markerScopes = {}; changed = true; }
    if (!Array.isArray(this.state.markerOutbox)) { this.state.markerOutbox = []; changed = true; }
    if (!this.state.markerRoomScopes || typeof this.state.markerRoomScopes !== 'object') {
      this.state.markerRoomScopes = {}; changed = true;
    }
    if (!this.state.markerRoomMigration || typeof this.state.markerRoomMigration !== 'object') {
      this.state.markerRoomMigration = { cursor: null, complete: false }; changed = true;
    }
    if (!this.state.retiredMarkerRooms || typeof this.state.retiredMarkerRooms !== 'object') {
      this.state.retiredMarkerRooms = {}; changed = true;
    }
    if (!this.state.projectionPublishers || typeof this.state.projectionPublishers !== 'object') {
      this.state.projectionPublishers = {}; changed = true;
    }
    if (!Array.isArray(this.state.expiryIndex)) {
      this.state.expiryIndex = Object.values(this.state.requests)
        .filter((record) => record.status === 'pending')
        .map((record) => ({ requestId: record.id, expiresAt: Number(record.expiresAt || 0) }))
        .sort((a, b) => a.expiresAt - b.expiresAt || a.requestId.localeCompare(b.requestId));
      changed = true;
    }
    if (this.state.version < STORE_VERSION && !Array.isArray(this.state.migration?.pendingIds)) {
      this.state.migration = { pendingIds: Object.keys(this.state.requests), offset: 0 };
      changed = true;
    }
    if (changed) this._migrationChanged = true;
  }

  _expiryLess(left, right) {
    return left.expiresAt < right.expiresAt
      || (left.expiresAt === right.expiresAt && left.requestId.localeCompare(right.requestId) < 0);
  }

  _expiryPush(entry) {
    const heap = this.state.expiryIndex;
    heap.push(entry);
    let index = heap.length - 1;
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      if (!this._expiryLess(heap[index], heap[parent])) break;
      [heap[index], heap[parent]] = [heap[parent], heap[index]];
      index = parent;
    }
  }

  _expiryPop() {
    const heap = this.state.expiryIndex;
    const first = heap[0];
    const last = heap.pop();
    if (heap.length && last) {
      heap[0] = last;
      let index = 0;
      while (true) {
        const left = index * 2 + 1;
        const right = left + 1;
        let next = index;
        if (left < heap.length && this._expiryLess(heap[left], heap[next])) next = left;
        if (right < heap.length && this._expiryLess(heap[right], heap[next])) next = right;
        if (next === index) break;
        [heap[index], heap[next]] = [heap[next], heap[index]];
        index = next;
      }
    }
    return first;
  }

  _indexPending(record) {
    this.state.expiryIndex ||= [];
    this._expiryPush({ requestId: record.id, expiresAt: Number(record.expiresAt || 0) });
  }

  _migrateLegacy() {
    if (this.state.version >= STORE_VERSION) return 0;
    const migration = this.state.migration;
    const start = Number(migration.offset) || 0;
    const end = Math.min(start + this.migrationBatchSize, migration.pendingIds.length);
    let migrated = 0;
    for (let index = start; index < end; index += 1) {
      const record = this.state.requests[migration.pendingIds[index]];
      if (!record || record.projectionRevision) continue;
      record.projectionRevision = 1;
      record.projectionMigration = 'legacy_v1';
      if (record.status === 'pending' && this.now() >= Number(record.expiresAt || 0)) {
        record.status = 'expired';
        record.decision = 'deny';
        record.denialReason = 'approval_expired';
        record.decidedAt = this.now();
        record.decisionEventId = stableDecisionEventId(record);
      }
      this._enqueueProjection(record, 'private_status', record.status, 'legacy_v1');
      migrated += 1;
    }
    migration.offset = end;
    if (end >= migration.pendingIds.length) {
      this.state.version = STORE_VERSION;
      this.state.migration = { pendingIds: [], offset: 0 };
    }
    this._migrationChanged = end > start || this.state.version === STORE_VERSION;
    return migrated;
  }

  migrateLegacyBatch() {
    const count = this._migrateLegacy();
    if (this._migrationChanged) {
      this._save();
      this._migrationChanged = false;
    }
    return {
      migrated: count,
      remaining: Math.max(0, (this.state.migration?.pendingIds?.length || 0) - (this.state.migration?.offset || 0)),
      complete: this.state.version === STORE_VERSION,
    };
  }

  _enqueueProjection(record, channel, state = record.status, migrationKind = 'native_v2') {
    this.state.projectionOutbox ||= [];
    const revision = record.projectionRevision;
    const targetRoomId = channel === 'public_notice' ? record.projectRoomId : record.ownerDmRoomId;
    if (!targetRoomId) return;
    const identity = `${record.id}\0${revision}\0${channel}`;
    if (this.state.projectionOutbox.some((row) => row.identity === identity)) return;
    this.state.projectionOutbox.push({ identity, casToken: createHash('sha256').update(identity).digest('hex'), requestId: record.id,
      revision, channel, state, targetRoomId,
      migrationKind, plan: null, attemptState: 'unprepared', eventId: null, nextAttemptAt: 0 });
  }

  _advanceProjection(record, state) {
    if (!record.projectionRevision) {
      record.projectionRevision = 1;
      record.projectionMigration = 'legacy_v1';
      this._enqueueProjection(record, 'private_status', state, 'legacy_v1');
      return;
    }
    for (const row of this.state.projectionOutbox || []) {
      if (row.requestId === record.id
        && row.state === 'pending'
        && !row.eventId) {
        row.superseded = true;
      }
    }
    record.projectionRevision = (record.projectionRevision || 1) + 1;
    this._enqueueProjection(record, 'private_status', state, record.projectionMigration || 'native_v2');
  }

  _audit(type, detail = {}) {
    this.state.audit.push({ type, at: this.now(), ...detail });
    if (this.state.audit.length > this.auditLimit) {
      this.state.audit.splice(0, this.state.audit.length - this.auditLimit);
    }
  }

  _withRollback(change, { requestId, grantId, expireRequests = false } = {}) {
    // Completed payloads are immutable in these operations. Snapshot only the
    // records this operation can change, plus the maps and bounded audit list.
    const previous = { ...this.state, requests: { ...this.state.requests }, grants: { ...this.state.grants }, audit: [...this.state.audit] };
    for (const [id, record] of Object.entries(previous.requests)) {
      if (id === requestId || expireRequests && record.status === 'pending') previous.requests[id] = { ...record };
    }
    if (grantId && previous.grants[grantId]) previous.grants[grantId] = { ...previous.grants[grantId] };
    try { return change(); } catch (error) { this.state = previous; throw error; }
  }

  _bindingAuthority(binding) {
    return binding.authorityId || createHash('sha256').update(JSON.stringify([
      binding.agent, binding.project, binding.projectRoomId, binding.ownerMxid,
      binding.ownerDmRoomId, binding.createdAt,
    ])).digest('hex');
  }

  _bindingCurrent(record) {
    const binding = this.state.bindings[bindingKey(record.agent, record.projectRoomId)];
    return binding && binding.active !== false && binding.project === record.project
      && binding.ownerMxid === record.ownerMxid && binding.ownerDmRoomId === record.ownerDmRoomId
      && (!record.bindingAuthority || record.bindingAuthority === this._bindingAuthority(binding));
  }

  _grantCurrent(grant) {
    return grant && grant.revokedAt == null && this._bindingCurrent(grant)
      && this.isAgentCurrent(grant.agent, grant.agentId)
      && (grant.scope === 'always' || (grant.scope === 'task' && this.isTaskActive(grant.taskId)
        && grant.taskEpoch === this.getTaskEpoch(grant.taskId)));
  }

  listGrants(agent, agentId = null) {
    return Object.values(this.state.grants).filter(g => g.agent === agent && (!agentId || g.agentId === agentId))
      .map(g => ({ ...g, active: Boolean(this._grantCurrent(g)) }));
  }

  revokeGrant(id, agent, actor) {
    return this._withRollback(() => {
      const grant = this.state.grants[id];
      if (!grant || grant.agent !== agent) return null;
      if (grant.revokedAt == null) {
        grant.revokedAt = this.now(); grant.revokedBy = text(actor, 'actor', 255);
        this._audit('authorization.revoked', { grantId: id, agent, actor: grant.revokedBy });
        this._save();
      }
      return { ...grant, active: false };
    }, { grantId: id });
  }

  _expire(record, now = this.now()) {
    if (record?.status !== 'pending' || now < Number(record.expiresAt || 0)) return false;
    record.status = 'expired';
    record.decision = 'deny';
    record.denialReason = 'approval_expired';
    record.decidedAt = now;
    record.decisionEventId = stableDecisionEventId(record);
    this._advanceProjection(record, 'expired');
    this._audit('approval.expired', { requestId: record.id, agent: record.agent });
    return true;
  }

  /**
   * Record whether the agent is actually joined to a bound room.
   *
   * SEPARATE FROM upsertBinding ON PURPOSE. Upserting a binding is a governance act — it asserts a
   * project may reach an agent, and it can deny pending requests when the owner changes. Observing
   * membership asserts nothing about permission; it reports what the homeserver says. Folding the
   * observation into the binding write would let a routine liveness check carry the authority of a
   * governance decision, and would make an unreachable room look like a withdrawn binding.
   *
   * Returns null when no such binding exists — an observation about a binding nobody made is not
   * something to store.
   */
  observeBindingMembership(input) {
    const agent = text(input?.agent, 'agent', 128);
    const projectRoomId = roomId(input?.project_room_id ?? input?.projectRoomId, 'project_room_id');
    const key = bindingKey(agent, projectRoomId);
    const binding = this.state.bindings[key];
    if (!binding) return null;
    const joined = input?.agent_joined ?? input?.agentJoined;
    if (typeof joined !== 'boolean') {
      throw new ApprovalStoreError('bad_request', 'agent_joined must be a boolean');
    }
    binding.agentJoined = joined;
    binding.membershipCheckedAt = this.now();
    // `_save()`, the convention every other mutator here uses. My first version called a
    // `persist()` that does not exist on this class — an error `node --check` cannot see, and the
    // same shape as the ReferenceError that killed the bot invite poll.
    this._save();
    return binding;
  }

  upsertBinding(input) {
    const now = this.now();
    const agent = text(input?.agent, 'agent', 128);
    const project = text(input?.project, 'project', 255);
    const projectRoomId = roomId(input?.project_room_id ?? input?.projectRoomId, 'project_room_id');
    const ownerMxid = fullMxid(input?.owner_mxid ?? input?.ownerMxid);
    const ownerDmRoomId = roomId(input?.owner_dm_room_id ?? input?.ownerDmRoomId, 'owner_dm_room_id');
    const key = bindingKey(agent, projectRoomId);
    const previous = this.state.bindings[key] || null;
    const changedOwner = previous && (
      previous.ownerMxid !== ownerMxid || previous.ownerDmRoomId !== ownerDmRoomId
    );
    const binding = {
      agent,
      project,
      projectRoomId,
      ownerMxid,
      ownerDmRoomId,
      authorityId: previous && !changedOwner && previous.project === project && previous.active !== false
        ? this._bindingAuthority(previous) : randomBytes(16).toString('hex'),
      active: true,
      createdAt: previous?.createdAt || now,
      updatedAt: now,
      /*
       * MEMBERSHIP IS CARRIED FORWARD, NOT RESET, by a binding write.
       *
       * A binding says a project may reach an agent. Whether the agent is actually IN that room is
       * a separate fact, owned by the bridge and observed against the homeserver — and the two can
       * disagree, which is the whole reason this field exists. Re-pushing a binding must not erase
       * the last observation, or every push would reset reachability to unknown and the console
       * would flicker between "confirmed" and "never checked".
       */
      agentJoined: previous?.agentJoined ?? null,
      membershipCheckedAt: previous?.membershipCheckedAt ?? null,
    };
    this.state.bindings[key] = binding;
    this._refreshBindingMarkers(agent);
    if (changedOwner) {
      for (const record of Object.values(this.state.requests)) {
        if (record.status !== 'pending' || record.agent !== agent || record.projectRoomId !== projectRoomId) continue;
        record.status = 'denied';
        record.decision = 'deny';
        record.denialReason = 'owner_binding_changed';
        record.decidedAt = now;
        record.decisionEventId = stableDecisionEventId(record);
        this._advanceProjection(record, 'denied');
        this._audit('approval.denied', { requestId: record.id, agent, reason: 'owner_binding_changed' });
      }
    }
    this._audit(previous ? 'binding.updated' : 'binding.created', {
      agent,
      project,
      projectRoomId,
      ownerMxid,
    });
    this._save();
    return { ...binding };
  }

  /**
   * Deactivate a binding without forgetting it.
   *
   * The compliance-correct shape, and the operator stated the rule: 「记录要留存，只是停用退役，删除
   * 会有合规问题」. A binding is evidence of who invited what (ADR-002), so it is kept; what changes
   * is that it stops CLAIMING the project can reach the agent.
   *
   * `listBindings` already filters on `active !== false`, so deactivation is enough to remove it from
   * every read — nothing had to learn a new state. `reason` is recorded because a binding that went
   * quiet without one is indistinguishable from a contributor withdrawing permission, and only they
   * may do that.
   */
  deactivateBinding(agentValue, projectRoomIdValue, reason = null) {
    const agent = text(agentValue, 'agent', 128);
    const projectRoomId = roomId(projectRoomIdValue, 'project_room_id');
    const record = this.state.bindings[bindingKey(agent, projectRoomId)];
    if (!record) return null;
    if (record.active === false) return record;
    record.active = false;
    record.deactivatedAt = this.now();
    record.deactivatedReason = reason ? String(reason).slice(0, 256) : null;
    this._refreshBindingMarkers(agent);
    this._audit('binding_deactivated', { agent, projectRoomId, reason: record.deactivatedReason });
    this._save();
    return record;
  }

  removeBinding(agentValue, projectRoomIdValue) {
    const agent = text(agentValue, 'agent', 128);
    const projectRoomId = roomId(projectRoomIdValue, 'project_room_id');
    const key = bindingKey(agent, projectRoomId);
    const binding = this.state.bindings[key];
    if (!binding) return null;
    const now = this.now();
    delete this.state.bindings[key];
    this._refreshBindingMarkers(agent);
    for (const record of Object.values(this.state.requests)) {
      if (record.status !== 'pending' || record.agent !== agent || record.projectRoomId !== projectRoomId) continue;
      record.status = 'denied';
      record.decision = 'deny';
      record.denialReason = 'owner_binding_removed';
      record.decidedAt = now;
      record.decisionEventId = stableDecisionEventId(record);
      this._advanceProjection(record, 'denied');
    }
    this._audit('binding.removed', { agent, projectRoomId, ownerMxid: binding.ownerMxid });
    this._save();
    return { ...binding };
  }

  listBindings(filters = {}) {
    const agent = typeof filters.agent === 'string' ? filters.agent.trim() : '';
    const project = typeof filters.project === 'string' ? filters.project.trim() : '';
    const projectRoomId = typeof filters.project_room_id === 'string'
      ? filters.project_room_id.trim()
      : (typeof filters.projectRoomId === 'string' ? filters.projectRoomId.trim() : '');
    /*
     * INACTIVE BINDINGS ARE HIDDEN BY DEFAULT, and that default is load-bearing: every existing caller
     * asks "which projects can reach this agent right now", and a deactivated binding is no longer an
     * answer to that. `includeInactive` is opt-in for the one caller that asks a different question —
     * "who has EVER served this project" — which decision 7 exists to keep answerable, since it
     * deactivates rather than deletes.
     */
    const includeInactive = filters.includeInactive === true;
    return Object.values(this.state.bindings)
      .filter((binding) => includeInactive || binding?.active !== false)
      .filter((binding) => !agent || binding.agent === agent)
      .filter((binding) => !project || binding.project === project || binding.projectRoomId === project)
      .filter((binding) => !projectRoomId || binding.projectRoomId === projectRoomId)
      .map((binding) => ({ ...binding }));
  }

  listMarkerRooms(options = {}) {
    const requestedLimit = options.limit === undefined ? 20 : options.limit;
    if (!Number.isInteger(requestedLimit) || requestedLimit < 1) {
      throw new ApprovalStoreError('bad_request', 'room inventory limit must be a positive integer');
    }
    const limit = Math.min(requestedLimit, 200);
    let after = null;
    if (options.after !== undefined && options.after !== null) {
      try {
        if (typeof options.after !== 'string' || options.after.length > 2048
          || !/^[A-Za-z0-9_-]+$/.test(options.after)) throw new Error('invalid');
        const decoded = JSON.parse(Buffer.from(options.after, 'base64url').toString('utf8'));
        if (!Array.isArray(decoded) || decoded.length !== 1
          || typeof decoded[0] !== 'string' || decoded[0].length > 255
          || !ROOM_ID_RE.test(decoded[0])) throw new Error('invalid');
        [after] = decoded;
      } catch {
        throw new ApprovalStoreError('bad_request', 'invalid room inventory cursor');
      }
    }

    // A due queue cannot discover a room after all its receipts have been saved.
    // This is a bounded response, not a claim that the retained-state scan is O(limit).
    const rooms = new Map();
    const add = (room, owner = null) => {
      if (typeof room !== 'string' || room.length > 255 || !ROOM_ID_RE.test(room)) return null;
      if (!rooms.has(room)) rooms.set(room, { active: [], owners: new Set() });
      const entry = rooms.get(room);
      if (typeof owner === 'string' && MXID_RE.test(owner)) entry.owners.add(owner);
      return entry;
    };
    for (const binding of Object.values(this.state.bindings)) {
      const entry = add(binding.ownerDmRoomId);
      if (entry && binding.active !== false) {
        entry.active.push(binding);
        entry.owners.add(binding.ownerMxid);
      }
    }
    for (const scope of Object.values(this.state.markerScopes)) add(scope.approvalRoomId, scope.ownerMxid);
    for (const scope of Object.values(this.state.markerRoomScopes)) add(scope.approvalRoomId, scope.ownerMxid);
    for (const row of this.state.markerOutbox) add(row.approvalRoomId, row.ownerMxid);
    for (const room of Object.keys(this.state.retiredMarkerRooms)) add(room);

    return [...rooms.keys()].sort(compareText)
      .filter(room => after === null || compareText(room, after) > 0)
      .slice(0, limit).map(room => {
        const entry = rooms.get(room);
        const owner = entry.owners.size === 1 ? [...entry.owners][0] : null;
        const conflict = entry.owners.size > 1;
        const eligible = !conflict && typeof owner === 'string' && MXID_RE.test(owner)
          && entry.active.length > 0;
        return {
          approval_room_id: room,
          owner_mxid: conflict ? null : owner,
          agent: eligible ? entry.active.map(binding => binding.agent).sort(compareText)[0] : null,
          synchronization_allowed: eligible,
          conflict,
          cursor: Buffer.from(JSON.stringify([room]), 'utf8').toString('base64url'),
        };
      });
  }

  _markerScopeKey(agent, approvalRoomId) { return `${agent}\0${approvalRoomId}`; }

  _markerAssociations(scope) {
    const active = Object.values(this.state.bindings)
      .filter((binding) => binding.agent === scope.agent && binding.ownerMxid === scope.ownerMxid
        && binding.ownerDmRoomId === scope.approvalRoomId && binding.active !== false)
      .map((binding) => binding.projectRoomId);
    const values = new Map((scope.associations || []).map((entry) => [entry.project_room_id, false]));
    for (const id of active) values.set(id, true);
    return [...values].sort(([left], [right]) => left.localeCompare(right))
      .map(([project_room_id, isActive]) => ({ project_room_id, active: isActive }));
  }

  _advanceMarker(scope, associations, publisherMxid = scope.publisherMxid) {
    if (associations.length > MARKER_ASSOCIATION_LIMIT) {
      throw new ApprovalStoreError('conflict', 'approval room marker association limit exceeded');
    }
    const previous = JSON.stringify([scope.associations || [], scope.publisherMxid || null]);
    const next = JSON.stringify([associations, publisherMxid || null]);
    if (previous === next && scope.generation) return false;
    for (const row of this.state.markerOutbox) {
      if (row.approvalRoomId === scope.approvalRoomId && row.agent === scope.agent && !row.eventId) row.superseded = true;
    }
    scope.generation = (scope.generation || 0) + 1;
    scope.associations = associations;
    scope.publisherMxid = publisherMxid || null;
    const identity = `${scope.approvalRoomId}\0${scope.generation}\0room_marker`;
    this.state.markerOutbox.push({ identity, casToken: createHash('sha256').update(identity).digest('hex'),
      approvalRoomId: scope.approvalRoomId, bindingGeneration: scope.generation, markerChannel: 'room_marker',
      agent: scope.agent, ownerMxid: scope.ownerMxid, publisherMxid: scope.publisherMxid,
      associations: structuredClone(associations), plan: null, attemptState: 'unprepared', eventId: null,
      nextAttemptAt: 0, superseded: false });
    return true;
  }

  _refreshBindingMarkers(agent) {
    for (const scope of Object.values(this.state.markerScopes)) {
      if (scope.agent !== agent || this.state.retiredMarkerRooms[scope.approvalRoomId]) continue;
      this._advanceMarker(scope, this._markerAssociations(scope));
    }
    const affectedRooms = new Set(Object.values(this.state.bindings)
      .filter((binding) => binding.agent === agent)
      .map((binding) => binding.ownerDmRoomId));
    for (const scope of Object.values(this.state.markerRoomScopes)) {
      const hasCurrentWork = this.state.markerOutbox.some((row) => (
        row.approvalRoomId === scope.approvalRoomId
        && row.markerChannel === MARKER_V2_CHANNEL
        && row.bindingGeneration === scope.generation
        && !row.superseded
      ));
      if (!affectedRooms.has(scope.approvalRoomId)
        && !(scope.associations || []).some((entry) => entry.agent === agent)
        && hasCurrentWork) continue;
      try {
        const associations = this._roomAssociations(scope);
        const associationsChanged = JSON.stringify(scope.associations || []) !== JSON.stringify(associations);
        if (associationsChanged || !hasCurrentWork) {
          this._invalidateRoomMarkerWork(scope.approvalRoomId);
          this._advanceRoomMarker(scope, !associationsChanged);
        }
      } catch (error) {
        if (!(error instanceof ApprovalStoreError) || error.code !== 'conflict') {
          throw error;
        }
        this._invalidateRoomMarkerWork(scope.approvalRoomId);
      }
    }
  }

  _invalidateRoomMarkerWork(approvalRoomId) {
    for (const row of this.state.markerOutbox) {
      if (row.approvalRoomId !== approvalRoomId || row.eventId) continue;
      row.superseded = true;
    }
  }

  _roomAssociations(scope) {
    const values = new Map((scope.associations || []).map((entry) => [JSON.stringify([
      entry.agent, entry.project_room_id,
    ]), { ...entry, active: false }]));
    const bindings = Object.values(this.state.bindings)
      .filter((binding) => binding.ownerDmRoomId === scope.approvalRoomId);
    for (const binding of bindings) {
      if (binding.ownerMxid !== scope.ownerMxid) {
        throw new ApprovalStoreError('conflict', 'approval room bindings require one common owner');
      }
      const association = {
        agent: binding.agent,
        project_room_id: binding.projectRoomId,
        active: binding.active !== false,
      };
      values.set(JSON.stringify([association.agent, association.project_room_id]), association);
    }
    const associations = [...values.values()].sort((left, right) => (
      compareText(left.agent, right.agent) || compareText(left.project_room_id, right.project_room_id)
    ));
    if (associations.length > MARKER_ASSOCIATION_LIMIT) {
      throw new ApprovalStoreError('conflict', 'approval room marker association limit exceeded');
    }
    return associations;
  }

  _roomHighWater(approvalRoomId) {
    let high = Number(this.state.markerRoomScopes[approvalRoomId]?.generation || 0);
    for (const scope of Object.values(this.state.markerScopes)) {
      if (scope.approvalRoomId === approvalRoomId) high = Math.max(high, Number(scope.generation || 0));
    }
    for (const row of this.state.markerOutbox) {
      if (row.approvalRoomId === approvalRoomId) high = Math.max(high, Number(row.bindingGeneration || 0));
    }
    return high;
  }

  _advanceRoomMarker(scope, force = false) {
    const associations = this._roomAssociations(scope);
    const unchanged = scope.generation
      && JSON.stringify(scope.associations || []) === JSON.stringify(associations);
    if (unchanged && !force) return false;
    for (const row of this.state.markerOutbox) {
      if (row.approvalRoomId === scope.approvalRoomId && !row.eventId
        && row.markerChannel !== MARKER_RETIREMENT_CHANNEL) row.superseded = true;
    }
    scope.generation = Math.max(Number(scope.generation || 0), this._roomHighWater(scope.approvalRoomId)) + 1;
    scope.associations = associations;
    const identity = `${scope.approvalRoomId}\0${scope.generation}\0${MARKER_V2_CHANNEL}`;
    this.state.markerOutbox.push({
      identity,
      casToken: createHash('sha256').update(identity).digest('hex'),
      approvalRoomId: scope.approvalRoomId,
      bindingGeneration: scope.generation,
      markerChannel: MARKER_V2_CHANNEL,
      ownerMxid: scope.ownerMxid,
      publisherMxid: scope.publisherMxid,
      publisherScope: scope.publisherScope,
      credentialKind: scope.credentialKind,
      credentialGeneration: scope.credentialGeneration,
      associations: structuredClone(associations),
      plan: null,
      attemptState: 'unprepared',
      eventId: null,
      nextAttemptAt: 0,
      superseded: false,
    });
    return true;
  }

  syncBindingMarker(input = {}) {
    if (!input.publisher_scope) return this._syncBindingMarkerV1(input);
    const agent = text(input.agent, 'agent', 128);
    const approvalRoomId = roomId(input.approval_room_id, 'approval_room_id');
    const ownerMxid = fullMxid(input.owner_mxid);
    const publisherMxid = fullMxid(input.publisher_mxid, 'publisher_mxid');
    const publisherScope = text(input.publisher_scope, 'publisher_scope', 255);
    const credentialKind = text(input.credential_kind, 'credential_kind', 64);
    const credentialGeneration = text(input.credential_generation, 'credential_generation', 255);
    const publisher = this.state.projectionPublishers[publisherScope];
    if (!this._isPrivateMarkerScope(publisherScope)
      || !publisher || publisher.publisherMxid !== publisherMxid
      || publisher.credentialKind !== credentialKind
      || publisher.credentialGeneration !== credentialGeneration) {
      throw new ApprovalStoreError('conflict', 'marker publisher context mismatch');
    }
    if (Object.hasOwn(input, 'project_room_associations')) {
      throw new ApprovalStoreError('bad_request', 'caller-authored marker associations are forbidden');
    }
    const matching = Object.values(this.state.bindings).filter((binding) => binding.agent === agent
      && binding.ownerMxid === ownerMxid && binding.ownerDmRoomId === approvalRoomId && binding.active !== false);
    if (!matching.length) throw new ApprovalStoreError('conflict', 'marker scope has no active canonical binding');
    const existing = this.state.markerRoomScopes[approvalRoomId];
    const scope = existing || {
      approvalRoomId,
      ownerMxid,
      publisherMxid,
      publisherScope,
      credentialKind,
      credentialGeneration,
      generation: this._roomHighWater(approvalRoomId),
      associations: [],
    };
    if (scope.ownerMxid !== ownerMxid) throw new ApprovalStoreError('conflict', 'marker owner mismatch');
    const publisherChanged = scope.publisherMxid !== publisherMxid
      || scope.publisherScope !== publisherScope
      || scope.credentialKind !== credentialKind
      || scope.credentialGeneration !== credentialGeneration;
    scope.publisherMxid = publisherMxid;
    scope.publisherScope = publisherScope;
    scope.credentialKind = credentialKind;
    scope.credentialGeneration = credentialGeneration;
    this.state.markerRoomScopes[approvalRoomId] = scope;
    const changed = this._advanceRoomMarker(scope, publisherChanged);
    if (changed) this._save();
    return { approval_room_id: scope.approvalRoomId, binding_generation: scope.generation,
      marker_channel: MARKER_V2_CHANNEL, publisher_mxid: scope.publisherMxid, owner_mxid: scope.ownerMxid,
      version: 2, project_room_associations: structuredClone(scope.associations) };
  }

  _syncBindingMarkerV1(input) {
    const agent = text(input.agent, 'agent', 128);
    const approvalRoomId = roomId(input.approval_room_id, 'approval_room_id');
    if (this.state.retiredMarkerRooms[approvalRoomId]) {
      throw new ApprovalStoreError('conflict', 'v1 approval room marker has been retired');
    }
    const ownerMxid = fullMxid(input.owner_mxid);
    const publisherMxid = fullMxid(input.publisher_mxid, 'publisher_mxid');
    const conflicting = Object.values(this.state.markerScopes)
      .find((scope) => scope.approvalRoomId === approvalRoomId && scope.agent !== agent);
    if (conflicting) {
      throw new ApprovalStoreError('conflict', 'approval room marker is already owned by another agent scope');
    }
    const key = this._markerScopeKey(agent, approvalRoomId);
    const existing = this.state.markerScopes[key];
    const matching = Object.values(this.state.bindings).filter((binding) => binding.agent === agent
      && binding.ownerMxid === ownerMxid && binding.ownerDmRoomId === approvalRoomId && binding.active !== false);
    if (!existing && !matching.length) {
      throw new ApprovalStoreError('conflict', 'marker scope has no active canonical binding');
    }
    const scope = existing || { agent, approvalRoomId, ownerMxid, generation: 0, associations: [] };
    if (scope.ownerMxid !== ownerMxid) throw new ApprovalStoreError('conflict', 'marker owner mismatch');
    this.state.markerScopes[key] = scope;
    const changed = this._advanceMarker(scope, this._markerAssociations(scope), publisherMxid);
    if (changed) this._save();
    return { approval_room_id: scope.approvalRoomId, binding_generation: scope.generation,
      marker_channel: 'room_marker', publisher_mxid: scope.publisherMxid, owner_mxid: scope.ownerMxid,
      agent: scope.agent, project_room_associations: structuredClone(scope.associations) };
  }

  _markerContent(row) {
    if (row.markerChannel === MARKER_RETIREMENT_CHANNEL) return {};
    if (row.markerChannel === MARKER_V2_CHANNEL) {
      return { version: 2, binding_generation: row.bindingGeneration, publisher_mxid: row.publisherMxid,
        owner_mxid: row.ownerMxid, project_room_associations: structuredClone(row.associations) };
    }
    return { version: 1, binding_generation: row.bindingGeneration, publisher_mxid: row.publisherMxid,
      owner_mxid: row.ownerMxid, agent: row.agent, project_room_associations: structuredClone(row.associations) };
  }

  _markerView(row) {
    return { approval_room_id: row.approvalRoomId, binding_generation: row.bindingGeneration, marker_channel: row.markerChannel,
      publisher_mxid: row.publisherMxid, publisher_scope: row.publisherScope || null,
      credential_kind: row.credentialKind || null,
      credential_generation: row.credentialGeneration || null,
      cas_token: row.casToken, cursor: Buffer.from(JSON.stringify([row.approvalRoomId, row.bindingGeneration, row.identity])).toString('base64url'),
      marker: this._markerContent(row),
      plan: row.plan ? { ...structuredClone(row.plan), cas_token: row.planCasToken, attempt_state: row.attemptState } : null };
  }

  migrateMarkerRoomsV2(options = {}) {
    const limit = Math.min(Math.max(Number(options.limit) || 100, 1), 200);
    const requestedRoom = Object.hasOwn(options, 'approval_room_id')
      ? roomId(options.approval_room_id, 'approval_room_id') : null;
    const migration = this.state.markerRoomMigration;
    const rooms = [...new Set(Object.values(this.state.markerScopes)
      .map((scope) => scope.approvalRoomId))]
      .sort(compareText)
      .filter(room => requestedRoom === null || room === requestedRoom)
      .filter((room) => !this.state.markerOutbox.some((row) => (
        row.approvalRoomId === room && row.markerChannel === MARKER_V2_CHANNEL
      )));
    const page = rooms.slice(0, limit);
    for (const approvalRoomId of page) {
      const legacy = Object.values(this.state.markerScopes)
        .filter((scope) => scope.approvalRoomId === approvalRoomId);
      const owners = new Set(legacy.map((scope) => scope.ownerMxid));
      const publishers = new Set(legacy.map((scope) => scope.publisherMxid).filter(Boolean));
      if (owners.size !== 1 || publishers.size !== 1) {
        throw new ApprovalStoreError('conflict', 'legacy room marker requires one owner and publisher');
      }
      const publisherMxid = [...publishers][0];
      const publisherContexts = Object.values(this.state.projectionPublishers)
        .filter((publisher) => publisher.publisherMxid === publisherMxid);
      if (publisherContexts.length !== 1) {
        throw new ApprovalStoreError('conflict', 'legacy marker publisher context is unavailable or ambiguous');
      }
      const publisher = publisherContexts[0];
      if (requestedRoom && (options.publisher_scope !== publisher.scope
        || options.publisher_mxid !== publisher.publisherMxid
        || options.credential_kind !== publisher.credentialKind
        || options.credential_generation !== publisher.credentialGeneration)) {
        throw new ApprovalStoreError('conflict', 'exact room migration publisher is stale or mismatched');
      }
      if (!this._isPrivateMarkerScope(publisher.scope)) {
        throw new ApprovalStoreError('conflict', 'legacy marker publisher is not a private room actor');
      }
      const scope = this.state.markerRoomScopes[approvalRoomId] || {
        approvalRoomId,
        ownerMxid: [...owners][0],
        publisherMxid,
        publisherScope: publisher.scope,
        credentialKind: publisher.credentialKind,
        credentialGeneration: publisher.credentialGeneration,
        generation: this._roomHighWater(approvalRoomId),
        associations: legacy.flatMap((item) => (item.associations || []).map((association) => ({
          agent: item.agent,
          project_room_id: association.project_room_id,
          active: association.active === true,
        }))),
      };
      if (scope.ownerMxid !== [...owners][0] || scope.publisherMxid !== publisherMxid) {
        throw new ApprovalStoreError('conflict', 'stored v2 marker conflicts with legacy room identity');
      }
      scope.publisherScope = publisher.scope;
      scope.credentialKind = publisher.credentialKind;
      scope.credentialGeneration = publisher.credentialGeneration;
      this.state.markerRoomScopes[approvalRoomId] = scope;
      const hasV2History = this.state.markerOutbox.some((row) => (
        row.approvalRoomId === approvalRoomId && row.markerChannel === MARKER_V2_CHANNEL
      ));
      this._advanceRoomMarker(scope, !hasV2History);
      if (!requestedRoom) migration.cursor = approvalRoomId;
    }
    const wasComplete = migration.complete === true;
    if (!requestedRoom) migration.complete = page.length === rooms.length;
    if (page.length || migration.complete !== wasComplete) this._save();
    return { examined: page.length, complete: requestedRoom ? page.length === rooms.length : migration.complete,
      cursor: requestedRoom ? null : migration.cursor };
  }

  listDueMarkers(options = {}) {
    const limit = Math.min(Math.max(Number(options.limit) || 100, 1), 200);
    const now = this.now();
    const rows = this.state.markerOutbox.filter((row) => !row.eventId && !row.superseded
      && Number(row.nextAttemptAt || 0) <= now)
      .sort((left, right) => compareText(left.approvalRoomId, right.approvalRoomId)
        || left.bindingGeneration - right.bindingGeneration || compareText(left.identity, right.identity));
    let start = 0;
    if (options.after) {
      let anchor;
      try { anchor = JSON.parse(Buffer.from(String(options.after), 'base64url').toString('utf8')); } catch { anchor = null; }
      if (!Array.isArray(anchor) || anchor.length !== 3 || typeof anchor[0] !== 'string'
        || !Number.isInteger(anchor[1]) || typeof anchor[2] !== 'string') {
        throw new ApprovalStoreError('bad_request', 'invalid marker cursor');
      }
      start = rows.findIndex((row) => row.approvalRoomId > anchor[0]
        || (row.approvalRoomId === anchor[0] && row.bindingGeneration > anchor[1])
        || (row.approvalRoomId === anchor[0] && row.bindingGeneration === anchor[1] && row.identity > anchor[2]));
      if (start < 0) start = rows.length;
    }
    return rows.slice(start, start + limit).map((row) => this._markerView(row));
  }

  _markerPlanRow(token, input = {}) {
    const row = this.state.markerOutbox.find((item) => item.planCasToken === token && item.plan);
    if (!row || input.approval_room_id !== row.approvalRoomId
      || Number(input.binding_generation) !== row.bindingGeneration || input.marker_channel !== row.markerChannel
      || input.publisher_mxid !== row.plan.publisher_mxid || input.credential_generation !== row.plan.credential_generation) {
      throw new ApprovalStoreError('conflict', 'marker plan identity mismatch');
    }
    return row;
  }

  _isPrivateMarkerScope(scope) {
    return scope === 'local_bot' || scope.startsWith('side-representative:');
  }

  _assertCurrentMarkerPublisher(row) {
    if (!row.publisherScope) return;
    const publisher = this.state.projectionPublishers[row.publisherScope];
    if (!this._isPrivateMarkerScope(row.publisherScope)
      || !publisher
      || publisher.publisherMxid !== row.publisherMxid
      || publisher.credentialKind !== row.credentialKind
      || publisher.credentialGeneration !== row.credentialGeneration) {
      throw new ApprovalStoreError('conflict', 'marker publisher context is no longer current');
    }
  }

  prepareMarker(casToken, input = {}) {
    const row = this.state.markerOutbox.find((item) => item.casToken === casToken && !item.eventId && !item.superseded);
    if (!row || input.approval_room_id !== row.approvalRoomId
      || Number(input.binding_generation) !== row.bindingGeneration || input.marker_channel !== row.markerChannel) {
      throw new ApprovalStoreError('conflict', 'marker row changed');
    }
    this._assertCurrentMarkerPublisher(row);
    if (!row.plan) {
      if (input.publisher_mxid !== row.publisherMxid) throw new ApprovalStoreError('conflict', 'marker publisher mismatch');
      const preparedEventType = row.markerChannel === MARKER_V2_CHANNEL
        ? MARKER_V2_EVENT_TYPE
        : MARKER_EVENT_TYPE;
      const credentialGeneration = text(input.credential_generation, 'credential_generation', 255);
      if (row.credentialGeneration && row.credentialGeneration !== credentialGeneration) {
        throw new ApprovalStoreError('conflict', 'marker credential generation mismatch');
      }
      row.plan = { publisher_mxid: row.publisherMxid,
        publisher_scope: row.publisherScope || null, credential_kind: row.credentialKind || null,
        credential_generation: credentialGeneration,
        prepared_event_type: preparedEventType, state_key: '', prepared_payload: this._markerContent(row) };
      row.planCasToken = createHash('sha256').update(JSON.stringify([row.identity, row.plan])).digest('hex');
      row.attemptState = 'ready';
      this._save();
    }
    return { plan: { ...structuredClone(row.plan), cas_token: row.planCasToken, attempt_state: row.attemptState } };
  }

  beginMarkerSend(token, input) {
    const row = this._markerPlanRow(token, input);
    this._assertCurrentMarkerPublisher(row);
    if (row.superseded) throw new ApprovalStoreError('conflict', 'marker generation superseded');
    if (row.eventId) return { plan: { ...structuredClone(row.plan), cas_token: row.planCasToken, attempt_state: 'receipted' } };
    if (row.attemptState === 'ready' || row.attemptState === 'uncertain') { row.attemptState = 'attempted'; this._save(); }
    return { plan: { ...structuredClone(row.plan), cas_token: row.planCasToken, attempt_state: row.attemptState } };
  }

  receiptMarker(token, input) {
    const eventId = text(input.event_id, 'event_id', 255);
    if (!EVENT_ID_RE.test(eventId)) throw new ApprovalStoreError('bad_request', 'event_id must be a Matrix event id');
    const row = this._markerPlanRow(token, input);
    if (row.eventId) {
      if (row.eventId === eventId) return { event_id: eventId };
      throw new ApprovalStoreError('conflict', 'marker already receipted');
    }
    if (row.attemptState !== 'attempted' && row.attemptState !== 'uncertain') throw new ApprovalStoreError('conflict', 'marker receipt mismatch');
    row.eventId = eventId;
    row.receiptedAt = this.now();
    this.state.markerReceiptSequence += 1;
    row.receiptSequence = this.state.markerReceiptSequence;
    if (row.markerChannel === MARKER_V2_CHANNEL) this._queueV1Retirement(row);
    if (row.markerChannel === MARKER_RETIREMENT_CHANNEL) {
      this.state.retiredMarkerRooms[row.approvalRoomId] = true;
      for (const old of this.state.markerOutbox) {
        if (old.approvalRoomId === row.approvalRoomId
          && old.markerChannel === 'room_marker' && !old.eventId) old.superseded = true;
      }
    }
    this._save();
    return { event_id: eventId };
  }

  _queueV1Retirement(v2Row) {
    for (const old of this.state.markerOutbox) {
      if (old.approvalRoomId === v2Row.approvalRoomId
        && old.markerChannel !== MARKER_V2_CHANNEL
        && old.markerChannel !== MARKER_RETIREMENT_CHANNEL
        && !old.eventId) old.superseded = true;
    }
    const currentPublisher = this.state.projectionPublishers[v2Row.publisherScope];
    const retirementPublisher = currentPublisher || {
      publisherMxid: v2Row.publisherMxid,
      scope: v2Row.publisherScope,
      credentialKind: v2Row.credentialKind,
      credentialGeneration: v2Row.credentialGeneration,
    };
    for (const row of this.state.markerOutbox) {
      if (row.approvalRoomId === v2Row.approvalRoomId
        && row.markerChannel === MARKER_RETIREMENT_CHANNEL
        && !row.eventId
        && (row.publisherMxid !== retirementPublisher.publisherMxid
          || row.publisherScope !== retirementPublisher.scope
          || row.credentialKind !== retirementPublisher.credentialKind
          || row.credentialGeneration !== retirementPublisher.credentialGeneration)) {
        row.superseded = true;
      }
    }
    const existing = this.state.markerOutbox.find((row) => (
      row.approvalRoomId === v2Row.approvalRoomId
      && row.markerChannel === MARKER_RETIREMENT_CHANNEL
      && !row.superseded && !row.eventId
    ));
    if (existing) return;
    const retirementSequence = this.state.markerOutbox.filter((row) => (
      row.approvalRoomId === v2Row.approvalRoomId
      && row.markerChannel === MARKER_RETIREMENT_CHANNEL
    )).length + 1;
    const identity = `${v2Row.approvalRoomId}\0${v2Row.bindingGeneration}\0${MARKER_RETIREMENT_CHANNEL}\0${retirementSequence}`;
    this.state.markerOutbox.push({
      identity,
      casToken: createHash('sha256').update(identity).digest('hex'),
      approvalRoomId: v2Row.approvalRoomId,
      bindingGeneration: v2Row.bindingGeneration,
      markerChannel: MARKER_RETIREMENT_CHANNEL,
      ownerMxid: v2Row.ownerMxid,
      publisherMxid: retirementPublisher.publisherMxid,
      publisherScope: retirementPublisher.scope,
      credentialKind: retirementPublisher.credentialKind,
      credentialGeneration: retirementPublisher.credentialGeneration,
      associations: [],
      plan: null,
      attemptState: 'unprepared',
      eventId: null,
      nextAttemptAt: 0,
      superseded: false,
    });
  }

  reconcileMarkerRetirements(options = {}) {
    const limit = Math.min(Math.max(Number(options.limit) || 100, 1), 200);
    if (Object.hasOwn(options, 'legacy_state_observed_nonempty')
      && typeof options.legacy_state_observed_nonempty !== 'boolean') {
      throw new ApprovalStoreError('bad_request', 'legacy_state_observed_nonempty must be boolean');
    }
    const legacyStateObserved = options.legacy_state_observed_nonempty === true;
    const requestedRoom = options.approval_room_id == null
      ? null
      : roomId(options.approval_room_id, 'approval_room_id');
    if (legacyStateObserved && !requestedRoom) {
      throw new ApprovalStoreError('bad_request', 'observed legacy state requires approval_room_id');
    }
    const rooms = (requestedRoom
      ? (this.state.retiredMarkerRooms[requestedRoom] ? [requestedRoom] : [])
      : Object.keys(this.state.retiredMarkerRooms).sort(compareText)
    ).slice(0, limit);
    let queued = 0;
    for (const approvalRoomId of rooms) {
      const rows = this.state.markerOutbox.filter((row) => row.approvalRoomId === approvalRoomId);
      const latestLegacyReceipt = Math.max(0, ...rows
        .filter((row) => row.markerChannel === 'room_marker' && row.eventId)
        .map((row) => Number(row.receiptSequence || 0)));
      const latestRetirementReceipt = Math.max(0, ...rows
        .filter((row) => row.markerChannel === MARKER_RETIREMENT_CHANNEL && row.eventId)
        .map((row) => Number(row.receiptSequence || 0)));
      const hasPendingRetirement = rows.some((row) => (
        row.markerChannel === MARKER_RETIREMENT_CHANNEL && !row.eventId && !row.superseded
      ));
      const latestV2 = rows.filter((row) => row.markerChannel === MARKER_V2_CHANNEL && row.eventId)
        .sort((left, right) => right.bindingGeneration - left.bindingGeneration)[0];
      if (latestV2 && !hasPendingRetirement
        && (legacyStateObserved || latestLegacyReceipt > latestRetirementReceipt)) {
        this._queueV1Retirement(latestV2);
        queued += 1;
      }
    }
    if (queued) this._save();
    return { examined: rooms.length, queued };
  }

  retryMarker(token, input) {
    const row = this._markerPlanRow(token, input);
    this._assertCurrentMarkerPublisher(row);
    if (row.superseded) throw new ApprovalStoreError('conflict', 'marker generation superseded');
    if (row.eventId) throw new ApprovalStoreError('conflict', 'marker already receipted');
    if (row.attemptState !== 'attempted' && row.attemptState !== 'uncertain') {
      throw new ApprovalStoreError('conflict', 'marker send has not begun');
    }
    const requested = Number(input.retry_at); const now = this.now();
    row.attemptState = 'uncertain';
    row.nextAttemptAt = Number.isFinite(requested) ? Math.min(Math.max(now, requested), now + MAX_RETRY_DELAY_MS) : now;
    row.lastErrorCode = optionalText(input.error_code, 128); this._save();
    return { attempt_state: row.attemptState, retry_at: row.nextAttemptAt };
  }

  upsertProjectionPublisher(input = {}) {
    const scope = text(input.scope, 'scope', 255);
    const record = {
      scope,
      publisherMxid: fullMxid(input.publisher_mxid, 'publisher_mxid'),
      homeserver: text(input.homeserver, 'homeserver', 255).toLowerCase(),
      credentialKind: text(input.credential_kind, 'credential_kind', 64),
      credentialGeneration: text(input.credential_generation, 'credential_generation', 255),
      sideId: optionalText(input.side_id, 255) || null,
      agent: optionalText(input.agent, 128) || null,
      updatedAt: this.now(),
    };
    const previous = this.state.projectionPublishers[scope];
    if (previous && JSON.stringify({ ...previous, updatedAt: 0 }) === JSON.stringify({ ...record, updatedAt: 0 })) {
      return structuredClone(previous);
    }
    this.state.projectionPublishers[scope] = record;
    this._save();
    return structuredClone(record);
  }

  projectionPublisher(scope) {
    const record = this.state.projectionPublishers[String(scope || '')];
    return record ? structuredClone(record) : null;
  }

  projectionForCas(casToken) {
    const row = this.state.projectionOutbox.find((item) => item.casToken === casToken || item.planCasToken === casToken);
    return row ? this._projectionView(row) : null;
  }

  privateRequestPublisher(requestId) {
    const plan = this.state.projectionOutbox.find((row) => (
      row.requestId === requestId && row.channel === 'private_request' && row.plan
    ))?.plan;
    return plan ? {
      scope: plan.publisher_scope,
      publisherMxid: plan.publisher_mxid,
      homeserver: plan.homeserver,
      credentialKind: plan.credential_kind,
      credentialGeneration: plan.credential_generation,
    } : null;
  }

  _legacyProjection(casToken, requestId) {
    if (typeof casToken !== 'string' || !/^[0-9a-f]{64}$/.test(casToken)) {
      throw new ApprovalStoreError('conflict', 'legacy projection requires an explicit CAS token');
    }
    const row = this.state.projectionOutbox.find((item) => item.casToken === casToken || item.planCasToken === casToken);
    const record = row && this.state.requests[row.requestId];
    if (!record || row.requestId !== requestId || row.channel !== 'private_status'
      || row.migrationKind !== 'legacy_v1' || record.projectionMigration !== 'legacy_v1'
      || row.targetRoomId !== record.ownerDmRoomId) {
      throw new ApprovalStoreError('conflict', 'legacy projection identity mismatch');
    }
    return { row, record };
  }

  // This is a consistency check on trusted bridge observations, not a Matrix proof reader.
  attestLegacyOriginal(requestId, input = {}) {
    const { row, record } = this._legacyProjection(input.cas_token, requestId);
    if (input.request_id !== requestId || input.revision !== row.revision
      || input.channel !== row.channel || input.cas_token !== row.casToken) {
      throw new ApprovalStoreError('conflict', 'legacy attestation route or CAS mismatch');
    }
    const normalized = canonicalMatrixContent(input);
    if (Buffer.byteLength(JSON.stringify(normalized)) > 16 * 1024 || input.schema_version !== 1) {
      throw new ApprovalStoreError('bad_request', 'legacy evidence must be schema 1 and at most 16 KiB');
    }
    const checkKeys = (value, allowed) => {
      if (!value || typeof value !== 'object' || Array.isArray(value)
        || Object.keys(value).some((key) => !allowed.includes(key))) {
        throw new ApprovalStoreError('bad_request', 'legacy evidence must contain only normalized observation fields');
      }
    };
    checkKeys(input, ['schema_version', 'request_id', 'revision', 'channel', 'cas_token',
      'candidate_verdict_event_id', 'verdict', 'original', 'publisher', 'private_context']);
    const commonFields = ['room_id', 'type', 'payload_key', 'version', 'edited', 'redacted',
      'request_id', 'agent', 'project', 'project_room_id', 'input_digest', 'event_id', 'sender', 'msgtype', 'kind'];
    checkKeys(input.verdict, [...commonFields, 'action', 'reply_to_event_id']);
    checkKeys(input.original, [...commonFields, 'runtime', 'upstream_request_id', 'expires_at']);
    checkKeys(input.publisher, ['publisher_scope', 'publisher_mxid', 'homeserver', 'credential_kind', 'credential_generation']);
    checkKeys(input.private_context, ['room_id', 'owner_mxid', 'ready', 'joined', 'encrypted']);
    if (input.private_context.room_id !== record.ownerDmRoomId
      || input.private_context.owner_mxid !== record.ownerMxid || input.private_context.ready !== true
      || input.private_context.joined !== true || typeof input.private_context.encrypted !== 'boolean') {
      throw new ApprovalStoreError('conflict', 'legacy private observation is unavailable');
    }
    const verdictId = record.matrixEventId;
    if (!EVENT_ID_RE.test(verdictId || '') || input.candidate_verdict_event_id !== verdictId) {
      throw new ApprovalStoreError('conflict', 'legacy candidate verdict changed or is missing');
    }
    const tuple = { request_id: record.id, agent: record.agent, project: record.project,
      project_room_id: record.projectRoomId, input_digest: record.inputDigest };
    const verdict = input.verdict;
    const original = input.original;
    for (const [role, event] of [['verdict', verdict], ['request', original]]) {
      const namespace = event?.payload_key;
      if (!event || !['com.agentchat.approval', 'com.hafleet.approval'].includes(namespace)
        || event.msgtype !== `${namespace}.${role}.v1` || event.version !== 1 || event.kind !== role
        || event.type !== 'm.room.message' || event.edited !== false || event.redacted !== false
        || event.room_id !== record.ownerDmRoomId || !EVENT_ID_RE.test(event.event_id || '')
        || typeof event.sender !== 'string' || event.sender.length > 255 || !MXID_RE.test(event.sender)
        || Object.entries(tuple).some(([key, value]) => typeof value !== 'string' || !value || event[key] !== value)) {
        throw new ApprovalStoreError('conflict', 'legacy event role or canonical tuple mismatch');
      }
    }
    if (verdict.event_id !== verdictId || verdict.sender !== record.ownerMxid
      || verdict.reply_to_event_id !== original.event_id || original.event_id === verdictId
      || !['allow', 'deny'].includes(record.decision)
      || verdict.action !== (record.decision === 'allow' ? 'approve_once' : 'deny')
      || original.runtime !== record.runtime || original.upstream_request_id !== record.upstreamRequestId
      || !Number.isFinite(original.expires_at) || original.expires_at !== record.expiresAt) {
      throw new ApprovalStoreError('conflict', 'legacy original relation or verdict mismatch');
    }
    const evidence = { schema_version: 1, ...tuple, owner_mxid: record.ownerMxid, room_id: record.ownerDmRoomId,
      candidate_verdict_event_id: verdictId, original_event_id: original.event_id, original_sender: original.sender,
      runtime: record.runtime, upstream_request_id: record.upstreamRequestId, expires_at: record.expiresAt,
      verdict_action: verdict.action, historical_credential_generation: null };
    const evidenceCas = createHash('sha256').update(JSON.stringify(evidence)).digest('hex');
    const previous = this.state.legacyOriginalEvidence[requestId];
    if (previous && previous.evidence_cas !== evidenceCas) {
      throw new ApprovalStoreError('conflict', 'legacy original evidence is immutable');
    }
    this._legacyCurrentPublisher(original.sender, input.publisher);
    if (previous) return { evidence: structuredClone(previous), duplicate: true };
    if (row.plan || row.eventId || row.superseded) {
      throw new ApprovalStoreError('conflict', 'legacy projection no longer accepts original evidence');
    }
    const accepted = { ...evidence, evidence_cas: evidenceCas, validated_at: this.now() };
    this.state.legacyOriginalEvidence[requestId] = accepted;
    this._save();
    return { evidence: structuredClone(accepted), duplicate: false };
  }

  _legacyCurrentPublisher(sender, proposed = {}) {
    const current = this.projectionPublisher(proposed.publisher_scope);
    if (!current || !(current.scope === 'local_bot' || current.scope.startsWith('side-representative:'))
      || current.publisherMxid !== sender || proposed.publisher_mxid !== sender
      || current.homeserver !== proposed.homeserver || current.credentialKind !== proposed.credential_kind
      || current.credentialGeneration !== proposed.credential_generation) {
      throw new ApprovalStoreError('conflict', 'legacy original publisher context is unavailable or stale');
    }
    return current;
  }

  legacyStatusPublisher(requestId) {
    const plan = this.state.projectionOutbox.find((row) => row.requestId === requestId
      && row.migrationKind === 'legacy_v1' && row.channel === 'private_status' && row.plan)?.plan;
    return plan ? { publisher_scope: plan.publisher_scope, publisher_mxid: plan.publisher_mxid,
      homeserver: plan.homeserver, credential_kind: plan.credential_kind,
      credential_generation: plan.credential_generation } : null;
  }

  legacyOriginalForProjection(casToken, requestId) {
    const { row, record } = this._legacyProjection(casToken, requestId);
    const evidence = this.state.legacyOriginalEvidence[requestId] || null;
    // The normalized tuple is bound to the historical request, never a replacement binding.
    if (evidence && (evidence.candidate_verdict_event_id !== record.matrixEventId
      || evidence.room_id !== record.ownerDmRoomId || evidence.owner_mxid !== record.ownerMxid
      || evidence.agent !== record.agent || evidence.project !== record.project
      || evidence.project_room_id !== record.projectRoomId || evidence.input_digest !== record.inputDigest)) {
      throw new ApprovalStoreError('conflict', 'legacy canonical evidence changed');
    }
    const detail = evidence ? { version: 1, kind: 'status', migration_kind: 'legacy_v1',
      request_id: record.id, agent: record.agent, project: record.project, project_room_id: record.projectRoomId,
      input_digest: record.inputDigest, owner_mxid: record.ownerMxid, publisher_mxid: evidence.original_sender,
      revision: row.revision, state: row.state, decision: record.decision || null } : null;
    return { evidence: evidence ? structuredClone(evidence) : null,
      candidate_verdict_event_id: EVENT_ID_RE.test(record.matrixEventId || '') ? record.matrixEventId : null,
      content: detail ? { msgtype: 'com.agentchat.approval.status.v1',
        body: row.state === 'consumed' ? 'Approval verdict consumed by runtime; task outcome unknown.' : `Approval ${row.state}.`,
        'com.agentchat.approval': detail,
        'm.relates_to': { 'm.in_reply_to': { event_id: evidence.original_event_id } } } : null };
  }

  _validateLegacyStatusPlan(row, input) {
    // Common native preparation normalizes text later; legacy content checks must
    // use the exact event type that will be persisted, never an unnormalized bypass.
    if (!['m.room.message', 'm.room.encrypted'].includes(input?.prepared_event_type)) {
      throw new ApprovalStoreError('bad_request', 'legacy prepared_event_type must be exact');
    }
    const { evidence, content } = this.legacyOriginalForProjection(row.casToken, row.requestId);
    if (!evidence || input?.legacy_evidence_cas !== evidence.evidence_cas) {
      throw new ApprovalStoreError('conflict', 'legacy original evidence is required');
    }
    this._legacyCurrentPublisher(evidence.original_sender, input);
    const firstPlan = this.legacyStatusPublisher(row.requestId);
    if (firstPlan && ['publisher_scope', 'publisher_mxid', 'homeserver', 'credential_kind', 'credential_generation']
      .some((key) => input[key] !== firstPlan[key])) {
      throw new ApprovalStoreError('conflict', 'legacy status must retain its first pinned publisher context');
    }
    if (!row.plan && input.prepared_event_type === 'm.room.message'
      && !isDeepStrictEqual(canonicalMatrixContent(input.prepared_payload), content)) {
      throw new ApprovalStoreError('conflict', 'legacy status must equal canonical readonly content');
    }
  }

  createRequest(input, options = {}) {
    return this._withRollback(() => this._createRequest(input, options), { expireRequests: true });
  }

  _createRequest(input, options) {
    const now = this.now();
    // Keep recent terminal receipts for retries and audit. Expired decisions
    // older than seven days cannot authorize another execution; grants retain
    // their own complete authority independently of these historical requests.
    const cutoff = now - 7 * 24 * 60 * 60_000;
    for (const [id, record] of Object.entries(this.state.requests)) {
      if (record.status !== 'pending' && record.expiresAt < cutoff
        && !(this.state.projectionOutbox || []).some((row) => row.requestId === id && !row.eventId)) {
        delete this.state.requests[id];
      }
    }
    const agent = text(input?.agent, 'agent', 128);
    const runtime = text(input?.runtime, 'runtime', 32).toLowerCase();
    if (runtime !== 'claude' && runtime !== 'codex') {
      throw new ApprovalStoreError('bad_request', 'runtime must be claude or codex');
    }
    const upstreamRequestId = text(input?.upstream_request_id ?? input?.upstreamRequestId, 'upstream_request_id', 255);
    const routerApprovalId = optionalText(options?.routerApprovalId, 255);
    if (routerApprovalId && routerApprovalId !== upstreamRequestId) {
      throw new ApprovalStoreError('bad_request', 'router approval origin must match upstream_request_id');
    }
    const requestedProject = optionalText(input?.project, 255);
    const requestedProjectRoomIdRaw = optionalText(
      input?.project_room_id ?? input?.projectRoomId,
      255,
    );
    const requestedProjectRoomId = requestedProjectRoomIdRaw
      ? roomId(requestedProjectRoomIdRaw, 'project_room_id')
      : '';
    const toolName = text(input?.tool_name ?? input?.toolName, 'tool_name', 255);
    const description = optionalText(input?.description, 4096);
    const inputPreview = optionalText(input?.input_preview ?? input?.inputPreview, 8192);

    for (const record of Object.values(this.state.requests)) {
      this._expire(record, now);
      if (record.status === 'pending'
        && record.agent === agent
        && record.runtime === runtime
        && record.upstreamRequestId === upstreamRequestId) {
        if ((record.routerApprovalId || '') !== routerApprovalId) {
          throw new ApprovalStoreError('conflict', 'pending upstream request already belongs to a different approval origin');
        }
        this._save();
        return publicRecord(record);
      }
    }

    const bindings = this.listBindings({
      agent,
      project: requestedProject,
      project_room_id: requestedProjectRoomId,
    });
    const binding = bindings.length === 1 ? bindings[0] : null;
    const id = `approval_${randomBytes(16).toString('hex')}`;
    const expiresAtRaw = Number(input?.expires_at ?? input?.expiresAt);
    const expiresAt = Number.isFinite(expiresAtRaw) && expiresAtRaw > now
      ? Math.min(Math.floor(expiresAtRaw), now + this.ttlMs)
      : now + this.ttlMs;

    if (!binding) {
      const reason = bindings.length === 0 ? 'owner_binding_missing' : 'owner_binding_ambiguous';
      const denied = {
        id,
        agent,
        runtime,
        project: requestedProject || null,
        projectRoomId: null,
        ownerMxid: null,
        ownerDmRoomId: null,
        upstreamRequestId,
        routerApprovalId: routerApprovalId || null,
        toolName,
        description,
        inputPreview,
        inputDigest: null,
        status: 'denied',
        decision: 'deny',
        denialReason: reason,
        createdAt: now,
        expiresAt,
        decidedAt: now,
        consumedAt: null,
      };
      denied.decisionEventId = stableDecisionEventId(denied);
      this.state.requests[id] = denied;
      this._audit('approval.denied', { requestId: id, agent, reason });
      this._save();
      return publicRecord(denied);
    }

    const record = {
      id,
      agent,
      runtime,
      project: binding.project,
      projectRoomId: binding.projectRoomId,
      ownerMxid: binding.ownerMxid,
      ownerDmRoomId: binding.ownerDmRoomId,
      upstreamRequestId,
      routerApprovalId: routerApprovalId || null,
      toolName,
      description,
      inputPreview,
      status: 'pending',
      decision: null,
      denialReason: null,
      createdAt: now,
      expiresAt,
      decidedAt: null,
      consumedAt: null,
    };
    record.bindingAuthority = this._bindingAuthority(binding);
    // The optional second argument is only supplied by the backend's owned
    // runner callback. Agent HTTP fields never reach this authority path.
    record.authorization = runtime === 'codex' ? deriveExecutionAuthorization(options.execution) : null;
    if (record.authorization?.taskId && !this.isTaskActive(record.authorization.taskId)) record.authorization.taskId = null;
    if (record.authorization?.taskId) {
      const epoch = this.getTaskEpoch(record.authorization.taskId);
      if (Number.isSafeInteger(epoch) && epoch >= 0) record.authorization.taskEpoch = epoch;
      else record.authorization.taskId = null;
    }
    if (record.authorization && !this.isAgentCurrent(agent, record.authorization.agentId)) record.authorization = null;
    record.inputDigest = digestRequest(record);
    record.projectionRevision = 1;
    record.projectionMigration = 'native_v2';
    this.state.requests[id] = record;
    this._indexPending(record);
    this._enqueueProjection(record, 'private_request', 'pending');
    this._enqueueProjection(record, 'public_notice', 'pending');
    this._audit('approval.created', {
      requestId: id,
      agent,
      project: record.project,
      projectRoomId: record.projectRoomId,
      ownerMxid: record.ownerMxid,
    });
    const context = record.authorization;
    const grant = context && Object.values(this.state.grants).find(g => this._grantCurrent(g)
      && g.agent === agent && g.agentId === context.agentId && g.projectRoomId === record.projectRoomId
      && g.project === record.project && g.bindingAuthority === record.bindingAuthority
      && g.scopeKey === context.scope.key && (g.scope === 'always' || g.taskId === context.taskId));
    if (grant) {
      record.status = 'approved'; record.decision = 'allow'; record.decidedAt = now;
      record.grantId = grant.id; record.authorizationAction = `approve_${grant.scope}`;
      record.decisionEventId = stableDecisionEventId(record);
      this._audit('authorization.matched', { requestId: id, grantId: grant.id, agent });
    }
    this._save();
    return publicRecord(record);
  }

  getRequest(id, options = {}) {
    const normalizedId = typeof id === 'string' ? id.trim() : '';
    const record = this.state.requests[normalizedId];
    if (!record) return null;
    if (!this._persistenceHealth.degraded && this._expire(record)) this._save();
    return options.matrix === true ? matrixRecord(record) : publicRecord(record);
  }

  listRequests(filters = {}) {
    const status = typeof filters.status === 'string' ? filters.status.trim() : '';
    const upstreamPrefix = typeof filters.upstream_request_prefix === 'string'
      ? filters.upstream_request_prefix.trim()
      : '';
    let changed = false;
    const rows = [];
    for (const record of Object.values(this.state.requests)) {
      if (!this._persistenceHealth.degraded && this._expire(record)) changed = true;
      if (status && record.status !== status) continue;
      if (upstreamPrefix && !record.upstreamRequestId.startsWith(upstreamPrefix)) continue;
      rows.push(publicRecord(record));
    }
    if (changed) this._save();
    return rows;
  }

  submitMatrixVerdict(id, input) {
    return this._withRollback(() => this._submitMatrixVerdict(id, input), { requestId: typeof id === 'string' ? id.trim() : '' });
  }

  _submitMatrixVerdict(id, input) {
    const normalizedId = typeof id === 'string' ? id.trim() : '';
    const record = this.state.requests[normalizedId];
    if (!record) return { ok: false, code: 'not_found', record: null };
    const now = this.now();
    if (this._expire(record, now)) {
      this._audit('approval.verdict_rejected', { requestId: normalizedId, reason: 'expired' });
      this._save();
      return { ok: false, code: 'expired', record: publicRecord(record) };
    }
    if (record.status !== 'pending') {
      this._audit('approval.verdict_rejected', { requestId: normalizedId, reason: 'not_pending', status: record.status });
      this._save();
      return { ok: false, code: 'not_pending', record: publicRecord(record) };
    }

    const action = typeof input?.action === 'string' ? input.action.trim() : '';
    const submittedEventId = optionalText(input?.event_id ?? input?.eventId, 255) || null;
    const expected = {
      senderMxid: record.ownerMxid,
      roomId: record.ownerDmRoomId,
      agent: record.agent,
      project: record.project,
      projectRoomId: record.projectRoomId,
      inputDigest: record.inputDigest,
    };
    const actual = {
      senderMxid: input?.sender_mxid ?? input?.senderMxid,
      roomId: input?.room_id ?? input?.roomId,
      agent: input?.agent,
      project: input?.project,
      projectRoomId: input?.project_room_id ?? input?.projectRoomId,
      inputDigest: input?.input_digest ?? input?.inputDigest,
    };
    const mismatch = Object.keys(expected).find((key) => expected[key] !== actual[key]);
    const context = record.authorization;
    const reusable = context && this.isAgentCurrent(record.agent, context.agentId);
    const invalidAction = !SCOPED_APPROVAL_ACTIONS.includes(action)
      || (['approve_task', 'approve_always'].includes(action) && !reusable)
      || (action === 'approve_task' && (!context?.taskId || !this.isTaskActive(context.taskId)
        || context.taskEpoch !== this.getTaskEpoch(context.taskId)));
    if (mismatch || invalidAction || !this._bindingCurrent(record)) {
      const reason = mismatch ? `${mismatch}_mismatch` : invalidAction ? 'invalid_action' : 'owner_binding_changed';
      this._audit('approval.verdict_rejected', {
        requestId: normalizedId,
        reason,
        senderMxid: typeof actual.senderMxid === 'string' ? actual.senderMxid : null,
        roomId: typeof actual.roomId === 'string' ? actual.roomId : null,
      });
      this._save();
      return { ok: false, code: reason, record: publicRecord(record) };
    }

    record.status = action !== 'deny' ? 'approved' : 'denied';
    record.decision = action !== 'deny' ? 'allow' : 'deny';
    record.authorizationAction = action;
    record.denialReason = action === 'deny' ? 'owner_denied' : null;
    record.decidedAt = now;
    record.decisionEventId = stableDecisionEventId(record);
    record.matrixEventId = submittedEventId;
    if (action === 'approve_task' || action === 'approve_always') {
      const grant = {
        id: `grant_${randomBytes(16).toString('hex')}`, agent: record.agent, agentId: context.agentId,
        project: record.project, projectRoomId: record.projectRoomId, ownerMxid: record.ownerMxid,
        ownerDmRoomId: record.ownerDmRoomId, bindingAuthority: record.bindingAuthority,
        workspace: context.workspace, scope: action === 'approve_task' ? 'task' : 'always',
        taskId: action === 'approve_task' ? context.taskId : null,
        taskEpoch: action === 'approve_task' ? context.taskEpoch : null,
        scopeKey: context.scope.key, kind: context.scope.kind, description: context.scope.description,
        createdAt: now, sourceRequestId: record.id, sourceEventId: record.matrixEventId, revokedAt: null,
      };
      this.state.grants[grant.id] = grant; record.grantId = grant.id;
      this._audit('authorization.created', { grantId: grant.id, requestId: record.id, agent: record.agent, scope: grant.scope });
    }
    this._advanceProjection(record, record.status);
    this._audit('approval.verdict_accepted', {
      requestId: normalizedId,
      agent: record.agent,
      decision: record.decision,
      senderMxid: record.ownerMxid,
      roomId: record.ownerDmRoomId,
      eventId: record.matrixEventId,
    });
    this._save();
    return { ok: true, code: record.status, record: publicRecord(record) };
  }

  denyPending(id, reason = 'delivery_failed') {
    const record = this.state.requests[id];
    if (!record) return null;
    const now = this.now();
    if (this._expire(record, now)) {
      this._save();
      return publicRecord(record);
    }
    if (record.status !== 'pending') return publicRecord(record);
    record.status = 'denied';
    record.decision = 'deny';
    record.denialReason = optionalText(reason, 255) || 'delivery_failed';
    record.decidedAt = now;
    record.decisionEventId = stableDecisionEventId(record);
    this._advanceProjection(record, record.status);
    this._audit('approval.denied', { requestId: id, agent: record.agent, reason: record.denialReason });
    this._save();
    return publicRecord(record);
  }

  consumeDecision(id, agentValue, inputDigestValue = null) {
    const record = this.state.requests[id];
    if (!record) return { ok: false, code: 'not_found', record: null };
    const now = this.now();
    if (this._expire(record, now)) {
      this._save();
      return { ok: false, code: 'expired', record: publicRecord(record) };
    }
    if (record.agent !== agentValue) return { ok: false, code: 'agent_mismatch', record: publicRecord(record) };
    if (inputDigestValue && record.inputDigest !== inputDigestValue) {
      return { ok: false, code: 'input_digest_mismatch', record: publicRecord(record) };
    }
    if (record.status === 'pending') return { ok: false, code: 'pending', record: publicRecord(record) };
    if (record.status === 'consumed') return { ok: false, code: 'consumed', record: publicRecord(record) };
    if (!TERMINAL_STATES.has(record.status)) return { ok: false, code: 'invalid_state', record: publicRecord(record) };

    if (record.decision === 'allow' && (!this._bindingCurrent(record)
      || (record.authorization && !this.isAgentCurrent(record.agent, record.authorization.agentId))
      || (record.grantId && !this._grantCurrent(this.state.grants[record.grantId])))) {
      record.decision = 'deny'; record.denialReason = 'authorization_no_longer_valid';
      record.decisionEventId = null;
      this._audit('authorization.invalidated_before_consume', { requestId: id, agent: record.agent });
    }
    const decision = record.decision === 'allow' ? 'allow' : 'deny';
    record.decisionEventId = stableDecisionEventId(record);
    record.status = 'consumed';
    record.consumedAt = now;
    this._advanceProjection(record, 'consumed');
    this._audit('approval.consumed', { requestId: id, agent: record.agent, decision });
    this._save();
    return { ok: true, code: 'consumed', decision, decision_event_id: record.decisionEventId, record: publicRecord(record) };
  }


  sweepExpired(options = {}) {
    const limit = Math.min(Math.max(Number(options.limit) || 100, 1), 200);
    const now = this.now();
    let examined = 0;
    let expired = 0;
    let changed = false;
    while (examined < limit && this.state.expiryIndex.length) {
      const entry = this.state.expiryIndex[0];
      const record = this.state.requests[entry.requestId];
      examined += 1;
      if (!record || record.status !== 'pending' || Number(record.expiresAt || 0) !== entry.expiresAt) {
        this._expiryPop();
        changed = true;
        continue;
      }
      if (entry.expiresAt > now) break;
      this._expiryPop();
      changed = true;
      if (this._expire(record, now)) expired += 1;
    }
    if (changed) this._save();
    return { scanned: examined, expired };
  }

  listDueProjections(options = {}) {
    const limit = Math.min(Math.max(Number(options.limit) || 100, 1), 200);
    const now = this.now();
    const deliveredPrivateRequests = new Set((this.state.projectionOutbox || [])
      .filter((row) => row.channel === 'private_request' && row.eventId)
      .map((row) => row.requestId));
    const pending = (this.state.projectionOutbox || []).filter((row) => !row.eventId && !row.superseded
      && (row.channel !== 'public_notice' || deliveredPrivateRequests.has(row.requestId)));
    const minimum = new Map();
    for (const row of pending) minimum.set(row.requestId, Math.min(minimum.get(row.requestId) ?? Infinity, row.revision));
    const ordered = pending.filter((r) => Number(r.nextAttemptAt || 0) <= now && r.revision === minimum.get(r.requestId))
      .sort((a, b) => Number(a.nextAttemptAt) - Number(b.nextAttemptAt)
        || a.requestId.localeCompare(b.requestId)
        || a.revision - b.revision
        || a.channel.localeCompare(b.channel));
    let start = 0;
    if (options.after) {
      const anchor = this._decodeProjectionCursor(options.after);
      start = ordered.findIndex((row) => this._compareProjectionCursor(row, anchor) > 0);
      if (start < 0) start = ordered.length;
    }
    return ordered.slice(start, start + limit).map((r) => this._projectionView(r));
  }

  _projectionView(row) {
    return { request_id: row.requestId, revision: row.revision, channel: row.channel, state: row.state,
      target_room_id: row.targetRoomId, migration_kind: row.migrationKind, cas_token: row.casToken,
      cursor: Buffer.from(JSON.stringify(this._projectionCursor(row)), 'utf8').toString('base64url'),
      plan: row.plan ? { ...structuredClone(row.plan), cas_token: row.planCasToken, attempt_state: row.attemptState } : null };
  }

  _projectionCursor(row) {
    return [Number(row.nextAttemptAt || 0), row.requestId, row.revision, row.channel, row.identity];
  }

  _decodeProjectionCursor(value) {
    try {
      if (typeof value !== 'string' || value.length > 2048) throw new Error('invalid');
      const cursor = JSON.parse(Buffer.from(value, 'base64url').toString('utf8'));
      if (!Array.isArray(cursor) || cursor.length !== 5 || !Number.isFinite(cursor[0])
        || typeof cursor[1] !== 'string' || !Number.isInteger(cursor[2])
        || typeof cursor[3] !== 'string' || typeof cursor[4] !== 'string') throw new Error('invalid');
      return cursor;
    } catch {
      throw new ApprovalStoreError('bad_request', 'invalid projection cursor');
    }
  }

  _compareProjectionCursor(row, cursor) {
    const value = this._projectionCursor(row);
    for (let index = 0; index < value.length; index += 1) {
      if (value[index] < cursor[index]) return -1;
      if (value[index] > cursor[index]) return 1;
    }
    return 0;
  }

  _planRow(planToken, input = {}) {
    const row = this.state.projectionOutbox.find((r) => r.planCasToken === planToken && r.plan);
    const p = row?.plan;
    if (!row || input.publisher_mxid !== p.publisher_mxid
      || (input.publisher_scope !== undefined && input.publisher_scope !== p.publisher_scope)
      || (input.request_id !== undefined && input.request_id !== row.requestId)
      || (input.revision !== undefined && Number(input.revision) !== row.revision)
      || (input.channel !== undefined && input.channel !== row.channel)
      || input.room_id !== row.targetRoomId
      || input.credential_generation !== p.credential_generation
      || input.transaction_id !== p.transaction_id) {
      throw new ApprovalStoreError('conflict', 'projection plan identity mismatch');
    }
    return row;
  }

  prepareProjection(casToken, input) {
    const row = this.state.projectionOutbox.find((r) => r.casToken === casToken && !r.eventId && !r.superseded);
    if (!row) throw new ApprovalStoreError('conflict', 'projection row changed');
    if ((input?.request_id !== undefined && input.request_id !== row.requestId)
      || (input?.revision !== undefined && Number(input.revision) !== row.revision)
      || (input?.channel !== undefined && input.channel !== row.channel)) {
      throw new ApprovalStoreError('conflict', 'projection route identity mismatch');
    }
    if (row.channel === 'private_status' && row.migrationKind === 'legacy_v1') {
      this._validateLegacyStatusPlan(row, input);
    } else if (row.channel === 'private_status') {
      const requestPlan = this.state.projectionOutbox.find((candidate) => (
        candidate.requestId === row.requestId && candidate.channel === 'private_request' && candidate.plan
      ))?.plan;
      const samePublisher = requestPlan
        && input?.publisher_scope === requestPlan.publisher_scope
        && input?.publisher_mxid === requestPlan.publisher_mxid
        && input?.homeserver === requestPlan.homeserver
        && input?.credential_kind === requestPlan.credential_kind
        && input?.credential_generation === requestPlan.credential_generation;
      if (!samePublisher) {
        throw new ApprovalStoreError('conflict', 'private status must retain the private request publisher');
      }
    }
    if (!row.plan) {
      const rawPayload = input?.prepared_payload;
      if (!rawPayload || typeof rawPayload !== 'object' || Array.isArray(rawPayload)) {
        throw new ApprovalStoreError('bad_request', 'prepared_payload must be a Matrix content object');
      }
      const canonicalPayload = canonicalMatrixContent(rawPayload);
      const payloadBytes = JSON.stringify(canonicalPayload);
      if (Buffer.byteLength(payloadBytes) > PROJECTION_PAYLOAD_MAX_BYTES) {
        throw new ApprovalStoreError('bad_request', 'prepared payload too large');
      }
      const payload = JSON.parse(payloadBytes);
      const payloadVersion = input?.payload_version === undefined ? 1 : Number(input.payload_version);
      if (!Number.isFinite(payloadVersion) || !Number.isInteger(payloadVersion) || payloadVersion !== 1) {
        throw new ApprovalStoreError('bad_request', 'payload_version must be the supported integer version 1');
      }
      const preparedEventType = text(input?.prepared_event_type, 'prepared_event_type', 255);
      if (preparedEventType !== 'm.room.message' && preparedEventType !== 'm.room.encrypted') {
        throw new ApprovalStoreError(
          'bad_request',
          'prepared_event_type must be m.room.message or m.room.encrypted',
        );
      }
      const plan = {
        publisher_scope: text(input?.publisher_scope, 'publisher_scope', 255),
        publisher_mxid: fullMxid(input?.publisher_mxid, 'publisher_mxid'),
        homeserver: text(input?.homeserver, 'homeserver', 255),
        credential_kind: text(input?.credential_kind, 'credential_kind', 64),
        credential_generation: text(input?.credential_generation, 'credential_generation', 255),
        payload_version: payloadVersion,
        prepared_event_type: preparedEventType,
        prepared_payload: structuredClone(payload),
        ...(row.migrationKind === 'legacy_v1' ? { legacy_evidence_cas: input.legacy_evidence_cas } : {}),
      };
      const transactionMaterial = `${row.identity}\0${plan.publisher_mxid}\0${plan.credential_generation}`;
      plan.transaction_id = `hafleet_${createHash('sha256').update(transactionMaterial).digest('hex').slice(0, 32)}`;
      row.planCasToken = createHash('sha256').update(JSON.stringify([row.identity,row.targetRoomId,plan])).digest('hex');
      row.plan = plan; row.attemptState = 'ready'; this._save();
    }
    return { plan: { ...structuredClone(row.plan), cas_token: row.planCasToken, attempt_state: row.attemptState } };
  }

  beginProjectionSend(planToken, input = {}) {
    const row = this._planRow(planToken, input);
    if (row.migrationKind === 'legacy_v1') this._validateLegacyStatusPlan(row, row.plan);
    if (row.superseded && row.attemptState === 'ready') {
      throw new ApprovalStoreError('conflict', 'projection plan superseded before send');
    }
    if (row.eventId) return { plan: { ...structuredClone(row.plan), cas_token: row.planCasToken, attempt_state: 'receipted' } };
    if (row.attemptState === 'ready' || row.attemptState === 'uncertain') { row.attemptState = 'attempted'; this._save(); }
    return { plan: { ...structuredClone(row.plan), cas_token: row.planCasToken, attempt_state: row.attemptState } };
  }

  receiptProjection(planToken, input) {
    const eventId = text(input?.event_id, 'event_id', 255);
    if (!EVENT_ID_RE.test(eventId)) throw new ApprovalStoreError('bad_request', 'event_id must be a Matrix event id');
    const row = this._planRow(planToken, input);
    if (row.eventId) {
      if (row.eventId === eventId) return { event_id: eventId };
      throw new ApprovalStoreError('conflict', 'projection already receipted');
    }
    if (row.attemptState !== 'attempted' && row.attemptState !== 'uncertain') {
      throw new ApprovalStoreError('conflict', 'projection receipt mismatch');
    }
    row.eventId = eventId; this._save(); return { event_id: row.eventId };
  }

  retryProjection(planToken, input = {}) {
    const row = this._planRow(planToken, input);
    if (row.migrationKind === 'legacy_v1') this._validateLegacyStatusPlan(row, row.plan);
    if (row.superseded) throw new ApprovalStoreError('conflict', 'projection plan superseded');
    if (row.eventId) throw new ApprovalStoreError('conflict', 'projection already receipted');
    if (row.attemptState !== 'attempted' && row.attemptState !== 'uncertain') {
      throw new ApprovalStoreError('conflict', 'projection send has not begun');
    }
    const requested = Number(input.retry_at); const now = this.now();
    row.attemptState = 'uncertain';
    row.nextAttemptAt = Number.isFinite(requested)
      ? Math.min(Math.max(now, requested), now + MAX_RETRY_DELAY_MS)
      : now;
    row.lastErrorCode = optionalText(input.error_code, 128); this._save();
    return { attempt_state: row.attemptState, retry_at: row.nextAttemptAt };
  }

  listAudit() {
    return this.state.audit.map((entry) => ({ ...entry }));
  }
}

export function createApprovalStore(filePath, options = {}) {
  return new ApprovalStore(filePath, options);
}
