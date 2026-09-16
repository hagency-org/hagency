import { expect, test } from 'vitest';
import { normalizeLegacyProofEvent } from '../lib/legacy-approval-projection.js';

const approval = { id: 'approval_' + '1'.repeat(32), agent: 'worker', project: 'p', project_room_id: '!project:test',
  input_digest: 'a'.repeat(64), owner_mxid: '@owner:test', runtime: 'codex', upstream_request_id: 'old-upstream', expires_at: 500, decision: 'allow' };
const room = '!owner:test';
function fixture(role, namespace = 'com.agentchat.approval') {
  const tuple = { request_id: approval.id, agent: approval.agent, project: approval.project,
    project_room_id: approval.project_room_id, input_digest: approval.input_digest };
  const event = { event_id: `$${role}`, room_id: room, sender: role === 'verdict' ? approval.owner_mxid : '@bot:test',
    type: 'm.room.message', content: { msgtype: `${namespace}.${role}.v1`, body: 'historical body',
      [namespace]: { version: 1, kind: role, ...tuple, ...(role === 'verdict' ? { action: 'approve_once' }
        : { runtime: 'codex', upstream_request_id: 'old-upstream', expires_at: 500, actions: [{ id: 'approve_once' }, { id: 'deny' }] }) },
      ...(role === 'verdict' ? { 'm.relates_to': { 'm.in_reply_to': { event_id: '$request' } } } : {}) } };
  return { raw: structuredClone(event), clear: event };
}
function normalize(pair, role) { return normalizeLegacyProofEvent(pair.raw, pair.clear, room, `$${role}`, role, approval); }

test('legacy proof rejects wrong role tuple sender relation and edited or redacted events', () => {
  const mutations = [
    ['returned event', 'verdict', p => { p.raw.event_id = '$other'; }],
    ['returned room', 'request', p => { p.raw.room_id = '!other:test'; }],
    ['raw redacted', 'request', p => { p.raw.unsigned = { redacted_because: {} }; }],
    ['clear redacted', 'request', p => { p.clear.unsigned = { redacted_because: {} }; }],
    ['clear event', 'request', p => { p.clear.event_id = '$other'; }],
    ['clear room', 'request', p => { p.clear.room_id = '!other:test'; }],
    ['clear sender', 'request', p => { p.clear.sender = '@attacker:test'; }],
    ['not full sender', 'request', p => { p.raw.sender = p.clear.sender = 'bot'; }],
    ['owner impersonation', 'verdict', p => { p.raw.sender = p.clear.sender = '@other:test'; }],
    ['unrecognized event type', 'request', p => { p.raw.type = p.clear.type = 'm.room.topic'; }],
    ['edited content', 'request', p => { p.clear.content['m.new_content'] = {}; }],
    ['replacement relation', 'request', p => { p.clear.content['m.relates_to'] = { rel_type: 'm.replace', event_id: '$old' }; }],
    ['missing reply', 'verdict', p => { delete p.clear.content['m.relates_to']; }],
    ['ambiguous reply', 'verdict', p => { p.clear.content['m.relates_to'].event_id = '$other'; }],
    ['unsupported nested reply', 'verdict', p => { p.clear.content['m.relates_to']['m.in_reply_to'].extra = '$other'; }],
    ['wrong decision', 'verdict', p => { p.clear.content['com.agentchat.approval'].action = 'deny'; }],
    ['wrong role', 'verdict', p => { p.clear.content['com.agentchat.approval'].kind = 'request'; }],
    ['wrong protocol pair', 'request', p => { p.clear.content.msgtype = 'com.hafleet.approval.request.v1'; }],
    ['ambiguous namespace', 'request', p => { p.clear.content['com.hafleet.approval'] = p.clear.content['com.agentchat.approval']; }],
    ...['request_id', 'agent', 'project', 'project_room_id', 'input_digest', 'runtime', 'upstream_request_id', 'expires_at'].map(key =>
      [key, 'request', p => { p.clear.content['com.agentchat.approval'][key] = 'different'; }]),
    ['caller publisher', 'request', p => { p.clear.content['com.agentchat.approval'].publisher_mxid = '@other:test'; }],
    ['caller owner', 'request', p => { p.clear.content['com.agentchat.approval'].owner_mxid = '@other:test'; }],
    ['wrong actions', 'request', p => { p.clear.content['com.agentchat.approval'].actions[0].id = 'approve_forever'; }],
  ];
  for (const [name, role, mutate] of mutations) {
    const pair = fixture(role); mutate(pair);
    expect(() => normalize(pair, role), name).toThrow();
  }
});

test.each(['com.agentchat.approval', 'com.hafleet.approval'])('legacy complete %s protocol normalizes only bounded proof fields', namespace => {
  const request = normalize(fixture('request', namespace), 'request');
  const verdict = normalize(fixture('verdict', namespace), 'verdict');
  expect(request).toMatchObject({ payload_key: namespace, sender: '@bot:test', event_id: '$request', runtime: 'codex', expires_at: 500 });
  expect(verdict).toMatchObject({ sender: '@owner:test', action: 'approve_once', reply_to_event_id: '$request' });
  expect(JSON.stringify({ request, verdict })).not.toContain('historical body');
  expect(request).not.toHaveProperty('actions');
  expect(request).not.toHaveProperty('content');
});
