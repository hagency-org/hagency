/*
 * 项目方 — the record that dissolves the chicken-and-egg (ADR-016 decision 1).
 *
 * An operator's objection is the whole reason this store exists: an agent MXID contains a server
 * name, so minting `@ac_biglittle:<our server>` at startup decided the homeserver before any project
 * was known. Their words: 「所以你先创建了 biglittle 的 matrix id 是错的」. The project side is what
 * becomes known first.
 *
 * THE HEAVIEST TESTS HERE ARE ABOUT WHAT MUST NOT COME BACK OUT. This store holds an `as_token`
 * that grants a whole namespace on someone else's homeserver, and decision 8 makes the credential
 * write-only. A leak through any projection is not a cosmetic defect: the console renders whatever
 * an API returns, and this repository has already shipped two cases of API text reaching a UI that
 * nobody intended to show it.
 */

import { afterEach, describe, expect, test } from 'vitest';
import { mkdtempSync, rmSync, statSync, writeFileSync, readFileSync } from 'fs';
import { tmpdir } from 'os';
import path from 'path';
import {
  ProjectSideStore,
  ProjectSideStoreError,
  createProjectSideStore,
  CREDENTIAL_KINDS,
  ACCESS_STATES,
} from '../lib/project-side-store.js';

const SERVER = 'palpo.test';
const API = 'http://127.0.0.1:8008';
const AS_TOKEN = 'as_secret_do_not_leak_0123456789';
const HS_TOKEN = 'hs_secret_do_not_leak_9876543210';
const REG_TOKEN = 'reg_secret_do_not_leak_abcdefgh';

let dir;
afterEach(() => { if (dir) rmSync(dir, { recursive: true, force: true }); dir = null; });

function store(options = {}) {
  dir = mkdtempSync(path.join(tmpdir(), 'project-side-'));
  return new ProjectSideStore(path.join(dir, 'project-sides.json'), options);
}

const appserviceCred = () => ({
  kind: 'appservice',
  asToken: AS_TOKEN,
  hsToken: HS_TOKEN,
  namespace: '@ac_.*',
  senderLocalpart: 'hagency',
});

const regTokenCred = () => ({
  kind: 'registrationToken',
  registrationToken: REG_TOKEN,
  representativeToken: null,
});

const side = (s, credential = appserviceCred()) => s.upsertSide({
  server_name: SERVER, api_base_url: API, label: 'Acme Corp', credential,
});

/** Every secret this test file knows about, so a leak check cannot be narrower than the secrets. */
const SECRETS = [AS_TOKEN, HS_TOKEN, REG_TOKEN];

function expectNoSecrets(value, what) {
  const serialized = JSON.stringify(value ?? null);
  for (const secret of SECRETS) {
    expect(serialized, `${what} leaked a secret`).not.toContain(secret);
  }
}

