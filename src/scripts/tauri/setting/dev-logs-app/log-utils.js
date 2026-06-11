export const LIVE_LOG_PANEL_BUFFER_LIMIT = 800;
export const LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE = 300;
export const LIVE_LOG_PANEL_WINDOW_GROW_STEP = 200;
export const LIVE_LOG_PANEL_MAX_WINDOW_SIZE = 800;

export const LOG_LEVEL_OPTIONS = ['ALL', 'DEBUG', 'INFO', 'WARN', 'ERROR'];

export function formatTimestamp(ms) {
    const date = new Date(Number(ms) || 0);
    if (Number.isNaN(date.getTime())) {
        return 'Invalid time';
    }
    return date.toLocaleString();
}

export function normalizeLevel(level) {
    const value = String(level || '').trim().toUpperCase();
    if (!value) {
        return 'INFO';
    }
    return value === 'WARNING' ? 'WARN' : value;
}

export function entryMatchesLevel(entry, filter) {
    if (!filter || filter === 'ALL') {
        return true;
    }
    return normalizeLevel(entry.level) === filter;
}

export function levelClass(level) {
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

export function formatEntryLine(entry) {
    const target = String(entry.target || '').trim();
    const targetSuffix = target ? ` [${target}]` : '';
    return `[${formatTimestamp(entry.timestampMs)}] [${normalizeLevel(entry.level)}]${targetSuffix} ${entry.message ?? ''}`;
}
