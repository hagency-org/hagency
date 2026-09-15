import { ConversationStore } from './conversations.js';
import { type ActivityEvent } from './activity.js';
import { type FileReplyManifest, type FileReplyResult } from './files.js';
import { type TaskOperationInput, type TaskOperationResult } from './task-operations.js';
import type { ActivationResult, ActiveTaskBinding, ApprovalDecisionEvent, AttachInputsResult, AttachTaskInputsInput, AuthenticatedMessageInput, CapabilityInput, ClaimDispatchInput, ClaimResult, CreateTaskIntentInput, EnqueueDispatchInput, EnqueueResult, EventPage, IngestResult, MatrixCommand, MatrixDeliveryFailure, MatrixDeliveryReceipt, LaunchDescriptor, OutcomeInspectionResult, OutcomeResolutionResult, ParkDispatchInput, ReconcileReport, Refusal, ReplyCommand, RouterOptions, RouterSnapshot, ResolveOutcomeUnknownInput, RunnerEffectInput, SessionInboxMessage, SessionView, SetSessionOverridesInput, SettleDispatchInput, SettleSuccess, StartedPayload, StoredMessageResult, TaskDeliveryResult, TaskDispatchFailureResult, TaskIntentResult } from './types.js';
interface TaskBindingRow {
    task_id: string;
    creator_agent_id: string | null;
    assignee_agent_id: string;
    room_id: string;
    thread_root_event_id: string;
    thread_anchor_event_id: string | null;
    activation_state: 'pending_thread' | 'active' | 'thread_delivery_failed' | 'closed';
    request_scope: string;
    request_key: string;
    request_digest: string;
}
interface DispatchRow {
    dispatch_id: string;
    session_id: string;
    task_id: string | null;
    state: 'queued' | 'leased' | 'started' | 'parked' | 'completed' | 'cancelled_before_start' | 'outcome_unknown';
    framework: string;
    payload_json: string;
    payload_digest: string;
    may_write: number;
    workspace_mode: 'shared' | 'worktree';
    workspace_resource_id: string | null;
    named_resources_json: string;
    fence_generation: number;
    runner_id: string | null;
    lease_until: number | null;
    effect_ack_at: number | null;
    started_at: number | null;
    parked_at: number | null;
    settled_at: number | null;
    terminal_reason: string | null;
    output_json: string | null;
    fenced_output_json: string | null;
    created_at: number;
    launch_failures: number;
    available_at: number | null;
    last_launch_error: string | null;
}
export declare class RouterStore {
    readonly conversations: ConversationStore;
    private readonly activity;
    readonly dbPath: string;
    private readonly now;
    private readonly randomBytes;
    private readonly eventRetention;
    constructor(options: RouterOptions);
    close(): void;
    private applyMigrations;
    private emit;
    private trimEvents;
    private resolveSessionInternal;
    resolveSession(input: {
        agentId: string;
        agentName: string;
        roomId: string;
        threadRootEventId?: string | null;
    }): SessionView;
    sessionById(sessionIdInput: string): SessionView | null;
    setSessionOverrides(input: SetSessionOverridesInput): SessionView | Refusal;
    queueSessionNotice(input: {
        roomId: string;
        threadRootEventId?: string | null;
        senderAgentName: string;
        dedupeKey: string;
        body: string;
    }): {
        ok: true;
    } | Refusal;
    initializeIngestionCursor(sourceInput: string, cursorInput: string): {
        created: boolean;
        value: string;
    };
    readIngestionCursor(sourceInput: string): string | null;
    advanceIngestionCursor(sourceInput: string, cursorInput: string): void;
    private storeMessageInternal;
    storeTaskMessage(input: AuthenticatedMessageInput): StoredMessageResult;
    ingestMessage(input: AuthenticatedMessageInput): IngestResult;
    createTaskIntent(input: CreateTaskIntentInput): TaskIntentResult;
    attachTaskInputs(input: AttachTaskInputsInput): AttachInputsResult;
    claimMatrixCommand(claimMs?: number): MatrixCommand | null;
    recordMatrixDelivery(input: MatrixDeliveryReceipt): ActivationResult;
    recordMatrixFailure(input: MatrixDeliveryFailure): TaskDeliveryResult;
    recordTaskDispatchFailure(taskIdInput: string, errorCodeInput: string): TaskDispatchFailureResult;
    private findThreadSession;
    findThreadTaskBinding(agentIdInput: string, roomIdInput: string, rootInput: string): {
        taskId: string;
        activationState: TaskBindingRow['activation_state'];
    } | Refusal | null;
    findActiveTaskBinding(agentIdInput: string, roomIdInput: string, rootInput: string): ActiveTaskBinding | Refusal;
    enqueueDispatch(input: EnqueueDispatchInput): EnqueueResult;
    private ensureResource;
    registerWorkspace(input: {
        resourceId: string;
        safeLabel: string;
        backendPath: string;
        branchName?: string | null;
    }): void;
    claimDispatch(input: ClaimDispatchInput): ClaimResult;
    claimDispatchWithWake(input: ClaimDispatchInput): {
        claim: ClaimResult;
        nextAvailableAt: number | null;
    };
    private claimDispatchObserved;
    nextQueuedDispatchAt(): number | null;
    private nextQueuedDispatchAfter;
    getLaunchDescriptor(input: CapabilityInput): LaunchDescriptor | Refusal;
    private validateCapability;
    takePayload(input: CapabilityInput): StartedPayload | Refusal;
    private buildRunnerContext;
    readConversation(input: CapabilityInput, offset?: number): Refusal | {
        messages: never[];
        next: null;
        totalMessages: number;
        totalParts: number;
        roomId?: never;
        ok: true;
    } | {
        roomId: string;
        messages: {
            eventId: string;
            sender: string;
            timestamp: number;
            threadRoot: string | null;
            part: number;
            parts: number;
            attachment?: {
                eventId: string;
                name: unknown;
                mime: unknown;
                size: unknown;
                errorCode?: {};
                error?: unknown;
                instruction: string;
            };
            body: string;
        }[];
        next: number | null;
        totalMessages: number;
        totalParts: number;
        ok: true;
    };
    acknowledgeRunnerEffect(input: RunnerEffectInput): {
        ok: true;
    } | Refusal;
    parkForApproval(input: ParkDispatchInput): {
        ok: true;
        replayed: boolean;
    } | Refusal;
    recordApprovalDecision(input: ApprovalDecisionEvent): {
        ok: true;
        replayed: boolean;
    } | Refusal;
    reconcileApprovalDecision(input: {
        approvalId: string;
        decisionEventId: string;
        decision: 'allow' | 'deny';
    }): {
        ok: true;
        replayed: boolean;
        deliverable: boolean;
    } | Refusal;
    approvalThreadOrigin(approvalId: string, agentId: string, roomId: string): string | null;
    readApprovalDecision(input: CapabilityInput & {
        approvalId: string;
        operationDigest: string;
    }): {
        ok: true;
        decision: 'allow' | 'deny' | null;
    } | Refusal;
    resumeAfterApproval(input: CapabilityInput & {
        approvalId: string;
        operationDigest: string;
    }): {
        ok: true;
        decision: 'allow' | 'deny';
    } | Refusal;
    settleAndRelease(input: SettleDispatchInput): SettleSuccess | Refusal;
    claimReplyCommand(claimMs?: number): ReplyCommand | null;
    private replySessionCreatedAt;
    recordReplyDelivery(input: {
        commandId: string;
        claimToken: string;
        eventId: string;
    }): {
        ok: true;
        replayed: boolean;
    } | Refusal;
    recordReplyFailure(input: {
        commandId: string;
        claimToken: string;
        errorCode: string;
    }): {
        ok: true;
        replayed: boolean;
    } | Refusal;
    cancelBeforeStart(dispatchIdInput: string, reasonInput?: string): {
        ok: true;
        state: 'cancelled_before_start';
    } | Refusal;
    requeueBeforeStart(input: CapabilityInput, retryDelayMs?: number, reasonInput?: string): {
        ok: true;
        state: 'queued';
        retryAt: number;
    } | Refusal;
    markOutcomeUnknown(dispatchIdInput: string, reasonInput: string): {
        ok: true;
        state: 'outcome_unknown';
    } | Refusal;
    private settleUnknownInternal;
    private enqueueThreadNotice;
    recordRunnerActivity(input: CapabilityInput & {
        event: ActivityEvent;
    }): {
        ok: true;
    } | Refusal;
    authorizeFileReply(input: CapabilityInput): {
        ok: true;
    } | Refusal;
    receiveFile(input: CapabilityInput & {
        eventId: string;
    }): {
        ok: true;
        file: Readonly<Record<string, unknown>>;
    } | Refusal;
    findFileReply(input: CapabilityInput & {
        requestKey: string;
        requestDigest: string;
    }): {
        ok: true;
        delivery: FileReplyResult | null;
    } | Refusal;
    queueFileReply(input: CapabilityInput & {
        requestKey: string;
        requestDigest: string;
        file: FileReplyManifest;
        body: string;
    }): {
        ok: true;
        delivery: FileReplyResult | null;
    } | Refusal;
    readFileReply(input: CapabilityInput & {
        commandId: string;
    }): {
        ok: true;
        delivery: FileReplyResult;
    } | Refusal;
    prepareFileReply(input: {
        commandId: string;
        claimToken: string;
        content: Readonly<Record<string, unknown>>;
    }): {
        ok: true;
        content: Readonly<Record<string, unknown>>;
    } | Refusal;
    private updateActivity;
    private enqueueTaskNotice;
    private insertNotice;
    private recordNoticeDelivery;
    private recordNoticeFailure;
    beginOutcomeInspection(dispatchIdInput: string, ttlMs?: number): OutcomeInspectionResult;
    resolveOutcomeUnknown(input: ResolveOutcomeUnknownInput): OutcomeResolutionResult;
    inspectWorkspace(resourceIdInput: string): Readonly<Record<string, unknown>> | Refusal;
    clearWorkspaceDirty(resourceIdInput: string): {
        ok: true;
    } | Refusal;
    taskOperation(input: TaskOperationInput): TaskOperationResult;
    checkInbox(input: CapabilityInput): readonly SessionInboxMessage[] | Refusal;
    checkInboxForAgent(input: CapabilityInput, agentNameInput: string): readonly SessionInboxMessage[] | Refusal;
    listAgentDispatches(agentIdInput: string): readonly {
        dispatchId: string;
        sessionId: string;
        state: DispatchRow['state'];
        createdAt: number;
        startedAt: number | null;
    }[];
    authorizeTaskFromDispatch(input: CapabilityInput & {
        taskId: string;
        write?: boolean;
    }): {
        ok: true;
    } | Refusal;
    deliverPeerMessageFromDispatch(input: CapabilityInput & {
        recipientAgentId: string;
        recipientAgentName: string;
        targetTaskId?: string;
        toolCallId: string;
        body: string;
    }): (IngestResult & {
        taskId?: string;
        threadRootEventId?: string;
    });
    listPendingPeerTaskInputs(): readonly {
        messageId: string;
        sessionId: string;
        taskId: string;
        roomId: string;
        threadRootEventId: string;
        recipientAgentId: string;
        recipientAgentName: string;
        senderAgentId: string;
        senderAgentName: string;
    }[];
    createTaskFromDispatch(input: CapabilityInput & {
        toolCallId: string;
        rootMessageId?: string;
        inputMessageIds: readonly string[];
        task: {
            title: string;
            description?: string;
            priority?: 'p0' | 'p1' | 'p2' | 'p3';
            granularity?: 'epic' | 'task' | 'subtask';
            assigneeAgentId: string;
            assigneeName: string;
            parentId?: string | null;
            labels?: readonly string[];
        };
        acknowledgementBody?: string;
    }): TaskIntentResult;
    assembleContext(sessionIdInput: string, tokenBudget?: number): Readonly<Record<string, unknown>> | Refusal;
    updateRollingSummary(sessionIdInput: string, summaryInput: string): {
        ok: true;
        contextGeneration: number;
    } | Refusal;
    /** Small allowlisted projection, independent of the dashboard history limit. */
    agentDispatchActivity(agentId: string): {
        activeDispatchCount: number;
        queuedDispatchCount: number;
        parkedDispatchCount: number;
    };
    snapshot(): RouterSnapshot;
    eventsAfter(after: number, limit?: number): EventPage;
    private meta;
    reconcileOnStart(): ReconcileReport;
}
export declare function openRouter(options: RouterOptions): RouterStore;
export {};
