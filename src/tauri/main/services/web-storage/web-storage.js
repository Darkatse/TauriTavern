// @ts-check

import { hostPlatform } from '../../../../scripts/util/host-identity.js';

/**
 * @typedef {{ key: string, bytes: number }} LocalStorageEntry
 * @typedef {{ usedBytes: number, quotaBytes: number, entries: LocalStorageEntry[] }} LocalStorageUsage
 * @typedef {{ localStorageKeys: string[], databases: string[] }} WebStorageSelection
 * @typedef {{
 *   removedKeys: number,
 *   removedBytes: number,
 *   deletedDatabases: number,
 *   failedDatabases: { name: string, message: string }[],
 * }} WebStorageCleanupReport
 */

const MiB = 1024 * 1024;
const NEARLY_FULL_RATIO = 0.9;
const SCHEDULED_CLEANUP_KEY = 'tt:webStorageCleanup';
const NON_LATIN1 = /[^\u0000-ÿ]/;

/** The origin's localStorage quota and the bytes the WebView charges for one string. */
function localStorageAccounting() {
    const platform = hostPlatform();
    switch (platform) {
        // WebKit stores Latin-1 strings with one byte per character and other strings with two.
        case 'ios':
        case 'macos':
        case 'linux':
            return {
                quotaBytes: 5 * MiB,
                charge: (/** @type {string} */ text) => text.length * (NON_LATIN1.test(text) ? 2 : 1),
            };
        // Chromium charges two bytes per UTF-16 code unit.
        case 'windows':
        case 'android':
        case 'ohos':
            return {
                quotaBytes: 10 * MiB,
                charge: (/** @type {string} */ text) => text.length * 2,
            };
        default:
            throw new Error(`Unsupported host platform for web storage: ${platform}`);
    }
}

/**
 * Entries are ordered by the space they take, largest first.
 * @returns {LocalStorageUsage}
 */
export function readLocalStorageUsage() {
    const { quotaBytes, charge } = localStorageAccounting();
    const storage = window.localStorage;
    /** @type {LocalStorageEntry[]} */
    const entries = [];
    let usedBytes = 0;
    for (let index = 0; index < storage.length; index += 1) {
        const key = /** @type {string} */ (storage.key(index));
        const bytes = charge(key) + charge(/** @type {string} */ (storage.getItem(key)));
        entries.push({ key, bytes });
        usedBytes += bytes;
    }
    entries.sort((a, b) => b.bytes - a.bytes || a.key.localeCompare(b.key));
    return { usedBytes, quotaBytes, entries };
}

/** @param {LocalStorageUsage} usage */
export function isLocalStorageNearlyFull(usage) {
    return usage.usedBytes >= usage.quotaBytes * NEARLY_FULL_RATIO;
}

/** @returns {Promise<string[]>} */
export async function listIndexedDbNames() {
    const databases = await window.indexedDB.databases();
    return databases
        .map(database => database.name)
        .filter(/** @returns {name is string} */ name => typeof name === 'string')
        .sort((a, b) => a.localeCompare(b));
}

/**
 * The selection is deleted on the next page load, before any extension can hold or rewrite it.
 * @param {WebStorageSelection} selection
 */
export function scheduleWebStorageCleanup(selection) {
    window.sessionStorage.setItem(SCHEDULED_CLEANUP_KEY, JSON.stringify({
        localStorageKeys: [...selection.localStorageKeys],
        databases: [...selection.databases],
    }));
}

/** @param {unknown} value @returns {value is string[]} */
function isStringArray(value) {
    return Array.isArray(value) && value.every(item => typeof item === 'string');
}

/** @param {string} raw @returns {WebStorageSelection} */
function parseScheduledCleanup(raw) {
    const value = JSON.parse(raw);
    if (!isStringArray(value?.localStorageKeys) || !isStringArray(value?.databases)) {
        throw new Error(`Scheduled web storage cleanup is malformed: ${raw}`);
    }
    return value;
}

/**
 * Resolves to the failure, or null once the database is gone.
 * @param {string} name
 * @returns {Promise<{ name: string, message: string } | null>}
 */
function deleteDatabase(name) {
    return new Promise(resolve => {
        const request = window.indexedDB.deleteDatabase(name);
        request.onsuccess = () => resolve(null);
        request.onerror = () => resolve({ name, message: String(request.error?.message ?? request.error) });
        request.onblocked = () => resolve({ name, message: 'The database is open in another window.' });
    });
}

/**
 * Applies the cleanup the previous page scheduled. localStorage keys are removed before this
 * returns; the promise settles once every selected database is deleted or has failed.
 * @returns {Promise<WebStorageCleanupReport | null>}
 */
export function runScheduledWebStorageCleanup() {
    const raw = window.sessionStorage.getItem(SCHEDULED_CLEANUP_KEY);
    if (raw === null) {
        return Promise.resolve(null);
    }
    window.sessionStorage.removeItem(SCHEDULED_CLEANUP_KEY);

    let selection;
    try {
        selection = parseScheduledCleanup(raw);
    } catch (error) {
        return Promise.reject(error);
    }

    const { charge } = localStorageAccounting();
    let removedKeys = 0;
    let removedBytes = 0;
    for (const key of selection.localStorageKeys) {
        const value = window.localStorage.getItem(key);
        if (value === null) {
            continue;
        }
        window.localStorage.removeItem(key);
        removedKeys += 1;
        removedBytes += charge(key) + charge(value);
    }

    return Promise.all(selection.databases.map(deleteDatabase)).then(outcomes => {
        const failedDatabases = outcomes.filter(/** @returns {outcome is { name: string, message: string }} */ outcome => outcome !== null);
        return {
            removedKeys,
            removedBytes,
            deletedDatabases: outcomes.length - failedDatabases.length,
            failedDatabases,
        };
    });
}
