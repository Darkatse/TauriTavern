declare module 'droll';
declare module '@iconfu/svg-inject';

// Global variables
interface Window {
    // Tauri globals
    __TAURI__?: any;
    __TAURI_INTERNALS__?: any;
    __TAURI_RUNNING__?: boolean;

    __TAURITAVERN_MAIN_READY__?: Promise<void>;

    // TauriTavern host contract (public globals)
    __TAURITAVERN__?: TauriTavernHostAbi;

    // SillyTavern ecosystem library shim ABI
    _?: any;

    __TAURITAVERN_THUMBNAIL__?: (type: string, file: string, useTimestamp?: boolean) => string;
    __TAURITAVERN_THUMBNAIL_BLOB_URL__?: (
        type: string,
        file: string,
        options?: { animated?: boolean; useTimestamp?: boolean },
    ) => Promise<string>;
    __TAURITAVERN_BACKGROUND_PATH__?: (file: string) => string;
    __TAURITAVERN_AVATAR_PATH__?: (file: string) => string | null;
    __TAURITAVERN_PERSONA_PATH__?: (file: string) => string;

    __TAURITAVERN_IMPORT_ARCHIVE_PICKER__?: {
        onNativeResult: (payload: any) => void;
    };
    __TAURITAVERN_EXPORT_ARCHIVE_PICKER__?: {
        onNativeResult: (payload: any) => void;
    };

    __TAURITAVERN_HANDLE_BACK__?: () => boolean;
    __TAURITAVERN_NATIVE_SHARE__?: {
        push: (payload: any) => boolean;
        subscribe: (handler: (payload: any) => void) => () => void;
    };
    __TAURITAVERN_MOBILE_RUNTIME_COMPAT__?: boolean;
    __TAURITAVERN_MOBILE_OVERLAY_COMPAT__?: {
        dispose: () => void;
        revalidate: () => void;
    };
    __TAURITAVERN_MOBILE_WINDOW_OPEN_COMPAT__?: boolean;

    __TAURITAVERN_EMBEDDED_RUNTIME__?: {
        profile: string;
        register: (slot: any) => { id: string; unregister: () => void };
        unregister: (id: string) => void;
        reconcile: () => void;
        getPerfSnapshot: () => any;
    };
}

type TauriTavernHostInvokeApi = {
    safeInvoke: (command: any, args?: any) => Promise<any>;
    invalidate: (command: any, args?: any) => void;
    invalidateAll: (command: any) => void;
    flush: (command: any) => Promise<void>;
    flushAll: () => Promise<void>;
    broker: any;
};

type TauriTavernHostAssetsApi = {
    thumbnailUrl?: (type: string, file: string, useTimestamp?: boolean) => string;
    thumbnailBlobUrl?: (
        type: string,
        file: string,
        options?: { animated?: boolean; useTimestamp?: boolean },
    ) => Promise<string>;
    backgroundPath?: (file: string) => string;
    avatarPath?: (file: string) => string | null;
    personaPath?: (file: string) => string;
};

type TauriTavernChatApi = {
    open: (ref: TauriTavernChatRef) => TauriTavernChatHandle;
    current: {
        ref: () => TauriTavernChatRef;
        handle: () => TauriTavernChatHandle;
        windowInfo: () => Promise<TauriTavernChatWindowInfo>;
    };
};

type TauriTavernAgentRunStatus =
    | 'created'
    | 'initializing_workspace'
    | 'assembling_context'
    | 'calling_model'
    | 'dispatching_tool'
    | 'applying_workspace_patch'
    | 'creating_checkpoint'
    | 'awaiting_host_commit'
    | 'finishing'
    | 'completed'
    | 'partial_success'
    | 'cancelling'
    | 'cancelled'
    | 'failed';

type TauriTavernAgentRunPresentation = 'foreground' | 'background';

type TauriTavernAgentRunEvent = {
    seq: number;
    id: string;
    runId: string;
    timestamp: string;
    level: 'debug' | 'info' | 'warn' | 'error';
    type: string;
    payload?: any;
};

type TauriTavernAgentInvocationKind = 'root' | 'subagent' | 'handoff';

type TauriTavernAgentInvocationStatus =
    | 'created'
    | 'running'
    | 'completed'
    | 'failed'
    | 'cancelled'
    | 'transferred';

type TauriTavernAgentInvocationExitPolicy = 'run_finish_allowed' | 'task_return_required';

type TauriTavernAgentDelegationContinuation = 'return_to_parent' | 'transfer_control';

type TauriTavernAgentTaskStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';

