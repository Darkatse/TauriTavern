// @ts-check

/**
 * Pure JSON <-> parameter mapping. No DOM, no settings access.
 *
 * Wire shape: an object whose keys are catalog keys. A present key means the
 * parameter is active with that value; an absent key means removed. Toggles
 * therefore only ever serialize as `true`.
 *
 * @typedef {{ type: 'number', min?: number, max?: number } | { type: 'string', options: string[] } | { type: 'text' } | { type: 'boolean' }} FieldType
 * @typedef {{ kind: 'syntax', detail: string } | { kind: 'unknown', key: string } | { kind: 'invalid', key: string }} ParseError
 */

/**
 * @param {Iterable<{ key: string, active: boolean, value: unknown }>} items
 * @returns {string}
 */
export function serializeParams(items) {
    /** @type {Record<string, unknown>} */
    const out = {};
    for (const { key, active, value } of items) {
        if (active) out[key] = value;
    }
    return JSON.stringify(out, null, 2);
}

/**
 * @param {string} text
 * @param {ReadonlyMap<string, FieldType>} schema
 * @returns {{ values: Map<string, unknown>, errors: ParseError[] }}
 */
export function parseParams(text, schema) {
    /** @type {ParseError[]} */
    const errors = [];
    /** @type {unknown} */
    let parsed;
    try {
        parsed = JSON.parse(text);
    } catch (error) {
        return { values: new Map(), errors: [{ kind: 'syntax', detail: error instanceof Error ? error.message : String(error) }] };
    }
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
        return { values: new Map(), errors: [{ kind: 'syntax', detail: 'expected an object' }] };
    }

    const values = new Map();
    for (const [key, value] of Object.entries(parsed)) {
        const field = schema.get(key);
        if (!field) {
            errors.push({ kind: 'unknown', key });
        } else if (!isValid(value, field)) {
            errors.push({ kind: 'invalid', key });
        } else {
            values.set(key, value);
        }
    }
    return { values, errors };
}

/**
 * @param {unknown} value
 * @param {FieldType} field
 */
function isValid(value, field) {
    switch (field.type) {
        case 'boolean': return typeof value === 'boolean';
        case 'text': return typeof value === 'string';
        case 'string': return typeof value === 'string' && field.options.includes(value);
        case 'number': return typeof value === 'number' && Number.isFinite(value)
            && (field.min === undefined || value >= field.min)
            && (field.max === undefined || value <= field.max);
    }
}
