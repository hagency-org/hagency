import { afterEach, describe, expect, test } from 'vitest';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'fs';
import { spawn } from 'node:child_process';
import http from 'node:http';
import net from 'net';
import os from 'os';
import path from 'path';

import {
  LocalServiceSupervisor,
  diagnoseServices,
  readServiceStatus,
} from '../src/local-service-supervisor.mjs';
import { getProcessStartIdentity } from '../src/process-identity.mjs';
import { startupEvidence, withReservedServicePorts } from './fixtures/local-service-fixture.mjs';

const repoRoot = path.resolve('.');
const fixtureScript = 'tests/fixtures/service-child.mjs';
const supervisors = [];
const runtimes = [];
const children = [];
const servers = [];

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

async function bindOnce(port) {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', reject);
    server.listen(port, '127.0.0.1', () => {
      server.close((error) => error ? reject(error) : resolve());
    });
  });
}

async function fixtureContext() {
  const [backendPort, dashboardPort] = await withReservedServicePorts((ports) => ports);
  const runtimeRoot = mkdtempSync(path.join(os.tmpdir(), 'hagency-services-runtime-'));
  runtimes.push(runtimeRoot);
  const eventLog = path.join(runtimeRoot, 'events.jsonl');
  const service = (name, dependsOn, health, extraEnv = {}) => ({
    name,
    command: ['node', fixtureScript],
    dependsOn,
    health,
    env: {
      SERVICE_CHILD_NAME: name,
      SERVICE_EVENT_LOG: eventLog,
      ...extraEnv,
    },
  });
  const profile = {
    name: 'services-local',
    services: [
      service('backend', [], {
        type: 'http', host: '127.0.0.1', defaultPort: backendPort, path: '/health', timeoutMs: 300,
      }, { SERVICE_CHILD_PORT: String(backendPort) }),
      service('dashboard', ['backend'], {
        type: 'tcp', host: '127.0.0.1', defaultPort: dashboardPort, timeoutMs: 300,
      }, { SERVICE_CHILD_PORT: String(dashboardPort) }),
      service('bridge', ['backend'], { type: 'process', timeoutMs: 300 }),
      service('relay', ['backend'], { type: 'process', timeoutMs: 300 }),
    ],
  };
  const supervisor = new LocalServiceSupervisor({
    profile,
    repoRoot,
    runtimeRoot,
    env: { ...process.env, API_TOKEN: 'must-not-appear-in-state' },
    restartDelayMs: 40,
    dependencyTimeoutMs: 3000,
  });
  supervisors.push(supervisor);
  return { supervisor, profile, runtimeRoot, eventLog };
}

async function waitFor(predicate, timeoutMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = await predicate();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`condition not met within ${timeoutMs}ms`);
}

async function waitForStartupEvents(supervisor, eventLog, timeoutMs = 3000) {
  let rows = [];
  try {
    return await waitFor(() => {
      rows = readFileSync(eventLog, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse);
      return rows.length === 4 ? rows : null;
    }, timeoutMs);
  } catch (error) {
    const reason = error.message === `condition not met within ${timeoutMs}ms`
      ? error.message : 'event log could not be read or parsed';
    throw new Error(`${reason}; startup evidence: ${JSON.stringify(startupEvidence(supervisor, rows))}`);
  }
}

afterEach(async () => {
  for (const supervisor of supervisors.splice(0).reverse()) {
    await supervisor.stop().catch(() => {});
  }
  for (const runtime of runtimes.splice(0)) rmSync(runtime, { recursive: true, force: true });
  for (const child of children.splice(0)) {
    try { child.kill('SIGKILL'); } catch {}
  }
  await Promise.all(servers.splice(0).map((server) => new Promise((resolve) => server.close(resolve))));
});

