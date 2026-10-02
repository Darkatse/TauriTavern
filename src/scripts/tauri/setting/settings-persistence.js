// @ts-check

import { invoke, isTauri } from '../../../tauri-bridge.js';

/** @type {unknown} */
let settingsRevision = null;
/** @type {Record<string, any>} */
let personaBaseline = {};

export class SettingsConflictError extends Error {
    /** @param {string} message */
    constructor(message) {
        super(message);
        this.name = 'SettingsConflictError';
    }
}

/** @param {unknown} error */
export function isSettingsConflictError(error) {
    return error instanceof SettingsConflictError;
}

/** @param {unknown} revision */
function requireSettingsRevision(revision) {
    if (revision == null) throw new Error('Settings response missing revision');
    return revision;
}

/**
 * Only Persona cards have independent acknowledgements. The settings tree stays live
 * in its existing owners; no second settings tree is retained for change detection.
 * @param {any} settings
 * @param {unknown} revision
 */
export function captureSettingsSaveState(settings, revision) {
    settingsRevision = isTauri() ? requireSettingsRevision(revision) : null;
    personaBaseline = settingsRevision == null ? {} : structuredClone(personaRecords(settings));
}

/** @param {any} left @param {any} right */
function sameJsonValue(left, right) {
    return Object.is(left, right) || JSON.stringify(left) === JSON.stringify(right);
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

/** @param {Record<string, any>} records */
function personaProjection(records) {
    /** @type {Record<string, any>} */
    const personas = {};
    /** @type {Record<string, any>} */
    const persona_descriptions = {};
    for (const [id, data] of Object.entries(records)) {
        if (data.name !== undefined) personas[id] = data.name;
        if (data.description !== undefined) persona_descriptions[id] = data.description;
    }
    return { personas, persona_descriptions };
}

/**
 * Refresh the disk projection without discarding edits waiting for the settings debounce.
 * @param {any} powerUser
 * @param {Record<string, any>} snapshot
 */
export function applyPersonaSnapshot(powerUser, snapshot) {
    if (settingsRevision == null) {
        throw new Error('Cannot refresh personas before settings have loaded');
    }
    const base = personaBaseline;
    const current = personaRecords({ power_user: powerUser });
    for (const id of new Set([...Object.keys(base), ...Object.keys(snapshot)])) {
        if (sameJsonValue(current[id], base[id])) {
            if (snapshot[id]) current[id] = structuredClone(snapshot[id]);
            else delete current[id];
        }
    }
    Object.assign(powerUser, personaProjection(current));
    personaBaseline = structuredClone(snapshot);
}

/** @param {any} powerUser */
export async function loadPersonaSnapshot(powerUser) {
    const snapshot = await invoke('get_personas');
    applyPersonaSnapshot(powerUser, snapshot);
    return Object.keys(snapshot);
}

/**
 * Capture the entire request before the first await. Only changed Persona cards
 * are projected into this request; omitting a card never deletes it.
 * @param {any} payload
 * @param {HeadersInit} headers
 */
export async function saveSettingsSnapshot(payload, headers) {
    const requestHeaders = new Headers(headers);
    const tauri = isTauri();
    let personaUpdates = {};
    if (tauri) {
        if (settingsRevision == null) {
            throw new Error('Cannot save settings before settings have loaded');
        }
        personaUpdates = Object.fromEntries(Object.entries(personaRecords(payload))
            .filter(([id, value]) => !sameJsonValue(personaBaseline[id], value)));
        // The small independent card snapshot must survive edits during the request.
        personaUpdates = JSON.parse(JSON.stringify(personaUpdates));
        payload = {
            ...payload,
            power_user: { ...payload.power_user, ...personaProjection(personaUpdates) },
        };
        requestHeaders.set('X-TauriTavern-Settings-Revision', JSON.stringify(settingsRevision));
    }
    const body = JSON.stringify(payload);
    if (typeof body !== 'string') throw new Error('Settings payload is not JSON serializable');

    const response = await fetch('/api/settings/save', {
        method: 'POST', headers: requestHeaders, body, cache: 'no-cache',
    });
    if (!response.ok) {
        const message = (await response.text()).trim() || response.statusText;
        if (response.status === 409) throw new SettingsConflictError(message);
        throw new Error(message);
    }
    if (!tauri) return {};

    const result = await response.json();
    settingsRevision = requireSettingsRevision(result.tauritavern_settings_revision);
    const personaErrors = result.persona_errors ?? {};
    // Refreshes may arrive in flight. Advance only the cards this request saved.
    for (const [id, persona] of Object.entries(personaUpdates)) {
        if (!Object.hasOwn(personaErrors, id)) personaBaseline[id] = persona;
    }
    return { personaErrors };
}
