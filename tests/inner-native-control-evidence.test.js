import { createHash } from 'node:crypto';
import {
  chmodSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawn } from 'node:child_process';
import { afterEach, expect, test } from 'vitest';

const modulePath = path.resolve('skills/hagency-inner-loop/scripts/native-control-evidence.mjs');
const moduleUrl = pathToFileURL(modulePath).href;

let evidence;
try {
  evidence = await import(moduleUrl);
} catch (error) {
  if (error?.code !== 'ERR_MODULE_NOT_FOUND') throw error;
  const missing = (name) => () => {
    throw new Error(`Expected native-control-evidence API ${name} to be implemented`);
  };
  evidence = {
    assertProcessIdentity: missing('assertProcessIdentity'),
    publishExclusiveJson: missing('publishExclusiveJson'),
    readReadonlyJson: missing('readReadonlyJson'),
    resolveProtectedCapture: missing('resolveProtectedCapture'),
    verifyProtectedFiles: missing('verifyProtectedFiles'),
  };
}

const {
  assertProcessIdentity,
  publishExclusiveJson,
  readReadonlyJson,
  resolveProtectedCapture,
  verifyProtectedFiles,
} = evidence;

const roots = [];
afterEach(() => {
  while (roots.length) rmSync(roots.pop(), { recursive: true, force: true });
});

function temporaryRoot(label = 'native control evidence with spaces-') {
  const root = realpathSync(mkdtempSync(path.join(tmpdir(), label)));
  roots.push(root);
  return root;
}

const expected = {
  pid: 4242,
  ppid: 4100,
  pgid: 4242,
  started: '1760000000.000017',
  cwd: '/private/tmp/project with spaces',
  argv: [
    '/opt/homebrew/bin/node',
    '/private/tmp/herdr entry.mjs',
    '--stdio-command',
    'node ./agent helper.mjs --label "two words"',
  ],
};

const row = (fields = {}) => ({
  pid: expected.pid,
  ppid: expected.ppid,
  pgid: expected.pgid,
  started: expected.started,
  state: 'R',
  ...fields,
});

const snapshot = (processes = [row()]) => ({ version: 1, processes });
const observation = (fields = {}) => ({
  before: snapshot(),
  metadata: { pid: expected.pid, cwd: expected.cwd, argv: [...expected.argv], session: 'opaque-session' },
  after: snapshot([row({ state: 'S' })]),
  ...fields,
});

const sha256 = (raw) => createHash('sha256').update(raw).digest('hex');

function readonlyJson(file, value) {
  const raw = `${JSON.stringify(value)}\n`;
  writeFileSync(file, raw, { mode: 0o600 });
  chmodSync(file, 0o444);
  return { raw, sha256: sha256(raw) };
}

test('preserves exact argv tokens while allowing mutable process state changes', () => {
  expect(assertProcessIdentity(expected, observation())).toBeUndefined();

  const joinedArgv = observation({
    metadata: { pid: expected.pid, cwd: expected.cwd, argv: [expected.argv.join(' ')] },
  });
  expect(() => assertProcessIdentity(expected, joinedArgv)).toThrow(/process identity/i);
});

test('accepts nonzero microseconds at zero seconds without allowing zero or overflow', () => {
  const zeroSecondExpected = { ...expected, started: '0.000017' };
  const zeroSecondRow = row({ started: zeroSecondExpected.started });
  const zeroSecondObservation = observation({
    before: snapshot([zeroSecondRow]),
    after: snapshot([{ ...zeroSecondRow, state: 'S' }]),
  });
  expect(assertProcessIdentity(zeroSecondExpected, zeroSecondObservation)).toBeUndefined();

  for (const started of [
    '0.000000',
    '00.000017',
    '01760000000.000017',
    '10000000000000000000.000017',
  ]) {
    expect(() => assertProcessIdentity(
      { ...expected, started },
      observation({ before: snapshot([row({ started })]), after: snapshot([row({ started })]) }),
    )).toThrow(/process identity/i);
  }
});

