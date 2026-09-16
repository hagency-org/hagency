/*
 * A DELIVERED APPROVAL IS NOT A SEEN ONE.
 *
 * `onApprovalRequested` reports success on an event id. An event id proves a message landed in a room. It
 * does not prove that the human who has to decide is IN that room — and on a live rig those two came apart
 * in the most ordinary way imaginable: `bridge-state.json` held a `botDmRooms` entry for `@operator:…`
 * whose only joined member was the BOT. The operator had been invited and had never accepted. The room
 * existed, was recorded, and was exactly the room `HAGENCY_OWNER_DM_ROOM` would have been pointed at.
 *
 * Every approval sent there would have been delivered, reported delivered, and waited for a decision from
 * somebody who could not see it being asked for — until the request expired and was denied for timing out.
 * Nothing in the product checks this: `resolveOwnerFor` takes the mxid and the room id as given, and
 * `upsertBinding` requires both without checking that one is in the other. The backend cannot check, since
 * reading a room's membership needs a Matrix credential for a room usually on HAgency's own homeserver.
 *
 * So the check lives at the one place holding both the room and a credential for it, it runs AFTER the
 * send, and it never blocks one — a message keeps, and a human who joins later will read it. What was
 * missing was anybody being told.
 */
import { afterAll, beforeAll, beforeEach, describe, expect, test, vi } from 'vitest';
import os from 'os';
import path from 'path';
import { mkdtempSync, rmSync } from 'fs';
import { pathToFileURL } from 'url';
import { restoreEnv, snapshotEnv } from './helpers/env.js';

const joinedMembersOnSide = vi.fn();

vi.mock('../lib/matrix-representative.js', async (importOriginal) => {
  const actual = await importOriginal();
  return { ...actual, joinedMembersOnSide: (...args) => joinedMembersOnSide(...args) };
});

const OURS = 'hagency.test';
const OWNER = `@alex:${OURS}`;
const DM = `!owner-dm:${OURS}`;
const SIDE = 'customer.test';

const approvalOn = (roomId, owner = OWNER) => ({
  id: '$a-1', agent: 'wf_coordinator', owner_mxid: owner, owner_dm_room_id: roomId,
});
const serverOf = (roomId) => String(roomId ?? '').slice(String(roomId ?? '').indexOf(':') + 1).toLowerCase();

let bridgeModule;
let runtimeDir;
let envSnapshot;

beforeAll(async () => {
  envSnapshot = snapshotEnv(['HAGENCY_RUNTIME_DIR', 'MATRIX_SERVER_NAME']);
  runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'approval-owner-visible-'));
  process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
  process.env.MATRIX_SERVER_NAME = OURS;
  bridgeModule = await import(`${pathToFileURL(path.resolve('bridge-matrix.js')).href}?as-owner-visible`);
});

afterAll(() => {
  restoreEnv(envSnapshot);
  rmSync(runtimeDir, { recursive: true, force: true });
});

beforeEach(() => { joinedMembersOnSide.mockReset(); });

/** The minimum the check reads. `members` may be an array, or a function that throws. */
function selfWith({ members = [], sides = {} } = {}) {
  const it = {
    warnings: [],
    postWarning(message, meta) { this.warnings.push({ message, ...meta }); },
    actingSideFor: (id) => sides[String(id).toLowerCase()] ?? null,
    botClient: {
      getJoinedRoomMembers: async () => (typeof members === 'function' ? members() : members),
    },
  };
  return it;
}

const check = (it, approval) => bridgeModule.MatrixBridge.prototype.warnIfOwnerCannotSeeApprovalRoom
  .call(it, approval, serverOf(approval.owner_dm_room_id));

const said = (it) => it.warnings.map((w) => w.message).join(' ');

describe('the owner DM room on our own homeserver', () => {
  test('THE DEFECT: an owner who is not in the room is reported, with the remedy', async () => {
    const it = selfWith({ members: ['@hagency:hagency.test'] });
    await check(it, approvalOn(DM));

    expect(it.warnings).toHaveLength(1);
    expect(said(it)).toContain(OWNER);
    expect(said(it)).toContain(DM);
    // The two ways it happens, both worth naming: never accepted, or left afterwards.
    expect(said(it)).toMatch(/invited and never joined, or since departed/);
    expect(said(it)).toMatch(/HAGENCY_OWNER_DM_ROOM/);
    // Deduped per ROOM: twenty approvals against one bad room file one alert, not twenty.
    expect(it.warnings[0].scope).toBe(DM);
    expect(it.warnings[0].kind).toBe('approval-owner-absent');
  });

  test('an owner who IS in the room says nothing at all', async () => {
    const it = selfWith({ members: ['@hagency:hagency.test', OWNER] });
    await check(it, approvalOn(DM));
    expect(it.warnings).toEqual([]);
  });

  test('the mxid comparison is case-insensitive, because Matrix localparts are', async () => {
    // A false alarm here would train an operator to ignore the alarm that matters.
    const it = selfWith({ members: [`@ALEX:${OURS.toUpperCase()}`] });
    await check(it, approvalOn(DM));
    expect(it.warnings).toEqual([]);
  });

  test('AN UNREADABLE MEMBERSHIP SAYS NOTHING — "I could not ask" is not "the owner is absent"', async () => {
    const it = selfWith({ members: () => { throw new Error('M_FORBIDDEN'); } });
    await check(it, approvalOn(DM));
    expect(it.warnings).toEqual([]);
  });

  test('nor does a shape that is not a member list', async () => {
    const it = selfWith({ members: null });
    await check(it, approvalOn(DM));
    expect(it.warnings).toEqual([]);
  });

  test('with no bot at all it is skipped, not guessed', async () => {
    // #119's bot-less mode: the inbound path stays alive without a bot, and a check that needs one
    // must decline rather than report an absence it cannot see.
    const it = selfWith({ members: [] });
    it.botClient = null;
    await check(it, approvalOn(DM));
    expect(it.warnings).toEqual([]);
  });
});

