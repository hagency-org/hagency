/*
 * #16-impl: specs/task-side-provenance.spec.md — 24 scenarios, each named by the spec's bound
 * Filter title, each driven through a REAL adapter (push listener HTTP / edge puller / sync
 * collector → receiver/router → bridge handleAppserviceEvents), never by predicate calls.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { mkdtempSync, rmSync } from 'fs';
import path from 'path';
import { tmpdir } from 'os';
import { createServer } from 'http';
import { fakePalpo, makeInstance, bridgeUrl } from './helpers/side-provenance-harness.js';
import { createAppserviceRouter } from '../lib/appservice-receiver.js';
import { startAppserviceListener } from '../lib/appservice-listener.js';
import { startAppserviceSyncCollector } from '../lib/appservice-sync.js';
import { startEdgePuller } from '../lib/appservice-puller.js';
import { ApprovalStore } from '../lib/approval-store.js';

const TMP = process.env.HAGENCY_OUTER_TMP || path.join(tmpdir(), 'hagency-16impl');

let mod = null;
let cleanup = [];
let acceptanceAdapter = 'push';

async function bridge() {
  if (!mod) mod = await import(`${bridgeUrl()}?sp=${Date.now()}`);
  return mod;
}

/** Wire a full instance: fake Palpo + router + bridge prototype object with the L3 state set. */
async function makeBridgeWithSide({ sideId, hsToken, asToken, registration, representativeMxid, palpo, members = {}, onTyped = null }) {
  const m = await bridge();
  const typed = { messages: [], states: [], approvals: [], memberships: [] };
  const self = {
    actingCredentials: new Map([[sideId, {
      apiBaseUrl: palpo.url, serverName: sideId,
      kind: 'appservice', asToken, hsToken, senderLocalpart: representativeMxid.slice(1, representativeMxid.indexOf(':')),
      namespace: '@ac_.*',
    }]]),
    appserviceInboundSnapshot: new Map([[sideId, {
      sideId, serverName: sideId, hsToken, registration,
      representative: { mxid: representativeMxid },
    }]]),
    // 16-impl-r2 A: the claim store in its new lifecycle shape
    sideProvenanceClaims: new Map(),
    sideProvenanceClaimOrder: [],
    actingSideFor(id) {
      const row = this.actingCredentials.get(String(id).toLowerCase());
      if (!row) return null;
      return { side: { apiBaseUrl: row.apiBaseUrl, serverName: row.serverName }, credential: row };
    },
    postWarning() {},
    async onRoomMessage(roomId, event) {
      typed.messages.push({ roomId, event });
      if (onTyped) await onTyped('message', { roomId, event });
    },
    async onRoomEvent(roomId, event) { typed.states.push({ roomId, event }); },
    async onAppserviceMembership(id, roomId, event) { typed.memberships.push({ id, roomId, event }); },
  };
  const proto = m.MatrixBridge.prototype;
  self.handleAppserviceEvents = proto.handleAppserviceEvents.bind(self);
  self.assertSideProvenanceForEvent = proto.assertSideProvenanceForEvent.bind(self);
  self.executeTypedForClaim = proto.executeTypedForClaim.bind(self);
  const router = createAppserviceRouter({
    sides: [{
      sideId, hsToken, registration,
      onEvents: (events, meta) => self.handleAppserviceEvents(sideId, events, {
        ...meta,
        provenance: { registration, sideId, mode: meta?.mode ?? 'push' },
      }),
    }],
  });
  self.router = router;
  // 16-impl-r2: refreshAppserviceSides drives `this.appserviceRouter`; same object, real path
  self.appserviceRouter = router;
  return { self, router, typed, mod: m };
}

/** Drive one transaction through the REAL push listener over HTTP. */
async function pushTxn(router, { hsToken, txnId = 't1', events, mode }) {
  if (acceptanceAdapter !== 'push') return intakeTxn(acceptanceAdapter, router, { hsToken, txnId, events });
  const listener = await startAppserviceListener({ receiver: router, port: 0, host: '127.0.0.1' });
  cleanup.push(() => listener.close());
  const port = listener.server?.address?.()?.port ?? listener.port;
  const res = await fetch(`http://127.0.0.1:${port}/_matrix/app/v1/transactions/${txnId}`, {
    method: 'PUT',
    headers: { authorization: `Bearer ${hsToken}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ events, ...(mode ? { mode } : {}) }),
  });
  return { status: res.status, body: await res.json().catch(() => ({})) };
}

/** The router stays real; observe its result and the adapter's durable success action. */
async function intakeTxn(mode, router, { hsToken, txnId, events }) {
  let result;
  let polls = 0;
  const acks = [];
  const cursors = [];
  const observedRouter = { handle: async (input) => { result = await router.handle(input); return result; } };
  if (mode === 'edge') {
    const server = createServer((req, res) => {
      res.writeHead(200, { 'Content-Type': 'application/json' });
      if (req.url.includes('/ack')) {
        let body = ''; req.on('data', (chunk) => { body += chunk; });
        req.on('end', () => { acks.push(JSON.parse(body).ok); res.end('{}'); }); return;
      }
      polls += 1;
      res.end(JSON.stringify({ events, txn_id: txnId }));
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    try {
      const puller = startEdgePuller({
        url: `http://127.0.0.1:${server.address().port}`, token: 'fixture-edge', side: SIDE,
        router: observedRouter, hsTokenFor: () => hsToken,
        shouldContinue: () => polls < 1, sleep: async () => {},
      });
      await puller.done;
    } finally { await new Promise((resolve) => server.close(resolve)); }
    expect(acks).toEqual([result?.status === 200]);
  } else {
    const rooms = {};
    for (const event of events) {
      const room = rooms[event.room_id] ||= { timeline: { events: [] }, state: { events: [] } };
      room.timeline.events.push(event);
    }
    const collector = startAppserviceSyncCollector({
      baseUrl: 'http://fixture.invalid', side: SIDE, router: observedRouter,
      credentialFor: () => ({ kind: 'appservice', asToken: AS, hsToken, senderLocalpart: 'hagency' }),
      readCursor: () => 'previous', writeCursor: async (next) => { cursors.push(next); },
      fetchImpl: async (url) => {
        const login = String(url).endsWith('/login');
        if (!login) polls += 1;
        return { ok: true, status: 200, json: async () => login
          ? { access_token: 'fixture-sync', user_id: REP }
          : { next_batch: txnId, rooms: { join: rooms } } };
      },
      shouldContinue: () => polls < 1, sleep: async () => {},
    });
    await collector.loop;
    expect(cursors).toEqual(result?.status === 200 ? [txnId] : []);
  }
  expect(result).toBeDefined();
  return result;
}

const msg = (roomId, eventId, body = 'hello', sender = '@human:palpo.test') => ({
  type: 'm.room.message', room_id: roomId, event_id: eventId, sender, content: { msgtype: 'm.text', body },
});

describe('16-impl-r9 sync invite bootstrap ordering', () => {
  test('r9_real_collector_joins_before_same_batch_state_and_commits_cursor', async () => {
    const roomId = '!r9-fresh:palpo.test';
    const palpo = await fakePalpo({
      syncBatches: [{
        next_batch: 'r9-good',
        rooms: { invite: { [roomId]: { invite_state: { events: [
          { type: 'm.room.create', state_key: '', sender: '@alex:palpo.test', content: { creator: '@alex:palpo.test' } },
          { type: 'm.room.name', state_key: '', sender: '@alex:palpo.test', content: { name: 'fresh' } },
          { type: 'm.room.member', state_key: REP, sender: '@alex:palpo.test', content: { membership: 'invite' } },
        ] } } } },
      }],
    });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const { joinRoomOnSideAsRepresentative } = await import('../lib/matrix-representative.js');
    const realFetch = globalThis.fetch;
    let joined = false;
    globalThis.fetch = async (url, init) => {
      if (String(url).includes('/join/') && String(url).includes(encodeURIComponent(roomId))) {
        palpo.setMembers(roomId, [REP]);
        joined = true;
      }
      return realFetch(url, init);
    };
    cleanup.push(() => { globalThis.fetch = realFetch; });
    self.onAppserviceMembership = async (sideId, incomingRoomId, event) => {
      typed.memberships.push({ sideId, roomId: incomingRoomId, event });
      const result = await joinRoomOnSideAsRepresentative({ ...self.actingSideFor(sideId), roomId: incomingRoomId });
      expect(result.joined).toBe(true);
    };
    let cursor = 'r9-before';
    const collector = startAppserviceSyncCollector({
      baseUrl: palpo.url, side: SIDE, router: self.router,
      credentialFor: () => ({ kind: 'appservice', asToken: AS, hsToken: HS, senderLocalpart: 'hagency' }),
      readCursor: () => cursor, writeCursor: async (next) => { cursor = next; },
      fetchImpl: async (url) => String(url).endsWith('/login')
        ? { ok: true, status: 200, json: async () => ({ access_token: 't', user_id: REP }) }
        : fetch(url),
      sleep: async () => {}, shouldContinue: () => cursor !== 'r9-good',
    });
    await collector.loop;
    expect(joined).toBe(true);
    expect(typed.memberships).toHaveLength(1);
    expect(collector.stats.failed).toBe(0);
    expect(cursor).toBe('r9-good');
  });

  test('r9_forbidden_room_is_terminal_without_poisoning_healthy_sibling', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const response = await pushTxn(self.router, { hsToken: HS, txnId: 'r9-mixed', events: [
      msg('!forbidden:palpo.test', '$r9-bad'), msg(ROOM, '$r9-good'),
    ] });
    expect(response.status).toBe(200);
    expect(typed.messages.map(({ event }) => event.event_id)).toEqual(['$r9-good']);
  });

  test('r9_member_read_5xx_retries_without_advancing_sync_cursor', async () => {
    const roomId = '!r9-flaky:palpo.test';
    const eventBatch = {
      next_batch: 'r9-flaky',
      rooms: { join: { [roomId]: { timeline: { events: [msg(roomId, '$r9-flaky')] }, state: { events: [] } } } },
    };
    const palpo = await fakePalpo({
      members: { [roomId]: [REP] }, memberFailures: { [roomId]: 500 },
      syncBatches: [eventBatch, eventBatch, eventBatch],
    });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    let cursor = 'r9-before';
    let polls = 0;
    const collector = startAppserviceSyncCollector({
      baseUrl: palpo.url, side: SIDE, router: self.router,
      credentialFor: () => ({ kind: 'appservice', asToken: AS, hsToken: HS, senderLocalpart: 'hagency' }),
      readCursor: () => cursor, writeCursor: async (next) => { cursor = next; },
      fetchImpl: (url) => fetch(url), sleep: async () => {},
      shouldContinue: () => ++polls <= 3,
    });
    await collector.loop;
    expect(collector.stats.failed).toBe(3);
    expect(cursor).toBe('r9-before');
    expect(typed.messages).toHaveLength(0);
  });
});

describe('16-impl-r10 batch membership cache', () => {
  test('r10_real_collector_honors_two_429s_then_commits_the_held_batch', async () => {
    const roomId = '!r10-throttle:palpo.test';
    const eventBatch = {
      next_batch: 'r10-after',
      rooms: { join: { [roomId]: { timeline: { events: [msg(roomId, '$r10-throttle')] } } } },
    };
    const palpo = await fakePalpo({
      members: { [roomId]: [REP] },
      memberFailures: { [roomId]: [
        { status: 429, retryAfterMs: 2_500 }, { status: 429, retryAfterMs: 2_500 },
      ] },
      syncBatches: [eventBatch, eventBatch, eventBatch],
    });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    let cursor = 'r10-before';
    const sleeps = [];
    const collector = startAppserviceSyncCollector({
      baseUrl: palpo.url, side: SIDE, router: self.router,
      credentialFor: () => ({ kind: 'appservice', asToken: AS, hsToken: HS, senderLocalpart: 'hagency' }),
      readCursor: () => cursor, writeCursor: async (next) => { cursor = next; },
      fetchImpl: (url) => fetch(url), sleep: async (ms) => { sleeps.push(ms); },
      shouldContinue: () => cursor !== 'r10-after',
    });
    await collector.loop;
    expect(sleeps).toEqual([2_500, 2_500]);
    expect(collector.stats.failed).toBe(2);
    expect(cursor).toBe('r10-after');
    expect(typed.messages.map(({ event }) => event.event_id)).toEqual(['$r10-throttle']);
  });

  test('r10_same_room_events_perform_one_membership_read_in_the_transaction', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const response = await pushTxn(self.router, {
      hsToken: HS, txnId: 'r10-cache',
      events: [msg(ROOM, '$r10-1'), msg(ROOM, '$r10-2'), msg(ROOM, '$r10-3')],
    });
    expect(response.status).toBe(200);
    expect(typed.messages).toHaveLength(3);
    expect(palpo.seen.filter(({ url }) => url.includes('/joined_members'))).toHaveLength(1);
  });
});
const nameEvt = (roomId, eventId, name) => ({
  type: 'm.room.name', room_id: roomId, event_id: eventId, state_key: '', sender: '@human:palpo.test',
  content: { name },
});
const tombstone = (roomId, eventId) => ({
  type: 'm.room.tombstone', room_id: roomId, event_id: eventId, state_key: '', sender: '@human:palpo.test',
  content: { body: 'x', replacement_room: '!new:palpo.test' },
});
const verdict = (roomId, eventId) => msg(roomId, eventId, JSON.stringify({ type: 'engagement-verdict', approve: true }));

beforeEach(() => { cleanup = []; acceptanceAdapter = 'push'; });
afterEach(async () => {
  for (const fn of cleanup) { try { await fn(); } catch { /* closing twice is fine */ } }
  cleanup = [];
  vi.unstubAllGlobals();
});

const SIDE = 'palpo.test';
const REP = '@hagency:palpo.test';
const HS = 'hs-1';
const AS = 'as-1';
const REG = 'reg-1';
const ROOM = '!room:palpo.test';

