/**
 * Selects the first available theme while preserving missing higher-priority
 * references for explicit error reporting.
 *
 * @param {Array<{scope: 'chat'|'character'|'group'|'fallback', name: string|undefined}>} candidates
 * @param {string[]} availableThemeNames
 * @returns {{selected: {scope: 'chat'|'character'|'group'|'fallback', name: string}|null, missing: Array<{scope: 'chat'|'character'|'group'|'fallback', name: string}>}}
 */
export function resolveThemeBinding(candidates, availableThemeNames) {
    const availableThemes = new Set(availableThemeNames);
    const missing = [];

    for (const candidate of candidates) {
        if (!candidate.name) {
            continue;
        }
        if (availableThemes.has(candidate.name)) {
            return { selected: candidate, missing };
        }
        missing.push(candidate);
    }

    return { selected: null, missing };
}
