// Approval wire and client-origin oracle: the RETAINED modules compute the vectors.
//
// PC-C4 of the migration-gates backlog. Two retained sources decide the
// approval wire shapes and the proxy origin rule, and both are EXECUTED here
// over fixed inputs rather than transcribed:
//
//  - bridge-matrix.js — `buildOwnerApprovalRequest` (:2600-2646),
//    `buildPublicApprovalNotice` (:2578-2598) and `parseApprovalVerdictEvent`
//    (:2648-2689), over the event constants at :215-220. These are the packet
//    the owner's private DM receives and the parser that reads the owner's
//    verdict back.
//  - mockup/app/api/hagency/[...path]/route.js — `sameOriginWrite` (:379-389),
//    the predicate that decides whether a state-changing console request came
//    from this app at all.
//
// WHY THE SOURCES ARE SLICED RATHER THAN IMPORTED. route.js has no imports and
// is evaluated whole (appended internals + a `data:` URL). bridge-matrix.js
// CANNOT be imported in a bare checkout: it pulls `matrix-bot-sdk`, which is
// absent from `node_modules` in this clone, so `import()` fails with
// ERR_MODULE_NOT_FOUND. The three functions and the six constants they need are
// therefore lifted by literal line anchor and evaluated as source text — the
// real retained statements, not a re-implementation — and every anchor is
// asserted so a moved block fails loudly instead of silently changing what
// "expected" means. This is the one deviation from the `ceiling-vectors.mjs`
// precedent, which imports its retained modules directly; it is recorded in the
// report because it changes what a future reader can assume.
//
// Both sources are pinned by sha256, so a drifted oracle fails `--check`
// instead of re-blessing different rules.
//
// The native counterparts these vectors are compared against:
// `hagency-matrix/src/approval_delivery/state.rs` (`Frozen::validate`, :96-124)
// for the packet, and `hagency/src/console.rs` (`same_origin`, :110-123) for the
// origin rule.
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const BRIDGE_FILE = 'bridge-matrix.js';
const ROUTE_FILE = 'mockup/app/api/hagency/[...path]/route.js';

const sha = (text) => createHash('sha256').update(text.replaceAll('\r\n', '\n')).digest('hex');
const bridgeSource = readFileSync(path.join(ROOT, BRIDGE_FILE), 'utf8');
const routeSource = readFileSync(path.join(ROOT, ROUTE_FILE), 'utf8');
const bridgeSha256 = sha(bridgeSource);
const routeSha256 = sha(routeSource);

// ── bridge-matrix.js: lift the constants and three function bodies ─────────
const bridgeLines = bridgeSource.split('\n');
const lineIndex = (prefix, label) => {
  const index = bridgeLines.findIndex((line) => line.startsWith(prefix));
  if (index < 0) throw new Error(`${label}: anchor not found: ${prefix}`);
  return index;
};
const functionBlock = (prefix, label) => {
  const start = lineIndex(prefix, label);
  let end = -1;
  for (let index = start + 1; index < bridgeLines.length; index += 1) {
    if (bridgeLines[index] === '}') { end = index; break; }
  }
  if (end < 0) throw new Error(`${label}: closing brace not found`);
  return bridgeLines.slice(start, end + 1).join('\n');
};
const constants = bridgeLines
  .slice(
    lineIndex('const APPROVAL_EVENT_KEY =', 'APPROVAL_EVENT_KEY'),
    lineIndex('const LEGACY_APPROVAL_VERDICT_MSGTYPE =', 'LEGACY_APPROVAL_VERDICT_MSGTYPE') + 1,
  )
  .join('\n');
const functions = [
  functionBlock('export function buildPublicApprovalNotice(approval) {', 'buildPublicApprovalNotice'),
  functionBlock('export function buildOwnerApprovalRequest(approval) {', 'buildOwnerApprovalRequest'),
  functionBlock('export function parseApprovalVerdictEvent(roomId, event) {', 'parseApprovalVerdictEvent'),
].join('\n').replace(/^export /gm, '');

const retained = new Function(`${constants}\n${functions}\nreturn {
  buildPublicApprovalNotice, buildOwnerApprovalRequest, parseApprovalVerdictEvent,
  APPROVAL_EVENT_KEY, APPROVAL_REQUEST_MSGTYPE, APPROVAL_STATUS_MSGTYPE,
};`)();
const KEY = retained.APPROVAL_EVENT_KEY;

// ── route.js: evaluate the whole module capturing the origin predicate ─────
const routeUrl = `data:text/javascript;base64,${Buffer.from(
  `${routeSource}\nglobalThis.__approvalOrigin = { sameOriginWrite };\n`,
).toString('base64')}`;
await import(routeUrl);
const { sameOriginWrite } = globalThis.__approvalOrigin;
delete globalThis.__approvalOrigin;

