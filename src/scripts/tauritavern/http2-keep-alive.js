// @ts-check

import { updateTauriTavernSettings } from '../../tauri-bridge.js';
import { t } from '../i18n.js';

/** @typedef {{ enabled: boolean, interval_seconds: number }} Http2KeepAliveSettings */

/** @param {Http2KeepAliveSettings | undefined} settings */
export function initializeHttp2KeepAliveSettings(settings) {
    if (!settings) return;

    const section = document.getElementById('http2_keep_alive_settings');
    const enabled = document.getElementById('http2_keep_alive_enabled');
    const interval = document.getElementById('http2_keep_alive_interval');
    if (!section || !(enabled instanceof HTMLInputElement) || !(interval instanceof HTMLInputElement)) {
        throw new Error('HTTP/2 keepalive settings controls are missing');
    }

    let saved = settings;

    const render = () => {
        enabled.checked = saved.enabled;
        enabled.disabled = false;
        interval.value = String(saved.interval_seconds);
        interval.disabled = !saved.enabled;
    };

    /** @param {Partial<Http2KeepAliveSettings>} changes */
    const save = async (changes) => {
        enabled.disabled = true;
        interval.disabled = true;
        try {
            const updated = await updateTauriTavernSettings({ http2_keep_alive: changes });
            saved = updated.http2_keep_alive;
        } catch (error) {
            console.error('Failed to save HTTP/2 keepalive settings', error);
            window.toastr?.error?.(t`Could not save HTTP/2 keepalive settings.`);
        } finally {
            render();
        }
    };

    render();
    section.hidden = false;

    enabled.addEventListener('change', () => {
        void save({ enabled: enabled.checked });
    });
    interval.addEventListener('change', () => {
        if (interval.reportValidity()) {
            void save({ interval_seconds: interval.valueAsNumber });
        }
    });
}
