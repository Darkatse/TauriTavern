// Core Tauri bridge for frontend modules.

export const isTauriEnv = typeof window !== 'undefined' && window.__TAURI_INTERNALS__ !== undefined;

function getTauri() {
    if (typeof window === 'undefined') {
        return null;
    }

    return window.__TAURI__ || null;
}

export const invoke = isTauriEnv
    ? (...args) => {
        const fn = getTauri()?.core?.invoke;
        if (typeof fn !== 'function') {
            throw new Error('Tauri invoke is unavailable');
        }
        return fn(...args);
    }
    : null;

export const listen = isTauriEnv
    ? (...args) => {
        const fn = getTauri()?.event?.listen;
        if (typeof fn !== 'function') {
            throw new Error('Tauri listen is unavailable');
        }
        return fn(...args);
    }
    : null;

export const convertFileSrc = isTauriEnv
    ? (path, protocol = 'asset') => {
        const fn = getTauri()?.core?.convertFileSrc;
        if (typeof fn !== 'function') {
            throw new Error('Tauri convertFileSrc is unavailable');
        }
        return fn(path, protocol);
    }
    : null;

export function isTauri() {
    return isTauriEnv;
}

export async function initializeBridge() {
    if (!isTauriEnv || !invoke) {
        return false;
    }

    try {
        return await invoke('is_ready');
    } catch (error) {
        console.error('Failed to initialize Tauri bridge:', error);
        return false;
    }
}

export async function getCsrfToken() {
    return 'tauri-dummy-token';
}

export async function initializeApp() {
    return initializeBridge();
}

export async function getVersion() {
    if (!isTauriEnv || !invoke) {
        const response = await fetch('/version');
        return response.json();
    }

    return invoke('get_version');
}

export async function getClientVersion() {
    if (!isTauriEnv || !invoke) {
        const response = await fetch('/version');
        return response.json();
    }

    try {
        return await invoke('get_client_version');
    } catch (error) {
        console.error('Error getting client version from Tauri backend:', error);
        const version = await invoke('get_version');
        return {
            agent: `TauriTavern/${version}`,
            pkgVersion: version,
            gitRevision: 'tauri',
            gitBranch: 'main',
        };
    }
}

export function getAssetUrl(path) {
    if (!isTauriEnv || !convertFileSrc || !path) {
        return path;
    }

    return convertFileSrc(path, 'asset');
}
