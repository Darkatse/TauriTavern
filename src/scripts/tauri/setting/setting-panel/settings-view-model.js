// @ts-check

import { hostPlatform, isDesktopHost } from '../../../util/host-identity.js';
import {
    getChatBackupStorageStats,
    getRuntimePaths,
    getTauriTavernSettings,
} from '../../../../tauri-bridge.js';
import { getActiveIosPolicyCapabilities } from '../../../tauritavern/ios-policy.js';
import { isOledBackgroundEnabled } from '../oled-background.js';
import { createDataRootState, createTauriTavernSettingsState } from './settings-state.js';

export function resolveTauriTavernSettingsCapabilities() {
    const iosCaps = getActiveIosPolicyCapabilities();
    const supportsDataRootSelection = isDesktopHost();

    return {
        requestProxyAllowed: iosCaps?.network?.request_proxy !== false,
        lanSyncAllowed: iosCaps?.sync?.lan !== false,
        supportsCloseToTrayOnClose: hostPlatform() === 'windows',
        supportsDataRootSelection,
    };
}

function normalizeChatBackupStorageStats(stats) {
    if (stats === null) {
        return null;
    }

    const originalBytes = Number(stats?.original_bytes);
    const storedBytes = Number(stats?.stored_bytes);
    if (
        !Number.isSafeInteger(originalBytes)
        || originalBytes < 0
        || !Number.isSafeInteger(storedBytes)
        || storedBytes < 0
    ) {
        throw new Error('TauriTavern settings: invalid chat backup storage stats');
    }

    return { originalBytes, storedBytes };
}

export async function loadChatBackupStorageStats() {
    return normalizeChatBackupStorageStats(await getChatBackupStorageStats());
}

export async function loadTauriTavernSettingsViewModel() {
    const settings = await getTauriTavernSettings();
    const capabilities = resolveTauriTavernSettingsCapabilities();
    const { supportsDataRootSelection } = capabilities;
    const runtimePaths = supportsDataRootSelection ? await getRuntimePaths() : null;

    return {
        capabilities,
        dataRoot: createDataRootState(runtimePaths),
        values: {
            ...createTauriTavernSettingsState(settings),
            oledBackgroundEnabled: isOledBackgroundEnabled(),
        },
    };
}
