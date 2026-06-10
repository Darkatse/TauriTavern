// @ts-check

import { mergeObjects } from './form-object-utils.js';
import { assertCharacterFileName } from './character-request-utils.js';

/** @param {string} message */
function badRequest(message) {
    return new Error(`Bad request: ${message}`);
}

/** @param {Record<string, any>} object @param {string} key */
function hasOwn(object, key) {
    return Object.prototype.hasOwnProperty.call(object, key);
}

/** @param {any} tagsRaw */
function splitTags(tagsRaw) {
    if (Array.isArray(tagsRaw)) {
        return tagsRaw.map((tag) => String(tag).trim()).filter(Boolean);
    }

    if (typeof tagsRaw === 'string') {
        return tagsRaw.split(',').map((tag) => tag.trim()).filter(Boolean);
    }

    return [];
}

/** @param {any} value @param {any} [fallback] @param {string} [label] */
export function parseJsonObjectStrict(value, fallback = {}, label = 'JSON payload') {
    if (typeof value !== 'string' || !value.trim()) {
        return fallback;
    }

    try {
        const parsed = JSON.parse(value);
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
            throw new Error('Expected JSON object');
        }
        return parsed;
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        throw badRequest(`Invalid ${label}: ${message}`);
    }
}

/** @param {any} value */
function boolFromValue(value) {
    if (typeof value === 'boolean') {
        return value;
    }

    if (typeof value === 'number') {
        return value !== 0;
    }

    if (value === null || value === undefined) {
        return false;
    }

    const normalized = String(value).trim().toLowerCase();
    return normalized === 'true' || normalized === '1' || normalized === 'on' || normalized === 'yes';
}

/** @param {any} value @param {number} fallback */
function numberFromValue(value, fallback) {
    const number = Number(value);
    return Number.isFinite(number) ? number : fallback;
}

/** @param {Record<string, any>} payload @param {Record<string, any>} data @param {string[]} keys @param {string} [fallback] */
function stringFromPayload(payload, data, keys, fallback = '') {
    for (const key of keys) {
        const value = payload[key] ?? data[key];
        if (value !== null && value !== undefined) {
            return String(value);
        }
    }

    return fallback;
}

/** @param {any} value */
function arrayFromValue(value) {
    if (Array.isArray(value)) {
        return value.map((item) => String(item)).filter(Boolean);
    }

    return [];
}

/** @param {Record<string, any>} payload */
function jsonDataFromPayload(payload) {
    if (!Object.prototype.hasOwnProperty.call(payload, 'json_data')) {
        return null;
    }

    if (
        payload.json_data === null
        || payload.json_data === undefined
        || (typeof payload.json_data === 'string' && !payload.json_data.trim())
    ) {
        return null;
    }

    return JSON.stringify(objectFromValue(payload.json_data, {}, 'character json_data'));
}

/** @param {any} value @param {Record<string, any>} [fallback] @param {string} [label] */
function objectFromValue(value, fallback = {}, label = 'JSON payload') {
    if (value === null || value === undefined || value === '') {
        return fallback;
    }

    if (typeof value === 'string') {
        return parseJsonObjectStrict(value, fallback, label);
    }

    if (typeof value === 'object' && !Array.isArray(value)) {
        return value;
    }

    throw badRequest(`Invalid ${label}: Expected JSON object`);
}

/** @param {FormData} formData @param {string} key @param {string} [fallback] */
function stringFromForm(formData, key, fallback = '') {
    const raw = formData.get(key);
    if (raw === null || raw === undefined) {
        return fallback;
    }

    return String(raw);
}

/** @param {FormData} formData @param {string} key */
function arrayNotationValuesFromForm(formData, key) {
    const values = [];

    for (const [entryKey, entryValue] of formData.entries()) {
        if (entryKey === `${key}[]` || (entryKey.startsWith(`${key}[`) && entryKey.endsWith(']'))) {
            const value = String(entryValue);
            if (value) {
                values.push(value);
            }
        }
    }

    return values;
}

/** @param {Record<string, any>} payload @param {Record<string, any>} data */
function buildCharacterExtensions(payload, data) {
    const dataExtensions = objectFromValue(data.extensions, {}, 'data.extensions');
    const explicitExtensions = objectFromValue(payload.extensions, {}, 'extensions JSON');
    const world = hasOwn(payload, 'world') ? String(payload.world ?? '') : '';
    const defaults = {
        world,
        depth_prompt: {
            prompt: String(payload.depth_prompt_prompt ?? dataExtensions.depth_prompt?.prompt ?? ''),
            depth: numberFromValue(payload.depth_prompt_depth ?? dataExtensions.depth_prompt?.depth, 4),
            role: String(payload.depth_prompt_role ?? dataExtensions.depth_prompt?.role ?? 'system'),
        },
        talkativeness: numberFromValue(payload.talkativeness ?? dataExtensions.talkativeness, 0.5),
        fav: boolFromValue(payload.fav ?? dataExtensions.fav),
    };

    return mergeObjects({}, defaults, dataExtensions, explicitExtensions);
}

