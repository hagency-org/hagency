/*
 * 项目方 over HTTP — the operator's CRUD, and what must never come back through it.
 *
 * ADR-016 decisions 1, 3, 7 and 8. The operator asked for this surface directly: 「增加一个项目方的
 * section，里面可以 CRUD hagency 加入的项目方」, with the constraint that deleting a project side must
 * take its agents with it.
 *
 * THE HEAVIEST GROUP IS THE LEAK CHECK, and it is heaviest because of what the credential is: an
 * `as_token` grants a whole namespace on a homeserver Hagency does not administer. The console
 * renders whatever an API returns, and this repository has already shipped two cases of API text
 * reaching a UI nobody intended to show it. So every handler is checked, not a sample.
 *
 * The verify tests run against a FAKE HOMESERVER started in-process rather than a mocked module.
 * The point of `verify` is that it makes a real authenticated call and classifies what comes back, so
 * stubbing the call would test the parts that were never in doubt.
 */

import { afterEach, describe, expect, test, vi } from 'vitest';
import request from 'supertest';
import { createServer } from 'http';
import { readFileSync, writeFileSync } from 'fs';
import path from 'path';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';

const SERVER = 'palpo.test';
const AS_TOKEN = 'as_secret_must_not_leak_0123456789';
const HS_TOKEN = 'hs_secret_must_not_leak_9876543210';
const REG_TOKEN = 'reg_secret_must_not_leak_abcdefghij';
const SECRETS = [AS_TOKEN, HS_TOKEN, REG_TOKEN];

const asCred = () => ({
  kind: 'appservice',
  asToken: AS_TOKEN,
  hsToken: HS_TOKEN,
  namespace: '@ac_.*',
  senderLocalpart: 'hagency',
});

let context = null;
let fake = null;

afterEach(async () => {
  context?.cleanup();
  context = null;
  if (fake) {
    await new Promise((resolve) => fake.server.close(resolve));
    fake = null;
  }
});

async function boot(seed = {}) {
  context = await createBackendTestContext('api-project-sides-', { agents: {}, ...seed });
  return context.app;
}

/**
 * A homeserver that answers only what `ensureRepresentative` asks it.
 *
 * `handler` receives (path, searchParams, method, body) and returns [status, jsonBody]. Started on
 * an ephemeral port so several tests can run without colliding.
 */
async function fakeHomeserver(handler) {
  const calls = [];
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (c) => { body += c; });
    req.on('end', () => {
      const url = new URL(req.url, 'http://x');
      calls.push({ path: url.pathname, method: req.method, query: Object.fromEntries(url.searchParams), body });
      const [status, payload] = handler(url.pathname, url.searchParams, req.method, body);
      res.writeHead(status, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(payload));
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  fake = { server, calls, baseUrl: `http://127.0.0.1:${server.address().port}` };
  return fake;
}

function expectNoSecrets(response, what) {
  const serialized = JSON.stringify(response.body ?? null) + (response.text ?? '');
  for (const secret of SECRETS) {
    expect(serialized, `${what} leaked a secret`).not.toContain(secret);
  }
}

describe('the credential never comes back through the API', () => {
  test('THE CASE THAT MATTERS: no endpoint returns a credential', async () => {
    /*
     * Enumerated rather than sampled, because a leak needs only one unguarded handler. The store's
     * `publicSide` is an allow-list projection so this SHOULD hold by construction — this test is
     * what makes "by construction" checkable, and what fails if a handler ever answers with a record
     * it fetched some other way.
     */
    const app = await boot();
    const created = await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', label: 'Acme', credential: asCred() });
    expect(created.status).toBe(200);
    expectNoSecrets(created, 'POST /api/project-sides');

    expectNoSecrets(await request(app).get('/api/project-sides'), 'GET list');
    expectNoSecrets(await request(app).get(`/api/project-sides/${SERVER}`), 'GET one');
    expectNoSecrets(
      await request(app).put(`/api/project-sides/${SERVER}/credential`).send({ credential: asCred() }),
      'PUT credential',
    );
    expectNoSecrets(await request(app).post(`/api/project-sides/${SERVER}/verify`), 'POST verify');
    expectNoSecrets(await request(app).post(`/api/project-sides/${SERVER}/deactivate`), 'POST deactivate');
    expectNoSecrets(await request(app).post(`/api/project-sides/${SERVER}/reactivate`), 'POST reactivate');
    expectNoSecrets(await request(app).post(`/api/project-sides/${SERVER}/deactivate`), 'POST deactivate again');
    expectNoSecrets(await request(app).delete(`/api/project-sides/${SERVER}`), 'DELETE');
  });

  test('the KIND is exposed, because an operator needs it to read the fleet', async () => {
    // An appservice side's agents hold no individual credential, so "no credential" is correct there
    // and alarming anywhere else. Hiding the kind would make that look like a provisioning failure.
    const app = await boot();
    const r = await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', credential: asCred() });
    expect(r.body.side.credentialKind).toBe('appservice');
    expect(r.body.side.hasCredential).toBe(true);
    expect(r.body.side).not.toHaveProperty('credential');
  });
});

describe('one side per homeserver', () => {
  test('the id is the server name, and a second POST updates rather than duplicates', async () => {
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', label: 'First' });
    await request(app).post('/api/project-sides')
      .send({ server_name: 'PALPO.TEST', api_base_url: 'http://127.0.0.1:8008', label: 'Second' });
    const list = await request(app).get('/api/project-sides');
    expect(list.body.sides).toHaveLength(1);
    expect(list.body.sides[0].label).toBe('Second');
    expect(list.body.sides[0].id).toBe(SERVER);
  });

  test('a URL as server_name is a 400 with the reason', async () => {
    const app = await boot();
    const r = await request(app).post('/api/project-sides')
      .send({ server_name: 'http://palpo.test', api_base_url: 'http://127.0.0.1:8008' });
    expect(r.status).toBe(400);
    expect(r.body.code).toBe('bad_request');
    expect(r.body.error).toMatch(/server_name must be a Matrix server name/);
  });

  test('an unknown side is a 404, not an empty 200', async () => {
    const app = await boot();
    expect((await request(app).get('/api/project-sides/never.configured')).status).toBe(404);
    expect((await request(app).post('/api/project-sides/never.configured/verify')).status).toBe(404);
    expect((await request(app).put('/api/project-sides/never.configured/credential')
      .send({ credential: asCred() })).status).toBe(404);
    expect((await request(app).delete('/api/project-sides/never.configured')).status).toBe(404);
  });

  test('a server name WITH A PORT works as a path segment', async () => {
    /*
     * `127.0.0.1:8008` is a legal Matrix server name and is this deployment's actual API host, so it
     * will be a real id. It also travels as a path segment through the console proxy, which
     * percent-encodes each segment before forwarding — so the colon makes a round trip through
     * `encodeURIComponent` and Express's param decoding. A route that only ever saw dotted names
     * would break on the first side an operator actually adds, and the symptom would be a 404 on a
     * side the list shows.
     */
    const app = await boot();
    const id = '127.0.0.1:8008';
    const created = await request(app).post('/api/project-sides')
      .send({ server_name: id, api_base_url: 'http://127.0.0.1:8008' });
    expect(created.body.side.id).toBe(id);

    expect((await request(app).get(`/api/project-sides/${encodeURIComponent(id)}`)).status).toBe(200);
    expect((await request(app).post(`/api/project-sides/${encodeURIComponent(id)}/deactivate`)).status).toBe(200);
    expect((await request(app).delete(`/api/project-sides/${encodeURIComponent(id)}`)).status).toBe(200);
  });

  test('activeOnly filters, and the default does not', async () => {
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    await request(app).post(`/api/project-sides/${SERVER}/deactivate`);
    expect((await request(app).get('/api/project-sides?active=true')).body.sides).toHaveLength(0);
    expect((await request(app).get('/api/project-sides')).body.sides).toHaveLength(1);
  });
});

describe('an update that omits the credential does not erase it', () => {
  test('saving a label keeps the credential', async () => {
    /*
     * The console can only WRITE this field, so a form that round-trips a record has no credential in
     * it. If the handler treated absent as null, every label edit would destroy a credential that
     * only the project side can reissue.
     */
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', credential: asCred() });
    const renamed = await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', label: 'Renamed' });
    expect(renamed.body.side.hasCredential).toBe(true);
    expect(renamed.body.side.label).toBe('Renamed');
  });

  test('an explicit null clears it, so there is still a way to mean it', async () => {
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', credential: asCred() });
    const cleared = await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', credential: null });
    expect(cleared.body.side.hasCredential).toBe(false);
  });
});

describe('verify records a verdict, and which kind of verdict', () => {
  test('an appservice credential the server accepts is recorded with the representative MXID', async () => {
    const hs = await fakeHomeserver((path, q) => {
      if (path === '/_matrix/client/v3/account/whoami') {
        // Masqueraded: the AS acts as sender_localpart. The server's answer is authoritative.
        return [200, { user_id: `@hagency:${SERVER}`, device_id: 'appservice' }];
      }
      return [404, { errcode: 'M_UNRECOGNIZED' }];
    });
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: hs.baseUrl, credential: asCred() });

    const r = await request(app).post(`/api/project-sides/${SERVER}/verify`);
    expect(r.status).toBe(200);
    expect(r.body.side.accessState).toBe('accepted');
    expect(typeof r.body.side.accessCheckedAt).toBe('number');
    expect(r.body.side.representative.mxid).toBe(`@hagency:${SERVER}`);
    // The masquerade is the call being made, not an implementation detail: it proves the namespace
    // claim functions rather than merely that the token is known.
    expect(hs.calls[0].query.user_id).toBe(`@hagency:${SERVER}`);
  });

  test('a REJECTED credential is recorded as rejected, and the side stays active', async () => {
    // Reachability failing is not the contributor withdrawing anything, so a verdict must not
    // deactivate a side. Only the operator may do that.
    const hs = await fakeHomeserver(() => [403, { errcode: 'M_FORBIDDEN', error: 'bad token' }]);
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: hs.baseUrl, credential: asCred() });

    const r = await request(app).post(`/api/project-sides/${SERVER}/verify`);
    expect(r.status).toBe(200);
    expect(r.body.side.accessState).toBe('rejected');
    expect(r.body.side.active).toBe(true);
    expect(r.body.side.accessDetail).toMatch(/M_FORBIDDEN/);
  });

  test('AN UNREACHABLE HOMESERVER IS NOT A REJECTED CREDENTIAL', async () => {
    /*
     * The distinction that costs somebody else's afternoon when it is wrong. Port 1 on loopback
     * refuses the connection, which produces an error with no HTTP status at all — exactly the shape
     * that must classify as `unreachable` rather than as a verdict on the token.
     */
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:1', credential: asCred() });

    const r = await request(app).post(`/api/project-sides/${SERVER}/verify`);
    expect(r.status).toBe(200);
    expect(r.body.side.accessState).toBe('unreachable');
  });

  test('a side with no credential verifies to unverified rather than failing', async () => {
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    const r = await request(app).post(`/api/project-sides/${SERVER}/verify`);
    expect(r.status).toBe(200);
    expect(r.body.side.accessState).toBe('unverified');
  });

  test('a newly minted representative token is STORED, so the next verify needs no registration', async () => {
    /*
     * The registration-token path. The credential patch has to be persisted or every verify would
     * register again — and the second attempt fails, because the localpart is already taken.
     */
    let registrations = 0;
    const hs = await fakeHomeserver((path, q, method, body) => {
      if (path === '/_matrix/client/v3/register') {
        registrations += 1;
        const parsed = JSON.parse(body || '{}');
        if (!parsed.auth) return [401, { session: 'uia-1', flows: [{ stages: ['m.login.registration_token'] }] }];
        return [200, { access_token: 'minted-rep-token', user_id: `@hagency:${SERVER}` }];
      }
      if (path === '/_matrix/client/v3/account/whoami') {
        return [200, { user_id: `@hagency:${SERVER}`, device_id: 'D1' }];
      }
      return [404, { errcode: 'M_UNRECOGNIZED' }];
    });
    const app = await boot();
    await request(app).post('/api/project-sides').send({
      server_name: SERVER,
      api_base_url: hs.baseUrl,
      credential: { kind: 'registrationToken', registrationToken: REG_TOKEN, representativeToken: null },
    });

    const first = await request(app).post(`/api/project-sides/${SERVER}/verify`);
    expect(first.body.side.accessState).toBe('accepted');
    const afterFirst = registrations;
    expect(afterFirst).toBeGreaterThan(0);

    const second = await request(app).post(`/api/project-sides/${SERVER}/verify`);
    expect(second.body.side.accessState).toBe('accepted');
    // No further registration: the second verify validated the stored token instead.
    expect(registrations).toBe(afterFirst);
  });

  test('a representative on the WRONG server is refused, and the verdict is still recorded', async () => {
    /*
     * The federation assumption trying to come back in (decision 2 assumes servers do not federate).
     * The refusal must not discard the access verdict that was already written — an operator needs to
     * see both that the credential worked and that the identity it produced was unusable, because
     * those imply different fixes.
     */
    const hs = await fakeHomeserver(() => [200, { user_id: '@hagency:someone-else.example', device_id: 'D1' }]);
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: hs.baseUrl, credential: asCred() });

    const r = await request(app).post(`/api/project-sides/${SERVER}/verify`);
    expect(r.status).toBe(400);
    expect(r.body.code).toBe('representative_rejected');
    expect(r.body.error).toMatch(/must live on palpo\.test/);
    // The verdict survived the refusal.
    expect(r.body.side.accessState).toBe('accepted');
    expect(r.body.side.representative).toBeNull();
  });

  test('changing the credential invalidates a previous verdict', async () => {
    const hs = await fakeHomeserver(() => [200, { user_id: `@hagency:${SERVER}`, device_id: 'D1' }]);
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: hs.baseUrl, credential: asCred() });
    await request(app).post(`/api/project-sides/${SERVER}/verify`);

    const replaced = await request(app).put(`/api/project-sides/${SERVER}/credential`)
      .send({ credential: { kind: 'registrationToken', registrationToken: REG_TOKEN } });
    expect(replaced.body.side.accessState).toBe('unverified');
    expect(replaced.body.side.accessCheckedAt).toBeNull();
  });
});

