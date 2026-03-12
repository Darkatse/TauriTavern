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
}
