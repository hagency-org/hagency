import { afterAll, beforeAll, describe, expect, test, vi } from 'vitest';
import { mkdtempSync, rmSync } from 'fs';
import os from 'os';
import path from 'path';
import { pathToFileURL } from 'url';
import { restoreEnv, snapshotEnv } from './helpers/env.js';

describe('Matrix owner approval bridge', () => {
  let runtimeDir;
  let envSnapshot;
  let MatrixBridge;
  let buildOwnerApprovalRequest;
  let buildPublicApprovalNotice;
  let parseApprovalVerdictEvent;
  let approvalRoomPowerLevels;
  let isPrivateControlRoomName;
  let resolveApprovalDmMode;
  let markRoomTrusted;

  const approval = {
    id: 'approval_0123456789abcdef0123456789abcdef',
    agent: 'wf_coordinator',
    project: 'robrix2',
    project_room_id: '!project:palpo.test',
    /*
     * The owner is on the CONFIGURED server, matching their DM room below. The original fixture had
     * both on `palpo.test` — internally consistent, but on a server this test is not configured for,
     * and this case exists to exercise the BOT sending an encrypted private request. The bot has an
     * account on its own homeserver only, so the pair has to be there.
     *
     * The realistic post-decision case — an owner on a project side, ADR-016's resolved collision — is
     * the representative branch. `project_room_id` stays on `palpo.test` because that is genuinely
     * where a project lives.
     */
    owner_mxid: '@alex:matrix.example.com',
    /*
     * ON THE CONFIGURED SERVER, which this fixture was not. It said `palpo.test` while the test leaves
     * MATRIX_HOMESERVER at its default, so the server name is `matrix.example.com` — an inconsistency
     * nothing noticed until something READ the room's server. This case exercises the bot sending an
     * encrypted private request, and the bot only has an account on its own homeserver, so the room has
     * to be there for the case to mean what its name says.
     */
    owner_dm_room_id: '!approval-dm:matrix.example.com',
    upstream_request_id: 'abcde',
    input_digest: 'a'.repeat(64),
    runtime: 'claude',
    tool_name: 'Bash',
    description: 'Create a GitHub issue',
    input_preview: '{"command":"gh issue create --repo private/repo"}',
    expires_at: Date.now() + 60_000,
    status: 'pending',
  };

  beforeAll(async () => {
    runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'hagency-bridge-approval-'));
    envSnapshot = snapshotEnv(['HAGENCY_RUNTIME_DIR', 'MATRIX_AGENT_PREFIX']);
    process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
    process.env.MATRIX_AGENT_PREFIX = 'ac_';
    const bridgeUrl = pathToFileURL(path.resolve('bridge-matrix.js')).href;
    ({
      MatrixBridge,
      buildOwnerApprovalRequest,
      buildPublicApprovalNotice,
      parseApprovalVerdictEvent,
      approvalRoomPowerLevels,
      isPrivateControlRoomName,
      resolveApprovalDmMode,
      markRoomTrusted,
    } = await import(`${bridgeUrl}?approval-test=${Date.now()}`));
  });

  afterAll(() => {
    rmSync(runtimeDir, { recursive: true, force: true });
    restoreEnv(envSnapshot);
  });

  test('public_approval_notice_is_redacted_and_non_actionable', () => {
    /*
     * REQ-OWNER-UI-APPROVAL-PUBLIC. This notice is the only approval artifact the project
     * room ever receives, so what it must NOT carry is the requirement: the assertions
     * below check the serialized event for the input preview, the request id, the digest,
     * and the `actions` key that would make it clickable. Serialized rather than a shallow
     * property check, because a leak nested one level deeper would still pass the latter.
     */
    const content = buildPublicApprovalNotice(approval);
    const serialized = JSON.stringify(content);

    expect(content).toMatchObject({
      msgtype: 'com.agentchat.approval.status.v1',
      'com.agentchat.approval': {
        version: 1,
        kind: 'status',
        agent: 'wf_coordinator',
        project: 'robrix2',
        state: 'waiting_for_owner',
      },
    });
    expect(serialized).not.toContain('gh issue create');
    expect(serialized).not.toContain(approval.id);
    expect(serialized).not.toContain(approval.input_digest);
    expect(serialized).not.toContain('actions');
  });

  test('scoped approval cards preserve private details and validate all four structured decisions', () => {
    const scoped = { ...approval, runtime: 'codex', thread_root_event_id: '$scoped-thread', reusable_scope: {
      description: 'Network host: aapt.org\nProtocol: https', workspace: '/work/edison', task_id: 'task-1',
    } };
    const content = buildOwnerApprovalRequest(scoped);
    const detail = content['com.agentchat.approval'];
    expect(detail.actions.map(a => a.id)).toEqual(['approve_once', 'approve_task', 'approve_always', 'deny']);
    expect(detail.description).toContain('aapt.org');
    expect(detail.description).toContain(approval.project);
    expect(JSON.stringify(buildPublicApprovalNotice(scoped))).not.toMatch(/aapt|\/work|reusable_scope|approve_always/);
    expect(buildPublicApprovalNotice(scoped)['m.relates_to']?.event_id).toBe('$scoped-thread');
    for (const action of ['approve_once', 'approve_task', 'approve_always', 'deny']) {
      const event = { sender: approval.owner_mxid, event_id: '$decision', content: {
        msgtype: 'com.agentchat.approval.verdict.v1', 'com.agentchat.approval': { ...detail, kind: 'verdict', action },
      } };
      expect(parseApprovalVerdictEvent(approval.owner_dm_room_id, event)).toMatchObject({
        action, sender_mxid: approval.owner_mxid, input_digest: approval.input_digest, project_room_id: approval.project_room_id,
      });
      event.content['com.agentchat.approval'].action = 'allow_anything';
      expect(parseApprovalVerdictEvent(approval.owner_dm_room_id, event)).toBeNull();
    }
    expect(buildOwnerApprovalRequest(approval)['com.agentchat.approval'].actions).toHaveLength(2);
  });

  test('thread approval notices remain in the originating task thread', () => {
    const content = buildPublicApprovalNotice({ ...approval, thread_root_event_id: '$original-thread' });
    expect(content['m.relates_to']).toEqual({ rel_type: 'm.thread', event_id: '$original-thread',
      is_falling_back: true, 'm.in_reply_to': { event_id: '$original-thread' } });
    expect(JSON.stringify(content)).not.toContain(approval.input_preview);
    expect(content['com.agentchat.approval']).not.toHaveProperty('actions');
    expect(buildPublicApprovalNotice(approval)).not.toHaveProperty('m.relates_to');
  });

  test('owner_dm_approval_request_contains_structured_actions', () => {
    /*
     * REQ-OWNER-UI-APPROVAL-DM and REQ-OWNER-UI-APPROVAL-UI. The DM event is the full
     * request — it is the only place the bound agent, project, project room, request id and
     * digest appear together — and the approve-once/deny action descriptors asserted here
     * are the only mechanism offered for answering it. The body line pinned at the end says
     * so to the owner in words, which is why "text replies are not approval" is an assertion
     * and not a nicety.
     */
    const content = buildOwnerApprovalRequest(approval);

    expect(content.msgtype).toBe('com.agentchat.approval.request.v1');
    expect(content['com.agentchat.approval']).toMatchObject({
      agent: 'wf_coordinator',
      project: 'robrix2',
      project_room_id: '!project:palpo.test',
      request_id: approval.id,
      input_digest: approval.input_digest,
      actions: [
        { id: 'approve_once', style: 'primary' },
        { id: 'deny', style: 'danger' },
      ],
    });
    expect(content.body).toContain('Text replies are not approval');
  });

  test('approval_text_message_is_ignored', () => {
    /*
     * REQ-OWNER-UI-APPROVAL-UI, the negative half: a verdict MUST be a structured UI action,
     * so approval-shaped chat text in the owner's own DM must parse to nothing. Null here
     * means the bridge never submits a verdict for it and the request stays pending — the
     * Chinese text is deliberate, since a keyword scanner is exactly what this forbids.
     */
    expect(parseApprovalVerdictEvent('!approval-dm:palpo.test', {
      event_id: '$text',
      sender: '@alex:palpo.test',
      content: { msgtype: 'm.text', body: '批准创建' },
    })).toBeNull();
  });

  test('structured verdict preserves authenticated Matrix sender and binding fields', () => {
    /*
     * REQ-OWNER-UI-APPROVAL-IDENTITY and REQ-OWNER-UI-APPROVAL-AUTHORITY. The exact-object
     * assertion is what carries both: `sender_mxid` is the complete MXID taken from
     * `event.sender` and `room_id` is the room the bridge observed the event in, neither of
     * which the client's `com.hagency.approval` payload can supply or override. That is the
     * whole of "Robrix2 emits, hagency authorizes" on the bridge side — the identity used
     * downstream is homeserver-stamped, not self-reported.
     */
    const parsed = parseApprovalVerdictEvent('!approval-dm:palpo.test', {
      event_id: '$verdict',
      sender: '@alex:palpo.test',
      content: {
        msgtype: 'com.agentchat.approval.verdict.v1',
        body: 'Approval response submitted',
        'com.agentchat.approval': {
          version: 1,
          kind: 'verdict',
          agent: approval.agent,
          project: approval.project,
          project_room_id: approval.project_room_id,
          request_id: approval.id,
          input_digest: approval.input_digest,
          action: 'approve_once',
        },
      },
    });

    expect(parsed).toEqual({
      request_id: approval.id,
      sender_mxid: '@alex:palpo.test',
      room_id: '!approval-dm:palpo.test',
      agent: approval.agent,
      project: approval.project,
      project_room_id: approval.project_room_id,
      input_digest: approval.input_digest,
      action: 'approve_once',
      event_id: '$verdict',
    });
  });

  test('legacy_hagency_namespace_verdict_still_accepted_in_transition', () => {
    /*
     * Wire-protocol transition guard. Verdicts already in flight — sent by deployed clients
     * under the old `com.hagency.approval.verdict.v1` name before the namespace returned to
     * `com.agentchat.*` — must not be dropped: losing one reads as a hung approval on both
     * ends. Only the VERDICT accepts the legacy name; status/request are outbound-only.
     */
    const parsed = parseApprovalVerdictEvent('!approval-dm:palpo.test', {
      event_id: '$legacy-verdict',
      sender: '@alex:palpo.test',
      content: {
        msgtype: 'com.hagency.approval.verdict.v1',
        body: 'Approval response submitted',
        'com.hagency.approval': {
          version: 1,
          kind: 'verdict',
          agent: approval.agent,
          project: approval.project,
          project_room_id: approval.project_room_id,
          request_id: approval.id,
          input_digest: approval.input_digest,
          action: 'deny',
        },
      },
    });
    expect(parsed).toMatchObject({ action: 'deny', request_id: approval.id, sender_mxid: '@alex:palpo.test' });
    /*
     * FULL-PAYLOAD HYBRIDS, both directions, both REJECTED. Earlier the hybrid fixture omitted
     * request_id/digest and passed for the wrong reason — an incomplete payload, not the pairing
     * rule. These two carry every required field and differ ONLY in namespace mixing, so a null
     * here is the pairing rule speaking and nothing else.
     */
    const fullDetail = {
      version: 1,
      kind: 'verdict',
      agent: approval.agent,
      project: approval.project,
      project_room_id: approval.project_room_id,
      request_id: approval.id,
      input_digest: approval.input_digest,
      action: 'approve_once',
    };
    // hybrid 1: NEW msgtype, LEGACY payload key
    expect(parseApprovalVerdictEvent('!approval-dm:palpo.test', {
      event_id: '$hybrid-new-msgtype-legacy-key',
      sender: '@alex:palpo.test',
      content: {
        msgtype: 'com.agentchat.approval.verdict.v1',
        body: 'Approval response submitted',
        'com.hagency.approval': { ...fullDetail },
      },
    })).toBeNull();
    // hybrid 2: LEGACY msgtype, NEW payload key
    expect(parseApprovalVerdictEvent('!approval-dm:palpo.test', {
      event_id: '$hybrid-legacy-msgtype-new-key',
      sender: '@alex:palpo.test',
      content: {
        msgtype: 'com.hagency.approval.verdict.v1',
        body: 'Approval response submitted',
        'com.agentchat.approval': { ...fullDetail },
      },
    })).toBeNull();
  });

  test('delayed_room_key_retries_encrypted_owner_verdict', async () => {    /*
     * REQ-OWNER-UI-APPROVAL-DELIVERY, both halves, in the order they appear below. The first
     * decryption rejects, and the assertions are that the ciphertext went to the durable
     * store and that onRoomMessage was NOT called — an undecryptable verdict is not an
     * approval. The second retry succeeds, and onRoomMessage is called exactly once with the
     * cleartext before the record is removed, so the key arriving late recovers the verdict
     * without replaying it.
     */
    const roomId = '!approval-delayed-key:palpo.test';
    const encrypted = {
      type: 'm.room.encrypted',
      event_id: '$encrypted-verdict',
      sender: approval.owner_mxid,
      origin_server_ts: Date.now(),
      content: {
        algorithm: 'm.megolm.v1.aes-sha2',
        device_id: 'OWNERDEVICE',
        sender_key: 'owner-curve25519-key',
        ciphertext: 'ciphertext',
        session_id: 'session-id',
      },
    };
    const clearEvent = {
      ...encrypted,
      type: 'm.room.message',
      content: {
        msgtype: 'com.agentchat.approval.verdict.v1',
        body: 'Approval response submitted',
        'com.agentchat.approval': {
          version: 1,
          kind: 'verdict',
          agent: approval.agent,
          project: approval.project,
          project_room_id: approval.project_room_id,
          request_id: approval.id,
          input_digest: approval.input_digest,
          action: 'approve_once',
        },
      },
    };
    const records = new Map();
    const pendingEncryptedEventStore = {
      put: vi.fn(({ roomId: queuedRoomId, event }) => {
        records.set(event.event_id, {
          eventId: event.event_id,
          roomId: queuedRoomId,
          event,
          receivedAt: Date.now(),
        });
      }),
      list: vi.fn(() => [...records.values()]),
      remove: vi.fn((eventId) => records.delete(eventId)),
      prune: vi.fn(() => []),
    };
    const bridge = new MatrixBridge({ pendingEncryptedEventStore });
    markRoomTrusted(roomId, {
      approvalDm: true,
      agent: approval.agent,
      ownerMxid: approval.owner_mxid,
    });
    bridge.onRoomMessage = vi.fn().mockResolvedValue({ ok: true });
    const client = {
      emit: vi.fn(),
      crypto: {
        decryptRoomEvent: vi.fn()
          .mockRejectedValueOnce(new Error('missing room key'))
          .mockResolvedValueOnce({ raw: clearEvent }),
      },
    };
    bridge.configureReliableBotSync(client);

    await client.agentChatSyncHandler(
      'room.failed_decryption',
      roomId,
      encrypted,
      new Error('missing room key'),
    );
    expect(pendingEncryptedEventStore.put).toHaveBeenCalledWith({ roomId, event: encrypted });
    expect(bridge.onRoomMessage).not.toHaveBeenCalled();

    await bridge.retryPendingApprovalDecryptions(client);
    expect(bridge.onRoomMessage).not.toHaveBeenCalled();
    expect(pendingEncryptedEventStore.remove).not.toHaveBeenCalled();

    await bridge.retryPendingApprovalDecryptions(client);
    expect(bridge.onRoomMessage).toHaveBeenCalledOnce();
    expect(bridge.onRoomMessage).toHaveBeenCalledWith(roomId, clearEvent);
    expect(bridge.agentOpsEncryptedEnvelopes.get(encrypted.event_id)).toMatchObject({
      roomId,
      sender: approval.owner_mxid,
      deviceId: 'OWNERDEVICE',
      senderKey: 'owner-curve25519-key',
    });
    expect(pendingEncryptedEventStore.remove).toHaveBeenCalledWith(encrypted.event_id);
    expect(records.size).toBe(0);
  });

  test('approval_requested only wakes canonical durable publication', async () => {
    const bridge = new MatrixBridge();
    bridge.wakeApprovalProjectionWorker = vi.fn(async () => ({}));
    bridge.botClient = { sendMessage: vi.fn() };
    bridge.sendAsAgentContent = vi.fn();

    await expect(bridge.onApprovalRequested({ request_id: approval.id }))
      .resolves.toEqual({ ok: true, requestId: approval.id, queued: true });
    expect(bridge.wakeApprovalProjectionWorker).toHaveBeenCalledOnce();
    expect(bridge.botClient.sendMessage).not.toHaveBeenCalled();
    expect(bridge.sendAsAgentContent).not.toHaveBeenCalled();
  });

  test('plaintext approval diagnostics require explicit non-production opt-in', () => {
    expect(resolveApprovalDmMode({})).toBe('required');
    expect(() => resolveApprovalDmMode({
      HAGENCY_APPROVAL_DM_MODE: 'plaintext-test',
    })).toThrow('HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST=1');
    expect(() => resolveApprovalDmMode({
      HAGENCY_APPROVAL_DM_MODE: 'plaintext-test',
      HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST: '1',
      NODE_ENV: 'production',
    })).toThrow('forbidden in production');
    expect(resolveApprovalDmMode({
      HAGENCY_APPROVAL_DM_MODE: 'plaintext-test',
      HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST: '1',
      NODE_ENV: 'test',
    })).toBe('plaintext-test');
  });

  test('plaintext approval diagnostic accepts only a room without encryption state', async () => {
    const bridge = new MatrixBridge({ approvalDmMode: 'plaintext-test' });
    bridge.botClient = {
      getRoomStateEvent: vi.fn().mockRejectedValue({
        errcode: 'M_NOT_FOUND',
        message: 'state event not found',
      }),
    };

    await expect(bridge.ensureApprovalDmSecurity('!plaintext-approval:palpo.test'))
      .resolves.toBe(true);

    bridge.botClient.getRoomStateEvent.mockResolvedValue({
      algorithm: 'm.megolm.v1.aes-sha2',
    });
    await expect(bridge.ensureApprovalDmSecurity('!encrypted-approval:palpo.test'))
      .rejects.toThrow('already encrypted');
  });

  test('plaintext approval diagnostic room name remains a private control room', () => {
    expect(isPrivateControlRoomName('Approval Test (UNENCRYPTED): wf_coordinator')).toBe(true);
    expect(isPrivateControlRoomName('Approval: wf_coordinator')).toBe(true);
    expect(isPrivateControlRoomName('robrix2-board')).toBe(false);
  });

  test('approval room cannot publish without E2EE support', async () => {
    const bridge = new MatrixBridge();
    bridge.botClient = {};
    await expect(bridge.ensureApprovalDmEncrypted('!approval-dm:palpo.test'))
      .rejects.toThrow('E2EE is unavailable');
  });

  test('approval room membership changes are bridge-only', async () => {
    const botUserId = '@agent-bridge:palpo.test';
    const powerLevels = approvalRoomPowerLevels(botUserId);

    expect(powerLevels).toMatchObject({
      invite: 100,
      kick: 100,
      ban: 100,
      redact: 100,
      state_default: 100,
      events_default: 0,
      users_default: 0,
      users: { [botUserId]: 100 },
    });
    expect(Object.keys(powerLevels.users)).toEqual([botUserId]);
  });

  test('existing approval rooms are upgraded to bridge-only membership', async () => {
    const bridge = new MatrixBridge();
    bridge.botUserId = '@agent-bridge:palpo.test';
    bridge.botClient = {
      getRoomStateEvent: vi.fn().mockResolvedValue({
        invite: 0,
        users: { '@agent-bridge:palpo.test': 100 },
        users_default: 0,
      }),
      sendStateEvent: vi.fn().mockResolvedValue('$power-levels'),
    };

    await expect(bridge.ensureApprovalDmRestricted('!approval-dm:palpo.test'))
      .resolves.toBe(true);
    expect(bridge.botClient.sendStateEvent).toHaveBeenCalledWith(
      '!approval-dm:palpo.test',
      'm.room.power_levels',
      '',
      approvalRoomPowerLevels('@agent-bridge:palpo.test'),
    );
  });
});