test('rejects missing duplicate replaced and mismatched process identities', () => {
  const rejected = [
    observation({ before: snapshot([]) }),
    observation({ after: snapshot([]) }),
    observation({ before: snapshot([row(), row()]) }),
    observation({ before: snapshot([row({ started: '1760000000.000018' })]) }),
    observation({ after: snapshot([row({ started: '1760000000.000018' })]) }),
    observation({ after: snapshot([row({ ppid: 4101 })]) }),
    observation({ after: snapshot([row({ pgid: 4101 })]) }),
    observation({ after: snapshot([row({ state: 'Z' })]) }),
    observation({ metadata: { pid: 42420, cwd: expected.cwd, argv: [...expected.argv] } }),
    observation({ metadata: { pid: expected.pid, cwd: `${expected.cwd}-replacement`, argv: [...expected.argv] } }),
    observation({ metadata: { pid: expected.pid, cwd: expected.cwd, argv: [...expected.argv, '--extra'] } }),
    observation({ metadata: { pid: expected.pid, cwd: expected.cwd } }),
  ];
  for (const candidate of rejected) {
    expect(() => assertProcessIdentity(expected, candidate)).toThrow(/process identity/i);
  }

  const malformedSnapshots = [
    null,
    [],
    {},
    { version: 2, processes: [row()] },
    { version: 1, processes: [row()], command: 'must not become authority' },
    snapshot([null]),
    snapshot([{ ...row(), pid: '4242' }]),
    snapshot([{ ...row(), pid: 0 }]),
    snapshot([{ ...row(), ppid: -1 }]),
    snapshot([{ ...row(), pgid: -1 }]),
    snapshot([{ ...row(), state: 'S+' }]),
    snapshot([{ ...row(), started: '1760000000.17' }]),
    snapshot([{ ...row(), started: '01760000000.000017' }]),
    snapshot([{ ...row(), started: 1760000000.000017 }]),
    snapshot([{ ...row(), argv: expected.argv }]),
  ];
  for (const before of malformedSnapshots) {
    expect(() => assertProcessIdentity(expected, observation({ before }))).toThrow(/process identity/i);
  }

  expect(() => assertProcessIdentity({ ...expected, optionalProof: true }, observation()))
    .toThrow(/process identity/i);
});

test('validates relative readonly UUID captures and rejects unsafe paths', () => {
  const root = temporaryRoot();
  const project = path.join(root, 'canonical project');
  const outputDirectory = path.join(project, '.autonomy', 'test-outputs');
  mkdirSync(outputDirectory, { recursive: true });
  const relative = '.autonomy/test-outputs/123e4567-e89b-42d3-a456-426614174000.json';
  const capture = path.join(project, relative);
  readonlyJson(capture, { result: 'bounded' });

  expect(resolveProtectedCapture(realpathSync(project), relative)).toBe(capture);

  const outside = path.join(root, 'outside.json');
  readonlyJson(outside, { private: 'outside' });
  const linkedCapture = path.join(outputDirectory, '123e4567-e89b-42d3-a456-426614174001.json');
  symlinkSync(outside, linkedCapture);
  const linkedDirectory = path.join(project, '.autonomy', 'linked-outputs');
  symlinkSync(outputDirectory, linkedDirectory, 'dir');
  const projectLink = path.join(root, 'project-link');
  symlinkSync(project, projectLink, 'dir');

  for (const [candidateProject, candidate] of [
    [project, capture],
    [project, '../outside.json'],
    [project, '.autonomy/test-outputs/not-a-uuid.json'],
    [project, '.autonomy/test-outputs/123e4567-e89b-12d3-a456-426614174000.json'],
    [project, '.autonomy/test-outputs/123e4567-e89b-42d3-c456-426614174000.json'],
    [project, '.autonomy/test-outputs/123e4567-e89b-42d3-a456-426614174001.json'],
    [projectLink, relative],
  ]) {
    expect(() => resolveProtectedCapture(candidateProject, candidate)).toThrow(/protected capture/i);
  }

  chmodSync(capture, 0o644);
  expect(() => resolveProtectedCapture(project, relative)).toThrow(/protected capture/i);
});

test('publishes readonly JSON exclusively and preserves collisions and symlinks', async () => {
  const root = temporaryRoot();
  const target = path.join(root, 'evidence file.json');
  const first = publishExclusiveJson(target, { operation: 'first', count: 1 });
  expect(first.value).toEqual({ operation: 'first', count: 1 });
  expect(Buffer.isBuffer(first.raw)).toBe(true);
  expect(first.sha256).toBe(sha256(first.raw));
  expect(lstatSync(target).mode & 0o777).toBe(0o444);
  const original = readFileSync(target);

  expect(() => publishExclusiveJson(target, { private: 'replacement' })).toThrow(/publish evidence/i);
  expect(readFileSync(target)).toEqual(original);
  expect(lstatSync(target).mode & 0o777).toBe(0o444);

  const outside = path.join(root, 'outside.json');
  writeFileSync(outside, 'original outside bytes', { mode: 0o600 });
  const linked = path.join(root, 'linked evidence.json');
  symlinkSync(outside, linked);
  expect(() => publishExclusiveJson(linked, { private: 'replacement' })).toThrow(/publish evidence/i);
  expect(readFileSync(outside, 'utf8')).toBe('original outside bytes');
  expect(lstatSync(outside).mode & 0o777).toBe(0o600);

  const raceTarget = path.join(root, 'raced evidence.json');
  const values = [{ winner: 'alpha' }, { winner: 'beta' }];
  const gate = Date.now() + 300;
  const childCode = [
    `const { publishExclusiveJson } = await import(process.argv[1]);`,
    `while (Date.now() < Number(process.argv[4])) {}`,
    `publishExclusiveJson(process.argv[2], JSON.parse(process.argv[3]));`,
  ].join('\n');
  const outcomes = await Promise.all(values.map((value) => new Promise((resolve) => {
    const child = spawn(process.execPath, [
      '--input-type=module', '--eval', childCode,
      moduleUrl, raceTarget, JSON.stringify(value), String(gate),
    ], { stdio: ['ignore', 'ignore', 'pipe'] });
    let stderr = '';
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.on('close', (status) => resolve({ status, stderr }));
  })));
  expect(outcomes.map(({ status }) => status).sort()).toEqual([0, 1]);
  expect(values).toContainEqual(JSON.parse(readFileSync(raceTarget, 'utf8')));
  expect(lstatSync(raceTarget).mode & 0o777).toBe(0o444);
});

