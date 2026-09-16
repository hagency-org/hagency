// A single owned legacy attempt. Scheduling and native approval authority live elsewhere.
export class LegacyApprovalProjectionError extends Error {
  constructor(code) { super(code); this.code = code; }
}
const fail = code => { throw new LegacyApprovalProjectionError(code); };
const eventId = value => typeof value === 'string' && /^\$[^\s]{1,254}$/.test(value);
const fullMxid = value => typeof value === 'string' && value.length <= 255 && /^@[^:\s]+:[^\s]+$/.test(value);

export function createLegacyApprovalBudget({ signal, isCurrent = () => true, timeoutMs = 25_000, httpTimeoutMs = 10_000 } = {}) {
  const deadline = Date.now() + Math.min(Math.max(timeoutMs, 1), 25_000);
  return {
    signal, actorGuard: null,
    check() {
      if (signal?.aborted || isCurrent() !== true) fail('legacy_stopped');
      if (Date.now() >= deadline) fail('legacy_deadline');
      if (this.actorGuard && !this.actorGuard()) fail('legacy_publisher_changed');
    },
    remaining() { this.check(); return Math.min(Math.max(httpTimeoutMs, 1), 10_000, deadline - Date.now()); },
  };
}

// The controller owns headers AND the full body, including any rate-limit observer.
// Crypto is deliberately never raced against this or another timeout Promise.
export async function legacyOwnedJsonRequest(url, options, budget) {
  const controller = new AbortController();
  const abort = () => controller.abort();
  const timeout = setTimeout(abort, budget.remaining());
  budget.signal?.addEventListener('abort', abort, { once: true });
  try {
    budget.check();
    const start = () => fetch(url, { method: options.method || 'GET', headers: options.headers, redirect: 'error',
      ...(options.body !== undefined ? { body: JSON.stringify(options.body) } : {}), signal: controller.signal });
    const response = options.admit
      ? await options.admit(start, controller.signal)
      : await start();
    controller.signal.throwIfAborted();
    const reader = response.body?.getReader();
    let size = 0; const chunks = [];
    if (reader) {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        size += value.byteLength;
        if (size > 256 * 1024) fail('legacy_response_too_large');
        chunks.push(value);
      }
    }
    controller.signal.throwIfAborted();
    if (!options.completeStartedSend) budget.check();
    const text = Buffer.concat(chunks).toString('utf8');
    let body;
    try { body = text ? JSON.parse(text) : null; } catch { fail('legacy_invalid_json'); }
    const result = { status: response.status, ok: response.ok, body };
    // Observers receive only the bounded parsed body, never a Response clone that
    // could consume an unbounded error stream ahead of this reader's byte cap.
    if (options.observeResponse && await options.observeResponse(result)) fail('legacy_rate_limited');
    if (!options.completeStartedSend) budget.check();
    return result;
  } catch (error) {
    if (error instanceof LegacyApprovalProjectionError) throw error;
    if (controller.signal.aborted) fail(budget.signal?.aborted ? 'legacy_stopped' : 'legacy_http_timeout');
    fail('legacy_transport_failed');
  } finally {
    controller.abort(); clearTimeout(timeout); budget.signal?.removeEventListener('abort', abort);
  }
}

function validateRawEnvelope(raw, roomId, expectedId) {
  if (!raw || !eventId(expectedId) || raw.event_id !== expectedId || raw.room_id !== roomId
    || !fullMxid(raw.sender) || raw.unsigned?.redacted_because
    || !['m.room.message', 'm.room.encrypted'].includes(raw.type)) fail('legacy_invalid_envelope');
}

function clearOriginalEnvelope(raw, clear, roomId, expectedId) {
  validateRawEnvelope(raw, roomId, expectedId);
  if (!clear || clear.event_id !== raw.event_id || clear.room_id !== raw.room_id
    || clear.sender !== raw.sender || clear.type !== 'm.room.message' || clear.unsigned?.redacted_because
    || !clear.content || typeof clear.content !== 'object' || Array.isArray(clear.content)
    || Object.hasOwn(clear.content, 'm.new_content')
    || clear.content['m.relates_to']?.rel_type === 'm.replace') fail('legacy_decrypted_envelope_mismatch');
  return clear;
}

