import { closeSync, openSync, readSync } from 'node:fs';
import net from 'node:net';
import path from 'node:path';

const serviceNames = ['backend', 'dashboard', 'bridge', 'relay'];

// Retain both listeners through selection. Releasing the first listener before
// choosing the second allows the OS to return the same ephemeral port twice.
export async function withReservedServicePorts(usePorts) {
  const listeners = [];
  try {
    for (let index = 0; index < 2; index += 1) {
      const listener = net.createServer();
      listeners.push(listener);
      await new Promise((resolve, reject) => {
        listener.once('error', reject);
        listener.listen(0, '127.0.0.1', resolve);
      });
    }
    return await usePorts(listeners.map((listener) => listener.address().port));
  } finally {
    await Promise.all(listeners.map((listener) => new Promise((resolve, reject) => {
      if (!listener.listening) return resolve();
      listener.close((error) => error ? reject(error) : resolve());
    })));
  }
}

function childErrorCodes(logDir, name) {
  let fd;
  try {
    fd = openSync(path.join(logDir, `${name}.log`), 'r');
    const buffer = Buffer.alloc(2048);
    const bytes = readSync(fd, buffer, 0, buffer.length, 0);
    const text = buffer.toString('utf8', 0, bytes);
    return ['EADDRINUSE', 'EACCES', 'ENOENT', 'MODULE_NOT_FOUND']
      .filter((code) => text.includes(code));
  } catch {
    return [];
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

const integerOrNull = (value) => Number.isSafeInteger(value) && value >= 0 ? value : null;

// Failure-only evidence: never serialize records, arbitrary event keys, env or
// raw child logs. The sample and each fixed service's log read are bounded.
export function startupEvidence(supervisor, rows) {
  return {
    events: {
      total: rows.length,
      omitted: Math.max(0, rows.length - 16),
      sample: rows.slice(0, 16).map((row) => ({
        name: serviceNames.includes(row?.name) ? row.name : 'unknown',
        event: ['ready', 'stopped'].includes(row?.event) ? row.event : 'unknown',
        pid: integerOrNull(row?.pid),
      })),
    },
    services: serviceNames.map((name) => {
      const record = supervisor.records.get(name);
      const signal = record?.lastExit?.signal;
      return {
        name,
        pid: integerOrNull(record?.pid),
        restarts: integerOrNull(record?.restarts),
        exitCode: integerOrNull(record?.lastExit?.code),
        exitSignal: ['SIGINT', 'SIGTERM', 'SIGKILL'].includes(signal) ? signal : null,
        childErrorCodes: childErrorCodes(supervisor.logDir, name),
      };
    }),
  };
}
