// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { getTauriTavernSettings } from '../../../../tauri-bridge.js';
import {
    applySillyTavernGlobalBackground,
    applySillyTavernTheme,
    assertSillyTavernGlobalBackgroundAvailable,
    assertSillyTavernThemeAvailable,
} from '../../adapters/st/appearance.js';
import { DYNAMIC_THEME_CHANGED_EVENT } from './constants.js';

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

    const themeEnabled = Boolean(settings.enabled);
    const dayTheme = String(settings.day_theme || '').trim();
    const nightTheme = String(settings.night_theme || '').trim();
    const wallpaperEnabled = Boolean(settings.wallpaper_enabled);
    const dayWallpaper = String(settings.day_wallpaper || '');
    const nightWallpaper = String(settings.night_wallpaper || '');

    return { themeEnabled, dayTheme, nightTheme, wallpaperEnabled, dayWallpaper, nightWallpaper };
}

export function installDynamicTheme() {
    const ready = getTauriTavernSettings().then((settings) => {
        let dynamicTheme = normalizeDynamicThemeSettings(settings.dynamic_theme);
        const preferredColorSchemeQuery = getPreferredColorSchemeQuery();
        let systemTheme = preferredColorSchemeQuery?.matches ? 'dark' : readSystemThemeFromMedia();
        let hasPendingVisibilitySync = false;

        /** @param {string} reason */
        const syncNow = async (reason) => {
            if (!dynamicTheme.themeEnabled && !dynamicTheme.wallpaperEnabled) {
                return;
            }

            /** @type {{ theme?: string, wallpaper?: string }} */
            const applied = {};
            let targetTheme = '';
            let targetWallpaper = '';

            if (dynamicTheme.themeEnabled) {
                targetTheme = systemTheme === 'dark' ? dynamicTheme.nightTheme : dynamicTheme.dayTheme;
                if (!targetTheme) {
                    throw new Error('Dynamic theme is enabled but the target theme is empty');
                }
            }

            if (dynamicTheme.wallpaperEnabled) {
                targetWallpaper = systemTheme === 'dark' ? dynamicTheme.nightWallpaper : dynamicTheme.dayWallpaper;
                if (!targetWallpaper) {
                    throw new Error('Dynamic wallpaper is enabled but the target wallpaper is empty');
                }
            }

            if (dynamicTheme.themeEnabled) {
                assertSillyTavernThemeAvailable(targetTheme);
            }
            if (dynamicTheme.wallpaperEnabled) {
                await assertSillyTavernGlobalBackgroundAvailable(targetWallpaper);
            }

            if (dynamicTheme.themeEnabled) {
                applySillyTavernTheme(targetTheme);
                applied.theme = targetTheme;
            }

            if (dynamicTheme.wallpaperEnabled) {
                await applySillyTavernGlobalBackground(targetWallpaper);
                applied.wallpaper = targetWallpaper;
            }

            console.debug('Dynamic appearance applied', { reason, systemTheme, ...applied });
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
                hasPendingVisibilitySync = true;
                return;
            }

            hasPendingVisibilitySync = false;
            void Promise.resolve()
                .then(() => syncNow(reason))
                .catch((error) => {
                    console.error('Dynamic appearance sync failed after system theme change', error);
                });
        };

        /** @param {Event} event */
        const handleConfigChanged = (event) => {
            dynamicTheme = normalizeDynamicThemeSettings(/** @type {any} */ (event).detail);
            void Promise.resolve()
                .then(() => syncNow('config-changed'))
                .catch((error) => {
                    console.error('Dynamic appearance sync failed after config change', error);
                });
        };

        window.addEventListener(DYNAMIC_THEME_CHANGED_EVENT, handleConfigChanged);

        eventSource.on(event_types.APP_READY, () => {
            void Promise.resolve()
                .then(() => syncNow('startup'))
                .catch((error) => {
                    console.error('Dynamic appearance initial sync failed', error);
                });

            if (!preferredColorSchemeQuery) {
                throw new Error('Dynamic appearance: matchMedia is unavailable');
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
                throw new Error('Dynamic appearance: matchMedia change listener is unavailable');
            }

            const listen = window.__TAURI__?.event?.listen;
            if (typeof listen !== 'function') {
                throw new Error('Dynamic appearance: Tauri theme listener is unavailable');
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
                if (nextTheme === systemTheme && hasPendingVisibilitySync) {
                    hasPendingVisibilitySync = false;
                    void Promise.resolve()
                        .then(() => syncNow('visibilitychange'))
                        .catch((error) => {
                            console.error('Dynamic appearance sync failed after visibility change', error);
                        });
                    return;
                }

                updateSystemThemeAndSync(nextTheme, 'visibilitychange');
            });
        });
    });

    return { ready };
}