export function normalizeLegacyProofEvent(raw, clear, roomId, expectedId, role, approval) {
  const event = clearOriginalEnvelope(raw, clear, roomId, expectedId);
  const content = event.content;
  const namespaces = ['com.agentchat.approval', 'com.hagency.approval', 'com.hafleet.approval'];
  const namespace = namespaces.find(key => content.msgtype === `${key}.${role}.v1`);
  const detail = namespace && content[namespace];
  if (!detail || detail.version !== 1 || detail.kind !== role
    || namespaces.filter(key => Object.hasOwn(content, key)).length !== 1) fail('legacy_invalid_role');
  const tuple = { request_id: approval.id, agent: approval.agent, project: approval.project,
    project_room_id: approval.project_room_id, input_digest: approval.input_digest };
  if (Object.entries(tuple).some(([key, value]) => typeof value !== 'string' || !value || detail[key] !== value)) fail('legacy_tuple_mismatch');
  const normalized = { room_id: roomId, type: 'm.room.message', payload_key: namespace, version: 1,
    edited: false, redacted: false, ...tuple, event_id: expectedId, sender: event.sender, msgtype: content.msgtype, kind: role };
  if (role === 'verdict') {
    const relation = content['m.relates_to'];
    if (event.sender !== approval.owner_mxid || !['allow', 'deny'].includes(approval.decision)
      || detail.action !== (approval.decision === 'allow' ? 'approve_once' : 'deny')
      || !relation || Object.keys(relation).some(key => key !== 'm.in_reply_to')
      || !eventId(relation['m.in_reply_to']?.event_id)
      || Object.keys(relation['m.in_reply_to']).some(key => key !== 'event_id')) fail('legacy_invalid_verdict');
    return { ...normalized, action: detail.action, reply_to_event_id: relation['m.in_reply_to'].event_id };
  }
  if (Object.hasOwn(content, 'm.relates_to') || detail.runtime !== approval.runtime
    || detail.upstream_request_id !== approval.upstream_request_id || !Number.isFinite(detail.expires_at)
    || detail.expires_at !== approval.expires_at
    || !Array.isArray(detail.actions) || detail.actions.length !== 2
    || detail.actions[0]?.id !== 'approve_once' || detail.actions[1]?.id !== 'deny'
    || (detail.owner_mxid !== undefined && detail.owner_mxid !== approval.owner_mxid)
    || (detail.publisher_mxid !== undefined && detail.publisher_mxid !== event.sender)) fail('legacy_invalid_original');
  return { ...normalized, runtime: detail.runtime, upstream_request_id: detail.upstream_request_id, expires_at: detail.expires_at };
}

function actorFields(actor) {
  return { publisher_scope: actor.scope, publisher_mxid: actor.publisher_mxid, homeserver: actor.homeserver,
    credential_kind: actor.credential_kind, credential_generation: actor.credential_generation };
}
function planMatches(plan, actor) {
  return plan && Object.entries(actorFields(actor)).every(([key, value]) => plan[key] === value);
}
function choosePublisher(io, row, evidence, pin) {
  const matches = io.actors(row).filter(actor => actor.publisher_mxid === evidence.original_sender
    && (!pin || planMatches(pin, actor)));
  if (matches.length !== 1) fail('legacy_original_publisher_unavailable');
  return matches[0];
}