type TauriTavernAgentRunTimelineInvocation = {
    invocationId: string;
    parentInvocationId?: string;
    profileId: string;
    kind: TauriTavernAgentInvocationKind;
    status: TauriTavernAgentInvocationStatus;
    exitPolicy: TauriTavernAgentInvocationExitPolicy;
    createdAt: string;
    updatedAt: string;
};

type TauriTavernAgentRunTimelineDelegationEdge = {
    taskId: string;
    sourceInvocationId: string;
    targetInvocationId: string;
    targetProfileId: string;
    workspaceKey: string;
    continuation: TauriTavernAgentDelegationContinuation;
    status: TauriTavernAgentTaskStatus;
    resultRef?: string;
    error?: string;
    createdAt: string;
    updatedAt: string;
};

type TauriTavernAgentRunTimelineProjection = {
    foregroundInvocationIds: string[];
    invocations: TauriTavernAgentRunTimelineInvocation[];
    delegationEdges: TauriTavernAgentRunTimelineDelegationEdge[];
};

type TauriTavernAgentRunHandle = {
    runId: string;
    workspaceId: string;
    stableChatId: string;
    generationType: string;
    status: TauriTavernAgentRunStatus;
};

type TauriTavernAgentRunListCursor = {
    createdAt: string;
    runId: string;
};

type TauriTavernAgentRunSummary = {
    runId: string;
    workspaceId: string;
    stableChatId: string;
    chatRef: TauriTavernChatRef;
    generationType: string;
    profileId?: string;
    skillScopeRefs?: {
        preset?: TauriTavernAgentPresetRef;
        characterId?: string;
    };
    persistBaseStateId?: string;
    inputMessageCount?: number;
    presentation: TauriTavernAgentRunPresentation;
    status: TauriTavernAgentRunStatus;
    createdAt: string;
    updatedAt: string;
    commitCount: number;
    committedMessage?: {
        commitId: string;
        messageId: string;
        messageIndex?: number;
        committedAt: string;
    };
    terminalAt?: string;
};

type TauriTavernAgentRunPruneRetention = {
    keepRecentTerminalRuns: number;
    keepFullRecentRuns: number;
};

type TauriTavernAgentRunRetentionSettings = TauriTavernAgentRunPruneRetention & {
    autoPruneEnabled: boolean;
};

type TauriTavernAgentRunPruneAction = 'slim_heavy_artifacts' | 'delete_run';
type TauriTavernAgentRunPruneReason = 'outside_full_retention_window' | 'outside_history_retention_window';
type TauriTavernAgentRunPruneBlockReason = 'active_run' | 'missing_terminal_event' | 'invalid_journal' | 'invalid_storage';

type TauriTavernAgentRunPruneCandidate = {
    runId: string;
    workspaceId: string;
    stableChatId: string;
    chatRef: TauriTavernChatRef;
    status: TauriTavernAgentRunStatus;
    createdAt: string;
    updatedAt: string;
    action: TauriTavernAgentRunPruneAction;
    reason: TauriTavernAgentRunPruneReason;
    fileCount: number;
    byteCount: number;
};

type TauriTavernAgentRunPruneBlockedRun = TauriTavernAgentRunPruneCandidate & {
    blockReason: TauriTavernAgentRunPruneBlockReason;
    message?: string;
};

type TauriTavernAgentRunPruneFailedRun = TauriTavernAgentRunPruneCandidate & {
    message: string;
};

type TauriTavernAgentRunPrunePlan = {
    retention: TauriTavernAgentRunPruneRetention;
    detailLimit: number;
    terminalRunCount: number;
    nonTerminalRunCount: number;
    blockedRunCount: number;
    fullRetainedRunCount: number;
    coreRetainedRunCount: number;
    slimCandidateCount: number;
    deleteCandidateCount: number;
    totalSlimFileCount: number;
    totalSlimByteCount: number;
    totalDeleteFileCount: number;
    totalDeleteByteCount: number;
    totalCandidateFileCount: number;
    totalCandidateByteCount: number;
    candidateDetailsTruncated: boolean;
    candidates: TauriTavernAgentRunPruneCandidate[];
    blockedDetailsTruncated: boolean;
    blockedRuns: TauriTavernAgentRunPruneBlockedRun[];
};

type TauriTavernAgentRunPruneApplyResult = {
    retention: TauriTavernAgentRunPruneRetention;
    detailLimit: number;
    slimmedRunCount: number;
    deletedRunCount: number;
    failedRunCount: number;
    removedFileCount: number;
    removedByteCount: number;
    failedDetailsTruncated: boolean;
    failedRuns: TauriTavernAgentRunPruneFailedRun[];
    afterPlan: TauriTavernAgentRunPrunePlan;
};

