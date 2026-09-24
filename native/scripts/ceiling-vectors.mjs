// Read-side ceiling-draw oracle: the retained JavaScript computes the figures.
//
// spent/consumed come from the retained ledger (lib/metering/ledger.js):
// `currentPeriod().drawn` is fresh tokens only (CEILING_KINDS: input, output,
// cacheWrite) and `.total` is the display figure over all four kinds. The
// combination with commitments is the two retained lines of `remainingFor`
// (backend-v2.js:14052-14053), mirrored verbatim below:
//
//   const drawn = spent === null ? reserved : Math.max(reserved, spent);
//
// An absent period bucket is unknown, never zero, so the commitment figure
// stands alone. Both source files are pinned by sha256 so a drifted oracle
// fails --check instead of silently re-blessing different arithmetic.
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createUsageLedger } from '../../lib/metering/ledger.js';
import { overCommitMessage } from '../../lib/engagement-store.js';
import { createAlertStore } from '../../lib/alert-store.js';

const sha = (p) => createHash('sha256').update(readFileSync(new URL(p, import.meta.url), 'utf8').replaceAll('\r\n', '\n')).digest('hex');
const ledgerSha256 = sha('../../lib/metering/ledger.js');
const backendSha256 = sha('../../backend-v2.js');
const engagementStoreSha256 = sha('../../lib/engagement-store.js');
const alertStoreSha256 = sha('../../lib/alert-store.js');

// The Rust fixture (native/hagency-store/tests/usage.rs Fixture::new) commits
// exactly one approved 100-token engagement on the resource, so reserved is
// this literal on both sides of the oracle.
const RESERVED = 100;
const T0 = Date.parse('2026-08-15T12:00:00.000Z');
const HOUR = 3_600_000;
const NEXT_MONTH = Date.parse('2026-09-15T12:00:00.000Z');
const counts = (input, output, cacheWrite, cacheRead) => ({ input, output, cacheWrite, cacheRead });

const cases = [
  // The lockout that motivated the operator ruling: 13.6M measured against a
  // 10M ceiling of which only 681k is fresh work (BigLittle, 2026-08-12).
  { name: 'lockout-cache-swamp', sources: [{ key: 'a', observations: [[T0, counts(604_823, 76_266, 0, 12_928_512)]] }] },
  // The counter-case: the spend is fresh, the ceiling is genuinely gone.
  { name: 'fresh-exhaustion', sources: [{ key: 'a', observations: [[T0, counts(9_500_000, 500_000, 0, 0)]] }] },
  // With no cache reads the two figures agree; cacheWrite draws because it
  // bills above fresh input.
  { name: 'no-cache-parity', sources: [{ key: 'a', observations: [[T0, counts(1000, 500, 250, 0)]] }] },
  { name: 'cache-write-draws', sources: [{ key: 'a', observations: [[T0, counts(0, 0, 5000, 0)]] }] },
  // Below the commitment: committed allocations are the binding draw.
  { name: 'committed-binding', sources: [{ key: 'a', observations: [[T0, counts(40, 0, 0, 0)]] }] },
  // Cache-read growth moves consumption but never the draw.
  { name: 'cache-read-growth-never-draws', sources: [{ key: 'a', observations: [[T0, counts(100, 50, 0, 0)], [T0 + HOUR, counts(100, 50, 0, 5000)]] }] },
  // A query in a period nobody measured is unknown, so commitments stand alone.
  { name: 'month-roll-unknown', sources: [{ key: 'a', observations: [[T0, counts(700, 0, 0, 0)]] }], queryAt: NEXT_MONTH },
  // Two sources on the same engagement add per-kind before the draw is taken.
  { name: 'two-sources-additive', sources: [
    { key: 'a', observations: [[T0, counts(100, 0, 0, 0)]] },
    { key: 'b', observations: [[T0 + HOUR, counts(0, 50, 0, 0)]] },
  ] },
];