export async function publishLegacyApprovalProjection(row, io, options = {}) {
  const budget = createLegacyApprovalBudget(options);
  let began = false; let identity; let privateContext;
  const prefix = `/api/approvals/${encodeURIComponent(row?.request_id || '')}/matrix`;
  const projectionPath = `${prefix}/projections/${row?.revision}`;
  const backend = (method, endpoint, body) => io.backend(method, endpoint, body, budget);
  const bind = actor => { budget.actorGuard = () => io.current(actor, row); budget.check(); return actor; };
  try {
    if (row?.channel !== 'private_status' || row.migration_kind !== 'legacy_v1'
      || !row.approval || row.approval.id !== row.request_id || row.approval.owner_dm_room_id !== row.target_room_id) fail('legacy_row_required');
    let metadata = await backend('GET', `${prefix}/legacy-original?cas_token=${encodeURIComponent(row.cas_token)}`);
    let actor;
    if (!metadata.evidence) {
      if (row.plan || !eventId(metadata.candidate_verdict_event_id)) fail('legacy_original_unresolved');
      const readers = io.actors(row);
      // Prefer the ready local crypto client for reading. This does not choose the
      // status publisher, which is resolved only from the proven original sender.
      const reader = bind(readers.find(item => item.scope === 'local_bot') || (readers.length === 1 ? readers[0] : null));
      privateContext = await io.privateContext(reader, row, budget);
      const read = async (id, role) => {
        budget.check(); const raw = await io.event(reader, row, id, budget);
        validateRawEnvelope(raw, row.target_room_id, id);
        let clear = raw;
        if (raw?.type === 'm.room.encrypted') {
          budget.check(); clear = await io.decrypt(reader, raw, row.target_room_id); budget.check();
        }
        return normalizeLegacyProofEvent(raw, clear, row.target_room_id, id, role, row.approval);
      };
      const verdict = await read(metadata.candidate_verdict_event_id, 'verdict');
      if (verdict.reply_to_event_id === verdict.event_id) fail('legacy_invalid_verdict');
      const original = await read(verdict.reply_to_event_id, 'request');
      actor = bind(choosePublisher(io, row, { original_sender: original.sender }, null));
      privateContext = await io.privateContext(actor, row, budget);
      await backend('PUT', '/api/approvals/matrix/publishers', { ...actorFields(actor), scope: actor.scope, ...(actor.side_id ? { side_id: actor.side_id } : {}) });
      await backend('POST', `${prefix}/legacy-original`, { schema_version: 1, request_id: row.request_id,
        revision: row.revision, channel: row.channel, cas_token: row.cas_token,
        candidate_verdict_event_id: metadata.candidate_verdict_event_id, verdict, original,
        publisher: actorFields(actor), private_context: privateContext });
      metadata = await backend('GET', `${prefix}/legacy-original?cas_token=${encodeURIComponent(row.cas_token)}`);
    }
    actor = bind(choosePublisher(io, row, metadata.evidence, metadata.status_publisher || row.plan));
    let plan = row.plan;
    if (!plan) {
      privateContext = await io.privateContext(actor, row, budget);
      const content = metadata.content;
      if (!content || content.msgtype !== 'com.agentchat.approval.status.v1'
        || content['com.agentchat.approval']?.migration_kind !== 'legacy_v1'
        || content['com.agentchat.approval']?.request_id !== row.request_id
        || content['com.agentchat.approval']?.publisher_mxid !== actor.publisher_mxid
        || Object.hasOwn(content['com.agentchat.approval'], 'actions')) fail('legacy_canonical_status_unavailable');
      budget.check();
      const prepared = privateContext.encrypted
        ? { event_type: 'm.room.encrypted', content: await io.encrypt(actor, row.target_room_id, structuredClone(content)) }
        : { event_type: 'm.room.message', content };
      budget.check();
      await backend('PUT', '/api/approvals/matrix/publishers', { ...actorFields(actor), scope: actor.scope, ...(actor.side_id ? { side_id: actor.side_id } : {}) });
      plan = (await backend('POST', `${projectionPath}/prepare`, { ...actorFields(actor), cas_token: row.cas_token,
        channel: row.channel, legacy_evidence_cas: metadata.evidence.evidence_cas, private_context: privateContext,
        payload_version: 1, prepared_event_type: prepared.event_type, prepared_payload: prepared.content })).plan;
    }
    if (!planMatches(plan, actor) || plan.legacy_evidence_cas !== metadata.evidence.evidence_cas
      || !['m.room.message', 'm.room.encrypted'].includes(plan.prepared_event_type)) fail('legacy_plan_mismatch');
    privateContext = await io.privateContext(actor, row, budget);
    if ((plan.prepared_event_type === 'm.room.encrypted') !== privateContext.encrypted) fail('legacy_plan_security_changed');
    identity = { cas_token: plan.cas_token, channel: row.channel, ...actorFields(actor), room_id: row.target_room_id,
      transaction_id: plan.transaction_id, private_context: privateContext };
    // A lost begin response can already have committed. Preserve uncertainty
    // from the start of this owned mutation, including stop/rotation on its reply.
    budget.check(); began = true;
    await backend('POST', `${projectionPath}/begin-send`, identity);
    budget.check();
    const response = await io.send(actor, row, plan, budget);
    if (!eventId(response?.event_id)) fail('legacy_send_receipt_missing');
    // A complete successful send has an exact immutable receipt to finish even
    // after worker stop. This fresh budget authorizes only this backend receipt.
    await io.backend('POST', `${projectionPath}/receipt`, { ...identity, event_id: response.event_id },
      createLegacyApprovalBudget({ timeoutMs: 10_000 }));
    return { ok: true, event_id: response.event_id };
  } catch (error) {
    const code = error instanceof LegacyApprovalProjectionError ? error.code : 'legacy_unresolved';
    if (began) {
      try { budget.check(); await backend('POST', `${projectionPath}/retry`, { ...identity, error_code: code }); } catch { /* Durable attempted work remains recoverable. */ }
      return { ok: false, uncertain: true, error_code: code };
    }
    return { ok: false, unresolved: true, error_code: code };
  }
}