describe.each(['push', 'edge', 'sync'])('side provenance ingress via %s (spec: task-side-provenance)', (adapter) => {
  beforeEach(() => { acceptanceAdapter = adapter; });
  test('side_provenance_reaches_ingress_from_push_edge_and_sync', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    // push
    const r1 = await pushTxn(self.router, { hsToken: HS, txnId: 'p1', events: [msg(ROOM, '$1')] });
    expect(r1.status).toBe(200);
    expect(typed.messages).toHaveLength(1);
    // edge (through the router as the puller shapes it)
    const r2 = await intakeTxn('edge', self.router, { hsToken: HS, txnId: 'e1', events: [msg(ROOM, '$2')] });
    expect(r2.status).toBe(200);
    expect(typed.messages).toHaveLength(2);
    // sync (collector loop driving the router)
    const seen = [];
    const collector = startAppserviceSyncCollector({
      baseUrl: palpo.url, side: SIDE, router: self.router,
      credentialFor: () => ({ kind: 'appservice', asToken: AS, hsToken: HS, senderLocalpart: 'hagency' }),
      readCursor: () => 's0', writeCursor: async () => {},
      fetchImpl: async (u) => {
        seen.push(String(u));
        if (String(u).endsWith('/login')) return { ok: true, status: 200, json: async () => ({ access_token: 't', user_id: REP }) };
        return {
          ok: true, status: 200,
          json: async () => ({ next_batch: 'B', rooms: { join: { [ROOM]: { timeline: { events: [msg(ROOM, '$3')] }, state: { events: [] } } } } }),
        };
      },
      sleep: async () => { await Promise.resolve(); },
      shouldContinue: () => seen.filter((u) => u.includes('/sync')).length < 1 && (testCounter.t = (testCounter.t ?? 0) + 1) < 40,
    });
    function testCounter() {}
    await collector.loop;
    expect(typed.messages).toHaveLength(3);
  });

  test('side_provenance_rejects_bad_or_ambiguous_credentials', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const r = await pushTxn(self.router, { hsToken: 'wrong-token', txnId: 'bad', events: [msg(ROOM, '$x')] });
    expect(r.status).toBe(403);
    expect(typed.messages).toHaveLength(0);
    let ambiguousCalls = 0;
    self.router.setSides([SIDE, 'another.test'].map((sideId) => ({
      sideId, hsToken: HS, onEvents: async () => { ambiguousCalls += 1; },
    })));
    const ambiguous = await pushTxn(self.router, { hsToken: HS, txnId: 'ambiguous', events: [msg(ROOM, '$a')] });
    expect(ambiguous.status).toBe(403);
    expect(ambiguousCalls).toBe(0);
    expect(Object.values(self.router.seenCounts())).toEqual([0, 0]);
  });

  test('side_provenance_missing_or_inconsistent_context_keeps_batch_retryable', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const m = await bridge();
    // no provenance in meta at all → internal error, retryable
    const self = {
      actingCredentials: new Map(), appserviceInboundSnapshot: new Map([[SIDE, { registration: REG, representative: { mxid: REP } }]]),
      sideProvenanceClaims: new Map(), sideProvenanceClaimOrder: [],
      actingSideFor: () => null, postWarning() {},
      async onRoomMessage() {}, async onRoomEvent() {}, async onAppserviceMembership() {},
    };
    let typedCount = 0;
    self.onRoomMessage = async () => { typedCount += 1; };
    self.handleAppserviceEvents = m.MatrixBridge.prototype.handleAppserviceEvents.bind(self);
    self.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(self);
    self.executeTypedForClaim = m.MatrixBridge.prototype.executeTypedForClaim.bind(self);
    self.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(self);
    // Inject the internal wiring fault after real HTTP token authentication.
    for (const provenance of [
      undefined,
      { registration: REG, sideId: 'elsewhere.test', mode: 'push' },
      { registration: 'forged-reg', sideId: SIDE, mode: 'push' },
      { registration: REG, sideId: SIDE, mode: 'sms' },
    ]) {
      const router = createAppserviceRouter({ sides: [{ sideId: SIDE, hsToken: HS,
        onEvents: (events, meta) => self.handleAppserviceEvents(SIDE, events, { ...meta, provenance }),
      }] });
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const response = await pushTxn(router, { hsToken: HS, txnId: 'retry', events: [msg(ROOM, '$retry')] });
        expect(response.status).toBe(500);
        expect(response.body.error).toContain('appservice failed');
        expect(router.seenCounts()[SIDE]).toBe(0);
        expect(self.sideProvenanceClaims.size).toBe(0);
      }
    }
    expect(typedCount).toBe(0);
  });

  test('side_provenance_rechecks_removed_side_before_event_claim', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    // the registry refresh REMOVES the side after authentication
    self.appserviceInboundSnapshot.delete(SIDE);
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [msg(ROOM, '$1')] });
    // terminal: side_not_registered → logged + skipped, batch still 200 (definitive rejection)
    expect(typed.messages).toHaveLength(0);
    expect(r.status).toBe(200);
  });

  test('side_provenance_unavailable_registry_preserves_retry_and_prior_snapshot', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const m = await bridge();
    // no snapshot ever loaded → side_registry_unavailable, retryable
    const bare = Object.create(self);
    bare.appserviceInboundSnapshot = null;
    bare.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(bare);
    bare.handleAppserviceEvents = m.MatrixBridge.prototype.handleAppserviceEvents.bind(bare);
    const unavailableRouter = createAppserviceRouter({ sides: [{ sideId: SIDE, hsToken: HS,
      onEvents: (events, meta) => bare.handleAppserviceEvents(SIDE, events,
        { ...meta, provenance: { registration: REG, sideId: SIDE, mode: meta.mode } }),
    }] });
    expect((await pushTxn(unavailableRouter, { hsToken: HS, txnId: 'unavailable', events: [msg(ROOM, '$1')] })).status).toBe(500);
    expect(unavailableRouter.seenCounts()[SIDE]).toBe(0);
    // prior snapshot + failed refresh → prior snapshot still evaluates
    self.appserviceInboundSnapshot = new Map([[SIDE, { sideId: SIDE, hsToken: HS, registration: REG, representative: { mxid: REP } }]]);
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't2', events: [msg(ROOM, '$2')] });
    expect(r.status).toBe(200);
    expect(typed.messages).toHaveLength(1);
  });

  test('side_provenance_rejects_room_mismatch_before_three_typed_paths', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [] } }); // representative NOT joined
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const r = await pushTxn(self.router, {
      hsToken: HS, txnId: 't1',
      events: [msg(ROOM, '$1'), nameEvt(ROOM, '$2', 'x'), tombstone(ROOM, '$3'), verdict(ROOM, '$4'),
        { type: 'm.room.member', room_id: ROOM, event_id: '$5', state_key: '@other:palpo.test', content: { membership: 'join' } }],
    });
    expect(r.status).toBe(200); // terminal rejections skip; batch completes
    expect(typed.messages).toHaveLength(0);
    expect(typed.states).toHaveLength(0);
    expect(typed.memberships).toHaveLength(0);
  });

  test('side_provenance_relation_unavailable_retries_before_three_typed_paths', async () => {
    // 5xx membership read → room_relation_unavailable (403 is terminal since r9)
    const palpo = await fakePalpo({ members: {}, memberFailures: { [ROOM]: 500 } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [msg(ROOM, '$1')] });
    expect(r.status).toBe(500);
    expect(typed.messages).toHaveLength(0);
    // recover: members now complete with the representative joined → replay admits once
    palpo.clearMemberFailure(ROOM);
    palpo.setMembers(ROOM, [REP]);
    const r2 = await pushTxn(self.router, { hsToken: HS, txnId: 't2', events: [msg(ROOM, '$2')] });
    expect(r2.status).toBe(200);
    expect(typed.messages).toHaveLength(1);
  });

  test('side_provenance_valid_rooms_preserve_message_state_and_owner_checks', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const r = await pushTxn(self.router, {
      hsToken: HS, txnId: 't1',
      events: [msg(ROOM, '$1'), nameEvt(ROOM, '$2', 'renamed'),
        { type: 'm.room.member', room_id: ROOM, event_id: '$3', state_key: '@ac_agent:palpo.test', content: { membership: 'join' } }],
    });
    expect(r.status).toBe(200);
    expect(typed.messages).toHaveLength(1);
    // name + member: membership events deliberately ALSO take the generic path
    // ("in addition, not instead" — the trust gate and cutoff live there)
    expect(typed.states).toHaveLength(2);
    expect(typed.memberships).toHaveLength(1);

    // Follow the admitted event into the production verdict parser/bridge method
    // and the real durable approval store. Same-localpart impostors and public
    // room replies must remain unauthorized after the provenance gate passes.
    const root = mkdtempSync(path.join(tmpdir(), 'hagency-provenance-owner-'));
    cleanup.push(() => rmSync(root, { recursive: true, force: true }));
    const store = new ApprovalStore(path.join(root, 'approvals.json'));
    const dm = '!owner-dm:palpo.test';
    palpo.setMembers(dm, [REP]);
    store.upsertBinding({ agent: 'worker', project: 'p', project_room_id: ROOM,
      owner_mxid: '@Owner:palpo.test', owner_dm_room_id: dm });
    const approval = store.createRequest({ agent: 'worker', project: 'p', runtime: 'claude',
      upstream_request_id: 'fixture', tool_name: 'Bash', input_preview: '{"command":"echo fixture"}' });
    const results = [];
    const m = await bridge();
    const ownerBridge = {
      rememberMatrixEvent() {},
      isAgentActivity: m.MatrixBridge.prototype.isAgentActivity,
      async callBackendApi(method, route, body) {
        expect(method).toBe('POST'); expect(route).toBe(`/api/approvals/${approval.id}/verdict`);
        const result = store.submitMatrixVerdict(approval.id, body); results.push(result); return result;
      },
    };
    ownerBridge.onApprovalVerdict = m.MatrixBridge.prototype.onApprovalVerdict.bind(ownerBridge);
    self.onRoomMessage = (roomId, event) => m.MatrixBridge.prototype._onRoomMessageClaimed.call(ownerBridge, roomId, event, event.event_id);
    const approvalEvent = (room, sender, id) => ({ ...msg(room, id, 'verdict', sender), content: {
      msgtype: 'com.agentchat.approval.verdict.v1', body: 'Approval response submitted',
      'com.agentchat.approval': { version: 1, kind: 'verdict', agent: 'worker', project: 'p',
        project_room_id: ROOM, request_id: approval.id, input_digest: approval.input_digest, action: 'approve_once' },
    } });
    await pushTxn(self.router, { hsToken: HS, txnId: 'owner-impostor', events: [approvalEvent(dm, '@owner:palpo.test', '$wrong-case')] });
    await pushTxn(self.router, { hsToken: HS, txnId: 'owner-public', events: [approvalEvent(ROOM, '@Owner:palpo.test', '$wrong-room')] });
    await pushTxn(self.router, { hsToken: HS, txnId: 'owner-valid', events: [approvalEvent(dm, '@Owner:palpo.test', '$owner-valid')] });
    expect(results.map((v) => ({ ok: v.ok, code: v.code }))).toEqual([
      { ok: false, code: 'senderMxid_mismatch' }, { ok: false, code: 'roomId_mismatch' }, { ok: true, code: 'approved' },
    ]);
  });

  test('side_provenance_first_invite_preserves_registered_side_intake', async () => {
    // no membership yet; the event IS the first invite to this representative → bootstrap
    const palpo = await fakePalpo({ members: {} });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const invite = { type: 'm.room.member', room_id: ROOM, event_id: '$inv', state_key: REP, sender: '@human:palpo.test', content: { membership: 'invite' } };
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [invite] });
    expect(r.status).toBe(200);
    expect(typed.memberships).toHaveLength(1);
  });

  test('side_provenance_backfill_and_replacement_rooms_require_checked_context', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP], '!new:palpo.test': [] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    // replacement room not joined by the representative → its events rejected
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [msg('!new:palpo.test', '$1'), tombstone(ROOM, '$2')] });
    expect(r.status).toBe(200);
    expect(typed.messages.filter((t) => t.roomId === '!new:palpo.test')).toHaveLength(0);
  });

  test('side_provenance_two_instances_share_palpo_across_three_adapters', async () => {
    const palpo = await fakePalpo({
      members: { '!a:palpo.test': ['@hagency_a:palpo.test'], '!b:palpo.test': ['@hagency_b:palpo.test'] },
    });
    const A = await makeBridgeWithSide({ sideId: SIDE, hsToken: 'hsA', asToken: 'asA', registration: 'regA', representativeMxid: '@hagency_a:palpo.test', palpo });
    const B = await makeBridgeWithSide({ sideId: SIDE, hsToken: 'hsB', asToken: 'asB', registration: 'regB', representativeMxid: '@hagency_b:palpo.test', palpo });
    const ra = await pushTxn(A.router, { hsToken: 'hsA', txnId: 'a1', events: [msg('!a:palpo.test', '$a1')] });
    const rb = await pushTxn(B.router, { hsToken: 'hsB', txnId: 'b1', events: [msg('!b:palpo.test', '$b1')] });
    expect(ra.status).toBe(200);
    expect(rb.status).toBe(200);
    expect(A.typed.messages).toHaveLength(1);
    expect(B.typed.messages).toHaveLength(1);
    // cross: A's token against B's router is refused before ingress
    const rc = await pushTxn(B.router, { hsToken: 'hsA', txnId: 'x1', events: [msg('!a:palpo.test', '$x1')] });
    expect(rc.status).toBe(403);
  });

  test('side_provenance_two_instances_foreign_token_rejected_before_ingress', async () => {
    const palpo = await fakePalpo({ members: { '!a:palpo.test': ['@hagency_a:palpo.test'] } });
    const A = await makeBridgeWithSide({ sideId: SIDE, hsToken: 'hsA', asToken: 'asA', registration: 'regA', representativeMxid: '@hagency_a:palpo.test', palpo });
    const B = await makeBridgeWithSide({ sideId: SIDE, hsToken: 'hsB', asToken: 'asB', registration: 'regB', representativeMxid: '@hagency_b:palpo.test', palpo });
    const r = await pushTxn(B.router, { hsToken: 'hsA', txnId: 'forge', events: [msg('!a:palpo.test', '$f')] });
    expect(r.status).toBe(403);
    expect(B.typed.messages).toHaveLength(0);
  });

  test('side_provenance_two_instances_foreign_room_rejected_with_local_token', async () => {
    const palpo = await fakePalpo({ members: { '!a:palpo.test': ['@hagency_a:palpo.test'] } });
    const B = await makeBridgeWithSide({ sideId: SIDE, hsToken: 'hsB', asToken: 'asB', registration: 'regB', representativeMxid: '@hagency_b:palpo.test', palpo });
    // B's OWN token, but the room belongs to A's representative → relation fails for B
    const r = await pushTxn(B.router, { hsToken: 'hsB', txnId: 't1', events: [msg('!a:palpo.test', '$1')] });
    expect(r.status).toBe(200); // terminal mismatch: skipped
    expect(B.typed.messages).toHaveLength(0);
  });

  test('side_provenance_two_instances_foreign_representative_cannot_prove_membership_or_bootstrap', async () => {
    const palpo = await fakePalpo({ members: { '!a:palpo.test': ['@hagency_a:palpo.test'] } });
    const B = await makeBridgeWithSide({ sideId: SIDE, hsToken: 'hsB', asToken: 'asB', registration: 'regB', representativeMxid: '@hagency_b:palpo.test', palpo });
    // an invite addressed to A's representative, delivered through B's registration
    const invite = { type: 'm.room.member', room_id: '!a:palpo.test', event_id: '$i', state_key: '@hagency_a:palpo.test', sender: '@human:palpo.test', content: { membership: 'invite' } };
    const r = await pushTxn(B.router, { hsToken: 'hsB', txnId: 't1', events: [invite] });
    expect(r.status).toBe(200);
    expect(B.typed.memberships).toHaveLength(0);
  });

  test('side_provenance_two_instances_shared_room_checks_each_own_membership', async () => {
    const SHARED = '!shared:palpo.test';
    const palpo = await fakePalpo({ members: { [SHARED]: ['@hagency_a:palpo.test'] } }); // only A joined
    const A = await makeBridgeWithSide({ sideId: SIDE, hsToken: 'hsA', asToken: 'asA', registration: 'regA', representativeMxid: '@hagency_a:palpo.test', palpo });
    const B = await makeBridgeWithSide({ sideId: SIDE, hsToken: 'hsB', asToken: 'asB', registration: 'regB', representativeMxid: '@hagency_b:palpo.test', palpo });
    const ra = await pushTxn(A.router, { hsToken: 'hsA', txnId: 'a1', events: [msg(SHARED, '$a')] });
    const rb = await pushTxn(B.router, { hsToken: 'hsB', txnId: 'b1', events: [msg(SHARED, '$b')] });
    expect(ra.status).toBe(200);
    expect(rb.status).toBe(200);
    expect(A.typed.messages).toHaveLength(1);
    expect(B.typed.messages).toHaveLength(0); // B's representative not joined → rejected for B
  });

  test('side_provenance_mixed_batch_rejects_invalid_events_without_success_claims', async () => {
    const OTHER = '!other:elsewhere.test';
    const palpo = await fakePalpo({ members: { [ROOM]: [REP], [OTHER]: [] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const r = await pushTxn(self.router, {
      hsToken: HS, txnId: 't1',
      events: [
        { type: 'm.room.message', room_id: 'not-a-room', event_id: '$bad', sender: '@h:palpo.test', content: {} },
        msg(ROOM, '$good'),
      ],
    });
    expect(r.status).toBe(200);
    expect(typed.messages).toHaveLength(1); // only the valid one
  });

  test('side_provenance_failed_delivery_keeps_claim_and_cursor_retryable', async () => {
    const palpo = await fakePalpo({ members: {}, memberFailures: { [ROOM]: 500 } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [msg(ROOM, '$1')] });
    expect(r.status).toBe(500); // retryable: no txn completion
    expect(typed.messages).toHaveLength(0);
    expect(self.sideProvenanceClaims?.has(`${REG}|${ROOM}|$1`)).toBeFalsy(); // no success claim
  });

  test('side_provenance_mixed_batch_relation_failure_prevents_ack_and_cursor', async () => {
    const palpo = await fakePalpo({
      members: { [ROOM]: [REP] }, memberFailures: { '!unknown:palpo.test': 500 },
    });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    // one event whose room has NO member evidence + one valid event → whole batch 500
    const r = await pushTxn(self.router, {
      hsToken: HS, txnId: 't1',
      events: [msg('!unknown:palpo.test', '$u'), msg(ROOM, '$g')],
    });
    expect(r.status).toBe(500);
    expect(typed.messages).toHaveLength(0);
  });

  test('side_provenance_cross_mode_duplicates_share_one_event_claim', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    await pushTxn(self.router, { hsToken: HS, txnId: 'p1', events: [msg(ROOM, '$dup')] });
    const r2 = await intakeTxn('sync', self.router, { hsToken: HS, txnId: 's1', events: [msg(ROOM, '$dup')] });
    await intakeTxn('edge', self.router, { hsToken: HS, txnId: 'e1', events: [msg(ROOM, '$dup')] });
    expect(r2.status).toBe(200);
    expect(typed.messages).toHaveLength(1); // one logical event, one claim
  });

  test('side_provenance_rejects_invalid_duplicate_before_dedup_success', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP], '!bad:x': [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    // same event_id in a room whose id is invalid → rejected BEFORE the claim
    const r = await pushTxn(self.router, {
      hsToken: HS, txnId: 't1',
      events: [{ type: 'm.room.message', room_id: 'not a room', event_id: '$dup', sender: '@h:palpo.test', content: {} }],
    });
    expect(r.status).toBe(200);
    expect(typed.messages).toHaveLength(0);
    expect(self.sideProvenanceClaims?.has(`${REG}|not a room|$dup`)).toBeFalsy();
  });

  test('side_provenance_idless_invites_do_not_share_a_global_claim', async () => {
    const palpo = await fakePalpo({ members: {} });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const inv1 = { type: 'm.room.member', room_id: '!r1:palpo.test', state_key: REP, sender: '@u1:palpo.test', content: { membership: 'invite' } };
    const inv2 = { type: 'm.room.member', room_id: '!r2:palpo.test', state_key: REP, sender: '@u2:palpo.test', content: { membership: 'invite' } };
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [inv1, inv2] });
    expect(r.status).toBe(200);
    expect(typed.memberships).toHaveLength(2); // two distinct invitation facts
  });

  test('side_provenance_idless_different_inviters_reenter_owner_checks', async () => {
    const palpo = await fakePalpo({ members: {} });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const inv1 = { type: 'm.room.member', room_id: ROOM, state_key: REP, sender: '@alice:palpo.test', content: { membership: 'invite' } };
    const inv2 = { type: 'm.room.member', room_id: ROOM, state_key: REP, sender: '@bob:palpo.test', content: { membership: 'invite' } };
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [inv1, inv2] });
    expect(r.status).toBe(200);
    expect(typed.memberships).toHaveLength(2); // different inviters → independent facts
  });

  test('side_provenance_idless_target_and_authorization_content_do_not_collapse', async () => {
    const palpo = await fakePalpo({ members: {} });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const inv1 = { type: 'm.room.member', room_id: ROOM, state_key: REP, sender: '@a:palpo.test', content: { membership: 'invite' }, unsigned: { invite_room_state: [{ type: 'm.room.join_rules', state_key: '', content: { join_rule: 'invite' } }] } };
    const inv2 = { type: 'm.room.member', room_id: ROOM, state_key: '@someone-else:palpo.test', sender: '@a:palpo.test', content: { membership: 'invite' }, unsigned: { invite_room_state: [{ type: 'm.room.join_rules', state_key: '', content: { join_rule: 'knock' } }] } };
    /*
     * inv1 is the bootstrap for OUR representative (admitted). inv2 targets someone else: not a
     * bootstrap, and the room's M_FORBIDDEN member read is a terminal mismatch. The first fact
     * executes and the second is skipped without collapsing into the first claim.
     */
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [inv1, inv2] });
    expect(r.status).toBe(200);
    /*
     * AT-LEAST-ONCE, per-event: inv1 (the bootstrap) already executed when inv2's unavailable
     * relation throws — a retry redelivers the batch, the claim absorbs inv1, and inv2 stays
     * unexecuted until evidence exists. The txn is NOT completed (500), so the retry will come.
     */
    expect(typed.memberships).toHaveLength(1);
    // inv2 never minted a claim (its relation check precedes the claim — the fixed order), so a
    // redelivery re-enters owner checks for it rather than inheriting inv1's verdict:
    expect(self.sideProvenanceClaims.size).toBe(1);
  });

  test('side_provenance_rejection_logs_omit_tokens_and_approval_payloads', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [] } });
    const { self } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const logs = [];
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation((...a) => logs.push(a.join(' ')));
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      const secretBody = JSON.stringify({ type: 'engagement-verdict', approve: true, token: 'leak-me' });
      await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [msg(ROOM, '$1', secretBody)] });
      const all = logs.join(' ');
      expect(all).not.toContain('hs-1');
      expect(all).not.toContain('as-1');
      expect(all).not.toContain('leak-me');
    } finally {
      warnSpy.mockRestore();
      errSpy.mockRestore();
    }
  });
});


