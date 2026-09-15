// Console-origin oracle: the RETAINED proxy and backend compute the verdicts.
//
// CL-S4' of the migration-gates backlog. The console's origin rules live in
// two retained sources, and both are EXECUTED here over a fixed request
// table rather than transcribed:
//
//  - mockup/app/api/hagency/[...path]/route.js — `sameOriginWrite`
//    (:379-389), the default-deny allowlist `allowed` (:264-267) over `READS`
//    (:35-128) and `WRITES` (:136-262), the path canonicaliser
//    `canonicalSegments` / `SAFE_SEGMENT` (:295-310), and the proxy caller
//    gate `callerAllowed` (:342-363).
//  - backend-v2.js — the one-origin CORS middleware for `/api`
//    (:7559-7568) and the loopback peer predicate `isLocalRequest`
//    (:1384-1387) over `LOCALHOST_IPS` (:156) and `CORS_ALLOWED_ORIGIN`
//    (:175).
//
// WHY THE SOURCES ARE SLICED RATHER THAN IMPORTED. route.js has no imports
// and is evaluated whole (appended internals + a `data:` URL). backend-v2.js
// cannot be imported at all in a bare checkout: its transitive dependencies
// (express, matrix-bot-sdk, better-sqlite3) are absent from `node_modules`,
// so `import()` fails with ERR_MODULE_NOT_FOUND. The two retained functions
// that matter are therefore extracted by literal anchor and evaluated as
// source text — they are the real retained statements, not a re-implementation,
// and the anchors are asserted so a moved block fails loudly instead of
// silently changing what "expected" means.
//
// Both sources are pinned by sha256, so a drifted oracle fails `--check`
// instead of re-blessing different rules.
//
// The native hoops this oracle is compared against are
// native/hagency/src/console.rs: `common_authority` (:98-109) and `same_origin`
// (:110-123). The three places native is deliberately STRICTER are recorded in
// the `divergences` section below and cited in the ADR-107 amendment; the Rust
// test binds them, this script only states them.
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const ROUTE_FILE = 'mockup/app/api/hagency/[...path]/route.js';
const BACKEND_FILE = 'backend-v2.js';
const routePath = path.join(ROOT, ROUTE_FILE);
const backendPath = path.join(ROOT, BACKEND_FILE);

const sha = (text) => createHash('sha256').update(text.replaceAll('\r\n', '\n')).digest('hex');
const routeSource = readFileSync(routePath, 'utf8');
const backendSource = readFileSync(backendPath, 'utf8');
const routeSha256 = sha(routeSource);
const backendSha256 = sha(backendSource);

// ── route.js: evaluate the whole module, capturing its internals ──────────
async function routeInternals(tag) {
  const patched = `${routeSource}\nglobalThis.__consoleOrigin = { sameOriginWrite, canonicalSegments, SAFE_SEGMENT, allowed, callerAllowed, READS, WRITES };\n//${tag}\n`;
  const url = `data:text/javascript;base64,${Buffer.from(patched).toString('base64')}`;
  await import(url);
  const internals = globalThis.__consoleOrigin;
  delete globalThis.__consoleOrigin;
  return internals;
}

// Two instances, because `callerAllowed` reads HAGENCY_CONSOLE_TOKEN at module
// evaluation time and the data: URL cache would otherwise return the first.
const CONSOLE_TOKEN_VALUE = 'oracle-console-token';
const previousToken = process.env.HAGENCY_CONSOLE_TOKEN;
delete process.env.HAGENCY_CONSOLE_TOKEN;
const routeNoToken = await routeInternals('no-token');
process.env.HAGENCY_CONSOLE_TOKEN = CONSOLE_TOKEN_VALUE;
const routeWithToken = await routeInternals('with-token');
if (previousToken === undefined) delete process.env.HAGENCY_CONSOLE_TOKEN;
else process.env.HAGENCY_CONSOLE_TOKEN = previousToken;

const request = (method, url, headers) => ({
  method,
  url,
  headers: { get: (name) => (name in headers ? headers[name] : null) },
});

