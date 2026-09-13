import { createHmac } from 'node:crypto';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
const canonical = value => value === null || typeof value !== 'object' ? JSON.stringify(value) : Array.isArray(value) ? `[${value.map(canonical).join(',')}]` : `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
const key = Buffer.from(Array.from({ length: 32 }, (_, index) => index));
const vectors = [];
for (const platform of ['unix-v1', 'windows-v1']) for (const high of [0, 255]) {
  const namespace = { platform, volume: 'fffffffffffffffe', object: [high,...Array(15).fill(17)] };
  const tuple = { version: 'native-namespace-v1', deployment: '0123456789abcdef0123456789abcdef', namespace, source: 'codex_default_namespace' };
  const encoded = canonical(tuple);
  vectors.push({ key: [...key], deployment: tuple.deployment, namespace, canonical: encoded, seat: `seat_native_${createHmac('sha256',key).update(encoded).digest('hex')}` });
}

// MA-S1 readiness oracle: the retained probeFramework state machine is
// MIRRORED with its citations (the 6255edc2 convention — the port never
// edits the retained file), so the native readiness vocabulary is pinned
// against retained rather than only against itself.
//
//   backend-v2.js:13498-13501, mirrored:
//     if (!onPath) state = 'absent';
//     else if (probeError) state = 'unusable';
//     else if (credentialPresent === false) state = 'needs_auth';
//     else state = 'ready';
//
//   backend-v2.js:13541, mirrored (the caveat that makes existence an
//   unsafe readiness answer, both directions):
//     "credentialPresent means the credential directory exists, not that a
//      valid session is in it".
//
// The agreement being pinned: retained 'ready' (a usable namespace)
// corresponds to a native OBSERVED fact whose mode discriminates
// subscription from api_key; every other retained state corresponds to
// native 'unknown'. Native is strictly a fact-table: it never infers
// 'ready' from a directory, and it never reports a mode without a
// classified login receipt — the vector set encodes both refusals.
const readiness = [
  { retained: 'absent', native: { mode: 'unknown' } },
  { retained: 'unusable', native: { mode: 'unknown' } },
  { retained: 'needs_auth', native: { mode: 'unknown' } },
  { retained: 'ready', native: { mode: 'subscription' } },
  { retained: 'ready', native: { mode: 'api_key' } },
];

// PROVENANCE PIN: which retained bytes the mirror was derived from (the
// corpus-retention-vectors.mjs convention; not drift enforcement).
const sha = (p) => createHash('sha256').update(readFileSync(new URL(p, import.meta.url), 'utf8').replaceAll('\r\n', '\n')).digest('hex');
const backendSha256 = sha('../../backend-v2.js');

const output = `${JSON.stringify({ semantics: 'native physical credential namespace identity, never provider authentication', vectors, readiness, backendSha256 }, null, 2)}\n`;
const path = new URL('../hagency-store/tests/fixtures/account-identity.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(path,'utf8').replaceAll('\r\n','\n') !== output) throw new Error('Native namespace identity vectors differ');
} else writeFileSync(path, output);
console.log(JSON.stringify({ vectors: vectors.length, readiness: readiness.length }));