const vectors = cases.map(({ name, sources, queryAt }) => {
  // One merged chronological timeline so every record happens at its own
  // observation instant, exactly like the native writer transaction does.
  const events = sources.flatMap((s) => s.observations.map(([at, totals]) => ({ at, key: s.key, totals })))
    .sort((x, y) => x.at - y.at);
  let now = events[0].at;
  const ledger = createUsageLedger({ now: () => now });
  for (const event of events) {
    now = event.at;
    ledger.record([{ agent: 'bound-agent', framework: 'claude', sessions: [{ key: event.key, totals: event.totals }] }]);
  }
  const at = queryAt ?? events[events.length - 1].at;
  const bucket = ledger.currentPeriod('bound-agent', 'monthly', at);
  const spent = bucket ? bucket.drawn : null;
  const consumed = bucket ? bucket.total : null;
  // backend-v2.js:14052-14053, mirrored verbatim.
  const drawn = spent === null ? RESERVED : Math.max(RESERVED, spent);
  return {
    name,
    reserved: RESERVED,
    ceilingTokens: 1000,
    period: 'monthly',
    queryAt: at,
    sources: sources.map((s) => ({ key: s.key, observations: s.observations.map(([at2, totals]) => ({ at: at2, totals })) })),
    expected: { reserved: RESERVED, spent, consumed, drawn, spendPeriodKey: bucket ? bucket.key : null },
  };
});

// Slice 2: refusal-message vectors. The expected strings are computed by
// importing overCommitMessage from the retained engagement-store.js (exported
// at :73), so the Rust over_commit_message stays pinned byte-for-byte to the
// JavaScript wording. The three cases mirror
// tests/ceiling-draws-fresh-tokens.test.js:279-346 exactly.
const messageCases = [
  { name: 'plain-form', agent: 'a1', alloc: 500, remaining: 100, context: null },
  {
    name: 'measured-binding',
    agent: 'BigLittle',
    alloc: 50_000,
    remaining: 0,
    context: {
      period: 'monthly',
      reserved: 250_000,
      spent: 10_000_000,
      consumed: 13_609_601,
      ceilingTokens: 10_000_000,
      presetName: 'codex-strong',
      spendPeriodKey: '2026-08',
    },
  },
  {
    name: 'committed-binding-mirror',
    agent: 'BigLittle',
    alloc: 50_000,
    remaining: 0,
    context: {
      period: 'monthly',
      reserved: 9_000_000,
      spent: 100_000,
      consumed: 100_000,
      ceilingTokens: 9_000_000,
      presetName: 'codex-strong',
      spendPeriodKey: '2026-08',
    },
  },
];
const messageVectors = messageCases.map(({ name, agent, alloc, remaining, context }) => ({
  name,
  agent,
  alloc,
  remaining,
  context,
  expected: overCommitMessage({ agent }, alloc, remaining, context),
}));

// Slice 3: end-to-end admission expectations transcribed from
// tests/ceiling-draws-fresh-tokens.test.js:177-232. The ledger computes the
// fresh draw; the admission rule is the mirrored remainingFor arithmetic
// (backend-v2.js:14052-14057) — drawn = spent === null ? reserved :
// max(reserved, spent); byCeiling = max(0, ceiling - drawn); approve iff
// alloc <= min of non-null limits; after approval reserved grows by alloc.
const CEILING = 10_000_000;
const ALLOC = 1_000_000;
const admissionCases = [
  // THE LOCKOUT: 13.6M consumed, 681k drawn — approves 1M under a 10M ceiling.
  { name: 'admission-lockout', seed: { input: 604_823, output: 76_266, cacheWrite: 0, cacheRead: 12_928_512 } },
  // The counter-case: 10M FRESH — refuses 1M because the ceiling is genuinely gone.
  { name: 'admission-fresh-exhaustion', seed: { input: 9_500_000, output: 500_000, cacheWrite: 0, cacheRead: 0 } },
];
const admissionVectors = admissionCases.map(({ name, seed }) => {
  const ledger = createUsageLedger({ now: () => T0 });
  ledger.record([{ agent: 'bound-agent', framework: 'claude', sessions: [{ key: 'a', totals: seed }] }]);
  const bucket = ledger.currentPeriod('bound-agent', 'monthly', T0);
  const spent = bucket ? bucket.drawn : null;
  const consumed = bucket ? bucket.total : null;
  const reservedBefore = 0;
  const drawnBefore = spent === null ? reservedBefore : Math.max(reservedBefore, spent);
  const byCeiling = Math.max(0, CEILING - drawnBefore);
  const approved = ALLOC <= byCeiling;
  const reservedAfter = approved ? reservedBefore + ALLOC : reservedBefore;
  const drawnAfter = spent === null ? reservedAfter : Math.max(reservedAfter, spent);
  return {
    name,
    ceilingTokens: CEILING,
    alloc: ALLOC,
    seed,
    expected: {
      spent,
      consumed,
      drawnBefore,
      byCeiling,
      approved,
      remainingAfterApproval: Math.max(0, CEILING - drawnAfter),
    },
  };
});