describe('LocalServiceSupervisor', () => {
  test('reserves two distinct service ports until the reservation scope ends', async () => {
    const ports = await withReservedServicePorts(async (reserved) => {
      expect(new Set(reserved).size).toBe(2);
      for (const port of reserved) {
        await expect(bindOnce(port)).rejects.toMatchObject({ code: 'EADDRINUSE' });
      }
      return reserved;
    });
    for (const port of ports) await bindOnce(port);
  });

  test('releases both service port reservations when fixture construction fails', async () => {
    let ports;
    const failure = new Error('controlled fixture construction failure');
    await expect(withReservedServicePorts(async (reserved) => {
      ports = reserved;
      for (const port of ports) {
        await expect(bindOnce(port)).rejects.toMatchObject({ code: 'EADDRINUSE' });
      }
      throw failure;
    })).rejects.toBe(failure);
    expect(ports).toHaveLength(2);
    for (const port of ports) await bindOnce(port);
  });

  test('starts all four services in dependency order and reports healthy', async () => {
    const { supervisor, eventLog } = await fixtureContext();
    await supervisor.start();
    const status = await supervisor.waitForHealthy(3000);

    expect(status.ok).toBe(true);
    expect(status.services.map((service) => service.name)).toEqual([
      'backend', 'dashboard', 'bridge', 'relay',
    ]);
    expect(status.services.every((service) => service.healthy && service.pid > 0)).toBe(true);

    const events = await waitForStartupEvents(supervisor, eventLog);
    // Startup awaits each configured probe, but process probes can observe a
    // wrapper before its fixture child writes ready. Backend readiness must
    // precede dependants; bridge/relay fixture readiness can still race.
    expect(events[0]).toMatchObject({ name: 'backend', event: 'ready' });
    expect(events.slice(1).map((event) => event.name).sort())
      .toEqual(['bridge', 'dashboard', 'relay']);
  });

  test('preserves extra stopped events and restart evidence in startup failures', async () => {
    const { supervisor, eventLog } = await fixtureContext();
    await supervisor.start();
    await waitForStartupEvents(supervisor, eventLog);
    await supervisor.stopService('relay', { restart: false });

    // A fifth real event remains a failure; do not filter stopped/restarted
    // children or change the normal startup deadline to hide that evidence.
    const failure = await waitForStartupEvents(supervisor, eventLog, 100)
      .then(() => null, (error) => error);
    expect(failure).toBeInstanceOf(Error);
    expect(failure.message).toContain('condition not met within 100ms');
    const evidence = JSON.parse(failure.message.split('; startup evidence: ')[1]);
    expect(evidence.events.total).toBe(5);
    expect(evidence.events.sample).toContainEqual(expect.objectContaining({
      name: 'relay', event: 'stopped',
    }));
    expect(evidence.services.find((service) => service.name === 'relay'))
      .toMatchObject({ restarts: 0, pid: null });
  });

  test('bounds startup diagnostics without exposing arbitrary event or log fields', () => {
    const runtimeRoot = mkdtempSync(path.join(os.tmpdir(), 'hagency-services-evidence-'));
    runtimes.push(runtimeRoot);
    const secret = 'must-not-appear-in-diagnostic';
    writeFileSync(path.join(runtimeRoot, 'dashboard.log'), `${secret}: EADDRINUSE\n${'x'.repeat(4096)}ENOENT`);
    const evidence = startupEvidence({
      logDir: runtimeRoot,
      records: new Map([['dashboard', {
        pid: 12, restarts: 6, lastExit: { code: 1 }, env: { API_TOKEN: secret },
      }]]),
    }, Array.from({ length: 100 }, () => ({
      name: 'dashboard', event: 'ready', pid: 11, body: secret, workspace: runtimeRoot,
    })));
    expect(evidence.events).toMatchObject({ total: 100, omitted: 84 });
    expect(evidence.events.sample).toHaveLength(16);
    expect(evidence.services).toHaveLength(4);
    expect(evidence.services.find((service) => service.name === 'dashboard'))
      .toMatchObject({ restarts: 6, exitCode: 1, childErrorCodes: ['EADDRINUSE'] });
    const text = JSON.stringify(evidence);
    expect(text.length).toBeLessThan(2048);
    expect(text).not.toContain(secret);
    expect(text).not.toContain(runtimeRoot);
    expect(startupEvidence({ logDir: runtimeRoot, records: new Map() }, [null, {
      name: secret, event: secret, pid: secret,
    }]).events.sample).toEqual([
      { name: 'unknown', event: 'unknown', pid: null },
      { name: 'unknown', event: 'unknown', pid: null },
    ]);
  });

  test('automatically restarts a crashed relay exactly once', async () => {
    const { supervisor, eventLog } = await fixtureContext();
    await supervisor.start();
    await supervisor.waitForHealthy(3000);
    const oldPid = supervisor.getServicePid('relay');
    const oldServicePid = await waitFor(() => {
      const rows = readFileSync(eventLog, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse);
      return rows.find((event) => event.name === 'relay' && event.event === 'ready')?.pid || null;
    });

    try {
      process.kill(oldPid, 'SIGKILL');
      const restarted = await waitFor(async () => {
        const status = await supervisor.status();
        const relay = status.services.find((service) => service.name === 'relay');
        return relay?.healthy && relay.pid !== oldPid && relay.restarts === 1 ? relay : null;
      });

      expect(restarted.pid).not.toBe(oldPid);
      await waitFor(() => {
        try { process.kill(oldServicePid, 0); return false; } catch { return true; }
      });
    } finally {
      try { process.kill(oldServicePid, 'SIGKILL'); } catch {}
    }
  });

  test('doctor names an explicitly stopped bridge', async () => {
    const { supervisor, profile, runtimeRoot } = await fixtureContext();
    await supervisor.start();
    await supervisor.waitForHealthy(3000);
    await supervisor.stopService('bridge', { restart: false });

    const diagnosis = await diagnoseServices({ profile, runtimeRoot, env: process.env });
    expect(diagnosis.ok).toBe(false);
    expect(diagnosis.failures).toContainEqual(expect.objectContaining({
      name: 'bridge',
      cause: expect.stringMatching(/stopped|process/i),
    }));
  });

  test('status reports a crashed service within five seconds', async () => {
    const { supervisor, profile, runtimeRoot } = await fixtureContext();
    await supervisor.start();
    await supervisor.waitForHealthy(3000);
    await supervisor.stopService('relay', { restart: false });

    const startedAt = Date.now();
    const status = await readServiceStatus({ profile, runtimeRoot, env: process.env });
    expect(Date.now() - startedAt).toBeLessThan(5000);
    expect(status.services.find((service) => service.name === 'relay')).toMatchObject({
      healthy: false,
      desired: 'stopped',
    });
  });

  test('writes atomic redacted state snapshots', async () => {
    const { supervisor, runtimeRoot } = await fixtureContext();
    await supervisor.start();
    await supervisor.waitForHealthy(3000);

    const stateDir = path.join(runtimeRoot, 'data', 'services-local');
    const stateText = readFileSync(path.join(stateDir, 'state.json'), 'utf8');
    expect(JSON.parse(stateText).services).toHaveLength(4);
    expect(stateText).not.toContain('must-not-appear-in-state');
    expect(readdirSync(stateDir).some((name) => name.includes('.tmp'))).toBe(false);
  });

  test('offline status rejects a live PID whose command does not match the service script', async () => {
    const runtimeRoot = mkdtempSync(path.join(os.tmpdir(), 'hagency-services-pid-reuse-'));
    runtimes.push(runtimeRoot);
    const stateDir = path.join(runtimeRoot, 'data', 'services-local');
    mkdirSync(stateDir, { recursive: true });
    const profile = {
      name: 'services-local',
      services: ['backend', 'dashboard', 'bridge', 'relay'].map((name) => ({
        name,
        command: ['node', fixtureScript],
        dependsOn: [],
        env: {},
        health: { type: 'process', timeoutMs: 300 },
      })),
    };
    writeFileSync(path.join(stateDir, 'state.json'), `${JSON.stringify({
      schemaVersion: 1,
      supervisor: {
        pid: process.pid,
        processStartIdentity: getProcessStartIdentity(process.pid),
      },
      services: profile.services.map(({ name }) => ({
        name,
        pid: process.pid,
        processStartIdentity: getProcessStartIdentity(process.pid),
        desired: 'running',
        restarts: 0,
        startedAt: new Date(Date.now() - 1000).toISOString(),
        startedAtMs: Date.now() - 1000,
      })),
    })}\n`);

    const status = await readServiceStatus({ profile, runtimeRoot, env: process.env });
    expect(status.ok).toBe(false);
    expect(status.services.every((service) => !service.healthy)).toBe(true);
    expect(status.services[0].reason).toMatch(/command|pid/i);
  });

  test('offline status rejects a matching command with a stale process identity', async () => {
    const runtimeRoot = mkdtempSync(path.join(os.tmpdir(), 'hagency-services-service-identity-'));
    runtimes.push(runtimeRoot);
    const stateDir = path.join(runtimeRoot, 'data', 'services-local');
    mkdirSync(stateDir, { recursive: true });
    const child = spawn(process.execPath, [fixtureScript, '--identity-test'], {
      cwd: repoRoot,
      stdio: 'ignore',
    });
    children.push(child);
    await waitFor(() => {
      try { process.kill(child.pid, 0); return true; } catch { return false; }
    });
    const profile = {
      name: 'services-local',
      services: ['backend', 'dashboard', 'bridge', 'relay'].map((name) => ({
        name,
        command: ['node', fixtureScript],
        dependsOn: [],
        env: {},
        health: { type: 'process', timeoutMs: 300 },
      })),
    };
    writeFileSync(path.join(stateDir, 'state.json'), `${JSON.stringify({
      supervisor: {
        pid: process.pid,
        processStartIdentity: getProcessStartIdentity(process.pid),
      },
      services: profile.services.map(({ name }) => ({
        name,
        pid: child.pid,
        processStartIdentity: 'stale-process-identity',
        desired: 'running',
        startedAtMs: Date.now() - 1000,
      })),
    })}\n`);

    const status = await readServiceStatus({ profile, runtimeRoot, env: process.env });
    expect(status.ok).toBe(false);
    expect(status.services.every((service) => !service.healthy)).toBe(true);
    expect(status.services[0].reason).toMatch(/identity/i);
  });

  test('offline status checks four slow probes concurrently within five seconds', async () => {
    const runtimeRoot = mkdtempSync(path.join(os.tmpdir(), 'hagency-services-bounded-status-'));
    runtimes.push(runtimeRoot);
    const stateDir = path.join(runtimeRoot, 'data', 'services-local');
    mkdirSync(stateDir, { recursive: true });
    const profile = { name: 'bounded-status', services: [] };
    const records = [];
    for (const name of ['backend', 'dashboard', 'bridge', 'relay']) {
      const server = http.createServer(() => {});
      servers.push(server);
      await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
      const child = spawn(process.execPath, [fixtureScript, '--status-probe', name], {
        cwd: repoRoot,
        stdio: 'ignore',
      });
      children.push(child);
      await waitFor(() => {
        try { process.kill(child.pid, 0); return true; } catch { return false; }
      });
      profile.services.push({
        name,
        command: ['node', fixtureScript],
        dependsOn: [],
        env: {},
        health: {
          type: 'http', host: '127.0.0.1', defaultPort: server.address().port,
          path: '/health', timeoutMs: 1500,
        },
      });
      records.push({
        name,
        pid: child.pid,
        processStartIdentity: getProcessStartIdentity(child.pid),
        desired: 'running',
        restarts: 0,
        startedAt: new Date(Date.now() - 1000).toISOString(),
        startedAtMs: Date.now() - 1000,
      });
    }
    writeFileSync(path.join(stateDir, 'state.json'), `${JSON.stringify({
      supervisor: {
        pid: process.pid,
        processStartIdentity: getProcessStartIdentity(process.pid),
      },
      services: records,
    })}\n`);

    const startedAt = Date.now();
    const status = await readServiceStatus({ profile, runtimeRoot, env: process.env });
    expect(Date.now() - startedAt).toBeLessThan(5000);
    expect(status.ok).toBe(false);
    expect(status.services.every((service) => !service.healthy)).toBe(true);
  });
});