describe('the credential never comes back out', () => {
  test('THE CASE THAT MATTERS: no projection returns a credential, on any method', () => {
    /*
     * Enumerated rather than spot-checked, because a leak needs only one unguarded return. The
     * projection is an allow-list (`publicSide`) precisely so that adding a field cannot silently
     * add a leak — this test is what makes that claim checkable.
     */
    const s = store();
    expectNoSecrets(side(s), 'upsertSide');
    expectNoSecrets(s.getSide(SERVER), 'getSide');
    expectNoSecrets(s.listSides(), 'listSides');
    expectNoSecrets(s.setCredential(SERVER, regTokenCred()), 'setCredential');
    expectNoSecrets(s.observeAccess(SERVER, { state: 'accepted' }), 'observeAccess');
    expectNoSecrets(s.setRepresentative(SERVER, { mxid: `@hagency:${SERVER}` }), 'setRepresentative');
    expectNoSecrets(s.deactivateSide(SERVER), 'deactivateSide');
    expectNoSecrets(s.reactivateSide(SERVER), 'reactivateSide');
    expectNoSecrets(s.listAudit(), 'listAudit');
    expectNoSecrets(s.deactivateSide(SERVER) && s.removeSide(SERVER), 'removeSide');
  });

  test('the audit trail records that a credential changed, never what it changed to', () => {
    // An audit entry quoting a secret is a second copy of it with a longer retention than the
    // record — and the audit is the one structure here that is append-only.
    const s = store();
    side(s);
    s.setCredential(SERVER, regTokenCred());
    const audit = s.listAudit();
    expectNoSecrets(audit, 'audit');
    // It must still be USEFUL: an operator has to be able to see that a change happened.
    expect(audit.some((e) => e.type === 'credential_set')).toBe(true);
    expect(audit.some((e) => e.credentialChanged === true || e.type === 'credential_set')).toBe(true);
  });

  test('the kind IS exposed, because an operator needs it to read the fleet', () => {
    /*
     * Not an oversight in the projection. An appservice side's agents hold no individual tokens at
     * all, so "this agent has no credential" is correct there and alarming anywhere else. Hiding the
     * kind would make that indistinguishable from a provisioning failure.
     */
    const s = store();
    const rec = side(s);
    expect(rec.credentialKind).toBe('appservice');
    expect(rec.hasCredential).toBe(true);
  });

  test('credentialFor is the only way to obtain it, and it returns a copy', () => {
    const s = store();
    side(s);
    const got = s.credentialFor(SERVER);
    expect(got.asToken).toBe(AS_TOKEN);
    // A copy: a caller mutating what it received must not rewrite the stored credential.
    got.asToken = 'tampered';
    expect(s.credentialFor(SERVER).asToken).toBe(AS_TOKEN);
  });

  test('the projection carries no key matching /credential/ that holds state', () => {
    /*
     * ADR-014 decision 6 discovered that the health writer's redaction guard DROPS any key matching
     * /credential/, silently — it had to rename `agentsMissingCredential` to `unprovisionedAgents`
     * after a field vanished from the health record and read as "never observed". So the verdict
     * field is `accessState`, and this test fails if someone renames it to the obvious thing.
     */
    const s = store();
    const rec = side(s);
    expect(rec).toHaveProperty('accessState');
    const stateful = Object.keys(rec).filter((k) => /credential/i.test(k));
    // `credentialKind` and `hasCredential` are booleans/enums an operator reads, not state a
    // health record needs to carry. Nothing else may match.
    expect(stateful.sort()).toEqual(['credentialKind', 'hasCredential']);
  });
});

describe('one side per homeserver', () => {
  test('the id IS the server name, so two records cannot claim one homeserver', () => {
    const s = store();
    side(s);
    s.upsertSide({ server_name: SERVER, api_base_url: 'http://other:8008', label: 'Renamed' });
    expect(s.listSides()).toHaveLength(1);
    expect(s.getSide(SERVER).label).toBe('Renamed');
  });

  test('case does not create a second side', () => {
    // `Palpo.test` and `palpo.test` are one homeserver. Two records would mean two credentials for
    // one namespace, which is the invariant this store exists to hold.
    const s = store();
    side(s);
    s.upsertSide({ server_name: 'PALPO.TEST', api_base_url: API });
    expect(s.listSides()).toHaveLength(1);
    expect(s.getSide('Palpo.Test').id).toBe(SERVER);
  });

  test('a URL is refused as a server name', () => {
    /*
     * The conflation this rejects is load-bearing: a server name is an IDENTITY component — it is
     * the part after the colon in every MXID this side mints — while the API base URL is a network
     * location `.well-known` may delegate elsewhere. One field for both makes "which homeserver"
     * unanswerable.
     */
    const s = store();
    for (const bad of ['http://palpo.test', 'https://palpo.test/', 'palpo.test/path', '@palpo.test']) {
      expect(() => s.upsertSide({ server_name: bad, api_base_url: API }), bad)
        .toThrow(/server_name must be a Matrix server name/);
    }
  });

  test('an IP literal with a port is accepted, because that is this deployment', () => {
    // The running deployment's MATRIX_SERVER_NAME is `palpo.test` against `127.0.0.1:8008`; a
    // validator that rejected host:port would refuse the only real configuration on hand.
    const s = store();
    expect(s.upsertSide({ server_name: '127.0.0.1:8008', api_base_url: API }).id).toBe('127.0.0.1:8008');
  });
});