const proxyRequest = (method, headers) => ({
  method,
  url: 'http://127.0.0.1:3100/api/hagency/engagements/eng_1/verdict',
  headers: { get: (name) => (name in headers ? headers[name] : null) },
});

// ── The fixed input tables ────────────────────────────────────────────────
const EXPIRES_AT = Date.parse('2026-09-13T12:00:00.000Z');
const REQUEST_32 = `approval_${'a'.repeat(32)}`;
const REQUEST_40 = `approval_${'b'.repeat(40)}`;
const DIGEST = 'c'.repeat(64);
const OWNER = '@owner:hq.test';
const PROJECT_ROOM = '!project:hq.test';

const base = (overrides) => ({
  agent: 'agent-one',
  project: 'project-one',
  project_room_id: PROJECT_ROOM,
  id: REQUEST_32,
  upstream_request_id: '7',
  input_digest: DIGEST,
  runtime: 'codex',
  tool_name: 'app_server_command',
  description: 'Run the requested command',
  input_preview: '{"command":"echo ok"}',
  expires_at: EXPIRES_AT,
  ...overrides,
});

// 1. The private request packet, and the action list it offers. `approve_task`
//    needs a task id; `approve_always` needs any reusable scope at all
//    (bridge-matrix.js:2615-2620).
const scope = (overrides) => ({ description: 'Workspace write', workspace: '/work/a', ...overrides });
const requestCases = [
  { name: 'bare-no-scope', input: base({}) },
  { name: 'scoped-with-task', input: base({ reusable_scope: scope({ task_id: 'task_1' }) }) },
  { name: 'scoped-project-only', input: base({ reusable_scope: scope({}) }) },
  { name: 'long-preview', input: base({ input_preview: 'x'.repeat(9000) }) },
  { name: 'blank-agent-and-project', input: base({ agent: '', project: '' }) },
].map(({ name, input }) => {
  const packet = retained.buildOwnerApprovalRequest(input);
  return {
    name,
    input: { ...input, input_preview: input.input_preview.slice(0, 64) + (input.input_preview.length > 64 ? '…' : '') },
    expected: {
      msgtype: packet.msgtype,
      actions: packet[KEY].actions.map((action) => action.id),
      requestIdPreserved: packet[KEY].request_id,
      descriptionBytes: Buffer.byteLength(packet[KEY].description, 'utf8'),
      inputPreviewBytes: Buffer.byteLength(packet[KEY].input_preview, 'utf8'),
      noTruncation: packet[KEY].input_preview === String(input.input_preview || ''),
    },
  };
});

// 2. The redacted public status notice (ADR-003's public surface).
const noticeCases = [
  { name: 'plain', input: { agent: 'agent-one', project: 'project-one' } },
  { name: 'threaded', input: { agent: 'agent-one', project: 'project-one', thread_root_event_id: '$root' } },
].map(({ name, input }) => ({
  name,
  input,
  expected: retained.buildPublicApprovalNotice(input),
}));

// 3. The owner-verdict parser's accept/refuse matrix. Two complete shapes are
//    accepted — current (com.agentchat.* both) and legacy (com.hagency.* both)
//    — and nothing mixed (bridge-matrix.js:2651-2657).
const verdictDetail = (overrides) => ({
  version: 1, kind: 'verdict', action: 'approve_once',
  request_id: REQUEST_32, agent: 'agent-one', project: 'project-one',
  project_room_id: PROJECT_ROOM, input_digest: DIGEST, ...overrides,
});
const verdictEvent = (msgtype, key, detail, sender = OWNER) => ({
  sender, event_id: '$verdict', content: { msgtype, [key]: detail },
});
const CURRENT_MSGTYPE = 'com.agentchat.approval.verdict.v1';
const LEGACY_MSGTYPE = 'com.hagency.approval.verdict.v1';
const LEGACY_KEY = 'com.hagency.approval';
const verdictCases = [
  { name: 'current-approve-once', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ action: 'approve_once' })) },
  { name: 'current-deny', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ action: 'deny' })) },
  { name: 'legacy-paired-deny', event: verdictEvent(LEGACY_MSGTYPE, LEGACY_KEY, verdictDetail({ action: 'deny' })) },
  { name: 'mixed-current-msgtype-legacy-key', event: verdictEvent(CURRENT_MSGTYPE, LEGACY_KEY, verdictDetail({})) },
  { name: 'mixed-legacy-msgtype-current-key', event: verdictEvent(LEGACY_MSGTYPE, KEY, verdictDetail({})) },
  // The retained regex admits exactly thirty-two hex characters; a
  // forty-character id is refused here and admitted natively (ADR-115).
  { name: 'forty-hex-request-id', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ request_id: REQUEST_40 })) },
  { name: 'short-request-id', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ request_id: 'approval_ff' })) },
  { name: 'non-hex-request-id', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ request_id: `approval_${'z'.repeat(32)}` })) },
  { name: 'unknown-action', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ action: 'approve_forever' })) },
  { name: 'wrong-version', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ version: 2 })) },
  { name: 'wrong-kind', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ kind: 'request' })) },
  { name: 'bad-sender-mxid', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({}), 'not-an-mxid') },
  { name: 'bad-project-room-id', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ project_room_id: 'project' })) },
  { name: 'bad-input-digest', event: verdictEvent(CURRENT_MSGTYPE, KEY, verdictDetail({ input_digest: 'short' })) },
  { name: 'foreign-msgtype', event: verdictEvent('com.other.verdict.v1', KEY, verdictDetail({})) },
  { name: 'missing-content', event: { sender: OWNER, content: null } },
].map(({ name, event }) => ({
  name,
  event,
  expected: retained.parseApprovalVerdictEvent('!owner-dm:hq.test', event),
}));

