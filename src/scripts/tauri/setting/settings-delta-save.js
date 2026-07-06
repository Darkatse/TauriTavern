// @ts-check

import { isTauri } from '../../../tauri-bridge.js';

export const SETTINGS_HASH_ALGORITHM = 'tt-user-settings-stable-sha256-v1';

const MAX_PATCH_OPS = 256;
const MAX_PATCH_BYTES = 512 * 1024;
const MAX_PATCH_TO_FULL_RATIO = 0.6;
const SETTINGS_HASH_PATTERN = /^[0-9a-f]{64}$/;

/**
 * @typedef {{ op: string, path: string[], value?: any }} SettingsPatchOp
 * @typedef {{ body: string, value: any }} PreparedSettingsPayload
 * @typedef {{ hash_algorithm: string, settings_hash: string }} SettingsRevision
 * @typedef {{ value: any, revision: SettingsRevision }} SettingsSaveBaseline
 */

/** @type {SettingsSaveBaseline | null} */
let settingsBaseline = null;

export class SettingsPatchConflictError extends Error {
    /** @param {string} message */
    constructor(message) {
        super(message);
        this.name = 'SettingsPatchConflictError';
    }
}

/** @param {unknown} error */
export function isSettingsPatchConflictError(error) {
    return error instanceof SettingsPatchConflictError;
}

/** @param {any} value */
function isJsonObject(value) {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/** @param {unknown} revision */
function normalizeSettingsRevision(revision) {
    if (!revision || typeof revision !== 'object') {
        return null;
    }

    const candidate = /** @type {{ hash_algorithm?: unknown, settings_hash?: unknown }} */ (revision);
    if (
        candidate.hash_algorithm !== SETTINGS_HASH_ALGORITHM
        || typeof candidate.settings_hash !== 'string'
        || !SETTINGS_HASH_PATTERN.test(candidate.settings_hash)
    ) {
        return null;
    }

    return {
        hash_algorithm: candidate.hash_algorithm,
        settings_hash: candidate.settings_hash,
    };
}

/**
 * @param {any} payload
 * @returns {PreparedSettingsPayload}
 */
export function prepareSettingsSavePayload(payload) {
    const body = JSON.stringify(payload);
    if (typeof body !== 'string') {
        throw new Error('Settings payload is not JSON serializable');
    }

    return {
        body,
        value: JSON.parse(body),
    };
}

/**
 * @param {any} settings
 * @param {unknown} revision
 */
export function captureSettingsSaveBaseline(settings, revision) {
    const normalizedRevision = normalizeSettingsRevision(revision);
    if (!normalizedRevision) {
        clearSettingsSaveBaseline();
        return;
    }

    settingsBaseline = {
        value: prepareSettingsSavePayload(settings).value,
        revision: normalizedRevision,
    };
}

export function clearSettingsSaveBaseline() {
    settingsBaseline = null;
}

/**
 * @param {SettingsPatchOp[]} ops
 * @param {SettingsPatchOp} op
 */
function pushPatchOp(ops, op) {
    ops.push(op);
    return ops.length <= MAX_PATCH_OPS;
}

/**
 * @param {any} left
 * @param {any} right
 */
function sameJsonValue(left, right) {
    return Object.is(left, right) || JSON.stringify(left) === JSON.stringify(right);
}

/**
 * @param {any} base
 * @param {any} next
 * @param {string[]} path
 * @param {SettingsPatchOp[]} ops
 */
function buildPatchOps(base, next, path, ops) {
    if (Array.isArray(base) || Array.isArray(next) || !isJsonObject(base) || !isJsonObject(next)) {
        if (sameJsonValue(base, next)) {
            return true;
        }
        return pushPatchOp(ops, { op: 'set', path, value: next });
    }

    const keys = new Set([...Object.keys(base), ...Object.keys(next)]);
    for (const key of [...keys].sort()) {
        const hasBase = Object.prototype.hasOwnProperty.call(base, key);
        const hasNext = Object.prototype.hasOwnProperty.call(next, key);
        const childPath = [...path, key];

        if (!hasNext) {
            if (!pushPatchOp(ops, { op: 'delete', path: childPath })) {
                return false;
            }
            continue;
        }

        if (!hasBase) {
            if (!pushPatchOp(ops, { op: 'set', path: childPath, value: next[key] })) {
                return false;
            }
            continue;
        }

        if (!buildPatchOps(base[key], next[key], childPath, ops)) {
            return false;
        }
    }

    return true;
}

/**
 * @param {SettingsRevision} revision
 * @param {SettingsPatchOp[]} ops
 */
function createPatch(revision, ops) {
    return {
        hash_algorithm: revision.hash_algorithm,
        base_hash: revision.settings_hash,
        ops,
    };
}

/**
 * @param {PreparedSettingsPayload} prepared
 * @param {SettingsRevision} revision
 */
function createRootSetPatch(prepared, revision) {
    return createPatch(revision, [{ op: 'set', path: [], value: prepared.value }]);
}

/**
 * @param {PreparedSettingsPayload} prepared
 * @returns {{ body: string, patch: { hash_algorithm: string, base_hash: string, ops: SettingsPatchOp[] } } | null}
 */
export function buildSettingsPatchSaveRequest(prepared) {
    if (!settingsBaseline) {
        return null;
    }

    /** @type {SettingsPatchOp[]} */
    const ops = [];
    const withinOpLimit = buildPatchOps(settingsBaseline.value, prepared.value, [], ops);
    let patch = createPatch(settingsBaseline.revision, ops);
    let body = JSON.stringify(patch);

    if (
        ops.length > 0
        && (!withinOpLimit
            || body.length > MAX_PATCH_BYTES
            || body.length >= prepared.body.length * MAX_PATCH_TO_FULL_RATIO)
    ) {
        patch = createRootSetPatch(prepared, settingsBaseline.revision);
        body = JSON.stringify(patch);
    }

    return { body, patch };
}

/** @param {Response} response */
async function readErrorMessage(response) {
    const text = (await response.text()).trim();
    return text || response.statusText || `HTTP ${response.status}`;
}

/**
 * @param {PreparedSettingsPayload} prepared
 * @param {HeadersInit} headers
 */
export async function trySaveSettingsDelta(prepared, headers) {
    if (!isTauri()) {
        return { saved: false, reason: 'not-tauri' };
    }

    const request = buildSettingsPatchSaveRequest(prepared);
    if (!request) {
        return { saved: false, reason: 'fallback' };
    }

    const response = await fetch('/api/settings/patch', {
        method: 'POST',
        headers,
        body: request.body,
        cache: 'no-cache',
    });

    if (response.ok) {
        const result = await response.json();
        const revision = normalizeSettingsRevision(result);
        if (!revision) {
            throw new Error('Settings patch response missing revision');
        }

        return {
            saved: true,
            mode: result?.mode || 'patch',
            revision,
        };
    }

    const message = await readErrorMessage(response);
    if (response.status === 409) {
        throw new SettingsPatchConflictError(message);
    }

    throw new Error(message);
}
