import { createHash } from 'node:crypto';
import { chmodSync, mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { afterEach, expect, test } from 'vitest';

const entry = path.resolve('skills/hagency-inner-loop/scripts/native-handle-summary.mjs');
const dirs = [];
afterEach(() => { for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true }); });
function record(start, end, result, request) {
  return { request_started_ms: start, request_returned_ms: end, result, request,
    output_sha256: createHash('sha256').update(result.output).digest('hex') };
}
function history() {
  return [
    record(100, 200, { session_id: 12079, output: '' }, { cmd: 'private command' }),
    record(210, 220, { session_id: 12079, output: '' }, { session_id: 12079, chars: '' }),
    record(230, 300, { exit_code: 1, output: '{"status":"failed","reason":"window_missed_checkpoint"}\n' }, { session_id: 12079, chars: '' }),
  ];
}
async function summarize(rows) {
  const module = await import(entry).catch(error => {
    if (error.code === 'ERR_MODULE_NOT_FOUND') return {};
    throw error;
  });
  expect(module.summarizeNativeHandle, 'summary export must exist').toBeTypeOf('function');
  return module.summarizeNativeHandle(rows, 12079);
}
function files(rows = history()) {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'hagency-handle-')); dirs.push(dir);
  rows.forEach((row, i) => writeFileSync(path.join(dir, `${i}.json`), JSON.stringify(row), { mode: 0o400 }));
  return dir;
}
function cli(dir, args = []) {
  return spawnSync(process.execPath, [entry, '--records', dir, '--session-id', '12079', ...args], { encoding: 'utf8', timeout: 10000 });
}

test('reports the persisted failed observer terminal', async () => {
  expect(await summarize(history().reverse())).toEqual({ version: 1, status: 'exited', session_id: 12079,
    record_count: 3, exit_code: 1, terminal_returned_ms: 300, reason: 'window_missed_checkpoint', full_acceptance: false });
});
test('reports missing terminal without claiming a running process', async () => {
  expect(await summarize(history().slice(0, 2))).toEqual({ version: 1, status: 'terminal_not_observed', session_id: 12079,
    record_count: 2, full_acceptance: false });
});
test('reports exit zero without accepting the job', async () => {
  const rows = history(); rows[2] = record(230, 300, { exit_code: 0, output: 'private output' }, { session_id: 12079, chars: '' });
  expect(await summarize(rows)).toMatchObject({ status: 'exited', exit_code: 0, full_acceptance: false });
});
test('accepts omitted empty input and refuses actual input', async () => {
  const rows = history(); delete rows[1].request.chars; delete rows[2].request.chars;
  expect(await summarize(rows)).toMatchObject({ status: 'exited', exit_code: 1 });
  rows[1].request.chars = 'x';
  await expect(summarize(rows)).rejects.toThrow('invalid_evidence');
});
test('does not expose arbitrary reason strings from private output', async () => {
  const rows = history();
  rows[2] = record(230, 300, { exit_code: 1, output: '{"reason":"private_fixture_token_abc123"}' }, { session_id: 12079, chars: '' });
  const summary = await summarize(rows);
  expect(summary.status).toBe('exited'); expect(summary).not.toHaveProperty('reason');
  expect(JSON.stringify(summary)).not.toContain('private_fixture_token_abc123');
});
test('rejects mixed handles and returns after terminal', async () => {
  const wrong = history(); wrong[1].request.session_id = 999;
  await expect(summarize(wrong)).rejects.toThrow('invalid_evidence');
  const later = history(); later.push(record(310, 320, { session_id: 12079, output: '' }, { session_id: 12079, chars: '' }));
  await expect(summarize(later)).rejects.toThrow('invalid_evidence');
});
test('rejects truncated hashes ambiguous results and overlapping requests', async () => {
  for (const mutate of [rows => { rows[2].output_sha256 = 'abc'; }, rows => { rows[2].result.session_id = 12079; },
    rows => { rows[1].request_started_ms = 190; }, rows => { rows[0].request = { session_id: 12079 }; },
    rows => { rows[2].result.exit_code = null; }]) {
    const rows = history(); mutate(rows); await expect(summarize(rows)).rejects.toThrow('invalid_evidence');
  }
});
test('CLI emits a compact failed terminal summary', () => {
  const run = cli(files()); expect(run.status).toBe(1); expect(run.stderr).toBe('');
  expect(JSON.parse(run.stdout)).toMatchObject({ status: 'exited', exit_code: 1, reason: 'window_missed_checkpoint' });
  expect(run.stdout).not.toContain('private command'); expect(run.stdout).not.toContain('"output"');
});
test('CLI distinguishes missing terminal and invalid arguments', () => {
  expect(cli(files(history().slice(0, 2))).status).toBe(2);
  const run = cli(files(), ['--extra']); expect(run.status).toBe(3);
  expect(JSON.parse(run.stdout)).toMatchObject({ status: 'invalid_evidence', full_acceptance: false });
});
test('rejects corrupt writable and symlinked records', () => {
  for (const kind of ['corrupt', 'writable', 'symlink', 'directory']) {
    const dir = files(), target = path.join(dir, '1.json'); chmodSync(target, 0o600);
    if (kind === 'corrupt') { writeFileSync(target, '{'); chmodSync(target, 0o400); }
    if (kind === 'symlink') { rmSync(target); symlinkSync(path.join(dir, '0.json'), target); }
    if (kind === 'directory') { rmSync(target); mkdirSync(target); }
    const run = cli(dir); expect(run.status, kind).toBe(3);
    expect(JSON.parse(run.stdout)).toMatchObject({ status: 'invalid_evidence', full_acceptance: false });
    expect(run.stderr).toBe('');
  }
});
