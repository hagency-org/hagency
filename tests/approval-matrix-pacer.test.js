import { expect, test } from 'vitest';
import { ApprovalMatrixPacer } from '../lib/approval-matrix-pacer.js';

const flush = async () => { for (let i = 0; i < 10; i += 1) await Promise.resolve(); };

test('approval pacer delayed timers start one request and reschedule from actual admission', async () => {
  let now = 0; const timers = []; const starts = [];
  const pacer = new ApprovalMatrixPacer({ now: () => now,
    wait: ms => new Promise(resolve => timers.push({ ms, resolve })) });
  const jobs = [0, 1, 2].map(id => pacer.start(() => { starts.push({ id, at: now }); return id; }));
  await flush(); expect(starts).toEqual([{ id: 0, at: 0 }]);
  expect(timers[0].ms).toBe(200);
  now = 1000; timers.shift().resolve(); await flush();
  expect(starts).toEqual([{ id: 0, at: 0 }, { id: 1, at: 1000 }]);
  expect(timers[0].ms).toBe(200);
  now = 1200; timers.shift().resolve();
  expect(await Promise.all(jobs)).toEqual([0, 1, 2]);
  expect(starts[2]).toEqual({ id: 2, at: 1200 });
});

test('approval pacer releases admission while the started response remains owned', async () => {
  let now = 0; let finish; const starts = [];
  const pacer = new ApprovalMatrixPacer({ now: () => now, wait: async ms => { now += ms; } });
  const first = pacer.start(() => { starts.push(now); return new Promise(resolve => { finish = resolve; }); });
  const second = pacer.start(() => { starts.push(now); return 'second'; });
  expect(await second).toBe('second'); expect(starts).toEqual([0, 200]);
  finish('first'); expect(await first).toBe('first');
});

test('approval pacer canceled ticket cannot start or block later admissions', async () => {
  let now = 0; let wake; let allowed = true; const starts = [];
  const pacer = new ApprovalMatrixPacer({ now: () => now, wait: () => new Promise(resolve => { wake = resolve; }) });
  await pacer.start(() => starts.push('first'));
  const controller = new AbortController();
  const queued = pacer.start(() => starts.push('canceled'), { signal: controller.signal });
  const rejected = expect(queued).rejects.toThrow('stopped');
  await flush(); controller.abort(new Error('stopped')); await rejected;
  now = 200; wake(); await flush();
  const stale = pacer.start(() => starts.push('stale'), { check() { if (!allowed) throw new Error('rotated'); } });
  allowed = false; await expect(stale).rejects.toThrow('rotated');
  await pacer.start(() => starts.push('next'));
  expect(starts).toEqual(['first', 'next']);
});

test('approval pacer keeps a started response owned after admission signal aborts', async () => {
  const pacer = new ApprovalMatrixPacer(); const controller = new AbortController(); let finish;
  let settled = false;
  const result = pacer.start(() => new Promise(resolve => { finish = resolve; }), { signal: controller.signal });
  result.then(() => { settled = true; }, () => { settled = true; });
  await flush(); controller.abort(new Error('stopped after start')); await flush();
  expect(settled).toBe(false);
  finish({ event_id: '$known' });
  expect(await result).toEqual({ event_id: '$known' });
});
