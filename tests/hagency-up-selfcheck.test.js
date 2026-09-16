import { describe, expect, test } from 'vitest';
import { execFile, execFileSync } from 'child_process';
import { chmodSync, mkdirSync, mkdtempSync, readdirSync, statSync, readFileSync, writeFileSync, rmSync } from 'fs';
import http from 'http';
import os from 'os';
import path from 'path';
import { promisify } from 'util';

const repoRoot = path.resolve('.');
const execFileAsync = promisify(execFile);

function shellQuote(value) {
  return `'${String(value).replaceAll("'", `'"'"'`)}'`;
}

function listen(handler) {
  return new Promise((resolve, reject) => {
    const server = http.createServer(handler);
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port }));
  });
}

/** Snapshot a directory tree: relative path → { bytes } (content hash by length+first/last bytes). */
function snap(dir) {
  const out = {};
  const walk = (rel) => {
    const abs = path.join(dir, rel);
    const st = statSync(abs);
    if (st.isDirectory()) {
      for (const e of readdirSync(abs)) walk(rel ? `${rel}/${e}` : e);
    } else {
      const b = readFileSync(abs);
      out[rel] = `${b.length}:${b.length ? b[0] : ''}:${b.length ? b[b.length - 1] : ''}`;
    }
  };
  walk('');
  return out;
}

describe('12-r2: --print-pane-target is zero-side-effect', () => {
  test('no runtime files created, no backend requests, output =<session>:1.1', async () => {
    const runtimeDir = mkdtempSync(path.join(os.tmpdir(), 'hagency-12r2-'));
    // Pre-create the runtime skeleton the script would use, so we can detect writes to it.
    const agentsDir = path.join(runtimeDir, 'agents');
    const tmpDir = path.join(runtimeDir, 'tmp');
    for (const d of [agentsDir, tmpDir]) {
      mkdirSync(d, { recursive: true });
    }
    const requests = [];
    const { server, port } = await listen((req, res) => {
      requests.push(`${req.method} ${req.url}`);
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end('{}');
    });

    let tmuxPath = null;
    const sock = `hagency-12r2-${process.pid}-${path.basename(runtimeDir)}`;
    try {
      // Isolated tmux server with base-index 1 + PATH shim for `tmux`.
      const conf = path.join(runtimeDir, 'tmux.conf');
      writeFileSync(conf, 'set -g base-index 1\nset -g pane-base-index 1\n');
      const session = `t12-${process.pid}`;
      const binDir = path.join(runtimeDir, 'bin');
      mkdirSync(binDir);
      tmuxPath = execFileSync('/usr/bin/env', ['sh', '-c', 'command -v tmux'], { encoding: 'utf8' }).trim();
      if (!path.isAbsolute(tmuxPath)) throw new Error(`tmux did not resolve to an absolute path: ${tmuxPath}`);
      writeFileSync(path.join(binDir, 'tmux'), `#!/usr/bin/env bash\nexec ${shellQuote(tmuxPath)} -L ${shellQuote(sock)} "$@"\n`);
      chmodSync(path.join(binDir, 'tmux'), 0o755);
      execFileSync(tmuxPath, ['-L', sock, '-f', conf, 'new-session', '-d', '-s', session, 'sleep 30']);
      const before = snap(runtimeDir);
      const { stdout, stderr } = await execFileAsync('/bin/bash', ['bin/hagency-up', '--print-pane-target', session], {
        cwd: repoRoot,
        encoding: 'utf8',
        timeout: 5_000,
        env: {
          ...process.env,
          PATH: `${binDir}:${process.env.PATH || ''}`,
          HAGENCY_INTERNAL_DISPATCH: '1',
          HAGENCY_RUNTIME_DIR: runtimeDir,
          HAGENCY_API: `http://127.0.0.1:${port}`,
        },
      });

      expect(stderr).not.toMatch(/Error|error/);
      expect(stdout.trim()).toBe(`=${session}:1.1`);

      // ZERO side effects: the runtime tree is byte-identical (same files, same fingerprints)
      const after = snap(runtimeDir);
      expect(Object.keys(after).sort()).toEqual(Object.keys(before).sort());
      for (const k of Object.keys(before)) expect(after[k]).toBe(before[k]);
      // and the backend saw NOTHING — no lifecycle, no registration, no heartbeat
      expect(requests).toEqual([]);
    } finally {
      if (tmuxPath) {
        try { execFileSync(tmuxPath, ['-L', sock, 'kill-server'], { stdio: 'ignore' }); } catch { /* already gone */ }
      }
      await new Promise(resolve => server.close(resolve));
      rmSync(runtimeDir, { recursive: true, force: true });
    }
  });
});
