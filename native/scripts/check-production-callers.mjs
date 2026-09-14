#!/usr/bin/env node
// ADR-146 (a): a spec `Production caller:` line must resolve in the production
// call graph — tests (`#[cfg(test)]` items, `#[test]` fns, */tests/*,
// tests.rs), native/fixtures/**, and the probe/fixture binaries stripped.
// Roots are the product binaries and services only: the `hagency` bin
// (hagency/src/main.rs), its Salvo handler registrations, the workers and
// sweeps it spawns, and the MCP stdio entry (mcp/stdio.rs via main.rs).
// `Production caller: owed (Gn)` is a tracked gap: the Gn must name a row in
// ADR-146's gap table; an unknown id fails. Exit 1 lists absent callers and
// unknown gap ids.
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

const STRIP_DIRS = ['native/fixtures'];
const STRIP_FILES = new Set([
  'native/hagency/src/bootstrap/driver.rs', // bootstrap probe
  'native/hagency-platform/src/bin/hagency-platform-probe.rs',
  'native/hagency-platform/src/bin/hagency-cgroup-probe.rs',
  'native/hagency-progress-runtime/src/bin/hagency-progress-probe.rs',
  'native/hagency-runtime/src/bin/hagency-runtime-probe.rs',
]);
const STRIP_PREFIXES = ['native/hagency-runtime/src/bin/approval_probe/'];
// hagency/tests/** is covered by the */tests/* rule (the [[bin]] fixture peers
// owned/file/receive/approval_mcp_peer.rs and matrix_crypto_peer.rs live there).