test('reads readonly JSON bytes with an optional exact SHA256 pin and bounded errors', () => {
  const root = temporaryRoot();
  const file = path.join(root, 'readonly plan with spaces.json');
  const written = readonlyJson(file, { operation: 'inspect', nested: { enabled: true } });

  const result = readReadonlyJson(file, written.sha256);
  expect(result.value).toEqual({ operation: 'inspect', nested: { enabled: true } });
  expect(result.raw).toEqual(Buffer.from(written.raw));
  expect(result.sha256).toBe(written.sha256);
  expect(readReadonlyJson(file).sha256).toBe(written.sha256);

  for (const digest of ['', 'abc', written.sha256.toUpperCase(), `${written.sha256.slice(0, -1)}g`]) {
    expect(() => readReadonlyJson(file, digest)).toThrow(/readonly json/i);
  }
  expect(() => readReadonlyJson(file, '0'.repeat(64))).toThrow(/readonly json/i);

  chmodSync(file, 0o644);
  expect(() => readReadonlyJson(file, written.sha256)).toThrow(/readonly json/i);

  const privateMarker = 'PRIVATE_MARKER_must_not_escape';
  const malformed = path.join(root, 'malformed.json');
  writeFileSync(malformed, `{${privateMarker}`, { mode: 0o400 });
  let message = '';
  try { readReadonlyJson(malformed); } catch (error) { message = error.message; }
  expect(message).toMatch(/readonly json/i);
  expect(message).not.toContain(privateMarker);
});

test('rejects symlinked JSON files and parent components', () => {
  const root = temporaryRoot();
  const canonicalDirectory = path.join(root, 'canonical');
  mkdirSync(canonicalDirectory);
  const file = path.join(canonicalDirectory, 'record.json');
  readonlyJson(file, { safe: true });
  const fileLink = path.join(canonicalDirectory, 'record-link.json');
  symlinkSync(file, fileLink);
  const directoryLink = path.join(root, 'directory-link');
  symlinkSync(canonicalDirectory, directoryLink, 'dir');

  expect(() => readReadonlyJson(fileLink)).toThrow(/readonly json/i);
  expect(() => readReadonlyJson(path.join(directoryLink, 'record.json'))).toThrow(/readonly json/i);
  expect(() => readReadonlyJson(path.relative(process.cwd(), file))).toThrow(/readonly json/i);
  expect(() => publishExclusiveJson(path.join(directoryLink, 'new.json'), { safe: false }))
    .toThrow(/publish evidence/i);
});

test('verifies a nonempty exact map of protected readonly file digests', () => {
  const root = temporaryRoot();
  const project = path.join(root, 'protected project');
  mkdirSync(path.join(project, 'plans'), { recursive: true });
  const first = readonlyJson(path.join(project, 'plans', 'one.json'), { one: 1 });
  const second = readonlyJson(path.join(project, 'plans', 'two with spaces.json'), { two: 2 });
  const proof = {
    'plans/one.json': first.sha256,
    'plans/two with spaces.json': second.sha256,
  };

  expect(verifyProtectedFiles(realpathSync(project), proof)).toBeUndefined();
  expect(() => verifyProtectedFiles(project, {})).toThrow(/protected files/i);
  expect(() => verifyProtectedFiles(project, { 'plans/one.json': '0'.repeat(64) }))
    .toThrow(/protected files/i);
  expect(() => verifyProtectedFiles(project, { '../outside.json': first.sha256 }))
    .toThrow(/protected files/i);
  expect(() => verifyProtectedFiles(project, { 'plans/one.json': first.sha256.toUpperCase() }))
    .toThrow(/protected files/i);

  chmodSync(path.join(project, 'plans', 'one.json'), 0o644);
  expect(() => verifyProtectedFiles(project, proof)).toThrow(/protected files/i);
});
