export const legacyId = 'approval_' + 'a'.repeat(32);
export function legacyRecord(overrides = {}) {
  return { id: legacyId, agent: 'worker', runtime: 'codex', project: 'p', projectRoomId: '!p:test',
    ownerMxid: '@owner:test', ownerDmRoomId: '!dm:test', upstreamRequestId: 'old-upstream',
    inputDigest: 'd'.repeat(64), toolName: 'Bash', description: 'private description', inputPreview: 'private command',
    status: 'consumed', decision: 'allow', createdAt: 100, expiresAt: 500, decidedAt: 200, consumedAt: 300,
    matrixEventId: '$owner-verdict', ...overrides };
}
export function legacyDisk(record = legacyRecord()) {
  return { version: 1, requests: { [record.id]: record }, bindings: {}, audit: [] };
}
export function publisherInput(overrides = {}) {
  return { publisher_scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
    credential_kind: 'local_bot', credential_generation: 'current-g1', ...overrides };
}
export function legacyAttestation(row, record = legacyRecord(), overrides = {}) {
  const tuple = { request_id: record.id, agent: record.agent, project: record.project,
    project_room_id: record.projectRoomId, input_digest: record.inputDigest };
  const common = { room_id: record.ownerDmRoomId, type: 'm.room.message', payload_key: 'com.agentchat.approval',
    version: 1, edited: false, redacted: false, ...tuple };
  return { schema_version: 1, request_id: record.id, revision: row.revision, channel: 'private_status', cas_token: row.cas_token,
    candidate_verdict_event_id: record.matrixEventId,
    verdict: { ...common, event_id: record.matrixEventId, sender: record.ownerMxid,
      msgtype: 'com.agentchat.approval.verdict.v1', kind: 'verdict', action: 'approve_once', reply_to_event_id: '$original-request' },
    original: { ...common, event_id: '$original-request', sender: '@bot:test',
      msgtype: 'com.agentchat.approval.request.v1', kind: 'request', runtime: record.runtime,
      upstream_request_id: record.upstreamRequestId, expires_at: record.expiresAt },
    publisher: publisherInput(),
    private_context: { room_id: record.ownerDmRoomId, owner_mxid: record.ownerMxid, ready: true, joined: true, encrypted: true },
    ...overrides };
}
export function planIdentity(row, plan) {
  return { request_id: row.request_id, revision: row.revision, channel: row.channel,
    publisher_scope: plan.publisher_scope, publisher_mxid: plan.publisher_mxid, room_id: row.target_room_id,
    credential_generation: plan.credential_generation, transaction_id: plan.transaction_id, cas_token: plan.cas_token };
}
