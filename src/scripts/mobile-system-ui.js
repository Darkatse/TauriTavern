import { isMobile } from './RossAscends-mods.js';

const ANDROID_SYSTEM_UI_BRIDGE_NAME = 'TauriTavernAndroidSystemUiBridge';

function getAndroidSystemUiBridge() {
    if (!isMobile()) {
        return null;
    }

    return window[ANDROID_SYSTEM_UI_BRIDGE_NAME] || null;
}

export function isMobileImmersiveFullscreenSupported() {
    const bridge = getAndroidSystemUiBridge();
    return typeof bridge?.setImmersiveFullscreenEnabled === 'function';
}

export function setMobileImmersiveFullscreenEnabled(enabled) {
    const bridge = getAndroidSystemUiBridge();
    if (!bridge || typeof bridge.setImmersiveFullscreenEnabled !== 'function') {
        return false;
    }

    bridge.setImmersiveFullscreenEnabled(Boolean(enabled));
    return true;
}

export function getMobileImmersiveFullscreenEnabled() {
    const bridge = getAndroidSystemUiBridge();
    if (!bridge || typeof bridge.isImmersiveFullscreenEnabled !== 'function') {
        return null;
    }

    return Boolean(bridge.isImmersiveFullscreenEnabled());
}