describe('generating the appservice registration', () => {
  const create = (app) => request(app).post('/api/project-sides')
    .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', label: 'Acme' });

  test('THE ONLY CHANCE: the tokens appear in this response and never again', async () => {
    /*
     * ADR-016 decision 8 makes a project side's credential write-only, and this endpoint stores the
     * generated tokens into it. So the YAML in this response is the only readable copy — a GET
     * afterwards carries the KIND and nothing else. The response says so, because the recovery is
     * regenerating, which invalidates a registration the project side may already have installed.
     */
    const app = await boot();
    await create(app);
    const r = await request(app).post(`/api/project-sides/${SERVER}/registration`)
      .send({ url: 'http://host.docker.internal:8009' });
    expect(r.status).toBe(200);
    expect(r.body.registrationYaml).toMatch(/as_token: [0-9a-f]{64}/);
    expect(r.body.registrationYaml).toMatch(/hs_token: [0-9a-f]{64}/);
    expect(r.body.onlyChance).toMatch(/never be returned again/);

    // Extract what was issued, then prove no read path returns it.
    const asToken = /as_token: ([0-9a-f]{64})/.exec(r.body.registrationYaml)[1];
    const hsToken = /hs_token: ([0-9a-f]{64})/.exec(r.body.registrationYaml)[1];
    expect(asToken).not.toBe(hsToken);

    for (const [what, response] of [
      ['GET one', await request(app).get(`/api/project-sides/${SERVER}`)],
      ['GET list', await request(app).get('/api/project-sides')],
      ['verify', await request(app).post(`/api/project-sides/${SERVER}/verify`)],
    ]) {
      const body = JSON.stringify(response.body);
      expect(body, `${what} leaked as_token`).not.toContain(asToken);
      expect(body, `${what} leaked hs_token`).not.toContain(hsToken);
    }
  });

  test('the credential is stored as appservice kind, and the side knows it has one', async () => {
    const app = await boot();
    await create(app);
    const r = await request(app).post(`/api/project-sides/${SERVER}/registration`)
      .send({ url: 'http://host.docker.internal:8009' });
    expect(r.body.side.credentialKind).toBe('appservice');
    expect(r.body.side.hasCredential).toBe(true);
    // A fresh credential resets the verdict: nothing has verified these tokens yet.
    expect(r.body.side.accessState).toBe('unverified');
  });

  test('THE URL IS REQUIRED, because Hagency cannot know its own address', async () => {
    /*
     * A tunnel hostname, a public IP, `host.docker.internal` for a homeserver in a container on the
     * same machine — only the operator knows which. Guessing produces a registration that installs
     * cleanly and never delivers anything, which from the project side looks like an appservice that is
     * configured and silent: the hardest failure here to diagnose.
     */
    const app = await boot();
    await create(app);
    const r = await request(app).post(`/api/project-sides/${SERVER}/registration`).send({});
    expect(r.status).toBe(400);
    expect(r.body.error).toMatch(/url is required/);
  });

  test('REPLACING an existing credential is refused without ?replace=true', async () => {
    /*
     * The tokens in a registration a homeserver has already loaded stop working the moment new ones are
     * stored — so an accidental regeneration takes the project side down until the new file is
     * installed and the homeserver restarted. The 409 says that.
     */
    const app = await boot();
    await create(app);
    await request(app).post(`/api/project-sides/${SERVER}/registration`).send({ url: 'http://a:1' });
    const again = await request(app).post(`/api/project-sides/${SERVER}/registration`).send({ url: 'http://a:1' });
    expect(again.status).toBe(409);
    expect(again.body.code).toBe('credential_exists');
    expect(again.body.detail).toMatch(/restarted/);
  });

  test('with ?replace=true it issues DIFFERENT tokens', async () => {
    const app = await boot();
    await create(app);
    const first = await request(app).post(`/api/project-sides/${SERVER}/registration`).send({ url: 'http://a:1' });
    const second = await request(app).post(`/api/project-sides/${SERVER}/registration?replace=true`)
      .send({ url: 'http://a:1' });
    expect(second.status).toBe(200);
    /*
     * `.body`, not the response. The first version of this passed the supertest RESPONSE objects to a
     * helper whose parameter was named `body`, so it read `response.registrationYaml` — undefined,
     * which `exec` coerces to the string "undefined", which does not match, which returns null. The
     * failure surfaced as "Cannot read properties of null" three lines away from the mistake.
     */
    const asTokenOf = (body) => {
      const match = /as_token: ([0-9a-f]{64})/.exec(body.registrationYaml);
      expect(match, 'no as_token in the returned YAML').not.toBeNull();
      return match[1];
    };
    expect(asTokenOf(second.body)).not.toBe(asTokenOf(first.body));
  });

  test('WHAT IS STORED MATCHES THE YAML — both tokens, in the right fields', async () => {
    /*
     * Found by mutation testing, and it is the failure that would have been hardest to diagnose:
     * storing `hsToken: registration.as_token` survived every other assertion here. The YAML still
     * shows two distinct, correct tokens — so the operator installs a valid registration — while the
     * stored credential has the wrong value in the hs_token field. The homeserver then pushes with the
     * hs_token from the file, Hagency compares it against the as_token, and EVERY transaction is 403.
     * An appservice that is configured, installed, and silent.
     *
     * The credential is write-only, so no API can observe it (ADR-016 decision 8). This reads the
     * store's file directly, which the test's own runtime dir owns. That couples the assertion to the
     * on-disk shape, which is a real cost — and the alternative is leaving the only field that decides
     * inbound authentication unverified until someone tries it against a live homeserver.
     */
    const app = await boot();
    await create(app);
    const r = await request(app).post(`/api/project-sides/${SERVER}/registration`)
      .send({ url: 'http://host.docker.internal:8009' });
    const yaml = r.body.registrationYaml;
    const asToken = /as_token: ([0-9a-f]{64})/.exec(yaml)[1];
    const hsToken = /hs_token: ([0-9a-f]{64})/.exec(yaml)[1];
    const localpart = /sender_localpart: (\S+)/.exec(yaml)[1];
    const namespace = /regex: "(.+)"/.exec(yaml)[1];

    const stored = JSON.parse(readFileSync(
      path.join(context.runtimeDir, 'data', 'project-sides.json'), 'utf8',
    )).sides[SERVER].credential;

    expect(stored.kind).toBe('appservice');
    expect(stored.asToken).toBe(asToken);
    expect(stored.hsToken).toBe(hsToken);
    expect(stored.senderLocalpart).toBe(localpart);
    expect(stored.namespace).toBe(namespace);
    // And the two are not the same value, which is the property the fields could silently violate.
    expect(stored.asToken).not.toBe(stored.hsToken);
  });

  test('the YAML tells the operator where it goes and that a restart is needed', async () => {
    // Measured against Palpo: registrations load into a OnceCell, so installing the file is not enough.
    const app = await boot();
    await create(app);
    const r = await request(app).post(`/api/project-sides/${SERVER}/registration`).send({ url: 'http://a:1' });
    expect(r.body.restartRequired).toBe(true);
    expect(r.body.installPath).toMatch(/appservice_registration_dir/);
    expect(r.body.registrationYaml).toMatch(/restart/i);
  });

  test('the claimed namespace matches the prefix the bridge already uses', async () => {
    // `@ac_.*` describes today's fleet — the bridge names agent accounts `ac_<name>` — so this
    // formalises the existing naming instead of requiring a rename (ADR-014 decision 2).
    const app = await boot();
    await create(app);
    const r = await request(app).post(`/api/project-sides/${SERVER}/registration`).send({ url: 'http://a:1' });
    expect(r.body.registrationYaml).toContain('regex: "@ac_.*"');
  });

  test('an unknown side is a 404', async () => {
    const app = await boot();
    expect((await request(app).post('/api/project-sides/never.configured/registration')
      .send({ url: 'http://a:1' })).status).toBe(404);
  });
});

