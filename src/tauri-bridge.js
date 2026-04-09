// Core Tauri bridge for frontend modules.

import { SILLYTAVERN_COMPAT_VERSION } from './compat-version.js';

function detectTauriEnv() {
    if (typeof window === 'undefined') {
        return false;
    }

    // __TAURI_RUNNING__ is set in init.js before dynamic imports to avoid
    // startup races where mobile bridge internals are injected a bit later.
    return window.__TAURI_RUNNING__ === true
        || window.__TAURI_INTERNALS__ !== undefined
        || typeof window.__TAURI__?.core?.invoke === 'function';
}

export const isTauriEnv = detectTauriEnv();

function getTauri() {
    if (typeof window === 'undefined') {
        return null;
    }

    return window.__TAURI__ || null;
}

function getInvokeFn() {
    const fn = getTauri()?.core?.invoke;
    return typeof fn === 'function' ? fn : null;
}

function isPlainObject(value) {
    return Object.prototype.toString.call(value) === '[object Object]';
}

function withTauriArgumentAliases(args) {
    if (!isPlainObject(args)) {
        return args;
    }

    const aliased = { ...args };
    for (const [key, value] of Object.entries(args)) {
        if (!key.includes('_')) {
            continue;
        }

        const camelCaseKey = key.replace(/_+([a-zA-Z0-9])/g, (_, char) => char.toUpperCase());
        if (!Object.prototype.hasOwnProperty.call(aliased, camelCaseKey)) {
            aliased[camelCaseKey] = value;
        }
    }

    return aliased;
}

export const invoke = (...args) => {
    const fn = getTauri()?.core?.invoke;
    if (typeof fn !== 'function') {
        throw new Error('Tauri invoke is unavailable');
    }

    if (args.length === 2 && isPlainObject(args[1])) {
        return fn(args[0], withTauriArgumentAliases(args[1]));
    }

    return fn(...args);
};

function getHostSafeInvoke() {
    if (typeof window === 'undefined') {
        return null;
    }

    const fn = window.__TAURITAVERN__?.invoke?.safeInvoke;
    return typeof fn === 'function' ? fn : null;
}

async function invokeWithHostNormalization(command, args) {
    const safeInvoke = getHostSafeInvoke();
    if (safeInvoke) {
        return safeInvoke(command, args);
    }

    return args === undefined ? invoke(command) : invoke(command, args);
}

export const listen = (...args) => {
    const fn = getTauri()?.event?.listen;
    if (typeof fn !== 'function') {
        throw new Error('Tauri listen is unavailable');
    }
    return fn(...args);
};

export function createChannel(onmessage) {
    const Channel = getTauri()?.core?.Channel;
    if (typeof Channel !== 'function') {
        throw new Error('Tauri Channel is unavailable');
    }

    return new Channel(onmessage);
}

export const convertFileSrc = (path, protocol = 'asset') => {
    const fn = getTauri()?.core?.convertFileSrc;
    if (typeof fn !== 'function') {
        throw new Error('Tauri convertFileSrc is unavailable');
    }
    return fn(path, protocol);
};

export function isTauri() {
    return detectTauriEnv();
}

export async function initializeBridge() {
    const invokeFn = getInvokeFn();
    if (!invokeFn) {
        return false;
    }

    try {
        return await invokeFn('is_ready');
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
    const invokeFn = getInvokeFn();
    if (!invokeFn) {
        const response = await fetch('/version');
        return response.json();
    }

    return invokeFn('get_version');
}

export async function getClientVersion() {
    const invokeFn = getInvokeFn();
    if (!invokeFn) {
        const response = await fetch('/version');
        return response.json();
    }

    try {
        return await invokeFn('get_client_version');
    } catch (error) {
        console.error('Error getting client version from Tauri backend:', error);
        const version = await invokeFn('get_version');
        return {
            agent: `SillyTavern:${SILLYTAVERN_COMPAT_VERSION}:TauriTavern`,
            pkgVersion: SILLYTAVERN_COMPAT_VERSION,
            tauriVersion: version,
            gitRevision: null,
            gitBranch: null,
        };
    }
}

export async function checkForUpdate() {
    return invokeWithHostNormalization('check_for_update');
}

export async function getTauriTavernSettings() {
    const invokeFn = getInvokeFn();
    if (!invokeFn) {
        throw new Error('Tauri invoke is unavailable');
    }

    return invokeFn('get_tauritavern_settings');
}

export async function updateTauriTavernSettings(dto) {
    const invokeFn = getInvokeFn();
    if (!invokeFn) {
        throw new Error('Tauri invoke is unavailable');
    }

    return invokeFn('update_tauritavern_settings', { dto });
}

export async function getRuntimePaths() {
    const invokeFn = getInvokeFn();
    if (!invokeFn) {
        throw new Error('Tauri invoke is unavailable');
    }

    return invokeFn('get_runtime_paths');
}

export async function setDataRoot(dataRoot) {
    return invokeWithHostNormalization('set_data_root', { data_root: dataRoot });
}

export async function openDialog(options = {}) {
    const invokeFn = getInvokeFn();
    if (!invokeFn) {
        throw new Error('Tauri invoke is unavailable');
    }

    if (options && typeof options === 'object') {
        Object.freeze(options);
    }

    return invokeWithHostNormalization('plugin:dialog|open', { options });
}

function normalizeExternalUrl(url) {
    const value = String(url instanceof URL ? url.href : url ?? '').trim();
    if (!value) {
        throw new Error('External URL is required');
    }

    try {
        return new URL(value, window.location.href).toString();
    } catch {
        throw new Error(`Invalid external URL: ${value}`);
    }
}

export async function openExternalUrl(url, openWith) {
    const href = normalizeExternalUrl(url);
    const invokeFn = getInvokeFn();

    if (invokeFn) {
        return invokeFn('plugin:opener|open_url', {
            url: href,
            with: openWith,
        });
    }

    const openedWindow = typeof window.open === 'function'
        ? window.open(href, '_blank', 'noopener,noreferrer')
        : null;

    if (openedWindow) {
        return;
    }

    if (typeof window.location?.assign === 'function') {
        window.location.assign(href);
        return;
    }

    throw new Error('Unable to open external URL');
}

export function getAssetUrl(path) {
    if (!isTauriEnv || !convertFileSrc || !path) {
        return path;
    }

    try {
        return convertFileSrc(path, 'asset');
    } catch {
        return path;
    }
}
