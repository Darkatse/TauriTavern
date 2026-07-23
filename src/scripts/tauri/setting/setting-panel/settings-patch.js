// @ts-check

import { normalizeEmbeddedRuntimeProfileName } from '../../../../tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js';
import { arraysEqual, normalizeRequestProxyBypass } from './settings-state.js';

const CHAT_BACKUP_STORAGE_UNIT_BYTES = {
    MiB: 1024 * 1024,
    GiB: 1024 * 1024 * 1024,
};

function normalizeChatBackupLimit(value) {
    if (
        (typeof value !== 'number' && typeof value !== 'string')
        || (typeof value === 'string' && value.trim() === '')
    ) {
        throw new Error('Chat backup limits must be -1, 0, or a positive integer.');
    }

    const normalized = Number(value);
    if (!Number.isSafeInteger(normalized) || normalized < -1) {
        throw new Error('Chat backup limits must be -1, 0, or a positive integer.');
    }

    return normalized;
}

function normalizeChatBackupStorageBytes(value, unit) {
    if (
        (typeof value !== 'number' && typeof value !== 'string')
        || (typeof value === 'string' && value.trim() === '')
        || !Object.hasOwn(CHAT_BACKUP_STORAGE_UNIT_BYTES, unit)
    ) {
        throw new Error('Chat backup storage limit must be -1, 0, or a positive number.');
    }

    const normalized = Number(value);
    if (normalized === -1 || normalized === 0) {
        return normalized;
    }
    if (!Number.isFinite(normalized) || normalized <= 0) {
        throw new Error('Chat backup storage limit must be -1, 0, or a positive number.');
    }

    const bytes = Math.round(normalized * CHAT_BACKUP_STORAGE_UNIT_BYTES[unit]);
    if (!Number.isSafeInteger(bytes) || bytes <= 0) {
        throw new Error('Chat backup limit is too large.');
    }

    return bytes;
}

function isChatBackupHistoryDisabled(settings) {
    return settings.maxFilesPerPrefix === 0
        || settings.maxTotalFiles === 0
        || settings.maxTotalBytes === 0;
}

/**
 * @param {ReturnType<import('./settings-state.js').createTauriTavernSettingsState>} initial
 * @param {Record<string, any>} draft
 */
