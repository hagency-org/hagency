/*
 * The agent Matrix credential is SUPPLIED, never derived (ADR-014 decision 3).
 *
 * What was deleted: an agent's Matrix password used to be sha256(MATRIX_AGENT_PASSWORD_SECRET +
 * ':' + agentName). One operator secret stood behind every agent identity, and three properties
 * condemned it — the secret could never be rotated (rotating it changes every derived password at
 * once, so the bridge can neither log in to the existing accounts nor re-register them, because
 * the usernames are taken); the credentials were not revocable (revoking a token achieves nothing
 * while the password is re-derivable from .env); and it required account-CREATION privilege on the
 * homeserver, which no third-party project grants an external bridge.
 *
 * What replaced it: a stored access token, plus MATRIX_AGENT_TOKEN_<AGENT> as the input path for a
 * new or rotated one. This file pins the properties of that replacement that would rot silently,
 * because each of them fails in a direction that looks like something else:
 *
 *   - a transient homeserver failure must NOT be reported as a dead token (it would send an
 *     operator to replace a credential that was fine — and replacing means human account work);
 *   - a bad env value must never displace a working stored token;
 *   - a missing credential must REFUSE, not return something falsy that a caller sends with.
 *
 * Nothing here contacts a homeserver: `fetch` is stubbed, which is the whole seam, since the
 * bridge reaches Matrix through bare `fetch` inside `fetchWithRateLimit`.
 */

import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, test } from 'vitest';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const HOMESERVER = 'https://hs.test';
const AGENT = 'wf_coordinator';
const ENV_VAR = 'MATRIX_AGENT_TOKEN_WF_COORDINATOR';

let mod;
let MatrixBridge;
let runtimeDir;
const savedEnv = {};
const realFetch = globalThis.fetch;

/**
 * whoami answers per token: 'ok' → accepted, 'dead' → 401 JSON, 'down' → network throw,
 * 'server-error' → 502, 'html-401' → 401 whose body is NOT JSON (a proxy or non-conforming server).
 */
let tokenBehaviour;
let whoamiCalls;
/** Per-token MXID, so a token can be made to belong to a DIFFERENT agent. */
let whoamiIdentity;
/** Profile display-name PUTs, to prove a refused token never renames anyone. */
let displayNameCalls;

function stubFetch() {
  globalThis.fetch = async (url, init) => {
    const authorization = init?.headers?.Authorization || '';
    const token = authorization.replace(/^Bearer\s+/, '');

    if (String(url).includes('/account/whoami')) {
      whoamiCalls.push(token);
      const behaviour = tokenBehaviour.get(token) ?? 'dead';
      if (behaviour === 'down') throw new Error('fetch failed');
      if (behaviour === 'dead') {
        return {
          ok: false,
          status: 401,
          json: async () => ({ errcode: 'M_UNKNOWN_TOKEN', error: 'Invalid access token' }),
        };
      }
      if (behaviour === 'server-error') {
        return { ok: false, status: 502, json: async () => ({ error: 'bad gateway' }) };
      }
      if (behaviour === 'html-401') {
        // The body is not JSON: res.json() rejects. The STATUS is the reliable signal.
        return {
          ok: false,
          status: 401,
          json: async () => { throw new SyntaxError('Unexpected token < in JSON at position 0'); },
        };
      }
      return {
        ok: true,
        status: 200,
        json: async () => ({
          user_id: whoamiIdentity.get(token) || `@ac_${AGENT}:hs.test`,
          device_id: 'DEV1',
        }),
      };
    }

    if (String(url).includes('/displayname')) {
      displayNameCalls.push(token);
      return { ok: true, status: 200, json: async () => ({}) };
    }

    // Anything else — accept quietly.
    return { ok: true, status: 200, json: async () => ({}) };
  };
}

