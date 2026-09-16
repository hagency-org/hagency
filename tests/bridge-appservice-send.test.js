/*
 * AN AGENT ON AN APPSERVICE SIDE CAN SPEAK — the `canSend: false` hole, closed.
 *
 * `POST /api/agents/:name/matrix-identity` has answered `canSend: false` with a note saying "the
 * bridge send path still requires [a per-agent token], so it cannot send as this agent yet" ever since
 * appservice sides existed. Everything else was in place: the namespace makes the agent addressable,
 * the representative invites it, and the as_token joins it. Then it had nothing to say with — the
 * outbound path resolved a token, found none, and dropped the message.
 *
 * THE ASSERTION THAT CARRIES THIS FILE is `?user_id=`. Sending with the as_token and no masquerade
 * posts as the REPRESENTATIVE while every caller believes the agent spoke — a false record of who said
 * what, in somebody else's room, which is worse than the message not being sent at all.
 */

import { afterAll, afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import os from 'os';
import path from 'path';
import { mkdtempSync, rmSync } from 'fs';
import { pathToFileURL } from 'url';
import { restoreEnv, snapshotEnv } from './helpers/env.js';

describe('sending as an agent that has no token of its own', () => {
  let MatrixBridge;
  let bridgeStateForTest;
  let runtimeDir;
  let envSnapshot;

  const SIDE = 'palpo.test';
  const ROOM = `!proj:${SIDE}`;
  const AGENT = 'biglittle';
  const AGENT_MXID = `@ac_${AGENT}:${SIDE}`;
  const AS_TOKEN = 'as_secret_never_logged';

  beforeAll(async () => {
    runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'hagency-as-send-'));
    envSnapshot = snapshotEnv(['HAGENCY_RUNTIME_DIR', 'MATRIX_AGENT_PREFIX', 'MATRIX_SERVER_NAME']);
    process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
    process.env.MATRIX_AGENT_PREFIX = 'ac_';
    ({ MatrixBridge, bridgeStateForTest } = await import(`${pathToFileURL(path.resolve('bridge-matrix.js')).href}?as-send`));
  });

  afterAll(() => {
    restoreEnv(envSnapshot);
    rmSync(runtimeDir, { recursive: true, force: true });
  });

  afterEach(() => { vi.unstubAllGlobals(); });

  /**
   * A bridge with only what the send path touches.
   *
   * Constructed as a bare object rather than a real `MatrixBridge`, because the constructor reaches for
   * a homeserver, a bot login and a state file — none of which this question depends on. The methods
   * under test are borrowed onto it, which is also how it stays honest: if `sendAsAgentContent` starts
   * depending on something else, this stops working rather than quietly testing a copy.
   */
  function bridgeStub() {
    const stub = {
      agentWork: new Map(),
      matrixDeliveryJournal: { get: () => null },
      ended: [],
      warnings: [],
      endAgentWork(name, roomId) { this.ended.push({ name, roomId }); },
      endAgentWorkForToken(token, roomId) { this.ended.push({ token, roomId }); },
      postWarning(message) { this.warnings.push(message); },
      rememberMatrixEvent() {},
      // F10 (17-r3): the required roster callback — admits this sender's own MXID
      isKnownAgentMxid: (mxid) => mxid === AGENT_MXID,
    };
    stub.sendAsAgentContent = MatrixBridge.prototype.sendAsAgentContent.bind(stub);
    stub.sendAsAgent = MatrixBridge.prototype.sendAsAgent.bind(stub);
    return stub;
  }

  const appserviceSender = (over = {}) => ({
    kind: 'appservice',
    side: { serverName: SIDE, apiBaseUrl: 'http://127.0.0.1:8008' },
    credential: { kind: 'appservice', asToken: AS_TOKEN, senderLocalpart: 'hagency', namespace: '@ac_.*' },
    agentUserId: AGENT_MXID,
    agentName: AGENT,
    ...over,
  });

  function captureFetch(response = { ok: true, status: 200, json: async () => ({ event_id: '$ev1' }) }) {
    const calls = [];
    vi.stubGlobal('fetch', async (url, init = {}) => {
      calls.push({ url: String(url), method: init.method, headers: init.headers ?? {}, body: init.body });
      return response;
    });
    return calls;
  }

  test('the as_token sends, and the AGENT is named in ?user_id=', async () => {
    const calls = captureFetch();
    const bridge = bridgeStub();

    const eventId = await bridge.sendAsAgentContent(
      appserviceSender(), ROOM, { msgtype: 'm.text', body: 'hello from the site' },
    );

    expect(eventId).toBe('$ev1');
    expect(calls).toHaveLength(1);
    const [call] = calls;
    expect(call.method).toBe('PUT');
    // The side's own base url, not this deployment's homeserver.
    expect(call.url).toContain('http://127.0.0.1:8008/_matrix/client/v3/rooms/');
    expect(call.headers.Authorization).toBe(`Bearer ${AS_TOKEN}`);
    /*
     * THE MASQUERADE. Without this parameter the message is posted by the representative and reported
     * as the agent — the one outcome worse than a failed send, because nothing anywhere says so.
     */
    expect(call.url).toContain(`user_id=${encodeURIComponent(AGENT_MXID)}`);
    expect(JSON.parse(call.body)).toMatchObject({ body: 'hello from the site' });
  });

  test('projection delivery uses the stored event type transaction id and exact publisher', async () => {
    const calls = captureFetch();
    const bridge = bridgeStub();
    const sender = appserviceSender();
    sender.credential.outboundGeneration = 'generation-1';
    bridge.actingSideFor = () => ({ side: sender.side, credential: sender.credential });
    const content = { algorithm: 'm.megolm.v1.aes-sha2', ciphertext: 'fixed' };
    await bridge.sendAsAgentContent(sender, ROOM, content, null, {
      transactionId: 'hafleet_fixed', preparedEventType: 'm.room.encrypted',
      expectedPublisherMxid: AGENT_MXID, expectedCredentialGeneration: 'generation-1', throwOnFailure: true,
    });
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toContain('/send/m.room.encrypted/hafleet_fixed');
    expect(JSON.parse(calls[0].body)).toEqual(content);

    await expect(bridge.sendAsAgentContent(sender, ROOM, content, null, {
      transactionId: 'hafleet_other', preparedEventType: 'm.room.encrypted',
      expectedPublisherMxid: '@ac_someone-else:side.test', expectedCredentialGeneration: 'generation-1', throwOnFailure: true,
    })).rejects.toThrow(/publisher/);
    expect(calls).toHaveLength(1);
  });

  test('canonical projection refuses ordinary direct-chat re-encryption without changing normal sends', async () => {
    const calls = captureFetch();
    const bridge = bridgeStub();
    const sender = appserviceSender();
    sender.credential.outboundGeneration = 'generation-1';
    bridge.actingSideFor = () => ({ side: sender.side, credential: sender.credential });
    bridge.directChats = { send: vi.fn(async () => '$ordinary-direct') };
    const state = bridgeStateForTest();
    const previous = state.trustedManagedRooms[ROOM];
    state.trustedManagedRooms[ROOM] = { directChat: true };
    try {
      await expect(bridge.sendAsAgentContent(sender, ROOM, { ciphertext: 'durable' }, null, {
        transactionId: 'fixed-projection', preparedEventType: 'm.room.encrypted',
        expectedPublisherMxid: AGENT_MXID, expectedCredentialGeneration: 'generation-1', throwOnFailure: true,
      })).rejects.toThrow('approval_projection_direct_chat_unsupported');
      expect(calls).toHaveLength(0);
      expect(bridge.directChats.send).not.toHaveBeenCalled();
      expect(await bridge.sendAsAgentContent(sender, ROOM, { body: 'ordinary direct message' }))
        .toBe('$ordinary-direct');
      expect(bridge.directChats.send).toHaveBeenCalledTimes(1);
      expect(calls).toHaveLength(0);
    } finally {
      if (previous === undefined) delete state.trustedManagedRooms[ROOM];
      else state.trustedManagedRooms[ROOM] = previous;
    }
  });

  test('projection delivery rejects incomplete publisher context before Matrix I/O', async () => {
    const calls = captureFetch();
    await expect(bridgeStub().sendAsAgentContent(appserviceSender(), ROOM, { body: 'fixed' }, null, {
      transactionId: 'hafleet_incomplete', preparedEventType: 'm.room.message',
      expectedPublisherMxid: AGENT_MXID, throwOnFailure: true,
    })).rejects.toThrow(/incomplete.*context/);
    expect(calls).toHaveLength(0);
  });

  test('projection delivery rejects a retired captured credential even when current matches the plan', async () => {
    const calls = captureFetch();
    const captured = appserviceSender();
    captured.credential.outboundGeneration = 'generation-1';
    const current = appserviceSender();
    current.credential.outboundGeneration = 'generation-2';
    const bridge = bridgeStub();
    bridge.actingSideFor = () => ({ side: current.side, credential: current.credential });
    await expect(bridge.sendAsAgentContent(captured, ROOM, { body: 'fixed' }, null, {
      transactionId: 'hafleet_captured_stale', preparedEventType: 'm.room.message',
      expectedPublisherMxid: AGENT_MXID, expectedCredentialGeneration: 'generation-2', throwOnFailure: true,
    })).rejects.toThrow(/captured.*durable plan/);
    expect(calls).toHaveLength(0);
  });

  test('projection membership recovery revalidates current send context before retry PUT', async () => {
    const calls = [];
    const original = appserviceSender();
    original.credential.outboundGeneration = 'generation-1';
    let current = original;
    vi.stubGlobal('fetch', async (url, init = {}) => {
      calls.push({ url: String(url), init });
      if (calls.length === 1) {
        current = appserviceSender();
        current.credential.outboundGeneration = 'generation-2';
        return { ok: false, status: 403, json: async () => ({ errcode: 'M_FORBIDDEN', error: 'membership leave' }) };
      }
      return { ok: true, status: 200, json: async () => ({ event_id: '$unexpected' }) };
    });
    const bridge = bridgeStub();
    bridge.actingSideFor = () => ({ side: current.side, credential: current.credential });
    await expect(bridge.sendAsAgentContent(original, ROOM, { body: 'fixed' }, null, {
      transactionId: 'hafleet_recovery', preparedEventType: 'm.room.message',
      expectedPublisherMxid: AGENT_MXID, expectedCredentialGeneration: 'generation-1', throwOnFailure: true,
    })).rejects.toThrow(/generation changed/);
    expect(calls).toHaveLength(1);
  });

  test('projection membership recovery revalidates after invite before join', async () => {
    const calls = [];
    const original = appserviceSender();
    original.credential.outboundGeneration = 'generation-1';
    let current = original;
    vi.stubGlobal('fetch', async (url, init = {}) => {
      calls.push({ url: String(url), init });
      if (calls.length === 1) {
        return { ok: false, status: 403, json: async () => ({ errcode: 'M_FORBIDDEN', error: 'membership leave' }) };
      }
      if (String(url).includes('/invite')) {
        current = appserviceSender();
        current.credential.outboundGeneration = 'generation-2';
      }
      return { ok: true, status: 200, json: async () => ({}) };
    });
    const bridge = bridgeStub();
    bridge.actingSideFor = () => ({ side: current.side, credential: current.credential });
    await expect(bridge.sendAsAgentContent(original, ROOM, { body: 'fixed' }, null, {
      transactionId: 'hafleet_invite_rotation', preparedEventType: 'm.room.message',
      expectedPublisherMxid: AGENT_MXID, expectedCredentialGeneration: 'generation-1', throwOnFailure: true,
    })).rejects.toThrow(/generation changed/);
    expect(calls).toHaveLength(2);
    expect(calls[1].url).toContain('/invite');
  });

  test('the work indicator ends by NAME, because there is no token to look the name up from', async () => {
    captureFetch();
    const bridge = bridgeStub();
    await bridge.sendAsAgentContent(appserviceSender(), ROOM, { msgtype: 'm.text', body: 'x' });
    expect(bridge.ended).toEqual([{ name: AGENT, roomId: ROOM }]);
  });

  test('a token sender still sends exactly as it did, with no user_id', async () => {
    /*
     * The regression that matters most: every agent registered the old way keeps its own path. A
     * `user_id` on a real token's send would be a masquerade request from an account with no
     * appservice rights — a 403 on every message the old fleet sends.
     */
    const calls = captureFetch();
    const bridge = bridgeStub();
    await bridge.sendAsAgentContent('agent-own-token', ROOM, { msgtype: 'm.text', body: 'x' });
    expect(calls[0].headers.Authorization).toBe('Bearer agent-own-token');
    expect(calls[0].url).not.toContain('user_id=');
    expect(bridge.ended).toEqual([{ token: 'agent-own-token', roomId: ROOM }]);
  });

  test('no token and no appservice credential REFUSES, instead of sending "Bearer undefined"', async () => {
    /*
     * What the old signature did with a missing token: interpolated `undefined` into the header, got a
     * 401, and warned about the room. The refusal now names the missing credential, which is the thing
     * an operator has to fix.
     */
    const calls = captureFetch();
    const bridge = bridgeStub();
    const eventId = await bridge.sendAsAgentContent(null, ROOM, { msgtype: 'm.text', body: 'x' });
    expect(eventId).toBeNull();
    expect(calls).toHaveLength(0);
    expect(bridge.warnings.join(' ')).toMatch(/no credential or appservice sender/);
  });

  test('an incomplete appservice sender is refused, not half-used', async () => {
    const calls = captureFetch();
    const bridge = bridgeStub();
    // No agentUserId: the one field that decides WHO speaks.
    const eventId = await bridge.sendAsAgentContent(
      { kind: 'appservice', side: { serverName: SIDE, apiBaseUrl: 'http://x' }, credential: { asToken: 'a' } },
      ROOM, { msgtype: 'm.text', body: 'x' },
    );
    expect(eventId).toBeNull();
    expect(calls).toHaveLength(0);
  });

  test('a failed send throws when the caller asked to be told, and warns when it did not', async () => {
    const bridge = bridgeStub();
    captureFetch({ ok: false, status: 403, json: async () => ({ errcode: 'M_FORBIDDEN', error: 'nope' }) });
    // Not a membership failure, so no re-admission is attempted — the plain refusal path.
    await expect(bridge.sendAsAgentContent(
      appserviceSender(), ROOM, { msgtype: 'm.text', body: 'x' }, null, { throwOnFailure: true },
    )).rejects.toThrow(/M_FORBIDDEN/);

    const quiet = bridgeStub();
    captureFetch({ ok: false, status: 403, json: async () => ({ errcode: 'M_FORBIDDEN', error: 'nope' }) });
    expect(await quiet.sendAsAgentContent(appserviceSender(), ROOM, { msgtype: 'm.text', body: 'x' })).toBeNull();
    expect(quiet.warnings.join(' ')).toMatch(/M_FORBIDDEN/);
  });

  test('normalizeSender: a string is a token, a complete object is a sender, anything else is nothing', () => {
    expect(MatrixBridge.normalizeSender('tok')).toEqual({ kind: 'token', token: 'tok' });
    expect(MatrixBridge.normalizeSender(appserviceSender()).kind).toBe('appservice');
    for (const bad of [null, undefined, 42, {}, { kind: 'appservice' }, { kind: 'token' }]) {
      expect(MatrixBridge.normalizeSender(bad)).toBeNull();
    }
  });
});

