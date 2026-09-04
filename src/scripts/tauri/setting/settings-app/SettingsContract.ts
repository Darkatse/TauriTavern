/**
 * Boundary contract for the settings-app feature.
 *
 * This module is the single home for the narrow types shared by the Settings
 * mount, the runtime validation of the JavaScript host boundary, and the pure
 * draft functions both the view and the controller rely on. It contains no
 * state, no React and no host access.
 */

export type SettingsTranslate = (key: string) => string;

// ── Host-normalized view model (see setting-panel/settings-view-model.js) ──

export type SettingsCapabilities = {
    requestProxyAllowed: boolean;
    lanSyncAllowed: boolean;
    supportsCloseToTrayOnClose: boolean;
    supportsDataRootSelection: boolean;
};

export type ChatBackupStorageStats = {
    originalBytes: number;
    storedBytes: number;
};

export type SettingsDataRootState = {
    currentDataRoot: string;
    configuredDataRoot: string;
    migrationPending: boolean;
    migrationError: string;
};

/** The camelCase subset of setting-panel/settings-state.js that the app reads. */
export type SettingsValues = {
    panelRuntimeProfile: string;
    embeddedRuntimeProfile: string;
    chatVirtualizationEnabled: boolean;
    codeMirrorEditorEnabled: boolean;
    chatBackups: {
        automaticEnabled: boolean;
        zstdCompressionEnabled: boolean;
        maxFilesPerPrefix: number;
        maxTotalFiles: number;
        maxTotalBytes: number;
    };
    closeToTrayOnClose: boolean;
    requestProxy: {
        enabled: boolean;
        url: string;
        bypass: string[];
    };
    allowKeysExposure: boolean;
    avatarPersonaOriginalImagesEnabled: boolean;
    nativeRegexBackendEnabled: boolean;
    dynamicTheme: {
        themeEnabled: boolean;
        dayTheme: string;
        nightTheme: string;
        wallpaperEnabled: boolean;
        dayWallpaper: string;
        nightWallpaper: string;
    };
    promptCacheTtl: string;
};

export type SettingsViewModel = {
    capabilities: SettingsCapabilities;
    values: SettingsValues;
    dataRoot: SettingsDataRootState | null;
    chatBackupStorageStats?: ChatBackupStorageStats | null;
};

// ── Options provided by the popup shell ─────────────────────────────────────

export type SettingsOption = {
    value: string;
    label: string;
};

export type SettingsBackgroundOption = SettingsOption & {
    thumbnailUrl: string;
    isAnimated: boolean;
};

// ── Unsaved draft ───────────────────────────────────────────────────────────

export const CHAT_BACKUP_STORAGE_UNIT_BYTES = {
    MiB: 1024 * 1024,
    GiB: 1024 * 1024 * 1024,
} as const;

export type ChatBackupStorageUnit = keyof typeof CHAT_BACKUP_STORAGE_UNIT_BYTES;

export const CHAT_BACKUP_STORAGE_UNITS: ChatBackupStorageUnit[] = ['MiB', 'GiB'];

/**
 * The unsaved popup draft. Numeric limits stay raw edit strings so clearing a
 * field never collapses into a destructive `0`; the final conversion and
 * validation stay with setting-panel/settings-patch.js.
 */
export type SettingsDraft = {
    panelRuntimeProfile: string;
    embeddedRuntimeProfile: string;
    chatVirtualizationEnabled: boolean;
    codeMirrorEditorEnabled: boolean;
    chatBackups: {
        automaticEnabled: boolean;
        zstdCompressionEnabled: boolean;
        maxFilesPerPrefix: string;
        maxTotalFiles: string;
        maxTotalValue: string;
        maxTotalUnit: ChatBackupStorageUnit;
    };
    closeToTrayOnClose: boolean;
    requestProxy: {
        enabled: boolean;
        url: string;
        bypass: string;
    };
    allowKeysExposure: boolean;
    avatarPersonaOriginalImagesEnabled: boolean;
    nativeRegexBackendEnabled: boolean;
    dynamicTheme: {
        themeEnabled: boolean;
        dayTheme: string;
        nightTheme: string;
        wallpaperEnabled: boolean;
        dayWallpaper: string;
        nightWallpaper: string;
    };
    promptCacheTtl: string;
};

/**
 * Builds the initial draft from the normalized values. Stored dynamic theme
 * and wallpaper emptiness is preserved as-is: fallbacks are only filled when
 * the user actively enables the matching switch, so an unedited Save produces
 * no `dynamic_theme` patch.
 */