// Slice (a)/(B6): the alert state machine EXECUTED by the retained
// lib/alert-store.js, not transcribed. Each case drives a real
// createAlertStore (fake clock, in-memory save) through the same transitions
// the native sweep performs — ingest on over (the payload shape
// backend-v2.js:9422-9447 sends), autoResolve on recovery — and derives
// raised/updated/resolved from the store's own outcomes (the ingest `created`
// flag, autoResolve's return), the mapping onto the native SweepOutcome
// counters. The one deliberate non-encoding: the reopen-window divergence
// (Node mints a NEW record when a re-over lands outside the 5-minute window,
// alert-store.js:254-271; native reopens the one row, ADR-124) — the vectors
// cover only the sequences where the two agree, and native's reopen is
// pinned by its own replay-test block.
const ALERT_RESOLVED_TTL_MS = 7 * 86400_000;
const sweepSeed = (name, ceiling, seed) => ({ name, ceiling, seed });

// Each case is a SEQUENCE of sweeps over one resource: state carries forward
// so dedupe/resolve transitions are exercised in order. `reserved` is the
// native fixture's commitment figure (100) except where a case says 0.
const sweepCases = [
  // Over by commitment: the 1.5M-committed-then-lowered-to-1M case the
  // retained test uses (:86). Unknown measurement falls back to reserved.
  sweepSeed('over-by-commitment', 1_000_000, { reserved: 1_500_000, spent: null, sweeps: 3 }),
  // The mutant-killer (:122): 1.2M FRESH with nothing committed, cacheRead
  // deliberately huge and excluded — drawn must be 1.2M, not 10.2M, not 0.
  sweepSeed('over-by-measured', 1_000_000, { reserved: 0, spent: 1_200_000, consumed: 10_200_000, sweeps: 1 }),
  // Inside the ceiling: no alert, ever.
  sweepSeed('inside', 2_000_000, { reserved: 500_000, spent: null, sweeps: 2 }),
  // Exactly ON the ceiling is not over it (strict >).
  sweepSeed('exactly-on', 1_000_000, { reserved: 1_000_000, spent: null, sweeps: 1 }),
  // No declared ceiling: skipped, not over.
  sweepSeed('no-ceiling', null, { reserved: 900_000, spent: null, sweeps: 1 }),
];

// One retained-store ingest carrying the sweep's payload
// (backend-v2.js:9422-9447); the actionability fields keep it a `warning`
// (buildActionability, alert-store.js:102-138).
function ingestOverrun(store, name, ceiling, reserved, spent, drawn, over) {
  const dedupeKey = `agent_ceiling_overrun:${name}`;
  return store.ingest({
    alertType: 'agent_ceiling_overrun',
    dedupeKey,
    severity: 'warning',
    source: 'backend',
    sourceAgent: name,
    summary: `${name} has drawn ${drawn} against a ceiling of ${ceiling} — ${over} past it`,
    detail: {
      agent: name, presetId: 'preset', ceilingTokens: ceiling,
      committedTokens: reserved, measuredTokens: spent ?? null,
      drawnTokens: drawn, overByTokens: over,
    },
    owner: 'hagency-operator',
    runbook: 'raise the ceiling on preset preset to cover what is already committed',
    impact: 'no new engagement can be approved against this agent',
    recoveryCondition: 'the drawn figure falls back under the ceiling',
    correlation: { dedupeKey },
    tags: ['ceiling', 'budget'],
  });
}