/**
 * Maps upstream-compatible JSON character create payloads to Rust DTOs.
 * @param {Record<string, any>} payload
 */
export function payloadToCreateCharacterDto(payload) {
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
        throw badRequest('Expected JSON object body');
    }

    const data = objectFromValue(payload.data, {}, 'data');
    const name = stringFromPayload(payload, data, ['ch_name', 'name']).trim();
    if (!name) {
        throw badRequest('Character name is required');
    }

    const extensions = buildCharacterExtensions(payload, data);
    const tags = payload.tags ?? data.tags;
    const alternateGreetings = payload.alternate_greetings ?? data.alternate_greetings;
    const fileName = assertCharacterFileName(
        stringFromPayload(payload, data, ['file_name', 'fileName']),
        'file_name',
    );
    const primaryLorebook = hasOwn(payload, 'world') ? String(payload.world ?? '') : '';

    return {
        file_name: fileName || null,
        json_data: jsonDataFromPayload(payload),
        primary_lorebook: primaryLorebook === '' ? null : primaryLorebook,
        name,
        description: stringFromPayload(payload, data, ['description']),
        personality: stringFromPayload(payload, data, ['personality']),
        scenario: stringFromPayload(payload, data, ['scenario']),
        first_mes: stringFromPayload(payload, data, ['first_mes', 'firstMessage']),
        mes_example: stringFromPayload(payload, data, ['mes_example', 'messageExamples']),
        creator: stringFromPayload(payload, data, ['creator']),
        creator_notes: stringFromPayload(payload, data, ['creator_notes', 'creatorNotes', 'creatorcomment']),
        character_version: stringFromPayload(payload, data, ['character_version', 'characterVersion']),
        tags: splitTags(tags),
        talkativeness: numberFromValue(payload.talkativeness ?? extensions.talkativeness, 0.5),
        fav: boolFromValue(payload.fav ?? extensions.fav),
        alternate_greetings: arrayFromValue(alternateGreetings),
        system_prompt: stringFromPayload(payload, data, ['system_prompt', 'systemPrompt']),
        post_history_instructions: stringFromPayload(payload, data, ['post_history_instructions', 'postHistoryInstructions']),
        extensions,
    };
}

/** @param {FormData} formData */
export function formDataToCreateCharacterDto(formData) {
    const alternateGreetings = formData.getAll('alternate_greetings').map((item) => String(item)).filter(Boolean);
    const bracketAlternateGreetings = arrayNotationValuesFromForm(formData, 'alternate_greetings');
    const bracketTags = arrayNotationValuesFromForm(formData, 'tags');

    return payloadToCreateCharacterDto({
        file_name: stringFromForm(formData, 'file_name', ''),
        ch_name: stringFromForm(formData, 'ch_name', ''),
        description: stringFromForm(formData, 'description', ''),
        personality: stringFromForm(formData, 'personality', ''),
        scenario: stringFromForm(formData, 'scenario', ''),
        first_mes: stringFromForm(formData, 'first_mes', ''),
        mes_example: stringFromForm(formData, 'mes_example', ''),
        creator: stringFromForm(formData, 'creator', ''),
        creator_notes: stringFromForm(formData, 'creator_notes', ''),
        character_version: stringFromForm(formData, 'character_version', ''),
        tags: bracketTags.length > 0 ? bracketTags : stringFromForm(formData, 'tags', ''),
        talkativeness: stringFromForm(formData, 'talkativeness', '0.5'),
        fav: stringFromForm(formData, 'fav', ''),
        alternate_greetings: alternateGreetings.length > 0 ? alternateGreetings : bracketAlternateGreetings,
        system_prompt: stringFromForm(formData, 'system_prompt', ''),
        post_history_instructions: stringFromForm(formData, 'post_history_instructions', ''),
        world: stringFromForm(formData, 'world', ''),
        depth_prompt_prompt: stringFromForm(formData, 'depth_prompt_prompt', ''),
        depth_prompt_depth: stringFromForm(formData, 'depth_prompt_depth', '4'),
        depth_prompt_role: stringFromForm(formData, 'depth_prompt_role', 'system'),
        extensions: stringFromForm(formData, 'extensions', ''),
        json_data: stringFromForm(formData, 'json_data', ''),
    });
}