// ── backend-v2.js: slice the two retained blocks by literal anchor ────────
function sliceBetween(source, anchor, close, label) {
  const start = source.indexOf(anchor);
  if (start < 0) throw new Error(`${label}: anchor not found: ${anchor}`);
  const end = source.indexOf(close, start + anchor.length);
  if (end < 0) throw new Error(`${label}: close not found after: ${anchor}`);
  return source.slice(start, end + close.length);
}
const lineMatching = (source, prefix, label) => {
  const line = source.split('\n').find((candidate) => candidate.startsWith(prefix));
  if (!line) throw new Error(`${label}: line not found: ${prefix}`);
  return line;
};

const localhostLine = lineMatching(backendSource, 'const LOCALHOST_IPS = new Set(', 'LOCALHOST_IPS');
const corsOriginLine = lineMatching(backendSource, 'const CORS_ALLOWED_ORIGIN = (process.env.FRP_API_ORIGIN', 'CORS_ALLOWED_ORIGIN');
const isLocalRequestFn = sliceBetween(
  backendSource,
  'function isLocalRequest(req) {',
  '\n}',
  'isLocalRequest',
);
const corsMiddleware = sliceBetween(
  backendSource,
  "app.use('/api', (req, res, next) => {",
  '\n});',
  'cors middleware',
);

const backend = new Function(`
let CORS_MIDDLEWARE = null;
const app = { use: (_path, handler) => { CORS_MIDDLEWARE = handler; } };
${localhostLine}
${corsOriginLine}
let localRequestOverrideForTest = null;
${isLocalRequestFn}
${corsMiddleware}
return { isLocalRequest, CORS_MIDDLEWARE, CORS_ALLOWED_ORIGIN, LOCALHOST_IPS };
`)();

// ── The fixed request table ───────────────────────────────────────────────
const ALLOWED_ORIGIN = 'https://hagency.example.com';
const CONSOLE_HOST = '127.0.0.1:3100';
const url = (p) => `http://${CONSOLE_HOST}/api/hagency/${p}`;

// 1. The proxy's write-origin predicate. sec-fetch-site wins when present;
//    otherwise Origin's host is compared to the request's; neither header
//    present means "not a browser" and is allowed (route.js:374-389).
const sameOriginCases = [
  { name: 'sec-fetch-same-origin', method: 'POST', headers: { 'sec-fetch-site': 'same-origin' } },
  { name: 'sec-fetch-cross-site', method: 'POST', headers: { 'sec-fetch-site': 'cross-site' } },
  { name: 'sec-fetch-none', method: 'POST', headers: { 'sec-fetch-site': 'none' } },
  { name: 'sec-fetch-same-site', method: 'POST', headers: { 'sec-fetch-site': 'same-site' } },
  { name: 'origin-same-host', method: 'POST', headers: { origin: `http://${CONSOLE_HOST}` } },
  { name: 'origin-foreign-host', method: 'POST', headers: { origin: 'https://evil.example.com' } },
  { name: 'origin-malformed', method: 'POST', headers: { origin: 'not a url' } },
  { name: 'no-browser-headers', method: 'POST', headers: {} },
  // sec-fetch-site present and cross-site must lose to a matching Origin:
  // the retained rule returns on the FIRST header it finds.
  { name: 'sec-fetch-cross-site-with-same-origin', method: 'POST', headers: { 'sec-fetch-site': 'cross-site', origin: `http://${CONSOLE_HOST}` } },
].map(({ name, method, headers }) => ({
  name,
  method,
  url: url('engagements/eng_1/verdict'),
  secFetchSite: headers['sec-fetch-site'] ?? null,
  origin: headers.origin ?? null,
  expected: { allowed: routeNoToken.sameOriginWrite(request(method, url('engagements/eng_1/verdict'), headers)) },
}));

