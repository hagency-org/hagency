/*
 * An agent's Matrix credential is a RECORD, and old state files migrate into it.
 *
 * ADR-014 decision 4, whose status line read "decided, not built" with the evidence
 * "`state.agentTokens[name]` is a bare access-token string, not `{ homeserver, accessToken }`".
 * ADR-016 made it load-bearing rather than tidy: once project homeservers are not assumed to
 * federate, a token only means anything against the server that issued it, and a map of bare strings
 * cannot say which server that is.
 *
 * THE MIGRATION IS THE RISK, not the shape. `bridge-state.json` may be the only copy of credentials
 * nothing can re-mint (ADR-014 decision 3), and this change rewrites how every one of them is read.
 * A migration that dropped an entry, or pointed one at the wrong homeserver, would be indistinguishable
 * from a revoked token at the next startup — so what is asserted here is mostly what SURVIVES.
 */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

let runtimeDir;
let dataDir;
let statePath;
const savedEnv = {};
let importCounter = 0;

/** A fresh module instance bound to a fresh runtime dir — `state` is module-level, so it must be. */
async function loadBridge() {
  const url = pathToFileURL(path.resolve('bridge-matrix.js')).href;
  importCounter += 1;
  return import(`${url}?credrecord=${importCounter}`);
}

const writeState = (agentTokens, extra = {}) => writeFileSync(
  statePath,
  JSON.stringify({ botToken: 'syt_bot', agentTokens, ...extra }),
  { mode: 0o600 },
);

beforeEach(() => {
  runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'cred-record-'));
  // DATA_DIR is `<runtime>/data/matrix` — the bridge namespaces its own state under the runtime root.
  dataDir = path.join(runtimeDir, 'data', 'matrix');
  mkdirSync(dataDir, { recursive: true });
  statePath = path.join(dataDir, 'bridge-state.json');
  for (const k of ['HAGENCY_RUNTIME_DIR', 'MATRIX_HOMESERVER', 'MATRIX_AGENT_PREFIX', 'MATRIX_SERVER_NAME']) {
    savedEnv[k] = process.env[k];
  }
  process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
  process.env.MATRIX_HOMESERVER = 'https://hs.test';
  process.env.MATRIX_AGENT_PREFIX = 'ac_';
  process.env.MATRIX_SERVER_NAME = 'hs.test';
});

afterEach(() => {
  rmSync(runtimeDir, { recursive: true, force: true });
  for (const [k, v] of Object.entries(savedEnv)) {
    if (v === undefined) delete process.env[k];
    else process.env[k] = v;
  }
  vi.restoreAllMocks();
});

describe('migrating a bare token string', () => {
  test('it becomes a record pointed at THIS deployment, and the token is unchanged', async () => {
    /*
     * The homeserver is not a guess. Under the model this replaces, every stored token was minted
     * against `MATRIX_HOMESERVER` — there was nowhere else it could have come from — so that is the
     * only server the value could have belonged to.
     */
    writeState({ alpha: 'syt_alpha' });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest().alpha).toEqual({
      homeserver: 'https://hs.test',
      serverName: 'hs.test',
      mxid: null,
      accessToken: 'syt_alpha',
      credentialGeneration: null,
    });
  });

  test('EVERY entry survives — a migration that dropped one would look like a revoked token', async () => {
    writeState({ alpha: 'syt_a', beta: 'syt_b', gamma: 'syt_c' });
    const mod = await loadBridge();
    const tokens = mod.agentTokenStateForTest();
    expect(Object.keys(tokens).sort()).toEqual(['alpha', 'beta', 'gamma']);
    expect(Object.values(tokens).map((c) => c.accessToken).sort()).toEqual(['syt_a', 'syt_b', 'syt_c']);
  });

  test('`mxid` starts null rather than composed', async () => {
    /*
     * ADR-014 decision 5 is still open: an MXID must be DISCOVERED, and nothing in a bare string says
     * what it was. Composing `@ac_alpha:hs.test` here would look helpful and would be a guess written
     * into a credential store — the precise habit decision 5 exists to end.
     */
    writeState({ alpha: 'syt_alpha' });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest().alpha.mxid).toBeNull();
  });
});

