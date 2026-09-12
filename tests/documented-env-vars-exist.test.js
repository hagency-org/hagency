/*
 * AN ENV VAR TABLE THAT NAMES VARIABLES NOTHING READS.
 *
 * `docs/architecture/system-components.md` documented the bridge's configuration as
 * `MATRIX_HOMESERVER_URL`, `MATRIX_DOMAIN`, `MATRIX_BOT_USER` and `MATRIX_PUPPET_PREFIX`. The code reads
 * `MATRIX_HOMESERVER`, `MATRIX_SERVER_NAME`, `MATRIX_BOT_USERNAME` and `MATRIX_AGENT_PREFIX`. Four of five
 * rows in one table, plus a whole section for a `server.js` that does not exist in this repository.
 *
 * WHY THAT IS WORSE THAN A MISSING TABLE. Every one of those names fails SILENTLY. An operator who sets
 * `MATRIX_BOT_USER` gets the default `agent-bridge`, the bot cannot log in — and since #119 a bot that
 * cannot start no longer takes the fleet down with it, so the deployment keeps running and nothing ever
 * says the variable was misspelled. The same wrong name was in `docs/OPERATOR-WALKTHROUGH.md` and
 * `docs/RUNNING-THE-SERVICES.md`, which is how a walk nearly followed it.
 *
 * WHAT THIS CHECKS, AND WHAT IT DELIBERATELY DOES NOT. Only rows of the env-var tables — `| \`NAME\` |
 * default | purpose |` — and only that the name is read SOMEWHERE in this repository's own source. It does
 * not check defaults, does not read prose, and does not require a variable to be read where the table says
 * it is: that would be a second, weaker copy of the table itself.
 *
 * PASS-THROUGHS ARE NAMED, NOT ERASED. `ANTHROPIC_MODEL` and the `CLAUDE_CODE_*` pair are read by the
 * agent runtime this repository launches, never by this repository. They belong in an operator's reference
 * and would fail a naive "must appear in code" rule, so they are listed here with that reason attached —
 * which also makes the list the place where a reader learns the distinction exists.
 */