type TauriTavernAgentModelTurn = {
    runId: string;
    round: number;
    modelResponsePath: string;
    provider: {
        source?: string;
        format?: string;
        model?: string;
        responseId?: string;
        usage?: any;
    };
    assistant: {
        text: string;
        totalChars: number;
        totalWords: number;
        truncated: boolean;
    };
    narration?: {
        source: 'assistantText';
        text: string;
        totalChars: number;
        totalWords: number;
        truncated: boolean;
    } | null;
    reasoning: Array<{
        source: string;
        text: string;
        totalChars: number;
        totalWords: number;
        truncated: boolean;
    }>;
    toolCalls: Array<{
        callId: string;
        name: string;
        modelName?: string;
    }>;
};

type TauriTavernAgentProfileSummary = {
    id: string;
    displayName: string;
    description?: string;
    directRunnable: boolean;
};

type TauriTavernAgentToolSpec = {
    name: string;
    modelName: string;
    title: string;
    description: string;
    inputSchema: any;
    outputSchema?: any;
    annotations?: any;
    source: string;
};

type TauriTavernAgentProfileDefinition = {
    schemaVersion: number;
    kind: 'tauritavern.agentProfile';
    id: string;
    displayName: string;
    description?: string;
    preset: {
        mode: 'currentPromptSnapshot' | 'ref' | 'none';
        ref?: {
            apiId: string;
            name: string;
        };
        required?: boolean;
    };
    model: {
        mode: 'currentPromptSnapshot' | 'connectionRef' | 'requiresConfiguration';
        connectionRef?: string;
        modelId?: string;
    };
    run: {
        presentation: TauriTavernAgentRunPresentation;
        directRunnable: boolean;
        modelRetry: {
            maxRetries: number;
            intervalMs: number;
        };
    };
    instructions: {
        agentSystemPrompt?: string | null;
    };
    tools: {
        allow: string[];
        deny?: string[];
        toolDescriptions?: Record<string, {
            description?: string;
            properties?: Record<string, string>;
        }>;
        maxRounds: number;
        maxCallsPerRun: number;
        maxCallsPerTool?: Record<string, number>;
    };
    skills: {
        visible: string[];
        deny?: string[];
        maxReadCharsPerCall: number;
        maxReadCharsPerRun: number;
    };
    workspace: {
        visibleRoots: string[];
        writableRoots: string[];
    };
    plan: {
        mode: 'none' | 'free' | 'strict' | 'hybrid';
        beta?: boolean;
        nodes?: Array<{
            id: string;
            title: string;
            locked: boolean;
        }>;
    };
    output: {
        artifacts: Array<{
            id: string;
            path: string;
            kind: string;
            target: 'messageBody';
            required?: boolean;
            assemblyOrder?: number;
        }>;
    };
};

type TauriTavernAgentPresetRef = {
    apiId: string;
    name: string;
};

type TauriTavernAgentProfileStorageIssue = {
    profileId: string;
    fileName: string;
    kind: 'invalidJson' | 'invalidFileIdentity' | 'invalidProfile';
    recommendedAction?: 'delete' | 'normalizeIdentity';
    message: string;
};

type TauriTavernAgentProfileDiagnostic = {
    code: string;
    severity: 'error';
    path: string;
    message: string;
    resource?: {
        kind: 'preset' | 'llmConnection' | 'model';
        apiId?: string;
        name?: string;
        id?: string;
        modelId?: string;
    };
    blocks?: Array<'preview' | 'promptAssembly' | 'directRun' | 'subAgent'>;
    repairActions?: Array<'selectPreset' | 'selectModel' | 'setModelRequiresConfiguration' | 'openJsonEditor'>;
};

type TauriTavernAgentProfileHealth = {
    profileId: string;
    previewAvailable: boolean;
    promptAssemblyAvailable: boolean;
    directRunAvailable: boolean;
    subAgentAvailable: boolean;
    diagnostics: TauriTavernAgentProfileDiagnostic[];
};