/*
 * A DM FOR AN AGENT THAT HAS NO TOKEN — the gap the live run found.
 *
 * The 2026-08-15 run against real Palpo got the agent invited into the project room, joined, and
 * speaking in it, and then a DM to the same human was dropped one layer ABOVE the send path that had
 * just been taught to do this: `ensureDmRoom` did `if (!fromToken) return null`. Everything below was
 * ready and nothing reached it.
 */
describe('a DM room for an agent with no token of its own', () => {
  let MatrixBridge;
  let bridgeStateForTest;
  let runtimeDir;
  let envSnapshot;
  let mod;

  const SIDE = 'palpo.test';
  const AGENT = 'sitehand';
  const AGENT_MXID = `@ac_${AGENT}:${SIDE}`;
  const HUMAN = 'borrower';
  const AS_TOKEN = 'as_secret_never_logged';

  beforeAll(async () => {
    runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'hagency-as-dm-'));
    envSnapshot = snapshotEnv(['HAGENCY_RUNTIME_DIR', 'MATRIX_AGENT_PREFIX']);
    process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
    process.env.MATRIX_AGENT_PREFIX = 'ac_';
    mod = await import(`${pathToFileURL(path.resolve('bridge-matrix.js')).href}?as-dm`);
    ({ MatrixBridge } = mod);
  });

  afterAll(() => {
    restoreEnv(envSnapshot);
    rmSync(runtimeDir, { recursive: true, force: true });
  });

  afterEach(() => { vi.unstubAllGlobals(); });

  /**
   * Only what `ensureDmRoomOnSide` touches, borrowed off the prototype.
   *
   * `agentSenderFor` is stubbed rather than driven through real state: what is under test is what this
   * method does with a sender, and building one through `actingSideFor` would be testing the credential
   * refresh loop instead.
   */
  function stub({ sender, humanMxid }) {
    const self = {
      dmRooms: new Map(),
      warnings: [],
      postWarning(message, meta) { this.warnings.push({ message, meta }); },
      agentSenderFor: () => sender,
      /*
       * F10 (17-r3): the roster callback is now REQUIRED on agent masquerades —
       * this stub admits the sender's own MXID, the minimal honest roster.
       */
      isKnownAgentMxid: (mxid) => mxid === sender?.agentUserId,
    };
    self.ensureDmRoomOnSide = MatrixBridge.prototype.ensureDmRoomOnSide.bind(self);
    /*
     * Seeded through the live observed-MXID map (#73's `humanMxidStateForTest`), because that is the
     * map `humanUserId` reads and the whole question here is which server it answers with. A dedicated
     * setter would have been a second way in — I reached for one and it does not exist, which is the
     * map doing its job of having one owner.
     */
    if (humanMxid) {
      const state = mod.humanMxidStateForTest();
      state[String(humanMxid.slice(1, humanMxid.indexOf(':'))).toLowerCase()] = humanMxid;
    }
    return self;
  }

  const sender = (over = {}) => ({
    kind: 'appservice',
    side: { serverName: SIDE, apiBaseUrl: 'http://127.0.0.1:8008' },
    credential: { kind: 'appservice', asToken: AS_TOKEN, senderLocalpart: 'hagency', namespace: '@ac_.*' },
    agentUserId: AGENT_MXID,
    agentName: AGENT,
    ...over,
  });

  function fakeMatrix({ createOk = true, joinOk = true } = {}) {
    const calls = [];
    vi.stubGlobal('fetch', async (url, init = {}) => {
      const u = String(url);
      calls.push({ url: u, method: init.method, headers: init.headers ?? {}, body: init.body });
      if (u.includes('/createRoom')) {
        return createOk
          ? { ok: true, status: 200, json: async () => ({ room_id: `!dm:${SIDE}` }) }
          : { ok: false, status: 403, json: async () => ({ errcode: 'M_FORBIDDEN' }) };
      }
      if (u.includes('/join/')) {
        return joinOk
          ? { ok: true, status: 200, json: async () => ({ room_id: `!dm:${SIDE}` }) }
          : { ok: false, status: 403, json: async () => ({ errcode: 'M_FORBIDDEN' }) };
      }
      return { ok: true, status: 200, json: async () => ({}) };
    });
    return calls;
  }

  test('the representative creates it on the SIDE, and the agent joins by masquerade', async () => {
    const calls = fakeMatrix();
    const self = stub({ sender: sender(), humanMxid: `@${HUMAN}:${SIDE}` });

    const roomId = await self.ensureDmRoomOnSide({
      agentName: AGENT, humanName: HUMAN, humanIsAgent: false, key: `dm:${AGENT}`,
    });

    expect(roomId).toBe(`!dm:${SIDE}`);
    const create = calls.find((c) => c.url.includes('/createRoom'));
    const join = calls.find((c) => c.url.includes('/join/'));
    expect(create).toBeDefined();
    expect(join).toBeDefined();

    // The side's own homeserver, never ours.
    expect(create.url.startsWith('http://127.0.0.1:8008/')).toBe(true);
    // Created AS THE REPRESENTATIVE; joined AS THE AGENT. One credential, two masquerades.
    expect(create.url).toContain(encodeURIComponent(`@hagency:${SIDE}`));
    expect(join.url).toContain(encodeURIComponent(AGENT_MXID));

    const body = JSON.parse(create.body);
    /*
     * BOTH parties in the invite list, and the agent's presence is the assertion a live run had to
     * teach this test: `private_chat` is invite-only and the representative is the creator, so an agent
     * that is not invited takes a 403 on the join. The first version asserted only the human and passed
     * against a fake homeserver that answered 200 to any join.
     */
    expect(body.invite).toEqual([`@${HUMAN}:${SIDE}`, AGENT_MXID]);
    expect(body.is_direct).toBe(true);
    /*
     * THE BOT IS NOT IN THE INVITE LIST, and that is the point rather than an omission: it holds an
     * account on our server only, so inviting it would leave a pending invite nobody can ever accept.
     */
    expect(JSON.stringify(body.invite)).not.toMatch(/bot/i);
    // Plaintext, stated: the representative holds no crypto store, and the appservice reads this room.
    expect(body.initial_state).toEqual([]);
  });

  test('a human on ANOTHER server is refused, and no room is created', async () => {
    /*
     * Without federation a room on `palpo.test` holds `palpo.test` accounts and nothing else, so this
     * DM is not a room that can exist. Creating one anyway and inviting an mxid nobody can accept is
     * the shape of the bug #73 fixed — a plausible identity composed onto the wrong server.
     */
    const calls = fakeMatrix();
    const self = stub({ sender: sender(), humanMxid: '@elsewhere:other.example' });

    const roomId = await self.ensureDmRoomOnSide({
      agentName: AGENT, humanName: 'elsewhere', humanIsAgent: false, key: `dm:${AGENT}`,
    });

    expect(roomId).toBeNull();
    expect(calls).toHaveLength(0);
    const warned = self.warnings.map((w) => w.message).join(' ');
    // Both servers are named, because which one is wrong decides the operator's fix.
    expect(warned).toMatch(/other\.example/);
    expect(warned).toMatch(/palpo\.test/);
  });

  test('an agent WITH a token never reaches this path', async () => {
    const calls = fakeMatrix();
    const self = stub({ sender: { kind: 'token', token: 'has-one' }, humanMxid: `@${HUMAN}:${SIDE}` });
    const roomId = await self.ensureDmRoomOnSide({
      agentName: AGENT, humanName: HUMAN, humanIsAgent: false, key: `dm:${AGENT}`,
    });
    expect(roomId).toBeNull();
    expect(calls).toHaveLength(0);
  });

  test('a created room the agent cannot join returns null, rather than a room it cannot post in', async () => {
    /*
     * Handing back a room the sender is not in would turn one clear failure into a 403 on every later
     * send, attributed to whatever message happened to be next.
     */
    const calls = fakeMatrix({ joinOk: false });
    const self = stub({ sender: sender(), humanMxid: `@${HUMAN}:${SIDE}` });
    const roomId = await self.ensureDmRoomOnSide({
      agentName: AGENT, humanName: HUMAN, humanIsAgent: false, key: `dm:${AGENT}`,
    });
    expect(roomId).toBeNull();
    expect(calls.some((c) => c.url.includes('/createRoom'))).toBe(true);
    expect(self.warnings.map((w) => w.message).join(' ')).toMatch(/could not join/);
  });

  test('an agent-to-agent room is refused rather than guessed', async () => {
    const calls = fakeMatrix();
    const self = stub({ sender: sender(), humanMxid: `@${HUMAN}:${SIDE}` });
    const roomId = await self.ensureDmRoomOnSide({
      agentName: AGENT, humanName: 'otheragent', humanIsAgent: true, key: `${AGENT}:otheragent`,
    });
    expect(roomId).toBeNull();
    expect(calls).toHaveLength(0);
  });
});

