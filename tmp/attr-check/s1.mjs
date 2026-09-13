import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import {
  claudeProjectDir,
  transcriptSearch,
  agentTranscriptWorkspace,
  meterAgent,
  summarizeFleet,
} from '../../lib/metering/attribute.js';

const source = readFileSync(new URL('../../lib/metering/attribute.js', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
// The nominal process cwd recorded for `path.resolve` of relative inputs. Every
// workspace in these vectors is absolute, so replay is cwd-independent anyway.
const processCwd = '/';
let seed = 0x4A721B5;
const next = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % 10000; };

const projectDir = [];
const search = [];
const workspace = [];
const meterAgentVectors = [];
const fleet = [];

/* ---------------------------------------------------------------- paths */

const addProjectDir = (name, workspacePath) =>
  projectDir.push({ name, workspacePath, expected: claudeProjectDir(workspacePath) });
addProjectDir('plain', '/Users/fixture/home/hagency');
addProjectDir('unicode', '/fixture/工程/项目');
addProjectDir('dashes', '/Users/fixture/my-project');
addProjectDir('root', '/');
addProjectDir('double-slash', '/a//b');
addProjectDir('trailing-slash', '/a/b/');
addProjectDir('relative', 'work/thing');
addProjectDir('empty', '');
addProjectDir('null', null);
addProjectDir('dot-segments', '/a/./b/../c');
for (let i = 0; i < 8; i += 1) {
  addProjectDir(`seeded-${i}`, `/Users/fixture/工程-${i}/ws-${next()}`);
}

/* -------------------------------------------------------------- search */

const addSearch = (name, framework, workspacePath, homeDir) =>
  search.push({ name, framework, workspacePath, homeDir, expected: transcriptSearch(framework, workspacePath, homeDir) });
addSearch('claude', 'claude', '/Users/fixture/work', '/Users/fixture/home');
addSearch('claude-uppercase', 'CLAUDE', '/w', '/h');
addSearch('claude-unicode-home', 'claude', '/Users/ユーザー/проект', '/Users/fixture/家');
addSearch('claude-empty-home', 'claude', '/w', '');
addSearch('claude-null-home', 'claude', '/w', null);
addSearch('claude-relative-workspace', 'claude', 'w', '/h');
addSearch('codex', 'codex', '/Users/fixture/work', '/Users/fixture/home');
addSearch('codex-mixed-case', 'Codex', '/w', '/h');
addSearch('codex-ignores-workspace-shape', 'codex', 'relative', '/h');
addSearch('codex-null-home', 'codex', '/w', null);
addSearch('unknown', 'something-new', '/w', '/h');
addSearch('octos', 'octos', '/w', '/h');
addSearch('empty-framework', '', '/w', '/h');
addSearch('null-framework', null, '/w', '/h');
addSearch('padded-framework', 'claude ', '/w', '/h');

/* --------------------------------------------------- workspace precedence */

const addWorkspace = (name, agent) =>
  workspace.push({ name, agent, expected: agentTranscriptWorkspace(agent) });
addWorkspace('last-first', { lastWorkspacePath: '/a', workspacePath: '/b', workdir: '/c' });
addWorkspace('workspace-fallback', { workspacePath: '/b', workdir: '/c' });
addWorkspace('stopped-agent', { workspacePath: null, lastWorkspacePath: '/kept' });
addWorkspace('workdir-ignored-off-demand', { workdir: '/c', runner: { mode: 'persistent' } });
addWorkspace('workdir-on-demand', { workdir: '/c', runner: { mode: 'on-demand' } });
addWorkspace('homedir-on-demand', { homeDir: '/hd', runner: { mode: 'on-demand' } });
addWorkspace('on-demand-still-prefers-last', {
  lastWorkspacePath: '/a', workdir: '/c', homeDir: '/hd', runner: { mode: 'on-demand' },
});
addWorkspace('on-demand-case-sensitive', { workdir: '/c', runner: { mode: 'ON-DEMAND' } });
addWorkspace('trimmed', { lastWorkspacePath: '  /spaced  ' });
addWorkspace('trim-falls-through', { lastWorkspacePath: '   ', workspacePath: '/kept' });
addWorkspace('null-fields', { lastWorkspacePath: null, workspacePath: null });
addWorkspace('empty-agent', {});
addWorkspace('null-agent', null);
addWorkspace('non-string-fields', { lastWorkspacePath: 42, workspacePath: true, workdir: { p: 1 } });
addWorkspace('bom-trimmed', { lastWorkspacePath: '﻿/bom' });
addWorkspace('runner-null', { workdir: '/c', runner: null });

/* ---------------------------------------------------------- transcripts */

const WS = '/Users/fixture/work/payments-api';
const OTHER = '/Users/fixture/work/other-api';

/** Complete Claude usage records only: shared vectors never omit a field. */
const claudeUsage = (input, output, cacheWrite, cacheRead, model = 'model-fixture') => ({
  cwd: WS, uuid: `id-${input}-${output}-${cacheWrite}-${cacheRead}-${next()}`,
  message: { model, usage: {
    input_tokens: input, output_tokens: output,
    cache_creation_input_tokens: cacheWrite, cache_read_input_tokens: cacheRead,
  } },
});
const codexUsage = (input, cached, output) => ({
  payload: { cwd: WS, type: 'token_count', info: { total_token_usage: {
    input_tokens: input, cached_input_tokens: cached, output_tokens: output,
    reasoning_output_tokens: Math.floor(output / 2), total_tokens: input + output,
  } } },
});
const claudeText = (records) => records.map((r) => (typeof r === 'string' ? r : JSON.stringify(r))).join('\n');
const claudeOther = (text) => text.replaceAll(WS, OTHER);
const codexText = (records) => records.map((r) => (typeof r === 'string' ? r : JSON.stringify(r))).join('\n');

const addMeter = async (name, { agent, homeDir = '/Users/fixture/home', sessions = [], bounds = null }) => {
  const readSessions = async function* gen() { for (const s of sessions) yield s; };
  readSessions.bounds = bounds;
  const expected = await meterAgent({ agent, homeDir, readSessions });
  meterAgentVectors.push({
    name, agent, homeDir, sessions, bounds,
    expected: JSON.parse(JSON.stringify(expected)),
  });
};

await addMeter('unsupported-octos', { agent: { name: 'o1', type: 'octos', workspacePath: WS }, sessions: [{ file: 'never-read', text: claudeText([claudeUsage(1, 2, 3, 4)]) }] });
await addMeter('unsupported-codex-acp', { agent: { name: 'o2', type: 'codex-acp', workspacePath: WS } });
await addMeter('unsupported-hermes', { agent: { name: 'o3', type: 'hermes', workspacePath: WS } });
await addMeter('unsupported-unknown', { agent: { name: 'o4', type: 'something-new', workspacePath: WS } });
await addMeter('unsupported-framework-case', { agent: { name: 'o5', type: 'OCTOS', workspacePath: WS } });
await addMeter('null-type', { agent: { name: 'o6', type: null, workspacePath: WS } });
await addMeter('no-workspace', { agent: { name: 'a1', type: 'claude', workspacePath: null } });
await addMeter('no-workspace-workdir-ignored', { agent: { name: 'a2', type: 'claude', workdir: WS } });
await addMeter('whitespace-workspace', { agent: { name: 'a3', type: 'claude', workspacePath: '   ' } });
await addMeter('empty-everything', { agent: { name: 'a4', type: 'codex' } });
await addMeter('no-sessions-at-all', { agent: { name: 'fresh', type: 'codex', lastWorkspacePath: '/Users/fixture/new' } });
await addMeter('claude-matched', {
  agent: { name: 'a1', type: 'claude', workspacePath: WS },
  sessions: [{ file: '😀-session.jsonl', text: claudeText([
    { type: 'user', cwd: WS }, claudeUsage(2, 1810, 1565, 339166),
    claudeUsage(10, 90, 0, 1000),
  ]) }],
});
await addMeter('claude-resumed-replay-counted-once', {
  agent: { name: 'a2', type: 'claude', workspacePath: WS },
  sessions: [{ file: 'resumed.jsonl', text: claudeText([claudeUsage(5, 5, 5, 5), claudeUsage(5, 5, 5, 5)]) }],
});
await addMeter('claude-malformed-lines-skipped', {
  agent: { name: 'a3', type: 'claude', workspacePath: WS },
  sessions: [{ file: 'damaged.jsonl', text: claudeText(['bad JSON', claudeUsage(7, 8, 9, 10), '{"broken":']) }],
});
await addMeter('claude-other-workspace-skipped', {
  agent: { name: 'a4', type: 'claude', workspacePath: WS },
  sessions: [{ file: 'elsewhere.jsonl', text: claudeOther(claudeText([claudeUsage(1, 2, 3, 4)])) }],
});
await addMeter('claude-no-cwd-skipped', {
  agent: { name: 'a5', type: 'claude', workspacePath: WS },
  sessions: [{ file: 'no-cwd.jsonl', text: claudeText([{ uuid: 'x1', message: { usage: { input_tokens: 1, output_tokens: 1, cache_creation_input_tokens: 1, cache_read_input_tokens: 1 } } }]) }],
});
await addMeter('claude-mixed-match-and-skip', {
  agent: { name: 'a6', type: 'claude', workspacePath: WS },
  sessions: [
    { file: 'mine.jsonl', text: claudeText([claudeUsage(100, 100, 100, 100)]) },
    { file: 'theirs.jsonl', text: claudeOther(claudeText([claudeUsage(9, 9, 9, 9)])) },
  ],
});
await addMeter('claude-zero-usage-counted', {
  agent: { name: 'a7', type: 'claude', workspacePath: WS },
  sessions: [{ file: 'zero.jsonl', text: claudeText([claudeUsage(0, 0, 0, 0)]) }],
});
await addMeter('codex-matched', {
  agent: { name: 'c1', type: 'codex', lastWorkspacePath: WS },
  sessions: [{ file: 'rollout-1.jsonl', text: codexText([
    { type: 'session_meta', payload: { cwd: WS } }, codexUsage(4000, 900, 500), codexUsage(4000, 900, 500),
  ]) }],
});
await addMeter('codex-two-files-summed', {
  agent: { name: 'c2', type: 'codex', lastWorkspacePath: WS },
  sessions: [
    { file: 'rollout-a.jsonl', text: codexText([codexUsage(1000, 100, 400)]) },
    { file: 'rollout-b.jsonl', text: codexText([codexUsage(2000, 200, 600)]) },
  ],
});
await addMeter('codex-other-workspace-skipped', {
  agent: { name: 'c3', type: 'codex', lastWorkspacePath: WS },
  sessions: [{ file: 'rollout-x.jsonl', text: codexText([{ type: 'session_meta', payload: { cwd: OTHER } }, codexUsage(50, 0, 50)]) }],
});
await addMeter('codex-malformed-lines', {
  agent: { name: 'c4', type: 'codex', lastWorkspacePath: WS },
  sessions: [{ file: 'rollout-bad.jsonl', text: codexText(['not json', codexUsage(30, 3, 20), '{"oops":']) }],
});
await addMeter('bounds-skipped-only', {
  agent: { name: 'b1', type: 'claude', workspacePath: WS },
  sessions: [{ file: 'theirs.jsonl', text: claudeOther(claudeText([claudeUsage(1, 1, 1, 1)])) }],
  bounds: null,
});
await addMeter('bounds-both-facts', {
  agent: { name: 'b2', type: 'codex', lastWorkspacePath: WS },
  sessions: [
    { file: 'busy-a.jsonl', text: codexText([{ type: 'session_meta', payload: { cwd: OTHER } }, codexUsage(10, 0, 5)]) },
    { file: 'busy-b.jsonl', text: codexText([{ type: 'session_meta', payload: { cwd: OTHER } }, codexUsage(10, 0, 5)]) },
  ],
  bounds: { droppedByCount: 380, entriesUnwalked: 7 },
});
await addMeter('bounds-unreached-only', {
  agent: { name: 'b3', type: 'codex', lastWorkspacePath: WS },
  sessions: [],
  bounds: { droppedByCount: 0, entriesUnwalked: 12 },
});
await addMeter('bounds-ignored-when-matched', {
  agent: { name: 'b4', type: 'codex', lastWorkspacePath: WS },
  sessions: [{ file: 'mine.jsonl', text: codexText([codexUsage(70, 7, 30)]) }],
  bounds: { droppedByCount: 5, entriesUnwalked: 5 },
});
for (let i = 0; i < 12; i += 1) {
  const input = next() + 100, cached = Math.floor(input / 2), output = next() + 50;
  await addMeter(`seeded-claude-${i}`, {
    agent: { name: `s${i}`, type: i % 2 ? 'Claude' : 'claude', workspacePath: WS },
    sessions: i % 3
      ? [{ file: `seed-${i}.jsonl`, text: claudeText([claudeUsage(input, output, cached, next()), claudeUsage(next(), next(), next(), next())]) }]
      : [{ file: `seed-${i}.jsonl`, text: claudeText([claudeUsage(input, output, cached, next())]) },
         { file: `seed-${i}-other.jsonl`, text: claudeOther(claudeText([claudeUsage(1, 1, 1, 1)])) }],
  });
  await addMeter(`seeded-codex-${i}`, {
    agent: { name: `t${i}`, type: i % 2 ? 'CODEX' : 'codex', lastWorkspacePath: WS, workspacePath: null },
    sessions: [{ file: `rollout-${i}.jsonl`, text: codexText([
      { type: 'session_meta', payload: { cwd: WS } },
      codexUsage(input, cached, output), codexUsage(input, cached, output), codexUsage(input + 120, cached + 10, output + 20),
    ]) }],
  });
}

/* --------------------------------------------------------------- fleet */

const ok = (agent, ws, input, output, cacheWrite, cacheRead) => ({
  agent, available: true, framework: 'claude', workspace: ws,
  totals: { input, output, cacheWrite, cacheRead }, total: input + output + cacheWrite + cacheRead,
});
const addFleet = (name, rows) => fleet.push({
  name, rows,
  expected: JSON.parse(JSON.stringify(summarizeFleet(JSON.parse(JSON.stringify(rows))))),
});
addFleet('shared-workspace-ambiguous', [ok('a1', WS, 1, 2, 3, 4), ok('a2', WS, 10, 20, 30, 40)]);
addFleet('distinct-workspaces-total', [ok('a1', WS, 1, 2, 3, 4), ok('a2', OTHER, 10, 20, 30, 40)]);
addFleet('same-workspace-different-spelling', [ok('a1', WS, 1, 1, 1, 1), ok('a2', `${WS}/./`, 2, 2, 2, 2)]);
addFleet('partial-total-reports-gap', [
  ok('a1', WS, 5, 5, 5, 5),
  { agent: 'a2', available: false, framework: 'octos', reason: 'no adapter' },
]);
addFleet('nothing-attributable', [{ agent: 'a1', available: false, framework: 'claude', reason: 'no workspace' }]);
addFleet('empty-fleet', []);
addFleet('unavailable-row-keeps-reason', [
  { agent: 'a1', available: false, framework: 'claude', workspace: WS, reason: 'no transcripts found for this workspace yet' },
  ok('a2', OTHER, 1, 1, 1, 1),
]);
addFleet('row-without-workspace-never-groups', [
  ok('a1', WS, 1, 1, 1, 1),
  { agent: 'a2', available: true, framework: 'claude', totals: { input: 9, output: 9, cacheWrite: 9, cacheRead: 9 }, total: 36 },
]);

const output = JSON.stringify({
  source: 'lib/metering/attribute.js',
  sourceSha256: createHash('sha256').update(source).digest('hex'),
  processCwd,
  vectors: { projectDir, search, workspace, meterAgent: meterAgentVectors, fleet },
}, null, 2) + '\n';
const path = new URL('../fixtures/attribution.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(path, 'utf8').replaceAll('\r\n', '\n') !== output) {
    throw new Error('Attribution vectors differ from retained JavaScript');
  }
} else writeFileSync(path, output);
console.log(JSON.stringify({
  vectors: projectDir.length + search.length + workspace.length + meterAgentVectors.length + fleet.length,
}));
