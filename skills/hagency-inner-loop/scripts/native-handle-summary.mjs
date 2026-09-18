import { createHash } from 'node:crypto';
import { closeSync, constants, fstatSync, lstatSync, openSync, readSync, readdirSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_RECORDS = 10000;
const MAX_RECORD_BYTES = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES = 32 * 1024 * 1024;
const PUBLIC_REASONS = new Set(['window_missed_checkpoint', 'observer_timeout', 'action_outcome_unknown']);
const integer = value => Number.isSafeInteger(value) && value >= 0;
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
function requireEvidence(value) { if (!value) throw new Error('invalid_evidence'); }
function identity(stat) {
  return [stat.dev, stat.ino, stat.size, stat.mode, stat.mtimeNs, stat.ctimeNs].join(':');
}

// This folds observed tool returns. It does not poll, authenticate their producer,
// or infer current OS liveness from a return carrying a resumable session ID.
export function summarizeNativeHandle(records, sessionId) {
  requireEvidence(integer(sessionId) && sessionId > 0 && Array.isArray(records)
    && records.length > 0 && records.length <= MAX_RECORDS);
  for (const row of records) {
    requireEvidence(object(row) && integer(row.request_started_ms) && integer(row.request_returned_ms)
      && row.request_started_ms <= row.request_returned_ms && object(row.request) && object(row.result)
      && typeof row.result.output === 'string'
      && row.output_sha256 === createHash('sha256').update(row.result.output).digest('hex'));
  }
  const rows = [...records].sort((a, b) => a.request_started_ms - b.request_started_ms);
  let terminal;
  for (const [index, row] of rows.entries()) {
    const { request, result } = row;
    requireEvidence(!terminal);
    if (index === 0) {
      requireEvidence(typeof request.cmd === 'string' && request.cmd.length > 0
        && !Object.hasOwn(request, 'session_id') && result.session_id === sessionId
        && !Object.hasOwn(result, 'exit_code'));
    } else {
      requireEvidence(rows[index - 1].request_returned_ms <= row.request_started_ms
        && rows[index - 1].request_started_ms < row.request_started_ms
        && request.session_id === sessionId && (!Object.hasOwn(request, 'chars') || request.chars === '')
        && !Object.hasOwn(request, 'cmd'));
    }
    if (Object.hasOwn(result, 'exit_code')) {
      requireEvidence(integer(result.exit_code) && !Object.hasOwn(result, 'session_id'));
      terminal = row;
    } else {
      requireEvidence(result.session_id === sessionId);
    }
  }
  const summary = { version: 1, status: terminal ? 'exited' : 'terminal_not_observed',
    session_id: sessionId, record_count: rows.length, full_acceptance: false };
  if (terminal) {
    summary.exit_code = terminal.result.exit_code;
    summary.terminal_returned_ms = terminal.request_returned_ms;
    try {
      const payload = JSON.parse(terminal.result.output);
      if (object(payload) && PUBLIC_REASONS.has(payload.reason)) {
        summary.reason = payload.reason;
      }
    } catch { /* Arbitrary shell output is not a structured diagnostic. */ }
  }
  return summary;
}

export function readNativeHandleRecords(directory) {
  requireEvidence(typeof directory === 'string' && path.isAbsolute(directory));
  const initialDirectory = lstatSync(directory, { bigint: true });
  requireEvidence(initialDirectory.isDirectory() && !initialDirectory.isSymbolicLink());
  const names = () => readdirSync(directory).filter(name => name.endsWith('.json')).sort();
  const before = names();
  requireEvidence(before.length > 0 && before.length <= MAX_RECORDS);
  const snapshots = [];
  let total = 0;
  const records = before.map(name => {
    const file = path.join(directory, name);
    const initial = lstatSync(file, { bigint: true });
    requireEvidence(initial.isFile() && !initial.isSymbolicLink() && (initial.mode & 0o222n) === 0n
      && initial.size <= BigInt(MAX_RECORD_BYTES));
    total += Number(initial.size);
    requireEvidence(total <= MAX_TOTAL_BYTES);
    const fd = openSync(file, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    let raw;
    try {
      requireEvidence(identity(initial) === identity(fstatSync(fd, { bigint: true })));
      const buffer = Buffer.alloc(Number(initial.size) + 1);
      let length = 0;
      while (length < buffer.length) {
        const count = readSync(fd, buffer, length, buffer.length - length, length);
        if (count === 0) break;
        length += count;
      }
      raw = buffer.subarray(0, length);
      requireEvidence(identity(initial) === identity(fstatSync(fd, { bigint: true })));
    } finally { closeSync(fd); }
    requireEvidence(BigInt(raw.length) === initial.size);
    snapshots.push({ file, identity: identity(initial) });
    return JSON.parse(raw);
  });
  requireEvidence(JSON.stringify(before) === JSON.stringify(names())
    && identity(initialDirectory) === identity(lstatSync(directory, { bigint: true })));
  for (const snapshot of snapshots) {
    requireEvidence(snapshot.identity === identity(lstatSync(snapshot.file, { bigint: true })));
  }
  return records;
}

function main(args) {
  let report;
  try {
    requireEvidence(args.length === 4 && args[0] === '--records' && args[2] === '--session-id'
      && /^[1-9][0-9]*$/.test(args[3]));
    report = summarizeNativeHandle(readNativeHandleRecords(args[1]), Number(args[3]));
    process.exitCode = report.status === 'terminal_not_observed' ? 2 : report.exit_code === 0 ? 0 : 1;
  } catch {
    report = { version: 1, status: 'invalid_evidence', full_acceptance: false };
    process.exitCode = 3;
  }
  process.stdout.write(`${JSON.stringify(report)}\n`);
}

let invokedAsScript = false;
try {
  invokedAsScript = Boolean(process.argv[1]) && realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1]);
} catch { /* Importing a library never starts the CLI. */ }
if (invokedAsScript) main(process.argv.slice(2));