export function createSettingsDraft(values: SettingsValues): SettingsDraft {
    const maxTotalBytes = values.chatBackups.maxTotalBytes;
    const maxTotalUnit: ChatBackupStorageUnit = maxTotalBytes >= CHAT_BACKUP_STORAGE_UNIT_BYTES.GiB ? 'GiB' : 'MiB';
    const maxTotalValue = maxTotalBytes > 0
        ? maxTotalBytes / CHAT_BACKUP_STORAGE_UNIT_BYTES[maxTotalUnit]
        : maxTotalBytes;

    return {
        panelRuntimeProfile: values.panelRuntimeProfile,
        embeddedRuntimeProfile: values.embeddedRuntimeProfile,
        chatVirtualizationEnabled: values.chatVirtualizationEnabled,
        codeMirrorEditorEnabled: values.codeMirrorEditorEnabled,
        chatBackups: {
            automaticEnabled: values.chatBackups.automaticEnabled,
            zstdCompressionEnabled: values.chatBackups.zstdCompressionEnabled,
            maxFilesPerPrefix: String(values.chatBackups.maxFilesPerPrefix),
            maxTotalFiles: String(values.chatBackups.maxTotalFiles),
            maxTotalValue: String(maxTotalValue),
            maxTotalUnit,
        },
        closeToTrayOnClose: values.closeToTrayOnClose,
        requestProxy: {
            enabled: values.requestProxy.enabled,
            url: values.requestProxy.url,
            bypass: values.requestProxy.bypass.join('\n'),
        },
        allowKeysExposure: values.allowKeysExposure,
        avatarPersonaOriginalImagesEnabled: values.avatarPersonaOriginalImagesEnabled,
        nativeRegexBackendEnabled: values.nativeRegexBackendEnabled,
        dynamicTheme: {
            themeEnabled: values.dynamicTheme.themeEnabled,
            dayTheme: values.dynamicTheme.dayTheme,
            nightTheme: values.dynamicTheme.nightTheme,
            wallpaperEnabled: values.dynamicTheme.wallpaperEnabled,
            dayWallpaper: values.dynamicTheme.dayWallpaper,
            nightWallpaper: values.dynamicTheme.nightWallpaper,
        },
        promptCacheTtl: values.promptCacheTtl,
    };
}

/** A raw limit string is a real zero only when it holds an actual number. */
export function isZeroLimit(value: string): boolean {
    const trimmed = value.trim();
    return trimmed !== '' && Number(trimmed) === 0;
}

export type DynamicThemeDraft = SettingsDraft['dynamicTheme'];

// ── Host ports ──────────────────────────────────────────────────────────────

export type SettingsActions = {
    chooseDataRoot: () => Promise<string | null | undefined>;
    chooseWallpaper: (request: { currentValue: string }) => Promise<string | null | undefined>;
    showHelp: (topicId: string) => Promise<unknown>;
    manageQuickAccess: () => Promise<unknown>;
    reloadFrontend: () => Promise<unknown>;
    openFrontendLogs: () => Promise<unknown>;
    openBackendLogs: () => Promise<unknown>;
    openLlmApiLogs: () => Promise<unknown>;
    openSync: () => Promise<unknown>;
};

export type SettingsMountOptions = {
    viewModel: SettingsViewModel;
    themeOptions?: SettingsOption[];
    backgroundOptions?: SettingsBackgroundOption[];
    currentBackground?: string;
    actions: SettingsActions;
    tr: SettingsTranslate;
};

export type SettingsHandle = {
    getDraft: () => SettingsDraft;
    setChatBackupStorageStats: (stats: ChatBackupStorageStats | null) => void;
    unmount: () => void;
};

// ── Boundary validation ─────────────────────────────────────────────────────

const REQUIRED_ACTIONS = [
    'chooseDataRoot',
    'chooseWallpaper',
    'showHelp',
    'manageQuickAccess',
    'reloadFrontend',
    'openFrontendLogs',
    'openBackendLogs',
    'openLlmApiLogs',
    'openSync',
] as const;

/** Validates the parts of the JS host boundary that TypeScript cannot see. */
export function validateSettingsBoundary(
    options: unknown,
): asserts options is SettingsMountOptions {
    if (!plainObject(options)
        || !plainObject(options.viewModel)
        || !options.viewModel.capabilities
        || !options.viewModel.values) {
        throw new Error('TauriTavern settings view model is required');
    }
    if (typeof options.tr !== 'function') {
        throw new Error('TauriTavern settings translator is required');
    }
    const actions = plainObject(options.actions) ? options.actions : {};
    for (const name of REQUIRED_ACTIONS) {
        if (typeof actions[name] !== 'function') {
            throw new Error(`TauriTavern settings action is unavailable: ${name}`);
        }
    }
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