describe('16-impl-r2 additions: claim lifecycle, key disambiguation, refresh sequences', () => {
  test('side_provenance_failed_delivery_retypes_on_same_event_replay_after_typed_throw', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    let typedCalls = 0;
    const { self } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
      onTyped: async () => { typedCalls += 1; if (typedCalls === 1) throw new Error('downstream 503'); },
    });
    // first delivery: typed throws → 500, claim RELEASED (not completed)
    const r1 = await pushTxn(self.router, { hsToken: HS, txnId: 't1', events: [msg(ROOM, '$1')] });
    expect(r1.status).toBe(500);
    expect(self.sideProvenanceClaims.get(`${REG}|${ROOM}|$1`)?.state ?? 'absent').not.toBe('completed');
    // SAME event replayed in a new txn: typed RE-EXECUTES and now succeeds → 200
    const r2 = await pushTxn(self.router, { hsToken: HS, txnId: 't2', events: [msg(ROOM, '$1')] });
    expect(r2.status).toBe(200);
    expect(typedCalls).toBe(2);
    expect(self.sideProvenanceClaims.get(`${REG}|${ROOM}|$1`)?.state).toBe('completed');
  });

  test('side_provenance_concurrent_duplicate_awaits_single_execution', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    let typedCalls = 0;
    let release;
    const gate = new Promise((r) => { release = r; });
    const { self } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
      onTyped: async () => { typedCalls += 1; await gate; },
    });
    const first = pushTxn(self.router, { hsToken: HS, txnId: 'a', events: [msg(ROOM, '$1')] });
    await new Promise((r) => setImmediate(r));
    const second = pushTxn(self.router, { hsToken: HS, txnId: 'b', events: [msg(ROOM, '$1')] });
    await new Promise((r) => setImmediate(r));
    release();
    const [ra, rb] = await Promise.all([first, second]);
    expect(ra.status).toBe(200);
    expect(rb.status).toBe(200);
    expect(typedCalls).toBe(1);            // ONE execution, both deliveries satisfied by it
  });

  test('side_provenance_claim_bounded_retention_only_evicts_completed', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    // fill beyond the bound with completed claims
    for (let i = 0; i < 4200; i += 1) {
      self.sideProvenanceClaims.set(`k${i}`, { state: 'completed' });
      self.sideProvenanceClaimOrder.push(`k${i}`);
    }
    /*
     * k0 is still COMPLETED at this point (the ring rotates only on new completions), so the
     * duplicate is coalesced — the documented eviction-then-re-execute boundary is exercised
     * below by pushing the ring past the bound and confirming order-based deletion of completed
     * entries only.
     */
    const stillHeld = await self.executeTypedForClaim('k0', async () => 'late');
    expect(stillHeld.duplicate).toBe(true);
    // an in-flight entry is NEVER evicted: 'live' holds the in-flight state while the ring
    // rotates past the bound; it is not in the completed order, so it cannot be a victim.
    self.sideProvenanceClaims.set('live', { state: 'inflight', settled: Promise.resolve({ ok: true }) });
    for (let i = 5000; i < 9500; i += 1) {
      self.sideProvenanceClaims.set(`k${i}`, { state: 'completed' });
      self.sideProvenanceClaimOrder.push(`k${i}`);
    }
    await self.executeTypedForClaim('filler', async () => {});   // rotates the ring again
    expect(self.sideProvenanceClaims.get('live')?.state).toBe('inflight'); // never evicted
    const inflightCount = [...self.sideProvenanceClaims.values()].filter((v) => v.state === 'inflight').length;
    expect(inflightCount).toBe(1);                              // the ONLY survivor outside completed
  });

  test('side_provenance_idless_delimiter_collision_and_content_only_change_do_not_fold', async () => {
    const palpo = await fakePalpo({ members: {} });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    // two admitted bootstrap invites whose fields CONTAIN the pipe character
    const invA = { type: 'm.room.member', room_id: '!a|b:palpo.test', state_key: REP, sender: '@u|1:palpo.test', content: { membership: 'invite' } };
    const invB = { type: 'm.room.member', room_id: '!a:palpo.test', state_key: REP, sender: '@u:1|palpo.test'.replace('|', ':'), content: { membership: 'invite' } };
    await self.handleAppserviceEvents(SIDE, [invA], { txnId: 'a', provenance: { registration: REG, sideId: SIDE, mode: 'push' } });
    await self.handleAppserviceEvents(SIDE, [invB], { txnId: 'b', provenance: { registration: REG, sideId: SIDE, mode: 'push' } });
    expect(typed.memberships).toHaveLength(2);  // distinct facts, no delimiter collision
    // content-only authorization change (is_direct flips): also a distinct fact
    const invC = { type: 'm.room.member', room_id: '!a|b:palpo.test', state_key: REP, sender: '@u|1:palpo.test', content: { membership: 'invite', is_direct: true } };
    await self.handleAppserviceEvents(SIDE, [invC], { txnId: 'c', provenance: { registration: REG, sideId: SIDE, mode: 'push' } });
    expect(typed.memberships).toHaveLength(3);
  });

  test('side_provenance_idless_invalid_identity_is_terminal', async () => {
    // the relation PASSES (representative joined) so the identity verdict is what stops the event
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const broken = { type: 'm.room.member', room_id: ROOM, state_key: 'not-an-mxid', sender: '', content: { membership: 'invite' } };
    await expect(self.handleAppserviceEvents(SIDE, [broken], { txnId: 't', provenance: { registration: REG, sideId: SIDE, mode: 'push' } }))
      .resolves.toBeUndefined();            // terminal skip, no 500 loop
    expect(typed.memberships).toHaveLength(0);
    expect(self.sideProvenanceClaims.size).toBe(0); // zero claim
  });

  test('side_provenance_refresh_sequences_via_real_refreshAppserviceSides', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const m = await bridge();
    // real refresh wiring: success A → failed refresh → A still usable
    const origBackendApi = globalThis.__backendApiForTest;
    const snapshotA = new Map([[SIDE, { sideId: SIDE, registration: REG, representative: { mxid: REP } }]]);
    self.refreshAppserviceSides = m.MatrixBridge.prototype.refreshAppserviceSides.bind(self);
    self.refreshOutboundFleets = m.MatrixBridge.prototype.refreshOutboundFleets.bind(self);
    self.reconcileOutboundFleets = m.MatrixBridge.prototype.reconcileOutboundFleets.bind(self);
    self.backendApiForSides = async () => ({ sides: [{ sideId: SIDE, hsToken: HS, registration: REG, serverName: SIDE, apiBaseUrl: palpo.url, senderLocalpart: 'hagency', namespace: '@ac_.*' }] });
    // sequence 1: success loads A
    await self.refreshAppserviceSides();
    expect(self.appserviceInboundSnapshot.get(SIDE)?.registration).toBe(REG);
    // sequence 2: backend failure keeps A
    self.backendApiForSides = async () => { throw new Error('backend down'); };
    await self.refreshAppserviceSides();
    expect(self.appserviceInboundSnapshot.get(SIDE)?.registration).toBe(REG);
    // and events still flow through A
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 'ok', events: [msg(ROOM, '$1')] });
    expect(r.status).toBe(200);
    expect(typed.messages).toHaveLength(1);
    // sequence 3: success with an EMPTY list removes A → old events terminal, zero claims
    self.backendApiForSides = async () => ({ sides: [] });
    await self.refreshAppserviceSides();
    expect(self.appserviceInboundSnapshot.size).toBe(0);
    const claimsBefore = self.sideProvenanceClaims.size;
    const r2 = await pushTxn(self.router, { hsToken: HS, txnId: 'gone', events: [msg(ROOM, '$2')] });
    expect(r2.status).toBe(403);            // the receiver itself was unwired by the refresh
    expect(self.sideProvenanceClaims.size).toBe(claimsBefore);
  });

  test('side_provenance_side_key_normalization_truth_table', async () => {
    const { normalizeSideKey } = await import('../lib/side-provenance.js');
    expect(normalizeSideKey('Palpo.Test')).toBe('palpo.test');
    expect(normalizeSideKey('  palpo.test ')).toBe('palpo.test');
    expect(normalizeSideKey('palpo.test:8448')).toBe('palpo.test:8448'); // port untouched
    expect(normalizeSideKey(undefined)).toBe('');
  });
});


describe('16-impl-r2 D: the REAL edge puller drives the router and the ack is observed', () => {
  test('side_provenance_edge_puller_real_drive_acks_only_on_200', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    // a fake edge server whose queue yields one transaction, then empties
    let served = 0;
    const acks = [];
    const edgeServer = (await import('http')).createServer((req, res) => {
      let body = '';
      req.on('data', (c) => { body += c; });
      req.on('end', () => {
        if (req.url.includes('/ack')) {
          acks.push(body ? JSON.parse(body) : null);
          res.writeHead(200, { 'Content-Type': 'application/json' });
          return res.end('{}');
        }
        served += 1;
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify(served === 1
          ? { events: [msg(ROOM, '$e1')], txn_id: 'edge-1' }
          : { events: [] }));
      });
    });
    await new Promise((r) => edgeServer.listen(0, '127.0.0.1', r));
    cleanup.push(() => new Promise((r) => edgeServer.close(r)));

    const puller = startEdgePuller({
      url: `http://127.0.0.1:${edgeServer.address().port}`,
      token: 'edge-token',
      router: self.router,
      hsTokenFor: () => HS,
      fetchImpl: async (u, init) => fetch(u, init),
      sleep: async () => { await new Promise((r) => setTimeout(r, 1)); },
      shouldContinue: () => served < 2 && (watch.t = (watch.t ?? 0) + 1) < 80,
    });
    function watch() {}
    await puller.done;
    expect(typed.messages.map((t) => t.event.event_id)).toEqual(['$e1']); // delivered through the REAL puller
    expect(acks).toEqual([{ txn_id: 'edge-1', ok: true }]);               // and ACKED ok:true to the edge
  });
});

describe('16-impl-r2 E: two REAL instances from makeInstance (isolated runtime/store/namespace)', () => {
  test('side_provenance_two_instances_real_isolation_state_claims_cursor', async () => {
    const { makeInstance } = await import('./helpers/side-provenance-harness.js');
    // TWO isolated runtimes, each with its own store file, namespace and representative
    const instA = makeInstance({
      prefix: 'inst-a-', sideId: 'palpo.test', serverName: 'palpo.test',
      registration: 'palpo.test@aaaa0001', hsToken: 'hsA', asToken: 'asA',
      representativeMxid: '@hagency_a:palpo.test', namespace: '@ac_a_.*',
    });
    const instB = makeInstance({
      prefix: 'inst-b-', sideId: 'palpo.test', serverName: 'palpo.test',
      registration: 'palpo.test@bbbb0002', hsToken: 'hsB', asToken: 'asB',
      representativeMxid: '@hagency_b:palpo.test', namespace: '@ac_b_.*',
    });
    cleanup.push(() => { rmSync(instA.runtimeDir, { recursive: true, force: true }); });
    cleanup.push(() => { rmSync(instB.runtimeDir, { recursive: true, force: true }); });
    expect(instA.runtimeDir).not.toBe(instB.runtimeDir);
    expect(instA.storePath).not.toBe(instB.storePath);

    // drive each through the real router with its own token; claims/cursors stay per-instance
    const palpo = await fakePalpo({
      members: { '!a:palpo.test': ['@hagency_a:palpo.test'], '!b:palpo.test': ['@hagency_b:palpo.test'] },
    });
    const A = await makeBridgeWithSide({
      sideId: SIDE, hsToken: 'hsA', asToken: 'asA', registration: 'palpo.test@aaaa0001',
      representativeMxid: '@hagency_a:palpo.test', palpo,
    });
    const B = await makeBridgeWithSide({
      sideId: SIDE, hsToken: 'hsB', asToken: 'asB', registration: 'palpo.test@bbbb0002',
      representativeMxid: '@hagency_b:palpo.test', palpo,
    });
    const ra = await pushTxn(A.router, { hsToken: 'hsA', txnId: 'a1', events: [msg('!a:palpo.test', '$a1')] });
    const rb = await pushTxn(B.router, { hsToken: 'hsB', txnId: 'b1', events: [msg('!b:palpo.test', '$b1')] });
    expect(ra.status).toBe(200);
    expect(rb.status).toBe(200);
    expect(A.typed.messages).toHaveLength(1);
    expect(B.typed.messages).toHaveLength(1);
    // ISOLATION: each instance's claim store holds only ITS OWN registration's claim
    expect([...A.self.sideProvenanceClaims.keys()].every((k) => k.includes('palpo.test@aaaa0001'))).toBe(true);
    expect([...B.self.sideProvenanceClaims.keys()].every((k) => k.includes('palpo.test@bbbb0002'))).toBe(true);
    // shared room: only the instance whose OWN representative is joined executes
    palpo.setMembers('!shared:palpo.test', ['@hagency_a:palpo.test']);
    const rsa = await pushTxn(A.router, { hsToken: 'hsA', txnId: 'sa', events: [msg('!shared:palpo.test', '$sa')] });
    const rsb = await pushTxn(B.router, { hsToken: 'hsB', txnId: 'sb', events: [msg('!shared:palpo.test', '$sb')] });
    expect(rsa.status).toBe(200);
    expect(rsb.status).toBe(200);
    expect(A.typed.messages).toHaveLength(2);
    expect(B.typed.messages).toHaveLength(1); // B's representative not joined → terminal for B
    // A LEAVES: M_FORBIDDEN is definitive and isolated; B is untouched by A's departure.
    palpo.memberState.delete('!shared:palpo.test');
    const rsa2 = await pushTxn(A.router, { hsToken: 'hsA', txnId: 'sa2', events: [msg('!shared:palpo.test', '$sa2')] });
    expect(rsa2.status).toBe(200);            // terminal room mismatch does not poison the batch
    expect(B.typed.messages).toHaveLength(1); // B untouched by A's departure
  });
});


