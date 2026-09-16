import { performance } from 'node:perf_hooks';
import { setTimeout as sleep } from 'node:timers/promises';

// Admission is serialized; the response Promise remains owned by the caller's
// existing worker slot. This queue does not become a second I/O concurrency pool.
export class ApprovalMatrixPacer {
  constructor({ gapMs = 200, now = () => performance.now(), wait = (ms, signal) => sleep(ms, undefined, { signal }) } = {}) {
    this.gapMs = Math.max(0, gapMs);
    this.now = now;
    this.wait = wait;
    this.nextAt = 0;
    this.tail = Promise.resolve();
  }

  start(request, { check = () => {}, signal } = {}) {
    let resolve; let reject;
    const result = new Promise((yes, no) => { resolve = yes; reject = no; });
    const abort = () => reject(signal.reason);
    if (signal?.aborted) return Promise.reject(signal.reason);
    signal?.addEventListener('abort', abort, { once: true });
    this.tail = this.tail.then(async () => {
      const validate = () => { signal?.throwIfAborted(); check(); };
      validate();
      while (this.nextAt > this.now()) {
        await this.wait(this.nextAt - this.now(), signal);
        validate();
      }
      validate();
      // Use actual admission time, never accumulated nominal slots. Late timers
      // can start one request, but cannot release a catch-up burst.
      this.nextAt = this.now() + this.gapMs;
      signal?.removeEventListener('abort', abort);
      resolve(request());
    }).catch(reject);
    return result.finally(() => signal?.removeEventListener('abort', abort));
  }
}