type TauriTavernAgentProfilesApi = {
    list: () => Promise<{
        profiles: TauriTavernAgentProfileSummary[];
        issues: TauriTavernAgentProfileStorageIssue[];
    }>;
    load: (input: string | { profileId: string }) => Promise<{ profile: TauriTavernAgentProfileDefinition | null }>;
    diagnose: (input: string | { profileId: string }) => Promise<TauriTavernAgentProfileHealth>;
    resolveSystemPrompt: (input?: string | { profileId?: string | null }) => Promise<{ agentSystemPrompt: string }>;
    repairFile: (input: { profileId: string; action: 'delete' | 'normalizeIdentity' }) => Promise<void>;
    retargetPresetRefs: (input: {
        from: TauriTavernAgentPresetRef;
        to: TauriTavernAgentPresetRef;
    }) => Promise<{ updated: number; profileIds: string[] }>;
    save: (input: TauriTavernAgentProfileDefinition | { profile: TauriTavernAgentProfileDefinition }) => Promise<void>;
    delete: (input: string | { profileId: string }) => Promise<void>;
};

type TauriTavernAgentToolsApi = {
    list: () => Promise<{ tools: TauriTavernAgentToolSpec[] }>;
};

type TauriTavernAgentPromptAssemblyApi = {
    prepare: (input: {
        profileId?: string | null;
        generationType?: string;
        frozenRunInputSnapshot: Record<string, any>;
        jsonSchema?: any;
    }) => Promise<{
        mode: 'currentPromptSnapshot' | 'frontendPromptAssembly';
        request?: any;
        assembly?: any;
    }>;
    buildSnapshot: (input: {
        generationType?: string;
        frozenRunInputSnapshot: Record<string, any>;
        settings?: Record<string, any>;
        presetSettings?: Record<string, any>;
        modelId?: string | null;
        profileId?: string | null;
        agentContextPolicy?: Record<string, any>;
        contextPolicy?: Record<string, any>;
        agentSystemPrompt?: string | null;
        agentTaskPrompt?: string | null;
        requiredAgentPromptComponents?: string[];
        jsonSchema?: any;
    }) => Promise<{
        promptSnapshot: any;
        frozenRunInputSnapshot: any;
        generationIntent: any;
        assembly: any;
    }>;
};

type TauriTavernAgentRetentionApi = {
    readSettings: () => Promise<TauriTavernAgentRunRetentionSettings>;
    updateSettings: (input: Partial<TauriTavernAgentRunRetentionSettings>) => Promise<TauriTavernAgentRunRetentionSettings>;
    planPrune: (input?: {
        retention?: TauriTavernAgentRunPruneRetention | TauriTavernAgentRunRetentionSettings;
        detailLimit?: number;
    }) => Promise<TauriTavernAgentRunPrunePlan>;
    applyPrune: (input?: {
        retention?: TauriTavernAgentRunPruneRetention | TauriTavernAgentRunRetentionSettings;
        detailLimit?: number;
    }) => Promise<TauriTavernAgentRunPruneApplyResult>;
};

type TauriTavernAgentApi = {
    startRunWithPromptSnapshot: (input: {
        chatRef: TauriTavernChatRef;
        stableChatId?: string;
        generationType?: string;
        profileId?: string | null;
        promptSnapshot: any;
        frozenRunInputSnapshot?: any;
        generationIntent?: any;
        presentation?: TauriTavernAgentRunPresentation;
        options?: { presentation?: TauriTavernAgentRunPresentation; stream?: boolean };
    }) => Promise<TauriTavernAgentRunHandle>;
    startRunFromLegacyGenerate: (input?: {
        chatRef?: TauriTavernChatRef;
        stableChatId?: string;
        generationType?: string;
        generateOptions?: Record<string, any>;
        profileId?: string | null;
        generationIntent?: any;
        presentation?: TauriTavernAgentRunPresentation;
        options?: { presentation?: TauriTavernAgentRunPresentation; stream?: false };
    }) => Promise<TauriTavernAgentRunHandle>;
    cancel: (runId: string) => Promise<TauriTavernAgentRunHandle>;
    readEvents: (input: {
        runId: string;
        afterSeq?: number;
        beforeSeq?: number;
        limit?: number;
        invocationId?: string;
        includeTimelineProjection?: boolean;
    }) => Promise<{
        events: TauriTavernAgentRunEvent[];
        timelineProjection?: TauriTavernAgentRunTimelineProjection;
    }>;
    readWorkspaceFile: (input: {
        runId: string;
        path: string;
    }) => Promise<{ path: string; text: string; chars: number; words: number; sha256: string }>;
    readModelTurn: (input: {
        runId: string;
        invocationId?: string;
        round: number;
        maxChars?: number;
    }) => Promise<TauriTavernAgentModelTurn>;
    subscribe: (
        runId: string,
        handler: (event: TauriTavernAgentRunEvent) => void,
        options?: { afterSeq?: number; limit?: number; intervalMs?: number; onError?: (error: unknown) => void },
    ) => TauriTavernHostUnsubscribe;
    profiles: TauriTavernAgentProfilesApi;
    tools: TauriTavernAgentToolsApi;
    promptAssembly: TauriTavernAgentPromptAssemblyApi;
    retention: TauriTavernAgentRetentionApi;
    approveToolCall: () => never;
    listRuns: (input?: {
        chatRef?: TauriTavernChatRef;
        stableChatId?: string;
        statuses?: TauriTavernAgentRunStatus[];
        before?: TauriTavernAgentRunListCursor;
        limit?: number;
    }) => Promise<{
        runs: TauriTavernAgentRunSummary[];
        nextCursor?: TauriTavernAgentRunListCursor;
    }>;
    readDiff: () => never;
    rollback: () => never;
};