const sweepVectors = sweepCases.map(({ name, ceiling, seed }) => {
  const sweeps = seed.sweeps ?? 1;
  const t0 = 1_000_000;
  const key = `agent_ceiling_overrun:${name}`;
  let clock = t0;
  const store = createAlertStore({ now: () => clock, save: () => {} });
  const states = [];
  for (let i = 0; i < sweeps; i += 1) {
    clock = t0 + i * 3_600_000;
    const drawn = seed.spent === null || seed.spent === undefined
      ? seed.reserved
      : Math.max(seed.reserved, seed.spent);
    const over = ceiling === null ? null : drawn - ceiling;
    let raised = 0; let updated = 0; let resolved = 0;
    if (over !== null && over > 0) {
      const { created } = ingestOverrun(store, name, ceiling, seed.reserved, seed.spent, drawn, over);
      if (created) raised = 1; else updated = 1;
    } else if (store.autoResolve(key)) {
      resolved = 1;
    }
    states.push({ at: clock, raised, updated, resolved });
  }
  const [row] = store.listAlerts();
  const finalRow = row
    ? {
        occurrences: row.occurrences,
        resolvedAt: row.status === 'resolved' ? row.resolvedAt : null,
        resolvedBy: row.status === 'resolved' ? row.resolvedBy : null,
      }
    : null;
  const finalAt = t0 + (sweeps - 1) * 3_600_000;
  const pruned = row && row.status === 'resolved'
    && (finalAt - row.resolvedAt) > ALERT_RESOLVED_TTL_MS ? store.pruneResolved() : 0;
  return {
    name,
    ceilingTokens: ceiling,
    reserved: seed.reserved,
    spent: seed.spent ?? null,
    consumed: seed.consumed ?? null,
    sweeps,
    expected: { states, finalRow, pruned },
  };
});

// Month rollover: an unmeasured NEW period means spent=null so drawn falls
// back to reserved; a resource over only by the CLOSED period's measurement
// auto-resolves on the first post-rollover sweep (backend-v2.js:9410 with
// ceilingSpendFor reading the CURRENT period). Two phases through the REAL
// store: over by measurement, then the new period is unmeasured.
const rolloverVector = (() => {
  const name = 'month-rollover-resolves';
  const ceiling = 1_000;
  const reserved = 100;
  const t0 = 1_000_000;
  const nextMonth = t0 + 31 * 86400_000;
  const key = `agent_ceiling_overrun:${name}`;
  let clock = t0;
  const store = createAlertStore({ now: () => clock, save: () => {} });
  const phases = [
    { at: t0, spent: 1_200 },       // measured over: fresh vs 1k
    { at: nextMonth, spent: null }, // new period unmeasured
  ];
  const states = [];
  for (const phase of phases) {
    clock = phase.at;
    const drawn = phase.spent === null ? reserved : Math.max(reserved, phase.spent);
    const over = drawn - ceiling;
    let raised = 0; let updated = 0; let resolved = 0;
    if (over > 0) {
      const { created } = ingestOverrun(store, name, ceiling, reserved, phase.spent, drawn, over);
      if (created) raised = 1; else updated = 1;
    } else if (store.autoResolve(key)) {
      resolved = 1;
    }
    states.push({ at: phase.at, raised, updated, resolved });
  }
  const [row] = store.listAlerts();
  return {
    name,
    ceilingTokens: ceiling,
    reserved,
    spent: phases[0].spent,
    spentByPhase: phases.map((p) => p.spent),
    sweeps: phases.length,
    expected: {
      states,
      finalRow: {
        occurrences: row.occurrences,
        resolvedAt: row.status === 'resolved' ? row.resolvedAt : null,
        resolvedBy: row.status === 'resolved' ? row.resolvedBy : null,
      },
      pruned: 0,
    },
    note: 'unknown != zero: the closed period is not carried, so the first post-rollover sweep resolves',
  };
})();
sweepVectors.push(rolloverVector);

