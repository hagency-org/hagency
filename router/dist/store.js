import Database from 'better-sqlite3';
import { createHash, randomBytes as nodeRandomBytes, randomUUID } from 'node:crypto';
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { INITIAL_SCHEMA } from './migrations/001-initial.js';
import { DISPATCH_MESSAGE_BATCHES_SCHEMA } from './migrations/002-dispatch-message-batches.js';
import { THREAD_NOTICES_SCHEMA } from './migrations/003-thread-notices.js';
import { OUTCOME_RECOVERY_SCHEMA } from './migrations/004-outcome-recovery.js';
import { RUNNER_RECOVERY_HARDENING_SCHEMA } from './migrations/005-runner-recovery-hardening.js';
import { AGENT_OPS_CLIENT_SCHEMA } from './migrations/006-agent-ops-client.js';
import { AGENT_OPS_SERVER_IDENTITY_SCHEMA } from './migrations/007-agent-ops-server-identity.js';
import { SESSION_OVERRIDES_SCHEMA } from './migrations/008-session-overrides.js';
import { APPROVAL_REQUEST_IDENTITY_SCHEMA } from './migrations/009-approval-request-identity.js';
import { TASK_AUTHORIZATION_EPOCH_SCHEMA } from './migrations/011-task-authorization-epoch.js';
import { ConversationStore } from './conversations.js';
import { CONVERSATION_SCHEMA } from './migrations/010-conversations.js';
import { DELIVERY_STORAGE_SCHEMA } from './migrations/013-delivery-storage.js';
import { ActivityStore } from './activity.js';
import {} from './files.js';
import { TASK_OPERATIONS_SCHEMA } from './migrations/009-task-operations.js';
import { applyTaskOperation } from './task-operations.js';
const SCHEMA_VERSION = 9;
const DEFAULT_EVENT_RETENTION = 10_000;
function refusal(code, message) {
    return { ok: false, code, message };
}
function requiredText(value, field, max = 4096) {
    if (typeof value !== 'string') {
        throw new RouterInputError(`${field} must be 1..${max} characters`);
    }
    const normalized = value.trim();
    if (!normalized || normalized.length > max) {
        throw new RouterInputError(`${field} must be 1..${max} characters`);
    }
    return normalized;
}
const CONTEXT_TRUNCATION_MARKER = '\n…[truncated to rebuild budget; use check_inbox for the full session input]';
function stringPrefixWithinJsonBudget(value, maxSerializedChars) {
    if (maxSerializedChars < 2)
        return '';
    let low = 0;
    let high = value.length;
    while (low < high) {
        const middle = Math.ceil((low + high) / 2);
        if (JSON.stringify(value.slice(0, middle)).length <= maxSerializedChars)
            low = middle;
        else
            high = middle - 1;
    }
    return value.slice(0, low);
}
function truncateTextWithinJsonBudget(value, maxSerializedChars, marker = CONTEXT_TRUNCATION_MARKER) {
    if (JSON.stringify(value).length <= maxSerializedChars)
        return value;
    const fittedMarker = stringPrefixWithinJsonBudget(marker, maxSerializedChars);
    const markerChars = JSON.stringify(fittedMarker).length - 2;
    const prefixBudget = Math.max(2, maxSerializedChars - markerChars);
    return `${stringPrefixWithinJsonBudget(value, prefixBudget)}${fittedMarker}`;
}
function optionalText(value, max = 4096) {
    if (typeof value !== 'string')
        return null;
    const normalized = value.trim();
    return normalized ? normalized.slice(0, max) : null;
}
function canonicalize(value) {
    if (Array.isArray(value))
        return value.map(canonicalize);
    if (value !== null && typeof value === 'object') {
        const record = value;
        const sorted = {};
        for (const key of Object.keys(record).sort())
            sorted[key] = canonicalize(record[key]);
        return sorted;
    }
    return value;
}
function canonicalJson(value) {
    return JSON.stringify(canonicalize(value));
}
function digest(value) {
    return createHash('sha256').update(typeof value === 'string' ? value : canonicalJson(value)).digest('hex');
}
function safeParseObject(value) {
    const parsed = JSON.parse(value);
    if (parsed === null || Array.isArray(parsed) || typeof parsed !== 'object') {
        throw new Error('stored JSON payload is not an object');
    }
    return parsed;
}
function publicRuntimeReason(reason) {
    if (reason && /(?:^|[\s"'`=(,:])(?:\/[^\s]|~\/|[A-Za-z]:[\\/])/.test(reason)) {
        return 'Runtime details redacted; inspect the dispatch through the operator recovery flow';
    }
    return reason;
}
function safeParseStringArray(value) {
    const parsed = JSON.parse(value);
    if (!Array.isArray(parsed) || parsed.some((item) => typeof item !== 'string')) {
        throw new Error('stored resource list is invalid');
    }
    return parsed;
}
function summarizeEarlierMessages(messages, maxChars) {
    if (messages.length === 0 || maxChars < 80)
        return '';
    const agreementPattern = /(?:\bagree(?:d|ment)?\b|\bmust\b|\bnever\b|\balways\b|\bconstraint\b|约定|同意|必须|不要|始终|永远|限制|决定)/iu;
    const agreements = [];
    const seen = new Set();
    for (const message of messages) {
        for (const rawLine of message.normalized_body.split(/\r?\n/u)) {
            const line = rawLine.replace(/\s+/gu, ' ').trim();
            if (!line || !agreementPattern.test(line) || seen.has(line))
                continue;
            seen.add(line);
            agreements.push(`[${message.sender_name ?? 'unknown'}] ${line}`);
        }
    }
    const history = messages.map((message) => (`[${message.sender_name ?? 'unknown'}] ${message.normalized_body.replace(/\s+/gu, ' ').trim()}`));
    const sections = [
        ...(agreements.length > 0 ? [`Standing agreements:\n${agreements.join('\n')}`] : []),
        `Earlier session history:\n${history.join('\n')}`,
    ];
    const joined = sections.join('\n\n');
    if (joined.length <= maxChars)
        return joined;
    const marker = '\n…[older context truncated to rebuild budget]';
    return `${joined.slice(0, Math.max(0, maxChars - marker.length))}${marker}`;
}
function sessionView(row) {
    return {
        sessionId: row.session_id,
        agentId: row.agent_id,
        agentName: row.agent_name,
        roomId: row.room_id,
        scopeKind: row.scope_kind,
        threadRootEventId: row.thread_root_event_id,
        contextGeneration: row.context_generation,
        lastActive: row.last_active,
        modelOverride: row.model_override ?? null,
        modeOverride: row.mode_override ?? null,
    };
}
const MODEL_OVERRIDE_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;
class RouterInputError extends Error {
}
export class RouterStore {
    conversations;
    activity;
    /** @internal Router package and white-box tests only; never a production integration surface. */
    db;
    dbPath;
    now;
    randomBytes;
    eventRetention;
    constructor(options) {
        this.dbPath = path.resolve(options.dbPath);
        mkdirSync(path.dirname(this.dbPath), { recursive: true, mode: 0o700 });
        this.now = options.now ?? Date.now;
        this.randomBytes = options.randomBytes ?? nodeRandomBytes;
        this.eventRetention = Math.max(100, options.eventRetention ?? DEFAULT_EVENT_RETENTION);
        this.db = new Database(this.dbPath);
        this.db.pragma('journal_mode = WAL');
        this.db.pragma('foreign_keys = ON');
        this.db.pragma('busy_timeout = 5000');
        this.applyMigrations();
        this.conversations = new ConversationStore(this.db);
        this.activity = new ActivityStore(this.db);
    }
    close() {
        this.db.close();
    }
    applyMigrations() {
        this.db.exec(INITIAL_SCHEMA);
        this.db.prepare('INSERT OR IGNORE INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(1, this.now());
        const versionTwo = this.db.prepare('SELECT version AS id FROM router_schema_migrations WHERE version = ?').get(2);
        if (!versionTwo) {
            const migrate = this.db.transaction(() => {
                this.db.exec(DISPATCH_MESSAGE_BATCHES_SCHEMA);
                this.db.prepare('INSERT INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(2, this.now());
            });
            migrate();
        }
        const versionThree = this.db.prepare('SELECT version AS id FROM router_schema_migrations WHERE version = ?').get(3);
        if (!versionThree) {
            const migrate = this.db.transaction(() => {
                this.db.exec(THREAD_NOTICES_SCHEMA);
                this.db.prepare('INSERT INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(3, this.now());
            });
            migrate();
        }
        const versionFour = this.db.prepare('SELECT version AS id FROM router_schema_migrations WHERE version = ?').get(4);
        if (!versionFour) {
            const migrate = this.db.transaction(() => {
                this.db.exec(OUTCOME_RECOVERY_SCHEMA);
                this.db.prepare('INSERT INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(4, this.now());
            });
            migrate();
        }
        const versionFive = this.db.prepare('SELECT version AS id FROM router_schema_migrations WHERE version = ?').get(5);
        if (!versionFive) {
            const migrate = this.db.transaction(() => {
                const duplicate = this.db.prepare(`SELECT assignee_agent_id, room_id, thread_root_event_id, COUNT(*) AS count
            FROM task_bindings
            GROUP BY assignee_agent_id, room_id, thread_root_event_id
            HAVING COUNT(*) > 1 LIMIT 1`).get();
                if (duplicate) {
                    throw new Error(`router migration 5 refused ambiguous task bindings for agent ${duplicate.assignee_agent_id}`);
                }
                this.db.exec(RUNNER_RECOVERY_HARDENING_SCHEMA);
                this.db.prepare('INSERT INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(5, this.now());
            });
            migrate();
        }
        const versionSix = this.db.prepare('SELECT version AS id FROM router_schema_migrations WHERE version = ?').get(6);
        if (!versionSix) {
            const migrate = this.db.transaction(() => {
                this.db.exec(AGENT_OPS_CLIENT_SCHEMA);
                this.db.prepare('INSERT INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(6, this.now());
            });
            migrate();
        }
        const versionSeven = this.db.prepare('SELECT version AS id FROM router_schema_migrations WHERE version = ?').get(7);
        if (!versionSeven) {
            const migrate = this.db.transaction(() => {
                this.db.exec(AGENT_OPS_SERVER_IDENTITY_SCHEMA);
                this.db.prepare('INSERT INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(7, this.now());
            });
            migrate();
        }
        const versionEight = this.db.prepare('SELECT version AS id FROM router_schema_migrations WHERE version = ?').get(8);
        if (!versionEight) {
            const migrate = this.db.transaction(() => {
                this.db.exec(SESSION_OVERRIDES_SCHEMA);
                this.db.prepare('INSERT INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(8, this.now());
            });
            migrate();
        }
        // Two released branches used migration 9 for different schemas. Migration
        // numbers alone cannot distinguish them. Converge their physical schemas
        // atomically, retaining the original history and recording the union as 12.
        if (!this.db.prepare('SELECT 1 FROM router_schema_migrations WHERE version=12').get())
            this.db.transaction(() => {
                const hasColumn = (table, column) => this.db.pragma(`table_info(${table})`).some((row) => row.name === column);
                if (!hasColumn('approval_waits', 'resumed_at'))
                    this.db.exec(APPROVAL_REQUEST_IDENTITY_SCHEMA);
                if (!this.db.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'runner_task_operations'").get()) {
                    this.db.exec(TASK_OPERATIONS_SCHEMA);
                }
                this.db.exec(CONVERSATION_SCHEMA);
                if (!hasColumn('tasks', 'execution_epoch'))
                    this.db.exec(TASK_AUTHORIZATION_EPOCH_SCHEMA);
                for (const version of [9, 10, 11, 12]) {
                    this.db.prepare('INSERT OR IGNORE INTO router_schema_migrations(version, applied_at) VALUES (?, ?)').run(version, this.now());
                }
            })();
        if (!this.db.prepare('SELECT 1 FROM router_schema_migrations WHERE version=13').get())
            this.db.transaction(() => {
                this.db.exec(DELIVERY_STORAGE_SCHEMA);
                this.db.prepare('INSERT INTO router_schema_migrations(version,applied_at) VALUES (?,?)').run(13, this.now());
            })();
    }
    emit(kind, payload, audience = 'operator') {
        const result = this.db.prepare('INSERT INTO router_events(schema_version, at, kind, audience_scope, payload_json) VALUES (?, ?, ?, ?, ?)').run(SCHEMA_VERSION, this.now(), kind, audience, canonicalJson(payload));
        const seq = Number(result.lastInsertRowid);
        this.db.prepare('UPDATE router_event_meta SET high_watermark = ? WHERE id = 1').run(seq);
        this.trimEvents();
        return seq;
    }
    trimEvents() {
        const count = this.db.prepare('SELECT COUNT(*) AS count FROM router_events').get()?.count ?? 0;
        if (count <= this.eventRetention)
            return;
        const remove = count - this.eventRetention;
        const boundary = this.db.prepare('SELECT seq FROM router_events ORDER BY seq LIMIT 1 OFFSET ?').get(remove);
        if (!boundary)
            return;
        this.db.prepare('DELETE FROM router_events WHERE seq < ?').run(boundary.seq);
        this.db.prepare('UPDATE router_event_meta SET low_watermark = ? WHERE id = 1').run(boundary.seq);
    }
    resolveSessionInternal(input) {
        const agentId = requiredText(input.agentId, 'agent_id', 255);
        const agentName = requiredText(input.agentName, 'agent_name', 255);
        const roomId = requiredText(input.roomId, 'room_id', 512);
        const threadRoot = optionalText(input.threadRootEventId, 512);
        const scope = threadRoot ? 'thread' : 'main';
        const existing = threadRoot === null
            ? this.db.prepare("SELECT * FROM sessions WHERE agent_id = ? AND room_id = ? AND scope_kind = 'main'").get(agentId, roomId)
            : this.db.prepare("SELECT * FROM sessions WHERE agent_id = ? AND room_id = ? AND scope_kind = 'thread' AND thread_root_event_id = ?").get(agentId, roomId, threadRoot);
        if (existing) {
            this.db.prepare('UPDATE sessions SET agent_name = ?, last_active = ? WHERE session_id = ?').run(agentName, this.now(), existing.session_id);
            return { row: { ...existing, agent_name: agentName, last_active: this.now() }, created: false };
        }
        const now = this.now();
        const row = {
            session_id: randomUUID(),
            agent_id: agentId,
            agent_name: agentName,
            room_id: roomId,
            scope_kind: scope,
            thread_root_event_id: threadRoot,
            context_generation: 1,
            rolling_summary: '',
            created_at: now,
            last_active: now,
            model_override: null,
            mode_override: null,
        };
        this.db.prepare(`INSERT INTO sessions(session_id, agent_id, agent_name, room_id, scope_kind,
        thread_root_event_id, created_at, last_active) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`).run(row.session_id, row.agent_id, row.agent_name, row.room_id, row.scope_kind, row.thread_root_event_id, now, now);
        this.emit('session.created', {
            sessionId: row.session_id,
            agentId: row.agent_id,
            roomId: row.room_id,
            scopeKind: row.scope_kind,
            threadRootEventId: row.thread_root_event_id,
        });
        return { row, created: true };
    }
    resolveSession(input) {
        const tx = this.db.transaction(() => this.resolveSessionInternal({
            ...input,
            threadRootEventId: input.threadRootEventId ?? null,
        }));
        return sessionView(tx().row);
    }
    sessionById(sessionIdInput) {
        const sessionId = requiredText(sessionIdInput, 'session_id', 255);
        const row = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(sessionId);
        return row ? sessionView(row) : null;
    }
    setSessionOverrides(input) {
        try {
            const requestedBy = requiredText(input.requestedBy, 'requested_by', 255);
            if (input.model === undefined && input.mode === undefined) {
                return refusal('bad_request', 'a session override directive must set model or mode');
            }
            if (input.model !== undefined && input.model !== null && !MODEL_OVERRIDE_PATTERN.test(input.model)) {
                return refusal('bad_request', 'model override must be a plain model name or alias (max 64 chars)');
            }
            if (input.mode !== undefined && input.mode !== null && input.mode !== 'plan' && input.mode !== 'auto') {
                return refusal('bad_request', "mode override must be 'plan' or 'auto'");
            }
            const tx = this.db.transaction(() => {
                const { row } = this.resolveSessionInternal({
                    agentId: input.agentId,
                    agentName: input.agentName,
                    roomId: input.roomId,
                    threadRootEventId: input.threadRootEventId ?? null,
                });
                const model = input.model === undefined ? (row.model_override ?? null) : input.model;
                const mode = input.mode === undefined ? (row.mode_override ?? null) : input.mode;
                this.db.prepare('UPDATE sessions SET model_override = ?, mode_override = ? WHERE session_id = ?').run(model, mode, row.session_id);
                // The grant is an authorization decision, so it must leave an audit
                // trail naming who made it — the session row alone cannot carry that.
                this.emit('session.overrides_set', {
                    sessionId: row.session_id,
                    agentId: row.agent_id,
                    roomId: row.room_id,
                    threadRootEventId: row.thread_root_event_id,
                    modelOverride: model,
                    modeOverride: mode,
                    requestedBy,
                });
                return sessionView({ ...row, model_override: model, mode_override: mode });
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    queueSessionNotice(input) {
        try {
            this.insertNotice({
                dispatchId: null,
                taskId: null,
                dedupeKey: requiredText(input.dedupeKey, 'dedupe_key', 255),
                roomId: requiredText(input.roomId, 'room_id', 512),
                threadRootEventId: optionalText(input.threadRootEventId, 512),
                senderAgentName: requiredText(input.senderAgentName, 'sender_agent_name', 255),
                body: requiredText(input.body, 'body', 4000),
            });
            return { ok: true };
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    initializeIngestionCursor(sourceInput, cursorInput) {
        const source = requiredText(sourceInput, 'source', 255);
        const cursor = requiredText(cursorInput, 'cursor_value', 2048);
        const inserted = this.db.prepare('INSERT OR IGNORE INTO ingestion_cursors(source, cursor_value, updated_at) VALUES (?, ?, ?)').run(source, cursor, this.now());
        const row = this.db.prepare('SELECT cursor_value FROM ingestion_cursors WHERE source = ?').get(source);
        if (!row)
            throw new Error('ingestion cursor disappeared after initialization');
        return { created: inserted.changes > 0, value: row.cursor_value };
    }
    readIngestionCursor(sourceInput) {
        const source = requiredText(sourceInput, 'source', 255);
        return this.db.prepare('SELECT cursor_value FROM ingestion_cursors WHERE source = ?').get(source)?.cursor_value ?? null;
    }
    advanceIngestionCursor(sourceInput, cursorInput) {
        const source = requiredText(sourceInput, 'source', 255);
        const cursor = requiredText(cursorInput, 'cursor_value', 2048);
        this.db.prepare(`INSERT INTO ingestion_cursors(source, cursor_value, updated_at) VALUES (?, ?, ?)
       ON CONFLICT(source) DO UPDATE SET cursor_value = excluded.cursor_value,
       updated_at = excluded.updated_at`).run(source, cursor, this.now());
    }
    storeMessageInternal(input) {
        const messageId = requiredText(input.messageId, 'message_id', 512);
        const body = requiredText(input.normalizedBody, 'normalized_body', 100_000);
        const roomId = requiredText(input.roomId, 'room_id', 512);
        const contentDigest = digest({
            roomId,
            matrixEventId: optionalText(input.matrixEventId, 512),
            threadRootEventId: optionalText(input.threadRootEventId, 512),
            senderMxid: optionalText(input.senderMxid, 512),
            senderName: optionalText(input.senderName, 255),
            body,
        });
        const previous = this.db.prepare('SELECT * FROM router_messages WHERE message_id = ?').get(messageId);
        if (previous) {
            if (previous.content_digest !== contentDigest) {
                return refusal('idempotency_conflict', 'message id was already ingested with different content');
            }
            return { ok: true, created: false, messageId };
        }
        this.db.prepare(`INSERT INTO router_messages(
      message_id, room_id, matrix_event_id, thread_root_event_id,
      sender_mxid, sender_name, normalized_body, content_digest, received_at,
      explicit_task
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`).run(messageId, roomId, optionalText(input.matrixEventId, 512), optionalText(input.threadRootEventId, 512), optionalText(input.senderMxid, 512), optionalText(input.senderName, 255), body, contentDigest, input.receivedAt ?? this.now(), input.explicitTask === true ? 1 : 0);
        this.emit('message.stored', { messageId, explicitTask: input.explicitTask === true });
        return { ok: true, created: true, messageId };
    }
    storeTaskMessage(input) {
        try {
            return this.db.transaction(() => this.storeMessageInternal(input))();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    ingestMessage(input) {
        try {
            const tx = this.db.transaction(() => {
                const stored = this.storeMessageInternal(input);
                if (!stored.ok)
                    return stored;
                const roomId = requiredText(input.roomId, 'room_id', 512);
                const resolved = this.resolveSessionInternal({
                    agentId: input.recipientAgentId,
                    agentName: input.recipientAgentName,
                    roomId,
                    threadRootEventId: optionalText(input.threadRootEventId, 512),
                });
                const projection = this.db.prepare('INSERT OR IGNORE INTO session_messages(session_id, message_id, projected_at) VALUES (?, ?, ?)').run(resolved.row.session_id, stored.messageId, this.now());
                if (projection.changes === 1) {
                    this.emit('message.ingested', {
                        messageId: stored.messageId,
                        sessionId: resolved.row.session_id,
                        explicitTask: input.explicitTask === true,
                    });
                }
                return {
                    ok: true,
                    created: stored.created || projection.changes === 1,
                    messageId: stored.messageId,
                    session: sessionView(resolved.row),
                };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    createTaskIntent(input) {
        try {
            const tx = this.db.transaction(() => {
                const requestScope = requiredText(input.requestScope, 'request_scope', 255);
                const requestKey = requiredText(input.requestKey, 'request_key', 512);
                const roomId = requiredText(input.roomId, 'room_id', 512);
                const rootEvent = requiredText(input.threadRootEventId, 'thread_root_event_id', 512);
                const rootMessageId = requiredText(input.rootMessageId, 'root_message_id', 512);
                const title = requiredText(input.task.title, 'task.title', 255);
                const assigneeAgentId = requiredText(input.task.assigneeAgentId, 'assignee_agent_id', 255);
                const assigneeName = requiredText(input.task.assigneeName, 'assignee_name', 255);
                const uniqueMessageIds = [...new Set([rootMessageId, ...input.inputMessageIds])];
                const requestPayload = {
                    roomId,
                    rootEvent,
                    rootMessageId,
                    inputMessageIds: uniqueMessageIds,
                    task: {
                        title,
                        description: optionalText(input.task.description, 4096) ?? '',
                        priority: input.task.priority ?? 'p2',
                        granularity: input.task.granularity ?? 'task',
                        assigneeAgentId,
                        assigneeName,
                        creatorAgentId: optionalText(input.task.creatorAgentId, 255),
                        createdBy: optionalText(input.task.createdBy, 255),
                        parentId: optionalText(input.task.parentId, 255),
                        labels: [...(input.task.labels ?? [])],
                    },
                    acknowledgementBody: optionalText(input.acknowledgementBody, 4096) ?? `Task created: ${title}`,
                };
                const requestDigest = digest(requestPayload);
                const prior = this.db.prepare('SELECT * FROM task_bindings WHERE request_scope = ? AND request_key = ?').get(requestScope, requestKey);
                if (prior) {
                    if (prior.request_digest !== requestDigest) {
                        return refusal('idempotency_conflict', 'task request key was reused with different content');
                    }
                    const command = this.db.prepare('SELECT * FROM matrix_outbox WHERE task_id = ?').get(prior.task_id);
                    if (!command)
                        throw new Error('task binding is missing Matrix command');
                    return {
                        ok: true,
                        replayed: true,
                        taskId: prior.task_id,
                        commandId: command.command_id,
                        transactionId: command.txn_id,
                        activationState: 'pending_thread',
                    };
                }
                const root = this.db.prepare('SELECT * FROM router_messages WHERE message_id = ?').get(rootMessageId);
                if (!root || root.room_id !== roomId || !root.matrix_event_id
                    || (root.thread_root_event_id ?? root.matrix_event_id) !== rootEvent) {
                    return refusal('bad_request', 'task root must match the authenticated Matrix root of the source input');
                }
                // A source may itself be a thread reply. Preserve that source input,
                // but acknowledgements must address its existing top-level thread.
                // Refuse a known nested target instead of perpetuating invalid history.
                const nestedRoot = this.db.prepare(`SELECT message_id AS id FROM router_messages WHERE room_id = ? AND matrix_event_id = ?
           AND thread_root_event_id IS NOT NULL LIMIT 1`).get(roomId, rootEvent);
                if (nestedRoot) {
                    return refusal('bad_request', 'task thread root must be a top-level Matrix event');
                }
                for (const messageId of uniqueMessageIds) {
                    const message = this.db.prepare('SELECT * FROM router_messages WHERE message_id = ?').get(messageId);
                    if (!message || message.room_id !== roomId) {
                        return refusal('bad_request', `task input is missing or belongs to another room: ${messageId}`);
                    }
                }
                const existingThreadTask = this.db.prepare(`SELECT task_id FROM task_bindings
           WHERE assignee_agent_id = ? AND room_id = ? AND thread_root_event_id = ?`).get(assigneeAgentId, roomId, rootEvent);
                if (existingThreadTask) {
                    return refusal('bad_request', 'this assignee already has a task bound to the originating Matrix thread');
                }
                const parentId = optionalText(input.task.parentId, 255);
                if (parentId && !this.db.prepare('SELECT task_id AS id FROM tasks WHERE task_id = ?').get(parentId)) {
                    return refusal('bad_request', 'parent task does not exist');
                }
                const taskId = optionalText(input.task.taskId, 255) ?? `task_${randomUUID()}`;
                const nowIso = new Date(this.now()).toISOString();
                this.db.prepare(`INSERT INTO tasks(
          task_id, title, description, status, priority, granularity,
          assignee_agent_id, assignee_name, created_by, parent_id, labels_json,
          comments_json, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`).run(taskId, title, requestPayload.task.description, 'created', requestPayload.task.priority, requestPayload.task.granularity, assigneeAgentId, assigneeName, requestPayload.task.createdBy, parentId, canonicalJson(requestPayload.task.labels), '[]', nowIso, nowIso);
                this.db.prepare(`INSERT INTO task_bindings(
          task_id, creator_agent_id, assignee_agent_id, room_id,
          thread_root_event_id, activation_state, request_scope, request_key,
          request_digest
        ) VALUES (?, ?, ?, ?, ?, 'pending_thread', ?, ?, ?)`).run(taskId, requestPayload.task.creatorAgentId, assigneeAgentId, roomId, rootEvent, requestScope, requestKey, requestDigest);
                const now = this.now();
                for (const messageId of uniqueMessageIds) {
                    this.db.prepare('INSERT INTO task_inputs(task_id, message_id, role, attached_at) VALUES (?, ?, ?, ?)').run(taskId, messageId, messageId === rootMessageId ? 'root' : 'supplement', now);
                }
                const commandId = `matrix_${randomUUID()}`;
                const txnId = `hagency_${digest(commandId).slice(0, 40)}`;
                const commandPayload = {
                    roomId,
                    threadRootEventId: rootEvent,
                    body: requestPayload.acknowledgementBody,
                    senderAgentName: assigneeName,
                };
                const payloadJson = canonicalJson(commandPayload);
                this.db.prepare(`INSERT INTO matrix_outbox(command_id, task_id, txn_id, payload_json,
            payload_digest, state) VALUES (?, ?, ?, ?, ?, 'pending')`).run(commandId, taskId, txnId, payloadJson, digest(payloadJson));
                this.emit('task.intent_created', { taskId, commandId, roomId, threadRootEventId: rootEvent });
                return {
                    ok: true,
                    replayed: false,
                    taskId,
                    commandId,
                    transactionId: txnId,
                    activationState: 'pending_thread',
                };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    attachTaskInputs(input) {
        try {
            const tx = this.db.transaction(() => {
                const taskId = requiredText(input.taskId, 'task_id', 255);
                const requestScope = requiredText(input.requestScope, 'request_scope', 255);
                const requestKey = requiredText(input.requestKey, 'request_key', 512);
                const messageIds = [...new Set(input.messageIds.map((id) => requiredText(id, 'message_id', 512)))];
                const requestDigest = digest({ taskId, messageIds });
                const previous = this.db.prepare('SELECT request_digest, attached_count FROM task_input_requests WHERE request_scope = ? AND request_key = ?').get(requestScope, requestKey);
                if (previous) {
                    if (previous.request_digest !== requestDigest) {
                        return refusal('idempotency_conflict', 'task input request key was reused with different content');
                    }
                    return { ok: true, replayed: true, attached: previous.attached_count };
                }
                const binding = this.db.prepare('SELECT * FROM task_bindings WHERE task_id = ?').get(taskId);
                if (!binding)
                    return refusal('not_found', 'task binding not found');
                let attached = 0;
                for (const messageId of messageIds) {
                    const message = this.db.prepare('SELECT * FROM router_messages WHERE message_id = ?').get(messageId);
                    if (!message || message.room_id !== binding.room_id) {
                        return refusal('bad_request', `task input is missing or belongs to another room: ${messageId}`);
                    }
                    const result = this.db.prepare(`INSERT OR IGNORE INTO task_inputs(task_id, message_id, role, attached_at, activated_at)
             VALUES (?, ?, 'supplement', ?, ?)`).run(taskId, messageId, this.now(), binding.activation_state === 'active' ? this.now() : null);
                    attached += result.changes;
                }
                this.db.prepare(`INSERT INTO task_input_requests(request_scope, request_key, task_id,
            request_digest, attached_count, created_at) VALUES (?, ?, ?, ?, ?, ?)`).run(requestScope, requestKey, taskId, requestDigest, attached, this.now());
                this.emit('task.inputs_attached', { taskId, attached });
                return { ok: true, replayed: false, attached };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    claimMatrixCommand(claimMs = 30_000) {
        const tx = this.db.transaction(() => {
            const now = this.now();
            const row = this.db.prepare(`SELECT * FROM matrix_outbox
         WHERE state = 'pending' OR (state = 'claimed' AND claimed_until < ?)
         ORDER BY rowid LIMIT 1`).get(now);
            if (!row)
                return null;
            const claimToken = this.randomBytes(32).toString('base64url');
            const claimUntil = now + Math.max(1_000, claimMs);
            this.db.prepare("UPDATE matrix_outbox SET state = 'claimed', claim_token_hash = ?, claimed_until = ? WHERE command_id = ?").run(digest(claimToken), claimUntil, row.command_id);
            const payload = safeParseObject(row.payload_json);
            const roomId = typeof payload.roomId === 'string' ? payload.roomId : '';
            const threadRootEventId = typeof payload.threadRootEventId === 'string' ? payload.threadRootEventId : '';
            const body = typeof payload.body === 'string' ? payload.body : '';
            const senderAgentName = typeof payload.senderAgentName === 'string' ? payload.senderAgentName : '';
            if (!roomId || !threadRootEventId || !body || !senderAgentName)
                throw new Error('stored Matrix command is corrupt');
            const task = this.db.prepare('SELECT created_at FROM tasks WHERE task_id=?').get(row.task_id);
            return {
                sourceCreatedAt: task ? Date.parse(task.created_at) : null,
                commandId: row.command_id,
                taskId: row.task_id,
                transactionId: row.txn_id,
                roomId,
                threadRootEventId,
                body,
                senderAgentName,
                payloadDigest: row.payload_digest,
                claimToken,
                claimUntil,
            };
        });
        return tx();
    }
    recordMatrixDelivery(input) {
        try {
            const tx = this.db.transaction(() => {
                const commandId = requiredText(input.commandId, 'command_id', 255);
                const eventId = requiredText(input.eventId, 'event_id', 512);
                const row = this.db.prepare('SELECT * FROM matrix_outbox WHERE command_id = ?').get(commandId);
                if (!row)
                    return refusal('not_found', 'Matrix command not found');
                const binding = this.db.prepare('SELECT * FROM task_bindings WHERE task_id = ?').get(row.task_id);
                if (!binding)
                    throw new Error('Matrix command task binding is missing');
                const task = this.db.prepare('SELECT assignee_name FROM tasks WHERE task_id = ?').get(binding.task_id);
                if (!task?.assignee_name)
                    throw new Error('task has no assignee name');
                if (row.state === 'delivered') {
                    if (row.delivered_event_id !== eventId) {
                        return refusal('matrix_command_conflict', 'Matrix command was already delivered with another event id');
                    }
                    const session = this.findThreadSession(binding.assignee_agent_id, binding.room_id, binding.thread_root_event_id);
                    if (!session)
                        throw new Error('delivered task is missing thread session');
                    return {
                        ok: true,
                        replayed: true,
                        taskId: binding.task_id,
                        sessionId: session.session_id,
                        agentId: binding.assignee_agent_id,
                        agentName: task.assignee_name,
                        roomId: binding.room_id,
                        threadRootEventId: binding.thread_root_event_id,
                        threadAnchorEventId: eventId,
                    };
                }
                if (row.state !== 'claimed' || row.claim_token_hash !== digest(input.claimToken)) {
                    return refusal('matrix_command_conflict', 'Matrix command claim is absent or stale');
                }
                const session = this.resolveSessionInternal({
                    agentId: binding.assignee_agent_id,
                    agentName: task.assignee_name,
                    roomId: binding.room_id,
                    threadRootEventId: binding.thread_root_event_id,
                });
                this.db.prepare("UPDATE matrix_outbox SET state = 'delivered', delivered_event_id = ?, claim_token_hash = NULL, claimed_until = NULL WHERE command_id = ?").run(eventId, commandId);
                this.db.prepare("UPDATE task_bindings SET activation_state = 'active', thread_anchor_event_id = ? WHERE task_id = ?").run(eventId, binding.task_id);
                this.db.prepare('UPDATE task_inputs SET activated_at = ? WHERE task_id = ? AND activated_at IS NULL').run(this.now(), binding.task_id);
                this.db.prepare(`INSERT OR IGNORE INTO session_messages(session_id, message_id, projected_at)
           SELECT ?, message_id, ? FROM task_inputs WHERE task_id = ? AND activated_at IS NOT NULL`).run(session.row.session_id, this.now(), binding.task_id);
                this.emit('task.activated', {
                    taskId: binding.task_id,
                    sessionId: session.row.session_id,
                    threadRootEventId: binding.thread_root_event_id,
                    threadAnchorEventId: eventId,
                });
                return {
                    ok: true,
                    replayed: false,
                    taskId: binding.task_id,
                    sessionId: session.row.session_id,
                    agentId: binding.assignee_agent_id,
                    agentName: task.assignee_name,
                    roomId: binding.room_id,
                    threadRootEventId: binding.thread_root_event_id,
                    threadAnchorEventId: eventId,
                };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    recordMatrixFailure(input) {
        try {
            const tx = this.db.transaction(() => {
                const commandId = requiredText(input.commandId, 'command_id', 255);
                const errorCode = requiredText(input.errorCode, 'error_code', 120);
                const row = this.db.prepare('SELECT * FROM matrix_outbox WHERE command_id = ?').get(commandId);
                if (!row)
                    return refusal('not_found', 'Matrix command not found');
                if (row.state === 'failed') {
                    return { ok: true, replayed: true, taskId: row.task_id, activationState: 'thread_delivery_failed' };
                }
                if (row.state !== 'claimed' || row.claim_token_hash !== digest(input.claimToken)) {
                    return refusal('matrix_command_conflict', 'Matrix command claim is absent or stale');
                }
                this.db.prepare("UPDATE matrix_outbox SET state = 'failed', last_error = ?, claim_token_hash = NULL, claimed_until = NULL WHERE command_id = ?").run(errorCode, commandId);
                this.db.prepare("UPDATE task_bindings SET activation_state = 'thread_delivery_failed' WHERE task_id = ?").run(row.task_id);
                this.enqueueTaskNotice(row.task_id, `task_thread_delivery_failed:${row.task_id}`, 'Task creation failed: the Matrix task thread could not be created. No coding work was started.');
                this.emit('task.thread_delivery_failed', { taskId: row.task_id, errorCode });
                return { ok: true, replayed: false, taskId: row.task_id, activationState: 'thread_delivery_failed' };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    recordTaskDispatchFailure(taskIdInput, errorCodeInput) {
        try {
            const tx = this.db.transaction(() => {
                const taskId = requiredText(taskIdInput, 'task_id', 255);
                const errorCode = requiredText(errorCodeInput, 'error_code', 120);
                if (!/^[a-z0-9_.-]+$/.test(errorCode)) {
                    return refusal('bad_request', 'error_code must be a stable machine code');
                }
                const binding = this.db.prepare('SELECT * FROM task_bindings WHERE task_id = ?').get(taskId);
                if (!binding)
                    return refusal('not_found', 'task binding not found');
                if (binding.activation_state !== 'active') {
                    return refusal('invalid_transition', 'only an active task can record a dispatch failure');
                }
                const dedupeKey = `task_dispatch_failed:${taskId}:${errorCode}`;
                const replayed = Boolean(this.db.prepare('SELECT command_id AS id FROM notice_outbox WHERE dedupe_key = ?').get(dedupeKey));
                const nowIso = new Date(this.now()).toISOString();
                this.db.prepare(`UPDATE tasks SET status = 'blocked', waiting_reason = ?, waiting_until = NULL,
           updated_at = ? WHERE task_id = ? AND status != ?`).run('task thread is active but no coding runner could be started', nowIso, taskId, 'done');
                this.enqueueTaskNotice(taskId, dedupeKey, 'Task thread created, but execution could not start. No coding runner was launched; inspect the agent and workspace configuration before retrying.');
                if (!replayed)
                    this.emit('task.dispatch_failed', { taskId, errorCode });
                return { ok: true, replayed, taskId, state: 'blocked' };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    findThreadSession(agentId, roomId, root) {
        return this.db.prepare("SELECT * FROM sessions WHERE agent_id = ? AND room_id = ? AND scope_kind = 'thread' AND thread_root_event_id = ?").get(agentId, roomId, root);
    }
    findThreadTaskBinding(agentIdInput, roomIdInput, rootInput) {
        try {
            const agentId = requiredText(agentIdInput, 'agent_id', 255);
            const roomId = requiredText(roomIdInput, 'room_id', 512);
            const root = requiredText(rootInput, 'thread_root_event_id', 512);
            const bindings = this.db.prepare(`SELECT * FROM task_bindings WHERE assignee_agent_id = ? AND room_id = ?
         AND thread_root_event_id = ?`).all(agentId, roomId, root);
            if (bindings.length > 1)
                return refusal('missing_task_credential', 'multiple task bindings make this thread ambiguous');
            const binding = bindings[0];
            return binding ? { taskId: binding.task_id, activationState: binding.activation_state } : null;
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    findActiveTaskBinding(agentIdInput, roomIdInput, rootInput) {
        try {
            const agentId = requiredText(agentIdInput, 'agent_id', 255);
            const roomId = requiredText(roomIdInput, 'room_id', 512);
            const root = requiredText(rootInput, 'thread_root_event_id', 512);
            const bindings = this.db.prepare(`SELECT * FROM task_bindings WHERE assignee_agent_id = ? AND room_id = ?
         AND thread_root_event_id = ? AND activation_state = 'active'`).all(agentId, roomId, root);
            if (bindings.length > 1) {
                return refusal('missing_task_credential', 'multiple active task bindings make this thread ambiguous');
            }
            const binding = bindings[0];
            if (!binding || !binding.thread_anchor_event_id)
                return refusal('missing_task_binding', 'no active task binding for this thread');
            const session = this.findThreadSession(agentId, roomId, root);
            if (!session)
                throw new Error('active task binding is missing its session');
            return {
                taskId: binding.task_id,
                sessionId: session.session_id,
                agentId,
                agentName: session.agent_name,
                roomId,
                threadRootEventId: root,
                threadAnchorEventId: binding.thread_anchor_event_id,
            };
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    enqueueDispatch(input) {
        try {
            const tx = this.db.transaction(() => {
                const sessionId = requiredText(input.sessionId, 'session_id', 255);
                const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(sessionId);
                if (!session)
                    return refusal('not_found', 'session not found');
                if (input.framework === 'octos') {
                    return refusal('unsupported_framework', 'Octos does not support disposable thread-session runners');
                }
                const serverId = optionalText(input.serverId, 255);
                if (serverId && serverId !== requiredText(input.localServerId, 'local_server_id', 255)) {
                    return refusal('remote_runner_unsupported', 'thread-session runners are local-only in v1');
                }
                const taskId = optionalText(input.taskId, 255);
                if (input.mayWrite === true) {
                    // Write authority has exactly two sources: an active task binding,
                    // or an operator's audited per-session mode grant. Both funnel
                    // through the same workspace-lease machinery, so the concurrency
                    // guarantees are identical either way.
                    if (!taskId && session.mode_override !== 'auto') {
                        return refusal('missing_task_credential', 'coding dispatch requires a task id or an operator mode grant');
                    }
                    if (taskId) {
                        const binding = this.db.prepare('SELECT * FROM task_bindings WHERE task_id = ?').get(taskId);
                        if (!binding)
                            return refusal('missing_task_binding', 'task has no execution binding');
                        if (binding.activation_state !== 'active' || !binding.thread_anchor_event_id) {
                            return refusal('task_not_active', 'task thread is not durably active');
                        }
                        if (session.agent_id !== binding.assignee_agent_id
                            || session.room_id !== binding.room_id
                            || session.thread_root_event_id !== binding.thread_root_event_id) {
                            return refusal('missing_task_credential', 'task binding does not authorize this session');
                        }
                    }
                }
                if ('reply_to' in input.payload || 'replyTarget' in input.payload || 'reply_target' in input.payload) {
                    return refusal('bad_request', 'runner payload must not contain a reply target');
                }
                const workspaceResourceId = optionalText(input.workspaceResourceId, 512);
                const named = [...new Set((input.namedResourceIds ?? []).map((id) => requiredText(id, 'resource_id', 512)))];
                if (input.mayWrite === true && !workspaceResourceId) {
                    return refusal('bad_request', 'writing dispatch requires a workspace resource');
                }
                if (workspaceResourceId)
                    this.ensureResource(workspaceResourceId, 'workspace');
                for (const id of named)
                    this.ensureResource(id, 'named');
                const pendingMessages = taskId
                    ? this.db.prepare(`SELECT sm.message_id FROM session_messages sm
               JOIN task_inputs ti ON ti.message_id = sm.message_id AND ti.task_id = ?
               LEFT JOIN dispatch_messages dm
                 ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
               WHERE sm.session_id = ? AND sm.processed_at IS NULL AND dm.message_id IS NULL
               ORDER BY sm.projected_at, sm.message_id`).all(taskId, sessionId)
                    : this.db.prepare(`SELECT sm.message_id FROM session_messages sm
               JOIN router_messages m ON m.message_id = sm.message_id
               LEFT JOIN dispatch_messages dm
                 ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
               WHERE sm.session_id = ? AND sm.processed_at IS NULL AND dm.message_id IS NULL
                 AND m.explicit_task = 0
               ORDER BY sm.projected_at, sm.message_id`).all(sessionId);
                const queued = taskId
                    ? this.db.prepare("SELECT * FROM dispatches WHERE session_id = ? AND task_id = ? AND state = 'queued' ORDER BY created_at LIMIT 1").get(sessionId, taskId)
                    : this.db.prepare("SELECT * FROM dispatches WHERE session_id = ? AND task_id IS NULL AND state = 'queued' ORDER BY created_at LIMIT 1").get(sessionId);
                if (queued) {
                    for (const message of pendingMessages) {
                        this.db.prepare('INSERT INTO dispatch_messages(dispatch_id, session_id, message_id, assigned_at) VALUES (?, ?, ?, ?)').run(queued.dispatch_id, sessionId, message.message_id, this.now());
                    }
                    if (pendingMessages.length > 0) {
                        this.emit('dispatch.batch_extended', { dispatchId: queued.dispatch_id, messageCount: pendingMessages.length });
                    }
                    return { ok: true, dispatchId: queued.dispatch_id, state: 'queued' };
                }
                if (pendingMessages.length === 0) {
                    const previous = taskId
                        ? this.db.prepare(`SELECT * FROM dispatches WHERE session_id = ? AND task_id = ?
                 AND state != 'cancelled_before_start' ORDER BY created_at DESC LIMIT 1`).get(sessionId, taskId)
                        : this.db.prepare(`SELECT * FROM dispatches WHERE session_id = ? AND task_id IS NULL
                 AND state != 'cancelled_before_start' ORDER BY created_at DESC LIMIT 1`).get(sessionId);
                    if (previous)
                        return { ok: true, dispatchId: previous.dispatch_id, state: previous.state };
                    return refusal('bad_request', 'session has no unassigned input to dispatch');
                }
                const dispatchId = randomUUID();
                const payloadJson = canonicalJson(input.payload);
                this.db.prepare(`INSERT INTO dispatches(
          dispatch_id, session_id, task_id, state, framework, payload_json,
          payload_digest, may_write, workspace_mode, workspace_resource_id,
          named_resources_json, created_at
        ) VALUES (?, ?, ?, 'queued', ?, ?, ?, ?, ?, ?, ?, ?)`).run(dispatchId, sessionId, taskId, input.framework, payloadJson, digest(payloadJson), input.mayWrite === true ? 1 : 0, input.workspaceMode ?? 'shared', workspaceResourceId, canonicalJson(named), this.now());
                for (const message of pendingMessages) {
                    this.db.prepare('INSERT INTO dispatch_messages(dispatch_id, session_id, message_id, assigned_at) VALUES (?, ?, ?, ?)').run(dispatchId, sessionId, message.message_id, this.now());
                }
                this.emit('dispatch.queued', {
                    dispatchId,
                    sessionId,
                    taskId,
                    mayWrite: input.mayWrite === true,
                    messageCount: pendingMessages.length,
                });
                return { ok: true, dispatchId, state: 'queued' };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    ensureResource(resourceId, kind) {
        this.db.prepare('INSERT OR IGNORE INTO resources(resource_id, kind, safe_label) VALUES (?, ?, ?)').run(resourceId, kind, resourceId.slice(0, 200));
    }
    registerWorkspace(input) {
        const tx = this.db.transaction(() => {
            const resourceId = requiredText(input.resourceId, 'resource_id', 512);
            const safeLabel = requiredText(input.safeLabel, 'safe_label', 255);
            const backendPath = path.resolve(requiredText(input.backendPath, 'backend_path', 4096));
            const branchName = optionalText(input.branchName, 512);
            const existing = this.db.prepare('SELECT kind, backend_path, branch_name FROM resources WHERE resource_id = ?').get(resourceId);
            if (existing) {
                if (existing.kind !== 'workspace') {
                    throw new Error(`resource ${resourceId} is already registered with another kind`);
                }
                if (existing.backend_path && path.resolve(existing.backend_path) !== backendPath) {
                    throw new Error(`workspace resource ${resourceId} cannot change its backend path`);
                }
                if (existing.branch_name && branchName && existing.branch_name !== branchName) {
                    throw new Error(`workspace resource ${resourceId} cannot change its branch`);
                }
                this.db.prepare(`UPDATE resources SET safe_label = ?, backend_path = COALESCE(backend_path, ?),
           branch_name = COALESCE(branch_name, ?) WHERE resource_id = ?`).run(safeLabel, backendPath, branchName, resourceId);
                return;
            }
            this.db.prepare(`INSERT INTO resources(resource_id, kind, safe_label, backend_path, branch_name)
         VALUES (?, 'workspace', ?, ?, ?)`).run(resourceId, safeLabel, backendPath, branchName);
        });
        tx();
    }
    claimDispatch(input) {
        return this.claimDispatchObserved(input).claim;
    }
    claimDispatchWithWake(input) {
        const { claim, cutoff } = this.claimDispatchObserved(input);
        return {
            claim,
            // A retry can become due between eligibility selection and this lookup.
            // Keep the actual selection cutoff so that work cannot miss both checks.
            nextAvailableAt: claim?.ok ? null : this.nextQueuedDispatchAfter(cutoff ?? this.now()),
        };
    }
    claimDispatchObserved(input) {
        let cutoff = null;
        try {
            const tx = this.db.transaction(() => {
                const runnerId = requiredText(input.runnerId, 'runner_id', 255);
                if (!Number.isFinite(input.leaseMs) || input.leaseMs <= 0)
                    return refusal('bad_request', 'lease_ms must be positive');
                if (!Number.isFinite(input.capabilityTtlMs) || input.capabilityTtlMs <= 0)
                    return refusal('bad_request', 'capability_ttl_ms must be positive');
                const live = this.db.prepare("SELECT COUNT(*) AS count FROM dispatches WHERE state IN ('leased','started','parked')").get()?.count ?? 0;
                if (live >= input.maxLiveRunners)
                    return refusal('live_runner_cap', 'live runner cap is full');
                const now = this.now();
                cutoff = now;
                this.db.prepare(`DELETE FROM resource_leases WHERE dispatch_id IN
           (SELECT dispatch_id FROM dispatches
            WHERE state IN ('queued','completed','cancelled_before_start','outcome_unknown'))`).run();
                const queued = this.db.prepare("SELECT * FROM dispatches WHERE state = 'queued' AND (available_at IS NULL OR available_at <= ?) ORDER BY created_at").all(now);
                for (const row of queued) {
                    const unresolved = this.db.prepare(`SELECT d.dispatch_id FROM dispatches d
             LEFT JOIN outcome_resolutions r ON r.dispatch_id = d.dispatch_id
             WHERE d.session_id = ? AND d.state = 'outcome_unknown' AND r.dispatch_id IS NULL
             ORDER BY d.settled_at DESC LIMIT 1`).get(row.session_id);
                    if (unresolved) {
                        this.enqueueThreadNotice(row, `session_quarantined:${row.dispatch_id}:${unresolved.dispatch_id}`, 'Waiting: a previous runner in this session stopped after work may have started. An operator must inspect and resolve that outcome before another turn can run.');
                        continue;
                    }
                    let completedFollowup = null;
                    if (row.task_id) {
                        const task = this.db.prepare('SELECT status, completed_at FROM tasks WHERE task_id = ?').get(row.task_id);
                        if (!task || task.status === 'blocked') {
                            if (task?.status === 'blocked') {
                                this.enqueueThreadNotice(row, `task_blocked:${row.dispatch_id}:${row.task_id}`, 'Waiting: this task is blocked and must be explicitly resumed by an operator before another runner can start.');
                            }
                            continue;
                        }
                        if (task.status === 'done') {
                            // A fresh human follow-up continues the unique task for this
                            // thread. Only already authenticated, unprocessed batch input
                            // from its original requester can reopen it; old work and peer
                            // messages never supply this authority. Check again at claim so
                            // a completion/arrival race and an existing queued dispatch after
                            // restart follow exactly the same path.
                            const followup = row.started_at === null ? this.db.prepare(`SELECT m.message_id AS id FROM dispatch_messages dm
                 JOIN dispatches d ON d.dispatch_id = dm.dispatch_id AND d.session_id = dm.session_id
                 JOIN sessions s ON s.session_id = d.session_id
                 JOIN task_bindings b ON b.task_id = d.task_id AND b.assignee_agent_id = s.agent_id
                   AND b.room_id = s.room_id AND b.thread_root_event_id = s.thread_root_event_id
                 JOIN task_inputs ti ON ti.task_id = b.task_id AND ti.message_id = dm.message_id AND ti.role = 'supplement'
                 JOIN router_messages m ON m.message_id = dm.message_id
                 JOIN session_messages sm ON sm.session_id = s.session_id AND sm.message_id = m.message_id
                 JOIN task_inputs original ON original.task_id = b.task_id AND original.role = 'root'
                 JOIN router_messages root ON root.message_id = original.message_id
                 WHERE d.dispatch_id = ? AND d.state = 'queued'
                   AND b.activation_state = 'active' AND b.thread_anchor_event_id IS NOT NULL
                   AND sm.processed_at IS NULL AND ti.activated_at IS NOT NULL
                   AND m.matrix_event_id IS NOT NULL AND m.sender_mxid IS NOT NULL
                   AND m.room_id = b.room_id AND m.thread_root_event_id = b.thread_root_event_id
                   AND root.room_id = b.room_id AND root.matrix_event_id IS NOT NULL
                   AND COALESCE(root.thread_root_event_id, root.matrix_event_id) = b.thread_root_event_id
                   AND m.sender_mxid = root.sender_mxid AND m.message_id != root.message_id
                 ORDER BY m.received_at, m.message_id LIMIT 1`).get(row.dispatch_id) : undefined;
                            if (!followup) {
                                this.enqueueThreadNotice(row, `completed_task_followup:${row.dispatch_id}:${row.task_id}`, 'This task is complete. To continue it, the original requester must send a new message mentioning the agent in this thread. Other project members can start a new task by mentioning the agent in the main room.');
                                continue;
                            }
                            completedFollowup = { messageId: followup.id, previousCompletedAt: task.completed_at };
                        }
                    }
                    const sameSessionRunning = this.db.prepare(`SELECT COUNT(*) AS count FROM dispatches
             WHERE session_id = ? AND state IN ('leased','started','parked')`).get(row.session_id)?.count ?? 0;
                    if (sameSessionRunning > 0)
                        continue;
                    const resources = [
                        ...(row.may_write === 1 && row.workspace_resource_id ? [row.workspace_resource_id] : []),
                        ...safeParseStringArray(row.named_resources_json),
                    ];
                    let available = true;
                    for (const resourceId of resources) {
                        const resource = this.db.prepare('SELECT dirty, safe_label, dirty_dispatch_id FROM resources WHERE resource_id = ?').get(resourceId);
                        if (resource?.dirty === 1) {
                            available = false;
                            this.enqueueThreadNotice(row, `workspace_quarantined:${row.dispatch_id}:${resourceId}:${resource.dirty_dispatch_id ?? 'unknown'}`, 'Waiting: this workspace is quarantined because a previous runner stopped after work may have started. An operator must inspect and resolve that outcome before another writer can run.');
                            break;
                        }
                        const held = this.db.prepare('SELECT dispatch_id, lease_until FROM resource_leases WHERE resource_id = ?').get(resourceId);
                        if (held && held.dispatch_id !== row.dispatch_id) {
                            available = false;
                            const holder = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(held.dispatch_id);
                            if (holder?.state === 'parked') {
                                this.enqueueThreadNotice(row, `waiting_for_approval:${row.dispatch_id}:${holder.dispatch_id}`, 'Waiting: this task is queued because its workspace is held by another task awaiting owner approval.');
                            }
                            break;
                        }
                    }
                    if (!available)
                        continue;
                    if (completedFollowup && row.task_id) {
                        this.db.prepare(`UPDATE tasks SET status = 'in_progress', completed_at = NULL,
               waiting_reason = NULL, waiting_until = NULL, updated_at = ?
               WHERE task_id = ? AND status = 'done'`).run(new Date(now).toISOString(), row.task_id);
                        this.emit('task.reopened_for_followup', { taskId: row.task_id,
                            dispatchId: row.dispatch_id, messageId: completedFollowup.messageId,
                            previousCompletedAt: completedFollowup.previousCompletedAt });
                    }
                    const fence = row.fence_generation + 1;
                    const leaseUntil = now + Math.floor(input.leaseMs);
                    const rawCapability = this.randomBytes(32).toString('base64url');
                    const capabilityHash = digest(rawCapability);
                    this.db.prepare(`UPDATE dispatches SET state = 'leased', runner_id = ?, fence_generation = ?,
             lease_until = ?, available_at = NULL WHERE dispatch_id = ? AND state = 'queued'`).run(runnerId, fence, leaseUntil, row.dispatch_id);
                    this.db.prepare(`INSERT INTO runner_capabilities(capability_hash, dispatch_id, runner_id,
              fence_generation, expires_at) VALUES (?, ?, ?, ?, ?)`).run(capabilityHash, row.dispatch_id, runnerId, fence, now + Math.floor(input.capabilityTtlMs));
                    for (const resourceId of resources) {
                        this.db.prepare('INSERT INTO resource_leases(resource_id, dispatch_id, acquired_at, lease_until) VALUES (?, ?, ?, ?)').run(resourceId, row.dispatch_id, now, leaseUntil);
                    }
                    this.emit('dispatch.leased', { dispatchId: row.dispatch_id, runnerId, fenceGeneration: fence });
                    return {
                        ok: true,
                        dispatchId: row.dispatch_id,
                        runnerId,
                        fenceGeneration: fence,
                        capability: rawCapability,
                        leaseUntil,
                        workspaceResourceId: row.workspace_resource_id,
                    };
                }
                return null;
            });
            return { claim: tx(), cutoff };
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return { claim: refusal('bad_request', error.message), cutoff };
            throw error;
        }
    }
    nextQueuedDispatchAt() {
        return this.nextQueuedDispatchAfter(this.now());
    }
    nextQueuedDispatchAfter(cutoff) {
        const row = this.db.prepare(`SELECT MIN(available_at) AS available_at FROM dispatches
       WHERE state = 'queued' AND available_at IS NOT NULL AND available_at > ?`).get(cutoff);
        return row?.available_at ?? null;
    }
    getLaunchDescriptor(input) {
        const checked = this.validateCapability(input);
        if ('ok' in checked)
            return checked;
        const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(checked.dispatch.session_id);
        if (!session)
            throw new Error('dispatch session is missing');
        if (checked.dispatch.framework !== 'claude' && checked.dispatch.framework !== 'codex') {
            return refusal('unsupported_framework', 'dispatch framework has no local runner');
        }
        const resource = checked.dispatch.workspace_resource_id
            ? this.db.prepare('SELECT backend_path FROM resources WHERE resource_id = ?').get(checked.dispatch.workspace_resource_id)
            : undefined;
        if (!resource?.backend_path)
            return refusal('bad_request', 'dispatch has no registered workspace path');
        return {
            dispatchId: checked.dispatch.dispatch_id,
            framework: checked.dispatch.framework,
            cwd: resource.backend_path,
            agentId: session.agent_id,
            agentName: session.agent_name,
            sessionId: session.session_id,
            roomId: session.room_id,
            taskId: checked.dispatch.task_id,
            workspaceMode: checked.dispatch.workspace_mode,
            // The launcher needs this: a dispatch without write authority takes no
            // workspace lease and records no dirty state on an unknown outcome, so
            // it must not be handed a runtime that can write. Reading it from the
            // dispatch row keeps the sandbox decision and the lease decision on the
            // same fact instead of two independently derived ones.
            mayWrite: checked.dispatch.may_write === 1,
            modelOverride: session.model_override ?? null,
        };
    }
    validateCapability(input) {
        if (typeof input?.dispatchId !== 'string'
            || typeof input?.runnerId !== 'string'
            || typeof input?.capability !== 'string'
            || !Number.isInteger(input?.fenceGeneration)
            || input.fenceGeneration <= 0) {
            return refusal('bad_request', 'complete runner capability identity is required');
        }
        const dispatch = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(input.dispatchId);
        if (!dispatch)
            return refusal('not_found', 'dispatch not found');
        const row = this.db.prepare('SELECT * FROM runner_capabilities WHERE capability_hash = ?').get(digest(input.capability));
        if (!row || row.revoked_at !== null)
            return refusal('invalid_capability', 'runner capability is invalid or revoked');
        if (row.expires_at < this.now())
            return refusal('capability_expired', 'runner capability expired');
        if (row.dispatch_id !== input.dispatchId
            || row.runner_id !== input.runnerId
            || row.fence_generation !== input.fenceGeneration
            || dispatch.runner_id !== input.runnerId
            || dispatch.fence_generation !== input.fenceGeneration) {
            return refusal('dispatch_not_current', 'runner identity or fence generation is stale');
        }
        return { dispatch, capability: row };
    }
    takePayload(input) {
        try {
            const tx = this.db.transaction(() => {
                const checked = this.validateCapability(input);
                if ('ok' in checked)
                    return checked;
                if (checked.dispatch.state !== 'leased') {
                    return refusal('invalid_transition', 'payload may only be taken from a leased dispatch');
                }
                const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(checked.dispatch.session_id);
                if (!session)
                    throw new Error('dispatch session is missing');
                const now = this.now();
                this.db.prepare("UPDATE dispatches SET state = 'started', started_at = ? WHERE dispatch_id = ?").run(now, checked.dispatch.dispatch_id);
                if (checked.dispatch.task_id) {
                    const nowIso = new Date(now).toISOString();
                    this.db.prepare(`UPDATE tasks SET status = 'in_progress', started_at = COALESCE(started_at, ?), updated_at = ?
             WHERE task_id = ? AND status IN ('created','accepted')`).run(nowIso, nowIso, checked.dispatch.task_id);
                }
                const messages = this.db.prepare(`SELECT m.* FROM router_messages m
           JOIN dispatch_messages dm ON dm.message_id = m.message_id
           WHERE dm.dispatch_id = ? ORDER BY dm.assigned_at, m.received_at, m.message_id`).all(checked.dispatch.dispatch_id);
                const inbox = messages.map((message) => ({
                    messageId: message.message_id,
                    senderName: message.sender_name,
                    body: message.normalized_body,
                    receivedAt: message.received_at,
                }));
                const rawPayload = safeParseObject(checked.dispatch.payload_json);
                const requestedBudget = typeof rawPayload.rebuildTokenBudget === 'number'
                    ? rawPayload.rebuildTokenBudget
                    : 12_000;
                const context = this.buildRunnerContext(session, checked.dispatch.dispatch_id, checked.dispatch.task_id, requestedBudget);
                context.discussion = this.conversations.prepare(checked.dispatch.dispatch_id, session.room_id, session.agent_id);
                this.emit('dispatch.started', {
                    dispatchId: checked.dispatch.dispatch_id,
                    runnerId: input.runnerId,
                    fenceGeneration: input.fenceGeneration,
                });
                return {
                    ok: true,
                    dispatchId: checked.dispatch.dispatch_id,
                    sessionId: session.session_id,
                    taskId: checked.dispatch.task_id,
                    fenceGeneration: input.fenceGeneration,
                    payload: rawPayload,
                    inbox,
                    context,
                };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    buildRunnerContext(session, dispatchId, taskId, requestedTokenBudget) {
        const tokenBudget = Math.min(100_000, Math.max(500, Math.floor(requestedTokenBudget)));
        const totalCharBudget = tokenBudget * 4;
        const eligible = this.db.prepare(`SELECT DISTINCT m.* FROM router_messages m
       JOIN session_messages sm ON sm.message_id = m.message_id
       LEFT JOIN dispatch_messages dm
         ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
       WHERE sm.session_id = ? AND (sm.processed_at IS NOT NULL OR dm.dispatch_id = ?)
       ORDER BY m.received_at, m.message_id`).all(session.session_id, dispatchId);
        const activeTasks = this.db.prepare(`SELECT t.task_id, t.title, t.status, b.activation_state
        FROM tasks t JOIN task_bindings b ON b.task_id = t.task_id
        WHERE b.assignee_agent_id = ? AND t.status != 'done'
        ORDER BY t.updated_at DESC LIMIT 50`).all(session.agent_id);
        const coordinatorDigest = [];
        const coordinatorBudget = Math.min(4_000, Math.floor(totalCharBudget * 0.2));
        if (session.scope_kind === 'main') {
            for (const row of activeTasks) {
                const item = {
                    taskId: row.task_id,
                    title: truncateTextWithinJsonBudget(row.title, 512),
                    status: row.status,
                    activationState: row.activation_state,
                };
                if (JSON.stringify([...coordinatorDigest, item]).length > coordinatorBudget)
                    break;
                coordinatorDigest.push(item);
            }
        }
        const taskDigest = taskId
            ? this.db.prepare('SELECT task_id, title, description, status, priority FROM tasks WHERE task_id = ?').get(taskId)
            : undefined;
        let safeTaskDigest = null;
        if (taskDigest) {
            const taskBudget = Math.max(500, Math.min(4_000, Math.floor(totalCharBudget * 0.25)));
            const fixedTask = {
                taskId: taskDigest.task_id,
                title: truncateTextWithinJsonBudget(taskDigest.title, 512),
                description: '',
                status: taskDigest.status,
                priority: taskDigest.priority,
            };
            const descriptionBudget = Math.max(2, taskBudget - JSON.stringify(fixedTask).length + JSON.stringify('').length);
            safeTaskDigest = {
                ...fixedTask,
                description: truncateTextWithinJsonBudget(taskDigest.description, descriptionBudget),
            };
        }
        const nextContextGeneration = session.context_generation + 1;
        const fixedContext = {
            contextGeneration: nextContextGeneration,
            rollingSummary: '',
            messages: [],
            coordinatorDigest,
            taskDigest: safeTaskDigest,
            tokenBudget,
        };
        const availableForMessagesAndSummary = Math.max(0, totalCharBudget - JSON.stringify(fixedContext).length);
        const recentBudget = Math.floor(availableForMessagesAndSummary * 0.68);
        const selectedRows = [];
        const selectedMessages = [];
        for (let index = eligible.length - 1; index >= 0; index -= 1) {
            const message = eligible[index];
            if (!message)
                continue;
            const view = {
                messageId: message.message_id,
                senderName: message.sender_name,
                body: message.normalized_body,
                receivedAt: message.received_at,
            };
            const next = [view, ...selectedMessages];
            if (JSON.stringify(next).length <= recentBudget) {
                selectedRows.unshift(message);
                selectedMessages.unshift(view);
                continue;
            }
            if (selectedMessages.length === 0 && recentBudget > 2) {
                const emptyView = { ...view, body: '' };
                const bodyBudget = Math.max(2, recentBudget - JSON.stringify([emptyView]).length + JSON.stringify('').length);
                selectedRows.unshift(message);
                selectedMessages.unshift({
                    ...view,
                    body: truncateTextWithinJsonBudget(view.body, bodyBudget),
                });
            }
            break;
        }
        const excludedCount = Math.max(0, eligible.length - selectedRows.length);
        const excluded = eligible.slice(0, excludedCount);
        const contextWithoutSummary = {
            ...fixedContext,
            messages: selectedMessages,
        };
        const summaryJsonBudget = Math.max(2, totalCharBudget - JSON.stringify(contextWithoutSummary).length + JSON.stringify('').length);
        const summaryCandidate = summarizeEarlierMessages(excluded, Math.max(0, summaryJsonBudget - 2));
        const rollingSummary = truncateTextWithinJsonBudget(summaryCandidate, summaryJsonBudget, '\n…[older context truncated to rebuild budget]');
        let contextGeneration = session.context_generation;
        if (rollingSummary !== session.rolling_summary) {
            contextGeneration += 1;
            this.db.prepare('UPDATE sessions SET rolling_summary = ?, context_generation = ? WHERE session_id = ?').run(rollingSummary, contextGeneration, session.session_id);
            this.emit('session.context_rotated', { sessionId: session.session_id, contextGeneration });
        }
        const context = {
            contextGeneration,
            rollingSummary,
            messages: selectedMessages,
            coordinatorDigest,
            taskDigest: safeTaskDigest,
            tokenBudget,
        };
        if (JSON.stringify(context).length > totalCharBudget) {
            throw new Error('context assembler exceeded the configured rebuild budget');
        }
        return context;
    }
    readConversation(input, offset = 0) {
        const checked = this.validateCapability(input);
        if ('ok' in checked)
            return checked;
        if (checked.dispatch.state !== 'started')
            return refusal('invalid_transition', 'conversation reads require a started dispatch');
        try {
            return { ok: true, ...this.conversations.page(input.dispatchId, offset) };
        }
        catch (error) {
            return refusal('bad_request', String(error.message));
        }
    }
    acknowledgeRunnerEffect(input) {
        const tx = this.db.transaction(() => {
            const checked = this.validateCapability(input);
            if ('ok' in checked)
                return checked;
            if (checked.dispatch.state !== 'started')
                return refusal('invalid_transition', 'effect acknowledgement requires started state');
            this.db.prepare('UPDATE dispatches SET effect_ack_at = ? WHERE dispatch_id = ?').run(input.acceptedAt ?? this.now(), input.dispatchId);
            this.emit('dispatch.effect_acknowledged', { dispatchId: input.dispatchId });
            this.updateActivity(checked.dispatch, { phase: 'started' });
            return { ok: true };
        });
        return tx();
    }
    parkForApproval(input) {
        try {
            const tx = this.db.transaction(() => {
                const checked = this.validateCapability(input);
                if ('ok' in checked)
                    return checked;
                const approvalId = requiredText(input.approvalId, 'approval_id', 255);
                const operationDigest = requiredText(input.operationDigest, 'operation_digest', 128);
                const upstreamThreadId = optionalText(input.upstreamThreadId, 255);
                const upstreamTurnId = optionalText(input.upstreamTurnId, 255);
                const upstreamItemId = optionalText(input.upstreamItemId, 255);
                const upstreamRequestId = optionalText(input.upstreamRequestId, 255);
                const prior = this.db.prepare('SELECT * FROM approval_waits WHERE approval_id = ?').get(approvalId);
                if (prior) {
                    if (prior.dispatch_id !== input.dispatchId || prior.operation_digest !== operationDigest
                        || prior.upstream_thread_id !== upstreamThreadId || prior.upstream_turn_id !== upstreamTurnId
                        || prior.upstream_item_id !== upstreamItemId || prior.upstream_request_id !== upstreamRequestId) {
                        return refusal('approval_mismatch', 'approval id is already bound to another operation');
                    }
                    return { ok: true, replayed: true };
                }
                if (checked.dispatch.state !== 'started') {
                    return refusal('invalid_transition', 'only a started dispatch may park for approval');
                }
                if (upstreamThreadId !== null && upstreamTurnId !== null && upstreamRequestId !== null) {
                    const previousRequest = this.db.prepare(`SELECT approval_id AS id FROM approval_waits WHERE dispatch_id = ?
             AND upstream_thread_id = ? AND upstream_turn_id = ? AND upstream_request_id = ?`).get(input.dispatchId, upstreamThreadId, upstreamTurnId, upstreamRequestId);
                    if (previousRequest)
                        return refusal('approval_mismatch', 'runtime request is already bound to an approval');
                }
                const parked = this.db.prepare("SELECT COUNT(*) AS count FROM dispatches WHERE state = 'parked'").get()?.count ?? 0;
                if (parked >= input.maxParkedRunners) {
                    return refusal('parked_runner_cap', 'parked runner cap is full');
                }
                this.db.prepare(`INSERT INTO approval_waits(
          approval_id, dispatch_id, operation_digest, upstream_thread_id,
          upstream_turn_id, upstream_item_id, upstream_request_id, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`).run(approvalId, input.dispatchId, operationDigest, upstreamThreadId, upstreamTurnId, upstreamItemId, upstreamRequestId, this.now());
                this.db.prepare("UPDATE dispatches SET state = 'parked', parked_at = ? WHERE dispatch_id = ?").run(this.now(), input.dispatchId);
                this.emit('dispatch.parked', { dispatchId: input.dispatchId, approvalId });
                this.updateActivity(checked.dispatch, { phase: 'waiting' });
                return { ok: true, replayed: false };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    recordApprovalDecision(input) {
        try {
            const tx = this.db.transaction(() => {
                const decisionEventId = requiredText(input.decisionEventId, 'decision_event_id', 255);
                const prior = this.db.prepare('SELECT approval_id, dispatch_id, decision, payload_digest FROM approval_inbox WHERE decision_event_id = ?').get(decisionEventId);
                const payloadDigest = digest({
                    approvalId: input.approvalId,
                    dispatchId: input.dispatchId,
                    operationDigest: input.operationDigest,
                    decision: input.decision,
                });
                if (prior) {
                    if (prior.approval_id !== input.approvalId
                        || prior.dispatch_id !== input.dispatchId
                        || prior.decision !== input.decision
                        || prior.payload_digest !== payloadDigest) {
                        return refusal('approval_mismatch', 'decision event id was replayed with different content');
                    }
                    return { ok: true, replayed: true };
                }
                const wait = this.db.prepare('SELECT * FROM approval_waits WHERE approval_id = ?').get(input.approvalId);
                if (!wait || wait.dispatch_id !== input.dispatchId || wait.operation_digest !== input.operationDigest) {
                    return refusal('approval_mismatch', 'approval decision does not match the parked operation');
                }
                if (wait.decision !== null) {
                    if (wait.decision !== input.decision)
                        return refusal('approval_mismatch', 'approval decision is already final');
                    return { ok: true, replayed: true };
                }
                if (wait.resumed_at !== null)
                    return refusal('approval_mismatch', 'approval request was already consumed');
                const dispatch = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(input.dispatchId);
                if (!dispatch || dispatch.state !== 'parked') {
                    return refusal('dispatch_not_current', 'approval decision targets a dispatch that is no longer parked');
                }
                this.db.prepare(`INSERT INTO approval_inbox(decision_event_id, approval_id, dispatch_id,
            decision, payload_digest, applied_at) VALUES (?, ?, ?, ?, ?, ?)`).run(decisionEventId, input.approvalId, input.dispatchId, input.decision, payloadDigest, this.now());
                this.db.prepare('UPDATE approval_waits SET decision = ?, resolved_at = ? WHERE approval_id = ?').run(input.decision, this.now(), input.approvalId);
                this.emit('approval.applied', {
                    decisionEventId,
                    approvalId: input.approvalId,
                    dispatchId: input.dispatchId,
                    decision: input.decision,
                }, 'owner');
                return { ok: true, replayed: false };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    reconcileApprovalDecision(input) {
        try {
            const tx = this.db.transaction(() => {
                const approvalId = requiredText(input.approvalId, 'approval_id', 255);
                const decisionEventId = requiredText(input.decisionEventId, 'decision_event_id', 255);
                const wait = this.db.prepare('SELECT * FROM approval_waits WHERE approval_id = ?').get(approvalId);
                if (!wait)
                    return refusal('approval_mismatch', 'approval wait is not present in router state');
                const payloadDigest = digest({
                    approvalId,
                    dispatchId: wait.dispatch_id,
                    operationDigest: wait.operation_digest,
                    decision: input.decision,
                });
                const previous = this.db.prepare('SELECT payload_digest FROM approval_inbox WHERE decision_event_id = ?').get(decisionEventId);
                const dispatch = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(wait.dispatch_id);
                if (!dispatch)
                    return refusal('not_found', 'approval dispatch is missing');
                if (previous) {
                    if (previous.payload_digest !== payloadDigest)
                        return refusal('approval_mismatch', 'decision event digest changed');
                    return { ok: true, replayed: true, deliverable: dispatch.state === 'parked' && wait.resumed_at === null };
                }
                if (wait.decision !== null) {
                    if (wait.decision !== input.decision)
                        return refusal('approval_mismatch', 'approval decision is already final');
                    return { ok: true, replayed: true, deliverable: dispatch.state === 'parked' && wait.resumed_at === null };
                }
                this.db.prepare(`INSERT INTO approval_inbox(decision_event_id, approval_id, dispatch_id,
           decision, payload_digest, applied_at) VALUES (?, ?, ?, ?, ?, ?)`).run(decisionEventId, approvalId, wait.dispatch_id, input.decision, payloadDigest, this.now());
                this.db.prepare('UPDATE approval_waits SET decision = ?, resolved_at = ? WHERE approval_id = ?').run(input.decision, this.now(), approvalId);
                this.emit('approval.reconciled', {
                    decisionEventId,
                    approvalId,
                    dispatchId: wait.dispatch_id,
                    decision: input.decision,
                    deliverable: dispatch.state === 'parked' && wait.resumed_at === null,
                }, 'owner');
                return { ok: true, replayed: false, deliverable: dispatch.state === 'parked' && wait.resumed_at === null };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    approvalThreadOrigin(approvalId, agentId, roomId) {
        const row = this.db.prepare(`SELECT s.thread_root_event_id FROM approval_waits a
       JOIN dispatches d ON d.dispatch_id = a.dispatch_id
       JOIN sessions s ON s.session_id = d.session_id
       WHERE a.approval_id = ? AND s.agent_id = ? AND s.room_id = ? AND s.scope_kind = 'thread'`).get(approvalId, agentId, roomId);
        return row?.thread_root_event_id ?? null;
    }
    readApprovalDecision(input) {
        const checked = this.validateCapability(input);
        if ('ok' in checked)
            return checked;
        const wait = this.db.prepare('SELECT * FROM approval_waits WHERE approval_id = ?').get(input.approvalId);
        if (!wait || wait.dispatch_id !== input.dispatchId || wait.operation_digest !== input.operationDigest) {
            return refusal('approval_mismatch', 'approval does not match this dispatch and operation');
        }
        return { ok: true, decision: wait.decision };
    }
    resumeAfterApproval(input) {
        const tx = this.db.transaction(() => {
            const checked = this.validateCapability(input);
            if ('ok' in checked)
                return checked;
            if (checked.dispatch.state !== 'parked') {
                return refusal('invalid_transition', 'dispatch is not parked for approval');
            }
            const wait = this.db.prepare('SELECT * FROM approval_waits WHERE approval_id = ?').get(input.approvalId);
            if (!wait
                || wait.dispatch_id !== input.dispatchId
                || wait.operation_digest !== input.operationDigest
                || wait.decision === null
                || wait.resumed_at !== null) {
                return refusal('approval_mismatch', 'approval decision is not durably applied');
            }
            this.db.prepare('UPDATE approval_waits SET resumed_at = ? WHERE approval_id = ?').run(this.now(), input.approvalId);
            this.db.prepare("UPDATE dispatches SET state = 'started', parked_at = NULL WHERE dispatch_id = ?").run(input.dispatchId);
            this.emit('dispatch.approval_resumed', {
                dispatchId: input.dispatchId,
                approvalId: input.approvalId,
                decision: wait.decision,
            });
            this.updateActivity(checked.dispatch, { phase: 'resumed' });
            return { ok: true, decision: wait.decision };
        });
        return tx();
    }
    settleAndRelease(input) {
        try {
            const tx = this.db.transaction(() => {
                const checked = this.validateCapability(input);
                if ('ok' in checked) {
                    const stale = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(input.dispatchId);
                    if (stale && input.output) {
                        this.db.prepare('UPDATE dispatches SET fenced_output_json = ? WHERE dispatch_id = ?').run(canonicalJson(input.output), input.dispatchId);
                        this.emit('dispatch.output_fenced', {
                            dispatchId: input.dispatchId,
                            runnerId: input.runnerId,
                            fenceGeneration: input.fenceGeneration,
                        });
                    }
                    return checked;
                }
                if (checked.dispatch.state !== 'started') {
                    return refusal('invalid_transition', 'only a started dispatch may settle; parked approval state must resolve first');
                }
                const terminal = input.outcome;
                this.updateActivity(checked.dispatch, { phase: terminal === 'completed' ? 'completed' : 'interrupted' });
                const now = this.now();
                this.db.prepare(`UPDATE dispatches SET state = ?, settled_at = ?, terminal_reason = ?,
           output_json = ? WHERE dispatch_id = ?`).run(terminal, now, optionalText(input.reason, 1000), input.output ? canonicalJson(input.output) : null, input.dispatchId);
                this.db.prepare('UPDATE runner_capabilities SET revoked_at = ? WHERE dispatch_id = ? AND revoked_at IS NULL').run(now, input.dispatchId);
                this.db.prepare('DELETE FROM resource_leases WHERE dispatch_id = ?').run(input.dispatchId);
                if (checked.dispatch.workspace_resource_id
                    && terminal === 'outcome_unknown'
                    && (input.workspaceDirty === true || checked.dispatch.may_write === 1)) {
                    this.db.prepare(`UPDATE resources SET dirty = 1, dirty_reason = ?,
             dirty_generation = dirty_generation + 1, dirty_dispatch_id = ?, inspected_at = NULL
             WHERE resource_id = ?`).run(optionalText(input.reason, 1000) ?? 'dispatch outcome is unknown', input.dispatchId, checked.dispatch.workspace_resource_id);
                }
                if (terminal === 'completed') {
                    this.db.prepare(`UPDATE session_messages SET processed_at = ? WHERE session_id = ? AND message_id IN
             (SELECT message_id FROM dispatch_messages WHERE dispatch_id = ?)`).run(now, checked.dispatch.session_id, input.dispatchId);
                    const text = input.output && typeof input.output.text === 'string'
                        ? input.output.text.trim()
                        : '';
                    if (text) {
                        const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(checked.dispatch.session_id);
                        if (!session)
                            throw new Error('dispatch session is missing during reply creation');
                        const replyMessageId = `runner_reply:${input.dispatchId}`;
                        this.db.prepare(`INSERT OR IGNORE INTO router_messages(
              message_id, room_id, matrix_event_id, thread_root_event_id,
              sender_mxid, sender_name, normalized_body, content_digest,
              received_at, explicit_task
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0)`).run(replyMessageId, session.room_id, null, session.thread_root_event_id, null, session.agent_name, text, digest({ sessionId: session.session_id, dispatchId: input.dispatchId, text }), now);
                        this.db.prepare(`INSERT OR IGNORE INTO session_messages(
                session_id, message_id, projected_at, processed_at
              ) VALUES (?, ?, ?, ?)`).run(session.session_id, replyMessageId, now, now);
                        const commandId = `reply_${randomUUID()}`;
                        const txnId = `hagency_${digest(commandId).slice(0, 40)}`;
                        const payloadDigest = digest({
                            dispatchId: input.dispatchId,
                            roomId: session.room_id,
                            threadRootEventId: session.thread_root_event_id,
                            body: text,
                        });
                        this.db.prepare(`INSERT INTO reply_outbox(command_id, dispatch_id, txn_id, room_id,
               thread_root_event_id, body, sender_agent_name, payload_digest, state)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending')`).run(commandId, input.dispatchId, txnId, session.room_id, session.thread_root_event_id, text, session.agent_name, payloadDigest);
                    }
                }
                else {
                    this.db.prepare(`UPDATE session_messages SET processed_at = COALESCE(processed_at, ?) WHERE session_id = ? AND message_id IN
             (SELECT message_id FROM dispatch_messages WHERE dispatch_id = ?)`).run(now, checked.dispatch.session_id, input.dispatchId);
                    this.enqueueThreadNotice(checked.dispatch, `outcome_unknown:${input.dispatchId}`, 'Result uncertain: the runner stopped after work may have started. Inspect the workspace before retrying; this dispatch will not be run again automatically.');
                    if (checked.dispatch.task_id) {
                        const nowIso = new Date(now).toISOString();
                        this.db.prepare(`UPDATE tasks SET status = 'blocked', waiting_reason = ?, waiting_until = NULL,
               updated_at = ? WHERE task_id = ? AND status != ?`).run('dispatch outcome is unknown; inspect workspace before retrying', nowIso, checked.dispatch.task_id, 'done');
                    }
                }
                this.emit(`dispatch.${terminal}`, {
                    dispatchId: input.dispatchId,
                    reason: optionalText(input.reason, 1000),
                    workspaceDirty: terminal === 'outcome_unknown' && checked.dispatch.may_write === 1,
                });
                return terminal === 'completed'
                    ? { ok: true, state: 'completed', fenced: false }
                    : {
                        ok: true,
                        state: 'outcome_unknown',
                        fenced: false,
                        workspaceDirty: checked.dispatch.may_write === 1,
                    };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    claimReplyCommand(claimMs = 30_000) {
        const tx = this.db.transaction(() => {
            const now = this.now();
            const row = this.db.prepare(`SELECT * FROM reply_outbox
         WHERE state = 'pending' OR (state = 'claimed' AND claimed_until < ?)
         ORDER BY rowid LIMIT 1`).get(now);
            if (!row) {
                const notice = this.db.prepare(`SELECT * FROM notice_outbox
           WHERE (state = 'pending' OR (state = 'claimed' AND claimed_until < ?))
           AND NOT (dedupe_key LIKE 'activity:%' AND EXISTS (
             SELECT 1 FROM notice_outbox held WHERE held.dispatch_id = notice_outbox.dispatch_id
             AND held.dedupe_key LIKE 'activity:%' AND held.state = 'claimed'
             AND held.command_id != notice_outbox.command_id AND held.claimed_until >= ?))
           ORDER BY CASE WHEN dedupe_key LIKE 'activity:%' THEN 1 ELSE 0 END, created_at LIMIT 1`).get(now, now);
                if (!notice)
                    return null;
                const file = this.db.prepare('SELECT * FROM file_replies WHERE command_id = ?').get(notice.command_id);
                const claimToken = this.randomBytes(32).toString('base64url');
                const claimUntil = now + Math.max(1_000, claimMs);
                this.db.prepare("UPDATE notice_outbox SET state = 'claimed', claim_token_hash = ?, claimed_until = ? WHERE command_id = ?").run(digest(claimToken), claimUntil, notice.command_id);
                return {
                    sourceCreatedAt: notice.dispatch_id ? this.replySessionCreatedAt(notice.dispatch_id) : notice.created_at,
                    commandId: notice.command_id,
                    dispatchId: notice.dispatch_id,
                    transactionId: notice.txn_id,
                    roomId: notice.room_id,
                    threadRootEventId: notice.thread_root_event_id,
                    body: notice.body,
                    senderAgentName: notice.sender_agent_name,
                    payloadDigest: notice.payload_digest,
                    claimToken,
                    claimUntil,
                    ...(notice.dedupe_key.startsWith('activity:') ? {
                        activity: { replaceEventId: this.activity.read(notice.dispatch_id)?.anchor ?? null },
                    } : {}),
                    ...(file ? { file: { ...JSON.parse(file.manifest_json),
                            preparedContent: file.prepared_json ? JSON.parse(file.prepared_json) : null } } : {}),
                };
            }
            const claimToken = this.randomBytes(32).toString('base64url');
            const claimUntil = now + Math.max(1_000, claimMs);
            this.db.prepare("UPDATE reply_outbox SET state = 'claimed', claim_token_hash = ?, claimed_until = ? WHERE command_id = ?").run(digest(claimToken), claimUntil, row.command_id);
            return {
                sourceCreatedAt: this.replySessionCreatedAt(row.dispatch_id),
                commandId: row.command_id,
                dispatchId: row.dispatch_id,
                transactionId: row.txn_id,
                roomId: row.room_id,
                threadRootEventId: row.thread_root_event_id,
                body: row.body,
                senderAgentName: row.sender_agent_name,
                payloadDigest: row.payload_digest,
                claimToken,
                claimUntil,
            };
        });
        return tx();
    }
    replySessionCreatedAt(dispatchId) {
        return this.db.prepare(`SELECT s.created_at FROM sessions s
      JOIN dispatches d ON d.session_id=s.session_id WHERE d.dispatch_id=?`).get(dispatchId)?.created_at ?? null;
    }
    recordReplyDelivery(input) {
        try {
            const tx = this.db.transaction(() => {
                const commandId = requiredText(input.commandId, 'command_id', 255);
                const eventId = requiredText(input.eventId, 'event_id', 512);
                const row = this.db.prepare('SELECT * FROM reply_outbox WHERE command_id = ?').get(commandId);
                if (!row)
                    return this.recordNoticeDelivery(input);
                if (row.state === 'delivered') {
                    if (row.delivered_event_id !== eventId) {
                        return refusal('matrix_command_conflict', 'reply command was delivered with another event id');
                    }
                    return { ok: true, replayed: true };
                }
                if (row.state !== 'claimed' || row.claim_token_hash !== digest(input.claimToken)) {
                    return refusal('matrix_command_conflict', 'reply command claim is absent or stale');
                }
                this.db.prepare("UPDATE reply_outbox SET state = 'delivered', delivered_event_id = ?, claim_token_hash = NULL, claimed_until = NULL WHERE command_id = ?").run(eventId, commandId);
                this.emit('reply.delivered', { commandId, dispatchId: row.dispatch_id, eventId });
                this.conversations.delivered(row.dispatch_id);
                return { ok: true, replayed: false };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    recordReplyFailure(input) {
        try {
            const tx = this.db.transaction(() => {
                const commandId = requiredText(input.commandId, 'command_id', 255);
                const errorCode = requiredText(input.errorCode, 'error_code', 120);
                const row = this.db.prepare('SELECT * FROM reply_outbox WHERE command_id = ?').get(commandId);
                if (!row)
                    return this.recordNoticeFailure(input);
                if (row.state === 'failed')
                    return { ok: true, replayed: true };
                if (row.state !== 'claimed' || row.claim_token_hash !== digest(input.claimToken)) {
                    return refusal('matrix_command_conflict', 'reply command claim is absent or stale');
                }
                this.db.prepare(`UPDATE reply_outbox SET state = 'failed', last_error = ?,
           claim_token_hash = NULL, claimed_until = NULL WHERE command_id = ?`).run(errorCode, commandId);
                this.emit('reply.delivery_failed', { commandId, dispatchId: row.dispatch_id, errorCode });
                return { ok: true, replayed: false };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    cancelBeforeStart(dispatchIdInput, reasonInput) {
        try {
            const tx = this.db.transaction(() => {
                const dispatchId = requiredText(dispatchIdInput, 'dispatch_id', 255);
                const row = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(dispatchId);
                if (!row)
                    return refusal('not_found', 'dispatch not found');
                if (!['queued', 'leased'].includes(row.state)) {
                    return refusal('invalid_transition', 'started dispatches cannot be cleanly cancelled');
                }
                const now = this.now();
                const reason = optionalText(reasonInput, 1000);
                this.db.prepare("UPDATE dispatches SET state = 'cancelled_before_start', settled_at = ?, terminal_reason = ? WHERE dispatch_id = ?").run(now, reason, dispatchId);
                this.db.prepare('UPDATE runner_capabilities SET revoked_at = ? WHERE dispatch_id = ? AND revoked_at IS NULL').run(now, dispatchId);
                this.db.prepare('DELETE FROM resource_leases WHERE dispatch_id = ?').run(dispatchId);
                if (reason === 'runner_launch_failed') {
                    this.enqueueThreadNotice(row, `runner_launch_failed:${dispatchId}`, 'Runner did not start because its local runtime could not be launched. No work was executed; fix the runtime configuration and resend the request.');
                }
                this.db.prepare('DELETE FROM dispatch_messages WHERE dispatch_id = ?').run(dispatchId);
                this.emit('dispatch.cancelled_before_start', { dispatchId });
                return { ok: true, state: 'cancelled_before_start' };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    requeueBeforeStart(input, retryDelayMs = 0, reasonInput) {
        try {
            const tx = this.db.transaction(() => {
                const checked = this.validateCapability(input);
                if ('ok' in checked)
                    return checked;
                if (checked.dispatch.state !== 'leased') {
                    return refusal('invalid_transition', 'only an unstarted leased dispatch may be requeued');
                }
                if (!Number.isFinite(retryDelayMs) || retryDelayMs < 0 || retryDelayMs > 3_600_000) {
                    return refusal('bad_request', 'retry delay must be between 0 and 3600000ms');
                }
                const now = this.now();
                const retryAt = now + Math.floor(retryDelayMs);
                const reason = optionalText(reasonInput, 1000);
                this.db.prepare(`UPDATE dispatches SET state = 'queued', runner_id = NULL, lease_until = NULL,
           available_at = ?, launch_failures = launch_failures + 1, last_launch_error = ?
           WHERE dispatch_id = ?`).run(retryAt, reason, input.dispatchId);
                this.db.prepare('UPDATE runner_capabilities SET revoked_at = ? WHERE dispatch_id = ? AND revoked_at IS NULL').run(now, input.dispatchId);
                this.db.prepare('DELETE FROM resource_leases WHERE dispatch_id = ?').run(input.dispatchId);
                this.enqueueThreadNotice(checked.dispatch, `runner_launch_retry:${input.dispatchId}`, 'Runner could not start, but no work was executed and no input was lost. The dispatch remains queued and will retry automatically.');
                this.emit('dispatch.requeued_before_start', { dispatchId: input.dispatchId, retryAt });
                return { ok: true, state: 'queued', retryAt };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    markOutcomeUnknown(dispatchIdInput, reasonInput) {
        try {
            const tx = this.db.transaction(() => {
                const dispatchId = requiredText(dispatchIdInput, 'dispatch_id', 255);
                const reason = requiredText(reasonInput, 'reason', 1000);
                const row = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(dispatchId);
                if (!row)
                    return refusal('not_found', 'dispatch not found');
                if (!['started', 'parked'].includes(row.state)) {
                    return refusal('invalid_transition', 'outcome_unknown requires a started dispatch');
                }
                this.settleUnknownInternal(row, reason);
                return { ok: true, state: 'outcome_unknown' };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    settleUnknownInternal(row, reason) {
        this.updateActivity(row, { phase: 'interrupted' });
        const now = this.now();
        this.db.prepare("UPDATE dispatches SET state = 'outcome_unknown', settled_at = ?, terminal_reason = ? WHERE dispatch_id = ?").run(now, reason, row.dispatch_id);
        this.db.prepare('UPDATE runner_capabilities SET revoked_at = ? WHERE dispatch_id = ? AND revoked_at IS NULL').run(now, row.dispatch_id);
        this.db.prepare('DELETE FROM resource_leases WHERE dispatch_id = ?').run(row.dispatch_id);
        this.db.prepare(`UPDATE session_messages SET processed_at = COALESCE(processed_at, ?) WHERE session_id = ? AND message_id IN
       (SELECT message_id FROM dispatch_messages WHERE dispatch_id = ?)`).run(now, row.session_id, row.dispatch_id);
        if (row.workspace_resource_id && row.may_write === 1) {
            this.db.prepare(`UPDATE resources SET dirty = 1, dirty_reason = ?,
         dirty_generation = dirty_generation + 1, dirty_dispatch_id = ?, inspected_at = NULL
         WHERE resource_id = ?`).run(reason, row.dispatch_id, row.workspace_resource_id);
        }
        if (row.task_id) {
            const nowIso = new Date(now).toISOString();
            this.db.prepare(`UPDATE tasks SET status = 'blocked', waiting_reason = ?, waiting_until = NULL,
         updated_at = ? WHERE task_id = ? AND status != ?`).run('dispatch outcome is unknown; inspect workspace before retrying', nowIso, row.task_id, 'done');
        }
        this.enqueueThreadNotice(row, `outcome_unknown:${row.dispatch_id}`, 'Result uncertain: the runner stopped after work may have started. Inspect the workspace before retrying; this dispatch will not be run again automatically.');
        this.emit('dispatch.outcome_unknown', {
            dispatchId: row.dispatch_id,
            reason,
            workspaceDirty: row.may_write === 1,
        });
    }
    enqueueThreadNotice(row, dedupeKey, body) {
        const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(row.session_id);
        if (!session)
            throw new Error('dispatch session is missing during notice creation');
        this.insertNotice({
            dispatchId: row.dispatch_id,
            taskId: row.task_id,
            dedupeKey,
            roomId: session.room_id,
            threadRootEventId: session.thread_root_event_id,
            senderAgentName: session.agent_name,
            body,
        });
    }
    recordRunnerActivity(input) {
        return this.db.transaction(() => {
            const checked = this.validateCapability(input);
            if ('ok' in checked)
                return checked;
            if (checked.dispatch.state !== 'started')
                return refusal('invalid_transition', 'activity requires an active runner');
            if (!['tool_start', 'tool_end', 'heartbeat'].includes(input.event.phase))
                return refusal('bad_request', 'runner cannot set lifecycle activity');
            this.updateActivity(checked.dispatch, input.event);
            return { ok: true };
        })();
    }
    authorizeFileReply(input) {
        const checked = this.validateCapability(input);
        if ('ok' in checked)
            return checked;
        return checked.dispatch.state === 'started' ? { ok: true } : refusal('invalid_transition', 'file tools require a running dispatch');
    }
    receiveFile(input) {
        const authorized = this.authorizeFileReply(input);
        if (!authorized.ok)
            return authorized;
        const file = this.conversations.receivedFile(input.dispatchId, input.eventId);
        return file ? { ok: true, file } : refusal('not_found', 'attachment is not available in this conversation input');
    }
    findFileReply(input) {
        const authorized = this.authorizeFileReply(input);
        if (!authorized.ok)
            return authorized;
        const key = `file:${input.dispatchId}:${input.requestKey}`;
        const file = this.db.prepare('SELECT * FROM file_replies WHERE request_key = ?').get(key);
        if (!file)
            return { ok: true, delivery: null };
        if (file.request_digest !== input.requestDigest)
            return refusal('matrix_command_conflict', 'file request id is already bound to another request');
        return this.readFileReply({ ...input, commandId: file.command_id });
    }
    queueFileReply(input) {
        return this.db.transaction(() => {
            const prior = this.findFileReply(input);
            if (!prior.ok || prior.delivery)
                return prior;
            const checked = this.validateCapability(input);
            if ('ok' in checked)
                return checked;
            const key = `file:${input.dispatchId}:${input.requestKey}`;
            const commandId = `notice_${digest(key).slice(0, 40)}`;
            this.enqueueThreadNotice(checked.dispatch, key, input.body || input.file.name);
            this.db.prepare('INSERT INTO file_replies(command_id, request_key, request_digest, manifest_json) VALUES (?, ?, ?, ?)')
                .run(commandId, key, input.requestDigest, canonicalJson(input.file));
            this.db.prepare('UPDATE notice_outbox SET payload_digest = ? WHERE command_id = ?')
                .run(digest({ file: input.file, body: input.body }), commandId);
            return this.readFileReply({ ...input, commandId });
        })();
    }
    readFileReply(input) {
        const checked = this.validateCapability(input);
        if ('ok' in checked)
            return checked;
        if (checked.dispatch.state !== 'started')
            return refusal('invalid_transition', 'file status requires a running dispatch');
        const notice = this.db.prepare('SELECT * FROM notice_outbox WHERE command_id = ?').get(input.commandId);
        const file = this.db.prepare('SELECT * FROM file_replies WHERE command_id = ?').get(input.commandId);
        if (!notice || !file)
            return refusal('not_found', 'file delivery not found');
        const source = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(notice.dispatch_id);
        if (source?.session_id !== checked.dispatch.session_id)
            return refusal('invalid_capability', 'file delivery is outside this conversation');
        const manifest = JSON.parse(file.manifest_json);
        return { ok: true, delivery: { deliveryId: input.commandId,
                status: notice.state === 'delivered' ? 'delivered' : notice.state === 'failed' ? 'failed' : 'queued',
                filename: manifest.name, size: manifest.size, sha256: manifest.sha256,
                eventId: notice.delivered_event_id, errorCode: notice.last_error } };
    }
    prepareFileReply(input) {
        return this.db.transaction(() => {
            const notice = this.db.prepare('SELECT * FROM notice_outbox WHERE command_id = ?').get(input.commandId);
            const file = this.db.prepare('SELECT * FROM file_replies WHERE command_id = ?').get(input.commandId);
            if (!notice || !file)
                return refusal('not_found', 'file delivery not found');
            if (notice.state !== 'claimed' || notice.claim_token_hash !== digest(input.claimToken))
                return refusal('matrix_command_conflict', 'file delivery claim is stale');
            const json = canonicalJson(input.content);
            if (json.length > 32_000 || !['m.file', 'm.image'].includes(String(input.content.msgtype)))
                return refusal('bad_request', 'invalid prepared file content');
            if (file.prepared_json && file.prepared_json !== json)
                return refusal('matrix_command_conflict', 'uploaded file content is already frozen');
            this.db.prepare('UPDATE file_replies SET prepared_json = ? WHERE command_id = ?').run(json, input.commandId);
            return { ok: true, content: input.content };
        })();
    }
    updateActivity(row, event) {
        const update = this.activity.update(row.dispatch_id, event, this.now());
        if (!update)
            return;
        // Supersede only unclaimed projections. A claimed transaction remains immutable for retries.
        this.db.prepare(`UPDATE notice_outbox SET state = 'failed', last_error = 'activity_superseded'
      WHERE dispatch_id = ? AND dedupe_key LIKE 'activity:%' AND state = 'pending'`).run(row.dispatch_id);
        this.enqueueThreadNotice(row, `activity:${row.dispatch_id}:${update.revision}`, update.body);
    }
    enqueueTaskNotice(taskId, dedupeKey, body) {
        const row = this.db.prepare(`SELECT b.room_id, b.thread_root_event_id, t.assignee_name
        FROM task_bindings b JOIN tasks t ON t.task_id = b.task_id
        WHERE b.task_id = ?`).get(taskId);
        if (!row?.assignee_name)
            throw new Error('task binding is missing notice routing metadata');
        this.insertNotice({
            dispatchId: null,
            taskId,
            dedupeKey,
            roomId: row.room_id,
            threadRootEventId: row.thread_root_event_id,
            senderAgentName: row.assignee_name,
            body,
        });
    }
    insertNotice(input) {
        const commandId = `notice_${digest(input.dedupeKey).slice(0, 40)}`;
        const txnId = `hagency_${digest(commandId).slice(0, 40)}`;
        const payloadDigest = digest({
            dispatchId: input.dispatchId,
            taskId: input.taskId,
            roomId: input.roomId,
            threadRootEventId: input.threadRootEventId,
            body: input.body,
        });
        const inserted = this.db.prepare(`INSERT OR IGNORE INTO notice_outbox(
      command_id, dispatch_id, task_id, dedupe_key, txn_id, room_id,
      thread_root_event_id, body, sender_agent_name, payload_digest, state, created_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?)`).run(commandId, input.dispatchId, input.taskId, input.dedupeKey, txnId, input.roomId, input.threadRootEventId, input.body, input.senderAgentName, payloadDigest, this.now());
        if (inserted.changes > 0) {
            this.emit('thread.notice_queued', { commandId, dispatchId: input.dispatchId, taskId: input.taskId });
        }
    }
    recordNoticeDelivery(input) {
        const commandId = requiredText(input.commandId, 'command_id', 255);
        const eventId = requiredText(input.eventId, 'event_id', 512);
        const row = this.db.prepare('SELECT * FROM notice_outbox WHERE command_id = ?').get(commandId);
        if (!row)
            return refusal('not_found', 'reply or notice command not found');
        if (row.state === 'delivered') {
            if (row.delivered_event_id !== eventId)
                return refusal('matrix_command_conflict', 'notice was delivered with another event id');
            return { ok: true, replayed: true };
        }
        if (row.state !== 'claimed' || row.claim_token_hash !== digest(input.claimToken)) {
            return refusal('matrix_command_conflict', 'notice command claim is absent or stale');
        }
        if (row.dedupe_key.startsWith('activity:') && row.dispatch_id)
            this.activity.delivered(row.dispatch_id, eventId);
        this.db.prepare("UPDATE notice_outbox SET state = 'delivered', delivered_event_id = ?, claim_token_hash = NULL, claimed_until = NULL WHERE command_id = ?").run(eventId, commandId);
        this.emit('thread.notice_delivered', { commandId, dispatchId: row.dispatch_id, taskId: row.task_id, eventId });
        return { ok: true, replayed: false };
    }
    recordNoticeFailure(input) {
        const commandId = requiredText(input.commandId, 'command_id', 255);
        const errorCode = requiredText(input.errorCode, 'error_code', 120);
        const row = this.db.prepare('SELECT * FROM notice_outbox WHERE command_id = ?').get(commandId);
        if (!row)
            return refusal('not_found', 'reply or notice command not found');
        if (row.state === 'failed')
            return { ok: true, replayed: true };
        if (row.state !== 'claimed' || row.claim_token_hash !== digest(input.claimToken)) {
            return refusal('matrix_command_conflict', 'notice command claim is absent or stale');
        }
        this.db.prepare(`UPDATE notice_outbox SET state = 'failed', last_error = ?,
       claim_token_hash = NULL, claimed_until = NULL WHERE command_id = ?`).run(errorCode, commandId);
        this.emit('thread.notice_delivery_failed', { commandId, dispatchId: row.dispatch_id, taskId: row.task_id, errorCode });
        return { ok: true, replayed: false };
    }
    beginOutcomeInspection(dispatchIdInput, ttlMs = 15 * 60_000) {
        try {
            const dispatchId = requiredText(dispatchIdInput, 'dispatch_id', 255);
            if (!Number.isFinite(ttlMs) || ttlMs < 60_000 || ttlMs > 60 * 60_000) {
                return refusal('bad_request', 'inspection ttl must be between 60000 and 3600000 ms');
            }
            const tx = this.db.transaction(() => {
                const dispatch = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(dispatchId);
                if (!dispatch)
                    return refusal('not_found', 'dispatch not found');
                if (dispatch.state !== 'outcome_unknown') {
                    return refusal('invalid_transition', 'only an outcome_unknown dispatch can be inspected for recovery');
                }
                const resolved = this.db.prepare('SELECT * FROM outcome_resolutions WHERE dispatch_id = ?').get(dispatchId);
                if (resolved)
                    return refusal('invalid_transition', 'outcome_unknown dispatch is already resolved');
                let resource = null;
                if (dispatch.may_write === 1) {
                    if (!dispatch.workspace_resource_id)
                        throw new Error('writing outcome_unknown dispatch has no workspace resource');
                    resource = this.db.prepare('SELECT * FROM resources WHERE resource_id = ?').get(dispatch.workspace_resource_id) ?? null;
                    if (!resource || resource.dirty !== 1 || resource.dirty_dispatch_id !== dispatchId) {
                        return refusal('workspace_quarantined', 'workspace quarantine no longer matches this dispatch');
                    }
                }
                const now = this.now();
                const inspectionId = randomUUID();
                const inspectionToken = this.randomBytes(32).toString('base64url');
                const expiresAt = now + Math.floor(ttlMs);
                this.db.prepare(`INSERT INTO outcome_inspections(
            inspection_id, dispatch_id, resource_id, dirty_generation,
            token_hash, created_at, expires_at
          ) VALUES (?, ?, ?, ?, ?, ?, ?)`).run(inspectionId, dispatchId, resource?.resource_id ?? null, resource?.dirty_generation ?? 0, digest(inspectionToken), now, expiresAt);
                this.emit('dispatch.outcome_inspection_started', {
                    dispatchId,
                    inspectionId,
                    expiresAt,
                    resourceId: resource?.resource_id ?? null,
                    dirtyGeneration: resource?.dirty_generation ?? 0,
                });
                return {
                    ok: true,
                    inspectionId,
                    inspectionToken,
                    dispatchId,
                    taskId: dispatch.task_id,
                    expiresAt,
                    resource: resource ? {
                        resourceId: resource.resource_id,
                        safeLabel: resource.safe_label,
                        branchName: resource.branch_name,
                        dirtyGeneration: resource.dirty_generation,
                        dirtyReason: resource.dirty_reason,
                    } : null,
                    terminalReason: dispatch.terminal_reason,
                };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    resolveOutcomeUnknown(input) {
        try {
            const dispatchId = requiredText(input.dispatchId, 'dispatch_id', 255);
            const inspectionId = requiredText(input.inspectionId, 'inspection_id', 255);
            const inspectionToken = requiredText(input.inspectionToken, 'inspection_token', 512);
            const requestId = requiredText(input.requestId, 'request_id', 255);
            const operatorNote = requiredText(input.operatorNote, 'operator_note', 2_000);
            let action;
            if (input.action === 'continue' || input.action === 'accept_completed' || input.action === 'keep_blocked') {
                action = input.action;
            }
            else {
                return refusal('bad_request', 'action must be continue, accept_completed, or keep_blocked');
            }
            const recoveryInstruction = optionalText(input.recoveryInstruction, 8_000);
            if (action === 'continue' && !recoveryInstruction) {
                return refusal('bad_request', 'continue requires a recovery_instruction');
            }
            if (action !== 'continue' && recoveryInstruction) {
                return refusal('bad_request', 'recovery_instruction is only valid for continue');
            }
            const requestDigest = digest({
                dispatchId,
                inspectionId,
                action,
                operatorNote,
                recoveryInstruction,
            });
            const tx = this.db.transaction(() => {
                const replayResult = (row) => ({
                    ok: true,
                    replayed: true,
                    dispatchId: row.dispatch_id,
                    action: row.action,
                    taskId: this.db.prepare('SELECT task_id FROM dispatches WHERE dispatch_id = ?').get(row.dispatch_id)?.task_id ?? null,
                    replacementDispatchId: row.replacement_dispatch_id,
                });
                const previousRequest = this.db.prepare('SELECT * FROM outcome_resolutions WHERE request_id = ?').get(requestId);
                if (previousRequest) {
                    if (previousRequest.dispatch_id !== dispatchId || previousRequest.request_digest !== requestDigest) {
                        return refusal('idempotency_conflict', 'resolution request id was reused with different content');
                    }
                    return replayResult(previousRequest);
                }
                const previousDispatch = this.db.prepare('SELECT * FROM outcome_resolutions WHERE dispatch_id = ?').get(dispatchId);
                if (previousDispatch) {
                    return refusal('idempotency_conflict', 'outcome_unknown dispatch was already resolved by another request');
                }
                const dispatch = this.db.prepare('SELECT * FROM dispatches WHERE dispatch_id = ?').get(dispatchId);
                if (!dispatch)
                    return refusal('not_found', 'dispatch not found');
                if (dispatch.state !== 'outcome_unknown') {
                    return refusal('invalid_transition', 'only an outcome_unknown dispatch can be resolved');
                }
                if (!dispatch.task_id && action !== 'continue') {
                    return refusal('invalid_transition', 'a non-task dispatch can only be continued with a new instruction');
                }
                const inspection = this.db.prepare('SELECT * FROM outcome_inspections WHERE inspection_id = ?').get(inspectionId);
                if (!inspection || inspection.dispatch_id !== dispatchId || inspection.token_hash !== digest(inspectionToken)) {
                    return refusal('inspection_required', 'a matching outcome inspection token is required');
                }
                if (inspection.consumed_at !== null)
                    return refusal('inspection_required', 'inspection token was already consumed');
                const now = this.now();
                if (inspection.expires_at < now)
                    return refusal('inspection_expired', 'inspection token expired');
                let resource = null;
                if (dispatch.may_write === 1) {
                    if (!dispatch.workspace_resource_id || inspection.resource_id !== dispatch.workspace_resource_id) {
                        return refusal('inspection_required', 'inspection is not bound to the dispatch workspace');
                    }
                    resource = this.db.prepare('SELECT * FROM resources WHERE resource_id = ?').get(dispatch.workspace_resource_id) ?? null;
                    if (!resource
                        || resource.dirty !== 1
                        || resource.dirty_dispatch_id !== dispatchId
                        || resource.dirty_generation !== inspection.dirty_generation) {
                        return refusal('workspace_quarantined', 'workspace changed after inspection started; inspect it again');
                    }
                }
                else if (inspection.resource_id !== null || inspection.dirty_generation !== 0) {
                    return refusal('inspection_required', 'non-writing inspection contains an unexpected workspace binding');
                }
                const active = this.db.prepare(`SELECT COUNT(*) AS count FROM dispatches
           WHERE session_id = ? AND state IN ('leased','started','parked')`).get(dispatch.session_id)?.count ?? 0;
                if (active > 0)
                    return refusal('invalid_transition', 'session has another active runner');
                const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(dispatch.session_id);
                if (!session)
                    throw new Error('outcome_unknown dispatch session is missing');
                if (dispatch.task_id) {
                    const task = this.db.prepare('SELECT status FROM tasks WHERE task_id = ?').get(dispatch.task_id);
                    if (!task)
                        throw new Error('outcome_unknown dispatch task is missing');
                    if (task.status === 'done') {
                        return refusal('invalid_transition', 'a completed task cannot be recovered from outcome_unknown');
                    }
                }
                if (resource) {
                    const cleared = this.db.prepare(`UPDATE resources SET dirty = 0, dirty_reason = NULL, dirty_dispatch_id = NULL,
             inspected_at = ? WHERE resource_id = ? AND dirty_generation = ? AND dirty_dispatch_id = ?`).run(now, resource.resource_id, resource.dirty_generation, dispatchId);
                    if (cleared.changes !== 1) {
                        return refusal('workspace_quarantined', 'workspace changed during outcome resolution');
                    }
                }
                const nowIso = new Date(now).toISOString();
                let replacementDispatchId = null;
                if (action === 'continue') {
                    const recoveryMessageId = `operator_recovery:${digest({ dispatchId, requestId }).slice(0, 40)}`;
                    const recoveryBody = `Operator recovery instruction after an inspected runner failure:\n${recoveryInstruction}`;
                    this.db.prepare(`INSERT INTO router_messages(
            message_id, room_id, matrix_event_id, thread_root_event_id,
            sender_mxid, sender_name, normalized_body, content_digest,
            received_at, explicit_task
          ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0)`).run(recoveryMessageId, session.room_id, null, session.thread_root_event_id, null, 'operator', recoveryBody, digest({ dispatchId, requestId, recoveryBody }), now);
                    this.db.prepare(`INSERT INTO session_messages(session_id, message_id, projected_at)
             VALUES (?, ?, ?)`).run(session.session_id, recoveryMessageId, now);
                    if (dispatch.task_id) {
                        this.db.prepare(`INSERT INTO task_inputs(task_id, message_id, role, attached_at, activated_at)
               VALUES (?, ?, 'supplement', ?, ?)`).run(dispatch.task_id, recoveryMessageId, now, now);
                    }
                    const queued = this.db.prepare("SELECT * FROM dispatches WHERE session_id = ? AND state = 'queued' ORDER BY created_at LIMIT 1").get(session.session_id);
                    if (queued && queued.task_id !== dispatch.task_id) {
                        throw new Error('queued recovery dispatch has a different task binding');
                    }
                    if (queued && (queued.framework !== dispatch.framework
                        || queued.may_write !== dispatch.may_write
                        || queued.workspace_mode !== dispatch.workspace_mode
                        || queued.workspace_resource_id !== dispatch.workspace_resource_id
                        || queued.named_resources_json !== dispatch.named_resources_json)) {
                        throw new Error('queued recovery dispatch execution resources differ from the inspected dispatch');
                    }
                    replacementDispatchId = queued?.dispatch_id ?? randomUUID();
                    const previousPayload = safeParseObject(dispatch.payload_json);
                    const configuredBudget = typeof previousPayload.rebuildTokenBudget === 'number'
                        && Number.isFinite(previousPayload.rebuildTokenBudget)
                        && previousPayload.rebuildTokenBudget > 0
                        ? Math.floor(previousPayload.rebuildTokenBudget)
                        : undefined;
                    const replacementPayload = {
                        kind: dispatch.may_write === 1 ? 'task_turn' : 'front_desk_turn',
                        ...(configuredBudget ? { rebuildTokenBudget: configuredBudget } : {}),
                    };
                    const replacementPayloadJson = canonicalJson(replacementPayload);
                    if (!queued) {
                        this.db.prepare(`INSERT INTO dispatches(
              dispatch_id, session_id, task_id, state, framework, payload_json,
              payload_digest, may_write, workspace_mode, workspace_resource_id,
              named_resources_json, created_at
            ) VALUES (?, ?, ?, 'queued', ?, ?, ?, ?, ?, ?, ?, ?)`).run(replacementDispatchId, dispatch.session_id, dispatch.task_id, dispatch.framework, replacementPayloadJson, digest(replacementPayloadJson), dispatch.may_write, dispatch.workspace_mode, dispatch.workspace_resource_id, dispatch.named_resources_json, now);
                    }
                    else {
                        this.db.prepare(`UPDATE dispatches SET payload_json = ?, payload_digest = ?
               WHERE dispatch_id = ? AND state = 'queued'`).run(replacementPayloadJson, digest(replacementPayloadJson), replacementDispatchId);
                    }
                    const pendingMessages = dispatch.task_id
                        ? this.db.prepare(`SELECT sm.message_id FROM session_messages sm
                 JOIN task_inputs ti ON ti.message_id = sm.message_id AND ti.task_id = ?
                 LEFT JOIN dispatch_messages dm
                   ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
                 WHERE sm.session_id = ? AND sm.processed_at IS NULL AND dm.message_id IS NULL
                 ORDER BY sm.projected_at, sm.message_id`).all(dispatch.task_id, session.session_id)
                        : this.db.prepare(`SELECT sm.message_id FROM session_messages sm
                 JOIN router_messages m ON m.message_id = sm.message_id
                 LEFT JOIN dispatch_messages dm
                   ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
                 WHERE sm.session_id = ? AND sm.processed_at IS NULL AND dm.message_id IS NULL
                   AND m.explicit_task = 0
                 ORDER BY sm.projected_at, sm.message_id`).all(session.session_id);
                    for (const message of pendingMessages) {
                        this.db.prepare('INSERT INTO dispatch_messages(dispatch_id, session_id, message_id, assigned_at) VALUES (?, ?, ?, ?)').run(replacementDispatchId, session.session_id, message.message_id, now);
                    }
                    if (dispatch.task_id) {
                        this.db.prepare(`UPDATE tasks SET status = 'in_progress', started_at = COALESCE(started_at, ?),
               completed_at = NULL, waiting_reason = NULL, waiting_until = NULL, updated_at = ?
               WHERE task_id = ?`).run(nowIso, nowIso, dispatch.task_id);
                    }
                }
                else if (action === 'accept_completed' && dispatch.task_id) {
                    this.db.prepare(`UPDATE tasks SET status = 'done', completed_at = ?, waiting_reason = NULL,
             waiting_until = NULL, updated_at = ? WHERE task_id = ?`).run(nowIso, nowIso, dispatch.task_id);
                }
                else if (action === 'keep_blocked' && dispatch.task_id) {
                    this.db.prepare(`UPDATE tasks SET status = 'blocked', waiting_reason = ?, waiting_until = NULL,
             updated_at = ? WHERE task_id = ?`).run('operator inspected runner outcome; task remains blocked', nowIso, dispatch.task_id);
                }
                this.db.prepare('UPDATE outcome_inspections SET consumed_at = ? WHERE inspection_id = ?').run(now, inspectionId);
                this.db.prepare(`INSERT INTO outcome_resolutions(
          dispatch_id, request_id, request_digest, inspection_id, action,
          operator_note, recovery_instruction, replacement_dispatch_id, resolved_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`).run(dispatchId, requestId, requestDigest, inspectionId, action, operatorNote, recoveryInstruction, replacementDispatchId, now);
                const notice = action === 'continue'
                    ? 'Operator inspection completed. A new recovery dispatch was queued from an explicit recovery instruction; the previous dispatch remains outcome_unknown and was not replayed.'
                    : action === 'accept_completed'
                        ? 'Operator inspection completed. The current result was accepted as complete; no dispatch was replayed.'
                        : 'Operator inspection completed. The task remains blocked; no dispatch was replayed.';
                this.enqueueThreadNotice(dispatch, `outcome_resolved:${dispatchId}`, notice);
                this.emit('dispatch.outcome_resolved', {
                    dispatchId,
                    inspectionId,
                    action,
                    taskId: dispatch.task_id,
                    replacementDispatchId,
                });
                return {
                    ok: true,
                    replayed: false,
                    dispatchId,
                    action,
                    taskId: dispatch.task_id,
                    replacementDispatchId,
                };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    inspectWorkspace(resourceIdInput) {
        try {
            const resourceId = requiredText(resourceIdInput, 'resource_id', 512);
            const row = this.db.prepare('SELECT * FROM resources WHERE resource_id = ?').get(resourceId);
            if (!row)
                return refusal('not_found', 'workspace resource not found');
            return {
                resourceId: row.resource_id,
                kind: row.kind,
                safeLabel: row.safe_label,
                branchName: row.branch_name,
                dirty: row.dirty === 1,
                dirtyReason: row.dirty_reason,
                dirtyGeneration: row.dirty_generation,
                quarantinedByDispatchId: row.dirty_dispatch_id,
                inspectedAt: row.inspected_at,
            };
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    clearWorkspaceDirty(resourceIdInput) {
        try {
            const tx = this.db.transaction(() => {
                const resourceId = requiredText(resourceIdInput, 'resource_id', 512);
                const row = this.db.prepare('SELECT resource_id, dirty_dispatch_id FROM resources WHERE resource_id = ?').get(resourceId);
                if (!row)
                    return refusal('not_found', 'workspace resource not found');
                if (row.dirty_dispatch_id) {
                    return refusal('inspection_required', 'outcome_unknown workspace quarantine must be cleared through an inspected dispatch resolution');
                }
                this.db.prepare(`UPDATE resources SET dirty = 0, dirty_reason = NULL, dirty_dispatch_id = NULL,
           inspected_at = ? WHERE resource_id = ?`).run(this.now(), resourceId);
                this.emit('workspace.dirty_cleared', { resourceId });
                return { ok: true };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    taskOperation(input) {
        try {
            return this.db.transaction(() => {
                const checked = this.validateCapability(input);
                if ('ok' in checked)
                    return checked;
                if (checked.dispatch.state !== 'started')
                    return refusal('invalid_transition', 'task operations require a currently started dispatch');
                const result = applyTaskOperation(this, input, checked.dispatch.session_id, checked.dispatch.task_id);
                if (result.ok && !result.replayed && !['get', 'list'].includes(input.action)) {
                    this.emit('task.updated', { taskId: input.taskId, dispatchId: input.dispatchId, action: input.action, status: result.task?.status });
                    if (input.action === 'transition' && result.task)
                        this.enqueueTaskNotice(result.task.id, `task_operation:${input.dispatchId}:${input.toolCallId}`, `Task status: ${result.task.status}`);
                }
                return result;
            })();
        }
        catch (error) {
            if (error instanceof Error && 'code' in error && typeof error.code === 'string' && !error.code.startsWith('SQLITE_'))
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    checkInbox(input) {
        const checked = this.validateCapability(input);
        if ('ok' in checked)
            return checked;
        return this.db.prepare(`SELECT DISTINCT m.* FROM router_messages m
       JOIN session_messages sm ON sm.message_id = m.message_id
       LEFT JOIN dispatch_messages dm
         ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
       WHERE sm.session_id = ? AND (sm.processed_at IS NOT NULL OR dm.dispatch_id = ?)
       ORDER BY m.received_at, m.message_id`).all(checked.dispatch.session_id, checked.dispatch.dispatch_id).map((message) => ({
            messageId: message.message_id,
            senderName: message.sender_name,
            body: message.normalized_body,
            receivedAt: message.received_at,
        }));
    }
    checkInboxForAgent(input, agentNameInput) {
        try {
            const checked = this.validateCapability(input);
            if ('ok' in checked)
                return checked;
            const agentName = requiredText(agentNameInput, 'agent_name', 255);
            const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(checked.dispatch.session_id);
            if (!session || session.agent_name !== agentName) {
                return refusal('invalid_capability', 'runner capability does not belong to this agent');
            }
            return this.checkInbox(input);
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    listAgentDispatches(agentIdInput) {
        const agentId = requiredText(agentIdInput, 'agent_id', 255);
        return this.db.prepare(`SELECT d.* FROM dispatches d JOIN sessions s ON s.session_id = d.session_id
       WHERE s.agent_id = ? AND d.state IN ('queued', 'leased', 'started', 'parked') ORDER BY d.created_at`).all(agentId).map((row) => ({ dispatchId: row.dispatch_id, sessionId: row.session_id,
            state: row.state, createdAt: row.created_at, startedAt: row.started_at }));
    }
    authorizeTaskFromDispatch(input) {
        if (typeof input.taskId !== 'string' || !input.taskId)
            return refusal('bad_request', 'task id is required');
        const checked = this.validateCapability(input);
        if ('ok' in checked)
            return checked;
        if (input.write && checked.dispatch.state !== 'started')
            return refusal('invalid_transition', 'task writes require a started dispatch');
        if (checked.dispatch.task_id === input.taskId)
            return { ok: true };
        if (!input.write && checked.dispatch.task_id) {
            const child = this.db.prepare(`SELECT t.task_id AS id FROM tasks t JOIN task_bindings b ON b.task_id = t.task_id
         JOIN sessions s ON s.session_id = ? WHERE t.task_id = ? AND t.parent_id = ?
           AND b.room_id = s.room_id AND b.creator_agent_id = s.agent_id`).get(checked.dispatch.session_id, input.taskId, checked.dispatch.task_id);
            if (child)
                return { ok: true };
        }
        return refusal('invalid_capability', 'task is outside this dispatch session');
    }
    deliverPeerMessageFromDispatch(input) {
        return this.db.transaction(() => {
            const checked = this.validateCapability(input);
            if ('ok' in checked)
                return checked;
            if (checked.dispatch.state !== 'started')
                return refusal('invalid_transition', 'messages require a started dispatch');
            const session = this.sessionById(checked.dispatch.session_id);
            if (!session)
                return refusal('not_found', 'sender session not found');
            const targets = this.db.prepare(`SELECT b.task_id, b.thread_root_event_id FROM task_bindings b JOIN tasks t ON t.task_id = b.task_id
         WHERE b.room_id = ? AND b.assignee_agent_id = ? AND b.activation_state = 'active'
           AND t.status != 'done'`).all(session.roomId, input.recipientAgentId).filter((row) => !input.targetTaskId || row.task_id === input.targetTaskId);
            if (targets.length !== 1)
                return refusal('missing_task_binding', 'message target must identify one active task in this project');
            const target = targets[0];
            if (!target)
                return refusal('missing_task_binding', 'target task is missing');
            const key = requiredText(input.toolCallId, 'tool_call_id', 512);
            const messageId = `peer_${createHash('sha256').update(`${checked.dispatch.dispatch_id}\0${key}`).digest('hex')}`;
            const requestScope = `peer:${checked.dispatch.dispatch_id}`;
            const previous = this.db.prepare('SELECT task_id FROM task_input_requests WHERE request_scope = ? AND request_key = ?').get(requestScope, key);
            if (previous && previous.task_id !== target.task_id) {
                return refusal('idempotency_conflict', 'peer message request key was reused for another task');
            }
            const result = this.ingestMessage({ messageId, roomId: session.roomId,
                threadRootEventId: target.thread_root_event_id, senderName: session.agentName,
                recipientAgentId: input.recipientAgentId, recipientAgentName: input.recipientAgentName,
                normalizedBody: requiredText(input.body, 'body', 100_000) });
            if (!result.ok)
                return result;
            const attached = this.attachTaskInputs({ taskId: target.task_id, requestScope, requestKey: key, messageIds: [messageId] });
            // A failure here must roll back the message and its projection as well.
            if (!attached.ok)
                throw new RouterInputError(attached.message);
            return { ...result, taskId: target.task_id, threadRootEventId: target.thread_root_event_id };
        })();
    }
    listPendingPeerTaskInputs() {
        // Recover only previously accepted, backend-addressed peer projections.
        // The exact durable recipient session fixes the target; no message body or
        // name heuristic may choose a room/task. Assigned/processed inputs stay inert.
        return this.db.prepare(`SELECT DISTINCT m.message_id, s.session_id, b.task_id, s.room_id, s.thread_root_event_id,
          s.agent_id, s.agent_name, sender.agent_id AS sender_agent_id, m.sender_name
        FROM session_messages sm JOIN router_messages m ON m.message_id = sm.message_id
        JOIN sessions s ON s.session_id = sm.session_id
        JOIN task_bindings b ON b.room_id = s.room_id AND b.thread_root_event_id = s.thread_root_event_id
          AND b.assignee_agent_id = s.agent_id
        JOIN tasks t ON t.task_id = b.task_id
        JOIN sessions sender ON sender.agent_name = m.sender_name AND sender.room_id = s.room_id
        JOIN dispatches sent ON sent.session_id = sender.session_id
          AND sent.started_at <= m.received_at AND (sent.settled_at IS NULL OR sent.settled_at >= m.received_at)
        LEFT JOIN dispatch_messages dm ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
        WHERE sm.processed_at IS NULL AND dm.message_id IS NULL
          AND m.matrix_event_id IS NULL AND m.sender_mxid IS NULL
          AND m.room_id = s.room_id AND m.thread_root_event_id = s.thread_root_event_id
          AND m.message_id GLOB 'peer_*' AND length(m.message_id) = 69
          AND substr(m.message_id, 6) NOT GLOB '*[^0-9a-f]*'
          AND b.activation_state = 'active' AND b.thread_anchor_event_id IS NOT NULL AND t.status != 'done'
        ORDER BY sm.projected_at, m.message_id`).all().map((row) => ({
            messageId: row.message_id, sessionId: row.session_id, taskId: row.task_id, roomId: row.room_id,
            threadRootEventId: row.thread_root_event_id, recipientAgentId: row.agent_id, recipientAgentName: row.agent_name,
            senderAgentId: row.sender_agent_id, senderAgentName: row.sender_name,
        }));
    }
    createTaskFromDispatch(input) {
        try {
            const checked = this.validateCapability(input);
            if ('ok' in checked)
                return checked;
            if (checked.dispatch.state !== 'started') {
                return refusal('invalid_transition', 'create_task requires a currently started dispatch');
            }
            if (input.task.parentId && input.task.parentId !== checked.dispatch.task_id) {
                return refusal('missing_task_credential', 'delegation parent must be the current dispatch task');
            }
            const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(checked.dispatch.session_id);
            if (!session)
                throw new Error('dispatch session is missing');
            const defaultRoot = input.rootMessageId === undefined ? this.db.prepare(`SELECT m.message_id AS id FROM session_messages sm JOIN router_messages m ON m.message_id = sm.message_id
         LEFT JOIN dispatch_messages dm ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
         WHERE sm.session_id = ? AND m.matrix_event_id IS NOT NULL
           AND (sm.processed_at IS NOT NULL OR dm.dispatch_id = ?)
         ORDER BY m.received_at DESC, m.message_id DESC LIMIT 1`).get(session.session_id, checked.dispatch.dispatch_id)?.id : undefined;
            const rootMessageId = requiredText(input.rootMessageId ?? defaultRoot, 'root_message_id', 512);
            const messageIds = [...new Set([rootMessageId, ...input.inputMessageIds])];
            for (const messageId of messageIds) {
                const projected = this.db.prepare(`SELECT sm.message_id AS id FROM session_messages sm
           LEFT JOIN dispatch_messages dm
             ON dm.session_id = sm.session_id AND dm.message_id = sm.message_id
           WHERE sm.session_id = ? AND sm.message_id = ?
             AND (sm.processed_at IS NOT NULL OR dm.dispatch_id = ?)`).get(session.session_id, messageId, checked.dispatch.dispatch_id);
                if (!projected)
                    return refusal('missing_task_credential', 'task input is outside the creator session');
            }
            const root = this.db.prepare('SELECT * FROM router_messages WHERE message_id = ?').get(rootMessageId);
            if (!root?.matrix_event_id) {
                return refusal('missing_task_credential', 'task root lacks an authenticated Matrix event id');
            }
            return this.createTaskIntent({
                requestScope: `dispatch:${checked.dispatch.dispatch_id}`,
                requestKey: requiredText(input.toolCallId, 'tool_call_id', 512),
                roomId: session.room_id,
                threadRootEventId: root.thread_root_event_id ?? root.matrix_event_id,
                rootMessageId,
                inputMessageIds: messageIds,
                task: {
                    ...input.task,
                    creatorAgentId: session.agent_id,
                    createdBy: session.agent_name,
                },
                ...(input.acknowledgementBody === undefined ? {} : { acknowledgementBody: input.acknowledgementBody }),
            });
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    assembleContext(sessionIdInput, tokenBudget = 12_000) {
        try {
            const sessionId = requiredText(sessionIdInput, 'session_id', 255);
            const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(sessionId);
            if (!session)
                return refusal('not_found', 'session not found');
            const all = this.db.prepare(`SELECT m.* FROM router_messages m JOIN session_messages sm ON sm.message_id = m.message_id
         WHERE sm.session_id = ? ORDER BY m.received_at DESC, m.message_id DESC`).all(sessionId);
            let remaining = Math.max(500, tokenBudget) * 4;
            const selected = [];
            for (const message of all) {
                if (message.normalized_body.length > remaining && selected.length > 0)
                    break;
                selected.push(message);
                remaining -= message.normalized_body.length;
                if (remaining <= 0)
                    break;
            }
            selected.reverse();
            const activeTasks = this.db.prepare(`SELECT t.task_id, t.title, t.status, b.activation_state
          FROM tasks t JOIN task_bindings b ON b.task_id = t.task_id
          WHERE b.assignee_agent_id = ? AND t.status != 'done'
          ORDER BY t.updated_at DESC LIMIT 50`).all(session.agent_id);
            return {
                session: sessionView(session),
                rollingSummary: session.rolling_summary,
                messages: selected.map((message) => ({
                    messageId: message.message_id,
                    senderName: message.sender_name,
                    body: message.normalized_body,
                    receivedAt: message.received_at,
                })),
                coordinatorDigest: session.scope_kind === 'main' ? activeTasks : [],
                tokenBudget,
            };
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    updateRollingSummary(sessionIdInput, summaryInput) {
        try {
            const tx = this.db.transaction(() => {
                const sessionId = requiredText(sessionIdInput, 'session_id', 255);
                const summary = requiredText(summaryInput, 'summary', 50_000);
                const session = this.db.prepare('SELECT * FROM sessions WHERE session_id = ?').get(sessionId);
                if (!session)
                    return refusal('not_found', 'session not found');
                const generation = session.context_generation + 1;
                this.db.prepare('UPDATE sessions SET rolling_summary = ?, context_generation = ? WHERE session_id = ?').run(summary, generation, sessionId);
                this.emit('session.context_rotated', { sessionId, contextGeneration: generation });
                return { ok: true, contextGeneration: generation };
            });
            return tx();
        }
        catch (error) {
            if (error instanceof RouterInputError)
                return refusal('bad_request', error.message);
            throw error;
        }
    }
    /** Small allowlisted projection, independent of the dashboard history limit. */
    agentDispatchActivity(agentId) {
        const rows = this.db.prepare(`SELECT d.state, COUNT(*) AS count FROM dispatches d
       JOIN sessions s ON s.session_id = d.session_id
       WHERE s.agent_id = ? GROUP BY d.state`).all(agentId);
        const result = { activeDispatchCount: 0, queuedDispatchCount: 0, parkedDispatchCount: 0 };
        for (const row of rows) {
            switch (row.state) {
                case 'queued':
                    result.queuedDispatchCount += row.count;
                    break;
                case 'parked':
                    result.parkedDispatchCount += row.count;
                    result.activeDispatchCount += row.count;
                    break;
                case 'leased':
                case 'started':
                    result.activeDispatchCount += row.count;
                    break;
                case 'completed':
                case 'cancelled_before_start':
                case 'outcome_unknown': break;
                default: throw new Error('unrecognized dispatch activity state');
            }
        }
        return result;
    }
    snapshot() {
        const meta = this.meta();
        const sessions = this.db.prepare('SELECT * FROM sessions ORDER BY last_active DESC').all().map((row) => ({
            sessionId: row.session_id,
            agentId: row.agent_id,
            agentName: row.agent_name,
            roomId: row.room_id,
            scopeKind: row.scope_kind,
            threadRootEventId: row.thread_root_event_id,
            contextGeneration: row.context_generation,
            lastActive: row.last_active,
        }));
        const tasks = this.db.prepare(`SELECT t.task_id, t.title, t.status, t.priority, t.assignee_name,
       b.activation_state, b.thread_root_event_id FROM tasks t
       LEFT JOIN task_bindings b ON b.task_id = t.task_id ORDER BY t.updated_at DESC`).all().map((row) => ({
            taskId: row.task_id,
            title: row.title,
            status: row.status,
            priority: row.priority,
            assignee: row.assignee_name,
            activationState: row.activation_state,
            threadRootEventId: row.thread_root_event_id,
        }));
        const dispatches = this.db.prepare('SELECT * FROM dispatches ORDER BY created_at DESC LIMIT 1000').all().map((row) => {
            const resolution = row.state === 'outcome_unknown'
                ? this.db.prepare(`SELECT action, replacement_dispatch_id, resolved_at
             FROM outcome_resolutions WHERE dispatch_id = ?`).get(row.dispatch_id)
                : undefined;
            let blockedBy = null;
            if (row.state === 'queued') {
                const unresolved = this.db.prepare(`SELECT d.dispatch_id FROM dispatches d
           LEFT JOIN outcome_resolutions r ON r.dispatch_id = d.dispatch_id
           WHERE d.session_id = ? AND d.state = 'outcome_unknown' AND r.dispatch_id IS NULL
           ORDER BY d.settled_at DESC LIMIT 1`).get(row.session_id);
                if (unresolved) {
                    blockedBy = {
                        dispatchId: unresolved.dispatch_id,
                        reason: 'outcome_unknown_inspection',
                    };
                }
                if (!blockedBy && row.task_id) {
                    const task = this.db.prepare('SELECT status FROM tasks WHERE task_id = ?').get(row.task_id);
                    if (!task || task.status === 'blocked' || task.status === 'done') {
                        blockedBy = { reason: 'task_status', taskStatus: task?.status ?? 'missing' };
                    }
                }
                if (!blockedBy && row.available_at !== null && row.available_at > this.now()) {
                    blockedBy = {
                        reason: 'runner_launch_backoff',
                        retryAt: row.available_at,
                        launchFailures: row.launch_failures,
                    };
                }
                const requiredResources = [
                    ...(row.may_write === 1 && row.workspace_resource_id ? [row.workspace_resource_id] : []),
                    ...safeParseStringArray(row.named_resources_json),
                ];
                for (const resourceId of blockedBy ? [] : requiredResources) {
                    const resource = this.db.prepare('SELECT dirty, safe_label, dirty_dispatch_id FROM resources WHERE resource_id = ?').get(resourceId);
                    if (resource?.dirty === 1) {
                        blockedBy = {
                            dispatchId: resource.dirty_dispatch_id,
                            reason: 'workspace_quarantined',
                            resourceLabel: resource.safe_label,
                        };
                        break;
                    }
                    const blocker = this.db.prepare(`SELECT d.dispatch_id, d.state, r.safe_label
              FROM resource_leases l
              JOIN dispatches d ON d.dispatch_id = l.dispatch_id
              JOIN resources r ON r.resource_id = l.resource_id
              WHERE l.resource_id = ?`).get(resourceId);
                    if (!blocker)
                        continue;
                    blockedBy = {
                        dispatchId: blocker.dispatch_id,
                        reason: blocker.state === 'parked' ? 'waiting_for_approval' : 'resource_lease',
                        resourceLabel: blocker.safe_label,
                    };
                    break;
                }
            }
            return {
                dispatchId: row.dispatch_id,
                sessionId: row.session_id,
                taskId: row.task_id,
                state: row.state,
                framework: row.framework,
                mayWrite: row.may_write === 1,
                workspaceMode: row.workspace_mode,
                fenceGeneration: row.fence_generation,
                terminalReason: publicRuntimeReason(row.terminal_reason),
                blockedBy,
                createdAt: row.created_at,
                startedAt: row.started_at,
                settledAt: row.settled_at,
                resolutionAction: resolution?.action ?? null,
                resolvedAt: resolution?.resolved_at ?? null,
                replacementDispatchId: resolution?.replacement_dispatch_id ?? null,
            };
        });
        const resources = this.db.prepare('SELECT * FROM resources').all().map((row) => ({
            resourceId: row.resource_id,
            kind: row.kind,
            safeLabel: row.safe_label,
            branchName: row.branch_name,
            dirty: row.dirty === 1,
            dirtyReason: publicRuntimeReason(row.dirty_reason),
            dirtyGeneration: row.dirty_generation,
            quarantinedByDispatchId: row.dirty_dispatch_id,
            inspectedAt: row.inspected_at,
        }));
        const attention = [
            ...dispatches.filter((row) => ((row.state === 'outcome_unknown' && row.resolutionAction === null) || row.state === 'parked')),
            ...tasks.filter((row) => row.activationState === 'thread_delivery_failed'),
            ...resources.filter((row) => row.dirty === true),
        ];
        return {
            schemaVersion: meta.schema_version,
            lowWatermark: meta.low_watermark,
            highWatermark: meta.high_watermark,
            sessions,
            tasks,
            dispatches,
            resources,
            attention,
        };
    }
    eventsAfter(after, limit = 500) {
        const meta = this.meta();
        const normalizedAfter = Number.isFinite(after) ? Math.max(0, Math.floor(after)) : 0;
        const gap = normalizedAfter > 0 && normalizedAfter < meta.low_watermark;
        const rows = gap ? [] : this.db.prepare('SELECT seq, schema_version, at, kind, payload_json FROM router_events WHERE seq > ? ORDER BY seq LIMIT ?').all(normalizedAfter, Math.min(Math.max(1, limit), 2_000));
        const events = rows.map((row) => {
            const payload = safeParseObject(row.payload_json);
            return {
                seq: row.seq,
                schemaVersion: row.schema_version,
                at: row.at,
                kind: row.kind,
                payload: typeof payload.reason === 'string'
                    ? { ...payload, reason: publicRuntimeReason(payload.reason) } : payload,
            };
        });
        return {
            schemaVersion: meta.schema_version,
            lowWatermark: meta.low_watermark,
            highWatermark: meta.high_watermark,
            gap,
            events,
        };
    }
    meta() {
        const meta = this.db.prepare('SELECT schema_version, low_watermark, high_watermark FROM router_event_meta WHERE id = 1').get();
        if (!meta)
            throw new Error('router event metadata is missing');
        return meta;
    }
    reconcileOnStart() {
        const tx = this.db.transaction(() => {
            const now = this.now();
            let requeued = 0;
            let outcomeUnknown = 0;
            // A runner lease belongs to this backend process. After a restart no old
            // runner identity is verifiable, even when its wall-clock lease has not
            // expired yet, so every unstarted lease must be made runnable again.
            const leased = this.db.prepare("SELECT * FROM dispatches WHERE state = 'leased'").all();
            for (const row of leased) {
                this.db.prepare(`UPDATE dispatches SET state = 'queued', runner_id = NULL, lease_until = NULL,
           available_at = NULL WHERE dispatch_id = ?`).run(row.dispatch_id);
                this.db.prepare('UPDATE runner_capabilities SET revoked_at = ? WHERE dispatch_id = ? AND revoked_at IS NULL').run(now, row.dispatch_id);
                this.db.prepare('DELETE FROM resource_leases WHERE dispatch_id = ?').run(row.dispatch_id);
                this.emit('dispatch.requeued_before_start', { dispatchId: row.dispatch_id });
                requeued += 1;
            }
            const inFlight = this.db.prepare("SELECT * FROM dispatches WHERE state IN ('started','parked')").all();
            for (const row of inFlight) {
                this.settleUnknownInternal(row, 'backend_restart_unverifiable_runner');
                outcomeUnknown += 1;
            }
            const result = this.db.prepare(`UPDATE matrix_outbox SET state = 'pending', claim_token_hash = NULL,
         claimed_until = NULL WHERE state = 'claimed' AND claimed_until < ?`).run(now);
            this.db.prepare(`UPDATE reply_outbox SET state = 'pending', claim_token_hash = NULL,
         claimed_until = NULL WHERE state = 'claimed' AND claimed_until < ?`).run(now);
            this.db.prepare(`UPDATE notice_outbox SET state = 'pending', claim_token_hash = NULL,
         claimed_until = NULL WHERE state = 'claimed' AND claimed_until < ?`).run(now);
            return { requeued, outcomeUnknown, expiredMatrixClaims: result.changes };
        });
        return tx();
    }
}
export function openRouter(options) {
    return new RouterStore(options);
}
