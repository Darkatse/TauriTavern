export const COMMAND_ERROR_PREFIX_PATTERNS = Object.freeze([
    /^internal server error:\s*/i,
    /^internal error:\s*/i,
    /^validation error:\s*/i,
    /^bad request:\s*/i,
    /^unauthorized:\s*/i,
    /^permission denied:\s*/i,
    /^not found:\s*/i,
]);

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
