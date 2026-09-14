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
  'native/hagency-platform/src/bin/hagency-platform-probe.rs',
  'native/hagency-platform/src/bin/hagency-cgroup-probe.rs',
  'native/hagency-progress-runtime/src/bin/hagency-progress-probe.rs',
  'native/hagency-runtime/src/bin/hagency-runtime-probe.rs',
]);
const STRIP_PREFIXES = ['native/hagency-runtime/src/bin/approval_probe/'];
// hagency/tests/** is covered by the */tests/* rule (the [[bin]] fixture peers
// owned/file/receive/approval_mcp_peer.rs and matrix_crypto_peer.rs live there).

export function isStripped(rel) {
  if (STRIP_FILES.has(rel)) return true;
  if (STRIP_PREFIXES.some((p) => rel.startsWith(p))) return true;
  if (STRIP_DIRS.some((d) => rel.startsWith(d + '/'))) return true;
  if (/(^|\/)tests?\//.test(rel)) return true;
  if (/(^|\/)examples\//.test(rel)) return true; // e.g. hagency-execution/examples/codex_qualify.rs
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
    // Compare on a comment-stripped window: a `// ... ;` inside a long
    // signature must not read as a declaration terminator.
    let i = m.index;
    const window = source.slice(i, i + 4000).replace(/\/\/[^\n]*/g, '');
    const semi = window.indexOf(';');
    const brace = window.indexOf('{');
    if (brace === -1 || (semi !== -1 && semi < brace)) { fns.push({ name, body: '' }); continue; }
    const braceAbs = i + brace;
    let depth = 0, end = braceAbs;
    for (; end < source.length; end++) {
      if (source[end] === '{') depth++;
      else if (source[end] === '}') { depth--; if (depth === 0) { end++; break; } }
    }
    fns.push({ name, body: source.slice(braceAbs, end), index: m.index });
  }
  return fns;
}

export function extractCalls(body) {
  const calls = new Set();
  // path-qualified: a::b::name(  -> full "a::b::name" ONLY. Registering the
  // short tail as well would make `palpo::Owner::start(` collide with every
  // bare `start` definition — the path is exactly what the author named.
  const qre = /([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)\s*\(/g;
  let m;
  while ((m = qre.exec(body))) {
    const full = m[1];
    const parts = full.split('::');
    const short = parts[parts.length - 1];
    if (!SKIP_WORDS.has(short)) calls.add(full);
  }
  // method or bare: .name( or name(. Method calls are recorded with a
  // `receiver.` prefix so resolution can (a) restrict to method definitions
  // (impls non-empty) and (b) prefer the impl matching a receiver type hint.
  const mre = /([a-z_][A-Za-z0-9_]*)\s*\.\s*([a-z_][A-Za-z0-9_]*)\s*\(/g;
  while ((m = mre.exec(body))) {
    if (!SKIP_WORDS.has(m[2])) calls.add(`${m[1]}.${m[2]}`);
  }
  const bre = /(?:^|[^.:\w])([a-z_][A-Za-z0-9_]*)\s*\(/g;
  while ((m = bre.exec(body))) {
    const name = m[1];
    if (!SKIP_WORDS.has(name)) calls.add(name);
  }
  // Handler/edge references passed as bare identifiers:
  // .get(handler) .post(..) .put(..) .delete(..) .push(..) .hoop(..) and
  // tokio::spawn(worker) — Salvo route registration and worker spawns pass the
  // fn by name, invisible to the call extraction above.
  const hre = /\.(?:get|post|put|delete|push|hoop)\s*\(\s*([a-z_][A-Za-z0-9_]*)/g;
  while ((m = hre.exec(body))) calls.add(m[1]);
  const sre = /\bspawn\s*\(\s*([a-z_][A-Za-z0-9_]*)/g;
  while ((m = sre.exec(body))) calls.add(m[1]);
  return [...calls];
}

function listRustFiles() {
  const out = execFileSync('git', ['-C', repoRoot, 'ls-files', 'native'], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  return out.split('\n').filter((f) => f.endsWith('.rs') && !isStripped(f));
}

export function buildGraph(files, read) {
  // fns: name -> [{ file, body, calls, impls }]; a name with entries in
  // several files is a collision. fileFns: file -> defs (defs carry impls).
  const fns = new Map();
  const fileFns = new Map();
  for (const rel of files) {
    const src = stripTestItems(read(rel));
    const defs = extractFns(src);
    for (const def of defs) { def.impls = implsOf(src, def); def.types = typeHints(src); }
    fileFns.set(rel, defs);
    for (const def of defs) {
      if (!fns.has(def.name)) fns.set(def.name, []);
      fns.get(def.name).push({ file: rel, body: def.body, calls: null, impls: def.impls });
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

// The impl type names whose blocks contain the fn's definition, e.g.
// ['DomainRepository'] for a method inside `impl DomainRepository { ... }`.
function implsOf(src, def) {
  const idx = def.index ?? src.indexOf(def.body);
  const impls = [];
  const re = /\bimpl\b[^{]*\b([A-Z][A-Za-z0-9_]*)[^{]*\{/g;
  let m;
  while ((m = re.exec(src))) {
    let depth = 0, end = src.indexOf('{', m.index);
    for (let i = end; i < src.length; i++) {
      if (src[i] === '{') depth++;
      else if (src[i] === '}') { depth--; if (depth === 0) { end = i; break; } }
    }
    if (idx > m.index && idx < end) impls.push(m[1]);
  }
  return impls;
}

// Simple receiver-type hints: `name: &Type`, `name: Type` in fn signatures
// and let-bindings, and struct fields. Maps variable -> type name.
export function typeHints(src) {
  const hints = new Map();
  for (const m of src.matchAll(/([a-z_][A-Za-z0-9_]*)\s*:\s*&?\s*(?:'\w+\s+)?(?:mut\s+)?([A-Z][A-Za-z0-9_]*)/g)) {
    if (!hints.has(m[1])) hints.set(m[1], m[2]);
  }
  return hints;
}

// crate::module[::Type]::fn -> the single definition it names, or null.
// hagency::bootstrap::accounts::run -> native/hagency/src/bootstrap/accounts.rs
// hagency_store::domain::approve -> native/hagency-store/src/domain.rs
// A Type segment (capitalised) is consumed as an impl filter, not a path part.
export function resolvePath(graph, files, full) {
  const parts = full.split('::');
  if (parts.length < 2) return { def: null, reason: 'not a qualified path' };
  const fnName = parts[parts.length - 1];
  let segs = parts.slice(0, -1);
  let typeName = null;
  if (/^[A-Z]/.test(segs[segs.length - 1])) typeName = segs.pop();
  if (!segs.length) return { def: null, reason: `not a module path (${full})` }; // e.g. Duration::from_millis
  const modPath = segs.slice(1).join('/');
  // Crate dir spellings: hagency -> native/hagency, hagency_store -> native/hagency-store.
  const crateDirs = [...new Set([segs[0], segs[0].replace(/_/g, '-')])].map((d) => `native/${d}/src`);
  const candidates = crateDirs.flatMap((crateDir) => [
    modPath ? `${crateDir}/${modPath}.rs` : `${crateDir}/lib.rs`,
    modPath ? `${crateDir}/${modPath}/mod.rs` : `${crateDir}/main.rs`,
  ]);
  const file = candidates.find((c) => files.includes(c));
  if (!file) return { def: null, reason: `no module file for ${segs.join('::')}` };
  const defs = (graph.fileFns.get(file) || []).filter((d) => d.name === fnName);
  const inImpl = typeName ? defs.filter((d) => (d.impls || []).includes(typeName)) : defs;
  const chosen = typeName ? inImpl : defs;
  if (chosen.length === 1) return { def: { file, name: fnName }, reason: null };
  if (chosen.length === 0) return { def: null, reason: `no fn ${fnName}${typeName ? ` in impl ${typeName}` : ''} in ${file}` };
  return { def: null, reason: `${chosen.length} definitions of ${fnName} in ${file}` };
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
  // Only rows of the gap table count: `| ... | ... | gap Gn ... |` lines.
  // Whole-document scanning would admit ids mentioned in prose or other tables.
  const gaps = new Set();
  for (const line of content.split('\n')) {
    if (!line.trim().startsWith('|')) continue;
    // `gap Gn` optionally followed by slash-listed sharers: `gap G2/G5 (shared)`.
    for (const m of line.matchAll(/\bgap\s+((?:G\d+)(?:\/G\d+)*)\b/gi)) {
      for (const id of m[1].toUpperCase().split('/')) gaps.add(id);
    }
  }
  return gaps;
}

export function resolveReachable(graph, roots) {
  // BFS from roots tracking, per reached definition, whether ANY chain from a
  // root reaches it through unambiguous edges only. A call resolves to
  // (a) a same-file definition, (b) a module::name qualified definition,
  // (c) otherwise ALL same-named definitions — the edge is ambiguous iff the
  // call binds more than one definition. Ambiguous edges propagate: a node
  // reached only through them is `ambiguous`, never `wired`.
  const best = new Map(); // key file::name -> 'clean' | 'tainted'
  const parents = new Map(); // key -> { from, via } of the first tainting edge
  const queue = [];
  const seed = (file, name, taint, from, via) => {
    const key = `${file}::${name}`;
    const cur = best.get(key);
    if (cur === 'clean' || (cur === 'tainted' && taint)) return;
    if (cur === 'tainted' && !taint) best.set(key, 'clean'); // upgrade
    else if (!cur) best.set(key, taint ? 'tainted' : 'clean');
    if (taint && !parents.has(key)) parents.set(key, { from, via });
    queue.push({ file, name, taint });
  };
  for (const r of roots) seed(r.file, r.name, false, null, null);
  const keyOf = (file, name) => `${file}::${name}`;
  while (queue.length) {
    const { file, name, taint } = queue.shift();
    const entry = (graph.fns.get(name) || []).find((e) => e.file === file);
    if (!entry || !entry.calls) continue;
    const def = (graph.fileFns.get(file) || []).find((d) => d.name === name);
    const types = def?.types || new Map();
    const local = new Set((graph.fileFns.get(file) || []).map((d) => d.name));
    for (const call of entry.calls) {
      const via = `${file}::${name} -> ${call}`;
      // Method call recorded as `receiver.name(`.
      const mm = call.match(/^([a-z_][A-Za-z0-9_]*)\.([a-z_][A-Za-z0-9_]*)$/);
      if (mm) {
        const [, recv, meth] = mm;
        let candidates = (graph.fns.get(meth) || []).filter((c) => (c.impls || []).length > 0);
        // (i) exactly one method definition -> clean. (ii) receiver type hint
        // picks the impl. (iii) otherwise ambiguous over all methods.
        const hint = types.get(recv);
        if (candidates.length > 1 && hint) {
          const typed = candidates.filter((c) => c.impls.includes(hint));
          if (typed.length === 1) candidates = typed;
        }
        if (candidates.length === 0) continue; // no method definition: no edge
        const ambiguousEdge = candidates.length > 1;
        for (const c of candidates) seed(c.file, meth, taint || ambiguousEdge, keyOf(file, name), via);
        continue;
      }
      const parts = call.split('::');
      const short = parts[parts.length - 1];
      // Unqualified calls bind the same-file definition first; qualified
      // calls resolve their path (a same-file short-name coincidence must
      // not swallow `bootstrap::accounts::run` just because main.rs also
      // defines a `run`).
      if (parts.length === 1 && local.has(short)) { seed(file, short, taint, keyOf(file, name), via); continue; }
      let targets = [];
      let ambiguousEdge = false;
      if (parts.length > 1) {
        // Qualified: resolve the module path to its definition. In-body paths
        // are often crate-relative (`bootstrap::accounts::run`,
        // `crate::x::y`) or module-relative (`driver::Driver::start` from
        // bootstrap.rs), so retry with the caller's crate, with `crate`
        // swapped for it, and relative to the caller's own module directory.
        const filesList = [...graph.fileFns.keys()];
        const callerCrate = file.match(/^native\/([^/]+)\/src\//)?.[1].replace(/-/g, '_');
        const callerModDir = file.replace(/^native\/[^/]+\/src\//, '').replace(/\.rs$/, '');
        const attempts = [call];
        if (callerCrate) {
          if (parts[0] === 'crate') attempts.push([callerCrate, ...parts.slice(1)].join('::'));
          else {
            attempts.push(`${callerCrate}::${call}`);
            const modParts = callerModDir.split('/');
            if (modParts.length && modParts[0]) attempts.push(`${callerCrate}::${modParts.join('::')}::${call}`);
          }
        }
        let r = { def: null };
        for (const attempt of attempts) {
          r = resolvePath(graph, filesList, attempt);
          if (r.def) break;
        }
        if (r.def) targets = [r.def];
        else targets = []; // unresolvable qualified path: no edge, not ambiguous
      } else {
        const candidates = graph.fns.get(short) || [];
        targets = candidates.map((c) => ({ file: c.file, name: short }));
        ambiguousEdge = candidates.length > 1;
      }
      for (const t of targets) seed(t.file, t.name, taint || ambiguousEdge, keyOf(file, name), via);
    }
  }
  return { best, parents };
}

// True when `def` is reachable from a root through unambiguous edges only.
export function isCleanlyReachable(reach, def) {
  return reach.best.get(`${def.file}::${def.name}`) === 'clean';
}

// The chain by which a tainted node was reached (for diagnostics).
export function taintChain(reach, def) {
  const chain = [];
  let key = `${def.file}::${def.name}`;
  let guard = 0;
  while (reach.parents.has(key) && guard++ < 50) {
    const p = reach.parents.get(key);
    chain.push(p.via);
    key = p.from;
  }
  return chain;
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
  const reach = resolveReachable(graph, roots);

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
  const ambiguous = [];
  const unresolved = [];
  const unknownGaps = [];
  for (const item of lines) {
    const ref = { file: item.file, line: item.line, caller: item.raw };
    if (item.parsed.kind === 'owed') {
      if (adrGaps.has(item.parsed.gap)) owed.push({ ...ref, gap: item.parsed.gap });
      else unknownGaps.push({ ...ref, gap: item.parsed.gap });
      continue;
    }
    // Exact full-path resolution only — never a bare-name search.
    const r = resolvePath(graph, files, item.parsed.full);
    if (!r.def) { unresolved.push({ ...ref, reason: r.reason }); continue; }
    if (isCleanlyReachable(reach, r.def)) { wired.push(ref); continue; }
    const key = `${r.def.file}::${r.def.name}`;
    if (reach.best.has(key)) ambiguous.push({ ...ref, definition: key, chain: taintChain(reach, r.def) });
    else missing.push({ ...ref, definition: key });
  }
  const result = {
    count: lines.length,
    wired: wired.length,
    owed: owed.map((o) => `${o.file}:${o.line} ${o.gap}`),
    missing,
    ambiguous,
    unresolved,
    unknownGaps,
    roots: roots.length,
    stripped: 'cfg(test) items, #[test] fns, */tests/*, */examples/*, tests.rs, native/fixtures/**, probe/fixture bins',
  };
  const failures = missing.length + ambiguous.length + unresolved.length + unknownGaps.length;
  return { result, ok: failures === 0 };
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
