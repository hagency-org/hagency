import { afterAll, beforeAll, describe, expect, test } from 'vitest';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, realpathSync, rmSync } from 'node:fs';
import { endianness, tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = path.join(root, 'skills/hagency-inner-loop/native/darwin-process-metadata.c');
const driverSource = path.join(root, 'tests/fixtures/darwin-process-metadata-driver.c');
const nodeIsDarwin = process.platform === 'darwin';
const rawCap = 4 * 1024 * 1024;

let tempRoot;
let cli;
let driver;

function compile(input, output) {
  const result = spawnSync('/usr/bin/clang', [
    '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', input, '-o', output,
  ], { cwd: root, encoding: 'utf8' });
  expect(result.status, result.stderr).toBe(0);
  expect(result.stderr).toBe('');
}

function run(binary, args, options = {}) {
  return spawnSync(binary, args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 8 * 1024 * 1024,
    ...options,
  });
}

function int32(value) {
  const bytes = Buffer.alloc(4);
  if (endianness() === 'BE') bytes.writeInt32BE(value);
  else bytes.writeInt32LE(value);
  return bytes;
}

function argumentBuffer({ width, executable = '/fixture', argv, environment = [], paddingByte = 0 }) {
  const executableBytes = Buffer.from(`${executable}\0`);
  const paddingLength = (width - ((16 + executableBytes.length) % width)) % width;
  const padding = Buffer.alloc(paddingLength, paddingByte);
  const tokens = [...argv, ...environment].map((value) =>
    Buffer.concat([Buffer.isBuffer(value) ? value : Buffer.from(value), Buffer.from([0])]),
  );
  return Buffer.concat([int32(argv.length), executableBytes, padding, ...tokens]);
}

function parseBuffer(raw, width) {
  return run(driver, ['parse', String(width)], { input: raw });
}

function waitForJsonLine(child) {
  return new Promise((resolve, reject) => {
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      stdout += chunk;
      const newline = stdout.indexOf('\n');
      if (newline !== -1) {
        try {
          resolve(JSON.parse(stdout.slice(0, newline)));
        } catch (error) {
          reject(error);
        }
      }
    });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.once('error', reject);
    child.once('exit', (code) => reject(new Error(`fixture exited before ready (${code}): ${stderr}`)));
  });
}

function exitOf(child) {
  return new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => resolve({ code, signal }));
  });
}

beforeAll(() => {
  tempRoot = mkdtempSync(path.join(tmpdir(), 'darwin metadata tests-'));
  cli = path.join(tempRoot, 'darwin-process-metadata');
  driver = path.join(tempRoot, 'darwin-process-metadata-driver');
  compile(source, cli);
  compile(driverSource, driver);
});

afterAll(() => {
  rmSync(tempRoot, { recursive: true, force: true });
});

