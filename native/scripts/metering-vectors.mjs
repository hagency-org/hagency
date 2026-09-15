import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { parseClaudeSession, parseCodexSession } from '../../lib/metering/parsers.js';

const source = readFileSync(new URL('../../lib/metering/parsers.js', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const workspace = '/fixture/工程/project';
let seed = 0x357195;
const next = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % 10000; };
const vectors = [];
const add = (name, framework, records) => {
  const text = records.map(record => typeof record === 'string' ? record : JSON.stringify(record)).join('\n');
  vectors.push({ name, framework, text, expected: framework === 'claude' ? parseClaudeSession(text) : parseCodexSession(text) });
};
const claude = (id, input, output, cacheWrite, cacheRead, model = 'model-fixture') => ({
  cwd: workspace, uuid: id, message: { model, usage: {
    input_tokens: input, output_tokens: output,
    cache_creation_input_tokens: cacheWrite, cache_read_input_tokens: cacheRead,
  } },
});
const codex = (input, cached, output, reasoning) => ({ payload: { info: {
  total_token_usage: { input_tokens: input, cached_input_tokens: cached,
    output_tokens: output, reasoning_output_tokens: reasoning, total_tokens: input + output },
  // This deliberately differs and repeats: last-token deltas must never be summed.
  last_token_usage: { input_tokens: 999999, output_tokens: 777777 },
} } });
for (let i = 0; i < 64; i += 1) {
  const first = claude(`id-${i}-a`, next(), next(), next(), next(), ['😀', '\uE000', '模型'][i % 3]);
  add(`claude-${i}`, 'claude', [
    { type: 'user', cwd: workspace }, first, first,
    claude(`id-${i}-b`, next(), next(), next(), next(), 'alpha'),
  ]);
  const input = next() + 1000, output = next() + 100;
  const firstTotal = codex(input, input - 900, output, output - 100);
  add(`codex-${i}`, 'codex', [
    { payload: { type: 'session_meta', cwd: workspace } }, firstTotal, firstTotal,
    codex(input + 1200, input - 100, output + 200, output - 10),
  ]);
}
add('claude-zero', 'claude', [claude('zero', 0, 0, 0, 0)]);
add('claude-no-id', 'claude', [claude(null, 1, 2, 3, 4), claude(null, 1, 2, 3, 4)]);
add('claude-damaged-lines', 'claude', ['bad JSON', claude('ok', 1, 2, 3, 4), '{"broken":']);
add('claude-utf16-model-order', 'claude', [claude('a', 1, 2, 3, 4, '\uE000'), claude('b', 1, 2, 3, 4, '😀')]);
add('claude-large-cache', 'claude', [claude('cache', 19765, 300, 20, 4800089833)]);
add('codex-zero', 'codex', [codex(0, 0, 0, 0)]);
add('codex-nonmonotonic', 'codex', [codex(5000, 4500, 300, 120), codex(1000, 900, 100, 40)]);
const output = JSON.stringify({
  source: 'lib/metering/parsers.js',
  sourceSha256: createHash('sha256').update(source).digest('hex'),
  vectors,
}, null, 2) + '\n';
const path = new URL('../fixtures/metering.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(path, 'utf8').replaceAll('\r\n', '\n') !== output) throw new Error('Metering vectors differ from retained JavaScript');
} else writeFileSync(path, output);
console.log(JSON.stringify({ vectors: vectors.length }));