export function buildTauriTavernSettingsUpdate(initial, draft) {
    const nextPanelRuntimeProfile = String(draft.panelRuntimeProfile || '').trim();
    const nextEmbeddedRuntimeProfile = normalizeEmbeddedRuntimeProfileName(draft.embeddedRuntimeProfile);
    const nextChatVirtualizationEnabled = draft.chatVirtualizationEnabled;
    if (typeof nextChatVirtualizationEnabled !== 'boolean') {
        throw new TypeError('Chat virtualization setting must be a boolean');
    }
    const nextChatBackupAutomaticEnabled = Boolean(draft.chatBackups?.automaticEnabled);
    const nextChatBackupZstdCompressionEnabled = Boolean(draft.chatBackups?.zstdCompressionEnabled);
    const nextChatBackupMaxFilesPerPrefix = normalizeChatBackupLimit(draft.chatBackups?.maxFilesPerPrefix);
    const nextChatBackupMaxTotalFiles = normalizeChatBackupLimit(draft.chatBackups?.maxTotalFiles);
    const nextChatBackupMaxTotalUnit = draft.chatBackups?.maxTotalUnit;
    const nextChatBackupMaxTotalValue = draft.chatBackups?.maxTotalValue;
    const nextChatBackupMaxTotalBytes = normalizeChatBackupStorageBytes(
        nextChatBackupMaxTotalValue,
        nextChatBackupMaxTotalUnit,
    );
    const nextCloseToTrayOnClose = Boolean(draft.closeToTrayOnClose);

    const nextDynamicThemeEnabled = Boolean(draft.dynamicTheme?.themeEnabled);
    const nextDynamicThemeDayTheme = String(draft.dynamicTheme?.dayTheme || '').trim();
    const nextDynamicThemeNightTheme = String(draft.dynamicTheme?.nightTheme || '').trim();
    const nextDynamicThemeWallpaperEnabled = Boolean(draft.dynamicTheme?.wallpaperEnabled);
    const nextDynamicThemeDayWallpaper = String(draft.dynamicTheme?.dayWallpaper || '');
    const nextDynamicThemeNightWallpaper = String(draft.dynamicTheme?.nightWallpaper || '');

    const nextAllowKeysExposure = Boolean(draft.allowKeysExposure);
    const nextAvatarPersonaOriginalImagesEnabled = Boolean(draft.avatarPersonaOriginalImagesEnabled);
    const nextNativeRegexBackendEnabled = Boolean(draft.nativeRegexBackendEnabled);
    const nextPromptCacheTtl = String(draft.promptCacheTtl || '').trim();

    const nextRequestProxyEnabled = Boolean(draft.requestProxy?.enabled);
    const nextRequestProxyUrl = String(draft.requestProxy?.url || '').trim();
    const nextRequestProxyBypass = normalizeRequestProxyBypass(draft.requestProxy?.bypass);

    const normalizedCurrentRequestProxyBypass = normalizeRequestProxyBypass(initial.requestProxy.bypass);
    const normalizedCurrentRequestProxyUrl = String(initial.requestProxy.url || '').trim();

    const hasPanelRuntimeChange = Boolean(nextPanelRuntimeProfile)
        && nextPanelRuntimeProfile !== initial.panelRuntimeProfileSource;
    const requiresEmbeddedRuntimeMigration =
        initial.configuredEmbeddedRuntimeProfile !== initial.embeddedRuntimeProfile;
    const hasEmbeddedRuntimeChange = Boolean(nextEmbeddedRuntimeProfile)
        && (nextEmbeddedRuntimeProfile !== initial.embeddedRuntimeProfile || requiresEmbeddedRuntimeMigration);
    const hasChatVirtualizationEnabledChange =
        nextChatVirtualizationEnabled !== initial.chatVirtualizationEnabled;
    const hasChatBackupAutomaticEnabledChange =
        nextChatBackupAutomaticEnabled !== initial.chatBackups.automaticEnabled;
    const hasChatBackupZstdCompressionEnabledChange =
        nextChatBackupZstdCompressionEnabled !== initial.chatBackups.zstdCompressionEnabled;
    const hasChatBackupMaxFilesPerPrefixChange =
        nextChatBackupMaxFilesPerPrefix !== initial.chatBackups.maxFilesPerPrefix;
    const hasChatBackupMaxTotalFilesChange =
        nextChatBackupMaxTotalFiles !== initial.chatBackups.maxTotalFiles;
    const hasChatBackupMaxTotalBytesChange =
        nextChatBackupMaxTotalBytes !== initial.chatBackups.maxTotalBytes;
    const hasChatBackupsChange = hasChatBackupAutomaticEnabledChange
        || hasChatBackupZstdCompressionEnabledChange
        || hasChatBackupMaxFilesPerPrefixChange
        || hasChatBackupMaxTotalFilesChange
        || hasChatBackupMaxTotalBytesChange;
    const hasCloseToTrayOnCloseChange = nextCloseToTrayOnClose !== initial.closeToTrayOnClose;
    const hasDynamicThemeChange = nextDynamicThemeEnabled !== initial.dynamicTheme.themeEnabled
        || nextDynamicThemeDayTheme !== initial.dynamicTheme.dayTheme
        || nextDynamicThemeNightTheme !== initial.dynamicTheme.nightTheme
        || nextDynamicThemeWallpaperEnabled !== initial.dynamicTheme.wallpaperEnabled
        || nextDynamicThemeDayWallpaper !== initial.dynamicTheme.dayWallpaper
        || nextDynamicThemeNightWallpaper !== initial.dynamicTheme.nightWallpaper;
    const hasAllowKeysExposureChange = nextAllowKeysExposure !== initial.allowKeysExposure;
    const hasAvatarPersonaOriginalImagesEnabledChange =
        nextAvatarPersonaOriginalImagesEnabled !== initial.avatarPersonaOriginalImagesEnabled;
    const hasNativeRegexBackendEnabledChange =
        nextNativeRegexBackendEnabled !== initial.nativeRegexBackendEnabled;
    const hasPromptCacheTtlChange = nextPromptCacheTtl !== initial.promptCacheTtlSource;
    const hasModelsChange = hasPromptCacheTtlChange;
    const hasRequestProxyChange = nextRequestProxyEnabled !== initial.requestProxy.enabled
        || nextRequestProxyUrl !== normalizedCurrentRequestProxyUrl
        || !arraysEqual(nextRequestProxyBypass, normalizedCurrentRequestProxyBypass);

    const changes = {
        panelRuntimeProfile: hasPanelRuntimeChange,
        embeddedRuntimeProfile: hasEmbeddedRuntimeChange,
        chatVirtualizationEnabled: hasChatVirtualizationEnabledChange,
        chatBackups: hasChatBackupsChange,
        closeToTrayOnClose: hasCloseToTrayOnCloseChange,
        dynamicTheme: hasDynamicThemeChange,
        allowKeysExposure: hasAllowKeysExposureChange,
        avatarPersonaOriginalImagesEnabled: hasAvatarPersonaOriginalImagesEnabledChange,
        nativeRegexBackendEnabled: hasNativeRegexBackendEnabledChange,
        promptCacheTtl: hasPromptCacheTtlChange,
        models: hasModelsChange,
        requestProxy: hasRequestProxyChange,
    };

    const hasChanges = Object.values(changes).some(Boolean);
    /** @type {Record<string, unknown>} */
    const patch = {};

    if (hasPanelRuntimeChange) {
        patch.panel_runtime_profile = nextPanelRuntimeProfile;
    }
    if (hasEmbeddedRuntimeChange) {
        patch.embedded_runtime_profile = nextEmbeddedRuntimeProfile;
    }
    if (hasChatVirtualizationEnabledChange) {
        patch.chat_virtualization_enabled = nextChatVirtualizationEnabled;
    }
    if (hasChatBackupsChange) {
        /** @type {Record<string, unknown>} */
        const chatBackups = {};
        if (hasChatBackupAutomaticEnabledChange) {
            chatBackups.automatic_enabled = nextChatBackupAutomaticEnabled;
        }
        if (hasChatBackupZstdCompressionEnabledChange) {
            chatBackups.zstd_compression_enabled = nextChatBackupZstdCompressionEnabled;
        }
        if (hasChatBackupMaxFilesPerPrefixChange) {
            chatBackups.max_files_per_prefix = nextChatBackupMaxFilesPerPrefix;
        }
        if (hasChatBackupMaxTotalFilesChange) {
            chatBackups.max_total_files = nextChatBackupMaxTotalFiles;
        }
        if (hasChatBackupMaxTotalBytesChange) {
            chatBackups.max_total_bytes = nextChatBackupMaxTotalBytes;
        }
        patch.chat_backups = chatBackups;
    }
    if (hasCloseToTrayOnCloseChange) {
        patch.close_to_tray_on_close = nextCloseToTrayOnClose;
    }
    if (hasDynamicThemeChange) {
        patch.dynamic_theme = {
            enabled: nextDynamicThemeEnabled,
            day_theme: nextDynamicThemeDayTheme,
            night_theme: nextDynamicThemeNightTheme,
            wallpaper_enabled: nextDynamicThemeWallpaperEnabled,
            day_wallpaper: nextDynamicThemeDayWallpaper,
            night_wallpaper: nextDynamicThemeNightWallpaper,
        };
    }
    if (hasAllowKeysExposureChange) {
        patch.allow_keys_exposure = nextAllowKeysExposure;
    }
    if (hasAvatarPersonaOriginalImagesEnabledChange) {
        patch.avatar_persona_original_images_enabled = nextAvatarPersonaOriginalImagesEnabled;
    }
    if (hasNativeRegexBackendEnabledChange) {
        patch.native_regex_backend_enabled = nextNativeRegexBackendEnabled;
    }
    if (hasModelsChange) {
        /** @type {Record<string, unknown>} */
        const claude = {};
        if (hasPromptCacheTtlChange) {
            claude.prompt_cache_ttl = nextPromptCacheTtl;
        }
        patch.models = { claude };
    }
    if (hasRequestProxyChange) {
        patch.request_proxy = {
            enabled: nextRequestProxyEnabled,
            url: nextRequestProxyUrl,
            bypass: nextRequestProxyBypass,
        };
    }

    return {
        hasChanges,
        patch,
        changes,
        requiresChatBackupPurgeConfirmation: hasChatBackupsChange
            && !isChatBackupHistoryDisabled(initial.chatBackups)
            && isChatBackupHistoryDisabled({
                maxFilesPerPrefix: nextChatBackupMaxFilesPerPrefix,
                maxTotalFiles: nextChatBackupMaxTotalFiles,
                maxTotalBytes: nextChatBackupMaxTotalBytes,
            }),
        next: {
            panelRuntimeProfile: nextPanelRuntimeProfile,
            embeddedRuntimeProfile: nextEmbeddedRuntimeProfile,
            chatVirtualizationEnabled: nextChatVirtualizationEnabled,
            chatBackups: {
                automaticEnabled: nextChatBackupAutomaticEnabled,
                zstdCompressionEnabled: nextChatBackupZstdCompressionEnabled,
                maxFilesPerPrefix: nextChatBackupMaxFilesPerPrefix,
                maxTotalFiles: nextChatBackupMaxTotalFiles,
                maxTotalBytes: nextChatBackupMaxTotalBytes,
            },
            closeToTrayOnClose: nextCloseToTrayOnClose,
            dynamicTheme: {
                themeEnabled: nextDynamicThemeEnabled,
                dayTheme: nextDynamicThemeDayTheme,
                nightTheme: nextDynamicThemeNightTheme,
                wallpaperEnabled: nextDynamicThemeWallpaperEnabled,
                dayWallpaper: nextDynamicThemeDayWallpaper,
                nightWallpaper: nextDynamicThemeNightWallpaper,
            },
            allowKeysExposure: nextAllowKeysExposure,
            avatarPersonaOriginalImagesEnabled: nextAvatarPersonaOriginalImagesEnabled,
            nativeRegexBackendEnabled: nextNativeRegexBackendEnabled,
            promptCacheTtl: nextPromptCacheTtl,
            requestProxy: {
                enabled: nextRequestProxyEnabled,
                url: nextRequestProxyUrl,
                bypass: nextRequestProxyBypass,
            },
        },
    };
}