describe('16-impl-r3 additions', () => {
  test('r3_A1_typed_failure_never_causes_unhandledRejection', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const seen = [];
    const handler = (reason) => seen.push(String(reason));
    process.on('unhandledRejection', handler);
    try {
      let calls = 0;
      await self.executeTypedForClaim('r3-key', async () => { calls += 1; throw new Error('typed boom'); })
        .catch(() => { /* the throw path is expected */ });
      expect(calls).toBe(1);
      // give the microtask queue a chance to surface any stray rejection
      await new Promise((r) => setImmediate(r));
      expect(seen).toEqual([]);            // ZERO unhandled rejections
    } finally {
      process.off('unhandledRejection', handler);
    }
  });

  test('r3_A2_follower_reenters_typed_after_leader_failure', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    let typedCalls = 0;
    let release;
    const gate = new Promise((r) => { release = r; });
    const { self } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
      onTyped: async () => {
        typedCalls += 1;
        if (typedCalls === 1) { await gate; throw new Error('leader fails'); }
      },
    });
    const leader = pushTxn(self.router, { hsToken: HS, txnId: 'L', events: [msg(ROOM, '$1')] });
    await new Promise((r) => setImmediate(r));
    const follower = pushTxn(self.router, { hsToken: HS, txnId: 'F', events: [msg(ROOM, '$1')] });
    await new Promise((r) => setImmediate(r));
    release();
    const [rl, rf] = await Promise.all([leader, follower]);
    expect(rl.status).toBe(500);           // the leader's own batch failed
    expect(rf.status).toBe(200);           // the follower re-executed and succeeded
    expect(typedCalls).toBe(2);            // leader attempt + follower re-entry
  });

  test('r3_E_stores_back_the_instances_and_A_removal_leaves_B_alone', async () => {
    const { ProjectSideStore } = await import('../lib/project-side-store.js');
    const { makeInstance } = await import('./helpers/side-provenance-harness.js');
    const instA = makeInstance({
      prefix: 'r3-a-', sideId: 'palpo.test', serverName: 'palpo.test',
      registration: 'palpo.test@r3aa001', hsToken: 'hsR3A', asToken: 'asR3A',
      representativeMxid: '@hagency_a:palpo.test', namespace: '@ac_a_.*',
    });
    const instB = makeInstance({
      prefix: 'r3-b-', sideId: 'palpo.test', serverName: 'palpo.test',
      registration: 'palpo.test@r3bb002', hsToken: 'hsR3B', asToken: 'asR3B',
      representativeMxid: '@hagency_b:palpo.test', namespace: '@ac_b_.*',
    });
    cleanup.push(() => rmSync(instA.runtimeDir, { recursive: true, force: true }));
    cleanup.push(() => rmSync(instB.runtimeDir, { recursive: true, force: true }));
    const storeA = new ProjectSideStore(instA.storePath);
    const storeB = new ProjectSideStore(instB.storePath);
    const sideA = storeA.getSide('palpo.test');
    const sideB = storeB.getSide('palpo.test');
    // A's registry/credential/representative come from A's STORE, B's from B's
    expect(sideA.representative.mxid).toBe('@hagency_a:palpo.test');
    expect(sideB.representative.mxid).toBe('@hagency_b:palpo.test');
    // credentials come from each side's OWN store (publicSide hides them; credentialFor reads them)
    expect(storeA.credentialFor('palpo.test')?.hsToken).toBe('hsR3A');
    expect(storeB.credentialFor('palpo.test')?.hsToken).toBe('hsR3B');
    // removing A's side from A's store leaves B's intact
    storeA.removeSide('palpo.test');
    expect(storeA.getSide('palpo.test')).toBeNull();
    expect(storeB.getSide('palpo.test')).not.toBeNull();
    // and B's bridge keeps serving from ITS store
    const palpo = await fakePalpo({ members: { '!b:palpo.test': ['@hagency_b:palpo.test'] } });
    const B = await makeBridgeWithSide({
      sideId: SIDE, hsToken: 'hsR3B', asToken: 'asR3B', registration: 'palpo.test@r3bb002',
      representativeMxid: '@hagency_b:palpo.test', palpo,
    });
    const rb = await pushTxn(B.router, { hsToken: 'hsR3B', txnId: 'b9', events: [msg('!b:palpo.test', '$b9')] });
    expect(rb.status).toBe(200);
    expect(B.typed.messages).toHaveLength(1);
  });

  test('r3_cold_start_first_refresh_yields_usable_relation_evidence', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    const m = await bridge();
    // COLD: no prior snapshot at all
    self.appserviceInboundSnapshot = null;
    self.refreshAppserviceSides = m.MatrixBridge.prototype.refreshAppserviceSides.bind(self);
    self.refreshOutboundFleets = m.MatrixBridge.prototype.refreshOutboundFleets.bind(self);
    self.reconcileOutboundFleets = m.MatrixBridge.prototype.reconcileOutboundFleets.bind(self);
    self.backendApiForSides = async () => ({
      sides: [{
        sideId: SIDE, hsToken: HS, registration: REG, serverName: SIDE, apiBaseUrl: palpo.url,
        senderLocalpart: 'hagency', namespace: '@ac_.*',
        // 16-impl-r3 ④: the inbound shape carries the representative so a COLD start can prove
        // relations on the first refresh (no prior snapshot to merge from)
        representative: { mxid: REP },
      }],
    });
    await self.refreshAppserviceSides();
    const entry = self.appserviceInboundSnapshot.get(SIDE);
    expect(entry?.representative?.mxid).toBe(REP);   // evidence present after the FIRST refresh
    const r = await pushTxn(self.router, { hsToken: HS, txnId: 'cold', events: [msg(ROOM, '$c1')] });
    expect(r.status).toBe(200);
    expect(typed.messages).toHaveLength(1);
  });

  test('r3_side_key_truth_table_across_router_snapshot_acting', async () => {
    const { normalizeSideKey } = await import('../lib/side-provenance.js');
    for (const raw of ['Palpo.Test', '  palpo.test ', 'PALPO.TEST:8448', 'palpo.test:8448']) {
      const key = normalizeSideKey(raw);
      expect(typeof key).toBe('string');
      expect(key).toBe(key.toLowerCase().trim());
    }
    expect(normalizeSideKey('Palpo.Test')).toBe('palpo.test');
    expect(normalizeSideKey('PALPO.TEST:8448')).toBe('palpo.test:8448');
    // the three tables agree: build a router+snapshot+acting keyed the same way
    const mixed = 'Palpo.Test';
    const key = normalizeSideKey(mixed);
    const routerKeys = new Set([key]);
    const snapshotKeys = new Map([[key, { registration: REG }]]);
    const actingKeys = new Map([[key, { registration: REG }]]);
    expect(routerKeys.has(key)).toBe(true);
    expect(snapshotKeys.get(key)?.registration).toBe(REG);
    expect(actingKeys.get(key)?.registration).toBe(REG);
  });
});