// 4. The proxy write-origin rule, scoped to the one mutation this oracle's
//    subject matter uses (a verdict POST). Slice 1's console-origin oracle
//    owns the exhaustive table; these rows are the approval-relevant subset.
const CONSOLE_HOST = '127.0.0.1:3100';
const originCases = [
  { name: 'sec-fetch-same-origin', headers: { 'sec-fetch-site': 'same-origin' } },
  { name: 'sec-fetch-cross-site', headers: { 'sec-fetch-site': 'cross-site' } },
  { name: 'sec-fetch-none', headers: { 'sec-fetch-site': 'none' } },
  { name: 'origin-same-host', headers: { origin: `http://${CONSOLE_HOST}` } },
  { name: 'origin-foreign-host', headers: { origin: 'https://evil.example.com' } },
  { name: 'no-browser-headers', headers: {} },
].map(({ name, headers }) => ({
  name,
  secFetchSite: headers['sec-fetch-site'] ?? null,
  origin: headers.origin ?? null,
  expected: { allowed: sameOriginWrite(proxyRequest('POST', headers)) },
}));

// ── Notes the fixture carries but does not execute ────────────────────────
const nativeNotes = [
  {
    rule: 'request-id-length',
    retained: 'The verdict parser admits exactly thirty-two lowercase hex characters after `approval_` (bridge-matrix.js:2676).',
    native: 'ADR-115 pairs the profiles and admits thirty-two OR forty; the row `forty-hex-request-id` refuses here and must be accepted natively.',
    citation: 'knowledge/decisions/adr-115-native-approval-wire-interop.md',
  },
  {
    rule: 'packet-kind',
    retained: 'A status notice carries `kind: "status"` under the same event key as the request (bridge-matrix.js:2586-2592).',
    native: '`Frozen::validate` refuses any non-request msgtype, any kind other than `request`, and any content object not exactly three keys; a status notice cannot pass it unmodified.',
    citation: 'native/hagency-matrix/src/approval_delivery/state.rs:104-124',
  },
  {
    rule: 'public-status-schema',
    retained: 'The status notice shape has a checked-in schema: `kind: "status"`, `state: "waiting_for_owner"` at most 512 bytes.',
    native: 'No native encoder emits it yet; the schema is the contract a future slice binds.',
    citation: 'schemas/approval/public-status-v1.schema.json',
  },
];

const output = JSON.stringify({
  source: 'bridge-matrix.js + mockup/app/api/hagency/[...path]/route.js',
  bridgeFile: BRIDGE_FILE,
  routeFile: ROUTE_FILE,
  bridgeSha256,
  routeSha256,
  semantics: 'two complete verdict shapes and nothing mixed; 32-hex request ids; approve_task needs a task id; one-origin write gate',
  eventKey: KEY,
  requestMsgtype: retained.APPROVAL_REQUEST_MSGTYPE,
  statusMsgtype: retained.APPROVAL_STATUS_MSGTYPE,
  requests: requestCases,
  notices: noticeCases,
  verdicts: verdictCases,
  origin: originCases,
  nativeNotes,
}, null, 2) + '\n';

const target = path.join(ROOT, 'native/hagency-matrix/tests/fixtures/approval-vectors.json');
if (process.argv.includes('--check')) {
  const current = readFileSync(target, 'utf8').replaceAll('\r\n', '\n');
  if (current !== output) throw new Error('Approval vectors differ from the retained bridge and proxy');
} else {
  writeFileSync(target, output);
}
console.log(JSON.stringify({
  requests: requestCases.length,
  notices: noticeCases.length,
  verdicts: verdictCases.length,
  origin: originCases.length,
}));