describe('both credential kinds, because appservice is mandatory', () => {
  test('the two kinds are the ones ADR-016 decision 2 names', () => {
    expect(CREDENTIAL_KINDS.sort()).toEqual(['appservice', 'registrationToken']);
  });

  test('an appservice credential requires all four of its fields', () => {
    // Refused at write time rather than producing a 401 at use time: a missing `hsToken` means the
    // homeserver's pushes get rejected, which surfaces as silence rather than as an error.
    const s = store();
    for (const missing of ['asToken', 'hsToken', 'namespace', 'senderLocalpart']) {
      const cred = appserviceCred();
      delete cred[missing];
      expect(() => s.upsertSide({ server_name: SERVER, api_base_url: API, credential: cred }), missing)
        .toThrow(new RegExp(`credential.${missing}`));
    }
  });

  test('a registration-token credential may have no representative token yet', () => {
    // The representative's own token is obtained by registering it, which happens after the side
    // exists. Requiring it up front would reintroduce an ordering problem this ADR removes.
    const s = store();
    const rec = side(s, regTokenCred());
    expect(rec.credentialKind).toBe('registrationToken');
    expect(s.credentialFor(SERVER).representativeToken).toBeNull();
  });

  test('an unknown kind is refused', () => {
    const s = store();
    expect(() => s.upsertSide({
      server_name: SERVER, api_base_url: API, credential: { kind: 'password', secret: 'x' },
    })).toThrow(/credential.kind must be one of/);
  });
});

describe('a credential that cannot be re-minted is not lost by accident', () => {
  test('a corrupt file THROWS rather than starting empty', () => {
    /*
     * ADR-014's most expensive lesson, and sharper here. There, `loadState` treated every read
     * failure as "start empty" and startup persisted that over the file — survivable only while
     * credentials could be re-derived from a master secret. A project side's credential was issued
     * by someone else's homeserver: nothing in this software can re-mint it, so starting empty and
     * saving over the file destroys something only the project side can replace.
     */
    dir = mkdtempSync(path.join(tmpdir(), 'project-side-'));
    const file = path.join(dir, 'project-sides.json');
    writeFileSync(file, '{"sides": {"palpo.test": {broken');
    expect(() => new ProjectSideStore(file)).toThrow(ProjectSideStoreError);
    expect(() => new ProjectSideStore(file)).toThrow(/failed to load project side store/);
    // And the file is still there, unmodified — the throw happened before any write.
    expect(readFileSync(file, 'utf8')).toContain('broken');
  });

  test('an update that omits the credential carries it forward', () => {
    /*
     * The console can only WRITE this field, so a form that saves a label round-trips a record with
     * no credential in it. Treating that as "clear the credential" would erase it on every edit, and
     * recovering needs an action on the project side.
     */
    const s = store();
    side(s);
    s.upsertSide({ server_name: SERVER, api_base_url: API, label: 'Acme Renamed' });
    expect(s.credentialFor(SERVER).asToken).toBe(AS_TOKEN);
    expect(s.getSide(SERVER).hasCredential).toBe(true);
  });

  test('an explicit null clears it, so there is still a way to mean it', () => {
    const s = store();
    side(s);
    s.upsertSide({ server_name: SERVER, api_base_url: API, credential: null });
    expect(s.credentialFor(SERVER)).toBeNull();
    expect(s.getSide(SERVER).hasCredential).toBe(false);
  });

  test('the file is 0600', () => {
    // It holds an as_token granting a whole namespace on a homeserver we do not administer.
    const s = store();
    side(s);
    expect(statSync(s.filePath).mode & 0o777).toBe(0o600);
  });
});

