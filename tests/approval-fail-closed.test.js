/*
 * "Two surfaces or nothing" — the fail-closed path when a Matrix surface cannot be delivered.
 *
 * ADR-003: every remote-execution approval uses BOTH the encrypted owner DM and the redacted
 * public notice, or neither. The "or neither" half is what happens when delivery fails, and it
 * had no coverage of its own end to end. `denyPending` (store) has unit tests now; this file
 * pins the explicit `delivery-failed` endpoint and the bridge boundary that now wakes the durable
 * projection worker without turning a transient publication failure into a verdict.
 *
 * Why it matters: a request left `pending` after a failed delivery is not benign. The owner
 * never saw the approve/deny buttons, so no verdict can ever arrive — the request sits until it
 * expires. The canonical projection outbox now owns retries and receipts, so the old direct-send
 * handler must not invoke this endpoint on a transient Matrix failure.
 */

import { afterAll, afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import request from 'supertest';
import { mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createBackendTestContext } from './helpers/backend-test-runtime.js';

const BRIDGE_SECRET = 'test-bridge-secret';

const seedApproval = () => ({
  env: { MATRIX_BRIDGE_SECRET: BRIDGE_SECRET },
});

/*
 * Create a pending approval through the same door the bridge uses, so the record under test is
 * the real thing the endpoint operates on rather than a hand-written fixture. The binding is
 * required first (owner resolution), then the request lands pending.
 */
async function createPending(ctx) {
  await request(ctx.app).put('/api/approval-bindings')
    .set('X-Bridge-Secret', BRIDGE_SECRET)
    .send({
      agent: 'wf_coordinator',
      project: 'robrix2',
      project_room_id: '!project:hq.example',
      owner_mxid: '@alex:hq.example',
      owner_dm_room_id: '!owner-dm:hq.example',
    });
  const created = await request(ctx.app).post('/api/approvals')
    .set('X-Bridge-Secret', BRIDGE_SECRET)
    .send({
      agent: 'wf_coordinator',
      runtime: 'claude',
      project: 'robrix2',
      upstream_request_id: 'u-1',
      tool_name: 'Bash',
      description: 'Create a GitHub issue',
      input_preview: '{"command":"gh issue create"}',
    });
  // 201 Created for a fresh pending request; 200 only on an idempotent replay.
  expect(created.status).toBe(201);
  return created.body.approval;
}

describe('POST /api/approvals/:id/delivery-failed', () => {
  let ctx;
  afterEach(async () => { await ctx?.cleanup?.(); ctx = null; });

  test('denies a pending request and broadcasts the verdict', async () => {
    ctx = await createBackendTestContext('approval-failclosed-', seedApproval());
    const approval = await createPending(ctx);
    expect(approval.status).toBe('pending');

    const res = await request(ctx.app).post(`/api/approvals/${approval.id}/delivery-failed`)
      .set('X-Bridge-Secret', BRIDGE_SECRET)
      .send({ reason: 'matrix_delivery_failed' });

    expect(res.status).toBe(200);
    expect(res.body.approval).toMatchObject({ status: 'denied', denial_reason: 'matrix_delivery_failed' });

    // And it is actually denied in the store, not merely reported so — the queue must not still
    // show it awaiting the owner.
    // Read back through the bridge-secret /matrix variant — the plain GET is agent-token
    // guarded, and this test holds the bridge secret, not an agent token.
    const after = await request(ctx.app).get(`/api/approvals/${approval.id}/matrix`)
      .set('X-Bridge-Secret', BRIDGE_SECRET);
    expect(after.body.approval.status).toBe('denied');
  });

  test('requires the bridge secret — a project cannot deny its own pending approval', async () => {
    /*
     * The endpoint fires when the bridge could not deliver. Letting an unauthenticated caller
     * reach it would let anyone in a room force-deny a request that was about to be approved —
     * a denial-of-service on the owner's decision.
     */
    ctx = await createBackendTestContext('approval-failclosed-auth-', seedApproval());
    const approval = await createPending(ctx);

    const res = await request(ctx.app).post(`/api/approvals/${approval.id}/delivery-failed`)
      .send({ reason: 'matrix_delivery_failed' });

    expect(res.status).toBe(403);
    const after = await request(ctx.app).get(`/api/approvals/${approval.id}/matrix`)
      .set('X-Bridge-Secret', BRIDGE_SECRET);
    // Untouched: the unauthenticated deny attempt must not have moved it off pending.
    expect(after.body.approval.status).toBe('pending');
  });

  test('an unknown id is 404, not a silently created denial', async () => {
    ctx = await createBackendTestContext('approval-failclosed-404-', seedApproval());
    const res = await request(ctx.app).post('/api/approvals/$nope/delivery-failed')
      .set('X-Bridge-Secret', BRIDGE_SECRET)
      .send({ reason: 'matrix_delivery_failed' });
    expect(res.status).toBe(404);
  });
});

describe('bridge onApprovalRequested — canonical worker wake', () => {
  test('queues durable projection work without direct sends or delivery-failed denial', async () => {
    const { MatrixBridge } = await import('../bridge-matrix.js');
    const bridge = new MatrixBridge();
    bridge.wakeApprovalProjectionWorker = vi.fn(async () => ({ ok: true }));
    bridge.callBackendApi = vi.fn();
    bridge.botClient = { sendMessage: vi.fn() };
    bridge.sendAsAgentContent = vi.fn();

    await expect(bridge.onApprovalRequested({ request_id: '$approval-1' }))
      .resolves.toEqual({ ok: true, requestId: '$approval-1', queued: true });
    expect(bridge.wakeApprovalProjectionWorker).toHaveBeenCalledOnce();
    expect(bridge.callBackendApi).not.toHaveBeenCalled();
    expect(bridge.botClient.sendMessage).not.toHaveBeenCalled();
    expect(bridge.sendAsAgentContent).not.toHaveBeenCalled();
  });
});
