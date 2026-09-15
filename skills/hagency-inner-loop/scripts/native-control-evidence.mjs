import { createHash, randomUUID, timingSafeEqual } from 'node:crypto';
import {
  closeSync,
  constants,
  fchmodSync,
  fstatSync,
  fsyncSync,
  linkSync,
  lstatSync,
  openSync,
  readFileSync,
  realpathSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';

const EXPECTED_PROCESS_KEYS = ['argv', 'cwd', 'pgid', 'pid', 'ppid', 'started'];
const SNAPSHOT_KEYS = ['processes', 'version'];
const PROCESS_ROW_KEYS = ['pgid', 'pid', 'ppid', 'started', 'state'];
const STARTED_PATTERN = /^(?:0|[1-9]\d{0,18})\.\d{6}$/;
const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const UUID_V4_JSON_PATTERN = /^\.autonomy\/test-outputs\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.json$/i;
const VALID_STATES = new Set(['I', 'R', 'S', 'T', 'Z']);

function fail(category) {
  throw new Error(category);
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function hasExactKeys(value, keys) {
  return isRecord(value)
    && Object.keys(value).sort().join('\0') === [...keys].sort().join('\0');
}

function isPid(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function isProcessGroupField(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function isStarted(value) {
  return typeof value === 'string'
    && STARTED_PATTERN.test(value)
    && value !== '0.000000';
}

function isArgv(value) {
  return Array.isArray(value)
    && value.length > 0
    && value.every((token) => typeof token === 'string');
}

function validateExpectedProcess(expected) {
  if (!hasExactKeys(expected, EXPECTED_PROCESS_KEYS)
    || !isPid(expected.pid)
    || !isProcessGroupField(expected.ppid)
    || !isProcessGroupField(expected.pgid)
    || !isStarted(expected.started)
    || typeof expected.cwd !== 'string'
    || expected.cwd.length === 0
    || !isArgv(expected.argv)) {
    fail('Invalid process identity evidence');
  }
}

function parseSnapshot(snapshot) {
  if (!hasExactKeys(snapshot, SNAPSHOT_KEYS)
    || snapshot.version !== 1
    || !Array.isArray(snapshot.processes)) {
    fail('Invalid process identity evidence');
  }

  const processes = new Map();
  for (const processRow of snapshot.processes) {
    if (!hasExactKeys(processRow, PROCESS_ROW_KEYS)
      || !isPid(processRow.pid)
      || !isProcessGroupField(processRow.ppid)
      || !isProcessGroupField(processRow.pgid)
      || !isStarted(processRow.started)
      || !VALID_STATES.has(processRow.state)
      || processes.has(processRow.pid)) {
      fail('Invalid process identity evidence');
    }
    processes.set(processRow.pid, processRow);
  }
  return processes;
}

function equalTokens(left, right) {
  return left.length === right.length && left.every((token, index) => token === right[index]);
}

function matchesBirth(expected, observed) {
  return observed !== undefined
    && observed.pid === expected.pid
    && observed.ppid === expected.ppid
    && observed.pgid === expected.pgid
    && observed.started === expected.started
    && observed.state !== 'Z';
}

export function assertProcessIdentity(expected, observation) {
  validateExpectedProcess(expected);
  if (!isRecord(observation)
    || !Object.hasOwn(observation, 'before')
    || !Object.hasOwn(observation, 'metadata')
    || !Object.hasOwn(observation, 'after')) {
    fail('Invalid process identity evidence');
  }

  const before = parseSnapshot(observation.before);
  const after = parseSnapshot(observation.after);
  const metadata = observation.metadata;
  if (!isRecord(metadata)
    || !isPid(metadata.pid)
    || typeof metadata.cwd !== 'string'
    || !isArgv(metadata.argv)
    || metadata.pid !== expected.pid
    || metadata.cwd !== expected.cwd
    || !equalTokens(metadata.argv, expected.argv)
    || !matchesBirth(expected, before.get(expected.pid))
    || !matchesBirth(expected, after.get(expected.pid))) {
    fail('Invalid process identity evidence');
  }
}

function assertCanonicalAbsolute(candidate, category) {
  if (typeof candidate !== 'string'
    || !path.isAbsolute(candidate)
    || path.normalize(candidate) !== candidate) {
    fail(category);
  }
}

function pathComponents(candidate) {
  const parsed = path.parse(candidate);
  const relative = candidate.slice(parsed.root.length);
  const components = relative === '' ? [] : relative.split(path.sep);
  const paths = [];
  let current = parsed.root;
  for (const component of components) {
    current = path.join(current, component);
    paths.push(current);
  }
  return paths;
}

function inspectCanonicalPath(candidate, category) {
  assertCanonicalAbsolute(candidate, category);
  let information;
  try {
    for (const component of pathComponents(candidate)) {
      const componentInformation = lstatSync(component);
      if (componentInformation.isSymbolicLink()) fail(category);
      if (component === candidate) information = componentInformation;
    }
    if (realpathSync(candidate) !== candidate) fail(category);
  } catch {
    fail(category);
  }
  return information;
}

function inspectCanonicalDirectory(directory, category) {
  const information = inspectCanonicalPath(directory, category);
  if (!information?.isDirectory()) fail(category);
  return information;
}

function inspectCanonicalReadonlyFile(file, category) {
  const information = inspectCanonicalPath(file, category);
  if (!information?.isFile() || information.isSymbolicLink() || (information.mode & 0o222) !== 0) {
    fail(category);
  }
  return information;
}

function sameFile(left, right) {
  return left.dev === right.dev && left.ino === right.ino;
}

function readReadonlyBytes(file, category) {
  const initial = inspectCanonicalReadonlyFile(file, category);
  let descriptor;
  try {
    descriptor = openSync(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const opened = fstatSync(descriptor);
    if (!opened.isFile() || (opened.mode & 0o222) !== 0 || !sameFile(initial, opened)) fail(category);
    const raw = readFileSync(descriptor);
    const final = fstatSync(descriptor);
    if (!final.isFile()
      || (final.mode & 0o222) !== 0
      || !sameFile(opened, final)
      || final.size !== opened.size
      || raw.length !== opened.size) {
      fail(category);
    }
    return raw;
  } catch {
    fail(category);
  } finally {
    if (descriptor !== undefined) {
      try { closeSync(descriptor); } catch {}
    }
  }
}

function digestBytes(raw) {
  return createHash('sha256').update(raw).digest('hex');
}

function validateDigest(digest, category) {
  if (typeof digest !== 'string' || !SHA256_PATTERN.test(digest)) fail(category);
}

function digestsEqual(left, right) {
  return timingSafeEqual(Buffer.from(left, 'hex'), Buffer.from(right, 'hex'));
}

export function readReadonlyJson(file, expectedSha256) {
  const category = 'Invalid readonly JSON evidence';
  if (expectedSha256 !== undefined) validateDigest(expectedSha256, category);
  const raw = readReadonlyBytes(file, category);
  const sha256 = digestBytes(raw);
  if (expectedSha256 !== undefined && !digestsEqual(sha256, expectedSha256)) fail(category);

  let value;
  try {
    const text = new TextDecoder('utf-8', { fatal: true }).decode(raw);
    value = JSON.parse(text);
  } catch {
    fail(category);
  }
  return { value, raw, sha256 };
}

function serializeJson(value, category) {
  try {
    const text = JSON.stringify(value);
    if (typeof text !== 'string') fail(category);
    const raw = Buffer.from(`${text}\n`, 'utf8');
    const parsed = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(raw));
    return { raw, parsed };
  } catch {
    fail(category);
  }
}

function assertVacantCanonicalTarget(file, category) {
  assertCanonicalAbsolute(file, category);
  const parent = path.dirname(file);
  if (parent === file || path.basename(file) === '') fail(category);
  inspectCanonicalDirectory(parent, category);
  try {
    lstatSync(file);
  } catch (error) {
    if (error?.code === 'ENOENT') return parent;
    fail(category);
  }
  fail(category);
}

function syncDirectory(directory, category) {
  let descriptor;
  try {
    descriptor = openSync(directory, constants.O_RDONLY | (constants.O_DIRECTORY ?? 0));
    fsyncSync(descriptor);
  } catch {
    fail(category);
  } finally {
    if (descriptor !== undefined) {
      try { closeSync(descriptor); } catch {}
    }
  }
}

export function publishExclusiveJson(file, value) {
  const category = 'Unable to publish evidence exclusively';
  const parent = assertVacantCanonicalTarget(file, category);
  const { raw, parsed } = serializeJson(value, category);
  const sha256 = digestBytes(raw);
  const temporary = path.join(parent, `.${path.basename(file)}.${process.pid}.${randomUUID()}.tmp`);
  let descriptor;
  let temporaryExists = false;
  let published = false;
  try {
    descriptor = openSync(
      temporary,
      constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | (constants.O_NOFOLLOW ?? 0),
      0o400,
    );
    temporaryExists = true;
    writeFileSync(descriptor, raw);
    fsyncSync(descriptor);
    fchmodSync(descriptor, 0o444);
    fsyncSync(descriptor);
    const completed = fstatSync(descriptor);
    if (!completed.isFile() || (completed.mode & 0o777) !== 0o444 || completed.size !== raw.length) fail(category);
    closeSync(descriptor);
    descriptor = undefined;

    linkSync(temporary, file);
    published = true;
    unlinkSync(temporary);
    temporaryExists = false;
    syncDirectory(parent, category);
  } catch {
    fail(category);
  } finally {
    if (descriptor !== undefined) {
      try { closeSync(descriptor); } catch {}
    }
    if (temporaryExists) {
      try { unlinkSync(temporary); } catch {}
    }
  }

  if (!published) fail(category);
  const retained = readReadonlyJson(file, sha256);
  if (JSON.stringify(retained.value) !== JSON.stringify(parsed)) fail(category);
  return retained;
}

function canonicalProject(project, category) {
  inspectCanonicalDirectory(project, category);
  return project;
}

export function resolveProtectedCapture(project, relative) {
  const category = 'Invalid protected capture';
  canonicalProject(project, category);
  if (typeof relative !== 'string' || !UUID_V4_JSON_PATTERN.test(relative)) fail(category);
  const capture = path.join(project, ...relative.split('/'));
  inspectCanonicalReadonlyFile(capture, category);
  return capture;
}

function resolveProtectedFile(project, relative, category) {
  if (typeof relative !== 'string'
    || relative.length === 0
    || relative.includes('\\')
    || path.posix.isAbsolute(relative)
    || path.posix.normalize(relative) !== relative
    || relative.split('/').some((component) => component === '' || component === '.' || component === '..')) {
    fail(category);
  }
  const resolved = path.join(project, ...relative.split('/'));
  if (resolved === project || !resolved.startsWith(`${project}${path.sep}`)) fail(category);
  return resolved;
}

export function verifyProtectedFiles(project, map) {
  const category = 'Invalid protected files evidence';
  canonicalProject(project, category);
  if (!isRecord(map)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(map))) {
    fail(category);
  }
  const entries = Object.entries(map);
  if (entries.length === 0) fail(category);

  for (const [relative, expectedSha256] of entries) {
    validateDigest(expectedSha256, category);
    const file = resolveProtectedFile(project, relative, category);
    const actualSha256 = digestBytes(readReadonlyBytes(file, category));
    if (!digestsEqual(actualSha256, expectedSha256)) fail(category);
  }
}
