import { useState } from 'react';

import type { SettingsTranslate } from './SettingsContract';
import { SettingRow, SettingsSection } from './SettingsComponents';

const BRIDGE_BASE_URL_KEY = 'ttPluginBridge.baseUrl';
const BRIDGE_AUTH_KEY = 'ttPluginBridge.auth';
const DEFAULT_BRIDGE_BASE_URL = 'http://127.0.0.1:8000';

function readBridgeSetting(key: string): string {
    try {
        return window.localStorage.getItem(key) ?? '';
    } catch {
        return '';
    }
}

function writeBridgeSetting(key: string, value: string): void {
    try {
        if (value) {
            window.localStorage.setItem(key, value);
        } else {
            window.localStorage.removeItem(key);
        }
    } catch {
        // Storage unavailable: the bridge route treats unreadable config as disabled.
    }
}

type SettingsBridgeSectionProps = {
    tr: SettingsTranslate;
};

/**
 * The "Server Plugin Bridge" section. Bridge settings intentionally live in
 * localStorage: the host address is a per-device deployment fact while
 * tauritavern-settings.json is synced verbatim between devices, so these
 * controls commit immediately instead of joining the draft/save flow (the
 * same immediate-commit pattern as the data root). An empty host field means
 * the default host; credentials follow SillyTavern's basicAuthUser value.
 */
export function SettingsBridgeSection({ tr }: SettingsBridgeSectionProps) {
    const [baseUrl, setBaseUrl] = useState(
        () => readBridgeSetting(BRIDGE_BASE_URL_KEY) || DEFAULT_BRIDGE_BASE_URL,
    );
    const [credentials, setCredentials] = useState(() => readBridgeSetting(BRIDGE_AUTH_KEY));

    return (
        <SettingsSection title={tr('Server Plugin Bridge')} icon="fa-plug">
            <SettingRow label={tr('Bridge Host URL')} hint={tr('Bridge Host URL hint')}>
                <input
                    type="text"
                    className="text_pole tt-settings-input"
                    value={baseUrl}
                    placeholder={DEFAULT_BRIDGE_BASE_URL}
                    spellCheck={false}
                    onChange={event => {
                        const value = event.target.value;
                        setBaseUrl(value);
                        writeBridgeSetting(BRIDGE_BASE_URL_KEY, value);
                    }}
                />
            </SettingRow>
            <SettingRow label={tr('Bridge Credentials')} hint={tr('Bridge Credentials hint')}>
                <input
                    type="password"
                    className="text_pole tt-settings-input"
                    value={credentials}
                    placeholder="user:password"
                    autoComplete="off"
                    onChange={event => {
                        const value = event.target.value;
                        setCredentials(value);
                        writeBridgeSetting(BRIDGE_AUTH_KEY, value);
                    }}
                />
            </SettingRow>
        </SettingsSection>
    );
}