describe('the access verdict is a verdict, with an age', () => {
  test('a fresh side is unverified rather than accepted', () => {
    const s = store();
    const rec = side(s);
    expect(rec.accessState).toBe('unverified');
    expect(rec.accessCheckedAt).toBeNull();
  });

  test('an observation carries the time it was taken', () => {
    // "accepted" with no age cannot be told apart from "accepted once, months ago, by a bridge that
    // has since stopped running" — the argument that put membershipCheckedAt beside agentJoined.
    const s = store({ now: () => 1_700_000_000_000 });
    side(s);
    const rec = s.observeAccess(SERVER, { state: 'accepted' });
    expect(rec.accessState).toBe('accepted');
    expect(rec.accessCheckedAt).toBe(1_700_000_000_000);
  });

  test('rejected and unreachable are DIFFERENT states', () => {
    /*
     * ADR-014 decision 6's distinction: only a 401/403 is a verdict on the token. Reading an outage
     * as revocation sends an operator to ask a project side for a new credential when the one they
     * have is fine.
     */
    expect(ACCESS_STATES).toContain('rejected');
    expect(ACCESS_STATES).toContain('unreachable');
    const s = store();
    side(s);
    expect(s.observeAccess(SERVER, { state: 'unreachable', detail: 'ECONNREFUSED' }).accessDetail)
      .toBe('ECONNREFUSED');
  });

  test('a state outside the enum is refused, so "probably fine" cannot be stored', () => {
    /*
     * Split by REASON rather than asserted with one pattern. An empty state is rejected for being
     * empty, before the enum is consulted at all — the first version of this test demanded the enum
     * message for `''` too and failed, which is the test being wrong about the code rather than the
     * code being wrong. Both are refusals; they are not the same refusal, and a test that blurs them
     * would keep passing if the enum check were deleted.
     */
    const s = store();
    side(s);
    for (const bad of ['ok', 'true', 'ACCEPTED', 'probably fine']) {
      expect(() => s.observeAccess(SERVER, { state: bad }), bad).toThrow(/state must be one of/);
    }
    for (const empty of ['', '   ', undefined, null]) {
      expect(() => s.observeAccess(SERVER, { state: empty }), String(empty))
        .toThrow(/state must be 1\.\.32 characters/);
    }
    // Surrounding whitespace is NORMALIZED, not rejected — `text()` trims before validating. Pinned
    // because my first version of this test listed `'accepted '` as invalid and failed: trimming is
    // deliberate, and a reader needs to see which of the two it is.
    expect(s.observeAccess(SERVER, { state: '  accepted  ' }).accessState).toBe('accepted');
  });

  test('CHANGING the credential invalidates the verdict', () => {
    /*
     * Otherwise the record reports a verdict about a value it no longer holds, which would hide a
     * paste error behind an old success — the same defect class as a stale membership flag.
     */
    const s = store();
    side(s);
    s.observeAccess(SERVER, { state: 'accepted' });
    expect(s.setCredential(SERVER, regTokenCred()).accessState).toBe('unverified');
  });

  test('changing the credential THROUGH upsertSide also invalidates the verdict', () => {
    /*
     * Found by mutation testing: neutering `if (credentialChanged)` in `upsertSide` survived,
     * because the existing test exercised `setCredential`, which resets the verdict in its own code
     * path. `upsertSide` is the route a console "save" takes, so the untested branch was the more
     * likely one.
     */
    const s = store();
    side(s);
    s.observeAccess(SERVER, { state: 'accepted' });
    const rec = s.upsertSide({
      server_name: SERVER, api_base_url: API, credential: regTokenCred(),
    });
    expect(rec.accessState).toBe('unverified');
    expect(rec.accessCheckedAt).toBeNull();
  });

  test('a side created with NO credential is unverified, not accepted', () => {
    /*
     * Also from mutation testing: defaulting `accessState` to 'accepted' survived every test,
     * because a side created WITH a credential has that default overwritten by the
     * credential-changed branch. Without one, the default stands — and the record would announce a
     * verdict about a credential that does not exist, which is the exact "claims something nobody
     * checked" defect this store's verdict field exists to prevent.
     */
    const s = store();
    const rec = s.upsertSide({ server_name: SERVER, api_base_url: API });
    expect(rec.hasCredential).toBe(false);
    expect(rec.accessState).toBe('unverified');
  });

  test('an unrelated update does NOT invalidate it', () => {
    // Invalidating on every save would make the verdict flicker and read as a failing credential.
    const s = store();
    side(s);
    s.observeAccess(SERVER, { state: 'accepted' });
    expect(s.upsertSide({ server_name: SERVER, api_base_url: API, label: 'X' }).accessState)
      .toBe('accepted');
  });
});