// Operator transitions (ADR-124 amendment): the RETAINED transition table
// replayed through the real store, over the four-state subset native
// carries. Executed, not transcribed: each case drives createAlertStore to
// one open alert, walks `from` (ingest → open; open→acknowledged;
 // open→suppressed), then applies `to` and records what the store did.
// The named divergences (native map vs retained store, both documented in
// the ADR amendment): native ADDS acknowledged→suppressed and
// suppressed→resolved (the retained console's own NEXT_STATUS offers
// acknowledged→suppressed — drift not ported), and DROPS the assigned
// state entirely (no agent-token authority natively). Only the pairs legal
// in BOTH models are encoded here; native's full four-state table is
// pinned by its own store test.
const TRANSITION_ACTOR = 'operator';
const transitionPairs = [
  ['open', 'acknowledged'],
  ['open', 'resolved'],
  ['open', 'suppressed'],
  ['acknowledged', 'resolved'],
  ['suppressed', 'open'],
];
function openAlertStore(name, ceiling, reserved, drawn) {
  let clock = 1_000_000;
  const store = createAlertStore({ now: () => clock, save: () => {} });
  const over = drawn - ceiling;
  const { created } = ingestOverrun(store, name, ceiling, reserved, null, drawn, over);
  if (!created) throw new Error('transition oracle seed did not create');
  return store;
}
const transitionVectors = transitionPairs.map(([from, to]) => {
  const name = `transition_${from}_${to}`;
  const store = openAlertStore(name, 1_000_000, 1_500_000, 1_500_000);
  const [seeded] = store.listAlerts();
  const id = seeded.id;
  if (from === 'acknowledged') store.transition(id, 'acknowledged', { actor: TRANSITION_ACTOR });
  if (from === 'suppressed') store.transition(id, 'suppressed', { actor: TRANSITION_ACTOR });
  const alert = store.transition(id, to, { actor: TRANSITION_ACTOR });
  return {
    from, to,
    expected: {
      status: alert.status,
      resolvedBy: alert.status === 'resolved' ? alert.resolvedBy : null,
      // The retained store keeps occurrences/lastSeen untouched by a
      // transition; native's UPDATE does the same (display columns only).
      occurrences: alert.occurrences,
    },
  };
});
// The terminal refusal both models share: resolved allows nothing.
transitionVectors.push((() => {
  const name = 'transition_resolved_refused';
  const store = openAlertStore(name, 1_000_000, 1_500_000, 1_500_000);
  const [seeded] = store.listAlerts();
  store.transition(seeded.id, 'resolved', { actor: TRANSITION_ACTOR });
  let refused = null;
  try { store.transition(seeded.id, 'open', { actor: TRANSITION_ACTOR }); }
  catch (error) { refused = error.code; }
  return { from: 'resolved', to: 'open', expected: { refusal: refused } };
})());

const output = JSON.stringify({
  source: 'lib/metering/ledger.js + backend-v2.js remainingFor drawn rule',
  ledgerSha256,
  backendSha256,
  engagementStoreSha256,
  semantics: 'fresh-token draw per current period; max(reserved, spent) with unknown fallback',
  vectors,
  messages: messageVectors,
  admission: admissionVectors,
  sweeps: sweepVectors,
  transitions: transitionVectors,
}, null, 2) + '\n';
const path = new URL('../hagency-store/tests/fixtures/ceiling-vectors.json', import.meta.url);if (process.argv.includes('--check')) {
  if (readFileSync(path, 'utf8').replaceAll('\r\n', '\n') !== output) throw new Error('Ceiling vectors differ from retained JavaScript');
} else writeFileSync(path, output);
console.log(JSON.stringify({ vectors: vectors.length }));
