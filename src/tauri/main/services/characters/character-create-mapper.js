// @ts-check

import { getByPath, mergeObjects, setByPath } from './form-object-utils.js';
import { assertCharacterFileName } from './character-request-utils.js';

/** @param {string} message */
function badRequest(message) {
    return new Error(`Bad request: ${message}`);
}

/** @param {Record<string, any>} object @param {string} key */
function hasOwn(object, key) {
    return Object.prototype.hasOwnProperty.call(object, key);
}

/** @param {any} item */
function isScalarListItem(item) {
    return typeof item === 'string' || typeof item === 'number' || typeof item === 'boolean';
}

/** @param {any} tagsRaw */
function splitTags(tagsRaw) {
    if (Array.isArray(tagsRaw)) {
        return tagsRaw.filter(isScalarListItem).map((tag) => String(tag).trim()).filter(Boolean);
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

/**
 * Greetings are message text: array entries stay verbatim, blank ones included.
 * @param {any} value
 */
function greetingsFromValue(value) {
    if (Array.isArray(value)) {
        return value.filter(isScalarListItem).map((item) => String(item));
    }

    if (typeof value === 'string' && value.length > 0) {
        return [value];
    }

    return [];
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

/** @param {any} value @param {string} label */
function optionalObjectFromValue(value, label) {
    try {
        return objectFromValue(value, {}, label);
    } catch (error) {
        console.warn(`Ignoring invalid ${label}:`, error);
        return {};
    }
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
    /** @type {FormDataEntryValue[]} */
    const values = [];

    for (const [entryKey, entryValue] of formData.entries()) {
        if (entryKey === `${key}[]` || (entryKey.startsWith(`${key}[`) && entryKey.endsWith(']'))) {
            values.push(entryValue);
        }
    }

    return values;
}

/** @param {Record<string, any>} payload @param {Record<string, any>} data */
function buildCharacterExtensions(payload, data) {
    const dataExtensions = optionalObjectFromValue(data.extensions, 'data.extensions');
    const explicitExtensions = optionalObjectFromValue(payload.extensions, 'extensions JSON');
    const world = hasOwn(payload, 'world') ? String(payload.world ?? '') : '';
    const defaults = {
        talkativeness: numberFromValue(payload.talkativeness ?? dataExtensions.talkativeness, 0.5),
        fav: boolFromValue(payload.fav ?? dataExtensions.fav),
        world,
        depth_prompt: {
            prompt: String(payload.depth_prompt_prompt ?? dataExtensions.depth_prompt?.prompt ?? ''),
            depth: numberFromValue(payload.depth_prompt_depth ?? dataExtensions.depth_prompt?.depth, 4),
            role: String(payload.depth_prompt_role ?? dataExtensions.depth_prompt?.role ?? 'system'),
        },
    };

    return mergeObjects({}, defaults, dataExtensions, explicitExtensions);
}

/** @param {Record<string, any>} payload */
function baseCardFromPayload(payload) {
    const card = optionalObjectFromValue(payload.json_data, 'character json_data');
    // An object passed as `json_data` belongs to the caller.
    return card === payload.json_data ? mergeObjects({}, card) : card;
}

/**
 * Builds the card upstream `charaFormatData` writes for a create or edit request.
 * Keys the request does not own keep their place in its `json_data` card.
 * @param {Record<string, any>} payload
 */
export function buildCharacterCard(payload) {
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
        throw badRequest('Expected JSON object body');
    }

    const data = objectFromValue(payload.data, {}, 'data');
    const name = stringFromPayload(payload, data, ['ch_name', 'name']).trim();
    if (!name) {
        throw badRequest('Character name is required');
    }

    const description = stringFromPayload(payload, data, ['description']);
    const personality = stringFromPayload(payload, data, ['personality']);
    const scenario = stringFromPayload(payload, data, ['scenario']);
    const firstMes = stringFromPayload(payload, data, ['first_mes', 'firstMessage']);
    const mesExample = stringFromPayload(payload, data, ['mes_example', 'messageExamples']);
    const creatorNotes = stringFromPayload(payload, data, ['creator_notes', 'creatorNotes', 'creatorcomment']);
    const tags = splitTags(payload.tags ?? data.tags);
    const extensions = buildCharacterExtensions(payload, data);

    const card = baseCardFromPayload(payload);
    delete card.json_data;

    const fields = {
        name,
        description,
        personality,
        scenario,
        first_mes: firstMes,
        mes_example: mesExample,
        creatorcomment: creatorNotes,
        avatar: 'none',
        talkativeness: numberFromValue(payload.talkativeness ?? extensions.talkativeness, 0.5),
        fav: boolFromValue(payload.fav ?? extensions.fav),
        tags,
        spec: 'chara_card_v2',
        spec_version: '2.0',
        'data.name': name,
        'data.description': description,
        'data.personality': personality,
        'data.scenario': scenario,
        'data.first_mes': firstMes,
        'data.mes_example': mesExample,
        'data.creator_notes': creatorNotes,
        'data.system_prompt': stringFromPayload(payload, data, ['system_prompt', 'systemPrompt']),
        'data.post_history_instructions': stringFromPayload(payload, data, ['post_history_instructions', 'postHistoryInstructions']),
        'data.tags': tags,
        'data.creator': stringFromPayload(payload, data, ['creator']),
        'data.character_version': stringFromPayload(payload, data, ['character_version', 'characterVersion']),
        'data.alternate_greetings': greetingsFromValue(payload.alternate_greetings ?? data.alternate_greetings),
        'data.extensions': mergeObjects({}, getByPath(card, 'data.extensions', {}), extensions),
    };
    for (const [path, value] of Object.entries(fields)) {
        setByPath(card, path, value);
    }

    return card;
}

/**
 * Maps upstream-compatible character create payloads to the Rust DTO.
 * @param {Record<string, any>} payload
 */
export function payloadToCreateCharacterDto(payload) {
    const card = buildCharacterCard(payload);
    const data = objectFromValue(payload.data, {}, 'data');
    const fileName = assertCharacterFileName(
        stringFromPayload(payload, data, ['file_name', 'fileName']),
        'file_name',
    );
    const primaryLorebook = hasOwn(payload, 'world') ? String(payload.world ?? '') : '';

    return {
        card_json: JSON.stringify(card),
        file_name: fileName || null,
        primary_lorebook: primaryLorebook === '' ? null : primaryLorebook,
    };
}

/** @param {FormData} formData */
export function formDataToCharacterPayload(formData) {
    const alternateGreetings = formData.getAll('alternate_greetings');
    const bracketAlternateGreetings = arrayNotationValuesFromForm(formData, 'alternate_greetings');
    const bracketTags = splitTags(arrayNotationValuesFromForm(formData, 'tags'));

    return {
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
    };
}
