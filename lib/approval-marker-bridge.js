const MARKER_V1_TYPE = 'com.agentchat.approval.room.v1';
const MAX_MARKER_RESPONSE_BYTES = 64 * 1024;

export async function markerOwnedJsonRequest(url, options, timeoutMs) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const fetchImpl = options.fetchImpl || fetch;
  try {
    const start = () => fetchImpl(url, {
      method: options.method || 'GET',
      headers: options.headers,
      ...(options.body === undefined ? {} : { body: JSON.stringify(options.body) }),
      redirect: 'error',
      signal: controller.signal,
    });
    const response = options.admit
      ? await options.admit(start, controller.signal)
      : await start();
    const reader = response.body?.getReader();
    const chunks = [];
    let size = 0;
    if (reader) {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        size += value.byteLength;
        if (size > MAX_MARKER_RESPONSE_BYTES) {
          throw new Error('approval marker state response is too large');
        }
        chunks.push(value);
      }
    }
    controller.signal.throwIfAborted();
    const text = Buffer.concat(chunks).toString('utf8');
    let body = null;
    if (text) {
      try {
        body = JSON.parse(text);
      } catch {
        throw new Error('approval marker state response is invalid JSON');
      }
    }
    const result = { status: response.status, ok: response.ok, body };
    if (options.observeResponse && await options.observeResponse(result)) {
      throw new Error('approval marker state request is rate limited');
    }
    controller.signal.throwIfAborted();
    return result;
  } catch (error) {
    if (controller.signal.aborted) {
      throw new Error('approval marker state request exceeded its deadline');
    }
    throw error;
  } finally {
    controller.abort();
    clearTimeout(timer);
  }
}

function sameActor(left, right) {
  return Boolean(left && right)
    && left.scope === right.scope
    && left.publisher_mxid === right.publisher_mxid
    && left.homeserver === right.homeserver
    && left.credential_kind === right.credential_kind
    && left.credential_generation === right.credential_generation;
}

function pinnedActor(row, current) {
  const plan = row.plan;
  if (!plan) return current;
  return {
    ...current,
    scope: plan.publisher_scope,
    publisher_mxid: plan.publisher_mxid,
    credential_kind: plan.credential_kind,
    credential_generation: plan.credential_generation,
  };
}

function markerIdentity(plan, row) {
  return {
    cas_token: plan.cas_token,
    approval_room_id: row.approval_room_id,
    binding_generation: row.binding_generation,
    marker_channel: row.marker_channel,
    publisher_scope: plan.publisher_scope,
    publisher_mxid: plan.publisher_mxid,
    credential_kind: plan.credential_kind,
    credential_generation: plan.credential_generation,
  };
}

function actorForPlan(plan, current) {
  return {
    ...current,
    scope: plan.publisher_scope,
    publisher_mxid: plan.publisher_mxid,
    credential_kind: plan.credential_kind,
    credential_generation: plan.credential_generation,
  };
}

function isCanonicalLegacyMarker(content) {
  if (!content || typeof content !== 'object' || Array.isArray(content)
    || content.version !== 1
    || !Number.isSafeInteger(content.binding_generation)
    || content.binding_generation < 1
    || typeof content.publisher_mxid !== 'string'
    || !/^@[^:\s]+:[^\s]+$/.test(content.publisher_mxid)
    || typeof content.owner_mxid !== 'string'
    || !/^@[^:\s]+:[^\s]+$/.test(content.owner_mxid)
    || typeof content.agent !== 'string'
    || !content.agent.trim()
    || !Array.isArray(content.project_room_associations)
    || content.project_room_associations.length > 64) {
    return false;
  }
  return content.project_room_associations.every((association) => (
    association
    && typeof association === 'object'
    && !Array.isArray(association)
    && typeof association.project_room_id === 'string'
    && /^![^:\s]+:[^\s]+$/.test(association.project_room_id)
    && typeof association.active === 'boolean'
  ));
}

