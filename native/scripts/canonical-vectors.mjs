// Build-only fixture generator. No Node process is part of native Hagency.
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import vm from 'node:vm';
// Execute the pure encoder from the pinned implementation, rather than a copied
// algorithm that could drift together with the Rust tests. No runtime imports.
const source = readFileSync(new URL('../../lib/agent-ops-client-auth.js', import.meta.url), 'utf8');
const start = source.indexOf('function canonicalize(value) {');
const end = source.indexOf('export function agentOpsBodyDigest(');
if (start < 0 || end <= start) throw new Error('Review the changed JavaScript encoder before refreshing vectors');
const canonicalAgentOpsJson = vm.runInNewContext(source.slice(start, end).replace(/^export /gm, '') + '\ncanonicalAgentOpsJson');
const inputs = [
  null, {}, { absent: null }, { z: 1, a: { y: 2, x: 1 } },
  { '小白': '中文名称', '\uE000': 1, '😀': 2, 'é': 'é' },
  { '10': 10, '2': 2, '01': 1, '4294967295': 5, '4294967294': 4, a: [null, true, false] },
  { max: Number.MAX_SAFE_INTEGER, min: Number.MIN_SAFE_INTEGER, zero: -0 },
  { control: '\b\t\n\f\r\u0000', separator: '\u2028\u2029', slash: '/\\"', timestamp: '2026-09-09T12:00:00.000Z' },
];
const vectors = inputs.map(input => {
  const canonical = canonicalAgentOpsJson(input);
  return { input, canonical, sha256: createHash('sha256').update(canonical).digest('hex') };
});
const path = new URL('../fixtures/canonical.json', import.meta.url);
const output = JSON.stringify(vectors, null, 2) + '\n';
if (process.argv.includes('--check')) {
  if (readFileSync(path, 'utf8') !== output) throw new Error('Canonical vectors differ from JavaScript');
} else writeFileSync(path, output);
