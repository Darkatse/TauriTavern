// @ts-check

import { normalizeEmbeddedRuntimeProfileName } from '../../../../tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js';
import { normalizeChatHistoryModeName } from '../../../../tauri/main/services/chat-history/chat-history-mode-state.js';
import {
    arraysEqual,
    normalizeRequestProxyBypass,
} from './settings-state.js';

/**
 * @param {ReturnType<import('./settings-state.js').createTauriTavernSettingsState>} initial
 * @param {Record<string, any>} draft
 */
export function buildTauriTavernSettingsUpdate(initial, draft) {
    const nextPanelRuntimeProfile = String(draft.panelRuntimeProfile || '').trim();
    const nextEmbeddedRuntimeProfile = normalizeEmbeddedRuntimeProfileName(draft.embeddedRuntimeProfile);
    const nextChatHistoryMode = normalizeChatHistoryModeName(draft.chatHistoryMode);
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
    const hasChatHistoryModeChange = nextChatHistoryMode !== initial.chatHistoryMode;
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
        chatHistoryMode: hasChatHistoryModeChange,
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
    if (hasChatHistoryModeChange) {
        patch.chat_history_mode = nextChatHistoryMode;
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
        next: {
            panelRuntimeProfile: nextPanelRuntimeProfile,
            embeddedRuntimeProfile: nextEmbeddedRuntimeProfile,
            chatHistoryMode: nextChatHistoryMode,
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
