export const COMMAND_ERROR_PREFIX_PATTERNS = Object.freeze([
    /^internal server error:\s*/i,
    /^internal error:\s*/i,
    /^validation error:\s*/i,
    /^bad request:\s*/i,
    /^unauthorized:\s*/i,
    /^permission denied:\s*/i,
    /^not found:\s*/i,
]);

export function extractErrorText(value) {
    if (!value) {
        return '';
    }

    if (typeof value === 'string') {
        return value.trim();
    }

    if (value instanceof Error) {
        const message = typeof value.message === 'string' ? value.message.trim() : '';
        return message || String(value).trim();
    }

    if (typeof value?.message === 'string') {
        return value.message.trim();
    }

    // Tauri serializes CommandError as a single enum variant, e.g. { BadRequest: "…" }.
    if (typeof value === 'object' && Object.keys(value).length === 1) {
        const detail = Object.values(value)[0];
        if (typeof detail === 'string') return detail.trim();
    }

    return String(value).trim();
}

export function getCommandErrorMessage(value) {
    return stripCommandErrorPrefixes(extractErrorText(value));
}

export function stripCommandErrorPrefixes(message) {
    let normalized = String(message || '').trim();
    if (!normalized) {
        return '';
    }

    let previous = '';
    while (normalized && normalized !== previous) {
        previous = normalized;
        for (const pattern of COMMAND_ERROR_PREFIX_PATTERNS) {
            normalized = normalized.replace(pattern, '').trim();
        }
    }

    return normalized;
}