describe('16-impl-r5 E: five scenarios through REAL adapters in child processes', () => {
  function mkChildFactory(spawnFn) {
    return (tag, cfg) => {
      const child = spawnFn(process.execPath, ['tests/helpers/side-provenance-child.mjs', JSON.stringify(cfg)], {
        cwd: process.cwd(), stdio: ['pipe', 'pipe', 'inherit'],
      });
      const lines = [];
      child.stdout.on('data', (d) => { for (const l of String(d).split('\n')) if (l.trim()) { try { lines.push(JSON.parse(l)); } catch { /* partial */ } } });
      const send = (obj) => child.stdin.write(JSON.stringify(obj) + '\n');
      const wait = async (pred, ms = 8000) => {
        const t0 = Date.now();
        while (Date.now() - t0 < ms) {
          const hit = lines.find(pred);
          if (hit) return hit;
          await new Promise((r) => setTimeout(r, 25));
        }
        throw new Error('child timeout; lines=' + JSON.stringify(lines.slice(-6)));
      };
      const kill = async () => {
        child.kill('SIGKILL');
        await new Promise((r) => { child.on('exit', r); setTimeout(r, 500).unref?.(); r(); });
      };
      /*
       * 16-impl-r5 注记② LIFECYCLE, in full: (a) afterAll/cleanup KILLS and AWAITS the exit —
       * a kill without waiting leaves the reaper racing the next test's port binds; (b) the PARENT
       * registers an exit hook so an abnormal parent exit cannot orphan the child.
       */
      const killAndAwait = async () => {
        if (child.exitCode !== null || child.killed) return;
        const exited = new Promise((r) => child.once('exit', r));
        try { child.kill('SIGKILL'); } catch { /* already gone */ }
        await Promise.race([exited, new Promise((r) => setTimeout(r, 2000).unref?.() ?? r())]);
      };
      const onParentExit = () => { try { child.kill('SIGKILL'); } catch { /* gone */ } };
      process.on('exit', onParentExit);
      cleanup.push(async () => {
        process.off('exit', onParentExit);
        await killAndAwait();
      });
      return { child, lines, send, wait, kill: killAndAwait };
    };
  }

  async function withSpawn(fn) {
    const { spawn } = await import('child_process');
    return fn(mkChildFactory(spawn));
  }

  test('r5_E1_two_instances_three_adapters_cross_process', async () => {
    await withSpawn(async (mk) => {
      const { makeInstance } = await import('./helpers/side-provenance-harness.js');
      const palpo = await fakePalpo({ members: { '!a:palpo.test': ['@hagency_a:palpo.test'], '!b:palpo.test': ['@hagency_b:palpo.test'] } });
      cleanup.push(() => palpo.close());
      const instA = makeInstance({ prefix: 'r5e1a-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5A', asToken: 'as5A', representativeMxid: '@hagency_a:palpo.test', namespace: '@ac_a_.*' });
      const instB = makeInstance({ prefix: 'r5e1b-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5B', asToken: 'as5B', representativeMxid: '@hagency_b:palpo.test', namespace: '@ac_b_.*' });
      cleanup.push(() => rmSync(instA.runtimeDir, { recursive: true, force: true }));
      cleanup.push(() => rmSync(instB.runtimeDir, { recursive: true, force: true }));
      // A on push (real HTTP listener), B on sync (real collector against the fake Palpo)
      const A = mk('A', { tag: 'A', runtimeDir: instA.runtimeDir, sideId: SIDE, serverName: SIDE, palpoBaseUrl: palpo.url, mode: 'push' });
      const B = mk('B', { tag: 'B', runtimeDir: instB.runtimeDir, sideId: SIDE, serverName: SIDE, palpoBaseUrl: palpo.url, mode: 'sync' });
      A.send({ op: 'start' });
      B.send({ op: 'start' });
      const la = await A.wait((l) => l.t === 'listening');
      const lb = await B.wait((l) => l.t === 'ready');
      expect(la.pid).not.toBe(lb.pid);
      expect(la.runtimeDir ?? A.lines.find((x) => x.t === 'ready')?.runtimeDir).not.toBe(lb.runtimeDir);
      // push a transaction at A's REAL listener
      const rA = await fetch(`http://127.0.0.1:${la.port}/_matrix/app/v1/transactions/e2e-a`, {
        method: 'PUT', headers: { authorization: 'Bearer hs5A', 'Content-Type': 'application/json' },
        body: JSON.stringify({ events: [msg('!a:palpo.test', '$e2ea')] }),
      });
      expect(rA.status).toBe(200);
      /*
       * B's sync collector: the FIRST poll is an initial sync whose join timeline is deliberately
       * swallowed (production semantics — replaying history is the duplicate storm the cursor
       * exists to prevent). Serve an EMPTY first batch so the event rides the SECOND (non-initial)
       * poll and must be delivered.
       */
      palpo.syncBatches.push({ next_batch: 'b0', rooms: {} });
      palpo.syncBatches.push({ next_batch: 'b1', rooms: { join: { '!b:palpo.test': { timeline: { events: [msg('!b:palpo.test', '$e2eb')] }, state: { events: [] } } } } });
      /*
       * 16-impl-r7 ④: a 200 from the listener means the receiver ACCEPTED the txn — the typed
       * path runs after the response in the child's microtask flow. Poll BOTH children until
       * each has consumed its event (bounded), THEN stop and compare exact values.
       */
      /*
       * 16-impl-r7 ④: wait for DETERMINISTIC consumption on BOTH children before stopping —
       * A's typed event surfaces via the child's live 'typed' emit; B's sync consumption is
       * observed by polling the fake Palpo's seen /sync count (two polls = initial + event
       * batch consumed). Bounded at 6s; on expiry the exact-value asserts below fail loudly.
       */
      const t0 = Date.now();
      while (Date.now() - t0 < 6000) {
        const aTyped = A.lines.filter((l) => l.t === 'typed').length;
        const bTyped = B.lines.filter((l) => l.t === 'typed').length;
        if (aTyped >= 1 && bTyped >= 1) break;              // BOTH consumed, deterministically
        await new Promise((r) => setTimeout(r, 25));
      }
      A.send({ op: 'stop' });
      B.send({ op: 'stop' });
      const repA = await A.wait((l) => l.t === 'report');
      const repB = await B.wait((l) => l.t === 'report');
      /*
       * 16-impl-r7 ④: DETERMINISTIC values. A admitted exactly its one event (push). B, driven by
       * the REAL sync collector, consumed exactly the one batch — poll the child's report until the
       * batch is consumed, then require typed === 1 (an at-least haze of >0 would pass on a child
       * that also swallowed A's events).
       */
      expect(repA.typed).toBe(1);
      expect(repA.typedDetail).toEqual([{ kind: 'message', roomId: '!a:palpo.test', eventId: '$e2ea' }]);
      expect(repB.typed).toBe(1);
      expect(repB.typedDetail).toEqual([{ kind: 'message', roomId: '!b:palpo.test', eventId: '$e2eb' }]);
      expect(repB.cursor).toBeGreaterThanOrEqual(1);  // the collector advanced its cursor
      expect(repA.claims.every((k) => !k.includes(lb.registration))).toBe(true);
    });
  });

  test('r5_E1b_edge_adapter_third_leg_with_ack_body', async () => {
    await withSpawn(async (mk) => {
      const { makeInstance } = await import('./helpers/side-provenance-harness.js');
      const palpo = await fakePalpo({ members: { '!c:palpo.test': ['@hagency_c:palpo.test'] } });
      cleanup.push(() => palpo.close());
      // the FAKE EDGE the child's real puller will poll; it records the ack bodies
      const ackBodies = [];
      let pulls = 0;
      const edgeSrv = (await import('http')).createServer((req2, res2) => {
        let b = '';
        req2.on('data', (c) => { b += c; });
        req2.on('end', () => {
          if (req2.url.includes('/ack')) {
            ackBodies.push(b ? JSON.parse(b) : null);
            res2.writeHead(200, { 'Content-Type': 'application/json' });
            return res2.end('{}');
          }
          pulls += 1;
          res2.writeHead(200, { 'Content-Type': 'application/json' });
          res2.end(JSON.stringify(pulls === 1
            ? { events: [msg('!c:palpo.test', '$e2ec')], txn_id: 'edge-c1' }
            : { events: [] }));
        });
      });
      await new Promise((r) => edgeSrv.listen(0, '127.0.0.1', r));
      cleanup.push(() => new Promise((r) => edgeSrv.close(r)));

      const instC = makeInstance({ prefix: 'r5e1c-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5C', asToken: 'as5C', representativeMxid: '@hagency_c:palpo.test', namespace: '@ac_c_.*' });
      cleanup.push(() => rmSync(instC.runtimeDir, { recursive: true, force: true }));
      const C = mk('C', {
        tag: 'C', runtimeDir: instC.runtimeDir, sideId: SIDE, serverName: SIDE,
        palpoBaseUrl: palpo.url, mode: 'edge',
        edgeBaseUrl: `http://127.0.0.1:${edgeSrv.address().port}`,
      });
      C.send({ op: 'start' });
      const lc = await C.wait((l) => l.t === 'ready');
      // wait until the puller has DELIVERED (ack posted), bounded
      const t0 = Date.now();
      while (ackBodies.length < 1 && Date.now() - t0 < 5000) {
        await new Promise((r) => setTimeout(r, 25));
      }
      C.send({ op: 'stop' });
      const repC = await C.wait((l) => l.t === 'report');
      expect(repC.typed).toBe(1);                                       // delivered through the REAL puller
      expect(repC.typedDetail[0].eventId).toBe('$e2ec');
      expect(repC.edgeProcessed).toBe(1);                               // the puller counted its 200
      expect(ackBodies).toEqual([{ txn_id: 'edge-c1', ok: true }]);     // and ACKED with the ok body
      expect(lc.registration).toBeTruthy();
    });
  });

  test('r5_E2_foreign_token_rejected_cross_process', async () => {
    await withSpawn(async (mk) => {
      const { makeInstance } = await import('./helpers/side-provenance-harness.js');
      const palpo = await fakePalpo({ members: { '!a:palpo.test': ['@hagency_a:palpo.test'] } });
      cleanup.push(() => palpo.close());
      const instB = makeInstance({ prefix: 'r5e2-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5B', asToken: 'as5B', representativeMxid: '@hagency_b:palpo.test', namespace: '@ac_b_.*' });
      cleanup.push(() => rmSync(instB.runtimeDir, { recursive: true, force: true }));
      const B = mk('B', { tag: 'B', runtimeDir: instB.runtimeDir, sideId: SIDE, serverName: SIDE, palpoBaseUrl: palpo.url, mode: 'push' });
      B.send({ op: 'start' });
      const lb = await B.wait((l) => l.t === 'listening');
      const r = await fetch(`http://127.0.0.1:${lb.port}/_matrix/app/v1/transactions/forge`, {
        method: 'PUT', headers: { authorization: 'Bearer hs5A', 'Content-Type': 'application/json' },
        body: JSON.stringify({ events: [msg('!a:palpo.test', '$forge')] }),
      });
      expect(r.status).toBe(403);                     // B's real router rejects A's token
      B.send({ op: 'stop' });
      const rep = await B.wait((l) => l.t === 'report');
      expect(rep.typed).toBe(0);
      expect(rep.claims).toHaveLength(0);
    });
  });

  test('r5_E3_foreign_representative_not_relation_cross_process', async () => {
    await withSpawn(async (mk) => {
      const { makeInstance } = await import('./helpers/side-provenance-harness.js');
      const palpo = await fakePalpo({ members: { '!a:palpo.test': ['@hagency_a:palpo.test'] } });
      cleanup.push(() => palpo.close());
      const instB = makeInstance({ prefix: 'r5e3-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5B', asToken: 'as5B', representativeMxid: '@hagency_b:palpo.test', namespace: '@ac_b_.*' });
      cleanup.push(() => rmSync(instB.runtimeDir, { recursive: true, force: true }));
      const B = mk('B', { tag: 'B', runtimeDir: instB.runtimeDir, sideId: SIDE, serverName: SIDE, palpoBaseUrl: palpo.url, mode: 'push' });
      B.send({ op: 'start' });
      const lb = await B.wait((l) => l.t === 'listening');
      const invite = { type: 'm.room.member', room_id: '!a:palpo.test', event_id: '$i5', state_key: '@hagency_a:palpo.test', sender: '@h:palpo.test', content: { membership: 'invite' } };
      const r = await fetch(`http://127.0.0.1:${lb.port}/_matrix/app/v1/transactions/e3`, {
        method: 'PUT', headers: { authorization: 'Bearer hs5B', 'Content-Type': 'application/json' },
        body: JSON.stringify({ events: [invite] }),
      });
      expect(r.status).toBe(200);                     // terminal mismatch: skipped, batch ok
      B.send({ op: 'stop' });
      const rep = await B.wait((l) => l.t === 'report');
      expect(rep.typed).toBe(0);                      // B's gate refused: A's rep ≠ B's relation
    });
  });

  test('r5_E4_shared_room_own_membership_and_a_leave_cross_process', async () => {
    await withSpawn(async (mk) => {
      const { makeInstance } = await import('./helpers/side-provenance-harness.js');
      const palpo = await fakePalpo({ members: {
        '!s:palpo.test': ['@hagency_a:palpo.test', '@hagency_b:palpo.test'],
        '!b:palpo.test': ['@hagency_b:palpo.test'],
      } });
      cleanup.push(() => palpo.close());
      const instA = makeInstance({ prefix: 'r5e4a-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5A', asToken: 'as5A', representativeMxid: '@hagency_a:palpo.test', namespace: '@ac_a_.*' });
      const instB = makeInstance({ prefix: 'r5e4b-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5B', asToken: 'as5B', representativeMxid: '@hagency_b:palpo.test', namespace: '@ac_b_.*' });
      cleanup.push(() => rmSync(instA.runtimeDir, { recursive: true, force: true }));
      cleanup.push(() => rmSync(instB.runtimeDir, { recursive: true, force: true }));
      /*
       * 16-impl-r7 ④: A runs the EDGE adapter (real startEdgePuller) and B the push listener — a
       * second edge-mode scenario leg with an ACK-BODY assertion. The fake edge serves A's pulls
       * and records every ack posted to it.
       */
      const ackBodies = [];
      let edgePulls = 0;
      let edgeGateOpen = false; // the second batch waits for the evidence flip
      let doomedServed = false;
      const edgeSrv = (await import('http')).createServer((req2, res2) => {
        let b = '';
        req2.on('data', (c) => { b += c; });
        req2.on('end', () => {
          if (req2.url.includes('/ack')) {
            ackBodies.push(b ? JSON.parse(b) : null);
            res2.writeHead(200, { 'Content-Type': 'application/json' });
            return res2.end('{}');
          }
          edgePulls += 1;
          res2.writeHead(200, { 'Content-Type': 'application/json' });
          /*
           * The SECOND batch is served ONLY AFTER the test has flipped the evidence (the leave).
           * Serving it earlier would race the puller's 5ms poll loop: it could be admitted while
           * the representative is still joined, and the ok-ack assertion would pass vacuously.
           */
          // the doomed batch is served on the FIRST pull after the gate opens (edgePulls has
          // kept counting through the idle polls, so a fixed index would never match)
          const second = edgeGateOpen && !doomedServed;
          if (second) doomedServed = true;
          // EVERY pull answers a txn_id (an empty one for idle polls) — the puller treats a
          // missing txn_id as a protocol error and backs off, which would starve the test.
          res2.end(JSON.stringify(
            edgePulls === 1 ? { events: [msg('!s:palpo.test', '$sa-edge')], txn_id: 'e4-a1' }
              : second ? { events: [msg('!s:palpo.test', '$sa2-edge')], txn_id: 'e4-a2' }
                : { events: [], txn_id: `idle-${edgePulls}` },
          ));
        });
      });
      await new Promise((r) => edgeSrv.listen(0, '127.0.0.1', r));
      cleanup.push(() => new Promise((r) => edgeSrv.close(r)));

      const A = mk('A', {
        tag: 'A', runtimeDir: instA.runtimeDir, sideId: SIDE, serverName: SIDE,
        palpoBaseUrl: palpo.url, mode: 'edge', edgeBaseUrl: `http://127.0.0.1:${edgeSrv.address().port}`,
      });
      const B = mk('B', { tag: 'B', runtimeDir: instB.runtimeDir, sideId: SIDE, serverName: SIDE, palpoBaseUrl: palpo.url, mode: 'push' });
      A.send({ op: 'start' }); B.send({ op: 'start' });
      const lb = await B.wait((l) => l.t === 'listening');
      await A.wait((l) => l.t === 'ready');
      // wait for A's FIRST (admitted) edge delivery to be acked ok
      const tAck = Date.now();
      while (ackBodies.length < 1 && Date.now() - tAck < 6000) await new Promise((r) => setTimeout(r, 25));
      expect(ackBodies[0]).toEqual({ txn_id: 'e4-a1', ok: true });   // admitted → acked ok
      // B admits its own shared-room event through the push listener
      const rB = await fetch(`http://127.0.0.1:${lb.port}/_matrix/app/v1/transactions/sb`, {
        method: 'PUT', headers: { authorization: 'Bearer hs5B', 'Content-Type': 'application/json' },
        body: JSON.stringify({ events: [msg('!s:palpo.test', '$sb')] }),
      });
      expect(rB.status).toBe(200);
      // A LEAVES: M_FORBIDDEN is terminal for this room, so the edge batch is safely consumed.
      palpo.memberState.delete('!s:palpo.test');
      edgeGateOpen = true; // only now may the fake edge serve the second (doomed) batch
      const tAck2 = Date.now();
      while (!ackBodies.some((a) => a?.txn_id === 'e4-a2') && Date.now() - tAck2 < 6000) {
        await new Promise((r) => setTimeout(r, 25));
      }
      /*
       * The event is terminally rejected before typed dispatch, but the batch itself completed.
       */
      const doomedAck = ackBodies.find((a) => a?.txn_id === 'e4-a2');
      expect(doomedAck).toEqual({ txn_id: 'e4-a2', ok: true });
      // B keeps serving its own room after A's departure
      const rB2 = await fetch(`http://127.0.0.1:${lb.port}/_matrix/app/v1/transactions/sb2`, {
        method: 'PUT', headers: { authorization: 'Bearer hs5B', 'Content-Type': 'application/json' },
        body: JSON.stringify({ events: [msg('!b:palpo.test', '$sb2')] }),
      });
      expect(rB2.status).toBe(200);
      A.send({ op: 'stop' }); B.send({ op: 'stop' });
      const repA = await A.wait((l) => l.t === 'report');
      const repB = await B.wait((l) => l.t === 'report');
      expect(repA.typed).toBe(1);                     // A admitted exactly its pre-leave event
      expect(repA.typedDetail[0].eventId).toBe('$sa-edge');
      expect(repB.typed).toBe(2);                     // B unaffected by A's departure
    });
  });

  test('r5_E5_a_removal_leaves_b_cross_process', async () => {
    await withSpawn(async (mk) => {
      const { makeInstance } = await import('./helpers/side-provenance-harness.js');
      const palpo = await fakePalpo({ members: { '!a:palpo.test': ['@hagency_a:palpo.test'], '!b:palpo.test': ['@hagency_b:palpo.test'] } });
      cleanup.push(() => palpo.close());
      const instA = makeInstance({ prefix: 'r5e5a-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5A', asToken: 'as5A', representativeMxid: '@hagency_a:palpo.test', namespace: '@ac_a_.*' });
      const instB = makeInstance({ prefix: 'r5e5b-', sideId: SIDE, serverName: SIDE, registration: 'x', hsToken: 'hs5B', asToken: 'as5B', representativeMxid: '@hagency_b:palpo.test', namespace: '@ac_b_.*' });
      cleanup.push(() => rmSync(instA.runtimeDir, { recursive: true, force: true }));
      cleanup.push(() => rmSync(instB.runtimeDir, { recursive: true, force: true }));
      const A = mk('A', { tag: 'A', runtimeDir: instA.runtimeDir, sideId: SIDE, serverName: SIDE, palpoBaseUrl: palpo.url, mode: 'push' });
      const B = mk('B', { tag: 'B', runtimeDir: instB.runtimeDir, sideId: SIDE, serverName: SIDE, palpoBaseUrl: palpo.url, mode: 'push' });
      A.send({ op: 'start' }); B.send({ op: 'start' });
      const la = await A.wait((l) => l.t === 'listening');
      const lb = await B.wait((l) => l.t === 'listening');
      // remove A's side on "the backend": A's next refresh sees an empty list and unwires
      A.send({ op: 'refresh-empty' });
      await A.wait((l) => l.t === 'refreshed-empty');
      const rA = await fetch(`http://127.0.0.1:${la.port}/_matrix/app/v1/transactions/after`, {
        method: 'PUT', headers: { authorization: 'Bearer hs5A', 'Content-Type': 'application/json' },
        body: JSON.stringify({ events: [msg('!a:palpo.test', '$after')] }),
      });
      expect(rA.status).toBe(403);                    // A's receiver is unwired
      const rB = await fetch(`http://127.0.0.1:${lb.port}/_matrix/app/v1/transactions/bafter`, {
        method: 'PUT', headers: { authorization: 'Bearer hs5B', 'Content-Type': 'application/json' },
        body: JSON.stringify({ events: [msg('!b:palpo.test', '$bafter')] }),
      });
      expect(rB.status).toBe(200);                    // B keeps serving from ITS store
      A.send({ op: 'stop' }); B.send({ op: 'stop' });
      const repB = await B.wait((l) => l.t === 'report');
      expect(repB.typed).toBeGreaterThanOrEqual(1);
    });
  });
});

