import { describe, expect, test, vi } from 'vitest';
import { publishApprovalProjectionForTest } from '../bridge-matrix.js';

const row = {
  request_id: 'approval_0123456789abcdef0123456789abcdef', revision: 1,
  channel: 'private_request', target_room_id: '!dm:test', cas_token: 'row-cas',
};
const actor = {
  scope: 'local_bot', publisher_mxid: '@bot:test', homeserver: 'test',
  credential_kind: 'local_bot', credential_generation: 'generation-1',
};
const winningPlan = {
  publisher_scope: actor.scope, publisher_mxid: actor.publisher_mxid,
  credential_generation: actor.credential_generation, prepared_event_type: 'm.room.encrypted',
  prepared_payload: { ciphertext: 'winning' }, transaction_id: 'hafleet_final', cas_token: 'plan-cas',
};

function harness(overrides = {}) {
  return {
    resolveActor: vi.fn().mockResolvedValue(actor),
    prepareContent: vi.fn().mockResolvedValue({ event_type: 'm.room.encrypted', content: { ciphertext: 'candidate' } }),
    registerPublisher: vi.fn().mockResolvedValue(undefined),
    prepare: vi.fn().mockResolvedValue({ plan: winningPlan }),
    begin: vi.fn().mockResolvedValue({ plan: { ...winningPlan, attempt_state: 'attempted' } }),
    send: vi.fn().mockResolvedValue('$event'),
    receipt: vi.fn().mockResolvedValue({ event_id: '$event' }),
    retry: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

describe('approval projection publish checkpoint', () => {
  test('prepares once and sends only the winning durable bytes and transaction identity', async () => {
    const io = harness();
    await expect(publishApprovalProjectionForTest(row, io)).resolves.toEqual({ ok: true, event_id: '$event' });
    expect(io.prepareContent).toHaveBeenCalledTimes(1);
    expect(io.send).toHaveBeenCalledWith(expect.objectContaining({
      transaction_id: 'hafleet_final', prepared_event_type: 'm.room.encrypted',
      prepared_payload: { ciphertext: 'winning' },
    }), actor, row);
    expect(io.receipt).toHaveBeenCalledWith(expect.objectContaining({ cas_token: 'plan-cas' }), '$event', row);
  });

  test('rechecks the actual actor after every preparation await and immediately before network', async () => {
    const rotated = { ...actor, credential_generation: 'generation-2' };
    const io = harness({ resolveActor: vi.fn()
      .mockResolvedValueOnce(actor)
      .mockResolvedValueOnce(actor)
      .mockResolvedValueOnce(actor)
      .mockResolvedValueOnce(actor)
      .mockResolvedValueOnce(rotated) });
    await expect(publishApprovalProjectionForTest(row, io)).resolves.toMatchObject({ ok: false, uncertain: true });
    expect(io.send).not.toHaveBeenCalled();
    expect(io.retry).toHaveBeenCalledTimes(1);
  });

  test('an ambiguous send failure records retry against the immutable attempted plan', async () => {
    const io = harness({ send: vi.fn().mockRejectedValue(new Error('connection reset')) });
    await expect(publishApprovalProjectionForTest(row, io)).resolves.toMatchObject({ ok: false, uncertain: true });
    expect(io.retry).toHaveBeenCalledWith(expect.objectContaining({
      cas_token: 'plan-cas', transaction_id: 'hafleet_final',
      credential_generation: 'generation-1',
    }), expect.any(Error), row);
    expect(io.prepareContent).toHaveBeenCalledTimes(1);
  });

  test('a failed durable begin performs no Matrix send', async () => {
    const io = harness({ begin: vi.fn().mockRejectedValue(new Error('pre-rename persistence failed')) });
    await expect(publishApprovalProjectionForTest(row, io)).rejects.toThrow(/persistence failed/);
    expect(io.send).not.toHaveBeenCalled();
    expect(io.retry).not.toHaveBeenCalled();
  });

  test('an uncertain durable plan replays stored ciphertext without preparing content again', async () => {
    const io = harness({ prepareContent: vi.fn().mockRejectedValue(new Error('crypto temporarily unavailable')) });
    const uncertain = { ...row, plan: { ...winningPlan, attempt_state: 'uncertain' } };
    await expect(publishApprovalProjectionForTest(uncertain, io)).resolves.toEqual({ ok: true, event_id: '$event' });
    expect(io.prepareContent).not.toHaveBeenCalled();
    expect(io.prepare).not.toHaveBeenCalled();
    expect(io.send).toHaveBeenCalledWith(uncertain.plan, actor, uncertain);
  });
});