beforeAll(async () => {
  runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'agent-credential-'));
  for (const k of ['HAGENCY_RUNTIME_DIR', 'MATRIX_HOMESERVER', 'MATRIX_AGENT_PREFIX', 'MATRIX_SERVER_NAME', ENV_VAR]) {
    savedEnv[k] = process.env[k];
  }
  process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
  process.env.MATRIX_HOMESERVER = HOMESERVER;
  process.env.MATRIX_AGENT_PREFIX = 'ac_';
  process.env.MATRIX_SERVER_NAME = 'hs.test';
  delete process.env[ENV_VAR];

  const url = pathToFileURL(path.resolve('bridge-matrix.js')).href;
  mod = await import(`${url}?agent-credential-test=1`);
  MatrixBridge = mod.MatrixBridge;
});

afterAll(() => {
  globalThis.fetch = realFetch;
  rmSync(runtimeDir, { recursive: true, force: true });
  for (const [k, v] of Object.entries(savedEnv)) {
    if (v === undefined) delete process.env[k];
    else process.env[k] = v;
  }
});

beforeEach(() => {
  tokenBehaviour = new Map();
  whoamiCalls = [];
  whoamiIdentity = new Map();
  displayNameCalls = [];
  stubFetch();
  // Clear stored tokens between cases — the module holds one live `state`.
  const tokens = mod.agentTokenStateForTest();
  for (const k of Object.keys(tokens)) delete tokens[k];
  delete process.env[ENV_VAR];
});

afterEach(() => {
  delete process.env[ENV_VAR];
});

