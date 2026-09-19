// @ts-check

// Keep the startup restoration in index.html in sync with these names.
const STORAGE_KEY = 'tauritavern:oled_background';
const CLASS_NAME = 'tt-oled-background';

export function isOledBackgroundEnabled() {
    return document.documentElement.classList.contains(CLASS_NAME);
}

/** @param {boolean} enabled */
export function setOledBackgroundEnabled(enabled) {
    if (enabled) {
        localStorage.setItem(STORAGE_KEY, '1');
    } else {
        localStorage.removeItem(STORAGE_KEY);
    }
    document.documentElement.classList.toggle(CLASS_NAME, enabled);
}
