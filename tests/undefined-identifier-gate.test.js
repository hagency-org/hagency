/*
 * The gate that catches an identifier that does not exist.
 *
 * WHY IT EXISTS, in three occurrences of one shape — each syntactically valid, each clean under
 * `node --check`, each a ReferenceError at runtime:
 *
 *   - a `pollBotInvites` call that survived a refactor and logged `invite poll failed` 490 times
 *     against a live deployment before anyone read the log;
 *   - `observeBindingMembership` calling a `persist()` that did not exist on the store;
 *   - four sites rewritten to `baseUrlForToken(agentToken)` where the variable is named `token`.
 *
 * `scripts/check-syntax.sh` runs `node --check`, which parses and does not resolve identifiers. One
 * eslint rule closes that, and this file is what stops the gate from being quietly removed or
 * neutered — a lint step that reports nothing because it is misconfigured looks exactly like a clean
 * codebase.
 */

import { describe, expect, test } from 'vitest';
import { execFileSync } from 'child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'fs';
import path from 'path';
import { ESLint } from 'eslint';

const REPO = new URL('..', import.meta.url).pathname;

/*
 * The probe lives INSIDE the repository, and that is not laziness. Flat config applies only to files
 * under the directory holding `eslint.config.js`, so a file in the OS temp dir is refused outright with
 * "Oops! Something went wrong!" — which the first version of this test read as the rule not firing.
 * Written into a uniquely named directory and removed afterwards.
 */
function withProbe(source, fn) {
  const dir = path.join(REPO, `.undef-probe-${process.pid}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(dir, { recursive: true });
  const file = path.join(dir, 'probe.js');
  writeFileSync(file, source);
  try {
    return fn(file);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function eslintOn(target) {
  try {
    execFileSync('npx', ['eslint', '--no-warn-ignored', target], { cwd: REPO, encoding: 'utf8', stdio: 'pipe' });
    return { ok: true, out: '' };
  } catch (error) {
    return { ok: false, out: `${error.stdout || ''}${error.stderr || ''}` };
  }
}

describe('the rule is enabled and configured to bite', () => {
  test('no-undef is the rule, and it is an error rather than a warning', () => {
    // A warning would let CI pass with the finding printed, which is the shape of a gate that reports
    // rather than gates.
    const config = readFileSync(path.join(REPO, 'eslint.config.js'), 'utf8');
    expect(config).toMatch(/'no-undef':\s*'error'/);
  });

  test('THE PROOF: a file with an undefined identifier is REJECTED', () => {
    /*
     * The assertion that makes the other one meaningful. A config can name a rule and still find
     * nothing — wrong globs, wrong parser, everything ignored. This runs the real binary over a real
     * file and requires a real failure.
     */
    withProbe('export function f() {\n  return somethingUndeclared + 1;\n}\n', (file) => {
      const result = eslintOn(file);
      expect(result.ok, `eslint accepted an undeclared identifier:\n${result.out}`).toBe(false);
      expect(result.out).toMatch(/no-undef/);
      expect(result.out).toMatch(/somethingUndeclared/);
    });
  });

  test('and `node --check` does NOT reject it — which is the gap', () => {
    /*
     * Pinned so the justification for this dependency stays checkable rather than remembered. If a
     * future Node did resolve identifiers, this test would fail and the gate could be reconsidered.
     */
    withProbe('export function f() {\n  return somethingUndeclared + 1;\n}\n', (file) => {
      expect(() => execFileSync('node', ['--check', file], { stdio: 'pipe' })).not.toThrow();
    });
  });

  test('the repository currently has NO undefined identifiers', () => {
    // The gate is prospective: it found nothing on the day it was added, because the four occurrences
    // that motivated it had just been fixed. This asserts the clean state it starts from.
    const result = eslintOn('.');
    expect(result.ok, `eslint reported findings:\n${result.out}`).toBe(true);
  }, 120_000);
});

describe('the gate runs in CI', () => {
  test('Rust targets are ignored while native JavaScript remains checked', async () => {
    const eslint = new ESLint({ cwd: REPO });
    for (const artifact of ['target/debug/build/generated.js', 'native/example/target/debug/generated.js']) {
      expect(await eslint.isPathIgnored(path.join(REPO, artifact))).toBe(true);
    }
    const source = path.join(REPO, 'native/scripts/identifier-probe.mjs');
    expect(await eslint.isPathIgnored(source)).toBe(false);
    const [result] = await eslint.lintText('unknownNativeBuildIdentifier();', { filePath: source });
    expect(result.messages.some((message) => message.ruleId === 'no-undef')).toBe(true);
    expect(result.errorCount).toBe(1);
  });

  test('verify:ci invokes it, so it cannot pass locally and be skipped remotely', () => {
    const verify = readFileSync(path.join(REPO, 'scripts/verify-ci.sh'), 'utf8');
    expect(verify).toContain('npm run check:undef');
  });

  test('the npm script exists and points at eslint', () => {
    const pkg = JSON.parse(readFileSync(path.join(REPO, 'package.json'), 'utf8'));
    expect(pkg.scripts['check:undef']).toMatch(/eslint/);
  });

  test('generated and foreign trees are ignored, so the signal is not buried', () => {
    /*
     * The first version of this config scoped `ignores` inside a config object, which in flat config
     * applies only to that object — so `mockup/.next` build artifacts were linted and produced most of
     * the output. 106 findings, none of them `no-undef`. A gate whose output nobody reads is worth less
     * than no gate, because it looks like coverage.
     */
    const config = readFileSync(path.join(REPO, 'eslint.config.js'), 'utf8');
    for (const ignored of ['node_modules/**', 'mockup/**', 'remote/**', '**/.next/**']) {
      expect(config).toContain(ignored);
    }
  });
});
