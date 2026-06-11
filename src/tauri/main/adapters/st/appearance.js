// @ts-check

function getSillyTavernThemeSelector() {
    const selector = document.getElementById('themes');
    if (!(selector instanceof HTMLSelectElement)) {
        throw new Error('Dynamic appearance: SillyTavern theme selector not found');
    }
    return selector;
}

/**
 * @param {string} themeName
 */
export function assertSillyTavernThemeAvailable(themeName) {
    const targetTheme = String(themeName || '').trim();
    if (!targetTheme) {
        throw new Error('Dynamic appearance theme target is empty');
    }

    const selector = getSillyTavernThemeSelector();
    const exists = Array.from(selector.options).some((option) => option.value === targetTheme);
    if (!exists) {
        throw new Error(`Dynamic appearance theme target not found: ${targetTheme}`);
    }

    return targetTheme;
}

/**
 * @param {string} themeName
 */
export function applySillyTavernTheme(themeName) {
    const targetTheme = assertSillyTavernThemeAvailable(themeName);
    const selector = getSillyTavernThemeSelector();
    if (selector.value === targetTheme) {
        return;
    }

    selector.value = targetTheme;
    selector.dispatchEvent(new Event('change', { bubbles: true }));
}

/**
 * @param {string} filename
 */
export async function assertSillyTavernGlobalBackgroundAvailable(filename) {
    const { assertSystemBackgroundExists } = await import('../../../../scripts/backgrounds.js');
    return assertSystemBackgroundExists(filename).filename;
}

/**
 * @param {string} filename
 */
export async function applySillyTavernGlobalBackground(filename) {
    const { applyGlobalBackground } = await import('../../../../scripts/backgrounds.js');
    await applyGlobalBackground(filename);
}
