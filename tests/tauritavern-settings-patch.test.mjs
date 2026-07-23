import test from 'node:test';
import assert from 'node:assert/strict';

import { createTauriTavernSettingsState } from '../src/scripts/tauri/setting/setting-panel/settings-state.js';
import { buildTauriTavernSettingsUpdate } from '../src/scripts/tauri/setting/setting-panel/settings-patch.js';
import { createTauriTavernSettingsApp } from '../src/scripts/tauri/setting/settings-app/SettingsApp.js';

const MEBIBYTE_BYTES = 1024 * 1024;
const GIBIBYTE_BYTES = 1024 * MEBIBYTE_BYTES;

function createSettings(overrides = {}) {
    return {
        panel_runtime_profile: 'off',
        embedded_runtime_profile: 'off',
        chat_virtualization_enabled: false,
        chat_backups: {
            automatic_enabled: true,
            zstd_compression_enabled: false,
            max_files_per_prefix: 20,
            max_total_files: 500,
            max_total_bytes: 1024 * 1024 * 1024,
        },
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

function createDraft(initial, overrides = {}) {
    const maxTotalBytes = initial.chatBackups.maxTotalBytes;
    const maxTotalUnit = maxTotalBytes >= GIBIBYTE_BYTES ? 'GiB' : 'MiB';

    return {
        ...initial,
        ...overrides,
        chatBackups: {
            automaticEnabled: initial.chatBackups.automaticEnabled,
            zstdCompressionEnabled: initial.chatBackups.zstdCompressionEnabled,
            maxFilesPerPrefix: initial.chatBackups.maxFilesPerPrefix,
            maxTotalFiles: initial.chatBackups.maxTotalFiles,
            maxTotalValue: maxTotalBytes > 0
                ? maxTotalBytes / (maxTotalUnit === 'GiB' ? GIBIBYTE_BYTES : MEBIBYTE_BYTES)
                : maxTotalBytes,
            maxTotalUnit,
            ...overrides.chatBackups,
        },
    };
}

test('buildTauriTavernSettingsUpdate returns an empty patch for unchanged settings', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    assert.equal(initial.dynamicTheme.dayWallpaper, ' Day.png');
    assert.equal(initial.dynamicTheme.nightWallpaper, 'Night .png');

    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        dynamicTheme: { ...initial.dynamicTheme },
        requestProxy: {
            enabled: false,
            url: '',
            bypass: '',
        },
    }));

    assert.equal(update.hasChanges, false);
    assert.deepEqual(update.patch, {});
});

test('buildTauriTavernSettingsUpdate persists the chat virtualization switch', () => {
    const initial = createTauriTavernSettingsState(createSettings());
    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        chatVirtualizationEnabled: true,
    }));

    assert.equal(update.hasChanges, true);
    assert.deepEqual(update.patch, { chat_virtualization_enabled: true });
    assert.equal(update.changes.chatVirtualizationEnabled, true);
});

test('createTauriTavernSettingsState requires the canonical chat virtualization switch', () => {
    const settings = createSettings();
    delete settings.chat_virtualization_enabled;

    assert.throws(
        () => createTauriTavernSettingsState(settings),
        /chat virtualization setting missing/,
    );
});

test('createTauriTavernSettingsState requires the canonical Zstandard backup switch', () => {
    const settings = createSettings();
    delete settings.chat_backups.zstd_compression_enabled;

    assert.throws(
        () => createTauriTavernSettingsState(settings),
        /Zstandard chat backup setting missing/,
    );
});

test('buildTauriTavernSettingsUpdate preserves minimal nested patch semantics', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        promptCacheTtl: '5m',
        requestProxy: {
            enabled: true,
            url: ' http://127.0.0.1:7890 ',
            bypass: 'localhost, 127.0.0.1\n10.0.0.0/8',
        },
    }));

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

    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        dynamicTheme: {
            ...initial.dynamicTheme,
            wallpaperEnabled: true,
            dayWallpaper: ' Soft Morning.png',
            nightWallpaper: 'Deep Night .webp',
        },
    }));

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

    const update = buildTauriTavernSettingsUpdate(legacyEffectiveInitial, createDraft(legacyEffectiveInitial, {
        embeddedRuntimeProfile: 'compat',
    }));

    assert.equal(update.hasChanges, true);
    assert.deepEqual(update.patch, {
        embedded_runtime_profile: 'compat',
    });
});

test('buildTauriTavernSettingsUpdate preserves minimal chat backup patch semantics', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        chatBackups: {
            automaticEnabled: false,
            maxTotalValue: 1.5,
            maxTotalUnit: 'GiB',
        },
    }));

    assert.equal(update.hasChanges, true);
    assert.equal(update.changes.chatBackups, true);
    assert.equal(update.requiresChatBackupPurgeConfirmation, false);
    assert.deepEqual(update.patch, {
        chat_backups: {
            automatic_enabled: false,
            max_total_bytes: 1536 * 1024 * 1024,
        },
    });
});

test('buildTauriTavernSettingsUpdate persists only the Zstandard backup switch', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        chatBackups: {
            zstdCompressionEnabled: true,
        },
    }));

    assert.equal(update.hasChanges, true);
    assert.equal(update.changes.chatBackups, true);
    assert.equal(update.requiresChatBackupPurgeConfirmation, false);
    assert.equal(update.next.chatBackups.zstdCompressionEnabled, true);
    assert.deepEqual(update.patch, {
        chat_backups: {
            zstd_compression_enabled: true,
        },
    });
});