describe('an entry that is already a record', () => {
  test('a FOREIGN homeserver is preserved, not rewritten to ours', async () => {
    /*
     * The whole point of the record. If migration normalized every entry to `MATRIX_HOMESERVER`, a
     * credential for another project side would be silently re-pointed at this deployment — which is
     * both wrong and a way to send one side's token to another's server.
     */
    writeState({
      alpha: {
        homeserver: 'https://other.example', serverName: 'other.example',
        mxid: '@ac_alpha:other.example', accessToken: 'syt_other',
      },
    });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest().alpha).toEqual({
      homeserver: 'https://other.example',
      serverName: 'other.example',
      mxid: '@ac_alpha:other.example',
      accessToken: 'syt_other',
      credentialGeneration: null,
    });
  });

  test('a partial record is completed from this deployment rather than rejected', async () => {
    // A record with a token but no homeserver is still a credential; refusing it would lose one.
    writeState({ alpha: { accessToken: 'syt_alpha' } });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest().alpha).toEqual({
      homeserver: 'https://hs.test', serverName: 'hs.test', mxid: null, accessToken: 'syt_alpha',
      credentialGeneration: null,
    });
  });

  test('`.token` is accepted as an alias for `.accessToken`', async () => {
    /*
     * Because `endAgentWorkForToken` already contained
     * `typeof stored === 'string' ? stored : stored?.token` — somebody anticipated a record and
     * guessed that field name. Reading both costs one line and means a state file written by any such
     * intermediate is not read as credential-less, which would take the agent silent.
     */
    writeState({ alpha: { token: 'syt_aliased' } });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest().alpha.accessToken).toBe('syt_aliased');
  });

  test('a trailing slash on the homeserver is trimmed', async () => {
    // Callers concatenate `/_matrix/...`, and some homeservers 404 a doubled slash rather than
    // normalizing it.
    writeState({ alpha: { homeserver: 'https://other.example/', accessToken: 'syt_x' } });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest().alpha.homeserver).toBe('https://other.example');
  });

  test('serverName is lowercased, because it is compared against a room id', async () => {
    // `setRoomAvatar`'s ladder compares this to the origin server in `!opaque:origin-server`. A
    // case mismatch there would silently skip a credential that should have been tried.
    writeState({ alpha: { serverName: 'Other.Example', accessToken: 'syt_x' } });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest().alpha.serverName).toBe('other.example');
  });
});

describe('entries that carry no credential', () => {
  test('they are dropped AND named, rather than dropped quietly', async () => {
    /*
     * An entry with no token is not a credential, so dropping it loses nothing. It is still named,
     * because this file is the one place where a wrong assumption about what is droppable costs
     * something nothing can re-mint — and an unexpected name in that list is the only signal that the
     * assumption was wrong.
     */
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    writeState({ alpha: 'syt_alpha', empty: '', blank: '   ', nulled: null, obj: {}, numeric: 42 });
    const mod = await loadBridge();

    expect(Object.keys(mod.agentTokenStateForTest())).toEqual(['alpha']);
    const message = warn.mock.calls.map((c) => c.join(' ')).join('\n');
    for (const name of ['empty', 'blank', 'nulled', 'obj', 'numeric']) {
      expect(message, name).toContain(name);
    }
  });

  test('a missing agentTokens key yields an empty map, not a failed load', async () => {
    // The unreadable-file path is reserved for bytes that could not be parsed at all; a parsed file
    // with no credentials is simply a bridge with no credentials.
    writeFileSync(statePath, JSON.stringify({ botToken: 'syt_bot' }), { mode: 0o600 });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest()).toEqual({});
    expect(mod.getPendingInvite).toBeTypeOf('function'); // the module loaded, rather than throwing
  });

  test('other state keys survive the migration untouched', async () => {
    // `loadState` now rebuilds the object to replace `agentTokens`. A spread that dropped siblings
    // would lose the room/group maps, which is how a bridge forgets which project a room belongs to.
    writeState({ alpha: 'syt_alpha' }, { roomGroupMap: { '!r:hs.test': 'proj' }, groupRoomMap: { proj: '!r:hs.test' } });
    const mod = await loadBridge();
    expect(mod.agentTokenStateForTest().alpha.accessToken).toBe('syt_alpha');
    // Read back through the file, since the maps have no test accessor.
    mod.saveStateForTest();
    const onDisk = JSON.parse(readFileSync(statePath, 'utf-8'));
    expect(onDisk.roomGroupMap).toEqual({ '!r:hs.test': 'proj' });
    expect(onDisk.botToken).toBe('syt_bot');
  });
});

describe('persistence round-trips the record', () => {
  test('a migrated bare string is written back as a record', async () => {
    /*
     * So the migration happens once rather than on every boot, and so the next reader sees the shape
     * without having to tolerate both.
     */
    writeState({ alpha: 'syt_alpha' });
    const mod = await loadBridge();
    mod.saveStateForTest();
    const onDisk = JSON.parse(readFileSync(statePath, 'utf-8'));
    expect(onDisk.agentTokens.alpha).toEqual({
      homeserver: 'https://hs.test', serverName: 'hs.test', mxid: null, accessToken: 'syt_alpha',
      credentialGeneration: null,
    });
  });

  test('a reload of the written file is a no-op', async () => {
    writeState({ alpha: 'syt_alpha' });
    const first = await loadBridge();
    first.saveStateForTest();
    const second = await loadBridge();
    expect(second.agentTokenStateForTest()).toEqual(first.agentTokenStateForTest());
  });
});

