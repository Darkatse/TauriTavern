import type { LiveLogEntry } from './DevLogsContract';

export const LIVE_LOG_PANEL_BUFFER_LIMIT = 800;
export const LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE = 300;
export const LIVE_LOG_PANEL_WINDOW_GROW_STEP = 200;
export const LIVE_LOG_PANEL_MAX_WINDOW_SIZE = 800;

export const LOG_LEVEL_OPTIONS = Object.freeze(['ALL', 'DEBUG', 'INFO', 'WARN', 'ERROR']);

export function formatTimestamp(ms: number): string {
    const date = new Date(ms);
    if (Number.isNaN(date.getTime())) {
        return 'Invalid time';
    }
    return date.toLocaleString();
}

/** Compact row time; the full datetime stays in the copied export line. */
export function formatTime(ms: number): string {
    const date = new Date(ms);
    if (Number.isNaN(date.getTime())) {
        return 'Invalid time';
    }
    return date.toLocaleTimeString();
}

export function normalizeLevel(level: unknown): string {
    const value = typeof level === 'string' ? level.trim().toUpperCase() : '';
    if (!value) {
        return 'INFO';
    }
    return value === 'WARNING' ? 'WARN' : value;
}

export function entryMatchesLevel(entry: Pick<LiveLogEntry, 'level'>, filter: string): boolean {
    if (!filter || filter === 'ALL') {
        return true;
    }
    return normalizeLevel(entry.level) === filter;
}

export function levelClass(level: unknown): string {
    switch (normalizeLevel(level)) {
        case 'ERROR':
            return 'tt-dev-log-level-error';
        case 'WARN':
            return 'tt-dev-log-level-warn';
        case 'INFO':
            return 'tt-dev-log-level-info';
        case 'DEBUG':
            return 'tt-dev-log-level-debug';
        default:
            return 'tt-dev-log-level-other';
    }
}

export function formatEntryLine(entry: LiveLogEntry): string {
    const target = entry.target?.trim() ?? '';
    const targetSuffix = target ? ` [${target}]` : '';
    return `[${formatTimestamp(entry.timestampMs)}] [${normalizeLevel(entry.level)}]${targetSuffix} ${entry.message ?? ''}`;
}