test('unchanged storage display does not rewrite a non-MiB-aligned byte limit', () => {
    const initial = createTauriTavernSettingsState(createSettings({
        chat_backups: {
            automatic_enabled: true,
            zstd_compression_enabled: false,
            max_files_per_prefix: 20,
            max_total_files: 500,
            max_total_bytes: 1024 * 1024 + 1,
        },
    }), {
        nativeRegexBackendEnabled: true,
    });

    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial));

    assert.equal(update.hasChanges, false);
    assert.deepEqual(update.patch, {});
    assert.equal(update.next.chatBackups.maxTotalBytes, 1024 * 1024 + 1);
});

test('buildTauriTavernSettingsUpdate flags the destructive zero limit transition', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        chatBackups: {
            maxTotalFiles: 0,
        },
    }));

    assert.equal(update.requiresChatBackupPurgeConfirmation, true);
    assert.deepEqual(update.patch, {
        chat_backups: {
            max_total_files: 0,
        },
    });
});

test('buildTauriTavernSettingsUpdate accepts unlimited chat backup sentinels', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    const update = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        chatBackups: {
            automaticEnabled: true,
            maxFilesPerPrefix: -1,
            maxTotalFiles: -1,
            maxTotalValue: -1,
            maxTotalUnit: 'GiB',
        },
    }));

    assert.equal(update.requiresChatBackupPurgeConfirmation, false);
    assert.deepEqual(update.patch, {
        chat_backups: {
            max_files_per_prefix: -1,
            max_total_files: -1,
            max_total_bytes: -1,
        },
    });
});

test('buildTauriTavernSettingsUpdate rejects invalid chat backup limits', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    assert.throws(
        () => buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
            chatBackups: {
                maxFilesPerPrefix: -2,
            },
        })),
        /must be -1, 0, or a positive integer/,
    );
    assert.throws(
        () => buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
            chatBackups: {
                maxTotalValue: '',
                maxTotalUnit: 'GiB',
            },
        })),
        /must be -1, 0, or a positive number/,
    );
});

test('MiB and GiB chat backup inputs save the same byte limit', () => {
    const initial = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });

    const fromMiB = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        chatBackups: {
            maxTotalValue: 1536,
            maxTotalUnit: 'MiB',
        },
    }));
    const fromGiB = buildTauriTavernSettingsUpdate(initial, createDraft(initial, {
        chatBackups: {
            maxTotalValue: 1.5,
            maxTotalUnit: 'GiB',
        },
    }));

    assert.equal(fromMiB.patch.chat_backups.max_total_bytes, 1536 * 1024 * 1024);
    assert.deepEqual(fromGiB.patch, fromMiB.patch);
});

test('chat backup storage unit selector converts the displayed value without changing bytes', () => {
    const values = createTauriTavernSettingsState(createSettings(), {
        nativeRegexBackendEnabled: true,
    });
    const component = createTauriTavernSettingsApp({
        viewModel: {
            capabilities: {},
            dataRoot: null,
            values,
        },
        actions: {},
        tr: (key) => key,
    });
    const state = component.data();

    assert.equal(state.draft.chatBackups.maxTotalUnit, 'GiB');
    assert.equal(state.draft.chatBackups.maxTotalValue, 1);

    component.methods.setChatBackupStorageUnit.call(state, 'MiB');
    assert.equal(state.draft.chatBackups.maxTotalUnit, 'MiB');
    assert.equal(state.draft.chatBackups.maxTotalValue, 1024);
    assert.deepEqual(
        buildTauriTavernSettingsUpdate(
            values,
            component.methods.getDraft.call(state),
        ).patch,
        {},
    );

    component.methods.setChatBackupStorageUnit.call(state, 'GiB');
    assert.equal(state.draft.chatBackups.maxTotalUnit, 'GiB');
    assert.equal(state.draft.chatBackups.maxTotalValue, 1);
});

test('zstd backup hint updates when aggregate storage stats arrive later', () => {
    const settings = createSettings();
    settings.chat_backups.zstd_compression_enabled = true;
    const values = createTauriTavernSettingsState(settings);
    const component = createTauriTavernSettingsApp({
        viewModel: {
            capabilities: {},
            dataRoot: null,
            values,
        },
        actions: {},
        tr: (key) => key,
    });
    const state = component.data();
    const context = { ...state, tr: (key) => key };

    assert.equal(component.computed.zstdCompressionHint.call(context).saved, '');
    context.chatBackupStorageStats = {
        originalBytes: GIBIBYTE_BYTES,
        storedBytes: 256 * MEBIBYTE_BYTES,
    };
    const compressedHint = component.computed.zstdCompressionHint.call(context);
    assert.match(compressedHint.summary, /Saves substantial space/);
    assert.match(compressedHint.before, /25%/);
    assert.equal(compressedHint.saved, '768.0 MB');
    assert.equal(compressedHint.after, '.');

    context.draft.chatBackups.zstdCompressionEnabled = false;
    assert.deepEqual(
        component.computed.zstdCompressionHint.call(context),
        {
            summary: 'Saves substantial space, but SillyTavern cannot read this format.',
            before: '',
            saved: '',
            after: '',
        },
    );
});