describe('the derived-password mechanism is gone', () => {
  test('no derivation symbol survives, and no env var can revive it', () => {
    /*
     * A source-level assertion on purpose. The security property is the ABSENCE of a code path, and
     * absence is what a behavioural test cannot see: a re-added `deriveAgentPassword` would sit
     * unexercised beside a passing suite. Reading the file is the only way to notice it came back.
     *
     * HONEST LIMIT, and it was raised in review: a grep cannot PROVE absence. Derivation could
     * return under different names, inline, via `process.env['...']`, or split across lines, and
     * these assertions would pass. They are a regression tripwire for the obvious reintroduction —
     * someone restoring the deleted code or its variables — not a proof. The real guarantee is
     * behavioural and lives in the ensureAgentAccount cases below: a credential is only ever read,
     * validated and adopted, and the function has no code path that produces one.
     */
    const src = readBridgeSource();
    for (const symbol of [
      'deriveAgentPassword',
      'legacyAgentPassword',
      'agentPasswordCandidates',
      'MATRIX_AGENT_PASSWORD_SECRET',
      'MATRIX_AGENT_PASSWORD_TEMPLATE',
      'MATRIX_ALLOW_LEGACY_AGENT_PASSWORD',
    ]) {
      // The prose block explaining the removal names the deleted env vars, so match CODE use:
      // an identifier that is read, not one inside a comment.
      const codeUse = new RegExp(`(?<!\\* +)(?<!\\* {2})\\b${symbol}\\b\\s*[(=)]`);
      const offending = src
        .split('\n')
        .filter((line) => !line.trimStart().startsWith('*') && !line.trimStart().startsWith('//'))
        .filter((line) => codeUse.test(line));
      expect(offending, `${symbol} is referenced in code again`).toEqual([]);
    }
  });

  test('process.env is never consulted for a password secret, by any access form', () => {
    const src = readBridgeSource();
    // Dotted and bracketed access, since `process.env['MATRIX_AGENT_PASSWORD_SECRET']` reads the
    // same variable and the dotted-only check would miss it.
    expect(src).not.toMatch(/process\.env\.MATRIX_AGENT_PASSWORD/);
    expect(src).not.toMatch(/process\.env\.MATRIX_ALLOW_LEGACY_AGENT_PASSWORD/);
    expect(src).not.toMatch(/process\.env\s*\[\s*['"`][^'"`]*(AGENT_PASSWORD|ALLOW_LEGACY)/);
  });

  test('no hash is computed over anything password-shaped', () => {
    /*
     * The mechanism, not the name: the defect was sha256(secret + ':' + agentName), so an inline
     * reintroduction under a fresh helper name is caught by looking for a digest taken over a
     * password-ish input rather than for the deleted identifiers.
     */
    const src = readBridgeSource();
    const lines = src.split('\n');
    const suspicious = lines.filter((line, i) => {
      if (line.trimStart().startsWith('*') || line.trimStart().startsWith('//')) return false;
      if (!/createHash|createHmac/.test(line)) return false;
      // The digest input may be on the next couple of lines (chained .update calls).
      return /password|passwd|secret/i.test(lines.slice(i, i + 3).join('\n'));
    });
    expect(suspicious, `a digest is being taken over something password-shaped:\n${suspicious.join('\n')}`).toEqual([]);
  });
});

describe('agentTokenFromEnv', () => {
  test('reads MATRIX_AGENT_TOKEN_<AGENT>, upper-cased with non-alphanumerics as underscores', () => {
    process.env[ENV_VAR] = 'syt_supplied';
    expect(mod.agentTokenFromEnvForTest(AGENT)).toBe('syt_supplied');
    // Same variable regardless of how the caller cased the agent name.
    expect(mod.agentTokenFromEnvForTest('WF_Coordinator')).toBe('syt_supplied');
  });

  test('trims, and treats whitespace-only as absent', () => {
    /*
     * A pasted token routinely carries a trailing newline; sending `Bearer syt_x\n` fails
     * authentication in a way that reads as a bad token rather than as a bad copy-paste. Note this
     * TRIMS but never truncates — the same distinction that mattered for API_TOKEN.
     */
    process.env[ENV_VAR] = '  syt_supplied\n';
    expect(mod.agentTokenFromEnvForTest(AGENT)).toBe('syt_supplied');
    process.env[ENV_VAR] = '   ';
    expect(mod.agentTokenFromEnvForTest(AGENT)).toBeNull();
  });

  test('an unset variable, or a nameless agent, is null — not undefined and not a throw', () => {
    expect(mod.agentTokenFromEnvForTest(AGENT)).toBeNull();
    expect(mod.agentTokenFromEnvForTest('')).toBeNull();
    expect(mod.agentTokenFromEnvForTest(null)).toBeNull();
  });

  test('a mixed-case agent name composes a LOWERCASE mxid', () => {
    /*
     * Matrix requires a lowercase localpart, so a homeserver normalises on registration — Palpo
     * accepted `ac_BigLittle` and returned `@ac_biglittle:…`. Composing with the agent's own casing
     * therefore produced an ID that could never exist, and the identity check refused a valid
     * credential for an agent named `BigLittle`. Invisible while every agent was lower case.
     */
    expect(mod.agentUserIdForTest('BigLittle')).toBe('@ac_biglittle:hs.test');
    expect(mod.agentUserIdForTest('wf_coordinator')).toBe('@ac_wf_coordinator:hs.test');
  });

  test('per agent: one agent\'s variable never answers for another', () => {
    // The property that replaced the single master secret. If this ever collapsed to one shared
    // variable, revoking one agent's token would stop being a per-agent action.
    process.env[ENV_VAR] = 'syt_coordinator';
    expect(mod.agentTokenFromEnvForTest('other_agent')).toBeNull();
  });
});

describe('isMatrixAuthFailure', () => {
  test('401 and 403 are credential rejections', () => {
    expect(mod.isMatrixAuthFailureForTest({ status: 401 })).toBe(true);
    expect(mod.isMatrixAuthFailureForTest({ status: 403 })).toBe(true);
  });

  test('everything else is transient — including shapes it has never seen', () => {
    /*
     * Answers FALSE for the unknown case deliberately: an unrecognised failure treated as
     * transient causes a retry, while the same failure treated as a verdict tells an operator to
     * go replace a working credential by hand. The costs are not symmetric.
     */
    for (const e of [{ status: 500 }, { status: 502 }, { status: 429 }, new Error('fetch failed'), {}, null, undefined]) {
      expect(mod.isMatrixAuthFailureForTest(e)).toBe(false);
    }
  });
});

describe('ensureAgentAccount — supplied credentials only', () => {
  test('a stored token that still works is returned, and nothing is minted', async () => {
    mod.agentTokenStateForTest()[AGENT] = 'syt_stored';
    tokenBehaviour.set('syt_stored', 'ok');

    await expect(mod.ensureAgentAccountForTest(AGENT)).resolves.toBe('syt_stored');
    expect(whoamiCalls).toEqual(['syt_stored']);
  });

  test('no stored token and no env var REFUSES, naming the variable to set', async () => {
    /*
     * Refuses rather than resolving null: a caller that received a falsy token would go on to send
     * with `Bearer undefined`, and an unauthenticated request is the class of bug that surfaces as
     * an empty result rather than as an error.
     */
    await expect(mod.ensureAgentAccountForTest(AGENT)).rejects.toThrow(/no Matrix access token is stored/);

    let caught;
    try { await mod.ensureAgentAccountForTest(AGENT); } catch (e) { caught = e; }
    expect(caught.name).toBe('AgentCredentialMissingError');
    expect(caught.needsProvisioning).toBe(true);
    expect(caught.agentName).toBe(AGENT);
    // Actionable: the operator is told the exact variable and the exact MXID.
    expect(caught.message).toContain(ENV_VAR);
    expect(caught.message).toContain(`@ac_${AGENT}:hs.test`);
    // And it must not have tried to reach the homeserver at all.
    expect(whoamiCalls).toEqual([]);
  });

  test('an env-supplied token is validated and adopted into state', async () => {
    process.env[ENV_VAR] = 'syt_supplied';
    tokenBehaviour.set('syt_supplied', 'ok');

    await expect(mod.ensureAgentAccountForTest(AGENT)).resolves.toBe('syt_supplied');
    // Adopted, so the next call is a single whoami rather than a re-read of the environment.
    expect(mod.agentTokenStateForTest()[AGENT]?.accessToken).toBe('syt_supplied');
    expect(mod.agentTokenStateForTest()[AGENT]?.credentialGeneration).toMatch(/^[a-f0-9]{32}$/);
  });

  test('rotation: a dead stored token falls through to the fresh env one', async () => {
    /*
     * The rotation story in one case. The operator edits one variable; the dead stored token is
     * tried, rejected, and REPLACED. This is what makes the credential revocable — the property
     * the derived password could never have, since its password was re-derivable from .env.
     */
    mod.agentTokenStateForTest()[AGENT] = 'syt_revoked';
    tokenBehaviour.set('syt_revoked', 'dead');
    process.env[ENV_VAR] = 'syt_fresh';
    tokenBehaviour.set('syt_fresh', 'ok');

    await expect(mod.ensureAgentAccountForTest(AGENT)).resolves.toBe('syt_fresh');
    expect(whoamiCalls).toEqual(['syt_revoked', 'syt_fresh']);
    expect(mod.agentTokenStateForTest()[AGENT]?.accessToken).toBe('syt_fresh');
  });

  test('a bad env value never displaces a working stored token', async () => {
    /*
     * Ordering matters: stored is tried first, so a typo in .env is never even reached while the
     * stored credential works. If this inverted, one mistyped variable would evict a live token
     * that nothing can re-mint.
     */
    mod.agentTokenStateForTest()[AGENT] = 'syt_stored';
    tokenBehaviour.set('syt_stored', 'ok');
    process.env[ENV_VAR] = 'syt_typo';
    tokenBehaviour.set('syt_typo', 'dead');

    await expect(mod.ensureAgentAccountForTest(AGENT)).resolves.toBe('syt_stored');
    expect(whoamiCalls).toEqual(['syt_stored']);
    expect(mod.agentTokenStateForTest()[AGENT]?.accessToken).toBe('syt_stored');
  });

  test('a homeserver outage is a transport error, NOT a dead credential', async () => {
    /*
     * The most consequential distinction in the file. During an outage every token fails, and if
     * that were reported as "your credentials were revoked" the operator would be sent to reissue
     * tokens for the whole fleet — by hand, on a homeserver, for credentials that were fine.
     */
    mod.agentTokenStateForTest()[AGENT] = 'syt_stored';
    tokenBehaviour.set('syt_stored', 'down');

    let caught;
    try { await mod.ensureAgentAccountForTest(AGENT); } catch (e) { caught = e; }
    expect(caught).toBeTruthy();
    expect(caught.name).not.toBe('AgentCredentialMissingError');
    expect(caught.needsProvisioning).toBeUndefined();
    // The stored token is untouched: an outage must not evict a credential.
    expect(mod.agentTokenStateForTest()[AGENT]?.accessToken).toBe('syt_stored');
  });

  test('an outage does not silently burn through to the env candidate', async () => {
    /*
     * If the transport error were swallowed and the loop continued, an outage would exhaust every
     * candidate and end at "all credentials rejected" — the exact misdiagnosis above, arrived at
     * by a different route. The first non-auth failure must stop the loop.
     */
    mod.agentTokenStateForTest()[AGENT] = 'syt_stored';
    tokenBehaviour.set('syt_stored', 'down');
    process.env[ENV_VAR] = 'syt_supplied';
    tokenBehaviour.set('syt_supplied', 'ok');

    await expect(mod.ensureAgentAccountForTest(AGENT)).rejects.toThrow(/fetch failed/);
    expect(whoamiCalls).toEqual(['syt_stored']);
  });

  test('a 5xx is also transient, not a verdict on the token', async () => {
    mod.agentTokenStateForTest()[AGENT] = 'syt_stored';
    tokenBehaviour.set('syt_stored', 'server-error');

    let caught;
    try { await mod.ensureAgentAccountForTest(AGENT); } catch (e) { caught = e; }
    expect(caught.needsProvisioning).toBeUndefined();
    expect(caught.status).toBe(502);
  });

  test('when every available token is rejected, it says so and names the variable', async () => {
    mod.agentTokenStateForTest()[AGENT] = 'syt_revoked';
    tokenBehaviour.set('syt_revoked', 'dead');
    process.env[ENV_VAR] = 'syt_also_revoked';
    tokenBehaviour.set('syt_also_revoked', 'dead');

    let caught;
    try { await mod.ensureAgentAccountForTest(AGENT); } catch (e) { caught = e; }
    expect(caught.name).toBe('AgentCredentialMissingError');
    expect(caught.needsProvisioning).toBe(true);
    expect(caught.message).toMatch(/rejected by the homeserver/);
    expect(caught.message).toContain(ENV_VAR);
    expect(whoamiCalls).toEqual(['syt_revoked', 'syt_also_revoked']);
    /*
     * And NOTHING unvalidated was written. State still holds the original stored token, not the
     * env candidate that was tried and rejected — adoption happens only after the homeserver
     * accepts, so a failed candidate never becomes the recorded credential. Without this
     * assertion, moving the state write above the whoami passes every other test in this file.
     */
    expect(mod.agentTokenStateForTest()[AGENT]?.accessToken).toBe('syt_revoked');
  });

  test('an identical stored and env token is not rejected twice', async () => {
    /*
     * The dedup, observable only on the failure path: after adoption the two values are the same,
     * and presenting the same dead token twice would log two rejections for one credential and
     * double the requests during an outage of a revoked account.
     */
    mod.agentTokenStateForTest()[AGENT] = 'syt_dead';
    process.env[ENV_VAR] = 'syt_dead';
    tokenBehaviour.set('syt_dead', 'dead');

    await expect(mod.ensureAgentAccountForTest(AGENT)).rejects.toThrow(/rejected by the homeserver/);
    expect(whoamiCalls).toEqual(['syt_dead']);
  });

  test('a stored token equal to the env value is tried once, not twice', async () => {
    // The steady state after adoption. A duplicate candidate would double every whoami.
    mod.agentTokenStateForTest()[AGENT] = 'syt_same';
    process.env[ENV_VAR] = 'syt_same';
    tokenBehaviour.set('syt_same', 'ok');

    await expect(mod.ensureAgentAccountForTest(AGENT)).resolves.toBe('syt_same');
    expect(whoamiCalls).toEqual(['syt_same']);
    expect(mod.agentTokenStateForTest()[AGENT]?.credentialGeneration).toMatch(/^[a-f0-9]{32}$/);
  });

  test('re-adopting the same token does not rewrite state', async () => {
    /*
     * saveState() writes bridge-state.json, which holds every credential. Rewriting it on each
     * call would put the file — and its 0600 temp-then-rename dance — on a hot path for no change.
     */
    mod.agentTokenStateForTest()[AGENT] = 'syt_stored';
    tokenBehaviour.set('syt_stored', 'ok');
    await mod.ensureAgentAccountForTest(AGENT);
    await mod.ensureAgentAccountForTest(AGENT);
    expect(mod.agentTokenStateForTest()[AGENT]?.accessToken).toBe('syt_stored');
  });
});

/** The bridge source, for the two absence assertions above. */
function readBridgeSource() {
  return readFileSync(path.resolve('bridge-matrix.js'), 'utf8');
}

describe('_upgradeLegacyDmRoom — the stale-account cleanup must not evict a real agent', () => {
  /*
   * This routine removes an `@ac_<humanName>` account from a legacy DM room by having it
   * self-leave. It needed a credential for that account, and used to DERIVE one; it now uses the
   * stored token, which changes the risk profile rather than the logic:
   *
   *   deriving required MATRIX_AGENT_PASSWORD_SECRET, and with that unset — the state of the
   *   deployment this was written against — `agentPasswordCandidates` returned [] and the block
   *   bailed out. The eviction was possible in principle and dormant in practice.
   *
   *   a stored token is not optional in that way: EVERY live agent has one. So switching to stored
   *   tokens re-arms the path, and `ensureHumanDmRoom` can reach it with an agent as `humanName`
   *   because `forceAgentName` sets `toIsAgent = false` without checking whether that side is
   *   actually an agent.
   *
   * Hence the guard, and hence this test: the failure it prevents is a working agent being made to
   * leave a room it belongs in — which nothing would undo, and which would read as the agent having
   * gone quiet.
   */
  function cleanupHarness({ knownAgent, storedToken }) {
    const bridge = new MatrixBridge();
    const staleUserId = `@ac_victim:hs.test`;
    const calls = [];

    bridge.getBotToken = () => 'syt_bot';
    bridge.getAgentToken = () => null;
    bridge.isKnownAgentName = (name) => knownAgent && name === 'victim';
    bridge.botClient = { getJoinedRoomMembers: async () => [staleUserId, '@ac_owner:hs.test'] };
    if (storedToken) mod.agentTokenStateForTest().victim = storedToken;

    globalThis.fetch = async (url, init) => {
      const u = String(url);
      calls.push(`${init?.method || 'GET'} ${u.replace('https://hs.test', '')}`);
      if (u.includes('/state/m.room.name') && !init?.method) {
        return { ok: true, status: 200, json: async () => ({ name: 'DM: owner' }) };
      }
      return { ok: true, status: 200, json: async () => ({}) };
    };
    return { bridge, calls, staleUserId };
  }

  const leaveCalls = (calls) => calls.filter((c) => c.includes('/leave'));

  test('a known agent is left in the room, even though a usable token exists', async () => {
    const { bridge, calls } = cleanupHarness({ knownAgent: true, storedToken: 'syt_victim' });
    await bridge._upgradeLegacyDmRoom('!room:hs.test', 'owner', 'victim');
    expect(leaveCalls(calls), 'a live agent was made to leave').toEqual([]);
  });

  test('a genuine stale account with a stored token does self-leave', async () => {
    // The guard must not disable the cleanup outright — only exempt real agents.
    const { bridge, calls } = cleanupHarness({ knownAgent: false, storedToken: 'syt_victim' });
    await bridge._upgradeLegacyDmRoom('!room:hs.test', 'owner', 'victim');
    expect(leaveCalls(calls).length).toBe(1);
  });

  test('with no stored token it warns and leaves the room alone — it cannot derive one', async () => {
    const { bridge, calls } = cleanupHarness({ knownAgent: false, storedToken: null });
    await bridge._upgradeLegacyDmRoom('!room:hs.test', 'owner', 'victim');
    expect(leaveCalls(calls)).toEqual([]);
  });

  test('the stored token is never logged out — that would take the agent offline fleet-wide', async () => {
    /*
     * The old code opened a throwaway login and closed it. Reusing the AGENT'S credential means a
     * logout would invalidate the real thing, and nothing can re-mint it, so a cosmetic cleanup
     * would end with an agent needing human re-provisioning.
     */
    const { bridge, calls } = cleanupHarness({ knownAgent: false, storedToken: 'syt_victim' });
    await bridge._upgradeLegacyDmRoom('!room:hs.test', 'owner', 'victim');
    expect(calls.filter((c) => c.includes('/logout'))).toEqual([]);
  });
});

describe('codex review findings', () => {
  /*
   * Each case below pins a defect an adversarial review found in the first version of this change.
   * They are grouped because they share one root: removing the re-minting fallback turned several
   * merely-inefficient paths into permanently destructive ones, and turned an expected condition
   * (no credential) into a thrown error on code paths written when a throw here was near-impossible.
   */

  test('F1: a valid token belonging to ANOTHER agent is refused, not adopted', async () => {
    /*
     * Validity is not identity. One variable per agent makes a paste error easy, and the variable
     * names are not injective — `octos-agent` and `octos_agent` both read
     * MATRIX_AGENT_TOKEN_OCTOS_AGENT — so a token can arrive under the wrong agent's name with
     * nothing malformed about it. Adopting it would send this agent's messages AS the other one and
     * rename the other one's profile on the way, both silently.
     */
    process.env[ENV_VAR] = 'syt_someone_else';
    tokenBehaviour.set('syt_someone_else', 'ok');
    whoamiIdentity.set('syt_someone_else', '@ac_other_agent:hs.test');

    let caught;
    try { await mod.ensureAgentAccountForTest(AGENT); } catch (e) { caught = e; }
    expect(caught?.name).toBe('AgentCredentialMissingError');
    expect(caught.message).toContain('@ac_other_agent:hs.test');
    expect(caught.message).toContain(`@ac_${AGENT}:hs.test`);
    // Never adopted, so the wrong identity cannot be used later.
    expect(mod.agentTokenStateForTest()[AGENT]).toBeUndefined();
  });

  test('F1: a mismatched token does not get the display name of the agent that requested it', async () => {
    // The rename is the destructive half — it edits the OTHER account's profile.
    process.env[ENV_VAR] = 'syt_someone_else';
    tokenBehaviour.set('syt_someone_else', 'ok');
    whoamiIdentity.set('syt_someone_else', '@ac_other_agent:hs.test');
    await mod.ensureAgentAccountForTest(AGENT).catch(() => {});
    expect(displayNameCalls).toEqual([]);
  });

  test('F9: a 401 whose body is not JSON is still classified as a dead credential', async () => {
    /*
     * `await res.json()` used to run before the status was inspected, so an empty or HTML 401 — a
     * proxy, a non-conforming server — threw a SyntaxError carrying no `.status`. That is
     * classified transient, so the dead token is retried forever and the fresh env candidate is
     * never reached: rotation silently cannot recover.
     */
    mod.agentTokenStateForTest()[AGENT] = 'syt_html401';
    tokenBehaviour.set('syt_html401', 'html-401');
    process.env[ENV_VAR] = 'syt_fresh';
    tokenBehaviour.set('syt_fresh', 'ok');

    await expect(mod.ensureAgentAccountForTest(AGENT)).resolves.toBe('syt_fresh');
    expect(whoamiCalls).toEqual(['syt_html401', 'syt_fresh']);
  });

  test('F6: the env var name comes from one place, and collisions are visible', () => {
    // Two distinct agents mangling to one variable is the collision the identity check fails closed
    // on. Asserting it here documents that the mangling is lossy rather than pretending otherwise.
    expect(mod.agentTokenEnvVarNameForTest('wf_coordinator')).toBe(ENV_VAR);
    expect(mod.agentTokenEnvVarNameForTest('wf-coordinator')).toBe(ENV_VAR);
    expect(mod.agentTokenEnvVarNameForTest('')).toBeNull();
    // And the reader uses exactly that name — no second copy of the mangling to drift.
    process.env[mod.agentTokenEnvVarNameForTest('wf-coordinator')] = 'syt_x';
    expect(mod.agentTokenFromEnvForTest('wf_coordinator')).toBe('syt_x');
  });
});