type TauriTavernLlmConnectionSummary = {
    id: string;
    displayName: string;
    description?: string;
    chatCompletionSource: string;
    customApiFormat?: string;
};

type TauriTavernLlmConnectionDefinition = {
    schemaVersion: number;
    kind: 'tauritavern.llmConnection';
    id: string;
    displayName: string;
    description?: string;
    provider: {
        chatCompletionSource: string;
        customApiFormat?: string;
    };
    endpoint?: {
        baseUrl?: string;
        sourceSpecific?: Record<string, any>;
    };
    auth: {
        secretRef: {
            key: string;
            id: string;
            labelSnapshot?: string;
        };
    };
    routing?: {
        reverseProxy?: {
            url: string;
        };
    };
    adapterHints?: {
        promptPostProcessing?: string;
        customIncludeHeaders?: string;
        customIncludeBody?: string;
        customExcludeBody?: string;
    };
    capabilities?: {
        streaming?: string;
        toolCalling?: string;
    };
};

type TauriTavernLlmConnectionsApi = {
    list: () => Promise<{ connections: TauriTavernLlmConnectionSummary[] }>;
    load: (input: string | { connectionId: string } | { connection_id: string }) => Promise<{
        connection: TauriTavernLlmConnectionDefinition | null;
    }>;
    save: (input: TauriTavernLlmConnectionDefinition | { connection: TauriTavernLlmConnectionDefinition }) => Promise<void>;
    delete: (input: string | { connectionId: string } | { connection_id: string }) => Promise<void>;
};

type TauriTavernSkillFileKind = 'text' | 'binary';

type TauriTavernSkillImportConflictKind = 'new' | 'same' | 'different';

type TauriTavernSkillInstallConflictStrategy = 'skip' | 'replace';

type TauriTavernSkillInstallAction = 'installed' | 'replaced' | 'already_installed' | 'skipped';

type TauriTavernSkillScope =
    | { kind: 'global' }
    | { kind: 'preset'; apiId: string; name: string }
    | { kind: 'profile'; profileId: string }
    | { kind: 'character'; characterId: string };

type TauriTavernSkillScopeFilter =
    | { kind: 'all' }
    | TauriTavernSkillScope;

type TauriTavernSkillIndexEntry = {
    scope: TauriTavernSkillScope;
    name: string;
    description: string;
    displayName?: string;
    sourceKind?: string;
    license?: string;
    author?: string;
    version?: string;
    tags: string[];
    installedHash: string;
    fileCount: number;
    totalBytes: number;
    hasScripts: boolean;
    hasBinary: boolean;
    installedAt: string;
    sourceRefs?: TauriTavernSkillSourceRef[];
};

type TauriTavernSkillSourceRef = {
    kind: string;
    id: string;
    label: string;
    installedHash: string;
};

type TauriTavernSkillInlineFile = {
    path: string;
    encoding?: 'utf8' | 'utf-8' | 'base64';
    content: string;
    mediaType?: string;
    sizeBytes?: number;
    sha256?: string;
};

type TauriTavernSkillImportInput =
    | {
        kind: 'inlineFiles';
        files: TauriTavernSkillInlineFile[];
        source?: any;
    }
    | {
        kind: 'directory';
        path: string;
        source?: any;
    }
    | {
        kind: 'archiveFile';
        path: string;
        source?: any;
    }
    | {
        kind: 'archiveBase64';
        fileName: string;
        contentBase64: string;
        sha256?: string;
        source?: any;
    };

type TauriTavernSkillFileRef = {
    path: string;
    kind: TauriTavernSkillFileKind;
    mediaType: string;
    sizeBytes: number;
    sha256: string;
};

