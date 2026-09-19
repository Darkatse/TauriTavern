// @ts-check

import { invoke, isTauri } from '../../../tauri-bridge.js';

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

/** @param {unknown} revision */
export function requireSettingsRevision(revision) {
    const normalizedRevision = normalizeSettingsRevision(revision);
    if (!normalizedRevision) {
        throw new Error('Settings save response missing revision');
    }

    return normalizedRevision;
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

/** @param {any} settings */
function personaRecords(settings) {
    const { personas = {}, persona_descriptions = {} } = settings.power_user ?? {};
    return Object.fromEntries([...new Set([...Object.keys(personas), ...Object.keys(persona_descriptions)])]
        .map(id => [id, {
            ...(Object.hasOwn(personas, id) ? { name: personas[id] } : {}),
            ...(Object.hasOwn(persona_descriptions, id) ? { description: persona_descriptions[id] } : {}),
        }]));
}

/** @param {any} settings */
function withoutPersonas(settings) {
    if (!settings.power_user) return settings;
    const { personas, persona_descriptions, ...power_user } = settings.power_user;
    return { ...settings, power_user };
}

/** @param {Record<string, any>} records */
function personaProjection(records) {
    return {
        personas: Object.fromEntries(Object.entries(records).filter(([, data]) => data.name !== undefined).map(([id, data]) => [id, data.name])),
        persona_descriptions: Object.fromEntries(Object.entries(records).filter(([, data]) => data.description !== undefined).map(([id, data]) => [id, data.description])),
    };
}

/**
 * Refresh the disk projection without discarding edits waiting for the settings debounce.
 * @param {any} powerUser
 * @param {Record<string, any>} snapshot
 */
export function applyPersonaSnapshot(powerUser, snapshot) {
    if (!settingsBaseline) {
        throw new Error('Cannot refresh personas before settings have loaded');
    }
    const base = personaRecords(settingsBaseline.value);
    const current = personaRecords({ power_user: powerUser });
    for (const id of new Set([...Object.keys(base), ...Object.keys(snapshot)])) {
        if (sameJsonValue(current[id], base[id])) {
            if (snapshot[id]) current[id] = structuredClone(snapshot[id]);
            else delete current[id];
        }
    }
    Object.assign(powerUser, personaProjection(current));
    settingsBaseline.value.power_user ??= {};
    Object.assign(settingsBaseline.value.power_user, personaProjection(structuredClone(snapshot)));
}

/** @param {any} powerUser */
export async function loadPersonaSnapshot(powerUser) {
    const snapshot = await invoke('get_personas');
    applyPersonaSnapshot(powerUser, snapshot);
    return Object.keys(snapshot);
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
 * @returns {{ body: string, patch: { hash_algorithm: string, base_hash: string, ops: SettingsPatchOp[], persona_updates: Record<string, any> } } | null}
 */
export function buildSettingsPatchSaveRequest(prepared) {
    if (!settingsBaseline) {
        return null;
    }

    /** @type {SettingsPatchOp[]} */
    const ops = [];
    const basePersonas = personaRecords(settingsBaseline.value);
    const personaUpdates = Object.fromEntries(Object.entries(personaRecords(prepared.value))
        .filter(([id, value]) => !sameJsonValue(basePersonas[id], value)));
    const core = withoutPersonas(prepared.value);
    const withinOpLimit = buildPatchOps(withoutPersonas(settingsBaseline.value), core, [], ops);
    let patch = { ...createPatch(settingsBaseline.revision, ops), persona_updates: personaUpdates };
    let body = JSON.stringify(patch);

    if (
        ops.length > 0
        && (!withinOpLimit
            || body.length > MAX_PATCH_BYTES
            || body.length >= prepared.body.length * MAX_PATCH_TO_FULL_RATIO)
    ) {
        patch = { ...createPatch(settingsBaseline.revision, [{ op: 'set', path: [], value: core }]), persona_updates: personaUpdates };
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
    if (!request || !settingsBaseline) {
        throw new Error('Cannot save settings before settings have loaded');
    }

    const response = await fetch('/api/settings/patch', {
        method: 'POST',
        headers,
        body: request.body,
        cache: 'no-cache',
    });

    if (response.ok) {
        const result = await response.json();
        const revision = requireSettingsRevision(result);
        const personaErrors = result.persona_errors ?? {};
        // A refresh may have arrived during the save. Failed edits keep their previous baseline.
        const savedPersonas = Object.fromEntries(Object.entries(request.patch.persona_updates)
            .filter(([id]) => !Object.hasOwn(personaErrors, id)));
        const personas = { ...personaRecords(settingsBaseline.value), ...savedPersonas };
        captureSettingsSaveBaseline({
            ...prepared.value,
            power_user: { ...prepared.value.power_user, ...personaProjection(personas) },
        }, revision);

        return {
            saved: true,
            mode: result?.mode || 'patch',
            revision,
            ...(Object.keys(personaErrors).length ? { personaErrors } : {}),
        };
    }

    const message = await readErrorMessage(response);
    if (response.status === 409) {
        throw new SettingsPatchConflictError(message);
    }

    throw new Error(message);
}
