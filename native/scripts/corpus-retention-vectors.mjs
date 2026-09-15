// Corpus retention vectors (ADR-125). Slice 1 briefly added test-only
// export hooks to backend-v2.js so this oracle could EXECUTE the retained
// planMessagePrune; that broke the rule the hosted run enforced
// (ceiling-vectors.mjs pins backend-v2.js's sha — the port never edits the
// retained file, byte for byte). Restored: this script does not import the
// retained backend and needs no hooks from it.
//
// WHERE THE VECTORS COME FROM: the retained code HAS corpus retention
// (backend-v2.js:3367-3386 planMessagePrune, :3325-3334
// collectUnreadRetainedMessageIds and :3336-3353
// collectRouterUncopiedMessageIds — the keep-set collectors), but it is
// not exported, and the port may not edit the file
// to export it. So the arithmetic below MIRRORS the retained lines with
// their citations — the same convention ceiling-vectors.mjs uses at its
// lines 73 and 136 — derived from ADR-125's stated rules:
//
//   backend-v2.js:3367-3386, mirrored:
//     if (list.length <= MESSAGE_RETENTION_LIMIT) -> retained=all, pruned=[];
//     retainFrom = max(0, list.length - MESSAGE_RETENTION_LIMIT);
//     keep = i >= retainFrom || unreadKeepIds.has(id) || routerKeepIds.has(id);
//   backend-v2.js:236, mirrored:
//     MESSAGE_RETENTION_LIMIT = max(100, parseInt(env) || 5000);
//
// The keep-set inputs (unread agents' inboxes, router-uncopied thread
// sources) are SEEDED directly — the seed IS the keep-set, so the mirrored
// planner computes the partition over it exactly as the retained planner
// would over its own globals. Native's extra clauses (P2..P10) have no
// retained counterpart and are pinned by the store tests, not here — the
// same honest split as before.
//
// PROVENANCE PIN: backendSha256 records the retained file's sha at the
// time the vectors were derived, so a reviewer can tell WHICH retained
// bytes the mirror was checked against. It is not drift enforcement: the
// retained file may legitimately change without these vectors changing,
// because the vectors derive from the mirrored rules (re-checked by
// review), not from executing the file. The Rust parity test asserts this
// same value.
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';

const sha = (p) => createHash('sha256').update(readFileSync(new URL(p, import.meta.url), 'utf-8').replaceAll('\r\n', '\n')).digest('hex');
const backendSha256 = sha('../../backend-v2.js');

// The pinned limit (backend-v2.js:236's floor): small enough that the
// vector stays cheap, above the floor the env guard enforces.
const LIMIT = 120;

// Seed shape, unchanged from the executable-oracle era: two agent records;
// 40 old rows no unread inbox ever holds (the pruned half), LIMIT rows
// routed to a live agent with its cursor at 0 (unread — the keep-set),
// and one group-mention row at the tail. The seed IS the keep-set input
// the retained collectors would read from their globals.
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

// The keep-set the seed implies (the retained collectors' semantics,
// backend-v2.js:3325-3334 mirrored): every message routed `to` an agent
// whose cursor has not passed it is unread; the group-mention row rides
// the tail inside the recency window anyway; router-uncopied is empty
// (no thread-session sources in this seed).
const unreadKeepIds = new Set(
  seed.filter((m) => typeof m.to === 'string' && m.to === 'alpha').map((m) => m.id),
);
const routerKeepIds = new Set();

// The mirrored planner (backend-v2.js:3367-3386). `seed` arrives in
// insertion order, which IS ts order (monotone ts), matching the retained
// caller's `messages` array.
const planMessagePrune = (rows) => {
  const list = Array.isArray(rows) ? rows : [];
  if (list.length <= LIMIT) {
    return { retained: list, pruned: [] };
  }
  const retainFrom = Math.max(0, list.length - LIMIT);
  const retained = [];
  const pruned = [];
  for (let i = 0; i < list.length; i += 1) {
    const msg = list[i];
    const keep = i >= retainFrom || (typeof msg?.id === 'string'
      && (unreadKeepIds.has(msg.id) || routerKeepIds.has(msg.id)));
    if (keep) retained.push(msg);
    else pruned.push(msg);
  }
  return { retained, pruned };
};

const plan = planMessagePrune(seed);

const vectors = {
  backendSha256,
  observedLimit: LIMIT,
  keep: {
    unread: [...unreadKeepIds],
    routerUncopied: [...routerKeepIds],
  },
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
  // Git checks text files with no eol attribute out as CRLF on Windows
  // (this fixture lives in hagency-store/tests/fixtures, not the pinned
  // /native/fixtures the .gitattributes marks eol=lf). Compare normalized,
  // exactly as matrix-format/metering/ceiling already do.
  const current = readFileSync(fixturePath, 'utf-8').replaceAll('\r\n', '\n');
  if (current !== fixture) {
    console.error('corpus-retention-vectors.json drifts from the oracle; regenerate with:');
    console.error('  node native/scripts/corpus-retention-vectors.mjs');
    process.exit(1);
  }
  console.log('corpus-retention-vectors.json matches the oracle');
} else {
  writeFileSync(fixturePath, fixture);
  console.log(`wrote ${fixturePath.pathname} (limit=${LIMIT}, pruned=${vectors.prunedCount})`);
}
