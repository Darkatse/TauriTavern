/** Convert macro results to text without depending on the browser runtime. */
export function normalizeMacroResult(value) {
    if (value === null || value === undefined) {
        return '';
    }
    if (value instanceof Date) {
        return value.toISOString();
    }
    if (typeof value === 'object' || Array.isArray(value)) {
        try {
            return JSON.stringify(value);
        } catch (_error) {
            return String(value);
        }
    }

    return String(value);
}
