import express from 'express';
import { resolveDirectAdmission } from './lib/matrix-direct-admission.js';
import { hasOtherAgentAllocation, retirePalpoAgent } from './lib/palpo-agent-retirement.js';
import {
  describeMatrixReach, diagnoseEdgeInbound, discoverBaseUrl, originFor, probeHomeserver,
  verifyCallbackFromHomeserver,
} from './lib/matrix-candidates.js';
import { buildReplyHint } from './lib/reply-hint.js';
import { meterFleet } from './lib/metering/reader.js';
import { meteringSupport, CEILING_KINDS } from './lib/metering/parsers.js';
import { createUsageLedger } from './lib/metering/ledger.js';
import { createPendingInviteStore, PendingInviteError } from './lib/pending-invite-store.js';
import {
  appendFileSync,
  chmodSync,
  closeSync,
  copyFileSync,
  existsSync,
  fsyncSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  truncateSync,
  unlinkSync,
  writeFileSync,
} from 'fs';
import { readFile as readFileAsync } from 'fs/promises';
import { homedir, hostname } from 'os';
import { execFile, execFileSync, execSync, spawn } from 'child_process';
import path from 'path';
import { createHash, randomBytes, timingSafeEqual } from 'crypto';
import { fileURLToPath } from 'url';
import { promisify } from 'util';

import { BLOCK_PATTERNS as LOCAL_BLOCK_PATTERNS, BLOCK_TIER_HARD, BLOCK_TIER_SOFT, BLOCK_TIER_TRANSIENT } from './lib/blocked-patterns.js';
import { createTmuxRuntime } from './lib/runtime/tmux.js';
import { sessionPolicyFromEnv } from './lib/session-policy.js';
import { getFramework, listFrameworks } from './lib/frameworks/index.js';
import roleCapacity from './lib/role-capacity.json' with { type: 'json' };
import { buildSeats, normalizeDeclaration, seatIdentity } from './lib/seat-store.js';
import { holdsAllocation, resourceAllocationBudget, usesResourcePool } from './lib/resource-allocation-budget.js';
import { createEngagementStore, routeRequest, EngagementError, overCommitMessage } from './lib/engagement-store.js';
import { createResourceAgentDefinitions } from './lib/resource-agent-definitions.js';
import { normalizeExecutionPolicy } from './lib/execution-authorization.js';
import { createMatrixWorkStore, awaitMatrixWork } from './lib/matrix-work-store.js';
import { engagementApprovalContent } from './lib/engagement-notice.js';
import { fleetIdForSender, fleetRequestContext, fleetRequestKey, verifyFleetTarget } from './lib/fleet-protocol.js';
import { publicResourceId, projectAgentRuntimeName } from './lib/project-agent-definition.js';
import { projectSideAgentPrefix } from './lib/matrix-agent-identity.js';
import { ProjectSideStore, ProjectSideStoreError } from './lib/project-side-store.js';
import { inboundCredentialsProjection, derivedRegistrationId } from './lib/project-side-inbound.js';
import {
  canRepresentativeInvite, ensureRepresentative, inviteToRoomOnSide, joinRoomOnSideAsAgent, joinedMembersOnSide,
  leaveRoomOnSideAsAgent, leaveRoomOnSideAsRepresentative,
  knockOnRoomOnSide, mintAgentIdentity, probeFederationFromSide, resolveAliasOnSide,
} from './lib/matrix-representative.js';
import {
  dropQueuedBackendNotificationsBySource,
  enqueueFromRequestBody,
  initDeliveryQueue,
  installDeliveryQueueRoutes,
  resetDeliveryQueueHooks,
  startDeliveryQueueLoops,
  stopDeliveryQueueLoops,
} from './lib/delivery-queue.js';
import { generateRegistration, renderRegistrationYaml } from './lib/appservice-receiver.js';
import { createTaskGraphStore } from './lib/task-graph.js';
import { createTaskStore } from './lib/task-store.js';
import {
  indexPool, agentRole, agentCapability, selectAgent, resolveTier, TIER_RUNTIME,
  modelTier, modelFamily, ROLES, CAPABILITY_TIERS, ROLE_DEFAULT_TIER,
} from './lib/matrix-agent.js';
import { DispatchLeaseStore } from './src/dispatch-lease-store.mjs';
import { createSupervisorSnapshotStore } from './lib/supervisor-snapshot-store.js';
import { createSupervisorActionEngine } from './lib/supervisor-action-engine.js';
import { createAlertStore, RECOVERY_MAP as ALERT_RECOVERY_MAP } from './lib/alert-store.js';
import { ApprovalStore, ApprovalStoreError } from './lib/approval-store.js';
import {
  authorizeAgentCredential as authorizeAgentCredentialAdapter,
  buildAgentTokenReadiness as buildAgentTokenReadinessAdapter,
  buildServerCredentialReadiness as buildServerCredentialReadinessAdapter,
  checkAgentToken as checkAgentTokenAdapter,
  createApiAuthMiddleware,
  createRequireAgentToken,
  createRequireBearer,
  createRequireBridgeSecret,
  getBearerToken,
  getBridgeSecret,
  getRequestAgentName as getRequestAgentNameAdapter,
  hasApiTokenAccess as hasApiTokenAccessAdapter,
  operatorBearerConfigured,
  loadAgentTokensFromHomes,
  resolveAgentTokenMode,
} from './lib/backend/auth-adapter.js';
import { buildFlowHealth } from './lib/backend/flow-health.js';
import { buildFleetInventory } from './lib/backend/fleet-classifier.js';
import { createSseAdapter } from './lib/backend/sse-adapter.js';
import { createJsonStorage } from './lib/backend/storage-adapter.js';
import { createSupervisorLifecycleManager, killTmuxSession as killSupervisorTmux } from './lib/supervisor-lifecycle-manager.js';
import { provisionSupervisorAgent, buildSupervisorAgentRecord } from './lib/supervisor-provisioning.js';
import { ClaudeRuntimeConfigurationError, prepareClaudeThreadRuntime, claudeThreadModel } from './lib/claude-thread-runtime.js';
import { AgentStateMachine, deriveStateFromLegacy, agentExpectsMcp } from './lib/agent-state.js';
import { assertRuntimeDir, isLocalAgentServer, resolveLocalServerId } from './lib/runtime-dir-guard.js';
import { enforceStartupConfig, resolveBindHost } from './lib/startup-config.js';
import { NotificationRouter } from './lib/notification-router.js';
import {
  readV1AgentManifest,
  defaultAgentchatHomeDir,
  allAgentHomeRoots,
  findV1ManifestByName,
} from './lib/agent-home-v1.js';
import { approvalAdapterTimeoutMs, resolveApprovalTtlMs } from './lib/runtime-approval-client.js';
import { snapshotSessionFile, SessionFileError } from './lib/session-file.js';
import { receiveMatrixFile } from './lib/matrix-file.js';
import { codexPermissionRequestNeedsOwnerApproval } from './lib/codex-permission-hook.js';
import { buildProjectBoardSnapshot } from './lib/project-board.js';
import { createProjectInspector } from './lib/project-inspector.js';
import {
  AGENT_OPS_CLIENT_SCHEMA,
  agentOpsGrantProofMaterial,
  agentOpsSessionProofMaterial,
  checkAgentOpsLoopbackRequest,
  loadAgentOpsServerIdentity,
  normalizeAgentOpsLoopbackOrigin,
  normalizeEd25519PublicJwk,
  parseAgentOpsSessionAuthorization,
  verifyAgentOpsProof,
} from './lib/agent-ops-client-auth.js';
import { MatrixDispatchStore } from './src/matrix-dispatch-store.mjs';

const __filename = fileURLToPath(import.meta.url);
const REPO_ROOT = path.dirname(__filename);
const RUNTIME_ROOT = (() => {
  const raw = String(process.env.HAGENCY_RUNTIME_DIR || '').trim();
  return raw ? path.resolve(raw) : REPO_ROOT;
})();
assertRuntimeDir(RUNTIME_ROOT);
const DATA_DIR = path.join(RUNTIME_ROOT, 'data');
const DEFAULT_BACKEND_PORT_RAW = Number.parseInt(process.env.HAGENCY_BACKEND_PORT || '8090', 10);
const PORT = Number.isFinite(DEFAULT_BACKEND_PORT_RAW) && DEFAULT_BACKEND_PORT_RAW > 0
  ? DEFAULT_BACKEND_PORT_RAW
  : 8090;
/*
 * The web-bridge constants that pointed at the retired portal (WEB_BASE_URL, PUSH_QUEUE_URL,
 * HAGENCY_DASHBOARD_TOKEN) lived here. The queue is in-process now — `postToDeliveryQueue` — and the
 * one legitimate remote case reads HAGENCY_QUEUE_URL directly, so a stale HAGENCY_WEB_PORT can no
 * longer steer notifications at a port nothing listens on.
 */
const execFileAsync = promisify(execFile);
const LOCALHOST_IPS = new Set(['127.0.0.1', '::1', '::ffff:127.0.0.1']);
const LOCAL_SERVER_ID = resolveLocalServerId();
const RECORD_LOCAL_SERVER = normalizeBoolean(process.env.HAGENCY_RECORD_LOCAL_SERVER) === true;
const LOCAL_GIT_VERSION = (() => { try { return execSync('git rev-parse --short HEAD', { encoding: 'utf-8', timeout: 5000 }).trim(); } catch { return null; } })();
// How this host reaches its agents. Every platform-specific operation — pane
// enumeration, output capture, keystroke delivery, session existence — goes
// through here rather than shelling out to tmux inline, which is what allows a
// non-tmux runtime to be added without touching the backend. See lib/runtime/.
const hostRuntime = createTmuxRuntime();
// Which sessions this host may manage. The relay applies the same policy when it
// enumerates sessions, but the backend enumerates panes itself in local mode and
// can still hold agent records registered before a policy was configured — so it
// has to enforce independently rather than trust the relay to have filtered.
const sessionPolicy = sessionPolicyFromEnv();
for (const warning of sessionPolicy.warnings) console.warn(`[backend] ${warning}`);

const USER_UID = (typeof process.getuid === 'function') ? process.getuid() : null;
const USER_RUNTIME_DIR = Number.isFinite(USER_UID) ? `/run/user/${USER_UID}` : null;
const USER_DBUS_SESSION_BUS = USER_RUNTIME_DIR ? `unix:path=${USER_RUNTIME_DIR}/bus` : null;
const CORS_ALLOWED_ORIGIN = (process.env.FRP_API_ORIGIN || 'https://hagency.example.com').trim();
const HEARTBEAT_TTL_MS = Number.parseInt(process.env.AGENT_HEARTBEAT_TTL_MS || '90000', 10);
const SERVER_SWEEP_INTERVAL_MS = Number.parseInt(process.env.AGENT_SERVER_SWEEP_INTERVAL_MS || '15000', 10);
const HUMAN_SUMMARY_LIMIT = Number.parseInt(process.env.HUMAN_SUMMARY_LIMIT || '50', 10);
const RULE_PUSH_ACK_TIMEOUT_MS = Number.parseInt(process.env.AGENT_RULE_PUSH_ACK_TIMEOUT_MS || '90000', 10);
const RULE_REPLY_TIMEOUT_MS = Number.parseInt(process.env.AGENT_RULE_REPLY_TIMEOUT_MS || '180000', 10);
const RULE_SWEEP_INTERVAL_MS = Number.parseInt(process.env.AGENT_RULE_SWEEP_INTERVAL_MS || '15000', 10);
const IDLE_THRESHOLD_MS = Number.parseInt(process.env.AGENT_IDLE_THRESHOLD_MS || '20000', 10);
const IDLE_THRESHOLD_SEC = Math.max(1, Math.floor((IDLE_THRESHOLD_MS + 999) / 1000));
const PROJECT_BOARD_STALE_AFTER_MS = Number.parseInt(
  process.env.AGENT_PROJECT_BOARD_STALE_AFTER_MS || '300000',
  10,
);
const MCP_HEARTBEAT_AUTHORITY_WINDOW_MS = 90_000;
const LOCAL_ACTIVITY_SWEEP_INTERVAL_MS = Number.parseInt(process.env.AGENT_LOCAL_ACTIVITY_SWEEP_MS || '5000', 10);
const LOCAL_ACTIVITY_CAPTURE_BUDGET_RAW = Number.parseInt(process.env.AGENT_LOCAL_ACTIVITY_CAPTURE_BUDGET || '0', 10);
const LOCAL_ACTIVITY_CAPTURE_BUDGET = Number.isFinite(LOCAL_ACTIVITY_CAPTURE_BUDGET_RAW)
  ? Math.max(0, LOCAL_ACTIVITY_CAPTURE_BUDGET_RAW)
  : 0;
const SWAP_SWEEP_INTERVAL_MS = Number.parseInt(process.env.AGENT_SWAP_SWEEP_INTERVAL_MS || '5000', 10);
const SWAP_ALERT_THRESHOLD_PCT_RAW = Number.parseFloat(process.env.AGENT_SWAP_ALERT_THRESHOLD_PCT || '80');
const SWAP_ALERT_THRESHOLD_PCT = Number.isFinite(SWAP_ALERT_THRESHOLD_PCT_RAW)
  ? Math.min(99.9, Math.max(1, SWAP_ALERT_THRESHOLD_PCT_RAW))
  : 80;
const SWAP_ALERT_CLEAR_PCT_RAW = Number.parseFloat(process.env.AGENT_SWAP_ALERT_CLEAR_PCT || String(Math.max(1, SWAP_ALERT_THRESHOLD_PCT - 5)));
const SWAP_ALERT_CLEAR_PCT = Number.isFinite(SWAP_ALERT_CLEAR_PCT_RAW)
  ? Math.max(0, Math.min(SWAP_ALERT_THRESHOLD_PCT - 0.1, SWAP_ALERT_CLEAR_PCT_RAW))
  : Math.max(0, SWAP_ALERT_THRESHOLD_PCT - 5);
const AGENT_SCOPE_MONITOR_ENABLED = (process.env.AGENT_SCOPE_MONITOR_ENABLED || 'true').trim().toLowerCase() !== 'false';
const AGENT_SCOPE_SWEEP_INTERVAL_MS = Number.parseInt(process.env.AGENT_SCOPE_SWEEP_INTERVAL_MS || '5000', 10);
const SUPERVISOR_LIFECYCLE_SWEEP_INTERVAL_MS = Number.parseInt(process.env.SUPERVISOR_LIFECYCLE_SWEEP_INTERVAL_MS || '60000', 10);
const AGENT_SCOPE_ALERT_COOLDOWN_MS = Number.parseInt(process.env.AGENT_SCOPE_ALERT_COOLDOWN_MS || '60000', 10);
const AGENT_SCOPE_ALERT_CLEAR_RATIO_RAW = Number.parseFloat(process.env.AGENT_SCOPE_ALERT_CLEAR_RATIO || '0.85');
const AGENT_SCOPE_ALERT_CLEAR_RATIO = Number.isFinite(AGENT_SCOPE_ALERT_CLEAR_RATIO_RAW)
  ? Math.min(0.99, Math.max(0.1, AGENT_SCOPE_ALERT_CLEAR_RATIO_RAW))
  : 0.85;
// Task 7: matrix-Agent dispatch lease TTL. Floor-validated — below this, a renew round-trip
// under real process/network latency has no realistic chance of landing before expiry, so a
// smaller configured value is almost certainly a misconfiguration and gets clamped up rather
// than honored.
const DISPATCH_LEASE_TTL_DEFAULT_MS = 15 * 60 * 1000; // 15 minutes
const DISPATCH_LEASE_TTL_FLOOR_MS = 1000; // 1 second
const DISPATCH_LEASE_TTL_MS_RAW = Number.parseInt(process.env.HAGENCY_DISPATCH_LEASE_TTL_MS || String(DISPATCH_LEASE_TTL_DEFAULT_MS), 10);
const DISPATCH_LEASE_TTL_MS = Number.isFinite(DISPATCH_LEASE_TTL_MS_RAW) && DISPATCH_LEASE_TTL_MS_RAW > 0
  ? Math.max(DISPATCH_LEASE_TTL_FLOOR_MS, DISPATCH_LEASE_TTL_MS_RAW)
  : DISPATCH_LEASE_TTL_DEFAULT_MS;
// Owner assigned to a lease when the caller doesn't supply one on POST /api/dispatch — dispatch
// itself never rejects a missing owner (only renew/release do), so a lease always exists to
// return a leaseId for.
const DISPATCH_LEASE_DEFAULT_OWNER = 'unspecified';
// POST /api/dispatch/release defaults to REJECTING the legacy {agent}-only shape (no leaseId,
// no owner): letting ownership be skipped just by omitting fields would defeat the whole point
// of "owner mismatch must fail" (a caller could always dodge the check by not claiming an
// owner). HAGENCY_ALLOW_LEGACY_RELEASE=1 is an explicit, off-by-default escape hatch for a
// caller that predates ownership (e.g. a reintroduced OpenFab Bridge — the version live at the
// time this lease system was built has since been descoped/stopped, so nothing needs it today).
const DISPATCH_ALLOW_LEGACY_RELEASE = normalizeBoolean(process.env.HAGENCY_ALLOW_LEGACY_RELEASE) === true;
const OFFLINE_CATCHUP_LIST_LIMIT = Number.parseInt(process.env.OFFLINE_CATCHUP_LIST_LIMIT || '50', 10);
const MESSAGE_ATTACHMENT_MAX_ITEMS = Number.parseInt(process.env.MESSAGE_ATTACHMENT_MAX_ITEMS || '8', 10);
const MESSAGE_ATTACHMENT_MAX_BYTES = Number.parseInt(process.env.MESSAGE_ATTACHMENT_MAX_BYTES || String(20 * 1024 * 1024), 10);
const MESSAGE_ATTACHMENT_STAGE_JSON_LIMIT = (process.env.MESSAGE_ATTACHMENT_STAGE_JSON_LIMIT || '30mb').trim() || '30mb';
const MESSAGE_RETENTION_LIMIT = Math.max(100, Number.parseInt(process.env.AGENT_MESSAGE_RETENTION_LIMIT || '5000', 10) || 5000);
const THREAD_SESSIONS_ENABLED = normalizeBoolean(process.env.HAGENCY_THREAD_SESSIONS) === true;
const ROUTER_TASK_CUTOVER_ENABLED = normalizeBoolean(process.env.HAGENCY_ROUTER_TASK_CUTOVER) === true;
const ROUTER_SHADOW_ENABLED = normalizeBoolean(process.env.HAGENCY_ROUTER_SHADOW) === true;
const AGENT_OPS_CLIENT_ENABLED = normalizeBoolean(process.env.HAGENCY_AGENT_OPS_CLIENT) === true;
const AGENT_OPS_LOOPBACK_ORIGIN = AGENT_OPS_CLIENT_ENABLED
  ? normalizeAgentOpsLoopbackOrigin(
    process.env.HAGENCY_AGENT_OPS_LOOPBACK_ORIGIN || `http://127.0.0.1:${PORT}`,
  )
  : null;
const RUNNER_LEASE_MS = Math.max(60_000, Number.parseInt(process.env.HAGENCY_RUNNER_LEASE_MS || '1200000', 10) || 1_200_000);
const RUNNER_ACK_MS = Math.max(1_000, Number.parseInt(process.env.HAGENCY_RUNNER_ACK_MS || '60000', 10) || 60_000);
const RUNNER_LAUNCH_RETRY_MS = Math.max(1_000, Math.min(
  60_000,
  Number.parseInt(process.env.HAGENCY_RUNNER_LAUNCH_RETRY_MS || '5000', 10) || 5_000,
));
const MAX_LIVE_RUNNERS = Math.max(1, Number.parseInt(process.env.HAGENCY_MAX_LIVE_RUNNERS || '8', 10) || 8);
const MAX_PARKED_RUNNERS_RAW = Number.parseInt(process.env.HAGENCY_MAX_PARKED_RUNNERS || '4', 10);
const MAX_PARKED_RUNNERS = Number.isFinite(MAX_PARKED_RUNNERS_RAW)
  ? Math.max(0, MAX_PARKED_RUNNERS_RAW)
  : 4;
const REBUILD_TOKEN_BUDGET = Math.max(1_000, Number.parseInt(process.env.HAGENCY_REBUILD_TOKEN_BUDGET || '12000', 10) || 12_000);
const THREAD_SESSION_MCP_SERVER_NAME = /^[A-Za-z0-9_-]{1,64}$/.test(process.env.HAGENCY_MCP_SERVER_NAME || '')
  ? process.env.HAGENCY_MCP_SERVER_NAME
  : 'hagency';
if (MAX_PARKED_RUNNERS >= MAX_LIVE_RUNNERS) {
  throw new Error('HAGENCY_MAX_PARKED_RUNNERS must be lower than HAGENCY_MAX_LIVE_RUNNERS');
}
if (THREAD_SESSIONS_ENABLED && !ROUTER_TASK_CUTOVER_ENABLED) {
  throw new Error('HAGENCY_THREAD_SESSIONS requires HAGENCY_ROUTER_TASK_CUTOVER=1');
}
if (AGENT_OPS_CLIENT_ENABLED && !THREAD_SESSIONS_ENABLED) {
  throw new Error('HAGENCY_AGENT_OPS_CLIENT requires HAGENCY_THREAD_SESSIONS=1');
}
const routerRuntime = (ROUTER_TASK_CUTOVER_ENABLED || THREAD_SESSIONS_ENABLED || ROUTER_SHADOW_ENABLED)
  ? await import('./router/dist/index.js')
  : null;
const {
  createRouterTaskStore,
  AgentOpsService,
  migrateLegacyTasks,
  openRouter,
  runClaudeDispatch,
  runCodexDispatch,
  WorktreeManager,
} = routerRuntime || {};
const JSON_WRITE_BATCH_WINDOW_MS_RAW = Number.parseInt(process.env.AGENT_JSON_WRITE_BATCH_MS || '1000', 10);
const JSON_WRITE_BATCH_WINDOW_MS = Number.isFinite(JSON_WRITE_BATCH_WINDOW_MS_RAW)
  ? Math.max(0, JSON_WRITE_BATCH_WINDOW_MS_RAW)
  : 1000;
const UNEXPECTED_OFFLINE_ALERT_THROTTLE_MS = Number.parseInt(process.env.UNEXPECTED_OFFLINE_ALERT_THROTTLE_MS || '120000', 10);
const AGENT_TMUX_MISSING_ALERT_GRACE_MS = Number.parseInt(process.env.AGENT_TMUX_MISSING_ALERT_GRACE_MS || '15000', 10);
const AGENT_TMUX_MISSING_ALERT_MAX_AGE_MS = Number.parseInt(process.env.AGENT_TMUX_MISSING_ALERT_MAX_AGE_MS || '900000', 10);
const AGENT_TMUX_MISSING_THRESHOLD_RAW = Number.parseInt(process.env.AGENT_TMUX_MISSING_THRESHOLD || '3', 10);
const AGENT_TMUX_MISSING_THRESHOLD = Number.isFinite(AGENT_TMUX_MISSING_THRESHOLD_RAW)
  ? Math.max(3, AGENT_TMUX_MISSING_THRESHOLD_RAW)
  : 3;
const AGENT_COMPACT_SUMMARY_MAX = Number.parseInt(process.env.AGENT_COMPACT_SUMMARY_MAX || '180', 10);
const AGENT_COMPACT_RUNTIME_DEDUPE_MS = Number.parseInt(process.env.AGENT_COMPACT_RUNTIME_DEDUPE_MS || '120000', 10);
const BACKEND_STARTUP_OPTIONAL_ENV = [
  {
    name: 'HAGENCY_DASHBOARD_TOKEN',
    description: 'Non-local dashboard mutations will remain unavailable unless this token is configured.',
  },
  ];
const SERVER_MAINTENANCE_IDS = new Set(
  String(process.env.AGENT_SERVER_MAINTENANCE_IDS ?? '')
    .split(',')
    .map(normalizeServer)
    .filter(Boolean)
);
const SERVER_MAINTENANCE_ENV_CONFIGURED = Object.prototype.hasOwnProperty.call(
  process.env,
  'AGENT_SERVER_MAINTENANCE_IDS'
);
const SERVER_MAINTENANCE_LAST_SEEN_UPDATE_MS = Number.parseInt(process.env.AGENT_SERVER_MAINTENANCE_LAST_SEEN_UPDATE_MS || '60000', 10);
const AGENT_COMPACT_HOOK_PATTERNS = [
  /\[(?:agent[_-]?compact|compact(?:ion)?)\]/i,
  /\bagent[_-]?compact(?:ion)?\s*:/i,
  /\bcompact[_-]?hook\b/i,
];
const AGENT_COMPACT_FALLBACK_PATTERNS = [
  { marker: 'codex-context-compacted', re: /(?:^|\n)\s*(?:•\s*)?Context compacted\s*(?:\n|$)/i },
  { marker: 'claude-conversation-compacted', re: /(?:^|\n)\s*(?:✻\s*)?Conversation compacted \(ctrl\+o for history\)\s*(?:\n|$)/i },
  { marker: 'claude-compacted-summary', re: /(?:^|\n)\s*(?:⎿\s*)?Compacted \(ctrl\+o to see full summary\)\s*(?:\n|$)/i },
];
const LOCAL_BLOCK_TAIL_LINES = Number.parseInt(process.env.AGENT_LOCAL_BLOCK_TAIL_LINES || '40', 10);
const LOCAL_BLOCK_RECENT_LINES = Number.parseInt(process.env.AGENT_LOCAL_BLOCK_RECENT_LINES || '14', 10);
const LOCAL_MCP_SESSION_CACHE_TTL_MS = Number.parseInt(process.env.AGENT_LOCAL_MCP_SESSION_CACHE_TTL_MS || '1000', 10);
const AGENT_SWEEP_INTERVAL_PER_AGENT_MS_RAW = Number.parseInt(process.env.AGENT_SWEEP_INTERVAL_PER_AGENT_MS || '500', 10);
const AGENT_SWEEP_INTERVAL_PER_AGENT_MS = Number.isFinite(AGENT_SWEEP_INTERVAL_PER_AGENT_MS_RAW)
  ? Math.max(1, AGENT_SWEEP_INTERVAL_PER_AGENT_MS_RAW)
  : 500;
const BLOCKED_NOTIFICATION_COOLDOWN_MS_RAW = Number.parseInt(process.env.AGENT_BLOCKED_NOTIFICATION_COOLDOWN_MS || '60000', 10);
const BLOCKED_NOTIFICATION_COOLDOWN_MS = Number.isFinite(BLOCKED_NOTIFICATION_COOLDOWN_MS_RAW)
  ? Math.max(0, BLOCKED_NOTIFICATION_COOLDOWN_MS_RAW)
  : 60000;
const BLOCKED_INFO_AGGREGATE_WINDOW_MS_RAW = Number.parseInt(process.env.AGENT_BLOCKED_INFO_AGGREGATE_WINDOW_MS || '30000', 10);
const BLOCKED_INFO_AGGREGATE_WINDOW_MS = Number.isFinite(BLOCKED_INFO_AGGREGATE_WINDOW_MS_RAW) && BLOCKED_INFO_AGGREGATE_WINDOW_MS_RAW >= 0
  ? BLOCKED_INFO_AGGREGATE_WINDOW_MS_RAW
  : 30_000;
// agent_blocked aggregation is handled by notificationRouter (initialized after emitSystemInfo)

const AUTO_CLEAR_COOLDOWN_MS = Number.parseInt(process.env.AGENT_AUTO_CLEAR_COOLDOWN_MS || '300000', 10);
const APPROVAL_TTL_MS = resolveApprovalTtlMs(process.env);
const APPROVAL_ADAPTER_TIMEOUT_MS = approvalAdapterTimeoutMs(process.env);
const autoClearLastTs = new Map();
const autoClearPrevReason = new Map();

mkdirSync(DATA_DIR, { recursive: true });
const MESSAGE_ATTACHMENT_DIR = path.join(DATA_DIR, 'message-attachments');
mkdirSync(MESSAGE_ATTACHMENT_DIR, { recursive: true });
const MATRIX_MEDIA_DIR = path.join(DATA_DIR, 'matrix', 'media');
mkdirSync(MATRIX_MEDIA_DIR, { recursive: true });
const MATRIX_OPERATOR_MXIDS = new Set(
  (process.env.MATRIX_OPERATOR_MXIDS || '').split(',').map(s => s.trim()).filter(Boolean)
);
const MATRIX_ADMIN_MXIDS = new Set(
  (process.env.MATRIX_ADMIN_MXIDS || '').split(',').map(s => s.trim()).filter(Boolean)
);
// Reads bridge secret fresh from env on each call (tests toggle process.env between cases).
const requireBridgeSecret = createRequireBridgeSecret({ env: process.env });
// ── Per-agent token authentication (5.8.6) ───────────────────────────
const { mode: AGENT_TOKEN_MODE, configuredMode: AGENT_TOKEN_CONFIGURED_MODE } = resolveAgentTokenMode(process.env);
const agentTokens = new Map(); // agentName → token string
function loadAgentTokens() {
  return loadAgentTokensFromHomes({
    agentTokens,
    agents,
    allAgentHomeRoots,
    mode: AGENT_TOKEN_MODE,
  });
}
function checkAgentToken(agentName, req) {
  /*
   * No `env` override: the adapter defaults to `process.env`, which is where this deployment's operator
   * credential lives, and threading it explicitly was dead code. Mutation testing showed why — swapping
   * it for `{}` changed no test, because this wrapper's only caller (`_alertTransitionAuth`) answers the
   * operator itself before ever reaching here. An argument that cannot affect any caller reads as a
   * live path and is not one.
   */
  return checkAgentTokenAdapter(agentName, req, { agentTokens });
}
function buildAgentTokenReadiness() {
  return buildAgentTokenReadinessAdapter({
    agents,
    agentTokens,
    agentTokenMode: AGENT_TOKEN_MODE,
    configuredMode: AGENT_TOKEN_CONFIGURED_MODE,
    isAgentRecord,
  });
}

function buildServerCredentialReadiness() {
  return buildServerCredentialReadinessAdapter({ env: process.env });
}
const requireAgentToken = createRequireAgentToken({
  agentTokens, agentTokenMode: AGENT_TOKEN_MODE, env: process.env,
});
const VALID_ENVIRONMENTS = new Set(['live', 'dev', 'benchmark', 'ephemeral']);
function classifyEnvironment(name) {
  const n = String(name).toLowerCase();
  if (/(?:^|[-_])(?:test|tmp|scratch|smoke|e2e)(?:[-_]|$)/.test(n)) return 'ephemeral';
  if (/(?:^|[-_])(?:bench|benchmark)(?:[-_]|$)/.test(n)) return 'benchmark';
  if (/(?:^|[-_])(?:dev|debug)(?:[-_]|$)/.test(n)) return 'dev';
  return 'live';
}
const MEDIA_FETCH_ALLOWED_ROOTS = [
  path.resolve(MESSAGE_ATTACHMENT_DIR),
  path.resolve(MATRIX_MEDIA_DIR),
];


/*
 * The queue lives IN this process now, so "post to the queue" is a function call. `HAGENCY_QUEUE_URL`
 * is still honoured for a deployment that runs the queue elsewhere — in that one case this really is
 * an HTTP request, carrying the operator bearer because the remote queue's guard accepts it.
 *
 * The Response-like return (`ok`, `status`, `json()`) is deliberate: the three call sites predate the
 * move and read a fetch Response, and keeping that contract meant none of their logic — event
 * emission, markAgentPushNotified, error paths — had to be re-derived during the migration.
 */
const REMOTE_QUEUE_URL = (process.env.HAGENCY_QUEUE_URL || '').trim().replace(/\/$/, '');
async function postToDeliveryQueue(bodyObj, idempotencyKey = '') {
  if (REMOTE_QUEUE_URL) {
    return fetch(REMOTE_QUEUE_URL, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(idempotencyKey ? { 'Idempotency-Key': idempotencyKey } : {}),
        ...(operatorBearerConfigured(process.env) ? { Authorization: `Bearer ${process.env.API_TOKEN.trim()}` } : {}),
      },
      signal: AbortSignal.timeout(5000),
      body: JSON.stringify(bodyObj),
    });
  }
  const { status, body } = enqueueFromRequestBody(bodyObj ?? {}, idempotencyKey || '');
  return { ok: status < 400, status, json: async () => body };
}

async function clearDeliveryQueueNotifications(agentName) {
  if (REMOTE_QUEUE_URL) {
    const url = `${REMOTE_QUEUE_URL}/agents/${encodeURIComponent(agentName)}/notifications`;
    return fetch(url, { method: 'DELETE', signal: AbortSignal.timeout(5000) });
  }
  const result = dropQueuedBackendNotificationsBySource(agentName, null, 'agent-notifications-cleared');
  return { ok: !result.persistFailed, status: result.persistFailed ? 503 : 200, json: async () => result };
}


// ── Storage helpers ───────────────────────────────────────────────────
const jsonStorage = createJsonStorage({
  dataDir: DATA_DIR,
  jsonWriteBatchWindowMs: JSON_WRITE_BATCH_WINDOW_MS,
  batchedFiles: ['agents.json', 'agent_runtime.json'],
});
const {
  dataPath,
  agentDataPath,
  loadJsonSync,
  saveJson: storageSaveJson,
  flushAllPendingJsonWrites,
  loadJsonlTailSync,
} = jsonStorage;
const forcedJsonSaveFailures = new Map();

function saveJson(name, data, options = {}) {
  const forcedFailure = forcedJsonSaveFailures.get(name);
  if (forcedFailure) {
    if (forcedFailure && typeof forcedFailure === 'object' && !Array.isArray(forcedFailure)) {
      const after = Math.max(0, Number(forcedFailure.after) || 0);
      if (after > 0) {
        forcedJsonSaveFailures.set(name, { ...forcedFailure, after: after - 1 });
      } else {
        const count = Math.max(1, Number(forcedFailure.count) || 1);
        if (count <= 1) forcedJsonSaveFailures.delete(name);
        else forcedJsonSaveFailures.set(name, { ...forcedFailure, count: count - 1 });
        console.error(`Forced JSON save failure for ${dataPath(name)}`);
        return false;
      }
    } else {
      console.error(`Forced JSON save failure for ${dataPath(name)}`);
      return false;
    }
  }
  return storageSaveJson(name, data, options);
}

function setJsonSaveFailureForTest(name, enabled = true) {
  if (typeof name !== 'string' || !name.trim()) return;
  if (enabled === false) {
    forcedJsonSaveFailures.delete(name);
  } else if (enabled && typeof enabled === 'object' && !Array.isArray(enabled)) {
    forcedJsonSaveFailures.set(name, {
      after: Math.max(0, Number(enabled.after) || 0),
      count: Math.max(1, Number(enabled.count) || 1),
    });
  } else {
    forcedJsonSaveFailures.set(name, true);
  }
}

function normalizeServer(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed || null;
}

function normalizeWorkspacePath(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed || trimmed.length > 4096) return null;
  if (!path.isAbsolute(trimmed)) return null;
  return path.resolve(trimmed);
}

function normalizeWorkspaceMode(value) {
  return value === 'worktree' ? 'worktree' : 'shared';
}

function normalizeWorktreeBootstrap(value) {
  if (!Array.isArray(value) || value.length === 0 || value.length > 64) return [];
  const argv = [];
  for (const item of value) {
    if (typeof item !== 'string' || !item.trim() || item.length > 4096) return [];
    argv.push(item);
  }
  return argv;
}

function normalizeRuntimeActiveNow(value) {
  if (value === true) return true;
  if (value === false) return false;
  return null;
}

function normalizeAgentName(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  // Case-insensitive lookup: return the canonical (stored) name if it exists
  if (agents[trimmed]) return trimmed;
  const lower = trimmed.toLowerCase();
  for (const key of Object.keys(agents)) {
    if (key.toLowerCase() === lower) return key;
  }
  return trimmed;
}

function normalizeAgentModelVersion(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (trimmed.length > 32) return null;
  return trimmed;
}

function normalizeLayoutVersion(value) {
  if (value === null || value === undefined) return null;
  const n = Number.parseInt(value, 10);
  if (!Number.isFinite(n) || n <= 0) return null;
  return n;
}

function normalizeAgentId(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (!/^[A-Za-z0-9_-]{1,128}$/.test(trimmed)) return null;
  return trimmed;
}

function normalizeOptionalText(value, maxLen = 4000) {
  if (value === null) return null;
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (trimmed.length > maxLen) return trimmed.slice(0, maxLen);
  return trimmed;
}

function normalizeRuntimeObservation(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const observerSource = normalizeOptionalText(value.observerSource, 64);
  if (!observerSource) return null;
  const observerServer = normalizeServer(value.observerServer);
  const observedAt = Math.max(0, Number(value.observedAt) || 0);
  if (!observedAt) return null;
  return {
    observerSource,
    observerServer,
    observedAt,
  };
}

function buildRuntimeObservation({ observerSource, observerServer, observedAt } = {}) {
  const source = normalizeOptionalText(observerSource, 64);
  if (!source) return null;
  return {
    observerSource: source,
    observerServer: normalizeServer(observerServer),
    observedAt: Math.max(0, Number(observedAt) || Date.now()),
  };
}

function serializeRuntimeObservation(runtime) {
  return normalizeRuntimeObservation(runtime?.observation);
}

function runtimeObservationEquals(left, right) {
  const a = normalizeRuntimeObservation(left);
  const b = normalizeRuntimeObservation(right);
  if (!a && !b) return true;
  if (!a || !b) return false;
  return a.observerSource === b.observerSource
    && (a.observerServer || null) === (b.observerServer || null)
    && a.observedAt === b.observedAt;
}

function setRuntimeObservation(runtime, details = {}) {
  if (!runtime || typeof runtime !== 'object') return false;
  const next = buildRuntimeObservation(details);
  if (!next) return false;
  const prev = normalizeRuntimeObservation(runtime.observation);
  if (runtimeObservationEquals(prev, next)) return false;
  runtime.observation = next;
  return true;
}

function normalizeBoolean(value) {
  if (value === true) return true;
  if (value === false) return false;
  if (typeof value === 'string') {
    const trimmed = value.trim().toLowerCase();
    if (['1', 'true', 'yes', 'on'].includes(trimmed)) return true;
    if (['0', 'false', 'no', 'off'].includes(trimmed)) return false;
  }
  return null;
}

function normalizePositiveInt(value, fallback, min = 1) {
  const n = Number.parseInt(value, 10);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, n);
}

function normalizeBlockedTier(value, fallback = null) {
  const n = Number.parseInt(value, 10);
  if (n === BLOCK_TIER_TRANSIENT || n === BLOCK_TIER_SOFT || n === BLOCK_TIER_HARD) return n;
  return fallback;
}

function blockedTierFromReason(reason) {
  const normalizedReason = normalizeOptionalText(reason, 256);
  if (!normalizedReason) return null;
  const matched = LOCAL_BLOCK_PATTERNS.find((pattern) => pattern.reason === normalizedReason);
  return normalizeBlockedTier(matched?.tier, BLOCK_TIER_HARD);
}

function blockedTierDebounceThreshold(tier) {
  switch (normalizeBlockedTier(tier, BLOCK_TIER_HARD)) {
    case BLOCK_TIER_TRANSIENT:
      return Number.POSITIVE_INFINITY;
    case BLOCK_TIER_SOFT:
      return 6;
    default:
      return 2;
  }
}

function normalizeNonNegativeInt(value, fallback = 0) {
  const n = Number.parseInt(value, 10);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(0, n);
}

function normalizeProvider(value) {
  const raw = String(value || '').trim().toLowerCase();
  if (['deepseek', 'qwen', 'openai', 'openai-compatible'].includes(raw)) return raw;
  return 'deepseek';
}

function normalizeProviderOrNull(value) {
  const raw = normalizeOptionalText(value, 64);
  if (!raw) return null;
  const lower = raw.toLowerCase();
  if (['deepseek', 'qwen', 'openai', 'openai-compatible'].includes(lower)) return lower;
  return null;
}

function defaultCompatibleEndpoint(provider) {
  switch (provider) {
    case 'deepseek':
      return 'https://api.deepseek.com/v1/chat/completions';
    case 'qwen':
      return 'https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions';
    case 'openai':
      return 'https://api.openai.com/v1/chat/completions';
    default:
      return 'https://api.deepseek.com/v1/chat/completions';
  }
}

function defaultCompatibleModel(provider) {
  switch (provider) {
    case 'deepseek':
      return 'deepseek-chat';
    case 'qwen':
      return 'qwen-plus';
    case 'openai':
      return 'gpt-4.1-mini';
    default:
      return 'deepseek-chat';
  }
}

function normalizeCompatibleEndpoint(baseOrEndpoint, defaultEndpoint) {
  const raw = normalizeOptionalText(baseOrEndpoint, 2048);
  if (!raw) return defaultEndpoint;
  if (raw.endsWith('/chat/completions')) return raw;
  if (raw.endsWith('/')) return `${raw}chat/completions`;
  return `${raw}/chat/completions`;
}

function normalizeCompatibleEndpointOrNull(baseOrEndpoint) {
  const raw = normalizeOptionalText(baseOrEndpoint, 2048);
  if (!raw) return null;
  return normalizeCompatibleEndpoint(raw, raw);
}

function normalizeJsonText(raw) {
  const text = String(raw || '').trim();
  if (!text) return '';
  if (text.startsWith('{') && text.endsWith('}')) return text;
  const fence = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fence && fence[1]) return fence[1].trim();
  const start = text.indexOf('{');
  const end = text.lastIndexOf('}');
  if (start >= 0 && end > start) return text.slice(start, end + 1);
  return text;
}

function normalizeManagedProjects(value) {
  if (!Array.isArray(value)) return [];
  const out = [];
  const seen = new Set();
  for (const row of value) {
    if (!row || typeof row !== 'object') continue;
    const name = normalizeOptionalText(row.name, 128);
    const projectPath = normalizeWorkspacePath(row.path);
    if (!name || !projectPath) continue;
    const source = normalizeOptionalText(row.source, 64) || 'unknown';
    const originPath = normalizeWorkspacePath(row.originPath) || null;
    const key = `${name}\n${projectPath}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ name, path: projectPath, source, originPath });
  }
  return out;
}

function normalizeHumanMeta(value, options = {}) {
  const raw = (value && typeof value === 'object') ? value : {};
  const preserveLegacy = options && options.preserveLegacy === true;
  const out = {
    owner: normalizeOptionalText(raw.owner, 256),
  };
  if (preserveLegacy) {
    if (Object.prototype.hasOwnProperty.call(raw, 'notes')) {
      out.notes = normalizeOptionalText(raw.notes, 8000) || '';
    }
    if (Object.prototype.hasOwnProperty.call(raw, 'projectScope')) {
      out.projectScope = normalizeOptionalText(raw.projectScope, 4000) || '';
    }
  }
  return out;
}

function mergeHumanMeta(existingValue, nextValue) {
  const existing = normalizeHumanMeta(existingValue, { preserveLegacy: true });
  if (nextValue === undefined) return existing;
  const raw = (nextValue && typeof nextValue === 'object') ? nextValue : {};
  const out = {
    ...existing,
    owner: Object.prototype.hasOwnProperty.call(raw, 'owner')
      ? normalizeOptionalText(raw.owner, 256)
      : existing.owner,
  };
  return out;
}

function normalizeIsoTimestamp(value) {
  const raw = normalizeOptionalText(value, 128);
  if (!raw) return null;
  const ms = Date.parse(raw);
  if (!Number.isFinite(ms)) return null;
  return new Date(ms).toISOString();
}

function normalizeTaskStatus(value) {
  const raw = normalizeOptionalText(value, 32);
  if (!raw) return null;
  const lower = raw.toLowerCase();
  if (['active', 'waiting', 'blocked', 'done'].includes(lower)) return lower;
  return null;
}

function normalizeAgentTask(value, fallbackOwner = null) {
  if (value === null) return null;
  if (!value || typeof value !== 'object') return null;
  const status = normalizeTaskStatus(value.status);
  if (!status) return null;
  const owner = normalizeAgentName(value.owner) || normalizeAgentName(fallbackOwner) || normalizeOptionalText(value.owner, 128);
  const updatedAt = normalizeIsoTimestamp(value.updated_at);
  const heartbeatAt = normalizeIsoTimestamp(value.heartbeat_at);
  const waitingReason = normalizeOptionalText(value.waiting_reason, 2000);
  const waitingUntil = normalizeIsoTimestamp(value.waiting_until);
  const id = normalizeOptionalText(value.id, 256);
  if (!id || !owner || !updatedAt || !heartbeatAt) return null;
  if (status === 'waiting') {
    if (!waitingReason || !waitingUntil) return null;
  }
  return {
    id,
    owner,
    status,
    updated_at: updatedAt,
    heartbeat_at: heartbeatAt,
    waiting_reason: status === 'waiting' ? waitingReason : null,
    waiting_until: status === 'waiting' ? waitingUntil : null,
  };
}

const SHELL_METACHAR_RE = /[;&|`$(){}!\\<>]/;

function normalizeRuntimeProfileRole(value) {
  if (value === null) return null;
  if (!value || typeof value !== 'object') return null;
  const framework = normalizeOptionalText(value.framework, 32);
  const provider = normalizeOptionalText(value.provider, 64);
  const rawModel = normalizeOptionalText(value.model, 256);
  const model = rawModel && SHELL_METACHAR_RE.test(rawModel) ? null : rawModel;
  const reasoning = normalizeOptionalText(value.reasoning, 64);
  const rawExtraArgs = normalizeOptionalText(value.extraArgs, 4000);
  const extraArgs = rawExtraArgs && SHELL_METACHAR_RE.test(rawExtraArgs) ? null : rawExtraArgs;
  const rawApiBaseUrl = normalizeOptionalText(value.apiBaseUrl, 512);
  let apiBaseUrl = null;
  if (rawApiBaseUrl) {
    try {
      const parsed = new URL(rawApiBaseUrl);
      if (!['http:', 'https:'].includes(parsed.protocol)) throw new Error('not http(s)');
      if (parsed.username || parsed.password) throw new Error('credentials in URL');
      apiBaseUrl = rawApiBaseUrl;
    } catch { apiBaseUrl = null; }
  }
  const apiKey = normalizeOptionalText(value.apiKey, 256);
  if (!framework && !provider && !model && !reasoning && !extraArgs && !apiBaseUrl && !apiKey) return null;
  return {
    framework: framework || null,
    provider: provider || null,
    model: model || null,
    reasoning: reasoning || null,
    ...(extraArgs ? { extraArgs } : {}),
    ...(apiBaseUrl ? { apiBaseUrl } : {}),
    ...(apiKey ? { apiKey } : {}),
  };
}

function normalizeRuntimeProfile(value) {
  if (value === null) return null;
  if (!value || typeof value !== 'object') return null;
  const primary = normalizeRuntimeProfileRole(value.primary);
  const supervisor = normalizeRuntimeProfileRole(value.supervisor);
  if (!primary && !supervisor) return null;
  return {
    primary: primary || null,
    supervisor: supervisor || null,
  };
}

function mergeRuntimeProfileApiKeys(newProfile, existingProfile) {
  if (!newProfile) return newProfile;
  for (const role of ['primary', 'supervisor']) {
    if (newProfile[role] && !newProfile[role].apiKey && existingProfile && existingProfile[role] && existingProfile[role].apiKey) {
      newProfile[role] = { ...newProfile[role], apiKey: existingProfile[role].apiKey };
    }
  }
  return newProfile;
}

function redactRuntimeProfileSecrets(profile) {
  if (!profile) return profile;
  const redacted = { ...profile };
  for (const role of ['primary', 'supervisor']) {
    if (redacted[role] && redacted[role].apiKey) {
      redacted[role] = { ...redacted[role], apiKey: true };
    }
  }
  return redacted;
}

function normalizeLooseAgentName(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (!/^[A-Za-z0-9._-]+$/.test(trimmed)) return null;
  return normalizeAgentName(trimmed);
}


function normalizeEventTs(value) {
  const n = Number.parseInt(value, 10);
  if (!Number.isFinite(n) || n <= 0) return Date.now();
  return n;
}

function msgSeq(id) {
  if (typeof id !== 'string') return 0;
  const n = Number.parseInt(id.replace(/^msg_/, ''), 10);
  return Number.isFinite(n) ? n : 0;
}

function compareMsgOrder(a, b) {
  if (!a && !b) return 0;
  if (!a) return -1;
  if (!b) return 1;
  if (a.ts !== b.ts) return a.ts - b.ts;
  const aSeq = msgSeq(a.id);
  const bSeq = msgSeq(b.id);
  if (aSeq !== bSeq) return aSeq - bSeq;
  return String(a.id || '').localeCompare(String(b.id || ''));
}

function makeHumanSummaryPreview(text) {
  const normalized = String(text || '').replace(/\s+/g, ' ').trim();
  if (!normalized) return '';
  const chars = [...normalized];
  if (chars.length <= HUMAN_SUMMARY_LIMIT) return normalized;
  return chars.slice(0, HUMAN_SUMMARY_LIMIT).join('') + '...';
}

function makeCompactPreview(text, maxChars = AGENT_COMPACT_SUMMARY_MAX) {
  const normalized = String(text || '').replace(/\s+/g, ' ').trim();
  if (!normalized) return '';
  const chars = [...normalized];
  if (chars.length <= maxChars) return normalized;
  return `${chars.slice(0, maxChars).join('')}...`;
}

function detectAgentCompactSignal(summary, full) {
  const raw = [summary || '', full || ''].filter(Boolean).join('\n').trim();
  if (!raw) return null;

  for (const re of AGENT_COMPACT_HOOK_PATTERNS) {
    if (re.test(raw)) return { mode: 'hook', marker: 'explicit-hook' };
  }
  for (const pattern of AGENT_COMPACT_FALLBACK_PATTERNS) {
    if (pattern.re.test(raw)) return { mode: 'pattern', marker: pattern.marker };
  }
  return null;
}

function recentTailWindow(tail, maxLines = LOCAL_BLOCK_RECENT_LINES) {
  const lines = String(tail || '')
    .split(/\r?\n/)
    .map(line => line.replace(/\s+$/g, ''))
    .filter(line => line.trim().length > 0);
  if (lines.length === 0) return '';
  return lines.slice(-Math.max(1, maxLines)).join('\n');
}

function detectLocalBlockedReason(tail, paneCmd = '') {
  if (!tail) return null;
  const cmd = String(paneCmd || '').toLowerCase();
  if (cmd && !cmd.includes('claude') && !cmd.includes('codex')) return null;
  const window = recentTailWindow(tail, LOCAL_BLOCK_RECENT_LINES);
  if (!window) return null;
  if (/tip:\s*use plan mode\b/i.test(window)) return null;

  for (const p of LOCAL_BLOCK_PATTERNS) {
    if (p.re.test(window)) return p.reason;
  }
  return null;
}

function buildAgentCompactEvent(msg, senderIsAgent) {
  if (!senderIsAgent) return null;
  if (!msg || msg.type === 'human' || msg.from === 'system') return null;
  const signal = detectAgentCompactSignal(msg.summary, msg.full);
  if (!signal) return null;

  const summary = makeCompactPreview(msg.summary || msg.full || '', AGENT_COMPACT_SUMMARY_MAX);
  return {
    id: `compact_${msg.id}`,
    ts: msg.ts || Date.now(),
    messageId: msg.id,
    agent: msg.from,
    mode: signal.mode,
    marker: signal.marker || null,
    source: 'message',
    summary,
    viewToken: msg.viewToken || null,
  };
}

function normalizeCompactMarker(value) {
  const marker = (typeof value === 'string' && value.trim()) ? value.trim().toLowerCase() : '';
  if (!marker) return 'unknown';
  if (marker === 'explicit-hook') return marker;
  if (AGENT_COMPACT_FALLBACK_PATTERNS.some(p => p.marker === marker)) return marker;
  return 'unknown';
}

function buildRuntimeCompactEvent(agentName, payload = {}) {
  const now = Date.now();
  const modeRaw = (typeof payload.mode === 'string' && payload.mode.trim()) ? payload.mode.trim().toLowerCase() : 'pattern';
  const mode = modeRaw === 'hook' ? 'hook' : 'pattern';
  const marker = normalizeCompactMarker(payload.marker);
  const summaryInput = (typeof payload.summary === 'string' && payload.summary.trim())
    ? payload.summary.trim()
    : marker.replace(/-/g, ' ');
  const source = (typeof payload.source === 'string' && payload.source.trim())
    ? payload.source.trim()
    : 'runtime';

  return {
    id: `compact_runtime_${agentName}_${now}_${Math.random().toString(36).slice(2, 8)}`,
    ts: now,
    messageId: null,
    agent: agentName,
    mode,
    marker,
    source,
    summary: makeCompactPreview(summaryInput, AGENT_COMPACT_SUMMARY_MAX),
  };
}

function emitRuntimeCompactEvent(agentName, payload = {}) {
  const marker = normalizeCompactMarker(payload?.marker);
  const modeRaw = (typeof payload?.mode === 'string' && payload.mode.trim())
    ? payload.mode.trim().toLowerCase()
    : 'pattern';
  const mode = modeRaw === 'hook' ? 'hook' : 'pattern';

  const event = buildRuntimeCompactEvent(agentName, { ...payload, mode, marker });
  const result = notificationRouter.emit('agent_compact', {
    agentName, marker, mode, sseEvent: 'agent_compact', sseData: event,
  });
  if (!result.accepted) {
    return { ok: true, suppressed: 'dedupe', agent: agentName, marker, mode };
  }
  return { ok: true, event };
}





function safeReadJsonFile(filePath, fallback = {}) {
  try {
    if (!filePath || !existsSync(filePath)) return fallback;
    return JSON.parse(readFileSync(filePath, 'utf-8'));
  } catch {
    return fallback;
  }
}

function safeWriteJsonFile(filePath, payload) {
  if (!filePath) return false;
  const tmpPath = `${filePath}.tmp-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  try {
    mkdirSync(path.dirname(filePath), { recursive: true });
    writeFileSync(tmpPath, `${JSON.stringify(payload, null, 2)}\n`, 'utf-8');
    renameSync(tmpPath, filePath);
    return true;
  } catch (error) {
    try { unlinkSync(tmpPath); } catch {}
    console.warn(`Failed to write JSON ${filePath}: ${error?.message || error}`);
    return false;
  }
}

















function extractTranscriptTextParts(content, out = []) {
  if (typeof content === 'string') {
    const text = normalizeOptionalText(content, 4000);
    if (text) out.push(text);
    return out;
  }
  if (Array.isArray(content)) {
    for (const item of content) extractTranscriptTextParts(item, out);
    return out;
  }
  if (!content || typeof content !== 'object') return out;
  if (content.type === 'text') {
    const text = normalizeOptionalText(content.text, 4000);
    if (text) out.push(text);
    return out;
  }
  if (Object.prototype.hasOwnProperty.call(content, 'content')) {
    extractTranscriptTextParts(content.content, out);
  }
  if (typeof content.text === 'string') {
    const text = normalizeOptionalText(content.text, 4000);
    if (text) out.push(text);
  }
  return out;
}

function extractTranscriptMessageText(row) {
  const text = extractTranscriptTextParts(row?.message?.content || row?.content || []).join('\n');
  return normalizeOptionalText(text.replace(/\s+/g, ' ').trim(), 4000);
}

function inferTranscriptSessionId(transcriptPath) {
  if (!transcriptPath) return null;
  const base = path.basename(String(transcriptPath), path.extname(String(transcriptPath)));
  return normalizeOptionalText(base, 200);
}

function parseClaudeConversationTranscript(sessionId, transcriptPath) {
  const resolvedPath = normalizeWorkspacePath(transcriptPath);
  const parsedSessionId = normalizeOptionalText(sessionId, 200) || inferTranscriptSessionId(resolvedPath);
  const base = {
    sessionId: parsedSessionId || null,
    transcriptPath: resolvedPath || null,
    transcriptExists: Boolean(resolvedPath && existsSync(resolvedPath)),
    transcriptLineCount: 0,
    eventCount: 0,
    userTurnCount: 0,
    assistantTurnCount: 0,
    startedAt: null,
    updatedAt: null,
    latestUserText: '',
    latestAssistantText: '',
    recentTurns: [],
  };
  if (!resolvedPath || !existsSync(resolvedPath)) return base;
  let text = '';
  try {
    text = readFileSync(resolvedPath, 'utf-8');
  } catch {
    return base;
  }
  const recentTurns = [];
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue;
    base.transcriptLineCount += 1;
    let row;
    try {
      row = JSON.parse(line);
    } catch {
      continue;
    }
    const rowSessionId = normalizeOptionalText(row?.sessionId, 200);
    if (parsedSessionId && rowSessionId && rowSessionId !== parsedSessionId) continue;
    if (!base.sessionId && rowSessionId) base.sessionId = rowSessionId;
    base.eventCount += 1;
    const at = normalizeOptionalText(row?.timestamp, 128);
    if (at && !base.startedAt) base.startedAt = at;
    if (at) base.updatedAt = at;
    if (row?.type === 'user' && row?.message?.role === 'user') {
      const preview = extractTranscriptMessageText(row);
      if (preview) {
        base.userTurnCount += 1;
        base.latestUserText = preview.slice(0, 320);
        recentTurns.push({ role: 'user', at: at || null, preview: preview.slice(0, 320) });
      }
      continue;
    }
    if (row?.type === 'assistant' && row?.message?.role === 'assistant') {
      const preview = extractTranscriptMessageText(row);
      if (preview) {
        base.assistantTurnCount += 1;
        base.latestAssistantText = preview.slice(0, 320);
        recentTurns.push({ role: 'assistant', at: at || null, preview: preview.slice(0, 320) });
      }
    }
  }
  base.recentTurns = recentTurns.slice(-8);
  return base;
}


function applyConversationSnapshotToContract(state, sessionSnapshot = null) {
  const contract = state?.contract;
  const conversationState = state?.conversationState;
  const store = conversationState?.store;
  if (!contract?.conversation || !store) return sessionSnapshot || null;
  const sessions = Array.isArray(store.sessions) ? store.sessions : [];
  const current = sessionSnapshot
    || sessions.find((row) => row.sessionId && row.sessionId === store.currentSessionId)
    || sessions.find((row) => row.transcriptPath && row.transcriptPath === store.currentTranscriptPath)
    || sessions[sessions.length - 1]
    || null;
  contract.conversation.kind = store.kind || 'claude-jsonl-session-journal';
  contract.conversation.path = conversationState?.path || null;
  contract.conversation.sessionCount = sessions.length;
  contract.conversation.sessionLimit = normalizePositiveInt(store.sessionLimit, 24);
  contract.conversation.currentSessionId = store.currentSessionId || current?.sessionId || null;
  contract.conversation.currentTranscriptPath = store.currentTranscriptPath || current?.transcriptPath || null;
  contract.conversation.lastSyncedAt = store.lastSyncedAt || null;
  contract.conversation.updatedAt = store.updatedAt || null;
  contract.conversation.current = current
    ? {
        sessionId: current.sessionId || null,
        transcriptPath: current.transcriptPath || null,
        transcriptExists: current.transcriptExists === true,
        transcriptLineCount: current.transcriptLineCount || 0,
        eventCount: current.eventCount || 0,
        userTurnCount: current.userTurnCount || 0,
        assistantTurnCount: current.assistantTurnCount || 0,
        startedAt: current.startedAt || null,
        updatedAt: current.updatedAt || null,
        lastEventAt: current.lastEventAt || null,
        lastHook: current.lastHook || null,
        lastToolName: current.lastToolName || null,
        lastRuntimeAt: current.lastRuntimeAt || null,
        lastRuntimeProvider: current.lastRuntimeProvider || null,
        lastRuntimeModel: current.lastRuntimeModel || null,
        latestUserText: current.latestUserText || '',
        latestAssistantText: current.latestAssistantText || '',
        recentTurns: Array.isArray(current.recentTurns) ? current.recentTurns : [],
      }
    : null;
  return current;
}








function normalizeAttachmentName(value, fallback = 'file') {
  let name = typeof value === 'string' ? value.trim() : '';
  if (!name) name = fallback;
  name = path.basename(name);
  name = name.replace(/[^\w.\-()[\] ]+/g, '_');
  if (!name) name = fallback;
  if (name.length > 120) {
    const ext = path.extname(name);
    const stem = name.slice(0, Math.max(1, 120 - ext.length));
    name = `${stem}${ext}`;
  }
  return name;
}

function normalizeAttachmentMime(value) {
  if (typeof value !== 'string') return null;
  const mime = value.trim().toLowerCase();
  if (!mime) return null;
  if (!/^[a-z0-9!#$&^_.+-]+\/[a-z0-9!#$&^_.+-]+$/.test(mime)) return null;
  return mime;
}

function inferAttachmentKind(rawKind, mime, name) {
  if (rawKind === 'image' || rawKind === 'file') return rawKind;
  if (typeof mime === 'string' && mime.startsWith('image/')) return 'image';
  const lower = String(name || '').toLowerCase();
  if (/\.(png|jpe?g|gif|webp|bmp|svg|avif|heic|heif|tiff?)$/.test(lower)) return 'image';
  return 'file';
}

function normalizeAttachmentInput(raw) {
  const item = (typeof raw === 'string') ? { path: raw } : (raw && typeof raw === 'object' ? raw : null);
  if (!item) return { error: 'invalid attachment item' };

  const pathValue = typeof item.path === 'string' ? item.path.trim() : '';
  if (!pathValue) return { error: 'attachment.path required' };
  if (pathValue.length > 4096) return { error: 'attachment.path too long' };

  const fallbackName = path.basename(pathValue) || 'file';
  const name = normalizeAttachmentName(item.name, fallbackName);
  const mime = normalizeAttachmentMime(item.mime);
  const kind = inferAttachmentKind(item.kind, mime, name);
  const sizeRaw = Number.parseInt(item.size, 10);
  const size = Number.isFinite(sizeRaw) && sizeRaw > 0 ? sizeRaw : null;
  const staged = item.staged === true;
  const sourcePath = (typeof item.source_path === 'string' && item.source_path.trim())
    ? item.source_path.trim().slice(0, 1024)
    : null;

  return {
    value: {
      path: pathValue,
      name,
      mime,
      kind,
      size,
      staged,
      source_path: sourcePath,
    },
  };
}

function isPathWithinRoot(filePath, rootPath) {
  return filePath === rootPath || filePath.startsWith(`${rootPath}${path.sep}`);
}

function resolveReadableMediaPath(rawPath) {
  const requested = typeof rawPath === 'string' ? rawPath.trim() : '';
  if (!requested) return { error: 'path required', status: 400 };
  if (requested.length > 4096) return { error: 'path too long', status: 400 };

  const resolved = path.resolve(requested);
  const allowed = MEDIA_FETCH_ALLOWED_ROOTS.some(rootPath => isPathWithinRoot(resolved, rootPath));
  if (!allowed) return { error: 'path not allowed', status: 403 };

  let stat;
  try {
    stat = statSync(resolved);
  } catch {
    return { error: 'file not found', status: 404 };
  }
  if (!stat.isFile()) return { error: 'path is not a file', status: 400 };
  if (stat.size <= 0) return { error: 'file is empty', status: 400 };
  if (stat.size > MESSAGE_ATTACHMENT_MAX_BYTES) {
    return { error: `file exceeds max bytes (${MESSAGE_ATTACHMENT_MAX_BYTES})`, status: 413 };
  }
  return { value: { path: resolved, size: stat.size } };
}

function guessMimeFromPath(filePath) {
  const ext = path.extname(filePath).toLowerCase();
  switch (ext) {
    case '.png': return 'image/png';
    case '.jpg':
    case '.jpeg': return 'image/jpeg';
    case '.gif': return 'image/gif';
    case '.webp': return 'image/webp';
    case '.bmp': return 'image/bmp';
    case '.svg': return 'image/svg+xml';
    case '.avif': return 'image/avif';
    case '.heic': return 'image/heic';
    case '.heif': return 'image/heif';
    case '.tif':
    case '.tiff': return 'image/tiff';
    case '.pdf': return 'application/pdf';
    case '.txt': return 'text/plain; charset=utf-8';
    case '.md': return 'text/markdown; charset=utf-8';
    case '.json': return 'application/json; charset=utf-8';
    default: return 'application/octet-stream';
  }
}

function inferRecordKind(record) {
  const explicit = typeof record?.kind === 'string' ? record.kind.trim().toLowerCase() : '';
  if (explicit === 'agent' || explicit === 'human') return explicit;

  const hasRegisteredAt = Number(record?.registeredAt) > 0;
  const hasTmux = typeof record?.tmux === 'string' && record.tmux.trim().length > 0;
  const hasServer = Boolean(normalizeServer(record?.server));
  const hasRole = typeof record?.role === 'string' && record.role.trim().length > 0;
  const hasIdentity = typeof record?.identity === 'string' && record.identity.trim().length > 0;
  return (hasRegisteredAt || hasTmux || hasServer || hasRole || hasIdentity) ? 'agent' : 'human';
}

function isAgentRecord(record) {
  return Boolean(record) && inferRecordKind(record) === 'agent';
}

/*
 * Overridable for tests, because the NON-local branch is otherwise unreachable from a test: supertest
 * always connects over loopback and this deliberately ignores X-Forwarded-For (trusting a header any
 * caller can set would let a remote caller claim to be local). The old dashboard process had the same
 * hook for the same reason.
 */
let localRequestOverrideForTest = null;
function setLocalRequestOverrideForTest(fn) {
  localRequestOverrideForTest = typeof fn === 'function' ? fn : null;
}
function isLocalRequest(req) {
  if (localRequestOverrideForTest) return localRequestOverrideForTest(req);
  const ip = req.ip || req.connection?.remoteAddress;
  return LOCALHOST_IPS.has(ip);
}

function hasApiTokenAccess(req) {
  return hasApiTokenAccessAdapter(req, { env: process.env });
}

function getRequestAgentName(req) {
  return getRequestAgentNameAdapter(req, { normalizeAgentName });
}

function authorizeAgentCredential(req, agentName) {
  return authorizeAgentCredentialAdapter(req, agentName, {
    agentTokens,
    normalizeAgentName,
    env: process.env,
  });
}



const requireBearer = createRequireBearer({ env: process.env });

/**
 * Is this request the OPERATOR's, rather than an agent speaking about itself?
 *
 * `POST /api/agents` is guarded by `requireAgentToken`, which deliberately fails OPEN for a name it
 * holds no token for. `checkAgentToken` states the reason and it is a real one — humans, the bridge and
 * system callers register through that same endpoint and have no agent token — so the guard cannot be
 * tightened without breaking them.
 *
 * What CAN be separated is which FIELDS a registration is allowed to set. Registration is mostly an
 * agent describing where it is: its tmux session, its host, its workdir. Four fields are not that. They
 * decide what the agent may cost and what work it may be given, and an agent that could set them would
 * be answering a question that belongs to whoever pays:
 *
 *   - `presetId` decides the CEILING — `remainingFor` reads `preset.ceiling.tokens` through it
 *   - `runtimeProfile` decides the MODEL, and `agentCapability` reads the tier back out of it
 *   - `capability` is that tier stated directly
 *   - `role` is what work the matrix will select the agent for
 *
 * A middleware could not make this distinction: the request is legitimate, only some of its fields are
 * not. So the split is made here, in one place, and each of the four says at its own site what it
 * decides.
 *
 * NO PRODUCTION CALLER LOSES ANYTHING. Verified rather than assumed:
 *
 *   - the agent's own MCP server sends `{name, type, tmux, server}` (`lib/mcp-server-core.js`) — none
 *     of the four;
 *   - `bin/hagency-up` sends name/type/tmux/server plus version and path metadata, and carries the
 *     operator bearer anyway;
 *   - the dashboard's New Agent form does send `presetId` and `role`, but it reaches this endpoint
 *     through `server.js`'s `/api/agents/create`, and `backendFetch` attaches the operator bearer to
 *     every proxied call — so it is an operator request and keeps both fields;
 *   - a provisioned agent (`POST /api/dispatch` → `status: 'provision'`) never needed to declare a role:
 *     the plan encodes it in the NAME and `canonicalRole` reads it back, which is why that format has
 *     the role in it.
 *
 * PASSES WHEN NO `API_TOKEN` IS CONFIGURED, matching `requireBearer` above. A deployment with no
 * operator credential has no way to tell an operator from an agent, and refusing everything there would
 * make the unconfigured case stricter than the configured one — which reads as a broken install rather
 * than as a security posture.
 */
function isOperatorRequest(req) {
  /*
   * `hasApiTokenAccess` rather than a comparison of its own. The first version of this function did
   * `normalizeOptionalText(process.env.API_TOKEN, 512)` and compared that — copied from
   * `_alertTransitionAuth`, and wrong in the way `normalizeSecret`'s comment describes: an API_TOKEN
   * longer than 512 characters would have made this return false for the real operator, silently
   * demoting every registration to an agent's and dropping the four fields it guards. Nobody would have
   * seen a refusal; the fields would just have stopped taking.
   */
  if (!operatorBearerConfigured(process.env)) return true;
  return hasApiTokenAccess(req);
}

function redactPathLikeText(value, maxLen = 1200) {
  const text = normalizeOptionalText(value, maxLen);
  if (!text) return null;
  return text.replace(/(^|[\s(])((?:\/[^/\s)]+)+\/?[^)\s]*)/g, '$1[path removed]');
}

function cloneJsonValue(value) {
  if (value === null || value === undefined) return value ?? null;
  return JSON.parse(JSON.stringify(value));
}

function normalizeDeliveryEvent(raw = {}) {
  const input = (raw && typeof raw === 'object') ? raw : {};
  const now = Date.now();
  const type = normalizeOptionalText(input.type, 128);
  if (!type) return { error: 'type required' };
  const notifyMeta = (input.notifyMeta && typeof input.notifyMeta === 'object' && !Array.isArray(input.notifyMeta))
    ? cloneJsonValue(input.notifyMeta)
    : null;
  const messageId = normalizeOptionalText(
    input.messageId ?? input.message_id ?? input.sourceMsgId ?? notifyMeta?.sourceMsgId,
    255
  );
  const agent = normalizeAgentName(input.agent) || normalizeOptionalText(input.agent, 255);
  const target = normalizeOptionalText(input.target, 255);
  const queueEntryIdRaw = Number(input.queueEntryId ?? input.queue_entry_id);
  const queueEntryId = Number.isFinite(queueEntryIdRaw) && queueEntryIdRaw > 0 ? queueEntryIdRaw : null;
  const queuedAtRaw = Number(input.queuedAt ?? input.queued_at);
  const queuedAt = Number.isFinite(queuedAtRaw) && queuedAtRaw > 0 ? queuedAtRaw : null;
  const tsRaw = Number(input.ts);
  const ts = Number.isFinite(tsRaw) && tsRaw > 0 ? tsRaw : now;
  const attemptId = normalizeOptionalText(input.attemptId ?? input.attempt_id, 512)
    || [messageId || 'unknown-message', agent || target || 'unknown-target', queueEntryId || queuedAt || ts].join(':');
  const rawMessageIds = Array.isArray(input.messageIds)
    ? input.messageIds
    : (Array.isArray(notifyMeta?.messageIds) ? notifyMeta.messageIds : []);
  const messageIds = rawMessageIds.map((id) => normalizeOptionalText(id, 255)).filter(Boolean);
  const targetAgents = Array.isArray(input.targetAgents)
    ? input.targetAgents.map((name) => normalizeAgentName(name) || normalizeOptionalText(name, 255)).filter(Boolean)
    : [];
  const row = {
    id: `devt_${now.toString(36)}_${Math.random().toString(36).slice(2, 10)}`,
    ts,
    type,
    source: normalizeOptionalText(input.source, 128) || 'backend',
  };
  if (messageId) row.messageId = messageId;
  if (messageIds.length) row.messageIds = messageIds;
  if (agent) row.agent = agent;
  if (targetAgents.length) row.targetAgents = targetAgents;
  if (target) row.target = target;
  if (queueEntryId !== null) row.queueEntryId = queueEntryId;
  if (queuedAt !== null) row.queuedAt = queuedAt;
  const deliveredAtRaw = Number(input.deliveredAt ?? input.delivered_at);
  if (Number.isFinite(deliveredAtRaw) && deliveredAtRaw > 0) row.deliveredAt = deliveredAtRaw;
  const ackedAtRaw = Number(input.ackedAt ?? input.acked_at);
  if (Number.isFinite(ackedAtRaw) && ackedAtRaw > 0) row.ackedAt = ackedAtRaw;
  row.attemptId = attemptId;
  const priority = normalizeMessagePriority(input.priority, null);
  if (priority) row.priority = priority;
  const reason = normalizeOptionalText(input.reason, 512);
  if (reason) row.reason = reason;
  const result = normalizeOptionalText(input.result, 128);
  if (result) row.result = result;
  const pathLabel = normalizeOptionalText(input.path, 128);
  if (pathLabel) row.path = pathLabel;
  const stage = normalizeOptionalText(input.stage, 128);
  if (stage) row.stage = stage;
  const statusRaw = Number(input.status);
  if (Number.isFinite(statusRaw) && statusRaw > 0) row.status = statusRaw;
  if (notifyMeta) row.notifyMeta = notifyMeta;
  if (input.cursor && typeof input.cursor === 'object' && !Array.isArray(input.cursor)) {
    row.cursor = cloneJsonValue(input.cursor);
  }
  if (input.context && typeof input.context === 'object' && !Array.isArray(input.context)) {
    row.context = cloneJsonValue(input.context);
  }
  return { row };
}

function appendDeliveryEvent(raw = {}) {
  const normalized = normalizeDeliveryEvent(raw);
  if (normalized.error) return { ok: false, error: normalized.error };
  const row = normalized.row;
  let fd = null;
  try {
    const created = !existsSync(DELIVERY_EVENT_LOG);
    fd = openSync(DELIVERY_EVENT_LOG, 'a', 0o600);
    chmodSync(DELIVERY_EVENT_LOG, 0o600);
    appendFileSync(fd, `${JSON.stringify(row)}\n`);
    fsyncSync(fd);
    if (created) fsyncParentDirectory(DELIVERY_EVENT_LOG);
    if (row.attemptId) deliveryEventAttemptIds.add(row.attemptId);
    return { ok: true, event: row };
  } catch (error) {
    console.warn(`Failed to append delivery event: ${error?.message || error}`);
    return { ok: false, error: error?.message || 'append failed' };
  } finally {
    if (fd !== null) closeSync(fd);
  }
}

const DELIVERY_EVENT_READ_CHUNK_BYTES = 64 * 1024;

function deliveryEventMatches(row, { normalizedMessageId, normalizedAgent }) {
  if (normalizedMessageId) {
    const ids = new Set();
    if (typeof row.messageId === 'string') ids.add(row.messageId);
    if (Array.isArray(row.messageIds)) {
      for (const id of row.messageIds) {
        if (typeof id === 'string') ids.add(id);
      }
    }
    if (!ids.has(normalizedMessageId)) return false;
  }
  if (normalizedAgent) {
    const agentsForRow = new Set();
    if (typeof row.agent === 'string') agentsForRow.add(row.agent);
    if (Array.isArray(row.targetAgents)) {
      for (const name of row.targetAgents) {
        if (typeof name === 'string') agentsForRow.add(name);
      }
    }
    if (!agentsForRow.has(normalizedAgent)) return false;
  }
  return true;
}

function parseDeliveryEventLine(line, filters) {
  if (!line.trim()) return null;
  try {
    const row = JSON.parse(line);
    if (!row || typeof row !== 'object') return null;
    return deliveryEventMatches(row, filters) ? row : null;
  } catch {
    return null;
  }
}

function readDeliveryEvents({ messageId = null, agent = null, limit = 100 } = {}) {
  if (!existsSync(DELIVERY_EVENT_LOG)) return [];
  const normalizedMessageId = normalizeOptionalText(messageId, 255);
  const normalizedAgent = normalizeAgentName(agent) || normalizeOptionalText(agent, 255);
  const boundedLimit = Math.min(1000, Math.max(1, Number.parseInt(limit, 10) || 100));
  const filters = { normalizedMessageId, normalizedAgent };
  const matches = [];
  let fd = null;
  try {
    fd = openSync(DELIVERY_EVENT_LOG, 'r');
    const { size } = statSync(DELIVERY_EVENT_LOG);
    let offset = size;
    let carry = Buffer.alloc(0);
    while (offset > 0 && matches.length < boundedLimit) {
      const bytesToRead = Math.min(DELIVERY_EVENT_READ_CHUNK_BYTES, offset);
      offset -= bytesToRead;
      const buffer = Buffer.allocUnsafe(bytesToRead);
      const bytesRead = readSync(fd, buffer, 0, bytesToRead, offset);
      const chunk = Buffer.concat([buffer.subarray(0, bytesRead), carry]);
      const lineBuffers = [];
      let lineStart = 0;
      for (let i = 0; i < chunk.length; i += 1) {
        if (chunk[i] !== 0x0a) continue;
        lineBuffers.push(chunk.subarray(lineStart, i));
        lineStart = i + 1;
      }
      carry = chunk.subarray(lineStart);
      for (let i = lineBuffers.length - 1; i >= 0 && matches.length < boundedLimit; i -= 1) {
        const row = parseDeliveryEventLine(lineBuffers[i].toString('utf-8'), filters);
        if (row) matches.push(row);
      }
    }
    if (matches.length < boundedLimit && carry.length > 0) {
      const row = parseDeliveryEventLine(carry.toString('utf-8'), filters);
      if (row) matches.push(row);
    }
  } catch {
    return [];
  } finally {
    if (fd !== null) {
      try { closeSync(fd); } catch {}
    }
  }
  return matches.reverse();
}

function buildPersistedUpstreamRecord(kind, record) {
  const safe = cloneJsonValue(record);
  if (!safe || typeof safe !== 'object') return {};

  delete safe.checkedAt;

  if (kind === 'session') {
    delete safe.messageSentAt;
    if (safe.notify && typeof safe.notify === 'object') {
      delete safe.notify.attemptedAt;
      delete safe.notify.messageSentAt;
    }
    return safe;
  }

  if (kind === 'userPrompt') {
    delete safe.attemptedAt;
    delete safe.messageSentAt;
    delete safe.transcriptLineCount;
    delete safe.lastProcessedIndexBefore;
    return safe;
  }

  if (kind === 'preTool') {
    delete safe.attemptedAt;
    delete safe.injectedAt;
    delete safe.newMessageCount;
    delete safe.changedBlockCount;
    delete safe.lastSeenMessageIdBefore;
    delete safe.toolName;
    return safe;
  }

  if (kind === 'stop') {
    delete safe.attemptedAt;
    delete safe.messageSentAt;
    delete safe.transcriptMessageCount;
    delete safe.newMessageCount;
    delete safe.lastProcessedIndexBefore;
    return safe;
  }

  return safe;
}

function buildPersistedUpstreamState(upstream) {
  const safe = cloneJsonValue(upstream);
  if (!safe || typeof safe !== 'object') return {};

  delete safe.checkedAt;
  if (safe.session && typeof safe.session === 'object') safe.session = buildPersistedUpstreamRecord('session', safe.session);
  if (safe.userPrompt && typeof safe.userPrompt === 'object') safe.userPrompt = buildPersistedUpstreamRecord('userPrompt', safe.userPrompt);
  if (safe.preTool && typeof safe.preTool === 'object') safe.preTool = buildPersistedUpstreamRecord('preTool', safe.preTool);
  if (safe.stop && typeof safe.stop === 'object') safe.stop = buildPersistedUpstreamRecord('stop', safe.stop);
  return safe;
}


// ── In-memory state ───────────────────────────────────────────────────
const sseAdapter = createSseAdapter();
const broadcastSSE = sseAdapter.broadcast;
const agents = loadJsonSync('agents.json', {});
// Migrate agents missing environment field
{ let migrated = 0;
  for (const a of Object.values(agents)) {
    if (a && typeof a === 'object' && !a.environment) { a.environment = classifyEnvironment(a.name); migrated++; }
  }
  if (migrated > 0) { saveJson('agents.json', agents, { immediate: true }); console.log(`[startup] migrated environment for ${migrated} agent(s)`); }
}
loadAgentTokens();
const deletedAgentTombstones = loadJsonSync('deleted_agents.json', {});
const groups = loadJsonSync('groups.json', {});
const workflowBindings = loadJsonSync('workflow_bindings.json', {});
const messages = loadJsonSync('messages.json', []);
let unreadMessageIndexVersion = 0;
const unreadMessageIndex = {
  version: -1,
  directByAgent: new Map(),
  groupByName: new Map(),
  groupMentionsByAgent: new Map(),
};
const cursors = loadJsonSync('cursors.json', {});
const servers = loadJsonSync('servers.json', {});
const agentRuntime = loadJsonSync('agent_runtime.json', {});
const taskGraphs = loadJsonSync('task_graphs.json', {});
const frameworkPresets = loadJsonSync('framework-presets.json', []);
function saveFrameworkPresets() { return saveJson('framework-presets.json', frameworkPresets); }

/*
 * Operator declarations about seats, keyed by derived seat id.
 *
 * Only the QUOTA is stored. Which agents share a seat is derived from how they
 * were launched (lib/seat-store.js), so persisting that would be persisting a
 * cache of a fact. What cannot be derived is how many tokens a subscription
 * includes — no provider exposes it and nothing here meters — so that is a
 * declaration, recorded as one.
 */
const seatDeclarations = loadJsonSync('seats.json', {});
function saveSeatDeclarations() { return saveJson('seats.json', seatDeclarations); }

/*
 * Engagements, offers and the whitelist, in one store because they are one
 * decision: whether a project may draw on this contributor's capacity, and for how
 * much. Splitting them would put the routing rule — which reads all three — outside
 * whatever holds the data.
 */
const engagementStore = createEngagementStore({
  load: () => loadJsonSync('engagements.json', {}),
  persist: (state) => saveJson('engagements.json', state),
});
const resourceAgentDefinitions = createResourceAgentDefinitions({
  presets: frameworkPresets, save: saveFrameworkPresets, agents: () => agents,
  engagements: () => engagementStore.list(),
  qualifies: (preset, role) => ['claude', 'codex'].includes(preset.framework)
    && ROLES.includes(role) && resourcesForRole(role, ROLE_DEFAULT_TIER[role], [preset]).length > 0,
});
const matrixWorkStore = createMatrixWorkStore({
  load: () => loadJsonSync('matrix-work.json', []),
  persist: (rows) => saveJson('matrix-work.json', rows, { immediate: true }),
});

/*
 * Measured consumption, persisted, because the transcripts it is read from are not ours.
 *
 * The coding CLIs rotate, prune and delete their own session files. Computing usage on
 * demand means a figure that silently drops when one goes away, while the ceiling was
 * still spent by that work. The ledger keeps a high-water mark per session, so the total
 * survives a source disappearing and is idempotent under re-reading — see
 * lib/metering/ledger.js for why an appended snapshot would have double-counted.
 */
const usageLedger = createUsageLedger({
  load: () => loadJsonSync('usage-ledger.json', {}),
  persist: (state) => saveJson('usage-ledger.json', state),
});

/*
 * Invitations a project has extended that the contributor has not answered (ADR-014).
 *
 * The bridge owns the Matrix state and pushes here; this is the copy the console reads. Same
 * shape as approval-bindings, for the same reason: the console talks to the backend, and a
 * bridge-owned fact has to cross that line somewhere.
 */
const pendingInviteStore = createPendingInviteStore({
  load: () => loadJsonSync('pending-invites.json', {}),
  persist: (state) => saveJson('pending-invites.json', state),
});

/*
 * Deployment-local key material for the seat digest, and its key id for rotation.
 *
 * Absent by default, and the digest says so in its own key id rather than
 * pretending to be keyed. An unkeyed hash over (server, framework, authMode) is
 * trivially reversible — the input set is tiny — so an unkeyed value must never be
 * mistaken for one that protects anything.
 */
const SEAT_KEY_SECRET = String(process.env.HAGENCY_SEAT_KEY || '').trim();
const SEAT_KEY_ID = String(process.env.HAGENCY_SEAT_KEY_ID || 'default').trim() || 'default';

/** Ceiling on a preset: the field the contributor is actually deciding. */
function normalizeCeiling(value) {
  if (value === null || value === undefined) return null;
  if (typeof value !== 'object') return null;
  const n = (v) => (Number.isFinite(Number(v)) && Number(v) > 0 ? Math.floor(Number(v)) : null);
  const tokens = n(value.tokens);
  if (tokens === null) return null;
  const period = ['daily', 'monthly'].includes(value.period) ? value.period : 'monthly';
  return {
    tokens,
    period,
    // Null and 0 are different: "no rate cap set" versus "nothing per day".
    rateCapPerDay: n(value.rateCapPerDay),
    /*
     * Always false, and stored rather than assumed by the reader. Nothing meters
     * tokens at any granularity, so this ceiling is a declaration of intent. A
     * client that treats it as a guard rail will over-promise and learn about it
     * from an exhausted plan. When metering lands, this flips at the store rather
     * than in every caller.
     */
    enforced: false,
  };
}
const taskStoreData = loadJsonSync('tasks.json', []);
const routerStore = (ROUTER_TASK_CUTOVER_ENABLED || THREAD_SESSIONS_ENABLED || ROUTER_SHADOW_ENABLED)
  ? openRouter({ dbPath: path.join(DATA_DIR, 'router.db') })
  : null;
let taskStore;
if (ROUTER_TASK_CUTOVER_ENABLED) {
  const migration = migrateLegacyTasks(routerStore, taskStoreData);
  if (!migration.replayed && existsSync(dataPath('tasks.json'))) {
    const backupName = `tasks.json.pre-router-${Date.now()}.bak`;
    copyFileSync(dataPath('tasks.json'), dataPath(backupName));
    chmodSync(dataPath(backupName), 0o600);
    console.log(`[router] task store cut over to SQLite; backup=${backupName} count=${migration.importedCount}`);
  }
  taskStore = createRouterTaskStore(routerStore);
} else {
  taskStore = createTaskStore({
    initialData: taskStoreData,
    save: (data) => saveJson('tasks.json', data),
  });
}
const worktreeManager = THREAD_SESSIONS_ENABLED ? new WorktreeManager() : null;
if (THREAD_SESSIONS_ENABLED) {
  const report = routerStore.reconcileOnStart();
  if (report.requeued || report.outcomeUnknown || report.expiredMatrixClaims) {
    console.warn(`[router] startup reconciliation requeued=${report.requeued} outcome_unknown=${report.outcomeUnknown} matrix_claims=${report.expiredMatrixClaims}`);
  }
}
const projectInspector = createProjectInspector();
const supervisorSnapshotData = loadJsonSync('supervisor_snapshots.json', {});
const supervisorSnapshotStore = createSupervisorSnapshotStore({
  initialData: supervisorSnapshotData,
  save: (data) => saveJson('supervisor_snapshots.json', data),
});
const alertStoreData = loadJsonSync('alerts.json', []);
const alertStore = createAlertStore({
  initialData: alertStoreData,
  save: (data) => saveJson('alerts.json', data),
  emitEvent: (eventName, alert) => broadcastSSE(eventName, alert),
});
const approvalStore = new ApprovalStore(path.join(DATA_DIR, 'approvals.json'), {
  ttlMs: APPROVAL_TTL_MS,
  isTaskActive: id => Boolean(id && ['created', 'accepted', 'in_progress', 'blocked'].includes(taskStore.getTask(id)?.status)),
  getTaskEpoch: id => taskStore.getExecutionEpoch?.(id) ?? null,
  isAgentCurrent: (name, id) => isAgentRecord(agents[name]) && executionAgentIdentity(agents[name]) === id,
});
/*
 * 项目方 — the project sides Hagency is registered with (ADR-016 decision 1).
 *
 * Separate from `approvals.json` rather than a section inside it, because this file holds
 * CREDENTIALS: an `as_token` granting a whole namespace on a homeserver we do not administer.
 * A distinct file keeps that surface small enough to reason about, and the store writes it 0600.
 */
const projectSideStore = new ProjectSideStore(path.join(DATA_DIR, 'project-sides.json'));
/*
 * The namespace an appservice registration claims by default. Formalises the existing
 * MATRIX_AGENT_PREFIX rather than changing it (ADR-014 decision 2) — the bridge already names agent
 * accounts `ac_<name>`, so `@ac_.*` describes today's fleet instead of requiring a rename.
 */
const MATRIX_AGENT_PREFIX_FOR_REGISTRATION = (process.env.MATRIX_AGENT_PREFIX || 'ac_').trim();
function agentPrefixOnSide(side, credential = undefined) {
  return projectSideAgentPrefix({ side,
    credential: credential === undefined && side?.id ? projectSideStore.credentialFor(side.id) : credential,
  }, MATRIX_AGENT_PREFIX_FOR_REGISTRATION);
}
const agentOpsService = AGENT_OPS_CLIENT_ENABLED ? new AgentOpsService(routerStore) : null;
const agentOpsServerIdentity = AGENT_OPS_CLIENT_ENABLED
  ? loadAgentOpsServerIdentity(path.join(DATA_DIR, 'agent-ops-server-identity.json'))
  : null;
if (agentOpsService && agentOpsServerIdentity) {
  const identityBinding = agentOpsService.bindServerIdentity(agentOpsServerIdentity.fingerprint);
  if (identityBinding.rotated) {
    console.warn(`[agent-ops] server identity rotated; revoked ${identityBinding.revokedScopes} scoped client scope(s)`);
  }
}
if (THREAD_SESSIONS_ENABLED) {
  for (const decision of approvalStore.listRequests({ status: 'consumed', upstream_request_prefix: 'tss_' })) {
    if (!decision.decision_event_id || !decision.upstream_request_id || !decision.decision) continue;
    const reconciled = routerStore.reconcileApprovalDecision({
      approvalId: decision.upstream_request_id,
      decisionEventId: decision.decision_event_id,
      decision: decision.decision,
    });
    if (!reconciled.ok && reconciled.code !== 'approval_mismatch') {
      console.warn(`[router] approval reconciliation failed: ${reconciled.code}: ${reconciled.message}`);
    }
  }
}
const liveThreadSessionRunners = new Map();
let routerPumpMicrotaskQueued = false;
let routerPumpTimer = null;
let routerPumpDueAt = null;
let routerPumpRunning = false;
let routerPumpAccepting = true;

function routerRefusalStatus(result) {
  if (!result || result.ok !== false) return 500;
  if (result.code === 'not_found') return 404;
  if (result.code === 'idempotency_conflict' || result.code === 'matrix_command_conflict') return 409;
  if (result.code === 'remote_runner_unsupported' || result.code === 'unsupported_framework') return 422;
  if (result.code === 'workspace_quarantined') return 423;
  if (result.code === 'inspection_required') return 409;
  if (result.code === 'inspection_expired') return 410;
  if (result.code === 'resource_unavailable' || result.code === 'live_runner_cap') return 503;
  return 400;
}

function threadSessionFramework(agent) {
  return String(agent?.runtimeProfile?.primary?.framework || agent?.type || '').trim().toLowerCase();
}

function threadSessionAgentEligibility(agent, { forProjection = false } = {}) {
  if (!forProjection && (agent?.manualDown || agent?.stopUnconfirmedDispatches?.length)) return { ok: false, code: 'agent_stopped', message: 'agent was stopped by its operator' };
  if (agent?.retiredAt || agent?.projectSide && !projectSideStore.getSide(agent.projectSide)?.active) {
    return { ok: false, code: 'agent_retired', message: 'agent is retired or its project side is inactive' };
  }
  const framework = threadSessionFramework(agent);
  if (framework !== 'claude' && framework !== 'codex') {
    return {
      ok: false,
      code: 'unsupported_framework',
      message: framework === 'octos'
        ? 'Octos does not support disposable thread-session runners'
        : `unsupported thread-session framework: ${framework || 'missing'}`,
    };
  }
  const serverId = normalizeServer(agent?.server);
  if (serverId && !isLocalAgentServer(serverId, LOCAL_SERVER_ID)) {
    return { ok: false, code: 'remote_runner_unsupported', message: 'thread-session runners are local-only in v1' };
  }
  return { ok: true, framework, serverId };
}

// Absence of a pane alone is not a runner declaration. Keep explicit legacy
// transports (including unknown future transports) out of this projection.
function isOnDemandThreadSessionAgent(agent) {
  const absent = value => value == null || (typeof value === 'string' && !value.trim());
  return THREAD_SESSIONS_ENABLED && isAgentRecord(agent)
    && Boolean(normalizeAgentId(agent.agentId))
    && absent(agent.transport)
    && absent(agent.tmux)
    && threadSessionAgentEligibility(agent, { forProjection: true }).ok === true;
}

// Thread routing also serves agents with a legacy terminal. Ledger activity is
// independent of that terminal's liveness and of permission to start a runner.
function serializeThreadSessionDispatchActivity(agent) {
  if (!THREAD_SESSIONS_ENABLED || !isAgentRecord(agent)
      || !normalizeAgentId(agent.agentId) || !threadSessionAgentEligibility(agent, { forProjection: true }).ok) return null;
  try {
    const counts = routerStore.agentDispatchActivity(agent.agentId);
    return {
      source: 'router-ledger',
      activity: counts.parkedDispatchCount > 0 ? 'parked'
        : counts.activeDispatchCount > 0 ? 'running'
          : counts.queuedDispatchCount > 0 ? 'queued' : 'idle',
      activeDispatchCount: counts.activeDispatchCount,
      queuedDispatchCount: counts.queuedDispatchCount,
      parkedDispatchCount: counts.parkedDispatchCount,
    };
  } catch {
    return { source: 'router-ledger', activity: 'unknown', activeDispatchCount: null,
      queuedDispatchCount: null, parkedDispatchCount: null };
  }
}

function serializeThreadSessionRunner(agent, dispatchActivity) {
  if (!isOnDemandThreadSessionAgent(agent)) return null;
  const model = agent.runtimeProfile?.primary?.model || null;
  const result = {
    mode: 'on-demand', availability: 'ready', reason: null,
    framework: threadSessionFramework(agent), activity: 'idle',
    activeDispatchCount: 0, queuedDispatchCount: 0, parkedDispatchCount: 0,
    model, modelSource: model ? 'runtime-profile' : 'provider-default',
  };
  let reason = null;
  if (agent.manualDown === true || isManualDownReason(agent.offlineReason)) reason = 'manual-stop';
  else if (agent.offlineReason && agent.offlineReason !== 'tmux-missing:auto') reason = 'runtime-unavailable';
  else if (!routerPumpAccepting) reason = 'router-stopping';
  else if (!agentTokens.get(agent.name)) reason = 'credential-unavailable';
  else if (agent.workspaceMode === 'worktree' && !normalizeWorkspacePath(agent.worktreesDir)) reason = 'workspace-unavailable';
  else {
    const workspace = normalizeWorkspacePath(agent.workdir || agent.homeDir);
    try {
      if (!workspace || !statSync(workspace).isDirectory()) reason = 'workspace-unavailable';
    } catch { reason = 'workspace-unavailable'; }
  }
  if (reason) { result.availability = 'unavailable'; result.reason = reason; }
  // Counts describe durable dispatches, not independently probed OS processes.
  result.activity = dispatchActivity.activity;
  result.activeDispatchCount = dispatchActivity.activeDispatchCount;
  result.queuedDispatchCount = dispatchActivity.queuedDispatchCount;
  result.parkedDispatchCount = dispatchActivity.parkedDispatchCount;
  if (dispatchActivity.activity === 'unknown' && !reason) {
    result.availability = 'unknown'; result.reason = 'dispatch-state-unavailable';
  }
  return result;
}

function isFrontDeskAgent(agent) {
  return agentRole(agent) === 'architect';
}

// The lease protects a DIRECTORY, so directory identity — and nothing else —
// must determine the resource id. Mixing the agent id in minted a separate
// resource per agent for one shared workdir, which let two writing runners
// into the same tree while each believed its lease was exclusive
// (REQ-TSS-WORKSPACE-LEASE). Symlinks are resolved so two spellings of one
// directory cannot slip past either.
// Returns both the resource id and the canonical path it was derived from. The
// caller MUST register the workspace under the SAME canonical path: the lease
// resource is keyed on the id (hash of realpath), and registerWorkspace refuses
// a resource whose backend_path changes, so registering the raw path would make
// two symlink spellings of one directory collide on the id yet disagree on the
// path — a permanent registration failure for whichever agent arrived second.
function safeWorkspaceResource(workspacePath) {
  let canonicalPath = workspacePath;
  try {
    canonicalPath = realpathSync(workspacePath);
  } catch {
    // Callers only reach here for a path that already passed existsSync, so a
    // resolution failure is exotic; hashing the given path still collides
    // correctly for identical spellings, which is the common case.
  }
  const suffix = createHash('sha256').update(canonicalPath).digest('hex').slice(0, 20);
  return { resourceId: `workspace:${suffix}`, canonicalPath };
}

async function resolveThreadSessionWorkspace(agent, threadRootEventId, mayWrite) {
  const repositoryPath = normalizeWorkspacePath(agent?.workdir || agent?.homeDir);
  if (!repositoryPath || !existsSync(repositoryPath)) {
    return { ok: false, code: 'bad_request', message: 'agent has no existing managed workspace' };
  }
  if (mayWrite && agent.workspaceMode === 'worktree') {
    if (!threadRootEventId || !agent.worktreesDir) {
      return { ok: false, code: 'bad_request', message: 'worktree mode requires a thread root and worktrees_dir' };
    }
    try {
      const info = await worktreeManager.ensureAsync({
        repositoryPath,
        worktreesDir: agent.worktreesDir,
        agentId: agent.agentId,
        threadRootEventId,
        bootstrap: agent.worktreeBootstrap,
      });
      routerStore.registerWorkspace({
        resourceId: info.resourceId,
        safeLabel: info.safeLabel,
        backendPath: info.path,
        branchName: info.branch,
      });
      return { ok: true, resourceId: info.resourceId, cwd: info.path, workspaceMode: 'worktree' };
    } catch (error) {
      return { ok: false, code: 'resource_unavailable', message: `worktree unavailable: ${error?.message || error}` };
    }
  }
  const { resourceId, canonicalPath } = safeWorkspaceResource(repositoryPath);
  routerStore.registerWorkspace({
    resourceId,
    // The label names the directory, not the agent: several agents may share
    // this one resource, so an agent-specific label would be misleading in
    // whichever of them happened to register it first.
    safeLabel: `${path.basename(canonicalPath) || 'workspace'}/shared`,
    // Register the canonical path so two symlink spellings that resolve here
    // agree on backend_path, not just on the id.
    backendPath: canonicalPath,
  });
  // Launch in the canonical directory too, so the runner writes exactly where
  // the lease guards, closing the enqueue→launch symlink-swap window.
  return { ok: true, resourceId, cwd: canonicalPath, workspaceMode: 'shared' };
}

async function enqueueThreadSessionDispatch({ agent, sessionId, taskId = null, threadRootEventId = null }) {
  const eligible = threadSessionAgentEligibility(agent);
  if (!eligible.ok) return eligible;
  const { framework, serverId } = eligible;
  // An operator's /thread mode auto grant lifts the front-desk read-only
  // default for this one session; the store still verifies the grant before
  // accepting a write dispatch, so this stays a hint, not an authority.
  const sessionInfo = routerStore.sessionById(sessionId);
  const mayWrite = !isFrontDeskAgent(agent) || sessionInfo?.modeOverride === 'auto';
  const workspace = await resolveThreadSessionWorkspace(agent, threadRootEventId, mayWrite);
  if (!workspace.ok) return workspace;
  const result = routerStore.enqueueDispatch({
    sessionId,
    taskId,
    framework,
    serverId,
    localServerId: LOCAL_SERVER_ID,
    workspaceMode: workspace.workspaceMode,
    workspaceResourceId: workspace.resourceId,
    mayWrite,
    payload: {
      kind: mayWrite ? 'task_turn' : 'front_desk_turn',
      rebuildTokenBudget: REBUILD_TOKEN_BUDGET,
    },
  });
  if (result.ok) scheduleRouterPump();
  return result;
}

const THREAD_DIRECTIVE_MODEL_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;

// Returns null when the message is not a /thread directive; otherwise an
// object with either {model?, mode?} (valid) or {error} (malformed, so the
// sender gets a refusal instead of the text silently becoming chat input).
function parseThreadSessionDirective(bodyRaw) {
  // Mention pills arrive as markdown links or bare @names; strip both so the
  // directive can lead the visible text.
  const body = String(bodyRaw || '')
    .replace(/\[[^\]]*\]\([^)]*\)/g, ' ')
    .trim()
    .replace(/^(?:@[\w.-]+[:,]?\s+)+/, '')
    .trim();
  if (!/^\/thread\b/i.test(body)) return null;
  const usage = 'usage: /thread model <name|default> | /thread mode <plan|auto>';
  const match = body.match(/^\/thread\s+(\S+)(?:\s+(\S+))?\s*$/i);
  if (!match) return { error: usage };
  const [, keyRaw, valueRaw] = match;
  const key = keyRaw.toLowerCase();
  if (key === 'model') {
    if (!valueRaw) return { error: usage };
    if (/^(default|reset|clear)$/i.test(valueRaw)) return { model: null };
    if (!THREAD_DIRECTIVE_MODEL_PATTERN.test(valueRaw)) {
      return { error: 'model must be a plain model name or alias (max 64 chars)' };
    }
    return { model: valueRaw };
  }
  if (key === 'mode') {
    const mode = (valueRaw || '').toLowerCase();
    if (mode === 'plan' || mode === 'auto') return { mode };
    if (/^(default|reset|clear)$/.test(mode)) return { mode: null };
    return { error: usage };
  }
  return { error: usage };
}

function threadSessionDirectiveConfirmation(sessionInfo) {
  const model = sessionInfo.modelOverride ? `model=${sessionInfo.modelOverride}` : 'model=default';
  const mode = sessionInfo.modeOverride ? `mode=${sessionInfo.modeOverride}` : 'mode=default (read-only)';
  const writeWarning = sessionInfo.modeOverride === 'auto'
    ? ' Runners in this thread may now write to the agent workspace (writes stay serialized by workspace lease).'
    : '';
  return `Thread session updated: ${model}, ${mode}.${writeWarning}`;
}

function makePromotedTaskTitle(body) {
  let title = String(body || '').trim();
  // Only remove leading address/command syntax from this display projection.
  // Native clients may place an escaped Markdown mention before /task; deleting
  // @tokens globally corrupts its URL and unrelated email, code and prose.
  let previous;
  do {
    previous = title;
    title = title
      .replace(/^\/task(?=\s|$)\s*/i, '')
      .replace(/^\[(?:\\.|[^\]\\])*\]\(https:\/\/matrix\.to\/#\/(?:@|%40)[^)\s]+\)(?:\s*[:,]\s*|\s+|$)/i, '')
      .replace(/^@[A-Za-z0-9_.-]+(?::[A-Za-z0-9.-]+(?::[0-9]+)?)?(?:\s*[:,]\s*|\s+|$)/, '');
  } while (title !== previous);
  return (title.split('\n').find((line) => line.trim()) || 'Matrix task').trim().slice(0, 255);
}

function shadowMatrixMessageToRouter(agentName, msg) {
  if (!ROUTER_SHADOW_ENABLED || THREAD_SESSIONS_ENABLED || !routerStore) {
    return { ok: true, shadowed: false, reason: 'shadow_disabled' };
  }
  const agent = agents[agentName];
  if (!isAgentRecord(agent) || !agent.agentId) {
    return { ok: true, shadowed: false, reason: 'unstable_agent_identity' };
  }
  const eligible = threadSessionAgentEligibility(agent);
  if (!eligible.ok) return { ok: true, shadowed: false, reason: eligible.code };
  const roomId = msg?.matrixContext?.roomId;
  const matrixEventId = msg?.matrixContext?.eventId;
  if (!roomId || !matrixEventId) {
    return { ok: true, shadowed: false, reason: 'missing_matrix_identity' };
  }
  const result = routerStore.ingestMessage({
    messageId: msg.id,
    roomId,
    matrixEventId,
    threadRootEventId: msg?.matrixContext?.threadRootEventId || null,
    senderMxid: msg.senderMxid,
    senderName: msg.from,
    recipientAgentId: agent.agentId,
    recipientAgentName: agent.name,
    normalizedBody: msg.full || msg.summary,
    receivedAt: msg.ts,
    explicitTask: /^\s*\/task\b/i.test(msg.full || msg.summary || ''),
  });
  if (!result.ok) return { ...result, shadowed: false };
  return { ok: true, shadowed: true, created: result.created, sessionId: result.session.sessionId };
}

async function routeMatrixMessageToThreadSession(agentName, msg) {
  if (!THREAD_SESSIONS_ENABLED) return { ok: true, legacy: true };
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return { ok: false, code: 'not_found', message: `agent not found: ${agentName}` };
  if (!agent.agentId) {
    return { ok: false, code: 'bad_request', message: `agent '${agentName}' has no stable v1 manifest id` };
  }
  const eligible = threadSessionAgentEligibility(agent);
  if (!eligible.ok) return eligible;
  const roomId = msg?.matrixContext?.roomId;
  const matrixEventId = msg?.matrixContext?.eventId;
  let threadRootEventId = msg?.matrixContext?.threadRootEventId || null;
  const direct = roomId ? routerStore.conversations.direct(roomId, agentName) : null;
  if (direct) {
    await authorizeDirectRoom(agentName, msg.senderMxid, roomId);
    if (direct.mode === 'group') {
      threadRootEventId = threadRootEventId && threadRootEventId !== direct.privateRootEventId ? threadRootEventId : null;
      // A public front-desk turn must never resume its former private main session.
      if (!threadRootEventId && isFrontDeskAgent(agent)) threadRootEventId = matrixEventId;
    } else {
      const root = routerStore.conversations.directRoot(roomId, matrixEventId, agentName);
      threadRootEventId = root === matrixEventId ? null : root;
    }
  }
  if (!roomId || !matrixEventId) {
    return { ok: false, code: 'bad_request', message: 'thread-session routing requires authenticated Matrix room and event ids' };
  }
  const directive = parseThreadSessionDirective(msg.full || msg.summary || '');
  if (directive) {
    // A bad directive must not fail message delivery (the bridge would post a
    // scary delivery-failure warning into the room). Consume it and answer
    // with an in-thread notice instead.
    const consumeWithNotice = (body) => {
      routerStore.queueSessionNotice({
        roomId,
        threadRootEventId,
        senderAgentName: agent.name,
        dedupeKey: `thread-directive-help:${matrixEventId}`,
        body,
      });
      scheduleRouterPump();
      return { ok: true, directive: true, applied: false };
    };
    if (msg.trustLevel !== 'operator') {
      return consumeWithNotice('/thread directives require operator trust; the message was not delivered as chat.');
    }
    if (directive.error) {
      return consumeWithNotice(directive.error);
    }
    const applied = routerStore.setSessionOverrides({
      agentId: agent.agentId,
      agentName: agent.name,
      roomId,
      threadRootEventId,
      ...(directive.model !== undefined ? { model: directive.model } : {}),
      ...(directive.mode !== undefined ? { mode: directive.mode } : {}),
      requestedBy: msg.senderMxid || msg.from,
    });
    // Success is a SessionView (no ok field); only an explicit refusal aborts.
    if (applied.ok === false) return applied;
    appendDeliveryEvent({
      type: 'message.thread_directive_applied',
      source: 'backend',
      messageId: msg.id,
      agent: agentName,
      context: {
        modelOverride: applied.modelOverride,
        modeOverride: applied.modeOverride,
        requestedBy: msg.senderMxid || msg.from,
      },
    });
    routerStore.queueSessionNotice({
      roomId,
      threadRootEventId,
      senderAgentName: agent.name,
      dedupeKey: `thread-directive:${matrixEventId}`,
      body: threadSessionDirectiveConfirmation(applied),
    });
    scheduleRouterPump();
    // Directives configure the session; they are not conversation input.
    return { ok: true, directive: true };
  }
  const explicitTask = /^\s*\/task\b/i.test(msg.full || msg.summary || '');
  if (explicitTask && isFrontDeskAgent(agent)) {
    const workerTargets = [...new Set((Array.isArray(msg.mentions) ? msg.mentions : [])
      .filter((name) => name !== agent.name && isAgentRecord(agents[name]) && !isFrontDeskAgent(agents[name])))];
    if (workerTargets.length !== 1) {
      return {
        ok: false,
        code: 'bad_request',
        message: 'explicit /task must address exactly one registered coding worker',
      };
    }
    return { ok: true, delegatedExplicitTaskTo: workerTargets[0] };
  }
  const authenticatedInput = {
    messageId: msg.id,
    roomId,
    matrixEventId,
    threadRootEventId,
    senderMxid: msg.senderMxid,
    senderName: msg.from,
    recipientAgentId: agent.agentId,
    recipientAgentName: agent.name,
    normalizedBody: msg.full || msg.summary,
    receivedAt: msg.ts,
    explicitTask,
  };

  const createWorkerTask = () => {
    const stored = routerStore.storeTaskMessage(authenticatedInput);
    if (!stored.ok) return stored;
    return routerStore.createTaskIntent({
      requestScope: `matrix-direct:${agent.agentId}`,
      requestKey: matrixEventId,
      roomId,
      threadRootEventId: threadRootEventId || matrixEventId,
      rootMessageId: msg.id,
      inputMessageIds: [msg.id],
      task: {
        title: makePromotedTaskTitle(msg.full || msg.summary),
        description: msg.full || msg.summary,
        assigneeAgentId: agent.agentId,
        assigneeName: agent.name,
        createdBy: msg.senderMxid || msg.from,
      },
      acknowledgementBody: `Task created for @${agent.name}: ${makePromotedTaskTitle(msg.full || msg.summary)}`,
    });
  };

  if (!threadRootEventId && !isFrontDeskAgent(agent)) return createWorkerTask();

  if (threadRootEventId && !isFrontDeskAgent(agent)) {
    const existing = routerStore.findThreadTaskBinding(agent.agentId, roomId, threadRootEventId);
    if (existing?.ok === false) return existing;
    if (!existing) {
      // A human can address a new worker inside another worker's thread. It
      // needs its own task and confirmed Matrix anchor, never the peer's task
      // credential. Keep the source event/root so durable retry is identical.
      if (msg.type !== 'human' || !/^@[^:]+:.+/.test(msg.senderMxid || '')
        || !msg.mentions?.includes(agentName)) {
        return { ok: false, code: 'missing_task_binding', message: 'joining a worker to a thread requires an authenticated human mention' };
      }
      return createWorkerTask();
    }
    if (existing.activationState === 'pending_thread') {
      // Sync can deliver more input before the acknowledgement is confirmed.
      // Keep it dormant on that same task; activation projects all inputs.
      const stored = routerStore.storeTaskMessage(authenticatedInput);
      if (!stored.ok) return stored;
      return routerStore.attachTaskInputs({ taskId: existing.taskId,
        requestScope: `matrix-thread:${agent.agentId}`, requestKey: matrixEventId, messageIds: [msg.id] });
    }
    const binding = routerStore.findActiveTaskBinding(agent.agentId, roomId, threadRootEventId);
    if (!binding.ok && binding.code) return binding;
    const ingested = routerStore.ingestMessage(authenticatedInput);
    if (!ingested.ok) return ingested;
    const attached = routerStore.attachTaskInputs({
      taskId: binding.taskId,
      requestScope: `matrix-thread:${agent.agentId}`,
      requestKey: matrixEventId,
      messageIds: [msg.id],
    });
    if (!attached.ok) return attached;
    return await enqueueThreadSessionDispatch({
      agent,
      sessionId: binding.sessionId,
      taskId: binding.taskId,
      threadRootEventId,
      prompt: msg.full || msg.summary,
    });
  }

  const ingested = routerStore.ingestMessage(authenticatedInput);
  if (!ingested.ok) return ingested;
  return await enqueueThreadSessionDispatch({
    agent,
    sessionId: ingested.session.sessionId,
    threadRootEventId,
    prompt: msg.full || msg.summary,
  });
}

const THREAD_SESSION_MESSAGE_CURSOR = 'messages.json:matrix-thread-router-v1';

function threadSessionMessageCursorValue(msg) {
  return JSON.stringify({ ts: Number(msg?.ts) || 0, id: typeof msg?.id === 'string' ? msg.id : '' });
}

function parseThreadSessionMessageCursor(value) {
  try {
    const parsed = JSON.parse(value);
    return {
      ts: Number(parsed?.ts) || 0,
      id: typeof parsed?.id === 'string' ? parsed.id : '',
    };
  } catch {
    throw new Error('thread-session message ingestion cursor is invalid');
  }
}

function isThreadSessionMatrixSourceMessage(msg) {
  return msg?.source === 'matrix'
    && typeof msg?.id === 'string'
    && typeof msg?.matrixContext?.roomId === 'string'
    && typeof msg?.matrixContext?.eventId === 'string';
}

async function routePersistedMatrixMessageToThreadSessions(msg) {
  const directTargetKind = msg?.to && isAgentRecord(agents[msg.to]) ? 'agent' : null;
  for (const agentName of deliveryTargetAgentsForMessage(msg, directTargetKind)) {
    const routed = await routeMatrixMessageToThreadSession(agentName, msg);
    if (!routed.ok) return routed;
  }
  return { ok: true };
}

async function reconcileThreadSessionSourceMessages() {
  if (!THREAD_SESSIONS_ENABLED) return { scanned: 0, initialized: false };
  const sourceMessages = messages.filter(isThreadSessionMatrixSourceMessage);
  const initialValue = threadSessionMessageCursorValue(sourceMessages[sourceMessages.length - 1]);
  const initialized = routerStore.initializeIngestionCursor(
    THREAD_SESSION_MESSAGE_CURSOR,
    initialValue,
  );
  if (initialized.created) return { scanned: 0, initialized: true };
  const cursor = parseThreadSessionMessageCursor(initialized.value);
  const startIndex = firstMessageAfterCursorIndex(sourceMessages, cursor.ts, cursor.id);
  let scanned = 0;
  for (const msg of sourceMessages.slice(startIndex)) {
    const routed = await routePersistedMatrixMessageToThreadSessions(msg);
    if (!routed.ok) {
      throw new Error(`thread-session source reconciliation refused message ${msg.id}: ${routed.code}: ${routed.message}`);
    }
    routerStore.advanceIngestionCursor(THREAD_SESSION_MESSAGE_CURSOR, threadSessionMessageCursorValue(msg));
    scanned += 1;
  }
  return { scanned, initialized: false };
}

async function reconcileThreadSessionPeerMessages() {
  if (!THREAD_SESSIONS_ENABLED) return { queued: 0 };
  let queued = 0;
  for (const input of routerStore.listPendingPeerTaskInputs()) {
    const sender = agents[input.senderAgentName];
    const target = agents[input.recipientAgentName];
    if (!isAgentRecord(sender) || sender.agentId !== input.senderAgentId
      || !isAgentRecord(target) || target.agentId !== input.recipientAgentId
      || !agentEligibleForRoom(sender, input.roomId)
      || !admittedRunnerPeer({ agent: sender, descriptor: { roomId: input.roomId } }, target)
      || !threadSessionAgentEligibility(target).ok) continue;
    const attached = routerStore.attachTaskInputs({ taskId: input.taskId,
      requestScope: `peer-recovery:${input.sessionId}`, requestKey: input.messageId, messageIds: [input.messageId] });
    if (!attached.ok) throw new Error(`peer input reconciliation refused: ${attached.code}`);
    const result = await enqueueThreadSessionDispatch({ agent: target, sessionId: input.sessionId,
      taskId: input.taskId, threadRootEventId: input.threadRootEventId });
    if (result.ok && result.state === 'queued') queued += 1;
    else if (!result.ok) {
      console.warn(`[router] persisted peer input awaits dispatch: ${result.code}`);
      scheduleRouterPump(RUNNER_LAUNCH_RETRY_MS);
    }
  }
  return { queued };
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function executionAgentIdentity(agent) {
  return `${agent.agentId}:${agent.registeredAt || agent.discoveredAt || 'legacy'}`;
}

async function requestThreadSessionOwnerApproval(agent, projectRoomId, request, descriptor) {
  projectRoomId = routerStore.conversations.direct(projectRoomId, agent.name)?.projectRoomId || projectRoomId;
  const created = approvalStore.createRequest({
    agent: agent.name,
    runtime: 'codex',
    project_room_id: projectRoomId,
    upstream_request_id: request.approvalId,
    tool_name: `app_server_${request.kind}`,
    description: request.reason || `${request.kind} permission request`,
    input_preview: request.inputPreview || JSON.stringify(request.mcp ? {
      operationDigest: request.operationDigest,
      upstreamThreadId: request.upstreamThreadId,
      upstreamTurnId: request.upstreamTurnId,
      upstreamItemId: request.upstreamItemId,
      upstreamRequestId: request.upstreamRequestId,
      ...request.mcp,
    } : { command: request.command, cwd: request.cwd }).slice(0, 8192),
  }, { routerApprovalId: request.approvalId, execution: descriptor && request.nativeRequest && request.dispatchId === descriptor.dispatchId ? {
    agentId: executionAgentIdentity(agent), taskId: descriptor.taskId,
    workspace: descriptor.cwd, mayWrite: descriptor.mayWrite,
    ...request.nativeRequest,
  } : null });
  if (created.status === 'pending') {
    broadcastSSE('approval_requested', { request_id: created.id, agent: created.agent });
  }
  let record = created;
  while (record.status === 'pending') {
    await delay(250);
    record = approvalStore.getRequest(created.id);
    if (!record) throw new Error('owner approval request disappeared');
  }
  const consumed = approvalStore.consumeDecision(record.id, agent.name, record.input_digest || null);
  if (!consumed.ok) throw new Error(`owner approval consume failed: ${consumed.code}`);
  return { decisionEventId: consumed.decision_event_id, decision: consumed.decision };
}

function runnerEnvironment(agent) {
  const token = agentTokens.get(agent.name);
  if (!token) throw new Error(`ephemeral runner agent token is unavailable for '${agent.name}'`);
  const env = {
    AGENT_NAME: agent.name,
    HAGENCY_API: `http://127.0.0.1:${PORT}`,
    HAGENCY_EPHEMERAL_RUNNER: '1',
    HAGENCY_MCP_SERVER_NAME: THREAD_SESSION_MCP_SERVER_NAME,
    HAGENCY_RUNTIME_DIR: RUNTIME_ROOT,
    HAGENCY_AGENT_ID: agent.agentId,
    AGENT_TOKEN: token,
  };
  if (agent.homeDir) env.HAGENCY_AGENT_STATE_DIR = path.join(agent.homeDir, 'state');
  const profile = agent.runtimeProfile?.primary;
  if (profile?.apiBaseUrl && threadSessionFramework(agent) === 'claude') env.ANTHROPIC_BASE_URL = profile.apiBaseUrl;
  if (profile?.apiKey && threadSessionFramework(agent) === 'claude') env.ANTHROPIC_API_KEY = profile.apiKey;
  return env;
}

// `mayWrite` selects the permission mode: only a dispatch holding the
// workspace lease may edit files. A front-desk turn runs in `plan`, which can
// read and reason but not modify — without a lease its edits would be neither
// serialized against other runners nor recorded as workspace dirt.
//
// Both mode values are still pending the real-CLI smoke test that
// docs/THREAD-SESSIONS.md gates the canary on; if either is rejected the
// dispatch fails closed to outcome_unknown rather than running unconstrained.
function claudeThreadSessionArgs(agent, mayWrite, modelOverride = null) {
  if (normalizeBoolean(process.env.HAGENCY_CLAUDE_PERMISSION_CHANNEL) === false) {
    throw new ClaudeRuntimeConfigurationError('managed Claude ephemeral runners require the owner-approval MCP channel');
  }
  const args = [
    '-p', '--output-format', 'stream-json', '--verbose',
    '--permission-mode', mayWrite ? 'auto' : 'plan',
    '--dangerously-load-development-channels', `server:${THREAD_SESSION_MCP_SERVER_NAME}`,
  ];
  const model = claudeThreadModel(agent, modelOverride);
  // Store-validated, but re-checked here: this string lands on a CLI argv.
  if (model) {
    args.push('--model', model);
  }
  const ownerSettings = agent.workdir ? path.join(agent.workdir, '.claude', 'settings.json') : null;
  if (ownerSettings && existsSync(ownerSettings)) args.push('--settings', ownerSettings);
  if (!existsSync('/etc/claude-code/managed-mcp.json')) {
    const mcpConfigCandidates = [agent.workdir, agent.homeDir]
      .map((root) => root ? path.join(root, '.mcp.json') : null)
      .filter(Boolean);
    const mcpConfig = mcpConfigCandidates.find((candidate) => existsSync(candidate)) || null;
    if (!mcpConfig || !existsSync(mcpConfig)) {
      throw new ClaudeRuntimeConfigurationError(`Claude owner-approval MCP configuration is unavailable for '${agent.name}'`);
    }
    args.push(`--mcp-config=${mcpConfig}`);
  }
  return args;
}

function prepareThreadRuntime(agent) {
  if (agent.type !== 'claude' || !agent.homeDir || !agent.stateDir) return;
  prepareClaudeThreadRuntime(agent, { repoRoot: REPO_ROOT, runtimeRoot: RUNTIME_ROOT,
    apiBaseUrl: `http://127.0.0.1:${PORT}`, serverName: THREAD_SESSION_MCP_SERVER_NAME });
}

function normalizedCodexEffort(value) {
  const effort = String(value || '').trim().toLowerCase();
  return ['low', 'medium', 'high', 'xhigh'].includes(effort) ? effort : undefined;
}

async function launchClaimedThreadSessionRunner(claim, signal, onCleanup) {
  const descriptor = routerStore.getLaunchDescriptor(claim);
  if (!descriptor.ok && descriptor.code) throw new Error(`${descriptor.code}: ${descriptor.message}`);
  const agent = agents[descriptor.agentName];
  if (!isAgentRecord(agent) || agent.agentId !== descriptor.agentId) {
    throw new Error('runner launch descriptor no longer matches the registered agent');
  }
  if (agent.manualDown) {
    const cancelled = routerStore.cancelBeforeStart(claim.dispatchId, 'operator_stopped');
    if (!cancelled.ok) throw new Error(cancelled.message);
    return cancelled;
  }
  // Read from the dispatch row, not re-derived from the agent's role: the
  // sandbox must agree with whatever the lease decision actually recorded.
  const mayWrite = descriptor.mayWrite === true;
  prepareThreadRuntime(agent);
  const base = {
    router: routerStore,
    claim,
    cwd: descriptor.cwd,
    env: runnerEnvironment(agent),
    acknowledgementTimeoutMs: RUNNER_ACK_MS,
    executionTimeoutMs: RUNNER_LEASE_MS,
    signal,
    onCleanup: (confirmed) => onCleanup?.(descriptor.agentName, descriptor.agentId, confirmed),
    mayWrite,
  };
  if (descriptor.framework === 'codex') {
    return runCodexDispatch({
      ...base,
      executable: process.env.HAGENCY_CODEX_RUNNER_BIN || 'codex',
      model: descriptor.modelOverride || agent.runtimeProfile?.primary?.model || undefined,
      effort: normalizedCodexEffort(agent.runtimeProfile?.primary?.reasoning),
      yolo: normalizeExecutionPolicy(agent.executionPolicy, 'codex').yolo,
      approvalTimeoutMs: APPROVAL_ADAPTER_TIMEOUT_MS,
      maxParkedRunners: MAX_PARKED_RUNNERS,
      coordinationNeedsOwnerApproval: codexPermissionRequestNeedsOwnerApproval,
      mcpServer: {
        name: THREAD_SESSION_MCP_SERVER_NAME,
        command: process.execPath,
        args: [path.join(REPO_ROOT, 'mcp-server.js')],
        envVars: [
          'AGENT_NAME', 'HAGENCY_API', 'HAGENCY_MCP_SERVER_NAME',
          'HAGENCY_RUNTIME_DIR', 'HAGENCY_AGENT_STATE_DIR',
          'HAGENCY_EPHEMERAL_RUNNER', 'HAGENCY_AGENT_ID', 'AGENT_TOKEN',
          'HAGENCY_DISPATCH_CAPABILITY', 'HAGENCY_DISPATCH_ID',
          'HAGENCY_RUNNER_ID', 'HAGENCY_FENCE_GENERATION',
        ],
      },
      requestOwnerApproval: (request) => requestThreadSessionOwnerApproval(agent, descriptor.roomId, request, descriptor),
    });
  }
  return runClaudeDispatch({
    ...base,
    executable: process.env.HAGENCY_CLAUDE_RUNNER_BIN || 'claude',
    args: claudeThreadSessionArgs(agent, mayWrite, descriptor.modelOverride),
  });
}

function scheduleRouterPump(delayMs = 0) {
  if (!THREAD_SESSIONS_ENABLED || !routerPumpAccepting) return;
  const normalizedDelay = Number.isFinite(delayMs) ? Math.max(0, Math.floor(delayMs)) : 0;
  if (normalizedDelay === 0) {
    if (routerPumpTimer) {
      clearTimeout(routerPumpTimer);
      lifecycleTimeouts.delete(routerPumpTimer);
      routerPumpTimer = null;
      routerPumpDueAt = null;
    }
    if (routerPumpMicrotaskQueued) return;
    routerPumpMicrotaskQueued = true;
    queueMicrotask(() => {
      routerPumpMicrotaskQueued = false;
      if (routerPumpAccepting) void pumpRouterDispatches();
    });
    return;
  }
  if (routerPumpMicrotaskQueued) return;
  const dueAt = Date.now() + normalizedDelay;
  if (routerPumpTimer && routerPumpDueAt !== null && routerPumpDueAt <= dueAt) return;
  if (routerPumpTimer) {
    clearTimeout(routerPumpTimer);
    lifecycleTimeouts.delete(routerPumpTimer);
  }
  routerPumpDueAt = dueAt;
  routerPumpTimer = trackLifecycleTimeout(() => {
    routerPumpTimer = null;
    routerPumpDueAt = null;
    if (routerPumpAccepting) void pumpRouterDispatches();
  }, normalizedDelay);
}

async function pumpRouterDispatches() {
  if (!THREAD_SESSIONS_ENABLED || !routerPumpAccepting || routerPumpRunning) return;
  routerPumpRunning = true;
  try {
    try { await reconcileThreadSessionPeerMessages(); }
    catch (error) {
      console.error(`[router] peer input reconciliation failed: ${error?.message || error}`);
      scheduleRouterPump(RUNNER_LAUNCH_RETRY_MS);
    }
    while (liveThreadSessionRunners.size < MAX_LIVE_RUNNERS) {
      const runnerId = `runner_${randomBytes(16).toString('hex')}`;
      const { claim, nextAvailableAt } = routerStore.claimDispatchWithWake({
        runnerId,
        leaseMs: RUNNER_LEASE_MS + APPROVAL_TTL_MS,
        capabilityTtlMs: RUNNER_LEASE_MS + APPROVAL_TTL_MS,
        maxLiveRunners: MAX_LIVE_RUNNERS,
      });
      if (!claim || !claim.ok) {
        if (nextAvailableAt !== null) scheduleRouterPump(Math.max(1, nextAvailableAt - Date.now()));
        break;
      }
      const controller = new AbortController();
      let nextPumpDelayMs = 0;
      const launchDescriptor = routerStore.getLaunchDescriptor(claim);
      const runner = { running: null, controller, cleanupConfirmed: false,
        agentName: launchDescriptor.agentName, agentId: launchDescriptor.agentId };
      const onCleanup = (agentName, agentId, confirmed) => {
        runner.cleanupConfirmed = confirmed;
        const agent = agents[agentName];
        if (!isAgentRecord(agent) || agent.agentId !== agentId) return;
        const pending = agent.stopUnconfirmedDispatches || [];
        if (confirmed && !pending.includes(claim.dispatchId)) return;
        if (confirmed) {
          agent.stopUnconfirmedDispatches = pending.filter((id) => id !== claim.dispatchId);
        } else {
          syncAgentMachine(agent.name, { manualDown: true });
          agent.offlineReason = 'runner-cleanup-unconfirmed';
          agent.stopUnconfirmedDispatches = [...new Set([...pending, claim.dispatchId])];
        }
        if (!saveAgents(true)) {
          if (confirmed) agent.stopUnconfirmedDispatches = pending;
          console.error(`[router-runner] failed to persist cleanup evidence dispatch=${claim.dispatchId}`);
        }
      };
      const running = launchClaimedThreadSessionRunner(claim, controller.signal, onCleanup)
        .catch((error) => {
          const state = routerStore.snapshot().dispatches.find((row) => row.dispatchId === claim.dispatchId)?.state;
          if (state === 'leased') {
            const reason = redactPathLikeText(error?.message || error, 1000) || 'runner launch failed';
            if (error instanceof ClaudeRuntimeConfigurationError) {
              // Retrying the same invalid model/config cannot repair it. Preserve
              // unprocessed input and emit the existing actionable launch notice.
              const cancelled = routerStore.cancelBeforeStart(claim.dispatchId, 'runner_launch_failed');
              if (!cancelled.ok) console.error(`[router-runner] failed to cancel invalid configuration dispatch=${claim.dispatchId}: ${cancelled.code}: ${cancelled.message}`);
            } else {
              const requeued = routerStore.requeueBeforeStart(claim, RUNNER_LAUNCH_RETRY_MS, reason);
              if (requeued.ok) nextPumpDelayMs = RUNNER_LAUNCH_RETRY_MS;
              else console.error(`[router-runner] failed to requeue dispatch=${claim.dispatchId}: ${requeued.code}: ${requeued.message}`);
            }
          }
          console.error(`[router-runner] dispatch=${claim.dispatchId} failed: ${error?.message || error}`);
        })
        .finally(() => {
          liveThreadSessionRunners.delete(claim.dispatchId);
          scheduleRouterPump(nextPumpDelayMs);
        });
      runner.running = running;
      liveThreadSessionRunners.set(claim.dispatchId, runner);
    }
  } finally {
    routerPumpRunning = false;
  }
}
const localActivitySweepState = loadJsonSync('local_activity_sweep.json', { selectionCursor: 0 });
let msgCounter = loadJsonSync('.msg_counter', 0);
const localActivityState = new Map(); // agent -> { lastHash, lastChangeSec, burstStartSec, burstLastSec }
const localTmuxMissingState = new Map(); // agent -> { since:number, alerted:boolean, misses:number, wasOnline:boolean }
const localCompactState = new Map(); // agent -> marker
const localRuntimeSignalDigest = new Map(); // agent -> digest of blocked/mcp/workspace
let localActivitySweepRunning = false;
let localSwapSweepRunning = false;
let agentScopeSweepRunning = false;
let supervisorLifecycleSweepRunning = false;
/*
 * Declared even though the project-side sweep runs once at startup, because `runAsyncSweep`'s
 * reentrancy guard is a hardcoded ladder of `stateKey` comparisons: an unrecognised key gets NO
 * guard, silently. A one-shot call does not need one, but the next person to put this sweep on a
 * timer would inherit overlapping runs against foreign homeservers and nothing would say why.
 */
let projectSideSweepRunning = false;
let localTmuxSnapshotWarnAt = 0;
const SYSTEM_INFO_LOG = dataPath('system-info.jsonl');
const AUDIT_LOG = dataPath('audit.jsonl');
const MESSAGE_ARCHIVE_LOG = dataPath('messages-archive.jsonl');
const DELIVERY_EVENT_LOG = dataPath('message-delivery-events.jsonl');
repairJsonlTornTail(DELIVERY_EVENT_LOG);
repairJsonlTornTail(MESSAGE_ARCHIVE_LOG);
const deliveryEventAttemptIds = new Set();
if (existsSync(DELIVERY_EVENT_LOG)) {
  for (const line of readFileSync(DELIVERY_EVENT_LOG, 'utf8').split('\n')) {
    if (!line.trim()) continue;
    try {
      const attemptId = JSON.parse(line)?.attemptId;
      if (attemptId) deliveryEventAttemptIds.add(attemptId);
    } catch {}
  }
}
const matrixDispatchStore = new MatrixDispatchStore({
  journalPath: dataPath('matrix/source-events.jsonl'),
});
let matrixDispatchFailureStageForTest = null;
const pendingHumanTargetCache = new Map(); // agent -> { hasPendingHuman, targets }
const swapAlertState = {
  active: false,
  lastPct: 0,
  lastAlertAt: 0,
};
// scopePressureState tracks the high/low state for resource alerts (state-based, not just cooldown)
const scopePressureState = new Map(); // agent -> { high:bool }
let localMcpSessionCacheAt = 0;

function messageCounterFromId(id) {
  if (typeof id !== 'string') return 0;
  const match = /^msg_(\d+)$/.exec(id.trim());
  if (!match) return 0;
  const value = Number.parseInt(match[1], 10);
  return Number.isFinite(value) && value > 0 ? value : 0;
}

function maxPersistedMessageCounter(rows = messages) {
  if (!Array.isArray(rows)) return 0;
  let max = 0;
  for (const row of rows) {
    max = Math.max(max, messageCounterFromId(row?.id));
  }
  return max;
}

{
  const loadedCounter = Number.isFinite(Number(msgCounter)) ? Math.max(0, Math.floor(Number(msgCounter))) : 0;
  const reconciledCounter = Math.max(loadedCounter, maxPersistedMessageCounter(messages));
  msgCounter = reconciledCounter;
  if (reconciledCounter !== loadedCounter) {
    saveJson('.msg_counter', msgCounter, { immediate: true });
  }
}
let localMcpSessionCache = new Set();
const agentMachines = new Map(); // agentName -> AgentStateMachine

function createAgentMachine(agentName, initialState, snapshot = null) {
  const m = new AgentStateMachine(initialState);
  m.onGraceExpired(() => {
    const a = agents[agentName];
    if (a) { a.state = m.state; saveAgents(); }
  });
  if (snapshot) m.restore(snapshot);
  agentMachines.set(agentName, m);
  return m;
}

function resetAgentMachine(agentName, snapshot = null) {
  const existing = agentMachines.get(agentName);
  if (existing) existing.destroy();
  agentMachines.delete(agentName);
  if (snapshot) createAgentMachine(agentName, snapshot.state, snapshot);
}

function snapshotAgentPersistenceState(agentName) {
  const hadAgent = Object.prototype.hasOwnProperty.call(agents, agentName);
  const machine = agentMachines.get(agentName);
  return {
    hadAgent,
    agent: hadAgent ? cloneJsonValue(agents[agentName]) : null,
    hadMachine: Boolean(machine),
    machineSnapshot: machine?.snapshot() || null,
  };
}

function snapshotAgentRuntimeState(agentName) {
  const hadRuntime = Object.prototype.hasOwnProperty.call(agentRuntime, agentName);
  return {
    hadRuntime,
    runtime: hadRuntime ? cloneJsonValue(agentRuntime[agentName]) : null,
  };
}

function restoreAgentPersistenceState(agentName, snapshot) {
  if (snapshot?.hadAgent) agents[agentName] = cloneJsonValue(snapshot.agent);
  else delete agents[agentName];
  resetAgentMachine(agentName, snapshot?.hadMachine ? snapshot.machineSnapshot : null);
}

function restoreAgentRuntimeState(agentName, snapshot) {
  if (snapshot?.hadRuntime) agentRuntime[agentName] = cloneJsonValue(snapshot.runtime);
  else delete agentRuntime[agentName];
}

function saveAgentsOrRollback(agentName, snapshot) {
  if (saveAgents(true)) return true;
  restoreAgentPersistenceState(agentName, snapshot);
  return false;
}

function getAgentMachine(agentName) {
  let m = agentMachines.get(agentName);
  if (m) return m;
  const agent = agents[agentName];
  const runtime = agentRuntime[agentName] || null;
  const initial = deriveStateFromLegacy(agent || null, runtime);
  return createAgentMachine(agentName, initial);
}

function transitionAgent(agentName, event) {
  const m = getAgentMachine(agentName);
  const newState = m.transition(event);
  const agent = agents[agentName];
  if (agent) {
    agent.state = newState;
    agent.online = m.online;
    agent.manualDown = m.manualDown;
  }
  return newState;
}

function syncAgentMachine(agentName, signals) {
  if (!agentName) return;
  const m = getAgentMachine(agentName);

  if (signals.manualDown === true) {
    transitionAgent(agentName, 'manual_down');
    return;
  }
  if (signals.manualDown === false && m.state === 'manual_down') {
    transitionAgent(agentName, 'manual_up');
  }

  if (signals.serverOffline) {
    transitionAgent(agentName, 'server_offline');
    return;
  }
  if (signals.tmuxMissing) {
    transitionAgent(agentName, 'tmux_missing');
    return;
  }
  if (signals.heartbeatMissing) {
    transitionAgent(agentName, 'heartbeat_missing');
    return;
  }

  if (signals.heartbeatPresent) {
    transitionAgent(agentName, 'heartbeat_present');
  } else if (signals.tmuxPresent && m.state === 'offline') {
    transitionAgent(agentName, 'tmux_detected');
  }

  const agent = agents[agentName];
  if (signals.mcpPresent === true) {
    transitionAgent(agentName, 'mcp_confirmed');
  } else if (signals.mcpPresent === false) {
    if (m.state !== 'starting') transitionAgent(agentName, 'mcp_missing_debounced');
  } else if (agent && !agentExpectsMcp(agent)) {
    transitionAgent(agentName, 'mcp_not_applicable');
  }
}

const agentsBeforeNormalization = JSON.stringify(agents);


for (const agent of Object.values(agents)) {
  agent.name = agent.name || null;
  if (!Object.prototype.hasOwnProperty.call(agent, 'server')) {
    agent.server = null;
  } else {
    agent.server = normalizeServer(agent.server);
  }
  if (!Object.prototype.hasOwnProperty.call(agent, 'online')) {
    agent.online = Boolean(agent.tmux);
  } else {
    agent.online = Boolean(agent.online);
  }
  if (!Object.prototype.hasOwnProperty.call(agent, 'lastSeen')) {
    agent.lastSeen = agent.discoveredAt || agent.registeredAt || Date.now();
  }
  if (!Object.prototype.hasOwnProperty.call(agent, 'offlineReason')) {
    agent.offlineReason = null;
  } else if (typeof agent.offlineReason !== 'string' || !agent.offlineReason.trim()) {
    agent.offlineReason = null;
  } else {
    agent.offlineReason = agent.offlineReason.trim();
  }
  if (!Object.prototype.hasOwnProperty.call(agent, 'manualDown')) {
    agent.manualDown = false;
  } else {
    agent.manualDown = agent.manualDown === true;
  }
  if (!Object.prototype.hasOwnProperty.call(agent, 'discoveredAt')) {
    agent.discoveredAt = agent.registeredAt || agent.lastSeen || Date.now();
  }
  agent.agentModelVersion = normalizeAgentModelVersion(agent.agentModelVersion) || null;
  agent.layoutVersion = normalizeLayoutVersion(agent.layoutVersion) || null;
  agent.agentId = normalizeAgentId(agent.agentId) || null;
  agent.homeDir = normalizeWorkspacePath(agent.homeDir) || null;
  agent.workdir = normalizeWorkspacePath(agent.workdir) || null;
  agent.workspaceMode = normalizeWorkspaceMode(agent.workspaceMode || agent.workspace_mode);
  agent.worktreesDir = normalizeWorkspacePath(agent.worktreesDir || agent.worktrees_dir) || null;
  agent.worktreeBootstrap = normalizeWorktreeBootstrap(agent.worktreeBootstrap || agent.worktree_bootstrap);
  agent.stateDir = normalizeWorkspacePath(agent.stateDir) || null;
  agent.managedProjects = normalizeManagedProjects(agent.managedProjects);
  agent.human = normalizeHumanMeta(agent.human, { preserveLegacy: true });
  agent.task = normalizeAgentTask(agent.task, agent.name);
  agent.runtimeProfile = normalizeRuntimeProfile(agent.runtimeProfile);
  agent.kind = inferRecordKind(agent);
  if (agent.kind === 'human') {
    agent.online = false;
    agent.offlineReason = null;
    agent.manualDown = false;
  }
}
if (JSON.stringify(agents) !== agentsBeforeNormalization) {
  saveJson('agents.json', agents);
}
for (const agent of Object.values(agents)) {
  if (!agent.name) continue;
  const m = getAgentMachine(agent.name);
  agent.state = m.state;
}

// ── Startup reconciliation: agent home manifests ──────────────────────
{
  let reconciled = 0;
  for (const agent of Object.values(agents)) {
    if (!agent.name || !agent.homeDir) continue;
    if (!isLocalAgentServer(normalizeServer(agent.server), LOCAL_SERVER_ID)) continue;
    const manifestPath = path.join(agent.homeDir, 'agent.json');
    try {
      const manifest = readV1AgentManifest(manifestPath);
      if (!manifest) continue;
      let changed = false;
      if (manifest.task && typeof manifest.task === 'object') {
        const diskTs = Date.parse(manifest.task.updated_at) || 0;
        const memTs = Date.parse(agent.task?.updated_at) || 0;
        if (diskTs > memTs) {
          agent.task = normalizeAgentTask(manifest.task, agent.name);
          changed = true;
        }
      }
      if (manifest.runtimeProfile && !agent.runtimeProfile) {
        agent.runtimeProfile = normalizeRuntimeProfile(manifest.runtimeProfile);
        if (agent.runtimeProfile) changed = true;
      }
      if (changed) reconciled++;
    } catch { /* skip unreadable manifests */ }
  }
  if (reconciled > 0) {
    saveJson('agents.json', agents);
    console.log(`[startup] reconciled ${reconciled} agent(s) from home manifests`);
  }
}

// ── Startup reconciliation: orphaned agent homes ──────────────────────
{
  let orphans = 0;
  try {
    for (const homeRoot of allAgentHomeRoots()) {
      const agentsDir = path.join(homeRoot, 'agents');
      if (!existsSync(agentsDir)) continue;
      const entries = readdirSync(agentsDir, { withFileTypes: true });
      for (const entry of entries) {
        if (!entry.isDirectory()) continue;
        const manifestPath = path.join(agentsDir, entry.name, 'agent.json');
        const manifest = readV1AgentManifest(manifestPath);
        if (!manifest || !manifest.name) continue;
        if (agents[manifest.name]) continue;
        const result = ensureAgentRecord(manifest.name, {
          type: manifest.type || 'agent',
          homeDir: manifest.homeDir,
          workdir: manifest.workdir,
          stateDir: manifest.stateDir,
          agentModelVersion: manifest.agentModelVersion,
          layoutVersion: manifest.layoutVersion,
          agentId: manifest.id,
          online: false,
          offlineReason: 'orphan-discovered',
          task: manifest.task,
          runtimeProfile: manifest.runtimeProfile,
          identity: manifest.identity || null,
          role: manifest.role || null,
        });
        if (!result) continue; // tombstoned — skip
        const { agent: created } = result;
        if (created) {
          const m = getAgentMachine(manifest.name);
          created.state = m.state;
          orphans++;
        }
      }
    }
    if (orphans > 0) {
      saveJson('agents.json', agents);
      console.log(`[startup] discovered ${orphans} orphaned agent home(s)`);
    }
  } catch (e) {
    console.error(`[startup] orphan scan failed: ${e.message}`);
  }
}

const groupsBeforeNormalization = JSON.stringify(groups);
for (const [groupKey, group] of Object.entries(groups)) {
  if (!group || typeof group !== 'object') {
    delete groups[groupKey];
    continue;
  }
  const canonicalName = (typeof group.name === 'string' ? group.name.trim() : '') || groupKey;
  group.name = canonicalName;
  if (canonicalName !== groupKey) {
    delete groups[groupKey];
    if (!groups[canonicalName]) groups[canonicalName] = group;
  }

  const members = Array.isArray(group.members) ? group.members : [];
  const normalizedMembers = [];
  const seen = new Set();
  for (const raw of members) {
    const memberName = normalizeAgentName(raw);
    if (!memberName) continue;
    const key = memberName.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    normalizedMembers.push(memberName);
  }
  group.members = normalizedMembers;
  if (!Number.isFinite(group.createdAt)) group.createdAt = Date.now();
}
if (JSON.stringify(groups) !== groupsBeforeNormalization) {
  saveJson('groups.json', groups);
}

const cursorsBeforeNormalization = JSON.stringify(cursors);
for (const [agentName, cursor] of Object.entries(cursors)) {
  if (!cursor || typeof cursor !== 'object') {
    cursors[agentName] = { inbox: 0, inboxId: null, groups: {}, groupIds: {} };
    continue;
  }
  cursor.inbox = Number(cursor.inbox) || 0;
  cursor.inboxId = typeof cursor.inboxId === 'string' ? cursor.inboxId : null;

  if (!cursor.groups || typeof cursor.groups !== 'object') cursor.groups = {};
  for (const [groupName, ts] of Object.entries(cursor.groups)) {
    cursor.groups[groupName] = Number(ts) || 0;
  }

  if (!cursor.groupIds || typeof cursor.groupIds !== 'object') cursor.groupIds = {};
  for (const [groupName, id] of Object.entries(cursor.groupIds)) {
    cursor.groupIds[groupName] = typeof id === 'string' ? id : null;
  }
}
if (JSON.stringify(cursors) !== cursorsBeforeNormalization) {
  saveJson('cursors.json', cursors);
}

let serverMaintenanceChanged = false;
for (const [serverId, server] of Object.entries(servers)) {
  if (!server || typeof server !== 'object') {
    servers[serverId] = {
      id: serverId,
      lastSeen: 0,
      heartbeatAt: 0,
      relayInstanceId: null,
      relayBootTs: 0,
      online: false,
      updatedAt: Date.now(),
      sessions: [],
      agents: [],
      agentCount: 0,
      maintenance: SERVER_MAINTENANCE_IDS.has(serverId),
    };
    serverMaintenanceChanged = true;
    continue;
  }
  server.id = server.id || serverId;
  server.lastSeen = Number(server.lastSeen) || 0;
  server.heartbeatAt = Number(server.heartbeatAt) || server.lastSeen || 0;
  server.relayInstanceId = (typeof server.relayInstanceId === 'string' && server.relayInstanceId.trim())
    ? server.relayInstanceId.trim()
    : null;
  server.relayBootTs = Number(server.relayBootTs) || 0;
  server.online = Boolean(server.online);
  server.updatedAt = Number(server.updatedAt) || server.lastSeen || 0;
  const hadMaintenance = Object.prototype.hasOwnProperty.call(server, 'maintenance');
  const previousMaintenance = server.maintenance;
  const configuredMaintenance = SERVER_MAINTENANCE_IDS.has(normalizeServer(server.id) || serverId);
  server.maintenance = SERVER_MAINTENANCE_ENV_CONFIGURED || !hadMaintenance
    ? configuredMaintenance
    : server.maintenance === true;
  if (!hadMaintenance || previousMaintenance !== server.maintenance) serverMaintenanceChanged = true;
  if (!Array.isArray(server.sessions)) server.sessions = [];
  if (!Array.isArray(server.agents)) server.agents = [];
  server.agentCount = Number(server.agentCount) || server.agents.length || 0;
}
if (serverMaintenanceChanged) saveJson('servers.json', servers);

for (const [agentName, runtime] of Object.entries(agentRuntime)) {
  if (!runtime || typeof runtime !== 'object') {
    delete agentRuntime[agentName];
    continue;
  }
  runtime.agent = agentName;
  runtime.blocked = runtime.blocked === true;
  runtime.blockedReason = (typeof runtime.blockedReason === 'string' && runtime.blockedReason.trim())
    ? runtime.blockedReason.trim()
    : null;
  runtime.blockedSince = Number(runtime.blockedSince) || null;
  runtime.blockedConsecutiveScans = Math.max(0, Number(runtime.blockedConsecutiveScans) || 0);
  runtime.blockedNotificationSent = runtime.blockedNotificationSent === true;
  runtime.lastBlockedNotificationTs = Math.max(0, Number(runtime.lastBlockedNotificationTs) || 0);
  runtime.updatedAt = Number(runtime.updatedAt) || 0;
  runtime.lastSeen = Number(runtime.lastSeen) || 0;
  runtime.lastPushNotifyAt = Number(runtime.lastPushNotifyAt) || 0;
  runtime.lastPushQueuedAt = Number(runtime.lastPushQueuedAt) || 0;
  runtime.lastPushDeliveredAt = Number(runtime.lastPushDeliveredAt) || 0;
  runtime.lastPushDeliveryDelayMs = Number(runtime.lastPushDeliveryDelayMs) || 0;
  runtime.lastActionablePushAt = Number(runtime.lastActionablePushAt) || 0;
  runtime.lastPushQueueEntryId = Number(runtime.lastPushQueueEntryId) || 0;
  runtime.lastPushNeedsInboxCheck = runtime.lastPushNeedsInboxCheck === true;
  runtime.lastPushUnreadCount = Number(runtime.lastPushUnreadCount) || 0;
  runtime.lastPushKind = (typeof runtime.lastPushKind === 'string' && runtime.lastPushKind.trim())
    ? runtime.lastPushKind.trim()
    : 'unknown';
  runtime.lastPushSourceMsgId = (typeof runtime.lastPushSourceMsgId === 'string' && runtime.lastPushSourceMsgId.trim())
    ? runtime.lastPushSourceMsgId.trim()
    : null;
  runtime.lastInboxCheckAt = Number(runtime.lastInboxCheckAt) || 0;
  runtime.lastAgentOutboundAt = Number(runtime.lastAgentOutboundAt) || 0;
  runtime.inboxGate = normalizeInboxGate(runtime.inboxGate);
  runtime.inboxReadAck = normalizeInboxReadAck(runtime.inboxReadAck);
  runtime.lastBlockedTail = (typeof runtime.lastBlockedTail === 'string') ? runtime.lastBlockedTail : '';
  runtime.lastBlockedCommand = (typeof runtime.lastBlockedCommand === 'string') ? runtime.lastBlockedCommand : '';
  runtime.lastBlockedServer = normalizeServer(runtime.lastBlockedServer);
  runtime.activeNow = normalizeRuntimeActiveNow(runtime.activeNow);
  runtime.activeDurationSec = Number(runtime.activeDurationSec) || 0;
  runtime.idleDurationSec = Number(runtime.idleDurationSec) || 0;
  runtime.lastTmuxActivitySec = Number(runtime.lastTmuxActivitySec) || null;
  runtime.workspacePath = normalizeWorkspacePath(runtime.workspacePath);
  runtime.observation = normalizeRuntimeObservation(runtime.observation);
  runtime.mcpPresent = runtime.mcpPresent === true
    ? true
    : (runtime.mcpPresent === false ? false : null);
  runtime.mcpMissingSince = Number(runtime.mcpMissingSince) || null;
  runtime.mcpHeartbeatAt = Number(runtime.mcpHeartbeatAt) || null;
  if (!runtime.rules || typeof runtime.rules !== 'object') runtime.rules = {};
}
localActivitySweepState.selectionCursor = Math.max(0, Number(localActivitySweepState.selectionCursor) || 0);

function reserveNextMsgId() {
  const nextCounter = msgCounter + 1;
  if (!saveJson('.msg_counter', nextCounter, { immediate: true })) {
    return { ok: false, error: 'msg_counter persistence failed' };
  }
  msgCounter = nextCounter;
  return { ok: true, id: `msg_${String(msgCounter).padStart(4, '0')}` };
}

function saveAgents(immediate = false) { return saveJson('agents.json', agents, { immediate }); }

function writeThruAgentHome(agentName) {
  const agent = agents[agentName];
  if (!agent || !agent.homeDir) return;
  const serverId = normalizeServer(agent.server);
  if (!isLocalAgentServer(serverId, LOCAL_SERVER_ID)) return;
  const agentJsonPath = path.join(agent.homeDir, 'agent.json');
  try {
    if (!existsSync(agent.homeDir)) return;
    let disk = {};
    try { disk = JSON.parse(readFileSync(agentJsonPath, 'utf-8')); } catch { /* new file */ }
    let changed = false;
    for (const field of ['task', 'runtimeProfile', 'identity', 'role']) {
      const next = agent[field] ?? null;
      const prev = disk[field] ?? null;
      if (JSON.stringify(next) !== JSON.stringify(prev)) {
        disk[field] = next;
        changed = true;
      }
    }
    if (changed) {
      writeFileSync(agentJsonPath, `${JSON.stringify(disk, null, 2)}\n`, 'utf-8');
    }
  } catch (e) {
    console.error(`writeThruAgentHome(${agentName}): ${e.message}`);
  }
}

function saveGroups() { return saveJson('groups.json', groups); }

function invalidateUnreadMessageIndex() {
  unreadMessageIndexVersion += 1;
  unreadMessageIndex.version = -1;
}

function addIndexedMessage(map, key, msg) {
  if (typeof key !== 'string' || !key) return;
  let rows = map.get(key);
  if (!rows) {
    rows = [];
    map.set(key, rows);
  }
  rows.push(msg);
}

function getUnreadMessageIndex() {
  if (unreadMessageIndex.version === unreadMessageIndexVersion) return unreadMessageIndex;
  const directByAgent = new Map();
  const groupByName = new Map();
  const groupMentionsByAgent = new Map();
  for (const msg of messages) {
    if (!msg || typeof msg !== 'object') continue;
    addIndexedMessage(directByAgent, msg.to, msg);
    addIndexedMessage(groupByName, msg.group, msg);
    if (!msg.group || !Array.isArray(msg.mentions)) continue;
    const seenMentions = new Set();
    for (const mention of msg.mentions) {
      if (typeof mention !== 'string' || !mention || seenMentions.has(mention)) continue;
      seenMentions.add(mention);
      addIndexedMessage(groupMentionsByAgent, mention, msg);
    }
  }
  for (const rows of [...directByAgent.values(), ...groupByName.values(), ...groupMentionsByAgent.values()]) {
    rows.sort(compareMsgOrder);
  }
  unreadMessageIndex.directByAgent = directByAgent;
  unreadMessageIndex.groupByName = groupByName;
  unreadMessageIndex.groupMentionsByAgent = groupMentionsByAgent;
  unreadMessageIndex.version = unreadMessageIndexVersion;
  return unreadMessageIndex;
}

function collectUnreadRetainedMessageIds() {
  const keep = new Set();
  for (const [agentName, agent] of Object.entries(agents)) {
    if (!isAgentRecord(agent) || agent.kind === 'human') continue;
    for (const msg of getUnreadInboxMessages(agentName).unread) {
      if (typeof msg?.id === 'string' && msg.id) keep.add(msg.id);
    }
  }
  return keep;
}

function collectRouterUncopiedMessageIds() {
  const keep = new Set();
  if (!THREAD_SESSIONS_ENABLED || !routerStore) return keep;
  const rawCursor = routerStore.readIngestionCursor(THREAD_SESSION_MESSAGE_CURSOR);
  if (!rawCursor) {
    for (const msg of messages) {
      if (isThreadSessionMatrixSourceMessage(msg)) keep.add(msg.id);
    }
    return keep;
  }
  const cursor = parseThreadSessionMessageCursor(rawCursor);
  for (const msg of messages) {
    if (isThreadSessionMatrixSourceMessage(msg) && isMessageAfterCursor(msg, cursor.ts, cursor.id)) {
      keep.add(msg.id);
    }
  }
  return keep;
}

// True when `msg` sorts strictly after the thread-session ingestion cursor —
// i.e. it has not been copied into the router yet and must survive retention
// pruning until reconciliation catches up. Same (ts, id) ordering as
// compareMsgOrder/firstMessageAfterCursorIndex.
function isMessageAfterCursor(msg, cursorTs, cursorId) {
  const normalizedTs = Number(cursorTs) || 0;
  const normalizedId = typeof cursorId === 'string' ? cursorId : null;
  return normalizedId
    ? compareMsgOrder(msg, { ts: normalizedTs, id: normalizedId }) > 0
    : (Number(msg?.ts) || 0) > normalizedTs;
}

function planMessagePrune(rows) {
  const list = Array.isArray(rows) ? rows : [];
  if (list.length <= MESSAGE_RETENTION_LIMIT) {
    return { retained: list, pruned: [] };
  }
  const retainFrom = Math.max(0, list.length - MESSAGE_RETENTION_LIMIT);
  const unreadKeepIds = collectUnreadRetainedMessageIds();
  const routerKeepIds = collectRouterUncopiedMessageIds();
  const retained = [];
  const pruned = [];

  for (let i = 0; i < list.length; i += 1) {
    const msg = list[i];
    const keep = i >= retainFrom || (typeof msg?.id === 'string'
      && (unreadKeepIds.has(msg.id) || routerKeepIds.has(msg.id)));
    if (keep) retained.push(msg);
    else pruned.push(msg);
  }
  return { retained, pruned };
}

function archivePrunedMessages(pruned) {
  if (!Array.isArray(pruned) || pruned.length === 0) return 0;
  let fd = null;
  try {
    const created = !existsSync(MESSAGE_ARCHIVE_LOG);
    fd = openSync(MESSAGE_ARCHIVE_LOG, 'a', 0o600);
    chmodSync(MESSAGE_ARCHIVE_LOG, 0o600);
    appendFileSync(fd, `${pruned.map((msg) => JSON.stringify(msg)).join('\n')}\n`);
    fsyncSync(fd);
    if (created) fsyncParentDirectory(MESSAGE_ARCHIVE_LOG);
    return pruned.length;
  } catch (e) {
    console.error(`Failed to archive pruned messages to ${MESSAGE_ARCHIVE_LOG}: ${e?.message || e}`);
    return 0;
  } finally {
    if (fd !== null) closeSync(fd);
  }
}

function fsyncParentDirectory(filePath) {
  const fd = openSync(path.dirname(filePath), 'r');
  try {
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
}

function repairJsonlTornTail(filePath) {
  if (!existsSync(filePath)) return false;
  const bytes = readFileSync(filePath);
  if (bytes.length === 0 || bytes[bytes.length - 1] === 0x0a) return false;
  const completeLength = bytes.lastIndexOf(0x0a) + 1;
  truncateSync(filePath, completeLength);
  const fd = openSync(filePath, 'r');
  try {
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  fsyncParentDirectory(filePath);
  return true;
}

function pruneMessagesInMemory() {
  if (!Array.isArray(messages) || messages.length <= MESSAGE_RETENTION_LIMIT) {
    return { pruned: 0, archived: 0 };
  }
  const { retained, pruned } = planMessagePrune(messages);

  if (pruned.length === 0) return { pruned: 0, archived: 0 };
  const archived = archivePrunedMessages(pruned);
  if (archived !== pruned.length) return { pruned: 0, archived: 0 };
  messages.splice(0, messages.length, ...retained);
  return { pruned: pruned.length, archived };
}

function saveMessages() {
  invalidateUnreadMessageIndex();
  const { retained, pruned } = planMessagePrune(messages);
  const nextMessages = pruned.length > 0 ? retained : messages;
  if (pruned.length > 0 && archivePrunedMessages(pruned) !== pruned.length) {
    invalidateUnreadMessageIndex();
    return false;
  }
  if (!saveJson('messages.json', nextMessages)) {
    invalidateUnreadMessageIndex();
    return false;
  }
  if (pruned.length > 0) {
    messages.splice(0, messages.length, ...retained);
  }
  invalidateUnreadMessageIndex();
  return true;
}
function saveCursors() { return saveJson('cursors.json', cursors); }
function saveServers() { saveJson('servers.json', servers); }
function saveAgentRuntime(immediate = false) { return saveJson('agent_runtime.json', agentRuntime, { immediate }); }
function saveTaskGraphs(next = taskGraphStore.dump()) { return saveJson('task_graphs.json', next); }
function saveLocalActivitySweepState() { saveJson('local_activity_sweep.json', localActivitySweepState); }

function ensureAgentRecord(name, defaults = {}) {
  const agentName = normalizeAgentName(name);
  if (!agentName) return null;
  if (deletedAgentTombstones[agentName]) return null;
  if (agents[agentName]) return { agent: agents[agentName], created: false };

  const now = Date.now();
  const server = defaults.server !== undefined ? normalizeServer(defaults.server) : null;
  const tmux = (typeof defaults.tmux === 'string' && defaults.tmux.trim()) ? defaults.tmux.trim() : null;
  const online = defaults.online === true;
  const manualDown = defaults.manualDown === true && !online;
  const reason = (typeof defaults.offlineReason === 'string' && defaults.offlineReason.trim())
    ? defaults.offlineReason.trim()
    : (online ? null : 'inactive');
  const type = (typeof defaults.type === 'string' && defaults.type.trim()) ? defaults.type.trim() : 'agent';
  const kind = inferRecordKind({ ...defaults, type, name: agentName });

  const agent = {
    name: agentName,
    role: defaults.role ?? null,
    identity: defaults.identity ?? null,
    tmux,
    type,
    server,
    online,
    lastSeen: now,
    offlineReason: reason,
    manualDown,
    discoveredAt: now,
    registeredAt: Number(defaults.registeredAt) > 0 ? Number(defaults.registeredAt) : now,
    agentModelVersion: normalizeAgentModelVersion(defaults.agentModelVersion) || null,
    layoutVersion: normalizeLayoutVersion(defaults.layoutVersion) || null,
    agentId: normalizeAgentId(defaults.agentId) || null,
    homeDir: normalizeWorkspacePath(defaults.homeDir) || null,
    workdir: normalizeWorkspacePath(defaults.workdir) || null,
    stateDir: normalizeWorkspacePath(defaults.stateDir) || null,
    managedProjects: normalizeManagedProjects(defaults.managedProjects),
    human: normalizeHumanMeta(defaults.human, { preserveLegacy: true }),
    task: normalizeAgentTask(defaults.task, agentName),
    runtimeProfile: normalizeRuntimeProfile(defaults.runtimeProfile),
    kind,
  };
  agents[agentName] = agent;
  return { agent, created: true };
}

function isManualDownReason(reason) {
  const text = (typeof reason === 'string' ? reason.trim().toLowerCase() : '');
  if (!text) return false;
  return text === 'manual-offline'
    || text === 'session-missing'
    || text.startsWith('hagency-down')
    || text.startsWith('server-maintenance:');
}

function maybeEmitUnexpectedOfflineAlert(agentName, reason, context = {}) {
  if (!agentName) return;
  if (isManualDownReason(reason)) return;
  const lines = [
    `Agent: ${agentName}`,
    `Reason: ${reason || 'unknown'}`,
    `Server: ${context.server || 'local'}`,
    'This looks like an unexpected shutdown/crash. Please intervene manually.',
  ];
  if (context.detail) lines.push(`Detail: ${context.detail}`);
  const result = notificationRouter.emit('agent_offline', {
    agentName, reason: reason || 'unknown',
    summary: `Agent '${agentName}' went offline unexpectedly`,
    full: lines.join('\n'),
  });
  if (!result.accepted) return;
}

function ensureInfoGroup() {
  if (groups.info) return true;
  groups.info = { name: 'info', members: [], createdAt: Date.now() };
  if (saveGroups()) return true;
  delete groups.info;
  return false;
}

function auditLog(req, { agent = null, summary = null, status = 200 } = {}) {
  try {
    appendFileSync(AUDIT_LOG, JSON.stringify({
      ts: new Date().toISOString(),
      ip: req.ip || req.connection?.remoteAddress || null,
      method: req.method,
      route: req.originalUrl || req.url,
      agent,
      summary,
      status,
    }) + '\n');
  } catch (e) {
    console.error(`Failed to append audit log: ${e.message}`);
  }
}

function appendSystemInfoLog(event) {
  try {
    appendFileSync(SYSTEM_INFO_LOG, JSON.stringify(event) + '\n');
  } catch (e) {
    console.error(`Failed to append system info log: ${e.message}`);
  }
}

// Severity classification for alert types
const ALERT_SEVERITY_MAP = {
  swap_high: 'critical', server_offline: 'critical',
  agent_blocked: 'warning', mcp_missing: 'warning', agent_offline: 'warning',
  resource_alert: 'warning', agent_rule: 'warning', bridge_warning: 'warning',
  mcp_recovered: 'info', swap_clear: 'info',
  server_takeover: 'info', supervisor_nudge: 'info', supervisor_escalation: 'warning',
};
const SYSTEM_INFO_RECOVERY_DAMPENER_MS_RAW = Number.parseInt(process.env.SYSTEM_INFO_RECOVERY_DAMPENER_MS || '300000', 10);
const SYSTEM_INFO_RECOVERY_DAMPENER_MS = Number.isFinite(SYSTEM_INFO_RECOVERY_DAMPENER_MS_RAW) && SYSTEM_INFO_RECOVERY_DAMPENER_MS_RAW > 0
  ? SYSTEM_INFO_RECOVERY_DAMPENER_MS_RAW
  : 0;
const recentlyResolvedAlertKeys = new Map();

const ALERT_ACTION_FIELD_KEYS = ['owner', 'assignee', 'runbook', 'impact', 'recoveryCondition', 'exitCondition', 'correlation'];

function mergeAlertCorrelation(base = null, override = null) {
  const result = {};
  if (base && typeof base === 'object' && !Array.isArray(base)) Object.assign(result, base);
  if (override && typeof override === 'object' && !Array.isArray(override)) Object.assign(result, override);
  return Object.keys(result).length ? result : null;
}

function defaultAlertActionFields(alertType, opts = {}) {
  const sourceAgent = normalizeOptionalText(opts.sourceAgent, 128);
  const dedupeKey = normalizeOptionalText(opts.dedupeKey, 255) || alertType;
  if (alertType === 'server_offline') {
    const serverId = sourceAgent || String(dedupeKey || '').replace(/^server_offline:/, '') || null;
    return {
      owner: 'remote-runtime',
      runbook: 'docs/runbooks/remote-server-offline.md',
      impact: 'remote agents on this server are marked offline and direct push delivery is unavailable until the relay recovers',
      recoveryCondition: 'the next accepted heartbeat from this server auto-resolves this alert',
      correlation: {
        dedupeKey,
        serverId,
      },
    };
  }
  if (alertType === 'mcp_missing') {
    return {
      owner: 'agent-runtime',
      runbook: 'docs/runbooks/mcp-missing.md',
      impact: 'the agent cannot receive MCP tool calls until its MCP process is restored',
      recoveryCondition: 'mcp_recovered for this agent auto-resolves this alert',
      correlation: {
        dedupeKey,
        agent: sourceAgent || null,
      },
    };
  }
  if (alertType === 'agent_blocked') {
    return {
      owner: 'agent-runtime',
      runbook: 'docs/runbooks/agent-blocked.md',
      impact: 'the agent may be unable to process pending human or operator messages until the blocked state clears',
      recoveryCondition: 'agent_recovered for this agent auto-resolves this alert',
      correlation: {
        dedupeKey,
        agent: sourceAgent || null,
      },
    };
  }
  if (alertType === 'agent_rule') {
    const parts = String(dedupeKey || '').split(':');
    return {
      owner: 'agent-runtime',
      runbook: 'docs/runbooks/agent-rule.md',
      impact: 'the agent may be violating an operator-facing response or inbox contract until the rule clears',
      recoveryCondition: 'the rule condition becomes false and auto-resolves this alert',
      correlation: {
        dedupeKey,
        agent: sourceAgent || parts[1] || null,
        rule: parts[2] || null,
      },
    };
  }
  if (alertType === 'resource_alert') {
    return {
      owner: 'host-runtime',
      runbook: 'docs/runbooks/resource-alert.md',
      impact: 'the agent process is above its resource budget and may stall or be killed',
      recoveryCondition: 'resource usage falls below the configured clear threshold',
      correlation: {
        dedupeKey,
        agent: sourceAgent || null,
      },
    };
  }
  return {};
}

function buildAlertActionFields(alertType, opts = {}) {
  const defaults = defaultAlertActionFields(alertType, opts);
  const provided = {};
  for (const key of ALERT_ACTION_FIELD_KEYS) {
    if (opts[key] !== undefined) provided[key] = opts[key];
  }
  return {
    ...defaults,
    ...provided,
    correlation: mergeAlertCorrelation(defaults.correlation, provided.correlation),
  };
}

function pruneRecentlyResolvedAlertKeys(now = Date.now()) {
  if (SYSTEM_INFO_RECOVERY_DAMPENER_MS <= 0 || recentlyResolvedAlertKeys.size === 0) return;
  const cutoff = now - SYSTEM_INFO_RECOVERY_DAMPENER_MS;
  for (const [key, ts] of recentlyResolvedAlertKeys) {
    if (ts < cutoff) recentlyResolvedAlertKeys.delete(key);
  }
}

function recordRecentlyResolvedAlertKey(dedupeKey, now = Date.now()) {
  if (SYSTEM_INFO_RECOVERY_DAMPENER_MS <= 0) return;
  const key = normalizeOptionalText(dedupeKey, 255);
  if (!key) return;
  pruneRecentlyResolvedAlertKeys(now);
  recentlyResolvedAlertKeys.set(key, now);
}

function wasRecentlyResolvedAlertKey(dedupeKey, now = Date.now()) {
  if (SYSTEM_INFO_RECOVERY_DAMPENER_MS <= 0) return false;
  const key = normalizeOptionalText(dedupeKey, 255);
  if (!key) return false;
  pruneRecentlyResolvedAlertKeys(now);
  const resolvedAt = recentlyResolvedAlertKeys.get(key);
  if (Number.isFinite(resolvedAt) && (now - resolvedAt) < SYSTEM_INFO_RECOVERY_DAMPENER_MS) return true;
  try {
    for (const alert of alertStore.dump()) {
      if (alert?.dedupeKey !== key || alert.status !== 'resolved') continue;
      const durableResolvedAt = Number(alert.resolvedAt) || 0;
      if (durableResolvedAt && (now - durableResolvedAt) < SYSTEM_INFO_RECOVERY_DAMPENER_MS) {
        recentlyResolvedAlertKeys.set(key, durableResolvedAt);
        return true;
      }
    }
  } catch { /* alert sidecar remains best-effort for system info */ }
  return false;
}

function emitSystemInfo(summary, full = '', alertType = null, opts = {}) {
  const now = Date.now();
  const eventDedupeKey = alertType ? normalizeOptionalText(opts.dedupeKey, 255) : null;
  const dedupeKey = alertType ? (eventDedupeKey || alertType) : null;
  const event = {
    id: `sys_${now}_${Math.random().toString(36).slice(2, 8)}`,
    ts: now,
    summary,
    full: full || '',
    alertType: alertType || null,
    dedupeKey: eventDedupeKey,
    source: 'system',
    group: 'info',
    type: 'inform',
  };

  let suppressSystemInfo = Boolean(opts.suppressSystemInfo);

  // Hook A: alert ingestion
  if (alertType) {
    const recoveryTarget = ALERT_RECOVERY_MAP[alertType];
    if (!recoveryTarget && dedupeKey && wasRecentlyResolvedAlertKey(dedupeKey, now)) {
      suppressSystemInfo = true;
    }
    if (recoveryTarget) {
      // Recovery event — auto-resolve matching alerts
      try {
        if (opts.sourceAgent) {
          const resolved = alertStore.autoResolve(`${recoveryTarget}:${opts.sourceAgent}`);
          if (resolved?.dedupeKey) recordRecentlyResolvedAlertKey(resolved.dedupeKey, now);
        } else {
          const resolved = alertStore.autoResolveByPrefix(recoveryTarget);
          if (Array.isArray(resolved)) {
            for (const alert of resolved) recordRecentlyResolvedAlertKey(alert?.dedupeKey, now);
          }
        }
      } catch { /* alert sidecar remains best-effort for system info */ }
    } else if (!opts.skipAlertIngest) {
      // Non-recovery event — ingest as alert
      try {
        alertStore.ingest({
          alertType,
          dedupeKey,
          severity: ALERT_SEVERITY_MAP[alertType] || 'info',
          source: opts.source || 'backend',
          sourceAgent: opts.sourceAgent || null,
          summary,
          detail: full || null,
          ...buildAlertActionFields(alertType, { ...opts, dedupeKey }),
        });
      } catch { /* ingest validation failure — non-fatal */ }
    }
  }

  if (!suppressSystemInfo) {
    ensureInfoGroup();
    appendSystemInfoLog(event);
    broadcastSSE('system_info', event);
  }

  return event;
}

function isSuppressedForAgent(msg, agentName) {
  return Array.isArray(msg?.suppressedRecipients) && msg.suppressedRecipients.includes(agentName);
}

// ── Notification Router ───────────────────────────────────────────────
const notificationRouter = new NotificationRouter({
  agent_blocked: {
    cooldownMs: BLOCKED_NOTIFICATION_COOLDOWN_MS,
    aggregateWindowMs: BLOCKED_INFO_AGGREGATE_WINDOW_MS,
    dedupeKeyFn: (p) => p.agentName || 'unknown',
    persistedCooldown: {
      read: (key) => {
        const rt = agentRuntime[key];
        return Math.max(0, Number(rt?.lastBlockedNotificationTs) || 0);
      },
      write: (key, ts) => {
        const rt = agentRuntime[key];
        if (rt && (Number(rt.lastBlockedNotificationTs) || 0) !== ts) {
          rt.lastBlockedNotificationTs = ts;
          saveAgentRuntime();
        }
      },
    },
    aggregateFn: (buffer) => {
      const blocked = [];
      const recovered = [];
      for (const [, p] of buffer) {
        if (p.recovered) recovered.push(p.agentName);
        else blocked.push(p);
      }
      // Per-agent alert ingestion (spec §11.4)
      const recentlyRecoveredBlockedKeys = new Set();
      for (const p of blocked) {
        const perAgentDedupeKey = `agent_blocked:${p.agentName}`;
        if (wasRecentlyResolvedAlertKey(perAgentDedupeKey)) {
          recentlyRecoveredBlockedKeys.add(perAgentDedupeKey);
        }
        try {
          alertStore.ingest({
            alertType: 'agent_blocked',
            dedupeKey: perAgentDedupeKey,
            severity: 'warning',
            source: 'backend',
            sourceAgent: p.agentName,
            summary: `Agent '${p.agentName}' blocked (${p.tier === BLOCK_TIER_TRANSIENT ? 'transient' : (p.tier === BLOCK_TIER_SOFT ? 'soft' : 'hard')})`,
            detail: p.full || null,
            owner: 'agent-runtime',
            runbook: 'docs/runbooks/agent-blocked.md',
            impact: 'the agent may be unable to process pending human or operator messages until the blocked state clears',
            recoveryCondition: 'agent_recovered for this agent auto-resolves this alert',
            correlation: {
              dedupeKey: perAgentDedupeKey,
              agent: p.agentName,
              tier: p.tier === BLOCK_TIER_TRANSIENT ? 'transient' : (p.tier === BLOCK_TIER_SOFT ? 'soft' : 'hard'),
            },
          });
        } catch { /* non-fatal */ }
      }
      for (const agentName of recovered) {
        const resolved = alertStore.autoResolve(`agent_blocked:${agentName}`);
        if (resolved?.dedupeKey) recordRecentlyResolvedAlertKey(resolved.dedupeKey);
      }
      const parts = [];
      const fullParts = [];
      if (blocked.length) {
        const entries = blocked.map(p => {
          const label = p.tier === BLOCK_TIER_TRANSIENT ? 'transient' : (p.tier === BLOCK_TIER_SOFT ? 'soft' : 'hard');
          return `${p.agentName} (${label})`;
        });
        parts.push(`${blocked.length} blocked: ${entries.join(', ')}`);
        for (const p of blocked) { if (p.full) fullParts.push(p.full); }
      }
      if (recovered.length) parts.push(`${recovered.length} recovered: ${recovered.join(', ')}`);
      const singleBlocked = blocked.length === 1 && recovered.length === 0 ? blocked[0] : null;
      const singleRecovered = recovered.length === 1 && blocked.length === 0 ? recovered[0] : null;
      return {
        summary: `Agent state summary: ${parts.join('; ')}`,
        full: fullParts.join('\n---\n'),
        alertType: singleRecovered ? 'agent_recovered' : 'agent_blocked',
        agentName: singleBlocked?.agentName || singleRecovered || null,
        dedupeKey: singleBlocked ? `agent_blocked:${singleBlocked.agentName}` : null,
        alertStoreManaged: true,
        suppressSystemInfo: singleBlocked ? recentlyRecoveredBlockedKeys.has(`agent_blocked:${singleBlocked.agentName}`) : false,
      };
    },
    sinks: ['log'],
  },
  agent_compact: {
    cooldownMs: AGENT_COMPACT_RUNTIME_DEDUPE_MS,
    dedupeKeyFn: (p) => `${p.agentName}:${p.marker}:${p.mode}`,
    sinks: ['sse'],
  },
  agent_offline: {
    cooldownMs: UNEXPECTED_OFFLINE_ALERT_THROTTLE_MS,
    dedupeKeyFn: (p) => `${p.agentName}:${p.reason || 'unknown'}`,
    sinks: ['log'],
  },
  resource_alert: {
    cooldownMs: AGENT_SCOPE_ALERT_COOLDOWN_MS,
    dedupeKeyFn: (p) => p.agentName,
    sinks: ['log'],
  },
}, {
  log: (_family, payload) => {
    if (!payload.summary) return;
    const opts = {};
    if (payload.agentName) {
      opts.sourceAgent = payload.agentName;
      opts.dedupeKey = `${_family}:${payload.agentName}:${payload.reason || ''}`.replace(/:$/, '');
    }
    if (payload.dedupeKey) opts.dedupeKey = payload.dedupeKey;
    if (payload.alertStoreManaged) opts.skipAlertIngest = true;
    if (payload.suppressSystemInfo) opts.suppressSystemInfo = true;
    emitSystemInfo(payload.summary, payload.full || '', payload.alertType || _family, opts);
  },
  sse: (_family, payload) => {
    if (payload.sseEvent) broadcastSSE(payload.sseEvent, payload.sseData || payload);
  },
});


const taskGraphStore = createTaskGraphStore({
  initialGraphs: taskGraphs,
  save: (nextGraphs) => saveTaskGraphs(nextGraphs),
  dispatchMessage: (payload) => dispatchTaskGraphMessage(payload),
  emitEvent: (eventName, payload) => broadcastSSE(eventName, payload),
});

function buildTaskGraphDispatchKey(graphId, nodeId) {
  const graphPart = normalizeOptionalText(graphId, 255);
  const nodePart = normalizeOptionalText(nodeId, 255);
  return graphPart && nodePart ? `task_graph_dispatch:${graphPart}:${nodePart}` : null;
}

function findTaskGraphDispatchMessage(graphId, nodeId, dispatchKey = null) {
  const normalizedGraphId = normalizeOptionalText(graphId, 255);
  const normalizedNodeId = normalizeOptionalText(nodeId, 255);
  const normalizedDispatchKey = normalizeOptionalText(dispatchKey, 512)
    || buildTaskGraphDispatchKey(normalizedGraphId, normalizedNodeId);
  if (!normalizedGraphId || !normalizedNodeId) return null;
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i];
    const schema = msg?.schema;
    if (schema?.kind !== 'task_graph_dispatch') continue;
    const payload = schema?.payload && typeof schema.payload === 'object' && !Array.isArray(schema.payload)
      ? schema.payload
      : {};
    const payloadDispatchKey = normalizeOptionalText(payload.dispatchKey ?? payload.dispatch_key, 512);
    const payloadGraphId = normalizeOptionalText(payload.graphId ?? payload.graph_id, 255);
    const payloadNodeId = normalizeOptionalText(payload.nodeId ?? payload.node_id, 255);
    if (normalizedDispatchKey && payloadDispatchKey === normalizedDispatchKey) return msg;
    if (payloadGraphId === normalizedGraphId && payloadNodeId === normalizedNodeId) return msg;
  }
  return null;
}

function dispatchTaskGraphMessage(payload = {}) {
  const schema = payload?.schema && typeof payload.schema === 'object' && !Array.isArray(payload.schema)
    ? payload.schema
    : {};
  const schemaPayload = schema.payload && typeof schema.payload === 'object' && !Array.isArray(schema.payload)
    ? schema.payload
    : {};
  const graphId = normalizeOptionalText(schemaPayload.graphId ?? schemaPayload.graph_id, 255);
  const nodeId = normalizeOptionalText(schemaPayload.nodeId ?? schemaPayload.node_id, 255);
  const dispatchKey = normalizeOptionalText(schemaPayload.dispatchKey ?? schemaPayload.dispatch_key, 512)
    || buildTaskGraphDispatchKey(graphId, nodeId);
  const existing = findTaskGraphDispatchMessage(graphId, nodeId, dispatchKey);
  if (existing) return existing;
  return dispatchInternalDirectMessage({
    ...payload,
    schema: {
      ...schema,
      kind: 'task_graph_dispatch',
      version: Number(schema.version) || 1,
      payload: {
        ...schemaPayload,
        dispatchKey,
        graphId,
        nodeId,
      },
    },
  });
}

function taskGraphErrorStatus(error) {
  switch (error?.code) {
    case 'graph_not_found':
    case 'node_not_found':
      return 404;
    case 'graph_exists':
      return 409;
    case 'graph_persistence_failed':
    case 'graph_dispatch_failed':
      return 503;
    default:
      return 400;
  }
}

function respondTaskGraphError(res, error, fallback = 'task graph error') {
  return res.status(taskGraphErrorStatus(error)).json({ error: error?.message || fallback });
}

function isTaskGraphDurabilityError(error) {
  return error?.code === 'graph_persistence_failed' || error?.code === 'graph_dispatch_failed';
}

function handleTaskGraphMessageHook(msg) {
  const kind = normalizeOptionalText(msg?.schema?.kind, 128);
  if (kind !== 'task_graph_result' && kind !== 'task_graph_failed') return null;
  const payload = (msg?.schema?.payload && typeof msg.schema.payload === 'object' && !Array.isArray(msg.schema.payload))
    ? msg.schema.payload
    : null;
  if (!payload) return null;
  const graphId = normalizeOptionalText(payload.graphId ?? payload.graph_id, 255);
  const nodeId = normalizeOptionalText(payload.nodeId ?? payload.node_id, 255);
  if (!graphId || !nodeId) return null;
  const graph = taskGraphStore.getGraph(graphId);
  const node = taskGraphStore.getNode(graphId, nodeId);
  if (!graph || !node || graph.status !== 'active') return null;
  if (!['pending', 'dispatched', 'active'].includes(node.status)) return null;
  if (String(msg?.from || '').trim().toLowerCase() !== String(node.assignee || '').trim().toLowerCase()) return null;
  if (node.message_id && normalizeOptionalText(msg?.reply_to, 255) !== node.message_id) return null;

  const patch = kind === 'task_graph_result'
    ? {
      status: 'complete',
      result: Object.prototype.hasOwnProperty.call(payload, 'result') ? payload.result : null,
    }
    : {
      status: 'failed',
      error: normalizeOptionalText(payload.error, 4000) || 'task graph node failed',
    };

  try {
    taskGraphStore.updateNode(graphId, nodeId, patch);
    const advanced = taskGraphStore.advanceGraph(graphId) || taskGraphStore.getGraph(graphId);
    return {
      handled: true,
      graphId,
      nodeId,
      status: patch.status,
      graphStatus: advanced?.status || graph.status,
    };
  } catch (error) {
    if (isTaskGraphDurabilityError(error)) throw error;
    console.warn(`task graph message hook ignored (${graphId}/${nodeId}): ${error?.message || error}`);
    return null;
  }
}

// ── Helpers ───────────────────────────────────────────────────────────
function relativeTime(ts) {
  const d = Date.now() - ts;
  if (d < 60_000) return `${Math.floor(d / 1000)}s ago`;
  if (d < 3_600_000) return `${Math.floor(d / 60_000)}m ago`;
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`;
  return `${Math.floor(d / 86_400_000)}d ago`;
}

function summarizeMsg(m) {
  const out = {
    id: m.id,
    from: m.from,
    type: m.type,
    priority: normalizeMessagePriority(m?.priority),
    summary: m.summary,
    full: m.full || '',
    mentions: m.mentions || [],
    attachments: Array.isArray(m.attachments) ? m.attachments : [],
    ts: m.ts,
    at: new Date(m.ts).toISOString(),
    time: relativeTime(m.ts),
    reply_to: m.reply_to || null,
    // Carried out to the bridge, which is the only consumer that can act on it — see the field's note
    // at the ingestion site. Serialised only when true so every other message keeps its current shape.
    ...(m.incidental === true ? { incidental: true } : {}),
    group: m.group || null,
    source: m.source || 'api',
    sourceRoom: m.sourceRoom || null,
    sourceEventId: m.sourceEventId || null,
    senderMxid: m.senderMxid || null,
    trustLevel: m.trustLevel || null,
    fromId: m.fromId || null,
    matrixContext: m.matrixContext || null,
    replyContext: m.replyContext || null,
    matrixDelivery: m.matrixDelivery || null,
  };
  const normalizedSchema = normalizeMessageSchema(m?.schema);
  if (normalizedSchema.value) out.schema = normalizedSchema.value;
  return out;
}

function createMessageViewToken() {
  return randomBytes(24).toString('base64url');
}

function constantTimeStringEqual(left, right) {
  const leftBuf = Buffer.from(String(left || ''), 'utf8');
  const rightBuf = Buffer.from(String(right || ''), 'utf8');
  if (leftBuf.length === 0 || leftBuf.length !== rightBuf.length) return false;
  return timingSafeEqual(leftBuf, rightBuf);
}

function normalizeMessagePriority(value, fallback = 'normal') {
  if (value === undefined || value === null) return fallback;
  const raw = normalizeOptionalText(value, 16);
  if (!raw) return fallback;
  const lower = raw.toLowerCase();
  if (lower === 'normal' || lower === 'high' || lower === 'urgent') return lower;
  return null;
}

function messagePriorityRank(value) {
  const priority = normalizeMessagePriority(value);
  if (priority === 'urgent') return 2;
  if (priority === 'high') return 1;
  return 0;
}

function highestMessagePriority(rows = []) {
  let best = 'normal';
  for (const row of Array.isArray(rows) ? rows : []) {
    if (messagePriorityRank(row?.priority) > messagePriorityRank(best)) {
      best = normalizeMessagePriority(row?.priority) || best;
    }
  }
  return best;
}

function normalizeMessageSchema(value) {
  if (value === undefined) return { value: null };
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return { error: 'schema must be an object' };
  }
  const kind = normalizeOptionalText(value.kind, 128);
  if (!kind) return { error: 'schema.kind required' };
  let version = 1;
  if (Object.prototype.hasOwnProperty.call(value, 'version') && value.version !== undefined && value.version !== null) {
    if (typeof value.version !== 'number') {
      return { error: 'schema.version must be a positive integer' };
    }
    const parsed = value.version;
    if (!Number.isInteger(parsed) || parsed < 1) {
      return { error: 'schema.version must be a positive integer' };
    }
    version = parsed;
  }
  const out = { kind, version };
  if (Object.prototype.hasOwnProperty.call(value, 'payload')) out.payload = value.payload;
  return { value: out };
}

function parseKindsFilter(value) {
  if (value === undefined || value === null) return [];
  const rawItems = Array.isArray(value)
    ? value.flatMap(item => String(item || '').split(','))
    : String(value || '').split(',');
  const out = [];
  const seen = new Set();
  for (const raw of rawItems) {
    const kind = normalizeOptionalText(raw, 128);
    if (!kind || seen.has(kind)) continue;
    seen.add(kind);
    out.push(kind);
  }
  return out;
}

function messageMatchesKinds(msg, kinds = null) {
  if (!(kinds instanceof Set) || kinds.size === 0) return true;
  const kind = normalizeOptionalText(msg?.schema?.kind, 128);
  return Boolean(kind && kinds.has(kind));
}

function ensureCursor(agentName) {
  if (!cursors[agentName]) {
    cursors[agentName] = { inbox: 0, inboxId: null, groups: {}, groupIds: {} };
  }
  if (!cursors[agentName].groups || typeof cursors[agentName].groups !== 'object') {
    cursors[agentName].groups = {};
  }
  if (!cursors[agentName].groupIds || typeof cursors[agentName].groupIds !== 'object') {
    cursors[agentName].groupIds = {};
  }
  if (!Object.prototype.hasOwnProperty.call(cursors[agentName], 'inbox')) cursors[agentName].inbox = 0;
  if (!Object.prototype.hasOwnProperty.call(cursors[agentName], 'inboxId')) cursors[agentName].inboxId = null;
  return cursors[agentName];
}

function snapshotCursor(agentName) {
  return Object.prototype.hasOwnProperty.call(cursors, agentName)
    ? cloneJsonValue(cursors[agentName])
    : undefined;
}

function restoreCursor(agentName, snapshot) {
  if (snapshot === undefined) delete cursors[agentName];
  else cursors[agentName] = snapshot;
}

function isAfterCursor(msg, ts, id) {
  if (!msg) return false;
  const cursorTs = Number(ts) || 0;
  const cursorId = typeof id === 'string' ? id : null;
  if (msg.ts > cursorTs) return true;
  if (msg.ts < cursorTs) return false;
  if (!cursorId) return false;
  return compareMsgOrder(msg, { ts: cursorTs, id: cursorId }) > 0;
}

function firstMessageAfterCursorIndex(rows, ts, id) {
  const list = Array.isArray(rows) ? rows : [];
  const cursorTs = Number(ts) || 0;
  const cursorId = typeof id === 'string' ? id : null;
  let low = 0;
  let high = list.length;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    const msg = list[mid];
    const after = cursorId
      ? compareMsgOrder(msg, { ts: cursorTs, id: cursorId }) > 0
      : (Number(msg?.ts) || 0) > cursorTs;
    if (after) high = mid;
    else low = mid + 1;
  }
  return low;
}

function advanceInboxCursor(cursor, unread) {
  if (!Array.isArray(unread) || unread.length === 0) return false;
  const last = unread[unread.length - 1];
  cursor.inbox = last.ts;
  cursor.inboxId = last.id;
  return true;
}

function getGroupCursor(cursor, groupName) {
  return {
    ts: Number(cursor.groups?.[groupName]) || 0,
    id: typeof cursor.groupIds?.[groupName] === 'string' ? cursor.groupIds[groupName] : null,
  };
}

function advanceGroupCursor(cursor, groupName, unread) {
  if (!Array.isArray(unread) || unread.length === 0) return false;
  const last = unread[unread.length - 1];
  if (!cursor.groups) cursor.groups = {};
  if (!cursor.groupIds) cursor.groupIds = {};
  cursor.groups[groupName] = last.ts;
  cursor.groupIds[groupName] = last.id;
  return true;
}

function getGroupMembers(groupName) {
  return Array.isArray(groups[groupName]?.members) ? groups[groupName].members : [];
}

function findGroupMember(groupName, name) {
  const target = normalizeAgentName(name);
  if (!target) return null;
  const members = getGroupMembers(groupName);
  const exact = members.find(m => normalizeAgentName(m) === target);
  if (exact) return exact;
  const targetLower = target.toLowerCase();
  return members.find(m => normalizeAgentName(m)?.toLowerCase() === targetLower) || null;
}

function isGroupMember(groupName, name) {
  return Boolean(findGroupMember(groupName, name));
}

function getUnreadInboxMessages(agentName, options = {}) {
  const cursor = ensureCursor(agentName);
  const inboxTs = cursor.inbox || 0;
  const inboxId = cursor.inboxId || null;
  const kinds = Array.isArray(options?.kinds) && options.kinds.length > 0
    ? new Set(options.kinds)
    : null;
  const index = getUnreadMessageIndex();
  const unreadById = new Map();

  const directRows = index.directByAgent.get(agentName) || [];
  for (const m of directRows.slice(firstMessageAfterCursorIndex(directRows, inboxTs, inboxId))) {
    if (isSuppressedForAgent(m, agentName)) continue;
    if (!messageMatchesKinds(m, kinds)) continue;
    unreadById.set(m.id, m);
  }
  const mentionRows = index.groupMentionsByAgent.get(agentName) || [];
  for (const m of mentionRows.slice(firstMessageAfterCursorIndex(mentionRows, inboxTs, inboxId))) {
    if (!isGroupMember(m.group, agentName) && m.matrixDefaultRecipient !== agentName) continue;
    if (!messageMatchesKinds(m, kinds)) continue;
    if (isSuppressedForAgent(m, agentName)) continue;
    unreadById.set(m.id, m);
  }

  const unread = [...unreadById.values()].sort(compareMsgOrder);
  return { inboxTs, inboxId, unread };
}

function invalidatePendingHumanTargets(agentNames = null) {
  if (agentNames === null || agentNames === undefined) {
    pendingHumanTargetCache.clear();
    return;
  }
  const names = Array.isArray(agentNames) ? agentNames : [agentNames];
  for (const raw of names) {
    const name = normalizeAgentName(raw) || (typeof raw === 'string' ? raw.trim() : '');
    if (!name) continue;
    pendingHumanTargetCache.delete(name);
  }
}

function invalidatePendingHumanTargetsForMessage(msg) {
  if (!msg || msg.type !== 'human') return;
  const targets = new Set();
  const directTarget = normalizeAgentName(msg.to) || (typeof msg.to === 'string' ? msg.to.trim() : '');
  if (directTarget) targets.add(directTarget);
  for (const mention of Array.isArray(msg.mentions) ? msg.mentions : []) {
    const name = normalizeAgentName(mention) || (typeof mention === 'string' ? mention.trim() : '');
    if (name) targets.add(name);
  }
  invalidatePendingHumanTargets([...targets]);
}

function snapshotForceDeletePersistenceState(name) {
  return {
    hadAgent: Object.prototype.hasOwnProperty.call(agents, name),
    agent: Object.prototype.hasOwnProperty.call(agents, name) ? cloneJsonValue(agents[name]) : null,
    hadRuntime: Object.prototype.hasOwnProperty.call(agentRuntime, name),
    runtime: Object.prototype.hasOwnProperty.call(agentRuntime, name) ? cloneJsonValue(agentRuntime[name]) : null,
    hadCursor: Object.prototype.hasOwnProperty.call(cursors, name),
    cursor: Object.prototype.hasOwnProperty.call(cursors, name) ? cloneJsonValue(cursors[name]) : null,
    hadTombstone: Object.prototype.hasOwnProperty.call(deletedAgentTombstones, name),
    tombstone: Object.prototype.hasOwnProperty.call(deletedAgentTombstones, name)
      ? cloneJsonValue(deletedAgentTombstones[name])
      : null,
  };
}

function restoreForceDeletePersistenceState(name, snapshot) {
  if (snapshot.hadAgent) agents[name] = cloneJsonValue(snapshot.agent);
  else delete agents[name];
  if (snapshot.hadRuntime) agentRuntime[name] = cloneJsonValue(snapshot.runtime);
  else delete agentRuntime[name];
  if (snapshot.hadCursor) cursors[name] = cloneJsonValue(snapshot.cursor);
  else delete cursors[name];
  if (snapshot.hadTombstone) deletedAgentTombstones[name] = cloneJsonValue(snapshot.tombstone);
  else delete deletedAgentTombstones[name];
}

function rollbackForceDeletePersistenceState(name, snapshot, changed) {
  restoreForceDeletePersistenceState(name, snapshot);
  let rollbackOk = true;
  if (changed.tombstone && !saveJson('deleted_agents.json', deletedAgentTombstones, { immediate: true })) rollbackOk = false;
  if (changed.agents && !saveAgents(true)) rollbackOk = false;
  if (changed.runtime && !saveAgentRuntime(true)) rollbackOk = false;
  if (changed.cursors && !saveCursors()) rollbackOk = false;
  if (!rollbackOk) {
    console.error(`[force-delete] failed to fully roll back persistence state for ${name}`);
  }
}

function persistForceDeletedAgentState(name) {
  const snapshot = snapshotForceDeletePersistenceState(name);
  const changed = {
    agents: agents[name] !== undefined,
    runtime: agentRuntime[name] !== undefined,
    cursors: cursors[name] !== undefined,
    tombstone: true,
  };

  if (changed.agents) delete agents[name];
  if (changed.runtime) delete agentRuntime[name];
  if (changed.cursors) delete cursors[name];
  deletedAgentTombstones[name] = { deletedAt: Date.now(), reason: 'force-delete' };

  if (!saveJson('deleted_agents.json', deletedAgentTombstones, { immediate: true })) {
    restoreForceDeletePersistenceState(name, snapshot);
    return { ok: false, error: 'agent force-delete persistence failed' };
  }
  if (changed.agents && !saveAgents(true)) {
    rollbackForceDeletePersistenceState(name, snapshot, changed);
    return { ok: false, error: 'agent force-delete persistence failed' };
  }
  if (changed.runtime && !saveAgentRuntime(true)) {
    rollbackForceDeletePersistenceState(name, snapshot, changed);
    return { ok: false, error: 'agent force-delete persistence failed' };
  }
  if (changed.cursors && !saveCursors()) {
    rollbackForceDeletePersistenceState(name, snapshot, changed);
    return { ok: false, error: 'agent force-delete persistence failed' };
  }

  return {
    ok: true,
    removed: changed.agents,
    runtimeRemoved: changed.runtime,
    cursorsRemoved: changed.cursors,
  };
}

function cleanupDeletedAgentRuntimeState(name) {
  let agentDataRemoved = false;

  const machine = agentMachines.get(name);
  if (machine) { machine.destroy(); agentMachines.delete(name); }
  invalidatePendingHumanTargets(name);
  localActivityState.delete(name);
  localTmuxMissingState.delete(name);
  localCompactState.delete(name);
  localRuntimeSignalDigest.delete(name);
  scopePressureState.delete(name);
  notificationRouter.clearAgent(name);

  // Clean up supervisor state for the deleted agent after the tombstone is durable.
  try {
    supervisorSnapshotStore.removeTarget(name);
  } catch (error) {
    console.warn(`[supervisor] failed to remove snapshot for deleted agent '${name}': ${error?.message || error}`);
  }
  try { killSupervisorTmux(`supervisor-${name}`); } catch { /* tmux not available */ }

  const agentDataDir = agentDataPath(name);
  if (existsSync(agentDataDir)) {
    try {
      rmSync(agentDataDir, { recursive: true, force: true });
      agentDataRemoved = true;
    } catch (error) {
      console.warn(`failed to remove agent data dir for ${name}: ${error?.message || error}`);
    }
  }

  return { agentDataRemoved };
}

function clearDeletedAgentState(agentName) {
  const name = normalizeAgentName(agentName);
  if (!name) return { ok: false, error: 'invalid agent name' };
  const persisted = persistForceDeletedAgentState(name);
  if (!persisted.ok) return persisted;
  const cleanup = cleanupDeletedAgentRuntimeState(name);
  return { ...persisted, ...cleanup };
}

function messageTargetsAgent(msg, agentName) {
  if (!msg || !agentName) return false;
  if (msg.matrixRoomRecipients?.includes(agentName)) return true;
  if (msg.to === agentName) return true;
  if (!msg.group) return false;
  if (!isGroupMember(msg.group, agentName) && msg.matrixDefaultRecipient !== agentName) return false;
  return Array.isArray(msg.mentions) && msg.mentions.includes(agentName);
}

function messageVisibleToAgent(msg, agentName) {
  const normalized = normalizeAgentName(agentName);
  if (!msg || !normalized) return false;
  if (msg.from === normalized || msg.to === normalized) return true;
  if (msg.matrixRoomRecipients?.includes(normalized)) return true;
  if (msg.matrixDefaultRecipient === normalized) return true;
  if (msg.group && isGroupMember(msg.group, normalized)) return true;
  return false;
}

function deliveryTargetAgentsForMessage(msg, directTargetKind = null) {
  if (msg?.matrixRoomRecipients) return msg.matrixRoomRecipients.filter(name => !isSuppressedForAgent(msg, name));
  const targets = new Set();
  if (msg?.to && directTargetKind === 'agent' && !isSuppressedForAgent(msg, msg.to)) {
    targets.add(msg.to);
  }
  if (msg?.group && Array.isArray(msg.mentions)) {
    for (const name of msg.mentions) {
      if (!name || name === msg.from || isSuppressedForAgent(msg, name)) continue;
      if (isAgentRecord(agents[name])) targets.add(name);
    }
  }
  if (msg?.matrixDefaultRecipient && isAgentRecord(agents[msg.matrixDefaultRecipient])) {
    targets.add(msg.matrixDefaultRecipient);
  }
  return [...targets];
}

function hasMessageViewTokenAccess(req, msg) {
  const token = normalizeOptionalText(req.query?.view || req.query?.view_token || req.query?.token, 256);
  if (!token || !msg?.viewToken) return false;
  return constantTimeStringEqual(token, msg.viewToken);
}

function authorizeMessageDetailAccess(req, msg, options = {}) {
  if (options.allowViewToken && hasMessageViewTokenAccess(req, msg)) return { ok: true, mode: 'view-token' };
  if (hasApiTokenAccess(req)) return { ok: true, mode: 'bearer' };

  const agentName = getRequestAgentName(req);
  if (!agentName) return { ok: false, status: 401, error: 'agent identity required' };
  if (!isAgentRecord(agents[agentName])) return { ok: false, status: 404, error: 'agent not found' };
  if (!messageVisibleToAgent(msg, agentName)) {
    return { ok: false, status: 403, error: `agent '${agentName}' cannot access message ${msg?.id || ''}`.trim() };
  }
  return authorizeAgentCredential(req, agentName);
}

function buildUnreadInboxSnapshot(agentName, options = {}) {
  const { unread } = getUnreadInboxMessages(agentName, options);
  let unreadDm = 0;
  let unreadGroupMentions = 0;
  for (const m of unread) {
    if (m.to === agentName) unreadDm++;
    else if (m.group) unreadGroupMentions++;
  }
  return {
    agent: agentName,
    unread_total: unread.length,
    unread_dm: unreadDm,
    unread_group_mentions: unreadGroupMentions,
    unread_ids: unread.map((m) => m?.id).filter(Boolean),
    latest: unread.length > 0 ? summarizeMsg(unread[unread.length - 1]) : null,
  };
}

function persistNewMessage(msg) {
  const index = messages.length;
  messages.push(msg);
  if (saveMessages()) return { ok: true };
  if (messages[index] === msg) {
    messages.splice(index, 1);
  } else {
    const currentIndex = messages.findIndex((row) => row === msg || row?.id === msg?.id);
    if (currentIndex >= 0) messages.splice(currentIndex, 1);
  }
  invalidateUnreadMessageIndex();
  return { ok: false, error: 'messages persistence failed' };
}

function dispatchStoredMessage(msg, options = {}) {
  const senderIsAgent = options.senderIsAgent === true;
  const directTargetKind = options.directTargetKind || null;
  const persisted = persistNewMessage(msg);
  if (!persisted.ok) {
    return { ok: false, status: 503, error: persisted.error };
  }
  const targetAgents = deliveryTargetAgentsForMessage(msg, directTargetKind);
  appendDeliveryEvent({
    type: 'message.accepted',
    source: 'backend',
    messageId: msg.id,
    agent: targetAgents.length === 1 ? targetAgents[0] : null,
    targetAgents,
    priority: msg.priority,
    context: {
      from: msg.from,
      to: msg.to || null,
      group: msg.group || null,
      type: msg.type,
      targetKind: directTargetKind,
    },
  });
  if (Array.isArray(msg.suppressedRecipients) && msg.suppressedRecipients.length > 0) {
    appendDeliveryEvent({
      type: 'message.suppressed',
      source: 'backend',
      messageId: msg.id,
      targetAgents: msg.suppressedRecipients,
      reason: 'suppressed-recipient',
    });
  }
  emitStoredMessageSideEffects(msg, { senderIsAgent, directTargetKind });
  return { ok: true, msg };
}

function emitStoredMessageSideEffects(msg, { senderIsAgent = false, directTargetKind = null } = {}) {
  invalidatePendingHumanTargetsForMessage(msg);
  const compactEvent = buildAgentCompactEvent(msg, senderIsAgent);
  if (compactEvent) {
    broadcastSSE('agent_compact', compactEvent);
  }
  broadcastSSE('message', { ...msg, deliveryOwner: 'dashboard-queue' });
  if (senderIsAgent) {
    markAgentOutbound(msg.from);
  }

  if (msg.to && directTargetKind === 'agent' && msg.to !== msg.from && !isSuppressedForAgent(msg, msg.to)) {
    const state = getAgentDeliveryState(msg.to);
    if (state.online) pushNotify(msg.to, msg);
  }
  if (msg.group && msg.mentions.length > 0) {
    for (const agent of msg.mentions) {
      if (agent === msg.from || isSuppressedForAgent(msg, agent)) continue;
      const state = getAgentDeliveryState(agent);
      if (state.online) pushNotify(agent, msg);
    }
  }
}

function deliveryEventAttemptExists(attemptId) {
  return Boolean(attemptId) && deliveryEventAttemptIds.has(attemptId);
}

function appendDeliveryEventOnce(raw, attemptId) {
  if (deliveryEventAttemptExists(attemptId)) return { ok: true, deduped: true };
  return appendDeliveryEvent({ ...raw, attemptId });
}

function archivedMessageExists(messageId) {
  if (!existsSync(MESSAGE_ARCHIVE_LOG)) return false;
  for (const line of readFileSync(MESSAGE_ARCHIVE_LOG, 'utf8').split('\n')) {
    if (!line.trim()) continue;
    if (JSON.parse(line)?.id === messageId) return true;
  }
  return false;
}

async function emitMatrixStoredMessageSideEffects(msg, receipt, { senderIsAgent = false, directTargetKind = null } = {}) {
  invalidatePendingHumanTargetsForMessage(msg);
  const compactEvent = buildAgentCompactEvent(msg, senderIsAgent);
  if (compactEvent) broadcastSSE('agent_compact', compactEvent);
  broadcastSSE('message', { ...msg, deliveryOwner: 'dashboard-queue' });
  if (senderIsAgent) markAgentOutbound(msg.from);

  for (const agentName of deliveryTargetAgentsForMessage(msg, directTargetKind)) {
    if (THREAD_SESSIONS_ENABLED) {
      const routed = await routeMatrixMessageToThreadSession(agentName, msg);
      if (!routed.ok) {
        appendDeliveryEvent({
          type: 'message.router_refused',
          source: 'backend',
          messageId: msg.id,
          agent: agentName,
          reason: routed.code,
          context: { detail: routed.message },
        });
        return {
          ok: false,
          status: routerRefusalStatus(routed),
          error: `thread-session routing refused for ${agentName}: ${routed.code}: ${routed.message}`,
        };
      }
      appendDeliveryEvent({
        type: 'message.router_accepted',
        source: 'backend',
        messageId: msg.id,
        agent: agentName,
      });
      continue;
    }
    if (ROUTER_SHADOW_ENABLED) {
      try {
        const shadowed = shadowMatrixMessageToRouter(agentName, msg);
        appendDeliveryEvent({
          type: shadowed.ok && shadowed.shadowed
            ? 'message.router_shadowed'
            : 'message.router_shadow_skipped',
          source: 'backend',
          messageId: msg.id,
          agent: agentName,
          reason: shadowed.reason || (shadowed.ok ? null : shadowed.code),
        });
      } catch (error) {
        console.warn(`[router-shadow] message=${msg.id} agent=${agentName} ignored error: ${error?.message || error}`);
      }
    }
    const state = getAgentDeliveryState(agentName);
    if (!state.online || !agents[agentName]?.tmux) continue;
    const result = await pushNotify(agentName, msg, {
      idempotencyKey: `matrix:${receipt.eventId}:${agentName}`,
    });
    const durableInboxFallback = result?.reason === 'local-session-not-found'
      || result?.reason === 'missing-tmux-target';
    if (!isCatchupNotificationComplete(result) && !durableInboxFallback) {
      return { ok: false, status: 503, error: `Matrix wake queue failed for ${agentName}: ${result?.reason || 'unknown error'}` };
    }
  }
  if (THREAD_SESSIONS_ENABLED) {
    routerStore.advanceIngestionCursor(THREAD_SESSION_MESSAGE_CURSOR, threadSessionMessageCursorValue(msg));
  }
  return { ok: true };
}

async function completeMatrixDispatch(receipt) {
  const msg = receipt.message;
  const liveMessageExists = messages.some((row) => row?.id === receipt.messageId);
  const durableArchiveExists = receipt.status === 'committed' && !liveMessageExists
    ? archivedMessageExists(receipt.messageId)
    : false;
  if (!liveMessageExists && !durableArchiveExists) {
    const persisted = persistNewMessage(msg);
    if (!persisted.ok) return { ok: false, status: 503, error: persisted.error };
  }
  if (receipt.status === 'committed') {
    return { ok: true, response: { ...(receipt.response || {}), deduped: true }, newlyCommitted: false };
  }
  if (matrixDispatchFailureStageForTest === 'after-message-persist') {
    matrixDispatchFailureStageForTest = null;
    throw new Error('injected Matrix dispatch failure after message persistence');
  }
  const senderIsAgent = receipt.dispatch?.senderIsAgent === true;
  const directTargetKind = receipt.dispatch?.directTargetKind || null;
  if (receipt.status === 'reserved') {
    const targetAgents = deliveryTargetAgentsForMessage(msg, directTargetKind);
    const accepted = appendDeliveryEventOnce({
      type: 'message.accepted',
      source: 'backend',
      messageId: msg.id,
      agent: targetAgents.length === 1 ? targetAgents[0] : null,
      targetAgents,
      priority: msg.priority,
      context: {
        from: msg.from,
        to: msg.to || null,
        group: msg.group || null,
        type: msg.type,
        targetKind: directTargetKind,
      },
    }, `matrix:${receipt.eventId}:accepted`);
    if (!accepted.ok) return { ok: false, status: 503, error: 'delivery event persistence failed' };
    if (Array.isArray(msg.suppressedRecipients) && msg.suppressedRecipients.length > 0) {
      const suppressed = appendDeliveryEventOnce({
        type: 'message.suppressed',
        source: 'backend',
        messageId: msg.id,
        targetAgents: msg.suppressedRecipients,
        reason: 'suppressed-recipient',
      }, `matrix:${receipt.eventId}:suppressed`);
      if (!suppressed.ok) return { ok: false, status: 503, error: 'suppression event persistence failed' };
    }
    receipt = matrixDispatchStore.accept(receipt.eventId, receipt.response);
  }

  const wake = await emitMatrixStoredMessageSideEffects(msg, receipt, { senderIsAgent, directTargetKind });
  if (!wake.ok) return wake;
  if (matrixDispatchFailureStageForTest === 'after-wake-before-commit') {
    matrixDispatchFailureStageForTest = null;
    throw new Error('injected Matrix dispatch failure after wake before commit');
  }
  const committed = matrixDispatchStore.commit(receipt.eventId, receipt.response);
  return { ok: true, response: committed.response || {}, newlyCommitted: true };
}

function dispatchInternalDirectMessage(payload = {}) {
  const fromName = normalizeAgentName(payload.from) || (typeof payload.from === 'string' ? payload.from.trim() : '') || 'system';
  const toName = normalizeAgentName(payload.to) || (typeof payload.to === 'string' ? payload.to.trim() : '');
  if (!toName) throw new Error('to required');
  const type = typeof payload.type === 'string' ? payload.type.trim().toLowerCase() : 'inform';
  if (!['inform', 'request', 'reply'].includes(type)) {
    throw new Error('invalid type');
  }
  const priority = normalizeMessagePriority(payload.priority);
  if (!priority) throw new Error('invalid priority');
  const normalizedSchema = normalizeMessageSchema(payload.schema);
  if (normalizedSchema.error) throw new Error(normalizedSchema.error);
  const summary = normalizeOptionalText(payload.summary, 4000);
  const full = typeof payload.full === 'string' ? payload.full.trim() : '';
  if (!summary) throw new Error('summary required');
  const idReservation = reserveNextMsgId();
  if (!idReservation.ok) {
    throw new Error(idReservation.error || 'message id reservation failed');
  }

  const msg = {
    id: idReservation.id,
    ts: Date.now(),
    from: fromName,
    to: toName,
    group: null,
    type,
    priority,
    summary,
    full,
    mentions: [],
    reply_to: null,
    source: 'system',
    sourceRoom: null,
    viewToken: createMessageViewToken(),
  };
  if (normalizedSchema.value) msg.schema = normalizedSchema.value;
  const senderIsAgent = fromName !== 'system' && isAgentRecord(agents[fromName]);
  const directTargetKind = isAgentRecord(agents[toName]) ? 'agent' : 'human';
  const result = dispatchStoredMessage(msg, { senderIsAgent, directTargetKind });
  if (!result.ok) throw new Error(result.error || 'message persistence failed');
  return result.msg;
}

const supervisorActionEngine = createSupervisorActionEngine({
  snapshotStore: supervisorSnapshotStore,
  sendMessage: (payload) => dispatchInternalDirectMessage(payload),
  broadcastSSE,
  alertStore,
});

const supervisorLifecycleManager = createSupervisorLifecycleManager({
  getAgents: () => agents,
  getRuntime: (name) => ensureAgentRuntimeRecord(name),
  snapshotStore: supervisorSnapshotStore,
  isAgentRecord,
  broadcastSSE,
});

function normalizeInboxGateReason(value) {
  const raw = (typeof value === 'string') ? value.trim() : '';
  if (raw === 'actionable_notification' || raw === 'merged_actionable_unread') return raw;
  return null;
}

function normalizeInboxGate(value) {
  if (!value || typeof value !== 'object') {
    return {
      requiresInboxCheck: false,
      sourceMsgId: null,
      raisedAt: null,
      reason: null,
    };
  }
  const sourceMsgId = (typeof value.sourceMsgId === 'string' && value.sourceMsgId.trim())
    ? value.sourceMsgId.trim()
    : null;
  const raisedAt = Number(value.raisedAt) || null;
  return {
    requiresInboxCheck: value.requiresInboxCheck === true,
    sourceMsgId,
    raisedAt,
    reason: normalizeInboxGateReason(value.reason),
  };
}

function normalizeInboxReadAck(value) {
  if (!value || typeof value !== 'object') {
    return {
      sourceMsgId: null,
      ackedAt: null,
    };
  }
  return {
    sourceMsgId: (typeof value.sourceMsgId === 'string' && value.sourceMsgId.trim())
      ? value.sourceMsgId.trim()
      : null,
    ackedAt: Number(value.ackedAt) || null,
  };
}

function buildInboxGateFromPushMeta(meta, deliveredAt) {
  if (!meta?.requiresInboxCheck) return normalizeInboxGate(null);
  const reason = meta.kind === 'merged_unread_actionable'
    ? 'merged_actionable_unread'
    : 'actionable_notification';
  return normalizeInboxGate({
    requiresInboxCheck: true,
    sourceMsgId: meta.sourceMsgId || null,
    raisedAt: Number(deliveredAt) || Date.now(),
    reason,
  });
}

function getPendingInboxGate(runtime) {
  const gate = normalizeInboxGate(runtime?.inboxGate);
  return gate.requiresInboxCheck ? gate : null;
}

function formatSenderList(names) {
  if (names.length <= 3) return names.join(', ');
  return `${names.slice(0, 3).join(', ')}, +${names.length - 3} more`;
}

function sanitizeForDisplay(text) {
  if (typeof text !== 'string') return '';
  return text
    .replace(/\x1B\[[0-9;]*[A-Za-z]/g, '')
    .replace(/[\x00-\x08\x0B\x0C\x0E-\x1F\x7F\x80-\x9F]/g, '');
}

function ensureAgentRuntimeRecord(name) {
  const agentName = normalizeAgentName(name);
  if (!agentName) return null;
  if (!agentRuntime[agentName] || typeof agentRuntime[agentName] !== 'object') {
    agentRuntime[agentName] = {
      agent: agentName,
      blocked: false,
      blockedReason: null,
      blockedTier: null,
      blockedSince: null,
      blockedConsecutiveScans: 0,
      blockedNotificationSent: false,
      blockedNotifiedTier: null,
      lastBlockedNotificationTs: 0,
      activeNow: null,
      activeDurationSec: 0,
      idleDurationSec: 0,
      lastTmuxActivitySec: null,
      workspacePath: null,
      mcpPresent: null,
      mcpMissingSince: null,
      mcpHeartbeatAt: null,
      updatedAt: 0,
      lastSeen: 0,
      lastPushNotifyAt: 0,
      lastPushQueuedAt: 0,
      lastPushDeliveredAt: 0,
      lastPushDeliveryDelayMs: 0,
      lastActionablePushAt: 0,
      lastPushQueueEntryId: 0,
      lastPushNeedsInboxCheck: false,
      lastPushUnreadCount: 0,
      lastPushKind: 'unknown',
      lastPushSourceMsgId: null,
      lastInboxCheckAt: 0,
      lastAgentOutboundAt: 0,
      observation: null,
      inboxGate: normalizeInboxGate(null),
      inboxReadAck: normalizeInboxReadAck(null),
      lastBlockedTail: '',
      lastBlockedCommand: '',
      lastBlockedServer: null,
      rules: {},
    };
  }
  return agentRuntime[agentName];
}

function normalizePushMeta(meta = {}) {
  const pushMeta = (meta && typeof meta === 'object') ? meta : {};
  const safeBool = (value) => value === true;
  const safeInt = (value) => {
    const n = Number.parseInt(value, 10);
    return Number.isFinite(n) ? Math.max(0, n) : 0;
  };
  const safeStr = (value, fallback = null) => {
    if (typeof value !== 'string') return fallback;
    const trimmed = value.trim();
    return trimmed || fallback;
  };
  return {
    kind: safeStr(pushMeta.kind, 'unknown'),
    requiresInboxCheck: safeBool(pushMeta.requiresInboxCheck),
    sourceMsgId: safeStr(pushMeta.sourceMsgId, null),
    unreadCount: safeInt(pushMeta.unreadCount),
    hasHumanUnread: safeBool(pushMeta.hasHumanUnread),
    hasRequestUnread: safeBool(pushMeta.hasRequestUnread),
    needsReply: safeBool(pushMeta.needsReply),
    hasMcp: safeBool(pushMeta.hasMcp),
  };
}

function markAgentPushNotified(agentName, details = {}) {
  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) return;
  const now = Date.now();
  const queuedAt = Number(details.queuedAt) || now;
  const queueEntryId = Number(details.queueEntryId) || 0;
  const meta = normalizePushMeta(details);
  runtime.lastPushNotifyAt = now;
  runtime.lastPushQueuedAt = queuedAt;
  runtime.lastPushQueueEntryId = queueEntryId;
  runtime.lastPushKind = meta.kind;
  runtime.lastPushNeedsInboxCheck = meta.requiresInboxCheck;
  runtime.lastPushUnreadCount = meta.unreadCount;
  runtime.lastPushSourceMsgId = meta.sourceMsgId;
  runtime.lastSeen = now;
  runtime.updatedAt = now;
  saveAgentRuntime();
}

function markAgentPushDelivered(agentName, details = {}) {
  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) return { ok: false, ignored: 'agent-not-found' };
  const now = Date.now();
  const deliveredAt = Number(details.deliveredAt) || now;
  const queuedAt = Number(details.queuedAt) || runtime.lastPushQueuedAt || deliveredAt;
  const queueEntryId = Number(details.queueEntryId) || 0;
  const meta = normalizePushMeta(details);
  const currentQueueEntryId = Number(runtime.lastPushQueueEntryId) || 0;
  const currentDeliveredAt = Number(runtime.lastPushDeliveredAt) || 0;
  const currentSourceMsgId = typeof runtime.lastPushSourceMsgId === 'string'
    ? runtime.lastPushSourceMsgId.trim()
    : '';
  const incomingSourceMsgId = typeof meta.sourceMsgId === 'string'
    ? meta.sourceMsgId.trim()
    : '';
  const staleQueueEntry = queueEntryId > 0
    && currentQueueEntryId > 0
    && queueEntryId !== currentQueueEntryId;
  const staleDeliveredAt = currentDeliveredAt > 0 && deliveredAt < currentDeliveredAt;
  const staleSource = incomingSourceMsgId
    && currentSourceMsgId
    && incomingSourceMsgId !== currentSourceMsgId;
  const inboxReadAck = normalizeInboxReadAck(runtime.inboxReadAck);
  const staleReadAck = incomingSourceMsgId
    && inboxReadAck.sourceMsgId === incomingSourceMsgId
    && Number(inboxReadAck.ackedAt) > 0;
  if (staleQueueEntry || staleDeliveredAt || staleSource || staleReadAck) {
    return { ok: true, ignored: 'stale-push-delivered' };
  }
  const delay = Math.max(0, deliveredAt - queuedAt);

  runtime.lastPushDeliveredAt = deliveredAt;
  runtime.lastPushQueuedAt = queuedAt;
  if (queueEntryId > 0) runtime.lastPushQueueEntryId = queueEntryId;
  runtime.lastPushDeliveryDelayMs = delay;
  runtime.lastPushKind = meta.kind;
  runtime.lastPushNeedsInboxCheck = meta.requiresInboxCheck;
  runtime.lastPushUnreadCount = meta.unreadCount;
  runtime.lastPushSourceMsgId = meta.sourceMsgId;
  if (meta.requiresInboxCheck) {
    runtime.inboxGate = buildInboxGateFromPushMeta(meta, deliveredAt);
  }
  if (meta.requiresInboxCheck) {
    runtime.lastActionablePushAt = deliveredAt;
  }
  runtime.lastSeen = deliveredAt;
  runtime.updatedAt = deliveredAt;
  saveAgentRuntime();
  return { ok: true };
}

function markAgentInboxChecked(agentName, details = {}) {
  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) return;
  const now = Date.now();
  runtime.lastInboxCheckAt = now;
  const ackSourceMsgId = (typeof details.sourceMsgId === 'string' && details.sourceMsgId.trim())
    ? details.sourceMsgId.trim()
    : null;
  const clearInboxGate = details.clearInboxGate === true;
  if (clearInboxGate) {
    runtime.inboxGate = normalizeInboxGate(null);
    runtime.inboxReadAck = normalizeInboxReadAck({
      sourceMsgId: ackSourceMsgId,
      ackedAt: now,
    });
  }
  runtime.lastSeen = now;
  runtime.updatedAt = now;
  saveAgentRuntime();
}

function markAgentOutbound(agentName) {
  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) return;
  const now = Date.now();
  runtime.lastAgentOutboundAt = now;
  runtime.lastSeen = now;
  runtime.updatedAt = now;
  saveAgentRuntime();
}

function setRuntimeActivityFields(runtime, payload = {}) {
  let changed = false;
  if (!runtime || typeof runtime !== 'object') return false;

  const rawActiveNow = payload.activeNow;
  const hasActiveNow = Object.prototype.hasOwnProperty.call(payload, 'activeNow')
    && (rawActiveNow === true || rawActiveNow === false || rawActiveNow === null);
  const effectiveActiveNow = normalizeRuntimeActiveNow(rawActiveNow);
  if (hasActiveNow && runtime.activeNow !== effectiveActiveNow) {
    runtime.activeNow = effectiveActiveNow;
    changed = true;
  }
  if (payload.activeDurationSec !== undefined && payload.activeDurationSec !== null) {
    const activeDurationSec = Math.max(0, Number.parseInt(payload.activeDurationSec, 10) || 0);
    if (runtime.activeDurationSec !== activeDurationSec) {
      runtime.activeDurationSec = activeDurationSec;
      changed = true;
    }
  }
  if (payload.idleDurationSec !== undefined && payload.idleDurationSec !== null) {
    const idleDurationSec = Math.max(0, Number.parseInt(payload.idleDurationSec, 10) || 0);
    if (runtime.idleDurationSec !== idleDurationSec) {
      runtime.idleDurationSec = idleDurationSec;
      changed = true;
    }
  }
  // NAME IS HISTORICAL. This means "when the agent last did something", for any
  // transport. An ACP agent has no tmux and still reports it, derived from
  // session/update counts rather than a pane hash.
  //
  // Not renamed because it is a relay wire field (lib/push-relay-core.js and its
  // remote/ twin) and is persisted in agent_runtime.json, so a rename needs a
  // dual-read compatibility window and a migration — disproportionate to the
  // confusion it causes. If it is renamed, do it as its own change.
  if (payload.lastTmuxActivitySec !== undefined) {
    const v = Number.parseInt(payload.lastTmuxActivitySec, 10);
    const normalized = Number.isFinite(v) && v > 0 ? v : null;
    if ((runtime.lastTmuxActivitySec || null) !== normalized) {
      runtime.lastTmuxActivitySec = normalized;
      changed = true;
    }
  }
  return changed;
}

function setRuntimeMcpFields(runtime, payload = {}, now = Date.now()) {
  if (!runtime || typeof runtime !== 'object') return false;
  if (!Object.prototype.hasOwnProperty.call(payload, 'mcpPresent')) return false;
  if (payload.mcpPresent === undefined) return false;

  let changed = false;
  const mcpNow = payload.mcpPresent === true
    ? true
    : (payload.mcpPresent === false ? false : null);
  const prevMcp = runtime.mcpPresent === true
    ? true
    : (runtime.mcpPresent === false ? false : null);
  const recentHeartbeatAt = Number(runtime.mcpHeartbeatAt) || 0;
  const hasRecentMcpHeartbeat = recentHeartbeatAt > 0
    && now - recentHeartbeatAt >= 0
    && now - recentHeartbeatAt <= MCP_HEARTBEAT_AUTHORITY_WINDOW_MS;

  if (mcpNow === false && hasRecentMcpHeartbeat) {
    return false;
  }

  if (prevMcp !== mcpNow) {
    runtime.mcpPresent = mcpNow;
    changed = true;
  }

  if (mcpNow === false) {
    const nextMissingSince = prevMcp === false
      ? (Number(runtime.mcpMissingSince) || now)
      : now;
    if (runtime.mcpMissingSince !== nextMissingSince) {
      runtime.mcpMissingSince = nextMissingSince;
      changed = true;
    }
  } else if (runtime.mcpMissingSince !== null) {
    runtime.mcpMissingSince = null;
    changed = true;
  }

  return changed;
}

function setRuntimeWorkspacePath(runtime, payload = {}) {
  if (!runtime || typeof runtime !== 'object') return false;
  if (!Object.prototype.hasOwnProperty.call(payload, 'workspacePath')) return false;
  const normalized = normalizeWorkspacePath(payload.workspacePath);
  /*
   * TWO FIELDS, TWO QUESTIONS.
   *
   * `workspacePath` answers "where is this agent running now", and the activity sweep
   * correctly clears it when an agent has no pane — a stopped agent is running nowhere.
   *
   * `lastWorkspacePath` answers "where did it last run", and is never cleared. Metering
   * needs that one: consumption is read from transcripts that stay on disk after the
   * agent stops, and clearing the only key to them made a stopped agent's usage
   * unattributable — a monthly ceiling still spent by work that has finished. Reporting
   * an agent as having consumed nothing because it is no longer running would be the
   * wrong answer to a question nobody asked.
   */
  let changed = false;
  if (normalized && runtime.lastWorkspacePath !== normalized) {
    runtime.lastWorkspacePath = normalized;
    changed = true;
  }
  if ((runtime.workspacePath || null) !== (normalized || null)) {
    runtime.workspacePath = normalized;
    changed = true;
  }
  return changed;
}

function syncLocalAgentOnlineState(agent, runtime, tmuxTarget, manualDown) {
  if (!isAgentRecord(agent) || !runtime) return false;
  let changed = false;
  const mcpMissing = runtime.mcpPresent === false;
  if ((!agent.tmux || !String(agent.tmux).trim()) && !manualDown) {
    agent.tmux = tmuxTarget;
    changed = true;
  }
  // Drive online/manualDown through machine (transitionAgent syncs agent.online/manualDown)
  const prevOnline = agent.online;
  const prevManualDown = agent.manualDown;
  syncAgentMachine(agent.name, {
    manualDown,
    tmuxPresent: true,
    mcpPresent: runtime.mcpPresent === true ? true : (runtime.mcpPresent === false ? false : undefined),
  });
  if (agent.online !== prevOnline || agent.manualDown !== prevManualDown) changed = true;
  // offlineReason is not machine-managed
  if (manualDown) {
    // offlineReason stays as-is for manual down
  } else if (mcpMissing && (agent.state === 'degraded' || agent.state === 'offline')) {
    if (agent.offlineReason !== 'mcp-missing:auto') {
      agent.offlineReason = 'mcp-missing:auto';
      changed = true;
    }
  } else if (!manualDown && !mcpMissing) {
    if (agent.offlineReason !== null) {
      agent.offlineReason = null;
      changed = true;
    }
  }
  if (changed) {
    agent.lastSeen = Date.now();
  }
  return changed;
}

function applyLocalMetadataOnlySignals(agentName, payload = {}) {
  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) return;
  applyLocalRuntimeSignals(agentName, {
    blocked: runtime.blocked === true,
    reason: runtime.blocked === true ? (runtime.blockedReason || null) : null,
    tail: runtime.blocked === true ? (runtime.lastBlockedTail || '') : '',
    command: runtime.blocked === true ? (runtime.lastBlockedCommand || '') : '',
    workspacePath: payload.workspacePath,
    mcpPresent: payload.mcpPresent,
    blockedObserved: false,
  });
}

function resolveLocalActivityCaptureSelection(agentNames = [], budget = 0) {
  const ordered = [...new Set((Array.isArray(agentNames) ? agentNames : []).filter(Boolean))].sort((a, b) => a.localeCompare(b));
  const count = ordered.length;
  const currentCursor = Math.max(0, Number(localActivitySweepState.selectionCursor) || 0);
  if (count === 0) {
    if (currentCursor !== 0) {
      localActivitySweepState.selectionCursor = 0;
      saveLocalActivitySweepState();
    }
    return { selected: new Set(), ordered, currentCursor: 0, nextCursor: 0 };
  }
  if (!Number.isFinite(budget) || budget <= 0 || budget >= count) {
    const nextCursor = 0;
    if (currentCursor !== nextCursor) {
      localActivitySweepState.selectionCursor = nextCursor;
      saveLocalActivitySweepState();
    }
    return { selected: new Set(ordered), ordered, currentCursor, nextCursor };
  }

  const normalizedCursor = currentCursor % count;
  const selected = [];
  for (let i = 0; i < budget; i++) {
    selected.push(ordered[(normalizedCursor + i) % count]);
  }
  const nextCursor = (normalizedCursor + budget) % count;
  if (currentCursor !== nextCursor) {
    localActivitySweepState.selectionCursor = nextCursor;
    saveLocalActivitySweepState();
  }
  return { selected: new Set(selected), ordered, currentCursor: normalizedCursor, nextCursor };
}

function isHumanMessageToAgent(msg, agentName) {
  if (!msg || msg.type !== 'human') return false;
  if (msg.to === agentName) return true;
  if (msg.group && Array.isArray(msg.mentions) && msg.mentions.includes(agentName)) return true;
  return false;
}

function didAgentAcknowledgeActionablePush(agentName, runtime, actionablePushAt, latestActionableMsg) {
  if (!runtime || (Number(runtime.lastAgentOutboundAt) || 0) < actionablePushAt) return false;
  if (!latestActionableMsg) return true;

  const sourceId = typeof latestActionableMsg.id === 'string' ? latestActionableMsg.id : null;
  const sourceFrom = typeof latestActionableMsg.from === 'string' ? latestActionableMsg.from : null;
  const sourceGroup = typeof latestActionableMsg.group === 'string' ? latestActionableMsg.group : null;

  let scanned = 0;
  const maxScan = 400;
  for (let i = messages.length - 1; i >= 0 && scanned < maxScan; i--) {
    const msg = messages[i];
    if (!msg || msg.from !== agentName) continue;
    scanned++;

    const ts = Number(msg.ts) || 0;
    if (ts < actionablePushAt) break;
    if (sourceId && msg.reply_to === sourceId) return true;
    if (sourceGroup && msg.group === sourceGroup) return true;
    if (!sourceGroup && sourceFrom && msg.to === sourceFrom) return true;
  }
  return false;
}

function getAgentInboxGateBlock(agentName) {
  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) return null;
  const gate = getPendingInboxGate(runtime);
  if (!gate) return null;
  return {
    agent: agentName,
    inboxGate: gate,
    error: 'inbox_check_required',
    hint: 'Call check_inbox() first to acknowledge the pending actionable notification before sending outbound progress or replies.',
  };
}

function collectBlockedHumanTargets(agentName) {
  const cached = pendingHumanTargetCache.get(agentName);
  if (cached) return cached;

  const unreadHuman = getUnreadInboxMessages(agentName).unread
    .filter(m => m.type === 'human' && m.from && m.from !== agentName);
  const selected = new Map();

  for (const msg of unreadHuman) {
    const prev = selected.get(msg.from);
    if (!prev || compareMsgOrder(msg, prev) > 0) {
      selected.set(msg.from, msg);
    }
  }

  const snapshot = {
    hasPendingHuman: unreadHuman.length > 0,
    targets: [...selected.values()]
      .sort(compareMsgOrder)
      .map(msg => ({
        human: msg.from,
        humanId: msg.fromId || msg.senderMxid || null,
        roomId: (typeof msg.sourceRoom === 'string' && msg.sourceRoom.trim()) ? msg.sourceRoom.trim() : null,
        group: msg.group || null,
        messageId: msg.id,
        pending: true,
        ts: msg.ts,
      })),
  };
  pendingHumanTargetCache.set(agentName, snapshot);
  return snapshot;
}

function applyRuntimeObservation(agentName, payload = {}) {
  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) return null;

  const now = Date.now();
  const blockedNow = payload.blocked === true;
  const blockedObserved = blockedNow && payload.blockedObserved !== false;
  const reasonNow = blockedNow && typeof payload.reason === 'string' && payload.reason.trim()
    ? payload.reason.trim()
    : null;
  const tierNow = blockedNow ? blockedTierFromReason(reasonNow) : null;
  const tailNow = blockedNow && typeof payload.tail === 'string' ? payload.tail : '';
  const cmdNow = blockedNow && typeof payload.command === 'string' ? payload.command : '';
  const serverNow = blockedNow ? normalizeServer(payload.server) : null;

  const prevBlocked = runtime.blocked === true;
  const prevReason = runtime.blockedReason || null;
  const prevBlockedTier = normalizeBlockedTier(runtime.blockedTier, prevBlocked ? blockedTierFromReason(prevReason) : null);
  const prevBlockedConsecutiveScans = Math.max(0, Number(runtime.blockedConsecutiveScans) || 0);
  const prevBlockedNotificationSent = runtime.blockedNotificationSent === true;
  const prevBlockedNotifiedTier = normalizeBlockedTier(
    runtime.blockedNotifiedTier,
    prevBlockedNotificationSent ? prevBlockedTier : null,
  );
  const prevMcpPresent = runtime.mcpPresent === true
    ? true
    : (runtime.mcpPresent === false ? false : null);
  let changed = false;

  if (runtime.blocked !== blockedNow) { runtime.blocked = blockedNow; changed = true; }
  if ((runtime.blockedReason || null) !== reasonNow) { runtime.blockedReason = reasonNow; changed = true; }
  if (normalizeBlockedTier(runtime.blockedTier, null) !== tierNow) { runtime.blockedTier = tierNow; changed = true; }
  if (runtime.lastSeen !== now) { runtime.lastSeen = now; changed = true; }
  if (runtime.updatedAt !== now) { runtime.updatedAt = now; changed = true; }
  if (blockedNow) {
    const sameBlockedSignature = prevBlocked && prevReason === reasonNow && prevBlockedTier === tierNow;
    const blockedConsecutiveScans = blockedObserved
      ? (sameBlockedSignature ? (prevBlockedConsecutiveScans + 1) : 1)
      : (sameBlockedSignature ? prevBlockedConsecutiveScans : 0);
    if (runtime.blockedConsecutiveScans !== blockedConsecutiveScans) {
      runtime.blockedConsecutiveScans = blockedConsecutiveScans;
      changed = true;
    }
    const blockedSince = prevBlocked ? (runtime.blockedSince || now) : now;
    if (runtime.blockedSince !== blockedSince) { runtime.blockedSince = blockedSince; changed = true; }
    if (runtime.lastBlockedTail !== tailNow) { runtime.lastBlockedTail = tailNow; changed = true; }
    if (runtime.lastBlockedCommand !== cmdNow) { runtime.lastBlockedCommand = cmdNow; changed = true; }
    if ((runtime.lastBlockedServer || null) !== (serverNow || null)) { runtime.lastBlockedServer = serverNow; changed = true; }
  } else {
    if (runtime.blockedTier !== null) { runtime.blockedTier = null; changed = true; }
    if (runtime.blockedSince !== null) { runtime.blockedSince = null; changed = true; }
    if (runtime.blockedConsecutiveScans !== 0) { runtime.blockedConsecutiveScans = 0; changed = true; }
    if (runtime.blockedNotificationSent !== false) { runtime.blockedNotificationSent = false; changed = true; }
    if (runtime.blockedNotifiedTier !== null) { runtime.blockedNotifiedTier = null; changed = true; }
    if (runtime.lastBlockedTail !== '') { runtime.lastBlockedTail = ''; changed = true; }
    if (runtime.lastBlockedCommand !== '') { runtime.lastBlockedCommand = ''; changed = true; }
    if (runtime.lastBlockedServer !== null) { runtime.lastBlockedServer = null; changed = true; }
  }

  if (setRuntimeActivityFields(runtime, payload)) changed = true;
  if (setRuntimeWorkspacePath(runtime, payload)) changed = true;
  if (Object.prototype.hasOwnProperty.call(payload, 'observerSource')) {
    const observerServer = Object.prototype.hasOwnProperty.call(payload, 'observerServer')
      ? payload.observerServer
      : payload.server;
    if (setRuntimeObservation(runtime, {
      observerSource: payload.observerSource,
      observerServer,
      observedAt: now,
    })) changed = true;
  }
  const agentForMcp = agents[agentName];
  if (agentForMcp && !agentExpectsMcp(agentForMcp) && payload.mcpPresent !== undefined) payload = { ...payload, mcpPresent: null };
  if (setRuntimeMcpFields(runtime, payload, now)) changed = true;
  if (changed) saveAgentRuntime();

  const mcpNow = runtime.mcpPresent === true
    ? true
    : (runtime.mcpPresent === false ? false : null);
  const mcpBecameMissing = prevMcpPresent !== false && mcpNow === false;
  const mcpRecovered = prevMcpPresent === false && mcpNow === true;

  const agent = agents[agentName];
  let agentChanged = false;
  let shouldCatchup = false;
  if (isAgentRecord(agent) && agent.kind !== 'human') {
    const wasOnline = agent.online === true;
    const wasManualDown = agent.manualDown === true;

    if (mcpNow === false && !wasManualDown) {
      // Paneless agents excluded: this exists to give a tmux agent a target it never
      // registered, and fabricating one for an ACP agent routes it down the pane path.
      if (!agent.tmux && agentTransport(agent) !== 'acp') { agent.tmux = `${agentName}:0.0`; agentChanged = true; }
      // online/manualDown driven by machine below
      if (agent.offlineReason !== 'mcp-missing:auto') { agent.offlineReason = 'mcp-missing:auto'; agentChanged = true; }
      if (agent.lastSeen !== now) { agent.lastSeen = now; agentChanged = true; }
    } else if (mcpNow === true) {
      const recoverable = agent.offlineReason === 'mcp-missing:auto' || !wasOnline;
      // Paneless agents excluded: this exists to give a tmux agent a target it never
      // registered, and fabricating one for an ACP agent routes it down the pane path.
      if (!agent.tmux && agentTransport(agent) !== 'acp') { agent.tmux = `${agentName}:0.0`; agentChanged = true; }
      // online/manualDown driven by machine below
      if (agent.offlineReason === 'mcp-missing:auto') { agent.offlineReason = null; agentChanged = true; }
      if (agent.lastSeen !== now) { agent.lastSeen = now; agentChanged = true; }
      if (!wasOnline && !wasManualDown && recoverable) shouldCatchup = true;
    }
    // Drive online/manualDown through machine
    const prevOnline = agent.online;
    syncAgentMachine(agent.name, {
      mcpPresent: mcpNow === true ? true : (mcpNow === false ? false : undefined),
    });
    if (agent.online !== prevOnline) agentChanged = true;
  }
  if (shouldCatchup) notifyAgentCatchup(agentName, 'mcp-restored');

  if (mcpBecameMissing) {
    const full = [
      `Agent: ${agentName}`,
      `Server: ${normalizeServer(payload.server) || normalizeServer(agent?.server) || 'local'}`,
      'State: tmux session present but mcp-server.js process not detected.',
      'Offline reason set to: mcp-missing:auto',
    ].join('\n');
    emitSystemInfo(`Agent '${agentName}' missing MCP process`, full, 'mcp_missing', { sourceAgent: agentName, dedupeKey: `mcp_missing:${agentName}` });
  } else if (mcpRecovered) {
    emitSystemInfo(`Agent '${agentName}' MCP process recovered`, `Agent '${agentName}' now has mcp-server.js running inside tmux.`, 'mcp_recovered', { sourceAgent: agentName });
  }

  return {
    agentName,
    runtime,
    payload,
    now,
    blockedNow,
    blockedObserved,
    reasonNow,
    tierNow,
    tailNow,
    serverNow,
    prevBlockedNotificationSent,
    prevBlockedNotifiedTier,
  };
}

function dispatchBlockedNotifications(transition) {
  if (!transition || !transition.runtime) return transition?.runtime || null;

  const {
    agentName,
    runtime,
    now,
    blockedNow,
    blockedObserved,
    reasonNow,
    tierNow,
    tailNow,
    serverNow,
    prevBlockedNotificationSent,
    prevBlockedNotifiedTier,
  } = transition;

  const blockedDebounceThreshold = blockedTierDebounceThreshold(tierNow);
  const blockedNotificationReady = blockedNow
    && blockedObserved
    && Number.isFinite(blockedDebounceThreshold)
    && runtime.blockedConsecutiveScans >= blockedDebounceThreshold;
  const becameBlocked = blockedNotificationReady && !prevBlockedNotificationSent;
  const severityIncreased = prevBlockedNotificationSent
    && blockedNotificationReady
    && normalizeBlockedTier(tierNow, null) !== null
    && normalizeBlockedTier(prevBlockedNotifiedTier, null) !== null
    && tierNow > prevBlockedNotifiedTier;
  const recovered = prevBlockedNotificationSent && !blockedNow;

  if (becameBlocked || severityIncreased) {
    const { hasPendingHuman, targets } = collectBlockedHumanTargets(agentName);
    const fullLines = [
      `Agent: ${agentName}`,
      `Reason: ${reasonNow || 'unknown'}`,
      `Tier: ${tierNow === BLOCK_TIER_TRANSIENT ? 'transient' : (tierNow === BLOCK_TIER_SOFT ? 'soft' : 'hard')}`,
      `Server: ${serverNow || 'local'}`,
      `Pending human messages: ${hasPendingHuman ? 'yes' : 'no'}`,
      `Target humans: ${targets.map(t => t.human).join(', ') || 'none'}`,
    ];
    if (tailNow) { fullLines.push('', 'Tail sample:', tailNow); }

    const result = notificationRouter.emit('agent_blocked', {
      agentName, tier: tierNow, full: fullLines.join('\n'),
    }, { bypassCooldown: severityIncreased });
    if (result.accepted) {
      let runtimeChanged = false;
      if (runtime.blockedNotificationSent !== true) {
        runtime.blockedNotificationSent = true;
        runtimeChanged = true;
      }
      if (normalizeBlockedTier(runtime.blockedNotifiedTier, null) !== tierNow) {
        runtime.blockedNotifiedTier = tierNow;
        runtimeChanged = true;
      }
      if (runtimeChanged) saveAgentRuntime();
      broadcastSSE('agent_blocked', {
        agent: agentName,
        reason: reasonNow || 'unknown',
        tier: tierNow,
        blockedSince: runtime.blockedSince || now,
        server: serverNow || null,
        hasPendingHuman,
        targets,
      });
    }
  } else if (recovered) {
    notificationRouter.emit('agent_blocked', {
      agentName, recovered: true,
    }, { bypassCooldown: true, skipPersistedWrite: true });
    broadcastSSE('agent_recovered', { agent: agentName, recoveredAt: now });
  }

  return runtime;
}

function setAgentRuleState(agentName, code, active, buildDetail) {
  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) return;
  if (!runtime.rules || typeof runtime.rules !== 'object') runtime.rules = {};
  const now = Date.now();
  const prev = runtime.rules[code] || { active: false, changedAt: 0, firedAt: 0 };
  if (prev.active === active) return;

  runtime.rules[code] = {
    active,
    changedAt: now,
    firedAt: active ? now : prev.firedAt || 0,
  };
  runtime.updatedAt = now;
  saveAgentRuntime();

  if (active) {
    const detail = typeof buildDetail === 'function' ? buildDetail() : '';
    emitSystemInfo(
      `Agent '${agentName}' rule alert: ${code}`,
      detail || `Rule ${code} triggered for agent '${agentName}'.`,
      'agent_rule',
      { sourceAgent: agentName, dedupeKey: `agent_rule:${agentName}:${code}` }
    );
  } else {
    alertStore.autoResolve(`agent_rule:${agentName}:${code}`);
  }
}

function sweepAgentRules() {
  const now = Date.now();
  for (const [agentName, agent] of Object.entries(agents)) {
    if (!isAgentRecord(agent)) continue;
    const state = getAgentDeliveryState(agentName);
    const runtime = ensureAgentRuntimeRecord(agentName);
    if (!runtime) continue;

    if (!state.online || runtime.blocked === true) {
      setAgentRuleState(agentName, 'no_inbox_check_after_push', false);
      setAgentRuleState(agentName, 'inbox_checked_no_reply', false);
      continue;
    }

    const unread = getUnreadInboxMessages(agentName).unread;
    const unreadHuman = unread.filter(m => m.type === 'human');
    const unreadActionable = unread.filter(m => m.type === 'human' || m.type === 'request');
    const actionablePushAt = Number(runtime.lastActionablePushAt) || 0;
    const latestActionableUnread = unreadActionable[unreadActionable.length - 1] || null;
    const activeKnown = runtime.activeNow === true || runtime.activeNow === false;
    const activeNow = runtime.activeNow === true;
    const idleDurationSec = Math.max(0, Number(runtime.idleDurationSec) || 0);
    const idleGateReady = activeKnown && !activeNow && idleDurationSec >= 1;
    const outboundAcked = didAgentAcknowledgeActionablePush(
      agentName,
      runtime,
      actionablePushAt,
      latestActionableUnread
    );
    const needsInboxCheck = actionablePushAt > 0
      && runtime.lastInboxCheckAt < actionablePushAt
      && !outboundAcked
      && idleGateReady
      && (now - actionablePushAt) >= RULE_PUSH_ACK_TIMEOUT_MS;
    setAgentRuleState(agentName, 'no_inbox_check_after_push', needsInboxCheck, () => {
      return [
        `Agent: ${agentName}`,
        `lastPushNotifyAt: ${runtime.lastPushNotifyAt ? new Date(runtime.lastPushNotifyAt).toISOString() : 'n/a'}`,
        `lastPushQueuedAt: ${runtime.lastPushQueuedAt ? new Date(runtime.lastPushQueuedAt).toISOString() : 'n/a'}`,
        `lastPushDeliveredAt: ${runtime.lastPushDeliveredAt ? new Date(runtime.lastPushDeliveredAt).toISOString() : 'n/a'}`,
        `lastActionablePushAt: ${actionablePushAt ? new Date(actionablePushAt).toISOString() : 'n/a'}`,
        `lastPushKind: ${runtime.lastPushKind || 'unknown'}`,
        `lastPushNeedsInboxCheck: ${runtime.lastPushNeedsInboxCheck === true ? 'yes' : 'no'}`,
        `lastPushDeliveryDelayMs: ${Number(runtime.lastPushDeliveryDelayMs) || 0}`,
        `lastInboxCheckAt: ${runtime.lastInboxCheckAt ? new Date(runtime.lastInboxCheckAt).toISOString() : 'n/a'}`,
        `lastAgentOutboundAt: ${runtime.lastAgentOutboundAt ? new Date(runtime.lastAgentOutboundAt).toISOString() : 'n/a'}`,
        `activeNow: ${activeKnown ? (activeNow ? 'yes' : 'no') : 'unknown'}`,
        `idleDurationSec: ${idleDurationSec}`,
        `idleGateReady: ${idleGateReady ? 'yes' : 'no'} (requires activeNow=no and idleDurationSec>=1)`,
        `idleThresholdMs: ${IDLE_THRESHOLD_MS}`,
        `outboundAcked: ${outboundAcked ? 'yes' : 'no'}`,
        `latestActionableUnreadId: ${latestActionableUnread?.id || 'n/a'}`,
        `timeoutMs: ${RULE_PUSH_ACK_TIMEOUT_MS}`,
      ].join('\n');
    });

    const checkedButNoReply = actionablePushAt > 0
      && runtime.lastInboxCheckAt >= actionablePushAt
      && runtime.lastAgentOutboundAt < runtime.lastInboxCheckAt
      && unreadHuman.filter(m => (Number(m.ts) || 0) <= runtime.lastInboxCheckAt).length > 0
      && (now - runtime.lastInboxCheckAt) >= RULE_REPLY_TIMEOUT_MS;
    setAgentRuleState(agentName, 'inbox_checked_no_reply', checkedButNoReply, () => {
      const unreadHumanBeforeCheck = unreadHuman.filter(m => (Number(m.ts) || 0) <= runtime.lastInboxCheckAt);
      const unreadHumanAfterCheck = unreadHuman.filter(m => (Number(m.ts) || 0) > runtime.lastInboxCheckAt);
      const humans = [...new Set(unreadHuman.map(m => m.from).filter(Boolean))];
      return [
        `Agent: ${agentName}`,
        `lastInboxCheckAt: ${new Date(runtime.lastInboxCheckAt).toISOString()}`,
        `lastAgentOutboundAt: ${runtime.lastAgentOutboundAt ? new Date(runtime.lastAgentOutboundAt).toISOString() : 'n/a'}`,
        `unreadHuman: ${unreadHuman.length}`,
        `unreadHumanBeforeCheck: ${unreadHumanBeforeCheck.length}`,
        `unreadHumanAfterCheck: ${unreadHumanAfterCheck.length}`,
        `senders: ${humans.join(', ') || 'none'}`,
        `timeoutMs: ${RULE_REPLY_TIMEOUT_MS}`,
      ].join('\n');
    });
  }
}

function ensureServerRecord(serverId) {
  if (!serverId) return null;
  if (!servers[serverId] || typeof servers[serverId] !== 'object') {
    servers[serverId] = {
      id: serverId,
      lastSeen: 0,
      heartbeatAt: 0,
      relayInstanceId: null,
      relayBootTs: 0,
      online: false,
      updatedAt: Date.now(),
      sessions: [],
      agents: [],
      agentCount: 0,
      sourceIp: null,
      version: null,
      maintenance: SERVER_MAINTENANCE_IDS.has(serverId),
    };
  }
  return servers[serverId];
}

function stringArrayEquals(left, right) {
  if (!Array.isArray(left) || !Array.isArray(right)) return false;
  if (left.length !== right.length) return false;
  for (let i = 0; i < left.length; i += 1) {
    if (left[i] !== right[i]) return false;
  }
  return true;
}

function recordLocalServerObservation(now = Date.now()) {
  if (!RECORD_LOCAL_SERVER) return false;
  const serverId = normalizeServer(LOCAL_SERVER_ID) || 'local';
  const server = ensureServerRecord(serverId);
  if (!server) return false;

  const localAgentRows = Object.values(agents)
    .filter(agent => isAgentRecord(agent))
    .filter(agent => isLocalAgentServer(normalizeServer(agent.server), LOCAL_SERVER_ID))
    .filter(agent => agent.manualDown !== true)
    .filter(agent => typeof agent.tmux === 'string' && agent.tmux.trim())
    .sort((a, b) => String(a.name || '').localeCompare(String(b.name || '')));
  const localAgentNames = localAgentRows.map(agent => agent.name);
  const sessions = [...new Set(localAgentRows.map(agent => agent.tmux.trim()).filter(Boolean))].sort((a, b) => a.localeCompare(b));

  let changed = false;
  if (server.lastSeen !== now) { server.lastSeen = now; changed = true; }
  if (server.heartbeatAt !== now) { server.heartbeatAt = now; changed = true; }
  if (server.online !== true) { server.online = true; changed = true; }
  if (server.updatedAt !== now) { server.updatedAt = now; changed = true; }
  if (!stringArrayEquals(server.sessions, sessions)) { server.sessions = sessions; changed = true; }
  if (!stringArrayEquals(server.agents, localAgentNames)) { server.agents = localAgentNames; changed = true; }
  if ((Number(server.agentCount) || 0) !== localAgentNames.length) { server.agentCount = localAgentNames.length; changed = true; }
  if (server.sourceIp !== 'local') { server.sourceIp = 'local'; changed = true; }
  if (server.relayInstanceId !== null) { server.relayInstanceId = null; changed = true; }
  if ((Number(server.relayBootTs) || 0) !== 0) { server.relayBootTs = 0; changed = true; }
  if (LOCAL_GIT_VERSION && server.version !== LOCAL_GIT_VERSION) { server.version = LOCAL_GIT_VERSION; changed = true; }
  if (!Object.prototype.hasOwnProperty.call(server, 'maintenance')) {
    server.maintenance = SERVER_MAINTENANCE_IDS.has(serverId);
    changed = true;
  }
  return changed;
}

function isServerInMaintenance(serverId, serverRecord = null) {
  const id = normalizeServer(serverId);
  if (!id) return false;
  const server = (serverRecord && typeof serverRecord === 'object') ? serverRecord : servers[id];
  if (server && typeof server.maintenance === 'boolean') return server.maintenance === true;
  return SERVER_MAINTENANCE_IDS.has(id);
}

function collectServerAffectedAgents(serverId, serverRecord = null) {
  const affected = new Set();
  if (serverRecord && Array.isArray(serverRecord.agents)) {
    for (const name of serverRecord.agents) {
      if (typeof name === 'string' && name.trim()) affected.add(name.trim());
    }
  }
  for (const agent of Object.values(agents)) {
    if (!isAgentRecord(agent)) continue;
    if (normalizeServer(agent.server) !== serverId) continue;
    if (agent.online === true && agent.manualDown !== true) affected.add(agent.name);
  }
  return [...affected].sort((a, b) => a.localeCompare(b));
}

function buildServerOfflineAlertDetail(serverId, reason, serverRecord = null, affectedAgents = []) {
  const now = Date.now();
  const heartbeatAt = Number(serverRecord?.heartbeatAt) || 0;
  const lastSeen = Number(serverRecord?.lastSeen) || 0;
  const heartbeatAgeMs = heartbeatAt > 0 ? Math.max(0, now - heartbeatAt) : null;
  return [
    `Server: ${serverId}`,
    `Reason: ${reason || 'server-offline'}`,
    `Last heartbeat: ${heartbeatAt ? new Date(heartbeatAt).toISOString() : 'unknown'}`,
    `Last seen: ${lastSeen ? new Date(lastSeen).toISOString() : 'unknown'}`,
    heartbeatAgeMs !== null ? `Heartbeat age ms: ${heartbeatAgeMs}` : null,
    `Affected agents: ${affectedAgents.length ? affectedAgents.join(', ') : 'none'}`,
    'Impact: remote agents on this server are marked offline and direct push delivery is unavailable until the relay recovers.',
    'Runbook: verify the remote host, relay service, network path, and deployed version; restart the relay or put the server into maintenance if the outage is expected.',
    'Recovery condition: the next accepted heartbeat from this server auto-resolves this alert.',
  ].filter(Boolean).join('\n');
}

function emitServerOfflineAlert(serverId, reason, serverRecord = null, affectedAgents = null) {
  const normalizedServerId = normalizeServer(serverId);
  if (!normalizedServerId) return null;
  if (isLocalAgentServer(normalizedServerId, LOCAL_SERVER_ID)) return null;
  if (isServerInMaintenance(normalizedServerId, serverRecord)) return null;
  const affected = Array.isArray(affectedAgents)
    ? [...new Set(affectedAgents.filter((name) => typeof name === 'string' && name.trim()).map((name) => name.trim()))].sort((a, b) => a.localeCompare(b))
    : collectServerAffectedAgents(normalizedServerId, serverRecord);
  try {
    const result = alertStore.ingest({
      alertType: 'server_offline',
      dedupeKey: `server_offline:${normalizedServerId}`,
      severity: 'critical',
      source: 'backend',
      sourceAgent: normalizedServerId,
      summary: `Remote server '${normalizedServerId}' is offline`,
      detail: buildServerOfflineAlertDetail(normalizedServerId, reason, serverRecord, affected),
      owner: 'remote-runtime',
      runbook: 'docs/runbooks/remote-server-offline.md',
      impact: 'remote agents on this server are marked offline and direct push delivery is unavailable until the relay recovers',
      recoveryCondition: 'the next accepted heartbeat from this server auto-resolves this alert',
      correlation: {
        dedupeKey: `server_offline:${normalizedServerId}`,
        serverId: normalizedServerId,
        reason: reason || 'server-offline',
        affectedAgents: affected,
      },
      tags: [
        'server-outage',
        `server:${normalizedServerId}`,
        ...affected.slice(0, 18).map((name) => `agent:${name}`),
      ],
    });
    return result.alert;
  } catch (error) {
    console.warn(`[server-alert] failed to ingest server_offline:${normalizedServerId}: ${error?.message || error}`);
    return null;
  }
}

function resolveServerOfflineAlert(serverId) {
  const normalizedServerId = normalizeServer(serverId);
  if (!normalizedServerId) return null;
  try {
    return alertStore.autoResolve(`server_offline:${normalizedServerId}`);
  } catch (error) {
    console.warn(`[server-alert] failed to resolve server_offline:${normalizedServerId}: ${error?.message || error}`);
    return null;
  }
}

function markAgentsOfflineForServer(serverId, reason, clearTmux = false) {
  let changed = false;
  for (const agent of Object.values(agents)) {
    if (normalizeServer(agent.server) !== serverId) continue;
    const prevOnline = agent.online;
    const prevManualDown = agent.manualDown;
    if (prevManualDown) {
      if (clearTmux && agent.tmux !== null) { agent.tmux = null; changed = true; }
      syncAgentMachine(agent.name, { serverOffline: true });
      if (agent.online !== prevOnline) changed = true;
    } else {
      if (agent.offlineReason !== reason) { agent.offlineReason = reason; changed = true; }
      if (clearTmux && agent.tmux !== null) { agent.tmux = null; changed = true; }
      syncAgentMachine(agent.name, { serverOffline: true });
      if (agent.online !== prevOnline || agent.manualDown !== prevManualDown) changed = true;
    }
    // Reset activity fields so remote agents don't retain stale activeNow: true
    const runtime = ensureAgentRuntimeRecord(agent.name);
    if (runtime) {
      const actReset = setRuntimeActivityFields(runtime, {
        activeNow: false,
        activeDurationSec: 0,
        idleDurationSec: 0,
        lastTmuxActivitySec: null,
      });
      if (actReset) { runtime.updatedAt = Date.now(); changed = true; }
    }
  }
  return changed;
}

function clearServerLiveState(server, now = Date.now()) {
  if (!server || typeof server !== 'object') return false;
  let changed = false;
  if (server.online !== false) { server.online = false; changed = true; }
  if (!Array.isArray(server.sessions) || server.sessions.length !== 0) { server.sessions = []; changed = true; }
  if (!Array.isArray(server.agents) || server.agents.length !== 0) { server.agents = []; changed = true; }
  if ((Number(server.agentCount) || 0) !== 0) { server.agentCount = 0; changed = true; }
  if (server.relayInstanceId !== null) { server.relayInstanceId = null; changed = true; }
  if ((Number(server.relayBootTs) || 0) !== 0) { server.relayBootTs = 0; changed = true; }
  if ((Number(server.updatedAt) || 0) !== now) { server.updatedAt = now; changed = true; }
  return changed;
}

function enforceServerMaintenanceOffline(serverId, server, now = Date.now()) {
  if (!server || typeof server !== 'object') return { serverChanged: false, agentsChanged: false };
  let serverChanged = false;
  const shouldTouchUpdatedAt = server.online !== false
    || !Array.isArray(server.sessions) || server.sessions.length !== 0
    || !Array.isArray(server.agents) || server.agents.length !== 0
    || (Number(server.agentCount) || 0) !== 0
    || server.relayInstanceId !== null
    || (Number(server.relayBootTs) || 0) !== 0
    || (Number(server.heartbeatAt) || 0) !== 0;
  const targetUpdatedAt = shouldTouchUpdatedAt ? now : (Number(server.updatedAt) || now);
  if ((Number(server.heartbeatAt) || 0) !== 0) { server.heartbeatAt = 0; serverChanged = true; }
  if (clearServerLiveState(server, targetUpdatedAt)) serverChanged = true;
  const agentsChanged = markAgentsOfflineForServer(serverId, `server-maintenance:${serverId}`, true);
  return { serverChanged, agentsChanged };
}

function normalizeRelayInstanceId(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed || null;
}

function normalizeRelayBootTs(value) {
  const n = Number(value);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
}

function evaluateHeartbeatLease(server, incomingInstanceId, incomingBootTs, now) {
  const currentInstanceId = normalizeRelayInstanceId(server?.relayInstanceId);
  const currentBootTs = normalizeRelayBootTs(server?.relayBootTs);
  const hasActiveLease = Boolean(server?.online)
    && Number(server?.heartbeatAt) > 0
    && (now - Number(server.heartbeatAt)) <= HEARTBEAT_TTL_MS
    && Boolean(currentInstanceId);

  // Backward compatibility: old relays without lease metadata.
  if (!incomingInstanceId) {
    if (!hasActiveLease) return { accept: true, takeover: false, reason: 'no-instance-id' };
    return { accept: false, takeover: false, reason: 'missing-instance-id-while-lease-active' };
  }
  if (!currentInstanceId) return { accept: true, takeover: false, reason: 'lease-empty' };
  if (incomingInstanceId === currentInstanceId) return { accept: true, takeover: false, reason: 'same-instance' };
  if (!hasActiveLease) return { accept: true, takeover: true, reason: 'stale-lease' };

  if (incomingBootTs > 0 && currentBootTs > 0) {
    if (incomingBootTs > currentBootTs) return { accept: true, takeover: true, reason: 'newer-boot' };
    return { accept: false, takeover: false, reason: 'older-boot' };
  }
  if (incomingBootTs > 0 && currentBootTs === 0) return { accept: true, takeover: true, reason: 'boot-present-over-empty' };
  if (incomingBootTs === 0 && currentBootTs > 0) return { accept: false, takeover: false, reason: 'missing-boot-ts' };
  return { accept: false, takeover: false, reason: 'different-instance-active' };
}

function refreshServerLiveness() {
  const now = Date.now();
  let serversChanged = recordLocalServerObservation(now);
  let agentsChanged = false;
  for (const [serverId, server] of Object.entries(servers)) {
    if (!server || typeof server !== 'object') continue;
    if (isServerInMaintenance(serverId, server)) {
      const maintenance = enforceServerMaintenanceOffline(serverId, server, now);
      if (maintenance.serverChanged) serversChanged = true;
      if (maintenance.agentsChanged) agentsChanged = true;
      continue;
    }
    const wasOnline = Boolean(server.online);
    const isLocalServer = isLocalAgentServer(serverId, LOCAL_SERVER_ID);
    const heartbeatAt = Number(server.heartbeatAt) || 0;
    const isOnline = heartbeatAt > 0 && (now - heartbeatAt) <= HEARTBEAT_TTL_MS;
    if (server.online !== isOnline) {
      let affectedAgents = [];
      if (!isOnline) {
        affectedAgents = collectServerAffectedAgents(serverId, server);
        if (clearServerLiveState(server, now)) serversChanged = true;
      } else {
        server.online = true;
        server.updatedAt = now;
        serversChanged = true;
      }
      if (wasOnline && !isOnline && !isLocalServer) {
        if (markAgentsOfflineForServer(serverId, `server-offline:${serverId}`, true)) {
          agentsChanged = true;
        }
        emitServerOfflineAlert(serverId, 'heartbeat-expired', server, affectedAgents);
      }
    }
  }
  if (serversChanged) saveServers();
  if (agentsChanged) saveAgents();
}

// Interrupt, clear the line, type /clear, submit. Inherently an interactive-TUI
// operation, so it is gated on the runtime advertising key support: a headless
// runtime has no prompt to interrupt.
async function injectSlashClear(tmuxTarget) {
  if (!tmuxTarget) return false;
  if (!hostRuntime.capabilities.keys) {
    console.error(`[backend] auto-clear unsupported by the ${hostRuntime.name} runtime`);
    return false;
  }
  // C-c then /clear would interrupt and wipe whatever is in the pane. Refuse
  // outright on a session this install does not own.
  const sessionName = sessionKeyFromTmuxTarget(tmuxTarget);
  const verdict = sessionPolicy.evaluate(sessionName);
  if (!verdict.allowed) {
    console.warn(`[backend] auto-clear refused for ${tmuxTarget}: ${verdict.reason}`);
    return false;
  }
  try {
    const opts = { timeoutMs: 5000 };
    await hostRuntime.sendKeys(tmuxTarget, ['C-c'], opts);
    await new Promise(r => setTimeout(r, 300));
    await hostRuntime.sendKeys(tmuxTarget, ['C-u'], opts);
    await new Promise(r => setTimeout(r, 300));
    await hostRuntime.sendKeys(tmuxTarget, ['/clear'], { ...opts, literal: true });
    await new Promise(r => setTimeout(r, 300));
    await hostRuntime.sendKeys(tmuxTarget, ['Enter'], opts);
    return true;
  } catch (e) {
    console.error(`[backend] auto-clear inject failed for ${tmuxTarget}: ${e.message}`);
    return false;
  }
}

// The runtime returns raw text; hashing is this caller's concern, since the hash
// exists to detect "pane unchanged since last sweep".
async function captureLocalPaneContentAsync(tmuxTarget) {
  if (!tmuxTarget) return null;
  if (!hostRuntime.capabilities.capture) return null;
  const text = await hostRuntime.capturePane(tmuxTarget);
  if (text === null || text === undefined) return null;
  return {
    text,
    hash: createHash('md5').update(text).digest('hex'),
  };
}

// Classification moved to the runtime, since "is this just an idle host?" is a
// per-runtime question. Kept as a named wrapper for readability at the call site.
function isTmuxEmptyServerError(error) {
  return hostRuntime.isEmptyServerError(error);
}

/**
 * Whole-host pane snapshot, indexed the way the liveness sweeps want it.
 *
 * Parsing and the tmux call now live in the runtime; what remains here is
 * backend policy: which fields matter, how paths are normalised, and the
 * session/tty indexes.
 *
 * `runExecFile` is retained as a test seam — passing one builds a runtime around
 * it, so tests can drive this without a live tmux server.
 */
async function buildLocalPaneMetadataSnapshotAsync(runExecFile = null) {
  const runtime = runExecFile ? createTmuxRuntime({ exec: runExecFile }) : hostRuntime;
  const sessions = new Map();
  const ttyToSession = new Map();

  const listing = await runtime.listPanes();
  if (!listing.ok) {
    const error = listing.error;
    return {
      ok: false,
      sessions,
      ttyToSession,
      error: {
        message: error instanceof Error ? error.message : String(error),
        code: error?.code || null,
        signal: error?.signal || null,
      },
    };
  }

  for (const pane of listing.panes) {
    // A session outside the policy is treated as though it were not on the host:
    // no pane snapshot, so the activity sweep and the dashboard both skip it.
    if (!sessionPolicy.allows(pane.session)) continue;
    ttyToSession.set(pane.tty, pane.session);
    if (sessions.has(pane.session)) continue;
    sessions.set(pane.session, {
      panePid: Number.isFinite(pane.pid) && pane.pid > 1 ? pane.pid : null,
      command: pane.command || '',
      workspacePath: normalizeWorkspacePath(pane.path),
    });
  }
  return {
    ok: true,
    sessions,
    ttyToSession,
    error: null,
    // Propagated so the sweep can tell "tmux was unreachable" from "tmux is idle".
    serverUnavailable: listing.serverUnavailable === true,
    // How many panes tmux reported before the session policy filtered them.
    // Without this an empty snapshot is ambiguous between "tmux returned nothing"
    // and "policy excluded everything", which are very different faults.
    rawPaneCount: listing.panes.length,
  };
}

function buildLocalPaneSnapshotMapFromMetadata(paneMetadataSnapshot) {
  if (paneMetadataSnapshot && paneMetadataSnapshot.sessions instanceof Map) {
    return paneMetadataSnapshot.sessions;
  }
  return new Map();
}

function sessionKeyFromTmuxTarget(tmuxTarget) {
  if (typeof tmuxTarget !== 'string') return '';
  const sessionName = tmuxTarget.split(':', 1)[0].trim();
  if (!sessionName) return '';
  return sessionName.startsWith('=') ? sessionName.slice(1) : sessionName;
}

function readLocalPaneSnapshot(tmuxTarget, paneSnapshotMap = null) {
  if (!tmuxTarget || !(paneSnapshotMap instanceof Map)) return null;
  const sessionName = sessionKeyFromTmuxTarget(String(tmuxTarget));
  if (!sessionName) return null;
  return paneSnapshotMap.get(sessionName) || null;
}

function normalizeMcpPresence(value) {
  return value === true
    ? true
    : (value === false ? false : null);
}

function applyLocalRuntimeSignals(agentName, payload = {}) {
  const blocked = payload.blocked === true;
  const reason = blocked && typeof payload.reason === 'string' && payload.reason.trim()
    ? payload.reason.trim()
    : null;
  const workspacePath = normalizeWorkspacePath(payload.workspacePath);
  const mcpPresent = normalizeMcpPresence(payload.mcpPresent);
  const blockedObserved = payload.blockedObserved === true;
  const digest = JSON.stringify({
    blocked,
    reason,
    workspacePath: workspacePath || null,
    mcpPresent,
  });
  if (localRuntimeSignalDigest.get(agentName) === digest && !(blocked && blockedObserved)) return;
  localRuntimeSignalDigest.set(agentName, digest);
  const transition = applyRuntimeObservation(agentName, {
    blocked,
    reason,
    tail: blocked && typeof payload.tail === 'string' ? payload.tail : '',
    command: typeof payload.command === 'string' ? payload.command : '',
    workspacePath,
    mcpPresent,
    blockedObserved,
    server: 'local',
    observerSource: 'local-sweep',
    observerServer: 'local',
  });
  dispatchBlockedNotifications(transition);
}

function isEphemeralAuditAgentName(name) {
  return typeof name === 'string' && /-system_audit-[a-z0-9]+$/i.test(name);
}

function pruneEphemeralAgents(names = [], reason = 'ephemeral-prune') {
  const unique = [...new Set((Array.isArray(names) ? names : []).filter(Boolean))];
  if (unique.length === 0) return;

  let agentsChanged = false;
  let runtimeChanged = false;
  let cursorsChanged = false;
  let groupsChanged = false;
  const removed = [];

  for (const name of unique) {
    const agent = agents[name];
    if (!isAgentRecord(agent)) continue;
    if (!isEphemeralAuditAgentName(name)) continue;

    delete agents[name];
    agentsChanged = true;
    removed.push(name);

    if (agentRuntime[name] !== undefined) {
      delete agentRuntime[name];
      runtimeChanged = true;
    }
    if (cursors[name] !== undefined) {
      delete cursors[name];
      cursorsChanged = true;
    }
    for (const group of Object.values(groups)) {
      if (!Array.isArray(group?.members)) continue;
      const nextMembers = group.members.filter(m => m !== name);
      if (nextMembers.length !== group.members.length) {
        group.members = nextMembers;
        groupsChanged = true;
      }
    }
  }

  if (agentsChanged) saveAgents();
  if (runtimeChanged) saveAgentRuntime();
  if (cursorsChanged) saveCursors();
  if (groupsChanged) saveGroups();

  // Intentionally silent: ephemeral audit agent pruning is routine housekeeping.
}


/**
 * How this agent is driven. Recorded on the record at registration; falls back to
 * the framework registry, then to tmux, so records written before transports
 * existed keep their behaviour.
 */
function agentTransport(agent) {
  const declared = typeof agent?.transport === 'string' ? agent.transport.trim().toLowerCase() : '';
  if (declared === 'acp' || declared === 'tmux') return declared;
  return getFramework(agent?.type)?.transport === 'acp' ? 'acp' : 'tmux';
}

/**
 * Liveness for a paneless ACP agent: is the process recorded at launch alive?
 * There is no pane hash to compare and no heartbeat from a relay, because the
 * relay enumerates tmux sessions and this agent has none.
 */
function syncAcpAgentLiveness(agent, runtime) {
  const pid = Number(agent.acpPid) || 0;
  let alive = false;
  if (pid > 1) {
    try { process.kill(pid, 0); alive = true; } catch { alive = false; }
  }
  let changed = false;
  if (agent.tmux !== null) { agent.tmux = null; changed = true; }
  if (agent.online !== alive) { agent.online = alive; changed = true; }
  const reason = alive ? null : 'acp-process-gone';
  if (agent.offlineReason !== reason) { agent.offlineReason = reason; changed = true; }
  if (changed) agent.lastSeen = Date.now();
  if (runtime && runtime.mcpPresent === undefined) runtime.mcpPresent = null;

  /*
   * FEED THE STATE MACHINE TOO, or the record and the API disagree.
   *
   * `agent.online` above is the persisted record; `serializeAgent` does not read it.
   * It reads getAgentMachine(name), and the tmux sweep is what keeps that machine
   * current — via syncAgentMachine, which this path never called. So a running ACP
   * agent had `online: true` on disk and `online: false` over the API, forever. On a
   * clean host that is the whole picture the console shows: a healthy octos agent,
   * launched and registered and serving a binding, displayed as offline.
   *
   * `heartbeatPresent` is the right signal rather than `tmuxPresent`: an ACP agent
   * has no pane, and its liveness IS the process check just performed. The machine's
   * heartbeat events carry exactly that meaning — the agent is answering — and using
   * the tmux event would make an agent with no pane depend on a pane transition.
   */
  syncAgentMachine(agent.name, alive
    ? { heartbeatPresent: true, manualDown: agent.manualDown === true }
    : { heartbeatMissing: true });

  return changed;
}

async function sweepLocalActivityDurations(paneMetadataSnapshotOverride = null) {
  const nowSec = Math.floor(Date.now() / 1000);
  const nowMs = Date.now();
  let runtimeChanged = false;
  let agentsChanged = false;
  const pruneCandidates = new Set();
  const localRuntimeAgents = new Set();
  const paneMetadataSnapshot = paneMetadataSnapshotOverride || await buildLocalPaneMetadataSnapshotAsync();
  if (paneMetadataSnapshot?.ok !== true) {
    if ((nowMs - localTmuxSnapshotWarnAt) >= 60_000) {
      localTmuxSnapshotWarnAt = nowMs;
      const detail = paneMetadataSnapshot?.error?.message || 'unknown tmux query failure';
      console.warn(`[backend] local tmux pane snapshot unavailable; preserving agent state: ${detail}`);
    }
    return;
  }
  const mcpSessions = await getLocalMcpSessionSetAsync(true, paneMetadataSnapshot);
  const paneSnapshotMap = buildLocalPaneSnapshotMapFromMetadata(paneMetadataSnapshot);
  const localRows = [];

  for (const agent of Object.values(agents)) {
    if (!isAgentRecord(agent)) continue;
    const serverId = normalizeServer(agent.server);
    if (!isLocalAgentServer(serverId, LOCAL_SERVER_ID)) continue;
    localRuntimeAgents.add(agent.name);

    if (isOnDemandThreadSessionAgent(agent)) {
      localTmuxMissingState.delete(agent.name);
      localActivityState.delete(agent.name);
      localCompactState.delete(agent.name);
      continue;
    }

    const manualDown = agent.manualDown === true;
    const configuredTmux = (typeof agent.tmux === 'string' && agent.tmux.trim()) ? agent.tmux.trim() : null;
    // An ACP agent is a subprocess, not a tmux session: there is no pane to
    // probe, capture or type into. Deriving a pane target for it would mark it
    // tmux-missing on the first sweep and keep it offline forever, which is
    // exactly what "agent equals tmux session" costs once that stops being true.
    const transport = agentTransport(agent);
    const tmuxTarget = transport === 'acp'
      ? null
      : (configuredTmux || (manualDown ? null : `${agent.name}:0.0`));
    const runtime = ensureAgentRuntimeRecord(agent.name);
    if (!runtime) continue;
    if (transport === 'acp') {
      // Liveness for these comes from the process itself, recorded at launch.
      if (syncAcpAgentLiveness(agent, runtime)) agentsChanged = true;
      continue;
    }
    localRows.push({
      agent,
      runtime,
      manualDown,
      tmuxTarget,
    });
  }

  const captureCandidates = localRows
    .filter(row => row.tmuxTarget)
    .map(row => row.agent.name);
  const sampled = resolveLocalActivityCaptureSelection(captureCandidates, LOCAL_ACTIVITY_CAPTURE_BUDGET).selected;

  for (const row of localRows) {
    const { agent, runtime, manualDown, tmuxTarget } = row;
    if (!tmuxTarget) {
      localTmuxMissingState.delete(agent.name);
      localActivityState.delete(agent.name);
      localCompactState.delete(agent.name);
      applyLocalRuntimeSignals(agent.name, {
        blocked: false,
        reason: null,
        tail: '',
        command: '',
        workspacePath: null,
        mcpPresent: null,
      });
      const resetChanged = setRuntimeActivityFields(runtime, {
        activeNow: false,
        activeDurationSec: 0,
        idleDurationSec: 0,
        lastTmuxActivitySec: null,
      });
      if (resetChanged) {
        runtime.updatedAt = Date.now();
        runtimeChanged = true;
      }
      continue;
    }

    const paneSnapshot = readLocalPaneSnapshot(tmuxTarget, paneSnapshotMap);
    const hasSession = !!paneSnapshot;
    const paneCmd = paneSnapshot?.command || '';
    const workspacePath = paneSnapshot?.workspacePath || null;
    const mcpPresent = hasSession ? mcpSessions.has(agent.name) : null;

    if (!hasSession) {
      let missing = localTmuxMissingState.get(agent.name);
      if (!missing) {
        missing = {
          since: nowMs,
          alerted: false,
          misses: 0,
          wasOnline: agent.online === true,
        };
      }
      missing.misses = Math.max(0, Number(missing.misses) || 0) + 1;
      localTmuxMissingState.set(agent.name, missing);
      if (missing.misses < AGENT_TMUX_MISSING_THRESHOLD) continue;

      applyLocalRuntimeSignals(agent.name, {
        blocked: false,
        reason: null,
        tail: '',
        command: paneCmd,
        workspacePath,
        mcpPresent,
      });
      localActivityState.delete(agent.name);
      localCompactState.delete(agent.name);
      const resetChanged = setRuntimeActivityFields(runtime, {
        activeNow: false,
        activeDurationSec: 0,
        idleDurationSec: 0,
        lastTmuxActivitySec: null,
      });
      if (resetChanged) {
        runtime.updatedAt = Date.now();
        runtimeChanged = true;
      }
      const missingForMs = Math.max(0, nowMs - (Number(missing.since) || nowMs));
      const wasOnline = missing.wasOnline === true;
      const prevLastSeenMs = Number(agent.lastSeen) || 0;
      const seenAgeMs = prevLastSeenMs > 0 ? Math.max(0, nowMs - prevLastSeenMs) : 0;
      const recentEnough = prevLastSeenMs <= 0 || seenAgeMs <= AGENT_TMUX_MISSING_ALERT_MAX_AGE_MS;
      const wasManualDown = manualDown;
      let transitioned = false;
      // online driven by machine via syncAgentMachine below
      const prevOnline = agent.online;
      // Retain the selected legacy transport when its pane disappears;
      // otherwise a later sweep could mistake this same agent for on-demand.
      if (agent.transport == null || (typeof agent.transport === 'string' && !agent.transport.trim())) {
        agent.transport = 'tmux'; agentsChanged = true;
      }
      if (agent.tmux !== null) { agent.tmux = null; agentsChanged = true; transitioned = true; }
      if (!wasManualDown && agent.offlineReason !== 'tmux-missing:auto') {
        agent.offlineReason = 'tmux-missing:auto';
        agentsChanged = true;
        transitioned = true;
      }
      syncAgentMachine(agent.name, { tmuxMissing: true });
      if (agent.online !== prevOnline) { agentsChanged = true; transitioned = true; }
      if (transitioned) {
        agent.lastSeen = nowMs;
        // Marking an agent tmux-missing used to be entirely silent, so an agent
        // whose session plainly existed could sit offline with nothing anywhere
        // saying why — the dashboard simply showed an empty fleet. Log the
        // transition once, with what the snapshot actually held, so the next
        // person does not have to reverse-engineer the sweep to find out.
        console.warn(`[backend] agent marked tmux-missing: agent=${agent.name} target=${tmuxTarget} `
          + `rawPanes=${paneMetadataSnapshot.rawPaneCount ?? '?'} `
          + `snapshotSessions=${JSON.stringify([...paneSnapshotMap.keys()])} misses=${missing.misses}`);
      }
      if (!wasManualDown
        && wasOnline
        && recentEnough
        && !missing.alerted
        && missingForMs >= AGENT_TMUX_MISSING_ALERT_GRACE_MS) {
        missing.alerted = true;
        localTmuxMissingState.set(agent.name, missing);
        if (agent.offlineReason === 'tmux-missing:auto') {
          maybeEmitUnexpectedOfflineAlert(agent.name, 'tmux-missing:auto', { server: 'local', detail: `tmux target ${tmuxTarget} not found` });
        }
      }
      if (isEphemeralAuditAgentName(agent.name)) pruneCandidates.add(agent.name);
      autoClearPrevReason.delete(agent.name);
      continue;
    }

    if (!sampled.has(agent.name)) {
      applyLocalMetadataOnlySignals(agent.name, {
        workspacePath,
        mcpPresent,
      });
      localTmuxMissingState.delete(agent.name);
      if (syncLocalAgentOnlineState(agent, runtime, tmuxTarget, manualDown)) {
        agentsChanged = true;
      }
      autoClearPrevReason.delete(agent.name);
      continue;
    }

    const paneCapture = await captureLocalPaneContentAsync(tmuxTarget);
    if (!paneCapture?.hash) {
      applyLocalMetadataOnlySignals(agent.name, {
        workspacePath,
        mcpPresent,
      });
      localTmuxMissingState.delete(agent.name);
      if (syncLocalAgentOnlineState(agent, runtime, tmuxTarget, manualDown)) {
        agentsChanged = true;
      }
      autoClearPrevReason.delete(agent.name);
      continue;
    }

    const paneHash = paneCapture.hash;
    const blockedReason = detectLocalBlockedReason(paneCapture.text, paneCmd);
    const blocked = Boolean(blockedReason);
    applyLocalRuntimeSignals(agent.name, {
      blocked,
      reason: blockedReason,
      tail: blocked ? recentTailWindow(paneCapture.text, LOCAL_BLOCK_TAIL_LINES) : '',
      command: paneCmd,
      workspacePath,
      mcpPresent: mcpSessions.has(agent.name),
    });

    // Auto-clear: when api-image-error is newly detected, inject /clear to recover
    const prevBlockedReason = autoClearPrevReason.get(agent.name) || null;
    autoClearPrevReason.set(agent.name, blockedReason);
    if (blockedReason === 'api-image-error' && prevBlockedReason !== 'api-image-error') {
      const now = Date.now();
      const lastClear = autoClearLastTs.get(agent.name) || 0;
      if ((now - lastClear) > AUTO_CLEAR_COOLDOWN_MS) {
        console.warn(`[backend] auto-clear: agent ${agent.name} stuck on api-image-error, injecting /clear to ${tmuxTarget}`);
        injectSlashClear(tmuxTarget).then(ok => {
          if (ok) autoClearLastTs.set(agent.name, Date.now());
        });
      }
    }

    localTmuxMissingState.delete(agent.name);
    const compactSignal = detectAgentCompactSignal('', paneCapture.text);
    if (compactSignal) {
      const marker = normalizeCompactMarker(compactSignal.marker);
      const prevMarker = localCompactState.get(agent.name) || null;
      if (prevMarker !== marker) {
        emitRuntimeCompactEvent(agent.name, {
          mode: compactSignal.mode,
          marker,
          source: 'local-sweep',
          summary: marker.replace(/-/g, ' '),
        });
      }
      localCompactState.set(agent.name, marker);
    } else {
      localCompactState.delete(agent.name);
    }

    let st = localActivityState.get(agent.name);
    if (!st) {
      st = {
        lastHash: paneHash,
        lastChangeSec: nowSec,
        burstStartSec: nowSec,
        burstLastSec: nowSec,
      };
      localActivityState.set(agent.name, st);
    } else if (paneHash !== st.lastHash) {
      const gap = nowSec - st.lastChangeSec;
      if (gap > IDLE_THRESHOLD_SEC) {
        st.burstStartSec = nowSec;
        st.burstLastSec = nowSec;
      } else {
        st.burstLastSec = nowSec;
      }
      st.lastHash = paneHash;
      st.lastChangeSec = nowSec;
    }

    const rawIdleSec = Math.max(0, nowSec - st.lastChangeSec);
    const activeNow = rawIdleSec < IDLE_THRESHOLD_SEC;
    const activeDurationSec = activeNow ? Math.max(0, nowSec - st.burstStartSec) : 0;
    const idleDurationSec = activeNow ? 0 : Math.max(0, rawIdleSec - IDLE_THRESHOLD_SEC);

    const changed = setRuntimeActivityFields(runtime, {
      activeNow,
      activeDurationSec,
      idleDurationSec,
      lastTmuxActivitySec: st.lastChangeSec,
    });
    if (changed) {
      runtime.updatedAt = Date.now();
      runtimeChanged = true;
    }

    if (syncLocalAgentOnlineState(agent, runtime, tmuxTarget, manualDown)) {
      agentsChanged = true;
    }
  }

  if (runtimeChanged) saveAgentRuntime();
  if (agentsChanged) saveAgents();
  for (const name of [...localRuntimeSignalDigest.keys()]) {
    if (!localRuntimeAgents.has(name)) {
      localRuntimeSignalDigest.delete(name);
    }
  }
  if (pruneCandidates.size > 0) {
    pruneEphemeralAgents([...pruneCandidates], 'tmux-missing:auto');
  }
}

async function readLocalSwapUsageSnapshot() {
  try {
    const raw = await readFileAsync('/proc/meminfo', 'utf-8');
    const fields = {};
    for (const line of raw.split('\n')) {
      const m = line.match(/^([A-Za-z_]+):\s+(\d+)\s+kB$/);
      if (!m) continue;
      fields[m[1]] = Number.parseInt(m[2], 10);
    }
    const totalKb = Number(fields.SwapTotal) || 0;
    const freeKb = Number(fields.SwapFree) || 0;
    if (totalKb <= 0) return null;
    const usedKb = Math.max(0, totalKb - freeKb);
    const usagePct = (usedKb / totalKb) * 100;
    return { totalKb, freeKb, usedKb, usagePct };
  } catch {
    return null;
  }
}

async function sweepLocalSwapPressure() {
  const snap = await readLocalSwapUsageSnapshot();
  if (!snap) return;

  swapAlertState.lastPct = snap.usagePct;
  const usagePctText = snap.usagePct.toFixed(1);
  const usedGb = (snap.usedKb / (1024 * 1024)).toFixed(2);
  const totalGb = (snap.totalKb / (1024 * 1024)).toFixed(2);

  if (snap.usagePct >= SWAP_ALERT_THRESHOLD_PCT) {
    if (!swapAlertState.active) {
      swapAlertState.active = true;
      swapAlertState.lastAlertAt = Date.now();
      emitSystemInfo(
        `OOM warning: swap usage ${usagePctText}% (>= ${SWAP_ALERT_THRESHOLD_PCT}%)`,
        [
          `Swap used: ${usedGb} GiB / ${totalGb} GiB (${usagePctText}%)`,
          `Threshold: ${SWAP_ALERT_THRESHOLD_PCT}%`,
          'System memory pressure is high. Please intervene manually to avoid OOM killing agents.',
        ].join('\n'),
        'swap_high',
        {
          dedupeKey: 'swap_high',
          owner: 'host-runtime',
          runbook: 'docs/runbooks/swap-high.md',
          impact: 'high swap usage can stall or kill local agent processes',
          recoveryCondition: `swap usage falls to or below ${SWAP_ALERT_CLEAR_PCT.toFixed(1)}% and swap_clear auto-resolves this alert`,
          correlation: {
            dedupeKey: 'swap_high',
            host: 'local',
            thresholdPct: SWAP_ALERT_THRESHOLD_PCT,
            clearPct: SWAP_ALERT_CLEAR_PCT,
          },
        }
      );
    }
    return;
  }

  if (swapAlertState.active && snap.usagePct <= SWAP_ALERT_CLEAR_PCT) {
    swapAlertState.active = false;
    emitSystemInfo(
      `OOM warning cleared: swap usage back to ${usagePctText}%`,
      [
        `Swap used: ${usedGb} GiB / ${totalGb} GiB (${usagePctText}%)`,
        `Clear threshold: ${SWAP_ALERT_CLEAR_PCT.toFixed(1)}%`,
      ].join('\n'),
      'swap_clear',
      { dedupeKey: 'swap_clear' }
    );
  }
}

function scopeUnitForAgent(agentName) {
  const base = String(agentName || '').trim().replace(/[^A-Za-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '');
  if (!base) return null;
  return `agent-${base}.scope`;
}

function scopeUnitFromCgroupPath(cgroupPath) {
  const text = String(cgroupPath || '').trim();
  if (!text) return null;
  const leaf = text.split('/').filter(Boolean).pop() || '';
  return leaf.endsWith('.scope') ? leaf : null;
}

async function scopeUnitForPid(pid) {
  const n = Number.parseInt(pid, 10);
  if (!Number.isFinite(n) || n <= 1) return null;
  try {
    const raw = await readFileAsync(`/proc/${n}/cgroup`, 'utf-8');
    for (const line of String(raw || '').split('\n')) {
      if (!line) continue;
      const idx = line.indexOf(':');
      const idx2 = idx >= 0 ? line.indexOf(':', idx + 1) : -1;
      if (idx2 < 0) continue;
      const pathPart = line.slice(idx2 + 1).trim();
      const unit = scopeUnitFromCgroupPath(pathPart);
      if (unit) return unit;
    }
  } catch {
    return null;
  }
  return null;
}

async function buildLocalPanePidMapAsync() {
  const out = new Map();
  const snapshotMap = buildLocalPaneSnapshotMapFromMetadata(await buildLocalPaneMetadataSnapshotAsync());
  for (const [session, snapshot] of snapshotMap.entries()) {
    const panePid = Number(snapshot?.panePid || 0);
    if (!session || !Number.isFinite(panePid) || panePid <= 1) continue;
    if (!out.has(session)) out.set(session, panePid);
  }
  return out;
}

function parseSystemdMemoryValue(raw) {
  const text = String(raw || '').trim().toLowerCase();
  if (!text || text === 'infinity' || text === 'max') return 0;
  const n = Number.parseInt(text, 10);
  return Number.isFinite(n) && n > 0 ? n : 0;
}

async function readAgentScopeMemory(agentName, panePidMap = null) {
  const agent = agents[agentName];
  const tmuxTarget = (typeof agent?.tmux === 'string' && agent.tmux.trim())
    ? agent.tmux.trim()
    : `${agentName}:0.0`;
  const sessionName = sessionKeyFromTmuxTarget(tmuxTarget) || agentName;
  const panePid = (panePidMap instanceof Map) ? panePidMap.get(sessionName) : null;
  const unit = (await scopeUnitForPid(panePid)) || scopeUnitForAgent(agentName);
  if (!unit) return null;
  try {
    const env = USER_RUNTIME_DIR && USER_DBUS_SESSION_BUS
      ? { ...process.env, XDG_RUNTIME_DIR: USER_RUNTIME_DIR, DBUS_SESSION_BUS_ADDRESS: USER_DBUS_SESSION_BUS }
      : process.env;
    const { stdout: out } = await execFileAsync(
      'systemctl',
      ['--user', 'show', unit, '--property=ActiveState', '--property=MemoryCurrent', '--property=MemoryHigh', '--value', '--no-pager'],
      { encoding: 'utf-8', timeout: 3000, env }
    );
    const [activeStateRaw, currentRaw, highRaw] = String(out || '').split('\n');
    const activeState = String(activeStateRaw || '').trim().toLowerCase();
    if (activeState !== 'active') return null;
    const memoryCurrent = parseSystemdMemoryValue(currentRaw);
    const memoryHigh = parseSystemdMemoryValue(highRaw);
    if (memoryCurrent <= 0 || memoryHigh <= 0) return null;
    return { unit, memoryCurrent, memoryHigh };
  } catch {
    return null;
  }
}

function pushResourceAlertToAgent(agentName, summary) {
  const agent = agents[agentName];
  if (!isAgentRecord(agent) || !agent.tmux) return;
  const state = getAgentDeliveryState(agentName);
  if (!state.online) return;

  const payload = `[RESOURCE ALERT] ${summary}\nPlease pause heavy tasks, checkpoint progress, and reduce memory usage immediately.`;
  postToDeliveryQueue({
      from: 'hagency-backend',
      to: agent.tmux,
      payload,
      notifyMeta: {
        kind: 'resource_alert',
        requiresInboxCheck: false,
        sourceMsgId: null,
        unreadCount: 0,
        hasHumanUnread: false,
        hasRequestUnread: false,
        needsReply: false,
        hasMcp: false,
      },
  }).catch((e) => {
    console.warn(`[scope-alert] queue push failed for ${agentName}: ${e.message}`);
  });
}

function formatBytesGiB(bytes) {
  return (bytes / (1024 ** 3)).toFixed(2);
}

async function sweepAgentScopePressure() {
  if (!AGENT_SCOPE_MONITOR_ENABLED) return;
  const now = Date.now();
  const panePidMap = await buildLocalPanePidMapAsync();
  const localAgentNames = Object.values(agents)
    .filter(isAgentRecord)
    .filter(agent => {
      const serverId = normalizeServer(agent.server);
      return isLocalAgentServer(serverId, LOCAL_SERVER_ID);
    })
    .map(agent => agent.name);

  const activeSet = new Set(localAgentNames);
  for (const key of [...scopePressureState.keys()]) {
    if (!activeSet.has(key)) scopePressureState.delete(key);
  }

  for (const agentName of localAgentNames) {
    const scope = await readAgentScopeMemory(agentName, panePidMap);
    const prev = scopePressureState.get(agentName) || { high: false };
    if (!scope) {
      if (prev.high) scopePressureState.set(agentName, { high: false });
      continue;
    }

    const ratio = scope.memoryCurrent / scope.memoryHigh;
    const highNow = ratio >= 1;

    if (highNow) {
      if (!prev.high) scopePressureState.set(agentName, { high: true });
      const summary = `agent=${agentName} unit=${scope.unit} memoryHigh exceeded (${formatBytesGiB(scope.memoryCurrent)}GiB / ${formatBytesGiB(scope.memoryHigh)}GiB, ${(ratio * 100).toFixed(1)}%)`;
      const result = notificationRouter.emit('resource_alert', {
        agentName,
        summary: `Agent '${agentName}' memory high exceeded`,
        full: summary,
      });
      if (result.accepted) pushResourceAlertToAgent(agentName, summary);
      continue;
    }

    const clearNow = ratio <= AGENT_SCOPE_ALERT_CLEAR_RATIO;
    if (prev.high && clearNow) {
      scopePressureState.set(agentName, { high: false });
      notificationRouter.emit('resource_alert', {
        agentName,
        summary: `Agent '${agentName}' memory pressure recovered`,
        full: `agent=${agentName} unit=${scope.unit} current=${formatBytesGiB(scope.memoryCurrent)}GiB high=${formatBytesGiB(scope.memoryHigh)}GiB (${(ratio * 100).toFixed(1)}%)`,
      }, { bypassCooldown: true });
    }
  }
}

function runAsyncSweep(label, fn, stateKey) {
  if (stateKey === 'localActivity' && localActivitySweepRunning) return;
  if (stateKey === 'localSwap' && localSwapSweepRunning) return;
  if (stateKey === 'agentScope' && agentScopeSweepRunning) return;
  if (stateKey === 'supervisorLifecycle' && supervisorLifecycleSweepRunning) return;
  if (stateKey === 'projectSides' && projectSideSweepRunning) return;

  if (stateKey === 'localActivity') localActivitySweepRunning = true;
  if (stateKey === 'localSwap') localSwapSweepRunning = true;
  if (stateKey === 'agentScope') agentScopeSweepRunning = true;
  if (stateKey === 'supervisorLifecycle') supervisorLifecycleSweepRunning = true;
  if (stateKey === 'projectSides') projectSideSweepRunning = true;

  Promise.resolve()
    .then(fn)
    .catch((error) => {
      console.error(`[${label}] ${error?.message || error}`);
    })
    .finally(() => {
      if (stateKey === 'localActivity') localActivitySweepRunning = false;
      if (stateKey === 'localSwap') localSwapSweepRunning = false;
      if (stateKey === 'agentScope') agentScopeSweepRunning = false;
      if (stateKey === 'supervisorLifecycle') supervisorLifecycleSweepRunning = false;
      if (stateKey === 'projectSides') projectSideSweepRunning = false;
    });
}

function countLocalSweepAgents() {
  let count = 0;
  for (const agent of Object.values(agents)) {
    if (!isAgentRecord(agent) || agent.kind === 'human') continue;
    const serverId = normalizeServer(agent.server);
    if (!isLocalAgentServer(serverId, LOCAL_SERVER_ID)) continue;
    count += 1;
  }
  return count;
}

export function computeAdaptiveSweepIntervalMs(baseIntervalMs, agentCount = 0) {
  const normalizedBase = Number.isFinite(Number(baseIntervalMs)) && Number(baseIntervalMs) > 0
    ? Math.floor(Number(baseIntervalMs))
    : 5000;
  const normalizedAgentCount = Math.max(0, Number.parseInt(agentCount, 10) || 0);
  return Math.max(normalizedBase, normalizedAgentCount * AGENT_SWEEP_INTERVAL_PER_AGENT_MS);
}

function scheduleAdaptiveSweepLoop(label, fn, stateKey, baseIntervalMs) {
  const tick = () => {
    if (!backgroundLoopsStarted) return;
    runAsyncSweep(label, fn, stateKey);
    const nextDelay = computeAdaptiveSweepIntervalMs(baseIntervalMs, countLocalSweepAgents());
    trackLifecycleTimeout(tick, nextDelay, { unref: true });
  };
  const initialDelay = computeAdaptiveSweepIntervalMs(baseIntervalMs, countLocalSweepAgents());
  trackLifecycleTimeout(tick, initialDelay, { unref: true });
}

function getAgentDeliveryState(name) {
  const agent = agents[name];
  if (!agent || !isAgentRecord(agent)) {
    return { exists: false, online: false, healthy: false, server: null, serverOnline: false, lastSeen: null, offlineReason: 'not-agent' };
  }
  const machine = getAgentMachine(name);
  const agentOnline = machine.online;
  const serverId = normalizeServer(agent.server);
  let serverOnline = true;
  let serverLastSeen = null;
  if (serverId && !isLocalAgentServer(serverId, LOCAL_SERVER_ID)) {
    const server = servers[serverId];
    if (server) {
      serverOnline = Boolean(server.online);
      serverLastSeen = server.lastSeen || null;
    } else {
      serverOnline = agentOnline;
    }
  }
  const online = agentOnline && serverOnline;
  return {
    exists: true,
    online,
    healthy: machine.healthy && serverOnline,
    agentOnline,
    server: serverId,
    serverOnline,
    lastSeen: agent.lastSeen || null,
    serverLastSeen,
    offlineReason: agent.offlineReason || null,
  };
}

function serializeAgents(records) {
  const snapshot = THREAD_SESSIONS_ENABLED && routerStore && records.some((agent) => agent.agentId)
    ? routerStore.snapshot() : null;
  return records.map((agent) => serializeAgent(agent, snapshot));
}

function serializeAgent(agent, sharedRouterSnapshot) {
  const deliveryState = getAgentDeliveryState(agent.name);
  const runtime = ensureAgentRuntimeRecord(agent.name);
  const machine = getAgentMachine(agent.name);
  let threadRuntime = null;
  if (routerStore && isOnDemandThreadSessionAgent(agent)) {
    const snapshot = sharedRouterSnapshot ?? routerStore.snapshot();
    const sessionIds = new Set(snapshot.sessions
      .filter((session) => session.agentId === agent.agentId)
      .map((session) => session.sessionId));
    if (sessionIds.size) {
      const history = snapshot.dispatches.filter((dispatch) => sessionIds.has(dispatch.sessionId));
      const live = routerStore.listAgentDispatches(agent.agentId);
      const started = live.find((dispatch) => dispatch.state === 'started');
      const parked = live.find((dispatch) => dispatch.state === 'parked');
      const leased = live.find((dispatch) => dispatch.state === 'leased');
      const queued = live.find((dispatch) => dispatch.state === 'queued');
      const unresolved = history.find((dispatch) => dispatch.state === 'outcome_unknown' && !dispatch.resolutionAction);
      const stopping = history.some((dispatch) => liveThreadSessionRunners.has(dispatch.dispatchId)
        && !live.some((row) => row.dispatchId === dispatch.dispatchId));
      const queuedDetail = queued && history.find((row) => row.dispatchId === queued.dispatchId);
      const blockedReason = unresolved?.terminalReason || queuedDetail?.blockedBy?.reason || null;
      const observedAt = Date.now();
      const lastSettlement = Math.max(0, ...history.map((dispatch) => Number(dispatch.settledAt) || 0));
      const state = started ? 'running' : parked ? 'waiting_approval' : leased ? 'starting'
        : stopping ? 'stopping' : agent.manualDown ? 'stopped'
          : blockedReason ? 'blocked' : queued ? 'queued' : 'idle';
      threadRuntime = {
        transport: 'thread-session',
        state,
        activeNow: Boolean(started),
        online: Boolean(started || parked || leased || stopping),
        agentOnline: Boolean(started || parked || leased || stopping),
        healthy: Boolean(started),
        blocked: !started && !leased && Boolean(parked || blockedReason),
        blockedReason: parked ? 'waiting_for_owner_approval' : blockedReason,
        activeDurationSec: started?.startedAt ? Math.max(0, Math.floor((observedAt - started.startedAt) / 1000)) : null,
        idleDurationSec: !live.length && !stopping && lastSettlement
          ? Math.max(0, Math.floor((observedAt - lastSettlement) / 1000)) : null,
        lastTmuxActivitySec: null,
        runtimeObservation: { observerSource: 'router', observerServer: LOCAL_SERVER_ID, observedAt },
      };
    }
  }
  const dispatchActivity = serializeThreadSessionDispatchActivity(agent);
  const runner = serializeThreadSessionRunner(agent, dispatchActivity);
  const { executionPolicy: _executionPolicy, ...publicAgent } = agent;
  return {
    ...publicAgent,
    server: normalizeServer(agent.server),
    state: agent.manualDown && agent.offlineReason === 'operator-stopped' ? 'stopped' : machine.state,
    healthy: machine.healthy,
    online: deliveryState.online,
    agentOnline: deliveryState.agentOnline,
    serverOnline: deliveryState.serverOnline,
    lastSeen: deliveryState.lastSeen,
    serverLastSeen: deliveryState.serverLastSeen,
    offlineReason: runner && deliveryState.offlineReason === 'tmux-missing:auto' ? null : deliveryState.offlineReason,
    runner,
    dispatchActivity,
    manualDown: agent.manualDown === true,
    blocked: runtime?.blocked === true,
    blockedReason: runtime?.blockedReason || null,
    blockedTier: normalizeBlockedTier(runtime?.blockedTier, null),
    blockedSince: runtime?.blockedSince || null,
    agentModelVersion: normalizeAgentModelVersion(agent.agentModelVersion) || null,
    layoutVersion: normalizeLayoutVersion(agent.layoutVersion) || null,
    agentId: normalizeAgentId(agent.agentId) || null,
    homeDir: normalizeWorkspacePath(agent.homeDir) || null,
    workdir: normalizeWorkspacePath(agent.workdir) || null,
    stateDir: normalizeWorkspacePath(agent.stateDir) || null,
    managedProjects: normalizeManagedProjects(agent.managedProjects),
    human: normalizeHumanMeta(agent.human),
    task: normalizeAgentTask(agent.task, agent.name),
    runtimeProfile: redactRuntimeProfileSecrets(normalizeRuntimeProfile(agent.runtimeProfile)),
    environment: VALID_ENVIRONMENTS.has(agent.environment) ? agent.environment : classifyEnvironment(agent.name),
    activeNow: normalizeRuntimeActiveNow(runtime?.activeNow),
    activeDurationSec: Number(runtime?.activeDurationSec) || 0,
    idleDurationSec: Number(runtime?.idleDurationSec) || 0,
    lastTmuxActivitySec: Number(runtime?.lastTmuxActivitySec) || null,
    workspacePath: runtime?.workspacePath || null,
    // Where it last ran, retained after it stops. Metering reads this; see
    // setRuntimeWorkspacePath.
    lastWorkspacePath: runtime?.lastWorkspacePath || runtime?.workspacePath || null,
    runtimeObservation: serializeRuntimeObservation(runtime),
    mcpPresent: runtime?.mcpPresent === true
      ? true
      : (runtime?.mcpPresent === false ? false : null),
    mcpMissingSince: Number(runtime?.mcpMissingSince) || null,
    ...threadRuntime,
  };
}

function applyServerHeartbeat(serverId, payload = {}, sourceIp = null) {
  const now = Date.now();
  const server = ensureServerRecord(serverId);
  if (isServerInMaintenance(serverId, server)) {
    let serversChanged = false;
    let agentsChanged = false;
    const lastSeen = Number(server.lastSeen) || 0;
    if (!lastSeen || (now - lastSeen) >= SERVER_MAINTENANCE_LAST_SEEN_UPDATE_MS) {
      server.lastSeen = now;
      serversChanged = true;
    }
    const nextSourceIp = sourceIp || null;
    if (server.sourceIp !== nextSourceIp) {
      server.sourceIp = nextSourceIp;
      serversChanged = true;
    }
    const maintenance = enforceServerMaintenanceOffline(serverId, server, now);
    if (maintenance.serverChanged) serversChanged = true;
    if (maintenance.agentsChanged) agentsChanged = true;
    if (serversChanged) saveServers();
    if (agentsChanged) saveAgents();
    return { ok: true, leaseAccepted: true, leaseReason: 'maintenance', maintenance: true, ignored: true };
  }
  const wasOnline = Boolean(server.online);
  const incomingInstanceId = normalizeRelayInstanceId(payload.instanceId);
  const incomingBootTs = normalizeRelayBootTs(payload.bootTs);
  const lease = evaluateHeartbeatLease(server, incomingInstanceId, incomingBootTs, now);
  if (!lease.accept) {
    return { ok: false, leaseAccepted: false, leaseReason: lease.reason };
  }
  const sessions = Array.isArray(payload.sessions)
    ? [...new Set(payload.sessions.filter(s => typeof s === 'string' && s.trim()).map(s => s.trim()))]
    : [];
  const heartbeatAgents = Array.isArray(payload.agents) ? payload.agents : sessions;
  const liveAgents = [...new Set(heartbeatAgents.filter(s => typeof s === 'string' && s.trim()).map(s => s.trim()))];
  const liveSet = new Set(liveAgents);

  server.lastSeen = now;
  server.heartbeatAt = now;
  server.relayInstanceId = incomingInstanceId;
  server.relayBootTs = incomingBootTs;
  server.online = true;
  server.updatedAt = now;
  server.sourceIp = sourceIp || null;
  server.version = typeof payload.version === 'string' && payload.version.trim() ? payload.version.trim() : (server.version || 'unknown-legacy');
  // Version-mismatch detection
  if (LOCAL_GIT_VERSION && server.version && server.version !== 'unknown-legacy' && server.version !== LOCAL_GIT_VERSION) {
    if (!server.versionMismatchSince) { server.versionMismatchSince = now; }
    const mismatchAge = now - server.versionMismatchSince;
    if (mismatchAge > 300_000) { // >5 minutes
      console.warn(`[version] server '${serverId}' version mismatch: remote=${server.version} local=${LOCAL_GIT_VERSION} (${Math.round(mismatchAge / 60_000)}m)`);
    }
  } else {
    if (server.versionMismatchSince) { server.versionMismatchSince = null; }
  }
  server.sessions = sessions;
  server.agents = liveAgents;
  server.agentCount = liveAgents.length;

  if (!wasOnline) {
    resolveServerOfflineAlert(serverId);
  }
  if (lease.takeover) {
    emitSystemInfo(
      `Remote server '${serverId}' heartbeat instance switched`,
      `Server '${serverId}' lease takeover: reason=${lease.reason}, instanceId=${incomingInstanceId || 'unknown'}, bootTs=${incomingBootTs || 0}.`,
      'server_takeover',
      { dedupeKey: `server_takeover:${serverId}` }
    );
  }

  let agentsChanged = false;
  const becameOnline = [];
  for (const name of liveSet) {
    const ensured = ensureAgentRecord(name, {
      server: serverId,
      tmux: `${name}:0.0`,
      online: true,
      type: 'agent',
      kind: 'agent',
      offlineReason: null,
      registeredAt: now,
    });
    if (!ensured) continue;
    const agent = ensured.agent;
    if (ensured.created) {
      agentsChanged = true;
      // Adopting a session as an agent was completely silent, so a tmux session
      // that had nothing to do with Hagency became a permanent agent record with
      // no trace of when or why. A throwaway session created to check something
      // by hand was adopted within one heartbeat and outlived the session itself.
      // Say so, and record it where an operator will actually see it.
      console.warn(`[backend] adopted tmux session as a new agent: ${name} (server=${serverId})`);
      emitSystemInfo(
        `Adopted tmux session '${name}' as an agent`,
        `Server '${serverId}' reported session '${name}', which had no agent record, so one was created. `
          + 'Hagency can now type into that pane. If it does not belong to Hagency, add it to '
          + 'HAGENCY_SESSION_DENYLIST (or set HAGENCY_SESSION_ALLOWLIST) and remove the record with '
          + 'bin/hagency-prune-agents.',
        'agent_adopted',
        { dedupeKey: `agent_adopted:${serverId}:${name}` }
      );
    }
    if (!isAgentRecord(agent)) {
      agent.kind = 'agent';
      if (!Number(agent.registeredAt)) agent.registeredAt = now;
      agentsChanged = true;
    }
    if (normalizeServer(agent.server) !== serverId) { agent.server = serverId; agentsChanged = true; }
    // Never fabricate a pane target for a paneless agent. This backfill exists so a
    // tmux agent that registered without one still gets swept, but it ran before the
    // ACP guard further down and so handed every ACP agent a tmux target it does not
    // have. The dashboard then routed it as a tmux agent and asked getPaneIdleMs for a
    // pane that cannot exist, which reported idleMs -1 forever while the agent was
    // plainly reporting activity.
    if (!agent.tmux && agentTransport(agent) !== 'acp') { agent.tmux = `${name}:0.0`; agentsChanged = true; }
    const wasAgentOnline = agent.online === true;
    const runtime = ensureAgentRuntimeRecord(name);
    // Drive online/manualDown through machine
    syncAgentMachine(name, {
      heartbeatPresent: true,
      manualDown: false,
      mcpPresent: runtime?.mcpPresent === true ? true : (runtime?.mcpPresent === false ? false : undefined),
    });
    if (!wasAgentOnline && agent.online) becameOnline.push(name);
    if (agent.online !== wasAgentOnline) agentsChanged = true;
    // offlineReason (non-machine field)
    const mcpMissing = runtime?.mcpPresent === false;
    if (!mcpMissing) {
      if (agent.offlineReason !== null) { agent.offlineReason = null; agentsChanged = true; }
    } else if (agent.offlineReason !== 'mcp-missing:auto') {
      agent.offlineReason = 'mcp-missing:auto';
      agentsChanged = true;
    }
    if (agent.lastSeen !== now) { agent.lastSeen = now; agentsChanged = true; }
  }

  for (const agent of Object.values(agents)) {
    if (normalizeServer(agent.server) !== serverId) continue;
    if (liveSet.has(agent.name)) continue;
    // The relay builds this list by enumerating tmux sessions, so a paneless ACP
    // agent is never in it. Treating that absence as heartbeat-missing marked
    // every ACP agent offline within one beat, undoing what the sweep had just
    // concluded from its live process. The sweep owns liveness for these.
    if (agentTransport(agent) === 'acp') continue;
    const wasOnline = agent.online === true;
    const wasManualDown = agent.manualDown === true;
    const reason = `heartbeat-missing:${serverId}`;
    if (agent.offlineReason !== reason) { agent.offlineReason = reason; agentsChanged = true; }
    if (agent.tmux !== null) { agent.tmux = null; agentsChanged = true; }
    // online/manualDown driven by machine
    syncAgentMachine(agent.name, { heartbeatMissing: true });
    if (agent.online !== wasOnline || agent.manualDown !== wasManualDown) agentsChanged = true;
    if (wasOnline && !wasManualDown) {
      maybeEmitUnexpectedOfflineAlert(agent.name, reason, { server: serverId, detail: 'Missing in remote heartbeat snapshot' });
    }
  }

  saveServers();
  if (agentsChanged) saveAgents();
  for (const name of becameOnline) {
    notifyAgentCatchup(name, `online:${serverId}`);
  }
  return { ok: true, leaseAccepted: true, leaseReason: lease.reason };
}

// ── Push notification relay ───────────────────────────────────────────
async function collectLocalMcpSessionsAsync(paneMetadataSnapshot = null) {
  try {
    const ptsMap = (paneMetadataSnapshot && paneMetadataSnapshot.ttyToSession instanceof Map)
      ? paneMetadataSnapshot.ttyToSession
      : (await buildLocalPaneMetadataSnapshotAsync()).ttyToSession;
    if (!ptsMap.size) return new Set();
    let pids;
    try {
      const { stdout } = await execFileAsync('pgrep', ['-f', 'node.*mcp-server.js'], { timeout: 3000, encoding: 'utf-8' });
      pids = stdout.trim().split('\n').filter(Boolean);
    } catch {
      return new Set();
    }
    if (!pids.length) return new Set();
    const matched = new Set();
    try {
      const { stdout } = await execFileAsync('ps', ['-o', 'pid=,tty=', '-p', pids.join(',')], {
        timeout: 3000,
        encoding: 'utf-8',
      });
      const psOut = stdout.trim();
      if (!psOut) return matched;
      for (const line of psOut.split('\n')) {
        const parts = line.trim().split(/\s+/, 2);
        if (parts.length < 2) continue;
        const pts = parts[1].trim();
        if (!pts || pts === '?') continue;
        const session = ptsMap.get(pts) || null;
        if (session) matched.add(session);
      }
    } catch {
      return new Set();
    }
    return matched;
  } catch {
    return new Set();
  }
}

async function getLocalMcpSessionSetAsync(forceRefresh = false, paneMetadataSnapshot = null) {
  const now = Date.now();
  if (!forceRefresh && (now - localMcpSessionCacheAt) <= LOCAL_MCP_SESSION_CACHE_TTL_MS) {
    return localMcpSessionCache;
  }
  localMcpSessionCache = await collectLocalMcpSessionsAsync(paneMetadataSnapshot);
  localMcpSessionCacheAt = now;
  return localMcpSessionCache;
}

async function agentHasMcpAsync(agentName) {
  if (!agentName) return false;
  const sessions = await getLocalMcpSessionSetAsync(false);
  return sessions.has(agentName);
}

async function localTmuxSessionExistsAsync(sessionName) {
  if (!hostRuntime.capabilities.sessions) return false;
  return hostRuntime.sessionExists(sessionName);
}

const mergedPushInboxCursor = new Map();
const catchupCursor = new Map();
const catchupPushCursor = new Map();
const pushNotifySkipLog = new Map();
const SYSTEM_CATCHUP_SCHEMA_KIND = 'system_catchup';
const SYSTEM_TASK_ASSIGNED_SCHEMA_KIND = 'system_task_assigned';

function isSystemCatchupMessage(msg) {
  return normalizeOptionalText(msg?.schema?.kind, 128) === SYSTEM_CATCHUP_SCHEMA_KIND;
}

function catchupKeyFromMessage(msg) {
  if (!isSystemCatchupMessage(msg)) return null;
  const latestId = normalizeOptionalText(msg?.schema?.payload?.latestId, 256);
  const sourceUnreadCount = Number(msg?.schema?.payload?.sourceUnreadCount || 0);
  if (!latestId || !Number.isFinite(sourceUnreadCount) || sourceUnreadCount <= 0) return null;
  return `${latestId}:${sourceUnreadCount}`;
}

function findCatchupMessageForKey(agentName, key) {
  const normalizedAgent = normalizeAgentName(agentName);
  if (!normalizedAgent || !key) return null;
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i];
    if (msg?.to !== normalizedAgent) continue;
    if (catchupKeyFromMessage(msg) === key) return msg;
  }
  return null;
}

function pushNotifyStatus({ queued = false, terminal = false, deduped = false, reason = null } = {}) {
  return {
    ok: queued || terminal || deduped,
    queued,
    terminal,
    deduped,
    reason,
  };
}

function isCatchupNotificationComplete(result) {
  return result?.queued === true || result?.terminal === true || result?.deduped === true;
}

function logPushNotifySkip(agentName, reason, detail = '') {
  const key = `${agentName}:${reason}`;
  const now = Date.now();
  const prev = pushNotifySkipLog.get(key) || 0;
  if ((now - prev) < 30_000) return;
  pushNotifySkipLog.set(key, now);
  const suffix = detail ? ` ${detail}` : '';
  console.log(`[push-notify] skip ${agentName}: ${reason}${suffix}`);
}

function clearQueuedNotificationsForAgent(agentName) {
  if (!agentName) return;
  clearDeliveryQueueNotifications(agentName)
    .then((r) => {
      if (!r.ok) {
        console.warn(`[push-notify] queue clear failed for ${agentName}: status ${r.status}`);
      }
    })
    .catch((e) => {
      console.warn(`[push-notify] queue clear failed for ${agentName}: ${e.message}`);
    });
}

async function notifyAgentCatchup(agentName, reason = 'online') {
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return;
  const state = getAgentDeliveryState(agentName);
  if (!state.online) return;

  const { unread: rawUnread } = getUnreadInboxMessages(agentName);
  const unread = rawUnread.filter((msg) => !isSystemCatchupMessage(msg));
  if (!unread.length) return;

  const oldest = unread[0];
  const latest = unread[unread.length - 1];
  const key = `${latest.id}:${unread.length}`;
  if (catchupCursor.get(agentName) === key && catchupPushCursor.get(agentName) === key) return;
  const existingCatchup = findCatchupMessageForKey(agentName, key);
  if (existingCatchup) {
    catchupCursor.set(agentName, key);
    const result = await pushNotify(agentName, existingCatchup);
    if (isCatchupNotificationComplete(result)) catchupPushCursor.set(agentName, key);
    return;
  }
  if (catchupCursor.get(agentName) === key) {
    catchupCursor.delete(agentName);
    catchupPushCursor.delete(agentName);
  }

  const senderNames = [...new Set(unread.map(m => m.from).filter(Boolean))];
  const summary = `Queued while offline: ${unread.length} message(s) (${new Date(oldest.ts).toISOString()} -> ${new Date(latest.ts).toISOString()}).`;
  const replayLimit = Math.max(1, OFFLINE_CATCHUP_LIST_LIMIT);
  const replay = unread.slice(-replayLimit);
  const omitted = Math.max(0, unread.length - replay.length);
  const replayLines = replay.map((m, idx) => {
    const summaryText = String(m.summary || '').replace(/\s+/g, ' ').trim() || '(no summary)';
    const channel = m.group ? `group:${m.group}` : 'dm';
    return `${idx + 1}. [${new Date(m.ts).toISOString()}] (${channel}/${m.type}) ${m.from}: ${summaryText}`;
  });
  const full = [
    `You were offline (${reason}).`,
    `Unread count: ${unread.length}`,
    `Window: ${new Date(oldest.ts).toISOString()} -> ${new Date(latest.ts).toISOString()}`,
    `Senders: ${senderNames.join(', ') || 'unknown'}`,
    `Offline replay list (latest ${replay.length}):`,
    ...replayLines,
    omitted > 0 ? `... ${omitted} older message(s) omitted from replay list.` : null,
    'These messages may be time-sensitive. Review timestamps and decide whether a reply is still needed.',
    'check_inbox() returns per-message time fields: ts / at / time.',
    'FIRST ACTION: call check_inbox() now. Use check_inbox() in hagency MCP for full context before acting.',
  ].filter(Boolean).join('\n');

  const idReservation = reserveNextMsgId();
  if (!idReservation.ok) {
    console.warn(`[catchup] failed to reserve message id for ${agentName}: ${idReservation.error || 'unknown error'}`);
    return;
  }

  const msg = {
    id: idReservation.id,
    ts: Date.now(),
    from: 'system',
    to: agentName,
    group: null,
    type: 'inform',
    priority: 'normal',
    summary,
    full,
    mentions: [],
    reply_to: null,
    source: 'system',
    viewToken: createMessageViewToken(),
    schema: {
      kind: SYSTEM_CATCHUP_SCHEMA_KIND,
      version: 1,
      payload: {
        reason,
        sourceUnreadCount: unread.length,
        sourceUnreadIds: unread.map((m) => m.id).filter(Boolean),
        oldestId: oldest.id,
        latestId: latest.id,
      },
    },
  };
  const persisted = persistNewMessage(msg);
  if (!persisted.ok) {
    console.warn(`[catchup] failed to persist catchup message for ${agentName}: ${persisted.error || 'unknown error'}`);
    return;
  }
  appendDeliveryEvent({
    type: 'message.accepted',
    source: 'backend',
    messageId: msg.id,
    agent: agentName,
    targetAgents: [agentName],
    priority: msg.priority,
    context: {
      from: msg.from,
      to: msg.to,
      type: msg.type,
      reason,
      catchupUnreadCount: unread.length,
    },
  });
  broadcastSSE('message', { ...msg, deliveryOwner: 'dashboard-queue' });
  catchupCursor.set(agentName, key);
  const result = await pushNotify(agentName, msg);
  if (isCatchupNotificationComplete(result)) catchupPushCursor.set(agentName, key);
}

export function buildMcpReplyActionHint(msg, replyTo = null) {
  // Delegates to lib/reply-hint.js so the ACP host applies the same rule. It did
  // not: it told its agent to reply unconditionally, so one `hagency tell` task
  // produced silence from claude and codex and a message from octos.
  return buildReplyHint(msg, replyTo);
}

async function pushNotify(agentName, msg, options = {}) {
  const agent = agents[agentName];
  // An ACP agent has no pane, and its session is held by a separate host process
  // (scripts/hagency-acp-agent.mjs) that this process cannot reach into. So the
  // backend does not push to it — the host pulls, by polling the same inbox
  // endpoint check_inbox uses, and prompts the agent over session/prompt.
  //
  // Reporting this as 'missing-tmux-target' was wrong twice over: it read as a
  // broken tmux agent, and pushNotifyStatus turned it into ok:false while
  // POST /api/messages still answered {"ok":true,"warnings":[]}. A message to an
  // ACP agent looked sent, was never delivered, and nothing said otherwise.
  // 'acp-pull-pending' is terminal-but-fine: the backend's obligation ends here.
  if (!agent?.tmux && agentTransport(agent) === 'acp') {
    appendDeliveryEvent({
      type: 'push.pull_pending',
      source: 'backend',
      messageId: msg?.id,
      messageIds: msg?.id ? [msg.id] : [],
      agent: agentName,
      reason: 'acp-pull-pending',
    });
    return pushNotifyStatus({ terminal: true, reason: 'acp-pull-pending' });
  }
  if (!agent?.tmux) {
    logPushNotifySkip(agentName, 'missing-tmux-target');
    appendDeliveryEvent({
      type: 'push.not_queued',
      source: 'backend',
      messageId: msg?.id,
      messageIds: msg?.id ? [msg.id] : [],
      agent: agentName,
      reason: 'missing-tmux-target',
    });
    return pushNotifyStatus({ reason: 'missing-tmux-target' });
  }
  const agentServer = normalizeServer(agent.server);
  if (agentServer && !isLocalAgentServer(agentServer, LOCAL_SERVER_ID)) {
    logPushNotifySkip(agentName, 'remote-relay-expected', `(server=${agentServer})`);
    appendDeliveryEvent({
      type: 'push.not_queued',
      source: 'backend',
      messageId: msg?.id,
      messageIds: msg?.id ? [msg.id] : [],
      agent: agentName,
      target: agent.tmux,
      reason: 'remote-relay-expected',
      context: { server: agentServer },
    });
    return pushNotifyStatus({ terminal: true, reason: 'remote-relay-expected' });
  }
  // If server is unknown (null), verify the tmux session exists locally before queueing
  if (!agentServer) {
    const sess = agent.tmux.split(':')[0];
    const hasSession = await localTmuxSessionExistsAsync(sess);
    if (!hasSession) {
      logPushNotifySkip(agentName, 'local-session-not-found', `(tmux=${agent.tmux})`);
      appendDeliveryEvent({
        type: 'push.not_queued',
        source: 'backend',
        messageId: msg?.id,
        messageIds: msg?.id ? [msg.id] : [],
        agent: agentName,
        target: agent.tmux,
        reason: 'local-session-not-found',
      });
      // The tmux session may belong to a remote agent before heartbeat attribution catches up.
      return pushNotifyStatus({ reason: 'local-session-not-found' });
    }
  }
  const isHumanMsg = msg.type === 'human';
  const hasMcp = await agentHasMcpAsync(agentName);
  const { inboxTs, unread: rawUnread } = getUnreadInboxMessages(agentName);
  const unread = isSystemCatchupMessage(msg)
    ? rawUnread.filter((item) => !isSystemCatchupMessage(item))
    : rawUnread;
  const unreadCount = unread.length;
  const latestUnread = unread[unread.length - 1] || msg;
  const unreadMessageIds = unread.map((item) => item?.id).filter(Boolean);
  const pushSourceMsgId = latestUnread?.id || msg?.id || null;
  const pushMessageIds = unreadMessageIds.length ? unreadMessageIds : (msg?.id ? [msg.id] : []);
  const replyTo = latestUnread.from || msg.from;
  const notificationPriority = unreadCount > 1 ? highestMessagePriority(unread) : normalizeMessagePriority(msg?.priority);

  // Determine if reply is expected based on message type
  const needsReply = msg.type === 'human' || msg.type === 'request';
  let notificationKind = 'single_inform';
  let requiresInboxCheck = false;
  let hasHumanUnread = false;
  let hasRequestUnread = false;

  let notification;
  let mergedDedupeKeyToCommit = null;
  if (unreadCount > 1) {
    const dedupeKey = `${inboxTs}:${latestUnread.id || 'none'}:${unreadCount}`;
    if (!isHumanMsg && mergedPushInboxCursor.get(agentName) === dedupeKey) {
      return pushNotifyStatus({ deduped: true, reason: 'merged-unread-deduped' });
    }
    mergedDedupeKeyToCommit = dedupeKey;

    const senderNames = [...new Set(unread.map(m => m.from).filter(Boolean))];
    const senderText = senderNames.length ? ` (from ${formatSenderList(senderNames)})` : '';
    const hasHuman = unread.some(m => m.type === 'human');
    const hasRequest = unread.some(m => m.type === 'request');
    const actionableUnread = hasHuman || hasRequest;
    hasHumanUnread = hasHuman;
    hasRequestUnread = hasRequest;
    notificationKind = actionableUnread ? 'merged_unread_actionable' : 'merged_unread_inform';
    requiresInboxCheck = hasMcp && actionableUnread;
    const hasOperatorHuman = unread.some(m => m.type === 'human' && m.trustLevel === 'operator');
    const hasNonMatrixHuman = unread.some(m => m.type === 'human' && m.source !== 'matrix');
    const humanHint = hasHuman
      ? (hasOperatorHuman || hasNonMatrixHuman ? ' This includes messages from your human operator.' : ' This includes human messages (via Matrix).')
      : '';
    const processHint = hasMcp
      ? ' FIRST ACTION: call check_inbox() now. Read ALL messages there before doing anything else. DO ALL JOBS before replying. After ALL WORK is done, send required replies.'
      : ' Read ALL messages first. DO ALL JOBS before replying. After ALL WORK is done, send required replies.';

    if (hasMcp) {
      notification = `[NOTIFICATION] FIRST ACTION: call check_inbox() now. You have ${unreadCount} unread messages${senderText}.${humanHint}${processHint}`;
    } else {
      notification = `[NOTIFICATION] You have ${unreadCount} unread messages${senderText}.${humanHint}${processHint}`;
    }
  } else {
    const isHuman = msg.type === 'human';
    const isGroup = !!msg.group;
    const safeSummary = sanitizeForDisplay(msg.summary);
    const isMatrix = msg.source === 'matrix';
    const isOperator = msg.trustLevel === 'operator';
    const humanTag = isHuman ? (isMatrix && !isOperator ? ' (via Matrix)' : ' (human)') : '';
    const operatorHint = isHuman && (isOperator || !isMatrix) ? ' This is your human operator.' : '';

    if (hasMcp) {
      const checkHint = `FIRST ACTION: call check_inbox() now. Use check_inbox() in hagency MCP for full context before acting.`;
      const actionHint = needsReply ? buildMcpReplyActionHint(msg, replyTo) : null;
      notificationKind = needsReply ? 'single_actionable' : 'single_inform';
      requiresInboxCheck = needsReply;
      notification = isHuman
        ? `[NOTIFICATION] From ${msg.from}${humanTag}: "${safeSummary}".${operatorHint} ${checkHint} ${actionHint}.`
        : needsReply
          ? `[NOTIFICATION] From ${msg.from}: "${safeSummary}". ${checkHint} ${actionHint}.`
          : `[NOTIFICATION] From ${msg.from}: "${safeSummary}".`;
    } else {
      const senderAgent = agents[replyTo];
      const senderTmux = senderAgent?.tmux || `${replyTo}:0.0`;
      let actionHint;
      if (needsReply) {
        actionHint = `Reply after ALL WORK is done, using /agent-message skill or: hagency-send ${senderTmux} "<your reply>"`;
      }
      notificationKind = needsReply ? 'single_actionable' : 'single_inform';
      requiresInboxCheck = false;
      notification = isHuman
        ? `[NOTIFICATION] From ${msg.from}${humanTag}: "${safeSummary}".${operatorHint} ${actionHint}.`
        : needsReply
          ? `[NOTIFICATION] From ${msg.from}: "${safeSummary}". ${actionHint}.`
          : `[NOTIFICATION] From ${msg.from}: "${safeSummary}".`;
    }
  }

  try {
    const notifyMeta = {
      kind: notificationKind,
      priority: notificationPriority || 'normal',
      requiresInboxCheck,
      sourceMsgId: pushSourceMsgId,
      messageIds: pushMessageIds,
      unreadCount,
      hasHumanUnread,
      hasRequestUnread,
      needsReply,
      hasMcp,
    };
    const resp = await postToDeliveryQueue(
      { from: 'hagency-backend', to: agent.tmux, payload: notification, priority: notificationPriority || 'normal', notifyMeta },
      options.idempotencyKey || '',
    );
    if (resp.ok) {
      const body = await resp.json().catch(() => ({}));
      appendDeliveryEvent({
        type: 'push.queued',
        source: 'backend',
        messageId: notifyMeta.sourceMsgId,
        messageIds: notifyMeta.messageIds,
        agent: agentName,
        target: agent.tmux,
        queueEntryId: body?.id,
        queuedAt: body?.queuedAt,
        priority: notificationPriority || 'normal',
        notifyMeta,
      });
      markAgentPushNotified(agentName, {
        queueEntryId: body?.id,
        queuedAt: body?.queuedAt,
        ...notifyMeta,
      });
      if (mergedDedupeKeyToCommit) mergedPushInboxCursor.set(agentName, mergedDedupeKeyToCommit);
      return pushNotifyStatus({ queued: true });
    } else {
      appendDeliveryEvent({
        type: 'push.queue_failed',
        source: 'backend',
        messageId: notifyMeta.sourceMsgId,
        messageIds: notifyMeta.messageIds,
        agent: agentName,
        target: agent.tmux,
        priority: notificationPriority || 'normal',
        reason: `status-${resp.status}`,
        status: resp.status,
        notifyMeta,
      });
      return pushNotifyStatus({ reason: `status-${resp.status}` });
    }
  } catch (e) {
    appendDeliveryEvent({
      type: 'push.queue_failed',
      source: 'backend',
      messageId: pushSourceMsgId,
      messageIds: pushMessageIds,
      agent: agentName,
      target: agent.tmux,
      priority: notificationPriority || 'normal',
      reason: e?.message || 'queue request failed',
    });
    console.error(`Push notify failed for ${agentName}:`, e.message);
    return pushNotifyStatus({ reason: e?.message || 'queue request failed' });
  }
}

// ── Express app ───────────────────────────────────────────────────────
const app = express();
app.set('trust proxy', 'loopback');  // trust nginx on localhost, use X-Forwarded-For for real IP
const API_TOKEN = process.env.API_TOKEN;
app.use((req, res, next) => {
  // Skip global JSON parser for large-upload routes (they have route-specific limits).
  if (req.method === 'POST' && (req.path.endsWith('/avatar') || req.path === '/api/media/stage')) return next();
  express.json({ limit: '100kb' })(req, res, next);
});
app.use('/api', (req, res, next) => {
  const origin = req.headers.origin;
  if (CORS_ALLOWED_ORIGIN && origin === CORS_ALLOWED_ORIGIN) {
    res.setHeader('Access-Control-Allow-Origin', CORS_ALLOWED_ORIGIN);
    res.setHeader('Vary', 'Origin');
  }
  res.setHeader('Access-Control-Allow-Methods', 'GET,POST,PATCH,DELETE,OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Authorization,Content-Type');
  if (req.method === 'OPTIONS') return res.status(204).end();
  return next();
});
app.use('/api', createApiAuthMiddleware({
  apiToken: API_TOKEN,
  isLocalRequest,
}));

app.post('/api/delivery-events', (req, res) => {
  const result = appendDeliveryEvent({
    ...(req.body || {}),
    source: normalizeOptionalText(req.body?.source, 128) || 'external',
  });
  if (!result.ok) return res.status(400).json({ error: result.error || 'invalid delivery event' });
  res.json({ ok: true, event: result.event });
});

// ── Health ────────────────────────────────────────────────────────────
app.get('/health', (_req, res) => {
  refreshServerLiveness();
  const serverRows = Object.values(servers);
  const onlineServers = serverRows.filter(s => s.online).length;
  const agentNames = Object.keys(agents).filter(name => isAgentRecord(agents[name]));
  const onlineAgents = agentNames.filter(name => getAgentDeliveryState(name).online).length;
  const agentTokenReadiness = buildAgentTokenReadiness();
  const serverCredentialReadiness = buildServerCredentialReadiness();
  res.json({
    ok: true,
    agents: agentNames.length,
    onlineAgents,
    servers: serverRows.length,
    onlineServers,
    messages: messages.length,
    auth: {
      agentTokens: agentTokenReadiness,
      serverCredential: serverCredentialReadiness,
    },
    health: buildFlowHealth({
      serverRows,
      agentNames,
      agentTokenReadiness,
      serverCredentialReadiness,
      heartbeatTtlMs: HEARTBEAT_TTL_MS,
      isServerInMaintenance,
      getAgentDeliveryState,
      getAgentRuntime: (name) => agentRuntime[name],
      getAgentRuntimeRows: () => Object.values(agentRuntime).filter(row => row && typeof row === 'object'),
      listAlerts: () => alertStore.dump(),
      readDeliveryEvents,
      messageCount: messages.length,
    }),
  });
});

// ── Supervisor audit (v2 — per-agent supervisor snapshot store) ───────
const _tokenFromSupervisorTarget = r => `supervisor-${r.params?.target || ''}`;
function respondSupervisorSnapshotError(res, error, fallbackMessage) {
  if (error.code === 'snapshot_persistence_failed') return res.status(503).json({ error: error.message });
  if (error.code) return res.status(400).json({ error: error.message });
  return res.status(500).json({ error: fallbackMessage });
}

app.patch('/api/supervisor-state/:target', requireAgentToken(_tokenFromSupervisorTarget), (req, res) => {
  const target = normalizeAgentName(req.params.target);
  if (!target) return res.status(400).json({ error: 'invalid target agent name' });
  const supervisorName = `supervisor-${target}`;
  // Require supervisor agent to be registered
  if (!isAgentRecord(agents[supervisorName])) {
    return res.status(403).json({ error: `supervisor agent '${supervisorName}' is not registered` });
  }
  // Fail closed: require token to be provisioned (not just registered)
  if (!agentTokens.get(supervisorName)) {
    return res.status(403).json({ error: `supervisor agent '${supervisorName}' has no token provisioned` });
  }

  try {
    // Renew lease BEFORE assessment so lifecycleState is accurate
    supervisorSnapshotStore.renewLease(target, supervisorName);
    const { snapshot, event } = supervisorSnapshotStore.updateAssessment(target, supervisorName, req.body || {});
    broadcastSSE('supervisor_audit', event);
    supervisorActionEngine.evaluateAction(target, snapshot);
    return res.json({ ok: true, snapshot });
  } catch (error) {
    return respondSupervisorSnapshotError(res, error, 'failed to update supervisor state');
  }
});

app.post('/api/supervisor-state/:target/heartbeat', requireAgentToken(_tokenFromSupervisorTarget), (req, res) => {
  const target = normalizeAgentName(req.params.target);
  if (!target) return res.status(400).json({ error: 'invalid target agent name' });
  const supervisorName = `supervisor-${target}`;
  if (!isAgentRecord(agents[supervisorName])) {
    return res.status(403).json({ error: `supervisor agent '${supervisorName}' is not registered` });
  }
  if (!agentTokens.get(supervisorName)) {
    return res.status(403).json({ error: `supervisor agent '${supervisorName}' has no token provisioned` });
  }
  supervisorSnapshotStore.renewLease(target, supervisorName);
  return res.json({ ok: true, target, leaseRenewed: true });
});

app.get('/api/supervisor/status', (_req, res) => {
  res.json(supervisorSnapshotStore.getStatus(agents));
});

app.get('/api/supervisor/agents', (_req, res) => {
  // Iterate live candidate set, enrich with snapshot store
  const agentList = Object.values(agents).filter(isAgentRecord);
  const summaries = agentList.map(a => {
    const snapshot = supervisorSnapshotStore.getTarget(a.name);
    return {
      name: a.name,
      online: a.online,
      task: a.task || null,
      state: snapshot ? {
        lastStatus: snapshot.state,
        classification: snapshot.classification,
        consecutiveNegative: snapshot.consecutiveNegative,
        lastReason: snapshot.reason,
        lastJudgedAt: snapshot.assessed_at_ms || null,
        lifecycleState: snapshot.lifecycleState,
      } : null,
    };
  });
  res.json({
    status: supervisorSnapshotStore.getStatus(agents),
    agents: summaries,
  });
});

app.get('/api/supervisor/agents/:name', (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  if (!isAgentRecord(agents[agentName])) return res.status(404).json({ error: 'agent not found' });
  const limitRaw = Number.parseInt(req.query.limit, 10);
  const limit = Number.isFinite(limitRaw) && limitRaw > 0 ? Math.min(limitRaw, 500) : 120;
  const snapshot = supervisorSnapshotStore.getTarget(agentName);
  const agentEvents = supervisorSnapshotStore.getEvents(agentName, limit);
  const agent = agents[agentName];
  return res.json({
    name: agentName,
    task: agent.task || null,
    state: snapshot ? {
      lastStatus: snapshot.state,
      classification: snapshot.classification,
      consecutiveNegative: snapshot.consecutiveNegative,
      lastReason: snapshot.reason,
      lastDomain: snapshot.domain,
      lastPattern: snapshot.pattern,
      lastSuggestion: snapshot.suggested_action,
      lastJudgedAt: snapshot.assessed_at_ms || null,
      lastWarningAt: snapshot.lastWarningAt,
      lastNudgeAt: snapshot.lastNudgeAt,
      lastNudgeCount: snapshot.lastNudgeCount,
      lastEscalationAt: snapshot.lastEscalationAt,
      lastEscalationCount: snapshot.lastEscalationCount,
      lastEventId: snapshot.lastEventId,
      lifecycleState: snapshot.lifecycleState,
    } : null,
    latest: agentEvents.length ? agentEvents[agentEvents.length - 1] : null,
    events: agentEvents,
  });
});

app.get('/api/supervisor/control', (_req, res) => {
  return res.json(supervisorSnapshotStore.getControl(agents));
});

app.post('/api/supervisor/control', requireBearer, (req, res) => {
  const body = req.body || {};

  if (Object.prototype.hasOwnProperty.call(body, 'allowedAgents')) {
    return res.status(400).json({
      error: 'allowedAgents is read-only in per-agent supervisor model — provision/deprovision supervisor agents to change membership',
    });
  }

  if (!Object.prototype.hasOwnProperty.call(body, 'enabled')) {
    return res.status(400).json({ error: 'no control fields provided' });
  }

  if (typeof body.enabled !== 'boolean') {
    return res.status(400).json({ error: 'enabled must be boolean' });
  }

  try {
    supervisorSnapshotStore.setEnabled(body.enabled);
    if (body.enabled) {
      try { supervisorLifecycleManager.sweepAll(); } catch (_) { /* best-effort */ }
    }
    const control = supervisorSnapshotStore.getControl(agents);
    const status = supervisorSnapshotStore.getStatus(agents);
    auditLog(req, { summary: { enabled: body.enabled } });
    return res.json({ ok: true, control, status });
  } catch (error) {
    return respondSupervisorSnapshotError(res, error, 'failed to update supervisor control');
  }
});

// ── Server heartbeats ─────────────────────────────────────────────────
app.post('/api/servers/heartbeat', requireBearer, (req, res) => {
  const serverId = normalizeServer(req.body?.server);
  if (!serverId) return res.status(400).json({ error: 'server required' });
  if (!isLocalRequest(req) && isLocalAgentServer(serverId, LOCAL_SERVER_ID)) {
    return res.status(400).json({
      error: 'remote server id must not be local',
      server: serverId,
    });
  }
  const heartbeatResult = applyServerHeartbeat(serverId, req.body || {}, req.ip || req.connection?.remoteAddress || null);
  refreshServerLiveness();
  const state = servers[serverId];
  auditLog(req, { summary: { server: serverId, agents: state?.agentCount || 0 } });
  const maintenance = isServerInMaintenance(serverId, state);
  if (heartbeatResult && heartbeatResult.leaseAccepted === false) {
    return res.status(409).json({
      ok: false,
      error: 'heartbeat_lease_rejected',
      reason: heartbeatResult.leaseReason || 'unknown',
      server: {
        id: state.id,
        online: Boolean(state.online),
        lastSeen: state.lastSeen || null,
        updatedAt: state.updatedAt || null,
        agentCount: state.agentCount || 0,
        sourceIp: state.sourceIp || null,
        maintenance,
      },
    });
  }
  return res.json({
    ok: true,
    maintenance,
    ignored: heartbeatResult?.ignored === true,
    server: {
      id: state.id,
      online: Boolean(state.online),
      lastSeen: state.lastSeen || null,
      updatedAt: state.updatedAt || null,
      agentCount: state.agentCount || 0,
      sourceIp: state.sourceIp || null,
      maintenance,
    },
  });
});

app.post('/api/servers/:id/offline', requireBearer, (req, res) => {
  const serverId = normalizeServer(req.params.id);
  if (!serverId) return res.status(400).json({ error: 'server required' });
  const server = ensureServerRecord(serverId);
  const requestInstanceId = normalizeRelayInstanceId(req.body?.instanceId);
  const activeInstanceId = normalizeRelayInstanceId(server.relayInstanceId);
  if (requestInstanceId && activeInstanceId && requestInstanceId !== activeInstanceId && server.online) {
    return res.status(409).json({
      ok: false,
      error: 'offline_lease_rejected',
      reason: 'different-instance-active',
      activeInstanceId,
      requestInstanceId,
    });
  }
  const wasOnline = Boolean(server.online);
  const affectedAgents = collectServerAffectedAgents(serverId, server);
  const previousHeartbeatAt = Number(server.heartbeatAt) || 0;
  const previousLastSeen = Number(server.lastSeen) || 0;
  const now = Date.now();
  server.heartbeatAt = 0;
  clearServerLiveState(server, now);
  const maintenance = isServerInMaintenance(serverId, server);
  const reason = maintenance ? `server-maintenance:${serverId}` : `server-offline:${serverId}`;
  if (markAgentsOfflineForServer(serverId, reason, true)) saveAgents();
  saveServers();
  if (wasOnline && !maintenance) {
    emitServerOfflineAlert(
      serverId,
      'explicit-offline',
      { ...server, heartbeatAt: previousHeartbeatAt, lastSeen: previousLastSeen },
      affectedAgents
    );
  }
  res.json({
    ok: true,
    server: {
      id: serverId,
      online: false,
      maintenance,
      lastSeen: server.lastSeen,
    },
  });
});

app.post('/api/servers/:id/maintenance', requireBearer, (req, res) => {
  const serverId = normalizeServer(req.params.id);
  if (!serverId) return res.status(400).json({ error: 'server required' });
  const enabled = req.body?.enabled;
  if (typeof enabled !== 'boolean') {
    return res.status(400).json({ error: 'enabled boolean required' });
  }

  const server = ensureServerRecord(serverId);
  server.maintenance = enabled;
  let serversChanged = true;
  let agentsChanged = false;
  if (enabled) {
    const maintenance = enforceServerMaintenanceOffline(serverId, server, Date.now());
    if (maintenance.serverChanged) serversChanged = true;
    if (maintenance.agentsChanged) agentsChanged = true;
  } else {
    const now = Date.now();
    if ((Number(server.updatedAt) || 0) !== now) {
      server.updatedAt = now;
      serversChanged = true;
    }
  }

  if (serversChanged) saveServers();
  if (agentsChanged) saveAgents();
  refreshServerLiveness();

  const state = servers[serverId];
  const maintenance = isServerInMaintenance(serverId, state);
  return res.json({
    ok: true,
    server: {
      id: state.id,
      online: Boolean(state.online),
      maintenance,
      lastSeen: state.lastSeen || null,
      updatedAt: state.updatedAt || null,
      agentCount: state.agentCount || 0,
      sourceIp: state.sourceIp || null,
    },
  });
});

app.get('/api/servers', (_req, res) => {
  refreshServerLiveness();
  const rows = Object.values(servers)
    .map(s => ({
      id: s.id,
      online: Boolean(s.online),
      maintenance: isServerInMaintenance(s.id, s),
      lastSeen: s.lastSeen || null,
      heartbeatAt: Number(s.heartbeatAt) || null,
      updatedAt: s.updatedAt || null,
      agentCount: Number(s.agentCount) || 0,
      sourceIp: s.sourceIp || null,
      relayInstanceId: normalizeRelayInstanceId(s.relayInstanceId),
      relayBootTs: normalizeRelayBootTs(s.relayBootTs) || null,
      version: s.version || null,
    }))
    .sort((a, b) => (b.lastSeen || 0) - (a.lastSeen || 0));
  res.json(rows);
});

app.get('/api/servers/fleet', (req, res) => {
  const expectedVersion = normalizeOptionalText(req.query.expectVersion, 128)
    || normalizeOptionalText(req.query.expectedVersion, 128);
  res.json(buildFleetInventory({
    servers: Object.values(servers),
    expectedVersion,
    localGitVersion: LOCAL_GIT_VERSION,
    heartbeatTtlMs: HEARTBEAT_TTL_MS,
    isServerInMaintenance,
    normalizeRelayInstanceId,
    normalizeRelayBootTs,
  }));
});

// ── SSE endpoint ──────────────────────────────────────────────────────
sseAdapter.installRoute(app, '/api/stream');

/*
 * ── The tmux delivery queue, in-process ──────────────────────────────────────────────────────────
 *
 * Lifted whole from the retired portal's process (lib/delivery-queue.js says how and what changed).
 * The sinks below are the five places that used to be HTTP requests from one local process to the
 * other; each is now the same function the corresponding route calls, so in-process and over-the-wire
 * callers cannot diverge.
 */
initDeliveryQueue({
  logsRoot: path.join(RUNTIME_ROOT, 'logs'),
  idleThresholdMs: Number.parseInt(process.env.AGENT_IDLE_THRESHOLD_MS || '20000', 10),
  reminderMergePreviewLimit: Number.parseInt(process.env.REMINDER_MERGE_PREVIEW_LIMIT || '20', 10),
  emitDeliveryEvent: (row) => appendDeliveryEvent({ ...row, source: row?.source || 'dashboard-queue' }),
  recordPushDelivered: async (body) => {
    const { status, body: out } = recordPushDelivered(body ?? {});
    return { ok: status < 400, status, errText: status >= 400 ? JSON.stringify(out) : '' };
  },
  unreadSnapshot: async (agentName) => {
    const name = normalizeAgentName(agentName);
    if (!name || !isAgentRecord(agents[name])) return null;
    return buildUnreadInboxSnapshot(name, {});
  },
  offlineLocalTmuxSessions: async () => {
    const sessions = new Set();
    for (const a of Object.values(agents)) {
      if (!isAgentRecord(a) || !a.tmux || a.online !== false) continue;
      if (normalizeServer(a.server) !== 'local') continue;
      const sessionName = String(a.tmux).split(':')[0];
      if (sessionName) sessions.add(sessionName);
    }
    return sessions;
  },
  broadcast: (event, data) => broadcastSSE(event, data),
});

/*
 * NO ROUTE-LEVEL GUARD, and that is a finding, not an omission. The first version added a
 * `requireLocalOrBearer` middleware here — and the tests for its non-local branch came back 401 where
 * 403 was expected, because the backend's GLOBAL `/api` middleware (`createApiAuthMiddleware`, mounted
 * below) already implements exactly this posture: local requests pass, non-local requires the operator
 * bearer, everything passes when no API_TOKEN is configured. A second copy of that decision inside the
 * queue routes could only ever agree with the first or drift from it. The routes are therefore declared
 * `global-api-auth-only` in the boundary manifest, which is the policy that already exists for this
 * exact situation (GET /api/stream uses it).
 *
 * What DID change from the old process: the portal accepted HAGENCY_DASHBOARD_TOKEN from non-local
 * callers; that token died with the portal, and the operator bearer is the remote credential now.
 */
installDeliveryQueueRoutes(app);


// ── Owner-scoped runtime approvals ───────────────────────────────────
function respondApprovalStoreError(res, error, fallback = 'approval operation failed') {
  if (error instanceof ApprovalStoreError) {
    if (error.code === 'bad_request') return res.status(400).json({ error: error.message, code: error.code });
    if (error.code === 'conflict') return res.status(409).json({ error: error.message, code: error.code });
    if (error.code === 'persistence_failed') return res.status(503).json({ error: error.message, code: error.code });
  }
  return res.status(500).json({ error: error?.message || fallback });
}

const _tokenFromApprovalBody = req => req.body?.agent || '';
const _tokenFromApprovalRecord = req => approvalStore.getRequest(req.params?.id)?.agent || '';
const requireApprovalBridgeSecret = (req, res, next) => {
  if (!getBridgeSecret()) {
    return res.status(503).json({ error: 'MATRIX_BRIDGE_SECRET is required for approval authorization' });
  }
  return requireBridgeSecret(req, res, next);
};
function fleetPublicEngagement(engagement) {
  const agent = agents[engagement.agent];
  const context = engagement.requestContext;
  return { ok: true, v: 1, fleetId: context.fleetId, requestId: context.requestId,
    engagementId: engagement.id, state: engagement.state, targetProjectId: context.targetProjectId,
    targetRoomId: engagement.projectRoomId, sourceRoomId: context.sourceRoomId,
    sourceEventId: context.sourceEventId, role: engagement.role,
    ...(context.agentDefinition ? { agentDefinition: context.agentDefinition } : {}),
    requestedTokens: engagement.requestedTokens, allocatedTokens: engagement.allocatedTokens,
    agentMxid: isAgentRecord(agent) ? recordedAgentMxid(agent) : null,
    bound: engagement.bound === true, serving: servingConfiguration(engagement.agent),
    fulfillment: engagement.fulfillment ? { phase: engagement.fulfillment.phase,
      incomplete: Boolean(engagement.fulfillment.error),
      ...(engagement.fulfillment.error ? { error: 'Agent setup is incomplete; the provider must retry.' } : {}) } : null,
    ready: engagement.state === 'active' && engagement.bound === true,
    decidedAt: engagement.decidedAt ?? null, endedAt: engagement.endedAt ?? null };
}

// This is a local bridge authority endpoint, never exposed by the AS listener.
// The callback exports only four versioned operations and verifies Matrix facts
// before forwarding a request. Recheck the current registration at commit time.
app.post('/api/fleet-control', requireApprovalBridgeSecret, async (req, res) => {
  const input = req.body ?? {};
  const side = projectSideStore.getSide(input.sideId);
  const credential = side && projectSideStore.credentialFor(side.id);
  const fleetId = fleetIdForSender(credential?.senderLocalpart);
  const registration = side && credential?.hsToken ? inboundCredentialsProjection(side, credential)?.registration : null;
  if (!side?.active || credential?.kind !== 'appservice' || !fleetId || input.registration !== registration
    || credential.transport && input.transportGeneration !== credential.transport.generation) {
    return res.json({ ok: false, status: 403, code: 'fleet_unavailable', error: 'Fleet registration is unavailable.' });
  }
  try {
    if (input.action === 'capabilities') return res.json({ ok: true, offers: fleetCatalogOffers(side.id) });
    if (input.action === 'status') {
      const engagement = engagementStore.list().find(e => e.requestContext?.fleetId === fleetId
        && e.requestContext?.requestId === input.requestId && sideIdForRoom(e.projectRoomId) === side.id);
      if (!engagement) return res.json({ ok: false, status: 404, code: 'not_found', error: 'Fleet request was not found.' });
      return res.json(fleetPublicEngagement(engagement));
    }
    if (input.action !== 'request') return res.json({ ok: false, status: 404, code: 'not_found', error: 'Unknown fleet operation.' });
    const context = fleetRequestContext(input.context);
    if (context.fleetId !== fleetId || sideIdForRoom(context.targetRoomId) !== side.id
      || sideIdForRoom(context.sourceRoomId) !== side.id || sideIdForRoom(context.ownerDmRoomId) !== side.id) {
      return res.json({ ok: false, status: 403, code: 'wrong_fleet', error: 'Fleet request scope does not match.' });
    }
    if (typeof input.projectName === 'string' && input.projectName.trim()) {
      const existing = side.projects?.find(project => project.roomId === context.targetRoomId);
      const byId = side.projects?.find(project => project.id === context.targetProjectId);
      if (!existing && byId?.roomId && byId.roomId !== context.targetRoomId) {
        return res.json({ ok: false, status: 409, code: 'project_metadata_conflict', error: 'Project metadata belongs to another room.' });
      }
      projectSideStore.upsertProject(side.id, {
        id: existing?.id ?? context.targetProjectId,
        name: input.projectName.trim().slice(0, 255), roomId: context.targetRoomId,
      });
    }
    req.body = { project: context.targetProjectId, projectRoomId: context.targetRoomId,
      role: context.role, requester: context.requesterMxid, requestedTokens: context.requestedTokens,
      ratePerDay: context.ratePerDay, requestId: fleetRequestKey(context), requestContext: context };
    req.engagementCaller = 'fleet';
    req.fleetContext = context;
    const fleetResponse = {
      statusCode: 200,
      status(value) { this.statusCode = value; return this; },
      json(value) {
        if (this.statusCode >= 400 || value?.ok === false) return res.json({ ok: false,
          status: this.statusCode >= 400 ? this.statusCode : 409, code: value?.code ?? 'request_refused',
          error: value?.code === 'conflict' ? 'Request ID already belongs to another request.'
            : 'The provider could not accept this request; check role availability and allocation.' });
        return res.json(value);
      },
    };
    return createEngagementRequest(req, fleetResponse);
  } catch (error) {
    return res.json({ ok: false, status: error.status ?? 409, code: error.code ?? 'fleet_request_failed',
      error: error.status ? error.message : 'The fleet request could not be recorded.' });
  }
});
app.post('/api/matrix-work/claim', requireApprovalBridgeSecret, (req, res) => {
  try {
    // Intent was committed with the verdict. Materialize durable work here so a
    // crash after approval, before the HTTP response, cannot lose the receipt.
    for (const engagement of engagementStore.list({ state: 'active' })) {
      if (engagement.approvalNotice?.state !== 'pending' || !engagement.bound) continue;
      const sideId = sideIdForRoom(engagement.projectRoomId);
      const side = projectSideStore.getSide(sideId);
      const agent = agents[engagement.agent];
      if (!side?.active || !isAgentRecord(agent) || agent.projectSide !== sideId) continue;
      const mxid = recordedAgentMxid(agent) || `@${agentPrefixOnSide(side)}${agent.name}:${side.serverName}`;
      const queued = matrixWorkStore.enqueue({ action: 'engagement-approved', agent: agent.name, sideId,
        roomId: engagement.requestContext?.sourceRoomId ?? engagement.projectRoomId, mxid, engagementId: engagement.id,
        content: engagementApprovalContent(engagement, servingConfiguration(agent.name), mxid) });
      if (queued.state === 'complete') engagementStore.settleApprovalNotice(engagement.id,
        queued.outcome?.ok ? { eventId: queued.outcome.eventId } : { code: queued.outcome?.code });
    }
    const job = matrixWorkStore.claim();
    if (!job) return res.json({ job: null });
    const side = projectSideStore.getSide(job.sideId);
    const credential = side && projectSideStore.credentialFor(side.id);
    if (job.action === 'engagement-approved') {
      const e = engagementStore.get(job.engagementId);
      if (!side?.active || !['registrationToken', 'appservice'].includes(credential?.kind)
        || e?.state !== 'active' || !e.bound || e.agent !== job.agent
        || (e.requestContext?.sourceRoomId ?? e.projectRoomId) !== job.roomId
        || sideIdForRoom(job.roomId) !== side.id || !isAgentRecord(agents[job.agent])
        || agents[job.agent].projectSide !== side.id) {
        matrixWorkStore.complete(job.id, job.claimToken, { ok: false, code: 'engagement_notice_unavailable' }, { retry: false });
        engagementStore.settleApprovalNotice(job.engagementId, { code: 'engagement_notice_unavailable' });
        return res.json({ job: null });
      }
      res.set('Cache-Control', 'no-store');
      return res.json({ job: { ...job,
        side: { apiBaseUrl: side.apiBaseUrl, serverName: side.serverName, representative: side.representative },
        credential: credential.kind === 'appservice'
          ? { kind: credential.kind, asToken: credential.asToken, senderLocalpart: credential.senderLocalpart }
          : { kind: credential.kind, representativeToken: credential.representativeToken },
      } });
    }
    if (!side || !credential || credential.kind !== 'registrationToken'
      || (!side.active && !['leave', 'logout', 'representative-logout'].includes(job.action))
      || job.action !== 'representative-logout' && !isAgentRecord(agents[job.agent])
      || !['leave', 'logout', 'representative-logout'].includes(job.action) && job.engagementId && engagementStore.get(job.engagementId)?.state === 'ended') {
      matrixWorkStore.complete(job.id, job.claimToken, { ok: false, code: 'side_or_agent_unavailable' });
      return res.json({ job: null });
    }
    res.set('Cache-Control', 'no-store');
    return res.json({ job: { ...job, side: { apiBaseUrl: side.apiBaseUrl, serverName: side.serverName },
      credential: job.action === 'identity' ? { kind: credential.kind, registrationToken: credential.registrationToken }
        : job.action === 'representative-logout' ? { representativeToken: credential.representativeToken } : null } });
  } catch (error) { return res.status(503).json({ error: error.message }); }
});
app.post('/api/matrix-work/:id/complete', requireApprovalBridgeSecret, (req, res) => {
  try {
    const job = matrixWorkStore.complete(req.params.id, req.body?.claimToken, req.body?.outcome || {});
    if (job.action === 'engagement-approved' && job.state === 'complete') {
      engagementStore.settleApprovalNotice(job.engagementId, job.outcome?.ok
        ? { eventId: job.outcome.eventId } : { code: job.outcome?.code || 'approval_notice_delivery_failed' });
    }
    return res.json({ ok: true });
  } catch (error) { return res.status(409).json({ error: error.message }); }
});
app.get('/api/matrix-work', requireBearer, (_req, res) => res.json({ jobs: matrixWorkStore.list() }));
const requireRouterBridgeSecret = (req, res, next) => {
  if (!THREAD_SESSIONS_ENABLED) return res.status(404).json({ error: 'thread-session router is disabled' });
  if (!getBridgeSecret()) return res.status(503).json({ error: 'MATRIX_BRIDGE_SECRET is required for router outboxes' });
  return requireBridgeSecret(req, res, next);
};
const requireRouterBearer = (req, res, next) => {
  if (!routerStore) return res.status(404).json({ error: 'router is disabled' });
  if (!process.env.API_TOKEN) return res.status(503).json({ error: 'API_TOKEN is required for router operator endpoints' });
  return requireBearer(req, res, next);
};

function agentOpsStatus(result) {
  if (!result || result.ok !== false) return 500;
  if (result.code === 'not_found') return 404;
  if (result.code === 'device_enrollment_required' || result.code === 'idempotency_conflict'
      || result.code === 'capability_consumed' || result.code === 'precondition_failed'
      || result.code === 'inspection_required' || result.code === 'invalid_transition') return 409;
  if (result.code === 'capability_expired' || result.code === 'inspection_expired') return 410;
  if (result.code === 'invalid_capability' || result.code === 'auth_fence_stale'
      || result.code === 'scope_mismatch' || result.code === 'device_mismatch') return 401;
  return 400;
}

function sendAgentOpsError(res, status, code, error) {
  return res.status(status).json({
    schema: AGENT_OPS_CLIENT_SCHEMA,
    error,
    code,
  });
}

function respondAgentOps(res, result) {
  if (result?.ok) return false;
  sendAgentOpsError(
    res,
    agentOpsStatus(result),
    result?.code || 'internal_error',
    result?.message || 'Agent Operations request failed',
  );
  return true;
}

function requireAgentOpsEnabled(_req, res, next) {
  if (!AGENT_OPS_CLIENT_ENABLED || !agentOpsService || !agentOpsServerIdentity) {
    return res.status(404).json({ error: 'Agent Operations client contract is disabled' });
  }
  return next();
}

function requireAgentOpsLoopback(req, res, next) {
  const checked = checkAgentOpsLoopbackRequest(req, AGENT_OPS_LOOPBACK_ORIGIN);
  if (!checked.ok) return sendAgentOpsError(res, 403, checked.code, checked.message);
  req.agentOpsAudience = checked.audience;
  res.setHeader('Cache-Control', 'no-store');
  res.setHeader('Pragma', 'no-cache');
  return next();
}

function registeredAgentByStableId(stableAgentId) {
  return Object.values(agents).find((candidate) => (
    isAgentRecord(candidate) && candidate.agentId === stableAgentId
  )) || null;
}

function currentAgentOpsBinding(scope) {
  if (!scope) return null;
  const agent = registeredAgentByStableId(scope.stable_agent_id);
  if (!agent) return null;
  const bindings = approvalStore.listBindings({ agent: agent.name, projectRoomId: scope.project_room_id });
  if (bindings.length !== 1) return null;
  const binding = bindings[0];
  if (
    binding.ownerMxid !== scope.owner_mxid
    || binding.ownerDmRoomId !== scope.owner_dm_room_id
  ) return null;
  return { agent, binding };
}

function requireLiveAgentOpsBinding(auth) {
  const scope = agentOpsService.scope(auth.scopeId);
  const bound = currentAgentOpsBinding(scope);
  if (bound) return bound;
  if (scope) agentOpsService.revokeScope(scope.scope_id);
  return null;
}

function requireAgentOpsClientSession(req, res, next) {
  const clientSessionId = normalizeOptionalText(req.headers['x-agent-ops-client-session'], 255);
  const sessionCapability = parseAgentOpsSessionAuthorization(req.headers.authorization);
  const proofNonce = normalizeOptionalText(req.headers['x-agent-ops-proof-nonce'], 255);
  const proof = normalizeOptionalText(req.headers['x-agent-ops-proof'], 1024);
  if (!clientSessionId || !sessionCapability || !proofNonce || !proof) {
    return sendAgentOpsError(res, 401, 'invalid_capability', 'Agent Operations session capability and proof headers are required');
  }
  const descriptor = agentOpsService.sessionProofDescriptor({
    clientSessionId,
    sessionCapability,
    audience: req.agentOpsAudience,
  });
  if (respondAgentOps(res, descriptor)) return;
  if (!requireLiveAgentOpsBinding(descriptor.auth)) {
    return sendAgentOpsError(res, 401, 'auth_fence_stale', 'Agent Operations owner binding or registered agent changed');
  }
  const material = agentOpsSessionProofMaterial({
    clientSessionId,
    proofNonce,
    method: req.method,
    requestPath: req.originalUrl,
    body: req.method === 'GET' ? null : (req.body ?? null),
    audience: req.agentOpsAudience,
  });
  if (!verifyAgentOpsProof(descriptor.clientPublicJwk, proof, material)) {
    return sendAgentOpsError(res, 401, 'invalid_capability', 'Agent Operations proof of possession is invalid');
  }
  const consumed = agentOpsService.consumeRequestNonce({
    ...descriptor.auth,
    nonce: proofNonce,
    sessionCapability,
  });
  if (respondAgentOps(res, consumed)) return;
  req.agentOpsAuth = descriptor.auth;
  req.agentOpsSessionCapability = sessionCapability;
  return next();
}

function resolveAgentOpsScopeRequest(body) {
  const agentName = normalizeAgentName(body?.agent);
  const agent = agentName ? agents[agentName] : null;
  if (!isAgentRecord(agent) || !agent.agentId) {
    return { ok: false, status: 404, code: 'not_found', error: 'registered stable agent not found' };
  }
  const projectRoomId = normalizeOptionalText(body?.project_room_id, 255);
  const ownerMxid = normalizeOptionalText(body?.owner_mxid, 255);
  const ownerDmRoomId = normalizeOptionalText(body?.owner_dm_room_id, 255);
  if (!projectRoomId || !ownerMxid || !ownerDmRoomId) {
    return { ok: false, status: 400, code: 'bad_request', error: 'project_room_id, owner_mxid and owner_dm_room_id are required' };
  }
  const bindings = approvalStore.listBindings({ agent: agent.name, projectRoomId });
  if (bindings.length !== 1) {
    return { ok: false, status: 409, code: 'scope_mismatch', error: bindings.length === 0 ? 'owner binding not found' : 'owner binding is ambiguous' };
  }
  const binding = bindings[0];
  if (binding.ownerMxid !== ownerMxid || binding.ownerDmRoomId !== ownerDmRoomId) {
    return { ok: false, status: 403, code: 'scope_mismatch', error: 'requested owner scope does not match the current binding' };
  }
  return {
    ok: true,
    scope: {
      ownerMxid,
      ownerDmRoomId,
      projectRoomId,
      stableAgentId: agent.agentId,
      agentName: agent.name,
    },
    agent,
    binding,
  };
}

app.put('/api/agent-ops/v1/operator/device-enrollment', requireAgentOpsEnabled, requireRouterBearer, (req, res) => {
  if (!isLocalRequest(req)) return sendAgentOpsError(res, 403, 'loopback_required', 'Agent Operations device enrollment is local-only');
  const resolved = resolveAgentOpsScopeRequest(req.body);
  if (!resolved.ok) return sendAgentOpsError(res, resolved.status, resolved.code, resolved.error);
  const result = agentOpsService.enrollDevice({
    ...resolved.scope,
    matrixDeviceId: req.body?.matrix_device_id,
    matrixDeviceEd25519: req.body?.matrix_device_ed25519,
    matrixDeviceCurve25519: req.body?.matrix_device_curve25519,
  });
  if (respondAgentOps(res, result)) return;
  auditLog(req, {
    agent: resolved.agent.name,
    summary: { action: 'agent-ops-device-enrollment', scopeId: result.scopeId, fence: result.authFenceGeneration },
  });
  return res.json({
    ok: true,
    schema: AGENT_OPS_CLIENT_SCHEMA,
    scope_id: result.scopeId,
    projection_id: result.projectionId,
    auth_fence_generation: result.authFenceGeneration,
    idempotent_request_replay: result.replayed,
  });
});

app.post('/api/agent-ops/v1/operator/revoke', requireAgentOpsEnabled, requireRouterBearer, (req, res) => {
  if (!isLocalRequest(req)) return sendAgentOpsError(res, 403, 'loopback_required', 'Agent Operations revocation is local-only');
  const scopeId = normalizeOptionalText(req.body?.scope_id, 255);
  if (!scopeId) return sendAgentOpsError(res, 400, 'bad_request', 'scope_id is required');
  const result = agentOpsService.revokeScope(scopeId, req.body?.clear_device === true);
  if (respondAgentOps(res, result)) return;
  auditLog(req, { summary: { action: 'agent-ops-revoke', scopeId: result.scopeId, fence: result.authFenceGeneration } });
  return res.json({ ok: true, schema: AGENT_OPS_CLIENT_SCHEMA, scope_id: result.scopeId, auth_fence_generation: result.authFenceGeneration });
});

app.post('/api/agent-ops/v1/control/revoke', requireAgentOpsEnabled, requireRouterBridgeSecret, (req, res) => {
  const scopeId = normalizeOptionalText(req.body?.scope_id, 255);
  if (scopeId) {
    const result = agentOpsService.revokeScope(scopeId, req.body?.clear_device === true);
    if (respondAgentOps(res, result)) return;
    return res.json({ ok: true, scope_id: result.scopeId, auth_fence_generation: result.authFenceGeneration });
  }
  const count = agentOpsService.revokeScopesByBinding({
    ownerMxid: normalizeOptionalText(req.body?.owner_mxid, 255) || undefined,
    ownerDmRoomId: normalizeOptionalText(req.body?.owner_dm_room_id, 255) || undefined,
    projectRoomId: normalizeOptionalText(req.body?.project_room_id, 255) || undefined,
    stableAgentId: normalizeOptionalText(req.body?.stable_agent_id, 255) || undefined,
  });
  return res.json({ ok: true, revoked_scope_count: count });
});

app.post('/api/agent-ops/v1/control/bootstrap', requireAgentOpsEnabled, requireRouterBridgeSecret, (req, res) => {
  if (
    req.body?.was_encrypted !== true
    || req.body?.device_self_signature_verified !== true
    || req.body?.room_members_verified !== true
  ) {
    return sendAgentOpsError(res, 403, 'invalid_capability', 'verified encrypted Matrix device attestation is required');
  }
  const resolved = resolveAgentOpsScopeRequest(req.body);
  if (!resolved.ok) return sendAgentOpsError(res, resolved.status, resolved.code, resolved.error);
  let clientPublicJwk;
  try {
    clientPublicJwk = normalizeEd25519PublicJwk(req.body?.client_public_jwk);
  } catch (error) {
    return sendAgentOpsError(res, 400, 'bad_request', error.message);
  }
  const result = agentOpsService.issueGrant({
    ...resolved.scope,
    matrixDeviceId: req.body?.matrix_device_id,
    matrixDeviceEd25519: req.body?.matrix_device_ed25519,
    matrixDeviceCurve25519: req.body?.matrix_device_curve25519,
    matrixEventId: req.body?.matrix_event_id,
    clientNonce: req.body?.client_nonce,
    clientPublicJwk,
    audience: AGENT_OPS_LOOPBACK_ORIGIN,
  });
  if (respondAgentOps(res, result)) return;
  const unsignedGrant = {
    schema: AGENT_OPS_CLIENT_SCHEMA,
    kind: 'client_session_grant',
    grant_jti: result.grantJti,
    scope_id: result.scopeId,
    projection_id: result.projectionId,
    client_nonce: result.clientNonce,
    server_challenge: result.serverChallenge,
    exchange_endpoint: `${result.audience}/api/agent-ops/v1/session/exchange`,
    audience: result.audience,
    auth_fence_generation: result.authFenceGeneration,
    expires_at_unix_ms: result.expiresAtUnixMs,
    server_public_jwk: agentOpsServerIdentity.publicJwk,
    server_key_fingerprint: agentOpsServerIdentity.fingerprint,
  };
  return res.status(result.replayed ? 200 : 201).json({
    ...unsignedGrant,
    server_signature: agentOpsServerIdentity.sign(unsignedGrant),
    idempotent_request_replay: result.replayed,
  });
});

app.post('/api/agent-ops/v1/session/exchange', requireAgentOpsEnabled, requireAgentOpsLoopback, (req, res) => {
  const grantJti = normalizeOptionalText(req.body?.grant_jti, 255);
  const clientNonce = normalizeOptionalText(req.body?.client_nonce, 255);
  const serverChallenge = normalizeOptionalText(req.body?.server_challenge, 255);
  const audience = normalizeOptionalText(req.body?.audience, 512);
  const proofNonce = normalizeOptionalText(req.headers['x-agent-ops-proof-nonce'], 255);
  const proof = normalizeOptionalText(req.headers['x-agent-ops-proof'], 1024);
  if (!grantJti || !clientNonce || !serverChallenge || !audience || !proofNonce || !proof) {
    return sendAgentOpsError(res, 400, 'bad_request', 'grant fields and proof headers are required');
  }
  if (audience !== req.agentOpsAudience) return sendAgentOpsError(res, 403, 'scope_mismatch', 'exchange audience mismatch');
  const descriptor = agentOpsService.grantProofDescriptor({ grantJti, clientNonce, serverChallenge, audience });
  if (respondAgentOps(res, descriptor)) return;
  const material = agentOpsGrantProofMaterial({
    grantJti,
    clientNonce,
    serverChallenge,
    proofNonce,
    method: req.method,
    requestPath: req.originalUrl,
    body: req.body,
    audience,
  });
  if (!verifyAgentOpsProof(descriptor.clientPublicJwk, proof, material)) {
    return sendAgentOpsError(res, 401, 'invalid_capability', 'grant proof of possession is invalid');
  }
  const result = agentOpsService.exchangeGrant({ grantJti, clientNonce, serverChallenge, audience });
  if (respondAgentOps(res, result)) return;
  auditLog(req, { summary: { action: 'agent-ops-session-exchange', scopeId: result.scopeId } });
  const unsignedSession = {
    schema: AGENT_OPS_CLIENT_SCHEMA,
    kind: 'client_session',
    client_session_id: result.clientSessionId,
    session_capability: result.sessionCapability,
    scope_id: result.scopeId,
    projection_id: result.projectionId,
    stream_epoch: result.streamEpoch,
    auth_fence_generation: result.authFenceGeneration,
    audience,
    expires_at_unix_ms: result.expiresAtUnixMs,
    server_key_fingerprint: agentOpsServerIdentity.fingerprint,
  };
  return res.status(201).json({ ...unsignedSession, server_signature: agentOpsServerIdentity.sign(unsignedSession) });
});

app.get('/api/agent-ops/v1/snapshot', requireAgentOpsEnabled, requireAgentOpsLoopback, requireAgentOpsClientSession, (req, res) => {
  const result = agentOpsService.snapshot(req.agentOpsAuth);
  if (respondAgentOps(res, result)) return;
  return res.json(result.snapshot);
});

app.get('/api/agent-ops/v1/invalidation', requireAgentOpsEnabled, requireAgentOpsLoopback, requireAgentOpsClientSession, (req, res) => {
  const after = Number.parseInt(req.query?.after, 10);
  if (!Number.isSafeInteger(after) || after < 0) return sendAgentOpsError(res, 400, 'bad_request', 'after must be a non-negative JSON-safe integer');
  const result = agentOpsService.invalidation(req.agentOpsAuth, after);
  if (respondAgentOps(res, result)) return;
  return result.invalidation ? res.json(result.invalidation) : res.status(204).end();
});

app.post('/api/agent-ops/v1/commands/cancel-dispatch', requireAgentOpsEnabled, requireAgentOpsLoopback, requireAgentOpsClientSession, (req, res) => {
  const result = agentOpsService.cancelDispatch(req.agentOpsAuth, req.body || {});
  if (respondAgentOps(res, result)) return;
  const dispatchId = normalizeOptionalText(req.body?.target?.entity_id, 255);
  auditLog(req, { summary: { action: 'agent-ops-cancel-dispatch', scopeId: req.agentOpsAuth.scopeId, dispatchId } });
  if (dispatchId) liveThreadSessionRunners.get(dispatchId)?.controller.abort();
  scheduleRouterPump();
  return res.json({ ...result.response, idempotent_request_replay: result.replayed });
});

app.post('/api/agent-ops/v1/commands/mark-resource-inspected', requireAgentOpsEnabled, requireAgentOpsLoopback, requireAgentOpsClientSession, (req, res) => {
  const result = agentOpsService.markResourceInspected(req.agentOpsAuth, req.body || {});
  if (respondAgentOps(res, result)) return;
  auditLog(req, { summary: { action: 'agent-ops-mark-resource-inspected', scopeId: req.agentOpsAuth.scopeId, resourceId: result.response.resource_id } });
  return res.json({ ...result.response, idempotent_request_replay: result.replayed });
});

app.post('/api/agent-ops/v1/commands/begin-outcome-inspection', requireAgentOpsEnabled, requireAgentOpsLoopback, requireAgentOpsClientSession, (req, res) => {
  const result = agentOpsService.beginOutcomeInspection(req.agentOpsAuth, req.body || {});
  if (respondAgentOps(res, result)) return;
  auditLog(req, { summary: { action: 'agent-ops-begin-outcome-inspection', scopeId: req.agentOpsAuth.scopeId, inspectionId: result.response.inspection_id } });
  return res.status(result.replayed ? 200 : 201).json({ ...result.response, idempotent_request_replay: result.replayed });
});

app.post('/api/agent-ops/v1/commands/resolve-outcome', requireAgentOpsEnabled, requireAgentOpsLoopback, requireAgentOpsClientSession, (req, res) => {
  const result = agentOpsService.resolveOutcome(req.agentOpsAuth, req.body || {});
  if (respondAgentOps(res, result)) return;
  auditLog(req, { summary: { action: 'agent-ops-resolve-outcome', scopeId: req.agentOpsAuth.scopeId, resolution: result.response.resolution } });
  scheduleRouterPump();
  return res.status(result.replayed ? 200 : 201).json({ ...result.response, idempotent_request_replay: result.replayed });
});

function runnerCapabilityFromRequest(req) {
  const dispatchId = normalizeOptionalText(req.headers['x-hagency-dispatch-id'], 255);
  const runnerId = normalizeOptionalText(req.headers['x-hagency-runner-id'], 255);
  const capability = normalizeOptionalText(req.headers['x-hagency-dispatch-capability'], 512);
  const fenceGeneration = Number.parseInt(req.headers['x-hagency-fence-generation'], 10);
  if (!dispatchId || !runnerId || !capability || !Number.isInteger(fenceGeneration) || fenceGeneration <= 0) return null;
  return { dispatchId, runnerId, capability, fenceGeneration };
}

function requireOwnedRunnerDescriptor(req, res) {
  const capability = runnerCapabilityFromRequest(req);
  if (!capability) {
    res.status(401).json({ error: 'runner dispatch capability required' });
    return null;
  }
  const agentName = normalizeAgentName(req.body?.agent);
  const agent = agentName ? agents[agentName] : null;
  if (!isAgentRecord(agent) || !agent.agentId) {
    res.status(404).json({ error: 'runner agent not found' });
    return null;
  }
  const descriptor = routerStore.getLaunchDescriptor(capability);
  if (!descriptor.ok && descriptor.code) {
    res.status(routerRefusalStatus(descriptor)).json({ error: descriptor.message, code: descriptor.code });
    return null;
  }
  if (descriptor.agentName !== agent.name || descriptor.agentId !== agent.agentId) {
    res.status(403).json({ error: 'runner capability does not belong to this agent' });
    return null;
  }
  if (agent.manualDown) {
    res.status(409).json({ error: 'agent was stopped by its operator' });
    return null;
  }
  return { capability, agent, descriptor };
}

function admittedRunnerPeer(owned, peer) {
  if (!isAgentRecord(peer) || !peer.agentId || !agentEligibleForRoom(peer, owned.descriptor.roomId)) return false;
  if (peer.name === owned.agent.name) return true;
  return [owned.agent, peer].every((agent) => approvalStore.listBindings({
    agent: agent.name, projectRoomId: owned.descriptor.roomId,
  }).length === 1);
}

app.post('/api/router/session-task', requireAgentToken((req) => req.body?.agent || ''), (req, res) => {
  if (!THREAD_SESSIONS_ENABLED) return res.status(404).json({ error: 'thread-session router is disabled' });
  const owned = requireOwnedRunnerDescriptor(req, res);
  if (!owned) return;
  const op = req.body?.op;
  if (!['get', 'list', 'transition', 'comment', 'heartbeat', 'execution'].includes(op)) return res.status(400).json({ error: 'unknown task operation' });
  try {
    if (op === 'list') {
      return res.json(taskStore.listTasks().filter((task) => routerStore.authorizeTaskFromDispatch({
        ...owned.capability, taskId: task.id,
      }).ok));
    }
    const id = normalizeOptionalText(req.body?.id, 255) || owned.descriptor.taskId;
    const authorized = routerStore.authorizeTaskFromDispatch({ ...owned.capability, taskId: id, write: op !== 'get' });
    if (!authorized.ok) return res.status(authorized.code === 'invalid_capability' ? 403 : routerRefusalStatus(authorized)).json({ error: authorized.message, code: authorized.code });
    if (op === 'get') {
      const task = taskStore.getTask(id);
      return task ? res.json(task) : res.status(404).json({ error: 'task not found' });
    }
    const task = op === 'heartbeat'
      ? taskStore.updateTaskExecution(id, { heartbeat_at: new Date().toISOString() })
      : op === 'execution'
      ? taskStore.updateTaskExecution(id, {
        // Task maintenance cannot set status/identity or supply its own clock.
        // Keep this explicit field projection behind the same dispatch write gate.
        ...(req.body?.heartbeat_at === true ? { heartbeat_at: new Date().toISOString() } : {}),
        ...(req.body?.waiting_reason !== undefined ? { waiting_reason: req.body.waiting_reason } : {}),
        ...(req.body?.waiting_until !== undefined ? { waiting_until: req.body.waiting_until } : {}),
      })
      : op === 'comment'
      ? taskStore.addComment(id, { author: owned.agent.name, text: req.body?.text })
      : taskStore.transitionTask(id, req.body?.status, { waiting_reason: req.body?.waiting_reason, waiting_until: req.body?.waiting_until });
    broadcastSSE('task_updated', task);
    return res.json({ ok: true, task });
  } catch (error) {
    return respondTaskStoreError(res, error, 'failed session task operation');
  }
});

app.post('/api/router/files', requireAgentToken((req) => req.body?.agent || ''), async (req, res) => {
  if (!THREAD_SESSIONS_ENABLED) return res.status(404).json({ error: 'thread-session router is disabled' });
  const owned = requireOwnedRunnerDescriptor(req, res);
  if (!owned) return;
  const allowed = routerStore.authorizeFileReply(owned.capability);
  if (!allowed.ok) return res.status(routerRefusalStatus(allowed)).json({ error: allowed.message, code: allowed.code });
  const fields = new Set(['agent', 'op', 'path', 'name', 'caption', 'tool_call_id', 'delivery_id', 'event_id']);
  if (Object.keys(req.body).some(key => !fields.has(key))) return res.status(400).json({ error: 'file tools cannot select destinations or additional fields' });
  let result;
  if (req.body.op === 'receive') {
    if (typeof req.body.event_id !== 'string') return res.status(400).json({ error: 'event_id required' });
    const received = routerStore.receiveFile({ ...owned.capability, eventId: req.body.event_id });
    if (!received.ok) return res.status(routerRefusalStatus(received)).json({ error: received.message, code: received.code });
    if (received.file.errorCode) return res.status(409).json({ error: received.file.error, code: received.file.errorCode });
    if (received.file.remoteContent) {
      try {
        const side = projectSideStore.getSide(owned.agent.projectSide);
        const credential = side?.active && projectSideStore.credentialFor(side.id);
        const token = credential?.kind === 'appservice' ? credential.asToken : credential?.representativeToken;
        if (!side || !token) return res.status(409).json({ error: 'Attachment server credential is unavailable', code: 'media_credential_unavailable' });
        const file = await receiveMatrixFile({ content: received.file.remoteContent, baseUrl: side.apiBaseUrl, token,
          directory: MATRIX_MEDIA_DIR, asUserId: credential.kind === 'appservice' ? side.representative?.mxid : null });
        const stillAllowed = routerStore.authorizeFileReply(owned.capability);
        if (!stillAllowed.ok) return res.status(routerRefusalStatus(stillAllowed)).json({ error: stillAllowed.message, code: stillAllowed.code });
        routerStore.conversations.cacheReceivedFile(owned.capability.dispatchId, req.body.event_id, file);
        received.file = file;
      } catch (error) {
        return res.status(error.permanent ? 409 : 503).json({ error: 'Attachment download failed; retry or ask the sender to upload it again', code: error.code || 'media_download_failed' });
      }
    }
    try {
      const verified = snapshotSessionFile({ workspace: MATRIX_MEDIA_DIR, requestedPath: received.file.path,
        name: received.file.name, directory: MATRIX_MEDIA_DIR });
      if (verified.sha256 !== received.file.sha256 || verified.size !== received.file.size) throw new Error('integrity');
    } catch { return res.status(409).json({ error: 'Received attachment is unavailable or changed', code: 'file_integrity_mismatch' }); }
    return res.json({ eventId: req.body.event_id, ...received.file });
  } else if (req.body.op === 'status') {
    if (typeof req.body.delivery_id !== 'string') return res.status(400).json({ error: 'delivery_id required' });
    result = routerStore.readFileReply({ ...owned.capability, commandId: req.body.delivery_id });
  } else if (req.body.op === 'send') {
    if (typeof req.body.path !== 'string' || req.body.path.length > 4096 || !req.body.path.trim()
      || typeof req.body.tool_call_id !== 'string' || !req.body.tool_call_id || req.body.tool_call_id.length > 512
      || (req.body.name !== undefined && typeof req.body.name !== 'string')
      || (req.body.caption !== undefined && (typeof req.body.caption !== 'string' || req.body.caption.length > 1000))) {
      return res.status(400).json({ error: 'valid file path, tool_call_id, filename and caption required' });
    }
    const requested = { path: req.body.path.trim(), name: req.body.name || null, caption: req.body.caption || '' };
    const requestInput = { ...owned.capability, requestKey: req.body.tool_call_id,
      requestDigest: createHash('sha256').update(JSON.stringify(requested)).digest('hex') };
    result = routerStore.findFileReply(requestInput);
    if (result.ok && !result.delivery) {
      try {
        const file = snapshotSessionFile({ workspace: owned.descriptor.cwd, requestedPath: requested.path,
          name: requested.name, directory: path.join(DATA_DIR, 'session-files') });
        result = routerStore.queueFileReply({ ...requestInput, file, body: requested.caption });
      } catch (error) {
        return res.status(error instanceof SessionFileError ? error.status : 400).json({
          error: error instanceof SessionFileError ? error.message : 'Unable to snapshot workspace file',
          code: error instanceof SessionFileError ? error.code : 'file_read_failed',
        });
      }
    }
  } else return res.status(400).json({ error: 'unknown file operation' });
  if (!result.ok) return res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
  return res.json(result.delivery);
});

app.post('/api/router/reply-outbox/:id/prepared-file', requireRouterBridgeSecret, (req, res) => {
  const content = req.body?.content;
  if (!content || typeof content !== 'object' || Array.isArray(content) || typeof req.body.claim_token !== 'string') {
    return res.status(400).json({ error: 'prepared content and claim token required' });
  }
  const result = routerStore.prepareFileReply({ commandId: req.params.id, claimToken: req.body.claim_token, content });
  return result.ok ? res.json(result) : res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
});

app.post('/api/router/messages', requireAgentToken((req) => req.body?.agent || ''), async (req, res) => {
  if (!THREAD_SESSIONS_ENABLED) return res.status(404).json({ error: 'thread-session router is disabled' });
  const owned = requireOwnedRunnerDescriptor(req, res);
  if (!owned) return;
  const target = agents[normalizeAgentName(req.body?.to)];
  if (!admittedRunnerPeer(owned, target)) return res.status(403).json({ error: 'target is not admitted to this project scope' });
  if (!threadSessionAgentEligibility(target).ok) return res.status(409).json({ error: 'target is unavailable for thread work' });
  if (!normalizeOptionalText(req.body?.tool_call_id, 512)) return res.status(400).json({ error: 'tool_call_id is required' });
  const kind = req.body?.type;
  const body = normalizeOptionalText(req.body?.full || req.body?.summary, 100_000);
  if (!body || !['request', 'inform', 'reply'].includes(kind)) return res.status(400).json({ error: 'message body and valid type are required' });
  if (req.body?.attachments?.length || req.body?.group || req.body?.room_id) return res.status(400).json({ error: 'session messaging cannot attach files or select a room' });
  try {
    if (kind === 'request') {
      const result = routerStore.createTaskFromDispatch({ ...owned.capability,
        toolCallId: `message:${req.body?.tool_call_id}`, rootMessageId: req.body?.root_message_id, inputMessageIds: [],
        task: { title: String(req.body?.summary || body).slice(0, 255), description: body,
          assigneeAgentId: target.agentId, assigneeName: target.name, parentId: owned.descriptor.taskId },
        acknowledgementBody: `Delegated to ${target.name}: ${String(req.body?.summary || body).slice(0, 200)}`,
      });
      if (!result.ok) return res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
      return res.status(result.replayed ? 200 : 201).json({ ok: true, task: result });
    }
    const result = routerStore.deliverPeerMessageFromDispatch({ ...owned.capability,
      recipientAgentId: target.agentId, recipientAgentName: target.name,
      targetTaskId: req.body?.target_task_id, toolCallId: req.body?.tool_call_id, body });
    if (!result.ok) return res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
    const queued = await enqueueThreadSessionDispatch({ agent: target, sessionId: result.session.sessionId,
      taskId: result.taskId, threadRootEventId: result.threadRootEventId });
    if (!queued.ok) return res.status(routerRefusalStatus(queued)).json({ error: queued.message, code: queued.code, persisted: true });
    return res.json({ ok: true, messageId: result.messageId, taskId: result.taskId,
      queued: queued.state === 'queued', dispatchId: queued.dispatchId, dispatchState: queued.state });
  } catch (error) {
    return res.status(400).json({ error: error.message });
  }
});

app.post('/api/router/tasks', requireAgentToken((req) => req.body?.agent || ''), (req, res) => {
  if (!THREAD_SESSIONS_ENABLED) return res.status(404).json({ error: 'thread-session router is disabled' });
  const capability = runnerCapabilityFromRequest(req);
  if (!capability) return res.status(401).json({ error: 'runner dispatch capability required' });
  const creatorName = normalizeAgentName(req.body?.agent);
  const creator = creatorName ? agents[creatorName] : null;
  if (!isAgentRecord(creator)) return res.status(404).json({ error: 'creator agent not found' });
  const assigneeName = normalizeAgentName(req.body?.assignee);
  const assignee = assigneeName ? agents[assigneeName] : null;
  if (!isAgentRecord(assignee) || !assignee.agentId) {
    return res.status(400).json({ error: 'assignee must be a registered agent with a stable v1 id' });
  }
  const eligible = threadSessionAgentEligibility(assignee);
  if (!eligible.ok) return res.status(routerRefusalStatus(eligible)).json({ error: eligible.message, code: eligible.code });
  const descriptor = routerStore.getLaunchDescriptor(capability);
  if (!descriptor.ok && descriptor.code) {
    return res.status(routerRefusalStatus(descriptor)).json({ error: descriptor.message, code: descriptor.code });
  }
  if (descriptor.agentName !== creator.name || descriptor.agentId !== creator.agentId) {
    return res.status(403).json({ error: 'runner capability does not belong to creator agent' });
  }
  if (!admittedRunnerPeer({ agent: creator, descriptor }, assignee)) {
    return res.status(403).json({ error: 'assignee is not admitted to this project scope' });
  }
  const result = routerStore.createTaskFromDispatch({
    ...capability,
    toolCallId: req.body?.tool_call_id,
    rootMessageId: req.body?.root_message_id,
    inputMessageIds: Array.isArray(req.body?.input_message_ids) ? req.body.input_message_ids : [],
    task: {
      title: req.body?.title,
      description: req.body?.description,
      priority: req.body?.priority,
      granularity: req.body?.granularity,
      assigneeAgentId: assignee.agentId,
      assigneeName: assignee.name,
      parentId: req.body?.parent_id ?? descriptor.taskId,
      labels: Array.isArray(req.body?.labels) ? req.body.labels : [],
    },
    acknowledgementBody: req.body?.acknowledgement_body,
  });
  if (!result.ok) return res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
  return res.status(result.replayed ? 200 : 201).json({ ok: true, task: result });
});

app.post('/api/router/task-operations', requireAgentToken((req) => req.body?.agent || ''), (req, res) => {
  if (!THREAD_SESSIONS_ENABLED) return res.status(404).json({ error: 'thread-session router is disabled' });
  const owned = requireOwnedRunnerDescriptor(req, res);
  if (!owned) return;
  const result = routerStore.taskOperation({
    ...owned.capability,
    action: req.body?.action,
    taskId: req.body?.task_id,
    toolCallId: req.body?.tool_call_id,
    patch: req.body?.patch,
  });
  if (!result.ok) return res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
  if (!result.replayed && result.task && !['get', 'list'].includes(req.body?.action)) broadcastSSE('task_updated', result.task);
  return res.json(result);
});

app.post('/api/router/approvals/claude', requireAgentToken((req) => req.body?.agent || ''), (req, res) => {
  if (!THREAD_SESSIONS_ENABLED) return res.status(404).json({ error: 'thread-session router is disabled' });
  const owned = requireOwnedRunnerDescriptor(req, res);
  if (!owned) return;
  if (owned.descriptor.framework !== 'claude') {
    return res.status(400).json({ error: 'Claude approval endpoint requires a Claude dispatch' });
  }
  const requestId = normalizeOptionalText(req.body?.request_id, 255);
  const toolName = normalizeOptionalText(req.body?.tool_name, 255);
  const description = normalizeOptionalText(req.body?.description, 4096) || 'Claude permission request';
  const inputPreview = normalizeOptionalText(req.body?.input_preview, 8192) || '';
  if (!requestId || !toolName) return res.status(400).json({ error: 'request_id and tool_name are required' });
  const operationDigest = createHash('sha256').update(JSON.stringify({
    requestId, toolName, description, inputPreview,
  })).digest('hex');
  const approvalId = `tss_claude_${createHash('sha256').update(`${owned.capability.dispatchId}\0${requestId}`).digest('hex')}`;
  const parked = routerStore.parkForApproval({
    ...owned.capability,
    approvalId,
    operationDigest,
    upstreamRequestId: requestId,
    maxParkedRunners: MAX_PARKED_RUNNERS,
  });
  if (!parked.ok) return res.status(routerRefusalStatus(parked)).json({ error: parked.message, code: parked.code });
  try {
    let approval = approvalStore.listRequests({ upstream_request_prefix: approvalId })
      .find((row) => row.upstream_request_id === approvalId
        && row.router_approval_id === approvalId && row.agent === owned.agent.name) || null;
    if (!approval) {
      approval = approvalStore.createRequest({
        agent: owned.agent.name,
        runtime: 'claude',
        project_room_id: routerStore.conversations.direct(owned.descriptor.roomId, owned.agent.name)?.projectRoomId || owned.descriptor.roomId,
        upstream_request_id: approvalId,
        tool_name: toolName,
        description,
        input_preview: inputPreview,
      }, { routerApprovalId: approvalId });
      if (approval.status === 'pending') {
        broadcastSSE('approval_requested', { request_id: approval.id, agent: approval.agent });
      }
    }
    return res.status(parked.replayed ? 200 : 201).json({
      ok: true,
      approval,
      router_approval_id: approvalId,
      operation_digest: operationDigest,
    });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to create Claude thread-session approval');
  }
});

app.post('/api/router/approvals/claude/apply', requireAgentToken((req) => req.body?.agent || ''), (req, res) => {
  if (!THREAD_SESSIONS_ENABLED) return res.status(404).json({ error: 'thread-session router is disabled' });
  const owned = requireOwnedRunnerDescriptor(req, res);
  if (!owned) return;
  const approvalRequestId = normalizeOptionalText(req.body?.approval_request_id, 255);
  const approvalId = normalizeOptionalText(req.body?.router_approval_id, 255);
  const operationDigest = normalizeOptionalText(req.body?.operation_digest, 128);
  if (!approvalRequestId || !approvalId || !operationDigest) {
    return res.status(400).json({ error: 'approval_request_id, router_approval_id and operation_digest are required' });
  }
  const record = approvalStore.getRequest(approvalRequestId);
  if (!record || record.agent !== owned.agent.name || record.upstream_request_id !== approvalId) {
    return res.status(409).json({ error: 'consumed approval does not match this runner operation' });
  }
  if (record.status !== 'consumed' || !record.decision_event_id || !['allow', 'deny'].includes(record.decision)) {
    return res.status(409).json({ error: 'approval decision has not been durably consumed' });
  }
  const applied = routerStore.recordApprovalDecision({
    decisionEventId: record.decision_event_id,
    approvalId,
    dispatchId: owned.capability.dispatchId,
    operationDigest,
    decision: record.decision,
  });
  if (!applied.ok) return res.status(routerRefusalStatus(applied)).json({ error: applied.message, code: applied.code });
  const resumed = routerStore.resumeAfterApproval({ ...owned.capability, approvalId, operationDigest });
  if (!resumed.ok) return res.status(routerRefusalStatus(resumed)).json({ error: resumed.message, code: resumed.code });
  return res.json({ ok: true, behavior: resumed.decision });
});

app.get('/api/router/snapshot', requireRouterBearer, (_req, res) => {
  return res.json(routerStore.snapshot());
});

app.get('/api/router/events', requireRouterBearer, (req, res) => {
  const after = Number.parseInt(req.query?.after, 10);
  const limit = Number.parseInt(req.query?.limit, 10);
  return res.json(routerStore.eventsAfter(Number.isFinite(after) ? after : 0, Number.isFinite(limit) ? limit : 500));
});

app.post('/api/router/dispatches/:id/cancel', requireRouterBearer, (req, res) => {
  const dispatchId = normalizeOptionalText(req.params.id, 255);
  if (!dispatchId) return res.status(400).json({ error: 'dispatch id is required' });
  const before = routerStore.snapshot().dispatches.find((row) => row.dispatchId === dispatchId);
  if (!before) return res.status(404).json({ error: 'dispatch not found' });
  liveThreadSessionRunners.get(dispatchId)?.controller.abort();
  let result;
  if (before.state === 'queued' || before.state === 'leased') {
    result = routerStore.cancelBeforeStart(dispatchId);
    if (!result.ok && result.code === 'invalid_transition') {
      result = routerStore.markOutcomeUnknown(dispatchId, 'operator_cancelled_after_start');
    }
  } else if (before.state === 'started' || before.state === 'parked') {
    result = routerStore.markOutcomeUnknown(dispatchId, 'operator_cancelled_after_start');
  } else {
    return res.status(409).json({ error: `dispatch is already terminal: ${before.state}` });
  }
  if (!result.ok) return res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
  scheduleRouterPump();
  return res.json({ ok: true, result });
});

app.post('/api/router/dispatches/:id/outcome-inspection', requireRouterBearer, (req, res) => {
  const dispatchId = normalizeOptionalText(req.params.id, 255);
  if (!dispatchId) return res.status(400).json({ error: 'dispatch id is required' });
  const result = routerStore.beginOutcomeInspection(dispatchId);
  if (!result.ok) return res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
  return res.status(201).json({ ok: true, inspection: result });
});

app.post('/api/router/dispatches/:id/resolve-outcome', requireRouterBearer, (req, res) => {
  const dispatchId = normalizeOptionalText(req.params.id, 255);
  if (!dispatchId) return res.status(400).json({ error: 'dispatch id is required' });
  const result = routerStore.resolveOutcomeUnknown({
    dispatchId,
    inspectionId: req.body?.inspection_id,
    inspectionToken: req.body?.inspection_token,
    requestId: req.body?.request_id,
    action: req.body?.action,
    operatorNote: req.body?.operator_note,
    recoveryInstruction: req.body?.recovery_instruction,
  });
  if (!result.ok) return res.status(routerRefusalStatus(result)).json({ error: result.message, code: result.code });
  scheduleRouterPump();
  return res.status(result.replayed ? 200 : 201).json({ ok: true, resolution: result });
});

app.post('/api/router/resources/:id/clear-dirty', requireRouterBearer, (req, res) => {
  const cleared = routerStore.clearWorkspaceDirty(req.params.id);
  if (!cleared.ok) return res.status(routerRefusalStatus(cleared)).json({ error: cleared.message, code: cleared.code });
  return res.json({ ok: true, result: cleared });
});

app.post('/api/router/matrix-outbox/claim', requireRouterBridgeSecret, (req, res) => {
  const claimMs = Number.parseInt(req.body?.claim_ms, 10);
  const command = routerStore.claimMatrixCommand(Number.isFinite(claimMs) ? claimMs : 30_000);
  return command ? res.json({ ok: true, command }) : res.status(204).end();
});

app.post('/api/router/matrix-outbox/:id/delivered', requireRouterBridgeSecret, async (req, res) => {
  const activated = routerStore.recordMatrixDelivery({
    commandId: req.params.id,
    claimToken: req.body?.claim_token,
    eventId: req.body?.event_id,
  });
  if (!activated.ok) return res.status(routerRefusalStatus(activated)).json({ error: activated.message, code: activated.code });
  const boundAgent = Object.values(agents).find((candidate) => candidate?.agentId === activated.agentId);
  if (!boundAgent) return res.status(500).json({ error: 'activated task assignee is not registered' });
  const queued = await enqueueThreadSessionDispatch({
    agent: boundAgent,
    sessionId: activated.sessionId,
    taskId: activated.taskId,
    threadRootEventId: activated.threadRootEventId,
    prompt: 'Execute the newly activated task from its session-scoped inputs.',
  });
  if (!queued.ok) {
    const recorded = routerStore.recordTaskDispatchFailure(activated.taskId, queued.code || 'dispatch_unavailable');
    if (!recorded.ok) {
      return res.status(500).json({
        error: 'task thread activated, but dispatch failure could not be recorded',
        code: 'dispatch_failure_record_failed',
        activation: activated,
      });
    }
    return res.json({
      ok: true,
      activation: activated,
      dispatch: null,
      dispatch_refusal: { code: queued.code || 'dispatch_unavailable' },
      attention: recorded,
    });
  }
  return res.json({ ok: true, activation: activated, dispatch: queued });
});

app.post('/api/router/matrix-outbox/:id/failed', requireRouterBridgeSecret, (req, res) => {
  const failed = routerStore.recordMatrixFailure({
    commandId: req.params.id,
    claimToken: req.body?.claim_token,
    errorCode: req.body?.error_code || 'matrix_send_failed',
  });
  if (!failed.ok) return res.status(routerRefusalStatus(failed)).json({ error: failed.message, code: failed.code });
  return res.json({ ok: true, result: failed });
});

app.post('/api/router/reply-outbox/claim', requireRouterBridgeSecret, (req, res) => {
  const claimMs = Number.parseInt(req.body?.claim_ms, 10);
  const command = routerStore.claimReplyCommand(Number.isFinite(claimMs) ? claimMs : 30_000);
  return command ? res.json({ ok: true, command }) : res.status(204).end();
});

app.post('/api/router/reply-outbox/:id/delivered', requireRouterBridgeSecret, (req, res) => {
  const delivered = routerStore.recordReplyDelivery({
    commandId: req.params.id,
    claimToken: req.body?.claim_token,
    eventId: req.body?.event_id,
  });
  if (!delivered.ok) return res.status(routerRefusalStatus(delivered)).json({ error: delivered.message, code: delivered.code });
  return res.json({ ok: true, result: delivered });
});

app.post('/api/router/reply-outbox/:id/failed', requireRouterBridgeSecret, (req, res) => {
  const failed = routerStore.recordReplyFailure({
    commandId: req.params.id,
    claimToken: req.body?.claim_token,
    errorCode: req.body?.error_code || 'matrix_send_failed',
  });
  if (!failed.ok) return res.status(routerRefusalStatus(failed)).json({ error: failed.message, code: failed.code });
  return res.json({ ok: true, result: failed });
});

// Only the authenticated Matrix bridge can assert room-scoped owner provenance.
/*
 * Whether a bound agent is actually IN the room, as observed against the homeserver.
 *
 * B1: a binding is what lets a project reach an agent, and nothing ever checked it against Matrix
 * membership in either direction. An operator asked why their agent appeared in three projects, and
 * the honest answer required me to go and read the room memberships by hand — the console could not
 * tell them, because the two facts live in different places and never met.
 *
 * Bridge-secret guarded and SEPARATE from the binding upsert beside it: observing membership
 * asserts nothing about permission, and folding it into the governance write would let a routine
 * liveness check carry a decision's authority.
 */
app.put('/api/approval-bindings/membership', requireApprovalBridgeSecret, (req, res) => {
  try {
    const binding = approvalStore.observeBindingMembership(req.body || {});
    // 404 rather than a silent ok: an observation about a binding nobody made is a mismatch worth
    // reporting to the caller, not something to store.
    if (!binding) return res.status(404).json({ error: 'no such binding' });
    return res.json({ ok: true, binding });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to record binding membership');
  }
});

app.put('/api/approval-bindings', requireApprovalBridgeSecret, (req, res) => {
  try {
    const previous = approvalStore.listBindings({
      agent: req.body?.agent,
      projectRoomId: req.body?.project_room_id ?? req.body?.projectRoomId,
    })[0] || null;
    const binding = approvalStore.upsertBinding(req.body || {});
    const authorityChanged = !previous
      || previous.ownerMxid !== binding.ownerMxid
      || previous.ownerDmRoomId !== binding.ownerDmRoomId;
    if (authorityChanged && agentOpsService) {
      const agent = agents[normalizeAgentName(binding.agent)];
      if (isAgentRecord(agent) && agent.agentId) {
        agentOpsService.revokeScopesByBinding({
          projectRoomId: binding.projectRoomId,
          stableAgentId: agent.agentId,
        });
      }
    }
    return res.json({ ok: true, binding });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to persist approval binding');
  }
});

app.delete('/api/approval-bindings/:agent/:roomId', requireApprovalBridgeSecret, (req, res) => {
  try {
    const binding = approvalStore.removeBinding(req.params.agent, req.params.roomId);
    if (!binding) return res.status(404).json({ error: 'approval binding not found' });
    if (agentOpsService) {
      const agent = agents[normalizeAgentName(binding.agent)];
      if (isAgentRecord(agent) && agent.agentId) {
        agentOpsService.revokeScopesByBinding({
          projectRoomId: binding.projectRoomId,
          stableAgentId: agent.agentId,
        });
      }
    }
    return res.json({ ok: true, binding });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to remove approval binding');
  }
});

app.get('/api/approval-bindings', requireApprovalBridgeSecret, (req, res) => {
  try {
    if (req.query?.thread_root_event_id !== undefined) {
      const roomId = req.query.project;
      const threadRootEventId = req.query.thread_root_event_id;
      const requesterMxid = req.query.requester_mxid;
      if (typeof roomId !== 'string' || !/^![^:\s]+:\S+$/.test(roomId) || roomId.length > 255
        || typeof threadRootEventId !== 'string' || !/^\$\S{1,254}$/.test(threadRootEventId)
        || typeof requesterMxid !== 'string' || !/^@[^:\s]+:\S+$/.test(requesterMxid) || requesterMxid.length > 255) {
        return res.status(400).json({ error: 'exact project room, thread root and requester MXID are required' });
      }
      const candidates = [];
      if (THREAD_SESSIONS_ENABLED && routerStore) {
        for (const approval of approvalStore.listBindings({ project: roomId })) {
          if (approval.active === false || approval.projectRoomId !== roomId) continue;
          const agent = agents[approval.agent];
          if (!agent?.agentId || !agentEligibleForRoom(agent, roomId)) continue;
          const binding = routerStore.findActiveTaskBinding(agent.agentId, roomId, threadRootEventId);
          if (!binding.taskId || binding.agentName !== agent.name) continue;
          const task = taskStore.getTask(binding.taskId);
          if (!task || task.status === 'done' || task.created_by !== requesterMxid) continue;
          candidates.push({ agent: agent.name, taskId: task.id });
        }
      }
      return res.json({ ok: true, threadLookup: { v: 1, roomId, threadRootEventId, requesterMxid,
        target: candidates.length === 1 ? candidates[0] : null } });
    }
    return res.json({
      ok: true,
      bindings: approvalStore.listBindings({ agent: req.query?.agent, project: req.query?.project }),
    });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to list approval bindings');
  }
});

/*
 * ── 项目方 Project sides (ADR-016 decisions 1, 3 and 8) ────────────────────────────────────
 *
 * A project side is one homeserver, the credential Hagency holds there, and one representative.
 * It is the record that dissolves the circular dependency an operator identified: an agent MXID
 * contains a server name, so an identity cannot be minted before the server is known — and until
 * now `agentUserId()` composed one on Hagency's OWN server at startup, before any project existed.
 *
 * THE CREDENTIAL IS WRITE-ONLY (decision 8). Every handler below returns the store's `publicSide`
 * projection, which has no credential field by construction. `credentialFor()` is called in exactly
 * one place — the verify handler, which needs it to talk to the homeserver — and its value never
 * reaches a response. The operator can set and replace it; nothing can read it back.
 *
 * Why that is a different rule from `cf.ownSecrets`, which says Hagency's own secrets are
 * deliberately not editable from a browser: that rule protects Hagency's own AUTHENTICATION, where a
 * bad value locks the operator out of the console itself. A project side's credential is inbound work
 * capacity — a bad value costs one project side's reachability and locks nobody out.
 */
function respondProjectSideError(res, error, fallback = 'project side operation failed') {
  if (error instanceof ProjectSideStoreError) {
    /*
     * `conflict` was missing until a test asked for 409 and got 500. The store has always been able to
     * throw it — `upsertProject` does, when two projects claim one room — and an unmapped code falls
     * through to 500, which tells a caller "we broke" about a request that was simply refused. Mapped
     * here rather than at the throw site, because the mapping is this layer's job.
     */
    const status = {
      bad_request: 400, conflict: 409, side_active: 409, not_found: 404, persistence_failed: 503,
    }[error.code] ?? 500;
    return res.status(status).json({ error: error.message, code: error.code });
  }
  return res.status(500).json({ error: error?.message || fallback });
}

/**
 * 项目方 → 项目 → 外派员工, joined for a read.
 *
 * THE THIRD LEVEL IS NOT A NEW FIELD, and that is the whole point. An approval BINDING already says
 * "this agent can be reached in this room" (ADR-002), and a project already says "this room is that
 * project" — so who serves a project is the intersection of two records that exist, not a third thing
 * to keep in step with them. Adding `agent.project` would have created a copy that drifts the first
 * time a binding is deactivated without anyone remembering to clear it.
 *
 * IT ALSO WORKS BACKWARDS. Bindings that predate projects resolve the moment their room is named,
 * which is how this deployment's existing rooms appear under a project without being touched.
 *
 * JOINED HERE, NOT IN THE STORE. `lib/project-side-store.js` deliberately knows nothing about
 * engagements or bindings — its header says so, and it is what lets a side record be read without
 * depending on another store being consistent. So the join lives at the read, where a failure degrades
 * to "no agents listed" rather than to a broken side record.
 *
 * `retiredAt` is carried because a retired agent must still be VISIBLE under the project it served:
 * decision 7 keeps the record precisely so the history stays attributable, and a tree that hid retired
 * agents would present a project as having been staffed by nobody.
 */
function withProjectStaffing(side) {
  if (!side?.projects?.length) return side;
  let bindings = [];
  try {
    bindings = approvalStore.listBindings({ includeInactive: true });
  } catch {
    // A read of the binding store must not cost the caller the side record it asked for.
    return side;
  }
  const byRoom = new Map();
  for (const b of bindings) {
    if (!b?.projectRoomId) continue;
    const list = byRoom.get(b.projectRoomId) || [];
    list.push(b);
    byRoom.set(b.projectRoomId, list);
  }
  /*
   * APPROVED BUT NOT ATTACHED, per room — the state that reads as "nobody is on this project".
   *
   * `agents` above comes from BINDINGS, and an engagement going active only creates one when an owner can
   * be resolved; without `HAGENCY_OWNER_MXID` the bind fails and records `bindError` on the engagement.
   * That is correct and it was invisible: on a live fleet a project with 50k committed to `soaker` showed
   * 「还没派人」, while the reason — "no owner known for this agent: set HAGENCY_OWNER_MXID and
   * HAGENCY_OWNER_DM_ROOM" — sat in the engagement record that no page read. The approval path's own
   * comment claims the failure is "shown in the console"; it was not.
   *
   * JOINED HERE, not on the page. The console is explicit that who staffs a project is the backend's
   * answer and re-deriving it client-side would be a second answer to one question — so the same rule
   * applies to why nobody does.
   *
   * A SEPARATE FIELD FROM `agents`, because an approved-but-unattached agent is not staff. Listing it
   * there would make a project look worked-on when nothing can reach it.
   */
  let awaitingByRoom = new Map();
  try {
    for (const e of engagementStore.list({ state: 'active' })) {
      if (!e?.projectRoomId || e.bound === true) continue;
      /*
       * THE BINDING STORE IS THE AUTHORITY ON ATTACHMENT, and the engagement's `bound` flag is only a record
       * of what happened when that engagement was approved. The two disagree in a real and ordinary way: an
       * agent bound by an earlier engagement keeps that binding, so a later engagement approved without an
       * owner records `bound: false` about an agent the project can already reach.
       *
       * FOUND ON A SECOND FLEET, not by the tests. One project listed `biglittle` as staff AND twice under
       * awaiting — which is the false alarm this whole field was supposed to avoid: a line that appears beside
       * healthy projects stops meaning anything. The first version trusted the flag alone.
       */
      const bound = (byRoom.get(e.projectRoomId) || [])
        .some((b) => b.agent === e.agent && b.active !== false);
      if (bound) continue;
      const list = awaitingByRoom.get(e.projectRoomId) || [];
      list.push({ agent: e.agent ?? null, role: e.role ?? null, bindError: e.bindError ?? null });
      awaitingByRoom.set(e.projectRoomId, list);
    }
  } catch {
    // Same rule as the binding read above: this must not cost the caller the side record it asked for.
    awaitingByRoom = new Map();
  }
  return {
    ...side,
    projects: side.projects.map((project) => {
      const rows = project.roomId ? (byRoom.get(project.roomId) || []) : [];
      return {
        ...project,
        awaitingBind: project.roomId ? (awaitingByRoom.get(project.roomId) || []) : [],
        agents: rows.map((b) => {
          const record = agents[b.agent];
          return {
            name: b.agent,
            // Whether the PROJECT can still reach this agent — the binding's claim, not the agent's health.
            bound: b.active !== false,
            // Whether the agent itself is up. Three states, and `null` is "no such record" rather than
            // offline: an agent named by a binding that no longer exists is a fact worth showing.
            online: record ? record.online !== false : null,
            retiredAt: record?.retiredAt ?? null,
            role: record?.role ?? null,
          };
        }).sort((a, b) => a.name.localeCompare(b.name)),
      };
    }),
  };
}

app.get('/api/project-sides', requireBearer, (req, res) => {
  try {
    return res.json({
      ok: true,
      sides: projectSideStore.listSides({ activeOnly: req.query?.active === 'true' })
        .map(withProjectStaffing),
    });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to list project sides');
  }
});

/*
 * REGISTERED BEFORE `/api/project-sides/:id`, and it has to be.
 *
 * Express matches in registration order, so with the parameterised route first this path resolves as
 * `:id = 'inbound-credentials'`, finds no such project side, and answers 404 — which is how it failed
 * the first time. The 404 looks like a missing route rather than a shadowed one, so a future reordering
 * would break inbound appservice authentication in a way that reads as "not implemented".
 */
/**
 * What a project side has left to spend, and whether it has anything at all.
 *
 * ADR-016's settled question 2 in force: a REAL allocation, not a slice. Returns
 * `{ allocated, committed, remaining }` where `allocated: null` means UNALLOCATED — which is not
 * unlimited. Nothing may be minted against a side with no allocation, so that a side which has been
 * configured and not yet budgeted refuses legacy requests. ADR-025 project
 * definitions instead draw from their explicitly selected Resource pool.
 */
function sideBudgetFor(sideId) {
  const side = projectSideStore.getSide(sideId);
  if (!side) return null;
  const allocated = side.allocatedTokens ?? null;
  const committed = engagementStore.committedForProjectSide(side.serverName, { legacyOnly: true });
  return {
    allocated,
    committed,
    remaining: allocated === null ? null : Math.max(0, allocated - committed),
  };
}

/**
 * What the committed number is made of, and whether each part still has an agent behind it.
 *
 * IT EXISTS BECAUSE THE TOTAL ALONE IS UNANSWERABLE. `已承诺 200k` beside a project with nobody assigned
 * gave the operator no way to ask what the 200k was; finding out meant fetching the engagement list,
 * fetching the agent list, and cross-referencing them by hand. Three of those four commitments belonged to
 * `e2e-probe-*` agents deleted hours earlier.
 *
 * SEPARATE FROM `sideBudgetFor` ON PURPOSE, and this was a correction. Adding the breakdown to that helper
 * widened all four of its callers, one of which builds a payload handed to a DISPATCHED RUNNER — which
 * would have told a borrowed agent's wrapper the names and sizes of every other commitment on that
 * customer's side. The numbers are an internal primitive; who holds them is an operator-facing read.
 *
 * `agentExists` IS COMPUTED, not stored. An engagement referring to a deleted agent is precisely the state
 * being reported, so a stored flag would have to be maintained by the same delete path that failed to
 * maintain the commitment in the first place.
 *
 * A DELETE NO LONGER LEAVES ONE BEHIND (see `app.delete('/api/agents/:name')`), so on a fleet running this
 * code `orphanedCommitted` should stay 0. It is reported anyway: fleets predating that fix still hold
 * orphans, and a number that is normally zero is the useful kind to show.
 */
function sideCommitmentsFor(side) {
  const all = engagementStore.commitmentsForProjectSide(side.serverName)
    .map((row) => ({
      ...row,
      agentExists: Boolean(row.agent && isAgentRecord(agents[normalizeAgentName(row.agent)])),
    }));
  const commitments = all.filter(row => !usesResourcePool(engagementStore.get(row.id)));
  const poolCommitments = all.filter(row => usesResourcePool(engagementStore.get(row.id)));
  return {
    commitments,
    poolCommitments,
    poolCommitted: poolCommitments.reduce((sum, row) => sum + row.allocatedTokens, 0),
    totalCommitted: all.reduce((sum, row) => sum + row.allocatedTokens, 0),
    orphanedCommitted: commitments
      .filter((row) => !row.agentExists)
      .reduce((n, row) => n + (row.allocatedTokens ?? 0), 0),
  };
}

/**
 * The project side a room belongs to, or null when the id carries no server.
 *
 * A room id is `!local:server.name`, and without federation the server part IS the project side —
 * which is what makes a room id enough to attribute spend, with no extra field to keep in sync.
 */
function sideIdForRoom(roomId) {
  const text = typeof roomId === 'string' ? roomId : '';
  const at = text.indexOf(':');
  return at > 0 ? text.slice(at + 1).toLowerCase() : null;
}

/**
 * ADR-016 decision 6, at the points where a borrower's allocation is actually committed.
 *
 * WHY THIS MOVED. The first build of this gate sat on `POST /api/dispatch` Phase 4 — the only
 * auto-provisioning path that existed when the ADR was written. Two things were wrong with that as the
 * load-bearing gate: ADR-013 decision 8 withdraws `/api/dispatch` "and any successor router-facing
 * assignment path", and that route has no product caller, so the gate protected a road nobody drives.
 * ADR-016's own decision TEXT says an agent instance is minted "on acceptance" — the engagement path.
 * Only its status rows pointed at dispatch. This helper is where those rows should have pointed.
 *
 * A REFUSAL, NOT A ROUTING OUTCOME — which is why this is not inside `routeRequest`. Every value in
 * that vocabulary (`notWhitelisted`, `overOffer`, `overCeiling`) ends in a PENDING engagement the
 * contributor can then decide, and a queue entry is the wrong answer here: it reads as "waiting for
 * capacity" when the truth is "will not proceed without more budget". The operator asked for an alarm
 * — 「应该报警，说无法创建 agent，需要加预算」 — and a queue entry is not one.
 *
 * `requireSide` is the ONE difference between the callers, and it is not arbitrary:
 *
 *   - MINTING an agent needs a side, because under decision 1 the identity comes from the side's
 *     credential. No side, no agent — so `no_project_side` is a real refusal there.
 *   - SERVING an engagement does not. The agent already exists and is already reachable, so a room on
 *     a server this deployment has not configured as a side is un-attributed, not unserviceable.
 *
 * THE LIMITATION, STATED: with `requireSide` false, an engagement on a server that has no side record
 * escapes the budget entirely. That is the migration state, not the design — every binding in this
 * deployment predates project sides. It is honest to leave the escape open and name it rather than to
 * refuse every existing engagement in the name of a budget nobody has allocated yet.
 *
 * Returns `true` when it has already answered the request, so a caller reads as
 * `if (refuseOverSideAllocation(...)) return;`.
 */
const SIDE_BUDGET_ALERT_KEY = (sideId) => `project_side_budget:${sideId}`;
const CEILING_OVERRUN_SWEEP_INTERVAL_MS = 3600_000;
const PROJECT_ROOM_MEMBERSHIP_SWEEP_INTERVAL_MS = 3600_000;

/**
 * THE ALARM the operator asked for: 「应该报警，说无法创建 agent，需要加预算」.
 *
 * The refusal already names the shortfall, but it goes back to WHOEVER ASKED — a borrower, or an
 * automated request — and the person who can fix it is the contributor's operator, who is not in that
 * conversation at all. Until this, decision 6 was half built: admission control worked and the alarm
 * did not exist, so the failure mode was silence on the only side that could act.
 *
 * DEDUPED BY SIDE, deliberately. A borrower retrying every minute must not produce a wall of alerts;
 * `ingest` increments `occurrences` on a repeat, so the retry rate becomes visible ON the one alert
 * rather than burying it.
 *
 * THE ACTIONABILITY FIELDS ARE NOT DECORATION. `buildActionability` silently DOWNGRADES a warning to
 * `info` unless it has owner-or-assignee, runbook, impact and recoveryCondition — so an alarm raised
 * carelessly is filed as a note and pages nobody, which is exactly the quiet non-alarm this exists to
 * prevent. Four of them are load-bearing; `correlation` is NOT, because `normalizeCorrelation`
 * synthesises `{alertType, dedupeKey}` from fields `ingest` already requires, so it can never be the
 * empty object that would trigger the downgrade. It is passed anyway to carry `sideId`, which the
 * synthesised form would not have. (Both facts read out of that function; mutation testing showed the
 * correlation half of an earlier version of this comment was wrong.)
 */
const CEILING_OVERRUN_ALERT_KEY = (agent) => `agent_ceiling_overrun:${agent}`;

/**
 * An agent that is ALREADY past its ceiling — the half of the alarm nothing raised.
 *
 * `project_side_budget` below fires when a request is REFUSED, which means it needs somebody to ask.
 * An overrun needs nobody: it happens when a ceiling is lowered under commitments that were
 * admissible when they were made, and admission control cannot retract those. So the state arrives
 * with no request to hang an alarm on, and until this the only place it appeared was a console meter —
 * visible to whoever happened to open the page, and to nobody else.
 *
 * PER AGENT, not per side. The ceiling being exceeded is the CONTRIBUTOR's, and one side's engagements
 * may run against several agents while one agent may serve several sides. Keying by side would merge
 * two different overruns and report one of their numbers.
 *
 * AUTO-RESOLVES when the overrun ends, by the same rule the budget alarm follows: raising the ceiling
 * or ending engagements is what fixes this, and an alert that survives its own fix trains an operator
 * to ignore alerts.
 */
function sweepCeilingOverruns() {
  for (const agent of Object.values(agents).filter(isAgentRecord)) {
    const name = agent?.name;
    if (!name) continue;
    const preset = agent.presetId ? frameworkPresets.find((p) => p.id === agent.presetId) : null;
    const ceiling = preset?.ceiling?.tokens;
    const dedupeKey = CEILING_OVERRUN_ALERT_KEY(name);
    if (!Number.isFinite(ceiling)) continue;

    const { reserved, spent } = ceilingSpendFor(name);
    /*
     * `max(committed, measured)` — the same figure `remainingFor` draws on, so this alarm and the
     * admission decision cannot disagree about whether an agent is over. An unknown spend falls back
     * to the allocation rather than to zero, for the reason stated there.
     */
    const drawn = spent === null ? reserved : Math.max(reserved, spent);
    const over = drawn - ceiling;
    if (over <= 0) {
      /*
       * `autoResolve`, the same call the allocation route makes when a raise actually helps — not a
       * hand-rolled transition. It is a no-op when no such alert is open, so the common case (an agent
       * comfortably inside its ceiling) costs a map lookup.
       */
      try {
        alertStore.autoResolve(dedupeKey);
      } catch { /* a stale alert is better than a sweep that dies on one agent */ }
      continue;
    }
    try {
      alertStore.ingest({
        alertType: 'agent_ceiling_overrun',
        dedupeKey,
        severity: 'warning',
        source: 'backend',
        sourceAgent: name,
        summary: `${name} has drawn ${drawn} against a ceiling of ${ceiling} — ${over} past it`,
        detail: {
          agent: name,
          presetId: agent.presetId ?? null,
          ceilingTokens: ceiling,
          committedTokens: reserved,
          measuredTokens: spent,
          drawnTokens: drawn,
          overByTokens: over,
        },
        owner: 'hagency-operator',
        runbook: `raise the ceiling on preset ${agent.presetId ?? '(none)'} to cover what is already `
          + `committed, or revoke engagements on ${name} until the drawn figure is back under it`,
        impact: 'no new engagement can be approved against this agent; the work already approved keeps '
          + 'running, because admission control cannot retract a commitment it already granted',
        recoveryCondition: 'the drawn figure falls back under the ceiling, by raising the ceiling or '
          + 'ending engagements — this alert auto-resolves when that happens',
        correlation: { dedupeKey, agent: name, presetId: agent.presetId ?? null },
        tags: ['ceiling', `agent:${name}`, 'budget'],
      });
    } catch (error) {
      console.warn(`[ceiling] failed to raise overrun alarm for ${name}: ${error?.message || error}`);
    }
  }
}

function raiseSideBudgetAlarm(sideId, { reason, act, budget, wanted }) {
  try {
    alertStore.ingest({
      alertType: 'project_side_budget',
      dedupeKey: SIDE_BUDGET_ALERT_KEY(sideId),
      severity: 'warning',
      source: 'backend',
      summary: reason === 'no_allocation'
        ? `Project side ${sideId} has no token allocation, so it can fund no work`
        : `Project side ${sideId} is out of allocated tokens and is refusing work`,
      detail: {
        sideId,
        reason,
        refusedAct: act,
        requestedTokens: wanted ?? null,
        allocatedTokens: budget?.allocated ?? null,
        committedTokens: budget?.committed ?? null,
        remainingTokens: budget?.remaining ?? null,
      },
      owner: 'hagency-operator',
      runbook: `PUT /api/project-sides/${sideId}/allocation with a higher allocated_tokens, `
        + 'or accept that this side stays closed to new work',
      impact: 'engagements on this project side are refused at admission; nothing running is stopped, '
        + 'and no agent is created for it',
      recoveryCondition: 'the side is given an allocation that leaves headroom above what is already '
        + 'committed — this alert auto-resolves when that happens',
      correlation: { dedupeKey: SIDE_BUDGET_ALERT_KEY(sideId), sideId, reason },
      tags: ['project-side', `side:${sideId}`, 'budget'],
    });
  } catch (error) {
    // An alarm that cannot be filed must not turn a clean refusal into a 500.
    console.warn(`[project-side] failed to raise budget alarm for ${sideId}: ${error?.message || error}`);
  }
}

function refuseOverSideAllocation(res, { projectRoomId, tokens, act, requireSide = false, publicResult = false, extra = {} }) {
  const refuse = (reason, error, fields = {}) => {
    res.status(409).json(publicResult
      ? { status: 'refused', reason: 'contribution_unavailable', error: 'The contributor cannot accept this request; contact its operator.' }
      : { status: 'refused', reason, error, ...extra, ...fields });
    return true;
  };
  const sideId = sideIdForRoom(projectRoomId);
  if (!sideId) {
    if (!requireSide) return false;
    return refuse('no_project_side', `${act} names no project side, so no budget can be charged for it`);
  }
  const budget = sideBudgetFor(sideId);
  if (!budget) {
    if (!requireSide) return false;
    /*
     * NO ALARM HERE, on purpose. This is a configuration gap, not an exhausted budget: there is no side
     * to attribute an alert to, so a per-side dedupe key would be a key for something that does not
     * exist, and any room id an unauthenticated caller invents would mint another alert. The refusal
     * still names the server.
     */
    return refuse('no_project_side',
      `no project side is configured for ${sideId}, so no budget can be charged for ${act}`, { sideId });
  }
  if (budget.allocated === null) {
    // UNALLOCATED IS NOT UNLIMITED, so the alarm arrives before the tokens rather than after.
    raiseSideBudgetAlarm(sideId, { reason: 'no_allocation', act, budget, wanted: tokens });
    return refuse('no_allocation',
      `project side ${sideId} has no token allocation — set one before ${act} can draw on it`,
      { sideId, ...budget });
  }
  const wanted = Number.isFinite(Number(tokens)) ? Math.max(0, Math.floor(Number(tokens))) : 0;
  if (wanted > budget.remaining) {
    raiseSideBudgetAlarm(sideId, { reason: 'over_allocation', act, budget, wanted });
    return refuse('over_allocation',
      `${act} needs ${wanted} tokens and project side ${sideId} has ${budget.remaining} of `
      + `${budget.allocated} left (${budget.committed} committed) — raise the allocation`,
      {
        sideId,
        allocatedTokens: budget.allocated,
        committedTokens: budget.committed,
        remainingTokens: budget.remaining,
        requestedTokens: wanted,
      });
  }
  return false;
}

/*
 * Set a side's allocation. The operator's 「真配额」.
 */
app.put('/api/project-sides/:id/allocation', requireBearer, (req, res) => {
  try {
    const raw = req.body?.allocated_tokens ?? req.body?.allocatedTokens;
    const side = projectSideStore.setAllocation(req.params.id, raw === undefined ? null : raw);
    if (!side) return res.status(404).json({ error: 'project side not found' });
    const budget = sideBudgetFor(side.id);
    /*
     * RESOLVED ONLY IF THE RAISE ACTUALLY HELPED. `remaining > 0` and not null, rather than "the
     * allocation changed": raising 100k to 200k against 300k already committed leaves the side just as
     * unable to fund work, and closing the alarm there would report a recovery that did not happen —
     * the operator would go back to a console showing nothing wrong and work still refused.
     */
    if (budget && budget.allocated !== null && budget.remaining > 0) {
      alertStore.autoResolve(SIDE_BUDGET_ALERT_KEY(side.id));
    }
    return res.json({ ok: true, side, budget });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to set allocation');
  }
});

/*
 * What a side has, has promised, and has left. Separate from GET /:id because it reaches across into
 * the engagement store, and a read of the side record should not silently depend on another store
 * being consistent.
 */
app.get('/api/project-sides/:id/budget', requireBearer, (req, res) => {
  const budget = sideBudgetFor(req.params.id);
  if (!budget) return res.status(404).json({ error: 'project side not found' });
  const side = projectSideStore.getSide(req.params.id);
  return res.json({ ok: true, sideId: side.id, ...budget, ...sideCommitmentsFor(side) });
});

/*
 * The credential that lets the bridge ACT as a representative on a project side.
 *
 * A SECOND, WIDER GRANT THAN `inbound-credentials`, AND A SEPARATE ENDPOINT ON PURPOSE. That one was
 * scoped to `hsToken` with the note that an `as_token` "acts AS an agent on that homeserver" and would
 * need "a separate grant with its own argument". This is that argument, and the two are kept apart so
 * that a caller wanting only inbound authentication cannot receive acting authority as a side effect of
 * one response shape widening.
 *
 * WHAT THE DIFFERENCE ACTUALLY IS:
 *   - `hsToken` authenticates a push INTO us. Holding it lets someone impersonate the homeserver
 *     towards Hagency — bounded, and the receiver's own idempotency and routing bound it further.
 *   - `asToken` acts as ANY identity in the claimed namespace, plus the representative itself. Holding
 *     it lets someone speak, create rooms and join rooms as every lent agent on that side.
 *
 * WHY THE BRIDGE GETS IT ANYWAY. The bridge IS the component that acts on Matrix for Hagency; refusing
 * it acting credentials would leave the representative concept inert — nothing could create the
 * approval room a borrower decides in (ADR-016's resolved collision), and nothing could publish into
 * it. It already holds the bot's token and every agent's, so this extends an existing trust boundary
 * to project sides rather than creating a new one. What it does NOT do is widen it to the console: this
 * is bridge-secret guarded, and the console proxy's read pattern is written to exclude both credential
 * routes by name.
 *
 * SCOPED, and each scope is a decision:
 *   - active sides only. A deactivated side is closed to new work, and acting on it would be taking
 *     work from a side the operator closed.
 *   - only sides that HAVE a usable acting credential. A registration-token side whose representative
 *     has not been registered yet has nothing to act with, and returning it would make the bridge
 *     attempt sends that cannot succeed.
 */
/**
 * What can this deployment reach, and where can it be reached — the read behind an "add a project side"
 * form, so that flow stops being a shell recipe with a guessed callback URL.
 *
 * `requireBearer` because it names this host's network interfaces and its own homeserver configuration.
 * Neither is a secret in the token sense, and both describe the operator's infrastructure to anyone who
 * asks — which is exactly the sort of thing that should not be readable without the operator credential.
 *
 * IT PROBES ON EVERY CALL rather than caching. A cached "reachable" is the one answer that matters least:
 * an operator opens this form BECAUSE something is being set up or has broken, and a stale yes sends them
 * to debug a registration when the homeserver is simply down.
 */
/**
 * Probe a homeserver the operator names, because discovery alone is circular.
 *
 * `GET /api/matrix/reach` lists what this deployment already knows about — its own homeserver and sides
 * already recorded. A NEW customer is by definition in neither, so a form built only on that list can
 * never add the first one. The operator asked exactly this: 「为何不能加 chinasoft 客户端」.
 *
 * IT IS AN OPERATOR CAPABILITY AND IS GATED AS ONE. This makes the server fetch a URL chosen by the
 * caller, which is the shape of an SSRF — so it is behind `requireBearer`, the same credential that can
 * already create sides and read every budget. An operator holding that token can curl anything this host
 * can reach; this adds convenience for them, not reach for anyone else. It is deliberately NOT exposed to
 * any weaker credential, and the console proxy admits it under the same operator-only path as the rest.
 *
 * What comes back is a verdict, never the body: only whether it answered as a Matrix homeserver and which
 * versions it claims. A tool that echoed the response would turn this into a general-purpose fetcher.
 */
/**
 * Which callback address does the homeserver ACTUALLY reach us at — asked from inside it, when possible.
 *
 * The step this serves was a question the operator could not be expected to answer — 「这是什么，填什么」 —
 * about the one field whose wrong value produces a setup that looks installed and stays silent forever.
 * For a homeserver running as a container on this host, the system can simply ask from in there instead.
 *
 * The homeserver is identified by ITS OWN URL and the candidates are regenerated server-side; nothing
 * about which container to enter or which address to fetch comes from the request body. Accepting either
 * would turn this into "run curl to a URL of my choosing inside a container of my choosing".
 */
app.post('/api/matrix/callback-check', requireBearer, async (req, res) => {
  const homeserverUrl = typeof req.body?.homeserver_url === 'string' ? req.body.homeserver_url.trim() : '';
  const rawPort = String(process.env.HAGENCY_APPSERVICE_PORT ?? '').trim();
  const port = /^\d+$/.test(rawPort) ? Number(rawPort) : null;
  try {
    /*
     * The edge's URL travels too, because what the homeserver must reach depends on which way in is
     * configured — and with an edge it is the edge, not this host.
     */
    const edgeUrl = String(process.env.HAGENCY_EDGE_URL ?? '').trim()
      && String(process.env.HAGENCY_EDGE_LINK_TOKEN ?? '').trim()
      && String(process.env.HAGENCY_EDGE_SIDE ?? '').trim()
      ? String(process.env.HAGENCY_EDGE_URL).trim()
      : null;
    const result = await verifyCallbackFromHomeserver({
      homeserverUrl, port, edgeUrl, execFileImpl: execFile,
    });
    return res.json(result);
  } catch (error) {
    return res.status(500).json({ applicable: false, reason: `check failed: ${error?.message || error}` });
  }
});

app.post('/api/matrix/probe', requireBearer, async (req, res) => {
  const serverName = typeof req.body?.server_name === 'string' ? req.body.server_name.trim() : '';
  const url = typeof req.body?.url === 'string' ? req.body.url.trim() : '';
  if (!serverName && !url) return res.status(400).json({ error: 'server_name or url is required' });

  /*
   * THE NAME IS ENOUGH, and demanding an address as well was the mistake: 「服务器地址你应该知道」. Matrix
   * specifies how a name becomes an address, so the caller supplies the name and this does the lookup —
   * the same thing every Matrix client does on every login.
   *
   * An EXPLICIT url still wins when given. Well-known is not always configured, especially on a private
   * or port-bearing deployment, and an override that the discovery result could quietly replace would be
   * an override in name only.
   */
  let origin = originFor(url);
  let via = origin ? 'you gave the address explicitly' : null;
  if (!origin) {
    const discovered = await discoverBaseUrl(serverName || url);
    origin = discovered.url;
    via = discovered.via;
  }
  if (!origin) {
    return res.status(400).json({ error: 'could not turn that into an address', code: 'not_an_origin', via });
  }
  try {
    const probe = await probeHomeserver(origin);
    return res.json({ origin, via, probe });
  } catch (error) {
    return res.status(500).json({ error: `probe failed: ${error?.message || error}` });
  }
});

app.get('/api/matrix/reach', requireBearer, async (req, res) => {
  try {
    const sides = projectSideStore.listSides({ activeOnly: false });
    const reach = await describeMatrixReach({ env: process.env, sides });
    /*
     * ASK THE EDGE WHAT THE REGISTRATION MUST SAY, because Hagency cannot know it and guessing shipped a
     * registration that silently received nothing.
     *
     * `HAGENCY_EDGE_URL` is how Hagency COLLECTS from the edge. The registration needs the address the
     * HOMESERVER dials — the edge's own socket, which is loopback when co-located. Walked on a clean pair of
     * machines: the console pre-filled the collect address (a public IP), the homeserver could not reach it,
     * `verify` still answered `accepted` because it only proves the outbound direction, and the edge's own
     * counter read `transactions from the homeserver: 0`. Nothing on screen said inbound was dead.
     *
     * FAILING TO ASK IS REPORTED, NOT PAPERED OVER. If the edge cannot be reached, `edgeRegistrationUrl`
     * stays null and `edgeReachable` says so — an operator who cannot reach their own edge has a problem
     * worth seeing before they hand a registration to a customer, and falling back to the collect address
     * is exactly the guess that caused this.
     */
    if (reach?.appservice?.inboundVia === 'edge') {
      const link = String(process.env.HAGENCY_EDGE_LINK_TOKEN ?? '').trim();
      try {
        const probe = await fetch(`${reach.appservice.edgeUrl}/_hagency/edge/status`, {
          headers: { 'x-hagency-link': link },
          signal: AbortSignal.timeout(5000),
        });
        if (!probe.ok) throw new Error(`the edge answered HTTP ${probe.status}`);
        const body = await probe.json();
        reach.appservice.edgeReachable = true;
        reach.appservice.edgeRegistrationUrl = body?.registrationUrl ?? null;
        /*
         * WHETHER ANYTHING IS ACTUALLY ARRIVING, carried to the screen because `verify` cannot tell you.
         * Verification proves the OUTBOUND direction; a side can be `accepted` while nothing has ever come
         * in. The counters live in the edge process on the customer's machine, so this is the only place a
         * console could learn it.
         */
        reach.appservice.inbound = diagnoseEdgeInbound(body);
        reach.appservice.edgeTraffic = {
          transactions: body?.transactions ?? 0,
          delivered: body?.delivered ?? 0,
          rejected: body?.rejected ?? 0,
          collecting: Boolean(body?.hagencyWaiting) || Boolean(body?.hagencyLastSeenAt),
        };
        if (!body?.registrationUrl) {
          reach.appservice.edgeNote = 'this edge does not report which address the homeserver should dial, '
            + 'so it predates that field — upgrade it, or read the "put this in the registration" line it '
            + 'prints at startup.';
        }
      } catch (error) {
        reach.appservice.edgeReachable = false;
        reach.appservice.edgeRegistrationUrl = null;
        reach.appservice.inbound = { state: 'unknown', detail: 'the edge could not be asked' };
        reach.appservice.edgeNote = `Hagency cannot reach the edge at ${reach.appservice.edgeUrl}: `
          + `${error?.message || error}. Nothing will be collected until it can, so fix this before `
          + 'issuing a registration.';
      }
    }
    res.json(reach);
  } catch (error) {
    res.status(500).json({ error: `could not describe Matrix reachability: ${error?.message || error}` });
  }
});

app.get('/api/project-sides/acting-credentials', requireApprovalBridgeSecret, (req, res) => {
  try {
    const sides = projectSideStore.listSides({ activeOnly: true }).map((side) => {
      const credential = projectSideStore.credentialFor(side.id);
      if (!credential) return null;
      if (credential.kind === 'appservice') {
        return {
          sideId: side.id,
          serverName: side.serverName,
          apiBaseUrl: side.apiBaseUrl,
          kind: 'appservice',
          asToken: credential.asToken,
          ...(credential.transport ? { transport: credential.transport } : {}),
          representative: side.representative ?? null,
          senderLocalpart: credential.senderLocalpart,
          namespace: credential.namespace,
          /*
           * 16-impl-r5 rotation: the SAME derived identity the inbound projection emits, so the
           * bridge can compare the acting generation against the snapshot generation.
           */
          registration: derivedRegistrationId(side.id, credential.hsToken),
        };
      }
      if (credential.kind === 'registrationToken' && credential.representativeToken) {
        return {
          sideId: side.id,
          serverName: side.serverName,
          apiBaseUrl: side.apiBaseUrl,
          kind: 'registrationToken',
          representativeToken: credential.representativeToken,
          representative: side.representative ?? null,
          registration: derivedRegistrationId(side.id, credential.representativeToken),
        };
      }
      /*
       * A registration-token side with no representative token yet is OMITTED rather than returned
       * with a null. Returning it would have the bridge attempt sends that cannot succeed, and the
       * failure would look like a rejected credential instead of an unfinished setup.
       */
      return null;
    }).filter(Boolean);
    return res.json({ ok: true, sides });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to read acting credentials');
  }
});

/*
 * The inbound credentials the BRIDGE needs, and nothing else.
 *
 * A NARROW, DELIBERATE EXCEPTION to ADR-016 decision 8, argued rather than assumed. "Write-only" was
 * decided against the CONSOLE: a browser holding the operator token must not be able to read a
 * credential, because the console renders whatever an API returns and this repository has already
 * shipped API text into a UI nobody meant to show it twice. The bridge is a different principal — it
 * is the component that must authenticate a homeserver's push, and it cannot do that without the
 * token. Refusing it would not protect anything; it would only mean the appservice cannot work.
 *
 * The precedent is exact rather than analogous: `GET /api/approval-bindings` is bridge-secret guarded
 * and returns `ownerDmRoomId`, which is deliberately withheld from the console proxy's projection. A
 * bridge-secret endpoint is already the place where this repository returns what the console must not
 * see.
 *
 * SO THE EXCEPTION IS SCOPED THREE WAYS, and each one is a decision:
 *   - `hsToken` only. The `asToken` acts AS an agent on that homeserver, and inbound authentication
 *     does not need it. When the bridge needs to send as an agent that will be a separate grant with
 *     its own argument.
 *   - appservice sides only. A registration-token side has no inbound push, so it has nothing to
 *     authenticate here. This filter is REDUNDANT today and kept as depth: the store validates
 *     credential fields per kind, so a registration-token credential structurally cannot carry an
 *     `hsToken`, and the `credential?.hsToken` check below already excludes it. Mutation testing
 *     confirms the pair is equivalent — no test can distinguish them, because no credential with a
 *     non-appservice kind and an `hsToken` can be constructed through the store. The filter stays so
 *     that adding such a kind later does not silently widen this endpoint.
 *   - active sides only. A deactivated side is closed to new work, and a listener that still accepted
 *     its pushes would be accepting work from a side the operator has closed.
 */
app.get('/api/project-sides/inbound-credentials', requireApprovalBridgeSecret, (req, res) => {
  try {
    const sides = projectSideStore.listSides({ activeOnly: true })
      .filter((side) => side.credentialKind === 'appservice')
      .map((side) => inboundCredentialsProjection(side, projectSideStore.credentialFor(side.id)))
      .filter(Boolean);
    return res.json({ ok: true, sides });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to read inbound credentials');
  }
});

app.get('/api/project-sides/:id', requireBearer, (req, res) => {
  const side = projectSideStore.getSide(req.params.id);
  if (!side) return res.status(404).json({ error: 'project side not found' });
  // Same staffing join as the list read, so one side and all sides answer the same shape. A page that
  // drilled into a side and lost its third level would look like the agents had gone.
  return res.json({ ok: true, side: withProjectStaffing(side) });
});

/*
 * Create or update. The id IS the server name, so this is an upsert on that key and there is no
 * separate POST/PUT split: a homeserver has exactly one credential and one representative, and a
 * create-only route would let a second record claim the same server.
 */
app.post('/api/project-sides', requireBearer, (req, res) => {
  try {
    const side = projectSideStore.upsertSide({
      server_name: req.body?.server_name ?? req.body?.serverName,
      api_base_url: req.body?.api_base_url ?? req.body?.apiBaseUrl,
      label: req.body?.label,
      // Present-but-undefined is CARRIED FORWARD by the store; an explicit null clears. The console
      // can only write this field, so a form that saves a label must not erase it.
      ...(Object.prototype.hasOwnProperty.call(req.body ?? {}, 'credential')
        ? { credential: req.body.credential }
        : {}),
    });
    return res.json({ ok: true, side });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to save project side');
  }
});

/** Replace the credential alone. Write-only: the response is the projection, never the value. */
app.put('/api/project-sides/:id/credential', requireBearer, (req, res) => {
  try {
    /*
     * THE FIELD MUST BE PRESENT. `req.body?.credential ?? null` meant a body that did not mention a
     * credential DESTROYED the existing one and answered `ok: true` — which is what happened when a
     * caller sent the credential's own fields at the top level instead of nested. Clearing has to be an
     * explicit `credential: null`, because on this path the cost of an accidental wipe lands on somebody
     * else: re-issuing means the project side installs a new registration file and RESTARTS their
     * homeserver. A destructive default is wrong wherever the destruction is expensive; here it is
     * expensive for a person we cannot even reach.
     */
    if (!Object.prototype.hasOwnProperty.call(req.body ?? {}, 'credential')) {
      return res.status(400).json({
        error: 'credential is required: send { credential: {...} } to set one, or { credential: null } to withdraw it',
        code: 'bad_request',
      });
    }
    const side = projectSideStore.setCredential(req.params.id, req.body.credential);
    if (!side) return res.status(404).json({ error: 'project side not found' });
    return res.json({ ok: true, side });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to set credential');
  }
});

/*
 * Ask the homeserver whether our credential works, and record the answer.
 *
 * The verdict is stored rather than only returned, because "accepted" with no age cannot be told
 * apart from "accepted once, months ago, by a process that has since stopped running" — the argument
 * that put `membershipCheckedAt` beside `agentJoined`.
 *
 * A REJECTED credential and an UNREACHABLE homeserver are different answers and this endpoint says
 * which: only 401/403 is a verdict on the token. Reporting an outage as rejected sends an operator to
 * ask a project side for a new credential when theirs is fine, and on this path that means a human
 * doing account work on somebody else's homeserver.
 */
app.post('/api/project-sides/:id/verify', requireBearer, async (req, res) => {
  const existing = projectSideStore.getSide(req.params.id);
  if (!existing) return res.status(404).json({ error: 'project side not found' });
  try {
    /*
     * A STAGED CREDENTIAL IS TRIED FIRST, and that is what makes staging finish itself.
     *
     * The operator's remaining job is to install the file and restart their homeserver — Hagency cannot do
     * either, and should not: writing to a customer's filesystem and restarting their Matrix server are
     * exactly the authorities `docs/FOR-PROJECT-SIDES.md` promises never to take. But it CAN notice the
     * moment the install lands, and promote without being asked. So the chore shrinks to "install it", with
     * no separate "now tell Hagency" step.
     *
     * The live credential is still tried if the staged one fails, so a verify during the window between
     * issuing and installing reports the truth about what is currently working rather than a failure.
     */
    const staged = projectSideStore.pendingCredentialFor(req.params.id);
    let credential = projectSideStore.credentialFor(req.params.id);
    let promoted = false;
    let result = null;

    if (staged) {
      const stagedResult = await ensureRepresentative({ side: existing, credential: staged });
      if (stagedResult.accessState === 'accepted') {
        projectSideStore.promotePendingCredential(req.params.id);
        credential = projectSideStore.credentialFor(req.params.id);
        promoted = true;
        // Reused rather than re-asked. The homeserver just answered for this exact credential, and a second
        // round trip could only disagree with the first — which would leave the promotion already written.
        result = stagedResult;
      }
    }
    if (!result) result = await ensureRepresentative({ side: existing, credential });

    /*
     * A newly minted representative token is stored BEFORE the verdict. If the order were reversed
     * and the process died between the two writes, the token would be lost while the homeserver
     * believed the account existed — and the localpart would then be taken, so re-registering would
     * fail. Storing the credential first makes the worst case a missing verdict, which the next
     * verify recomputes.
     */
    if (result.credentialPatch) {
      projectSideStore.setCredential(req.params.id, { ...credential, ...result.credentialPatch });
    }
    projectSideStore.observeAccess(req.params.id, {
      state: result.accessState,
      detail: result.detail,
    });
    if (result.mxid) {
      /*
       * Guarded: the store refuses an MXID whose server is not this side's, which is the check that
       * stops the federation assumption creeping back in. A refusal here must not turn a successful
       * verification into a 500, so it is recorded as a bad_request against the side rather than
       * discarding the access verdict already written above.
       */
      try {
        projectSideStore.setRepresentative(req.params.id, { mxid: result.mxid });
      } catch (error) {
        return res.status(400).json({
          error: error?.message || 'representative mxid refused',
          code: 'representative_rejected',
          side: projectSideStore.getSide(req.params.id),
        });
      }
    }
    /*
     * `promoted` is reported because it is a change the operator did not ask for in this call and would
     * otherwise only infer from a field going away. It is the answer to "did my new credential take effect".
     */
    return res.json({ ok: true, promoted, side: projectSideStore.getSide(req.params.id) });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to verify project side');
  }
});

/*
 * Generate the appservice registration a project side installs.
 *
 * THE `url` IS AN INPUT AND CANNOT BE DERIVED. Hagency does not know what address it has in the
 * project side's eyes — a tunnel hostname, a public IP, `host.docker.internal` for a homeserver in a
 * container on the same machine. Guessing one produces a registration that installs cleanly and never
 * delivers anything, which is the failure mode hardest to diagnose from the project side: an
 * appservice that is configured and silent.
 *
 * THE RESPONSE IS THE ONLY TIME THE TOKENS ARE READABLE. ADR-016 decision 8 makes a project side's
 * credential write-only, and this endpoint stores the generated tokens into it — so `GET` will never
 * return them again and there is no second chance to copy the YAML. That is stated in the response
 * rather than left as a surprise, because the recovery is regenerating, which invalidates the
 * registration the project side may already have installed.
 *
 * Replacing an existing credential is therefore refused unless `?replace=true`. The tokens in a
 * registration a homeserver has already loaded stop working the moment new ones are stored, and that
 * takes the project side down until they install the new file and restart.
 */
/**
 * The same registration, DELIVERED AS A FILE, so a setup flow can be a UI without a namespace-wide
 * credential passing through a browser.
 *
 * WHY A SECOND ENDPOINT RATHER THAN A FLAG. The console proxy's allowlist matches method and path, not
 * request bodies, so a `deliver: "file"` option on the endpoint below could not be admitted without also
 * admitting the form that returns tokens. A separate path is the only shape the allowlist can tell apart —
 * and the proxy's refusal to forward the token-returning one is a decision its own comment marks as
 * deliberate, not an oversight to route around.
 *
 * WHAT THE CALLER GETS: a path, a fingerprint, and the facts needed to finish the install. What it never
 * gets is `as_token` or `hs_token`. That keeps the operator's browser — its memory, its devtools, its
 * history, its extensions — out of the only moment those two values are readable, while still letting the
 * flow be driven by clicking rather than by pasting curl commands whose traps are documented in
 * `docs/FOR-PROJECT-SIDES.md`.
 *
 * THE FILE IS 0600 AND UNDER THE RUNTIME DIRECTORY, not the repo. A registration in a working tree is a
 * registration one `git add -A` away from a public commit; this repo is public.
 */
app.post('/api/project-sides/:id/registration-file', requireBearer, (req, res) => {
  const side = projectSideStore.getSide(req.params.id);
  if (!side) return res.status(404).json({ error: 'project side not found' });
  const url = typeof req.body?.url === 'string' ? req.body.url.trim() : '';
  if (!url) {
    return res.status(400).json({
      error: 'url is required: the address this project side\'s homeserver can reach Hagency at',
      code: 'bad_request',
    });
  }
  /*
   * NO 409, AND NO `?replace=true` NEEDED, because issuing no longer breaks anything. A second issue is
   * STAGED: the live credential keeps working and the new one waits until a `verify` proves the homeserver
   * accepts it.
   *
   * The 409 was there because issuing used to invalidate a registration the homeserver already had. It made
   * the dangerous act explicit, which was right — but the danger was avoidable, and the operator who hit it
   * was handed a repair job for a state Hagency had created by replacing something that worked.
   */
  /*
   * STAGED ONLY OVER A CREDENTIAL THAT IS KNOWN TO WORK, and a walkthrough is what found the difference.
   *
   * Staging exists to protect something working. A side holding a credential that verification has already
   * REJECTED or found BLOCKED has nothing worth protecting — and parking the replacement behind it means the
   * operator generates a fix, sees "the credential this side is USING has not changed", and is left with the
   * broken one live. The reassurance becomes the obstacle.
   *
   * `unverified` counts as worth protecting. It means nobody has asked yet, not that it fails, and replacing an
   * unexamined credential outright would recreate the original defect for anyone who had not run verify.
   */
  const liveIsBroken = side.accessState === 'rejected' || side.accessState === 'blocked';
  const staging = Boolean(side.hasCredential) && !liveIsBroken;

  /*
   * REFUSED WITHOUT A RUNTIME DIRECTORY rather than falling back to the repo or to a temp path. A
   * credential written somewhere the operator did not choose is a credential nobody will remember to
   * remove, and `HAGENCY_RUNTIME_DIR` is the one location this deployment has already declared as the
   * place its secrets live.
   */
  const runtimeDir = String(process.env.HAGENCY_RUNTIME_DIR || '').trim();
  if (!runtimeDir) {
    return res.status(503).json({
      error: 'HAGENCY_RUNTIME_DIR is not set, so there is no declared place to write a credential',
      code: 'no_runtime_dir',
      detail: 'start the services with the runtime .env sourced, or use the terminal form of this endpoint',
    });
  }

  try {
    const registration = generateRegistration({
      id: req.body?.registration_id || `hagency-${side.id}`,
      url,
      senderLocalpart: req.body?.sender_localpart || 'hagency',
      userNamespaceRegex: req.body?.user_namespace || `@${MATRIX_AGENT_PREFIX_FOR_REGISTRATION}.*`,
      userNamespaceExclusive: req.body?.exclusive !== false,
    });
    const yaml = renderRegistrationYaml(registration);

    const dir = path.join(runtimeDir, 'registrations');
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    const file = path.join(dir, `${side.id.replace(/[^\w.-]/g, '_')}.yaml`);
    /*
     * Written with the mode in the open, not chmod'd after. A file that exists world-readable for even a
     * moment has already been readable, and this one authorises a namespace on somebody else's server.
     */
    writeFileSync(file, yaml, { mode: 0o600 });

    projectSideStore.setCredential(side.id, {
      kind: 'appservice',
      asToken: registration.as_token,
      hsToken: registration.hs_token,
      namespace: registration.namespaces.users[0].regex,
      senderLocalpart: registration.sender_localpart,
      // Remembered so a later reissue reuses the address this one was built for.
      url,
    }, { stage: staging });

    return res.json({
      ok: true,
      staged: staging,
      path: file,
      mode: '0600',
      registrationId: registration.id,
      senderLocalpart: registration.sender_localpart,
      representative: `@${registration.sender_localpart}:${side.serverName}`,
      namespace: registration.namespaces.users[0].regex,
      url,
      /*
       * A FINGERPRINT, so the operator can confirm the file they installed is the one this issued without
       * either of them ever reading a token. Four bytes: enough to say "these differ", useless to
       * authenticate with.
       */
      asTokenFingerprint: createHash('sha256').update(registration.as_token).digest('hex').slice(0, 8),
      hsTokenFingerprint: createHash('sha256').update(registration.hs_token).digest('hex').slice(0, 8),
      /*
       * NAMED, NOT DETECTED. `/_matrix/federation/v1/version` would identify the software, and neither
       * Palpo instance on the host that walked this answers it — so a step that claimed to know which
       * homeserver you run would be guessing. Both keys are given instead, and the trap is stated as one
       * that was verified rather than as general advice.
       */
      /*
       * SAID FIRST when this is a spare, because the operator's mental model after clicking "generate" is
       * "done" — and the one thing they must know is that nothing changed yet.
       */
      ...(staging ? {
        stagedNote: 'The credential this side is USING has not changed. This new one is held until you '
          + 'install it and verification proves the homeserver accepts it, so nothing breaks in the '
          + 'meantime — and if you generated it by mistake, ignore the file and nothing happens.',
      } : {}),
      /*
       * SAID WHEN IT REPLACED SOMETHING, because "not staged" has two very different causes: there was nothing
       * there, or what was there was broken. An operator replacing a broken credential should know the old one
       * is gone rather than infer it from the absence of a note.
       */
      ...(liveIsBroken ? {
        replacedNote: `The previous credential was ${side.accessState} and has been replaced rather than `
          + 'held back — there was nothing working to protect.',
      } : {}),
      nextSteps: [
        'Put this file where your homeserver reads appservice registrations. The key differs by software: '
        + 'Synapse takes `app_service_config_files` (a list of FILES); Palpo takes '
        + '`appservice_registration_dir` (a DIRECTORY, so the file goes inside it).',
        'In a TOML config the key must be TOP-LEVEL, above every [section]. Verified on Palpo: placed '
        + 'after a section header it becomes `<that section>.appservice_registration_dir` and is silently '
        + 'ignored — everything then fails as though the token were wrong.',
        'Restart the homeserver once. Registrations load at startup only, so nothing happens until it '
        + 'does.',
        'Replacing tokens later needs more than replacing this file: Palpo persists registrations in its '
        + 'database keyed by id, and a restart will not update an existing row.',
        'The representative above arrives with users_default power. A default Matrix room requires power '
        + '50 to invite, so either grant it that or invite each agent yourself — the approval response '
        + 'names which agent it assigned.',
      ],
    });
  } catch (error) {
    return res.status(500).json({ error: `could not issue registration: ${error?.message || error}` });
  }
});

app.post('/api/project-sides/:id/registration', requireBearer, (req, res) => {
  const side = projectSideStore.getSide(req.params.id);
  if (!side) return res.status(404).json({ error: 'project side not found' });
  const url = typeof req.body?.url === 'string' ? req.body.url.trim() : '';
  if (!url) {
    return res.status(400).json({
      error: 'url is required: the address this project side\'s homeserver can reach Hagency at',
      code: 'bad_request',
    });
  }
  if (side.hasCredential && req.query?.replace !== 'true') {
    return res.status(409).json({
      error: 'this project side already has a credential; pass ?replace=true to issue new tokens',
      code: 'credential_exists',
      detail: 'replacing invalidates the registration the homeserver may already have installed, '
        + 'which stops delivery until the new file is installed and the homeserver restarted',
    });
  }
  try {
    const registration = generateRegistration({
      id: req.body?.registration_id || `hagency-${side.id}`,
      url,
      senderLocalpart: req.body?.sender_localpart || 'hagency',
      userNamespaceRegex: req.body?.user_namespace || `@${MATRIX_AGENT_PREFIX_FOR_REGISTRATION}.*`,
      userNamespaceExclusive: req.body?.exclusive !== false,
    });
    const stored = projectSideStore.setCredential(side.id, {
      kind: 'appservice',
      asToken: registration.as_token,
      hsToken: registration.hs_token,
      namespace: registration.namespaces.users[0].regex,
      senderLocalpart: registration.sender_localpart,
    });
    return res.json({
      ok: true,
      side: stored,
      /*
       * Returned once. Named `registrationYaml` rather than anything matching /credential/, because the
       * health writer's redaction guard drops such keys silently — ADR-014 decision 6 hit that and had
       * to rename a field after it vanished from a record.
       */
      registrationYaml: renderRegistrationYaml(registration),
      installPath: 'the homeserver\'s appservice_registration_dir',
      onlyChance: 'These tokens are not stored in readable form and will never be returned again. '
        + 'Save this file now; regenerating issues new tokens and invalidates this registration.',
      restartRequired: true,
    });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to generate registration');
  }
});

/*
 * 项目 under a 项目方 — the middle layer of 项目方 → 项目 → 外派员工.
 *
 * A project is a NAME and a ROOM, nothing more: the 接单员 is one per side (the operator's ruling) and
 * the budget is one per side, so a project holds neither. What it adds is the thing this product has
 * never stored — `groupForRoom(projectRoomId) || meta.group || projectRoomId` degrades to a raw room
 * id, which is why every binding in this deployment displays `!aXbY7pQ2:hq.example` instead of a name.
 *
 * `requireBearer`: which customers we work for, and under what names, is the contributor's own record.
 * No project-side caller writes here.
 */
/**
 * Mint an agent's Matrix identity ON THE PROJECT SIDE IT SERVES.
 *
 * THE CIRCULAR DEPENDENCY THIS BREAKS is the operator's opening question: 「agent 在没有接受项目邀请之前
 * 是不知道加入哪个 home server 的,所以你先创建了 biglittle 的 matrix id 是错的」. `agentUserId()` composes
 * `@ac_<name>:<MATRIX_SERVER_NAME>` from a module constant, so an identity existed before any project was
 * known — on a server that, without federation, the project cannot see. `mintAgentIdentity` was written
 * for this and had NO product caller: its 12 references were all in tests. This is the caller.
 *
 * THE SIDE IS RESOLVED HERE, NOT BY THE BRIDGE, because the bridge's eight `ensureAgentAccount` call
 * sites do not all know a room — threading one through them would be a refactor in service of a lookup
 * the backend can already do. Two sources, in order:
 *
 *   1. `agent.projectSide`, set from the provision plan and never from the agent's own request body.
 *   2. failing that, the agent's own BINDINGS: a binding names a room, a room id carries its server, and
 *      that server IS the side. This is what makes the endpoint work for agents that predate
 *      provisioning — including every agent in this deployment.
 *
 * NO SIDE IS A REFUSAL, not a fallback to `MATRIX_SERVER_NAME`. An agent serving nobody has no side to
 * hold an identity on, and quietly minting on our own server is precisely the bug above.
 *
 * WHAT EACH CREDENTIAL KIND YIELDS, stated because they differ and only one is complete end to end:
 *
 *   appservice        an MXID and NO token. The namespace already authorises it, so nothing is
 *                     registered and Hagency acts as the agent by masquerading (`?user_id=`). The
 *                     identity is real; the bridge's send path still wants a per-agent token, so an
 *                     appservice-minted agent can be ADDRESSED but cannot yet SEND. Named as the next
 *                     gap rather than hidden behind a success.
 *   registrationToken an MXID and a real access token, from a real `/register` on their homeserver.
 *                     Complete: the bridge stores it and sends with it.
 */
app.post('/api/agents/:name/matrix-identity', requireBearer, async (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });

  let sideId = agent.projectSide ?? null;
  let source = 'agent.projectSide';
  if (!sideId) {
    try {
      const rooms = approvalStore.listBindings({ agent: agentName }).map((b) => b.projectRoomId);
      for (const room of rooms) {
        const candidate = sideIdForRoom(room);
        if (candidate && projectSideStore.getSide(candidate)) { sideId = candidate; source = `binding:${room}`; break; }
      }
    } catch { /* the refusal below is the same either way */ }
  }
  if (!sideId) {
    return res.status(409).json({
      error: `no project side can be determined for ${agentName}: it has no projectSide and no binding `
        + 'on a configured side. An identity is minted ON a side, so there is nowhere to mint one.',
      code: 'no_project_side',
    });
  }
  const side = projectSideStore.getSide(sideId);
  if (!side) return res.status(409).json({ error: `project side ${sideId} is not configured`, code: 'no_project_side' });

  let credential;
  try {
    credential = projectSideStore.credentialFor(sideId);
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to read the project side credential');
  }
  if (!credential) {
    return res.status(409).json({
      error: `project side ${sideId} has no credential, so nothing can be minted on it`,
      code: 'no_credential',
    });
  }

  try {
    const minted = await mintAgentIdentity({
      side,
      credential,
      localpart: `${agentPrefixOnSide(side, credential)}${agentName}`.toLowerCase(),
    });
    /*
     * `mintAgentIdentity` RETURNS a verdict rather than throwing — `{ minted: false, reason }` — and the
     * first version of this endpoint assumed a throw, so every refusal would have been answered 200 with
     * `mxid: null`. Found by reading the function instead of trusting the shape I expected: a caller that
     * reports "minted" for a refusal is worse than one that crashes, because the record it writes is
     * empty and nothing says so.
     */
    if (!minted.minted) {
      return res.status(409).json({
        error: `could not mint an identity for ${agentName} on ${sideId}: ${minted.reason || 'refused'}`,
        code: minted.reason || 'mint_refused',
        sideId,
        credentialKind: credential.kind,
      });
    }
    /*
     * The TOKEN is returned and not stored here. The bridge keeps agent credentials in its own
     * `bridge-state.json` — the store `agentMxid()` reads — and a second copy in the backend would be a
     * second thing to rotate. So this endpoint mints and answers; the caller persists.
     */
    return res.json({
      ok: true,
      agent: agentName,
      sideId,
      sideResolvedFrom: source,
      mxid: minted.mxid,
      credentialKind: credential.kind,
      // `null` for appservice, and that is the identity being complete rather than the mint failing.
      accessToken: minted.accessToken ?? null,
      /*
       * `canSend` is no longer "has a token". An appservice identity carries none and CAN send: the
       * bridge masquerades with the side's as_token and `?user_id=`, which is what the namespace is
       * for. The field answers the question a caller actually asks — will messages from this agent
       * reach a room — and answering it by token presence was true only while the send path had no
       * other way.
       */
      canSend: Boolean(minted.accessToken) || credential.kind === 'appservice',
      hasOwnToken: Boolean(minted.accessToken),
      note: minted.accessToken
        ? null
        : 'this identity carries no per-agent token; the bridge sends as it through the side\'s '
          + 'appservice credential, so the agent can be addressed AND can speak',
    });
  } catch (error) {
    const code = error?.code === 'taken' ? 409 : error?.code === 'bad_request' ? 400 : 502;
    return res.status(code).json({ error: error?.message || 'failed to mint the identity', code: error?.code ?? null });
  }
});

app.post('/api/project-sides/:id/projects', requireBearer, (req, res) => {
  try {
    const project = projectSideStore.upsertProject(req.params.id, req.body || {});
    if (!project) return res.status(404).json({ error: 'project side not found' });
    return res.json({ ok: true, project, side: projectSideStore.getSide(req.params.id) });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to save project');
  }
});

/*
 * ARCHIVE, and there is no DELETE — 「项目方暂时不可以删除,可以 archive 掉」. Reversible, because an
 * archive that cannot be undone is a delete with a gentler name; `archived: false` in the body restores.
 * A project's room carries the engagements served through it, so forgetting the project would leave
 * that history attributable to nothing.
 */
app.post('/api/project-sides/:id/projects/:projectId/archive', requireBearer, (req, res) => {
  try {
    const archived = req.body?.archived === undefined ? true : req.body.archived === true;
    const project = projectSideStore.setProjectArchived(req.params.id, req.params.projectId, archived);
    if (!project) return res.status(404).json({ error: 'project not found on this project side' });
    return res.json({ ok: true, project });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to archive project');
  }
});

/**
 * Knock on a project's published room — ADR-016 decision 5, the invite object.
 *
 * The operator supplies `#its-project:its-server`, which is the whole point of choosing an alias: it is
 * a handle a project can publish and a human can paste, with no token for us to issue or leak.
 *
 * A KNOCK IS NOT ACCESS AND NOT AN APPROVAL. It leaves membership in `knock` state until somebody on
 * the project's side invites us, and lending an agent stays a separate audited act — ADR-014's ruling
 * that 「joining a Discord costs the joiner nothing. Lending an agent spends tokens.」 is not softened by
 * making a project findable. So the response says `knocked` and reports the room it asked about; it
 * never reports a project as engaged.
 *
 * The alias is resolved FIRST and reported separately, because the two failures send an operator to
 * different places: an unresolvable alias is a typo or an unpublished room, and a refused knock is the
 * project's join rule. Answering both as one "failed" would hide which.
 */
app.post('/api/project-sides/:id/knock', requireBearer, async (req, res) => {
  try {
    const sideId = String(req.params.id || '').toLowerCase();
    const side = projectSideStore.getSide(sideId);
    if (!side) return res.status(404).json({ error: 'project side not found', code: 'no_project_side' });
    let credential;
    try {
      credential = projectSideStore.credentialFor(sideId);
    } catch (error) {
      return respondProjectSideError(res, error, 'failed to read the project side credential');
    }
    if (!credential) {
      return res.status(409).json({
        error: `project side ${sideId} has no credential, so the representative cannot knock`,
        code: 'no_credential',
      });
    }
    const alias = typeof req.body?.alias === 'string' ? req.body.alias.trim() : '';
    if (!alias) {
      return res.status(400).json({
        error: 'alias is required: the invite object is a published room alias, like #project:server',
        code: 'bad_request',
      });
    }

    const acting = { side: { apiBaseUrl: side.apiBaseUrl, serverName: side.serverName }, credential };
    const resolved = alias.startsWith('#')
      ? await resolveAliasOnSide({ ...acting, alias })
      : { resolved: true, roomId: alias, reason: null };
    if (!resolved.resolved) {
      return res.status(409).json({
        status: 'refused', reason: 'alias_unresolved', sideId, alias, error: resolved.reason,
      });
    }

    const knock = await knockOnRoomOnSide({
      ...acting,
      aliasOrRoomId: resolved.roomId,
      reason: typeof req.body?.reason === 'string' ? req.body.reason.slice(0, 200) : null,
    });
    if (!knock.knocked && !knock.already) {
      return res.status(409).json({
        status: 'refused',
        reason: knock.state === 'unsupported' ? 'knock_unsupported' : 'knock_refused',
        sideId, alias, roomId: resolved.roomId, error: knock.reason,
      });
    }
    return res.json({
      ok: true,
      sideId,
      alias,
      roomId: resolved.roomId,
      // `knock` is a state, not access: the project still has to invite us, and the contributor still
      // has to approve any lending. Both facts ride back so no caller has to assume either.
      state: knock.already ? 'already_member' : 'knocked',
      awaits: knock.already ? null : 'the project side invites the representative',
    });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to knock');
  }
});

app.post('/api/project-sides/:id/deactivate', requireBearer, (req, res) => {
  try {
    const side = projectSideStore.deactivateSide(req.params.id);
    if (!side) return res.status(404).json({ error: 'project side not found' });
    return res.json({ ok: true, side });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to deactivate project side');
  }
});

app.post('/api/project-sides/:id/reactivate', requireBearer, (req, res) => {
  try {
    const side = projectSideStore.reactivateSide(req.params.id);
    if (!side) return res.status(404).json({ error: 'project side not found' });
    return res.json({ ok: true, side });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to reactivate project side');
  }
});

/**
 * Retire the agents minted for a project side.
 *
 * ADR-016 decision 7, and "retire" is deliberately not "delete". Erasing an agent would erase its
 * consumption with it, and ADR-013 makes the token the unit of account — a closed period's totals
 * would change retroactively and the ledger would become falsifiable by deletion. So: the identity
 * stops being usable, the record and every ledger row stay, and the reason is recorded ON the agent so
 * an operator reading the roster later can see why it went quiet.
 *
 * The `inactive` shape is reused rather than a new lifecycle state invented, because every surface
 * that already understands "offline with a reason" understands this one for free.
 */
/**
 * End the engagements on a project side, and deactivate its bindings.
 *
 * ADR-016 decision 7's first three steps: end engagements → release commitments → deactivate bindings.
 * The operator stated the rule that shapes all of it: 「记录要留存，只是停用退役，删除会有合规问题」.
 *
 * ENDING RELEASES THE COMMITMENT FOR FREE, which is why these are one step rather than three.
 * `committedFor` and `committedForProjectSide` both sum only `active` engagements, so the transition to
 * `ended` is what frees the tokens — no separate release, and no figure that can disagree with the
 * state it was derived from.
 *
 * WHY THIS IS NOT A COSMETIC TIDY-UP. An `active` engagement is a claim about the PRESENT: work in
 * progress, tokens promised. Left behind on a side that no longer exists, with an agent that has been
 * retired, it goes on holding allocation and the console goes on showing work that will never proceed.
 * A binding left `active: true` claims the project can still reach the agent. This repository has
 * already produced that exact pair by another route: force-deleting an agent left three active bindings
 * and a 250k commitment pointing at nothing.
 *
 * Nothing is deleted. `ended` and `active: false` are history; `active` is a claim.
 */
function endEngagementsAndBindingsForSide(sideId, reason) {
  const endedEngagements = [];
  for (const engagement of engagementStore.list().filter((e) => e.state !== 'ended')) {
    const room = typeof engagement.projectRoomId === 'string' ? engagement.projectRoomId : '';
    const at = room.indexOf(':');
    if (at <= 0 || room.slice(at + 1).toLowerCase() !== sideId) continue;
    try {
      if (engagement.state === 'pending') engagementStore.decide({ engagementId: engagement.id, approve: false, by: 'operator', reason });
      else engagementStore.revoke({ engagementId: engagement.id, by: 'operator', reason });
      endedEngagements.push(engagement.id);
    } catch (error) {
      /*
       * Reported per engagement rather than aborting the cascade. One engagement that cannot be ended
       * must not leave the remaining ones active AND the side removed, which is the worst combination:
       * allocation held by records nothing can reach.
       */
      throw new Error(`could not end engagement ${engagement.id}: ${error?.message || error}`);
    }
  }

  const deactivatedBindings = [];
  for (const binding of approvalStore.listBindings({})) {
    const room = typeof binding.projectRoomId === 'string' ? binding.projectRoomId : '';
    const at = room.indexOf(':');
    if (at <= 0 || room.slice(at + 1).toLowerCase() !== sideId) continue;
    try {
      if (approvalStore.deactivateBinding(binding.agent, binding.projectRoomId, reason)) {
        deactivatedBindings.push(`${binding.agent}@${binding.projectRoomId}`);
      }
    } catch (error) {
      throw new Error(`could not deactivate binding for ${binding.agent}: ${error?.message || error}`);
    }
  }

  return { endedEngagements, deactivatedBindings };
}

function retireAgentsForSide(sideId) {
  const retired = [];
  const snapshots = new Map();
  for (const agent of Object.values(agents)) {
    if (!isAgentRecord(agent) || agent.projectSide !== sideId) continue;
    snapshots.set(agent.name, { ...agent });
    agent.tmux = null;
    agent.online = false;
    agent.offlineReason = `retired:project-side-removed:${sideId}`;
    agent.retiredAt = Date.now();
    retired.push(agent.name);
  }
  if (retired.length && !saveJson('agents.json', agents, { immediate: true })) {
    for (const [name, snapshot] of snapshots) agents[name] = snapshot;
    throw new Error('agent retirement could not be persisted; side credentials retained');
  }
  return retired;
}

/*
 * Forget a side, taking its agents with it.
 *
 * ORDER IS LOAD-BEARING and this endpoint performs only the part it owns. Decision 7's sequence is:
 * end engagements → release commitments → deactivate bindings → retire identities → forget the
 * credential LAST, because leaving rooms and revoking tokens all need the credential that removal
 * destroys. Two of those steps are done here — retiring identities, then forgetting the side — and the
 * two that are not are named in the response rather than implied by a bare 200.
 *
 * An ACTIVE side is still refused (409): removing it drops the credential every earlier step needs.
 * `?force=true` overrides, matching `DELETE /api/agents/:name`.
 */
app.delete('/api/project-sides/:id', requireBearer, async (req, res) => {
  try {
    const existing = projectSideStore.getSide(req.params.id);
    if (!existing) return res.status(404).json({ error: 'project side not found' });
    /*
     * THE PRECONDITION IS CHECKED BEFORE ANYTHING IS RETIRED, and it is checked here rather than left
     * to `removeSide`.
     *
     * A bug I wrote and a test caught: retiring first and letting `removeSide` throw `side_active`
     * meant a REFUSED delete had already taken the side's agents down. The operator sees a 409, keeps
     * the side, and every agent on it has silently gone quiet. Writing the comment about ordering is
     * what produced it — the order was right, the guard was missing from the front of it.
     */
    if (existing.active && req.query?.force !== 'true') {
      return res.status(409).json({
        error: `project side ${existing.id} is still active — deactivate it (and end its engagements) `
          + 'before removing, or pass force',
        code: 'side_active',
      });
    }
    /*
     * Agents are retired BEFORE the side is forgotten. Reversed, a persistence failure on the removal
     * would leave a live side whose agents had already been retired — work refused by a side the
     * operator still sees as configured, which is the confusing half of the two.
     */
    /*
     * ORDER, and it is ADR-016 decision 7's: end engagements and release their commitments, deactivate
     * bindings, retire identities, forget the credential LAST. Each earlier step may need the
     * credential the last one destroys, and each later step is meaningless if an earlier one left a
     * live claim behind.
     */
    // Deactivation gates new admission while remote withdrawals are in flight.
    projectSideStore.deactivateSide(existing.id);
    // A pending join must finish (and compensate on seeing deactivation) while
    // its side credential still exists. No new fulfillment can pass admission.
    await Promise.allSettled([...engagementFulfillments.entries()]
      .filter(([id]) => sideIdForRoom(engagementStore.get(id)?.projectRoomId) === existing.id)
      .map(([, work]) => work));
    if (matrixWorkStore.unsettled(existing.id)) return res.status(409).json({
      ok: false, code: 'cleanup_incomplete', error: 'Matrix bridge work is still pending; credentials are retained for retry.',
      side: projectSideStore.getSide(existing.id),
    });
    const memberships = new Map();
    for (const row of [...engagementStore.list(), ...approvalStore.listBindings({ includeInactive: true })]) {
      if (row.agent && sideIdForRoom(row.projectRoomId) === existing.id
        && (row.allocatedTokens > 0 || row.ownerMxid)) {
        memberships.set(`${row.agent}\0${row.projectRoomId}`, { agent: row.agent, roomId: row.projectRoomId });
      }
    }
    const withdrawals = [];
    for (const membership of memberships.values()) {
      withdrawals.push({ ...membership, ...await withdrawAgentFromProjectRoom(membership.agent, membership.roomId) });
    }
    const representativeRooms = new Set([
      ...existing.projects.map((p) => p.roomId),
      ...[...memberships.values()].map((m) => m.roomId),
    ].filter(Boolean));
    const credential = projectSideStore.credentialFor(existing.id);
    if (existing.representative?.mxid && !matrixWorkStore.loggedOut(null, existing.id)) {
      for (const roomId of representativeRooms) {
        withdrawals.push({ agent: null, roomId, ...await leaveRoomOnSideAsRepresentative({ side: existing, credential, roomId }) });
      }
    }
    const abandonUnreachable = req.query?.force === 'true' && req.query?.abandon_unreachable === 'true';
    const terminalCleanupCodes = new Set(['agent_credential_unavailable', 'credential_side_mismatch', 'side_or_agent_unavailable']);
    const blocking = (reason, state = null) => reason !== 'not_a_registered_agent'
      && !(abandonUnreachable && (state === 'unreachable' || terminalCleanupCodes.has(reason)));
    if (withdrawals.some((w) => !w.left && blocking(w.reason, w.state))) return res.status(409).json({
      ok: false, code: 'cleanup_incomplete', side: projectSideStore.getSide(existing.id), withdrawals,
      error: 'Room withdrawal failed; credentials are retained. Repair the bridge credential and retry. Permanently unavailable credentials may be abandoned explicitly with force=true&abandon_unreachable=true.',
    });
    const revocations = [];
    if (credential?.kind === 'registrationToken') {
      for (const agent of Object.values(agents).filter((a) => isAgentRecord(a) && a.projectSide === existing.id)) {
        const outcome = await runMatrixWork({ action: 'logout', agent: agent.name, sideId: existing.id, mxid: recordedAgentMxid(agent) });
        revocations.push({ agent: agent.name, revoked: outcome.ok, reason: outcome.code });
        if (!outcome.ok && blocking(outcome.code)) return res.status(409).json({ ok: false, code: 'cleanup_incomplete',
          error: 'Agent token revocation failed; side credentials are retained for retry.' });
      }
      if (credential.representativeToken) {
        const outcome = await runMatrixWork({ action: 'representative-logout', agent: null, sideId: existing.id });
        revocations.push({ agent: null, revoked: outcome.ok, reason: outcome.code });
        if (!outcome.ok && blocking(outcome.code)) return res.status(409).json({ ok: false, code: 'cleanup_incomplete',
          error: 'Representative token revocation failed; side credentials are retained for retry.' });
      }
    }
    const { endedEngagements, deactivatedBindings } =
      endEngagementsAndBindingsForSide(existing.id, `project side ${existing.id} removed`);
    const retiredAgents = retireAgentsForSide(existing.id);
    const side = projectSideStore.removeSide(req.params.id, { force: req.query?.force === 'true',
      cleanup: { withdrawals, revocations, abandonedUnreachable: abandonUnreachable } });
    if (!side) return res.status(404).json({ error: 'project side not found' });
        /*
     * TWO MORE STORES, and ADR-016 row 7 recorded that nothing outside the first three was swept.
     *
     * PENDING INVITATIONS on the removed side can never be accepted — there is no side left to join, and
     * a contributor looking at 「等我决定」 would be looking at a decision that cannot matter. Declined
     * rather than deleted, with the operator recorded as `project-side-removed`, because the compliance
     * rule for this whole cascade is 「不删除，只是停用退役」 and a declined invitation is history where a
     * missing one is amnesia.
     *
     * SIDE-SCOPED ALERTS are resolved by dedupe prefix. `project_side_budget:<side>` and
     * `agent_identity_unminted:<agent>:<side>` both name a side that no longer exists, and an alert
     * pointing at nothing is worse than no alert: an operator chasing it finds a 404 and learns to
     * distrust the next one. `autoResolveByPrefix` is the same call the allocation route uses.
     */
    const removedSideId = String(req.params.id || '').toLowerCase();
    let declinedInvites = [];
    try {
      declinedInvites = pendingInviteStore.list({ state: 'pending' })
        /*
         * `projectServer` rather than deriving the server from the room id again: the store already
         * recorded it at upsert (`serverFromRoomId`), and a second derivation is a second place to get
         * it wrong. The field name is `projectRoomId`, not `roomId` — a first version filtered on the
         * latter, matched nothing, and reported an empty sweep as a successful one.
         */
        .filter((invite) => String(invite.projectServer || '').toLowerCase() === removedSideId)
        .map((invite) => {
          try {
            pendingInviteStore.settle(invite.projectRoomId, invite.agent, 'declined', 'project-side-removed');
            return { roomId: invite.projectRoomId, agent: invite.agent };
          } catch (error) {
            // Reported per invitation rather than aborting, like the engagement loop above it.
            console.warn(`[project-side] could not decline ${invite.agent}@${invite.roomId}: ${error?.message || error}`);
            return null;
          }
        })
        .filter(Boolean);
    } catch (error) {
      console.warn(`[project-side] could not sweep pending invitations for ${removedSideId}: ${error?.message || error}`);
    }
    let resolvedAlerts = [];
    try {
      resolvedAlerts = [
        ...[alertStore.autoResolve(SIDE_BUDGET_ALERT_KEY(removedSideId))].filter(Boolean),
        ...alertStore.dump()
          .filter((entry) => entry.dedupeKey?.startsWith('agent_identity_unminted:')
            && entry.dedupeKey.endsWith(`:${removedSideId}`))
          .map((entry) => alertStore.autoResolve(entry.dedupeKey)).filter(Boolean),
      ].map((entry) => entry?.dedupeKey ?? entry);
    } catch (error) {
      console.warn(`[project-side] could not resolve alerts for ${removedSideId}: ${error?.message || error}`);
    }
return res.json({
      ok: true,
      side,
      retiredAgents,
      endedEngagements,
      deactivatedBindings,
      declinedInvites,
      resolvedAlerts,
      withdrawals,
      revocations,
      /*
       * Reported as a list of what happened rather than a claim of completeness. "Complete" would be a
       * statement about everything in the system that could reference a side, which no single handler
       * can make — and this repository has already produced the failure that phrasing invites:
       * force-deleting an agent left three active bindings and a 250k commitment pointing at it.
       */
      cascade: withdrawals.some((w) => !w.left) || revocations.some((r) => !r.revoked) ? 'partial' : 'performed',
      cascadeNote: 'engagements ended (releasing their commitments), bindings deactivated, agents '
        + 'retired, pending invitations declined, side-scoped alerts resolved — all records kept, since '
        + 'ended and inactive are history while active is a claim',
    });
  } catch (error) {
    return respondProjectSideError(res, error, 'failed to remove project side');
  }
});

/*
 * Verify every active side once, after the server is listening.
 *
 * NON-BLOCKING and after `listening` deliberately: this makes network calls to homeservers Hagency
 * does not control, and a slow or unreachable project side must not delay or fail startup. Each side
 * is independent — `ensureRepresentative` returns a state rather than throwing, so one unreachable
 * server cannot stop the others being checked, which is the property that makes a sweep worth having
 * at all.
 */
async function sweepProjectSideRepresentatives() {
  const sides = projectSideStore.listSides({ activeOnly: true });
  if (!sides.length) return;
  for (const side of sides) {
    try {
      const credential = projectSideStore.credentialFor(side.id);
      const result = await ensureRepresentative({ side, credential });
      if (result.credentialPatch) {
        projectSideStore.setCredential(side.id, { ...credential, ...result.credentialPatch });
      }
      projectSideStore.observeAccess(side.id, { state: result.accessState, detail: result.detail });
      if (result.mxid) {
        try { projectSideStore.setRepresentative(side.id, { mxid: result.mxid }); } catch { /* recorded by the verdict */ }
      }
      if (result.accessState !== 'accepted') {
        console.warn(`[project-side] ${side.id}: ${result.accessState}${result.detail ? ` — ${result.detail}` : ''}`);
      }
    } catch (error) {
      // A per-side failure is logged and the sweep continues. Never the whole sweep's problem.
      console.warn(`[project-side] ${side.id}: sweep failed — ${error?.message || 'unknown'}`);
    }
  }
}

/*
 * ── Pending project invitations (ADR-014 amendment 2026-08-11) ─────────────────────────────
 *
 * A project invites one of the contributor's agents into its room. Until this existed the
 * invitation either caused a silent auto-join with no ownership binding — present and
 * permanently unengageable — or was dropped with a log line the contributor never saw.
 *
 * The split of authority below is the point:
 *
 *   the BRIDGE reports (bridge secret). It is the only thing that sees Matrix state.
 *   the OPERATOR decides (bearer). Accepting spends their tokens, so it is their call.
 *
 * Deciding does not happen here. The backend records the answer and broadcasts it; only the
 * bridge can join a Matrix room, mark it trusted, and write the ownership binding. Recording a
 * decision the bridge then failed to carry out would be the "looks accepted, works for nothing"
 * state this replaces — so the endpoints below record and broadcast, and the bridge's own
 * acceptance is what makes it true.
 */
function respondPendingInviteError(res, error, fallback) {
  if (error instanceof PendingInviteError) {
    const status = { bad_request: 400, not_found: 404, conflict: 409 }[error.code] ?? 500;
    return res.status(status).json({ error: error.message, code: error.code });
  }
  console.error(`[pending-invites] ${fallback}:`, error?.message || error);
  return res.status(500).json({ error: fallback });
}

app.put('/api/matrix/pending-invites', requireBridgeSecret, (req, res) => {
  try {
    return res.json({ ok: true, invite: pendingInviteStore.upsert(req.body || {}) });
  } catch (error) {
    return respondPendingInviteError(res, error, 'failed to record pending invitation');
  }
});

app.get('/api/matrix/pending-invites', requireBearer, (req, res) => {
  try {
    const state = normalizeOptionalText(req.query?.state, 16) || 'pending';
    return res.json({
      ok: true,
      invites: pendingInviteStore.list({ state }),
      pending: pendingInviteStore.pendingCount(),
    });
  } catch (error) {
    return respondPendingInviteError(res, error, 'failed to list pending invitations');
  }
});

app.post('/api/matrix/pending-invites/decide', requireBearer, (req, res) => {
  try {
    const b = req.body || {};
    const roomId = normalizeOptionalText(b.projectRoomId ?? b.project_room_id, 256);
    const agent = normalizeOptionalText(b.agent, 64);
    const accept = b.accept === true;
    if (!roomId || !agent) {
      return res.status(400).json({ error: 'projectRoomId and agent are required' });
    }
    const record = pendingInviteStore.settle(
      roomId, agent, accept ? 'accepted' : 'declined',
      getRequestAgentName(req) || 'operator',
    );
    /*
     * Broadcast, because the join can only happen in the bridge. The response says the decision
     * was recorded and queued — deliberately not that the agent has joined, which the backend
     * cannot know. Overstating that is the same class of defect as the binding outcome that used
     * to be omitted from a verdict.
     */
    broadcastSSE('matrix_invite_decision', { projectRoomId: roomId, agent, accept });
    return res.json({ ok: true, queued: true, invite: record });
  } catch (error) {
    return respondPendingInviteError(res, error, 'failed to record the invitation decision');
  }
});

function consoleExecutionGrant(grant) {
  const { id, agent, scope, project, ownerMxid, taskId, kind, description, createdAt, revokedAt, active } = grant;
  return { id, agent, scope, project, ownerMxid, taskId, kind, description, createdAt, revokedAt, active };
}

app.get('/api/agents/:name/execution-policy', requireBearer, (req, res) => {
  const agent = agents[req.params.name];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  return res.json({ executionPolicy: { yolo: agent.executionPolicy?.yolo === true },
    grants: approvalStore.listGrants(agent.name, executionAgentIdentity(agent)).map(consoleExecutionGrant), appliesTo: 'next_dispatch' });
});

app.put('/api/agents/:name/execution-policy', requireBearer, (req, res) => {
  const agent = agents[req.params.name];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  let policy;
  try { policy = normalizeExecutionPolicy(req.body?.executionPolicy, threadSessionFramework(agent)); }
  catch (error) { return res.status(400).json({ error: error.message }); }
  const previous = agent.executionPolicy;
  agent.executionPolicy = policy;
  if (!saveAgents(true)) {
    agent.executionPolicy = previous;
    return res.status(503).json({ error: 'execution policy persistence failed' });
  }
  auditLog(req, { agent: agent.name, summary: { action: 'execution-policy', yolo: policy.yolo } });
  return res.json({ ok: true, executionPolicy: policy, appliesTo: 'next_dispatch' });
});

app.delete('/api/agents/:name/execution-grants/:id', requireBearer, (req, res) => {
  const agent = agents[req.params.name];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  try {
    const grant = approvalStore.revokeGrant(req.params.id, agent.name, 'operator');
    if (!grant) return res.status(404).json({ error: 'authorization rule not found' });
    return res.json({ ok: true, grant: consoleExecutionGrant(grant), appliesTo: 'subsequent_requests' });
  } catch (error) { return respondApprovalStoreError(res, error, 'failed to revoke authorization'); }
});

app.post('/api/approvals', requireAgentToken(_tokenFromApprovalBody), (req, res) => {
  try {
    const record = approvalStore.createRequest(req.body || {});
    if (record.status === 'pending') {
      // Tool details never enter the shared SSE stream. The bridge fetches them
      // through the secret-authenticated Matrix endpoint below.
      broadcastSSE('approval_requested', { request_id: record.id, agent: record.agent });
    }
    return res.status(record.status === 'pending' ? 201 : 200).json({ ok: true, approval: record });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to create approval request');
  }
});

app.get('/api/approvals/:id', requireAgentToken(_tokenFromApprovalRecord), (req, res) => {
  try {
    const record = approvalStore.getRequest(req.params.id);
    if (!record) return res.status(404).json({ error: 'approval request not found' });
    return res.json({ ok: true, approval: record });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to read approval request');
  }
});

app.get('/api/approvals/:id/matrix', requireApprovalBridgeSecret, (req, res) => {
  try {
    const record = approvalStore.getRequest(req.params.id, { matrix: true });
    if (!record) return res.status(404).json({ error: 'approval request not found' });
    const agent = agents[record.agent];
    const threadRootEventId = routerStore && agent?.agentId && record.router_approval_id
      ? routerStore.approvalThreadOrigin(record.router_approval_id, agent.agentId, record.project_room_id)
      : null;
    return res.json({ ok: true, approval: { ...record, thread_root_event_id: threadRootEventId } });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to read Matrix approval request');
  }
});

app.post('/api/approvals/:id/verdict', requireApprovalBridgeSecret, (req, res) => {
  try {
    const result = approvalStore.submitMatrixVerdict(req.params.id, req.body || {});
    if (!result.record && result.code === 'not_found') {
      return res.status(404).json({ error: 'approval request not found', code: result.code });
    }
    if (!result.ok) {
      const status = result.code === 'expired' ? 410 : (result.code === 'not_pending' ? 409 : 403);
      return res.status(status).json({ error: 'approval verdict rejected', code: result.code, approval: result.record });
    }
    broadcastSSE('approval_verdict', {
      request_id: result.record.id,
      agent: result.record.agent,
      status: result.record.status,
    });
    return res.json({ ok: true, approval: result.record });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to apply approval verdict');
  }
});

app.post('/api/approvals/:id/delivery-failed', requireApprovalBridgeSecret, (req, res) => {
  try {
    const record = approvalStore.denyPending(req.params.id, req.body?.reason || 'matrix_delivery_failed');
    if (!record) return res.status(404).json({ error: 'approval request not found' });
    broadcastSSE('approval_verdict', { request_id: record.id, agent: record.agent, status: record.status });
    return res.json({ ok: true, approval: record });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to deny undeliverable approval');
  }
});

app.post('/api/approvals/:id/consume', requireAgentToken(_tokenFromApprovalRecord), (req, res) => {
  try {
    const result = approvalStore.consumeDecision(
      req.params.id,
      req.body?.agent,
      req.body?.input_digest || null,
    );
    if (!result.record && result.code === 'not_found') {
      return res.status(404).json({ error: 'approval request not found', code: result.code });
    }
    if (!result.ok) {
      const status = result.code === 'pending' ? 202 : (result.code === 'expired' ? 410 : 409);
      return res.status(status).json({ ok: false, code: result.code, approval: result.record });
    }
    return res.json({ ok: true, decision: result.decision, approval: result.record });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to consume approval decision');
  }
});

// ── Agents CRUD ───────────────────────────────────────────────────────
const _tokenFromBody = r => r.body?.from || r.body?.name || '';
const _tokenFromName = r => r.params?.name || '';
const _tokenFromAgent = r => r.body?.agent || r.params?.agent || r.query?.agent || '';
const _tokenFromNodeAssignee = r => { const g = taskGraphStore.getGraph(r.params?.id); return g?.nodes?.[r.params?.nodeId]?.assignee || ''; };
app.post('/api/agents', requireAgentToken(r => r.body?.name || ''), (req, res) => {
  const registeringName = normalizeAgentName(req.body?.name);
  if (stoppingAgents.has(registeringName) || agents[registeringName]?.stopUnconfirmedDispatches?.length) {
    return res.status(409).json({ error: 'agent shutdown is not yet confirmed', code: 'agent_stopping' });
  }
  const {
    name,
    role,
    capability,
    tmux,
    type: agentType,
    identity,
    server,
    agentModelVersion,
    layoutVersion,
    agentId,
    homeDir,
    workdir,
    stateDir,
    presetId,
    managedProjects,
    human,
    task,
    runtimeProfile,
    environment,
  } = req.body;
  if (!name) return res.status(400).json({ error: 'name required' });
  if (task !== undefined && task !== null && !normalizeAgentTask(task, normalizeAgentName(name) || String(name || '').trim())) {
    return res.status(400).json({ error: 'invalid task payload' });
  }
  if (runtimeProfile !== undefined && runtimeProfile !== null && !normalizeRuntimeProfile(runtimeProfile)) {
    return res.status(400).json({ error: 'invalid runtimeProfile payload' });
  }
  refreshServerLiveness();
  const agentName = normalizeAgentName(name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  // Agent tokens were read once at startup, so a token minted afterwards — which
  // is every agent created while the backend is already running — was invisible
  // until a restart, and with HAGENCY_AGENT_TOKEN_MODE=hard every call that
  // agent made was rejected. Registration is exactly when a new token appears.
  // loadAgentTokens keeps already-known entries, so this cannot rotate a live one.
  if (!agentTokens.has(agentName)) loadAgentTokens();
  if (deletedAgentTombstones[agentName]) return res.status(410).json({ error: 'agent permanently deleted', tombstone: deletedAgentTombstones[agentName] });
  const existing = agents[agentName] || {};
  const existingOnline = Boolean(existing.online);
  const normalizedServer = normalizeServer(server);
  const resolvedServer = normalizedServer ?? (isLocalRequest(req) ? 'local' : normalizeServer(existing.server));
  const resolvedTmux = tmux ?? existing.tmux ?? null;
  const resolvedOnline = resolvedTmux ? true : Boolean(existing.online);
  const persistenceSnapshot = snapshotAgentPersistenceState(agentName);
  /*
   * WHO IS ASKING. Four of this body's fields decide cost and capability rather than location, and an
   * agent may not set its own — see `isOperatorRequest` for what each decides and for why the split is
   * per-field instead of per-request. A non-operator registration keeps whatever the operator last set.
   */
  const byOperator = isOperatorRequest(req);
  /*
   * A role outside the vocabulary is a 400 naming it — only for the OPERATOR, whose write would
   * otherwise vanish into `existing.role` with a 200. An agent-token caller's role is dropped by the
   * self-declaration gate whatever its value, so refusing it here would leak which values are valid to
   * a caller that may not set the field at all.
   */
  if (byOperator && role !== undefined && role !== null && !ROLES.includes(role)) {
    return res.status(400).json({
      error: `unknown role: ${String(role).slice(0, 64)} — one of ${ROLES.join(', ')} (free text about `
        + 'an agent belongs in identity)',
    });
  }
  /*
   * Dropped BEFORE resolution, not after. Gating only the stored `presetId` would leave the preset's
   * framework reaching `type` and its runtime profile reaching `runtimeProfile` — the agent would still
   * have chosen its own model, with the record no longer saying which preset it came from.
   */
  const requestedPresetId = byOperator ? presetId : undefined;
  /*
   * A provision plan's preset applies at registration THE SAME WAY an operator's would — resolved
   * through frameworkPresets into the runtime profile below. The value comes from the backend's own
   * plan (`provisionedSides`, written when the plan was handed out), never from the request body, so
   * the self-declaration gate above stays intact: an agent still cannot pick its own preset, and an
   * operator's explicit presetId still outranks the plan's.
   */
  const plannedPresetId = provisionedSides.get(agentName)?.presetId || null;
  // Resolve preset into runtimeProfile if presetId is provided
  let resolvedRuntimeProfile = byOperator ? runtimeProfile : undefined;
  let presetFramework = null;
  const applyPresetId = ((typeof requestedPresetId === 'string' && requestedPresetId) || plannedPresetId) || null;
  if (applyPresetId) {
    const preset = frameworkPresets.find(p => p.id === applyPresetId);
    if (!preset) return res.status(400).json({ error: `unknown preset: ${applyPresetId}` });
    presetFramework = preset.framework || null;
    resolvedRuntimeProfile = runtimeProfileFromPreset(preset);
  }
  agents[agentName] = {
    name: agentName,
    /*
     * Which project side this agent was minted for (ADR-016 decision 4), taken from the provision
     * plan rather than from the request body — see `provisionedSides` for why the agent may not
     * declare it. `null` for an agent created by hand, which decision 5 keeps as an explicit
     * operator-only path.
     */
    projectSide: provisionedSides.get(agentName)?.sideId ?? existing.projectSide ?? null,
    /*
     * What the matrix will select this agent FOR. Operator-set, because an agent that could name its
     * own role could put itself in a cell it cannot staff — `selectAgent` matches on this, and the
     * borrower's request is expressed in the same vocabulary.
     *
     * VALIDATED AGAINST ROLES — a reversal of #60's deliberate omission, because the reason for the
     * omission died. The old portal's New Agent form sent its GUIDANCE textarea in this field, so a
     * strict check would have silently discarded prose an operator typed; that portal is deleted, and
     * every living writer sends a vocabulary key or nothing (verified: the console's onboard sends
     * presetId, hagency-up sends no role, the provision endpoint takes framework/presetId). Free-text
     * about an agent belongs in `identity`, which has always been the prose field. An operator-supplied
     * role that is not in the vocabulary is REFUSED above rather than silently kept, because a typo the
     * operator never learns about is the silent-unstaffable-agent trap with better manners.
     */
    role: (byOperator ? role : undefined) ?? existing.role ?? null,
    // matrix-Agent capability tier; invalid, absent, or not the operator's → keep existing. Reads the
    // tier list from role-capacity.json rather than repeating it, so adding a tier does not need this
    // line found again.
    capability: (byOperator && CAPABILITY_TIERS.includes(capability)
      ? capability
      : (existing.capability ?? null)),
    identity: identity ?? existing.identity ?? null,
    tmux: resolvedTmux,
    type: presetFramework ?? agentType ?? existing.type ?? 'agent',
    // Which named preset produced runtimeProfile. Recorded, not just consumed:
    // the resolved values alone cannot say which reusable configuration an agent
    // is on, so a client could show the model but never the preset behind it,
    // and editing a preset could not identify the agents it affects.
    // THE CEILING: `remainingFor` reads `preset.ceiling.tokens` through this field, so an agent able to
    // set it could raise its own budget by re-registering. `PUT /api/agents/:name/preset` names that
    // hole in its own comment and can only close its own route.
    presetId: normalizeOptionalText(applyPresetId, 128) || existing.presetId || null,
    executionPolicy: existing.executionPolicy || normalizeExecutionPolicy(
      existing.name ? undefined : frameworkPresets.find(p => p.id === applyPresetId)?.executionPolicy,
      presetFramework ?? agentType ?? existing.type,
    ),
    // Recorded so the sweep does not have to re-derive it, and so a record stays
    // correct even if the registry later changes.
    transport: (() => {
      const fromBody = typeof req.body?.transport === 'string' ? req.body.transport.trim().toLowerCase() : '';
      if (fromBody === 'acp' || fromBody === 'tmux') return fromBody;
      if (existing.transport) return existing.transport;
      return getFramework(presetFramework ?? agentType ?? existing.type)?.transport === 'acp' ? 'acp' : 'tmux';
    })(),
    acpPid: Number(req.body?.acpPid) > 1 ? Number(req.body.acpPid) : (existing.acpPid ?? null),
    kind: 'agent',
    server: resolvedServer,
    online: resolvedOnline,
    lastSeen: resolvedOnline ? Date.now() : (existing.lastSeen || Date.now()),
    offlineReason: resolvedOnline ? null : (existing.offlineReason || 'offline'),
    manualDown: resolvedOnline ? false : (existing.manualDown === true),
    registeredAt: existing.registeredAt || Date.now(),
    discoveredAt: existing.discoveredAt || existing.registeredAt || Date.now(),
    agentModelVersion: normalizeAgentModelVersion(agentModelVersion)
      || normalizeAgentModelVersion(existing.agentModelVersion)
      || null,
    layoutVersion: normalizeLayoutVersion(layoutVersion)
      || normalizeLayoutVersion(existing.layoutVersion)
      || null,
    agentId: normalizeAgentId(agentId) || normalizeAgentId(existing.agentId) || null,
    homeDir: normalizeWorkspacePath(homeDir) || normalizeWorkspacePath(existing.homeDir) || null,
    workdir: normalizeWorkspacePath(workdir) || normalizeWorkspacePath(existing.workdir) || null,
    workspaceMode: normalizeWorkspaceMode(existing.workspaceMode),
    worktreesDir: normalizeWorkspacePath(existing.worktreesDir) || null,
    worktreeBootstrap: normalizeWorktreeBootstrap(existing.worktreeBootstrap),
    stateDir: normalizeWorkspacePath(stateDir) || normalizeWorkspacePath(existing.stateDir) || null,
    managedProjects: Array.isArray(managedProjects)
      ? normalizeManagedProjects(managedProjects)
      : normalizeManagedProjects(existing.managedProjects),
    human: mergeHumanMeta(existing.human, human),
    task: task !== undefined
      ? normalizeAgentTask(task, agentName)
      : normalizeAgentTask(existing.task, agentName),
    runtimeProfile: resolvedRuntimeProfile !== undefined
      ? mergeRuntimeProfileApiKeys(normalizeRuntimeProfile(resolvedRuntimeProfile), normalizeRuntimeProfile(existing.runtimeProfile))
      : normalizeRuntimeProfile(existing.runtimeProfile),
    environment: (VALID_ENVIRONMENTS.has(environment) ? environment : null)
      || existing.environment || classifyEnvironment(agentName),
  };
  if (resolvedOnline) {
    const isLocal = isLocalAgentServer(resolvedServer, LOCAL_SERVER_ID);
    if (isLocal && resolvedTmux) {
      // Local registration with tmux → STARTING (grace timer for MCP)
      transitionAgent(agentName, 'api_register_with_tmux');
    } else {
      // Remote or non-tmux → direct ONLINE
      syncAgentMachine(agentName, { heartbeatPresent: true, manualDown: false });
    }
  }
  if (!saveAgentsOrRollback(agentName, persistenceSnapshot)) {
    return res.status(503).json({ error: 'agents persistence failed' });
  }
  /*
   * THE PLAN IS FULFILLED: the agent it promised now exists, so the seat the plan was holding in its
   * (role, tier) cell is released and the name→side memory is consumed. AFTER persistence, not before —
   * a 503'd registration has not fulfilled anything, and the plan must keep holding its seat for the
   * retry. This is one of the reservation's two legitimate ends; the other is the TTL expiry for plans
   * whose launcher died.
   */
  const fulfilledPlan = provisionedSides.get(agentName);
  if (fulfilledPlan) {
    provisionedSides.delete(agentName);
    if (fulfilledPlan.reservationKey) releaseProvisionReservation(fulfilledPlan.reservationKey);
  }
  /*
   * THE IDENTITY IS MINTED HERE — ADR-016 decision 4's last unbuilt clause. `mintAgentIdentity` had no
   * product caller: all of its references were in tests and in the operator endpoint, so the function
   * that exists to give a dispatched agent an account on the customer's homeserver was never reached by
   * dispatching one.
   *
   * WHY THIS MOMENT. It is the first instant both facts are true: the agent record exists (persisted,
   * just above) and it is known which side it serves (from the plan). Earlier there is no record to
   * attach a verdict to; later nothing is looking.
   *
   * FIRE AND FORGET, AND THAT IS ARGUED. Minting talks to somebody else's homeserver, and a registration
   * that 200s only when a foreign server answers would make every launcher's success depend on it.
   * ADR-016 already settles what happens without an identity: the agent can be invited and joined by
   * the representative and can speak through the appservice, so a failed mint costs a display name and
   * an audit trail, not the ability to work. The verdict is logged and — on failure — raised, because
   * "the agent works but has no identity on their server" is exactly the kind of half-state that goes
   * unnoticed until somebody asks who @ac_x is.
   */
  if (fulfilledPlan?.sideId) {
    /*
     * `.catch` on the call itself, not only inside it. The function became async when the federation
     * probe was added, so a throw before its own try/catch — a bad probe mxid, say — would surface as an
     * unhandled rejection and take the process down over a remote homeserver's answer. The registration
     * has already been persisted and answered; nothing here may undo that.
     */
    mintIdentityForProvisionedAgent(agentName, fulfilledPlan.sideId).catch((error) => {
      console.warn(`[mint] identity work for ${agentName} on ${fulfilledPlan.sideId} threw: ${error?.message || error}`);
    });
  }
  writeThruAgentHome(agentName);
  if (!existingOnline && resolvedOnline) {
    notifyAgentCatchup(agentName, 'agent-online-update').catch((e) => {
      console.error(`catchup notify failed for ${agentName}:`, e.message);
    });
  }
  auditLog(req, { agent: agentName, summary: { type: agentType, server: resolvedServer, online: resolvedOnline } });
  res.json({ ok: true, agent: serializeAgent(agents[agentName]) });
});

/**
 * Mint a provisioned agent's identity on the side it was dispatched to, without blocking on it.
 *
 * Separated from the registration route so the route reads as one act. Errors never propagate: the
 * caller has already answered, and an unhandled rejection here would take the process down over a
 * remote homeserver's mood.
 *
 * SILENT WHEN THERE IS NOTHING TO DO. A side with no credential yet, or one whose credential cannot be
 * read, is a configuration state an operator is already looking at on the console — raising a second
 * alarm per registration would bury the one that matters.
 */
/**
 * A local mxid that certainly exists, for the federation probe to ask about.
 *
 * The bot: it is the one account this deployment definitely has, and asking about a user that may not
 * exist turns the probe's clearest answer (`M_NOT_FOUND`) into an ambiguous one. Read from the same env
 * the bridge logs in with; empty when this deployment has no bot, in which case nothing is probed and
 * everything mints.
 */
const MATRIX_BOT_MXID_FOR_PROBE = (() => {
  const localpart = String(process.env.MATRIX_BOT_USERNAME || '').trim().toLowerCase();
  const server = String(process.env.MATRIX_SERVER_NAME || '').trim().toLowerCase();
  if (!localpart || !server) return null;
  return localpart.startsWith('@') ? localpart : `@${localpart}:${server}`;
})();

/**
 * The identity this agent already has on OUR homeserver, or null.
 *
 * Only a stored token proves one exists: the bridge holds agent credentials, and composing
 * `@ac_<name>:<our server>` without one would assert an account nobody registered — the mistake #73
 * fixed for humans. So the absence of a token is read as the absence of an identity, which is the
 * conservative direction here too: no local identity means mint, and minting is never wrong.
 */
function agentMatrixIdentityOnOurServer(agentName) {
  const configured = String(process.env[`MATRIX_AGENT_TOKEN_${String(agentName).toUpperCase()}`] || '').trim();
  if (!configured) return null;
  const server = String(process.env.MATRIX_SERVER_NAME || '').trim().toLowerCase();
  if (!server) return null;
  return `@${MATRIX_AGENT_PREFIX_FOR_REGISTRATION}${agentName}:${server}`.toLowerCase();
}

async function mintIdentityForProvisionedAgent(agentName, sideId) {
  const side = projectSideStore.getSide(sideId);
  if (!side) return;
  let credential;
  try {
    credential = projectSideStore.credentialFor(sideId);
  } catch {
    return;
  }
  if (!credential) return;

  /*
   * FEDERATION IS CHECKED FIRST — ADR-016 decision 2's optimization, which the row recorded as decided
   * and unbuilt: "where the project's server federates with an existing agent's server, that identity MAY
   * be reused instead of minting a new one", recorded as a FLAG rather than a second flow, because a
   * second flow is a second set of failure modes and the cheap one becomes the only tested one.
   *
   * So this is not a branch in the flow: it is one probe before the same call, and its only effect is to
   * skip a mint that would duplicate an identity the project can already see. `isolated` and `unknown`
   * both mint, which is the safe direction — minting an account that was not strictly needed costs a row
   * in their user table, while reusing one they cannot see produces an agent that is addressable only in
   * theory.
   */
  const probeMxid = MATRIX_BOT_MXID_FOR_PROBE;
  const federation = probeMxid
    ? await probeFederationFromSide({ side, credential, probeMxid }).catch(() => ({ federation: 'isolated' }))
    : { federation: 'unknown', reason: 'no local mxid to probe with' };
  const localIdentity = agentMatrixIdentityOnOurServer(agentName);
  if (federation.federation === 'federates' && localIdentity) {
    const record = Object.values(agents).filter(isAgentRecord).find((a) => a.name === agentName);
    if (record) {
      record.reusedIdentity = true;
      record.matrixIdentity = { mxid: localIdentity, sideId, kind: 'federated' };
      saveAgents();
    }
    console.log(`[mint] ${agentName} reuses ${localIdentity} on ${sideId}: their server federates with ours`);
    return;
  }

  mintAgentIdentity({
    side,
    credential,
    localpart: `${agentPrefixOnSide(side, credential)}${agentName}`.toLowerCase(),
  }).then((minted) => {
    if (minted?.minted) {
      console.log(`[mint] ${agentName} has an identity on ${sideId}: ${minted.mxid}`);
      return;
    }
    /*
     * RAISED, not only logged. The agent still works — the representative can invite and join it and the
     * appservice can speak as it — so this is precisely the half-state that survives unnoticed: an agent
     * doing work under an mxid nobody on the customer's side can account for.
     */
    const reason = minted?.reason || 'refused without a reason';
    console.warn(`[mint] ${agentName} has no identity on ${sideId}: ${reason}`);
    try {
      alertStore.ingest({
        alertType: 'agent_identity_unminted',
        dedupeKey: `agent_identity_unminted:${agentName}:${sideId}`,
        severity: 'warning',
        source: 'backend',
        sourceAgent: agentName,
        summary: `${agentName} was dispatched to ${sideId} without an identity minted there`,
        detail: { agent: agentName, sideId, reason },
        owner: 'hagency-operator',
        runbook: `POST /api/agents/${agentName}/matrix-identity to retry, after fixing what the reason `
          + `names — usually the side's credential or its namespace`,
        impact: 'the agent can still be invited, joined and spoken as through the appservice, so work is '
          + 'not blocked; what is missing is an account the customer can attribute that work to',
        recoveryCondition: 'the identity is minted, by retrying that endpoint or by re-provisioning',
        correlation: { agent: agentName, sideId },
        tags: ['project-side', `side:${sideId}`, 'identity'],
      });
    } catch (error) {
      console.warn(`[mint] could not raise the unminted alarm for ${agentName}: ${error?.message || error}`);
    }
  }).catch((error) => {
    console.warn(`[mint] ${agentName} identity attempt on ${sideId} threw: ${error?.message || error}`);
  });
}

/**
 * A preset, expressed as the runtimeProfile an agent runs under.
 *
 * Extracted rather than copied, because the two callers must not drift: `POST /api/agents`
 * resolves a preset at registration, and `PUT /api/agents/:name/preset` resolves the same preset
 * when a contributor attaches one later. The first version of the binding route set `presetId`
 * ALONE, and the result was an agent that looked configured and still filled no role —
 * `agentForRole()` reads `modelTier(agent.runtimeProfile)`, not the preset, so a ceiling without a
 * profile is a tierless agent. Nothing failed loudly; the engagement simply came back with
 * `agent: null` and then `no_ceiling`.
 */
function runtimeProfileFromPreset(preset) {
  if (!preset) return null;
  return {
    primary: {
      framework: preset.framework || null,
      provider: preset.provider || null,
      model: preset.model || null,
      reasoning: preset.reasoning || null,
      ...(preset.extraArgs ? { extraArgs: preset.extraArgs } : {}),
      ...(preset.apiBaseUrl ? { apiBaseUrl: preset.apiBaseUrl } : {}),
      ...(preset.apiKey ? { apiKey: preset.apiKey } : {}),
    },
  };
}

/**
 * Bind an agent to a framework preset — the CONTRIBUTOR's act, not the agent's.
 *
 * WHY THIS ROUTE EXISTS. `presetId` is what attaches a CEILING to an agent:
 * `remainingFor()` reads `agents[name].presetId` -> `preset.ceiling.tokens`, and
 * `engagementStore.decide()` refuses to allocate against an agent whose remaining is null
 * ("cannot allocate against an agent with no declared ceiling"). Until this route, the only
 * writer of `presetId` was `POST /api/agents`, guarded by `requireAgentToken` — and no CLI path
 * passes a preset. So a contributor could create a preset with a ceiling, onboard an agent, and
 * never connect them: every engagement was unapprovable, and the console showed the agent under
 * "bare" with no action to fix it. Found by walking the flow end to end rather than by a failing
 * test.
 *
 * WHY `requireBearer` AND NOT `requireAgentToken`. The ceiling is the contributor's
 * declaration about their own resource — how much of their capacity they are willing to lend
 * (ADR-013's L1/L2). Gating it on the agent's own credential makes the resource the authority
 * on its own budget, which is self-authorization: an agent could raise its ceiling by
 * re-registering with a richer preset.
 *
 * THE SECOND HALF OF THAT SENTENCE USED TO READ "the runtimeProfile stays the agent's to report; the
 * budget does not", and it does not survive its own argument. `agentCapability` prefers
 * `modelTier(runtimeProfile)` over the role default, so the profile decides the TIER — and the tier is
 * priced. Both writers of this field now require the operator's bearer; see the note at the PATCH
 * route for what an honest self-report would need instead.
 *
 * Unbinding is `presetId: null` rather than a DELETE, so "which preset" and "no preset" travel
 * through one code path and cannot disagree about what the absent case means.
 */
/**
 * Bind an agent to a project side — the OPERATOR's act, and it had no route at all.
 *
 * WHY THIS EXISTS, found by running the whole thing on a clean fleet rather than by a failing test.
 * `agent.projectSide` is what lets `POST /api/agents/:name/matrix-identity` decide which side to mint on,
 * and without it that endpoint refuses with `no_project_side` — correctly, since an identity is minted ON
 * a side. But nothing could set it. `POST /api/agents` could, and is guarded by `requireAgentToken`; the
 * only other source was a binding written by the Matrix-side owner approval, which needs the agent to
 * already be reachable. So on a fresh fleet the chain closed on itself: no side, so no identity; no
 * identity, so the agent could not act; and no route in between. Every gate refused informatively and the
 * sequence was still impossible.
 *
 * This is the same shape, and the same fix, as `PUT .../preset` directly below — that route exists because
 * `presetId` had only an agent-authenticated writer too.
 *
 * `requireBearer`, NOT `requireAgentToken`, and that is the whole reason it is a separate route rather
 * than a field on PATCH. Which customer an agent serves is a decision about someone else's homeserver and
 * someone else's budget; an agent that could claim a side would be choosing its own employer. The comment
 * on the identity endpoint says `projectSide` is set "never from the agent's own request body", and that
 * stays true.
 */
app.put('/api/agents/:name/project-side', requireBearer, (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });

  const raw = req.body?.projectSide ?? req.body?.project_side;
  // Absent and explicit-null both mean unbind; anything else must name a side that exists.
  const requested = raw === null || raw === undefined || raw === '' ? null : normalizeOptionalText(raw, 255);
  if (requested !== null && !requested) return res.status(400).json({ error: 'invalid projectSide' });

  /*
   * THE SIDE MUST EXIST, checked here rather than left to the identity endpoint. A binding to a side
   * nobody configured would move the same confusion one step later, where it reads as a minting failure
   * instead of a typo — and this is the one place that knows the operator just typed it.
   *
   * A side with no CREDENTIAL is still allowed. Configuring the side and issuing its credential are two
   * acts that legitimately happen in either order, and refusing here would force one of them.
   */
  if (requested && !projectSideStore.getSide(requested)) {
    const known = projectSideStore.listSides({ activeOnly: false }).map((side) => side.id);
    return res.status(400).json({
      error: `unknown project side: ${requested}`,
      code: 'unknown_project_side',
      known,
    });
  }

  const persistenceSnapshot = snapshotAgentPersistenceState(agentName);
  const previous = agent.projectSide ?? null;
  agent.projectSide = requested;
  if (!saveAgentsOrRollback(agentName, persistenceSnapshot)) {
    return res.status(503).json({ error: 'agents persistence failed' });
  }
  auditLog(req, { agent: agentName, summary: { projectSide: requested, previousProjectSide: previous } });
  return res.json({
    ok: true,
    agent: serializeAgent(agents[agentName]),
    /*
     * Said in the response because it is the next step and the reason this route was needed. An operator
     * who binds a side and stops here has an agent that still cannot act, which is the state this whole
     * route exists to get out of.
     */
    nextStep: requested
      ? `POST /api/agents/${agentName}/matrix-identity to mint an identity on ${requested}`
      : 'unbound; this agent can no longer be minted on any side',
  });
});

app.put('/api/agents/:name/preset', requireBearer, (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });

  const raw = req.body?.presetId;
  // Absent and explicit-null both mean unbind; anything else must name a real preset.
  const requested = raw === null || raw === undefined || raw === '' ? null : normalizeOptionalText(raw, 128);
  if (requested !== null && !requested) return res.status(400).json({ error: 'invalid presetId' });
  const preset = requested ? frameworkPresets.find((p) => p.id === requested) : null;
  if (requested && !preset) return res.status(400).json({ error: `unknown preset: ${requested}` });

  const persistenceSnapshot = snapshotAgentPersistenceState(agentName);
  const previous = agent.presetId ?? null;
  agent.presetId = requested;
  /*
   * The PROFILE moves with the preset, not just the id. `agentForRole()` decides whether an agent
   * can fill a role from `modelTier(agent.runtimeProfile)`, so setting `presetId` alone produces an
   * agent with a ceiling, no tier, and no role — which surfaces later as an engagement with
   * `agent: null` and a `no_ceiling` refusal, pointing at the budget rather than at the profile.
   *
   * Unbinding clears it. Leaving the last preset's profile behind would let a contributor detach a
   * resource and have it keep serving on the terms they just withdrew.
   */
  const resolvedProfile = runtimeProfileFromPreset(preset);
  agent.runtimeProfile = resolvedProfile ? normalizeRuntimeProfile(resolvedProfile) : null;
  if (!saveAgentsOrRollback(agentName, persistenceSnapshot)) {
    return res.status(503).json({ error: 'agents persistence failed' });
  }
  auditLog(req, { agent: agentName, summary: { presetId: requested, previousPresetId: previous } });
  /*
   * `remaining` is returned because it is the question the caller actually has — binding a
   * preset with no ceiling leaves the agent just as unapprovable as before, and answering with
   * only `ok: true` would hide that.
   */
  return res.json({
    ok: true,
    agent: serializeAgent(agents[agentName]),
    ceilingTokens: preset?.ceiling?.tokens ?? null,
    remaining: remainingFor(agentName),
    // The tier is what makes the agent fillable at all, so it is reported beside the budget.
    tier: modelTier(agents[agentName].runtimeProfile) ?? null,
  });
});

app.patch('/api/agents/:name', requireAgentToken(_tokenFromName), (req, res) => {
  const updatingName = normalizeAgentName(req.params.name);
  if (stoppingAgents.has(updatingName) || agents[updatingName]?.stopUnconfirmedDispatches?.length) {
    return res.status(409).json({ error: 'agent shutdown is not yet confirmed', code: 'agent_stopping' });
  }
  refreshServerLiveness();
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  /*
   * REFUSED RATHER THAN IGNORED, because being ignored is worse.
   *
   * This route is agent-authenticated, so `projectSide` must not be settable here — an agent choosing
   * which customer it serves is an agent choosing its own employer. That part was already right. What was
   * wrong is that the field was simply absent from the destructuring below, so a caller sending it got
   * `ok: true` and no binding, discoverable only by reading the record back. A walkthrough on a clean
   * fleet lost real time to exactly that, and it was one of three writes found that day which accepted a
   * field and reported success without applying it.
   *
   * Naming the operator route in the refusal is the point: the answer to "how do I set this" should come
   * from the failure, not from reading the source.
   */
  if (req.body && ('projectSide' in req.body || 'project_side' in req.body)) {
    return res.status(400).json({
      error: 'projectSide cannot be set here: this route is agent-authenticated, and which customer an '
        + 'agent serves is not the agent\'s decision',
      code: 'project_side_not_settable_here',
      use: `PUT /api/agents/${agentName}/project-side with the operator token`,
    });
  }
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  const wasOnline = Boolean(agent.online);
  const {
    role,
    identity,
    tmux,
    online,
    offlineReason,
    manualDown,
    agentModelVersion,
    layoutVersion,
    agentId,
    homeDir,
    workdir,
    stateDir,
    managedProjects,
    human,
    task,
    runtimeProfile,
    environment,
  } = req.body;
  if (task !== undefined && task !== null && !normalizeAgentTask(task, agentName)) {
    return res.status(400).json({ error: 'invalid task payload' });
  }
  if (runtimeProfile !== undefined && runtimeProfile !== null && !normalizeRuntimeProfile(runtimeProfile)) {
    return res.status(400).json({ error: 'invalid runtimeProfile payload' });
  }
  const persistenceSnapshot = snapshotAgentPersistenceState(agentName);
  /*
   * THE SAME DOOR, ROUND THE BACK. Gating the four cost-and-capability fields on `POST /api/agents`
   * and not here would close nothing: this route is guarded by the agent's own token, so an agent that
   * could not declare a role at registration could simply patch one in a second later. `role` and
   * `runtimeProfile` are the two of the four this route writes.
   */
  const byOperator = isOperatorRequest(req);
  /*
   * Same vocabulary check as registration, same asymmetry: the operator's invalid role is a 400, the
   * agent's is dropped by the gate regardless. `null` stays legal — clearing a role is how an operator
   * takes an agent out of the matrix without deleting it.
   */
  if (byOperator && role !== undefined && role !== null && !ROLES.includes(role)) {
    return res.status(400).json({
      error: `unknown role: ${String(role).slice(0, 64)} — one of ${ROLES.join(', ')} (free text about `
        + 'an agent belongs in identity)',
    });
  }
  if (role !== undefined && byOperator) agent.role = role;
  if (identity !== undefined) agent.identity = identity;
  if (tmux !== undefined) {
    agent.tmux = tmux;
    if (tmux) {
      // online driven by machine
      syncAgentMachine(agentName, { tmuxPresent: true });
      agent.offlineReason = null;
      agent.lastSeen = Date.now();
    } else if (online === undefined) {
      syncAgentMachine(agentName, { tmuxMissing: true });
      agent.offlineReason = agent.offlineReason || 'tmux-cleared';
    }
  }
  if (online !== undefined) {
    // online:true only clears manualDown — real liveness comes from sweep/heartbeat
    if (Boolean(online)) {
      syncAgentMachine(agentName, { manualDown: false });
      agent.lastSeen = Date.now();
    } else {
      syncAgentMachine(agentName, { tmuxMissing: true });
    }
  }
  if (offlineReason !== undefined) {
    agent.offlineReason = (typeof offlineReason === 'string' && offlineReason.trim()) ? offlineReason.trim() : null;
  }
  if (manualDown !== undefined) {
    syncAgentMachine(agentName, { manualDown: manualDown === true });
  }
  if (agentModelVersion !== undefined) {
    agent.agentModelVersion = normalizeAgentModelVersion(agentModelVersion) || null;
  }
  if (layoutVersion !== undefined) {
    agent.layoutVersion = normalizeLayoutVersion(layoutVersion) || null;
  }
  if (agentId !== undefined) {
    agent.agentId = normalizeAgentId(agentId) || null;
  }
  if (homeDir !== undefined) {
    agent.homeDir = normalizeWorkspacePath(homeDir) || null;
  }
  if (workdir !== undefined) {
    agent.workdir = normalizeWorkspacePath(workdir) || null;
  }
  if (stateDir !== undefined) {
    agent.stateDir = normalizeWorkspacePath(stateDir) || null;
  }
  if (managedProjects !== undefined) {
    agent.managedProjects = normalizeManagedProjects(managedProjects);
  }
  if (human !== undefined) {
    agent.human = mergeHumanMeta(agent.human, human);
  }
  if (task !== undefined) {
    agent.task = normalizeAgentTask(task, agentName);
  }
  /*
   * WITHDRAWN FROM THE AGENT, and this contradicts a sentence in `PUT /api/agents/:name/preset` that
   * said the profile stays the agent's to report. That sentence was right about the intent and wrong
   * about the consequence: `agentCapability` reads `modelTier(runtimeProfile)` and prefers it over the
   * role default, so this field IS the tier, and the tier is what the borrower is charged for. Worse,
   * it is the same field the preset route writes the contributor's TERMS into — so an agent's report
   * did not sit beside those terms, it replaced them, and the preset the operator bound became
   * unobservable.
   *
   * An honest self-report is still worth having; it needs a field of its own, next to the terms rather
   * than on top of them. That is a change to the record's shape and is not made here.
   */
  if (runtimeProfile !== undefined && byOperator) {
    agent.runtimeProfile = mergeRuntimeProfileApiKeys(normalizeRuntimeProfile(runtimeProfile), normalizeRuntimeProfile(agent.runtimeProfile));
  }
  if (environment !== undefined && VALID_ENVIRONMENTS.has(environment)) {
    agent.environment = environment;
  }
  if (agent.online === true && agent.manualDown !== false) {
    agent.manualDown = false;
  }
  if (!saveAgentsOrRollback(agentName, persistenceSnapshot)) {
    return res.status(503).json({ error: 'agents persistence failed' });
  }
  writeThruAgentHome(agentName);
  if (!wasOnline && agent.online === true) {
    notifyAgentCatchup(agentName, 'agent-online-patch').catch((e) => {
      console.error(`catchup notify failed for ${agentName}:`, e.message);
    });
  }
  auditLog(req, { agent: agentName, summary: { fields: Object.keys(req.body) } });
  res.json({ ok: true, agent: serializeAgent(agent) });
});

app.get('/api/agents', (req, res) => {
  refreshServerLiveness();
  const records = Object.values(agents).filter(isAgentRecord);
  if ((String(req.query.view || '').trim().toLowerCase()) === 'names') {
    const names = records
      .filter(agent => !agent.retiredAt)
      .map(agent => (typeof agent?.name === 'string' ? agent.name.trim() : ''))
      .filter(Boolean)
      .sort((a, b) => a.localeCompare(b));
    return res.json(names);
  }
  res.json(serializeAgents(records));
});

// matrix-Agent pool view (Phase 2): the role×capability grid, for capability-aware dispatch.
// Read-only. Filters: ?role= ?capability= ?state=idle|busy|any (default any).
// matrix-Agent capability scheduler (Phase 3): a reservation/queue over the pool. hagency
// decides *which* agent staffs a (role, capability); the caller (e.g. the OpenFab Bridge)
// delivers the task to it and calls /release on completion. Busy/queue are in-memory.
//
// Task 7: the reservation is an owner-bound, renewable lease (DispatchLeaseStore), not a bare
// busy flag — a crashed/stuck caller can no longer pin an agent forever. A lease is claimed at
// dispatch time (leaseId + expiresAt in the response), extended via POST /api/dispatch/renew,
// and released via POST /api/dispatch/release; renew/release both require the exact
// (leaseId, agent, owner) tuple and reject otherwise (owner mismatch, unknown/stale leaseId, or
// an already-expired lease — distinct 4xx reasons). POST /api/dispatch/release rejects the
// pre-Task-7 call shape ({agent} only, no leaseId/owner) by default — letting the ownership
// check be skipped just by omitting fields would defeat it — unless HAGENCY_ALLOW_LEGACY_RELEASE=1
// is set, an explicit opt-in compatibility shim for a caller that predates ownership (see the
// Task 7 report for the caller inventory; nothing in this stack needs the shim as of this
// writing). An unrenewed lease is reaped once its TTL lapses (checked lazily on GET /api/pool and
// POST /api/dispatch, mirroring refreshServerLiveness()'s placement): the lease is invalidated
// first, then the agent is freed, so nothing can observe "agent free, lease still valid" mid-
// reap. A reaped lease is requeued at most once, and only if it carries a durable ticket (the
// queue-drain ticket, or one the caller supplied at dispatch time); otherwise the task is
// presumed failed and an alert is raised — never silently retried/duplicated. The dispatch
// queue itself remains process-local (known limitation, unchanged by Task 7): a restart drops
// in-flight leases and queued tickets alike; this is not restart-safe queueing.
const dispatchLeaseStore = new DispatchLeaseStore({ ttlMs: DISPATCH_LEASE_TTL_MS });
const dispatchQueues = new Map();    // `${role}:${tier}` → [{ ticket, role, tier, task, room }]
/*
 * `${role}:${tier}` → count of outstanding provision plans.
 *
 * A reservation exists to stop ten concurrent requests all provisioning into the same cell between
 * "plan handed out" and "agent registered". It therefore has TWO legitimate ends — the agent registers,
 * or the plan is abandoned — and until 2026-08-15 it had NEITHER: the count only ever went up, so every
 * plan ever issued held its cell's capacity for the life of the process, and a cell whose plans kept
 * failing to launch would refuse provisioning forever while zero agents ran.
 */
const provisionReservations = new Map();
const PROVISION_RESERVATION_TTL_MS = 15 * 60 * 1000;
/*
 * `${role}:${tier}` → [issuedAtMs, ...] — one timestamp per outstanding plan, oldest first. Kept beside
 * the count rather than replacing it so the cap check stays one map read; the timestamps exist so an
 * abandoned plan EXPIRES: a launcher that died between plan and registration must not hold a seat.
 */
const provisionReservationAges = new Map();

function releaseProvisionReservation(key) {
  const count = provisionReservations.get(key) || 0;
  if (count <= 1) provisionReservations.delete(key);
  else provisionReservations.set(key, count - 1);
  const ages = provisionReservationAges.get(key);
  if (ages?.length) {
    ages.shift();
    if (!ages.length) provisionReservationAges.delete(key);
  }
}

/** Drop reservations whose plan is old enough to be presumed dead. Called lazily at the cap check. */
function expireProvisionReservations(key, now = Date.now()) {
  const ages = provisionReservationAges.get(key);
  if (!ages?.length) return;
  while (ages.length && now - ages[0] > PROVISION_RESERVATION_TTL_MS) {
    ages.shift();
    const count = provisionReservations.get(key) || 0;
    if (count <= 1) provisionReservations.delete(key);
    else provisionReservations.set(key, count - 1);
  }
  if (!ages.length) provisionReservationAges.delete(key);
}
/*
 * agentName → sideId, remembered when a provision plan is HANDED OUT and applied when that name
 * registers (ADR-016 decision 4).
 *
 * WHY NOT LET THE AGENT DECLARE ITS SIDE. `POST /api/agents` is the agent's own registration, so a
 * `projectSide` in that body would let an agent claim a side it was never provisioned for — and a side
 * is what decision 7's cascade uses to decide what to retire, and what decision 6's budget charges.
 * The assignment is the backend's decision, so the backend remembers it.
 *
 * Process-local, like `provisionReservations` beside it: a restart between the plan and the
 * registration loses the intent, and the agent registers with no side rather than a wrong one. That is
 * the safe direction — an unattributed agent is visible and fixable, a misattributed one is neither.
 */
const provisionedSides = new Map(); // agentName → { sideId, reservationKey, presetId }

/*
 * ── Role → resource matching (ADR-016 decision 4's recorded gap) ────────
 *
 * Until this existed, a provision plan's resource declaration was "an operator-chosen preset
 * rather than a role-matched selection" — the ADR's own words: the plan's runtime came from
 * TIER_RUNTIME, a static tier→model table that knows nothing about what this deployment has
 * actually configured. These helpers answer the question the plan was skipping: WHICH configured
 * preset can staff this (role, tier) ask.
 */

/**
 * The capability tier a framework preset's model qualifies for, or null.
 *
 * Derived through the SAME authority the rest of the system uses — `modelTier` reading
 * lib/role-capacity.json — over the profile `runtimeProfileFromPreset` shapes, which is the exact
 * pair the registration path resolves a preset with. Deliberately NOT a second model→tier mapping:
 * two tables answering "what does this model qualify for" would drift, and the drift would surface
 * as an agent minted for a cell it cannot staff.
 */
function presetTier(preset) {
  return modelTier(runtimeProfileFromPreset(preset));
}

/**
 * Every preset that can staff (role, tier), in selection order. Two requirements, both hard:
 *
 *  - A QUALIFYING TIER: at least `tier`, under the same TIER_RANK subsumption the engagement path
 *    uses (strong ⊇ medium ⊇ lightweight). Over-qualified can serve; under-qualified never can.
 *  - A CEILING: a preset with no ceiling produces an agent no engagement can ever be approved
 *    against — `remainingFor` returns null for it, and the engagement store refuses to allocate
 *    against an undeclared budget. Minting such an agent looks like capacity and serves nothing.
 *
 * Order: LOWEST qualifying tier first — do not burn a strong seat on a lightweight ask, the same
 * cheapest-sufficient rule `selectAgent` applies to live agents. Within a tier, by id: picking the
 * largest remaining ceiling headroom would need per-preset accounting that does not exist yet
 * (`remainingFor` is per-agent, and one preset may back several agents), so a stable id order buys
 * determinism without inventing a figure.
 *
 *  - NOT ON THE ROLE'S EXCLUSION LIST. role-capacity.json's `excluded` names models a given role
 *    may not use (`architect` and `review` may not run on haiku-4-5 or fable-5, "below the strong
 *    floor"). An earlier draft of this function argued the tier check already covered that, since
 *    both roles default to `strong` — but `resolveTier` lets an EXPLICIT request win over the
 *    role's default, so `architect` at `lightweight` is reachable and would have matched exactly
 *    the presets the file forbids. The list is consulted directly instead of inferred from the
 *    tier. (Before this, nothing in the codebase read `excluded` at all — it stated a rule no
 *    code enforced.)
 */
function modelsExcludedForRole(role) {
  return new Set((roleCapacity.excluded ?? [])
    .filter((entry) => entry?.role === role)
    .flatMap((entry) => entry?.models ?? []));
}

function resourcesForRole(role, tier, presets) {
  const rank = Object.fromEntries([...CAPABILITY_TIERS].reverse().map((t, i) => [t, i]));
  const forbidden = modelsExcludedForRole(role);
  return (presets ?? [])
    .map((preset) => ({ preset, tier: presetTier(preset) }))
    .filter(({ preset, tier: got }) => got
      && rank[got] >= rank[tier]
      && !forbidden.has(runtimeProfileFromPreset(preset)?.primary?.model)
      && Number.isFinite(preset?.ceiling?.tokens))
    .sort((a, b) => (rank[a.tier] - rank[b.tier])
      || (a.preset.id < b.preset.id ? -1 : a.preset.id > b.preset.id ? 1 : 0));
}

/** The preset a provision plan for (role, tier) mints from, or null when none can staff it. */
function resourceForRole(role, tier, presets) {
  return resourcesForRole(role, tier, presets)[0]?.preset ?? null;
}

let dispatchTicketSeq = 0;
const cellKey = (role, tier) => `${role}:${tier}`;
const annotateBusy = (records) => records.map((a) => ({ ...a, busy: dispatchLeaseStore.isBusy(a.name) }));
const poolRecords = () =>
  annotateBusy(serializeAgents(Object.values(agents).filter((a) => agentEligibleForRoom(a))));

const DISPATCH_LEASE_ERROR_STATUS = {
  missing_fields: 400,
  owner_mismatch: 403,
  lease_not_found: 404,
  lease_expired: 410,
  agent_busy: 409,
};

function respondDispatchLeaseError(res, error) {
  const reason = error?.reason || 'internal_error';
  const status = DISPATCH_LEASE_ERROR_STATUS[reason] || 500;
  return res.status(status).json({ error: error?.message || 'dispatch lease error', reason });
}

// Reap leases whose TTL lapsed without a renew. For each: invalidate the lease, free the agent,
// then either requeue its ticket (durable tickets only, at most once) or mark it failed and
// raise an alert (no ticket ⇒ never silently duplicate the task). See the Task 7 note above.
function reapDispatchLeases() {
  const reaped = dispatchLeaseStore.reapExpired(Date.now());
  for (const { lease, context, outcome } of reaped) {
    if (outcome === 'requeued') {
      const key = cellKey(context.role, context.tier);
      const q = dispatchQueues.get(key) || [];
      q.push({ ticket: lease.ticket, role: context.role, tier: context.tier, task: context.task, room: context.room });
      dispatchQueues.set(key, q);
      console.warn(`[dispatch-lease] ${lease.agent} lease ${lease.leaseId} expired unrenewed; requeued ticket ${lease.ticket}`);
      continue;
    }
    console.warn(`[dispatch-lease] ${lease.agent} lease ${lease.leaseId} expired unrenewed with no durable ticket; marking failed`);
    try {
      alertStore.ingest({
        alertType: 'dispatch_lease_expired',
        dedupeKey: `dispatch_lease_expired:${lease.agent}:${lease.leaseId}`,
        severity: 'warning',
        source: 'backend',
        sourceAgent: lease.agent,
        summary: `Dispatch lease for '${lease.agent}' expired without renewal and has no durable ticket to requeue`,
        detail: {
          leaseId: lease.leaseId, agent: lease.agent, owner: lease.owner, taskId: lease.taskId,
          role: context.role, tier: context.tier, expiresAt: lease.expiresAt,
        },
        owner: 'matrix-agent-dispatch',
        runbook: 'inspect the dispatch lease + task state for this agent; redispatch manually if the work is still needed',
        impact: 'the in-flight task on this agent is presumed failed; no automatic retry because no durable ticket exists to requeue from',
        recoveryCondition: 'operator investigates and manually redispatches, or explicitly resolves this alert',
        correlation: { dedupeKey: `dispatch_lease_expired:${lease.agent}:${lease.leaseId}`, leaseId: lease.leaseId, agent: lease.agent },
        tags: ['dispatch-lease', `agent:${lease.agent}`],
      });
    } catch (error) {
      console.warn(`[dispatch-lease] failed to ingest expiry alert for ${lease.leaseId}: ${error?.message || error}`);
    }
  }
  return reaped;
}

app.get('/api/pool', (req, res) => {
  refreshServerLiveness();
  reapDispatchLeases();
  let records = poolRecords();
  const state = String(req.query.state || 'any').toLowerCase();
  if (state === 'idle') records = records.filter(a => a.online !== false && a.busy !== true);
  else if (state === 'busy') records = records.filter(a => a.busy === true);
  if (req.query.role) records = records.filter(a => agentRole(a) === String(req.query.role));
  if (req.query.capability) records = records.filter(a => agentCapability(a) === String(req.query.capability));
  const grid = indexPool(records);
  const counts = {};
  for (const [r, byCap] of Object.entries(grid)) {
    counts[r] = Object.fromEntries(Object.entries(byCap).map(([c, list]) => [c, list.length]));
  }
  res.json({
    grid,
    counts,
    total: records.length,
    agents: records.map(a => ({ name: a.name, role: agentRole(a), capability: agentCapability(a), online: a.online !== false, busy: a.busy === true })),
  });
});

// Reserve an agent for (role, capability), or queue when the pool can't staff it. The caller
// delivers the task to the returned agent and calls /api/dispatch/release when it completes.
// Optional body fields `owner`/`ticket`/`taskId` feed the Task 7 lease (see notes above); a
// missing `owner` gets DISPATCH_LEASE_DEFAULT_OWNER so pre-Task-7 callers keep working.
app.post('/api/dispatch', (req, res) => {
  refreshServerLiveness();
  reapDispatchLeases();
  const role = req.body?.role ? String(req.body.role) : null;
  if (!role) return res.status(400).json({ error: 'role required' });
  const tier = resolveTier(role, req.body?.capability ? String(req.body.capability) : undefined);
  const agent = selectAgent(poolRecords(), role, tier); // poolRecords() already carries busy
  if (agent) {
    const owner = req.body?.owner ? String(req.body.owner) : DISPATCH_LEASE_DEFAULT_OWNER;
    const ticket = req.body?.ticket ? String(req.body.ticket) : null;
    const taskId = req.body?.taskId ? String(req.body.taskId) : null;
    const lease = dispatchLeaseStore.create({
      agent: agent.name, owner, taskId, ticket,
      role, tier, task: req.body?.task ?? null, room: req.body?.room ?? null,
    });
    return res.json({ status: 'routed', agent: agent.name, role, tier, leaseId: lease.leaseId, expiresAt: lease.expiresAt });
  }
  // Phase 4: auto-provision. When MATRIX_AGENT_MAX_PER_CELL > 0 and the cell is below the cap,
  // return a `provision` plan (the launcher runs `up-v1` with the tier's runtime) instead of
  // queuing. Default 0 = off (pure queue) — safe. The decision is pure; spawning is the edge.
  // No lease is created here: the named agent doesn't exist yet, so there's nothing to reserve.
  const key = cellKey(role, tier);
  const cap = Number(process.env.MATRIX_AGENT_MAX_PER_CELL || 0);
  if (cap > 0) {
    const inCell = poolRecords().filter((a) => agentRole(a) === role && agentCapability(a) === tier).length;
    expireProvisionReservations(key);
    const reserved = provisionReservations.get(key) || 0;
    if (inCell + reserved < cap) {
      /*
       * BUDGET ADMISSION HERE IS DEFENSIVE, NOT LOAD-BEARING — and that is a correction of an earlier
       * claim, not a hedge. ADR-016's decision 6 rows cited this gate as the build; ADR-013 decision 8
       * withdraws `/api/dispatch` "and any successor router-facing assignment path", and this route has
       * no product caller, so what those rows pointed at was a road nobody drives. The gate that
       * matters is on the engagement path — both callers of `refuseOverSideAllocation` are there now.
       *
       * KEPT RATHER THAN DELETED, for one reason worth stating: this route carries NO auth guard, so
       * anyone who can reach the backend can ask it for a provision plan. Removing the check would
       * leave the unguarded route as the one place a plan is produced with no budget consulted.
       * Withdrawn-but-present is exactly when a cheap check earns its keep.
       *
       * SCOPED BY WHETHER THE REQUEST NAMES A ROOM. A room id carries its origin server, which is the
       * project side whose budget is at stake. A request with NO room is not project-side work — it is
       * the contributor dispatching against their own agents, already governed by each agent's own
       * ceiling — so requiring a side there would break internal dispatch to enforce a budget nobody
       * is spending. A first version of this did exactly that and broke two existing cases; the
       * failure was the scope being wrong, not the gate.
       *
       * `requireSide: true` because this path MINTS: under decision 1 the identity comes from the
       * side's credential, so a named room with no configured side cannot produce an agent at all.
       */
      const room = typeof req.body?.room === 'string' ? req.body.room : '';
      const sideId = sideIdForRoom(room);

      if (sideId && refuseOverSideAllocation(res, {
        projectRoomId: room,
        tokens: req.body?.requestedTokens,
        act: 'creating this agent',
        requireSide: true,
        extra: { role, tier },
      })) return undefined;

      /*
       * ROLE→RESOURCE MATCHING (ADR-016 decision 4). The plan is minted FROM a configured preset
       * that can staff the ask, or refused when none can — never from the static tier table while
       * presets exist to consult.
       *
       * REFUSED, NEVER QUEUED — the same argument as the budget gate directly above: a queue entry
       * answers "waiting for capacity" when the truth is "no configured resource can ever staff
       * this", and an agent that can never be minted cannot be waited for. Refused BEFORE the
       * reservation is taken, so an impossible ask holds no seat in its cell.
       *
       * SCOPED TO DEPLOYMENTS THAT DECLARE PRESETS, the way the budget gate above is scoped to
       * requests that name a room. A deployment with zero presets has made no resource declarations
       * to match against — it predates the preset model and still runs on the static TIER_RUNTIME
       * row, and refusing every mint there would turn an upgrade into an outage. Once ANY preset
       * exists the deployment is describing its resources, and an ask none of them can staff is
       * refused with the remedy named.
       */
      const preset = resourceForRole(role, tier, frameworkPresets);
      if (frameworkPresets.length && !preset) {
        return res.status(409).json({
          status: 'refused',
          reason: 'no_resource_for_role',
          error: `no configured resource can staff a '${role}' ask at tier '${tier}': `
            + `${frameworkPresets.length} preset(s) considered and none qualifies at '${tier}' or `
            + 'above while carrying a ceiling — add such a preset (POST /api/framework-presets) '
            + 'or lower the requested capability',
          role,
          tier,
          presetsConsidered: frameworkPresets.length,
        });
      }

      provisionReservations.set(key, reserved + 1);
      const ages = provisionReservationAges.get(key) || [];
      ages.push(Date.now());
      provisionReservationAges.set(key, ages);
      /*
       * The name carries the SIDE when there is one. `mx_${role}_${tier}_${seq}` is unattributable,
       * and ADR-016 decision 7's cascade needs exactly that attribution to know what to take with a
       * side when it is removed. Without a side the existing format is kept, because that request is
       * not a side's to account for.
       */
      const name = sideId
        ? `mx_${sideId.replace(/[^a-z0-9]+/g, '_')}_${role}_${tier}_${dispatchTicketSeq++}`
        : `mx_${role}_${tier}_${dispatchTicketSeq++}`;
      // The reservation key rides along so REGISTRATION can release the seat it was holding, and
      // the preset id so registration can bind the agent to the resource the plan matched.
      provisionedSides.set(name, { sideId: sideId ?? null, reservationKey: key, presetId: preset?.id ?? null });
      return res.json({
        status: 'provision', role, tier, name,
        /*
         * `runtime` predates the preset match and launchers read it (`up-v1 <name> <runtime>`), so
         * it stays — derived from the matched preset when there is one, the static tier row only
         * when the deployment declares no presets at all. `reasoning` rides along when the preset
         * states one: the same model at a different reasoning level is a different tier
         * (lib/role-capacity.json lists gpt-5.6-sol at all three), so dropping it would launch a
         * different capability than the one that was matched.
         */
        runtime: preset
          ? { runtime: preset.framework, model: preset.model, ...(preset.reasoning ? { reasoning: preset.reasoning } : {}) }
          : TIER_RUNTIME[tier],
        ...(preset ? { presetId: preset.id } : {}),
        ...(sideId ? { sideId, budget: sideBudgetFor(sideId) } : {}),
      });
    }
  }
  const ticket = `disp-${Date.now()}-${dispatchTicketSeq++}`;
  const q = dispatchQueues.get(key) || [];
  q.push({ ticket, role, tier, task: req.body?.task ?? null, room: req.body?.room ?? null });
  dispatchQueues.set(key, q);
  res.json({ status: 'queued', ticket, role, tier, queueDepth: q.length });
});

// Extend a lease's expiresAt from now (Task 7). Requires the exact (leaseId, agent, owner)
// tuple that POST /api/dispatch handed out; this is how a long task survives past one TTL
// window — the lease is never killed on createdAt age alone as long as it keeps renewing.
app.post('/api/dispatch/renew', (req, res) => {
  const leaseId = req.body?.leaseId ? String(req.body.leaseId) : null;
  const agent = req.body?.agent ? String(req.body.agent) : null;
  const owner = req.body?.owner ? String(req.body.owner) : null;
  if (!leaseId || !agent || !owner) {
    return res.status(400).json({ error: 'leaseId, agent, and owner are required', reason: 'missing_fields' });
  }
  try {
    const lease = dispatchLeaseStore.renew({ leaseId, agent, owner });
    return res.json({ status: 'renewed', agent, leaseId: lease.leaseId, expiresAt: lease.expiresAt });
  } catch (error) {
    return respondDispatchLeaseError(res, error);
  }
});

// Free a reserved agent; if a ticket is waiting for its cell, reserve it again and hand the
// caller the drained ticket to deliver. (Auto-provision of new agents is Phase 4.)
//
// Task 7 ownership: pass {agent, leaseId, owner} to release under the strict lease contract
// (owner mismatch / unknown-or-stale leaseId / already-expired lease → rejected, distinct 4xx
// reasons). {agent} alone (no leaseId, no owner) is the pre-Task-7 legacy shape — rejected by
// default (`missing_fields`, since it can't prove ownership of anything) unless
// HAGENCY_ALLOW_LEGACY_RELEASE=1 is set, in which case it releases whatever lease currently
// holds that agent, no ownership check. Passing exactly one of leaseId/owner (not both, not
// neither) is always a malformed request, rejected as `missing_fields` regardless of the flag.
app.post('/api/dispatch/release', (req, res) => {
  const name = req.body?.agent ? String(req.body.agent) : null;
  if (!name) return res.status(400).json({ error: 'agent required' });
  const leaseId = req.body?.leaseId ? String(req.body.leaseId) : null;
  const owner = req.body?.owner ? String(req.body.owner) : null;
  if (leaseId || owner) {
    if (!leaseId || !owner) {
      return res.status(400).json({ error: 'leaseId and owner must be provided together', reason: 'missing_fields' });
    }
    try {
      dispatchLeaseStore.release({ leaseId, agent: name, owner });
    } catch (error) {
      return respondDispatchLeaseError(res, error);
    }
  } else if (DISPATCH_ALLOW_LEGACY_RELEASE) {
    dispatchLeaseStore.releaseByAgent(name); // shim explicitly enabled — tolerant no-op if nothing to release
  } else {
    return res.status(400).json({
      error: 'leaseId, agent, and owner are required (set HAGENCY_ALLOW_LEGACY_RELEASE=1 to allow legacy {agent}-only release)',
      reason: 'missing_fields',
    });
  }
  const rec = agents[name];
  const role = rec ? agentRole(serializeAgent(rec)) : null;
  const tier = rec ? agentCapability(serializeAgent(rec)) : null;
  if (role && tier) {
    const key = cellKey(role, tier);
    const q = dispatchQueues.get(key) || [];
    const next = q.shift();
    if (next) {
      dispatchQueues.set(key, q);
      // Re-reserve for the drained ticket under a fresh lease. The releasing owner (if any)
      // picks it back up; legacy (ownerless) releases fall back to the default owner.
      const drainedLease = dispatchLeaseStore.create({
        agent: name, owner: owner || DISPATCH_LEASE_DEFAULT_OWNER, taskId: null, ticket: next.ticket,
        role, tier, task: next.task, room: next.room,
      });
      return res.json({ status: 'drained', agent: name, ...next, leaseId: drainedLease.leaseId, expiresAt: drainedLease.expiresAt });
    }
  }
  res.json({ status: 'released', agent: name });
});

app.get('/api/agents/:name', (req, res) => {
  refreshServerLiveness();
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  const memberOf = Object.values(groups).filter(g => g.members.includes(agent.name)).map(g => g.name);
  res.json({ ...serializeAgent(agent), groups: memberOf });
});

app.delete('/api/agents/:name', requireBearer, async (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  if (agentOpsService && agent.agentId) {
    agentOpsService.revokeScopesByBinding({ stableAgentId: agent.agentId });
  }

  const releasedEngagements = [];
  const leftGroups = [];
  let leftProjectRooms = [];
  if (req.query.force === 'true') {
    /*
     * FIRST IT LEAVES THE CUSTOMER'S ROOMS, because that is the only part of this cleanup somebody else can
     * see. Awaited before the record goes so the bindings that name those rooms still exist, and reported
     * rather than enforced — see `withdrawAgentFromProjectRooms` for why a customer's homeserver being down
     * must not make an agent unremovable.
     */
    leftProjectRooms = await withdrawAgentFromProjectRooms(agent.name);
    /*
     * AND IT LEAVES ITS GROUPS — the fourth place one removal did not finish.
     *
     * `soakroom` on the live fleet listed `e2e-probe-1786990578`, `e2e-probe-1787004468` and
     * `e2e-probe-1787020657` as members hours after those agents were deleted. A group naming a member with no
     * record is the same shape as the commitment below: one relationship maintained from one side only.
     *
     * SAME `force` RULE, for the same reason. A soft delete is reversible and `undelete` restores the record;
     * dropping its memberships would make the restored agent silently absent from every room it worked in.
     */
    for (const group of Object.values(groups)) {
      if (!Array.isArray(group?.members) || !group.members.includes(agent.name)) continue;
      group.members = group.members.filter((m) => m !== agent.name);
      leftGroups.push(group.name);
    }
    if (leftGroups.length && !saveGroups()) {
      return res.status(503).json({
        error: `could not remove ${agentName} from ${leftGroups.join(', ')}, so it was not removed`,
        code: 'group_membership_release_failed',
      });
    }
    /*
     * ITS ACTIVE ENGAGEMENTS ARE REVOKED, AND THEIR BUDGET RELEASED. Deleting the record did not touch them, so
     * a removed agent kept a project side's tokens committed forever.
     *
     * Found on the live fleet by the operator reading the projects screen: `已承诺 200k` beside a project that had
     * nobody assigned. Three of the four active engagements belonged to `e2e-probe-*` agents that no longer
     * existed — 150k of somebody's quota held by agents that had been deleted hours earlier, and no path in the
     * product could ever release it.
     *
     * REMOVING A PROJECT SIDE ALREADY DID THIS CORRECTLY, which is what makes it a clear defect rather than an
     * open question: the same rule, enforced on one side of the same relationship and not the other. A commitment
     * outlives its agent means a contributor's quota drains by attrition.
     *
     * ONLY ON `?force=true`, and that distinction cost a correction. A first version released before the force
     * check, so a SOFT delete — which is reversible and has an `undelete` route — would irreversibly destroy the
     * commitments. `revoke` has no inverse, so undelete could not have restored them: a reversible action would
     * have had one permanent consequence.
     *
     * An inactive agent therefore keeps its commitment. That is the right trade: the record survives, the
     * operator can change their mind, and the promise reflects work that was actually promised.
     *
     * BEFORE the record is deleted, because `revoke` refuses an engagement it cannot resolve — and a failure here
     * must not leave an agent half-removed.
     */
    try {
      for (const engagement of engagementStore.list({ state: 'active' })) {
        if (normalizeAgentName(engagement.agent) !== agentName) continue;
        engagementStore.revoke({
          engagementId: engagement.id,
          by: 'agent-removal',
          reason: `agent ${agentName} was removed`,
        });
        releasedEngagements.push(engagement.id);
      }
    } catch (error) {
      /*
       * REPORTED, NOT SWALLOWED, and the delete does not proceed. Removing the agent while its commitment stands
       * is precisely the state this fixes, so failing to release is a reason to stop rather than to continue and
       * leave the leak with a log line about it.
       */
      return res.status(503).json({
        error: `could not release ${agentName}'s active engagements, so it was not removed: ${error?.message || error}`,
        code: 'engagement_release_failed',
      });
    }

    /*
     * STOP IT FIRST. Deleting the record left the agent's tmux session and its codex process
     * running: an orphan that keeps spending the contributor's tokens, keeps its MCP server talking
     * to a backend that no longer knows it, and holds the workspace — while the console reports the
     * agent gone. Observed for real: a session from 00:03 still alive after the record was deleted
     * at 07:55.
     *
     * Not conditional on `online`: the record's own liveness can be stale, and the session either
     * exists or it does not — asking tmux is cheaper and more truthful than trusting the flag.
     */
    const sessionName = normalizeOptionalText(String(agent.tmux || '').split(':')[0], 128) || agentName;
    /*
     * Through `hostRuntime`, never a direct shell-out to the tmux binary. The first version of this
     * fix did exactly that and tests/runtime-interface.test.js rejected it: 34 raw invocations were
     * extracted from this file precisely so it does not assume one platform, and adding one back
     * re-couples it. (The invariant is a source grep, so this comment must not spell the forbidden
     * call either — quoting it verbatim tripped the check on the very line explaining the rule.) Gated on the capability for the same reason — a headless ACP runtime has no
     * pane to kill, and it answers for itself rather than being assumed to behave like tmux.
     */
    let stopped = false;
    if (hostRuntime.capabilities.sessions) {
      stopped = await hostRuntime.killSession(sessionName);
      if (stopped) console.log(`Agent '${agentName}': stopped session '${sessionName}' before deleting`);
    } else {
      console.warn(`[backend] ${hostRuntime.name} runtime cannot stop sessions; deleting '${agentName}' may leave its process running`);
    }
    const deletion = clearDeletedAgentState(agentName);
    if (!deletion.ok) return res.status(503).json({ error: deletion.error || 'agent force-delete persistence failed' });
    console.log(`Agent '${agentName}' permanently deleted`);
    auditLog(req, { agent: agentName, summary: { action: 'force-delete', sessionKilled: stopped } });
    /*
     * REPORTED, because releasing somebody's committed budget is a side effect of this call that the caller did
     * not ask for and cannot see. Silence would make the quota move for reasons nobody could trace.
     */
    return res.json({
      ok: true, deleted: true, name: agentName, sessionKilled: stopped, releasedEngagements,
      leftGroups, leftProjectRooms,
    });
  }
  const persistenceSnapshot = snapshotAgentPersistenceState(agentName);
  agent.tmux = null;
  agent.lastSeen = Date.now();
  if (!agent.offlineReason) agent.offlineReason = 'inactive';
  transitionAgent(agentName, 'api_unregister');
  if (!saveAgentsOrRollback(agentName, persistenceSnapshot)) {
    return res.status(503).json({ error: 'agents persistence failed' });
  }
  auditLog(req, { agent: agentName, summary: { action: 'unregister' } });
  res.json({
    ok: true,
    deprecated: true,
    message: 'unregister is disabled; agent marked inactive. Use ?force=true to permanently delete.',
    agent: serializeAgent(agent),
  });
});

app.post('/api/agents/:name/undelete', requireBearer, (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  if (!deletedAgentTombstones[agentName]) return res.status(404).json({ error: 'no tombstone found' });
  delete deletedAgentTombstones[agentName];
  saveJson('deleted_agents.json', deletedAgentTombstones, { immediate: true });
  auditLog(req, { agent: agentName, summary: { action: 'undelete' } });
  console.log(`Agent '${agentName}' tombstone removed — re-registration allowed`);
  res.json({ ok: true, undeleted: true, name: agentName });
});

app.post('/api/agents/:name/supervisor', requireBearer, (req, res) => {
  if (!isLocalRequest(req)) return res.status(403).json({ error: 'local-only endpoint' });
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  const supervisorName = `supervisor-${agentName}`;
  if (agents[supervisorName] && isAgentRecord(agents[supervisorName])) {
    return res.json({ ok: true, alreadyExists: true, supervisor: supervisorName });
  }
  try {
    provisionSupervisorAgent(agentName);
  } catch (e) {
    console.error(`[supervisor-provision] failed for ${agentName}:`, e.message);
    return res.status(500).json({ error: 'provisioning failed', detail: e.message });
  }
  const record = buildSupervisorAgentRecord(supervisorName, agentName);
  agents[supervisorName] = record;
  saveAgents();
  try { supervisorLifecycleManager.sweepAll(); } catch (_) { /* best-effort */ }
  auditLog(req, { agent: agentName, summary: { action: 'provision-supervisor', supervisor: supervisorName } });
  console.log(`[supervisor-provision] provisioned ${supervisorName} for ${agentName}`);
  res.json({ ok: true, supervisor: supervisorName });
});

app.get('/api/agents/:name/launch-env', requireBearer, (req, res) => {
  if (!isLocalRequest(req)) return res.status(403).json({ error: 'local-only endpoint' });
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  const rp = normalizeRuntimeProfile(agent.runtimeProfile);
  res.json({ runtimeProfile: rp || null });
});

/**
 * Provision a NEW agent — the step nothing could do before.
 *
 * THE HOLE THIS FILLS. Every launcher in the system starts an agent that already exists:
 * `/start` 404s on an unknown name, and the supervisor's own comment says it "launches into an
 * already-provisioned home". The only writer of a new agent record is `POST /api/agents`, which is
 * `requireAgentToken` — the agent registers ITSELF. But it cannot authenticate without a token, and
 * the token lives in a provisioned home, and the only thing that provisioned a home was a shell
 * command. So the first launch of any agent had to be typed by a human on the host, and no UI
 * could offer it.
 *
 * WHY local-only, like `/start`. This spawns a process on the machine the backend runs on. A
 * console served from host A cannot provision on contributor host B — that needs an agent-side
 * daemon there, which does not exist. Rather than pretend otherwise, the constraint is enforced
 * here and stated on the page.
 *
 * WHAT IT DELIBERATELY DOES NOT DO: launch. Provisioning writes a home, a token and an offline
 * agent record; `/start` runs it. Kept separate because they fail for unrelated reasons — a
 * provisioning failure is a filesystem or naming problem, a launch failure is tmux or the
 * framework CLI — and a single call would report the second while hiding which one happened.
 */
/**
 * The agent's pane, as text. Read-only.
 *
 * Asked for directly: "why don't you show the live tmux session in the console". Everything needed
 * already existed — `hostRuntime.capturePane` and `captureLocalPaneContentAsync` — but only the old
 * dashboard on its own port exposed it (`GET /api/tmux/capture/:session` in server.js), and the
 * console proxy speaks to this backend alone. So the capability was present and unreachable.
 *
 * Through the runtime, never a direct tmux call: tests/runtime-interface.test.js forbids the latter,
 * and gating on `capabilities.capture` is what keeps this honest on a headless ACP runtime, which
 * has no pane to capture and says so rather than returning an empty string that reads as "idle".
 *
 * `requireBearer` and NOT local-only: capturing is a read, and unlike /start it spawns nothing. But
 * a pane is the agent's raw screen — it can contain anything the agent printed, including secrets it
 * was handed — so it stays behind the operator credential and is never part of any public payload.
 */
app.get('/api/agents/:name/pane', requireBearer, async (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  if (!hostRuntime.capabilities.capture) {
    /*
     * 501, not an empty capture: "this runtime cannot show you a screen" and "the screen is blank"
     * are different answers, and a caller that cannot tell them apart will render the second.
     */
    return res.status(501).json({
      error: `the ${hostRuntime.name} runtime cannot capture a pane`,
      capture: false,
    });
  }
  const target = normalizeOptionalText(agent.tmux, 128);
  if (!target) {
    return res.status(409).json({ error: 'agent has no pane', reason: agent.online ? 'no_tmux_target' : 'offline' });
  }
  try {
    /*
     * `{ text, hash }`, not a bare string — this helper hashes for the change-detection callers, and
     * assuming a string here cost one round trip with `text.split is not a function`.
     */
    const capture = await captureLocalPaneContentAsync(target);
    if (!capture || typeof capture.text !== 'string') {
      // The pane target is recorded but the runtime could not read it — a session that has gone.
      return res.status(409).json({ error: 'pane could not be captured', target, reason: 'capture_failed' });
    }
    const text = capture.text;
    return res.json({
      ok: true,
      agent: agentName,
      target,
      capturedAt: Date.now(),
      // The hash lets a poller skip re-rendering an unchanged pane.
      hash: capture.hash,
      // Line-capped so one enormous paste cannot make this response the biggest thing on the wire.
      text: text.split('\n').slice(-400).join('\n'),
    });
  } catch (error) {
    console.error(`[pane] capture failed for ${agentName}: ${error.message}`);
    return res.status(502).json({ error: 'pane capture failed', detail: error.message });
  }
});

app.post('/api/agents/:name/provision', requireBearer, (req, res) => {
  if (!isLocalRequest(req)) return res.status(403).json({ error: 'local-only endpoint' });
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  /*
   * Refused rather than merged. Provisioning re-runs a script that writes a home directory and a
   * token; doing that over a live agent could replace the credential it is currently authenticating
   * with, which would take a working agent offline for no stated reason.
   */
  if (isAgentRecord(agents[agentName])) {
    return res.status(409).json({ error: `agent '${agentName}' already exists` });
  }

  const framework = normalizeOptionalText(req.body?.framework, 32);
  const VALID_FRAMEWORKS = new Set(['claude', 'codex']);
  if (!framework || !VALID_FRAMEWORKS.has(framework)) {
    return res.status(400).json({ error: `framework must be one of: ${[...VALID_FRAMEWORKS].join(', ')}` });
  }

  const presetId = normalizeOptionalText(req.body?.presetId, 128) || null;
  const preset = presetId ? frameworkPresets.find((p) => p.id === presetId) : null;
  if (presetId && !preset) return res.status(400).json({ error: `unknown preset: ${presetId}` });
  /*
   * A preset whose framework disagrees with the requested one would produce an agent whose
   * runtimeProfile says codex while its launcher runs claude. Refused here, where the mismatch is
   * still a sentence, rather than at launch where it is a confusing crash.
   */
  if (preset?.framework && preset.framework !== framework) {
    return res.status(400).json({
      error: `preset '${preset.name}' is for ${preset.framework}, not ${framework}`,
    });
  }

  /*
   * `~/foo` is expanded rather than refused. The onboarding form's own placeholder shows `~/ops-ws`,
   * so the shape it teaches was the shape the API rejected — and `normalizeWorkspacePath` wants an
   * absolute path. The expansion is unambiguous HERE and only here: this endpoint is local-only, so
   * `~` can only mean the home of the user the backend runs as, which is the same user the agent
   * will run as. It would not be well-defined on a remote route, which is why it is not done in the
   * shared helper.
   */
  const rawProject = String(req.body?.project ?? '').trim();
  const tildeExpanded = rawProject === '~' || rawProject.startsWith('~/')
    ? path.join(homedir(), rawProject.slice(1))
    : rawProject;
  const project = normalizeWorkspacePath(tildeExpanded);
  if (rawProject && !project) {
    return res.status(400).json({ error: 'project must be an absolute path (or start with ~/)' });
  }

  /*
   * Created if missing, because the form's own hint promises it ("created automatically if it does
   * not exist") while `provision-v1-agent-home.js` refuses with "project path does not exist". The
   * contradiction is what an operator actually hit: they typed a directory they intended to work in
   * and provisioning failed.
   *
   * Only a directory is created, and only the one named. This is a local-only, operator-authenticated
   * route, so the path is the caller's own filesystem and the caller just asked for it — but it is
   * still a write outside the runtime dir, which is why it is logged.
   */
  if (project && !existsSync(project)) {
    try {
      mkdirSync(project, { recursive: true });
      console.log(`[provision] created project directory ${project}`);
    } catch (error) {
      return res.status(400).json({ error: `could not create project directory ${project}: ${error.message}` });
    }
  }

  const script = path.join(REPO_ROOT, 'scripts', 'provision-v1-agent-home.js');
  const args = [script, '--name', agentName, '--type', framework];
  if (project) args.push('--project', project, '--project-mode', 'symlink');

  let provisioned;
  try {
    // Synchronous on purpose: the caller cannot do anything useful until the home exists, and a
    // detached provision whose failure arrives later is how a UI ends up showing a half-made agent.
    const out = execFileSync(process.execPath, args, {
      cwd: REPO_ROOT,
      env: process.env,
      encoding: 'utf-8',
      timeout: 120_000,
      maxBuffer: 8 * 1024 * 1024,
    });
    provisioned = JSON.parse(out);
  } catch (error) {
    // stderr carries the actual reason (bad name, unreadable project path, template missing).
    const detail = String(error?.stderr || error?.message || 'provisioning failed').trim().slice(0, 400);
    console.error(`[provision] ${agentName} failed: ${detail}`);
    return res.status(500).json({ error: `provisioning failed: ${detail}` });
  }
  if (!provisioned?.ok || !provisioned?.paths) {
    return res.status(500).json({ error: 'provisioning script returned no paths' });
  }

  const paths = provisioned.paths;
  const persistenceSnapshot = snapshotAgentPersistenceState(agentName);
  agents[agentName] = {
    name: agentName,
    kind: 'agent',
    type: framework,
    presetId,
    runtimeProfile: preset ? normalizeRuntimeProfile(runtimeProfileFromPreset(preset)) : null,
    executionPolicy: normalizeExecutionPolicy(preset?.executionPolicy, framework),
    agentId: normalizeAgentId(provisioned.name ? `agent_${agentName}` : null) || null,
    homeDir: normalizeWorkspacePath(paths.homeDir) || null,
    workdir: normalizeWorkspacePath(paths.workdir) || null,
    stateDir: normalizeWorkspacePath(paths.stateDir) || null,
    server: LOCAL_SERVER_ID,
    tmux: null,
    // Provisioned is NOT running. Said explicitly so the UI shows "ready to start" rather than a
    // bare offline agent indistinguishable from one that crashed.
    online: false,
    offlineReason: 'provisioned',
    manualDown: false,
    registeredAt: Date.now(),
    discoveredAt: Date.now(),
    lastSeen: Date.now(),
  };
  if (!saveAgentsOrRollback(agentName, persistenceSnapshot)) {
    return res.status(503).json({ error: 'agents persistence failed' });
  }
  /*
   * Reload tokens NOW. Provisioning just wrote <state>/agent-token, and until the backend loads it
   * the agent's own calls are checked against a map that has no entry — which under
   * HAGENCY_AGENT_TOKEN_MODE=hard is the difference between an agent that works and one whose every
   * request is rejected with nothing said in its pane.
   */
  loadAgentTokens();
  auditLog(req, { agent: agentName, summary: { action: 'provision', framework, presetId } });
  return res.status(201).json({
    ok: true,
    agent: serializeAgent(agents[agentName]),
    paths: { homeDir: paths.homeDir, workdir: paths.workdir, stateDir: paths.stateDir },
    nextStep: `POST /api/agents/${encodeURIComponent(agentName)}/start`,
  });
});

const stoppingAgents = new Set();
const startingAgents = new Set();

async function stopManagedAgent(req, res) {
  if (!isLocalRequest(req)) return res.status(403).json({ error: 'local-only endpoint', stopped: false });
  const name = normalizeAgentName(req.params.name);
  if (!name) return res.status(400).json({ error: 'invalid agent name', stopped: false });
  const agent = agents[name];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found', stopped: false });
  if (stoppingAgents.has(name) || startingAgents.has(name)) {
    return res.status(409).json({ error: 'agent start or stop is already in progress', code: 'agent_lifecycle_busy', stopped: false });
  }
  if (!isLocalAgentServer(normalizeServer(agent.server), LOCAL_SERVER_ID)) {
    return res.status(422).json({ error: 'remote agent stop is not supported by this host', code: 'remote_stop_unsupported', stopped: false });
  }
  // ACP hosts are separate processes. Their self-reported PID does not give
  // this backend authority to signal a host process or prove its children exited.
  if (agentTransport(agent) === 'acp') {
    return res.status(422).json({ error: 'ACP requires a confirmed stop from its supervising host', code: 'acp_stop_unconfirmed', stopped: false });
  }
  // Thread runners are owned through dispatch guardians, not tmux names. Fresh
  // provisioned agents have no pane target; inferring one from their name both
  // rejects names outside a legacy allowlist and risks targeting another session.
  // A recorded tmux target still requires the exact legacy ownership policy.
  const hasTmuxTarget = agent.tmux != null && agent.tmux !== '';
  const threadOnly = !hasTmuxTarget && THREAD_SESSIONS_ENABLED && routerStore && agent.agentId
    && ['claude', 'codex'].includes(threadSessionFramework(agent));
  const sessionName = threadOnly ? null : sessionKeyFromTmuxTarget(agent.tmux) || name;
  if (sessionName && (sessionName !== name || !sessionPolicy.allows(sessionName))) {
    return res.status(403).json({ error: 'agent session is outside this host management policy', code: 'unmanaged_session', stopped: false });
  }
  const beforePersistence = snapshotAgentPersistenceState(name);
  syncAgentMachine(name, { manualDown: true });
  agent.offlineReason = 'operator-stop-requested';
  if (!saveAgents(true)) {
    restoreAgentPersistenceState(name, beforePersistence);
    return res.status(503).json({ error: 'could not persist stop fence', stopped: false });
  }
  const cancelledDispatches = [];
  let sessionKilled = false;
  stoppingAgents.add(name);
  try {
    if (agent.stopUnconfirmedDispatches?.length) {
      // Outcome inspection establishes the workspace state, not process exit.
      // Its resolutionAction cannot release this separate termination fence.
      throw new Error('runner termination remains unconfirmed; workspace outcome resolution cannot verify host process cleanup');
    }
    const dispatches = THREAD_SESSIONS_ENABLED && agent.agentId
      ? routerStore.listAgentDispatches(agent.agentId) : [];
    const dispatchIds = new Set(dispatches.map((dispatch) => dispatch.dispatchId));
    // Cancellation may already have settled the router row while its guardian
    // is still terminating. Process ownership outlives that row's live state.
    const ownedEntries = [...liveThreadSessionRunners].filter(([dispatchId, runner]) =>
      dispatchIds.has(dispatchId) || (runner.agentId === agent.agentId && runner.agentName === name));
    const ownedRunners = ownedEntries.map(([, runner]) => runner);
    const ownedIds = ownedEntries.map(([dispatchId]) => dispatchId);
    const unconfirmedRunners = [];
    for (const dispatch of dispatches) {
      const runner = liveThreadSessionRunners.get(dispatch.dispatchId);
      if (!runner && dispatch.state !== 'queued') unconfirmedRunners.push(dispatch.dispatchId);
      // Fence the capability before asking the child to exit. Started work is
      // still uncertain and retains its resource quarantine after termination.
      const result = dispatch.state === 'queued' || dispatch.state === 'leased'
        ? routerStore.cancelBeforeStart(dispatch.dispatchId)
        : routerStore.markOutcomeUnknown(dispatch.dispatchId, 'operator_stopped_agent');
      if (!result.ok) throw new Error(`dispatch ${dispatch.dispatchId}: ${result.message || result.code}`);
      cancelledDispatches.push(dispatch.dispatchId);
    }
    for (const runner of ownedRunners) runner.controller.abort();
    if (unconfirmedRunners.length) {
      agent.stopUnconfirmedDispatches = unconfirmedRunners;
      if (!saveAgents(true)) throw new Error('could not persist unconfirmed runner termination');
    }
    // A timed-out HTTP request must not turn a second stop into success while
    // its child is still exiting. Clear these receipts only after owned cleanup.
    agent.stopUnconfirmedDispatches = [...new Set([...(agent.stopUnconfirmedDispatches || []), ...ownedIds])];
    if (ownedIds.length && !saveAgents(true)) throw new Error('could not persist pending runner cleanup');
    const cleanup = Promise.allSettled(ownedRunners.map((runner) => runner.running)).then(() => {
      const confirmedIds = ownedIds.filter((_, index) => ownedRunners[index].cleanupConfirmed === true);
      const pending = agent.stopUnconfirmedDispatches || [];
      agent.stopUnconfirmedDispatches = pending.filter((id) => !confirmedIds.includes(id));
      if (ownedIds.length && !saveAgents(true)) {
        agent.stopUnconfirmedDispatches = pending;
        throw new Error('could not persist confirmed runner cleanup');
      }
      if (confirmedIds.length !== ownedIds.length) throw new Error('owned runner settled without confirmed host process cleanup');
    });
    let cleanupTimer;
    try {
      await Promise.race([
        cleanup,
        new Promise((_, reject) => {
          cleanupTimer = setTimeout(() => reject(new Error('runner process cleanup did not finish')), 10000);
        }),
      ]);
    } finally {
      clearTimeout(cleanupTimer);
    }
    if (unconfirmedRunners.length) throw new Error('runner process ownership is unavailable; termination is unconfirmed');
    if (sessionName) {
      if (!hostRuntime.capabilities.sessions) throw new Error('host runtime cannot verify session termination');
      const before = await hostRuntime.listPanes();
      if (!before.ok) throw new Error('host runtime could not enumerate managed sessions');
      if (before.panes.some((pane) => pane.session === sessionName)) {
        sessionKilled = await hostRuntime.killSession(sessionName);
      }
      const after = await hostRuntime.listPanes();
      if (!after.ok || after.panes.some((pane) => pane.session === sessionName)) {
        throw new Error('managed session termination was not confirmed');
      }
    }
    if (THREAD_SESSIONS_ENABLED && agent.agentId && routerStore.listAgentDispatches(agent.agentId).length) {
      throw new Error('agent still has an unsettled dispatch');
    }
    agent.tmux = null;
    agent.offlineReason = 'operator-stopped';
    agent.lastSeen = Date.now();
    const runtime = ensureAgentRuntimeRecord(name);
    runtime.activeNow = false;
    runtime.activeDurationSec = 0;
    runtime.idleDurationSec = 0;
    setRuntimeObservation(runtime, { observerSource: 'operator-stop', observerServer: LOCAL_SERVER_ID, observedAt: agent.lastSeen });
    if (!saveAgentRuntime(true)) throw new Error('processes stopped but runtime persistence failed');
    if (!saveAgents(true)) throw new Error('processes stopped but agent persistence failed');
    auditLog(req, { agent: name, summary: { action: 'stop', sessionKilled, cancelledDispatches } });
    return res.json({ ok: true, stopped: true, name, sessionKilled, cancelledDispatches, agent: serializeAgent(agent) });
  } catch (error) {
    auditLog(req, { agent: name, summary: { action: 'stop-unconfirmed', sessionKilled, cancelledDispatches } });
    return res.status(503).json({ error: error.message, code: 'stop_unconfirmed', stopped: false, sessionKilled, cancelledDispatches });
  } finally {
    stoppingAgents.delete(name);
  }
}
app.post('/api/agents/:name/stop', requireBearer, (req, res) => {
  if (!isLocalRequest(req)) return res.status(403).json({ error: 'local-only endpoint', stopped: false });
  return stopManagedAgent(req, res);
});

app.post('/api/agents/:name/start', requireBearer, (req, res) => {
  if (!isLocalRequest(req)) return res.status(403).json({ error: 'local-only endpoint' });
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  if (agent.online) return res.status(409).json({ error: 'agent already online' });
  if (stoppingAgents.has(agentName) || startingAgents.has(agentName) || agent.stopUnconfirmedDispatches?.length) {
    return res.status(409).json({ error: 'agent lifecycle cleanup is still pending', code: 'agent_lifecycle_busy' });
  }
  const VALID_FRAMEWORKS = new Set(['claude', 'codex']);
  const framework = agent.type;
  if (!framework || !VALID_FRAMEWORKS.has(framework)) {
    return res.status(400).json({ error: `agent has no valid framework (type='${agent.type || 'null'}'). Update agent type to claude or codex first.` });
  }
  const hagencyBin = path.join(REPO_ROOT, 'bin', 'hagency');
  const launchEnv = { ...process.env, HAGENCY_LAUNCH_ENV_READY: '1' };
  const rp = agent.runtimeProfile?.primary;
  if (rp?.apiBaseUrl) launchEnv.ANTHROPIC_BASE_URL = rp.apiBaseUrl;
  if (rp?.apiKey) launchEnv.ANTHROPIC_API_KEY = rp.apiKey;
  if (rp?.model) launchEnv.HAGENCY_LAUNCH_MODEL = rp.model;
  if (rp?.extraArgs) launchEnv.HAGENCY_LAUNCH_EXTRA_ARGS = rp.extraArgs;
  startingAgents.add(agentName);
  try {
    /*
     * The launcher's OUTPUT IS KEPT. This was `stdio: 'ignore'`, so a launch that failed — a
     * missing framework CLI, tmux refusing, an untrusted hook, a bad workdir — produced
     * `{ok: true, pid}` and nothing else: the endpoint reported success because the process had
     * been created, and the reason it then died went to /dev/null. Observed doing exactly that.
     */
    const logDir = path.join(RUNTIME_ROOT, 'data', 'agents', agentName);
    mkdirSync(logDir, { recursive: true });
    const logPath = path.join(logDir, 'launch.log');
    const logFd = openSync(logPath, 'a');
    const child = spawn(hagencyBin, ['up-v1', agentName, framework], {
      cwd: REPO_ROOT,
      env: launchEnv,
      // stdin stays closed: a launcher that waits for input from a daemon would hang forever, and
      // the one prompt that exists (codex hook trust) is designed to refuse rather than block.
      stdio: ['ignore', logFd, logFd],
      detached: true,
    });
    /*
     * Attached BEFORE unref, and it still fires: unref only stops the child keeping the event loop
     * alive, it does not stop the exit event while the backend is running. This is the only place a
     * non-zero exit becomes visible at all, since the response has long since been sent.
     */
    child.on('exit', (code, signal) => {
      startingAgents.delete(agentName);
      try { closeSync(logFd); } catch { /* already closed */ }
      if (code === 0) return;
      console.error(`[start] hagency up-v1 ${agentName} exited ${signal ? `on ${signal}` : `with code ${code}`} — see ${logPath}`);
      /*
       * Undo the optimistic online. The record was marked running on the assumption the launch
       * would work; leaving it there after a failure claims a tmux session that does not exist,
       * which reads as a live agent that has merely gone quiet.
       */
      const a = agents[agentName];
      // The session sweep may have already noticed the missing pane. Preserve
      // the launcher's more specific failure, but never overwrite operator Stop.
      if (isAgentRecord(a) && a.manualDown) return;
      if (isAgentRecord(a)) {
        a.tmux = null;
        a.offlineReason = `launch-failed:exit-${signal || code}`;
        transitionAgent(agentName, 'api_unregister');
        saveAgents();
      }
    });
    child.on('error', () => startingAgents.delete(agentName));
    child.unref();
    agent.tmux = `${agentName}:0.0`;
    agent.lastSeen = Date.now();
    agent.offlineReason = null;
    transitionAgent(agentName, 'api_register_with_tmux');
    saveAgents();
    auditLog(req, { agent: agentName, summary: { action: 'start', framework, pid: child.pid } });
    console.log(`[start] launched hagency up-v1 ${agentName} ${framework} (pid=${child.pid}, log=${logPath})`);
    /*
     * `launching`, not `started`. The process exists; whether the agent comes up is decided by the
     * launcher over the next few seconds and reported by its own heartbeat. Saying "ok" alone
     * invited the caller to treat spawn success as agent success, which is the bug above.
     */
    res.json({ ok: true, name: agentName, framework, pid: child.pid, state: 'launching', log: logPath });
  } catch (e) {
    startingAgents.delete(agentName);
    console.error(`[start] failed to launch ${agentName}:`, e.message);
    res.status(500).json({ error: 'launch failed', detail: e.message });
  }
});

app.post('/api/agents/:name/offline', requireAgentToken(_tokenFromName), (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  const wasOnline = agent.online === true;
  const wasManualDown = agent.manualDown === true;
  const reason = (typeof req.body?.reason === 'string' && req.body.reason.trim())
    ? req.body.reason.trim()
    : 'manual-offline';
  const clearTmux = req.body?.clearTmux !== false;
  const manualDown = req.body?.manualDown === undefined
    ? isManualDownReason(reason)
    : req.body.manualDown === true;
  const persistenceSnapshot = snapshotAgentPersistenceState(agentName);
  // online/manualDown driven by machine
  agent.lastSeen = Date.now();
  agent.offlineReason = reason;
  if (clearTmux) agent.tmux = null;
  if (manualDown) syncAgentMachine(agentName, { manualDown: true });
  else syncAgentMachine(agentName, { tmuxMissing: true });
  if (!saveAgentsOrRollback(agentName, persistenceSnapshot)) {
    return res.status(503).json({ error: 'agents persistence failed' });
  }
  if (wasOnline && !manualDown && !wasManualDown) {
    maybeEmitUnexpectedOfflineAlert(agentName, reason, { server: normalizeServer(agent.server) || 'local', detail: 'Marked offline via API' });
  }
  res.json({ ok: true, agent: serializeAgent(agent) });
});

app.post('/api/agents/:name/heartbeat', requireAgentToken(_tokenFromName), (req, res) => {
  refreshServerLiveness();
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });

  const now = Date.now();
  const server = normalizeServer(req.body?.server)
    || normalizeServer(agents[agentName]?.server)
    || (isLocalRequest(req) ? 'local' : null);
  const workspacePath = Object.prototype.hasOwnProperty.call(req.body || {}, 'workspacePath')
    ? req.body.workspacePath
    : undefined;
  const normalizedWorkspacePath = normalizeWorkspacePath(workspacePath);
  const tmux = (typeof req.body?.tmux === 'string' && req.body.tmux.trim())
    ? req.body.tmux.trim()
    : `${agentName}:0.0`;

  const agentSnapshot = snapshotAgentPersistenceState(agentName);
  const runtimeSnapshot = snapshotAgentRuntimeState(agentName);
  let created = false;
  let agent = agents[agentName];
  if (!isAgentRecord(agent)) {
    const ensured = ensureAgentRecord(agentName, {
      server,
      tmux,
      online: true,
      type: 'agent',
      kind: 'agent',
      offlineReason: null,
      workdir: normalizedWorkspacePath,
    });
    if (!ensured) return res.status(410).json({ error: 'agent cannot be registered' });
    agent = ensured.agent;
    created = ensured.created;
  }

  const wasOnline = agent.online === true;
  const wasManualDown = agent.manualDown === true;
  if (server && normalizeServer(agent.server) !== server) agent.server = server;
  // The heartbeat comes from the agent's own mcp-server.js child, which sends a
  // tmux target because historically every agent had one. An ACP agent does not,
  // and accepting it here routes a paneless agent down the pane path: the
  // dashboard then asks getPaneIdleMs about a pane that cannot exist and reports
  // idleMs -1 forever. This only started happening once octos gained MCP support,
  // because before that it never sent a heartbeat at all.
  if (!wasManualDown && (!agent.tmux || !String(agent.tmux).trim())
      && agentTransport(agent) !== 'acp') agent.tmux = tmux;
  if (normalizedWorkspacePath && !normalizeWorkspacePath(agent.workdir)) agent.workdir = normalizedWorkspacePath;
  if (!wasManualDown && (agent.offlineReason === 'mcp-missing:auto' || agent.offlineReason === 'inactive')) {
    agent.offlineReason = null;
  }
  agent.lastSeen = now;
  syncAgentMachine(agentName, {
    heartbeatPresent: true,
    tmuxPresent: !wasManualDown,
    mcpPresent: true,
  });

  const runtime = ensureAgentRuntimeRecord(agentName);
  if (!runtime) {
    restoreAgentPersistenceState(agentName, agentSnapshot);
    restoreAgentRuntimeState(agentName, runtimeSnapshot);
    return res.status(500).json({ error: 'runtime update failed' });
  }
  setRuntimeMcpFields(runtime, { mcpPresent: true }, now);
  runtime.mcpHeartbeatAt = now;
  if (workspacePath !== undefined) setRuntimeWorkspacePath(runtime, { workspacePath });
  setRuntimeObservation(runtime, {
    observerSource: 'mcp-heartbeat',
    observerServer: server,
    observedAt: now,
  });
  runtime.lastSeen = now;
  runtime.updatedAt = now;

  if (!saveAgentsOrRollback(agentName, agentSnapshot)) {
    restoreAgentRuntimeState(agentName, runtimeSnapshot);
    return res.status(503).json({ error: 'agents persistence failed' });
  }
  if (!saveAgentRuntime(true)) {
    restoreAgentPersistenceState(agentName, agentSnapshot);
    restoreAgentRuntimeState(agentName, runtimeSnapshot);
    saveAgents(true);
    return res.status(503).json({ error: 'agent runtime persistence failed' });
  }

  writeThruAgentHome(agentName);
  if (!wasOnline && !wasManualDown && agent.online === true) {
    notifyAgentCatchup(agentName, 'mcp-heartbeat-restored').catch((e) => {
      console.error(`catchup notify failed for ${agentName}:`, e.message);
    });
  }
  auditLog(req, {
    agent: agentName,
    summary: {
      heartbeat: true,
      created,
      server,
      mcpPresent: true,
      observerSource: 'mcp-heartbeat',
    },
  });
  res.json({
    ok: true,
    created,
    agent: serializeAgent(agent),
    runtime: {
      agent: agentName,
      mcpPresent: runtime.mcpPresent === true,
      mcpMissingSince: Number(runtime.mcpMissingSince) || null,
      lastSeen: runtime.lastSeen || now,
      updatedAt: runtime.updatedAt || now,
      workspacePath: runtime.workspacePath || null,
      observation: serializeRuntimeObservation(runtime),
    },
  });
});

app.post('/api/agents/:name/runtime', requireAgentToken(_tokenFromName), (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });

  const blocked = req.body?.blocked === true;
  const reason = (typeof req.body?.reason === 'string' && req.body.reason.trim())
    ? req.body.reason.trim()
    : null;
  const tail = (typeof req.body?.tail === 'string') ? req.body.tail : '';
  const command = (typeof req.body?.command === 'string') ? req.body.command : '';
  const server = normalizeServer(req.body?.server);
  const activeNow = Object.prototype.hasOwnProperty.call(req.body || {}, 'activeNow')
    ? (req.body.activeNow === true ? true : (req.body.activeNow === false ? false : null))
    : undefined;
  const activeDurationSec = req.body?.activeDurationSec;
  const idleDurationSec = req.body?.idleDurationSec;
  const lastTmuxActivitySec = req.body?.lastTmuxActivitySec;
  const workspacePath = Object.prototype.hasOwnProperty.call(req.body || {}, 'workspacePath')
    ? req.body.workspacePath
    : undefined;
  const mcpPresent = Object.prototype.hasOwnProperty.call(req.body || {}, 'mcpPresent')
    ? (req.body.mcpPresent === true ? true : (req.body.mcpPresent === false ? false : null))
    : undefined;
  const blockedObserved = Object.prototype.hasOwnProperty.call(req.body || {}, 'blockedObserved')
    ? req.body.blockedObserved === true
    : true;

  const bodyTransport = typeof req.body?.transport === 'string' ? req.body.transport.trim().toLowerCase() : '';
  let agent = agents[agentName];
  if (!isAgentRecord(agent)) {
    const ensured = ensureAgentRecord(agentName, {
      server,
      // A paneless agent reporting runtime state must not be conjured into existence
      // as a tmux agent. The body declares its transport; honour it.
      tmux: bodyTransport === 'acp' ? null : `${agentName}:0.0`,
      transport: bodyTransport === 'acp' ? 'acp' : undefined,
      online: true,
      type: 'agent',
      kind: 'agent',
      offlineReason: null,
    });
    if (!ensured) return res.status(400).json({ error: 'invalid agent name' });
    agent = ensured.agent;
    saveAgents();
  }

  const transition = applyRuntimeObservation(agentName, {
    blocked,
    reason,
    tail,
    command,
    server,
    activeNow,
    activeDurationSec,
    idleDurationSec,
    lastTmuxActivitySec,
    /*
     * SPREAD, NOT SHORTHAND — the shorthand defeated its own guard downstream.
     *
     * `workspacePath` is `undefined` when the request omitted it, and
     * `workspacePath,` puts the KEY there regardless. setRuntimeWorkspacePath()
     * correctly asks `hasOwnProperty`, sees the key, normalizes `undefined` to null,
     * and erases a path that was already known. So a heartbeat that mentioned nothing
     * about the workspace wiped it, and the next one restored it.
     *
     * Latent until something actually set the field for ACP agents: while it was always
     * null, clearing it was a no-op. It matters now because the workspace is the only
     * link between an agent and the token usage its CLI records, and attribution that
     * flickers with the heartbeat is worse than attribution that is absent.
     */
    ...(workspacePath !== undefined ? { workspacePath } : {}),
    mcpPresent,
    blockedObserved,
    observerSource: 'runtime-api',
    observerServer: server,
  });
  const runtime = dispatchBlockedNotifications(transition);
  if (!runtime) return res.status(500).json({ error: 'runtime update failed' });
  auditLog(req, {
    agent: agentName,
    summary: {
      blocked,
      reason,
      mcpPresent: runtime.mcpPresent ?? null,
      server,
      observerSource: runtime.observation?.observerSource || null,
    },
  });
  res.json({
    ok: true,
    runtime: {
      agent: agentName,
      blocked: runtime.blocked === true,
      blockedReason: runtime.blockedReason || null,
      blockedTier: normalizeBlockedTier(runtime.blockedTier, null),
      blockedSince: runtime.blockedSince || null,
      activeNow: normalizeRuntimeActiveNow(runtime.activeNow),
      activeDurationSec: Number(runtime.activeDurationSec) || 0,
      idleDurationSec: Number(runtime.idleDurationSec) || 0,
      lastTmuxActivitySec: Number(runtime.lastTmuxActivitySec) || null,
      workspacePath: runtime.workspacePath || null,
      observation: serializeRuntimeObservation(runtime),
      mcpPresent: runtime.mcpPresent === true
        ? true
        : (runtime.mcpPresent === false ? false : null),
      mcpMissingSince: Number(runtime.mcpMissingSince) || null,
      updatedAt: runtime.updatedAt || Date.now(),
    },
  });
});

app.post('/api/runtime/compact', requireBearer, (req, res) => {
  const agentName = normalizeAgentName(req.body?.agent);
  if (!agentName) return res.status(400).json({ error: 'agent required' });

  const agent = agents[agentName];
  if (!isAgentRecord(agent)) {
    return res.json({ ok: true, ignored: 'agent-not-found', agent: agentName });
  }

  const result = emitRuntimeCompactEvent(agentName, {
    mode: req.body?.mode,
    marker: req.body?.marker,
    source: req.body?.source,
    summary: req.body?.summary,
  });
  res.json(result);
});

/*
 * Shared by the HTTP route below and the delivery queue's in-process sink. The queue used to POST this
 * to its own backend over HTTP; it now lives here, so the ack is a function call — same record, same
 * delivery event, no socket. The `local only` check stays on the ROUTE: the in-process caller is by
 * definition local, and gating the function would re-check a property it cannot fail.
 */
function recordPushDelivered(rawBody = {}) {
  const agentName = normalizeAgentName(rawBody?.agent);
  if (!agentName) return { status: 400, body: { error: 'agent required' } };
  if (!isAgentRecord(agents[agentName])) {
    return { status: 200, body: { ok: true, ignored: 'agent-not-found', agent: agentName } };
  }

  const details = {
    deliveredAt: rawBody?.deliveredAt,
    queuedAt: rawBody?.queuedAt,
    queueEntryId: rawBody?.queueEntryId,
  };
  const notifyMeta = (rawBody?.notifyMeta && typeof rawBody.notifyMeta === 'object')
    ? rawBody.notifyMeta
    : {};
  const result = markAgentPushDelivered(agentName, {
    ...details,
    ...notifyMeta,
  });
  appendDeliveryEvent({
    type: 'push.delivered_ack',
    source: 'backend',
    messageId: notifyMeta.sourceMsgId,
    messageIds: notifyMeta.messageIds,
    agent: agentName,
    queueEntryId: details.queueEntryId,
    queuedAt: details.queuedAt,
    deliveredAt: details.deliveredAt,
    notifyMeta,
    result: result?.ignored ? 'ignored' : 'accepted',
    reason: result?.ignored || null,
  });
  if (result?.ignored) {
    return { status: 200, body: { ok: true, agent: agentName, ignored: result.ignored } };
  }
  return { status: 200, body: { ok: true, agent: agentName } };
}

app.post('/api/runtime/push-delivered', (req, res) => {
  if (!isLocalRequest(req)) return res.status(403).json({ error: 'local only' });
  const { status, body } = recordPushDelivered(req.body ?? {});
  return res.status(status).json(body);
});











// ── Tasks CRUD ───────────────────────────────────────────────────────
const _tokenFromTaskAssignee = r => { const t = taskStore.getTask(r.params?.id); return t?.assignee || ''; };
function parseTaskPageInt(value, fallback = 0) {
  const n = Number.parseInt(value, 10);
  return Number.isFinite(n) && n >= 0 ? n : fallback;
}
function parseTaskPageLimit(value) {
  const n = Number.parseInt(value, 10);
  if (!Number.isFinite(n) || n <= 0) return null;
  return Math.min(n, 500);
}

function respondTaskStoreError(res, error, fallbackMessage) {
  if (error.code === 'not_found') return res.status(404).json({ error: error.message });
  if (error.code === 'persistence_failed') return res.status(503).json({ error: error.message });
  if (error.code) return res.status(400).json({ error: error.message });
  return res.status(500).json({ error: fallbackMessage });
}

function notifyTaskAssignee(task) {
  if (!task || !task.assignee) return;
  if (!isAgentRecord(agents[task.assignee])) return;
  const title = (task.title || '').trim() || '(untitled)';
  const desc = (task.description || '').trim();
  const summary = `New task assigned: ${title} (${task.id}, ${task.priority})`;
  const full = [
    `You have been assigned a new task.`,
    `id: ${task.id}`,
    `title: ${title}`,
    `priority: ${task.priority}`,
    `status: ${task.status}`,
    desc ? `description: ${desc}` : 'description: (none)',
  ].join('\n');
  try {
    dispatchInternalDirectMessage({
      from: 'system',
      to: task.assignee,
      type: 'inform',
      priority: 'normal',
      summary,
      full,
      schema: {
        kind: SYSTEM_TASK_ASSIGNED_SCHEMA_KIND,
        version: 1,
        payload: { taskId: task.id, priority: task.priority, title },
      },
    });
  } catch (e) {
    // Best-effort notification: task creation is the primary contract and must
    // succeed even if push-relay/queue isn't healthy. Surface for ops without
    // failing the request.
    console.warn(`[task] failed to notify assignee ${task.assignee} of ${task.id}: ${e.message}`);
  }
}

app.post('/api/tasks', requireBearer, requireTaskReadAccess, (req, res) => {
  try {
    const task = taskStore.createTask(req.body || {});
    broadcastSSE('task_created', task);
    notifyTaskAssignee(task);
    return res.json({ ok: true, task });
  } catch (error) {
    return respondTaskStoreError(res, error, 'failed to create task');
  }
});

function requireTaskReadAccess(req, res, next) {
  if (THREAD_SESSIONS_ENABLED && !hasApiTokenAccess(req)) {
    return res.status(403).json({ error: 'global task access requires operator authority; runners must use scoped task operations' });
  }
  return next();
}

app.get('/api/tasks', requireTaskReadAccess, (req, res) => {
  const filters = {};
  if (req.query.assignee) filters.assignee = req.query.assignee;
  if (req.query.status) filters.status = req.query.status;
  if (req.query.priority) filters.priority = req.query.priority;
  if (req.query.label) filters.label = req.query.label;
  const offset = parseTaskPageInt(req.query.offset, 0);
  const limit = parseTaskPageLimit(req.query.limit);
  let taskRows = taskStore.listTasks(filters);
  if (offset > 0 || limit !== null) {
    taskRows = taskRows.slice(offset, limit === null ? undefined : offset + limit);
  }
  const tasks = taskRows.map(task => {
    if (!task.assignee) return task;
    const snapshot = supervisorSnapshotStore.getTarget(task.assignee);
    if (!snapshot) return task;
    return { ...task, health: { state: snapshot.state, confidence: snapshot.confidence, reason: snapshot.reason, suggested_action: snapshot.suggested_action, domain: snapshot.domain, pattern: snapshot.pattern, assessed_at: snapshot.assessed_at, assessed_by: snapshot.supervisor } };
  });
  return res.json(tasks);
});

app.get('/api/tasks/:id', requireTaskReadAccess, (req, res) => {
  const task = taskStore.getTask(req.params.id);
  if (!task) return res.status(404).json({ error: 'task not found' });
  // Read-through: enrich health from supervisor snapshot store
  const enriched = { ...task };
  if (task.assignee) {
    const snapshot = supervisorSnapshotStore.getTarget(task.assignee);
    if (snapshot) {
      enriched.health = {
        state: snapshot.state,
        confidence: snapshot.confidence,
        reason: snapshot.reason,
        suggested_action: snapshot.suggested_action,
        domain: snapshot.domain,
        pattern: snapshot.pattern,
        assessed_at: snapshot.assessed_at,
        assessed_by: snapshot.supervisor,
      };
    }
  }
  return res.json(enriched);
});

app.patch('/api/tasks/:id', requireBearer, requireTaskReadAccess, (req, res) => {
  try {
    const task = taskStore.updateTask(req.params.id, req.body || {});
    broadcastSSE('task_updated', task);
    return res.json({ ok: true, task });
  } catch (error) {
    return respondTaskStoreError(res, error, 'failed to update task');
  }
});

function requireLegacyTaskAccess(req, res, next) {
  if (THREAD_SESSIONS_ENABLED && !hasApiTokenAccess(req)) {
    const task = taskStore.getTask(req.params.id);
    const assignee = task?.assignee ? agents[task.assignee] : null;
    if (isAgentRecord(assignee) && threadSessionAgentEligibility(assignee).ok) {
      return res.status(403).json({ error: 'thread-session agents must use capability-scoped task operations', code: 'runner_capability_required' });
    }
  }
  return next();
}

app.patch('/api/tasks/:id/execution', requireAgentToken(_tokenFromTaskAssignee), requireLegacyTaskAccess, (req, res) => {
  try {
    const task = taskStore.updateTaskExecution(req.params.id, req.body || {});
    broadcastSSE('task_updated', task);
    return res.json({ ok: true, task });
  } catch (error) {
    return respondTaskStoreError(res, error, 'failed to update task execution');
  }
});

app.delete('/api/tasks/:id', requireBearer, requireTaskReadAccess, (req, res) => {
  try {
    const task = taskStore.deleteTask(req.params.id);
    if (!task) return res.status(404).json({ error: 'task not found' });
    broadcastSSE('task_deleted', task);
    return res.json({ ok: true, task });
  } catch (error) {
    return respondTaskStoreError(res, error, 'failed to delete task');
  }
});

app.post('/api/tasks/:id/accept', requireAgentToken(_tokenFromTaskAssignee), requireLegacyTaskAccess, (req, res) => {
  try {
    const task = taskStore.transitionTask(req.params.id, 'accepted');
    broadcastSSE('task_updated', task);
    return res.json({ ok: true, task });
  } catch (error) {
    return respondTaskStoreError(res, error, 'failed to accept task');
  }
});

app.post('/api/tasks/:id/transition', requireAgentToken(_tokenFromTaskAssignee), requireLegacyTaskAccess, (req, res) => {
  try {
    const status = (typeof req.body?.status === 'string') ? req.body.status.trim() : '';
    if (!status) return res.status(400).json({ error: 'status is required' });
    const task = taskStore.transitionTask(req.params.id, status, req.body);
    broadcastSSE('task_updated', task);
    if (status === 'in_progress') scheduleRouterPump();
    return res.json({ ok: true, task });
  } catch (error) {
    return respondTaskStoreError(res, error, 'failed to transition task');
  }
});

app.post('/api/tasks/:id/comments', requireBearer, requireTaskReadAccess, (req, res) => {
  try {
    const task = taskStore.addComment(req.params.id, req.body || {});
    broadcastSSE('task_updated', task);
    return res.json({ ok: true, task });
  } catch (error) {
    return respondTaskStoreError(res, error, 'failed to add comment');
  }
});

app.get('/api/agents/:name/tasks', requireTaskReadAccess, (req, res) => {
  const name = normalizeAgentName(req.params.name);
  if (!name) return res.status(400).json({ error: 'invalid agent name' });
  const snapshot = supervisorSnapshotStore.getTarget(name);
  const tasks = taskStore.listTasks({ assignee: name }).map(task => {
    if (!snapshot) return task;
    return { ...task, health: { state: snapshot.state, confidence: snapshot.confidence, reason: snapshot.reason, suggested_action: snapshot.suggested_action, domain: snapshot.domain, pattern: snapshot.pattern, assessed_at: snapshot.assessed_at, assessed_by: snapshot.supervisor } };
  });
  return res.json(tasks);
});

// ── Framework manifests ────────────────────────────────────────────────
/*
 * The five adapters, as a client can consume them.
 *
 * listFrameworks() has been available since the registry existed but was never
 * routed, so every caller that needed to know "which frameworks are there, and
 * what will they refuse" had to hardcode the answer. A configuration wizard is
 * the first client that cannot: it has to show the sandbox being lent before the
 * contributor commits to lending it.
 *
 * Projected rather than returned wholesale — a compiled manifest carries RegExp
 * and Set values that JSON.stringify flattens to `{}`, so a bare res.json() would
 * silently ship empty guards and read as "this framework refuses nothing".
 */
function serializeFramework(f) {
  return {
    id: f.id,
    displayName: f.displayName,
    transport: f.transport,
    launchable: f.launchable,
    notLaunchableReason: f.notLaunchableReason,
    command: f.launch?.command ?? null,
    defaultArgs: [...(f.launch?.defaultArgs ?? [])],
    modelFlag: f.launch?.modelFlag ?? null,
    permissionSummary: f.launch?.permissionSummary ?? null,
    // ACP-specific notes. `codex-acp` accepts --model and ignores it; `hermes`
    // dies on it. A wizard that offers a model choice the adapter cannot honour
    // is worse than one that says so, hence these travel with the manifest.
    acpModelFlag: f.launch?.acpModelFlag ?? null,
    acpModelFlagNote: f.launch?.acpModelFlagNote ?? null,
    commandNote: f.launch?.commandNote ?? null,
    // Flattened from the guard's Set/RegExp forms so the client can list what
    // hagency will refuse without reimplementing the matcher.
    refusedFlags: f.flagGuard
      ? [...f.flagGuard.exact, ...f.flagGuard.prefix].sort()
      : [],
    guardMessage: f.flagGuard?.message ?? null,
  };
}

app.get('/api/frameworks', (_req, res) => {
  return res.json(listFrameworks().map(serializeFramework));
});

/*
 * What this host can actually run — probed, not declared.
 *
 * The distinction from GET /api/frameworks matters and is the reason both exist:
 * that route answers "which adapters does hagency know", which is a property of the
 * manifests and identical on every machine. This one answers "which of them will
 * start HERE", which is a property of the machine and is the only question an
 * onboarding page can be built on. A console that showed the manifest list as
 * though it were an inventory would invite a contributor to onboard a framework
 * that is not installed, and the failure would surface at launch.
 *
 * WHAT IS PROBED AND WHAT IS NOT:
 *
 *  - **on PATH, and its version** — real, by running the binary's own version flag
 *    with a short timeout. A binary that hangs is reported as present-but-unusable
 *    rather than allowed to hang this request.
 *  - **credential home present** — real, but only that the path EXISTS. Whether the
 *    credential inside it is valid cannot be determined without spending a request
 *    against the provider, so `credentialPresent` means "there is somewhere for a
 *    login to live", never "you are logged in". Conflating those two would be the
 *    worst possible error here: it would tell a contributor they are ready when the
 *    first real task will fail on auth.
 *
 * hagency does not install frameworks and does not hold their credentials, so the
 * response carries the one command that fixes each gap rather than offering to fix
 * it.
 */
const VERSION_FLAG = { claude: '--version', codex: '--version', 'codex-acp': '--version', octos: '--version', hermes: '--version' };

async function probeFramework(f) {
  const command = f.launch?.command ?? f.id;
  let onPath = false;
  let version = null;
  let probeError = null;
  try {
    // `which` first: a missing binary must be reported as missing, not as a
    // version probe that failed for some unexplained reason.
    await execFileAsync('which', [command], { timeout: 3000 });
    onPath = true;
  } catch {
    onPath = false;
  }
  if (onPath) {
    try {
      const { stdout } = await execFileAsync(command, [VERSION_FLAG[f.id] ?? '--version'], {
        timeout: 5000,
        // Never inherit stdin: a CLI that reads it waits forever, which is how a
        // health probe becomes an outage.
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      version = String(stdout).trim().split('\n')[0].slice(0, 80) || null;
    } catch (e) {
      probeError = e?.killed ? 'version probe timed out' : (e?.message ?? 'version probe failed').slice(0, 120);
    }
  }

  /*
   * FOR AN ACP FRAMEWORK, `--version` IS NOT EVIDENCE IT CAN START.
   *
   * The probe used to stop above, so a binary that answered `--version` was
   * reported `state: ready` — and octos 0.1.1 on a fresh machine did exactly that,
   * then `hagency acp-up` died with `unrecognized subcommand 'acp'`. The console
   * had told the operator the framework was ready for a launch path the installed
   * version does not have. This manifest's own note says it was verified against
   * 2.0.2; nothing checked that.
   *
   * So the subcommand the launch path actually uses is probed too. `--help` on it
   * is cheap, needs no credentials, starts no session and reads no stdin. Any
   * non-zero exit means the launch would fail, which is the thing `ready` claims
   * will not happen.
   */
  const acpSubcommand = f.transport === 'acp' ? (f.launch?.acpArgs ?? [])[0] : null;
  if (onPath && !probeError && acpSubcommand) {
    try {
      await execFileAsync(command, [acpSubcommand, '--help'], {
        timeout: 5000,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
    } catch (e) {
      const detail = String(e?.stderr || e?.message || '').split('\n')[0].slice(0, 100);
      probeError = e?.killed
        ? `\`${command} ${acpSubcommand}\` probe timed out`
        : `installed ${command} has no working \`${acpSubcommand}\` subcommand, which is how hagency starts it: ${detail}`;
    }
  }

  /*
   * Where a login would live. Reported as a path relative to home so the response
   * never contains the operator's absolute directory layout.
   */
  const CRED = {
    claude: '.claude',
    codex: '.codex',
    'codex-acp': '.codex',
    octos: path.join('.config', 'octos'),
    hermes: '.hermes',
  };
  const credRel = CRED[f.id] ?? null;
  const credAbs = credRel ? path.join(homedir(), credRel) : null;
  const credentialPresent = credAbs ? existsSync(credAbs) : null;

  /*
   * `launchable: false` does NOT mean "cannot be started".
   *
   * Every ACP manifest carries it, and every one of their reasons says the same
   * thing: `hagency up` opens a tmux session and an ACP agent has no pane, so use
   * `hagency acp-up` instead. That is a different START COMMAND, not an inability —
   * and reporting it as a state alongside `absent` told a contributor their
   * installed, working framework was unusable. The distinction is carried by
   * `startWith`, which already resolves to the right command per transport.
   */
  let state;
  if (!onPath) state = 'absent';
  else if (probeError) state = 'unusable';
  else if (credentialPresent === false) state = 'needs_auth';
  else state = 'ready';

  return {
    id: f.id,
    displayName: f.displayName,
    transport: f.transport,
    command,
    onPath,
    version,
    probeError,
    // `~/.codex`, never `/Users/someone/.codex`.
    credentialHome: credRel ? `~/${credRel}` : null,
    // true / false / null: null means this adapter has no credential home to check,
    // which is not the same as "not present".
    credentialPresent,
    launchable: f.launchable,
    notLaunchableReason: f.notLaunchableReason,
    permissionSummary: f.launch?.permissionSummary ?? null,
    state,
    // The one command that fixes this state, or null when nothing is wrong.
    fix: state === 'absent' ? `install ${command} and put it on PATH`
      : state === 'needs_auth' ? `${command} login`
        : state === 'unusable' ? `check that \`${command} --version\` returns`
          : null,
    startWith: f.transport === 'acp' ? 'hagency acp-up' : 'hagency up',
  };
}

app.get('/api/frameworks/detect', requireBearer, async (_req, res) => {
  try {
    const frameworks = await Promise.all(listFrameworks().map(probeFramework));
    return res.json({
      scannedAt: Date.now(),
      host: hostname(),
      frameworks,
      /*
       * Said in the payload, not left to the client to remember: a present
       * credential directory is not a valid session, and nothing short of a real
       * request to the provider can tell the difference.
       */
      caveat: 'credentialPresent means the credential directory exists, not that a valid session is in it',
    });
  } catch (error) {
    console.error('[frameworks/detect] probe failed:', error?.message || error);
    return res.status(500).json({ error: 'framework detection failed' });
  }
});

// ── Role capability ────────────────────────────────────────────────────
/*
 * Which roles this deployment can actually fill, and why not when it cannot.
 *
 * `lib/role-capacity.json` shipped with no consumer at all: the role vocabulary
 * existed as a constant nothing imported, and the enumeration of which
 * (framework, model, reasoning) combinations qualify at each tier existed only as
 * a file. This is its first reader, which means role eligibility is now computed
 * from each agent's RESOLVED MODEL rather than inferred from its name.
 *
 * Two distinctions the response keeps that a headcount would lose:
 *
 *  - **Why a role cannot be filled.** "A model I have not configured" and "an
 *    agent I do not own" are different problems with different fixes, so `unable`
 *    carries a reason per agent rather than a total.
 *  - **What subsumption costs.** A stronger tier fills a weaker role, so an
 *    Opus-only deployment fills everything and pays Opus rates to write
 *    documentation. `overTier` reports that rather than the check refusing it —
 *    it is the operator's trade to make.
 *
 * Read-only, and it deliberately does not decide anything: nothing here staffs,
 * reserves or dispatches. It answers what is possible.
 */
/*
 * Guarded, unlike GET /api/frameworks.
 *
 * It was left open on the reasoning that role capability is the outward-facing layer.
 * But the payload names AGENTS — which agent backs each role, its tier, its family,
 * its online state — and ADR-013's L2 boundary is precisely that a project sees roles
 * and never the raw agents behind them. An open endpoint that publishes the private
 * half of that mapping inverts the boundary it was meant to express. A project-facing
 * projection would have to drop `able`/`unable` entirely; until one exists, this is
 * the contributor's own view and is authenticated as such.
 */
app.get('/api/capability', requireBearer, (_req, res) => {
  refreshServerLiveness();
  const rows = serializeAgents(Object.values(agents).filter((a) => agentEligibleForRoom(a)));
  const canProvision = (preset) => ['claude', 'codex'].includes(preset.framework);
  const TIER_RANK = Object.fromEntries(
    [...CAPABILITY_TIERS].reverse().map((t, i) => [t, i]),
  );

  const roles = ROLES.map((key) => {
    const def = roleCapacity.roles[key];
    const need = ROLE_DEFAULT_TIER[key];
    const able = [];
    const unable = [];

    for (const a of rows) {
      const tier = modelTier(a.runtimeProfile);
      if (!tier) {
        // Two different absences, and the fix differs: no profile at all versus a
        // profile whose model no tier accepts.
        unable.push({
          agent: a.name,
          reason: a.runtimeProfile?.primary?.model ? 'model-not-accepted' : 'no-model',
          model: a.runtimeProfile?.primary?.model ?? null,
        });
        continue;
      }
      if (TIER_RANK[tier] < TIER_RANK[need]) {
        unable.push({ agent: a.name, reason: 'below-tier', tier, need });
        continue;
      }
      able.push({
        agent: a.name,
        presetId: a.presetId ?? null,
        framework: a.runtimeProfile?.primary?.framework ?? a.type ?? null,
        model: a.runtimeProfile?.primary?.model ?? null,
        reasoning: a.runtimeProfile?.primary?.reasoning ?? null,
        tier,
        family: modelFamily(a.runtimeProfile),
        overTier: TIER_RANK[tier] - TIER_RANK[need],
        online: a.online === true,
      });
    }

    const eligibleResources = resourcesForRole(key, need, frameworkPresets).filter(({ preset }) => canProvision(preset));
    const families = [...new Set([
      ...able.map((r) => r.family),
      ...eligibleResources.map(({ preset }) => modelFamily(runtimeProfileFromPreset(preset))),
    ].filter(Boolean))].sort();
    const representedPresets = new Set(able.map((row) => row.presetId).filter(Boolean));
    const provisionable = eligibleResources.filter(({ preset }) => !representedPresets.has(preset.id)).length;
    return {
      role: key,
      displayName: def.displayName,
      defaultTier: need,
      // lib/matrix-agent.js:26 — review must be staffed from two different model
      // families, so one family cannot cover both sides however many agents it
      // has. Reported as a property of the role, not as a warning.
      crossFamily: def.crossFamily === true,
      crossFamilyOk: def.crossFamily === true ? families.length >= 2 : true,
      families,
      /*
       * FILLABLE MEANS STAFFABLE, NOT "MEETS THE TIER BAR".
       *
       * This was `able.length`, which ignores the cross-family rule right above it —
       * so a deployment with one model family reported `review` as
       * `fillable: 1, crossFamilyOk: false`, two fields of the same object
       * contradicting each other. Review needs two different families, so one agent
       * cannot staff it however strong it is, and a caller reading the headline
       * number was told it could.
       *
       * Found on a clean single-agent install; a mixed-family deployment never shows
       * it, which is why it survived. The console already gated on both
       * (mockup/lib/derive.js:170) — this makes the API agree with the surface that
       * was getting it right.
       *
       * `able` still lists every agent that clears the tier, so nothing is hidden:
       * the two fields now answer different questions instead of the same one twice.
       */
      // A configured resource may supply its first agent on demand. Do not count
      // a preset again when its existing agent already represents that capacity.
      fillable: (def.crossFamily === true && families.length < 2) ? 0 : able.length + provisionable,
      able,
      unable,
      overTier: able.filter((r) => r.overTier > 0).length,
      excluded: (roleCapacity.excluded ?? []).filter((e) => e.role === key),
    };
  });

  /*
   * WHAT COULD BE MINTED, beside what exists. `roles[].able` answers "which live agents fill this
   * role"; `resources` answers the provisioning question — which configured presets a provision
   * plan for the role could mint from, at which tier, in the exact order `resourceForRole` would
   * pick them (lowest qualifying tier first, then id). Each non-qualifying preset carries its
   * reason, matching `unable` above: a model no tier accepts, a tier below the role's floor, and a
   * missing ceiling are different problems with different fixes. Preset ids, names, tiers and
   * ceilings and model identity only — never the resolved profile, which carries the apiKey.
   */
  const resources = Object.fromEntries(ROLES.map((role) => {
    const need = ROLE_DEFAULT_TIER[role];
    const ranked = resourcesForRole(role, need, frameworkPresets).filter(({ preset }) => canProvision(preset));
    const qualifiedIds = new Set(ranked.map(({ preset }) => preset.id));
    return [role, {
      qualified: ranked.map(({ preset, tier }) => ({
        presetId: preset.id,
        name: preset.name,
        framework: preset.framework ?? null,
        model: preset.model ?? null,
        reasoning: preset.reasoning ?? null,
        family: modelFamily(runtimeProfileFromPreset(preset)),
        tier,
        overTier: TIER_RANK[tier] - TIER_RANK[need],
        ceilingTokens: preset.ceiling?.tokens ?? null,
      })),
      // The head of `qualified`, named so a client does not re-implement the pick.
      selected: ranked[0]?.preset.id ?? null,
      unqualified: frameworkPresets
        .filter((p) => !qualifiedIds.has(p.id))
        .map((p) => {
          const tier = presetTier(p);
          return {
            presetId: p.id,
            name: p.name,
            tier: tier ?? null,
            reason: !canProvision(p)
              ? 'framework-not-provisionable'
              : !tier
              ? (p.model ? 'model-not-accepted' : 'no-model')
              : (TIER_RANK[tier] < TIER_RANK[need] ? 'below-tier' : 'no-ceiling'),
          };
        }),
      // Stated so an empty `qualified` cannot read as "no presets exist" when the truth is
      // "presets exist and none qualifies" — the difference is the whole refusal.
      considered: frameworkPresets.length,
    }];
  }));

  return res.json({
    generatedAt: Date.now(),
    tiers: CAPABILITY_TIERS,
    agents: rows.length,
    // Named so a client cannot mistake a computed judgement for stored state.
    source: 'lib/role-capacity.json',
    roles,
    resources,
  });
});

// ── Engagements, offers, whitelist ─────────────────────────────────────
/*
 * What replaces dispatch. See lib/engagement-store.js for the routing rules and
 * why each branch is the way it is; this layer supplies the two facts the store
 * deliberately does not know.
 *
 *  - WHICH AGENT would serve a role. That is the capability model's answer, and it
 *    has to be computed BEFORE a decision so an approval form can name whose
 *    ceiling is about to be spent.
 *  - HOW MUCH IS LEFT on that agent. Its declared ceiling minus what active
 *    engagements have already committed against it. Per agent, because an
 *    engagement draws on one agent's ceiling — two projects wanting an architect
 *    served by the same agent share it.
 */

/** The agent that would serve a role: qualified, and with the most headroom. */
/**
 * What is disclosed about the resource serving a role — and what is not.
 *
 * OPERATOR RULING 2026-08-11, inverting ADR-013 decision 2: the coding agent and its model
 * are not hidden from the borrower, they are made TRANSPARENT. See the amendment in
 * `knowledge/decisions/adr-013-resource-contribution-console.md`.
 *
 * The original decision reasoned that hiding the `(agent × model)` mapping is what makes
 * this a resource market rather than a remote-shell directory. The opposite is more likely
 * true, and it is why this reads as a correction rather than a relaxation: a market where
 * model quality is undisclosed is a market where the borrower cannot tell Opus from a cheap
 * model, so they discount every offer to the worst case. For a CONTRIBUTION console that is
 * backwards — a contributor lending their Opus subscription becomes indistinguishable from
 * one lending nothing much, and disclosure is what makes the contribution legible.
 *
 * THE LINE THAT REPLACES THE OLD ONE. Not "everything is public" — the distinction is
 * between the CAPABILITY and the DEPLOYMENT:
 *
 *   disclosed   framework, model, reasoning level, the tier it qualifies at, the agent's
 *               name — everything a borrower needs to judge whether the work will be good
 *               enough, and to attribute it afterwards
 *
 *   private     host, workspace path, credential home, seat, API keys, tmux session, owner
 *               MXID, env var names — the provider's deployment, which tells a borrower
 *               nothing about the work and is a standing invitation to probe
 *
 * The request direction is unchanged: a borrower still asks for a ROLE and cannot pick an
 * agent (`b.agent` is a hint that must independently qualify, refused otherwise). So the
 * provider keeps allocation freedom while the borrower is told what they got. Choosing is
 * still the provider's; knowing is now the borrower's.
 */
function servingConfiguration(agentName) {
  if (!agentName) return null;
  const row = serializeAgents(Object.values(agents).filter(isAgentRecord))
    .find((a) => a.name === agentName);
  if (!row) return null;
  const primary = row.runtimeProfile?.primary ?? {};
  return {
    agent: agentName,
    framework: primary.framework ?? row.type ?? null,
    model: primary.model ?? null,
    /*
     * Included because it is load-bearing rather than decorative: the same model at a
     * different reasoning level is a different tier (`lib/role-capacity.json` lists
     * gpt-5.6-sol at both high and medium, qualifying differently), so a model name alone
     * does not tell a borrower what they are getting.
     */
    reasoning: primary.reasoning ?? null,
    tier: modelTier(row.runtimeProfile) ?? null,
  };
}

function agentEligibleForRoom(agent, projectRoomId = null, fulfillmentId = null) {
  if (!isAgentRecord(agent) || agent.retiredAt || String(agent.offlineReason ?? '').startsWith('retired:')) return false;
  // A stopped or unconfirmed runner cannot accept a new engagement. An idle
  // provisioned home remains eligible; current online telemetry is not a fence.
  if (agent.manualDown || agent.stopUnconfirmedDispatches?.length) return false;
  if (agent.engagementProvisioningId && agent.engagementProvisioningId !== fulfillmentId
    && engagementStore.get(agent.engagementProvisioningId)?.fulfillment?.phase !== 'complete') return false;
  if (engagementStore.list({ state: 'pending' }).some((e) => e.fulfillment && e.agent === agent.name
    && e.id !== fulfillmentId && e.fulfillment.phase !== 'failed')) return false;
  const requestedSide = projectRoomId ? sideIdForRoom(projectRoomId) : null;
  const side = requestedSide ? projectSideStore.getSide(requestedSide) : null;
  if (side && !side.active) return false;
  if (agent.projectSide) {
    const ownSide = projectSideStore.getSide(agent.projectSide);
    if (!ownSide?.active) return false;
    if (requestedSide && agent.projectSide !== requestedSide) return false;
  } else if (side) return false;
  return true;
}

function agentForRole(role, projectRoomId = null) {
  const rows = serializeAgents(Object.values(agents).filter((a) => agentEligibleForRoom(a, projectRoomId)));
  const TIER_RANK = Object.fromEntries([...CAPABILITY_TIERS].reverse().map((t, i) => [t, i]));
  const need = ROLE_DEFAULT_TIER[role];
  if (!need) return null;
  const qualified = rows
    .map((a) => ({ a, tier: modelTier(a.runtimeProfile) }))
    .filter(({ a, tier }) => tier && TIER_RANK[tier] >= TIER_RANK[need]
      && (!agents[a.name]?.projectAgentDefinition || agents[a.name].projectAgentDefinition.projectRoomId === projectRoomId)
      && (!agents[a.name]?.resourceDefinitionId || resourceAgentDefinitions.find(agents[a.name].resourceDefinitionId)?.definition.role === role
        && resourceAgentDefinitions.find(agents[a.name].resourceDefinitionId)?.definition.enabled === true));
  if (!qualified.length) return null;
  /*
   * Most headroom first, so a second request for the same role does not pile onto
   * the agent that is already nearly committed. Deliberately NOT round-robin: the
   * point is to keep as many roles fillable as possible, and an agent with nothing
   * left cannot fill anything however fairly it was chosen.
   */
  qualified.sort((x, y) => (remainingFor(y.a.name) ?? -1) - (remainingFor(x.a.name) ?? -1));
  return qualified[0].a.name;
}

function allocationChoice(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new EngagementError('bad_request', 'Choose an Agent definition or existing Agent');
  if (value.kind === 'definition' && typeof value.definitionId === 'string') return { kind: 'definition', definitionId: value.definitionId };
  if (value.kind === 'agent' && typeof value.agent === 'string') return { kind: 'agent', agent: value.agent };
  if (value.kind === 'project-definition') return { kind: 'project-definition' };
  throw new EngagementError('bad_request', 'Invalid Agent allocation choice');
}

function allocationCandidate(e, input) {
  const choice = allocationChoice(input);
  if (e.requestContext?.agentDefinition) {
    if (choice.kind !== 'project-definition') throw new EngagementError('conflict', 'Approve the Agent and resource defined by the project, or reject this request');
    const definition = e.requestContext.agentDefinition;
    const preset = frameworkPresets.find(p => publicResourceId(p) === definition.resourceId);
    if (!preset || (!e.fulfillment && !fleetCatalogOffers(sideIdForRoom(e.projectRoomId))
      .some(offer => offer.published && offer.role === e.role && offer.resources.some(resource => resource.id === definition.resourceId)))
      || !['claude', 'codex'].includes(preset.framework)
      || !resourcesForRole(e.role, ROLE_DEFAULT_TIER[e.role], [preset]).length) {
      throw new EngagementError('agent_unavailable', 'The requested resource is no longer published or does not qualify for this role');
    }
    if (e.fulfillment && preset.id !== e.fulfillment.presetId) throw new EngagementError('conflict', 'The reserved resource cannot change');
    const name = projectAgentRuntimeName(e.requestContext), agent = agents[name];
    if (agent && (agent.projectAgentRequestId !== e.requestId || !agentEligibleForRoom(agent, e.projectRoomId, e.id))) {
      throw new EngagementError('conflict', 'The requested Agent identity is unavailable');
    }
    const budget = resourceBudget(preset, { excludeEngagementId: e.id });
    return { choice, name, displayName: definition.name, preset, provision: !agent, budget,
      remainingTokens: agent ? remainingFor(name, { excludeEngagementId: e.id }) : budget.remainingTokens };
  }
  if (choice.kind === 'project-definition') throw new EngagementError('bad_request', 'This request has no project Agent definition');
  const found = choice.kind === 'definition' ? resourceAgentDefinitions.find(choice.definitionId) : null;
  if (choice.kind === 'definition' && (!found || !found.definition.enabled || found.definition.role !== e.role)) {
    throw new EngagementError('agent_unavailable', 'Agent definition is unavailable for this role');
  }
  const name = found?.definition.name || choice.agent;
  const agent = agents[name];
  const preset = found?.preset || frameworkPresets.find(p => p.id === agent?.presetId);
  if (!preset || !resourcesForRole(e.role, ROLE_DEFAULT_TIER[e.role], [preset]).length) {
    throw new EngagementError('agent_unavailable', 'Selected resource does not qualify for this role');
  }
  if (found && agent && agent.resourceDefinitionId !== found.definition.id) throw new EngagementError('conflict', 'Agent name belongs to another identity');
  if (agent) {
    if (agent.projectAgentDefinition && agent.projectAgentDefinition.projectRoomId !== e.projectRoomId) {
      throw new EngagementError('agent_unavailable', 'This Agent belongs to a different project');
    }
    if (agent.resourceDefinitionId) {
      const definition = resourceAgentDefinitions.find(agent.resourceDefinitionId)?.definition;
      if (!definition?.enabled || definition.role !== e.role) throw new EngagementError('agent_unavailable', 'Agent definition is unavailable for this role');
    }
    if (!agentEligibleForRoom(agent, e.projectRoomId, e.id)
      || !modelTier(agent.runtimeProfile)
      || !resourcesForRole(e.role, ROLE_DEFAULT_TIER[e.role], [{ ...preset, ...agent.runtimeProfile?.primary }]).length) {
      throw new EngagementError('agent_unavailable', 'Selected Agent is unavailable on this project side or does not qualify');
    }
  } else {
    if (!found || !['claude', 'codex'].includes(preset.framework)) throw new EngagementError('agent_unavailable', 'Selected Agent does not exist');
    if (engagementStore.list().some(other => other.id !== e.id && other.agent === name && other.state === 'pending'
      && other.fulfillment && other.fulfillment.phase !== 'failed')) throw new EngagementError('conflict', 'Agent definition is already reserved by another request');
  }
  return { choice, name, preset, definitionId: found?.definition.id || null, provision: !agent,
    remainingTokens: agent ? remainingFor(name, { excludeEngagementId: e.id }) : resourceRemaining(preset, { excludeEngagementId: e.id }) };
}

function engagementCandidates(e) {
  const choices = e.requestContext?.agentDefinition ? [{ kind: 'project-definition' }] : resourceAgentDefinitions.all().filter(row => row.definition.role === e.role)
    .map(row => ({ kind: 'definition', definitionId: row.definition.id }));
  for (const agent of Object.values(agents).filter(a => isAgentRecord(a) && !e.requestContext?.agentDefinition)) {
    if (!agent.resourceDefinitionId) choices.push({ kind: 'agent', agent: agent.name });
  }
  return choices.flatMap(choice => {
    try {
      const c = allocationCandidate(e, choice);
      return [{ choice, name: c.displayName || c.name, resource: c.preset.name, framework: c.preset.framework,
        model: c.preset.model, reasoning: c.preset.reasoning, provision: c.provision, remainingTokens: c.remainingTokens,
        ...(c.budget ? { budget: c.budget } : {}) }];
    } catch (error) { if (error instanceof EngagementError) return []; throw error; }
  });
}

function publicResourceConfigurations(role, _sideId) {
  return resourcesForRole(role, ROLE_DEFAULT_TIER[role], frameworkPresets)
    .filter(({ preset }) => preset.catalogPublished === true && ['claude', 'codex'].includes(preset.framework))
    .map(({ preset, tier }) => ({ id: publicResourceId(preset), name: preset.name, framework: preset.framework,
      model: preset.model, reasoning: preset.reasoning || null, tier }));
}

// Catalog visibility is derived, not an automatic-acceptance offer. Keep the
// engagement store's explicit offers unchanged: publishing capacity cannot
// grant budget or bypass the provider's review of a project-defined Agent.
function fleetCatalogOffers(sideId = null) {
  return ROLES.flatMap(role => {
    const offer = engagementStore.getOffer(role);
    const resources = offer?.published === false || !roleCrossFamilyAvailable(role, null, sideId)
      ? [] : publicResourceConfigurations(role, sideId);
    if (!offer && !resources.length) return [];
    return [{ role, count: offer?.count ?? null, budgetCapPerEngagement: offer?.budgetCapPerEngagement ?? null,
      rateCap: offer?.rateCap ?? null, published: offer?.published === true || resources.length > 0, resources }];
  });
}

function roleCrossFamilyAvailable(role, projectRoomId = null, projectSideId = null) {
  if (!roleCapacity.roles[role]?.crossFamily) return true;
  const rank = Object.fromEntries([...CAPABILITY_TIERS].reverse().map((tier, i) => [tier, i]));
  const need = rank[ROLE_DEFAULT_TIER[role]];
  const families = Object.values(agents).filter((a) => agentEligibleForRoom(a, projectRoomId))
    .filter((a) => !projectSideId || a.projectSide === projectSideId)
    .filter((a) => rank[modelTier(a.runtimeProfile)] >= need)
    .map((a) => modelFamily(a.runtimeProfile)).filter(Boolean);
  return new Set(families).size >= 2;
}

/**
 * What is left on the SEAT this agent occupies, or null if no quota is declared.
 *
 * The seat is the accounting root: agents sharing a credential home share one quota
 * however much each of them declares. Without this, the per-agent ceiling was the
 * only constraint and it is the wrong one — verified against a running backend, two
 * agents with 5M ceilings on a seat declared at 6M both took a 4M allocation, for
 * 8M committed against 6M. The seat page reported `overSubscribed: true` afterwards
 * and nothing had stopped it.
 */
function seatRemainingFor(agentName, excludeEngagementId = null) {
  const rows = Object.values(agents).filter(isAgentRecord);
  const target = rows.find((a) => a.name === agentName);
  if (!target) return null;
  const seatId = seatIdentity(target, { keyId: SEAT_KEY_ID, secret: SEAT_KEY_SECRET }).seatId;
  const declaration = seatDeclarations[seatId];
  const quota = declaration?.quotaTokens;
  const preset = frameworkPresets.find((p) => p.id === target.presetId);
  if (!declaration?.period || declaration.period !== preset?.ceiling?.period) return null;
  // Undeclared quota is UNKNOWN, not unlimited. Returning null lets the caller
  // decide; returning Infinity here would silently reinstate the bug.
  if (!Number.isFinite(quota) || quota < 0) return null;

  const sameSeat = rows.filter((a) => (
    seatIdentity(a, { keyId: SEAT_KEY_ID, secret: SEAT_KEY_SECRET }).seatId === seatId
  ));
  const own = excludeEngagementId ? engagementStore.get(excludeEngagementId) : null;
  const committed = sameSeat.reduce((n, a) => n + engagementStore.committedFor(a.name), 0)
    + unprovisionedSeatCommitments(seatId)
    - (own && sameSeat.some((a) => a.name === own.agent) && own.fulfillment?.phase !== 'failed' ? own.allocatedTokens || 0 : 0);
  return Math.max(0, quota - committed);
}

/**
 * How much may still be allocated to this agent: the TIGHTER of its own declared
 * ceiling and what is left on its seat.
 *
 * Both can be null, and null means "not declared" rather than "no limit". A null on
 * either side must not relax the other — so the result is the minimum of whichever
 * limits exist, and null only when neither does.
 */
/*
 * WHAT IS LEFT OF AN AGENT'S CEILING, counting what was SPENT as well as what was promised.
 *
 * This used to subtract only allocations. That was the whole truth while nothing measured
 * consumption: a ceiling could be over-promised but never over-spent, because spending was
 * invisible. Now that lib/metering measures it, the two can disagree in both directions —
 * an engagement allocated 400k and consumed 900k has exceeded the ceiling while its
 * allocation says otherwise, and one allocated 400k that consumed 50k is holding 350k it
 * has not used.
 *
 * `max(reserved, spent)`, not the sum. Consumption largely happens INSIDE an allocation, so
 * adding them double-counts the same tokens; taking the larger is the conservative reading
 * that never under-reports what the ceiling has to cover.
 *
 * SPENT IS PERIOD-SCOPED OR IT IS NOT USED. A ceiling is `{tokens, period}`, and comparing
 * an all-time total to a monthly budget exhausts it permanently. If the ledger has no
 * bucket for the current period, spend is treated as UNKNOWN rather than zero and the
 * allocation figure stands alone — an absent bucket means nobody has measured this period,
 * which is not the same as measuring none.
 */
function ceilingSpendFor(agentName) {
  const agent = Object.values(agents).filter(isAgentRecord).find((a) => a.name === agentName);
  const preset = agent?.presetId ? frameworkPresets.find((p) => p.id === agent.presetId) : null;
  const period = preset?.ceiling?.period === 'daily' ? 'daily' : 'monthly';
  const bucket = usageLedger.currentPeriod(agentName, period);
  return {
    period,
    reserved: engagementStore.committedFor(agentName),
    /*
     * `drawn`, NOT `total`. A ceiling draws against fresh tokens; cache reads are measured
     * and reported but never spend it (CEILING_KINDS, operator ruling 2026-08-12). This
     * read was `bucket.total`, so the first agent to be metered successfully was locked out
     * at 13.6M against a 10M ceiling — of which 12.9M was cache reads and 681k was work.
     */
    spent: bucket ? bucket.drawn : null,
    // Kept so callers can SHOW consumption without enforcing on it. The two figures must
    // never be swapped: one is what the contributor lent, the other what was read.
    consumed: bucket ? bucket.total : null,
    ceilingTokens: Number.isFinite(preset?.ceiling?.tokens) ? preset.ceiling.tokens : null,
    presetName: preset?.name ?? null,
    spendPeriodKey: bucket?.key ?? null,
  };
}

function remainingFor(agentName, { forAutoJoin = false, excludeEngagementId = null } = {}) {
  const agent = Object.values(agents).filter(isAgentRecord).find((a) => a.name === agentName);
  if (!agent?.presetId) return null;
  const preset = frameworkPresets.find((p) => p.id === agent.presetId);
  if (forAutoJoin) {
    const seatId = seatIdentity(agent, { keyId: SEAT_KEY_ID, secret: SEAT_KEY_SECRET }).seatId;
    const declaration = seatDeclarations[seatId];
    if (declaration && (!Number.isFinite(declaration.quotaTokens)
      || !declaration.period || declaration.period !== preset?.ceiling?.period)) return null;
  }
  const ceiling = preset?.ceiling?.tokens;
  const { reserved: allReserved, spent } = ceilingSpendFor(agentName);
  const own = excludeEngagementId ? engagementStore.get(excludeEngagementId) : null;
  const reserved = allReserved - (own?.agent === agentName && own.fulfillment?.phase !== 'failed' ? own.allocatedTokens || 0 : 0);
  // An unknown spend cannot lower the figure; it also must not be read as zero, which is
  // why it falls back to the allocation rather than to `ceiling - 0`.
  const drawn = spent === null ? reserved : Math.max(reserved, spent);
  const byCeiling = Number.isFinite(ceiling) ? Math.max(0, ceiling - drawn) : null;
  const bySeat = seatRemainingFor(agentName, excludeEngagementId);
  const poolBudget = preset ? resourceBudget(preset, { forAutoJoin, excludeEngagementId }) : null;
  if (poolBudget?.seat.status === 'period_mismatch') return null;
  const byPool = poolBudget?.remainingTokens ?? null;
  const limits = [byCeiling, bySeat, byPool].filter((v) => v !== null);
  return limits.length ? Math.min(...limits) : null;
}

/*
 * A REQUESTER IS NOT THE CONTRIBUTOR.
 *
 * `POST /api/engagements` is the one project-facing write on this surface: a project
 * asks to draw on someone's capacity. Every other engagement route — the verdict,
 * the revoke, the whitelist, the offer — is the contributor deciding. They all sat
 * behind the same `requireBearer`, so a project handed the credential it needs to
 * ASK could also approve its own request, whitelist itself for future auto-join, and
 * widen the offer it was measured against.
 *
 * This is the narrow, correct half of the scoped-token problem: the console still
 * has no read-only tier and one shared API_TOKEN still opens the rest of `/api`.
 * What is fixed here is the specific escalation — submitting no longer implies
 * deciding.
 *
 * `HAGENCY_REQUESTER_TOKEN` is a separate secret that authorises submission ONLY.
 * The operator token continues to work, because the console and the local operator
 * legitimately submit too (the seed script and the preview both do).
 */
const REQUESTER_TOKEN = String(process.env.HAGENCY_REQUESTER_TOKEN || '').trim();

function requireRequester(req, res, next) {
  const bridgeSecret = getBridgeSecret();
  if (bridgeSecret && req.headers['x-bridge-secret'] === bridgeSecret) { req.engagementCaller = 'matrix'; return next(); }
  const auth = req.headers.authorization || '';
  if (REQUESTER_TOKEN && auth === `Bearer ${REQUESTER_TOKEN}`) { req.engagementCaller = 'requester'; return next(); }
  // The operator may still submit; a requester may do nothing else.
  return requireBearer(req, res, () => { req.engagementCaller = 'operator'; next(); });
}

/*
 * MAKE THE APPROVAL DO SOMETHING.
 *
 * An engagement going active used to change nothing about the world: it allocated a
 * number, wrote an audit line, and left the agent unattached to the project. Six
 * active engagements existed against zero bindings. `upsertBinding()` at
 * lib/approval-store.js:186 is the record that actually attaches an agent to a
 * project room with an owner, and nothing was calling it from here.
 *
 * WHERE THE OWNER COMES FROM, in order:
 *
 *  1. An existing binding for this agent. If the agent is already bound to some
 *     project, that binding names the human who owns it and the DM room the
 *     approval flow uses — reusing it keeps one owner per agent, which is what the
 *     approval machinery assumes.
 *  2. `HAGENCY_OWNER_MXID` + `HAGENCY_OWNER_DM_ROOM`, for the first binding on a
 *     fresh deployment.
 *
 * There is deliberately no third fallback. `POST /api/dm/ensure` only broadcasts a
 * request to the bridge and returns no room id, so the backend cannot invent one,
 * and `upsertBinding` requires both fields. When neither source has an owner the
 * bind FAILS AND SAYS SO — recorded on the engagement, returned to the caller, and
 * shown in the console. Silently approving without binding is the exact defect this
 * replaces; a silent partial success would be the same defect wearing a hat.
 */
const OWNER_MXID = String(process.env.HAGENCY_OWNER_MXID || '').trim();
const OWNER_DM_ROOM = String(process.env.HAGENCY_OWNER_DM_ROOM || '').trim();

function resolveOwnerFor(agentName, projectRoomId = null) {
  try {
    /*
     * F04: the binding is scoped by (agent, project room). The previous code
     * read listBindings({agent})[0] — insertion order, i.e. the FIRST project's
     * owner — and wrote that into whichever engagement accepted next, silently
     * re-pointing a second project's approvals at the first project's owner.
     * Select the binding for THIS room when one exists; only when none does may
     * the accepted-source flow (below) establish one. Never borrow another
     * project's record.
     */
    const bindings = approvalStore.listBindings(
      projectRoomId ? { agent: agentName, projectRoomId } : { agent: agentName },
    );
    const roomBindings = projectRoomId ? approvalStore.listBindings({ projectRoomId }) : [];
    const roomOwners = new Set(roomBindings.map((b) => JSON.stringify([b.ownerMxid, b.ownerDmRoomId])));
    const target = (agentName ? bindings[0] : null) || (roomOwners.size === 1 ? roomBindings[0] : null);
    if (target?.ownerMxid && target?.ownerDmRoomId) {
      return { ownerMxid: target.ownerMxid, ownerDmRoomId: target.ownerDmRoomId, from: 'existing binding' };
    }
  } catch { return null; }
  // ADR-016 makes the configured provider owner a bootstrap fallback only.
  // A configured project side needs its own borrower binding or an explicit verdict owner.
  if (!projectSideStore.getSide(sideIdForRoom(projectRoomId)) && OWNER_MXID && OWNER_DM_ROOM) {
    return { ownerMxid: OWNER_MXID, ownerDmRoomId: OWNER_DM_ROOM, from: 'HAGENCY_OWNER_MXID' };
  }
  return null;
}

/** Attach the agent to the project. Returns the outcome; never throws at the caller. */
/**
 * Put the agent in the project's room on the project side — ADR-016 decision 3's remaining half.
 *
 * WHY THE BACKEND AND NOT THE BRIDGE. The bridge's join site is `getAgentToken`, and an appservice
 * side mints no per-agent token: the namespace makes an agent addressable, not able to act. The
 * credential that CAN do it is the side's, and the backend is where it lives — it is the process that
 * serves `/api/project-sides/acting-credentials` to the bridge in the first place.
 *
 * WHY HERE AND NOT INSIDE `bindEngagement`. That function's job is one local store write and it is
 * called synchronously; a Matrix round-trip inside it would make a remote homeserver's latency part
 * of recording a decision that has already been made. So this runs beside it and its outcome rides
 * back on the response the same way `binding` does — for the same stated reason: an approval that
 * allocated budget but could not put the agent in the room is a half-done thing, and the form that
 * pressed Approve is the only place anyone will look.
 *
 * NOT AN ERROR WHEN THERE IS NO SIDE. An engagement whose room is on the contributor's own server
 * needs no admission — the agent is already local to it. `reason: 'no_project_side'` is that case
 * reported, not a failure.
 *
 * THE MXID IS COMPOSED, and this is the one place composition is right: the localpart is the rule WE
 * mint by (`MATRIX_AGENT_PREFIX_FOR_REGISTRATION`), on a server the room id names, and
 * `joinRoomOnSideAsAgent` re-checks it against the namespace the registration claimed before any
 * token is presented. Composing a HUMAN's mxid is the bug #73 fixed; composing an identity we
 * ourselves minted by a fixed rule, and then verifying it, is not the same act.
 */
/**
 * Re-admit agents whose room membership was lost while they had nothing to say.
 *
 * ADR-016 row 3's last unbuilt clause, and the shape of the gap is the point: a send that fails on
 * membership re-invites and rejoins, so an agent that WORKS heals itself. An idle one does not — its
 * membership can be dropped by a kick, a room upgrade or a server-side cleanup, and the next thing that
 * notices is the next message, which may be days away and will be the one that fails.
 *
 * SWEPT RATHER THAN WATCHED, because nothing tells us. Membership on somebody else's homeserver changes
 * without asking us, and the appservice intake only sees rooms it is in — the very membership in question.
 *
 * IDEMPOTENT BY REUSE, not by a new code path: `admitAgentToProjectRoom` already answers `alreadyMember`
 * for an agent that is in the room, and a second invite of a member is the 403 that function was written
 * to read correctly. So the sweep asks the same question the acceptance path asks, and the common case —
 * everyone still where they were — costs one invite attempt per active engagement and changes nothing.
 * A separate "check membership first" call would be a second way to be wrong about the same fact.
 */
async function sweepProjectRoomMembership() {
  const seen = new Set();
  for (const engagement of engagementStore.list({ state: 'active' })) {
    const roomId = engagement?.projectRoomId;
    const agentName = engagement?.agent;
    if (!roomId || !agentName) continue;
    /*
     * One pass per (agent, room), not per engagement. Six concurrent engagements between one agent and
     * one room is a real shape in this deployment's data — the unbind logic exists because of it — and
     * six identical invites would be five needless calls to a customer's homeserver.
     */
    const key = `${agentName}\u0000${roomId}`;
    if (seen.has(key)) continue;
    seen.add(key);
    /*
     * Skipped here as well as inside `admitAgentToProjectRoom`, which answers `no_project_side` for the
     * same case. That makes this line an early exit rather than a guard, and it stays for one reason worth
     * stating: a contributor's own rooms are the COMMON case in this deployment's data, and paying a
     * function call plus a store lookup per engagement per hour to re-learn "not a project side" is work
     * with no possible outcome. Removing it changes nothing observable — an equivalent mutant, recorded
     * as one rather than defended with a test that would only assert the call count.
     */
    if (!sideIdForRoom(roomId)) continue;

    try {
      const outcome = await admitAgentToProjectRoom(engagement);
      /*
       * Only a RE-admission is worth a line. `alreadyMember` is the expected answer and logging it would
       * bury the one case an operator wants: an agent that had silently fallen out of a customer's room.
       */
      if (outcome?.invited && outcome?.joined) {
        console.log(`[readmit] ${agentName} was not in ${roomId} and has been let back in`);
      }
    } catch (error) {
      console.warn(`[readmit] ${agentName} in ${roomId}: ${error?.message || error}`);
    }
  }
}

/**
 * Take a departing agent OUT of the project rooms it joined, on the customer's homeserver.
 *
 * READ OFF A LIVE HOMESERVER, and this is the reason it exists: four `@ac_e2e-probe-*` accounts still
 * joined to a project room, every one an agent Hagency had deleted hours earlier. Deleting an agent
 * removed its record, its scopes, its commitments and its group memberships — and left its identity
 * sitting in somebody else's house. In the 施工队 model every Matrix server belongs to a customer, so what
 * the customer sees is a member list of contractors who left, inside a namespace that still admits them.
 *
 * BEST EFFORT, AND THE DELETE PROCEEDS REGARDLESS. This is the one cleanup in the delete path that
 * depends on a machine we do not run. A customer's homeserver being down must not make an agent
 * unremovable from Hagency — that would hand the operator a fleet they cannot manage because someone
 * else's server is unreachable. So every room's outcome is REPORTED and none of them fails the delete:
 * the caller learns exactly which seats are still occupied, which is the honest version of a cleanup that
 * cannot be guaranteed.
 *
 * ROOMS COME FROM ENGAGEMENTS **AND** BINDINGS, and the first version used bindings alone — which made it
 * ineffective on the fleet it was written for. Proved by running the whole sequence after deploying it:
 * `@ac_e2e-probe-1787089246` was still in the room afterwards, because an agent reaches a project room
 * through `admitAgentToProjectRoom(engagement)` and this deployment had NO bindings at all (its engagements
 * were approved without a resolvable owner). The comment even claimed `sweepProjectRoomMembership` reads
 * bindings; it iterates active engagements. A source misread while writing the thing that depended on it.
 *
 * EVERY STATE OF ENGAGEMENT, not just active. An ended engagement does not remove the agent from the room,
 * so the room it named is still a seat to give back — and this runs BEFORE the release loop below, so the
 * active ones are still there to be read either way.
 *
 * NOT FROM THE HOMESERVER, which would be the authoritative list: asking which rooms a user is in needs
 * that user's own view, and an appservice side mints no per-agent token. So this is our record of where we
 * put it, and anything we did not record is out of reach — which is why the outcomes are reported.
 */
async function withdrawAgentFromProjectRooms(agentName) {
  const outcomes = [];
  const rooms = new Set();
  try {
    for (const binding of approvalStore.listBindings({ agent: agentName, includeInactive: true })) {
      if (binding?.projectRoomId) rooms.add(binding.projectRoomId);
    }
  } catch (error) {
    outcomes.push({ roomId: null, left: false, reason: `bindings unreadable: ${error?.message ?? error}` });
  }
  try {
    for (const engagement of engagementStore.list({})) {
      if (normalizeAgentName(engagement?.agent) !== agentName) continue;
      if (engagement?.projectRoomId) rooms.add(engagement.projectRoomId);
    }
  } catch (error) {
    outcomes.push({ roomId: null, left: false, reason: `engagements unreadable: ${error?.message ?? error}` });
  }
  for (const roomId of rooms) {
    outcomes.push(await withdrawAgentFromProjectRoom(agentName, roomId));
  }
  return outcomes;
}

/**
 * Give back ONE seat: take the agent out of one project room.
 *
 * EXTRACTED SO THE END OF AN ENGAGEMENT CAN USE IT TOO, which is the defect this addresses. Deleting an
 * agent has withdrawn it from every room since #114/#115 on the stated grounds that an agent which no
 * longer exists must not remain in a customer's room. An engagement that no longer exists is the same
 * fact about the same seat, and nothing was doing it — so every finished engagement left its agent sitting
 * in the borrower's room permanently.
 *
 * Confirmed against the homeserver rather than inferred: after two runs of the full-loop suite — whose own
 * teardown reported "the run leaves no room behind in any account" — `@ac_soaker:palpo2.test` was still
 * joined to both abandoned rooms. That check could not see it, because it knows only its own account and
 * the bot's.
 *
 * ONE OUTCOME OBJECT, NEVER A THROW, for the reason the multi-room caller's comment gives: a customer's
 * homeserver being unreachable must not make an engagement un-revokable. The caller reports what happened
 * to the seat instead.
 */
/*
 * F10 (17-r3): the backend's authoritative roster, in the one shape the masquerade exit asks
 * for — an MXID predicate. An agent is registered iff its name is a live key of `agents`
 * (the registry this process owns and persists), and the MXID must compose to the same
 * localpart on the room's side — the composition every project-side helper already uses.
 * Anything else is a ghost: in-namespace syntactically, but nobody this fleet vouches for.
 */
/*
 * F10 (17-r4) FINAL RULING — `admits(mxid, sideId)` iff:
 *   ① the agent record exists;
 *   ② `agent.projectSide === sideId` — the target side IS the agent's authoritative home
 *      (a MISSING projectSide fails, it does not fall through);
 *   ③ `mxid` equals the agent's AUTHORITATIVE MXID verbatim (Matrix case rules: localpart
 *      case-SENSITIVE, server case-insensitive). The authoritative MXID is the RECORDED
 *      credential MXID (`agent.matrixIdentity` — the discovered-on-another-server identity,
 *      which `mintIdentityForProvisionedAgent` writes when federation lets an agent reuse
 *      its home-server identity) when one exists, else the composition
 *      `@<prefix><agentName>:<side.serverName>`.
 *
 * WHY THE OLD SHAPE WAS REJECTED: it checked prefix + same-name-exists + server match and never
 * read `agent.projectSide`, so an agent provisioned on side A could be re-composed as
 * `@ac_<name>:<sideB>` and act under side B's as_token — the cross-side impersonation the
 * masquerade exit exists to prevent. Prefix-and-server is "the string looks plausible"; this
 * ruling is "this fleet vouches that THIS identity belongs on THIS side."
 *
 * Case handling per Matrix: localparts are case-sensitive (so `@ac_Big:side` is NOT the agent
 * `@ac_big:side`), server names compare case-insensitively. The authoritative MXID's server is
 * compared the same way, so a recorded `...@Palpo.Test` still matches the side's `palpo.test`.
 */
function recordedAgentMxid(agent) {
  const value = typeof agent?.matrixIdentity === 'string' ? agent.matrixIdentity : agent?.matrixIdentity?.mxid;
  return typeof value === 'string' && /^@[^:\s]+:[^\s]+$/.test(value.trim()) ? value.trim() : null;
}

function backendRosterAdmits(mxid, sideId, agentsMap = agents, sideStore = projectSideStore) {
  const id = typeof mxid === 'string' ? mxid.trim() : '';
  const m = id.match(/^@([^:\s@]+):([^\s@]+)$/);
  if (!m) return false;
  const [, localpart, server] = m;
  const side = sideStore.getSide(sideId);
  if (!side?.serverName) return false;
  let prefix;
  try { prefix = projectSideAgentPrefix({ side, credential: sideStore.credentialFor?.(sideId) }, MATRIX_AGENT_PREFIX_FOR_REGISTRATION); }
  catch { return false; }
  if (!localpart.startsWith(prefix)) return false;
  const agentName = localpart.slice(prefix.length);
  const recordedMatches = Object.values(agentsMap).filter((a) => recordedAgentMxid(a) === id);
  if (recordedMatches.length > 1) return false;
  const agent = recordedMatches[0] || agentsMap[agentName];
  if (!isAgentRecord(agent)) return false;                                  // ①
  if (agent.retiredAt) return false;
  if (String(agent.projectSide ?? '') !== String(sideId ?? '')) return false; // ② missing ≠ admitted
  const recorded = recordedAgentMxid(agent);
  const authoritative = recorded
    ?? `@${prefix}${agentName}:${side.serverName}`.toLowerCase();
  const am = authoritative.match(/^@([^:\s@]+):([^\s@]+)$/);
  if (!am) return false;
  return localpart === am[1] && server.toLowerCase() === am[2].toLowerCase(); // ③
}

// F10 (17-r3)/(17-r4): the roster predicate, exported so tests drive it without spinning the
// server. `sideStore` may be overridden with a { getSide } stub so the ruling's side lookup is
// tested against a controlled store rather than the live one.
export function __backendRosterAdmitsForTest(mxid, sideId, agentsMap, sideStore) {
  return backendRosterAdmits(mxid, sideId, agentsMap, sideStore);
}

async function withdrawAgentFromProjectRoom(agentName, roomId) {
  const sideId = sideIdForRoom(roomId);
  const side = sideId ? projectSideStore.getSide(sideId) : null;
  if (!side) return { roomId, left: false, reason: 'no project side owns that room' };
  let credential = null;
  try {
    credential = projectSideStore.credentialFor(sideId);
  } catch (error) {
    return { roomId, left: false, reason: `credential unreadable: ${error?.message ?? error}` };
  }
  if (!credential) return { roomId, left: false, reason: 'the side has no credential to act with' };
  /*
   * THE SAME COMPOSITION RULE AS `admitAgentToProjectRoom`, deliberately identical: the localpart is the
   * one WE mint by, on the server the room id names, and `leaveRoomOnSideAsAgent` re-checks it against
   * the namespace the registration claimed before presenting any token.
   */
  const agentMxid = recordedAgentMxid(agents[agentName]) || `@${agentPrefixOnSide(side, credential)}${agentName}:${side.serverName}`.toLowerCase();
  /*
   * F10 (17-r5): THE SAME FIRST GATE as admission — before any external request. A leave for an
   * identity the fleet does not vouch for is an external side effect (the homeserver records the
   * attempt and any audit trail sees the name), so it is refused here, at zero requests; the
   * join/leave exit's own roster check stays as depth.
   */
  if (!backendRosterAdmits(agentMxid, sideId)) {
    console.warn(`[room-withdraw] REFUSED: ${agentMxid} is not a registered agent of this fleet on side ${sideId}`);
    return { roomId, left: false, reason: 'not_a_registered_agent', sideId, mxid: agentMxid };
  }
  if (credential.kind === 'registrationToken') {
    if (matrixWorkStore.loggedOut(agentName, sideId)) return { roomId, mxid: agentMxid, left: true, reason: 'already_withdrawn_and_logged_out' };
    try {
      const result = await runMatrixWork({ action: 'leave', agent: agentName, sideId, roomId, mxid: agentMxid });
      return { roomId, mxid: agentMxid, left: result.ok, reason: result.code };
    } catch { return { roomId, mxid: agentMxid, left: false, reason: 'bridge_work_pending' }; }
  }
  const result = await leaveRoomOnSideAsAgent({
    side: { apiBaseUrl: side.apiBaseUrl, serverName: side.serverName },
    credential,
    roomId,
    agentUserId: agentMxid,
    /*
     * F10 (17-r3): the backend's OWN registry is the roster — the leave masquerades as an agent
     * this fleet registered, so the single exit must hear it from the authority that mints
     * agent identities. Composed from the requested localpart on the room's server, so a
     * withdrawn agent already deleted from the registry is refused before any request.
     */
    /*
     * F10 (17-r4): sideId, not serverName — the ruling reads agent.projectSide, which is
     * keyed by side id. The composed agentMxid below already carries the side's server; if
     * this agent's authoritative home is a DIFFERENT side, ② refuses here.
     */
    isRegisteredAgent: (mxid) => backendRosterAdmits(mxid, sideId),
  });
  return { roomId, mxid: agentMxid, left: Boolean(result.left), ...(result.state ? { state: result.state } : {}), reason: result.reason ?? null };
}

async function admitAgentToProjectRoom(engagement) {
  const roomId = engagement?.projectRoomId;
  const agentName = engagement?.agent;
  if (!roomId || !agentName) return { admitted: false, reason: 'engagement names no room or agent' };

  const sideId = sideIdForRoom(roomId);
  if (!sideId) return { admitted: false, reason: 'no_project_side' };
  const side = projectSideStore.getSide(sideId);
  if (!side) return { admitted: false, reason: 'no_project_side', sideId };

  let credential;
  try {
    credential = projectSideStore.credentialFor(sideId);
  } catch (error) {
    return { admitted: false, reason: 'credential_unreadable', sideId, detail: error?.message ?? String(error) };
  }
  if (!credential) return { admitted: false, reason: 'no_credential', sideId };

  const acting = { side, credential };
  const agentMxid = recordedAgentMxid(agents[agentName]) || `@${agentPrefixOnSide(side, credential)}${agentName}:${side.serverName}`.toLowerCase();

  /*
   * F10 (17-r5): THE ROSTER GATE COMES FIRST — before the invite, before any external request at
   * all. The invite itself is an external side effect naming the agent: a ghost or a cross-side
   * re-composition used to receive a real invitation from the representative before the join's
   * roster check refused the masquerade — half an admission, on the wire, for an identity this
   * fleet does not vouch for. Refusing here means ZERO requests for a refused identity; the exit's
   * isRegisteredAgent on the join stays as depth (a second gate that must never be the first).
   */
  if (!backendRosterAdmits(agentMxid, sideId)) {
    console.warn(`[room-admission] REFUSED: ${agentMxid} is not a registered agent of this fleet on side ${sideId}`);
    return { admitted: false, reason: 'not_a_registered_agent', sideId, mxid: agentMxid };
  }

  const invite = await inviteToRoomOnSide({ ...acting, roomId, userId: agentMxid });
  if (!invite.invited && !invite.already) {
    /*
     * WHY, not just that it failed. A live run took `M_FORBIDDEN` here and the cause was neither the
     * credential nor the agent: the representative had entered this room by KNOCKING (decision 5), so it
     * holds `users_default` — 0 — against an `invite` requirement of 50 that the project set. It is in the
     * room and cannot bring anyone with it, and a bare 403 sends an operator to check the credential.
     *
     * Read from the room's power levels rather than matched on the error string, on the failure path only.
     * The remedy belongs to the project, so the message says which of the two things they can do.
     */
    const power = await canRepresentativeInvite({ ...acting, roomId });
    if (power.known && power.can === false) {
      return {
        admitted: false,
        reason: 'representative_lacks_invite_power',
        sideId,
        mxid: agentMxid,
        detail: `our representative holds power ${power.mine} in ${roomId} and inviting needs `
          + `${power.required}. The project side either grants it that power or invites ${agentMxid} `
          + 'itself; nothing on our side can raise it.',
      };
    }
    return { admitted: false, reason: 'invite_refused', sideId, mxid: agentMxid, detail: invite.reason };
  }

  /*
   * A registrationToken side stops here, and that is complete rather than half-done: there the agent
   * holds its own token and the bridge's existing join path uses it. Reporting `joined: false` with
   * the reason is what tells the two apart — an invite that is waiting for its own agent, versus one
   * nobody will ever act on.
   */
  /*
   * F10 (17-r3): same roster contract as the withdraw path — the backend's registry decides
   * which `@ac_` identities this fleet owns, and the single exit refuses a ghost.
   */
  const join = credential.kind === 'registrationToken'
    ? await Promise.resolve().then(() => runMatrixWork({ action: 'join', agent: agentName, sideId, roomId, mxid: agentMxid, engagementId: engagement.id }))
      .then((result) => ({ joined: result.ok, reason: result.code }))
      .catch(() => ({ joined: false, reason: 'bridge_work_pending' }))
    : await joinRoomOnSideAsAgent({
    ...acting, roomId, agentUserId: agentMxid,
    // F10 (17-r4): sideId — the authoritative-home check lives in the ruling
    isRegisteredAgent: (mxid) => backendRosterAdmits(mxid, sideId),
  });
  return {
    admitted: Boolean(join.joined),
    invited: Boolean(invite.invited),
    alreadyMember: Boolean(invite.already),
    joined: Boolean(join.joined),
    reason: join.joined ? null : (join.reason ?? null),
    sideId,
    mxid: agentMxid,
  };
}

function bindEngagement(engagement, resolvedOwner = null) {
  if (!engagement?.agent) {
    return { bound: false, error: 'engagement names no agent' };
  }
  const owner = resolvedOwner || resolveOwnerFor(engagement.agent, engagement.projectRoomId ?? null);
  if (!owner) {
    /*
     * F04: no binding for (agent, THIS room) and no accepted-source fallback.
     * Reporting "unbound" honestly beats borrowing another project's record —
     * the old [0] pick wrote the first project's owner into every later
     * engagement, which is the cross-project bleed this fixes.
     */
    return {
      bound: false,
      error: `no owner known for agent ${engagement.agent} in project room ${engagement.projectRoomId ?? '(none)'}: `
        + 'let the Matrix bridge create this room binding from its inviter, or set HAGENCY_OWNER_MXID and HAGENCY_OWNER_DM_ROOM',
    };
  }
  try {
    approvalStore.upsertBinding({
      agent: engagement.agent,
      project: engagement.project,
      project_room_id: engagement.projectRoomId,
      owner_mxid: owner.ownerMxid,
      owner_dm_room_id: owner.ownerDmRoomId,
    });
    return { bound: true, ownerMxid: owner.ownerMxid, from: owner.from };
  } catch (error) {
    return { bound: false, error: error?.message ?? 'upsertBinding failed' };
  }
}

function engagementResult(req, engagement, extras = {}) {
  if (req.engagementCaller === 'fleet') return fleetPublicEngagement(engagement);
  const internal = req.engagementCaller !== 'requester' && req.engagementCaller !== 'matrix';
  if (internal) return { ok: true, engagement, ...extras };
  const fields = ['id', 'requestId', 'idempotent', 'project', 'projectRoomId', 'role', 'requester',
    'requestedTokens', 'ratePerDay', 'agent', 'createdAt', 'route', 'autoJoined', 'state',
    'allocatedTokens', 'decidedAt', 'endedAt', 'bound'];
  const safe = Object.fromEntries(fields.filter((key) => engagement?.[key] !== undefined).map((key) => [key, engagement[key]]));
  const binding = extras.binding ? {
    bound: extras.binding.bound === true,
    ...(extras.binding.bound ? {} : { error: 'The agent could not be attached; the contributor must resolve its setup.' }),
  } : undefined;
  return { ok: true, engagement: safe, serving: extras.serving ?? servingConfiguration(engagement?.agent),
    ...(binding ? { binding } : {}),
    ...(extras.roomAdmission ? { roomAdmission: { admitted: extras.roomAdmission.admitted === true } } : {}),
  };
}

const engagementFulfillments = new Map();
const engagementAllocationChoices = new Map();
function runMatrixWork(command) {
  if (!getBridgeSecret()) throw new Error('Matrix bridge authorization is not configured');
  const job = matrixWorkStore.enqueue(command);
  return awaitMatrixWork(matrixWorkStore, job.id);
}
let launchEngagementAgent = async (agent) => {
  // Thread runners are launched by dispatch, with their workspace lease already held.
  if (THREAD_SESSIONS_ENABLED) {
    if (!threadSessionAgentEligibility(agent).ok) throw new Error('agent is not eligible for thread dispatch');
    return;
  }
  const rp = agent.runtimeProfile?.primary;
  const env = { ...process.env, HAGENCY_LAUNCH_MODEL: rp?.model || '' };
  if (rp?.apiBaseUrl && agent.type === 'claude') env.ANTHROPIC_BASE_URL = rp.apiBaseUrl;
  if (rp?.apiKey && agent.type === 'claude') env.ANTHROPIC_API_KEY = rp.apiKey;
  await execFileAsync(path.join(REPO_ROOT, 'bin', 'hagency'), ['up-v1', agent.name, agent.type], {
    cwd: REPO_ROOT, env, encoding: 'utf8', timeout: 120_000, maxBuffer: 8 * 1024 * 1024,
  });
};

function resourceBudget(preset, options = {}) {
  const candidate = { type: preset.framework, server: LOCAL_SERVER_ID, runtimeProfile: runtimeProfileFromPreset(preset) };
  const identity = (a) => seatIdentity(a, { keyId: SEAT_KEY_ID, secret: SEAT_KEY_SECRET }).seatId;
  const seatId = identity(candidate);
  const commitments = engagementStore.list().filter(holdsAllocation).map(e => {
    const agent = isAgentRecord(agents[e.agent]) ? agents[e.agent] : null;
    const reservedPreset = frameworkPresets.find(p => p.id === e.fulfillment?.presetId);
    const target = agent || (reservedPreset ? { type: reservedPreset.framework, server: LOCAL_SERVER_ID,
      runtimeProfile: runtimeProfileFromPreset(reservedPreset) } : null);
    return { ...e, presetId: agent?.presetId || reservedPreset?.id, seatId: target ? identity(target) : null };
  });
  return resourceAllocationBudget({ preset, seatId, declaration: seatDeclarations[seatId], commitments, ...options });
}

function resourceRemaining(preset, options = {}) {
  return preset ? resourceBudget(preset, options).remainingTokens : null;
}

function unprovisionedSeatCommitments(seatId) {
  return engagementStore.list({ state: 'pending' }).filter((e) => e.fulfillment
    && e.fulfillment.phase !== 'failed' && !isAgentRecord(agents[e.agent]))
    .reduce((sum, e) => {
      const preset = frameworkPresets.find((p) => p.id === e.fulfillment.presetId);
      if (!preset) return sum;
      const candidate = { type: preset.framework, server: LOCAL_SERVER_ID, runtimeProfile: runtimeProfileFromPreset(preset) };
      return sum + (seatIdentity(candidate, { keyId: SEAT_KEY_ID, secret: SEAT_KEY_SECRET }).seatId === seatId ? e.allocatedTokens : 0);
    }, 0);
}

function fulfillEngagement(id, options = {}) {
  if (engagementFulfillments.has(id)) {
    if (options.allocation && JSON.stringify(allocationChoice(options.allocation)) !== engagementAllocationChoices.get(id)) {
      return Promise.reject(new EngagementError('conflict', 'A different Agent selection is already in progress'));
    }
    const e = engagementStore.get(id);
    if (options.allocatedTokens !== undefined && Number(options.allocatedTokens) !== e?.allocatedTokens
      || options.owner && JSON.stringify(options.owner) !== JSON.stringify(e?.fulfillment?.owner)) {
      return Promise.reject(new EngagementError('conflict', 'a different fulfillment decision is already in progress'));
    }
    return engagementFulfillments.get(id);
  }
  engagementAllocationChoices.set(id, options.allocation ? JSON.stringify(allocationChoice(options.allocation)) : null);
  const work = fulfillEngagementOnce(id, options).finally(() => { engagementFulfillments.delete(id); engagementAllocationChoices.delete(id); });
  engagementFulfillments.set(id, work);
  return work;
}

async function fulfillEngagementOnce(id, { allocatedTokens, by = 'operator', reason, autoJoin = false, owner: suppliedOwner = null, allocation = null } = {}) {
  let e = engagementStore.get(id);
  if (!e) throw new EngagementError('not_found', 'engagement not found');
  const selectedChoice = allocation ? allocationChoice(allocation) : e.allocationChoice || e.fulfillment?.allocationChoice
    || (e.requestContext?.agentDefinition ? { kind: 'project-definition' } : null);
  if (e.requestContext?.agentDefinition && selectedChoice.kind !== 'project-definition') {
    throw new EngagementError('conflict', 'The project defined this Agent and resource; submit a new request to change them');
  }
  const savedChoice = e.allocationChoice || e.fulfillment?.allocationChoice || (e.agent ? { kind: 'agent', agent: e.agent } : null);
  if (allocation && (e.fulfillment || e.state === 'active') && JSON.stringify(selectedChoice) !== JSON.stringify(savedChoice)) {
    throw new EngagementError('conflict', 'A reserved or active assignment cannot change its Agent');
  }
  let selected = e.state === 'pending' && selectedChoice ? allocationCandidate(e, selectedChoice) : null;
  if (selected) e = { ...e, agent: selected.name };
  if (e.state === 'active') {
    if (!autoJoin && getBridgeSecret() && projectSideStore.getSide(sideIdForRoom(e.projectRoomId))) {
      e = engagementStore.queueApprovalNotice(id);
    }
    return { engagement: e };
  }
  if (e.state !== 'pending') throw new EngagementError('conflict', 'engagement is no longer pending');
  if (!roleCrossFamilyAvailable(e.role, e.projectRoomId)) {
    throw new EngagementError('cross_family_unavailable', 'this role requires qualifying agents from two model families on the project side');
  }
  const alloc = Number(allocatedTokens ?? e.allocatedTokens ?? e.requestedTokens);
  if (!Number.isSafeInteger(alloc) || alloc <= 0) throw new EngagementError('bad_request', 'allocatedTokens must be a positive integer');
  if (e.fulfillment && allocatedTokens !== undefined && alloc !== e.allocatedTokens) {
    throw new EngagementError('conflict', 'retry must retain the reserved allocation; reject this request to release it');
  }
  if (suppliedOwner && (!/^@[^:\s]+:[^\s]+$/.test(suppliedOwner.ownerMxid)
    || !/^![^:\s]+:[^\s]+$/.test(suppliedOwner.ownerDmRoomId))) {
    throw new EngagementError('bad_request', 'owner requires a full MXID and a private Matrix room id');
  }
  const owner = suppliedOwner || e.fulfillment?.owner || resolveOwnerFor(e.agent, e.projectRoomId);
  if (!owner) {
    engagementStore.setBindingOutcome(id, { bound: false, error: 'project owner is unavailable' });
    throw new EngagementError('owner_unavailable', 'project owner is unavailable; engagement remains pending');
  }
  const sideId = sideIdForRoom(e.projectRoomId);
  const side = sideId ? projectSideStore.getSide(sideId) : null;
  if (e.requestContext) {
    const credential = side && projectSideStore.credentialFor(sideId);
    if (!side?.active || credential?.kind !== 'appservice'
      || fleetIdForSender(credential.senderLocalpart) !== e.requestContext.fleetId
      || owner.ownerMxid !== e.requestContext.ownerMxid || owner.ownerDmRoomId !== e.requestContext.ownerDmRoomId) {
      throw new EngagementError('owner_unavailable', 'Use the verified project owner and private approval room for this request.');
    }
    try {
      await verifyFleetTarget(e.requestContext, { ...side, fleetId: e.requestContext.fleetId,
        representativeMxid: side.representative?.mxid }, async (_side, roomId, suffix, options = {}) => {
        const url = new URL(`${side.apiBaseUrl.replace(/\/+$/, '')}/_matrix/client/v3/rooms/${encodeURIComponent(roomId)}/${suffix}`);
        url.searchParams.set('user_id', side.representative.mxid);
        const response = await fetch(url, { headers: { Authorization: `Bearer ${credential.asToken}` }, signal: AbortSignal.timeout(15_000) });
        if (options.optional && response.status === 404) return null;
        if (!response.ok) throw new Error('Matrix target authorization unavailable');
        return response.json();
      });
    } catch {
      throw new EngagementError('owner_unavailable', 'Project authorization changed or could not be verified; engagement remains pending.');
    }
  }
  if (owner.ownerDmRoomId === e.projectRoomId) {
    throw new EngagementError('bad_request', 'owner approval room must be private and distinct from the project room');
  }
  const mxidKey = (value) => {
    if (typeof value !== 'string') return null;
    const separator = value.indexOf(':');
    return separator < 0 ? null : value.slice(0, separator) + value.slice(separator).toLowerCase();
  };
  const ownerKey = mxidKey(owner.ownerMxid);
  if (owner.ownerMxid?.startsWith(`@${agentPrefixOnSide(side)}`)
    || Object.values(agents).some((candidate) => isAgentRecord(candidate) && mxidKey(recordedAgentMxid(candidate)) === ownerKey)
    || ownerKey && ownerKey === mxidKey(side?.representative?.mxid)) {
    throw new EngagementError('bad_request', 'project owner must be a human, not an agent or project representative');
  }
  if (side && !side.active) throw new EngagementError('agent_unavailable', 'project side is deactivated');
  const reserved = e.fulfillment && e.fulfillment.phase !== 'failed' ? e.allocatedTokens || 0 : 0;
  const budget = side ? sideBudgetFor(sideId) : null;
  if (!usesResourcePool(e) && budget && (budget.allocated === null || alloc > budget.remaining + reserved)) {
    throw new EngagementError('over_allocation', 'project side has insufficient allocation');
  }
  if (selected) {
    const current = allocationCandidate(e, selected.choice);
    if (current.name !== selected.name) throw new EngagementError('conflict', 'Agent definition changed during approval; review it again');
    selected = current;
  }
  const needsProvisioning = !e.agent || Boolean(e.fulfillment) || selected?.provision;
  if (!needsProvisioning) {
    if (!agentEligibleForRoom(agents[e.agent], e.projectRoomId)) throw new EngagementError('agent_unavailable', 'agent is no longer eligible for this project side');
    const remaining = remainingFor(e.agent, { forAutoJoin: autoJoin });
    if (remaining === null) throw new EngagementError('no_ceiling', 'cannot allocate without a known limit');
    if (alloc > remaining) throw new EngagementError('over_commit', autoJoin
      ? 'The contributor cannot accept this allocation.' : overCommitMessage(e, alloc, remaining, ceilingSpendFor(e.agent)));
    if (selected) e = engagementStore.selectAgent(id, selected.name, selected.choice, by);
    const binding = bindEngagement(e, owner);
    engagementStore.setBindingOutcome(id, binding);
    if (!binding.bound) throw new EngagementError('owner_unavailable', 'project owner could not be bound');
    e = engagementStore.decide({ engagementId: id, approve: true, allocatedTokens: alloc, remainingTokens: remaining, by, reason, autoJoin,
      notify: Boolean(side && getBridgeSecret()) });
    return { engagement: e, binding, roomAdmission: await admitAgentToProjectRoom(e) };
  }
  if (!side) throw new EngagementError('no_project_side', 'configure the project side before provisioning');
  const credential = projectSideStore.credentialFor(sideId);
  if (!['appservice', 'registrationToken'].includes(credential?.kind)
    || credential.kind === 'registrationToken' && !getBridgeSecret()) {
    throw new EngagementError('provision_unavailable', 'on-demand provisioning requires a side credential and its authenticated Matrix bridge');
  }
  const preset = e.fulfillment ? frameworkPresets.find((p) => p.id === e.fulfillment.presetId)
    : selected ? selected.preset : resourcesForRole(e.role, ROLE_DEFAULT_TIER[e.role], frameworkPresets)
      .map((r) => r.preset).find((p) => ['claude', 'codex'].includes(p.framework)
        && (resourceRemaining(p, { forAutoJoin: autoJoin }) ?? -1) >= alloc);
  if (!preset || !['claude', 'codex'].includes(preset.framework)) throw new EngagementError('no_ceiling', 'no qualifying resource has sufficient capacity');
  if (selected && (selected.remainingTokens ?? -1) < alloc) {
    const limits = selected.budget;
    const detail = limits?.seat.status === 'period_mismatch' ? 'The shared account quota period does not match this pool.'
      : limits?.pool.remaining < alloc ? `Selected pool has ${limits.pool.remaining} tokens available; this approval needs ${alloc}.`
        : limits?.seat.remaining !== null && limits?.seat.remaining < alloc
          ? `The shared account has ${limits.seat.remaining} tokens available across its pools; this approval needs ${alloc}.`
          : 'Selected resource has insufficient remaining capacity';
    throw new EngagementError('over_commit', detail);
  }
  const agentName = e.agent || normalizeAgentName(`mx_${sideId.replace(/[^a-z0-9]/g, '_').slice(0, 24)}_${e.role}_${createHash('sha256').update(id).digest('hex').slice(0, 12)}`);
  e = engagementStore.planFulfillment(id, { agent: agentName, allocatedTokens: alloc, presetId: preset.id, sideId, owner, ...(selected ? { allocationChoice: selected.choice, definitionId: selected.definitionId } : {}) });
  const assertPending = () => {
    if (engagementStore.get(id)?.state !== 'pending' || !projectSideStore.getSide(sideId)?.active) {
      throw new EngagementError('conflict', 'engagement was cancelled or its side deactivated during provisioning');
    }
  };
  try {
    if (!isAgentRecord(agents[agentName])) {
      const { stdout } = await execFileAsync(process.execPath, [path.join(REPO_ROOT, 'scripts/provision-v1-agent-home.js'),
        '--name', agentName, '--type', preset.framework], { cwd: REPO_ROOT, env: process.env, encoding: 'utf8', timeout: 120_000 });
      assertPending();
      const provisioned = JSON.parse(stdout);
      if (!provisioned.ok || !provisioned.paths) throw new Error('provisioner returned no agent home');
      const snapshot = snapshotAgentPersistenceState(agentName);
      agents[agentName] = {
        name: agentName, kind: 'agent', type: preset.framework, projectSide: sideId, presetId: preset.id,
        ...(e.fulfillment.definitionId ? { resourceDefinitionId: e.fulfillment.definitionId } : {}),
        ...(e.requestContext?.agentDefinition ? { projectAgentRequestId: e.requestId,
          projectAgentDefinition: { ...e.requestContext.agentDefinition, projectRoomId: e.projectRoomId },
          displayName: e.requestContext.agentDefinition.name, role: e.role } : {}),
        engagementProvisioningId: id,
        runtimeProfile: normalizeRuntimeProfile(runtimeProfileFromPreset(preset)), agentId: `agent_${agentName}`,
        executionPolicy: normalizeExecutionPolicy(preset.executionPolicy, preset.framework),
        homeDir: provisioned.paths.homeDir, workdir: provisioned.paths.workdir, stateDir: provisioned.paths.stateDir,
        server: LOCAL_SERVER_ID, tmux: null, online: false, offlineReason: 'provisioned', manualDown: false,
        registeredAt: Date.now(), discoveredAt: Date.now(), lastSeen: Date.now(),
      };
      if (!saveAgentsOrRollback(agentName, snapshot)) throw new Error('agent persistence failed');
      loadAgentTokens();
    }
    if (!agentEligibleForRoom(agents[agentName], e.projectRoomId, id)) throw new Error('provisioned agent is not eligible for this side');
    prepareThreadRuntime(agents[agentName]);
    engagementStore.setFulfillmentPhase(id, 'provisioned');
    if (!recordedAgentMxid(agents[agentName])) {
      const localpart = `${agentPrefixOnSide(side, credential)}${agentName}`;
      const minted = credential.kind === 'registrationToken'
        ? await runMatrixWork({ action: 'identity', agent: agentName, sideId, localpart, engagementId: id })
          .then((result) => ({ minted: result.ok, mxid: result.mxid, kind: 'registrationToken', reason: result.code }))
        : await mintAgentIdentity({ side, credential, localpart });
      assertPending();
      if (!minted.minted) throw new Error(minted.reason || 'Matrix identity could not be minted');
      const snapshot = snapshotAgentPersistenceState(agentName);
      agents[agentName].matrixIdentity = { mxid: minted.mxid, sideId, kind: minted.kind };
      if (!saveAgentsOrRollback(agentName, snapshot)) throw new Error('identity persistence failed');
    }
    engagementStore.setFulfillmentPhase(id, 'identified');
    const binding = bindEngagement(e, owner);
    engagementStore.setBindingOutcome(id, binding);
    if (!binding.bound) throw new Error('project owner could not be bound');
    engagementStore.setFulfillmentPhase(id, 'bound');
    const roomAdmission = await admitAgentToProjectRoom(e);
    assertPending();
    if (!roomAdmission.admitted) throw new Error(roomAdmission.reason || 'agent could not join the project room');
    engagementStore.setFulfillmentPhase(id, 'joined');
    if (!agents[agentName].online) {
      engagementStore.setFulfillmentPhase(id, 'starting');
      await launchEngagementAgent(agents[agentName]);
      assertPending();
    }
    // Readiness must outlive the bounded engagement history. Do not require a
    // completed agent to keep its original engagement row forever to be reused.
    if (agents[agentName].engagementProvisioningId) {
      const snapshot = snapshotAgentPersistenceState(agentName);
      delete agents[agentName].engagementProvisioningId;
      if (!saveAgentsOrRollback(agentName, snapshot)) throw new Error('agent readiness persistence failed');
    }
    const remaining = remainingFor(agentName, { forAutoJoin: autoJoin, excludeEngagementId: id });
    e = engagementStore.decide({ engagementId: id, approve: true, allocatedTokens: alloc, remainingTokens: remaining, by, reason, autoJoin,
      notify: Boolean(side && getBridgeSecret()) });
    return { engagement: e, binding, roomAdmission };
  } catch (error) {
    // Keep the reservation while setup is incomplete. A verdict retries the same
    // plan; rejection releases it. Releasing here could overbook a live partial agent.
    const current = engagementStore.get(id);
    if (current?.state !== 'pending' || !projectSideStore.getSide(sideId)?.active) {
      // Rejection may have withdrawn while the in-flight join was still pending.
      // Repeat the idempotent withdrawal after that join settles.
      await detachEngagement(current).catch((cleanupError) => {
        console.warn(`[engagements] cancellation cleanup ${id}: ${cleanupError.message}`);
      });
    }
    if (current?.state === 'pending') engagementStore.setFulfillmentPhase(id, current.fulfillment.phase, 'setup incomplete; operator retry required');
    console.error(`[engagements] fulfillment ${id}: ${error.message}`);
    throw new EngagementError('fulfillment_incomplete', 'setup incomplete; retry the verdict or reject the engagement');
  }
}

/**
 * Detach on revoke or rejection — but only when this was the LAST live engagement holding
 * the binding.
 *
 * A binding is keyed on `(agent, projectRoomId)` while engagements are individual, so one
 * binding serves every engagement between that agent and that room. Removing it whenever any
 * one of them ended detached the agent from the project while other engagements were still
 * ACTIVE: the access granted by an approval was revoked by an unrelated refusal.
 *
 * Not hypothetical. The live store holds six concurrent active engagements for a single
 * (agent, project room) pair, each drawing on the ceiling separately, so refusing a seventh
 * request would have cut the access the other six were relying on — a rejection acting as a
 * revocation of work nobody decided to end.
 *
 * Caught by tests/engagement-binding.test.js, and only because that case approves before it
 * rejects. Rejecting without a prior approval cannot tell "the rejection removed nothing"
 * from "nothing was there to remove", which is why the first version of that test passed
 * against this bug.
 */
function unbindEngagement(engagement) {
  if (!engagement?.agent || !engagement?.projectRoomId) return;
  try {
    /*
     * Live means active or pending — anything a project could still be relying on.
     *
     * The `e.id !== engagement.id` term is REDUNDANT TODAY and kept deliberately. The store
     * records the new state before calling here, so this engagement is already `rejected` or
     * `revoked` and the state filter alone excludes it; a mutation removing the id term
     * passes every test, which is the honest description of it. It stays because the
     * alternative failure is silent and total: if that ordering ever changed, the engagement
     * would count itself as still live and NOTHING would ever unbind, leaving standing
     * reachability behind every ended engagement.
     */
    const stillLive = engagementStore.list()
      .filter((e) => e.id !== engagement.id
        && e.agent === engagement.agent
        && e.projectRoomId === engagement.projectRoomId
        && (e.state === 'active' || e.state === 'pending'));
    if (stillLive.length > 0) {
      console.log(
        `[engagements] keeping ${engagement.agent} bound to ${engagement.projectRoomId}: `
        + `${stillLive.length} engagement(s) still live`,
      );
      return { keptBound: true, reason: `${stillLive.length} engagement(s) still live` };
    }
    /*
     * `removed` is null when there was no binding, which is an ordinary state rather than a failure —
     * this deployment approved six engagements with no resolvable owner and therefore no bindings at
     * all, while the agents were in the rooms. So the answer callers need is `keptBound`, not "did a
     * record exist": a room seat has to be given back whether or not we ever wrote the binding for it.
     */
    const removed = approvalStore.removeBinding(engagement.agent, engagement.projectRoomId);
    return { keptBound: false, removedBinding: Boolean(removed), reason: null };
  } catch (error) {
    // A binding that was never created is not an error worth failing the revoke for.
    console.warn(`[engagements] unbind ${engagement.agent}: ${error?.message ?? error}`);
    return { keptBound: false, removedBinding: false, reason: String(error?.message ?? error) };
  }
}

/**
 * End an engagement's reachability: the binding AND the seat in the room.
 *
 * THE HALF THAT WAS MISSING. `unbindEngagement` removed Hagency's own record and stopped there, so a
 * revoked engagement left the agent joined to the borrower's room forever. Confirmed against the
 * homeserver, not inferred — see `withdrawAgentFromProjectRoom`.
 *
 * TWO GATES, AND BOTH ARE NARROW.
 *
 *  1. NO OTHER LIVE ENGAGEMENT holds this agent in this room — which `unbindEngagement` already decides,
 *     and is the whole reason this is one function rather than two calls at each site. Six concurrent
 *     engagements between one agent and one room is a real shape in this deployment's data, and
 *     withdrawing while another is live would take a working agent out of a room it is still serving.
 *
 *  2. THIS ENGAGEMENT ACTUALLY ALLOCATED. `allocatedTokens` is set only by an approval, so it is exactly
 *     "this engagement is why the agent is in that room". A request REJECTED from pending never admitted
 *     anybody, and leaving as an agent who was never let in would be a needless call to somebody else's
 *     homeserver on every refusal.
 *
 * NOT GATED ON A BINDING HAVING EXISTED. That was the mistake #114 shipped and #115 fixed: an agent
 * reaches a project room through `admitAgentToProjectRoom(engagement)`, and a deployment can have agents
 * in rooms with no bindings at all.
 *
 * AWAITED, for the reason `admitAgentToProjectRoom` is awaited on the approve path: an operator reading
 * `ok: true` must not be told the agent is detached while it is still sitting in the customer's room. The
 * outcome rides back on the response the same way the admission does.
 */
async function detachEngagement(engagement) {
  const binding = unbindEngagement(engagement);
  if (binding?.keptBound) return { binding, roomWithdrawal: null };
  if (!engagement?.agent || !engagement?.projectRoomId) return { binding, roomWithdrawal: null };
  if (!Number.isFinite(engagement.allocatedTokens) || engagement.allocatedTokens <= 0) {
    return { binding, roomWithdrawal: null };
  }
  try {
    const roomWithdrawal = await withdrawAgentFromProjectRoom(
      normalizeAgentName(engagement.agent),
      engagement.projectRoomId,
    );
    return { binding, roomWithdrawal };
  } catch (error) {
    /*
     * The engagement IS over — that decision is already recorded. A seat we could not give back is
     * reported, never allowed to turn a completed revoke into a 500.
     */
    return { binding, roomWithdrawal: { roomId: engagement.projectRoomId, left: false, reason: String(error?.message ?? error) } };
  }
}

function respondEngagementError(res, error, fallback, extras = {}) {
  if (error instanceof EngagementError) {
    const status = { bad_request: 400, not_found: 404, conflict: 409, over_commit: 409, no_ceiling: 409,
      owner_unavailable: 409, agent_unavailable: 409, over_allocation: 409, no_project_side: 409,
      provision_unavailable: 409, cross_family_unavailable: 409, fulfillment_incomplete: 503 }[error.code] ?? 500;
    return res.status(status).json({ ...extras, ok: false, error: error.message, code: error.code });
  }
  console.error(`[engagements] ${fallback}:`, error?.message || error);
  return res.status(500).json({ error: fallback });
}

app.get('/api/engagements', requireBearer, (req, res) => {
  const state = normalizeOptionalText(req.query.state, 32) || undefined;
  const rows = engagementStore.list({ state }).map((e) => ({
    ...e,
    // Readiness is scoped to this project; the console must not infer an owner
    // from its requester or an unrelated project's private approval binding.
    ownerBindingRequired: e.state === 'pending'
      && !(e.fulfillment?.owner || resolveOwnerFor(e.agent, e.projectRoomId)),
    // What is LEFT on the agent behind it, so the queue can show over-commitment
    // before the decision rather than reporting it afterwards.
    agentRemainingTokens: e.agent ? remainingFor(e.agent) : null,
  }));
  return res.json({ engagements: rows, generatedAt: Date.now() });
});

app.get('/api/engagements/audit', requireBearer, (req, res) => {
  const limit = Number(req.query.limit) || 200;
  return res.json({ audit: engagementStore.listAudit({ limit }) });
});

/*
 * An inbound request. Routed on arrival, and the agent is resolved here so the
 * record names it from the start.
 *
 * Not guarded by requireApprovalBridgeSecret: an engagement request comes from a
 * project, which is a different caller from the bridge that carries per-tool-call
 * approvals. Wiring it behind the bridge secret would have made the console unable
 * to read its own queue, which is the mistake the existing binding endpoint makes.
 */
app.post('/api/engagements', requireRequester, createEngagementRequest);
async function createEngagementRequest(req, res) {
  try {
    const b = req.body || {};
    const role = normalizeOptionalText(b.role, 64);
    if (!ROLES.includes(role)) return res.status(400).json({ error: `unknown role: ${role}` });
    const replay = engagementStore.replayRequest({ ...b, role, requestContext: req.fleetContext ?? null });
    if (replay) return res.json(engagementResult(req, replay));

    /*
     * WHICH AGENT SERVES A ROLE IS NOT THE REQUESTER'S CHOICE.
     *
     * `normalizeOptionalText(b.agent) || agentForRole(role)` let the body override the
     * capability model entirely: posting `{ role: 'architect', agent: 'some-weak-agent' }`
     * produced an architect engagement served by an agent that does not qualify at
     * `strong`, and auto-joined it if that agent had headroom. The tier check was
     * simply skipped.
     *
     * A requested agent is now a HINT, honoured only if it independently qualifies
     * for the role. Anything else falls back to the capability model's own answer.
     */
    const requested = normalizeOptionalText(b.agent, 128);
    let agent = req.fleetContext?.agentDefinition ? null : agentForRole(role, b.projectRoomId);
    let requestedResource = null;
    if (req.fleetContext?.agentDefinition) {
      const context = req.fleetContext;
      requestedResource = allocationCandidate({ role, projectRoomId: b.projectRoomId, requestContext: context }, { kind: 'project-definition' }).preset;
      if (engagementStore.list().some(e => e.projectRoomId === b.projectRoomId && ['pending', 'active'].includes(e.state)
        && e.requestContext?.agentDefinition?.name === context.agentDefinition.name)) {
        throw new EngagementError('conflict', 'This project already has a pending or active Agent with that name');
      }
    }
    if (requested) {
      const TIER_RANK = Object.fromEntries([...CAPABILITY_TIERS].reverse().map((t, i) => [t, i]));
      const row = serializeAgents(Object.values(agents).filter(isAgentRecord))
        .find((a) => a.name === requested);
      if (row && !agentEligibleForRoom(agents[requested], b.projectRoomId)) {
        return res.status(400).json({ error: 'requested agent is not eligible for this project side' });
      }
      if (row && agents[requested].projectAgentDefinition
        && agents[requested].projectAgentDefinition.projectRoomId !== b.projectRoomId) {
        return res.status(400).json({ error: 'requested agent belongs to another project' });
      }
      const tier = row ? modelTier(row.runtimeProfile) : null;
      const qualifies = tier && TIER_RANK[tier] >= TIER_RANK[ROLE_DEFAULT_TIER[role]];
      if (qualifies) agent = requested;
      else if (row) {
        // Refused rather than silently reassigned: a caller that named an agent is
        // making a claim, and quietly serving a different one hides that it was wrong.
        return res.status(400).json({
          error: `${requested} does not qualify for ${role} (needs ${ROLE_DEFAULT_TIER[role]}, has ${tier ?? 'no qualifying model'})`,
        });
      } else {
        return res.status(400).json({ error: `unknown agent: ${requested}` });
      }
    }
    /*
     * NOBODY CAN FILL THIS ROLE — so say what it would TAKE, without refusing the request.
     *
     * ADR-016 decision 4's role-matched selection lived on `POST /api/dispatch`, which ADR-013 decision 8
     * withdrew and which has no product caller: the matcher was correct and unreachable. This is the
     * reachable end of it — the engagement path, where a project actually asks for a role.
     *
     * A HINT, NOT A REFUSAL, and that is a correction of my own first version. Refusing would have been a
     * change to the contract a project side depends on, and two existing tests encode the opposite: a
     * request with no qualifying agent is RECORDED (the project asked; that is a fact worth keeping) and
     * discloses `serving: null` rather than a guess. Turning that into a 409 would make the queue model
     * ADR-013 chose unreachable from the outside.
     *
     * IT PLANS, IT DOES NOT PROVISION. Launching an agent is an operator act with a cost, and doing it
     * inside somebody else's request would spend the contributor's money because a stranger asked. The
     * hint names the preset that WOULD staff it, so the next step is one command rather than an
     * investigation — and names none when nothing configured qualifies, which is a different problem.
     */
    const provisionHint = agent ? null : (() => {
      const need = ROLE_DEFAULT_TIER[role];
      const candidate = resourceForRole(role, need, frameworkPresets);
      return {
        reason: candidate ? 'no_agent_provisioned_for_role' : 'no_resource_for_role',
        role,
        tier: need,
        presetId: candidate?.id ?? null,
        presetsConsidered: frameworkPresets.length,
        detail: candidate
          ? `no agent serves ${role} at ${need} yet; preset ${candidate.id} (${candidate.model}) qualifies`
          : `no agent serves ${role} at ${need}, and none of the ${frameworkPresets.length} configured `
            + 'preset(s) reaches that tier with a ceiling',
      };
    })();

    /*
     * THE BORROWER'S BUDGET, CHECKED BEFORE THE RECORD EXISTS.
     *
     * Here rather than after `createRequest` because a whitelisted project with a published offer
     * AUTO-JOINS: `routeRequest` returns `autoJoin: true`, the engagement is born `active`, and its
     * `requestedTokens` are committed against the side without any operator ever seeing it. Checking
     * afterwards would mean unwinding a commitment already made, which is the shape of bug this
     * repository has produced before.
     *
     * Verified Palpo requests cannot auto-join. Recording their definitions does
     * not reserve tokens or create an Agent, so an exhausted side can still ask
     * for a new Resource and await the provider's decision. The verdict and
     * fulfillment paths both recheck the side budget before any reservation.
     */
    if (req.engagementCaller !== 'requester' && req.engagementCaller !== 'fleet' && refuseOverSideAllocation(res, {
      projectRoomId: b.projectRoomId,
      tokens: b.requestedTokens,
      act: 'this engagement',
      publicResult: req.engagementCaller === 'matrix',
    })) return undefined;
    const engagement = engagementStore.createRequest({
      project: b.project,
      projectRoomId: b.projectRoomId,
      role,
      requester: b.requester,
      requestedTokens: b.requestedTokens,
      ratePerDay: b.ratePerDay,
      /*
       * PRD A-R0-1: repeating the same request_id and digest yields the same
       * assignment; a different digest is a conflict. The bridge supplies the Matrix
       * event id, which both sides already share and the sender cannot forge.
       *
       * Optional, because requiring it would refuse every existing caller. Its absence
       * is recorded on the engagement rather than replaced with a generated key, so a
       * request that could not be deduped says so.
       */
      requestId: b.requestId,
      requestContext: req.fleetContext ?? null,
      agent,
      remainingTokens: agent ? remainingFor(agent, { forAutoJoin: true })
        : resourceRemaining(requestedResource || resourceForRole(role, ROLE_DEFAULT_TIER[role], frameworkPresets), { forAutoJoin: true }),
      allowAutoJoin: req.engagementCaller !== 'requester' && req.engagementCaller !== 'fleet',
      crossFamilyOk: roleCrossFamilyAvailable(role, b.projectRoomId),
      deferActivation: true,
    });
    /*
     * An auto-join goes straight to active, so it must bind here — the verdict
     * route it would otherwise pass through never runs for it.
     */
    /*
     * `serving` rides with the engagement rather than being looked up separately, so a
     * borrower learns what they got in the same response that tells them they got it —
     * and so a caller cannot end up displaying a role without its fulfilment.
     */
    const serving = servingConfiguration(engagement.agent);
    if (engagement.route === 'autoJoin') {
      try {
        const result = await fulfillEngagement(engagement.id, { by: 'auto', autoJoin: true });
        return res.json(engagementResult(req, result.engagement, { ...result, serving: servingConfiguration(result.engagement.agent) }));
      } catch (error) {
        return respondEngagementError(res, error, 'failed to fulfill engagement',
          engagementResult(req, engagementStore.get(engagement.id)));
      }
    }
    return res.json(engagementResult(req, engagement, { serving, ...(provisionHint ? { provisionHint } : {}) }));
  } catch (e) {
    return respondEngagementError(res, e, 'failed to create engagement');
  }
}

app.get('/api/engagements/:id/candidates', requireBearer, (req, res) => {
  const e = engagementStore.get(req.params.id);
  if (!e) return res.status(404).json({ error: 'Engagement not found' });
  return res.json({ candidates: engagementCandidates(e), locked: Boolean(e.fulfillment) || e.state !== 'pending',
    allocation: e.allocationChoice || e.fulfillment?.allocationChoice || (e.requestContext?.agentDefinition ? { kind: 'project-definition' } : null) });
});

app.post('/api/engagements/:id/verdict', requireBearer, async (req, res) => {
  try {
    const b = req.body || {};
    if (b.approve === true) {
      const existing = engagementStore.get(req.params.id);
      const reserved = existing?.fulfillment && existing.fulfillment.phase !== 'failed' ? existing.allocatedTokens || 0 : 0;
      if (existing?.state === 'pending' && !usesResourcePool(existing) && refuseOverSideAllocation(res, {
        projectRoomId: existing.projectRoomId,
        tokens: Number(b.allocatedTokens ?? existing.allocatedTokens ?? existing.requestedTokens) - reserved,
        act: 'approving this engagement', extra: { engagementId: existing.id },
      })) return undefined;
      try {
        const result = await fulfillEngagement(req.params.id, {
          allocation: b.allocation, allocatedTokens: b.allocatedTokens, by: getRequestAgentName(req) || 'operator', reason: b.reason, owner: b.owner,
        });
        return res.json({ ok: true, ...result, serving: servingConfiguration(result.engagement.agent) });
      } catch (error) {
        return respondEngagementError(res, error, 'failed to fulfill engagement', { engagement: engagementStore.get(req.params.id) });
      }
    }
    const e = engagementStore.decide({ engagementId: req.params.id, approve: false,
      by: getRequestAgentName(req) || 'operator', reason: b.reason });
    const { roomWithdrawal } = await detachEngagement(e);
    return res.json({ ok: true, engagement: e, ...(roomWithdrawal ? { roomWithdrawal } : {}) });
  } catch (e) {
    return respondEngagementError(res, e, 'failed to record verdict');
  }
});

const engagementRevocations = new Map();
async function retireEngagementAgent(req, engagement, credential) {
  const agent = agents[engagement.agent];
  const mxid = recordedAgentMxid(agent);
  if (!agent || !mxid || agent.projectSide !== sideIdForRoom(engagement.projectRoomId)) {
    throw new Error('Agent retirement identity is unavailable');
  }
  const before = structuredClone(agent);
  agent.retiredAt ??= Date.now();
  agent.matrixRetirement = { state: 'pending', requestId: engagement.requestContext.requestId, mxid };
  if (!saveAgents(true)) { agents[engagement.agent] = before; throw new Error('Agent retirement fence could not be persisted'); }
  let bindingError = null;
  try {
    for (const binding of approvalStore.listBindings({ agent: agent.name })) {
      approvalStore.deactivateBinding(agent.name, binding.projectRoomId, 'allocation_revoked');
    }
  } catch (error) { bindingError = error.message; }
  const stopRequest = Object.create(req);
  stopRequest.params = { name: agent.name };
  const stopped = await stopManagedAgent(stopRequest, { status() { return this; }, json(value) { return value; } });
  let remote = null, error = bindingError;
  try { remote = await retirePalpoAgent({ engagement, mxid, transport: credential.transport, localStopped: stopped.stopped }); }
  catch (cause) { error = cause.message; }
  if (!stopped.stopped) error ||= stopped.error || 'Local process termination is unconfirmed';
  agent.matrixRetirement = { ...agent.matrixRetirement, state: error ? 'failed' : 'complete',
    localStopped: stopped.stopped === true, remote, error, checkedAt: Date.now() };
  if (!saveAgents(true)) throw new Error('Agent retirement outcome could not be persisted');
  return { binding: { keptBound: false, reason: bindingError },
    roomWithdrawal: { roomId: engagement.projectRoomId, left: Boolean(remote), reason: remote ? null : error },
    retirement: { ...agent.matrixRetirement } };
}

app.post('/api/engagements/:id/revoke', requireBearer, async (req, res) => {
  try {
    let pending = engagementRevocations.get(req.params.id);
    if (!pending) {
      pending = (async () => {
        const existing = engagementStore.get(req.params.id);
        const legacyRetirement = existing?.state === 'ended' && existing.allocatedTokens > 0 && existing.requestContext?.fleetId;
        const e = existing?.state === 'ended' && (existing.withdrawal || legacyRetirement) ? existing : engagementStore.revoke({
          engagementId: req.params.id,
          by: getRequestAgentName(req) || 'operator',
          reason: req.body?.reason,
        });
        // The decision is already durable. Retry only detachment, rechecking
        // other live allocations before removing a binding or Matrix room seat.
        const sideId = sideIdForRoom(e.projectRoomId);
        const credential = sideId && projectSideStore.credentialFor(sideId);
        const wholeAgent = e.agent && e.requestContext?.fleetId && credential?.kind === 'appservice'
          && credential.transport?.mode === 'outbound' && !hasOtherAgentAllocation(e, engagementStore.list());
        if (wholeAgent || legacyRetirement) engagementStore.beginWithdrawal(e.id, wholeAgent ? 'agent' : 'room');
        const outcome = wholeAgent ? await retireEngagementAgent(req, e, credential) : await detachEngagement(e);
        const updated = engagementStore.setWithdrawalOutcome(e.id, outcome);
        return { ok: true, engagement: updated, ...outcome };
      })();
      engagementRevocations.set(req.params.id, pending);
      pending.finally(() => engagementRevocations.delete(req.params.id)).catch(() => {});
    }
    return res.json(await pending);
  } catch (e) {
    return respondEngagementError(res, e, 'failed to revoke engagement');
  }
});

/*
 * `GET /api/offer-book?projectRoomId=…` — what a PROJECT may see before it asks.
 *
 * The gap the transparency ruling opened. Once the serving agent and its model are disclosed
 * (see the 2026-08-11 amendment to ADR-013), a borrower will reasonably want to know what is
 * on offer BEFORE committing to a request — and until now the only inbound verb was
 * `!request` itself. `/api/capability`, `/api/offers` and `/api/whitelist` all exist, but they
 * are the PROVIDER's views: every role whether offered or not, every agent including ones
 * that serve nothing, and the whitelist in full. Handing those to a project would disclose
 * the provider's whole posture, which is a different thing from disclosing what serves a role.
 *
 * So this is a separate, narrower projection rather than a relaxation of the existing three:
 *
 *   PUBLISHED OFFERS ONLY. An unpublished offer is not on offer
 *   (REQ-CONTRIBUTION-CONSOLE-ROUTE), and listing it would advertise capacity the provider
 *   has deliberately not advertised. Absent roles are omitted rather than returned as null,
 *   because a project cannot act on the difference between "not offered" and "no such role".
 *
 *   THIS ROOM'S OWN WHITELIST STATE, never the list. Whether YOUR request auto-joins is the
 *   single most actionable fact for you and yours alone; who else is trusted is not yours.
 *
 *   NO CEILINGS. The offer caps are a promise the provider published; remaining ceiling is
 *   internal state that moves with every other project's activity. A request over it still
 *   falls back to approval with `overCeiling` as the stated reason, so the fact is disclosed
 *   when it becomes relevant rather than published as a number that is stale on arrival.
 *
 * Guarded by `requireRequester`, so a project can read it with the submit-only credential —
 * the same scope that may already ask. It grants nothing new: every field is either the
 * provider's own published decision or a fact about the caller's own room.
 */
app.get('/api/offer-book', requireRequester, (req, res) => {
  const room = normalizeOptionalText(req.query.projectRoomId, 256);
  const active = engagementStore.list({ state: 'active' });
  const roles = engagementStore.listOffers()
    .filter((o) => o.published)
    .map((o) => {
      const agent = agentForRole(o.role, req.engagementCaller === 'requester' ? null : room);
      const crossFamilyOk = roleCrossFamilyAvailable(o.role, req.engagementCaller === 'requester' ? null : room);
      const resource = agent ? null : resourcesForRole(o.role, ROLE_DEFAULT_TIER[o.role], frameworkPresets)
        .find(({ preset }) => ['claude', 'codex'].includes(preset.framework) && resourceRemaining(preset) > 0)?.preset;
      const primary = resource ? runtimeProfileFromPreset(resource)?.primary : null;
      // A saved resource can supply the first agent after admission. Publish only
      // its serving configuration; the preset identity and seat state stay private.
      const serving = servingConfiguration(agent) ?? (primary ? {
        agent: null, framework: primary.framework, model: primary.model,
        reasoning: primary.reasoning ?? null, tier: presetTier(resource), provisioningRequired: true,
      } : null);
      return {
        role: o.role,
        crossFamilyOk,
        budgetCapPerEngagement: o.budgetCapPerEngagement ?? null,
        rateCap: o.rateCap ?? null,
        count: o.count ?? null,
        // Named `runningNow` rather than `used`: `count` bounds concurrent engagements, so
        // this is a live figure and not a quota consumed.
        runningNow: active.filter((e) => e.role === o.role).length,
        /*
         * Who would serve it if the request arrived now — deliberately not a reservation.
         * `agentForRole` picks by most remaining headroom, so this answer moves as other
         * projects are served, and a borrower who read it as a promise would be wrong.
         */
        serving: crossFamilyOk ? serving : null,
        resources: publicResourceConfigurations(o.role, room ? sideIdForRoom(room) : null),
      };
    });
  return res.json({
    roles,
    /*
     * Null rather than false when no room was named: false would assert that the caller's
     * room is not trusted, which is a claim about a room nobody identified.
     */
    whitelisted: room && req.engagementCaller !== 'requester' ? engagementStore.isWhitelisted(room) : null,
    projectRoomId: room || null,
  });
});

app.get('/api/offers', requireBearer, (_req, res) => {
  /*
   * Every role, whether or not it has an offer. An absent offer is a real state —
   * capacity configured but not advertised — and returning only the configured ones
   * would make "not offered" indistinguishable from "role does not exist".
   */
  const configured = new Map(engagementStore.listOffers().map((o) => [o.role, o]));
  const catalog = new Map(fleetCatalogOffers().map(o => [o.role, o]));
  return res.json({
    offers: ROLES.map((role) => ({ ...(configured.get(role) ?? {
      role, count: null, budgetCapPerEngagement: null, rateCap: null, published: false, updatedAt: null,
    }), catalogPublished: Boolean(catalog.get(role)?.resources.length) })),
  });
});

app.put('/api/offers/:role', requireBearer, (req, res) => {
  try {
    const role = normalizeOptionalText(req.params.role, 64);
    // Narrow, never invent: the project side has to recognise a role name for any
    // of this to mean anything, so a role outside the vocabulary is refused.
    if (!ROLES.includes(role)) return res.status(400).json({ error: `unknown role: ${role}` });
    const offer = engagementStore.setOffer({
      role,
      count: req.body?.count,
      budgetCapPerEngagement: req.body?.budgetCapPerEngagement,
      rateCap: req.body?.rateCap,
      published: req.body?.published,
      by: getRequestAgentName(req) || 'operator',
    });
    return res.json({ ok: true, offer });
  } catch (e) {
    return respondEngagementError(res, e, 'failed to set offer');
  }
});

app.get('/api/whitelist', requireBearer, (_req, res) => {
  return res.json({ whitelist: engagementStore.listWhitelist() });
});

app.post('/api/whitelist', requireBearer, (req, res) => {
  try {
    const entry = engagementStore.addToWhitelist({
      projectRoomId: req.body?.projectRoomId,
      displayName: req.body?.displayName,
      addedBy: req.body?.addedBy,
    });
    return res.json({ ok: true, entry });
  } catch (e) {
    return respondEngagementError(res, e, 'failed to add to whitelist');
  }
});

app.delete('/api/whitelist/:roomId', requireBearer, (req, res) => {
  try {
    const result = engagementStore.removeFromWhitelist({
      projectRoomId: req.params.roomId,
      by: getRequestAgentName(req) || 'operator',
    });
    /*
     * `stillActive` is returned rather than swallowed: removal affects future
     * requests only, so the caller has to be told what is still running under the
     * trust it just withdrew. Silently leaving them would be correct behaviour
     * reported as if nothing happened.
     */
    return res.json({ ok: true, ...result });
  } catch (e) {
    return respondEngagementError(res, e, 'failed to remove from whitelist');
  }
});

/*
 * A bearer-readable projection of the contribution binding.
 *
 * `GET /api/approval-bindings` is guarded by requireApprovalBridgeSecret — a
 * bridge-only secret — so a console holding the API token cannot read the record
 * that already carries the (agent, project, room, owner) tuple. Widening that guard
 * would hand the console the bridge's authority; this projects instead, and omits
 * `ownerDmRoomId`, which is a private channel the console has no business seeing.
 */
app.get('/api/contributions', requireBearer, (_req, res) => {
  try {
    const bindings = approvalStore.listBindings({});
    return res.json({
      contributions: bindings.map((b) => ({
        agent: b.agent,
        project: b.project,
        projectRoomId: b.projectRoomId,
        ownerMxid: b.ownerMxid,
        active: b.active !== false,
        /*
         * Three states, and the third is the point: true = membership confirmed, false = the agent
         * is NOT in a room this binding says can reach it, null = never checked. Collapsing null
         * into false would accuse every binding of being broken before the first observation.
         */
        agentJoined: b.agentJoined ?? null,
        membershipCheckedAt: b.membershipCheckedAt ?? null,
      })),
    });
  } catch (error) {
    return respondApprovalStoreError(res, error, 'failed to list contributions');
  }
});

/** What a hypothetical request WOULD do, without creating anything. */
app.get('/api/engagements/preview', requireBearer, (req, res) => {
  const role = normalizeOptionalText(req.query.role, 64);
  const projectRoomId = normalizeOptionalText(req.query.projectRoomId, 256);
  const requestedTokens = Number(req.query.requestedTokens) || 0;
  const agent = agentForRole(role, projectRoomId);
  const decision = routeRequest({
    request: { requestedTokens, ratePerDay: Number(req.query.ratePerDay) || null },
    whitelisted: engagementStore.isWhitelisted(projectRoomId),
    offer: engagementStore.getOffer(role),
    remainingTokens: agent ? remainingFor(agent, { forAutoJoin: true }) : null,
    crossFamilyOk: roleCrossFamilyAvailable(role, projectRoomId),
  });
  return res.json({ ...decision, agent, agentRemainingTokens: agent ? remainingFor(agent) : null });
});

// ── Usage ──────────────────────────────────────────────────────────────
/*
 * What each contributed agent actually did, and — stated rather than implied —
 * which of those signals is measured.
 *
 * THE PARTITION IS THE POINT. Three signals, and they are not equally real:
 *
 *   tasks     MEASURED. lib/task-store.js has five statuses and every task carries
 *             an assignee, so "what did my agent work on" is answerable.
 *   busyTime  MEASURED. The runtime sweep observes each agent's pane or ACP session
 *             and records activeDurationSec / idleDurationSec.
 *   tokens    NOT MEASURED, at any granularity. Every `usage` and `budget` match in
 *             lib/ and backend-v2.js is a CLI help string.
 *
 * WHY TOKENS CANNOT SIMPLY BE ADDED. It is not a missing field. Hagency launches a
 * coding-agent CLI and that CLI talks to the provider directly — the API traffic
 * never passes through this process, so there is no response to read a usage header
 * from. This holds in API-key mode too, which is worth saying because it is the
 * intuitive place to expect numbers and they are not there either. The two routes
 * that could work are (a) reading each framework's own session log, which is
 * per-framework and best-effort, or (b) becoming a proxy, which changes what
 * Hagency is. Both are decisions, not omissions.
 *
 * So `tokens` is null with a reason on every row, and the `metering` block below
 * declares availability per signal so a client never has to guess which of its
 * columns is a measurement. A zero here would claim a reading nobody took, and the
 * difference between "this cost me nothing" and "I cannot see what this cost me" is
 * the whole reason a contributor opens this page.
 */
/*
 * Why a framework cannot be metered, when it cannot.
 *
 * No longer a blanket statement. Hagency still never sees an API response — it launches a
 * CLI that talks to the provider directly — but the CLIs write the provider's own figures
 * to disk, and lib/metering reads them. So the reason is now per framework: Claude Code
 * and Codex record usage, octos records none.
 */
const TOKENS_UNAVAILABLE_REASON =
  'this framework writes no token accounting hagency can read: hagency launches a CLI '
  + 'that talks to the provider directly, so no API response passes through it, and this '
  + "CLI does not record the provider's figures to disk either.";

app.get('/api/usage', requireBearer, async (_req, res) => {
  refreshServerLiveness();
  const rows = serializeAgents(Object.values(agents).filter(isAgentRecord));
  const allTasks = taskStore.listTasks();
  const TASK_STATUSES = ['created', 'accepted', 'in_progress', 'blocked', 'done'];

  /*
   * Measured consumption, from the transcripts the CLIs write themselves.
   *
   * Bounded and cached (lib/metering/reader.js): a scan that opened every transcript on
   * every request would cost seconds and grow with history. Failure here degrades to
   * unavailable-with-a-reason rather than failing the endpoint — usage is a read-only
   * view, and losing the task and busy-time figures because a transcript was unreadable
   * would be the wrong trade.
   */
  let metered = null;
  let meteredError = null;
  try {
    metered = await meterFleet({ agents: rows, homeDir: homedir() });
  } catch (error) {
    meteredError = String(error?.message ?? error).slice(0, 200);
  }
  /*
   * Record what this scan saw, then answer from the LEDGER rather than the scan.
   *
   * The scan only knows what is on disk right now. The ledger knows what was ever
   * observed, which is the honest answer to "what did this agent consume" once a
   * transcript can disappear between two requests.
   */
  if (metered && !metered.cached) {
    try {
      usageLedger.record((metered.agents ?? [])
        .filter((m) => m.available)
        .map((m) => ({
          agent: m.agent,
          framework: m.framework,
          sessions: (m.files ?? []).map((f) => ({ key: f.file, totals: f.totals })),
        })));
    } catch (error) {
      // A ledger write failure must not cost the caller the figures it already has.
      meteredError = meteredError ?? `ledger write failed: ${String(error?.message ?? error).slice(0, 120)}`;
    }
  }
  const meteredByAgent = new Map((metered?.agents ?? []).map((m) => [m.agent, m]));

  const byAgent = rows.map((a) => {
    const mine = allTasks.filter((t) => t.assignee === a.name);
    const tasksByStatus = Object.fromEntries(TASK_STATUSES.map((s) => [s, 0]));
    for (const t of mine) if (tasksByStatus[t.status] !== undefined) tasksByStatus[t.status] += 1;
    const preset = a.presetId ? frameworkPresets.find((p) => p.id === a.presetId) : null;
    return {
      agent: a.name,
      framework: a.type ?? null,
      model: a.runtimeProfile?.primary?.model ?? null,
      // Measured.
      busySec: Number(a.activeDurationSec) || 0,
      idleSec: Number(a.idleDurationSec) || 0,
      activeNow: a.activeNow === true,
      lastActivitySec: a.lastTmuxActivitySec ?? null,
      tasks: mine.length,
      tasksByStatus,
      // Declared, which is knowable: I know what I promised.
      ceilingTokens: preset?.ceiling?.tokens ?? null,
      /*
       * Measured where the framework records it and the agent's workspace is known;
       * null with the specific reason otherwise. Never 0 — a zero here reads as "this
       * agent consumed nothing", which is a claim rather than an absence.
       */
      /*
       * THE AUTHORITY'S OWN HEADROOM FIGURE, published so nothing has to re-derive it.
       *
       * `remainingFor` is `ceiling - max(committed, measuredSpend)`. The console computed
       * `ceiling - committed` client-side instead, which agreed for as long as spend was
       * always null — and diverged the moment metering worked: the page advertised 9.8M
       * available on an agent an approval would refuse above 9.0M. A contributor reading a
       * number that the decision path does not use is worse than no number.
       *
       * Two figures, not one, because "0 left" and "no ceiling declared" are different
       * answers and only the first is a limit.
       */
      remainingTokens: remainingFor(a.name),
      ...(() => {
        const m = meteredByAgent.get(a.name);
        /*
         * The ledger's figure, which includes sessions whose transcripts are gone. It is
         * never smaller than the live scan, so preferring it cannot understate.
         */
        const ever = usageLedger.totalsFor(a.name);
        if (m?.available || ever) {
          const kinds = ever ? ever.totals : m.totals;
          return {
            tokensUsed: ever ? ever.total : m.total,
            /*
             * WHAT THE CEILING ACTUALLY DRAWS AGAINST, reported next to total consumption
             * rather than instead of it.
             *
             * `tokensUsed` is everything measured; `tokensDrawn` is fresh tokens only, which
             * is what enforcement uses (CEILING_KINDS, operator ruling 2026-08-12). Without
             * both, a console can only render a ceiling percentage from the total — which is
             * how an agent that had used 7% of its ceiling came to be shown at 136% of it and
             * refused every engagement.
             */
            tokensDrawn: CEILING_KINDS.reduce((n, k) => n + (Number(kinds?.[k]) || 0), 0),
            // The kinds stay apart: cache reads run several orders of magnitude above
            // fresh input, so one summed figure hides the only number that matters for
            // comparing two agents.
            tokensByKind: kinds,
            tokensSessions: ever ? ever.sessions + ever.retiredSessions : m.sessions,
            // Non-zero means a transcript reported less than it had before, so the figure
            // rests on a source that changed underneath. Surfaced, not buried.
            tokensSourceRegressions: ever?.regressions ?? 0,
            // True when the figure includes work whose transcript is no longer on disk.
            tokensFromLedger: Boolean(ever && !m?.available),
            tokensReason: null,
          };
        }
        return {
          tokensUsed: null,
          tokensDrawn: null,
          tokensByKind: null,
          tokensReason: m?.reason ?? (meteredError
            ? `metering failed: ${meteredError}`
            : TOKENS_UNAVAILABLE_REASON),
        };
      })(),
    };
  });

  return res.json({
    generatedAt: Date.now(),
    /*
     * Availability per signal, so a client renders a blank-with-a-reason rather
     * than inferring one from an absent key — an absent key is indistinguishable
     * from a zero once it has been through JSON.
     */
    metering: {
      tasks: { available: true, source: 'lib/task-store.js' },
      busyTime: { available: true, source: 'agent runtime observation (tmux pane / ACP session sweep)' },
      tokens: {
        /*
         * True when ANY agent could be metered, and the per-agent rows carry the detail.
         * A global false would deny measurements that exist; a global true would promise
         * ones that do not. REQ-CONTRIBUTION-CONSOLE-METERING-SCOPE requires it per
         * framework, which is what `frameworks` below reports.
         */
        available: (metered?.attributed ?? 0) > 0,
        source: 'lib/metering — the coding CLIs\' own transcripts, which record the '
          + "provider's reported usage rather than an estimate",
        attributed: metered?.attributed ?? 0,
        unattributed: metered?.unattributed ?? null,
        // Two distinct caveats, deliberately not merged: agents that could not be
        // attributed at all, and a scan that stopped early and therefore understates.
        reason: metered?.reason ?? (meteredError ? `metering failed: ${meteredError}` : null),
        boundsReason: metered?.boundsReason ?? null,
        frameworks: [...new Set(rows.map((r) => r.type).filter(Boolean))]
          .map((f) => meteringSupport(f)),
        computedAt: metered?.computedAt ?? null,
        cached: metered?.cached ?? null,
        /*
         * WHAT IS STILL NOT MEASURED, and what would close each gap.
         *
         * This field used to name two ways to obtain token figures at all — CLI session
         * logs, or proxying provider traffic — because at the time neither existed. The
         * first is now BUILT and is the `source` named above, so presenting it as an open
         * option made the console contradict itself: an operator read "measured, from
         * lib/metering" and, three lines lower, that getting real numbers was a pending
         * decision. Same class as a panel reporting a state it never read.
         *
         * What remains genuinely open is narrower and worth naming precisely.
         */
        /*
         * EACH GAP CARRIES A STABLE ID, so a localized console does not have to print this
         * English at a reader. The `detail` stays for non-console consumers (a CLI, a log, an
         * operator reading the raw endpoint) — it is the API's own answer and should not
         * shrink to an opaque key.
         *
         * The id exists because the console echoed these paragraphs verbatim into a Chinese
         * UI, which is the same defect as the provenance line printing `lib/task-store.js`.
         * Twice was enough to make it the API's problem rather than the page's.
         */
        remainingGaps: [
          {
            id: 'per-project',
            detail: 'per-project spend: a transcript records the directory the CLI ran in, not '
              + 'which engagement the work was for, so an agent serving two projects from one '
              + 'workdir produces one undivided total. Closing it needs an engagement-scoped '
              + 'workspace, not a better reader.',
          },
          {
            id: 'no-accounting',
            detail: 'frameworks that write no accounting (octos, codex-acp): nothing is on disk '
              + 'to read, so the only path is hagency proxying provider traffic — which changes '
              + 'what hagency is, and is a decision rather than an oversight.',
          },
          {
            id: 'file-budget',
            detail: 'a codex scan competes for its file budget with every workspace on the '
              + 'machine, because sessions are filed by date rather than by workspace. An agent '
              + 'whose transcript is not among the newest reports a bounded scan rather than a '
              + 'figure.',
          },
        ],
      },
    },
    agents: byAgent,
    /*
     * A fleet total that carries its own denominator.
     *
     * `tokensUsed` was unconditionally null here even when per-agent rows were measured,
     * which threw away a real figure. But a bare sum would be worse: with 2 of 7 agents
     * attributable it understates the fleet while looking authoritative, and nothing in the
     * number says so.
     *
     * So the numerator never travels without its denominator, and it stays null when
     * nothing at all was measured — null is "not known", 0 would be the claim that this
     * fleet consumed nothing.
     */
    totals: (() => {
      const measured = byAgent.filter((r) => typeof r.tokensUsed === 'number');
      return {
        agents: byAgent.length,
        busySec: byAgent.reduce((n, r) => n + r.busySec, 0),
        tasks: byAgent.reduce((n, r) => n + r.tasks, 0),
        tokensUsed: measured.length
          ? measured.reduce((n, r) => n + r.tokensUsed, 0)
          : null,
        // The ceiling-drawing part of the same figure, so a consumer never has to
        // re-derive it from tokensByKind and pick the wrong kinds.
        tokensDrawn: measured.length
          ? measured.reduce((n, r) => n + (r.tokensDrawn ?? 0), 0)
          : null,
        // Read these together with the figure above or not at all.
        tokensMeasuredFor: measured.length,
        tokensPartial: measured.length > 0 && measured.length < byAgent.length,
      };
    })(),
  });
});

// ── Seats ──────────────────────────────────────────────────────────────
/*
 * The unit capacity is actually bought in, and the over-subscription it makes
 * visible.
 *
 * A ceiling is declared per agent because that is the unit an operator reasons
 * about. But two Claude agents on one host share one authenticated subscription —
 * `$HOME` is never reassigned in the launch path and bin/hagency-up:1640-1644 only
 * unsets ANTHROPIC_API_KEY when no per-agent key is set — so their two ceilings are
 * two claims on one quota, not two quotas. Nothing in the product could say that
 * before this route.
 *
 * Identity is derived; quota is declared. See lib/seat-store.js for why the split
 * is that way round and why the seat id is a keyed digest rather than a path.
 */
app.get('/api/seats', requireBearer, (_req, res) => {
  refreshServerLiveness();
  const rows = Object.values(agents).filter(isAgentRecord);
  const seats = buildSeats({
    agents: rows,
    presets: frameworkPresets,
    declarations: seatDeclarations,
    keyId: SEAT_KEY_ID,
    secret: SEAT_KEY_SECRET,
  });
  return res.json({
    generatedAt: Date.now(),
    // Surfaced rather than buried: an unkeyed digest protects nothing, and a
    // caller deciding whether a seat id is safe to log needs to know which it is.
    keyed: Boolean(SEAT_KEY_SECRET),
    keyId: SEAT_KEY_ID,
    seats,
  });
});

/*
 * Declare what a seat holds.
 *
 * PUT rather than POST because the seat already exists — it is derived from the
 * agents on it. What is being created is the operator's belief about its quota, and
 * that belief is addressed by the seat's own id.
 */
app.put('/api/seats/:seatId', requireBearer, (req, res) => {
  const seatId = normalizeOptionalText(req.params.seatId, 128);
  if (!seatId) return res.status(400).json({ error: 'seatId is required' });

  // Refuse a declaration about a seat no agent occupies. Otherwise seats.json
  // accumulates beliefs about seats that never existed — usually a typo, and
  // indistinguishable afterwards from a seat whose agents were removed.
  const rows = Object.values(agents).filter(isAgentRecord);
  const known = new Set(rows.map((a) => seatIdentity(a, { keyId: SEAT_KEY_ID, secret: SEAT_KEY_SECRET }).seatId));
  if (!known.has(seatId)) {
    return res.status(404).json({ error: `no agent occupies seat ${seatId}` });
  }

  const declaration = normalizeDeclaration(req.body || {});
  if (!declaration) return res.status(400).json({ error: 'nothing to declare: quotaTokens, period or planLabel required' });

  const previous = seatDeclarations[seatId];
  seatDeclarations[seatId] = {
    ...declaration,
    declaredBy: getRequestAgentName(req) || 'operator',
    declaredAt: Date.now(),
  };
  if (!saveSeatDeclarations()) {
    if (previous === undefined) delete seatDeclarations[seatId];
    else seatDeclarations[seatId] = previous;
    return res.status(503).json({ error: 'seat declaration persistence failed' });
  }
  return res.json({ ok: true, seatId, declaration: seatDeclarations[seatId] });
});

app.delete('/api/seats/:seatId', requireBearer, (req, res) => {
  const seatId = normalizeOptionalText(req.params.seatId, 128);
  const previous = seatDeclarations[seatId];
  if (previous === undefined) return res.status(404).json({ error: 'no declaration for that seat' });
  delete seatDeclarations[seatId];
  if (!saveSeatDeclarations()) {
    seatDeclarations[seatId] = previous;
    return res.status(503).json({ error: 'seat declaration persistence failed' });
  }
  return res.json({ ok: true, seatId });
});

// ── Framework Presets CRUD ─────────────────────────────────────────────
app.get('/api/framework-presets', requireBearer, (_req, res) => {
  return res.json(frameworkPresets.map(p => ({
    ...p,
    agentDefinitions: resourceAgentDefinitions.list(p),
    apiKey: p.apiKey ? true : null,
  })));
});

function mutateResourceAgentDefinition(req, res) {
  try {
    const definition = resourceAgentDefinitions.edit(req.params.id, req.params.definitionId || null,
      req.method === 'DELETE' ? null : req.body || {});
    return res.json({ ok: true, definition });
  } catch (error) { return respondEngagementError(res, error, 'Agent definition could not be saved'); }
}
app.post('/api/framework-presets/:id/agents', requireBearer, mutateResourceAgentDefinition);
app.put('/api/framework-presets/:id/agents/:definitionId', requireBearer, mutateResourceAgentDefinition);
app.delete('/api/framework-presets/:id/agents/:definitionId', requireBearer, mutateResourceAgentDefinition);
app.put('/api/framework-presets/:id/catalog', requireBearer, (req, res) => {
  const preset = frameworkPresets.find(p => p.id === req.params.id);
  if (!preset) return res.status(404).json({ error: 'Resource not found' });
  if (typeof req.body?.published !== 'boolean') return res.status(400).json({ error: 'published must be a boolean' });
  const previous = preset.catalogPublished;
  preset.catalogPublished = req.body.published;
  if (!saveFrameworkPresets()) { preset.catalogPublished = previous; return res.status(503).json({ error: 'Publication could not be saved' }); }
  return res.json({ ok: true, published: preset.catalogPublished });
});

app.post('/api/framework-presets', requireBearer, (req, res) => {
  const b = req.body || {};
  let executionPolicy;
  try { executionPolicy = normalizeExecutionPolicy(b.executionPolicy, b.framework); }
  catch (error) { return res.status(400).json({ error: error.message }); }
  if (b.catalogPublished !== undefined && typeof b.catalogPublished !== 'boolean') {
    return res.status(400).json({ error: 'catalogPublished must be a boolean' });
  }
  const name = normalizeOptionalText(b.name, 128);
  if (!name) return res.status(400).json({ error: 'name is required' });
  let id = 'preset_' + Date.now().toString(36) + '_' + Math.random().toString(36).slice(2, 6);
  for (let i = 0; i < 10 && frameworkPresets.some(p => p.id === id); i++) {
    id = 'preset_' + Date.now().toString(36) + '_' + Math.random().toString(36).slice(2, 6);
  }
  if (frameworkPresets.some(p => p.id === id)) return res.status(500).json({ error: 'failed to generate unique preset id' });
  const extraArgs = normalizeOptionalText(b.extraArgs, 4000) || null;
  if (extraArgs && SHELL_METACHAR_RE.test(extraArgs)) {
    return res.status(400).json({ error: 'extraArgs contains disallowed shell characters' });
  }
  const apiBaseUrl = normalizeOptionalText(b.apiBaseUrl, 512) || null;
  if (apiBaseUrl) {
    try {
      const parsed = new URL(apiBaseUrl);
      if (!['http:', 'https:'].includes(parsed.protocol)) throw new Error('not http(s)');
      if (parsed.username || parsed.password) throw new Error('credentials in URL');
    } catch {
      return res.status(400).json({ error: 'apiBaseUrl must be a valid HTTP(S) URL without embedded credentials' });
    }
  }
  const preset = {
    id,
    name,
    framework: normalizeOptionalText(b.framework, 32) || null,
    provider: normalizeOptionalText(b.provider, 64) || null,
    model: normalizeOptionalText(b.model, 256) || null,
    reasoning: normalizeOptionalText(b.reasoning, 64) || null,
    catalogPublished: b.catalogPublished !== false,
    executionPolicy,
    extraArgs,
    apiBaseUrl,
    apiKey: normalizeOptionalText(b.apiKey, 256) || null,
    /*
     * The one field a contributor is really deciding, and until now the only one
     * with nowhere to live: this handler built its record from a closed field
     * list, so a `ceiling` sent by a client was accepted with 200 and dropped.
     * The console had to render every ceiling cell as "no ceiling field upstream".
     */
    ceiling: normalizeCeiling(b.ceiling),
  };
  frameworkPresets.push(preset);
  if (!saveFrameworkPresets()) {
    frameworkPresets.pop();
    return res.status(503).json({ error: 'framework preset persistence failed' });
  }
  return res.json({ ok: true, preset: { ...preset, apiKey: preset.apiKey ? true : null } });
});

app.put('/api/framework-presets/:id', requireBearer, (req, res) => {
  const idx = frameworkPresets.findIndex(p => p.id === req.params.id);
  if (idx === -1) return res.status(404).json({ error: 'preset not found' });
  const b = req.body || {};
  const name = normalizeOptionalText(b.name, 128);
  if (!name) return res.status(400).json({ error: 'name is required' });
  const extraArgs = normalizeOptionalText(b.extraArgs, 4000) || null;
  if (extraArgs && SHELL_METACHAR_RE.test(extraArgs)) {
    return res.status(400).json({ error: 'extraArgs contains disallowed shell characters' });
  }
  const apiBaseUrl = normalizeOptionalText(b.apiBaseUrl, 512) || null;
  if (apiBaseUrl) {
    try {
      const parsed = new URL(apiBaseUrl);
      if (!['http:', 'https:'].includes(parsed.protocol)) throw new Error('not http(s)');
      if (parsed.username || parsed.password) throw new Error('credentials in URL');
    } catch {
      return res.status(400).json({ error: 'apiBaseUrl must be a valid HTTP(S) URL without embedded credentials' });
    }
  }
  const previousPreset = frameworkPresets[idx];
  let executionPolicy;
  try { executionPolicy = normalizeExecutionPolicy(b.executionPolicy === undefined ? previousPreset.executionPolicy : b.executionPolicy, b.framework); }
  catch (error) { return res.status(400).json({ error: error.message }); }
  const nextPreset = {
    ...previousPreset,
    name,
    executionPolicy,
    framework: normalizeOptionalText(b.framework, 32) || null,
    provider: normalizeOptionalText(b.provider, 64) || null,
    model: normalizeOptionalText(b.model, 256) || null,
    reasoning: normalizeOptionalText(b.reasoning, 64) || null,
    extraArgs,
    apiBaseUrl,
    apiKey: normalizeOptionalText(b.apiKey, 256) || previousPreset.apiKey || null,
    /*
     * An omitted `ceiling` KEEPS the stored one, matching how apiKey behaves just
     * above. The alternative — treating absent as "clear it" — would silently
     * unset a contributor's budget every time a client saved the form without
     * re-sending it. An explicit `null` still clears.
     */
    ceiling: b.ceiling === undefined
      ? (previousPreset.ceiling ?? null)
      : normalizeCeiling(b.ceiling),
  };
  if (((previousPreset.agentDefinitions || []).some(d => resourceAgentDefinitions.inUse(d.name))
    || engagementStore.list().some(e => e.state === 'pending' && e.fulfillment?.presetId === previousPreset.id))
    && JSON.stringify(runtimeProfileFromPreset(previousPreset)) !== JSON.stringify(runtimeProfileFromPreset(nextPreset))) {
    return res.status(409).json({ error: 'This resource has provisioned or reserved Agents; create a new resource to change their runtime configuration' });
  }
  if ((nextPreset.agentDefinitions || []).some(d => !resourcesForRole(d.role, ROLE_DEFAULT_TIER[d.role], [nextPreset]).length
    || !['claude', 'codex'].includes(nextPreset.framework))) return res.status(400).json({ error: 'Resource changes would invalidate its Agent definitions' });
  frameworkPresets[idx] = nextPreset;
  if (!saveFrameworkPresets()) {
    frameworkPresets[idx] = previousPreset;
    return res.status(503).json({ error: 'framework preset persistence failed' });
  }
  return res.json({ ok: true, preset: { ...frameworkPresets[idx], apiKey: frameworkPresets[idx].apiKey ? true : null } });
});

app.delete('/api/framework-presets/:id', requireBearer, (req, res) => {
  const idx = frameworkPresets.findIndex(p => p.id === req.params.id);
  if (idx === -1) return res.status(404).json({ error: 'preset not found' });
  if (frameworkPresets[idx].agentDefinitions?.length) return res.status(409).json({ error: 'Remove unused Agent definitions before deleting their resource' });
  if (engagementStore.list().some(e => e.state === 'pending' && e.fulfillment?.presetId === req.params.id)) {
    return res.status(409).json({ error: 'This resource has a reserved Agent; finish or reject the request first' });
  }
  const removed = frameworkPresets.splice(idx, 1)[0];
  if (!saveFrameworkPresets()) {
    frameworkPresets.splice(idx, 0, removed);
    return res.status(503).json({ error: 'framework preset persistence failed' });
  }
  return res.json({ ok: true, preset: { ...removed, apiKey: removed.apiKey ? true : null } });
});

app.post('/api/task-graphs', requireBearer, (req, res) => {
  try {
    const created = taskGraphStore.createGraph(req.body || {});
    const graph = taskGraphStore.advanceGraph(created.id) || created;
    return res.json({ ok: true, graph });
  } catch (error) {
    return respondTaskGraphError(res, error, 'failed to create task graph');
  }
});

app.get('/api/task-graphs', (req, res) => {
  try {
    const status = normalizeOptionalText(req.query?.status, 32);
    return res.json(taskGraphStore.listGraphs(status ? { status } : {}));
  } catch (error) {
    return respondTaskGraphError(res, error, 'failed to list task graphs');
  }
});

app.get('/api/task-graphs/:id', (req, res) => {
  const graph = taskGraphStore.getGraph(req.params.id);
  if (!graph) return res.status(404).json({ error: 'task graph not found' });
  return res.json(graph);
});

app.delete('/api/task-graphs/:id', requireBearer, (req, res) => {
  try {
    const graph = taskGraphStore.deleteGraph(req.params.id);
    if (!graph) return res.status(404).json({ error: 'task graph not found' });
    return res.json({ ok: true, graph });
  } catch (error) {
    return respondTaskGraphError(res, error, 'failed to delete task graph');
  }
});

app.patch('/api/task-graphs/:id/nodes/:nodeId', requireAgentToken(_tokenFromNodeAssignee), (req, res) => {
  try {
    taskGraphStore.updateNode(req.params.id, req.params.nodeId, req.body || {});
    const graph = taskGraphStore.advanceGraph(req.params.id) || taskGraphStore.getGraph(req.params.id);
    if (!graph) return res.status(404).json({ error: 'task graph not found' });
    const node = taskGraphStore.getNode(req.params.id, req.params.nodeId);
    if (!node) return res.status(404).json({ error: 'task graph node not found' });
    return res.json({ ok: true, graph, node });
  } catch (error) {
    return respondTaskGraphError(res, error, 'failed to update task graph node');
  }
});

// Read-only, privacy-filtered project projection for Dashboard and future
// clients. Group membership is the project boundary; direct/approval messages
// and raw agent records never leave this endpoint.
app.get('/api/project-board', async (req, res) => {
  try {
    const agentRows = await Promise.all(
      Object.values(agents).filter(isAgentRecord).map(async agent => {
        const row = serializeAgent(agent);
        const manifest = findV1ManifestByName(row.name);
        const managedProjects = manifest?.managedProjects?.length
          ? manifest.managedProjects
          : row.managedProjects;
        const projectInspections = await Promise.all(
          normalizeManagedProjects(managedProjects).map(project =>
            projectInspector.inspectManagedProject(project, row.name)),
        );
        return { ...row, projectInspections };
      }),
    );
    const snapshot = buildProjectBoardSnapshot({
      groups,
      bindings: workflowBindings,
      agents: agentRows,
      tasks: taskStore.listTasks(),
      taskGraphs: taskGraphStore.listGraphs(),
      messages,
      staleAfterMs: PROJECT_BOARD_STALE_AFTER_MS,
      activityLimit: req.query?.activity_limit,
    });
    return res.json(snapshot);
  } catch (error) {
    console.error('[project-board] snapshot failed:', error?.message || error);
    return res.status(500).json({ error: 'project board snapshot failed' });
  }
});

// ── Alerts CRUD ───────────────────────────────────────────────────────
function respondAlertStoreError(res, error, fallbackMessage) {
  if (error.code === 'not_found') return res.status(404).json({ error: error.message });
  if (error.code === 'persistence_failed') return res.status(503).json({ error: error.message });
  if (error.code === 'bad_transition' || error.code === 'bad_request') {
    return res.status(400).json({ error: error.message });
  }
  if (error.code) return res.status(400).json({ error: error.message });
  return res.status(500).json({ error: fallbackMessage });
}

app.get('/api/alerts', requireBearer, (req, res) => {
  const filters = {};
  if (req.query.status) filters.status = req.query.status;
  if (req.query.severity) filters.severity = req.query.severity;
  if (req.query.sourceAgent) filters.sourceAgent = req.query.sourceAgent;
  if (req.query.alertType) filters.alertType = req.query.alertType;
  if (req.query.assignee) filters.assignee = req.query.assignee;
  if (req.query.limit) filters.limit = req.query.limit;
  if (req.query.offset) filters.offset = req.query.offset;
  return res.json(alertStore.listAlerts(filters));
});

app.get('/api/alerts/stats', requireBearer, (_req, res) => {
  return res.json(alertStore.getStats());
});

app.get('/api/alerts/:id', requireBearer, (req, res) => {
  const alert = alertStore.getAlert(req.params.id);
  if (!alert) return res.status(404).json({ error: 'alert not found' });
  return res.json(alert);
});

// Accept Bearer OR agent-token (agent can only transition alerts assigned to them)
const _alertTransitionAuth = (req, res, next) => {
  /*
   * The bearer branch is now REDUNDANT with `checkAgentToken`, which accepts the operator itself — and
   * it is kept because the ORDER differs, not the answer. Falling straight through to the agent check
   * would look up the alert first and answer 404 for a missing id; the operator's own call must not
   * depend on the alert existing to be authorised. Two of the three inline workarounds of this shape
   * were removed; this one earns its place.
   */
  if (!operatorBearerConfigured(process.env)) return next();
  if (hasApiTokenAccess(req)) return next();
  // Fall back to agent-token auth: agent can transition their assigned alerts
  const alert = alertStore.getAlert(req.params.id);
  if (!alert) return res.status(404).json({ error: 'alert not found' });
  const assignee = alert.assignee;
  if (!assignee) return res.status(403).json({ error: 'alert has no assignee — bearer token required' });
  const tokenResult = checkAgentToken(assignee, req);
  if (!tokenResult.ok) {
    if (AGENT_TOKEN_MODE === 'audit') { console.warn(`[auth] alert-transition agent-token ${tokenResult.reason}: agent=${assignee}`); return next(); }
    return res.status(403).json({ error: `agent token ${tokenResult.reason}` });
  }
  next();
};
app.post('/api/alerts/:id/transition', _alertTransitionAuth, (req, res) => {
  try {
    const status = (typeof req.body?.status === 'string') ? req.body.status.trim() : '';
    if (!status) return res.status(400).json({ error: 'status is required' });
    const alert = alertStore.transition(req.params.id, status, {
      actor: req.body.actor || 'operator',
      assignee: req.body.assignee || null,
      suppressUntil: req.body.suppressUntil ? Number(req.body.suppressUntil) : undefined,
    });
    return res.json({ ok: true, alert });
  } catch (error) {
    return respondAlertStoreError(res, error, 'failed to transition alert');
  }
});

app.post('/api/alerts/:id/notes', requireBearer, (req, res) => {
  try {
    const alert = alertStore.addNote(req.params.id, {
      author: req.body?.author || 'operator',
      text: req.body?.text || '',
    });
    return res.json({ ok: true, alert });
  } catch (error) {
    return respondAlertStoreError(res, error, 'failed to add note');
  }
});

app.patch('/api/alerts/:id', requireBearer, (req, res) => {
  try {
    const alert = alertStore.updateAlert(req.params.id, req.body || {});
    return res.json({ ok: true, alert });
  } catch (error) {
    return respondAlertStoreError(res, error, 'failed to update alert');
  }
});

app.delete('/api/alerts/:id', requireBearer, (req, res) => {
  try {
    const alert = alertStore.deleteAlert(req.params.id);
    if (!alert) return res.status(404).json({ error: 'alert not found' });
    broadcastSSE('alert_deleted', alert);
    return res.json({ ok: true, alert });
  } catch (error) {
    return respondAlertStoreError(res, error, 'failed to delete alert');
  }
});

// ── Groups CRUD ───────────────────────────────────────────────────────
app.post('/api/groups', requireBridgeSecret, (req, res) => {
  const { name, members } = req.body;
  const groupName = (typeof name === 'string' ? name.trim() : '');
  if (!groupName) return res.status(400).json({ error: 'name required' });
  if (groups[groupName]) return res.status(409).json({ error: 'group already exists' });
  const normalizedMembers = [];
  const seen = new Set();
  for (const raw of (Array.isArray(members) ? members : [])) {
    const memberName = normalizeAgentName(raw);
    if (!memberName) continue;
    const key = memberName.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    normalizedMembers.push(memberName);
  }
  const groupRecord = { name: groupName, members: normalizedMembers, createdAt: Date.now() };
  groups[groupName] = groupRecord;
  if (!saveGroups()) {
    delete groups[groupName];
    return res.status(503).json({ error: 'group persistence failed' });
  }
  broadcastSSE('group_created', groupRecord);
  res.json({ ok: true, group: groupRecord });
});

app.get('/api/groups', (_req, res) => {
  res.json(Object.values(groups));
});

app.get('/api/groups/:name', (req, res) => {
  const group = groups[req.params.name];
  if (!group) return res.status(404).json({ error: 'group not found' });
  res.json(group);
});

app.post('/api/groups/:name/members', requireBridgeSecret, (req, res) => {
  const group = groups[req.params.name];
  if (!group) return res.status(404).json({ error: 'group not found' });
  const matrixObservation = req.body.source === 'matrix';
  if (matrixObservation && !getBridgeSecret()) {
    return res.status(503).json({ error: 'MATRIX_BRIDGE_SECRET is required for Matrix membership observations' });
  }
  const { add, remove } = req.body;
  const addList = [];
  const addSeen = new Set();
  if (Array.isArray(add)) {
    for (const raw of add) {
      const memberName = normalizeAgentName(raw);
      if (!memberName) continue;
      const key = memberName.toLowerCase();
      if (addSeen.has(key)) continue;
      addSeen.add(key);
      addList.push(memberName);
    }
  }
  const removeKeys = new Set();
  const removeList = [];
  const removeSeen = new Set();
  if (Array.isArray(remove)) {
    for (const raw of remove) {
      const memberName = normalizeAgentName(raw);
      if (!memberName) continue;
      const key = memberName.toLowerCase();
      if (removeSeen.has(key)) continue;
      removeSeen.add(key);
      removeKeys.add(key);
      removeList.push(memberName);
    }
  }

  const nextMembers = Array.isArray(group.members) ? [...group.members] : [];
  const existingKeys = new Set(nextMembers.map(m => String(m).toLowerCase()));
  for (const memberName of addList) {
    const key = memberName.toLowerCase();
    if (!existingKeys.has(key)) {
      nextMembers.push(memberName);
      existingKeys.add(key);
    }
  }
  if (removeKeys.size > 0) {
    for (let i = nextMembers.length - 1; i >= 0; i--) {
      if (removeKeys.has(String(nextMembers[i]).toLowerCase())) nextMembers.splice(i, 1);
    }
  }
  const nextGroup = { ...group, members: nextMembers };
  groups[req.params.name] = nextGroup;
  if (!saveGroups()) {
    groups[req.params.name] = group;
    return res.status(503).json({ error: 'group persistence failed' });
  }
  broadcastSSE('group_members', {
    name: nextGroup.name, members: nextGroup.members, added: addList, removed: removeList,
    ...(matrixObservation ? { source: 'matrix' } : {}),
  });
  res.json({ ok: true, group: nextGroup });
});

app.delete('/api/groups/:name', requireBridgeSecret, (req, res) => {
  const previousGroup = groups[req.params.name];
  if (!previousGroup) return res.status(404).json({ error: 'group not found' });
  delete groups[req.params.name];
  if (!saveGroups()) {
    groups[req.params.name] = previousGroup;
    return res.status(503).json({ error: 'group persistence failed' });
  }
  res.json({ ok: true });
});

// ── DM ensure (triggers bridge to create Matrix DM room) ─────────────
async function authorizeDirectRoom(agentName, humanMxid, roomId) {
  const agent = agents[agentName];
  return resolveDirectAdmission({ agent, humanMxid, roomId,
    existing: routerStore.conversations.direct(roomId, agentName),
    engagements: engagementStore.list({ state: 'active' }),
    bindings: approvalStore.listBindings({ agent: agentName }),
    eligible: agentEligibleForRoom(agent, roomId),
    membersForProject: async projectRoomId => {
      const side = projectSideStore.getSide(sideIdForRoom(projectRoomId));
      const credential = side && projectSideStore.credentialFor(side.id || side.serverName);
      if (!side?.active || !credential) return { known: false, members: [] };
      return joinedMembersOnSide({ side, credential, roomId: projectRoomId });
    },
  });
}

app.post('/api/matrix/direct-rooms', requireBridgeSecret, async (req, res) => {
  try {
    const { agent, humanMxid, roomId, mode, sinceTs } = req.body;
    if (mode !== undefined && !['direct', 'group'].includes(mode)
      || sinceTs !== undefined && (!Number.isSafeInteger(sinceTs) || sinceTs < 0)) return res.status(400).json({ error: 'invalid room mode or history boundary' });
    const binding = await authorizeDirectRoom(agent, humanMxid, roomId);
    if (mode === 'group') routerStore.conversations.promoteRoom(roomId, sinceTs ?? Date.now());
    return res.json({ ok: true, binding: routerStore.conversations.bindDirect({ ...binding, mode, sinceTs }) });
  } catch (error) {
    return res.status(String(error.message).endsWith('_unavailable') ? 503 : 403).json({ error: error.message });
  }
});

app.get('/api/matrix/direct-agents', requireBridgeSecret, (_req, res) => {
  const active = new Set(engagementStore.list({ state: 'active' }).filter(e => !e.endedAt).map(e => e.agent));
  res.json({ agents: [...active].filter(name => agentEligibleForRoom(agents[name])) });
});

app.post('/api/matrix/conversations/admissions', requireBridgeSecret, (req, res) => {
  const { roomId, admissions } = req.body || {};
  if (!Array.isArray(admissions) || !admissions.length || admissions.length > 100) return res.status(400).json({ error: 'invalid admissions' });
  const bindings = approvalStore.listBindings({ projectRoomId: roomId });
  if (!roomId || admissions.some(row => !bindings.some(binding => binding.agent === row.agent && binding.projectRoomId === roomId)
    || !Number.isSafeInteger(row.sinceTs) || row.sinceTs < 0)) return res.status(403).json({ error: 'room admission is not bound' });
  for (const row of admissions) routerStore.conversations.setAdmission(roomId, row.agent, row.sinceTs);
  return res.json({ ok: true });
});

app.post('/api/matrix/conversations/events', requireBridgeSecret, async (req, res) => {
  try {
    const input = req.body;
    const invited = routerStore.conversations.roomBindings(input.roomId);
    const bindings = approvalStore.listBindings({ projectRoomId: input.roomId });
    let admitted = !invited.length && bindings.some(b => b.active !== false && b.projectRoomId === input.roomId);
    for (const binding of invited) {
      if (recordedAgentMxid(agents[binding.agent]) === input.senderMxid) { admitted = true; break; }
      try { await authorizeDirectRoom(binding.agent, input.senderMxid, input.roomId); admitted = true; break; }
      catch { /* Another binding may authorize this sender. */ }
    }
    if (!admitted) {
      return res.status(403).json({ error: 'conversation room is not admitted' });
    }
    let attachment;
    if (input.attachment?.errorCode) {
      const errors = { file_too_large: 'Attachment exceeds 20 MiB', invalid_media: 'Matrix attachment has no valid media URI',
        media_unavailable: 'Matrix attachment is no longer available', file_decryption_failed: 'Attachment integrity or decryption failed' };
      if (!errors[input.attachment.errorCode]) return res.status(400).json({ error: 'invalid attachment failure' });
      attachment = { name: String(input.attachment.name || 'attachment').slice(0, 240),
        errorCode: input.attachment.errorCode, error: errors[input.attachment.errorCode] };
    } else if (input.attachment?.remoteContent) {
      const c = input.attachment.remoteContent;
      if (!['m.file', 'm.image'].includes(c.msgtype) || JSON.stringify(c).length > 32_768) return res.status(400).json({ error: 'invalid attachment metadata' });
      attachment = { name: String(c.filename || c.body || 'attachment').slice(0, 240),
        mime: String(c.info?.mimetype || 'application/octet-stream').slice(0, 255), size: Number(c.info?.size) || null,
        remoteContent: { msgtype: c.msgtype, body: c.body, filename: c.filename, url: c.url, file: c.file, info: c.info } };
    } else if (input.attachment) {
      attachment = snapshotSessionFile({ workspace: MATRIX_MEDIA_DIR, requestedPath: input.attachment.path,
        name: input.attachment.name, directory: MATRIX_MEDIA_DIR });
      if (attachment.sha256 !== input.attachment.sha256 || attachment.size !== input.attachment.size) {
        return res.status(400).json({ error: 'incoming file integrity mismatch' });
      }
    }
    return res.json({ ok: true, ...routerStore.conversations.archive({ ...input, attachment }) });
  } catch (error) { return res.status(400).json({ error: error.message }); }
});

app.post('/api/router/conversation', requireAgentToken(_tokenFromBody), (req, res) => {
  const owned = requireOwnedRunnerDescriptor(req, res);
  if (!owned) return;
  const result = routerStore.readConversation(owned.capability, req.body.offset ?? 0);
  return res.status(result.ok ? 200 : routerRefusalStatus(result)).json(result);
});

app.post('/api/dm/ensure', requireBearer, (req, res) => {
  const { agent, human, humanId } = req.body;
  if (!agent || !human) return res.status(400).json({ error: 'agent and human required' });
  const resolvedHumanId = (typeof humanId === 'string' && humanId.trim()) ? humanId.trim() : null;
  broadcastSSE('dm_ensure', { agent, human, humanId: resolvedHumanId });
  console.log(`[dm/ensure] Requested DM room: agent=${agent}, human=${human}${resolvedHumanId ? ` humanId=${resolvedHumanId}` : ''}`);
  res.json({ ok: true, queued: true, agent, human, humanId: resolvedHumanId });
});

app.post('/api/agents/:name/avatar', express.json({ limit: '10mb' }), requireAgentToken(_tokenFromName), (req, res) => {
  const name = req.params.name;
  if (!/^[\w\-]+$/.test(name)) return res.status(400).json({ error: 'invalid agent name' });
  const force = req.body?.generate === true || req.query.force === 'true';
  const image = req.body?.image; // base64 encoded image
  const mime = req.body?.mime || 'image/png';
  broadcastSSE('agent_avatar', { name, force, image, mime });
  console.log(`[avatar] Requested avatar ${force ? 'regeneration' : (image ? 'custom upload' : 'ensure')} for: ${name}`);
  res.json({ ok: true, queued: true, name, force, custom: !!image });
});

// ── System info (log-only; does not enter message store) ──────────────
app.post('/api/system/info', requireBridgeSecret, (req, res) => {
  const summary = (typeof req.body?.summary === 'string') ? req.body.summary.trim() : '';
  const full = (typeof req.body?.full === 'string') ? req.body.full : '';
  if (!summary) return res.status(400).json({ error: 'summary required' });
  const alertType = (typeof req.body?.alertType === 'string') ? req.body.alertType.trim() : null;
  const dedupeKey = (typeof req.body?.dedupeKey === 'string') ? req.body.dedupeKey.trim() : undefined;
  const sourceAgent = (typeof req.body?.sourceAgent === 'string') ? req.body.sourceAgent.trim() : undefined;
  const opts = {};
  if (dedupeKey) opts.dedupeKey = dedupeKey;
  if (sourceAgent) opts.sourceAgent = sourceAgent;
  for (const key of ALERT_ACTION_FIELD_KEYS) {
    if (key === 'correlation') {
      if (req.body?.correlation && typeof req.body.correlation === 'object' && !Array.isArray(req.body.correlation)) {
        opts.correlation = req.body.correlation;
      }
      continue;
    }
    const value = normalizeOptionalText(req.body?.[key], key === 'runbook' ? 512 : 1024);
    if (value) opts[key] = value;
  }
  opts.source = 'bridge';
  const event = emitSystemInfo(summary, full, alertType || null, opts);
  res.json({ ok: true, id: event.id });
});

// ── Media staging for agent attachments ───────────────────────────────
app.post('/api/media/stage', express.json({ limit: MESSAGE_ATTACHMENT_STAGE_JSON_LIMIT }), requireAgentToken(_tokenFromBody), (req, res) => {
  const fromName = normalizeAgentName(req.body?.from || '');
  if (!fromName) return res.status(400).json({ error: 'from required' });
  if (!isAgentRecord(agents[fromName])) return res.status(404).json({ error: `agent not found: ${fromName}` });

  const contentBase64 = (typeof req.body?.content_base64 === 'string') ? req.body.content_base64.trim() : '';
  if (!contentBase64) return res.status(400).json({ error: 'content_base64 required' });

  let bytes;
  try {
    bytes = Buffer.from(contentBase64, 'base64');
  } catch {
    return res.status(400).json({ error: 'invalid base64 payload' });
  }
  if (!bytes || bytes.length === 0) return res.status(400).json({ error: 'empty attachment payload' });
  if (bytes.length > MESSAGE_ATTACHMENT_MAX_BYTES) {
    return res.status(413).json({ error: `attachment exceeds max bytes (${MESSAGE_ATTACHMENT_MAX_BYTES})` });
  }

  const sourcePath = (typeof req.body?.source_path === 'string' && req.body.source_path.trim())
    ? req.body.source_path.trim()
    : '';
  const requestedName = (typeof req.body?.name === 'string' && req.body.name.trim())
    ? req.body.name.trim()
    : (sourcePath ? path.basename(sourcePath) : 'file.bin');
  const name = normalizeAttachmentName(requestedName, 'file.bin');
  const ext = path.extname(name) || '.bin';
  const fileName = `${Date.now()}-${Math.random().toString(36).slice(2, 10)}${ext}`;
  const filePath = path.join(MESSAGE_ATTACHMENT_DIR, fileName);
  writeFileSync(filePath, bytes);

  const mime = normalizeAttachmentMime(req.body?.mime);
  const kind = inferAttachmentKind(req.body?.kind, mime, name);
  const attachment = {
    path: filePath,
    name,
    mime,
    kind,
    size: bytes.length,
    staged: true,
    source_path: sourcePath || null,
  };
  res.json({ ok: true, attachment });
});

app.get('/api/media/fetch', (req, res) => {
  const resolved = resolveReadableMediaPath(req.query?.path);
  if (resolved.error) {
    return res.status(resolved.status || 400).json({ error: resolved.error });
  }

  const filePath = resolved.value.path;
  const fileName = normalizeAttachmentName(path.basename(filePath), 'file.bin');
  const mime = guessMimeFromPath(filePath);
  let bytes;
  try {
    bytes = readFileSync(filePath);
  } catch (e) {
    return res.status(500).json({ error: `failed to read file: ${e.message}` });
  }
  if (!bytes || bytes.length === 0) return res.status(400).json({ error: 'file is empty' });
  if (bytes.length > MESSAGE_ATTACHMENT_MAX_BYTES) {
    return res.status(413).json({ error: `file exceeds max bytes (${MESSAGE_ATTACHMENT_MAX_BYTES})` });
  }

  const encodedName = encodeURIComponent(fileName);
  res.setHeader('Content-Type', mime);
  res.setHeader('Content-Length', String(bytes.length));
  res.setHeader('Content-Disposition', `inline; filename="${fileName}"; filename*=UTF-8''${encodedName}`);
  return res.send(bytes);
});

// ── Messages ──────────────────────────────────────────────────────────
app.post('/api/messages', requireAgentToken(_tokenFromBody), async (req, res) => {
  const { from, to, group, type, summary, full, mentions, reply_to, source, target_type, source_room, source_event_id, thread_root_event_id, matrix_default_recipient, attachments, schema, priority, sender_mxid, from_id, incidental } = req.body;
  const fromName = normalizeAgentName(from) || from;
  const toName = to ? normalizeAgentName(to) : null;
  const sourceType = typeof source === 'string' ? source.trim().toLowerCase() : 'api';
  const targetType = typeof target_type === 'string' ? target_type.trim().toLowerCase() : 'auto';
  const sourceRoom = (typeof source_room === 'string' && source_room.trim() && source_room.length <= 255)
    ? source_room.trim()
    : null;
  // Matrix worker ingestion is accepted only through the authenticated bridge.
  const bridgeSecret = getBridgeSecret();
  const isBridgeAuthenticated = !bridgeSecret || req.headers['x-bridge-secret'] === bridgeSecret;
  const hasAuthenticatedBridgeSecret = Boolean(bridgeSecret)
    && req.headers['x-bridge-secret'] === bridgeSecret;
  let sourceEventId = null;
  let threadRootEventId = null;
  if (sourceType === 'matrix') {
    if (!bridgeSecret) {
      return res.status(503).json({ error: 'MATRIX_BRIDGE_SECRET is required for Matrix ingestion' });
    }
    if (!hasAuthenticatedBridgeSecret) {
      return res.status(401).json({ error: 'invalid Matrix bridge credentials' });
    }
    if (typeof source_event_id !== 'string' || !source_event_id.trim() || source_event_id.trim().length > 255) {
      return res.status(400).json({ error: 'source_event_id must be 1..255 characters' });
    }
    sourceEventId = source_event_id.trim();
    if (thread_root_event_id !== undefined && thread_root_event_id !== null && thread_root_event_id !== '') {
      if (typeof thread_root_event_id !== 'string' || !thread_root_event_id.trim() || thread_root_event_id.trim().length > 255) {
        return res.status(400).json({ error: 'thread_root_event_id must be 1..255 characters' });
      }
      threadRootEventId = thread_root_event_id.trim();
    }
  } else if (thread_root_event_id !== undefined) {
    return res.status(400).json({ error: 'thread_root_event_id is reserved for authenticated Matrix ingestion' });
  }
  const matrixDefaultRecipient = hasAuthenticatedBridgeSecret
    && sourceType === 'matrix'
    && matrix_default_recipient === 'wf_coordinator'
    ? 'wf_coordinator'
    : null;
  if (matrix_default_recipient !== undefined && !matrixDefaultRecipient) {
    return res.status(400).json({ error: 'invalid matrix_default_recipient' });
  }
  const senderMxid = isBridgeAuthenticated && sourceType === 'matrix' && typeof sender_mxid === 'string' && /^@[^:]+:.+/.test(sender_mxid.trim())
    ? sender_mxid.trim().slice(0, 255) : null;
  let matrixRoomRecipients;
  if (req.body.room_agent_targets !== undefined) {
    const targets = req.body.room_agent_targets;
    if (!hasAuthenticatedBridgeSecret || sourceType !== 'matrix' || !Array.isArray(targets) || !targets.length || targets.length > 32
      || targets.some(name => typeof name !== 'string') || !targets.includes(toName)) return res.status(400).json({ error: 'invalid invited-room recipients' });
    matrixRoomRecipients = [...new Set(targets)];
    for (const name of matrixRoomRecipients) {
      const binding = routerStore.conversations.direct(source_room, name);
      if (!binding || binding.mode === 'group' && !mentions?.includes(name)) return res.status(403).json({ error: 'room recipient was not admitted and mentioned' });
      try { await authorizeDirectRoom(name, senderMxid, source_room); }
      catch { return res.status(403).json({ error: 'room recipient is no longer authorized' }); }
    }
  }
  // Derive trustLevel server-side from validated senderMxid — never trust caller-supplied value
  const trustLevel = senderMxid ? (MATRIX_OPERATOR_MXIDS.has(senderMxid) ? 'operator' : 'external') : null;
  // Normalize literal \n (two chars) to actual newlines — some agents double-escape them
  const normNl = s => s.replace(/\\n/g, '\n');
  const rawSummary = typeof summary === 'string' ? normNl(summary) : '';
  const rawFull = typeof full === 'string' ? normNl(full) : '';
  const isHumanMessage = type === 'human';
  const canonicalHumanFull = isHumanMessage ? (rawFull || rawSummary).trim() : '';
  const canonicalSummary = isHumanMessage ? makeHumanSummaryPreview(canonicalHumanFull) : rawSummary;
  const canonicalFull = isHumanMessage ? canonicalHumanFull : rawFull;
  const rawAttachments = Array.isArray(attachments) ? attachments : [];
  if (rawAttachments.length > MESSAGE_ATTACHMENT_MAX_ITEMS) {
    return res.status(400).json({ error: `too many attachments (max ${MESSAGE_ATTACHMENT_MAX_ITEMS})` });
  }
  const normalizedAttachments = [];
  for (let i = 0; i < rawAttachments.length; i++) {
    const normalized = normalizeAttachmentInput(rawAttachments[i]);
    if (normalized.error) {
      return res.status(400).json({ error: `attachments[${i}]: ${normalized.error}` });
    }
    normalizedAttachments.push(normalized.value);
  }
  const normalizedSchema = normalizeMessageSchema(schema);
  if (normalizedSchema.error) {
    return res.status(400).json({ error: normalizedSchema.error });
  }
  const normalizedPriority = normalizeMessagePriority(priority);
  if (!normalizedPriority) {
    return res.status(400).json({ error: 'priority must be one of: normal, high, urgent' });
  }

  if (!fromName) return res.status(400).json({ error: 'from required' });
  if (!toName && !group) return res.status(400).json({ error: 'to or group required' });
  if (toName && group) return res.status(400).json({ error: 'to and group are mutually exclusive' });
  if (!type) return res.status(400).json({ error: 'type required' });
  if (isHumanMessage && !canonicalFull) {
    return res.status(400).json({ error: 'human message requires summary or full' });
  }
  if (!isHumanMessage && !canonicalSummary) {
    return res.status(400).json({ error: 'summary required' });
  }
  if (!['auto', 'agent', 'human'].includes(targetType)) {
    return res.status(400).json({ error: 'target_type must be one of: auto, agent, human' });
  }
  if (sourceEventId) {
    const existing = matrixDispatchStore.get(sourceEventId);
    if (existing) {
      try {
        const completed = await completeMatrixDispatch(existing);
        if (!completed.ok) {
          return res.status(completed.status || 503).json({ error: completed.error || 'Matrix dispatch recovery failed' });
        }
        return res.json({ ...completed.response, ok: true, id: existing.messageId, deduped: true });
      } catch (error) {
        return res.status(503).json({ error: `Matrix dispatch recovery failed: ${error.message}` });
      }
    }
  }
  let directTargetKind = null;
  let assumedHumanTarget = false;
  const senderRecord = agents[fromName] || null;
  const senderIsAgent = isAgentRecord(senderRecord);
  if (senderIsAgent) {
    const block = getAgentInboxGateBlock(fromName);
    if (block) {
      return res.status(409).json(block);
    }
  }
  if (sourceType === 'api' && fromName !== 'system' && !senderIsAgent) {
    return res.status(403).json({ error: `sender agent not registered: ${fromName}` });
  }
  if (toName) {
    const targetRecord = agents[toName];
    const knownAgentTarget = isAgentRecord(targetRecord);
    if (targetType === 'agent') {
      if (!knownAgentTarget) return res.status(404).json({ error: `target agent not found: ${toName}` });
      directTargetKind = 'agent';
    } else if (targetType === 'human') {
      directTargetKind = 'human';
    } else if (knownAgentTarget) {
      directTargetKind = 'agent';
    } else if (sourceType === 'matrix') {
      return res.status(404).json({ error: `target agent not found: ${toName}` });
    } else {
      directTargetKind = 'human';
      assumedHumanTarget = targetType === 'auto';
    }
  }
  if (group && !groups[group]) {
    if (group === 'info') {
      if (!ensureInfoGroup()) return res.status(503).json({ error: 'group persistence failed' });
    } else {
      return res.status(404).json({ error: `group not found: ${group}` });
    }
  }
  if (group && senderIsAgent && fromName !== 'system') {
    const matchedMember = findGroupMember(group, fromName);
    if (!matchedMember) {
      return res.status(403).json({ error: `sender '${fromName}' is not a member of group '${group}'` });
    }
    if (matchedMember !== fromName) {
      const members = getGroupMembers(group);
      const idx = members.indexOf(matchedMember);
      if (idx >= 0) {
        members[idx] = fromName;
        saveGroups();
      }
    }
  }
  refreshServerLiveness();

  // Auto-extract @mentions from text and merge with explicit mentions.
  // Resolve mentions case-insensitively to canonical stored names.
  const knownNameMap = new Map();
  const rememberKnownName = (raw) => {
    if (typeof raw !== 'string') return;
    const name = raw.trim();
    if (!name) return;
    const key = name.toLowerCase();
    if (!knownNameMap.has(key)) knownNameMap.set(key, name);
  };
  for (const agentName of Object.keys(agents)) rememberKnownName(agentName);
  for (const g of Object.values(groups)) {
    for (const m of (Array.isArray(g?.members) ? g.members : [])) rememberKnownName(m);
  }
  const resolveKnownName = (raw) => {
    if (typeof raw !== 'string') return null;
    const key = raw.trim().toLowerCase();
    if (!key) return null;
    return knownNameMap.get(key) || null;
  };
  const explicitMentions = Array.isArray(mentions) ? mentions : [];
  const textMentions = new Set();
  for (const explicit of explicitMentions) {
    const canonical = resolveKnownName(explicit) || (typeof explicit === 'string' ? explicit.trim() : '');
    if (canonical && canonical !== fromName) textMentions.add(canonical);
  }
  const mentionRegex = /@([a-zA-Z0-9_-]+)/g;
  const mentionScanTexts = isHumanMessage ? [canonicalFull] : [canonicalSummary || '', canonicalFull || ''];
  for (const text of mentionScanTexts) {
    mentionRegex.lastIndex = 0;
    let match;
    while ((match = mentionRegex.exec(text)) !== null) {
      const canonical = resolveKnownName(match[1]);
      if (canonical && canonical !== fromName) textMentions.add(canonical);
    }
  }

  const idReservation = reserveNextMsgId();
  if (!idReservation.ok) {
    return res.status(503).json({ error: idReservation.error || 'message id reservation failed' });
  }

  const msg = {
    id: idReservation.id,
    ts: Date.now(),
    from: fromName,
    to: toName || null,
    group: group || null,
    type,
    priority: normalizedPriority,
    summary: canonicalSummary,
    full: canonicalFull,
    mentions: [...textMentions],
    ...(matrixRoomRecipients ? { matrixRoomRecipients } : {}),
    reply_to: reply_to || null,
    /*
     * INCIDENTAL: this message exists for whoever is waiting on a specific message, and for nobody
     * else in the room. Progress reports are the case — 「在多人的房间…agent 老说话，把其他人的对话
     * 都冲了」 — and the operator is right that a step-by-step feed in a shared room is worse than
     * silence, because silence at least does not cost other people their conversation.
     *
     * ACCEPTED FROM ANY CALLER, unlike `thread_root_event_id` above, and the difference is the whole
     * reason this is a separate field rather than a reuse of that one. `thread_root_event_id` NAMES a
     * thread, so an agent that could set it could drop output into any conversation in the room; it is
     * restricted to authenticated Matrix ingestion and must stay that way. This grants nothing: it
     * asks for LESS reach than the reply it accompanies — same target event, thread instead of room.
     * A caller who lies about it makes its own message quieter.
     */
    incidental: incidental === true,
    source: source || 'api',
    sourceRoom,
    sourceEventId,
    matrixDefaultRecipient,
    senderMxid,
    trustLevel,
    fromId: isBridgeAuthenticated && (typeof from_id === 'string' && from_id.trim()) ? from_id.trim().slice(0, 255)
      : (senderMxid || null),
    viewToken: createMessageViewToken(),
  };
  if (sourceType === 'matrix' && sourceRoom && sourceEventId) {
    msg.matrixContext = {
      roomId: sourceRoom,
      eventId: sourceEventId,
      threadRootEventId,
    };
  }
  if (senderIsAgent && msg.reply_to) {
    const repliedTo = messages.find((candidate) => candidate?.id === msg.reply_to);
    const sourceContext = repliedTo?.source === 'matrix'
      ? repliedTo?.matrixContext
      : repliedTo?.matrixDelivery;
    const eventId = repliedTo?.source === 'matrix'
      ? sourceContext?.eventId
      : sourceContext?.primaryEventId;
    // Derived only from backend-owned history. It is a routing hint, not authority: the bridge
    // still loads the replied-to record and verifies group + room before sending.
    if (repliedTo?.group === msg.group
      && typeof sourceContext?.roomId === 'string' && sourceContext.roomId
      && typeof eventId === 'string' && eventId) {
      msg.replyContext = {
        roomId: sourceContext.roomId,
        eventId,
        threadRootEventId: sourceContext.threadRootEventId || null,
      };
    }
  }
  if (normalizedAttachments.length > 0) {
    msg.attachments = normalizedAttachments;
  }
  if (normalizedSchema.value) {
    msg.schema = normalizedSchema.value;
  }

  const warnings = [];
  const notices = [];
  if (msg.to && directTargetKind === 'human' && assumedHumanTarget) {
    notices.push({
      code: 'target_classified_human',
      target: msg.to,
      reason: 'unknown-target-treated-as-human',
    });
  }
  const suppressedRecipients = new Set();
  // A router-served agent has no tmux pane and never will; its reachability
  // comes from disposable runners, so tmux-based offline warnings are noise.
  const isRouterServedAgent = (name) => {
    if (!THREAD_SESSIONS_ENABLED) return false;
    const agent = agents[name];
    return isAgentRecord(agent) && Boolean(agent.agentId) && threadSessionAgentEligibility(agent).ok === true;
  };
  if (msg.to && directTargetKind === 'agent' && !isRouterServedAgent(msg.to)) {
    const state = getAgentDeliveryState(msg.to);
    if (!state.online) {
      warnings.push({
        code: 'target_offline',
        target: msg.to,
        server: state.server,
        reason: state.offlineReason || 'offline',
        queued: true,
      });
    }
  }
  if (msg.group && msg.mentions.length > 0) {
    const groupMemberSet = new Set(getGroupMembers(msg.group).map(n => n.toLowerCase()));
    const mentionStates = msg.mentions
      .filter(name => name !== msg.from)
      .map(name => ({
        name,
        state: getAgentDeliveryState(name),
        isGroupMember: groupMemberSet.has(String(name).toLowerCase()),
      }));

    const offlineMentions = mentionStates
      .filter(item => item.state.exists && !item.state.online && !isRouterServedAgent(item.name))
      .map(item => ({
        target: item.name,
        server: item.state.server,
        reason: item.state.offlineReason || 'offline',
      }));
    if (offlineMentions.length) {
      warnings.push({ code: 'mentions_offline', targets: offlineMentions });
    }

    const unknownMentions = mentionStates
      .filter(item => !item.state.exists && !item.isGroupMember)
      .map(item => ({ target: item.name, reason: 'not-found' }));
    if (unknownMentions.length) {
      warnings.push({ code: 'mentions_unknown', targets: unknownMentions });
    }

    const outOfGroupMentions = mentionStates
      .filter(item => item.state.exists && !item.isGroupMember && item.name !== matrixDefaultRecipient)
      .map(item => ({ target: item.name, reason: 'not-in-group' }));
    if (outOfGroupMentions.length) {
      warnings.push({ code: 'mentions_not_in_group', targets: outOfGroupMentions });
      for (const item of outOfGroupMentions) suppressedRecipients.add(item.target);
    }
  }
  if (suppressedRecipients.size > 0) {
    msg.suppressedRecipients = [...suppressedRecipients];
  }

  const responseBase = {
    ok: true,
    id: msg.id,
    warnings,
    notices,
    delivery: { suppressed: msg.suppressedRecipients || [], targetKind: directTargetKind || null },
    taskGraph: null,
  };
  let dispatchResult;
  if (sourceEventId) {
    try {
      const receipt = matrixDispatchStore.reserve({
        eventId: sourceEventId,
        messageId: msg.id,
        message: msg,
        dispatch: { senderIsAgent, directTargetKind },
        response: responseBase,
      });
      dispatchResult = await completeMatrixDispatch(receipt);
    } catch (error) {
      dispatchResult = { ok: false, status: 503, error: `Matrix dispatch persistence failed: ${error.message}` };
    }
  } else {
    dispatchResult = dispatchStoredMessage(msg, { senderIsAgent, directTargetKind });
  }
  if (!dispatchResult.ok) {
    return res.status(dispatchResult.status || 503).json({ error: dispatchResult.error || 'message persistence failed' });
  }
  let taskGraph = null;
  try {
    taskGraph = handleTaskGraphMessageHook(msg);
  } catch (error) {
    return res.status(taskGraphErrorStatus(error)).json({
      error: error?.message || 'task graph hook failed',
      id: msg.id,
      messageAccepted: true,
      warnings,
      notices,
      delivery: { suppressed: msg.suppressedRecipients || [], targetKind: directTargetKind || null },
      taskGraph: null,
    });
  }

  res.json({ ...responseBase, taskGraph });
});

// ── DM history (bearer-authenticated, for web UI) ─────────────────────
app.get('/api/dm/:agent/history', requireBearer, (req, res) => {
  const agentName = normalizeAgentName(req.params.agent);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const limitRaw = Number.parseInt(req.query.limit, 10);
  const limit = Number.isFinite(limitRaw) && limitRaw > 0 ? Math.min(limitRaw, 500) : 100;
  const beforeRaw = Number.parseInt(req.query.before, 10);
  const before = Number.isFinite(beforeRaw) && beforeRaw > 0 ? beforeRaw : Infinity;
  // Return DMs involving this agent (sent to or from, excluding group messages)
  const dms = messages
    .filter(m => !m.group && (m.to === agentName || m.from === agentName) && m.ts < before)
    .sort((a, b) => a.ts - b.ts);
  const rows = dms.slice(-limit);
  res.json({
    agent: agentName,
    total: dms.length,
    returned: rows.length,
    messages: rows.map(summarizeMsg),
  });
});

app.get('/api/messages/:id', (req, res) => {
  const msg = messages.find(m => m.id === req.params.id);
  if (!msg) return res.status(404).json({ error: 'message not found' });
  const auth = authorizeMessageDetailAccess(req, msg);
  if (!auth.ok) return res.status(auth.status || 403).json({ error: auth.error || 'message access denied' });
  const normalizedSchema = normalizeMessageSchema(msg?.schema);
  res.json({
    ...msg,
    priority: normalizeMessagePriority(msg?.priority),
    schema: normalizedSchema.value || undefined,
    ts: undefined,
    time: relativeTime(msg.ts),
  });
});

app.put('/api/messages/:id/matrix-delivery', requireBridgeSecret, (req, res) => {
  if (!getBridgeSecret()) {
    return res.status(503).json({ error: 'MATRIX_BRIDGE_SECRET is required for Matrix delivery persistence' });
  }
  const msg = messages.find(m => m.id === req.params.id);
  if (!msg) return res.status(404).json({ error: 'message not found' });
  if (!msg.group || msg.source === 'matrix' || msg.type === 'human') {
    return res.status(400).json({ error: 'Matrix delivery persistence is limited to outbound group messages' });
  }

  const normalizeDeliveryId = (value, field, optional = false) => {
    if (optional && (value === undefined || value === null || value === '')) return { value: null };
    if (typeof value !== 'string' || !value.trim() || value.trim().length > 255) {
      return { error: `${field} must be 1..255 characters` };
    }
    return { value: value.trim() };
  };
  const room = normalizeDeliveryId(req.body?.room_id ?? req.body?.roomId, 'room_id');
  const primary = normalizeDeliveryId(
    req.body?.primary_event_id ?? req.body?.primaryEventId,
    'primary_event_id',
  );
  const threadRoot = normalizeDeliveryId(
    req.body?.thread_root_event_id ?? req.body?.threadRootEventId,
    'thread_root_event_id',
    true,
  );
  const invalid = room.error || primary.error || threadRoot.error;
  if (invalid) return res.status(400).json({ error: invalid });

  const candidate = {
    roomId: room.value,
    primaryEventId: primary.value,
    threadRootEventId: threadRoot.value,
  };
  const existing = msg.matrixDelivery || null;
  if (existing) {
    const identical = existing.roomId === candidate.roomId
      && existing.primaryEventId === candidate.primaryEventId
      && (existing.threadRootEventId || null) === candidate.threadRootEventId;
    if (!identical) {
      return res.status(409).json({
        error: 'primary Matrix delivery already recorded',
        matrixDelivery: existing,
      });
    }
    return res.json({ ok: true, id: msg.id, deduped: true, matrixDelivery: existing });
  }

  msg.matrixDelivery = candidate;
  if (!saveMessages()) {
    delete msg.matrixDelivery;
    invalidateUnreadMessageIndex();
    return res.status(503).json({ error: 'messages persistence failed' });
  }
  return res.json({ ok: true, id: msg.id, deduped: false, matrixDelivery: candidate });
});

app.get('/api/messages/:id/delivery', (req, res) => {
  const msg = messages.find(m => m.id === req.params.id);
  if (!msg) return res.status(404).json({ error: 'message not found' });
  const auth = authorizeMessageDetailAccess(req, msg);
  if (!auth.ok) return res.status(auth.status || 403).json({ error: auth.error || 'message access denied' });
  const agent = normalizeAgentName(req.query.agent) || null;
  const limit = Number.parseInt(req.query.limit, 10);
  const events = readDeliveryEvents({
    messageId: msg.id,
    agent,
    limit: Number.isFinite(limit) ? limit : 100,
  });
  res.json({ messageId: msg.id, agent, events });
});

app.get('/api/agents/:name/delivery-events', requireAgentToken(_tokenFromName), (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  if (!isAgentRecord(agents[agentName])) return res.status(404).json({ error: 'agent not found' });
  const limit = Number.parseInt(req.query.limit, 10);
  res.json({
    agent: agentName,
    events: readDeliveryEvents({
      agent: agentName,
      limit: Number.isFinite(limit) ? limit : 100,
    }),
  });
});

app.post('/api/messages/:id/suppress', requireAgentToken(_tokenFromAgent), (req, res) => {
  const agentName = normalizeAgentName(req.body?.agent);
  if (!agentName) return res.status(400).json({ error: 'agent required' });
  if (!isAgentRecord(agents[agentName])) return res.status(404).json({ error: 'agent not found' });

  const msg = messages.find(m => m.id === req.params.id);
  if (!msg) return res.status(404).json({ error: 'message not found' });
  if (!messageTargetsAgent(msg, agentName)) {
    return res.status(400).json({ error: `message ${msg.id} is not deliverable to ${agentName}` });
  }

  const before = getUnreadInboxMessages(agentName).unread.some(m => m.id === msg.id);
  if (!Array.isArray(msg.suppressedRecipients)) msg.suppressedRecipients = [];
  if (!msg.suppressedRecipients.includes(agentName)) {
    msg.suppressedRecipients.push(agentName);
    if (!saveMessages()) {
      msg.suppressedRecipients = msg.suppressedRecipients.filter((name) => name !== agentName);
      if (msg.suppressedRecipients.length === 0) delete msg.suppressedRecipients;
      invalidateUnreadMessageIndex();
      return res.status(503).json({ error: 'messages persistence failed' });
    }
    invalidatePendingHumanTargets(agentName);
    appendDeliveryEvent({
      type: 'message.suppressed',
      source: 'backend',
      messageId: msg.id,
      agent: agentName,
      targetAgents: [agentName],
      reason: normalizeOptionalText(req.body?.reason, 128) || 'explicit-suppress',
      context: {
        wasUnread: before,
      },
    });
  }
  const after = getUnreadInboxMessages(agentName).unread.some(m => m.id === msg.id);

  res.json({
    ok: true,
    id: msg.id,
    agent: agentName,
    suppressed: true,
    was_unread: before,
    is_unread_now: after,
    suppressedRecipients: msg.suppressedRecipients,
  });
});

// ── Full message HTML page (for Matrix links) ────────────────────────
app.get('/msg/:id', (req, res) => {
  const msg = messages.find(m => m.id === req.params.id);
  if (!msg) return res.status(404).send('<h1>Message not found</h1>');
  const auth = authorizeMessageDetailAccess(req, msg, { allowViewToken: true });
  if (!auth.ok) {
    return res.status(auth.status || 403).type('html').send(`<h1>${auth.status || 403}</h1><p>${String(auth.error || 'message access denied').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</p>`);
  }
  const escape = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const agentLinkParam = getRequestAgentName(req) ? `?agent=${encodeURIComponent(getRequestAgentName(req))}` : '';
  const attachmentsHtml = Array.isArray(msg.attachments) && msg.attachments.length > 0
    ? '<div class="meta">Attachments:<br>' + msg.attachments
      .map(a => {
        const label = escape(a?.name || path.basename(String(a?.path || 'file')));
        const meta = [a?.kind, a?.mime, a?.size ? `${a.size} bytes` : null].filter(Boolean).join(' · ');
        const pathText = escape(String(a?.path || ''));
        return `• <strong>${label}</strong>${meta ? ` (${escape(meta)})` : ''}<br><code>${pathText}</code>`;
      })
      .join('<br>')
      + '</div>'
    : '';
  // JSON-encode full text for safe embedding in <script>
  const fullJson = JSON.stringify(msg.full || '');
  res.type('html').send(`<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Message ${escape(msg.id)}</title>
<script src="https://cdn.jsdelivr.net/npm/marked/marked.min.js"><\/script>
<style>
  body { font-family: -apple-system, system-ui, sans-serif; max-width: 700px; margin: 2rem auto; padding: 0 1rem; background: #0a0a0f; color: #e0e0e0; }
  .meta { color: #888; font-size: 0.9rem; margin-bottom: 1rem; }
  .meta span { margin-right: 1rem; }
  .type { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 0.8rem; font-weight: 600; }
  .type-request { background: #1a3a5c; color: #4dabf7; }
  .type-inform { background: #1a3c1a; color: #69db7c; }
  .type-reply { background: #3a3a1a; color: #ffd43b; }
  .type-human { background: #3a1a3a; color: #da77f2; }
  .summary { font-size: 1.1rem; font-weight: 500; margin: 1rem 0; padding: 0.8rem; background: #151520; border-radius: 6px; border-left: 3px solid #4dabf7; }
  .full { font-size: 0.9rem; padding: 1rem; background: #151520; border-radius: 6px; line-height: 1.6; }
  .full h1,.full h2,.full h3,.full h4 { color: #4dabf7; margin-top: 1.2em; margin-bottom: 0.5em; border-bottom: 1px solid #222; padding-bottom: 0.3em; }
  .full h1 { font-size: 1.4em; } .full h2 { font-size: 1.2em; } .full h3 { font-size: 1.05em; }
  .full code { background: #1a1a2e; padding: 2px 6px; border-radius: 3px; font-family: 'SF Mono', Monaco, monospace; font-size: 0.9em; color: #69db7c; }
  .full pre { background: #0d0d1a; padding: 1rem; border-radius: 6px; overflow-x: auto; border: 1px solid #222; }
  .full pre code { background: none; padding: 0; color: #e0e0e0; }
  .full ul,.full ol { padding-left: 1.5em; margin: 0.5em 0; }
  .full li { margin: 0.3em 0; }
  .full blockquote { border-left: 3px solid #4dabf7; margin: 0.8em 0; padding: 0.5em 1em; color: #aaa; background: #0d0d1a; }
  .full table { border-collapse: collapse; margin: 0.8em 0; width: 100%; }
  .full th,.full td { border: 1px solid #333; padding: 6px 10px; text-align: left; }
  .full th { background: #1a1a2e; color: #4dabf7; }
  .full strong { color: #ffd43b; }
  .full a { color: #4dabf7; }
  .full p { margin: 0.5em 0; }
  .mentions { color: #da77f2; }
</style></head><body>
<h2>Agent Chat Message</h2>
<div class="meta">
  <span class="type type-${escape(msg.type)}">${escape(msg.type)}</span>
  <span>From: <strong>${escape(msg.from)}</strong></span>
  <span>${msg.to ? 'To: <strong>' + escape(msg.to) + '</strong>' : 'Group: <strong>' + escape(msg.group) + '</strong>'}</span>
  <span>${relativeTime(msg.ts)}</span>
</div>
${msg.mentions.length ? '<div class="mentions">Mentions: ' + msg.mentions.map(m => '@' + escape(m)).join(' ') + '</div>' : ''}
${msg.reply_to ? '<div class="meta">Reply to: <a href="/msg/' + escape(msg.reply_to) + agentLinkParam + '" style="color:#4dabf7">' + escape(msg.reply_to) + '</a></div>' : ''}
${attachmentsHtml}
<div class="summary">${escape(msg.summary).replace(/\\n/g, '<br>').replace(/\n/g, '<br>')}</div>
<h3>Full Message</h3>
<div class="full" id="full-content"></div>
<script>
  const raw = ${fullJson}.replace(/\\\\n/g, '\\n');
  try {
    document.getElementById('full-content').innerHTML = marked.parse(raw);
  } catch(e) {
    document.getElementById('full-content').textContent = raw;
  }
<\/script>
</body></html>`);
});

// ── Inbox ─────────────────────────────────────────────────────────────
app.get('/api/inbox/:agent/unread', requireAgentToken(_tokenFromAgent), (req, res) => {
  const agentName = normalizeAgentName(req.params.agent);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  if (!isAgentRecord(agents[agentName])) return res.status(404).json({ error: 'agent not found' });
  const kinds = parseKindsFilter(req.query.kinds);
  const snapshot = buildUnreadInboxSnapshot(agentName, { kinds });
  res.json(snapshot);
});

/*
 * "Accept Bearer (web-tier proxy) OR per-agent token" used to be spelled out here. `requireAgentToken`
 * does exactly that now, so the inline copy is gone — and with it the 512-character truncation it
 * carried, which would have locked the web tier out of this route with a long API_TOKEN.
 */
app.get('/api/inbox/:agent/unread-list', requireAgentToken(_tokenFromAgent), (req, res) => {
  const agentName = normalizeAgentName(req.params.agent);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  if (!isAgentRecord(agents[agentName])) return res.status(404).json({ error: 'agent not found' });

  const limitRaw = Number.parseInt(req.query.limit, 10);
  const limit = Number.isFinite(limitRaw) && limitRaw >= 0 ? Math.min(limitRaw, 500) : 50;
  const kinds = parseKindsFilter(req.query.kinds);
  const { unread } = getUnreadInboxMessages(agentName, { kinds });
  const rows = limit === 0 ? unread : unread.slice(-limit);
  res.json({
    agent: agentName,
    unread_total: unread.length,
    unread_returned: rows.length,
    unread_omitted: Math.max(0, unread.length - rows.length),
    messages: rows.map(summarizeMsg),
  });
});

app.get('/api/inbox/:agent', requireAgentToken(_tokenFromAgent), (req, res) => {
  const agentName = normalizeAgentName(req.params.agent);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  const agent = agents[agentName];
  if (!isAgentRecord(agent)) return res.status(404).json({ error: 'agent not found' });
  // A local Claude/Codex agent is served exclusively by capability-bound runners while
  // thread sessions are enabled. Header absence is not evidence of a legacy caller: a
  // compromised or buggy runner also holds the agent token and could simply omit its
  // dispatch headers. Require the complete capability before choosing the scoped read.
  if (THREAD_SESSIONS_ENABLED && threadSessionAgentEligibility(agent).ok) {
    const capability = runnerCapabilityFromRequest(req);
    if (!capability) {
      return res.status(401).json({
        error: 'complete runner dispatch capability required',
        code: 'runner_capability_required',
      });
    }
    const scoped = routerStore.checkInboxForAgent(capability, agentName);
    if (!Array.isArray(scoped)) {
      return res.status(routerRefusalStatus(scoped)).json({ error: scoped.message, code: scoped.code });
    }
    return res.json({
      dm: scoped.map((message) => ({
        id: message.messageId,
        from: message.senderName,
        summary: message.body.slice(0, 200),
        full: message.body,
        ts: message.receivedAt,
      })),
      group: [],
      session_scoped: true,
    });
  }
  const kindsList = parseKindsFilter(req.query.kinds);
  const kinds = kindsList.length > 0 ? new Set(kindsList) : null;

  const cursorSnapshot = snapshotCursor(agentName);
  const cursor = ensureCursor(agentName);
  const { unread } = getUnreadInboxMessages(agentName, { kinds: kindsList });
  const dmRaw = unread.filter(m => m.to === agentName);
  const dm = dmRaw.map(summarizeMsg);

  const groupRaw = unread.filter(m => m.group && m.to !== agentName);
  const group = groupRaw.map(summarizeMsg);

  // Filtered reads are preview-only: a global inbox cursor cannot safely advance over one kind
  // without implicitly skipping unread messages of other kinds.
  const runtime = ensureAgentRuntimeRecord(agentName);
  const pendingGate = getPendingInboxGate(runtime);
  // A full, unfiltered read always satisfies a pending gate: the agent has now seen the entire
  // inbox. This must NOT depend on the gate's sourceMsgId still being present in `unread` --
  // once the cursor advances past it (e.g. from an earlier full read), it can never reappear
  // there, which would otherwise deadlock the agent's send path forever. Empty `unread` counts
  // as a satisfied read too.
  const clearsPendingGate = Boolean(pendingGate) && !kinds;
  if (!kinds && advanceInboxCursor(cursor, unread)) {
    if (!saveCursors()) {
      restoreCursor(agentName, cursorSnapshot);
      return res.status(503).json({ error: 'cursor persistence failed' });
    }
    invalidatePendingHumanTargets(agentName);
  }
  if (!kinds) {
    markAgentInboxChecked(agentName, {
      clearInboxGate: clearsPendingGate,
      sourceMsgId: clearsPendingGate ? pendingGate.sourceMsgId : null,
    });
    if (unread.length > 0 || clearsPendingGate) {
      appendDeliveryEvent({
        type: 'inbox.read_ack',
        source: 'backend',
        agent: agentName,
        messageIds: unread.map((msg) => msg.id).filter(Boolean),
        messageId: clearsPendingGate ? pendingGate.sourceMsgId : null,
        ackedAt: Date.now(),
        cursor: {
          inboxTs: cursor.inbox || 0,
          inboxId: cursor.inboxId || null,
        },
        reason: clearsPendingGate ? 'inbox-gate-consumed' : 'inbox-read',
      });
    }
    // If the agent just consumed inbox, stale queued notifications should be removed immediately.
    clearQueuedNotificationsForAgent(agentName);
  }

  res.json({ dm, group });
});

// ── Group messages (unread + read split) ──────────────────────────────
app.get('/api/groups/:name/messages', (req, res) => {
  const groupName = req.params.name;
  if (!groups[groupName]) return res.status(404).json({ error: 'group not found' });

  const agentName = req.query.agent;
  if (!agentName) return res.status(400).json({ error: 'agent query param required' });
  const resolvedAgentName = normalizeAgentName(agentName);
  if (!resolvedAgentName) return res.status(400).json({ error: 'invalid agent query param' });
  if (!isAgentRecord(agents[resolvedAgentName])) return res.status(404).json({ error: 'agent not found' });
  if (!isGroupMember(groupName, resolvedAgentName)) {
    return res.status(403).json({ error: `agent '${resolvedAgentName}' is not a member of group '${groupName}'` });
  }
  const auth = authorizeAgentCredential(req, resolvedAgentName);
  if (!auth.ok) return res.status(auth.status || 403).json({ error: auth.error || 'agent credential required for group messages' });

  const limitRaw = Number.parseInt(req.query.limit, 10);
  const limit = Number.isFinite(limitRaw) && limitRaw >= 0 ? Math.min(limitRaw, 200) : 10;
  const hasAdvanceParam = typeof req.query.advance === 'string';
  const hasUnreadLimitParam = req.query.unread_limit !== undefined;
  const advanceModeRaw = hasAdvanceParam ? req.query.advance.trim().toLowerCase() : '';
  let advanceMode = ['all', 'delivered', 'none'].includes(advanceModeRaw) ? advanceModeRaw : null;
  if (!advanceMode) {
    // Backward-compatible "active read" escape hatch for old MCP schemas:
    // check_group(..., limit=0) => consume all unread.
    advanceMode = (!hasAdvanceParam && !hasUnreadLimitParam && limit === 0) ? 'all' : 'none';
  }
  const unreadLimitRaw = Number.parseInt(req.query.unread_limit, 10);
  let unreadLimit = Number.isFinite(unreadLimitRaw) && unreadLimitRaw > 0
    ? Math.min(unreadLimitRaw, 500)
    : null;
  if (unreadLimit === null && advanceMode !== 'all') {
    unreadLimit = 10; // default preview window
  }
  const cursorSnapshot = snapshotCursor(resolvedAgentName);
  const cursor = ensureCursor(resolvedAgentName);
  const groupCursor = getGroupCursor(cursor, groupName);
  const groupTs = groupCursor.ts;
  const groupId = groupCursor.id;

  const groupMsgs = (getUnreadMessageIndex().groupByName.get(groupName) || [])
    .filter(m => !isSuppressedForAgent(m, resolvedAgentName))
    .sort(compareMsgOrder);
  const unreadStart = firstMessageAfterCursorIndex(groupMsgs, groupTs, groupId);
  const unreadRaw = groupMsgs.slice(unreadStart);
  const unreadTotal = unreadRaw.length;
  const deliveredUnreadRaw = unreadLimit ? unreadRaw.slice(-unreadLimit) : unreadRaw;
  const unread = deliveredUnreadRaw.map(summarizeMsg);
  const unreadReturned = deliveredUnreadRaw.length;
  const unreadOmitted = Math.max(0, unreadTotal - unreadReturned);
  const read = groupMsgs.slice(0, unreadStart).slice(-limit).map(summarizeMsg);

  // Advance group cursor by mode:
  // - all: consume all unread (legacy behavior)
  // - delivered: consume only returned unread subset
  // - none: preview only
  if (advanceMode !== 'none') {
    const cursorSource = advanceMode === 'all' ? unreadRaw : deliveredUnreadRaw;
    if (advanceGroupCursor(cursor, groupName, cursorSource)) {
      if (!saveCursors()) {
        restoreCursor(resolvedAgentName, cursorSnapshot);
        return res.status(503).json({ error: 'cursor persistence failed' });
      }
      const advancedCursor = getGroupCursor(cursor, groupName);
      const advancedIds = unreadRaw
        .filter((msg) => !isAfterCursor(msg, advancedCursor.ts, advancedCursor.id))
        .map((msg) => msg?.id)
        .filter(Boolean);
      const returnedIds = cursorSource.map((msg) => msg?.id).filter(Boolean);
      appendDeliveryEvent({
        type: 'group.read_ack',
        source: 'backend',
        agent: resolvedAgentName,
        messageId: advancedIds[advancedIds.length - 1] || null,
        messageIds: advancedIds,
        ackedAt: Date.now(),
        cursor: {
          group: groupName,
          groupTs: advancedCursor.ts,
          groupId: advancedCursor.id,
        },
        reason: `group-read-${advanceMode}`,
        context: {
          group: groupName,
          unreadTotal,
          unreadReturned,
          returnedMessageIds: returnedIds,
        },
      });
    }
  }

  res.json({
    group: groupName,
    unread,
    read,
    unread_total: unreadTotal,
    unread_returned: unreadReturned,
    unread_omitted: unreadOmitted,
    advance: advanceMode,
  });
});

// ── Agent's groups with unread counts ─────────────────────────────────
app.get('/api/agents/:name/groups', (req, res) => {
  const agentName = normalizeAgentName(req.params.name);
  if (!agentName) return res.status(400).json({ error: 'invalid agent name' });
  if (!isAgentRecord(agents[agentName])) return res.status(404).json({ error: 'agent not found' });

  const cursor = ensureCursor(agentName);
  const inboxTs = cursor.inbox || 0;
  const inboxId = cursor.inboxId || null;
  const messageIndex = getUnreadMessageIndex();

  const result = Object.values(groups)
    .filter(g => isGroupMember(g.name, agentName))
    .map(g => {
      const groupMsgs = (messageIndex.groupByName.get(g.name) || [])
        .filter(m => !isSuppressedForAgent(m, agentName))
        .sort(compareMsgOrder);
      const { ts: groupTs, id: groupId } = getGroupCursor(cursor, g.name);

      const unread_messages = groupMsgs.length - firstMessageAfterCursorIndex(groupMsgs, groupTs, groupId);
      const mentionMsgs = (messageIndex.groupMentionsByAgent.get(agentName) || [])
        .filter(m => m.group === g.name && !isSuppressedForAgent(m, agentName))
        .sort(compareMsgOrder);
      const unread_mentions = mentionMsgs.length - firstMessageAfterCursorIndex(mentionMsgs, inboxTs, inboxId);

      return { name: g.name, members: g.members, unread_mentions, unread_messages };
    });

  res.json(result);
});

// ── Graceful shutdown ─────────────────────────────────────────────────
let shutdownPromise = null;
function shutdown() {
  if (shutdownPromise) return shutdownPromise;
  console.log('Shutting down, terminating runners and saving data...');
  shutdownPromise = (async () => {
    try {
      refreshServerLiveness();
      const sweeps = Promise.allSettled([
        sweepLocalActivityDurations(),
        sweepLocalSwapPressure(),
        sweepAgentScopePressure(),
      ]);
      // stopServer closes ingress, aborts every live runner, and waits until
      // each dispatch has durably settled or been safely requeued.
      await stopServer();
      await sweeps;
      sweepAgentRules();
      flushAllPendingJsonWrites();
      saveAgents(true);
      saveGroups();
      saveMessages();
      saveCursors();
      saveServers();
      saveAgentRuntime(true);
    } catch (error) {
      console.error(`Shutdown failed: ${error?.message || error}`);
    } finally {
      process.exit(0);
    }
  })();
  return shutdownPromise;
}
let startupHooksInstalled = false;
let backgroundLoopsStarted = false;
let serverInstance = null;
const lifecycleIntervals = new Set();
const lifecycleTimeouts = new Set();

function trackLifecycleInterval(fn, delay) {
  const timer = setInterval(fn, delay);
  lifecycleIntervals.add(timer);
  return timer;
}

function trackLifecycleTimeout(fn, delay, { unref = false } = {}) {
  const timer = setTimeout(() => {
    lifecycleTimeouts.delete(timer);
    fn();
  }, delay);
  lifecycleTimeouts.add(timer);
  if (unref && typeof timer?.unref === 'function') timer.unref();
  return timer;
}

function clearLifecycleHandles() {
  for (const timer of lifecycleIntervals) clearInterval(timer);
  lifecycleIntervals.clear();
  for (const timer of lifecycleTimeouts) clearTimeout(timer);
  lifecycleTimeouts.clear();
}

function endSseClients() {
  for (const client of [...sseAdapter.clients]) {
    try { client.end(); } catch (_) { /* ignore close errors */ }
  }
  sseAdapter.clients.clear();
}

function installStartupHooks() {
  if (startupHooksInstalled) return;
  process.on('SIGTERM', shutdown);
  process.on('SIGINT', shutdown);
  startupHooksInstalled = true;
}

function removeStartupHooks() {
  if (!startupHooksInstalled) return;
  process.off('SIGTERM', shutdown);
  process.off('SIGINT', shutdown);
  startupHooksInstalled = false;
}

function startBackgroundLoops() {
  // The delivery queue's own loops (pane sweep, delivery tick, reminder tick) start and stop with the
  // backend's, so a test that never starts the backend's loops never gets a stray tmux capture either.
  startDeliveryQueueLoops();
  if (backgroundLoopsStarted) return;
  backgroundLoopsStarted = true;
  routerPumpAccepting = true;

  // A process restart may leave durable queued dispatches behind.  Kick the
  // router once at startup instead of waiting for a new Matrix event to make
  // already-accepted work runnable again.
  if (THREAD_SESSIONS_ENABLED) scheduleRouterPump();

  // SSE keepalive: send comment pings every 30s to prevent proxy idle timeout.
  sseAdapter.startKeepalive(trackLifecycleInterval);

  trackLifecycleInterval(() => {
    refreshServerLiveness();
  }, SERVER_SWEEP_INTERVAL_MS);

  scheduleAdaptiveSweepLoop('sweepLocalActivityDurations', sweepLocalActivityDurations, 'localActivity', LOCAL_ACTIVITY_SWEEP_INTERVAL_MS);

  trackLifecycleInterval(() => {
    sweepAgentRules();
  }, RULE_SWEEP_INTERVAL_MS);

  scheduleAdaptiveSweepLoop('sweepLocalSwapPressure', sweepLocalSwapPressure, 'localSwap', SWAP_SWEEP_INTERVAL_MS);

  scheduleAdaptiveSweepLoop('sweepAgentScopePressure', sweepAgentScopePressure, 'agentScope', AGENT_SCOPE_SWEEP_INTERVAL_MS);

  // Supervisor lifecycle sweep — manages per-agent supervisor tmux sessions
  scheduleAdaptiveSweepLoop('sweepSupervisorLifecycle', () => supervisorLifecycleManager.sweepAll(), 'supervisorLifecycle', SUPERVISOR_LIFECYCLE_SWEEP_INTERVAL_MS);

  /*
   * Overruns are swept rather than detected at a decision point, because nothing decides them: a
   * ceiling lowered under live commitments produces one with no request involved. Hourly, because the
   * condition is a standing one — an agent past its ceiling at 09:00 is still past it at 09:05, and a
   * tighter loop would only re-file the same alert.
   */
  trackLifecycleInterval(() => { sweepCeilingOverruns(); }, CEILING_OVERRUN_SWEEP_INTERVAL_MS);

  /*
   * Membership on a customer's homeserver, swept for the agents that cannot notice their own absence.
   * Hourly for the same reason the overrun sweep is: the condition is standing, and a tighter loop would
   * re-ask a foreign homeserver about rooms nothing has changed.
   */
  trackLifecycleInterval(() => {
    sweepProjectRoomMembership().catch((error) => {
      console.warn(`[readmit] sweep failed: ${error?.message || error}`);
    });
  }, PROJECT_ROOM_MEMBERSHIP_SWEEP_INTERVAL_MS);

  // Prune resolved alerts every hour
  trackLifecycleInterval(() => { alertStore.pruneResolved(); }, 3600_000);
}

function stopBackgroundLoops() {
  backgroundLoopsStarted = false;
  stopDeliveryQueueLoops();
  resetDeliveryQueueHooks();
  routerPumpAccepting = false;
  clearLifecycleHandles();
  routerPumpTimer = null;
  routerPumpDueAt = null;
  endSseClients();
}

function stopRouterPumpForTest() {
  routerPumpAccepting = false;
  for (const { controller } of liveThreadSessionRunners.values()) controller.abort();
  liveThreadSessionRunners.clear();
  routerPumpMicrotaskQueued = false;
  routerPumpTimer = null;
  routerPumpDueAt = null;
  routerPumpRunning = false;
  clearLifecycleHandles();
}

// Bind address. Loopback by default; see lib/startup-config.js resolveBindHost
// for why widening it is opt-in and logged.
function resolvedBindHost() {
  const { host, warning } = resolveBindHost(process.env.HAGENCY_BACKEND_HOST);
  if (warning) console.warn(`[bind] ${warning}`);
  return host;
}

export function startServer({ port = PORT, host = resolvedBindHost() } = {}) {
  if (serverInstance) return serverInstance;
  installStartupHooks();
  void reconcileThreadSessionSourceMessages().then((sourceReconciliation) => {
    if (sourceReconciliation.scanned > 0) {
      console.log(`[router] reconciled ${sourceReconciliation.scanned} persisted Matrix message(s)`);
    }
  }).catch((error) => {
    console.error(`[router] startup source reconciliation failed: ${error?.message || error}`);
    void stopServer().finally(() => { process.exitCode = 1; });
  });
  startBackgroundLoops();

  const MAX_LISTEN_RETRIES = 10;
  let listenAttempt = 0;

  function tryListen() {
    const server = app.listen(port, host);
    server.on('listening', () => {
      serverInstance = server;
      runAsyncSweep('sweepLocalActivityDurations', sweepLocalActivityDurations, 'localActivity');
      runAsyncSweep('sweepLocalSwapPressure', sweepLocalSwapPressure, 'localSwap');
      runAsyncSweep('sweepAgentScopePressure', sweepAgentScopePressure, 'agentScope');
      runAsyncSweep('sweepSupervisorLifecycle', () => supervisorLifecycleManager.sweepAll(), 'supervisorLifecycle');
      runAsyncSweep('sweepProjectSideRepresentatives', sweepProjectSideRepresentatives, 'projectSides');
      try { const orphans = supervisorLifecycleManager.cleanOrphanSessions(); if (orphans.length) console.log(`  Cleaned ${orphans.length} orphan supervisor session(s): ${orphans.join(', ')}`); } catch { /* tmux not available */ }
      console.log(`Agent Chat v2 backend listening on http://${host}:${port}`);
      const agentCount = Object.values(agents).filter(isAgentRecord).length;
      console.log(`  Agents: ${agentCount}, Messages: ${messages.length}, Groups: ${Object.keys(groups).length}`);
    });
    server.on('error', (err) => {
      if (err.code === 'EADDRINUSE') {
        listenAttempt++;
        if (listenAttempt >= MAX_LISTEN_RETRIES) {
          console.error(`[FATAL] Port ${port} still in use after ${MAX_LISTEN_RETRIES} retries — exiting`);
          process.exit(1);
        }
        const delay = Math.min(30_000, 1000 * Math.pow(2, listenAttempt - 1));
        console.warn(`[EADDRINUSE] Port ${port} in use — retry ${listenAttempt}/${MAX_LISTEN_RETRIES} in ${delay}ms`);
        trackLifecycleTimeout(tryListen, delay);
      } else {
        console.error(`[FATAL] Server listen error: ${err.message}`);
        process.exit(1);
      }
    });
    return server;
  }

  serverInstance = tryListen();
  return serverInstance;
}

export async function stopServer() {
  stopBackgroundLoops();
  removeStartupHooks();
  const server = serverInstance;
  serverInstance = null;
  let serverClose = Promise.resolve();
  if (server) {
    if (typeof server.closeAllConnections === 'function') {
      try { server.closeAllConnections(); } catch (_) { /* ignore close errors */ }
    }
    serverClose = new Promise((resolve, reject) => {
      server.close((error) => {
        if (!error || error.code === 'ERR_SERVER_NOT_RUNNING') resolve();
        else reject(error);
      });
    });
  }
  const activeRunners = [...liveThreadSessionRunners.values()];
  for (const active of activeRunners) active.controller.abort();
  await Promise.allSettled(activeRunners.map((active) => active.running));
  liveThreadSessionRunners.clear();
  flushAllPendingJsonWrites();
  await serverClose;
}

export { app };
export {
  normalizeAgentName,
  normalizeHumanMeta,
  mergeHumanMeta,
  normalizeAgentTask,
  serializeAgent,
  notificationRouter,
};
export const __backendV2TestInternals = {
  // The directory this module bound to when its body evaluated. RUNTIME_ROOT is
  // read from process.env at import time, and process.env is process-global, so a
  // test that sets it and then awaits import() can have the value changed
  // underneath by any other test doing the same. A module that bound to someone
  // else's directory finds no seeded agents and answers 404 to everything —
  // observed as GET /api/agents/doomed and DELETE /api/agents/deletetest returning
  // 404 for agents that were definitely seeded. Exposed so a caller can check
  // rather than discover it as a mystery failure much later.
  runtimeRootForTest: RUNTIME_ROOT,
  setEngagementLauncherForTest(fn) { launchEngagementAgent = fn; },
  setLocalRequestOverrideForTest,
  /*
   * What this module can actually SEE in the store it loaded, for the test helper's post-import check.
   *
   * BOTH LISTS, and that is the whole point: `records` is every key it read, `agentRecords` only those
   * `inferRecordKind` calls an agent. A first version of the check compared seeded KEYS against
   * agent-records-only and reported a human seed (`kind: 'human'`) as missing data — a false positive that
   * deterministically broke two test files while claiming to have caught a flake. The kind rule lives
   * here, so the answer has to come from here too.
   */
  storeSnapshotForTest: () => ({
    records: Object.keys(agents),
    agentRecords: Object.values(agents).filter(isAgentRecord).map((a) => a.name),
  }),
  /*
   * The overrun sweep runs on an hourly interval, and a test must not wait an hour or start the
   * backend's loops to see what it files. Exposed so the alarm can be driven directly — the interval
   * is scheduling, the alarm is the behaviour.
   */
  sweepCeilingOverrunsForTest: sweepCeilingOverruns,
  sweepProjectRoomMembershipForTest: sweepProjectRoomMembership,
  // F10 (17-r4): the two real admission/withdraw paths, exported for the roster counterexamples.
  admitAgentToProjectRoomForTest: admitAgentToProjectRoom,
  withdrawAgentFromProjectRoomForTest: withdrawAgentFromProjectRoom,
  backendRosterAdmitsForTest: backendRosterAdmits,
  buildLocalPaneMetadataSnapshotForTest: buildLocalPaneMetadataSnapshotAsync,
  injectSlashClearForTest: injectSlashClear,
  sessionPolicyForTest: sessionPolicy,
  notifyAgentCatchupForTest: notifyAgentCatchup,
  pushNotifyForTest: pushNotify,
  sseAdapterForTest: sseAdapter,
  safeWriteJsonFile,
  setJsonSaveFailureForTest,
  setMatrixDispatchFailureForTest(stage = null) {
    matrixDispatchFailureStageForTest = stage;
  },
  dispatchLeaseStoreForTest: dispatchLeaseStore,
  approvalStoreForTest: approvalStore,
  approvalAdapterTimeoutMsForTest: APPROVAL_ADAPTER_TIMEOUT_MS,
  routerStoreForTest: routerStore,
  liveThreadSessionRunnersForTest: liveThreadSessionRunners,
  agentOpsServiceForTest: agentOpsService,
  agentOpsServerIdentityForTest: agentOpsServerIdentity,
  shadowMatrixMessageToRouterForTest: shadowMatrixMessageToRouter,
  routeMatrixMessageToThreadSessionForTest: routeMatrixMessageToThreadSession,
  reconcileThreadSessionSourceMessagesForTest: reconcileThreadSessionSourceMessages,
  reconcileThreadSessionPeerMessagesForTest: reconcileThreadSessionPeerMessages,
  scheduleRouterPumpForTest: scheduleRouterPump,
  stopRouterPumpForTest,
  dispatchQueuesForTest: dispatchQueues,
  sweepLocalActivityDurationsForTest: sweepLocalActivityDurations,
  /*
   * The live server store, for tests that need a heartbeat to be OLD.
   *
   * The alternative was what tests/api-server-heartbeat.test.js actually did: set the TTL to
   * 100ms and sleep(200) to make a server stale — which put every assertion after the recovery
   * heartbeat inside a 100ms window that a GC pause under load regularly blew. Specimen (run 2 of
   * the forensic hunt): s1's outage re-opened between the recovery heartbeat and the assertion
   * GET, so `openAfter` read [s1, s2] instead of [s2]. Rewriting servers.json does not work —
   * this store is the in-memory truth and the file is only its persistence.
   */
  serversForTest: servers,
};

if (process.argv[1] === __filename) {
  enforceStartupConfig({
    serviceName: 'Agent Chat v2 backend',
    optional: BACKEND_STARTUP_OPTIONAL_ENV,
  });
  startServer();
}
