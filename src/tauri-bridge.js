// Core Tauri bridge for frontend modules.

import { SILLYTAVERN_COMPAT_VERSION } from './compat-version.js';

export const isTauriEnv = typeof window !== 'undefined'
    && (window.__TAURI_INTERNALS__ !== undefined || typeof window.__TAURI__?.core?.invoke === 'function');

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

export const invoke = isTauriEnv
    ? (...args) => {
        const fn = getTauri()?.core?.invoke;
        if (typeof fn !== 'function') {
            throw new Error('Tauri invoke is unavailable');
        }

        if (args.length === 2 && isPlainObject(args[1])) {
            return fn(args[0], withTauriArgumentAliases(args[1]));
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