describe('the representative belongs to the side', () => {
  test('its MXID is stored as discovered, not composed', () => {
    // ADR-014 decision 5: composing @localpart:server is wrong the moment the account lives on a
    // server we do not control. The authoritative answer comes from /whoami.
    const s = store();
    side(s);
    const rec = s.setRepresentative(SERVER, { mxid: `@hagency:${SERVER}` });
    expect(rec.representative.mxid).toBe(`@hagency:${SERVER}`);
    expect(rec.representative.localpart).toBe('hagency');
  });

  test("THE CASE THAT MATTERS: a representative on ANOTHER server is refused", () => {
    /*
     * This is the federation assumption trying to come back in. ADR-016 decision 2 assumes servers
     * do NOT federate, so a representative whose identity lives elsewhere cannot act on the server
     * this side claims to be registered with — and the failure would be silent: a side that looks
     * configured and can do nothing.
     */
    const s = store();
    side(s);
    expect(() => s.setRepresentative(SERVER, { mxid: '@hagency:someone-else.example' }))
      .toThrow(/must live on palpo\.test/);
  });

  test('a malformed mxid is refused', () => {
    const s = store();
    side(s);
    for (const bad of ['hagency', '@hagency', 'hagency:palpo.test', '']) {
      expect(() => s.setRepresentative(SERVER, { mxid: bad }), bad).toThrow(ProjectSideStoreError);
    }
  });
});

describe('removal is the last step of a cascade, never the first', () => {
  test('an ACTIVE side refuses to be removed', () => {
    /*
     * This store cannot verify the earlier steps ran — it does not know about engagements or
     * bindings, deliberately. What it can do is refuse to be the accidental FIRST step, because
     * forgetting the credential first orphans everything that needed it: leaving rooms, revoking
     * tokens, telling the borrower.
     */
    const s = store();
    side(s);
    expect(() => s.removeSide(SERVER)).toThrow(/still active/);
    expect(s.getSide(SERVER)).not.toBeNull();
  });

  test('deactivating first allows removal', () => {
    const s = store();
    side(s);
    s.deactivateSide(SERVER);
    expect(s.removeSide(SERVER).id).toBe(SERVER);
    expect(s.getSide(SERVER)).toBeNull();
  });

  test('force removes an active side, and says so in the audit', () => {
    // The shape is taken from DELETE /api/agents/:name, which already answers "unregister is
    // disabled; agent marked inactive. Use ?force=true" — a pattern this codebase had before the
    // ADR proposed it.
    const s = store();
    side(s);
    expect(s.removeSide(SERVER, { force: true }).id).toBe(SERVER);
    expect(s.listAudit().some((e) => e.type === 'side_removed' && e.forced === true)).toBe(true);
  });

  test('deactivation does not forget the credential', () => {
    // The whole point of the reversible step: the caller still needs the credential to leave rooms
    // and revoke tokens AFTER closing the side to new work.
    const s = store();
    side(s);
    s.deactivateSide(SERVER);
    expect(s.credentialFor(SERVER).asToken).toBe(AS_TOKEN);
  });

  test('removing an unknown side returns null rather than throwing', () => {
    const s = store();
    expect(s.removeSide('never-configured.example')).toBeNull();
  });

  test('an inactive side is excluded from an activeOnly listing but still listed by default', () => {
    const s = store();
    side(s);
    s.deactivateSide(SERVER);
    expect(s.listSides({ activeOnly: true })).toHaveLength(0);
    expect(s.listSides()).toHaveLength(1);
  });
});