type TauriTavernSkillImportPreview = {
    skill: TauriTavernSkillIndexEntry;
    files: TauriTavernSkillFileRef[];
    conflict: {
        kind: TauriTavernSkillImportConflictKind;
        installedHash?: string;
    };
    warnings: string[];
    source: any;
};

type TauriTavernSkillInstallResult = {
    scope: TauriTavernSkillScope;
    name: string;
    action: TauriTavernSkillInstallAction;
    skill?: TauriTavernSkillIndexEntry;
};

type TauriTavernSkillReadResult = {
    name: string;
    path: string;
    content: string;
    chars: number;
    words: number;
    totalChars: number;
    totalWords: number;
    startChar: number;
    endChar: number;
    totalLines: number;
    startLine: number;
    endLine: number;
    bytes: number;
    sha256: string;
    truncated: boolean;
    resourceRef: string;
};

type TauriTavernSkillExportPayload = {
    fileName: string;
    contentBase64: string;
    sha256: string;
};

type TauriTavernSkillApi = {
    list: (options?: { scope?: TauriTavernSkillScopeFilter; filter?: TauriTavernSkillScopeFilter }) => Promise<TauriTavernSkillIndexEntry[]>;
    listFiles: (options: { scope?: TauriTavernSkillScope; name: string }) => Promise<TauriTavernSkillFileRef[]>;
    pickImportArchive: () => Promise<TauriTavernSkillImportInput | null>;
    discardPickedImport: (input?: TauriTavernSkillImportInput | null) => Promise<void>;
    downloadImport: (options: { url: string }) => Promise<TauriTavernSkillImportInput>;
    previewImport: (options: {
        input: TauriTavernSkillImportInput;
        targetScope?: TauriTavernSkillScope;
    }) => Promise<TauriTavernSkillImportPreview>;
    installImport: (request: {
        input: TauriTavernSkillImportInput;
        targetScope?: TauriTavernSkillScope;
        conflictStrategy?: TauriTavernSkillInstallConflictStrategy;
    }) => Promise<TauriTavernSkillInstallResult>;
    readFile: (options: {
        scope?: TauriTavernSkillScope;
        name: string;
        path: string;
        maxChars?: number;
        startLine?: number;
        lineCount?: number;
        startChar?: number;
    }) => Promise<TauriTavernSkillReadResult>;
    writeFile: (options: {
        scope?: TauriTavernSkillScope;
        name: string;
        path: string;
        content: string;
        expectedSha256?: string;
    }) => Promise<TauriTavernSkillReadResult>;
    export: (options: { scope?: TauriTavernSkillScope; name: string }) => Promise<TauriTavernSkillExportPayload>;
    delete: (options: { scope?: TauriTavernSkillScope; name: string }) => Promise<void>;
    move: (request: {
        name: string;
        fromScope: TauriTavernSkillScope;
        toScope: TauriTavernSkillScope;
        conflictStrategy?: TauriTavernSkillInstallConflictStrategy;
    }) => Promise<TauriTavernSkillInstallResult>;
    retargetScope: (request: {
        fromScope: TauriTavernSkillScope;
        toScope: TauriTavernSkillScope;
    }) => Promise<any>;
};

type TauriTavernFrontendLogsApi = {
    list: (options?: { limit?: number }) => Promise<TauriTavernFrontendLogEntry[]>;
    subscribe: (
        handler: (entry: TauriTavernFrontendLogEntry) => void,
    ) => Promise<TauriTavernHostUnsubscribe>;
    getConsoleCaptureEnabled: () => Promise<boolean>;
    setConsoleCaptureEnabled: (enabled: boolean) => Promise<void>;
};

type TauriTavernBackendLogsApi = {
    tail: (options?: { limit?: number }) => Promise<TauriTavernBackendLogEntry[]>;
    subscribe: (
        handler: (entry: TauriTavernBackendLogEntry) => void,
    ) => Promise<TauriTavernHostUnsubscribe>;
};

type TauriTavernLlmApiLogsApi = {
    index: (options?: { limit?: number }) => Promise<TauriTavernLlmApiLogIndexEntry[]>;
    getPreview: (id: number) => Promise<TauriTavernLlmApiLogPreview>;
    getRaw: (id: number) => Promise<TauriTavernLlmApiLogRaw>;
    subscribeIndex: (
        handler: (entry: TauriTavernLlmApiLogIndexEntry) => void,
    ) => Promise<TauriTavernHostUnsubscribe>;
    getKeep: () => Promise<number>;
    setKeep: (value: number) => Promise<void>;
};

