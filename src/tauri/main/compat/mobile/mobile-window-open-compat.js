import { openExternalUrl } from '../../../../tauri-bridge.js';

const COMPAT_KEY = '__TAURITAVERN_MOBILE_WINDOW_OPEN_COMPAT__';
const EXTERNAL_PROTOCOLS = new Set(['http:', 'https:', 'mailto:', 'tel:']);

function resolveExternalTarget(target) {
    const value = String(target instanceof URL ? target.href : target ?? '').trim();
    if (!value) {
        return null;
    }

    try {
        const url = new URL(value, window.location.href);
        if (!EXTERNAL_PROTOCOLS.has(url.protocol)) {
            return null;
        }

        if ((url.protocol === 'http:' || url.protocol === 'https:')
            && url.origin === window.location.origin) {
            return null;
        }

        return url;
    } catch {
        return null;
    }
}

export function installMobileWindowOpenCompat() {
    if (window[COMPAT_KEY]) {
        return;
    }
    window[COMPAT_KEY] = true;

    if (typeof window.open !== 'function') {
        return;
    }

    const originalOpen = window.open.bind(window);
    window.open = function mobileWindowOpen(url, target, features, ...rest) {
        const externalTarget = resolveExternalTarget(url);
        if (externalTarget) {
            void openExternalUrl(externalTarget).catch((error) => {
                console.error('Failed to open external URL:', error);
            });
            return null;
        }

        return originalOpen(url, target, features, ...rest);
    };
}
