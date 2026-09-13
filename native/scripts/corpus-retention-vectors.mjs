// Corpus retention oracle (ADR-125): the retained JavaScript computes the
// shared subset of the prune plan; the Rust vector fixture records it.
//
// THE ORACLE'S HONEST LIMIT (F5): the retained predicate has ONE keep-set —
// unread agents (`collectUnreadRetainedMessageIds`) plus router-uncopied
// messages. Native splits that into P2 (per-session unprocessed) and P3'
// (claimed-but-unprocessed), and adds P4..P10 (dispatch custody, unknown
// fate, open tasks, attachments, provenance-moves-with-the-message), which
// have NO retained counterpart at all. So this vector pins only the SHARED
// subset — recency vs inbox membership, and archive membership (A2: the
// retained `archivedMessageExists` vs the native archive read agree on
// "already durably recorded"). It can pass while P2..P10 diverge; those are
// pinned by the native store tests (native/hagency-store/tests/retention.rs),
// the same split the ceiling slice used.
//
// `backend-v2.js` needs exactly one external package at module-eval time
// (`express`; every other import is a Node builtin), and it is used only to
// build the route table — no server runs on import (startServer fires only
// when the module is the entry point). This sandbox has no node_modules and
// no network, so the ONE bare specifier is resolved to a stub through
// `node:module`'s `registerHooks` before the dynamic import. The stub only
// answers route registration; nothing it registers ever executes. The prune
// planner, the unread index and the retention hooks are the real module's
// own code — the arithmetic under test is not stubbed.
//
// The seed is written to the module's own persisted store BEFORE import, and
// the corpus size depends on the retention limit, so the limit is PINNED via
// `AGENT_MESSAGE_RETENTION_LIMIT` before import (not merely observed): the
// vector's expected counts are exact, and the observed limit is asserted
// equal to the pinned one so a drifted env cannot pass unnoticed. The
// keep-set hook is read in the same step as `planMessagePrune` with no await
// between (both depend on live cursors/inboxes). The backend source is
// sha-pinned so a drifted oracle fails --check instead of re-blessing
// different arithmetic.
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { registerHooks } from 'node:module';
import os from 'node:os';
import path from 'node:path';

const sha = (p) => createHash('sha256').update(readFileSync(new URL(p, import.meta.url), 'utf-8').replaceAll('\r\n', '\n')).digest('hex');
const backendSha256 = sha('../../backend-v2.js');

// The pinned limit: small enough that the vector stays cheap, above the
// floor the env guard enforces (`Math.max(100, parseInt(env) || 5000)`).
const LIMIT = 120;

// The full bare-specifier stub set. `backend-v2.js` and its relative import
// graph pull nine external packages at module-eval time (express, zod,
// another-json, better-sqlite3, markdown-it, sanitize-html, the Matrix
// crypto SDK and the two MCP SDK entry points); every other import is a Node
// builtin. None of the nine is on the prune path — they are the HTTP layer,
// the MCP server, the fleet sqlite cache, markdown rendering and attachment
// crypto — so stubbing them cannot change `planMessagePrune`, the unread
// index or the cursors. Defaults are callables; `z` returns itself from any
// access or call so schema builders chain.
const callable = () => {
  const fn = () => callable();
  return new Proxy(fn, {
    get: (target, key) => {
      if (key === Symbol.toPrimitive) return () => 'stub';
      return callable();
    },
    apply: () => callable(),
  });
};
const stubSource = (named) => `
  const express = () => {
    const app = {};
    for (const method of ['use','set','get','post','put','delete','patch','all','listen','param','engine']) {
      app[method] = () => app;
    }
    return app;
  };
  express.json = () => (req, res, next) => next();
  express.urlencoded = () => (req, res, next) => next();
  express.static = () => (req, res, next) => next();
  const stubDefault = ${named.has('__defaultExpress') ? 'express' : 'undefined'};
  ${[...named].filter((n) => n !== '__defaultExpress').map((n) => `export const ${n} = callable();`).join('\n')}
  export default stubDefault === undefined ? callable() : stubDefault;
`;
const namedBySpecifier = new Map([
  ['express', new Set(['__defaultExpress'])],
  ['@matrix-org/matrix-sdk-crypto-nodejs', new Set(['Attachment', 'EncryptedAttachment'])],
  ['@modelcontextprotocol/sdk/server/mcp.js', new Set(['McpServer'])],
  ['@modelcontextprotocol/sdk/server/stdio.js', new Set(['StdioServerTransport'])],
  ['zod', new Set(['z'])],
  ['another-json', new Set([])],
  ['better-sqlite3', new Set([])],
  ['markdown-it', new Set([])],
  ['sanitize-html', new Set([])],
]);
const stubUrls = new Map(
  [...namedBySpecifier].map(([specifier, named]) => [
    specifier,
    `data:text/javascript;charset=utf-8,${encodeURIComponent(
      `const callable = ${callable.toString()};${stubSource(named)}`,
    )}`,
  ]),
);
// One-shot process: the hook lives and dies with this script, so there is no
// unregister step (this Node build's registerHooks returns no unregistrar).
registerHooks({
  resolve(specifier, context, nextResolve) {
    const stub = stubUrls.get(specifier);
    if (stub) return { url: stub, shortCircuit: true };
    return nextResolve(specifier, context);
  },
});

