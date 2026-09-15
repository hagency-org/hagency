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
    if (brace === -1 || (semi !== -1 && semi < brace)) { fns.push({ name, body: '', sig: '' }); continue; }
    const braceAbs = i + brace;
    let depth = 0, end = braceAbs;
    for (; end < source.length; end++) {
      if (source[end] === '{') depth++;
      else if (source[end] === '}') { depth--; if (depth === 0) { end++; break; } }
    }
    fns.push({ name, body: source.slice(braceAbs, end), index: m.index, sig: source.slice(m.index, braceAbs) });
  }
  return fns;
}

// Remove `//` line comments and `/* */` block comments while preserving
// string and char literals (so `"//"` inside a literal is not eaten and a
// literal's contents survive). Handles escapes inside literals.
export function stripComments(src) {
  let out = '';
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    const two = src.slice(i, i + 2);
    if (two === '//') {
      const nl = src.indexOf('\n', i);
      i = nl === -1 ? n : nl;
      continue;
    }
    if (two === '/*') {
      let depth = 1;
      i += 2;
      while (i < n && depth > 0) {
        if (src.slice(i, i + 2) === '/*') { depth++; i += 2; }
        else if (src.slice(i, i + 2) === '*/') { depth--; i += 2; }
        else i++;
      }
      continue;
    }
    if (c === '"' || c === "'") {
      // char vs lifetime: `'a'` literal vs `'a` lifetime — treat as literal
      // only when it closes within a short span and the next char is `'`.
      const quote = c;
      let j = i + 1;
      let lit = c;
      let closed = false;
      while (j < n) {
        if (src[j] === '\\') { lit += src.slice(j, j + 2); j += 2; continue; }
        if (src[j] === '\n') break;
        lit += src[j];
        if (src[j] === quote) { closed = true; j++; break; }
        j++;
      }
      if (quote === "'" && !closed) { out += c; i++; continue; } // lifetime
      out += lit;
      i = j;
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

export function extractCalls(body) {
  body = stripComments(body);
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
  // A receiver that is itself a call chain is recorded via the clone-chain
  // rule below or as `?.name` (unknown receiver — never dropped).
  const mre = /([a-z_][A-Za-z0-9_]*)\s*\.\s*([a-z_][A-Za-z0-9_]*)\s*\(/g;
  while ((m = mre.exec(body))) {
    if (!SKIP_WORDS.has(m[2])) calls.add(`${m[1]}.${m[2]}`);
  }
  // Call-chain receivers: `self.app.clone().router()` — the receiver of the
  // outer method is the field `app` (clone preserves the type), recorded as
  // `app.router` so the receiver-type hint resolves it. A fully unknown
  // chain receiver falls back to `?.name` (ambiguous over method defs).
  const cre = /(?:self\s*\.\s*)?([a-z_][A-Za-z0-9_]*)\s*\.\s*clone\s*\(\s*\)\s*\.\s*([a-z_][A-Za-z0-9_]*)\s*\(/g;
  while ((m = cre.exec(body))) {
    if (!SKIP_WORDS.has(m[2])) calls.add(`${m[1]}.${m[2]}`);
  }
  const xre = /\)\s*\.\s*([a-z_][A-Za-z0-9_]*)\s*\(/g;
  while ((m = xre.exec(body))) {
    if (!SKIP_WORDS.has(m[1])) calls.add(`?.${m[1]}`);
  }
  const bre = /(?:^|[^.:\w])([a-z_][A-Za-z0-9_]*)\s*\(/g;
  while ((m = bre.exec(body))) {
    const name = m[1];
    if (!SKIP_WORDS.has(name)) calls.add(name);
  }
  // Handler/edge references passed as arguments — identifiers or qualified
  // call expressions: .get(handler), .delete(revoke_grant),
  // .push(approvals::router()), .hoop(guard), and tokio::spawn(worker).
  const hre = /\.(?:get|post|put|delete|push|hoop)\s*\(\s*([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)/g;
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
  // structFields: structName -> Map(field -> type), built across all files so
  // a `let x = self.field` hint works even when the struct is declared in
  // another file of the crate.
  const fns = new Map();
  const fileFns = new Map();
  const sources = new Map();
  const structFields = new Map();
  for (const rel of files) {
    const src = stripTestItems(read(rel));
    sources.set(rel, src);
    for (const [structName, fields] of structFieldTypes(src)) {
      const prev = structFields.get(structName) || new Map();
      for (const [k, v] of fields) prev.set(k, v);
      structFields.set(structName, prev);
    }
  }
  for (const rel of files) {
    const src = sources.get(rel);
    const defs = extractFns(src);
    for (const def of defs) { def.impls = implsOf(src, def); def.types = typeHints(def.sig || '', def.body, def.impls, structFields); }
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
  return { fns, fileFns, structFields };
}

// The impl type names whose blocks contain the fn's definition, e.g.
// ['DomainRepository'] for a method inside `impl DomainRepository { ... }`.
function implsOf(src, def) {
  const idx = def.index ?? src.indexOf(def.body);
  const impls = [];
  // Inherent `impl T {` / `impl<'a> T {` keys as [T]. A trait impl
  // `impl Trait for T {` keys as [Trait, T] so BOTH a line naming the trait
  // and a line naming the concrete type resolve (the receiver is a T, and
  // the trait's method is callable through it).
  const re = /\bimpl\b(?:\s*<[^>]*>)?([^\n{]*)\{/g;
  let m;
  while ((m = re.exec(src))) {
    let depth = 0, end = src.indexOf('{', m.index);
    for (let i = end; i < src.length; i++) {
      if (src[i] === '{') depth++;
      else if (src[i] === '}') { depth--; if (depth === 0) { end = i; break; } }
    }
    if (!(idx > m.index && idx < end)) continue;
    const header = m[1];
    const traitFor = header.match(/\b([A-Z][A-Za-z0-9_]*)\s+for\s+([A-Z][A-Za-z0-9_]*)/);
    if (traitFor) impls.push(traitFor[1], traitFor[2]);
    else {
      const t = header.match(/\b([A-Z][A-Za-z0-9_]*)/);
      if (t) impls.push(t[1]);
    }
  }
  return [...new Set(impls)];
}

// Receiver-type hints from the evidence Rust gives for free, per fn body:
// - fn parameter types:            `collector: &Collector`
// - let annotations:               `let x: Type = ...`
// - constructor results:           `let x = Type::new(..)` / `Type::start(..)`
// - Self inside an impl block:     `self.method(` -> the impl's type
// - field bindings:                `let x = self.field...` -> the field's
//   type in the impl's struct (structFields), wrappers unwrapped
// Maps variable -> type name; `self` maps to the enclosing impl type.
const WRAPPERS = /^(?:Arc|Box|Option|Vec|Rc|Mutex|RwLock|RefCell|Weak|Cell|OnceCell|LazyLock)$/;
function unwrapType(t) {
  let cur = t.trim();
  for (;;) {
    const m = cur.match(/^([A-Z][A-Za-z0-9_]*)\s*<\s*(.+)>$/);
    if (!m || !WRAPPERS.test(m[1])) return cur.replace(/[<>\s].*$/, '');
    cur = m[2].split(',')[0].trim().replace(/^&(?:'\w+\s+)?(?:mut\s+)?/, '');
  }
}

// structName -> Map(field -> unwrapped inner type), from `struct T { ... }`
// blocks. Brace-matched; tuple structs and unit structs yield nothing.
export function structFieldTypes(src) {
  const out = new Map();
  const re = /\bstruct\s+([A-Z][A-Za-z0-9_]*)[^{;]*\{/g;
  let m;
  while ((m = re.exec(src))) {
    let depth = 0, end = src.indexOf('{', m.index);
    for (let i = end; i < src.length; i++) {
      if (src[i] === '{') depth++;
      else if (src[i] === '}') { depth--; if (depth === 0) { end = i; break; } }
    }
    const body = src.slice(src.indexOf('{', m.index), end);
    const fields = new Map();
    for (const f of body.matchAll(/\b([a-z_][A-Za-z0-9_]*)\s*:\s*&?\s*(?:'\w+\s+)?(?:mut\s+)?([A-Z][A-Za-z0-9_<>: ,&']+?)\s*[,}]/g)) {
      if (!fields.has(f[1])) fields.set(f[1], unwrapType(f[2]));
    }
    const prev = out.get(m[1]) || new Map();
    for (const [k, v] of fields) prev.set(k, v);
    out.set(m[1], prev);
  }
  return out;
}

export function typeHints(sig, body, impls, structFields) {
  // Scoped to ONE function: its signature and body only. Hints must never
  // leak across functions of a file (a param typed &A in fn1 must not type
  // fn2's receiver).
  const hints = new Map();
  const set = (k, v) => { if (k && v && !hints.has(k)) hints.set(k, v); };
  const text = `${sig}\n${stripComments(body || '')}`;
  // name: [&]['a] [mut] Type  (fn params in the signature, typed lets)
  for (const m of text.matchAll(/\b([a-z_][A-Za-z0-9_]*)\s*:\s*&\s*(?:'\w+\s+)?(?:mut\s+)?([A-Z][A-Za-z0-9_]*)/g)) set(m[1], m[2]);
  for (const m of text.matchAll(/\blet\s+(?:mut\s+)?([a-z_][A-Za-z0-9_]*)\s*:\s*([A-Z][A-Za-z0-9_]*)/g)) set(m[1], m[2]);
  // let x = Type::new/start/build/open/connect/bind( — constructor result
  for (const m of text.matchAll(/\blet\s+(?:mut\s+)?([a-z_][A-Za-z0-9_]*)\s*=\s*(?:[A-Za-z_][A-Za-z0-9_]*::)*([A-Z][A-Za-z0-9_]*)::(?:new|start|build|open|connect|bind)\s*\(/g)) set(m[1], m[2]);
  if (impls && impls.length) set('self', impls[0]);
  // Destructuring: let Type { a, b, .. } = expr; — the type name is explicit,
  // so field types come from structFields regardless of the source value.
  for (const m of text.matchAll(/\blet\s+([A-Z][A-Za-z0-9_]*)\s*\{([^}]*)\}\s*=/g)) {
    const fields = structFields && structFields.get(m[1]);
    if (!fields) continue;
    for (let fname of m[2].split(',')) {
      fname = fname.trim().replace(/^#\[[^\]]*\]\s*/g, '').replace(/^(?:ref\s+|mut\s+)/, '').split(':')[0].trim();
      if (/^[a-z_][A-Za-z0-9_]*$/.test(fname)) set(fname, fields.get(fname));
    }
  }
  // let x = self.field[.clone()|...] — the field's struct type (clone and
  // Arc derefs preserve it). The struct may be declared in another file.
  const own = structFields && impls && impls.length ? structFields.get(impls[0]) : null;
  if (own) {
    for (const m of text.matchAll(/\blet\s+(?:mut\s+)?([a-z_][A-Za-z0-9_]*)\s*=\s*self\.([a-z_][A-Za-z0-9_]*)/g)) set(m[1], own.get(m[2]));
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
  if (!typeName) {
    // crate::module::fn — a Type-less path. The FREE function wins when both
    // a free fn and an impl method share the name (shape decides). With no
    // free fn, a single impl method of that name resolves (spec lines name
    // impl methods without the Type segment too — e.g.
    // hagency::bootstrap::open_with_options is Bootstrap::open_with_options);
    // more than one impl providing it is unresolved.
    const free = defs.filter((d) => !(d.impls || []).length);
    if (free.length === 1) return { def: { file, name: fnName, impls: [] }, reason: null };
    if (free.length > 1) return { def: null, reason: `${free.length} free definitions of ${fnName} in ${file}` };
    const methods = defs.filter((d) => (d.impls || []).length);
    if (methods.length === 1) return { def: { file, name: fnName, impls: methods[0].impls }, reason: null };
    if (methods.length === 0) return { def: null, reason: `no fn ${fnName} in ${file}` };
    const keys = methods.map((d) => `${(d.impls || []).join('&')}::${fnName}`);
    return { def: null, reason: `no free fn ${fnName} in ${file} and the method is provided by more than one impl: ${keys.join(', ')}` };
  }
  // crate::module::Type::fn — inherent impl of Type first, then trait impls
  // FOR Type (implsOf records both names). A trait-name path resolves the
  // same way since the trait is in the impls list.
  const withType = defs.filter((d) => (d.impls || []).includes(typeName));
  if (withType.length === 1) return { def: { file, name: fnName, impls: withType[0].impls }, reason: null };
  if (withType.length === 0) return { def: null, reason: `no fn ${fnName} in impl ${typeName} in ${file}` };
  // Two impls both reachable through `Type::fn` — e.g. the method comes from
  // two different traits implemented for Type. Genuinely ambiguous.
  const keys = withType.map((d) => `${(d.impls || []).join('&')}::${fnName}`);
  return { def: null, reason: `${fnName} for ${typeName} is provided by more than one impl: ${keys.join(', ')}` };
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
  // Node identity is (file, name, impls): a method inside `impl T` is a
  // different node from a same-named free fn or a method of another impl in
  // the same file, so a verdict never names the wrong function.
  const best = new Map(); // key file::Type::name (or file::name) -> 'clean' | 'tainted'
  const parents = new Map(); // key -> { from, via } of the first tainting edge
  const queue = [];
  const norm = (impls) => [...(impls || [])].sort().join('&');
  const keyOf = (file, name, impls) => (norm(impls) ? `${file}::${norm(impls)}::${name}` : `${file}::${name}`);
  const seed = (file, name, impls, taint, from, via) => {
    const key = keyOf(file, name, impls);
    const cur = best.get(key);
    if (cur === 'clean' || (cur === 'tainted' && taint)) return;
    if (cur === 'tainted' && !taint) best.set(key, 'clean'); // upgrade
    else if (!cur) best.set(key, taint ? 'tainted' : 'clean');
    if (taint && !parents.has(key)) parents.set(key, { from, via });
    queue.push({ file, name, impls, taint });
  };
  for (const r of roots) seed(r.file, r.name, r.impls || [], false, null, null);
  while (queue.length) {
    const { file, name, impls, taint } = queue.shift();
    // The node is one definition: (file, name, impls). Its body drives the
    // calls; its own hints drive receiver typing.
    const def = (graph.fileFns.get(file) || []).find((d) => d.name === name && norm(d.impls) === norm(impls));
    if (!def) continue;
    const entry = (graph.fns.get(name) || []).find((e) => e.file === file && e.body === def.body);
    if (!entry || !entry.calls) continue;
    const types = def.types || new Map();
    const local = new Set((graph.fileFns.get(file) || []).filter((d) => !d.impls?.length).map((d) => d.name));
    for (const call of entry.calls) {
      const via = `${keyOf(file, name, impls)} -> ${call}`;
      // Method call recorded as `?.name(` — receiver is a call chain with an
      // unknown type: resolve over same-named METHOD definitions, ambiguous
      // when several, never dropped.
      const um = call.match(/^\?\.([a-z_][A-Za-z0-9_]*)$/);
      if (um) {
        const meth = um[1];
        const candidates = (graph.fns.get(meth) || []).filter((c) => (c.impls || []).length > 0);
        const ambiguousEdge = candidates.length > 1;
        for (const c of candidates) seed(c.file, meth, c.impls, taint || ambiguousEdge, keyOf(file, name, impls), via);
        continue;
      }
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
        for (const c of candidates) seed(c.file, meth, c.impls, taint || ambiguousEdge, keyOf(file, name, impls), via);
        continue;
      }
      const parts = call.split('::');
      const short = parts[parts.length - 1];
      // Unqualified calls bind the same-file FREE definition first; qualified
      // calls resolve their path (a same-file short-name coincidence must
      // not swallow `bootstrap::accounts::run` just because main.rs also
      // defines a `run`).
      if (parts.length === 1 && local.has(short)) { seed(file, short, [], taint, keyOf(file, name, impls), via); continue; }
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
        // Module of the caller: `bootstrap/driver.rs` lives in module
        // `bootstrap` (the stem is the module file, not a submodule); only
        // mod.rs/lib.rs/main.rs keep their directory as the module path.
        let callerModDir = file.replace(/^native\/[^/]+\/src\//, '').replace(/\.rs$/, '');
        if (!/\/(mod|lib|main)$/.test(callerModDir)) callerModDir = callerModDir.replace(/\/[^/]+$/, '');
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
        targets = candidates.map((c) => ({ file: c.file, name: short, impls: c.impls }));
        ambiguousEdge = candidates.length > 1;
      }
      for (const t of targets) seed(t.file, t.name, t.impls || [], taint || ambiguousEdge, keyOf(file, name, impls), via);
    }
  }
  return { best, parents };
}

const keyFor = (def) => {
  const norm = [...(def.impls || [])].sort().join('&');
  return norm ? `${def.file}::${norm}::${def.name}` : `${def.file}::${def.name}`;
};

// True when `def` is reachable from a root through unambiguous edges only.
export function isCleanlyReachable(reach, def) {
  return reach.best.get(keyFor(def)) === 'clean';
}

// The chain by which a tainted node was reached (for diagnostics).
export function taintChain(reach, def) {
  const chain = [];
  let key = keyFor(def);
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
    const key = keyFor(r.def);
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

// `--explain crate::path[::Type]::fn`: print, for the target definition, the
// nearest reached definition and the call sites between it and the target
// that the graph did NOT resolve into an edge — or "no reached caller of any
// name" when nothing sharing the short name is reached at all.
export function explain(full) {
  const files = listRustFiles();
  const graph = buildGraph(files, (rel) => readFileSync(path.join(repoRoot, rel), 'utf8'));
  const main = 'native/hagency/src/main.rs';
  const roots = files.includes(main) ? (graph.fileFns.get(main) || []).map((d) => ({ file: main, name: d.name })) : [];
  const reach = resolveReachable(graph, roots);
  const r = resolvePath(graph, files, full);
  if (!r.def) return { target: full, resolved: false, reason: r.reason };
  const key = keyFor(r.def);
  const state = reach.best.get(key) || 'unreached';
  const short = r.def.name;
  // Candidate callers: every definition whose extracted calls mention the
  // short name (as bare, method, qualified, or handler argument).
  const candidates = [];
  for (const [file, defs] of graph.fileFns) {
    for (const d of defs) {
      const entry = (graph.fns.get(d.name) || []).find((e) => e.file === file && e.body === d.body);
      if (!entry?.calls) continue;
      const mentions = entry.calls.filter((c) => c === short || c.endsWith(`::${short}`) || c.endsWith(`.${short}`));
      if (mentions.length) candidates.push({ at: keyFor({ file, name: d.name, impls: d.impls }), via: mentions, state: reach.best.get(keyFor({ file, name: d.name, impls: d.impls })) || 'unreached' });
    }
  }
  const reached = candidates.filter((c) => c.state === 'clean' || c.state === 'tainted');
  const nearest = reached[0] || null;
  return {
    target: full,
    definition: key,
    state,
    taintChain: state === 'tainted' ? taintChain(reach, r.def) : undefined,
    nearestReachedCaller: nearest,
    unresolvedCallSites: reached.slice(1).concat(candidates.filter((c) => c.state === 'unreached')).slice(0, 12),
    note: nearest ? undefined : 'no reached caller of any name',
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const explainIdx = process.argv.indexOf('--explain');
  if (explainIdx !== -1) {
    console.log(JSON.stringify(explain(process.argv[explainIdx + 1]), null, 2));
  } else {
    const { result, ok } = checkProductionCallers();
    console.log(JSON.stringify(result, null, 2));
    if (!result.count || !ok) process.exitCode = 1;
  }
}