describe('an operating agent uses the MXID that was DISCOVERED', () => {
  /*
   * ADR-014 decision 5. `agentUserId()` composes `@ac_<name>:<our server>`, which is a SPECIFICATION —
   * correct for telling an operator what account to create, wrong for asserting the identity of an
   * agent that may live on a project side's homeserver. `agentMxid()` is the assertion form.
   *
   * The cost of getting it wrong is already recorded in bridge-matrix.js: the invite poll once composed
   * a state_key inline and missed the lowercasing the homeserver applies, so it looked for
   * `@ac_BigLittle:…` while the event carried `@ac_biglittle:…`, found nothing, and reported a null
   * inviter. Owner IS the inviter (ADR-002), so that meant an untrusted room, no ownership record, and
   * every later approval failing `owner_binding_missing`. A wrong SERVER is the same mechanism with a
   * larger error.
   */
  test('THE POINT: a discovered MXID on another server WINS over composition', async () => {
    writeState({
      alpha: {
        homeserver: 'https://other.example', serverName: 'other.example',
        mxid: '@ac_alpha:other.example', accessToken: 'syt_x',
      },
    });
    const mod = await loadBridge();
    // Composition would say `@ac_alpha:hs.test` — this deployment's own server.
    expect(mod.agentMxidForTest('alpha')).toBe('@ac_alpha:other.example');
  });

  test('a discovered MXID whose LOCALPART differs is still believed', async () => {
    // The homeserver decides the localpart. A project side may have registered the account under a
    // name that does not match our prefix convention, and it is still the account we hold a token for.
    writeState({ alpha: { mxid: '@intake-7:hs.test', accessToken: 'syt_x' } });
    const mod = await loadBridge();
    expect(mod.agentMxidForTest('alpha')).toBe('@intake-7:hs.test');
  });

  test('with no credential it FALLS BACK to composition, which is this deployment', async () => {
    /*
     * Safe because every caller concerns an agent that is OPERATING — deriving an inviter, matching an
     * invite state_key, checking approval-room membership, addressing a DM, sending a typing
     * notification. An agent with no credential record has no token, so it cannot have acted and
     * cannot have been invited as anything worth comparing against.
     */
    writeState({});
    const mod = await loadBridge();
    expect(mod.agentMxidForTest('alpha')).toBe('@ac_alpha:hs.test');
  });

  test('a credential with no mxid recorded also falls back', async () => {
    // A migrated bare string has `mxid: null` until the next adoption runs a whoami.
    writeState({ alpha: 'syt_bare_string' });
    const mod = await loadBridge();
    expect(mod.agentMxidForTest('alpha').startsWith('@ac_alpha:')).toBe(true);
  });

  test('the name is matched case-insensitively, as the token lookup is', async () => {
    // `BigLittle` and `biglittle` are the same agent; the homeserver lowercases localparts. This is
    // the difference that produced the recorded failure above.
    writeState({ BigLittle: { mxid: '@ac_biglittle:other.example', accessToken: 'syt_x' } });
    const mod = await loadBridge();
    expect(mod.agentMxidForTest('biglittle')).toBe('@ac_biglittle:other.example');
  });

  test('GUARD: `agentMxid` is never used as a bare value', async () => {
    /*
     * A source assertion for a trap I fell into while writing this change, and it is here because a
     * behavioural test could not have caught it cheaply.
     *
     * One call site held its result in a local named `agentMxid`. Renaming the local to avoid shadowing
     * the new function left two later references reading `agentMxid` — which then resolved to the
     * FUNCTION. `node --check` passes, no linter complains (both names exist), and
     * `members.includes(agentMxid)` compares a member list against a function object: always false, so
     * the agent would be re-invited to its approval room on every single call.
     *
     * Found by reading the diff, not by a test. The guard is cheap and the failure was silent.
     */
    const { readFileSync } = await import('fs');
    const source = readFileSync(new URL('../bridge-matrix.js', import.meta.url), 'utf8');
    const bare = source
      .split('\n')
      .map((line, i) => [i + 1, line])
      // Skip the declaration, the export alias, and prose.
      .filter(([, line]) => /\bagentMxid\b/.test(line))
      .filter(([, line]) => !/function agentMxid|agentMxid as agentMxidForTest|^\s*\*|^\s*\/\//.test(line))
      // Every remaining use must be a CALL.
      .filter(([, line]) => !/\bagentMxid\(/.test(line));
    expect(bare, `agentMxid used as a value on line(s) ${bare.map(([n]) => n).join(', ')}`).toEqual([]);
  });
});