describe('persistence round-trips', () => {
  test('a reopened store keeps the credential and the verdict', () => {
    dir = mkdtempSync(path.join(tmpdir(), 'project-side-'));
    const file = path.join(dir, 'project-sides.json');
    const a = new ProjectSideStore(file);
    a.upsertSide({ server_name: SERVER, api_base_url: API, credential: appserviceCred() });
    a.observeAccess(SERVER, { state: 'accepted' });
    a.setRepresentative(SERVER, { mxid: `@hagency:${SERVER}` });

    const b = new ProjectSideStore(file);
    expect(b.credentialFor(SERVER).asToken).toBe(AS_TOKEN);
    expect(b.getSide(SERVER).accessState).toBe('accepted');
    expect(b.getSide(SERVER).representative.mxid).toBe(`@hagency:${SERVER}`);
  });

  test('the factory and the class agree', () => {
    dir = mkdtempSync(path.join(tmpdir(), 'project-side-'));
    const s = createProjectSideStore(path.join(dir, 'project-sides.json'));
    expect(s).toBeInstanceOf(ProjectSideStore);
  });

  test('getSide tolerates a malformed id rather than throwing at a read', () => {
    // A read path that throws on bad input turns a 404 into a 500 for any handler that passes a
    // path parameter straight through.
    const s = store();
    expect(s.getSide('http://not-a-server-name')).toBeNull();
    expect(s.getSide('')).toBeNull();
    expect(s.getSide(undefined)).toBeNull();
  });
});