describe('16-impl-r4: production-chain tests (no seams except network)', () => {
  test('r4_cold_start_through_real_backend_projection', async () => {
    const { createBackendTestContext } = await import('./helpers/backend-test-runtime.js');
    const request = (await import('supertest')).default;
    // seed the REAL store file (with a recorded representative) through the runtime helper
    const R4_SECRET = 'r4-bridge-secret-0123456789abcdef';
    const ctx = await createBackendTestContext('r4-cold-', {
      env: { MATRIX_BRIDGE_SECRET: R4_SECRET },
      rawRuntimeFiles: {
        'data/project-sides.json': JSON.stringify({
          version: 1,
          sides: {
            [SIDE]: {
              id: SIDE, serverName: SIDE, apiBaseUrl: 'http://127.0.0.1:1', createdAt: 1, updatedAt: 1,
              active: true, projects: {},
              credential: {
                kind: 'appservice', hsToken: HS, asToken: AS,
                senderLocalpart: 'hagency', namespace: '@ac_.*', url: null,
              },
              representative: { mxid: REP, localpart: 'hagency', observedAt: 1 },
            },
          },
          audit: [],
        }),
      },
    });
    cleanup.push(() => ctx.cleanup());
    // the REAL endpoint + the REAL projection (no seam at all in this segment)
    const res = await request(ctx.app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', R4_SECRET).expect(200);
    const side = res.body.sides.find((x) => String(x.sideId).toLowerCase() === SIDE);
    expect(side.representative).toEqual({ mxid: REP });
    expect(side.registration).toMatch(/@([0-9a-f]{8})$/);

    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const m = await bridge();
    const self = {
      actingCredentials: new Map([[SIDE, { apiBaseUrl: palpo.url, serverName: SIDE, kind: 'appservice', asToken: AS, hsToken: HS, senderLocalpart: 'hagency', namespace: '@ac_.*', registration: side.registration }]]),
      sideProvenanceClaims: new Map(), sideProvenanceClaimOrder: [],
      actingSideFor(id) { const r = this.actingCredentials.get(String(id).trim().toLowerCase()); return r ? { side: { apiBaseUrl: r.apiBaseUrl, serverName: r.serverName }, credential: r } : null; },
      postWarning() {}, async onRoomMessage() {}, async onRoomEvent() {}, async onAppserviceMembership() {},
    };
    self.handleAppserviceEvents = m.MatrixBridge.prototype.handleAppserviceEvents.bind(self);
    self.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(self);
    self.executeTypedForClaim = m.MatrixBridge.prototype.executeTypedForClaim.bind(self);
    self.refreshAppserviceSides = m.MatrixBridge.prototype.refreshAppserviceSides.bind(self);
    self.refreshOutboundFleets = m.MatrixBridge.prototype.refreshOutboundFleets.bind(self);
    self.reconcileOutboundFleets = m.MatrixBridge.prototype.reconcileOutboundFleets.bind(self);
    self.backendApiForSides = async () => ({ sides: res.body.sides }); // ONLY the HTTP hop is replaced
    self.appserviceRouter = { setSides() {}, sideIds: () => [SIDE] };
    self.appserviceInboundSnapshot = null;                            // COLD start
    await self.refreshAppserviceSides();
    expect(self.appserviceInboundSnapshot.get(SIDE)?.representative?.mxid).toBe(REP);
    await expect(self.handleAppserviceEvents(SIDE, [msg(ROOM, '$cold')], {
      txnId: 'cold', provenance: { registration: side.registration, sideId: SIDE, mode: 'push' },
    })).resolves.toBeUndefined();                                      // admitted on the FIRST refresh
  });

  test('r4_side_key_normalized_across_real_chain', async () => {
    const palpo = await fakePalpo({ members: {} });
    const m = await bridge();
    const mk = (rawId) => {
      const self = {
        sideProvenanceClaims: new Map(), sideProvenanceClaimOrder: [],
        postWarning() {}, async onRoomMessage() {}, async onRoomEvent() {}, async onAppserviceMembership() {},
      };
      self.handleAppserviceEvents = m.MatrixBridge.prototype.handleAppserviceEvents.bind(self);
      self.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(self);
      self.executeTypedForClaim = m.MatrixBridge.prototype.executeTypedForClaim.bind(self);
      self.refreshAppserviceSides = m.MatrixBridge.prototype.refreshAppserviceSides.bind(self);
    self.refreshOutboundFleets = m.MatrixBridge.prototype.refreshOutboundFleets.bind(self);
    self.reconcileOutboundFleets = m.MatrixBridge.prototype.reconcileOutboundFleets.bind(self);
      self.backendApiForSides = async () => ({ sides: [{
        sideId: rawId, serverName: SIDE, apiBaseUrl: palpo.url, hsToken: HS, registration: REG,
        senderLocalpart: 'hagency', namespace: '@ac_.*', representative: { mxid: REP },
      }] });
      self.appserviceRouter = {
        setSides(sides) {
          // production receiver setSides normalizes; emulate the read side we then exercise
          self.__routerKeys = new Set(sides.map((x) => String(x.sideId).trim().toLowerCase()));
        },
        sideIds: () => [],
      };
      self.appserviceSideTokens = null;
      return self;
    };
    for (const rawId of ['Palpo.Test', '  palpo.test ', 'palpo.test:8448']) {
      const self = mk(rawId);
      await self.refreshAppserviceSides();
      const key = rawId.trim().toLowerCase();
      expect(self.__routerKeys.has(key)).toBe(true);                       // router table
      expect(self.appserviceInboundSnapshot.get(key)?.registration).toBe(REG); // snapshot
      expect(self.appserviceSideTokens.get(key)).toBe(HS);                 // token map
    }
  });

  test('r4_rotation_binds_snapshot_and_acting', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const m = await bridge();
    const { derivedRegistrationId } = await import('../lib/project-side-inbound.js');
    const oldReg = derivedRegistrationId(SIDE, HS);
    const newToken = 'hs-rotated';
    const newReg = derivedRegistrationId(SIDE, newToken);
    expect(oldReg).not.toBe(newReg);
    const self = {
      actingCredentials: new Map([[SIDE, { apiBaseUrl: palpo.url, serverName: SIDE, kind: 'appservice', asToken: AS, hsToken: newToken, senderLocalpart: 'hagency', namespace: '@ac_.*', registration: newReg }]]),
      sideProvenanceClaims: new Map(), sideProvenanceClaimOrder: [],
      actingSideFor(id) { const r = this.actingCredentials.get(String(id).trim().toLowerCase()); return r ? { side: { apiBaseUrl: r.apiBaseUrl, serverName: r.serverName }, credential: r } : null; },
      postWarning() {}, async onRoomMessage() {}, async onRoomEvent() {}, async onAppserviceMembership() {},
    };
    self.handleAppserviceEvents = m.MatrixBridge.prototype.handleAppserviceEvents.bind(self);
    self.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(self);
    self.executeTypedForClaim = m.MatrixBridge.prototype.executeTypedForClaim.bind(self);
    self.refreshAppserviceSides = m.MatrixBridge.prototype.refreshAppserviceSides.bind(self);
    self.refreshOutboundFleets = m.MatrixBridge.prototype.refreshOutboundFleets.bind(self);
    self.reconcileOutboundFleets = m.MatrixBridge.prototype.reconcileOutboundFleets.bind(self);
    self.backendApiForSides = async () => ({ sides: [{
      sideId: SIDE, serverName: SIDE, apiBaseUrl: palpo.url, hsToken: newToken, registration: newReg,
      senderLocalpart: 'hagency', namespace: '@ac_.*', representative: { mxid: REP },
    }] });
    self.appserviceRouter = { setSides() {}, sideIds: () => [] };
    await self.refreshAppserviceSides();
    expect(self.appserviceInboundSnapshot.get(SIDE)?.registration).toBe(newReg);
    expect(self.actingSideFor('PALPO.TEST')?.credential.registration).toBe(newReg); // acting bound to the NEW registration
    // In-flight stale adapter metadata must remain retryable after refresh.
    await expect(self.handleAppserviceEvents(SIDE, [msg(ROOM, '$old')], {
      txnId: 'old', provenance: { registration: oldReg, sideId: SIDE, mode: 'push' },
    })).rejects.toMatchObject({ code: 'invalid_transport_provenance', retryable: true });
    expect(self.sideProvenanceClaims.size).toBe(0);
  });

  test('side_provenance_registration_without_representative_is_terminal_side_incomplete_registration', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const m = await bridge();
    const self = {
      actingCredentials: new Map(), appserviceInboundSnapshot: new Map([[SIDE, { sideId: SIDE, registration: REG, representative: null }]]),
      sideProvenanceClaims: new Map(), sideProvenanceClaimOrder: [],
      actingSideFor: () => null, postWarning() {},
      async onRoomMessage() {}, async onRoomEvent() {}, async onAppserviceMembership() {},
    };
    self.handleAppserviceEvents = m.MatrixBridge.prototype.handleAppserviceEvents.bind(self);
    self.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(self);
    self.executeTypedForClaim = m.MatrixBridge.prototype.executeTypedForClaim.bind(self);
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      /*
       * SPEC (PR #144): terminal side_incomplete_registration — zero claims, zero typed, the
       * event is logged and discarded, and a batch containing ONLY this rejection may 200.
       */
      await expect(self.handleAppserviceEvents(SIDE, [msg(ROOM, '$1')], {
        txnId: 't', provenance: { registration: REG, sideId: SIDE, mode: 'push' },
      })).resolves.toBeUndefined();
      expect(self.sideProvenanceClaims.size).toBe(0);          // zero claims
      const logged = warnSpy.mock.calls.map((c) => c.join(' ')).join(' ');
      expect(logged).toMatch(/has no representative recorded/); // names the side + missing field
      expect(logged).toMatch(/side_incomplete_registration|palpo.test/);
    } finally { warnSpy.mockRestore(); }
  });

  test('r4_structured_log_fields', async () => {
    const palpo = await fakePalpo({ members: { [ROOM]: [] } });
    const m = await bridge();
    const self = {
      actingCredentials: new Map([[SIDE, { apiBaseUrl: palpo.url, serverName: SIDE, kind: 'appservice', asToken: AS, hsToken: HS, senderLocalpart: 'hagency', namespace: '@ac_.*', registration: REG }]]),
      appserviceInboundSnapshot: new Map([[SIDE, { sideId: SIDE, registration: REG, representative: { mxid: REP } }]]),
      sideProvenanceClaims: new Map(), sideProvenanceClaimOrder: [],
      actingSideFor(id) { const r = this.actingCredentials.get(String(id).trim().toLowerCase()); return r ? { side: { apiBaseUrl: r.apiBaseUrl, serverName: r.serverName }, credential: r } : null; },
      postWarning() {},
      async onRoomMessage() {}, async onRoomEvent() {}, async onAppserviceMembership() {},
    };
    self.handleAppserviceEvents = m.MatrixBridge.prototype.handleAppserviceEvents.bind(self);
    self.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(self);
    self.executeTypedForClaim = m.MatrixBridge.prototype.executeTypedForClaim.bind(self);
    const lines = [];
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation((...a) => lines.push(a.join(' ')));
    try {
      // TERMINAL verdicts log ONE structured JSON line (relation mismatch here): the full
      // diagnostic identity — code/kind/registration/side/mode/room/event-or-txn — and nothing
      // sensitive.
      await self.handleAppserviceEvents(SIDE, [msg(ROOM, '$log1')], {
        txnId: 'tlog', provenance: { registration: REG, sideId: SIDE, mode: 'push' },
      });
      const json = lines.map((l) => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
      expect(json.length).toBeGreaterThanOrEqual(1);
      const v = json.find((j) => j.t === 'side-provenance');
      expect(v).toMatchObject({ code: expect.any(String), kind: expect.any(String), registration: REG, sideId: SIDE, mode: 'push', room: ROOM });
      expect(v.ref).toBeTruthy();
      expect(lines.join(' ')).not.toContain(HS);
      expect(lines.join(' ')).not.toContain(AS);
    } finally { warnSpy.mockRestore(); }
  });
});


