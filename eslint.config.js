/*
 * ONE RULE, ON PURPOSE: `no-undef`.
 *
 * This config exists because a class of defect kept reaching main and nothing here could catch it.
 * Three occurrences, all the same shape — syntactically valid, `node --check` clean, ReferenceError at
 * runtime:
 *
 *   - a `pollBotInvites` call that survived a refactor, which logged `invite poll failed` 490 times
 *     against a live deployment before anyone read the log;
 *   - `observeBindingMembership` calling a `persist()` that did not exist on the store;
 *   - four sites rewritten to `baseUrlForToken(agentToken)` where the variable is named `token`.
 *
 * `scripts/check-syntax.sh` runs `node --check`, which PARSES and does not resolve identifiers. That is
 * the gap, and one rule closes it.
 *
 * WHY NOT A STYLE PRESET. This repository has 12k-line modules with deliberate, argued-for shapes and a
 * `check:dep-isolation` gate whose whole purpose is to keep dependency additions deliberate. Turning on
 * a recommended set would produce hundreds of findings that are all opinions, and the signal that
 * matters — "this identifier does not exist" — would arrive buried in them. A lint run nobody reads is
 * worth less than no lint run, because it looks like coverage.
 *
 * TWO THINGS THE FIRST VERSION OF THIS FILE GOT WRONG, both of which produced exactly that noise:
 *
 *   - `ignores` inside a config object scopes only that object. Build artifacts under `mockup/.next`
 *     were linted anyway, and generated bundles produced most of the output. Ignores belong in their
 *     own top-level entry.
 *   - `reportUnusedDisableDirectives` was on, with the note that a stale disable comment is drift worth
 *     catching. It is not stale here: this repository carries `eslint-disable` comments for rules from
 *     presets it does not run, so every one of them was reported as unused. Flagging a comment for a
 *     rule this config deliberately does not enable is noise by construction.
 *
 * If more rules are wanted later, adding them is one line each and each deserves its own argument.
 */

import globals from './scripts/eslint-globals.js';

export default [
  {
    ignores: [
      'node_modules/**',
      // Its own package with its own toolchain (mockup/AGENTS.md), and its build output is generated.
      'mockup/**',
      // Generated from the root package by scripts/build-remote-package.sh — lint the source.
      'remote/**',
      'coverage/**',
      '**/.next/**',
      '**/dist/**',
      // Cargo removes temporary compiler directories while builds are active.
      '**/target/**',
      '**/*.min.js',
    ],
  },
  {
    files: ['**/*.js', '**/*.mjs'],
    languageOptions: {
      // `latest` rather than a year: import attributes (`with { type: 'json' }`) are used in the tests
      // and a pinned older version reports them as a parse error.
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals,
    },
    linterOptions: {
      /*
       * OFF. This repository carries `eslint-disable` comments for rules from presets it does not run —
       * `no-await-in-loop`, `no-control-regex` — so every one of them reports as unused. Four warnings
       * for comments that are correct in their own context would bury the one line that matters, which
       * is the count of real findings.
       */
      reportUnusedDisableDirectives: 'off',
    },
    rules: {
      'no-undef': 'error',
    },
  },
];
