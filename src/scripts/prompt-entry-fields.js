import { t } from './i18n.js';
import { INJECTION_POSITION, isPromptInjectionPosition } from './prompt-injections.js';

export const TOOL_DEFINITION_ROLE = 'tool_definition';

export class PromptEntryError extends Error {
    constructor(message) {
        super(message);
        this.name = 'PromptEntryError';
    }
}

/**
 * @param {{ name?: string, identifier?: string, role?: string, fields?: unknown, injection_position?: number }} prompt
 * @param {(text: string) => string} substitute
 * @returns {Record<string, unknown>|undefined}
 * @throws {PromptEntryError}
 */
export function prepareEntryFields(prompt, substitute) {
    const entry = prompt.name || prompt.identifier;
    const fail = message => {
        throw new PromptEntryError(message);
    };

    let fields;
    if (prompt.fields !== undefined && prompt.fields !== null) {
        if (!isPlainObject(prompt.fields)) {
            fail(t`Prompt "${entry}": fields must be a JSON object.`);
        }
        fields = expandMacros(prompt.fields, substitute, key => fail(t`Prompt "${entry}": fields contain "${key}" twice once macros are replaced.`));
        if (Object.keys(fields).length === 0) {
            fields = undefined;
        }
    }

    const ownedKeys = prompt.role === TOOL_DEFINITION_ROLE ? ['role', 'content', 'description'] : ['role', 'content'];
    const ownedKey = ownedKeys.find(key => fields && Object.hasOwn(fields, key));
    if (ownedKey) {
        fail(t`Prompt "${entry}": fields cannot contain "${ownedKey}".`);
    }

    switch (prompt.role) {
        case TOOL_DEFINITION_ROLE:
            if (!isNonEmptyString(fields?.name)) {
                fail(t`Prompt "${entry}": a tool definition needs a name.`);
            }
            return fields;
        case 'tool':
            if (!isNonEmptyString(fields?.tool_call_id)) {
                fail(t`Prompt "${entry}": a tool response needs a tool_call_id.`);
            }
            break;
        case 'assistant':
            if (fields && Object.hasOwn(fields, 'tool_calls')) {
                if (!Array.isArray(fields.tool_calls) || !fields.tool_calls.every(isNamedToolCall)) {
                    fail(t`Prompt "${entry}": tool_calls must be an array of calls, each with an id and a function name.`);
                }
                fields.tool_calls = fields.tool_calls.map(serializeArguments);
            }
            break;
    }

    if (fields && isPromptInjectionPosition(prompt, INJECTION_POSITION.ATTACH_EXISTING)) {
        fail(t`Prompt "${entry}": an entry with fields cannot attach to an existing message.`);
    }
    return fields;
}

/**
 * @param {{ role?: string, fields?: unknown }} prompt
 * @returns {boolean}
 */
export function isToolCallEntry(prompt) {
    return prompt.role === 'assistant' && isPlainObject(prompt.fields) && Array.isArray(prompt.fields.tool_calls) && prompt.fields.tool_calls.length > 0;
}

/**
 * @param {{ content?: string, fields: Record<string, unknown> }} prompt Prepared entry.
 * @returns {{ type: 'function', function: Record<string, unknown> }}
 */
export function toolDefinitionFromEntry(prompt) {
    const { name, ...rest } = prompt.fields;
    return {
        type: 'function',
        function: {
            name,
            ...(prompt.content ? { description: prompt.content } : {}),
            ...rest,
        },
    };
}

/**
 * @param {unknown} value
 * @param {(text: string) => string} substitute
 * @param {(key: string) => never} duplicateKey
 * @returns {any}
 */
function expandMacros(value, substitute, duplicateKey) {
    if (typeof value === 'string') {
        return substitute(value);
    }
    if (Array.isArray(value)) {
        return value.map(item => expandMacros(item, substitute, duplicateKey));
    }
    if (!isPlainObject(value)) {
        return value;
    }
    const expanded = {};
    for (const [key, item] of Object.entries(value)) {
        const expandedKey = substitute(key);
        if (Object.hasOwn(expanded, expandedKey)) {
            duplicateKey(expandedKey);
        }
        expanded[expandedKey] = expandMacros(item, substitute, duplicateKey);
    }
    return expanded;
}

function serializeArguments(call) {
    const args = call.function.arguments;
    if (args === null || typeof args !== 'object') {
        return call;
    }
    return { ...call, function: { ...call.function, arguments: JSON.stringify(args) } };
}

function isNamedToolCall(call) {
    return isPlainObject(call) && isNonEmptyString(call.id) && isPlainObject(call.function) && isNonEmptyString(call.function.name);
}

function isNonEmptyString(value) {
    return typeof value === 'string' && value.trim().length > 0;
}

function isPlainObject(value) {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}
