// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';
import { getTauriTavernSettings, updateTauriTavernSettings } from '../../../../tauri-bridge.js';
import { installChatEmbeddedRuntimeAdapters } from '../../adapters/embedded-runtime/chat-embedded-runtime-adapter.js';
import { createEmbeddedRuntimeService } from './embedded-runtime-service.js';
import {
    clearLegacyEmbeddedRuntimeProfileName,
    EMBEDDED_RUNTIME_PROFILE_OFF,
    normalizeEmbeddedRuntimeProfileName,
    resolveEffectiveEmbeddedRuntimeProfileName,
    setEmbeddedRuntimeBootstrapProfileName,
} from './embedded-runtime-profile-state.js';

export function installEmbeddedRuntime() {
    const ready = getTauriTavernSettings().then(async (settings) => {
        const configuredProfileName = normalizeEmbeddedRuntimeProfileName(settings.embedded_runtime_profile);
        const effectiveProfileName = resolveEffectiveEmbeddedRuntimeProfileName(configuredProfileName);

        let finalProfileName = configuredProfileName;
        if (effectiveProfileName !== configuredProfileName) {
            const updatedSettings = await updateTauriTavernSettings({
                embedded_runtime_profile: effectiveProfileName,
            });
            finalProfileName = normalizeEmbeddedRuntimeProfileName(updatedSettings.embedded_runtime_profile);
        }

        setEmbeddedRuntimeBootstrapProfileName(finalProfileName);
        clearLegacyEmbeddedRuntimeProfileName();

        if (finalProfileName === EMBEDDED_RUNTIME_PROFILE_OFF) {
            return null;
        }

        const service = createEmbeddedRuntimeService({ profileName: finalProfileName });

        eventSource.on(event_types.APP_READY, () => {
            installChatEmbeddedRuntimeAdapters({ manager: service.manager });
        });

        return service;
    });

    return { ready };
}