/*
 * WHICH SERVER ACTS IN WHICH ROOM — ADR-016 row 1's audit, at the two sites a side room reaches today.
 *
 * Every `HOMESERVER` in the bridge predates project sides and asserts our own server for whatever room
 * it was handed. Most are still right (the bot's own account, rooms on our server). These two are not,
 * and one of them became reachable only because `ensureDmRoomOnSide` now persists side DM rooms — a
 * reachability introduced by the fix above it, which is why it is tested beside it.
 */
describe('a room on a project side is acted in by the side, not by us', () => {
  let MatrixBridge;
  let bridgeStateForTest;
  let runtimeDir;
  let envSnapshot;

  const SIDE = 'palpo.test';
  const SIDE_ROOM = `!proj:${SIDE}`;
  const OUR_ROOM = '!ours:matrix.example.test';

  beforeAll(async () => {
    runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'hagency-room-actor-'));
    envSnapshot = snapshotEnv(['HAGENCY_RUNTIME_DIR', 'MATRIX_AGENT_PREFIX', 'MATRIX_SERVER_NAME']);
    process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
    process.env.MATRIX_AGENT_PREFIX = 'ac_';
    process.env.MATRIX_SERVER_NAME = 'matrix.example.test';
    ({ MatrixBridge, bridgeStateForTest } = await import(`${pathToFileURL(path.resolve('bridge-matrix.js')).href}?room-actor`));
  });

  afterAll(() => {
    restoreEnv(envSnapshot);
    rmSync(runtimeDir, { recursive: true, force: true });
  });

  afterEach(() => { vi.unstubAllGlobals(); });

  const acting = {
    side: { serverName: SIDE, apiBaseUrl: 'http://127.0.0.1:8008' },
    credential: { kind: 'appservice', asToken: 'as_secret_never_logged', senderLocalpart: 'hagency', namespace: '@ac_.*' },
  };

  function stub({ sides = { [SIDE]: acting } } = {}) {
    const self = {
      warnings: [],
      postWarning(m) { this.warnings.push(m); },
      actingSideFor: (server) => sides[String(server).toLowerCase()] ?? null,
      getBotToken: () => 'bot-token',
      getAgentToken: () => 'agent-token',
      botUserId: '@hagencybot:matrix.example.test',
      /*
       * F10 (17-r3): the required roster callback. This describe drives invite/actor
       * paths whose senders carry several agent MXIDs, so the stub admits any
       * `@ac_*` localpart on the side — the namespace the real roster would vouch
       * for here, without re-deriving each test's composed MXID.
       */
      isKnownAgentMxid: (mxid) => /^@ac_[^:]+:palpo\.test$/.test(mxid),
    };
    for (const m of ['sideForRoom', '_inviteHumanToDm', 'inviteBotIntoAgentRoom']) {
      self[m] = MatrixBridge.prototype[m].bind(self);
    }
    return self;
  }

  test('sideForRoom answers for a side room and null for our own', () => {
    const self = stub();
    expect(self.sideForRoom(SIDE_ROOM)).toBe(acting);
    expect(self.sideForRoom(OUR_ROOM)).toBeNull();
    // A server we hold no acting credential for is not ours to act in either.
    expect(self.sideForRoom('!x:stranger.example')).toBeNull();
    /*
     * OUR OWN SERVER WINS EVEN IF A CREDENTIAL EXISTS FOR IT. A deployment that registered an
     * appservice on its OWN homeserver would otherwise have every local room rerouted through the
     * representative — the bot's rooms answered with somebody else's actor. Found by a surviving
     * mutant: deleting the `=== MATRIX_SERVER_NAME` check passed every other test here, because none of
     * them had an acting credential for our own name.
     */
    const alsoOurs = stub({ sides: { [SIDE]: acting, 'matrix.example.test': acting } });
    expect(alsoOurs.sideForRoom(OUR_ROOM)).toBeNull();
    expect(alsoOurs.sideForRoom(SIDE_ROOM)).toBe(acting);
    for (const bad of [null, undefined, 'not-a-room', 42]) expect(self.sideForRoom(bad)).toBeNull();
  });

  test('inviting a human into a SIDE DM goes through the representative', async () => {
    const calls = [];
    vi.stubGlobal('fetch', async (url, init = {}) => {
      calls.push({ url: String(url), headers: init.headers ?? {} });
      return { ok: true, status: 200, json: async () => ({}) };
    });
    const self = stub();

    const r = await self._inviteHumanToDm(SIDE_ROOM, 'borrower');
    expect(r).toMatchObject({ ok: true, via: 'representative' });
    expect(calls).toHaveLength(1);
    // The SIDE's homeserver with the as_token — never ours with the bot's.
    expect(calls[0].url.startsWith('http://127.0.0.1:8008/')).toBe(true);
    expect(calls[0].headers.Authorization).toBe('Bearer as_secret_never_logged');
  });

  test('inviting the bot into a SIDE room is skipped, with nothing attempted', async () => {
    /*
     * Not a failure to retry: the bot has no account there, so the invite would sit pending forever —
     * and it would be sent with an AGENT token against OUR homeserver, two wrong things at once.
     */
    const calls = [];
    vi.stubGlobal('fetch', async (url) => { calls.push(String(url)); return { ok: true, status: 200, json: async () => ({}) }; });
    const self = stub();
    expect(await self.inviteBotIntoAgentRoom(SIDE_ROOM, 'agent-token')).toBe('skipped-project-side');
    expect(calls).toHaveLength(0);
  });

  test('our own rooms keep the old path exactly', async () => {
    const calls = [];
    vi.stubGlobal('fetch', async (url, init = {}) => {
      calls.push({ url: String(url), headers: init.headers ?? {} });
      return { ok: true, status: 200, json: async () => ({}) };
    });
    const self = stub();
    await self.inviteBotIntoAgentRoom(OUR_ROOM, 'agent-token');
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toContain('matrix.example.test');
    expect(calls[0].headers.Authorization).toBe('Bearer agent-token');
  });
});