describe('native Darwin process metadata', () => {
  // Register platform-specific selectors on every host for specification inventory.
  // Runtime skips remain skips; only Darwin executes the native observation assertions.
  test('reads exact argv tokens and cwd from an independently owned process', async ({ skip }) => {
    if (!nodeIsDarwin) skip('Requires Darwin process metadata APIs');
    const cwd = path.join(tempRoot, 'cwd with spaces 雪');
    mkdirSync(cwd);
    const expectedArgv = ['fixture argv0 with spaces', '--hold', '', 'two words', '中文', 'quote"slash\\'];
    const fixture = spawn(driver, expectedArgv.slice(1), {
      argv0: expectedArgv[0],
      cwd,
      env: { OWNED_ENV_SENTINEL: 'PRIVATE_VALUE_MUST_NOT_APPEAR' },
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    const exited = exitOf(fixture);

    try {
      const ready = await waitForJsonLine(fixture);
      expect(ready.pid).toBe(fixture.pid);
      const result = run(cli, ['--pid', String(fixture.pid)]);
      expect(result.status, result.stderr).toBe(0);
      expect(result.stderr).toBe('');
      expect(JSON.parse(result.stdout)).toEqual({
        version: 1,
        pid: fixture.pid,
        cwd: realpathSync(cwd),
        argv: expectedArgv,
      });
      expect(result.stdout).not.toContain('OWNED_ENV_SENTINEL');
      expect(result.stdout).not.toContain('PRIVATE_VALUE_MUST_NOT_APPEAR');
    } finally {
      fixture.stdin.end();
      await expect(exited).resolves.toEqual({ code: 0, signal: null });
    }
  });

  test('rejects invalid PID commands and nonexistent processes without metadata', () => {
    const invalid = [
      [], ['--pid'], ['--pid', '0'], ['--pid', '01'], ['--pid', '+1'],
      ['--pid', '-1'], ['--pid', '2147483648'], ['--pid', 'abc'], ['--pid', '1', 'extra'],
    ];
    for (const args of invalid) {
      const result = run(cli, args);
      expect(result.status, JSON.stringify(args)).not.toBe(0);
      expect(result.stdout, JSON.stringify(args)).toBe('');
      expect(result.stderr).toBe('{"category":"invalid_command"}\n');
    }

    const missing = run(cli, ['--pid', '2147483647']);
    expect(missing.status).not.toBe(0);
    expect(missing.stdout).toBe('');
    expect(missing.stderr).toMatch(/^\{"category":"(?:process_identity|unsupported_platform)"\}\n$/);
  });

  test('parses exactly argc arguments and never serializes the environment suffix', () => {
    const cases = [
      { width: 4, executable: '/ab', argv: ['argv0', '', 'tail'] },
      { width: 4, executable: '/a', argv: ['argv0', 'four'] },
      { width: 8, executable: '/abcdef', argv: ['argv0', 'eight'] },
      { width: 8, executable: '/a', argv: ['argv0', '', 'two words', '雪', ''] },
    ];
    for (const item of cases) {
      const raw = argumentBuffer({
        ...item,
        environment: ['ENV_SENTINEL=must-not-serialize', Buffer.from([0xff, 0xfe])],
      });
      const result = parseBuffer(raw, item.width);
      expect(result.status, result.stderr).toBe(0);
      expect(result.stderr).toBe('');
      expect(JSON.parse(result.stdout).argv).toEqual(item.argv);
      expect(result.stdout).not.toContain('ENV_SENTINEL');
    }
  });

  test('rejects malformed argument buffers and invalid UTF8 before output', () => {
    const malformed = [
      { width: 8, raw: argumentBuffer({ width: 8, argv: ['argv0'], paddingByte: 1 }) },
      { width: 8, raw: int32(0) },
      { width: 8, raw: int32(-1) },
      { width: 8, raw: int32(16385) },
      { width: 8, raw: Buffer.concat([int32(1), Buffer.from('/missing-exec-nul')]) },
      { width: 3, raw: argumentBuffer({ width: 8, argv: ['argv0'] }) },
      { width: 8, raw: argumentBuffer({ width: 8, argv: ['argv0', 'unterminated'] }).subarray(0, -1) },
      { width: 8, raw: argumentBuffer({ width: 8, argv: ['argv0', Buffer.from([0xc0, 0x80])] }) },
      { width: 8, raw: argumentBuffer({ width: 8, argv: ['argv0', Buffer.from([0xed, 0xa0, 0x80])] }) },
      { width: 8, raw: argumentBuffer({ width: 8, argv: ['argv0', Buffer.from([0xf4, 0x90, 0x80, 0x80])] }) },
      { width: 8, raw: argumentBuffer({ width: 8, argv: ['argv0', Buffer.from([0xe2, 0x82])] }) },
    ];
    for (const item of malformed) {
      const result = parseBuffer(item.raw, item.width);
      expect(result.status).not.toBe(0);
      expect(result.stdout).toBe('');
      expect(result.stderr).toBe('{"category":"argument_data"}\n');
    }

    const oversized = parseBuffer(Buffer.alloc(rawCap + 1), 8);
    expect(oversized.status).not.toBe(0);
    expect(oversized.stdout).toBe('');
    expect(oversized.stderr).toBe('{"category":"argument_data"}\n');
  });

  test('rejects failed and short native observations without a fallback', ({ skip }) => {
    if (!nodeIsDarwin) skip('Requires Darwin process metadata APIs');
    const cases = [
      ['sizing-error', 'process_arguments'],
      ['sizing-oversize', 'process_arguments'],
      ['fetch-error', 'process_arguments'],
      ['fetch-short', 'process_arguments'],
      ['fetch-oversize', 'process_arguments'],
      ['cwd-error', 'cwd'],
      ['cwd-short', 'cwd'],
      ['cwd-no-nul', 'cwd'],
      ['cwd-relative', 'cwd'],
      ['cwd-invalid-utf8', 'cwd'],
      ['bsd-short', 'process_identity'],
      ['bsd-after-short', 'process_identity'],
      ['bsd-pid', 'process_identity'],
      ['bsd-after-pid', 'process_identity'],
      ['bsd-lp64-change', 'process_identity'],
      ['bsd-birth-change', 'process_identity'],
      ['budget', 'deadline_exceeded'],
      ['budget-after-sizing-error', 'deadline_exceeded'],
    ];
    for (const [fault, category] of cases) {
      const result = run(driver, ['mock', fault]);
      expect(result.status, fault).not.toBe(0);
      expect(result.stdout, fault).toBe('');
      expect(result.stderr, fault).toBe(`{"category":"${category}"}\n`);
    }
  });

  test('rejects an owned process with empty argv0 without exposing its environment', async ({ skip }) => {
    if (!nodeIsDarwin) skip('Requires Darwin process metadata APIs');
    const sentinel = 'EMPTY_ARGV0_PRIVATE_SENTINEL_7c93';
    const fixture = spawn(driver, ['launch-empty', sentinel], {
      cwd: tempRoot,
      env: {},
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    const exited = exitOf(fixture);

    try {
      const ready = await waitForJsonLine(fixture);
      const result = run(cli, ['--pid', String(ready.pid)]);
      expect(result.status).not.toBe(0);
      expect(result.stdout).toBe('');
      expect(result.stderr).toBe('{"category":"argument_data"}\n');
      expect(result.stderr).not.toContain(sentinel);
    } finally {
      fixture.stdin.end();
      await expect(exited).resolves.toEqual({ code: 0, signal: null });
    }
  });
});
