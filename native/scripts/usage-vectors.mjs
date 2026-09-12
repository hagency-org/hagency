import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createUsageLedger } from '../../lib/metering/ledger.js';

const source = readFileSync(new URL('../../lib/metering/ledger.js', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const counts = (input, output, cacheWrite, cacheRead) => ({ input, output, cacheWrite, cacheRead });
const vectors = [];
let seed = 0x631709;
const next = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % 200; };
for (let index = 0; index < 16; index++) {
  let now = Date.parse(index % 2 ? '2024-02-29T23:59:59.999Z' : '2026-08-31T23:59:59.999Z');
  const ledger = createUsageLedger({ now: () => now });
  const initial = counts(next()+1, next()+1, next()+1, next()+1);
  const growth = Object.fromEntries(Object.entries(initial).map(([key,value]) => [key,value+next()+1]));
  const mixed = { ...growth, input: Math.floor(initial.input / 2), output: growth.output + 7 };
  const observations = [];
  for (const [offset, totals] of [[0,initial],[0,initial],[1,growth],[2,mixed],[3,mixed]]) {
    const at = now + offset;
    now = at;
    ledger.record([{agent:'bound-agent', framework:'claude', sessions:[{key:'host-bound-source',totals}]}]);
    observations.push({ at, totals, expected: {
      highWater: ledger.totalsFor('bound-agent').totals,
      total: ledger.totalsFor('bound-agent').total,
      fresh: ledger.totalsFor('bound-agent').drawn,
      regressions: ledger.totalsFor('bound-agent').regressions,
      daily: ledger.currentPeriod('bound-agent','daily',at),
      monthly: ledger.currentPeriod('bound-agent','monthly',at),
    }});
  }
  vectors.push({ name:`observed-utc-growth-${index}`, observations });
}
const output = JSON.stringify({ source:'lib/metering/ledger.js', sourceSha256:createHash('sha256').update(source).digest('hex'), semantics:'retained known positive counts; observed high-water and UTC growth only', vectors },null,2)+'\n';
const path = new URL('../hagency-store/tests/fixtures/usage-vectors.json', import.meta.url);
if(process.argv.includes('--check')) {
  if(readFileSync(path,'utf8').replaceAll('\r\n','\n')!==output) throw new Error('Usage vectors differ from retained JavaScript');
} else writeFileSync(path,output);
console.log(JSON.stringify({vectors:vectors.length}));
