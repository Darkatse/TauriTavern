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
    __TAURITAVERN__?: {
        abiVersion: number;
        traceHeader: string;
        ready: Promise<void> | null;
        invoke: {
            safeInvoke: (command: any, args?: any) => Promise<any>;
            invalidate: (command: any, args?: any) => void;
            invalidateAll: (command: any) => void;
            flush: (command: any) => Promise<void>;
            flushAll: () => Promise<void>;
            broker: any;
        };
        assets: {
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
        api?: {
            chat?: {
                open: (ref: TauriTavernChatRef) => TauriTavernChatHandle;
                current: {
                    ref: () => TauriTavernChatRef;
                    handle: () => TauriTavernChatHandle;
                    windowInfo: () => Promise<TauriTavernChatWindowInfo>;
                };
            };
        };
    };

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

    __TAURITAVERN_EMBEDDED_RUNTIME__?: {
        profile: string;
        register: (slot: any) => { id: string; unregister: () => void };
        unregister: (id: string) => void;
        reconcile: () => void;
        getPerfSnapshot: () => any;
    };
}

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
