export declare const LIVE_LOG_PANEL_BUFFER_LIMIT: number;
export declare const LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE: number;
export declare const LIVE_LOG_PANEL_WINDOW_GROW_STEP: number;
export declare const LIVE_LOG_PANEL_MAX_WINDOW_SIZE: number;

export declare const LOG_LEVEL_OPTIONS: string[];

export declare function formatTimestamp(ms: number): string;
export declare function formatTime(ms: number): string;
export declare function normalizeLevel(level: unknown): string;
export declare function entryMatchesLevel(entry: { level?: unknown }, filter: string): boolean;
export declare function levelClass(level: unknown): string;
export declare function formatEntryLine(entry: {
    timestampMs?: unknown;
    level?: unknown;
    target?: unknown;
    message?: unknown;
}): string;