// 2. The default-deny allowlist. A path not named is refused, and the method
//    must match the entry (a GET allowlist entry does not admit a POST).
const allowlistCases = [
  { name: 'read-agents', method: 'GET', target: 'agents' },
  { name: 'read-alerts', method: 'GET', target: 'alerts' },
  { name: 'read-engagements', method: 'GET', target: 'engagements' },
  { name: 'read-usage', method: 'GET', target: 'usage' },
  { name: 'read-whitelist', method: 'GET', target: 'whitelist' },
  { name: 'write-verdict', method: 'POST', target: 'engagements/eng_1/verdict' },
  { name: 'write-revoke', method: 'POST', target: 'engagements/eng_1/revoke' },
  { name: 'write-alert-transition', method: 'POST', target: 'alerts/a1/transition' },
  { name: 'write-whitelist-add', method: 'POST', target: 'whitelist' },
  { name: 'write-whitelist-remove', method: 'DELETE', target: 'whitelist/!aXbY7pQ2:hq.example' },
  // Not on either list. The bridge-secret record `/api/approval-bindings` is
  // deliberately absent (route.js:63-72), and so is the credential reader
  // `/api/project-sides/inbound-credentials` (route.js:103-115).
  { name: 'refuse-approval-bindings', method: 'GET', target: 'approval-bindings' },
  { name: 'refuse-inbound-credentials', method: 'GET', target: 'project-sides/inbound-credentials' },
  // Method mismatch: `agents` is a read, never a delete.
  { name: 'refuse-agent-delete-via-read-entry', method: 'PUT', target: 'agents' },
  { name: 'refuse-unknown-path', method: 'GET', target: 'definitely-not-a-route' },
].map(({ name, method, target }) => ({
  name,
  method,
  target,
  expected: { allowed: routeNoToken.allowed(method, target) },
}));

// 3. Path canonicalisation. The proxy refuses a non-canonical segment before
//    matching, because the string checked must be the string requested
//    (route.js:269-310). `%252e%252e` is the documented traversal vector.
const canonicalisationCases = [
  { name: 'plain-segments', segments: ['whitelist', 'agents'] },
  { name: 'double-encoded-traversal', segments: ['whitelist', '%252e%252e', 'agents'] },
  { name: 'single-encoded-traversal', segments: ['whitelist', '%2e%2e', 'agents'] },
  { name: 'dot-segment', segments: ['whitelist', '.'] },
  { name: 'dotdot-segment', segments: ['whitelist', '..'] },
  { name: 'matrix-room-id', segments: ['whitelist', '!aXbY7pQ2:hq.example'] },
  { name: 'slash-in-segment', segments: ['whitelist', 'a/b'] },
  { name: 'empty-list', segments: [] },
  { name: 'too-many-segments', segments: ['a', 'b', 'c', 'd', 'e', 'f', 'g'] },
].map(({ name, segments }) => ({
  name,
  segments,
  expected: { canonical: routeNoToken.canonicalSegments(segments) },
}));

// 4. The backend one-origin CORS gate. Exactly one origin is echoed, and it
//    is echoed only when the request's Origin matches it exactly
//    (backend-v2.js:7559-7568). OPTIONS is answered 204 and does not reach
//    the router.
const corsCases = [
  { name: 'allowed-origin', origin: ALLOWED_ORIGIN, method: 'GET' },
  { name: 'foreign-origin', origin: 'https://evil.example.com', method: 'GET' },
  { name: 'absent-origin', origin: null, method: 'GET' },
  { name: 'preflight-from-foreign-origin', origin: 'https://evil.example.com', method: 'OPTIONS' },
].map(({ name, origin, method }) => {
  const headers = {};
  let status = null;
  const res = {
    setHeader: (key, value) => { headers[key] = value; },
    status: (code) => { status = code; return res; },
    end: () => {},
  };
  const req = { method, headers: origin === null ? {} : { origin } };
  let passedThrough = false;
  backend.CORS_MIDDLEWARE(req, res, () => { passedThrough = true; });
  return {
    name,
    origin,
    method,
    expected: {
      accessControlAllowOrigin: headers['Access-Control-Allow-Origin'] ?? null,
      vary: headers.Vary ?? null,
      allowMethods: headers['Access-Control-Allow-Methods'] ?? null,
      allowHeaders: headers['Access-Control-Allow-Headers'] ?? null,
      answered204: status === 204,
      passedThrough,
    },
  };
});