describe('staging a credential, and the guard the endpoint makes unreachable', () => {
  /*
   * WHY STAGING EXISTS. Issuing REPLACED the live credential, so an operator clicking "generate" on a working
   * side broke Hagency's own outbound auth before they could install the new file. It happened for real: the
   * fleet held one token, the homeserver still had the previous one, and the operator was handed a repair job
   * for a state Hagency had created. 「为啥生成接单员之后还需要用户去做这些琐事」 — the chore was the symptom.
   *
   * These live HERE rather than only against the API because one guard is unreachable from the endpoint: it
   * passes `stage: true` only when a live credential exists, so `record.credential &&` inside the store can be
   * deleted without any API test noticing. Found by mutation, recorded in the store's comment, and tested
   * where it lives.
   */
  const cred = (asToken) => ({
    kind: 'appservice', asToken, hsToken: `${asToken}-hs`, namespace: '@ac_.*', senderLocalpart: 'hagency',
  });

  test('staging over a live credential leaves the live one in use', () => {
    const s = store();
    side(s, cred('live'));
    s.setCredential(SERVER, cred('next'), { stage: true });
    expect(s.credentialFor(SERVER).asToken).toBe('live');
    expect(s.pendingCredentialFor(SERVER).asToken).toBe('next');
  });

  test('staging a FIRST credential makes it live instead of parking it', () => {
    /*
     * THE UNREACHABLE GUARD. Parked, the side would hold a credential it will not use while `hasCredential`
     * reported true — selectable for work and unable to do any.
     */
    const s = store();
    s.upsertSide({ server_name: SERVER, api_base_url: API });
    s.setCredential(SERVER, cred('first'), { stage: true });
    expect(s.credentialFor(SERVER).asToken).toBe('first');
    expect(s.pendingCredentialFor(SERVER)).toBeNull();
  });

  test('promotion swaps in the staged one and keeps no fallback', () => {
    // A homeserver honours one registration per id, so keeping the loser would keep a token that authorises
    // nothing — and two live credentials are two things that can each be revoked without the other noticing.
    const s = store();
    side(s, cred('live'));
    s.setCredential(SERVER, cred('next'), { stage: true });
    s.promotePendingCredential(SERVER);
    expect(s.credentialFor(SERVER).asToken).toBe('next');
    expect(s.pendingCredentialFor(SERVER)).toBeNull();
  });

  test('promoting with nothing staged is a no-op, not a wipe', () => {
    // Called on every verify, so "nothing staged" is the common case and must not disturb what works.
    const s = store();
    side(s, cred('live'));
    expect(s.promotePendingCredential(SERVER)).toBeNull();
    expect(s.credentialFor(SERVER).asToken).toBe('live');
  });

  test('withdrawing clears the staged one as well', () => {
    const s = store();
    side(s, cred('live'));
    s.setCredential(SERVER, cred('next'), { stage: true });
    s.setCredential(SERVER, null);
    expect(s.credentialFor(SERVER)).toBeNull();
    expect(s.pendingCredentialFor(SERVER)).toBeNull();
  });

  test('replacing without staging drops any staged one', () => {
    // An explicit replace is a decision; a stale pending left behind it could later be promoted over the
    // credential the operator had just chosen.
    const s = store();
    side(s, cred('live'));
    s.setCredential(SERVER, cred('next'), { stage: true });
    s.setCredential(SERVER, cred('forced'));
    expect(s.credentialFor(SERVER).asToken).toBe('forced');
    expect(s.pendingCredentialFor(SERVER)).toBeNull();
  });

  test('outbound generation follows the credential that authorizes Matrix sends', () => {
    const s = store();
    side(s, cred('live'));
    const first = s.credentialFor(SERVER).outboundGeneration;
    expect(first).toMatch(/^[a-f0-9]{32}$/);
    s.setCredential(SERVER, cred('live'));
    expect(s.credentialFor(SERVER).outboundGeneration).toBe(first);
    s.setCredential(SERVER, cred('rotated'));
    expect(s.credentialFor(SERVER).outboundGeneration).not.toBe(first);
  });

  test('registration representative generation is unavailable until the actual send token exists', () => {
    const s = store();
    side(s, regTokenCred());
    expect(s.credentialFor(SERVER).outboundGeneration).toBeNull();
    s.setCredential(SERVER, { ...regTokenCred(), representativeToken: 'representative-live-token' });
    const first = s.credentialFor(SERVER).outboundGeneration;
    expect(first).toMatch(/^[a-f0-9]{32}$/);
    s.setCredential(SERVER, { ...regTokenCred(), representativeToken: 'representative-live-token' });
    expect(s.credentialFor(SERVER).outboundGeneration).toBe(first);
  });

  test('the staged flag is named without the word credential, and that is load-bearing', () => {
    /*
     * The health writer's redaction guard silently DROPS any key matching /credential/ (ADR-014 decision 6).
     * The projection test above enforces it and caught `hasPendingCredential` on the way in — so the field is
     * `awaitingInstall`, which is also the truer name: the credential is issued, the INSTALL is what is
     * pending, and it happens on somebody else's machine.
     */
    const s = store();
    const rec = side(s, cred('live'));
    expect(rec).toHaveProperty('awaitingInstall');
    expect(Object.keys(rec).filter((k) => /pending/i.test(k) && /credential/i.test(k))).toEqual([]);
  });
});

