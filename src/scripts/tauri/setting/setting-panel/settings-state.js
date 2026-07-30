// @ts-check

import {
    normalizeEmbeddedRuntimeProfileName,
    resolveEffectiveEmbeddedRuntimeProfileName,
} from '../../../../tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js';
import { readNativeRegexBackendEnabledFromSettings } from '../../regex/native-regex-settings.js';

export const PROMPT_CACHE_TTL_VALUES = ['off', '5m', '1h'];

function requireChatBackupLimit(value, field) {
    if (!Number.isSafeInteger(value) || value < -1) {
        throw new Error(`TauriTavern settings: invalid chat backup ${field}`);
    }

    return value;
}

/**
 * @param {unknown} value
 * @returns {string[]}
 */
export function normalizeRequestProxyBypass(value) {
    const text = Array.isArray(value) ? value.join('\n') : String(value || '');

    return text
        .split(/\r?\n/)
        .flatMap((line) => line.split(','))
        .map((entry) => entry.trim())
        .filter(Boolean);
}

/**
 * @param {string[]} left
 * @param {string[]} right
 * @returns {boolean}
 */
export function arraysEqual(left, right) {
    if (left.length !== right.length) {
        return false;
    }

    for (let index = 0; index < left.length; index += 1) {
        if (left[index] !== right[index]) {
            return false;
        }
    }

    return true;
}

/**
 * @param {Record<string, any>} settings
 * @param {{ nativeRegexBackendEnabled?: boolean }} [options]
 */
export function createTauriTavernSettingsState(settings, options = {}) {
    const rawPanelRuntimeProfile = settings.panel_runtime_profile;
    const panelRuntimeProfile = typeof rawPanelRuntimeProfile === 'string' && rawPanelRuntimeProfile
        ? rawPanelRuntimeProfile
        : 'off';

    const configuredEmbeddedRuntimeProfile = normalizeEmbeddedRuntimeProfileName(settings.embedded_runtime_profile);
    const embeddedRuntimeProfile = resolveEffectiveEmbeddedRuntimeProfileName(configuredEmbeddedRuntimeProfile);
    const chatVirtualizationEnabled = settings.chat_virtualization_enabled;
    if (typeof chatVirtualizationEnabled !== 'boolean') {
        throw new Error('TauriTavern settings: chat virtualization setting missing');
    }

    const avatarPersonaOriginalImagesEnabled = settings.avatar_persona_original_images_enabled;
    if (typeof avatarPersonaOriginalImagesEnabled !== 'boolean') {
        throw new Error('TauriTavern settings: avatar/persona original images setting missing');
    }

    const dynamicTheme = settings.dynamic_theme;
    if (!dynamicTheme || typeof dynamicTheme !== 'object') {
        throw new Error('TauriTavern settings: dynamic theme settings missing');
    }

    const chatBackups = settings.chat_backups;
    if (!chatBackups || typeof chatBackups !== 'object') {
        throw new Error('TauriTavern settings: chat backup settings missing');
    }
    if (typeof chatBackups.automatic_enabled !== 'boolean') {
        throw new Error('TauriTavern settings: automatic chat backup setting missing');
    }
    if (typeof chatBackups.zstd_compression_enabled !== 'boolean') {
        throw new Error('TauriTavern settings: Zstandard chat backup setting missing');
    }

    const maxFilesPerPrefix = requireChatBackupLimit(
        chatBackups.max_files_per_prefix,
        'per-character file limit',
    );
    const maxTotalFiles = requireChatBackupLimit(chatBackups.max_total_files, 'total file limit');
    const maxTotalBytes = requireChatBackupLimit(chatBackups.max_total_bytes, 'storage limit');

    const rawPromptCacheTtl = typeof settings.models?.claude?.prompt_cache_ttl === 'string'
        ? settings.models.claude.prompt_cache_ttl
        : 'off';

    return {
        panelRuntimeProfile,
        panelRuntimeProfileSource: rawPanelRuntimeProfile,
        configuredEmbeddedRuntimeProfile,
        embeddedRuntimeProfile,
        chatVirtualizationEnabled,
        chatBackups: {
            automaticEnabled: chatBackups.automatic_enabled,
            zstdCompressionEnabled: chatBackups.zstd_compression_enabled,
            maxFilesPerPrefix,
            maxTotalFiles,
            maxTotalBytes,
        },
        closeToTrayOnClose: Boolean(settings.close_to_tray_on_close),
        requestProxy: {
            enabled: Boolean(settings.request_proxy?.enabled),
            url: typeof settings.request_proxy?.url === 'string' ? settings.request_proxy.url : '',
            bypass: Array.isArray(settings.request_proxy?.bypass) ? settings.request_proxy.bypass : [],
        },
        allowKeysExposure: Boolean(settings.allow_keys_exposure),
        avatarPersonaOriginalImagesEnabled,
        nativeRegexBackendEnabled: typeof options.nativeRegexBackendEnabled === 'boolean'
            ? options.nativeRegexBackendEnabled
            : readNativeRegexBackendEnabledFromSettings(settings),
        dynamicTheme: {
            themeEnabled: Boolean(dynamicTheme.enabled),
            dayTheme: String(dynamicTheme.day_theme || '').trim(),
            nightTheme: String(dynamicTheme.night_theme || '').trim(),
            wallpaperEnabled: Boolean(dynamicTheme.wallpaper_enabled),
            dayWallpaper: String(dynamicTheme.day_wallpaper || ''),
            nightWallpaper: String(dynamicTheme.night_wallpaper || ''),
        },
        promptCacheTtl: PROMPT_CACHE_TTL_VALUES.includes(rawPromptCacheTtl) ? rawPromptCacheTtl : 'off',
        promptCacheTtlSource: rawPromptCacheTtl,
    };
}

/**
 * @param {Record<string, any> | null} runtimePaths
 */
export function createDataRootState(runtimePaths) {
    if (!runtimePaths) {
        return null;
    }

    return {
        currentDataRoot: String(runtimePaths.data_root || '').trim(),
        configuredDataRoot: String(runtimePaths.configured_data_root || '').trim(),
        migrationPending: Boolean(runtimePaths.migration_pending),
        migrationError: String(runtimePaths.migration_error || '').trim(),
    };
}
