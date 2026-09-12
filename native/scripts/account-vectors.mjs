import { createHmac } from 'node:crypto';
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
const output = `${JSON.stringify({ semantics: 'native physical credential namespace identity, never provider authentication', vectors }, null, 2)}\n`;
const path = new URL('../hagency-store/tests/fixtures/account-identity.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(path,'utf8').replaceAll('\r\n','\n') !== output) throw new Error('Native namespace identity vectors differ');
} else writeFileSync(path, output);
console.log(JSON.stringify({ vectors: vectors.length }));
