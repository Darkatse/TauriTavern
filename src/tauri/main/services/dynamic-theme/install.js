// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { getTauriTavernSettings } from '../../../../tauri-bridge.js';
import { DYNAMIC_THEME_CHANGED_EVENT } from './constants.js';

function getSillyTavernThemeSelector() {
    const selector = document.getElementById('themes');
    if (!(selector instanceof HTMLSelectElement)) {
        throw new Error('Dynamic theme: SillyTavern theme selector not found');
    }
    return selector;
}

function readSystemThemeFromMedia() {
    const query = globalThis.matchMedia?.('(prefers-color-scheme: dark)');
    if (!query) {
        return 'light';
    }

    return query.matches ? 'dark' : 'light';
}

function getPreferredColorSchemeQuery() {
    const matchMedia = globalThis.matchMedia;
    if (typeof matchMedia !== 'function') {
        return null;
    }

    return matchMedia('(prefers-color-scheme: dark)');
}

/**
 * @param {unknown} payload
 */
function normalizeDynamicThemeSettings(payload) {
    if (!payload || typeof payload !== 'object') {
        throw new Error('Dynamic theme settings are missing');
    }

    const settings = /** @type {any} */ (payload);

    const enabled = Boolean(settings.enabled);
    const dayTheme = String(settings.day_theme || '').trim();
    const nightTheme = String(settings.night_theme || '').trim();

    return { enabled, dayTheme, nightTheme };
}

/**
 * @param {string} themeName
 */
function applySillyTavernTheme(themeName) {
    const selector = getSillyTavernThemeSelector();
    if (selector.value === themeName) {
        return;
    }

    const exists = Array.from(selector.options).some((option) => option.value === themeName);
    if (!exists) {
        throw new Error(`Dynamic theme target not found: ${themeName}`);
    }

    selector.value = themeName;
    selector.dispatchEvent(new Event('change', { bubbles: true }));
}

export function installDynamicTheme() {
    const ready = getTauriTavernSettings().then((settings) => {
        let dynamicTheme = normalizeDynamicThemeSettings(settings.dynamic_theme);
        const preferredColorSchemeQuery = getPreferredColorSchemeQuery();
        let systemTheme = preferredColorSchemeQuery?.matches ? 'dark' : readSystemThemeFromMedia();

        /** @param {string} reason */
        const syncNow = (reason) => {
            if (!dynamicTheme.enabled) {
                return;
            }

            const targetTheme = systemTheme === 'dark' ? dynamicTheme.nightTheme : dynamicTheme.dayTheme;
            if (!targetTheme) {
                throw new Error('Dynamic theme is enabled but the target theme is empty');
            }

            applySillyTavernTheme(targetTheme);
            console.debug('Dynamic theme applied', { reason, systemTheme, targetTheme });
        };

        /**
         * @param {'light' | 'dark'} nextTheme
         * @param {string} reason
         */
        const updateSystemThemeAndSync = (nextTheme, reason) => {
            if (nextTheme === systemTheme) {
                return;
            }

            systemTheme = nextTheme;
            if (document.visibilityState === 'hidden') {
                return;
            }

            void Promise.resolve()
                .then(() => syncNow(reason))
                .catch((error) => {
                    console.error('Dynamic theme sync failed after system theme change', error);
                });
        };

        /** @param {Event} event */
        const handleConfigChanged = (event) => {
            dynamicTheme = normalizeDynamicThemeSettings(/** @type {any} */ (event).detail);
            void Promise.resolve()
                .then(() => syncNow('config-changed'))
                .catch((error) => {
                    console.error('Dynamic theme sync failed after config change', error);
                });
        };

        window.addEventListener(DYNAMIC_THEME_CHANGED_EVENT, handleConfigChanged);

        eventSource.on(event_types.APP_READY, () => {
            void Promise.resolve()
                .then(() => syncNow('startup'))
                .catch((error) => {
                    console.error('Dynamic theme initial sync failed', error);
                });

            if (!preferredColorSchemeQuery) {
                throw new Error('Dynamic theme: matchMedia is unavailable');
            }

            const handlePreferredColorSchemeChange = (/** @type {any} */ event) => {
                const nextTheme = event?.matches ? 'dark' : 'light';
                updateSystemThemeAndSync(nextTheme, 'matchMedia');
            };

            if (typeof preferredColorSchemeQuery.addEventListener === 'function') {
                preferredColorSchemeQuery.addEventListener('change', handlePreferredColorSchemeChange);
            } else if (typeof preferredColorSchemeQuery.addListener === 'function') {
                preferredColorSchemeQuery.addListener(handlePreferredColorSchemeChange);
            } else {
                throw new Error('Dynamic theme: matchMedia change listener is unavailable');
            }

            const listen = window.__TAURI__?.event?.listen;
            if (typeof listen !== 'function') {
                throw new Error('Dynamic theme: Tauri theme listener is unavailable');
            }

            void listen('tauri://theme-changed', (/** @type {any} */ event) => {
                const nextTheme = event?.payload === 'dark' ? 'dark' : 'light';
                updateSystemThemeAndSync(nextTheme, 'tauri://theme-changed');
            });

            document.addEventListener('visibilitychange', () => {
                if (document.visibilityState !== 'visible') {
                    return;
                }

                const nextTheme = preferredColorSchemeQuery.matches ? 'dark' : 'light';
                updateSystemThemeAndSync(nextTheme, 'visibilitychange');
            });
        });
    });

    return { ready };
}