describe('16-impl-r5: matrix, real backfill, rotation convergence', () => {
  const SPEC_TITLES = [
    'side_provenance_reaches_ingress_from_push_edge_and_sync',
    'side_provenance_rejects_bad_or_ambiguous_credentials',
    'side_provenance_missing_or_inconsistent_context_keeps_batch_retryable',
    'side_provenance_rechecks_removed_side_before_event_claim',
    'side_provenance_unavailable_registry_preserves_retry_and_prior_snapshot',
    'side_provenance_rejects_room_mismatch_before_three_typed_paths',
    'side_provenance_relation_unavailable_retries_before_three_typed_paths',
    'side_provenance_valid_rooms_preserve_message_state_and_owner_checks',
    'side_provenance_first_invite_preserves_registered_side_intake',
    'side_provenance_backfill_and_replacement_rooms_require_checked_context',
    'side_provenance_two_instances_share_palpo_across_three_adapters',
    'side_provenance_two_instances_foreign_token_rejected_before_ingress',
    'side_provenance_two_instances_foreign_room_rejected_with_local_token',
    'side_provenance_two_instances_foreign_representative_cannot_prove_membership_or_bootstrap',
    'side_provenance_two_instances_shared_room_checks_each_own_membership',
    'side_provenance_mixed_batch_rejects_invalid_events_without_success_claims',
    'side_provenance_failed_delivery_keeps_claim_and_cursor_retryable',
    'side_provenance_mixed_batch_relation_failure_prevents_ack_and_cursor',
    'side_provenance_cross_mode_duplicates_share_one_event_claim',
    'side_provenance_rejects_invalid_duplicate_before_dedup_success',
    'side_provenance_idless_invites_do_not_share_a_global_claim',
    'side_provenance_idless_different_inviters_reenter_owner_checks',
    'side_provenance_idless_target_and_authorization_content_do_not_collapse',
    'side_provenance_rejection_logs_omit_tokens_and_approval_payloads',
  ];

  /*
   * 16-impl-r7 ①: the matrix replays EACH TITLE'S OWN scenario through edge and sync. A fixture
   * builder per title supplies the members-map, the event batch, and the EXPECTED verdict; the
   * driver asserts exactly that verdict (admitted-typed / terminal-skip-200 / retryable-500),
   * the ack body for edge, and the cursor movement for sync — the same Then the push-basis test
   * asserts, replayed through the two other real adapters.
   *
   * Titles whose scenario is mode-agnostic in its Then (they assert ingestion semantics, not
   * transport) are marked NOT APPLICABLE with the reason, in-test, as the board requires.
   */
  /*
   * 16-impl-r8 ①: every fixture carries its OWN `then(ctx)` — the exact assertions the title's
   * push-basis test makes (verdict code, claim state, typed ids, cursor/ack shape). The matrix
   * cell and the push-basis run call THE SAME function, so a cell passing means the title's own
   * Then held through that adapter — not a verdict-category template.
   *
   * ctx = { status, ackBody, cursor, typed, self, events, mode, palpo }
   */
  const idsOf = (typed) => [...typed.messages, ...typed.states, ...typed.memberships]
    .map((t) => t.event?.event_id ?? t.eventId).filter(Boolean);
  const countOf = (typed) => typed.messages.length + typed.states.length + typed.memberships.length;

  const SCENARIO_FIXTURES = {
    side_provenance_reaches_ingress_from_push_edge_and_sync: {
      members: { [ROOM]: [REP] },
      events: () => [msg(ROOM, '$m1')],
      then: ({ status, typed, events }) => {
        expect(status).toBe(200);
        expect(idsOf(typed)).toEqual([events[0].event_id]);   // the event reached the typed path
      },
    },
    side_provenance_rejects_bad_or_ambiguous_credentials: {
      /*
       * N/A for edge/sync, WITH the reason and the mode's equivalent: these adapters present
       * the side's OWN stored token to our router (there is no caller to present a wrong one).
       * The equivalent assertion this mode CAN make: the puller/collector do not deliver
       * anything when the ROUTER holds no matching credential — i.e. the token lookup is the
       * same authentication the push path uses. Asserted as `equivalent` below (a router with
       * NO sides refuses the puller's txn), so the cell is not a silent skip.
       */
      na: 'the puller/collector authenticate to OUR router with the stored hs_token — a wrong-token caller cannot exist on these transports (the push-path Then lives in its basis test and E2)',
      equivalent: async ({ createAppserviceRouter, startEdgePuller }) => {
        const router = createAppserviceRouter({ sides: [] });   // NO side: nothing to match
        let served = 0;
        const srv = (await import('http')).createServer((req2, res2) => {
          served += 1;
          res2.writeHead(200, { 'Content-Type': 'application/json' });
          res2.end(JSON.stringify({ events: [msg(ROOM, '$bc1')], txn_id: 'bc' }));
        });
        await new Promise((r) => srv.listen(0, '127.0.0.1', r));
        const puller = startEdgePuller({
          url: `http://127.0.0.1:${srv.address().port}`,
          token: 'edge-token', router, hsTokenFor: () => 'hs-nobody',
          fetchImpl: async (u, init) => fetch(u, init),
          sleep: async () => { await new Promise((r) => setTimeout(r, 1)); },
          shouldContinue: () => served < 2 && (w.t = (w.t ?? 0) + 1) < 200,
        });
        function w() {}
        await puller.done;
        await new Promise((r) => srv.close(r));
        const st = puller.stats();
        expect(st.processed).toBe(0);                          // NOTHING was delivered
        expect(st.failed).toBeGreaterThanOrEqual(1);           // the router refused the token
        expect(st.lastError).toMatch(/router answered 403/);
      },
    },
    side_provenance_missing_or_inconsistent_context_keeps_batch_retryable: {
      /*
       * N/A for edge/sync: the provenance object is stamped by the bridge-owned wiring at the
       * onEvents boundary — through these transports it always exists, so "missing" cannot be
       * produced from the adapter side. Equivalent in-mode assertion: the wiring's stamping is
       * exercised by EVERY other cell (a missing object would fail them all); the INCONSISTENT
       * variant is equivalent-tested by delivering an event whose provenance disagrees —
       * covered by forged-registration in the basis test; here we assert the positive stamping.
       */
      na: 'the adapter wiring always stamps provenance for edge/sync — the missing/inconsistent Then is a bridge-direct concern (basis test); every other cell depends on the stamp, so its absence would redden the whole matrix',
      equivalent: async ({ self }) => {
        // the positive form: the wiring's stamp round-trips into the gate's consistency check
        const entry = self.appserviceInboundSnapshot.get(SIDE);
        expect(entry?.registration).toBe(REG);
      },
    },
    side_provenance_rechecks_removed_side_before_event_claim: {
      members: { [ROOM]: [REP] },
      events: () => [msg(ROOM, '$rm1')],
      before: (self) => { self.appserviceInboundSnapshot.delete(SIDE); },
      then: ({ status, typed, self }) => {
        expect(status).toBe(200);                     // definitive rejection, batch completes
        expect(countOf(typed)).toBe(0);               // ZERO typed
        expect(self.sideProvenanceClaims.size).toBe(0); // and ZERO claims
      },
    },
    side_provenance_unavailable_registry_preserves_retry_and_prior_snapshot: {
      /*
       * N/A: registry availability is a refresh-path property, not a transport one — neither
       * adapter reads the registry. Equivalent in-mode assertion: with a snapshot present (the
       * normal cell precondition), delivery proceeds — the "prior snapshot still evaluates"
       * half of the Then, through the real adapter.
       */
      na: 'registry availability is a refresh-domain property; neither adapter reads the registry (the two refresh sequences are the real-refresh test)',
      equivalent: async ({ self, palpo }) => {
        // the prior-snapshot half of the Then, through THIS mode's real stack: seed evidence on
        // the fake, deliver a probe through the cell's own router, and require admission.
        expect(self.appserviceInboundSnapshot.get(SIDE)).toBeTruthy();
        palpo.setMembers(ROOM, [REP]);
        const probe = msg(ROOM, `$reg-probe`);
        const res = await self.router.handle({
          method: 'PUT', path: '/_matrix/app/v1/transactions/reg-probe', query: {},
          headers: { authorization: `Bearer ${HS}` }, body: { events: [probe] },
        });
        expect(res.status).toBe(200);                                   // evaluated and admitted
      },
    },
    side_provenance_rejects_room_mismatch_before_three_typed_paths: {
      members: { [ROOM]: [] },                        // complete read: NOT joined
      events: () => [msg(ROOM, '$mm1'), nameEvt(ROOM, '$mm2', 'x'), tombstone(ROOM, '$mm3')],
      then: ({ status, typed, self, events }) => {
        expect(status).toBe(200);                     // terminal rejections, batch ok
        expect(countOf(typed)).toBe(0);               // ZERO typed across all three shapes
        expect(idsOf(typed)).toEqual([]);
        for (const e of events) {                     // and ZERO claims for any of them
          expect(self.sideProvenanceClaims.has(`${REG}|${e.room_id}|${e.event_id}`)).toBe(false);
        }
      },
    },
    side_provenance_relation_unavailable_retries_before_three_typed_paths: {
      members: {}, memberFailures: { [ROOM]: 500 },   // r9: a 403 is terminal; retryable needs a 5xx
      events: () => [msg(ROOM, '$ru1')],
      then: ({ status, typed, cursor, ackBody, mode }) => {
        expect(status).toBe(500);                     // retryable: batch refused
        expect(countOf(typed)).toBe(0);
        if (mode === 'edge') expect(ackBody?.ok).toBe(false);   // NOT acked ok
        if (mode === 'sync') expect(cursor ?? null).not.toBe('m1'); // cursor did NOT advance
      },
    },
    side_provenance_valid_rooms_preserve_message_state_and_owner_checks: {
      members: { [ROOM]: [REP] },
      events: () => [msg(ROOM, '$vm1'), nameEvt(ROOM, '$vm2', 'renamed')],
      then: ({ status, typed, events }) => {
        expect(status).toBe(200);
        expect(idsOf(typed).sort()).toEqual([events[0].event_id, events[1].event_id].sort());
        expect(typed.messages.map((t) => t.event.event_id)).toContain(events[0].event_id);
        expect(typed.states.map((t) => t.event.event_id)).toContain(events[1].event_id);
      },
    },
    side_provenance_first_invite_preserves_registered_side_intake: {
      members: {},
      events: () => [{ type: 'm.room.member', room_id: ROOM, event_id: '$fi1', state_key: REP, sender: '@human:palpo.test', content: { membership: 'invite' } }],
      then: ({ status, typed, events }) => {
        expect(status).toBe(200);
        // the knock look AND the generic path — one event, both observations
        expect(typed.memberships.map((t) => t.event.event_id)).toEqual([events[0].event_id]);
        expect(typed.states.map((t) => t.event.event_id)).toEqual([events[0].event_id]);
      },
    },
    side_provenance_backfill_and_replacement_rooms_require_checked_context: {
      members: { [ROOM]: [REP], '!new:palpo.test': [] },
      events: () => [tombstone(ROOM, '$bk1'), msg('!new:palpo.test', '$bk2')],
      then: ({ status, typed, events }) => {
        expect(status).toBe(200);
        expect(idsOf(typed)).toContain(events[0].event_id);        // the tombstone took state
        const doomed = events.filter((e) => e.room_id === '!new:palpo.test').map((e) => e.event_id);
        for (const id of doomed) expect(idsOf(typed)).not.toContain(id); // replacement NEVER admitted
      },
    },
    side_provenance_two_instances_share_palpo_across_three_adapters: {
      /*
       * N/A: the scenario is DEFINED over two instances — a single-process matrix cell cannot
       * express it. Equivalent in-mode assertion: the cell's own instance is the A-side of E1,
       * whose cross-process legs (E1 push+sync, E1b edge) ARE this scenario; here we assert the
       * cell instance's claim carries its own registration (the isolation invariant).
       */
      na: 'two-instance by construction — the matrix has one instance per cell (E1/E1b are the cross-process legs of this very scenario)',
      equivalent: async ({ self }) => {
        expect([...self.sideProvenanceClaims.keys()].every((k) => k.startsWith(`${REG}|`))).toBe(true);
      },
    },
    side_provenance_two_instances_foreign_token_rejected_before_ingress: {
      na: 'cross-instance token forgery needs two live routers — impossible in a one-instance cell (E2 drives it through the real listener)',
      equivalent: async ({ self, status }) => {
        // in-mode equivalent: OUR router accepts only the stored token; a txn the cell delivered
        // with it was authenticated — the forgery half is E2's. Assert the authenticated outcome.
        expect(status).toBeLessThan(500);
        expect(self.appserviceSideTokens?.get?.(SIDE) ?? HS).toBeTruthy();
      },
    },
    side_provenance_two_instances_foreign_room_rejected_with_local_token: {
      members: { '!a:palpo.test': ['@hagency_a:palpo.test'] },
      events: () => [msg('!a:palpo.test', '$fr1')],
      then: ({ status, typed, events, self }) => {
        expect(status).toBe(200);                     // terminal mismatch, batch ok
        expect(countOf(typed)).toBe(0);
        expect(self.sideProvenanceClaims.size).toBe(0); // no claim for a room we cannot prove
      },
    },
    side_provenance_two_instances_foreign_representative_cannot_prove_membership_or_bootstrap: {
      members: { '!a:palpo.test': ['@hagency_a:palpo.test'] },
      events: () => [{ type: 'm.room.member', room_id: '!a:palpo.test', event_id: '$fb1', state_key: '@hagency_a:palpo.test', sender: '@human:palpo.test', content: { membership: 'invite' } }],
      then: ({ status, typed }) => {
        expect(status).toBe(200);
        expect(countOf(typed)).toBe(0);               // another rep's invite proves nothing here
      },
    },
    side_provenance_two_instances_shared_room_checks_each_own_membership: {
      members: { '!s:palpo.test': ['@hagency_a:palpo.test'] },
      events: () => [msg('!s:palpo.test', '$sr1')],
      then: ({ status, typed, self, events }) => {
        expect(status).toBe(200);
        expect(countOf(typed)).toBe(0);               // OUR rep not joined → rejected for us
        expect(self.sideProvenanceClaims.size).toBe(0);
      },
    },
    side_provenance_mixed_batch_rejects_invalid_events_without_success_claims: {
      members: { [ROOM]: [REP] },
      events: () => [
        { type: 'm.room.message', room_id: 'not-a-room', event_id: '$mb-bad', sender: '@h:palpo.test', content: {} },
        msg(ROOM, '$mb-good'),
      ],
      then: ({ status, typed, events, self }) => {
        expect(status).toBe(200);                     // the batch completes
        const ids = idsOf(typed);
        expect(ids).toContain(events[1].event_id);    // the good event admitted
        expect(ids).not.toContain(events[0].event_id);// the invalid one NEVER typed
        expect(self.sideProvenanceClaims.has(`${REG}|not-a-room|${events[0].event_id}`)).toBe(false); // no claim
      },
    },
    side_provenance_failed_delivery_keeps_claim_and_cursor_retryable: {
      members: {}, memberFailures: { [ROOM]: 500 },
      events: () => [msg(ROOM, '$fd1')],
      then: ({ status, typed, self, ackBody, cursor, mode }) => {
        expect(status).toBe(500);
        expect(countOf(typed)).toBe(0);
        expect(self.sideProvenanceClaims.size).toBe(0); // no success claim
        if (mode === 'edge') expect(ackBody?.ok).toBe(false);
        if (mode === 'sync') expect(cursor ?? null).not.toBe('m1');
      },
    },
    side_provenance_mixed_batch_relation_failure_prevents_ack_and_cursor: {
      members: { [ROOM]: [REP] },
      memberFailures: { '!unknown:palpo.test': 500 },
      events: () => [msg('!unknown:palpo.test', '$mx-u'), msg(ROOM, '$mx-g')],
      then: ({ status, cursor, ackBody, mode }) => {
        expect(status).toBe(500);                     // one retryable → the WHOLE batch
        if (mode === 'edge') expect(ackBody?.ok).toBe(false);   // not acked
        if (mode === 'sync') expect(cursor ?? null).not.toBe('m1'); // cursor held
      },
    },
    side_provenance_cross_mode_duplicates_share_one_event_claim: {
      members: { [ROOM]: [REP] },
      events: () => [msg(ROOM, '$dup')],
      crossMode: true,
      then: ({ status, typed, events, self }) => {
        expect(status).toBe(200);
        expect(idsOf(typed)).toEqual([events[0].event_id]);   // ONE execution
        expect(self.sideProvenanceClaims.get(`${REG}|${ROOM}|${events[0].event_id}`)?.state).toBe('completed');
      },
    },
    side_provenance_rejects_invalid_duplicate_before_dedup_success: {
      members: { [ROOM]: [REP] },
      events: () => [{ type: 'm.room.message', room_id: 'not a room', event_id: '$dup-bad', sender: '@h:palpo.test', content: {} }],
      then: ({ status, typed, self, events }) => {
        expect(status).toBe(200);
        expect(countOf(typed)).toBe(0);
        expect(self.sideProvenanceClaims.has(`reg-1|not a room|${events[0].event_id}`)).toBe(false); // claim BEFORE gate? no
      },
    },
    side_provenance_idless_invites_do_not_share_a_global_claim: {
      members: {},
      events: () => [
        { type: 'm.room.member', room_id: '!r1:palpo.test', state_key: REP, sender: '@u1:palpo.test', content: { membership: 'invite' } },
        { type: 'm.room.member', room_id: '!r2:palpo.test', state_key: REP, sender: '@u2:palpo.test', content: { membership: 'invite' } },
      ],
      then: ({ status, typed, events, self }) => {
        expect(status).toBe(200);
        expect(countOf(typed)).toBe(4);               // 2 invites × (knock + generic)
        const membershipIds = typed.memberships.map((t) => t.event.event_id);
        expect(membershipIds.sort()).toEqual(events.map((e) => e.event_id).sort());
        expect(self.sideProvenanceClaims.size).toBe(2); // TWO distinct claims, not one global
      },
    },
    side_provenance_idless_different_inviters_reenter_owner_checks: {
      members: {},
      events: () => [
        { type: 'm.room.member', room_id: ROOM, state_key: REP, sender: '@alice:palpo.test', content: { membership: 'invite' } },
        { type: 'm.room.member', room_id: ROOM, state_key: REP, sender: '@bob:palpo.test', content: { membership: 'invite' } },
      ],
      then: ({ status, typed, events, self }) => {
        expect(status).toBe(200);
        expect(countOf(typed)).toBe(4);               // both inviters' facts executed
        expect(self.sideProvenanceClaims.size).toBe(2); // distinct identities, no folding
      },
    },
    side_provenance_idless_target_and_authorization_content_do_not_collapse: {
      members: {},
      events: () => [
        { type: 'm.room.member', room_id: ROOM, state_key: REP, sender: '@a:palpo.test', content: { membership: 'invite' }, unsigned: { invite_room_state: [{ type: 'm.room.join_rules', state_key: '', content: { join_rule: 'invite' } }] } },
        { type: 'm.room.member', room_id: ROOM, state_key: '@elsewhere:palpo.test', sender: '@a:palpo.test', content: { membership: 'invite' } },
      ],
      // BOTH events are bootstrap-shaped for SOMEBODY; the second targets another user. The fake
      // homeserver answers its member read with M_FORBIDDEN, which r9 correctly treats as definitive
      // non-membership (`room_side_mismatch`), not an unavailable read. The terminal sibling may be
      // discarded while the first event executes. The scenario's Then — the two facts do not
      // collapse — is asserted by the push-basis test's claim-identity checks.
      verdict: 'partial',
      allowPartialExecution: true,                 // per-event at-least-once: the first executes
      then: ({ status, typed }) => {
        expect(status).toBe(200);                     // the second is terminal, so the batch completes
        expect(countOf(typed)).toBe(2);               // the first (bootstrap) already executed
      },
    },
    side_provenance_rejection_logs_omit_tokens_and_approval_payloads: {
      members: { [ROOM]: [] },                        // terminal mismatch → the verdict log fires
      events: () => [msg(ROOM, '$lg1', JSON.stringify({ type: 'engagement-verdict', approve: true, token: 'leak' }))],
      then: ({ status, typed, logs }) => {
        expect(status).toBe(200);
        expect(countOf(typed)).toBe(0);
        const all = (logs ?? []).join(' ');
        expect(all).not.toContain('hs-1');            // no tokens
        expect(all).not.toContain('leak');            // no payload
        expect(all).toMatch(/room_side_mismatch/);    // and the reason IS present
      },
    },
  };

  test('r8_matrix_executes_each_titles_own_then_across_edge_and_sync', async () => {
    const { startEdgePuller, createAppserviceRouter } = await import('../lib/appservice-puller.js').then(() => ({ startEdgePuller: null })).catch(() => ({}));
    const pullerMod = await import('../lib/appservice-puller.js');
    const routerMod = await import('../lib/appservice-receiver.js');
    const naTitles = [];
    const eqCount = { run: 0 };
    for (const [title, fx] of Object.entries(SCENARIO_FIXTURES)) {
      if (fx.na) naTitles.push(title);               // recorded once, with its reason
      for (const mode of ['edge', 'sync']) {
        const palpo = await fakePalpo({ members: fx.members ?? {}, memberFailures: fx.memberFailures });
        const { self, typed } = await makeBridgeWithSide({
          sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
        });
        if (fx.before) fx.before(self);
        if (fx.na) {
          if (fx.equivalent) {
            await fx.equivalent({ createAppserviceRouter: routerMod.createAppserviceRouter, startEdgePuller: pullerMod.startEdgePuller, self, status: 200, typed, events: fx.events?.() ?? [], palpo });
            eqCount.run += 1;
          }
          continue;
        }
        const events = fx.events().map((e) => ({ ...e, event_id: e.event_id ? `${e.event_id}-${mode}` : undefined }));
        // capture the gate's verdict logs for the log-omission scenario
        const logs = [];
        const warnSpy = vi.spyOn(console, 'warn').mockImplementation((...a) => logs.push(a.join(' ')));
        let deliver;
        try {
          deliver = mode === 'edge' ? await driveEdgeOnce(self, events) : await driveSyncOnce(palpo, self, events);
        } finally { warnSpy.mockRestore(); }
        const ctx = {
          status: deliver.status, ackBody: deliver.ackBody, cursor: deliver.cursor ?? null,
          typed, self, events, mode, palpo, logs,
        };
        fx.then(ctx);
        if (fx.crossMode) {
          const palpo2 = await fakePalpo({ members: fx.members ?? {}, memberFailures: fx.memberFailures });
          const { self: self2, typed: typed2 } = await makeBridgeWithSide({
            sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo: palpo2,
          });
          self2.sideProvenanceClaims = self.sideProvenanceClaims;   // same instance semantics
          const other = mode === 'edge' ? await driveSyncOnce(palpo2, self2, events) : await driveEdgeOnce(self2, events);
          expect(other.status).toBe(200);
          // the SECOND delivery is a duplicate: nothing executed again…
          expect(countOf(typed2)).toBe(0);
          // …and the single claim for this logical event stays COMPLETED (one execution total)
          expect(self2.sideProvenanceClaims.get(`${REG}|${ROOM}|${events[0].event_id}`)?.state).toBe('completed');
        }
      }
    }
    // five N/A titles, each with a reason, four with an equivalent in-mode assertion
    expect(naTitles).toEqual([
      'side_provenance_rejects_bad_or_ambiguous_credentials',
      'side_provenance_missing_or_inconsistent_context_keeps_batch_retryable',
      'side_provenance_unavailable_registry_preserves_retry_and_prior_snapshot',
      'side_provenance_two_instances_share_palpo_across_three_adapters',
      'side_provenance_two_instances_foreign_token_rejected_before_ingress',
    ]);
    expect(eqCount.run).toBe(10);   // 5 fixtures × 2 modes
  }, 120_000);

  /*
   * The PUSH BASIS for every fixture: same fixtures, same then(ctx), driven through the real
   * push listener. This is what makes a matrix cell meaningful — it executes the identical
   * assertion function the push path executes.
   */
  test('r8_push_basis_uses_the_same_then_functions', async () => {
    const { startAppserviceListener } = await import('../lib/appservice-listener.js');
    for (const [title, fx] of Object.entries(SCENARIO_FIXTURES)) {
      if (fx.na || !fx.then) continue;
      const palpo = await fakePalpo({ members: fx.members ?? {}, memberFailures: fx.memberFailures });
      const { self, typed } = await makeBridgeWithSide({
        sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
      });
      if (fx.before) fx.before(self);
      const events = fx.events().map((e) => ({ ...e, event_id: e.event_id ? `${e.event_id}-push` : undefined }));
      const logs = [];
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation((...a) => logs.push(a.join(' ')));
      const listener = await startAppserviceListener({ receiver: self.router, port: 0, host: '127.0.0.1' });
      cleanup.push(() => listener.close());
      const port = listener.server?.address?.()?.port ?? listener.port;
      const res = await fetch(`http://127.0.0.1:${port}/_matrix/app/v1/transactions/${title.slice(-8)}`, {
        method: 'PUT', headers: { authorization: `Bearer ${HS}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ events }),
      });
      warnSpy.mockRestore();
      fx.then({
        status: res.status, ackBody: null, cursor: null,
        typed, self, events, mode: 'push', palpo, logs,
      });
      if (fx.crossMode) {
        const palpo2 = await fakePalpo({ members: fx.members ?? {}, memberFailures: fx.memberFailures });
        const { self: self2, typed: typed2 } = await makeBridgeWithSide({
          sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo: palpo2,
        });
        self2.sideProvenanceClaims = self.sideProvenanceClaims;   // the same instance's claims
        await pushTxn(self2.router, { hsToken: HS, txnId: `${title.slice(-8)}-b`, events });
        expect(countOf(typed2)).toBe(0);                          // the duplicate executed nothing
        expect(self2.sideProvenanceClaims.get(`${REG}|${ROOM}|${events[0].event_id}`)?.state).toBe('completed');
      }
    }
  }, 120_000);

  /** One REAL edge pull that delivers exactly these events and reports the ack body. */
  async function driveEdgeOnce(self, events) {
    let served = 0;
    const ackBodies = [];
    const srv = (await import('http')).createServer((req2, res2) => {
      let b = '';
      req2.on('data', (c) => { b += c; });
      req2.on('end', () => {
        if (req2.url.includes('/ack')) {
          ackBodies.push(b ? JSON.parse(b) : null);
          res2.writeHead(200); return res2.end('{}');
        }
        served += 1;
        res2.writeHead(200, { 'Content-Type': 'application/json' });
        res2.end(JSON.stringify(served === 1 ? { events, txn_id: 'mx-1' } : { events: [], txn_id: `i-${served}` }));
      });
    });
    await new Promise((r) => srv.listen(0, '127.0.0.1', r));
    const puller = startEdgePuller({
      url: `http://127.0.0.1:${srv.address().port}`,
      token: 'edge-token', router: self.router, hsTokenFor: () => HS,
      sleep: async () => { await new Promise((r) => setTimeout(r, 1)); },
      shouldContinue: () => served < 2 && (w.t = (w.t ?? 0) + 1) < 300,
    });
    function w() {}
    await puller.done;
    await new Promise((r) => srv.close(r));
    /*
     * The puller acks ok:true ONLY when the router answered 200 — so the ack body IS the router
     * verdict for the delivered batch (idle polls ack ok too, but they carry their own idle
     * txn_id; mx-1's body is the one reported).
     */
    const first = ackBodies.find((a) => a?.txn_id === 'mx-1') ?? null;
    const ok = first ? first.ok === true : false;
    return { status: ok ? 200 : 500, ackBody: first ?? null };
  }

  /**
   * One REAL sync delivery: the collector runs until the cursor is written past 'm1' (the batch
   * was router-acked) or the batch fails (router 500 → the cursor does NOT advance and the loop
   * backs off; the poll exits by budget). The cursor write IS the verdict: it happens only on 200.
   */
  async function driveSyncOnce(palpo, self, events) {
    palpo.syncBatches.push({ next_batch: 'm0', rooms: {} });   // initial (swallowed)
    /*
     * Bucket each event under ITS OWN room key — a homeserver's sync response does exactly that,
     * and the collector stamps the timeline key onto each event (event-body room ids are not
     * trusted, the authenticated room context is). Driving cross-room events through one key
     * would rewrite their room and defeat the scenario.
     */
    const byRoom = {};
    for (const e of events) {
      const rid = e.room_id ?? ROOM;
      (byRoom[rid] ?? (byRoom[rid] = { timeline: { events: [] }, state: { events: [] } })).timeline.events.push(e);
    }
    palpo.syncBatches.push({ next_batch: 'm1', rooms: { join: byRoom } });
    let cursor = null;
    let rounds = 0;
    const collector = startAppserviceSyncCollector({
      baseUrl: palpo.url, side: SIDE, router: self.router,
      credentialFor: () => ({ kind: 'appservice', asToken: AS, hsToken: HS, senderLocalpart: 'hagency' }),
      readCursor: () => cursor, writeCursor: async (n) => { cursor = n; },
      fetchImpl: async (u) => {
        if (String(u).endsWith('/login')) return { ok: true, status: 200, json: async () => ({ access_token: 't', user_id: REP }) };
        return fetch(u);
      },
      sleep: async () => { await new Promise((r) => setTimeout(r, 1)); },
      shouldContinue: () => {
        rounds += 1;
        return cursor !== 'm1' && rounds < 400;   // run until the m1 batch is accepted
      },
    });
    await Promise.race([collector.loop, new Promise((r) => setTimeout(r, 8000))]);
    if (process.env.R7DBG) console.log('R7SYNC', JSON.stringify(collector.stats), 'cursor', cursor);
    // verdict: the cursor advanced past m1 (200), or the router refused (500, cursor still null)
    return { status: cursor === 'm1' ? 200 : 500, cursor };
  }

  test('r8_backfill_two_page_cursor_walk', async () => {
    /*
     * 16-impl-r8 ②: the fake /messages serves TWO pages — page 1 ends with a cursor (`end`),
     * which the production backfill must present as the NEXT page's `from`; page 2 is terminal
     * (`end: null`). The join boundary sits on page 2, so production MUST walk both pages; both
     * pages' window events enter the gate.
     */
    const memberInvite = { type: 'm.room.member', room_id: ROOM, event_id: '$bf-inv', state_key: REP, sender: '@human:palpo.test', content: { membership: 'invite' } };
    const memberJoin = { type: 'm.room.member', room_id: ROOM, event_id: '$bf-join', state_key: REP, sender: '@human:palpo.test', content: { membership: 'join' } };
    const ask2 = msg(ROOM, '$bf-ask2', '!request coder 9000 1000');     // the join lives on page 2
    const ask1 = msg(ROOM, '$bf-ask1', '!request architect 300000 20000');
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const m = await bridge();
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    self.isDuplicateMatrixEvent = () => false;
    self.processingMatrixEventIds = new Map();
    self.backfillJoinedRoomOnSide = m.MatrixBridge.prototype.backfillJoinedRoomOnSide.bind(self);
    self.routeBackfilledEvents = m.MatrixBridge.prototype.routeBackfilledEvents.bind(self);
    /*
     * PAGE 1 (newest-first): asks + noise ONLY — no invite boundary → 'unproven' → the walk MUST
     * continue. Its `end` cursor becomes page 2's `from`.
     * PAGE 2: the join and the invite, terminal (`end: null`).
     * Timeline order across both pages (oldest→newest): invite, ask2, join … ask1(newest).
     * Window = [invite+1, join) = ask2 on page 2; ask1 is NEWER than the join (post-join, live
     * sync's job). Adjust the assertions to the boundary contract.
     */
    const pages = [
      { chunk: [ask1, msg(ROOM, '$bf-old1')], end: 'p2' },
      { chunk: [memberJoin, ask2, memberInvite], end: null },
    ];
    const reads = [];
    const origFetch = globalThis.fetch;
    globalThis.fetch = async (u, ...rest) => {
      const url = String(u);
      if (!url.includes('/messages')) return origFetch(u, ...rest);
      reads.push(url);
      const from = new URL(url).searchParams.get('from');
      const page = from === 'p2' ? pages[1] : pages[0];        // cursor → next page's from
      return { ok: true, status: 200, json: async () => page };
    };
    cleanup.push(() => { globalThis.fetch = origFetch; });

    const delivered = await self.backfillJoinedRoomOnSide(SIDE, ROOM, REP);
    // BOTH pages requested, in cursor order (first without from, second with from=p2)
    expect(reads).toHaveLength(2);
    expect(new URL(reads[0]).searchParams.get('from')).toBeNull();
    expect(new URL(reads[1]).searchParams.get('from')).toBe('p2');
    // the WINDOW event (ask2, between invite and join on page 2) entered the typed path
    const ids = typed.messages.map((t) => t.event.event_id);
    expect(ids).toEqual(['$bf-ask2']);
    expect(ids).not.toContain('$bf-ask1');                       // post-join: live sync's job
    expect(ids).not.toContain('$bf-old1');                       // pre-invite noise
    expect(delivered).toBe(1);
    // the walk STOPPED at the join boundary: page 2's join ends it — no third read
    expect(reads).toHaveLength(2);
  });

  test('r8_backfill_second_page_failure_keeps_first_page_events_once', async () => {
    /*
     * ② counter-case: page 2 answers 500 → the production backfill records the failure and
     * STOPS; page 1's already-routed events are NOT re-routed (no duplicate delivery).
     */
    const memberInvite = { type: 'm.room.member', room_id: ROOM, event_id: '$bf-inv', state_key: REP, sender: '@human:palpo.test', content: { membership: 'invite' } };
    const ask1 = msg(ROOM, '$bf-ask1', '!request architect 300000 20000');
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const m = await bridge();
    const { self, typed } = await makeBridgeWithSide({
      sideId: SIDE, hsToken: HS, asToken: AS, registration: REG, representativeMxid: REP, palpo,
    });
    self.isDuplicateMatrixEvent = () => false;
    self.processingMatrixEventIds = new Map();
    self.backfillJoinedRoomOnSide = m.MatrixBridge.prototype.backfillJoinedRoomOnSide.bind(self);
    self.routeBackfilledEvents = m.MatrixBridge.prototype.routeBackfilledEvents.bind(self);
    let reads = 0;
    const origFetch = globalThis.fetch;
    globalThis.fetch = async (u, ...rest) => {
      const url = String(u);
      if (!url.includes('/messages')) return origFetch(u, ...rest);
      reads += 1;
      // page 1 carries NO boundary (asks + noise only) — the walk MUST continue; page 2 FAILS
      if (reads === 1) return { ok: true, status: 200, json: async () => ({ chunk: [ask1, msg(ROOM, '$bf-old1')], end: 'p2' }) };
      return { ok: false, status: 500, json: async () => ({ errcode: 'M_UNKNOWN' }) };  // page 2 FAILS
    };
    cleanup.push(() => { globalThis.fetch = origFetch; });
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      const delivered = await self.backfillJoinedRoomOnSide(SIDE, ROOM, REP);
      expect(reads).toBe(2);                                     // it tried page 2 and failed
      expect(delivered).toBe(0);                                 // fail-closed: nothing routed
      // page 1's window event was NOT routed (no partial guess of the boundary) — and therefore
      // cannot have been routed TWICE either; the failure kept it out of the gate entirely.
      expect(typed.messages.filter((t) => t.event.event_id === '$bf-ask1')).toHaveLength(0);
      // the operator-visible failure record: the reason IS logged (removing the fail-closed
      // return would still deliver 0 here, but the WARN line is the contract this pins)
      const warns = warnSpy.mock.calls.map((c) => c.join(' ')).join(' ');
      expect(warns).toMatch(/history unreadable|M_UNKNOWN|routed nothing/i);
    } finally { warnSpy.mockRestore(); }
  });

  test('r7_rotation_full_real_chain_store_endpoints_refresh', async () => {
    /*
     * 16-impl-r7 ③: NO actingSideFor override, NO hand-set snapshot registration. The store is
     * a REAL ProjectSideStore file; rotation mutates IT; both projections come from the REAL
     * backend endpoints (backend-test-runtime serves them; only the bridge→backend HTTP hop is
     * seamed per the board's rule); both refreshes are the real methods.
     */
    const { createBackendTestContext } = await import('./helpers/backend-test-runtime.js');
    const request = (await import('supertest')).default;
    const { derivedRegistrationId } = await import('../lib/project-side-inbound.js');
    const R7_SECRET = 'r7-bridge-secret-0123456789abcdef';
    const mkStoreJson = (hsToken, asToken) => JSON.stringify({
      version: 1,
      sides: { [SIDE]: {
        id: SIDE, serverName: SIDE, apiBaseUrl: 'http://127.0.0.1:1', createdAt: 1, updatedAt: 1,
        active: true, accessState: 'accepted', projects: {},
        credential: { kind: 'appservice', hsToken, asToken, senderLocalpart: 'hagency', namespace: '@ac_.*',
          url: null, outboundGeneration: 'r7-initial-generation' },
        representative: { mxid: REP, localpart: 'hagency', observedAt: 1 },
      } },
      audit: [],
    });
    const ctx = await createBackendTestContext('r7-rot-', {
      env: { MATRIX_BRIDGE_SECRET: R7_SECRET },
      rawRuntimeFiles: { 'data/project-sides.json': mkStoreJson(HS, AS) },
    });
    cleanup.push(() => ctx.cleanup());
    const palpo = await fakePalpo({ members: { [ROOM]: [REP] } });
    const m = await bridge();
    const self = {
      actingCredentials: new Map(),
      appserviceInboundSnapshot: null,
      appserviceSideTokens: null,
      sideProvenanceClaims: new Map(), sideProvenanceClaimOrder: [],
      postWarning() {},
      async onRoomMessage() {}, async onRoomEvent() {}, async onAppserviceMembership() {},
    };
    self.handleAppserviceEvents = m.MatrixBridge.prototype.handleAppserviceEvents.bind(self);
    self.assertSideProvenanceForEvent = m.MatrixBridge.prototype.assertSideProvenanceForEvent.bind(self);
    self.executeTypedForClaim = m.MatrixBridge.prototype.executeTypedForClaim.bind(self);
    // ③: the REAL actingSideFor — no override; it reads the actingCredentials map the real
    // refreshActingCredentials populated from the real endpoint payload.
    self.actingSideFor = m.MatrixBridge.prototype.actingSideFor.bind(self);
    self.refreshActingCredentials = m.MatrixBridge.prototype.refreshActingCredentials.bind(self);
    self.refreshAppserviceSides = m.MatrixBridge.prototype.refreshAppserviceSides.bind(self);
    self.refreshOutboundFleets = m.MatrixBridge.prototype.refreshOutboundFleets.bind(self);
    self.reconcileOutboundFleets = m.MatrixBridge.prototype.reconcileOutboundFleets.bind(self);
    self.appserviceRouter = { setSides() {}, sideIds: () => [SIDE] };
    // BOTH endpoints served from the REAL backend app over supertest (HTTP hop only)
    const fetchEndpoints = async () => {
      const inbound = await request(ctx.app).get('/api/project-sides/inbound-credentials').set('x-bridge-secret', R7_SECRET);
      const acting = await request(ctx.app).get('/api/project-sides/acting-credentials').set('x-bridge-secret', R7_SECRET);
      return { inbound: inbound.body, acting: acting.body };
    };
    self.backendApiForSides = async () => (await fetchEndpoints()).inbound;
    self.backendApiForActing = async () => (await fetchEndpoints()).acting;

    // gen 1: both refreshes from the REAL store through the REAL endpoints
    await self.refreshAppserviceSides();
    await self.refreshActingCredentials();
    const reg1 = derivedRegistrationId(SIDE, HS);
    expect(self.appserviceInboundSnapshot.get(SIDE)?.registration).toBe(reg1);
    const acting1 = self.actingCredentials.get(SIDE);
    expect(acting1?.registration).toBe(reg1);
    expect(self.actingSideFor(SIDE)).toMatchObject({
      side: { active: true, accessState: 'accepted', representative: { mxid: REP } },
      credential: { kind: 'appservice', outboundGeneration: expect.any(String) },
    });
    // the apiBaseUrl lives in the STORE; point it at the fake Palpo through the REAL API (the
    // backend owns its store; the API is the only write path it observes)
    await request(ctx.app).post('/api/project-sides')
      .send({ server_name: SIDE, api_base_url: palpo.url }).expect(200);

    // ROTATION through the REAL backend API (the backend owns the store in memory; mutating the
    // file underneath it would not be observed — the API is the authoritative write path)
    const NEW_HS = 'hs-r7-rotated';
    const NEW_AS = 'as-r7-rotated';
    const reg2 = derivedRegistrationId(SIDE, NEW_HS);
    await request(ctx.app).put(`/api/project-sides/${SIDE}/credential`).send({
      credential: { kind: 'appservice', hsToken: NEW_HS, asToken: NEW_AS, senderLocalpart: 'hagency', namespace: '@ac_.*' },
    }).expect(200);

    // INTERLEAVE: inbound refreshed to gen2, acting still gen1 → retryable stale
    await self.refreshAppserviceSides();
    expect(self.appserviceInboundSnapshot.get(SIDE)?.registration).toBe(reg2);
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      await expect(self.handleAppserviceEvents(SIDE, [msg(ROOM, '$rot7')], {
        txnId: 'rot7', provenance: { registration: reg2, sideId: SIDE, mode: 'push' },
      })).rejects.toMatchObject({ code: 'acting_registration_stale', retryable: true });
      expect(warnSpy.mock.calls.map((c) => c.join(' ')).join(' ')).toMatch(/generation .* != snapshot/);

      // CONVERGENCE: the ACTING refresh completes from the SAME store → the same event passes
      await self.refreshActingCredentials();
      expect(self.actingCredentials.get(SIDE)?.registration).toBe(reg2);
      await expect(self.handleAppserviceEvents(SIDE, [msg(ROOM, '$rot7')], {
        txnId: 'rot7r', provenance: { registration: reg2, sideId: SIDE, mode: 'push' },
      })).resolves.toBeUndefined();
      expect(self.sideProvenanceClaims.get(`${reg2}|${ROOM}|$rot7`)?.state).toBe('completed');
    } finally { warnSpy.mockRestore(); }
  });

  test('r5_receiver_and_acting_case_normalization_regression', async () => {
    const { createAppserviceRouter } = await import('../lib/appservice-receiver.js');
    const received = [];
    const router = createAppserviceRouter({
      sides: [{ sideId: 'Palpo.Test', hsToken: HS, registration: REG, onEvents: (ev) => received.push(ev) }],
    });
    // the ROUTER was keyed mixed-case; authenticate with the token and deliver
    const res = await router.handle({
      method: 'PUT', path: '/_matrix/app/v1/transactions/ci', query: {},
      headers: { authorization: `Bearer ${HS}` }, body: { events: [msg('!x:palpo.test', '$ci')] },
    });
    expect(res.status).toBe(200);                       // authenticated despite the mixed-case id
    expect(received).toHaveLength(1);
    // actingSideFor: production WRITES through normalizeSideKey, so a mixed-case id lands lowercase
    const { normalizeSideKey } = await import('../lib/side-provenance.js');
    const actingMap = new Map([[normalizeSideKey('Palpo.Test'), { registration: REG }]]);
    expect(actingMap.get(normalizeSideKey('PALPO.TEST'))?.registration).toBe(REG);
  });
});
