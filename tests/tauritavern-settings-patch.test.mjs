import test from 'node:test';
import assert from 'node:assert/strict';

import { createTauriTavernSettingsState } from '../src/scripts/tauri/setting/setting-panel/settings-state.js';
import { buildTauriTavernSettingsUpdate } from '../src/scripts/tauri/setting/setting-panel/settings-patch.js';

function createSettings(overrides = {}) {
    return {
        panel_runtime_profile: 'off',
        embedded_runtime_profile: 'off',
        chat_history_mode: 'windowed',
        close_to_tray_on_close: false,
        request_proxy: {
            enabled: false,
            url: '',
            bypass: [],
        },
        allow_keys_exposure: false,
        avatar_persona_original_images_enabled: false,
        native_regex_backend_enabled: true,
        dynamic_theme: {
            enabled: false,
            day_theme: 'Default',
            night_theme: 'Dark',
            wallpaper_enabled: false,
            day_wallpaper: ' Day.png',
            night_wallpaper: 'Night .png',
        },
        models: {
            claude: {
                prompt_cache_ttl: 'off',
            },
        },
        ...overrides,
    };
}

test('buildTauriTavernSettingsUpdate returns an empty patch for unchanged settings', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    assert.equal(initial.dynamicTheme.dayWallpaper, ' Day.png');
    assert.equal(initial.dynamicTheme.nightWallpaper, 'Night .png');

    const update = buildTauriTavernSettingsUpdate(initial, {
        ...initial,
        dynamicTheme: { ...initial.dynamicTheme },
        requestProxy: {
            enabled: false,
            url: '',
            bypass: '',
        },
    });

    assert.equal(update.hasChanges, false);
    assert.deepEqual(update.patch, {});
});

test('buildTauriTavernSettingsUpdate preserves minimal nested patch semantics', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    const update = buildTauriTavernSettingsUpdate(initial, {
        ...initial,
        promptCacheTtl: '5m',
        requestProxy: {
            enabled: true,
            url: ' http://127.0.0.1:7890 ',
            bypass: 'localhost, 127.0.0.1\n10.0.0.0/8',
        },
    });

    assert.equal(update.hasChanges, true);
    assert.deepEqual(update.patch, {
        models: {
            claude: {
                prompt_cache_ttl: '5m',
            },
        },
        request_proxy: {
            enabled: true,
            url: 'http://127.0.0.1:7890',
            bypass: ['localhost', '127.0.0.1', '10.0.0.0/8'],
        },
    });
});

test('buildTauriTavernSettingsUpdate persists dynamic wallpaper settings with theme settings', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    const update = buildTauriTavernSettingsUpdate(initial, {
        ...initial,
        dynamicTheme: {
            ...initial.dynamicTheme,
            wallpaperEnabled: true,
            dayWallpaper: ' Soft Morning.png',
            nightWallpaper: 'Deep Night .webp',
        },
    });

    assert.equal(update.hasChanges, true);
    assert.deepEqual(update.patch, {
        dynamic_theme: {
            enabled: false,
            day_theme: 'Default',
            night_theme: 'Dark',
            wallpaper_enabled: true,
            day_wallpaper: ' Soft Morning.png',
            night_wallpaper: 'Deep Night .webp',
        },
    });
});

test('buildTauriTavernSettingsUpdate persists embedded runtime legacy migration', () => {
    const initial = createTauriTavernSettingsState(createSettings({
        embedded_runtime_profile: 'compat',
    }), {
        nativeRegexBackendEnabled: true,
    });
    const legacyEffectiveInitial = {
        ...initial,
        configuredEmbeddedRuntimeProfile: 'auto',
        embeddedRuntimeProfile: 'compat',
    };

    const update = buildTauriTavernSettingsUpdate(legacyEffectiveInitial, {
        ...legacyEffectiveInitial,
        embeddedRuntimeProfile: 'compat',
    });

    assert.equal(update.hasChanges, true);
    assert.deepEqual(update.patch, {
        embedded_runtime_profile: 'compat',
    });
});