describe('an owner DM room on a project side', () => {
  const acting = {
    side: { serverName: SIDE, apiBaseUrl: 'http://127.0.0.1:8008' },
    credential: { kind: 'appservice', asToken: 'as_secret_never_logged', senderLocalpart: 'hagency', namespace: '@ac_.*' },
  };
  const theirDm = `!owner-dm:${SIDE}`;
  const theirOwner = `@borrower:${SIDE}`;

  test('read with the SIDE\'s credential, because the bot has no account there', async () => {
    /*
     * ADR-016: an approval is the borrower's decision, so the room goes where the decider is — on their
     * homeserver, where `getJoinedRoomMembers` cannot reach. Asking with the bot would fail and the
     * check would silently never run.
     */
    const it = selfWith({ sides: { [SIDE]: acting } });
    joinedMembersOnSide.mockResolvedValue({ known: true, members: ['@hagency:customer.test'], reason: null });

    await check(it, approvalOn(theirDm, theirOwner));

    expect(joinedMembersOnSide).toHaveBeenCalledTimes(1);
    const [args] = joinedMembersOnSide.mock.calls[0];
    expect(args.credential).toEqual(acting.credential);
    expect(args.roomId).toBe(theirDm);
    expect(said(it)).toContain(theirOwner);
  });

  test('a borrower who is in their own room says nothing', async () => {
    const it = selfWith({ sides: { [SIDE]: acting } });
    joinedMembersOnSide.mockResolvedValue({ known: true, members: [theirOwner], reason: null });
    await check(it, approvalOn(theirDm, theirOwner));
    expect(it.warnings).toEqual([]);
  });

  test('`known: false` is silence, not an absence', async () => {
    const it = selfWith({ sides: { [SIDE]: acting } });
    joinedMembersOnSide.mockResolvedValue({ known: false, members: [], reason: 'membership unreadable: 502' });
    await check(it, approvalOn(theirDm, theirOwner));
    expect(it.warnings).toEqual([]);
  });

  test('a side we hold no credential for is skipped without asking', async () => {
    const it = selfWith({ sides: {} });
    await check(it, approvalOn(theirDm, theirOwner));
    expect(joinedMembersOnSide).not.toHaveBeenCalled();
    expect(it.warnings).toEqual([]);
  });
});

/*
 * AND THAT ANYTHING CALLS IT.
 *
 * A MUTATION SURVIVED without this. Deleting the call from `onApprovalRequested` left every test above
 * green, because they drive the method directly — the same hole `tests/join-backfill.test.js` documents
 * about itself, and the same one the appservice invite path fell through for months. A method that works
 * and is never invoked is indistinguishable from no method at all.
 */
describe('approval requested wake boundary', () => {
  test('does not run the old visibility or direct-publication pipeline', async () => {
    const bridge = new bridgeModule.MatrixBridge();
    bridge.wakeApprovalProjectionWorker = vi.fn(async () => ({}));
    bridge.warnIfOwnerCannotSeeApprovalRoom = vi.fn();
    bridge.botClient = { sendMessage: vi.fn() };
    bridge.sendAsAgentContent = vi.fn();

    await expect(bridge.onApprovalRequested({ request_id: '$approval-wired' }))
      .resolves.toEqual({ ok: true, requestId: '$approval-wired', queued: true });
    expect(bridge.wakeApprovalProjectionWorker).toHaveBeenCalledOnce();
    expect(bridge.warnIfOwnerCannotSeeApprovalRoom).not.toHaveBeenCalled();
    expect(bridge.botClient.sendMessage).not.toHaveBeenCalled();
    expect(bridge.sendAsAgentContent).not.toHaveBeenCalled();
  });
});

describe('what it refuses to do', () => {
  test('an approval naming no room or no owner is not a warning', async () => {
    const it = selfWith({ members: [] });
    await check(it, { ...approvalOn(DM), owner_dm_room_id: null });
    await check(it, { ...approvalOn(DM), owner_mxid: null });
    expect(it.warnings).toEqual([]);
  });

  test('IT NEVER BLOCKS THE DELIVERY IT IS CHECKING', async () => {
    /*
     * The ordering that matters. Refusing to deliver, or throwing out of this, would turn "the owner may
     * not see this" into "the owner definitely did not get it" — a strictly worse outcome, and one that
     * `onApprovalRequested` would fail closed on.
     */
    const it = selfWith({ members: () => { throw new Error('everything is on fire'); } });
    await expect(check(it, approvalOn(DM))).resolves.toBeUndefined();
  });
});