function isStripped(rel) {
  if (STRIP_FILES.has(rel)) return true;
  if (STRIP_PREFIXES.some((p) => rel.startsWith(p))) return true;
  if (STRIP_DIRS.some((d) => rel.startsWith(d + '/'))) return true;
  if (/(^|\/)tests?\//.test(rel)) return true;
  if (rel.endsWith('tests.rs')) return true;
  return false;
}

// Remove `#[cfg(test)]` items and `#[test]` fns by brace matching from the
// attribute line. Scanner-based; string/comment contents with unbalanced
// braces would mislead it — acceptable per the ADR's "simple name-based graph".
export function stripTestItems(source) {
  const lines = source.split('\n');
  const out = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const isCfgTest = /#\[cfg\(test\)\]/.test(line);
    const isTestAttr = /#\[(tokio::)?test\b/.test(line);
    if (!isCfgTest && !isTestAttr) { out.push(line); continue; }
    // Find the next item-ish line; skip attribute lines and blank lines.
    let j = i + 1;
    while (j < lines.length && (/^\s*#\[/.test(lines[j]) || /^\s*$/.test(lines[j]))) j++;
    if (j >= lines.length) { i = j - 1; continue; }
    const itemLine = lines[j];
    // #[cfg(test)] only strips items (mod/fn/impl/static/const/use blocks), and
    // only when the attributed item has a brace body. A bare `#[test]` also
    // implies a fn with a body.
    const bodyStart = sourceOffsetOf(out, itemLine, lines, j);
    void bodyStart;
    if (!/\{/.test(itemLine) && !isTestAttr) {
      // e.g. `#[cfg(test)] use ...;` — a single-line item.
      i = j; // skip the item line itself
      continue;
    }
    // Brace-match from the first '{' at or after the item line.
    let depth = 0, started = false, k = j;
    for (; k < lines.length; k++) {
      for (const ch of lines[k]) {
        if (ch === '{') { depth++; started = true; }
        else if (ch === '}') { depth--; }
      }
      if (started && depth === 0) break;
    }
    i = k; // consumed through the closing brace
  }
  return out.join('\n');
}
function sourceOffsetOf() { return 0; }

const SKIP_WORDS = new Set(['if', 'for', 'while', 'loop', 'match', 'impl', 'trait', 'struct', 'enum', 'mod', 'type', 'const', 'static', 'let', 'return', 'pub', 'async', 'unsafe', 'extern', 'where', 'use', 'else', 'in', 'fn', 'ref', 'move', 'dyn', 'box']);

export function extractFns(source) {
  const fns = [];
  const re = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[<(]/g;
  let m;
  while ((m = re.exec(source))) {
    const name = m[1];
    if (SKIP_WORDS.has(name)) continue;
    // Body starts at the first '{' after the signature (skip where-clauses by
    // scanning forward; a ';' first means a trait declaration with no body).
    let i = m.index;
    let semi = source.indexOf(';', i);
    let brace = source.indexOf('{', i);
    if (brace === -1 || (semi !== -1 && semi < brace)) { fns.push({ name, body: '' }); continue; }
    let depth = 0, end = brace;
    for (; end < source.length; end++) {
      if (source[end] === '{') depth++;
      else if (source[end] === '}') { depth--; if (depth === 0) { end++; break; } }
    }
    fns.push({ name, body: source.slice(brace, end) });
  }
  return fns;
}

export function extractCalls(body) {
  const calls = new Set();
  // path-qualified: a::b::name(  -> full "a::b::name" and short "name"
  const qre = /([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)\s*\(/g;
  let m;
  while ((m = qre.exec(body))) {
    const full = m[1];
    const parts = full.split('::');
    const short = parts[parts.length - 1];
    if (!SKIP_WORDS.has(short)) { calls.add(full); calls.add(short); }
  }
  // method or bare: .name( or name(
  const bre = /(?:\.|\b)([a-z_][A-Za-z0-9_]*)\s*\(/g;
  while ((m = bre.exec(body))) {
    const name = m[1];
    if (!SKIP_WORDS.has(name)) calls.add(name);
  }
  return [...calls];
}

function listRustFiles() {
  const out = execFileSync('git', ['-C', repoRoot, 'ls-files', 'native'], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  return out.split('\n').filter((f) => f.endsWith('.rs') && !isStripped(f));
}

export function buildGraph(files, read) {
  // fns: name -> [{ file, body, calls }]; multiple entries = ambiguous.
  const fns = new Map();
  const fileFns = new Map();
  for (const rel of files) {
    const src = stripTestItems(read(rel));
    const defs = extractFns(src);
    fileFns.set(rel, defs);
    for (const def of defs) {
      if (!fns.has(def.name)) fns.set(def.name, []);
      fns.get(def.name).push({ file: rel, body: def.body, calls: null });
    }
  }
  for (const defs of fileFns.values()) {
    for (const def of defs) {
      for (const entry of fns.get(def.name)) {
        if (entry.body === def.body) entry.calls = extractCalls(def.body);
      }
    }
  }
  return { fns, fileFns };
}

function parseCaller(raw) {
  const s = raw.trim();
  const owed = s.match(/^owed\s*\((G\d+)\)$/);
  if (owed) return { kind: 'owed', gap: owed[1] };
  const parts = s.split('::');
  return { kind: 'caller', full: s, name: parts[parts.length - 1] };
}

export function parseSpecLines(content, file) {
  const out = [];
  const lines = content.split('\n');
  for (const [index, line] of lines.entries()) {
    const m = line.match(/^\s*Production caller:\s*(\S.*?)\s*$/);
    if (m) out.push({ file, line: index + 1, raw: m[1], parsed: parseCaller(m[1]) });
  }
  return out;
}

export function parseAdrGaps(content) {
  const gaps = new Set();
  for (const m of content.matchAll(/\b(G\d+)\b/g)) gaps.add(m[1]);
  return gaps;
}

export function resolveReachable(graph, roots) {
  // BFS from roots. Same-file resolution first; then a unique global name;
  // ambiguous names expand to all candidates (conservative, name-based).
  const seen = new Set(); // keys: name or file::name
  const ambiguous = new Set();
  const queue = [];
  const seed = (file, name) => { const key = `${file}::${name}`; if (!seen.has(key)) { seen.add(key); queue.push({ file, name }); } };
  for (const r of roots) seed(r.file, r.name);
  while (queue.length) {
    const { file, name } = queue.shift();
    const entries = (graph.fns.get(name) || []).filter((e) => e.file === file);
    const entry = entries[0] || (graph.fns.get(name) || [])[0];
    if (!entry || !entry.calls) continue;
    const local = new Set((graph.fileFns.get(file) || []).map((d) => d.name));
    for (const call of entry.calls) {
      const parts = call.split('::');
      const short = parts[parts.length - 1];
      // Same-file resolution first; then a name defined in exactly one file;
      // only a name defined in several files is ambiguous (name-based graph,
      // so it conservatively expands to every candidate file).
      if (local.has(short)) { seed(file, short); continue; }
      const candidates = graph.fns.get(short) || [];
      const filesFor = [...new Set(candidates.map((c) => c.file))];
      if (filesFor.length === 1) seed(filesFor[0], short);
      else if (filesFor.length > 1) {
        ambiguous.add(short);
        for (const c of candidates) seed(c.file, short);
      }
    }
  }
  return { seen, ambiguous };
}

export function defaultRoots(files) {
  // main.rs is the `hagency` bin; every fn defined there is a root because the
  // clap dispatch names subcommand handlers (e.g. accounts::run at :204) and
  // the Salvo `Router` registrations, worker/sweep spawns and the MCP stdio
  // entry (mcp/stdio.rs via main.rs:1,152) are all called from main's flow.
  const graphFiles = files;
  const roots = [];
  const main = 'native/hagency/src/main.rs';
  if (graphFiles.includes(main)) {
    // Seeded by name list after the graph is built — see checkProductionCallers.
    roots.push({ file: main, name: 'main' });
  }
  return roots;
}

export function checkProductionCallers({ root = repoRoot, read, files: givenFiles } = {}) {
  const readFn = read || ((rel) => readFileSync(path.join(root, rel), 'utf8'));
  const files = givenFiles || listRustFilesAt(root);
  const graph = buildGraph(files, readFn);
  // Roots: main plus every fn defined in main.rs (clap subcommand handlers and
  // the serve paths are all selected from main's match).
  const main = 'native/hagency/src/main.rs';
  const roots = [];
  if (files.includes(main)) {
    for (const def of graph.fileFns.get(main) || []) roots.push({ file: main, name: def.name });
  }
  const { seen, ambiguous } = resolveReachable(graph, roots);

  const specsDir = path.join(root, 'specs');
  const lines = [];
  for (const name of readdirSync(specsDir).filter((n) => n.endsWith('.spec.md'))) {
    const content = readFileSync(path.join(specsDir, name), 'utf8');
    lines.push(...parseSpecLines(content, name));
  }
  const adrPath = 'knowledge/decisions/adr-146-production-callers-and-store-surface.md';
  const adrGaps = existsSync(path.join(root, adrPath)) ? parseAdrGaps(readFn(adrPath)) : new Set();

  const wired = [];
  const owed = [];
  const missing = [];
  const unknownGaps = [];
  for (const item of lines) {
    const ref = { file: item.file, line: item.line, caller: item.raw };
    if (item.parsed.kind === 'owed') {
      if (adrGaps.has(item.parsed.gap)) owed.push({ ...ref, gap: item.parsed.gap });
      else unknownGaps.push({ ...ref, gap: item.parsed.gap });
      continue;
    }
    const name = item.parsed.name;
    const reachable = [...seen].some((key) => key.endsWith(`::${name}`));
    if (reachable) wired.push(ref);
    else missing.push(ref);
  }
  const result = {
    count: lines.length,
    wired: wired.length,
    owed: owed.map((o) => `${o.file}:${o.line} ${o.gap}`),
    missing,
    unknownGaps,
    ambiguous: [...ambiguous].sort(),
    roots: roots.length,
    stripped: 'cfg(test) items, #[test] fns, */tests/*, tests.rs, native/fixtures/**, probe/fixture bins, bootstrap/driver.rs',
  };
  return { result, ok: missing.length === 0 && unknownGaps.length === 0 };
}

function listRustFilesAt(root) {
  if (root === repoRoot) return listRustFiles();
  const out = execFileSync('git', ['-C', root, 'ls-files', 'native'], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  return out.split('\n').filter((f) => f.endsWith('.rs') && !isStripped(f));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { result, ok } = checkProductionCallers();
  console.log(JSON.stringify(result, null, 2));
  if (!result.count || !ok) process.exitCode = 1;
}