import { describe, expect, test } from 'vitest';
import { readFileSync, readdirSync, statSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'fs';
import { tmpdir } from 'os';
import path from 'path';

const DOC = path.resolve('docs/architecture/system-components.md');

/*
 * Variables this repository does NOT read, and should still document.
 *
 * Each one is somebody else's contract that an operator nonetheless sets on our behalf. Anything added
 * here needs the same one-line reason, because an unexplained allowlist is how a real omission gets
 * parked.
 */
const PASS_THROUGH = new Map([
  ['ANTHROPIC_MODEL', 'read by the Claude Code runtime we launch, not by us'],
  ['CLAUDE_CODE_MAX_OUTPUT_TOKENS', 'read by the Claude Code runtime we launch, not by us'],
  ['CLAUDE_CODE_AUTO_COMPACT_WINDOW', 'read by the Claude Code runtime we launch, not by us'],
  ['DISABLE_PROMPT_CACHING', 'read by the Claude Code runtime we launch, not by us'],
  ['SUPERVISOR_API_KEY', 'consumed by the supervisor evaluator’s own config, not read here'],
  ['SUPERVISOR_INTERVAL_MS', 'declared in .env.example; the evaluator reads its own config'],
  ['LETTA_BASE_URL', 'a Letta server address handed to the memory sidecar, not read here'],
  ['HAGENCY_SUBCONSCIOUS_EVENT_TOKEN', 'read by the subconscious hook in the agent home, not by this repo'],
]);

/** Every env-var table row: `| `NAME` | default | purpose |`. */
function documentedVars(markdown) {
  const names = new Set();
  for (const line of markdown.split('\n')) {
    const m = line.match(/^\|\s*`([A-Z][A-Z0-9_]{2,})`\s*\|/);
    if (m) names.add(m[1]);
  }
  return [...names];
}

/** Everything this repository could read an env var from, source and shell alike. */
function sourceFiles(dir = process.cwd(), acc = [], depth = 0) {
  if (depth > 4) return acc;
  // Cargo binaries have no extension on Unix; reading target/debug as source can
  // exceed V8's maximum string length and falsely attribute generated code to us.
  const skip = new Set(['node_modules', '.git', 'docs', 'knowledge', 'coverage', '.next', 'data', 'target']);
  for (const entry of readdirSync(dir)) {
    if (skip.has(entry) || entry.startsWith('.')) continue;
    const full = path.join(dir, entry);
    let stat;
    try { stat = statSync(full); } catch { continue; }
    if (stat.isDirectory()) { sourceFiles(full, acc, depth + 1); continue; }
    if (/\.(js|mjs|cjs|jsx|sh|json)$/.test(entry) || !path.extname(entry)) acc.push(full);
  }
  return acc;
}

test('documentation scans exclude Cargo artifacts and retain extensionless scripts', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'hagency-source-scan-'));
  try {
    mkdirSync(path.join(root, 'target', 'debug'), { recursive: true });
    mkdirSync(path.join(root, 'bin'));
    mkdirSync(path.join(root, 'lib'));
    writeFileSync(path.join(root, 'target', 'debug', 'hagency'), 'env.GENERATED_ONLY');
    writeFileSync(path.join(root, 'bin', 'hagency-task'), 'echo "$HAGENCY_TOKEN"');
    writeFileSync(path.join(root, 'lib', 'runtime.js'), 'env.MATRIX_HOMESERVER');
    expect(sourceFiles(root).map(file => path.relative(root, file)).sort()).toEqual([
      path.join('bin', 'hagency-task'), path.join('lib', 'runtime.js'),
    ]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

describe('the env vars the architecture reference names', () => {
  const markdown = readFileSync(DOC, 'utf8');
  const documented = documentedVars(markdown);
  const haystack = sourceFiles()
    .map((f) => { try { return readFileSync(f, 'utf8'); } catch { return ''; } })
    .join('\n');

  test('the tables are actually being read — the parse is not vacuous', () => {
    /*
     * The assertion that keeps the rest honest. A regex that matched nothing would make every check
     * below pass forever, which is the failure mode of every docs-as-truth test.
     */
    expect(documented.length).toBeGreaterThan(15);
    expect(documented).toContain('MATRIX_BOT_USERNAME');
    expect(documented).toContain('MATRIX_HOMESERVER');
  });

  test('every documented variable is read by this repository, or listed as a pass-through', () => {
    /*
     * WORD-BOUNDED, and the first version was not — which is how it hid two more wrong rows from me
     * within a minute of being written. `haystack.includes('BRIDGE_SECRET')` is true because the code
     * reads `MATRIX_BRIDGE_SECRET`, and `DATA_DIR` is inside `HAGENCY_DATA_DIR`: a substring match makes
     * exactly the prefix-dropping mistakes this table had made, invisible.
     */
    /*
     * AND IT HAS TO BE AN ENV READ, not any occurrence of the name. `DATA_DIR` passed a word-bounded
     * search because `bridge-matrix.js` has a local `const DATA_DIR` — a module's own variable is not a
     * knob an operator can turn, and counting it made the check agree with the table about a variable
     * that does not exist. So: `process.env.NAME` for JS, `$NAME`/`${NAME}` for the shell scripts, and a
     * bare `NAME=` line for `.env.example`, which is itself a declaration an operator reads.
     *
     * KNOWN LIMIT, stated rather than hidden: `$NAME` in a shell script cannot be distinguished from a
     * variable that script assigns itself. That is how `DATA_DIR` earned a pass — `bin/hagency-up` sets
     * and reads its own — so this rule catches a documented name nothing knows about, and not every
     * documented name that is really a local. The narrower half is still worth having.
     */
    const isRead = (name) => [
      // `env.NAME` as well as `process.env.NAME`: every adapter in lib/backend takes an injected `env`,
      // which is how `HAGENCY_AGENT_TOKEN_MODE` — read at lib/backend/auth-adapter.js:49 — was reported
      // missing by the first version of this rule.
      new RegExp(`\\benv\\.${name}(?![A-Z0-9_])`),
      new RegExp(`\\benv\\[['"\`]${name}['"\`]\\]`),
      new RegExp(`\\$\\{?${name}(?![A-Z0-9_])`),
      new RegExp(`^${name}=`, 'm'),
    ].some((re) => re.test(haystack));
    const missing = documented.filter((name) => !PASS_THROUGH.has(name) && !isRead(name));
    expect(missing, `documented but read nowhere: ${missing.join(', ')}`).toEqual([]);
  });

  test('and the four names that were WRONG stay gone', () => {
    // Named individually, because a regression here is silent by construction and this is the one
    // shape of it that has actually happened.
    for (const wrong of ['MATRIX_HOMESERVER_URL', 'MATRIX_DOMAIN', 'MATRIX_BOT_USER`', 'MATRIX_PUPPET_PREFIX']) {
      expect(markdown, `${wrong} is not a variable this code reads`).not.toContain(wrong);
    }
  });

  test('no pass-through entry sits there without a reason', () => {
    for (const [name, why] of PASS_THROUGH) {
      expect(why.length, `${name} needs a reason`).toBeGreaterThan(20);
    }
  });
});

describe('the two operator-facing guides name the same variables the code does', () => {
  /*
   * These two are what an operator actually types from, and both carried `MATRIX_BOT_USER`. Checked
   * separately from the reference table because they are prose with fenced env blocks rather than tables,
   * so the rule here is narrower: the wrong names must not appear at all.
   */
  const guides = ['docs/OPERATOR-WALKTHROUGH.md', 'docs/RUNNING-THE-SERVICES.md']
    .map((rel) => ({ rel, text: readFileSync(path.resolve(rel), 'utf8') }));

  test('MATRIX_BOT_USER is never given in a form an operator would copy', () => {
    /*
     * THE ASSIGNABLE FORM ONLY. A first version forbade the string outright and failed on this very
     * round's own trap-catalogue row, which names the misspelling in order to warn about it — a rule that
     * forbids describing a mistake forbids the documentation that prevents it. What must not exist is a
     * line somebody pastes into an env file.
     */
    for (const { rel, text } of guides) {
      expect(text, `${rel} has a copyable MATRIX_BOT_USER= line`).not.toMatch(/MATRIX_BOT_USER\s*=/);
    }
  });

  test('and they still say something about the bot at all, so the check cannot pass by silence', () => {
    for (const { rel, text } of guides) {
      expect(text, `${rel} no longer mentions the bot account`).toMatch(/MATRIX_BOT_USERNAME/);
    }
  });
});