// Seed shape: two agent records; the corpus is seeded through the module's
// own persisted store so its `messages` global IS the corpus — the keep-set
// hook reads module globals, not the planner's `rows` argument, so seeding
// disk (not just the argument) is load-bearing. Old rows carry no
// `to`/`group`/`mentions`, so the unread index never sees them and no
// keep-set member holds them (the pruned half); newer rows are routed to a
// live agent (unread while its cursor sits at 0), and one group-mention row
// rides at the tail.
const T0 = 1_750_000_000_000;
const OLD_UNREFERENCED = 40;
const message = (i, extra = {}) => ({
  id: `msg_${i}`,
  ts: T0 + i * 1000,
  from: '@owner:example.test',
  text: `Message ${i}`,
  ...extra,
});
const seed = [];
for (let i = 0; i < OLD_UNREFERENCED; i += 1) seed.push(message(i));
for (let i = 0; i < LIMIT; i += 1) seed.push(message(OLD_UNREFERENCED + i, { to: 'alpha' }));
seed.push(message(OLD_UNREFERENCED + LIMIT, { group: 'group_x', mentions: ['beta'] }));

let backend;
{
  const runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'corpus-retention-oracle-'));
  mkdirSync(path.join(runtimeDir, 'data'), { recursive: true });
  const writeJson = (name, value) =>
    writeFileSync(path.join(runtimeDir, 'data', name), JSON.stringify(value, null, 2));
  writeJson('agents.json', {
    alpha: { name: 'alpha', kind: 'agent', framework: 'codex' },
    beta: { name: 'beta', kind: 'agent', framework: 'codex' },
  });
  writeJson('cursors.json', {});
  writeJson('messages.json', seed);
  writeJson('groups.json', {});
  writeJson('servers.json', {});
  writeJson('agent_runtime.json', {});
  writeJson('alerts.json', []);
  writeJson('framework-presets.json', []);
  writeJson('supervisor_state.json', { agents: {}, selectionCursor: 0 });
  writeJson('local_activity_sweep.json', { selectionCursor: 0 });
  writeFileSync(path.join(runtimeDir, 'data', '.msg_counter'), '0');

  process.env.AGENT_MESSAGE_RETENTION_LIMIT = String(LIMIT);
  process.env.HAGENCY_RUNTIME_DIR = runtimeDir;
  backend = await import(`../../backend-v2.js?oracle=${Date.now()}-${Math.random()}`);
  rmSync(runtimeDir, { recursive: true, force: true });
}

const {
  planMessagePruneForTest: planMessagePrune,
  messageRetentionLimitForTest,
  retentionKeepIdsForTest,
} = backend.__backendV2TestInternals;
if (typeof planMessagePrune !== 'function' || typeof retentionKeepIdsForTest !== 'function') {
  throw new Error('backend-v2 test internals are missing the retention hooks');
}
const limit = messageRetentionLimitForTest;
if (!Number.isInteger(limit) || limit < 100) throw new Error(`observed limit is not floor-bounded: ${limit}`);
if (limit !== LIMIT) throw new Error(`observed limit ${limit} != pinned ${LIMIT}: the seed no longer matches the env guard`);

const plan = planMessagePrune(seed);
const keepIds = retentionKeepIdsForTest();

const vectors = {
  backendSha256,
  observedLimit: limit,
  keep: keepIds,
  total: seed.length,
  prunedCount: plan.pruned.length,
  retainedCount: plan.retained.length,
  firstPrunedId: plan.pruned[0]?.id ?? null,
  lastPrunedId: plan.pruned[plan.pruned.length - 1]?.id ?? null,
  // The membership pair (A2): the retained archive membership read and the
  // native archive read must agree on "already durably recorded". The
  // retained writer contract: every pruned row is durably recorded (the
  // append happens before the in-memory drop) and no retained row is.
  membership: {
    prunedIds: plan.pruned.map((m) => m.id),
    retainedCount: plan.retained.length,
  },
};

const fixture = JSON.stringify({ kind: 'corpus-retention-vectors', vectors }, null, 2) + '\n';
const fixturePath = new URL('../hagency-store/tests/fixtures/corpus-retention-vectors.json', import.meta.url);
if (process.argv.includes('--check')) {
  const current = readFileSync(fixturePath, 'utf-8');
  if (current !== fixture) {
    console.error('corpus-retention-vectors.json drifts from the oracle; regenerate with:');
    console.error('  node native/scripts/corpus-retention-vectors.mjs');
    process.exit(1);
  }
  console.log('corpus-retention-vectors.json matches the oracle');
} else {
  writeFileSync(fixturePath, fixture);
  console.log(`wrote ${fixturePath.pathname} (limit=${limit}, pruned=${vectors.prunedCount})`);
}