export async function publishApprovalMarker(row, io) {
  const current = await io.resolveActor(row);
  const actor = pinnedActor(row, current);
  if (!actor || !sameActor(actor, current)) {
    throw new Error('approval marker publisher changed before replay');
  }

  let plan = row.plan;
  if (!plan) {
    const proposal = {
      cas_token: row.cas_token,
      approval_room_id: row.approval_room_id,
      binding_generation: row.binding_generation,
      marker_channel: row.marker_channel,
      publisher_scope: actor.scope,
      publisher_mxid: actor.publisher_mxid,
      credential_kind: actor.credential_kind,
      credential_generation: actor.credential_generation,
    };
    plan = (await io.prepare(proposal, row)).plan;
    if (!sameActor(actor, await io.resolveActor(row))) {
      throw new Error('approval marker publisher changed after durable preparation');
    }
  }
  if (!plan || !sameActor(actor, actorForPlan(plan, actor))) {
    throw new Error('approval marker durable plan publisher mismatch');
  }

  const identity = markerIdentity(plan, row);
  let began = false;
  try {
    await io.begin(identity, row);
    began = true;
    if (!sameActor(actor, await io.resolveActor(row))) {
      throw new Error('approval marker publisher changed before Matrix state send');
    }
    const eventId = await io.send(plan, actor, row);
    if (typeof eventId !== 'string' || !eventId.startsWith('$')) {
      throw new Error('Matrix marker state send did not return an event id');
    }
    await io.receipt(identity, eventId, row);
    return { ok: true, event_id: eventId };
  } catch (error) {
    if (!began) throw error;
    try {
      await io.retry(identity, error, row);
      return { ok: false, uncertain: true, error };
    } catch (retryError) {
      return { ok: false, uncertain: true, error, retry_error: retryError };
    }
  }
}

export async function syncApprovalRoomMarker(binding, io) {
  const actor = await io.resolveActor({ approval_room_id: binding.approval_room_id });
  if (!actor) throw new Error('approval marker publisher unavailable');
  await io.registerPublisher(actor);
  if (!sameActor(actor, await io.resolveActor({ approval_room_id: binding.approval_room_id }))) {
    throw new Error('approval marker publisher changed during registration');
  }
  const result = await io.sync({
    agent: binding.agent,
    owner_mxid: binding.owner_mxid,
    approval_room_id: binding.approval_room_id,
    publisher_scope: actor.scope,
    publisher_mxid: actor.publisher_mxid,
    credential_kind: actor.credential_kind,
    credential_generation: actor.credential_generation,
  });
  if (!sameActor(actor, await io.resolveActor({ approval_room_id: binding.approval_room_id }))) {
    throw new Error('approval marker publisher changed during synchronization');
  }
  return result;
}

// An inactive retained scope needs no active binding candidate, but it still
// requires the exact currently verified private actor and backend/store proof.
export async function migrateApprovalRoomMarker(room, io) {
  const row = { approval_room_id: room.approval_room_id };
  const actor = await io.resolveActor(row);
  if (!actor) throw new Error('approval marker publisher unavailable');
  await io.registerPublisher(actor);
  if (!sameActor(actor, await io.resolveActor(row))) throw new Error('approval marker publisher changed');
  return io.migrate({ ...row, limit: 1, publisher_scope: actor.scope,
    publisher_mxid: actor.publisher_mxid, credential_kind: actor.credential_kind,
    credential_generation: actor.credential_generation });
}

export async function reconcileObservedLegacyMarker(room, io) {
  const actor = await io.resolveActor({ approval_room_id: room.approval_room_id });
  if (!actor) throw new Error('approval marker publisher unavailable');
  const observed = await io.readLegacyState(room.approval_room_id, actor, MARKER_V1_TYPE);
  if (!sameActor(actor, await io.resolveActor({ approval_room_id: room.approval_room_id }))) {
    throw new Error('approval marker publisher changed during legacy state observation');
  }
  if (!isCanonicalLegacyMarker(observed)) {
    return { observed_nonempty: false, reconciliation: null };
  }
  const reconciliation = await io.reconcile({
    approval_room_id: room.approval_room_id,
    legacy_state_observed_nonempty: true,
    limit: 1,
    publisher_scope: actor.scope,
    publisher_mxid: actor.publisher_mxid,
    credential_kind: actor.credential_kind,
    credential_generation: actor.credential_generation,
  });
  if (!sameActor(actor, await io.resolveActor({ approval_room_id: room.approval_room_id }))) {
    throw new Error('approval marker publisher changed during retirement reconciliation');
  }
  return { observed_nonempty: true, reconciliation };
}