describe('a project accepts the spelling its own output uses', () => {
  /*
   * FOUND BY WALKING THE FLOW. `upsertProject` read `room_id` and `roomId` but not `room` — and `room` is what
   * the record is CALLED when you read it back. So a caller who looked at the output and sent it back got
   * `ok: true` and a project with no room.
   *
   * AND MY FIRST DIAGNOSIS OF IT WAS WRONG, which is worth keeping. Walking the flow I read `room=None` off the
   * response and concluded the field had been dropped — but the record calls it `roomId`, so I had read a name
   * that does not exist. The real gap was narrower: `room` was not an accepted INPUT spelling, while being the
   * obvious one for anyone who saw `roomId` and typed the short form.
   *
   * Three writes found this way genuinely did drop what they were sent — `projectSide` on PATCH /api/agents,
   * `allocation` on the allocation route, a numeric `ceiling` on a preset. This one was half that and half my
   * own misreading, and the round-trip test below is the version that would have told me which.
   */
  const ROOM = '!abc:palpo.test';

  test.each(['room', 'room_id', 'roomId'])('%s is accepted', (key) => {
    const s = store();
    side(s);
    const project = s.upsertProject(SERVER, { name: 'a project', [key]: ROOM });
    expect(project.roomId).toBe(ROOM);
  });

  test('the room comes back under the name it can be sent under', () => {
    // The round trip is the property: read a project, send it back, get the same project.
    const s = store();
    side(s);
    const first = s.upsertProject(SERVER, { name: 'round trip', room: ROOM });
    const again = s.upsertProject(SERVER, { ...first });
    expect(again.roomId).toBe(ROOM);
  });

  test('omitting the room does not invent one', () => {
    const s = store();
    side(s);
    expect(s.upsertProject(SERVER, { name: 'no room' }).roomId ?? null).toBeNull();
  });
});

describe('the callback address a reissue must reuse', () => {
  /*
   * FOUND BY ADDING A BUTTON. Offering "reissue" needs the url the registration was built for — and nothing
   * recorded it. The url lives inside a YAML file on somebody else's machine, so a reissue had nothing to reuse,
   * and a reissue with the wrong url produces a registration that installs cleanly and receives nothing: the
   * documented silent failure, arriving from a button instead of a typo.
   *
   * `null` is a real state, not a gap to paper over: a project side who generated their own registration has the
   * url in their file and never told us. The console refuses to reissue then, rather than defaulting.
   */
  const withUrl = (url) => ({
    kind: 'appservice', asToken: AS_TOKEN, hsToken: HS_TOKEN,
    namespace: '@ac_.*', senderLocalpart: 'hagency', ...(url === undefined ? {} : { url }),
  });

  test('the url is remembered and exposed as an address, not withheld as a secret', () => {
    const s = store();
    side(s, withUrl('http://127.0.0.1:8095'));
    expect(s.getSide(SERVER).appserviceUrl).toBe('http://127.0.0.1:8095');
  });

  test('a credential with no url reports null rather than an empty string', () => {
    // The console branches on it. An empty string is truthy enough in the wrong places to reissue against "".
    const s = store();
    side(s, withUrl(undefined));
    expect(s.getSide(SERVER).appserviceUrl).toBeNull();
    side(s, withUrl(''));
    expect(s.getSide(SERVER).appserviceUrl).toBeNull();
  });

  test('the url survives a staged replacement, which is when a reissue happens', () => {
    // Staging writes a second credential; the live one's address must not be disturbed by it.
    const s = store();
    side(s, withUrl('http://first:8095'));
    s.setCredential(SERVER, withUrl('http://second:8095'), { stage: true });
    expect(s.getSide(SERVER).appserviceUrl).toBe('http://first:8095');
    s.promotePendingCredential(SERVER);
    expect(s.getSide(SERVER).appserviceUrl).toBe('http://second:8095');
  });

  test('the url is not a token, so it is allowed to be readable', () => {
    // The projection test elsewhere forbids readable secrets. This one states the opposite for an address, so a
    // future reader does not "fix" it by hiding it and breaking reissue.
    const s = store();
    const rec = side(s, withUrl('http://127.0.0.1:8095'));
    expect(rec.appserviceUrl).toBe('http://127.0.0.1:8095');
    expect(JSON.stringify(rec)).not.toContain(AS_TOKEN);
  });
});