type TauriTavernDevApi = {
    frontendLogs: TauriTavernFrontendLogsApi;
    backendLogs: TauriTavernBackendLogsApi;
    llmApiLogs: TauriTavernLlmApiLogsApi;
};

type TauriTavernWorldInfoApi = {
    getLastActivation: () => Promise<TauriTavernWorldInfoActivationBatch | null>;
    subscribeActivations: (
        handler: (batch: TauriTavernWorldInfoActivationBatch) => void,
    ) => Promise<TauriTavernHostUnsubscribe>;
    openEntry: (ref: TauriTavernWorldInfoEntryRef) => Promise<{ opened: boolean }>;
};

type TauriTavernExtensionStoreApi = {
    getJson: (options: { namespace: string; key: string; table?: string }) => Promise<any>;
    tryGetJson: (options: { namespace: string; key: string; table?: string }) => Promise<{ found: boolean; value?: any }>;
    setJson: (options: { namespace: string; key: string; value: any; table?: string }) => Promise<void>;
    updateJson: (options: { namespace: string; key: string; value: any; table?: string }) => Promise<void>;
    updateJSON: (options: { namespace: string; key: string; value: any; table?: string }) => Promise<void>;
    renameKey: (options: { namespace: string; key: string; newKey: string; table?: string }) => Promise<void>;
    updateKey: (options: { namespace: string; key: string; newKey: string; table?: string }) => Promise<void>;
    deleteJson: (options: { namespace: string; key: string; table?: string }) => Promise<void>;
    listKeys: (options: { namespace: string; table?: string }) => Promise<string[]>;
    listTables: (options: { namespace: string }) => Promise<string[]>;
    deleteTable: (options: { namespace: string; table: string }) => Promise<void>;
    getBlob: (options: { namespace: string; key: string; table?: string }) => Promise<Blob>;
    setBlob: (options: {
        namespace: string;
        key: string;
        table?: string;
        data: Blob | ArrayBuffer | Uint8Array | string;
    }) => Promise<void>;
    deleteBlob: (options: { namespace: string; key: string; table?: string }) => Promise<void>;
    listBlobKeys: (options: { namespace: string; table?: string }) => Promise<string[]>;
};

type TauriTavernExtensionApi = {
    store: TauriTavernExtensionStoreApi;
};

type TauriTavernLayoutInsets = {
    top: number;
    right: number;
    bottom: number;
    left: number;
};

type TauriTavernLayoutFrame = {
    left: number;
    top: number;
    width: number;
    height: number;
    right: number;
    bottom: number;
};

type TauriTavernLayoutImeKind = 'composer' | 'fixed-shell' | 'dialog';

type TauriTavernLayoutImeSnapshot = {
    activeSurface: Element | null;
    kind: TauriTavernLayoutImeKind;
    bottom: number;
    viewportBottomInset: number;
    keyboardOffset: number;
};

type TauriTavernLayoutSnapshot = {
    version: number;
    timestampMs: number;
    viewport: TauriTavernLayoutFrame;
    safeInsets: TauriTavernLayoutInsets;
    safeFrame: TauriTavernLayoutFrame;
    ime: TauriTavernLayoutImeSnapshot;
};

type TauriTavernLayoutApi = {
    snapshot: () => TauriTavernLayoutSnapshot;
    subscribe: (
        handler: (snapshot: TauriTavernLayoutSnapshot) => void,
    ) => Promise<TauriTavernHostUnsubscribe>;
};

type TauriTavernHostApi = {
    chat?: TauriTavernChatApi;
    agent?: TauriTavernAgentApi;
    llmConnections?: TauriTavernLlmConnectionsApi;
    skill?: TauriTavernSkillApi;
    layout?: TauriTavernLayoutApi;
    dev?: TauriTavernDevApi;
    worldInfo?: TauriTavernWorldInfoApi;
    extension?: TauriTavernExtensionApi;
};

type TauriTavernHostAbi = {
    abiVersion: number;
    traceHeader: string;
    ready: Promise<void> | null;
    invoke: TauriTavernHostInvokeApi;
    assets: TauriTavernHostAssetsApi;
    api?: TauriTavernHostApi;
};

type TauriTavernHostUnsubscribe = () => void | Promise<void>;

type TauriTavernFrontendLogEntry = {
    id: number;
    timestampMs: number;
    level: 'debug' | 'info' | 'warn' | 'error';
    message: string;
    target?: string;
};

type TauriTavernBackendLogEntry = {
    id: number;
    timestampMs: number;
    level: 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
    target: string;
    message: string;
};

