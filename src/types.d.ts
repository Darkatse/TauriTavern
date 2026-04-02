// Type declarations for modules without type definitions

declare module 'crypto-browserify';
declare module 'stream-browserify';
declare module 'os-browserify/browser';
declare module 'slidetoggle';
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

type TauriTavernHostApi = {
    chat?: TauriTavernChatApi;
    dev?: TauriTavernDevApi;
    worldInfo?: TauriTavernWorldInfoApi;
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