describe('removal refuses to be the first step of a cascade', () => {
  test('an ACTIVE side is refused with 409', async () => {
    /*
     * Because removing it drops the credential every earlier cascade step still needs — leaving
     * rooms, revoking tokens, telling the borrower. Deactivate first.
     */
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', credential: asCred() });
    const r = await request(app).delete(`/api/project-sides/${SERVER}`);
    expect(r.status).toBe(409);
    expect(r.body.code).toBe('side_active');
    expect((await request(app).get(`/api/project-sides/${SERVER}`)).status).toBe(200);
  });

  test('deactivating first allows removal', async () => {
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    await request(app).post(`/api/project-sides/${SERVER}/deactivate`);
    expect((await request(app).delete(`/api/project-sides/${SERVER}`)).status).toBe(200);
    expect((await request(app).get(`/api/project-sides/${SERVER}`)).status).toBe(404);
  });

  test('force removes an active side', async () => {
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    expect((await request(app).delete(`/api/project-sides/${SERVER}?force=true`)).status).toBe(200);
  });

  test('the response lists what it DID, rather than claiming completeness', async () => {
    /*
     * "Complete" would be a statement about everything in the system that could reference a side, which
     * no single handler can make. This repository has already produced the failure that phrasing
     * invites: force-deleting an agent left three active bindings and a 250k commitment pointing at it.
     */
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    const r = await request(app).delete(`/api/project-sides/${SERVER}?force=true`);
    expect(r.body.cascade).toBe('performed');
    expect(r.body.cascadeNote).toMatch(/ended and inactive are history while active is a claim/);
    expect(r.body).toMatchObject({ retiredAgents: [], endedEngagements: [], deactivatedBindings: [] });
  });

  test('an ACTIVE engagement on the side is ENDED, and its commitment released', async () => {
    /*
     * ADR-016 decision 7 and the operator's rule that records are kept and only stood down. `ended` is
     * history; `active` is a claim about the present — work in progress, tokens promised. Left behind on
     * a side that no longer exists it goes on holding allocation, and the console goes on showing work
     * that will never proceed.
     *
     * Releasing the commitment needs no separate step: `committedForProjectSide` sums only `active`, so
     * the transition IS the release, and there is no figure that can disagree with the state it came
     * from.
     */
    const hs = await fakeHomeserver(() => [200, {}]);
    const app = await boot({
      agents: { someone: { name: 'someone', kind: 'agent', type: 'claude', projectSide: SERVER } },
      rawDataFiles: {
        'engagements.json': JSON.stringify({
          version: 1,
          engagements: {
            live: {
              id: 'live', state: 'active', agent: 'someone', createdAt: 1,
              projectRoomId: `!room:${SERVER}`, allocatedTokens: 400000,
            },
            elsewhere: {
              id: 'elsewhere', state: 'active', agent: 'someone', createdAt: 1,
              projectRoomId: '!other:other.example', allocatedTokens: 100000,
            },
          },
          whitelist: {}, offers: {}, audit: [],
        }),
      },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: hs.baseUrl });
    await request(app).put(`/api/project-sides/${SERVER}/allocation`).send({ allocated_tokens: 1000000 });

    const before = await request(app).get(`/api/project-sides/${SERVER}/budget`);
    expect(before.body.committed).toBe(400000);

    await request(app).put(`/api/project-sides/${SERVER}/credential`).send({ credential: asCred() }).expect(200);
    const r = await request(app).delete(`/api/project-sides/${SERVER}?force=true`).expect(200);
    expect(hs.calls.some((c) => c.path.endsWith('/leave'))).toBe(true);
    expect(r.body.endedEngagements).toEqual(['live']);

    const rows = (await request(app).get('/api/engagements')).body.engagements;
    const live = rows.find((e) => e.id === 'live');
    // NOT deleted - the record survives with a reason, which is the compliance shape.
    expect(live).toBeTruthy();
    expect(live.state).toBe('ended');
    expect(live.endedReason).toMatch(new RegExp(SERVER));
    // And the one on another side is untouched: a different side is a different budget.
    expect(rows.find((e) => e.id === 'elsewhere').state).toBe('active');
  });

  test('a BINDING on the side is DEACTIVATED, not deleted', async () => {
    /*
     * A binding is evidence of who invited what (ADR-002), so it is kept. What changes is that it stops
     * claiming the project can reach the agent - `listBindings` already filters `active !== false`, so
     * deactivation removes it from every read without anything learning a new state.
     */
    const hs = await fakeHomeserver(() => [200, {}]);
    const app = await boot({
      agents: { ag: { name: 'ag', kind: 'agent', type: 'claude', projectSide: SERVER } },
      env: { MATRIX_BRIDGE_SECRET: 'secret-for-binding-cascade' },
      rawDataFiles: {
        'approvals.json': JSON.stringify({
          version: 1,
          bindings: {
            'ag\u0000!room:palpo.test': {
              agent: 'ag', project: 'p', projectRoomId: `!room:${SERVER}`,
              ownerMxid: `@alex:${SERVER}`, ownerDmRoomId: `!dm:${SERVER}`,
              active: true, addedAt: 1, updatedAt: 1, agentJoined: null, membershipCheckedAt: null,
            },
            'ag\u0000!other:other.example': {
              agent: 'ag', project: 'q', projectRoomId: '!other:other.example',
              ownerMxid: '@bob:other.example', ownerDmRoomId: '!dm:other.example',
              active: true, addedAt: 1, updatedAt: 1, agentJoined: null, membershipCheckedAt: null,
            },
          },
          requests: {}, audit: [],
        }),
      },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: hs.baseUrl });
    await request(app).put(`/api/project-sides/${SERVER}/credential`).send({ credential: asCred() }).expect(200);
    const r = await request(app).delete(`/api/project-sides/${SERVER}?force=true`).expect(200);
    expect(hs.calls.some((c) => c.path.endsWith('/leave'))).toBe(true);
    expect(r.body.deactivatedBindings).toEqual([`ag@!room:${SERVER}`]);

    // The record is still on disk - deactivated, not forgotten.
    const stored = JSON.parse(readFileSync(
      path.join(context.runtimeDir, 'data', 'approvals.json'), 'utf8',
    )).bindings;
    const ours = Object.values(stored).find((b) => b.projectRoomId === `!room:${SERVER}`);
    expect(ours.active).toBe(false);
    expect(ours.deactivatedReason).toMatch(new RegExp(SERVER));
    expect(typeof ours.deactivatedAt).toBe('number');
    // The other side's binding is untouched.
    expect(Object.values(stored).find((b) => b.projectRoomId === '!other:other.example').active).toBe(true);
  });

  test('a REFUSED delete touches no engagement and no binding', async () => {
    /*
     * The precondition guard again, now covering three kinds of damage instead of one. A 409 that had
     * already ended a side's engagements would leave the operator with a side they still see as
     * configured and work that has silently stopped.
     */
    const app = await boot({
      rawDataFiles: {
        'engagements.json': JSON.stringify({
          version: 1,
          engagements: {
            live: {
              id: 'live', state: 'active', agent: 'someone', createdAt: 1,
              projectRoomId: `!room:${SERVER}`, allocatedTokens: 400000,
            },
          },
          whitelist: {}, offers: {}, audit: [],
        }),
      },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    expect((await request(app).delete(`/api/project-sides/${SERVER}`)).status).toBe(409);
    const rows = (await request(app).get('/api/engagements')).body.engagements;
    expect(rows.find((e) => e.id === 'live').state).toBe('active');
  });

  test('AN AGENT CANNOT CLAIM A SIDE IT WAS NOT PROVISIONED FOR', async () => {
    /*
     * Found by mutation testing, and it is the hole the `provisionedSides` map exists to close — I had
     * written the comment explaining it and never tested it.
     *
     * `POST /api/agents` is the agent's OWN registration. If a `projectSide` in that body were honoured,
     * an agent could attach itself to any side, and a side is what decision 7's cascade uses to decide
     * what to retire and what decision 6's budget charges. The assignment is the backend's decision,
     * taken when the provision plan is handed out.
     */
    const app = await boot();
    await request(app).post('/api/agents').send({
      name: 'self_declared', type: 'agent', projectSide: SERVER,
    });
    const roster = await request(app).get('/api/agents');
    const rows = Array.isArray(roster.body) ? roster.body : roster.body.agents;
    const agent = rows.find((a) => a.name === 'self_declared');
    expect(agent, 'the agent did not register at all').toBeTruthy();
    expect(agent.projectSide, 'an agent claimed a project side from its own request body').toBeNull();
  });

  test('a side survives a re-registration of one of its agents', async () => {
    /*
     * The other direction: once the backend HAS assigned a side, an agent re-registering — which it does
     * on every restart — must not lose it. `existing.projectSide` is the carry-forward, and without it a
     * restart would silently orphan an agent from the side whose budget it is charged to.
     */
    const app = await boot({
      agents: { assigned: { name: 'assigned', kind: 'agent', type: 'agent', projectSide: SERVER } },
    });
    await request(app).post('/api/agents').send({ name: 'assigned', type: 'agent' });
    const roster = await request(app).get('/api/agents');
    const rows = Array.isArray(roster.body) ? roster.body : roster.body.agents;
    expect(rows.find((a) => a.name === 'assigned').projectSide).toBe(SERVER);
  });

  test('pending invitations on the side are DECLINED, not left waiting on a decision that cannot matter', async () => {
    /*
     * ADR-016 row 7 recorded that nothing outside the first three stores was swept. A pending invitation
     * to a room on a removed side can never be accepted — there is no side left to join — so a
     * contributor looking at 「等我决定」 would be looking at a decision with nothing behind it.
     *
     * DECLINED, not deleted, and the decider is recorded as `project-side-removed`: the compliance rule
     * for this whole cascade is 「不删除，只是停用退役」, and a declined invitation is history where a missing
     * one is amnesia.
     */
    const app = await boot({ env: { MATRIX_BRIDGE_SECRET: 'secret-for-invite-cascade' } });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' }).expect(200);

    const onSide = `!work:${SERVER}`;
    const elsewhere = '!local:contributor.example';
    for (const roomId of [onSide, elsewhere]) {
      await request(app).put('/api/matrix/pending-invites')
        .set('X-Bridge-Secret', 'secret-for-invite-cascade')
        .send({ project_room_id: roomId, agent: 'someone', inviter: `@lin:${SERVER}`, roomName: 'work' })
        .expect(200);
    }

    const r = await request(app).delete(`/api/project-sides/${SERVER}?force=true`);
    expect(r.status).toBe(200);
    expect(r.body.declinedInvites).toEqual([{ roomId: onSide, agent: 'someone' }]);

    const left = await request(app).get('/api/matrix/pending-invites');
    const rows = left.body.invites ?? left.body ?? [];
    /*
     * The one on ANOTHER server is untouched: removing a side says nothing about rooms it never owned.
     * Read as `projectRoomId`, which is what the store records — the field is not `roomId`, and asserting
     * on that name gave `[undefined]`, a comparison that would have passed for the wrong reason if the
     * expectation had been loose.
     */
    expect(rows.filter((i) => i.state === 'pending').map((i) => i.projectRoomId)).toEqual([elsewhere]);
  });

  test('alerts that name the side are resolved, because an alert pointing at nothing teaches distrust', async () => {
    /*
     * `project_side_budget:<side>` outlives its side, and an operator who chases it finds a 404 and
     * learns to trust the next alert less. Resolved rather than deleted, by the same `autoResolve` path
     * the allocation route uses when a raise actually helps.
     */
    const app = await boot();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' }).expect(200);
    // An unallocated side refuses work and raises the alarm — the cheapest way to get a real one.
    await request(app).post('/api/engagements').send({
      project: 'p', projectRoomId: `!r:${SERVER}`, role: 'coding', requester: `@r:${SERVER}`,
      requestedTokens: 1000, ratePerDay: 100, requestId: '$cascade-alarm',
    });
    const before = (await request(app).get('/api/alerts')).body;
    const open = (before.alerts ?? before ?? []).filter((a) => a.alertType === 'project_side_budget');
    expect(open.length).toBeGreaterThan(0);
    expect(open.every((a) => a.status !== 'resolved')).toBe(true);

    await request(app).delete(`/api/project-sides/${SERVER}?force=true`).expect(200);

    const after = (await request(app).get('/api/alerts')).body;
    const now = (after.alerts ?? after ?? []).filter((a) => a.alertType === 'project_side_budget');
    expect(now.length).toBeGreaterThan(0);
    expect(now.every((a) => a.status === 'resolved')).toBe(true);
  });

  test('agents minted for the side are RETIRED, and their record survives', async () => {
    /*
     * Retire, not delete. Erasing an agent erases its consumption with it, and ADR-013 makes the token
     * the unit of account — a closed period's totals would change retroactively and the ledger would
     * become falsifiable by deletion.
     */
    const app = await boot({
      agents: {
        mx_palpo_test_worker: {
          name: 'mx_palpo_test_worker', kind: 'agent', type: 'agent',
          projectSide: SERVER, tmux: 'sess:0', online: true,
        },
        unrelated: { name: 'unrelated', kind: 'agent', type: 'agent', projectSide: null, tmux: 'other:0' },
      },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    const r = await request(app).delete(`/api/project-sides/${SERVER}?force=true`);
    expect(r.body.retiredAgents).toEqual(['mx_palpo_test_worker']);

    const roster = await request(app).get('/api/agents');
    const rows = Array.isArray(roster.body) ? roster.body : roster.body.agents;
    const retired = rows.find((a) => a.name === 'mx_palpo_test_worker');
    // The record SURVIVES — this is the whole difference between retiring and deleting.
    expect(retired).toBeTruthy();
    expect(retired.offlineReason).toMatch(new RegExp(`retired:project-side-removed:${SERVER}`));
    expect(retired.tmux).toBeNull();
    expect(typeof retired.retiredAt).toBe('number');
  });

  test('an agent on ANOTHER side, or on none, is untouched', async () => {
    const app = await boot({
      agents: {
        mine: { name: 'mine', kind: 'agent', type: 'agent', projectSide: SERVER, tmux: 'a:0' },
        theirs: { name: 'theirs', kind: 'agent', type: 'agent', projectSide: 'other.example', tmux: 'b:0' },
        manual: { name: 'manual', kind: 'agent', type: 'agent', projectSide: null, tmux: 'c:0' },
      },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    const r = await request(app).delete(`/api/project-sides/${SERVER}?force=true`);
    expect(r.body.retiredAgents).toEqual(['mine']);

    const roster = await request(app).get('/api/agents');
    const rows = Array.isArray(roster.body) ? roster.body : roster.body.agents;
    for (const name of ['theirs', 'manual']) {
      const a = rows.find((x) => x.name === name);
      expect(a.tmux, name).not.toBeNull();
      expect(String(a.offlineReason || ''), name).not.toMatch(/retired/);
    }
  });

  test('AGENTS ARE RETIRED BEFORE THE SIDE IS FORGOTTEN, not after', async () => {
    /*
     * Reversed, a persistence failure on the removal would leave a live side whose agents had already
     * been retired — work refused by a side the operator still sees as configured, which is the
     * confusing half of the two. Observed here by the retirement being reported alongside a removal
     * that succeeded; the ordering itself is asserted in the source, since forcing a mid-sequence
     * failure would need the store to be made to fail on demand.
     */
    const app = await boot({
      agents: { mine: { name: 'mine', kind: 'agent', type: 'agent', projectSide: SERVER, tmux: 'a:0' } },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    const r = await request(app).delete(`/api/project-sides/${SERVER}?force=true`);
    expect(r.status).toBe(200);
    expect(r.body.retiredAgents).toEqual(['mine']);
    expect((await request(app).get(`/api/project-sides/${SERVER}`)).status).toBe(404);
  });

  test('an ACTIVE side is still refused, and NOTHING is retired', async () => {
    // The refusal must happen before any of the cascade runs, or a refused delete would still have
    // taken the side's agents down.
    const app = await boot({
      agents: { mine: { name: 'mine', kind: 'agent', type: 'agent', projectSide: SERVER, tmux: 'a:0' } },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    const r = await request(app).delete(`/api/project-sides/${SERVER}`);
    expect(r.status).toBe(409);

    const roster = await request(app).get('/api/agents');
    const rows = Array.isArray(roster.body) ? roster.body : roster.body.agents;
    expect(rows.find((a) => a.name === 'mine').tmux).toBe('a:0');
  });
});

describe('persistence', () => {
  test('a seeded store is read at boot', async () => {
    // Proves the backend binds the store to its data dir rather than starting empty every time.
    const app = await boot({
      rawDataFiles: {
        'project-sides.json': JSON.stringify({
          version: 1,
          sides: {
            [SERVER]: {
              id: SERVER, serverName: SERVER, label: 'Seeded', apiBaseUrl: 'http://127.0.0.1:8008',
              credential: null, representative: null, active: true,
              createdAt: 1, updatedAt: 1, accessState: 'unverified', accessDetail: null, accessCheckedAt: null,
            },
          },
          audit: [],
        }),
      },
    });
    const r = await request(app).get('/api/project-sides');
    expect(r.body.sides).toHaveLength(1);
    expect(r.body.sides[0].label).toBe('Seeded');
  });
});

describe('the inbound credentials the BRIDGE needs', () => {
  /*
   * A narrow, deliberate exception to ADR-016 decision 8, and the tests are mostly about how narrow.
   *
   * "Write-only" was decided against the CONSOLE: a browser holding the operator token must not read a
   * credential, because the console renders whatever an API returns and this repository has shipped API
   * text into an unintended UI twice. The bridge is a different principal — it must authenticate a
   * homeserver's push and cannot do that without the token, so refusing it would not protect anything,
   * it would only mean the appservice cannot work.
   *
   * The precedent is exact rather than analogous: `GET /api/approval-bindings` is bridge-secret guarded
   * and returns `ownerDmRoomId`, deliberately withheld from the console proxy.
   */
  const SECRET = 'bridge-secret-for-inbound-credentials';

  async function bootWithSecret() {
    return boot({ env: { MATRIX_BRIDGE_SECRET: SECRET } });
  }

  const withRegistration = async (app, server = SERVER) => {
    await request(app).post('/api/project-sides')
      .send({ server_name: server, api_base_url: 'http://127.0.0.1:8008' });
    const r = await request(app).post(`/api/project-sides/${server}/registration`)
      .send({ url: 'http://host.docker.internal:8009' });
    return /hs_token: ([0-9a-f]{64})/.exec(r.body.registrationYaml)[1];
  };

  test('THE POINT: the operator token alone does NOT open it', async () => {
    /*
     * If a bearer token were enough, the console proxy would be one allowlist entry away from putting a
     * namespace-granting credential in a browser — which is the thing decision 8 exists to prevent.
     */
    const app = await bootWithSecret();
    await withRegistration(app);
    const res = await request(app).get('/api/project-sides/inbound-credentials');
    expect(res.status).toBe(403);
    expect(JSON.stringify(res.body)).not.toMatch(/[0-9a-f]{64}/);
  });

  test('the bridge secret opens it, and the hs_token is what comes back', async () => {
    const app = await bootWithSecret();
    const hsToken = await withRegistration(app);
    const res = await request(app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', SECRET);
    expect(res.status).toBe(200);
    expect(res.body.sides).toHaveLength(1);
    expect(res.body.sides[0]).toMatchObject({
      sideId: SERVER, serverName: SERVER, hsToken, senderLocalpart: 'hagency',
    });
  });

  test('THE SCOPE: the as_token is NOT returned', async () => {
    /*
     * The `as_token` acts AS an agent on that homeserver; inbound authentication does not need it. When
     * the bridge needs to send as an agent, that is a separate grant with its own argument — and until
     * then handing it over would be widening the exception for a use that does not exist yet.
     */
    const app = await bootWithSecret();
    await withRegistration(app);
    const created = await request(app).post(`/api/project-sides/${SERVER}/registration?replace=true`)
      .send({ url: 'http://host.docker.internal:8009' });
    const asToken = /as_token: ([0-9a-f]{64})/.exec(created.body.registrationYaml)[1];

    const res = await request(app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', SECRET);
    expect(JSON.stringify(res.body)).not.toContain(asToken);
    expect(res.body.sides[0]).not.toHaveProperty('asToken');
  });

  test('a DEACTIVATED side is omitted, because it is closed to new work', async () => {
    // A listener that still accepted its pushes would be taking work from a side the operator closed.
    const app = await bootWithSecret();
    await withRegistration(app);
    await request(app).post(`/api/project-sides/${SERVER}/deactivate`);
    const res = await request(app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', SECRET);
    expect(res.body.sides).toHaveLength(0);
  });

  test('a registration-token side is omitted, because it has no inbound push', async () => {
    const app = await bootWithSecret();
    await request(app).post('/api/project-sides')
      .send({
        server_name: SERVER,
        api_base_url: 'http://127.0.0.1:8008',
        credential: { kind: 'registrationToken', registrationToken: REG_TOKEN, representativeToken: null },
      });
    const res = await request(app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', SECRET);
    expect(res.body.sides).toHaveLength(0);
  });

  test('a side with no credential at all is omitted rather than returned empty', async () => {
    const app = await bootWithSecret();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    const res = await request(app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', SECRET);
    expect(res.body.sides).toHaveLength(0);
  });

  test('several appservice sides each come back with their OWN token', async () => {
    // The router identifies a side by its token, so two sides sharing one would be unroutable —
    // and this is where that would first be visible.
    const app = await bootWithSecret();
    const tokenA = await withRegistration(app, 'a.example');
    const tokenB = await withRegistration(app, 'b.example');
    expect(tokenA).not.toBe(tokenB);
    const res = await request(app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', SECRET);
    const byId = Object.fromEntries(res.body.sides.map((s) => [s.sideId, s.hsToken]));
    expect(byId['a.example']).toBe(tokenA);
    expect(byId['b.example']).toBe(tokenB);
  });

  test('it fails closed when no bridge secret is configured at all', async () => {
    /*
     * The common deployment has no secret set, and comparing two empty strings would let everyone
     * through. `requireApprovalBridgeSecret` answers 503 for exactly this.
     */
    const app = await boot();
    await withRegistration(app);
    const res = await request(app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', '');
    expect(res.status).toBe(503);
    expect(JSON.stringify(res.body)).not.toMatch(/[0-9a-f]{64}/);
  });
});

describe('a side\'s allocation, and refusing to mint without one', () => {
  /*
   * ADR-016's settled question 2 and decision 6. The operator chose a REAL per-side allocation over a
   * slice of the deployment total, and stated the requirement for the gate directly: 「如果 token
   * 预算已经超标了…这时候应该报警，说无法创建 agent，需要加预算」.
   *
   * The gate lives on the provisioning path, which counted AGENTS and never consulted a budget at all.
   */
  const ROOM = `!proj:${SERVER}`;

  const sideWithAllocation = async (app, tokens) => {
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    if (tokens !== undefined) {
      await request(app).put(`/api/project-sides/${SERVER}/allocation`).send({ allocated_tokens: tokens });
    }
  };

  const provision = (app, body = {}) => request(app).post('/api/dispatch')
    .send({ role: 'documentation', capability: 'lightweight', room: ROOM, ...body });

  async function bootProvisionable() {
    return boot({ env: { MATRIX_AGENT_MAX_PER_CELL: '1' } });
  }

  test('UNALLOCATED IS NOT UNLIMITED: minting is refused with the reason', async () => {
    /*
     * The default chosen while it was still free. A side that has been configured and not yet budgeted
     * refuses work rather than drawing on whatever is left — so the alarm arrives before the tokens
     * instead of after them.
     */
    const app = await bootProvisionable();
    await sideWithAllocation(app); // no allocation set
    const r = await provision(app, { requestedTokens: 1000 });
    expect(r.status).toBe(409);
    expect(r.body.reason).toBe('no_allocation');
    expect(r.body.error).toMatch(/no token allocation/);
  });

  test('an unconfigured project side is refused, not charged to nobody', async () => {
    const app = await bootProvisionable();
    const r = await provision(app, { requestedTokens: 1000 });
    expect(r.status).toBe(409);
    expect(r.body.reason).toBe('no_project_side');
    expect(r.body.sideId).toBe(SERVER);
  });

  test('within the allocation it provisions, and the name CARRIES THE SIDE', async () => {
    /*
     * `mx_${role}_${tier}_${seq}` was unattributable. Decision 7's cascade needs to know which agents
     * belong to a side in order to take them with it.
     */
    const app = await bootProvisionable();
    await sideWithAllocation(app, 1_000_000);
    const r = await provision(app, { requestedTokens: 250_000 });
    expect(r.status).toBe(200);
    expect(r.body.status).toBe('provision');
    expect(r.body.sideId).toBe(SERVER);
    expect(r.body.name).toContain('palpo_test');
    expect(r.body.budget).toMatchObject({ allocated: 1_000_000, committed: 0, remaining: 1_000_000 });
  });

  test('THE ALARM: over the allocation is a REFUSAL, never a queue entry', async () => {
    /*
     * Falling through to the queue would answer "waiting for capacity" when the truth is "will never
     * be created without more budget" — and an agent that was never created cannot be waited for. The
     * refusal names the shortfall so the remedy is obvious.
     */
    const app = await bootProvisionable();
    await sideWithAllocation(app, 100_000);
    const r = await provision(app, { requestedTokens: 250_000 });
    expect(r.status).toBe(409);
    expect(r.body.reason).toBe('over_allocation');
    expect(r.body).toMatchObject({
      allocatedTokens: 100_000, remainingTokens: 100_000, requestedTokens: 250_000,
    });
    expect(r.body.error).toMatch(/raise the allocation/);
    /*
     * `status` is the field a caller reads, and asserting only the absence of a ticket was not enough:
     * mutation testing showed that relabelling this response `queued` survived every other assertion
     * here, which is precisely the "waiting for capacity" lie this refusal exists to avoid.
     */
    expect(r.body.status).toBe('refused');
    expect(r.body.ticket).toBeUndefined();
  });

  test('a request with NO room is untouched by the gate', async () => {
    /*
     * Scope, and the reason a first version of this was wrong. A request with no room is not
     * project-side work — it is the contributor dispatching against their own agents, already governed
     * by each agent's own ceiling. Requiring a side there broke two existing dispatch cases.
     */
    const app = await bootProvisionable();
    const r = await request(app).post('/api/dispatch')
      .send({ role: 'documentation', capability: 'lightweight' });
    expect(r.body.status).toBe('provision');
    expect(r.body.name).toMatch(/^mx_documentation_lightweight_/);
    expect(r.body.sideId).toBeUndefined();
  });

  test('A FULFILLED PLAN RELEASES ITS SEAT: register the provisioned agent, and the cell frees up', async () => {
    /*
     * THE LEAK THIS PINS. A reservation exists to hold a (role, tier) seat between "plan handed out"
     * and "agent registered" — and until 2026-08-15 it was incremented and NEVER released, so every
     * plan ever issued held its seat for the life of the process. With MATRIX_AGENT_MAX_PER_CELL=1:
     * plan issued (reserved=1), agent registered and later deleted (cell empty again) — and the next
     * provision was still refused, because the ghost reservation counted forever.
     */
    const app = await bootProvisionable();
    await sideWithAllocation(app, 1_000_000);

    const plan = await provision(app, { requestedTokens: 100_000 });
    expect(plan.body.status).toBe('provision');

    // The seat is HELD while the plan is outstanding: same cell, second ask queues.
    expect((await provision(app, { requestedTokens: 100_000 })).body.status).toBe('queued');

    // The plan is fulfilled — and the registration consumes it: side attributed, reservation released.
    await request(app).post('/api/agents').send({ name: plan.body.name, type: 'claude' }).expect(200);
    expect((await request(app).get(`/api/agents/${plan.body.name}`)).body.projectSide).toBe(SERVER);

    /*
     * THE DISTINGUISHING ASK. The registered agent has no runtime profile, so it does not qualify for
     * the cell and cannot be routed to — which isolates the reservation as the only thing that could
     * block this. Under the leak the ghost reservation counted forever and this stayed `queued` for the
     * life of the process; with the release it provisions.
     */
    const after = await provision(app, { requestedTokens: 100_000 });
    expect(after.body.status).toBe('provision');
  });

  test('an ABANDONED plan expires: the launcher died, and the seat comes back on its own', async () => {
    /*
     * The reservation's OTHER legitimate end. A plan whose launcher crashed between plan and
     * registration must not hold the seat forever — 15 minutes of silence is presumed dead, and the
     * expiry is lazy (checked at the next cap read) so nothing needs a timer.
     */
    vi.useFakeTimers();
    try {
      const app = await bootProvisionable();
      await sideWithAllocation(app, 1_000_000);

      expect((await provision(app, { requestedTokens: 100_000 })).body.status).toBe('provision');
      expect((await provision(app, { requestedTokens: 100_000 })).body.status).toBe('queued');

      vi.setSystemTime(Date.now() + 16 * 60 * 1000);
      const revived = await provision(app, { requestedTokens: 100_000 });
      expect(revived.body.status).toBe('provision');
    } finally {
      vi.useRealTimers();
    }
  });

  test('zero is a real allocation and is not the same as unset', async () => {
    // An operator may want to close a side to new work without deactivating it.
    const app = await bootProvisionable();
    await sideWithAllocation(app, 0);
    const r = await provision(app, { requestedTokens: 1 });
    expect(r.body.reason).toBe('over_allocation');
    expect(r.body.allocatedTokens).toBe(0);
  });

  test('the allocation survives an unrelated save', async () => {
    /*
     * An allocation is a budgeting decision, and an upsert that saved a label must not silently
     * un-budget a side — which would present as every mint being refused for no visible reason.
     */
    const app = await boot();
    await sideWithAllocation(app, 500_000);
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008', label: 'Renamed' });
    const budget = await request(app).get(`/api/project-sides/${SERVER}/budget`);
    expect(budget.body).toMatchObject({ allocated: 500_000, remaining: 500_000 });
  });

  test('the allocation can be cleared back to unallocated', async () => {
    const app = await boot();
    await sideWithAllocation(app, 500_000);
    const cleared = await request(app).put(`/api/project-sides/${SERVER}/allocation`).send({});
    expect(cleared.body.side.allocatedTokens).toBeNull();
    expect(cleared.body.budget.remaining).toBeNull();
  });

  test('a negative or fractional allocation is refused', async () => {
    const app = await boot();
    await sideWithAllocation(app);
    for (const bad of [-1, 1.5, 'lots']) {
      const r = await request(app).put(`/api/project-sides/${SERVER}/allocation`).send({ allocated_tokens: bad });
      expect(r.status, String(bad)).toBe(400);
    }
  });

  test('only ACTIVE engagements count against the allocation', async () => {
    /*
     * Found by mutation testing: dropping the state filter from the per-side sum survived, because no
     * test had a non-active engagement on the side. It matters in the direction that costs capacity —
     * an ended engagement still counted would permanently consume a side's allocation, and the only
     * remedy an operator would find is raising a budget that is not actually being spent.
     *
     * Seeded through the store's own file rather than driven through the API, because reaching an
     * `ended` state needs a requester credential, an approval and a revoke — three things that would
     * test everything except the sum.
     */
    const app = await boot({
      rawDataFiles: {
        'engagements.json': JSON.stringify({
          version: 1,
          engagements: {
            ended_one: {
              id: 'ended_one', state: 'ended', agent: 'someone',
              projectRoomId: `!old:${SERVER}`, allocatedTokens: 900_000,
            },
            active_one: {
              id: 'active_one', state: 'active', agent: 'someone',
              projectRoomId: `!live:${SERVER}`, allocatedTokens: 100_000,
            },
          },
          whitelist: {}, offers: {}, audit: [],
        }),
      },
    });
    await sideWithAllocation(app, 1_000_000);
    const budget = await request(app).get(`/api/project-sides/${SERVER}/budget`);
    // 100k committed, not 1,000,000 — the ended engagement releases its promise.
    expect(budget.body).toMatchObject({ allocated: 1_000_000, committed: 100_000, remaining: 900_000 });
  });

  test('an engagement on ANOTHER side does not count against this one', async () => {
    // The side is derived from the room's origin server, so a room on a different homeserver is a
    // different budget. Grouping them would make one project side's spend refuse another's work.
    const app = await boot({
      rawDataFiles: {
        'engagements.json': JSON.stringify({
          version: 1,
          engagements: {
            elsewhere: {
              id: 'elsewhere', state: 'active', agent: 'someone',
              projectRoomId: '!r:other.example', allocatedTokens: 999_000,
            },
          },
          whitelist: {}, offers: {}, audit: [],
        }),
      },
    });
    await sideWithAllocation(app, 1_000_000);
    const budget = await request(app).get(`/api/project-sides/${SERVER}/budget`);
    expect(budget.body).toMatchObject({ committed: 0, remaining: 1_000_000 });
  });

  test('the budget endpoint 404s for an unknown side', async () => {
    const app = await boot();
    expect((await request(app).get('/api/project-sides/never.configured/budget')).status).toBe(404);
  });
});

describe('the ACTING credential — a second, wider grant', () => {
  /*
   * `inbound-credentials` was scoped to `hsToken` with the note that an `as_token` "acts AS an agent on
   * that homeserver" and would need "a separate grant with its own argument". This is that grant, and
   * the tests are mostly about it staying separate.
   *
   * The difference is real: `hsToken` authenticates a push INTO us, bounded further by the receiver's
   * own idempotency and routing. `asToken` acts as ANY identity in the claimed namespace — speaking,
   * creating rooms and joining rooms as every lent agent on that side.
   */
  const SECRET = 'bridge-secret-for-acting-credentials';
  const bootWithSecret = () => boot({ env: { MATRIX_BRIDGE_SECRET: SECRET } });

  const appserviceSide = async (app, server = SERVER) => {
    await request(app).post('/api/project-sides')
      .send({ server_name: server, api_base_url: 'http://127.0.0.1:8008' });
    const r = await request(app).post(`/api/project-sides/${server}/registration`)
      .send({ url: 'http://host.docker.internal:8009' });
    return {
      asToken: /as_token: ([0-9a-f]{64})/.exec(r.body.registrationYaml)[1],
      hsToken: /hs_token: ([0-9a-f]{64})/.exec(r.body.registrationYaml)[1],
    };
  };

  const acting = (app) => request(app).get('/api/project-sides/acting-credentials')
    .set('x-bridge-secret', SECRET);

  test('the operator token alone does NOT open it', async () => {
    const app = await bootWithSecret();
    await appserviceSide(app);
    const r = await request(app).get('/api/project-sides/acting-credentials');
    expect(r.status).toBe(403);
    expect(JSON.stringify(r.body)).not.toMatch(/[0-9a-f]{64}/);
  });

  test('an appservice side returns its as_token and what it needs to masquerade', async () => {
    const app = await bootWithSecret();
    const { asToken } = await appserviceSide(app);
    const r = await acting(app);
    expect(r.status).toBe(200);
    expect(r.body.sides[0]).toMatchObject({
      sideId: SERVER, kind: 'appservice', asToken, senderLocalpart: 'hagency', namespace: '@ac_.*',
      active: true, accessState: 'unverified',
    });
    expect(r.body.sides[0].outboundGeneration).toMatch(/^[0-9a-f]{32}$/);
  });

  test('THE GRANTS STAY SEPARATE: acting does not carry hsToken, inbound does not carry asToken', async () => {
    /*
     * The reason these are two endpoints. If one response carried both, a caller wanting only inbound
     * authentication would receive acting authority as a side effect — and the widening would be
     * invisible, because nothing about the request would have changed.
     */
    const app = await bootWithSecret();
    const { asToken, hsToken } = await appserviceSide(app);

    const actingBody = JSON.stringify((await acting(app)).body);
    expect(actingBody).toContain(asToken);
    expect(actingBody, 'the acting grant leaked the inbound token').not.toContain(hsToken);

    const inboundBody = JSON.stringify((await request(app).get('/api/project-sides/inbound-credentials')
      .set('x-bridge-secret', SECRET)).body);
    expect(inboundBody).toContain(hsToken);
    expect(inboundBody, 'the inbound grant leaked acting authority').not.toContain(asToken);
  });

  test('a registration-token side WITHOUT a representative token is OMITTED', async () => {
    /*
     * Returning it with a null would have the bridge attempt sends that cannot succeed, and the failure
     * would look like a rejected credential rather than an unfinished setup.
     */
    const app = await bootWithSecret();
    await request(app).post('/api/project-sides').send({
      server_name: SERVER,
      api_base_url: 'http://127.0.0.1:8008',
      credential: { kind: 'registrationToken', registrationToken: REG_TOKEN, representativeToken: null },
    });
    expect((await acting(app)).body.sides).toHaveLength(0);
  });

  test('a registration-token side WITH one returns that token, and not the registration token', async () => {
    // The registration token mints accounts; it is not what acts. Returning it would widen the grant
    // past what sending needs.
    const app = await bootWithSecret();
    await request(app).post('/api/project-sides').send({
      server_name: SERVER,
      api_base_url: 'http://127.0.0.1:8008',
      credential: { kind: 'registrationToken', registrationToken: REG_TOKEN, representativeToken: 'rep_tok_x' },
    });
    const body = (await acting(app)).body;
    expect(body.sides[0]).toMatchObject({ sideId: SERVER, kind: 'registrationToken',
      representativeToken: 'rep_tok_x', active: true, accessState: 'unverified' });
    expect(body.sides[0].outboundGeneration).toMatch(/^[0-9a-f]{32}$/);
    expect(JSON.stringify(body)).not.toContain(REG_TOKEN);
    const { MatrixBridge } = await import('../bridge-matrix.js');
    const bridge = Object.assign(Object.create(MatrixBridge.prototype), {
      actingCredentials: new Map(),
      backendApiForActing: async () => body,
      forgetRoomsOnSides: () => {},
    });
    await bridge.refreshActingCredentials();
    // The protected cache preserves readiness metadata; projection actor resolution enforces it.
    expect(bridge.actingSideFor(SERVER)).toMatchObject({
      side: { active: true, accessState: 'unverified' },
      credential: { kind: 'registrationToken', outboundGeneration: expect.any(String) },
    });
  });

  test('a DEACTIVATED side is omitted', async () => {
    const app = await bootWithSecret();
    await appserviceSide(app);
    await request(app).post(`/api/project-sides/${SERVER}/deactivate`);
    expect((await acting(app)).body.sides).toHaveLength(0);
  });

  test('a side with no credential at all is omitted', async () => {
    const app = await bootWithSecret();
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    expect((await acting(app)).body.sides).toHaveLength(0);
  });

  test('it fails closed with no bridge secret configured', async () => {
    const app = await boot();
    await appserviceSide(app);
    const r = await request(app).get('/api/project-sides/acting-credentials').set('x-bridge-secret', '');
    expect(r.status).toBe(503);
    expect(JSON.stringify(r.body)).not.toMatch(/[0-9a-f]{64}/);
  });
});

describe('issuing a second credential does not break the first', () => {
  /*
   * WHY THIS EXISTS, in the operator's words: 「为啥生成接单员之后还需要用户去做这些琐事，你应该自动注册」.
   *
   * They were right, and the root was not the chore. Issuing REPLACED the live credential, so clicking
   * "generate" on a working side broke Hagency's own outbound auth instantly — before they could possibly have
   * installed the new file. It happened for real: the fleet held one token, the homeserver still had the
   * previous one, `verify` correctly reported `rejected`, and the operator was handed a repair job for a state
   * Hagency had created.
   *
   * What CANNOT be automated is stated rather than papered over: writing a file onto a customer's filesystem
   * and restarting their Matrix server are the authorities `docs/FOR-PROJECT-SIDES.md` promises never to take.
   * But Hagency can notice the moment the install lands and promote without being asked, so the remaining job
   * is "install it" with no separate "now tell Hagency" step.
   *
   * Observed through the API and the on-disk record, the way the rest of this file does — the store's
   * internals are not part of the contract these tests are pinning.
   */
  const SIDE = 'staging.test';

  /** The record as persisted, which is where a staged credential has to survive a restart. */
  function persisted(id) {
    const raw = readFileSync(path.join(context.runtimeDir, 'data', 'project-sides.json'), 'utf8');
    return JSON.parse(raw).sides[id];
  }

  async function sideWithCredential(app, token) {
    await request(app).post('/api/project-sides').set('Authorization', `Bearer ${token}`)
      .send({ server_name: SIDE, api_base_url: 'https://matrix.staging.test' });
    await request(app).put(`/api/project-sides/${SIDE}/credential`).set('Authorization', `Bearer ${token}`)
      .send({
        credential: {
          kind: 'appservice', asToken: 'live-as', hsToken: 'live-hs',
          namespace: '@ac_.*', senderLocalpart: 'hagency',
        },
      });
  }

  test('a second credential is STAGED, and the live one keeps working', async () => {
    const app = await boot({ env: { API_TOKEN: 'op-staging' } });
    await sideWithCredential(app, 'op-staging');
    expect(persisted(SIDE).credential.asToken).toBe('live-as');

    const res = await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-staging')
      .send({ url: 'http://127.0.0.1:8095' });

    // No 409 and no ?replace=true: issuing is safe now, so demanding a force flag would only be ceremony.
    expect(res.status).toBe(200);
    expect(res.body.staged).toBe(true);
    expect(res.body.stagedNote).toMatch(/has not changed/);

    // The credential in USE is untouched — the entire point.
    const record = persisted(SIDE);
    expect(record.credential.asToken).toBe('live-as');
    expect(record.pendingCredential).toBeTruthy();
    expect(record.pendingCredential.asToken).not.toBe('live-as');
  });

  test('the side reports that something is waiting to be installed', async () => {
    // Hidden, this would be a silent "why is my new token not working".
    const app = await boot({ env: { API_TOKEN: 'op-staging' } });
    await sideWithCredential(app, 'op-staging');
    await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-staging').send({ url: 'http://127.0.0.1:8095' });

    const res = await request(app).get(`/api/project-sides/${SIDE}`)
      .set('Authorization', 'Bearer op-staging');
    const side = res.body.side ?? res.body;
    expect(side).toMatchObject({ hasCredential: true, awaitingInstall: true });
    expect(side.awaitingInstallSince).toBeTruthy();
  });

  test('staging does not reset the verdict on the credential that is working', async () => {
    /*
     * `accessState` describes the LIVE credential. Resetting it because somebody generated a spare would
     * report a healthy side as unverified — the misleading-status defect arriving from the other direction.
     */
    const app = await boot({ env: { API_TOKEN: 'op-staging' } });
    await sideWithCredential(app, 'op-staging');

    /*
     * DRIVEN TO `accepted` FIRST, and that is what makes this test bite. A first version compared the state
     * against itself on a freshly created side — where it is `unverified` either way — so it passed even with
     * the reset put back in. Verified by mutation: a test that survives the defect it names is not testing it.
     */
    const hs = await fakeHomeserver((pathname) => {
      if (pathname === '/_matrix/client/v3/account/whoami') return [200, { user_id: `@hagency:${SIDE}` }];
      return [200, {}];
    });
    fake = hs;
    await request(app).post('/api/project-sides').set('Authorization', 'Bearer op-staging')
      .send({ server_name: SIDE, api_base_url: hs.baseUrl });
    await request(app).post(`/api/project-sides/${SIDE}/verify`).set('Authorization', 'Bearer op-staging').send({});
    expect(persisted(SIDE).accessState).toBe('accepted');

    await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-staging').send({ url: 'http://127.0.0.1:8095' });

    // Still accepted: the verdict describes the credential that is LIVE, and generating a spare changed none
    // of what that credential can do.
    expect(persisted(SIDE).accessState).toBe('accepted');
  });

  test('the FIRST credential is not staged — there is nothing to protect', async () => {
    // Staging a side with no live credential would leave it with none at all: unable to act, while holding a
    // credential it will not use.
    const app = await boot({ env: { API_TOKEN: 'op-staging' } });
    await request(app).post('/api/project-sides').set('Authorization', 'Bearer op-staging')
      .send({ server_name: SIDE, api_base_url: 'https://matrix.staging.test' });

    const res = await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-staging').send({ url: 'http://127.0.0.1:8095' });
    expect(res.body.staged).toBe(false);
    expect(res.body.stagedNote).toBeUndefined();

    /*
     * LIVE, not staged, and the API view is what proves it: `hasCredential` is what every gate reads, so a
     * first credential parked in `pendingCredential` would leave the side unable to act while appearing to
     * hold one. A first version only checked the record fields, and passed even when staging applied to the
     * first credential too — verified by mutation.
     */
    const view = await request(app).get(`/api/project-sides/${SIDE}`).set('Authorization', 'Bearer op-staging');
    expect(view.body.side ?? view.body).toMatchObject({ hasCredential: true, awaitingInstall: false });
    expect((persisted(SIDE).pendingCredential ?? null)).toBeNull();
  });

  test('withdrawing the credential clears the staged one too', async () => {
    // Otherwise a side with no credential still holds a token, and the next issue would stage against nothing.
    const app = await boot({ env: { API_TOKEN: 'op-staging' } });
    await sideWithCredential(app, 'op-staging');
    await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-staging').send({ url: 'http://127.0.0.1:8095' });
    expect(persisted(SIDE).pendingCredential).toBeTruthy();

    await request(app).put(`/api/project-sides/${SIDE}/credential`)
      .set('Authorization', 'Bearer op-staging').send({ credential: null });

    const record = persisted(SIDE);
    expect(record.credential ?? null).toBeNull();
    expect(record.pendingCredential ?? null).toBeNull();
  });

  test('exclusive defaults true and an explicit false overrides it — registration-file endpoint', async () => {
    /*
     * The impersonation guard pinned at the ISSUE boundary: the YAML a project installs says
     * `exclusive: true` unless the operator explicitly opted out, and the opt-out reaches the
     * rendered file. Both endpoints share generateRegistration, so the pairing test below covers
     * the other one.
     */
    const app = await boot({ env: { API_TOKEN: 'op-staging' } });
    await request(app).post('/api/project-sides').set('Authorization', 'Bearer op-staging')
      .send({ server_name: SIDE, api_base_url: 'https://matrix.staging.test' });

    const def = await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-staging').send({ url: 'http://127.0.0.1:8095' });
    expect(def.status).toBe(200);
    const yamlDefault = readFileSync(def.body.path, 'utf8');
    expect(yamlDefault).toMatch(/exclusive: true/);

    await request(app).put(`/api/project-sides/${SIDE}/credential`)
      .set('Authorization', 'Bearer op-staging').send({ credential: null });

    const off = await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-staging')
      .send({ url: 'http://127.0.0.1:8095', exclusive: false });
    expect(off.status).toBe(200);
    const yamlOff = readFileSync(off.body.path, 'utf8');
    expect(yamlOff).toMatch(/exclusive: false/);
  });

  test('exclusive defaults true and an explicit false overrides it — registration (response-body) endpoint', async () => {
    const app = await boot({ env: { API_TOKEN: 'op-staging' } });
    await request(app).post('/api/project-sides').set('Authorization', 'Bearer op-staging')
      .send({ server_name: SIDE, api_base_url: 'https://matrix.staging.test' });

    const def = await request(app).post(`/api/project-sides/${SIDE}/registration`)
      .set('Authorization', 'Bearer op-staging').send({ url: 'http://127.0.0.1:8095' });
    expect(def.status).toBe(200);
    expect(def.body.registrationYaml).toMatch(/exclusive: true/);

    await request(app).put(`/api/project-sides/${SIDE}/credential`)
      .set('Authorization', 'Bearer op-staging').send({ credential: null });

    const off = await request(app).post(`/api/project-sides/${SIDE}/registration`)
      .set('Authorization', 'Bearer op-staging')
      .send({ url: 'http://127.0.0.1:8095', exclusive: false });
    expect(off.status).toBe(200);
    expect(off.body.registrationYaml).toMatch(/exclusive: false/);
  });
});

describe('staging protects what works, not merely what exists', () => {
  /*
   * FOUND BY WALKING THE FLOW. A side whose credential verification had already found BLOCKED still had the
   * replacement staged behind it — so the operator generated a fix, read "the credential this side is USING has
   * not changed", and was left with the broken one live. The reassurance became the obstacle.
   *
   * `unverified` is deliberately still protected: it means nobody has asked yet, not that it fails, and
   * replacing an unexamined credential outright would recreate the original defect for anyone who had not run
   * verify.
   *
   * THE VERDICT IS PRODUCED, NOT PLANTED. A first version wrote `accessState` straight into the JSON on disk —
   * which the running store never re-reads, so both tests failed against correct code. Driving a real verify
   * against a homeserver that refuses is the only version that exercises the path an operator takes.
   */
  const SIDE = 'broken-live.test';

  async function sideWithRefusal(app, token, respond) {
    const hs = await fakeHomeserver(respond);
    fake = hs;
    await request(app).post('/api/project-sides').set('Authorization', `Bearer ${token}`)
      .send({
        server_name: SIDE,
        api_base_url: hs.baseUrl,
        credential: {
          kind: 'appservice', asToken: 'live-as', hsToken: 'live-hs',
          namespace: '@ac_.*', senderLocalpart: 'hagency',
        },
      });
    await request(app).post(`/api/project-sides/${SIDE}/verify`).set('Authorization', `Bearer ${token}`).send({});
  }

  test('a rejected credential is REPLACED, not staged behind', async () => {
    const app = await boot({ env: { API_TOKEN: 'op-broken' } });
    // 403 is the one status that IS a verdict on a credential.
    await sideWithRefusal(app, 'op-broken', () => [403, { errcode: 'M_FORBIDDEN', error: 'bad token' }]);

    const res = await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-broken').send({ url: 'http://127.0.0.1:8095' });
    expect(res.status).toBe(200);
    expect(res.body.staged).toBe(false);
    // And it SAYS the old one is gone: "not staged" otherwise has two causes an operator cannot tell apart.
    expect(res.body.replacedNote).toMatch(/rejected/);
  });

  test('an unverified credential is still staged behind, because nobody has asked yet', async () => {
    const app = await boot({ env: { API_TOKEN: 'op-broken' } });
    await request(app).post('/api/project-sides').set('Authorization', 'Bearer op-broken')
      .send({
        server_name: SIDE,
        api_base_url: 'https://matrix.broken-live.test',
        credential: {
          kind: 'appservice', asToken: 'live-as', hsToken: 'live-hs',
          namespace: '@ac_.*', senderLocalpart: 'hagency',
        },
      });

    const res = await request(app).post(`/api/project-sides/${SIDE}/registration-file`)
      .set('Authorization', 'Bearer op-broken').send({ url: 'http://127.0.0.1:8095' });
    expect(res.body.staged).toBe(true);
    expect(res.body.replacedNote).toBeUndefined();
  });
});

describe('what the committed figure is made of', () => {
  /*
   * `已承诺 200k` beside a project with nobody assigned. The number was right and told the operator nothing:
   * learning that three of those four commitments belonged to `e2e-probe-*` agents deleted hours earlier meant
   * fetching the engagement list, fetching the agent list, and cross-referencing them by hand.
   *
   * A delete now releases its commitments, so `orphanedCommitted` is 0 on a fleet running that code. This is
   * for the fleets that predate it — and for any future path that manages to leave one behind.
   */
  const LIVE = 'still-here';

  async function bootWithCommitments() {
    const app = await boot({
      agents: { [LIVE]: { name: LIVE, type: 'agent', kind: 'agent', capability: 'coding' } },
      rawDataFiles: {
        'engagements.json': JSON.stringify({
          version: 1,
          engagements: {
            kept: {
              id: 'kept', state: 'active', agent: LIVE, createdAt: 1,
              projectRoomId: `!room:${SERVER}`, allocatedTokens: 50000, role: 'documentation',
            },
            orphan: {
              id: 'orphan', state: 'active', agent: 'deleted-probe', createdAt: 1,
              projectRoomId: `!room:${SERVER}`, allocatedTokens: 150000, role: 'documentation',
            },
            done: {
              id: 'done', state: 'ended', agent: 'deleted-probe', createdAt: 1,
              projectRoomId: `!room:${SERVER}`, allocatedTokens: 900000, role: 'documentation',
            },
          },
          whitelist: {}, offers: {}, audit: [],
        }),
      },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    await request(app).put(`/api/project-sides/${SERVER}/allocation`).send({ allocated_tokens: 1000000 });
    return app;
  }

  test('the breakdown names each commitment and whether its agent still exists', async () => {
    const app = await bootWithCommitments();
    const res = await request(app).get(`/api/project-sides/${SERVER}/budget`);
    expect(res.status).toBe(200);

    const byId = Object.fromEntries((res.body.commitments ?? []).map((c) => [c.id, c]));
    expect(Object.keys(byId).sort()).toEqual(['kept', 'orphan']);   // `done` is ended, so it commits nothing
    expect(byId.kept.agentExists).toBe(true);
    expect(byId.orphan.agentExists).toBe(false);
    expect(byId.orphan.agent).toBe('deleted-probe');
    expect(res.body.orphanedCommitted).toBe(150000);
  });

  test('the breakdown sums to the committed figure it explains', async () => {
    /*
     * The property that makes it worth showing. A breakdown that can disagree with its own total is worse than
     * none — it makes the number look explained when it is not. Both come from one filter in the store, and
     * this is what holds that true.
     */
    const app = await bootWithCommitments();
    const res = await request(app).get(`/api/project-sides/${SERVER}/budget`);
    const sum = (res.body.commitments ?? []).reduce((n, c) => n + c.allocatedTokens, 0);
    expect(sum).toBe(res.body.committed);
    expect(res.body.committed).toBe(200000);
  });

  test('a healthy side reports zero orphaned, not a missing field', async () => {
    // The page renders this line only when it is above zero, so `undefined` and `0` would look identical on
    // screen and differ everywhere else. It is a number.
    const app = await bootWithCommitments();
    await request(app).post('/api/agents').send({ name: 'deleted-probe', role: 'documentation' });
    const res = await request(app).get(`/api/project-sides/${SERVER}/budget`);
    expect(res.body.orphanedCommitted).toBe(0);
    expect(res.body.commitments.every((c) => c.agentExists)).toBe(true);
  });

  test('a dispatch payload does NOT carry the breakdown', async () => {
    /*
     * A CORRECTION, kept as a test. The first version put the breakdown in `sideBudgetFor`, which widened all
     * four of its callers — one of which builds the payload handed to a DISPATCHED RUNNER. That would have told
     * a borrowed agent's wrapper the names and sizes of every other commitment on that customer's side. The
     * numbers are an internal primitive; who holds them is an operator-facing read.
     */
    const source = readFileSync(new URL('../backend-v2.js', import.meta.url), 'utf8');
    const helper = /function sideBudgetFor\(sideId\) \{[\s\S]*?\n\}/.exec(source)?.[0] ?? '';
    expect(helper).not.toMatch(/commitments|orphanedCommitted/);
    expect(source).toMatch(/sideCommitmentsFor\(side\)/);
  });
});

describe('a project with an approved engagement nobody attached', () => {
  /*
   * THE TWO TRUE STATEMENTS THAT MADE NO SENSE TOGETHER. On the live fleet, one project read 「还没派人」
   * in its staff column and 「已承诺 50k」 in its budget column. Both were correct: `agents` comes from
   * BINDINGS, and an engagement going active only creates one when an owner can be resolved — without
   * `HAGENCY_OWNER_MXID` the bind fails and records why on the engagement.
   *
   * The reason was already in the data (`bindError: "no owner known for this agent: set
   * HAGENCY_OWNER_MXID and HAGENCY_OWNER_DM_ROOM…"`) and no page read it, though the approval path's own
   * comment says the failure is "shown in the console". So the operator saw a project that looked
   * untouched, with money promised against it, and nothing pointing at the setting that would fix it.
   */
  const AGENT = 'promised-nobody-attached';
  const ROOM = `!proj:${SERVER}`;

  async function bootWithApprovedUnbound({ bound = false, bindError = 'no owner known for this agent', bridgeSecret = null } = {}) {
    const app = await boot({
      agents: { [AGENT]: { name: AGENT, type: 'agent', kind: 'agent', capability: 'coding' } },
      ...(bridgeSecret ? { env: { MATRIX_BRIDGE_SECRET: bridgeSecret } } : {}),
      rawDataFiles: {
        'engagements.json': JSON.stringify({
          version: 1,
          engagements: {
            promised: {
              id: 'promised', state: 'active', agent: AGENT, createdAt: 1, role: 'documentation',
              projectRoomId: ROOM, allocatedTokens: 50000, bound, bindError,
            },
          },
          whitelist: {}, offers: {}, audit: [],
        }),
      },
    });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    await request(app).put(`/api/project-sides/${SERVER}/allocation`).send({ allocated_tokens: 1000000 });
    await request(app).post(`/api/project-sides/${SERVER}/projects`)
      .send({ name: 'a project', room_id: ROOM });
    return app;
  }

  const projectFrom = async (app) => {
    const res = await request(app).get('/api/project-sides');
    const side = (res.body.sides ?? []).find((s) => s.id === SERVER);
    return (side?.projects ?? [])[0];
  };

  test('it says who was approved and why they are not attached', async () => {
    const project = await projectFrom(await bootWithApprovedUnbound());
    expect(project.agents).toEqual([]);              // still not staff: nothing can reach the project
    expect(project.awaitingBind).toHaveLength(1);
    expect(project.awaitingBind[0].agent).toBe(AGENT);
    expect(project.awaitingBind[0].bindError).toMatch(/no owner known/);
  });

  test('an engagement that DID bind is staff, not awaiting', async () => {
    /*
     * The distinction has to hold in both directions, or the new line would appear beside every healthy
     * project and stop meaning anything. `bound: true` belongs to the binding store's answer.
     */
    const project = await projectFrom(await bootWithApprovedUnbound({ bound: true, bindError: null }));
    expect(project.awaitingBind).toEqual([]);
  });

  test('an agent the project can already reach is NOT reported as awaiting', async () => {
    /*
     * FOUND ON A SECOND FLEET, on real data, after the first version shipped. One project listed `biglittle`
     * as staff and twice under awaiting: it had a binding from an earlier engagement, and two later
     * engagements approved without an owner each recorded `bound: false` about an agent the project could
     * already reach.
     *
     * The binding store is the authority on attachment; the engagement's flag records only what happened at
     * approval time. Trusting the flag alone produces exactly the false alarm this field exists to avoid — a
     * line beside healthy projects stops meaning anything.
     */
    const app = await bootWithApprovedUnbound({ bridgeSecret: 'secret-for-awaiting' });
    // A binding for the same agent and room, which is what `agents` reads and what makes the project reachable.
    await request(app).put('/api/approval-bindings')
      .set('X-Bridge-Secret', 'secret-for-awaiting')
      .send({
        agent: AGENT, project: 'a project', projectRoomId: ROOM,
        ownerMxid: `@owner:${SERVER}`, ownerDmRoomId: `!dm:${SERVER}`,
      }).expect(200);

    const project = await projectFrom(app);
    expect(project.agents.map((a) => a.name)).toEqual([AGENT]);   // reachable: it is staff
    expect(project.awaitingBind).toEqual([]);                     // and therefore not awaiting
  });

  test('a project with no engagement at all is still plainly unstaffed', async () => {
    // 「还没派人」 must survive. It is the honest answer when nobody has been promised, and replacing it
    // everywhere with the new wording would trade one misleading cell for another.
    const app = await boot({ agents: {} });
    await request(app).post('/api/project-sides')
      .send({ server_name: SERVER, api_base_url: 'http://127.0.0.1:8008' });
    await request(app).post(`/api/project-sides/${SERVER}/projects`)
      .send({ name: 'untouched', room_id: `!empty:${SERVER}` });
    const project = await projectFrom(app);
    expect(project.agents).toEqual([]);
    expect(project.awaitingBind).toEqual([]);
  });
});
