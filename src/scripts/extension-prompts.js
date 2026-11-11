import { inject_ids } from './constants.js';

export const NOTE_MODULE_NAME = '2_floating_prompt'; // Sort after memory.

export function isWorldInfoInjection(key) {
    return key.startsWith(inject_ids.CUSTOM_WI_DEPTH) || key.startsWith(inject_ids.CUSTOM_WI_OUTLET(''));
}

/** World Info surrounds the note; Persona surrounds that result. */
export function composeAuthorsNote(note, { position, depth, role, worldInfo, persona }) {
    let value = note?.value ?? '';
    if (worldInfo) {
        value = `${worldInfo.before}\n${value}\n${worldInfo.after}`.replace(/(^\n)|(\n$)/g, '');
    }
    if (persona?.before) value = `${persona.before}\n${value}`;
    if (persona?.after) value = `${value}\n${persona.after}`;
    return { ...note, value, position, depth, role };
}

/**
 * @enum {number} Extension prompt types
 */
export const extension_prompt_types = {
    NONE: -1,
    IN_PROMPT: 0,
    IN_CHAT: 1,
    BEFORE_PROMPT: 2,
};

/**
 * @enum {number} Extension prompt roles
 */
export const extension_prompt_roles = {
    SYSTEM: 0,
    USER: 1,
    ASSISTANT: 2,
};
