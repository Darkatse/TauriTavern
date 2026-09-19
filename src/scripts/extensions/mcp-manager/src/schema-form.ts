/**
 * Maps a tool inputSchema (JSON Schema 2020-12 object) to a flat list of
 * user-facing argument fields, and collects the edited values back into a
 * raw argumentsJson string.
 *
 * Raw number/boolean/enum/JSON tokens are spliced into the output verbatim —
 * never round-tripped through Number or JSON.stringify — so the precision
 * guarantees of the Host ABI string boundary (i64/u64) survive the form.
 */

export type ArgumentFieldKind = 'text' | 'number' | 'integer' | 'boolean' | 'enum' | 'lines' | 'json';

export type ArgumentField = {
    name: string;
    required: boolean;
    hint: string;
    kind: ArgumentFieldKind;
    /** enum: selectable JSON tokens with a human label each. */
    options: Array<{ token: string; label: string }>;
    /** lines: element kind of the one-value-per-line editor. */
    itemKind: 'text' | 'number' | 'integer';
    /** Initial editor value; '' means unset. boolean fields use '' | 'true' | 'false'. */
    initial: string;
};

export type ArgumentFieldError = 'required' | 'number' | 'integer' | 'json';

export type CollectResult =
    | { ok: true; json: string }
    | { ok: false; errors: Record<string, ArgumentFieldError> };

const JSON_NUMBER = /^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?$/;
const JSON_INTEGER = /^-?(0|[1-9]\d*)$/;

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function scalarKind(type: unknown): 'text' | 'number' | 'integer' | 'boolean' | 'json' {
    switch (type) {
        case 'string': return 'text';
        case 'number': return 'number';
        case 'integer': return 'integer';
        case 'boolean': return 'boolean';
        default: return 'json';
    }
}

function initialFor(kind: ArgumentFieldKind, fallback: unknown): string {
    if (fallback === undefined) {
        return '';
    }
    switch (kind) {
        case 'text':
            return typeof fallback === 'string' ? fallback : '';
        case 'number':
        case 'integer':
            return typeof fallback === 'number' && Number.isFinite(fallback) ? String(fallback) : '';
        case 'boolean':
            return typeof fallback === 'boolean' ? String(fallback) : '';
        case 'enum':
            return JSON.stringify(fallback) ?? '';
        case 'lines':
            return Array.isArray(fallback) ? fallback.map(item => String(item)).join('\n') : '';
        case 'json':
            return JSON.stringify(fallback, null, 2) ?? '';
    }
}

function buildField(name: string, required: boolean, schema: unknown): ArgumentField {
    const base = { name, required, options: [], itemKind: 'text' as const };
    if (!isPlainObject(schema)) {
        return { ...base, hint: '', kind: 'json', initial: '' };
    }
    const hint = typeof schema.description === 'string' ? schema.description : '';

    if (Array.isArray(schema.enum) && schema.enum.length > 0
        && schema.enum.every(option => ['string', 'number', 'boolean'].includes(typeof option))) {
        const options = (schema.enum as Array<string | number | boolean>).map(option => ({
            token: JSON.stringify(option),
            label: typeof option === 'string' ? option : String(option),
        }));
        const initial = initialFor('enum', schema.default);
        return {
            ...base,
            hint,
            kind: 'enum',
            options,
            initial: options.some(option => option.token === initial) ? initial : '',
        };
    }

    if (schema.type === 'array') {
        const items = isPlainObject(schema.items) ? scalarKind(schema.items.type) : 'json';
        if (items === 'text' || items === 'number' || items === 'integer') {
            return { ...base, hint, kind: 'lines', itemKind: items, initial: initialFor('lines', schema.default) };
        }
        return { ...base, hint, kind: 'json', initial: initialFor('json', schema.default) };
    }

    const kind = scalarKind(schema.type);
    return { ...base, hint, kind, initial: initialFor(kind, schema.default) };
}

export function buildArgumentFields(inputSchema: Record<string, unknown>): ArgumentField[] {
    if (!isPlainObject(inputSchema.properties)) {
        return [];
    }
    const required = new Set(
        Array.isArray(inputSchema.required)
            ? inputSchema.required.filter((name): name is string => typeof name === 'string')
            : [],
    );
    return Object.entries(inputSchema.properties).map(([name, schema]) => (
        buildField(name, required.has(name), schema)
    ));
}

export function initialArgumentValues(fields: ArgumentField[]): Record<string, string> {
    return Object.fromEntries(fields.map(field => [field.name, field.initial]));
}

/** Validates one edited value and yields its raw JSON token, or null when unset. */
function collectToken(
    field: ArgumentField,
    rawValue: string,
): { token: string } | { unset: true } | { error: ArgumentFieldError } {
    const value = rawValue.trim();
    switch (field.kind) {
        case 'text':
            return value === '' ? { unset: true } : { token: JSON.stringify(rawValue) };
        case 'number':
            if (value === '') return { unset: true };
            return JSON_NUMBER.test(value) ? { token: value } : { error: 'number' };
        case 'integer':
            if (value === '') return { unset: true };
            return JSON_INTEGER.test(value) ? { token: value } : { error: 'integer' };
        case 'boolean':
        case 'enum':
            return value === '' ? { unset: true } : { token: value };
        case 'lines': {
            const lines = rawValue.split('\n').map(line => line.trim()).filter(line => line !== '');
            if (lines.length === 0) return { unset: true };
            if (field.itemKind === 'text') {
                return { token: `[${lines.map(line => JSON.stringify(line)).join(',')}]` };
            }
            const pattern = field.itemKind === 'integer' ? JSON_INTEGER : JSON_NUMBER;
            return lines.every(line => pattern.test(line))
                ? { token: `[${lines.join(',')}]` }
                : { error: field.itemKind };
        }
        case 'json': {
            if (value === '') return { unset: true };
            try {
                JSON.parse(value);
            } catch {
                return { error: 'json' };
            }
            return { token: value };
        }
    }
}

export function collectArgumentsJson(
    fields: ArgumentField[],
    values: Record<string, string>,
): CollectResult {
    const errors: Record<string, ArgumentFieldError> = {};
    const entries: string[] = [];
    for (const field of fields) {
        const collected = collectToken(field, values[field.name] ?? '');
        if ('error' in collected) {
            errors[field.name] = collected.error;
            continue;
        }
        if ('unset' in collected) {
            if (field.required) {
                errors[field.name] = 'required';
            }
            continue;
        }
        entries.push(`${JSON.stringify(field.name)}:${collected.token}`);
    }
    return Object.keys(errors).length > 0
        ? { ok: false, errors }
        : { ok: true, json: `{${entries.join(',')}}` };
}
