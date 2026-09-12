import { mkdirSync, mkdtempSync, rmSync, writeFileSync, utimesSync, chmodSync, readdirSync } from 'node:fs';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import {
  makeSessionReader, boundsReport, meterFleet, resetMeteringCache,
} from '../../lib/metering/reader.js';
import { transcriptSearch } from '../../lib/metering/attribute.js';

const source = readFileSync(new URL('../../lib/metering/reader.js', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
// Fixed injected clock: every mtime is an offset from NOW, so the fixture is
// deterministic across machines and dates.
const NOW = 1_800_000_000_000;
const DAY = 86_400_000;
const HOME_TOKEN = '<HOME>';
const WS_CLAUDE = '/Users/fixture/work';
const WS_CODEX = '/Users/fixture/work';

let seed = 0x5EED1A;
const next = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % 10000; };

/** Claude usage record; complete fields only (shared vectors never coerce). */
const claudeLine = (id, input, output, cacheWrite, cacheRead) => JSON.stringify({
  cwd: WS_CLAUDE, uuid: id, message: { model: 'model-fixture', usage: {
    input_tokens: input, output_tokens: output,
    cache_creation_input_tokens: cacheWrite, cache_read_input_tokens: cacheRead,
  } },
});
const codexLine = (input, cached, output) => JSON.stringify({
  payload: { cwd: WS_CODEX, type: 'token_count', info: { total_token_usage: {
    input_tokens: input, cached_input_tokens: cached, output_tokens: output,
    reasoning_output_tokens: Math.floor(output / 2), total_tokens: input + output,
  } } },
});
const claudeTranscript = (n) => Array.from({ length: n }, (_, i) => claudeLine(`c-${i}`, 10 + i, 20, 3, 40)).join('\n') + '\n';
const codexTranscript = (total) => [
  JSON.stringify({ type: 'session_meta', payload: { cwd: WS_CODEX } }),
  codexLine(total - 100, 10, 100),
].join('\n') + '\n';

/** Build a tree under home: [{ path (relative), mtimeOffsetMs, content }]. */
function buildTree(home, tree) {
  for (const node of tree) {
    const full = path.join(home, node.path);
    mkdirSync(path.dirname(full), { recursive: true });
    writeFileSync(full, node.content);
    const when = new Date(NOW + node.mtimeOffsetMs);
    utimesSync(full, when, when);
  }
}

/** Absolute home paths recorded as `<HOME>/…` so replay works anywhere. */
if (process.platform === 'win32') {
  // The retained JavaScript joins and relativizes with the host path module;
  // on Windows that yields drive-letter backslash paths, so the recorded POSIX
  // fixture cannot be regenerated or checked here. The Rust tests replay it.
  throw new Error('reader vectors are a POSIX oracle; run this script on a POSIX host');
}
const relativize = (text, home) => (typeof text === 'string' ? text.split(home + path.sep).join(HOME_TOKEN + '/') : text);
const relativizeRow = (row, home) => {
  const copy = JSON.parse(JSON.stringify(row));
  for (const agent of copy.agents ?? []) {
    for (const file of agent.files ?? []) file.file = relativize(file.file, home);
  }
  return copy;
};

const reads = [];
const reports = [];
const fleets = [];

/** A fresh throwaway home under the workspace tmp dir (never ~/.claude/~/.codex). */
const CASE_HOME = () => {
  const root = process.env.TMPDIR && process.env.TMPDIR.startsWith('/')
    ? process.env.TMPDIR
    : 'tmp/oracle';
  mkdirSync(root, { recursive: true });
  const home = mkdtempSync(path.join(root, 'reader-vector-'));
  homes.push(home);
  return home;
};
const homes = [];
process.on('exit', () => { for (const home of homes) { try { rmSync(home, { recursive: true, force: true }); } catch { /* best effort */ } } });

const addRead = async (name, { framework, workspacePath, limits, tree }) => {
  const home = CASE_HOME();
  buildTree(home, tree);
  const search = transcriptSearch(framework, workspacePath, home);
  const reader = makeSessionReader({ now: () => NOW, ...limits });
  const sessions = [];
  for await (const session of reader(search)) sessions.push(session);
  reads.push({
    name, framework, workspacePath, limits: limits ?? {},
    tree, searchNarrowed: search.narrowed, searchRecursive: search.recursive,
    expected: {
      files: sessions.map((s) => ({ file: relativize(s.file, home), text: s.text })),
      bounds: reader.bounds,
    },
  });
};

/* ------------------------------------------------------ discovery + bounds */

await addRead('codex-nested-date-tree-discovered', {
  framework: 'codex', workspacePath: WS_CODEX,
  tree: [
    { path: '.codex/sessions/2026/09/01/rollout-a.jsonl', mtimeOffsetMs: -3 * 3600_000, content: codexTranscript(1200) },
    { path: '.codex/sessions/2026/09/02/rollout-b.jsonl', mtimeOffsetMs: -2 * 3600_000, content: codexTranscript(2400) },
    { path: '.codex/sessions/2026/08/31/rollout-c.jsonl', mtimeOffsetMs: -1 * 3600_000, content: codexTranscript(3600) },
  ],
  limits: {},
});
await addRead('codex-newest-first', {
  framework: 'codex', workspacePath: WS_CODEX,
  tree: [
    { path: '.codex/sessions/2026/09/01/rollout-old.jsonl', mtimeOffsetMs: -9 * 3600_000, content: codexTranscript(100) },
    { path: '.codex/sessions/2026/09/01/rollout-new.jsonl', mtimeOffsetMs: -1 * 3600_000, content: codexTranscript(200) },
    { path: '.codex/sessions/2026/09/01/rollout-mid.jsonl', mtimeOffsetMs: -5 * 3600_000, content: codexTranscript(300) },
  ],
  limits: {},
});
await addRead('claude-flat-and-non-recursive', {
  framework: 'claude', workspacePath: WS_CLAUDE,
  tree: [
    { path: '.claude/projects/-Users-fixture-work/session.jsonl', mtimeOffsetMs: -2 * 3600_000, content: claudeTranscript(3) },
    { path: '.claude/projects/-Users-fixture-work/nested/deeper.jsonl', mtimeOffsetMs: -1 * 3600_000, content: claudeTranscript(3) },
    { path: '.claude/projects/-Users-fixture-work/notes.txt', mtimeOffsetMs: -1 * 3600_000, content: 'ignored\n' },
  ],
  limits: {},
});
await addRead('claude-narrowed-age-drop-understates', {
  framework: 'claude', workspacePath: WS_CLAUDE,
  tree: [
    { path: '.claude/projects/-Users-fixture-work/current.jsonl', mtimeOffsetMs: -3600_000, content: claudeTranscript(2) },
    { path: '.claude/projects/-Users-fixture-work/old.jsonl', mtimeOffsetMs: -40 * DAY, content: claudeTranscript(2) },
  ],
  limits: {},
});
await addRead('codex-outside-window-never-read', {
  framework: 'codex', workspacePath: WS_CODEX,
  tree: [
    { path: '.codex/sessions/2026/01/02/rollout-old.jsonl', mtimeOffsetMs: -40 * DAY, content: codexTranscript(500) },
  ],
  limits: {},
});
await addRead('file-ceiling-drops-oldest', {
  framework: 'codex', workspacePath: WS_CODEX,
  tree: [0, 1, 2].map((i) => ({
    path: `.codex/sessions/2026/09/0${i + 1}/rollout-${i}.jsonl`,
    mtimeOffsetMs: -(i + 1) * 3600_000,
    content: codexTranscript(100 * (i + 1)),
  })),
  limits: { maxFiles: 2 },
});
await addRead('byte-ceiling-truncates-at-line-boundary', {
  framework: 'claude', workspacePath: WS_CLAUDE,
  tree: [
    { path: '.claude/projects/-Users-fixture-work/fat.jsonl', mtimeOffsetMs: -3600_000, content: claudeTranscript(6) },
  ],
  limits: { maxBytes: 140 },
});
await addRead('traversal-ceiling-reports-unwalked', {
  framework: 'codex', workspacePath: WS_CODEX,
  tree: [0, 1, 2, 3, 4].map((i) => ({
    path: `.codex/sessions/2026/09/1${i}/rollout-${i}.jsonl`,
    mtimeOffsetMs: -(i + 1) * 3600_000,
    content: codexTranscript(100 + i),
  })),
  limits: { maxEntries: 4 },
});
await addRead('missing-root-is-empty-not-an-error', {
  framework: 'codex', workspacePath: WS_CODEX,
  tree: [],
  limits: {},
});
await addRead('unicode-names-survive', {
  framework: 'codex', workspacePath: WS_CODEX,
  tree: [
    { path: '.codex/sessions/2026/09/01/rollout-😀-工程.jsonl', mtimeOffsetMs: -4 * 3600_000, content: codexTranscript(700) },
  ],
  limits: {},
});
await addRead('window-narrowing-by-limits', {
  framework: 'claude', workspacePath: WS_CLAUDE,
  tree: [
    { path: '.claude/projects/-Users-fixture-work/edge-in.jsonl', mtimeOffsetMs: -2 * 3600_000, content: claudeTranscript(2) },
    { path: '.claude/projects/-Users-fixture-work/edge-out.jsonl', mtimeOffsetMs: -3 * 3600_000, content: claudeTranscript(2) },
  ],
  limits: { windowMs: 3600_000 * 2 + 1 },
});
await addRead('mixed-old-fresh-and-oversize', {
  framework: 'codex', workspacePath: WS_CODEX,
  tree: [
    { path: '.codex/sessions/2026/09/01/rollout-fresh.jsonl', mtimeOffsetMs: -2 * 3600_000, content: codexTranscript(900) },
    { path: '.codex/sessions/2026/09/01/rollout-fat.jsonl', mtimeOffsetMs: -1 * 3600_000, content: codexTranscript(10) + codexLine(1, 0, 1).repeat(30) + '\n' },
    { path: '.codex/sessions/2025/01/01/rollout-ancient.jsonl', mtimeOffsetMs: -300 * DAY, content: codexTranscript(999) },
  ],
  limits: { maxBytes: 200 },
});

/* ------------------------------------------------------- unreadable (portable) */

{
  const home = CASE_HOME();
  const locked = path.join(home, '.codex', 'sessions', '2026', '09', '01', '锁');
  mkdirSync(locked, { recursive: true });
  const inside = path.join(locked, 'rollout-hidden.jsonl');
  writeFileSync(inside, codexTranscript(400));
  const when = new Date(NOW - 3600_000);
  utimesSync(inside, when, when);
  let denied = true;
  chmodSync(locked, 0o000);
  try {
    if (readdirSync(locked).length >= 0) denied = false;
  } catch { denied = true; }
  const search = transcriptSearch('codex', WS_CODEX, home);
  const reader = makeSessionReader({ now: () => NOW });
  const sessions = [];
  for await (const session of reader(search)) sessions.push(session);
  reads.push({
    name: 'unreadable-directory-skipped', framework: 'codex', workspacePath: WS_CODEX,
    limits: {}, denied,
    tree: [{ path: '.codex/sessions/2026/09/01/锁/rollout-hidden.jsonl', mtimeOffsetMs: -3600_000, content: codexTranscript(400) }],
    expected: { files: sessions.map((s) => ({ file: relativize(s.file, home), text: s.text })), bounds: reader.bounds },
  });
  chmodSync(locked, 0o755);
}

/* --------------------------------------------------------- boundsReport (pure) */

const addReport = (name, bounds) => reports.push({ name, bounds, expected: boundsReport(bounds) });
addReport('null-when-complete', { filesSeen: 3, filesRead: 3, droppedByCount: 0, droppedByAge: 0, truncated: 0, unreadOutsideWindow: 0, entriesWalked: 9, entriesUnwalked: 0 });
addReport('understates-only', { filesSeen: 2, filesRead: 1, droppedByCount: 1, droppedByAge: 0, truncated: 0, unreadOutsideWindow: 0, entriesWalked: 4, entriesUnwalked: 0 });
addReport('unknown-only', { filesSeen: 1, filesRead: 1, droppedByCount: 0, droppedByAge: 0, truncated: 0, unreadOutsideWindow: 3, entriesWalked: 6, entriesUnwalked: 0 });
addReport('both-claims-composed', { filesSeen: 5, filesRead: 2, droppedByCount: 2, droppedByAge: 1, truncated: 0, unreadOutsideWindow: 4, entriesWalked: 12, entriesUnwalked: 0 });
addReport('truncation-and-traversal-named', { filesSeen: 2, filesRead: 2, droppedByCount: 0, droppedByAge: 0, truncated: 1, unreadOutsideWindow: 0, entriesWalked: 7, entriesUnwalked: 9 });

/* ------------------------------------------------------------------ fleet + cache */

const fleetTree = [
  { path: '.claude/projects/-Users-fixture-work/session-a.jsonl', mtimeOffsetMs: -2 * 3600_000, content: claudeTranscript(4) },
  { path: '.codex/sessions/2026/09/01/rollout-x.jsonl', mtimeOffsetMs: -3 * 3600_000, content: codexTranscript(4313968) },
  { path: '.codex/sessions/2026/09/01/rollout-y.jsonl', mtimeOffsetMs: -4 * 3600_000, content: codexTranscript(5000) },
  { path: '.codex/sessions/2026/09/02/rollout-z.jsonl', mtimeOffsetMs: -5 * 3600_000, content: codexTranscript(6000) },
  { path: '.claude/projects/-Users-fixture-work/old.jsonl', mtimeOffsetMs: -40 * DAY, content: claudeTranscript(2) },
];
const fleetAgents = [
  { name: 'BigLittle', type: 'codex', lastWorkspacePath: WS_CODEX, workspacePath: null },
  { name: 'ClaudeA', type: 'claude', workspacePath: WS_CLAUDE },
];
const otherAgents = [
  { name: 'Newcomer', type: 'claude', workspacePath: '/Users/fixture/fresh' },
];

const addFleet = async (name, { tree, steps }) => {
  const home = CASE_HOME();
  buildTree(home, tree);
  resetMeteringCache();
  const calls = [];
  for (const step of steps) {
    const value = await meterFleet({ agents: step.agents, homeDir: home, now: () => step.now, force: step.force === true });
    calls.push({ agents: step.agents, force: step.force === true, now: step.now, expected: relativizeRow(value, home) });
  }
  resetMeteringCache();
  fleets.push({ name, tree, steps: calls });
};

await addFleet('cache-ttl-force-and-stamp', {
  tree: fleetTree,
  steps: [
    { agents: fleetAgents, now: NOW, force: true },
    { agents: fleetAgents, now: NOW + 30_000 },
    { agents: fleetAgents, now: NOW + 59_999 },
    { agents: fleetAgents, now: NOW + 60_000 },
    { agents: fleetAgents, now: NOW + 61_000, force: true },
    { agents: otherAgents, now: NOW + 62_000 },
    { agents: fleetAgents, now: NOW + 63_000 },
  ],
});
await addFleet('fleet-rows-and-scan-caveats', {
  tree: [
    { path: '.codex/sessions/2026/09/01/rollout-mine.jsonl', mtimeOffsetMs: -3 * 3600_000, content: codexTranscript(777) },
    { path: '.codex/sessions/2026/09/01/rollout-theirs.jsonl', mtimeOffsetMs: -2 * 3600_000, content: codexTranscript(111).replace(WS_CODEX, '/Users/fixture/other') },
    { path: '.codex/sessions/2026/01/01/rollout-ancient.jsonl', mtimeOffsetMs: -100 * DAY, content: codexTranscript(999) },
  ],
  steps: [
    { agents: [
      { name: 'Mine', type: 'codex', lastWorkspacePath: WS_CODEX },
      { name: 'NoWs', type: 'codex' },
      { name: 'Octo', type: 'octos' },
    ], now: NOW + 5_000, force: true },
  ],
});
await addFleet('shared-workspace-ambiguous', {
  tree: [
    { path: '.claude/projects/-Users-fixture-work/shared.jsonl', mtimeOffsetMs: -1 * 3600_000, content: claudeTranscript(2) },
  ],
  steps: [
    { agents: [
      { name: 'a1', type: 'claude', workspacePath: WS_CLAUDE },
      { name: 'a2', type: 'claude', workspacePath: WS_CLAUDE },
    ], now: NOW + 6_000, force: true },
  ],
});

/* ------------------------------------------------------------------- fixture IO */

const output = JSON.stringify({
  source: 'lib/metering/reader.js',
  sourceSha256: createHash('sha256').update(source).digest('hex'),
  now: NOW,
  vectors: { reads, reports, fleets },
}, null, 2) + '\n';
const fixture = new URL('../fixtures/reader.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(fixture, 'utf8').replaceAll('\r\n', '\n') !== output) {
    throw new Error('Reader vectors differ from retained JavaScript');
  }
} else writeFileSync(fixture, output);
console.log(JSON.stringify({
  vectors: reads.length + reports.length + fleets.length,
  reads: reads.length, reports: reports.length, fleets: fleets.length,
}));