type TauriTavernLlmApiRawKind = 'json' | 'sse';

type TauriTavernLlmApiLogIndexEntry = {
    id: number;
    timestampMs: number;
    level: 'INFO' | 'ERROR';
    ok: boolean;
    source: string;
    model: string | null;
    endpoint: string;
    durationMs: number;
    stream: boolean;
};

type TauriTavernLlmApiLogPreview = {
    id: number;
    timestampMs: number;
    level: 'INFO' | 'ERROR';
    ok: boolean;
    source: string;
    model: string | null;
    endpoint: string;
    durationMs: number;
    stream: boolean;
    errorMessage: string | null;
    requestReadable: string;
    responseReadable: string;
    responseRawKind: TauriTavernLlmApiRawKind | null;
};

type TauriTavernLlmApiLogRaw = {
    id: number;
    requestRaw: string;
    responseRaw: string;
    responseRawKind: TauriTavernLlmApiRawKind | null;
};

type TauriTavernWorldInfoEntryRef = {
    world: string;
    uid: string | number;
};

type TauriTavernWorldInfoActivationPosition =
    | 'before'
    | 'after'
    | 'an_top'
    | 'an_bottom'
    | 'depth'
    | 'em_top'
    | 'em_bottom'
    | 'outlet';

type TauriTavernWorldInfoActivationEntry = {
    world: string;
    uid: string | number;
    displayName: string;
    constant: boolean;
    position?: TauriTavernWorldInfoActivationPosition;
};

type TauriTavernWorldInfoActivationBatch = {
    timestampMs: number;
    trigger: string;
    entries: TauriTavernWorldInfoActivationEntry[];
};

type TauriTavernChatRef =
    | { kind: 'character'; characterId: string; fileName: string }
    | { kind: 'group'; chatId: string };

type TauriTavernChatSummary = {
    character_name: string;
    file_name: string;
    file_size: number;
    message_count: number;
    preview: string;
    date: number;
    chat_id: string | null;
    chat_metadata?: unknown | null;
};

type TauriTavernChatHistoryPage = {
    startIndex: number;
    totalCount: number;
    messages: ChatMessage[];
    cursor: any;
    hasMoreBefore: boolean;
};

type TauriTavernChatWindowInfo = {
    mode: 'windowed' | 'off';
    chatKind: TauriTavernChatRef['kind'];
    chatRef: TauriTavernChatRef;
    totalCount: number;
    windowStartIndex: number;
    windowLength: number;
};

type TauriTavernChatMessageSearchFilters = {
    role?: 'user' | 'assistant' | 'system';
    startIndex?: number;
    endIndex?: number;
    scanLimit?: number;
};

type TauriTavernChatMessageSearchHit = {
    index: number;
    score: number;
    snippet: string;
    role: 'user' | 'assistant' | 'system';
    text: string;
};

type TauriTavernChatHandle = {
    ref: TauriTavernChatRef;
    summary: (options?: { includeMetadata?: boolean }) => Promise<TauriTavernChatSummary>;
    stableId: () => Promise<string>;
    searchMessages: (options: {
        query: string;
        limit?: number;
        filters?: TauriTavernChatMessageSearchFilters;
    }) => Promise<TauriTavernChatMessageSearchHit[]>;
    metadata: {
        get: () => Promise<ChatMetadata>;
        setExtension: (options: { namespace: string; value: unknown }) => Promise<void>;
    };
    store: {
        getJson: (options: { namespace: string; key: string }) => Promise<unknown>;
        setJson: (options: { namespace: string; key: string; value: unknown }) => Promise<void>;
        updateJson: (options: { namespace: string; key: string; value: unknown }) => Promise<void>;
        updateJSON: (options: { namespace: string; key: string; value: unknown }) => Promise<void>;
        renameKey: (options: { namespace: string; key: string; newKey: string }) => Promise<void>;
        deleteJson: (options: { namespace: string; key: string }) => Promise<void>;
        listKeys: (options: { namespace: string }) => Promise<string[]>;
    };
    locate: {
        findLastMessage: (query?: unknown) => Promise<{ index: number; message: ChatMessage } | null>;
    };
    history: {
        tail: (options: { limit: number }) => Promise<TauriTavernChatHistoryPage>;
        before: (
            page: TauriTavernChatHistoryPage,
            options: { limit: number },
        ) => Promise<TauriTavernChatHistoryPage>;
        beforePages: (
            page: TauriTavernChatHistoryPage,
            options: { limit: number; pages: number },
        ) => Promise<TauriTavernChatHistoryPage[]>;
    };
};