// 5. The proxy's caller gate. With a console token set it is required; without
//    one, only an obviously-forwarded request is refused (route.js:342-363).
const callerCases = [
  { name: 'token-set-correct', token: true, headers: { authorization: `Bearer ${CONSOLE_TOKEN_VALUE}` } },
  { name: 'token-set-absent', token: true, headers: {} },
  { name: 'token-set-wrong', token: true, headers: { authorization: 'Bearer wrong' } },
  { name: 'token-unset-loopback-xff', token: false, headers: { 'x-forwarded-for': '127.0.0.1' } },
  { name: 'token-unset-foreign-xff', token: false, headers: { 'x-forwarded-for': '10.0.0.9' } },
  { name: 'token-unset-no-xff', token: false, headers: {} },
].map(({ name, token, headers }) => {
  const internals = token ? routeWithToken : routeNoToken;
  return {
    name,
    consoleTokenConfigured: token,
    forwardedFor: headers['x-forwarded-for'] ?? null,
    authorization: headers.authorization ?? null,
    expected: { allowed: internals.callerAllowed(request('GET', url('agents'), headers)) },
  };
});

// ── The named divergences (native is stricter; cited, not executed) ───────
const divergences = [
  {
    rule: 'host-header',
    retained: 'Never checks Host; a Host check was rejected as theatre and replaced by the loopback bind (route.js:321-325).',
    native: 'Requires exactly one host header equal to the bound authority; refuses forwarded/x-forwarded-* and any authorization header.',
    citation: 'native/hagency/src/console.rs:98-109 (common_authority)',
  },
  {
    rule: 'sec-fetch-site-scope',
    retained: 'Checked only for mutations (route.js:423) and `none` is accepted beside `same-origin` (route.js:381).',
    native: 'Required exactly once and equal to `same-origin` on every console request, reads included.',
    citation: 'native/hagency/src/console.rs:110-123 (same_origin)',
  },
  {
    rule: 'origin-absent',
    retained: 'A mutation with neither sec-fetch-site nor origin is ALLOWED — "not from a browser" (route.js:374-383).',
    native: 'A mutation must carry origin equal to `http://<authority>`; absence is refused.',
    citation: 'native/hagency/src/console.rs:119-122',
  },
];

const output = JSON.stringify({
  source: 'mockup/app/api/hagency/[...path]/route.js + backend-v2.js',
  routeFile: ROUTE_FILE,
  backendFile: BACKEND_FILE,
  routeSha256,
  backendSha256,
  semantics: 'default-deny allowlist; canonical-before-match; sec-fetch-site wins over origin; one-origin CORS; loopback bind is the caller control',
  routeReadEntries: routeNoToken.READS.length,
  routeWriteEntries: routeNoToken.WRITES.length,
  allowedOrigin: backend.CORS_ALLOWED_ORIGIN,
  loopbackIps: [...backend.LOCALHOST_IPS],
  sameOrigin: sameOriginCases,
  allowlist: allowlistCases,
  canonicalisation: canonicalisationCases,
  cors: corsCases,
  caller: callerCases,
  divergences,
}, null, 2) + '\n';

const target = path.join(ROOT, 'native/hagency/tests/fixtures/console-origin-vectors.json');
if (process.argv.includes('--check')) {
  const current = readFileSync(target, 'utf8').replaceAll('\r\n', '\n');
  if (current !== output) throw new Error('Console origin vectors differ from the retained proxy/backend');
} else {
  writeFileSync(target, output);
}
console.log(JSON.stringify({
  sameOrigin: sameOriginCases.length,
  allowlist: allowlistCases.length,
  canonicalisation: canonicalisationCases.length,
  cors: corsCases.length,
  caller: callerCases.length,
}));
