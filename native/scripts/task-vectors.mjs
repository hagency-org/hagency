import { readFileSync, writeFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../../router/src/task-repository.ts', import.meta.url), 'utf8');
const match = source.match(/const TRANSITIONS = new Map\(\[[\s\S]*?\n\]\);/);
if (!match) throw new Error('Canonical task transition source not found');
const transitions = runInNewContext(`${match[0]}\nTRANSITIONS`);
const states = ['created', 'accepted', 'in_progress', 'blocked', 'done'];
const cases = states.flatMap(from => states.map(to => ({ from, to, allowed: transitions.get(from)?.has(to) ?? false })));
const output = `${JSON.stringify(cases, null, 2)}\n`;
const path = new URL('../fixtures/task-transitions.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(path, 'utf8') !== output) throw new Error('Native task transitions differ from JavaScript');
} else writeFileSync(path, output);