/*
 * THE ROOM DECIDES WHICH CREDENTIAL SPEAKS — found by running a SECOND customer, and findable no other
 * way.
 *
 * `biglittle` holds a real token on palpo.test. Asked to speak in a room on acme.test, the token branch
 * matched first, `baseUrlForToken` resolved to palpo.test, and the send asked that homeserver about a room
 * it had never heard of: `M_NOT_FOUND: room frame is not found`. An agent with a token SOMEWHERE is not an
 * agent with a token EVERYWHERE — a distinction that cannot appear while one project side exists, which is
 * how it survived every test and three live runs.
 */
describe('which credential speaks is decided by the room, not by what the agent holds', () => {
  let MatrixBridge;
  let bridgeStateForTest;
  let runtimeDir;
  let envSnapshot;

  const SIDE = 'acme.test';
  const OURS = 'matrix.example.test';

  beforeAll(async () => {
    runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'hagency-room-decides-'));
    envSnapshot = snapshotEnv(['HAGENCY_RUNTIME_DIR', 'MATRIX_AGENT_PREFIX', 'MATRIX_SERVER_NAME']);
    process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
    process.env.MATRIX_AGENT_PREFIX = 'ac_';
    process.env.MATRIX_SERVER_NAME = OURS;
    ({ MatrixBridge, bridgeStateForTest } = await import(`${pathToFileURL(path.resolve('bridge-matrix.js')).href}?room-decides`));
  });

  afterAll(() => {
    restoreEnv(envSnapshot);
    rmSync(runtimeDir, { recursive: true, force: true });
  });

  const acting = {
    side: { serverName: SIDE, apiBaseUrl: 'http://127.0.0.1:8018' },
    credential: { kind: 'appservice', asToken: 'acme_as_token', senderLocalpart: 'hagency', namespace: '@ac_.*' },
  };

  function bridge({ hasToken = true } = {}) {
    const self = {
      normalizeName: (n) => String(n || '').toLowerCase(),
      getAgentToken: () => (hasToken ? 'its-own-token-on-our-server' : null),
      actingSideFor: (server) => (String(server).toLowerCase() === SIDE ? acting : null),
    };
    self.sideForRoom = MatrixBridge.prototype.sideForRoom.bind(self);
    self.agentSenderFor = MatrixBridge.prototype.agentSenderFor.bind(self);
    return self;
  }

  test('a room on a project side is spoken into by THAT side, even when the agent has its own token', () => {
    const sender = bridge().agentSenderFor('biglittle', `!work:${SIDE}`);
    expect(sender.kind).toBe('appservice');
    expect(sender.agentUserId).toBe(`@ac_biglittle:${SIDE}`);
    expect(sender.credential.asToken).toBe('acme_as_token');
  });

  test('a room on OUR server still uses the agent\'s own token', () => {
    /*
     * The regression that would hurt most: every agent registered the old way speaks in our rooms with
     * its own credential, and routing those through an appservice it has no relationship with would be a
     * 403 on every message the existing fleet sends.
     */
    const sender = bridge().agentSenderFor('biglittle', `!ours:${OURS}`);
    expect(sender).toMatchObject({ kind: 'token', token: 'its-own-token-on-our-server' });
  });

  test('no room named falls back to the token, which is what the DM path needs', () => {
    // A DM's room is resolved later, so the sender is chosen before one is known.
    expect(bridge().agentSenderFor('biglittle').kind).toBe('token');
  });

  test('an agent with NO token still gets the side when the room is a side room', () => {
    const sender = bridge({ hasToken: false }).agentSenderFor('sitehand', `!work:${SIDE}`);
    expect(sender.kind).toBe('appservice');
    expect(sender.agentUserId).toBe(`@ac_sitehand:${SIDE}`);
  });

  test('an unknown server is nobody\'s side, so a token-less agent gets nothing rather than a guess', () => {
    expect(bridge({ hasToken: false }).agentSenderFor('sitehand', '!x:stranger.example')).toBeNull();
  });
});
