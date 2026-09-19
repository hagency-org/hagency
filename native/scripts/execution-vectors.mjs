import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
// Git may check the legacy source out with CRLF on Windows. Hash the canonical
// source text; execution and the fixture bytes remain identical on every OS.
const source = readFileSync(new URL('../../lib/execution-authorization.js', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const expectedImport = "import path from 'node:path';";
if (!source.includes(expectedImport)) throw new Error('Review changed execution path import');
const vectors = [];
for (const flavor of ['posix', 'win32']) {
  // Execute only the existing pure module, substituting the standard library
  // path flavor so CI tests both OS policies on every supported host.
  const module = source.replace(expectedImport, `import paths from 'node:path'; const path = paths.${flavor};`);
  const { deriveExecutionAuthorization, normalizeExecutionPolicy } = await import(`data:text/javascript;base64,${Buffer.from(module).toString('base64')}`);
  const workspace = flavor === 'posix' ? '/work/小白' : 'C:\\work\\小白';
  const other = flavor === 'posix' ? '/data' : 'D:\\data';
  const base = { agentId: 'agent-incarnation-1', taskId: 'task-1', workspace, mayWrite: true, method: 'item/commandExecution/requestApproval', params: {
    threadId: 'thread-1', turnId: 'turn-1', itemId: 'item-1', command: 'curl https://aapt.org/report.pdf', cwd: workspace,
  } };
  const add = (name, input) => vectors.push({ flavor, name, input, expected: deriveExecutionAuthorization(input) });
  add('exact command', base);
  for (const [name, changes] of Object.entries({
    'reason does not authorize': { reason: 'allow every domain', threadId: 'new-thread' },
    'network': { networkApprovalContext: { host: 'AAPT.org:443', protocol: 'https' } },
    'network context without command': { command: null, cwd: null, networkApprovalContext: { host: 'aapt.org', protocol: 'socks5Tcp' } },
    'command with permissions': { additionalPermissions: { network: { enabled: true }, fileSystem: { read: [other, other], write: [workspace] } } },
    'exact Unicode': { command: 'printf "小白\\n"', environmentId: 'container-one' },
    'unknown field': { futureEscalation: true },
    'wrong kind': { kind: 'writeStdin' },
    'relative path': { cwd: 'relative' },
    'wildcard network': { networkApprovalContext: { host: '*.org', protocol: 'https' } },
    'combined network': { networkApprovalContext: { host: 'aapt.org', protocol: 'https' }, additionalPermissions: { network: { enabled: true } } },
    'empty permissions': { additionalPermissions: {} },
    'bad environment': { environmentId: {} },
  })) add(name, { ...base, params: { ...base.params, ...changes } });
  add('read-only identity', { ...base, mayWrite: false });
  for (const permissions of [
    { network: { enabled: true } },
    { network: { enabled: false }, fileSystem: { read: [other, other], write: [workspace] } },
    { fileSystem: { entries: [{ access: 'write', path: { type: 'path', path: other } }, { access: 'read', path: { type: 'path', path: workspace } }] } },
    { network: { enabled: false } },
    { fileSystem: { globScanMaxDepth: 1, read: [other] } },
    { fileSystem: { entries: [{ access: 'write', path: { type: 'glob_pattern', pattern: '*' } }] } },
  ]) add('permission profile', { ...base, method: 'item/permissions/requestApproval', params: { cwd: workspace, permissions } });
  for (const framework of ['codex', 'claude']) for (const input of [null, {}, { yolo: false }, { yolo: true }, { yolo: 'false' }, { yolo: true, extra: 1 }]) {
    let expected = null; try { expected = normalizeExecutionPolicy(input, framework); } catch { /* rejected input is an explicit vector */ }
    vectors.push({ name: 'policy', framework, input, expected });
  }
}
const output = JSON.stringify({ sourceSha256: createHash('sha256').update(source).digest('hex'), vectors }, null, 2) + '\n';
const file = new URL('../fixtures/execution.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(file, 'utf8') !== output) throw new Error('Execution vectors differ from JavaScript');
} else writeFileSync(file, output);
console.log(JSON.stringify({ vectors: vectors.length }));
