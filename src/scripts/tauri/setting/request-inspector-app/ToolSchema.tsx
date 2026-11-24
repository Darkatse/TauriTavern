import { JsonTree } from './JsonTree';
import type { Translate } from './RequestInspectorContract';
import { clip } from './json-tree';

type SchemaRow = { name: string; nested: boolean; required: boolean; type: string; description: string };

/** Keywords that make a schema more than a flat list of properties. */
const COMBINATORS = ['anyOf', 'oneOf', 'allOf', 'not', '$ref'];
const ENUM_LENGTH = 60;

function isRecord(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function typeName(schema: unknown): string {
    if (!isRecord(schema)) {
        return '';
    }
    if (Array.isArray(schema.enum)) {
        return clip(schema.enum.map(option => JSON.stringify(option)).join(' | '), ENUM_LENGTH);
    }
    if (schema.type === 'array') {
        return `${typeName(schema.items) || 'any'}[]`;
    }
    if (Array.isArray(schema.type)) {
        return schema.type.filter(type => typeof type === 'string').join(' | ');
    }
    return typeof schema.type === 'string' ? schema.type : '';
}

/** The parameters of a schema that is a plain property list; `null` for any other schema. */
function schemaRows(schema: unknown, nested = false): SchemaRow[] | null {
    if (!isRecord(schema) || !isRecord(schema.properties) || COMBINATORS.some(key => key in schema)) {
        return null;
    }
    const required = new Set(Array.isArray(schema.required) ? schema.required : []);
    return Object.entries(schema.properties).flatMap(([name, property]) => [
        {
            name,
            nested,
            required: required.has(name),
            type: typeName(property),
            description: isRecord(property) && typeof property.description === 'string' ? property.description : '',
        },
        ...(nested ? [] : schemaRows(property, true) ?? []),
    ]);
}

export function ToolSchema({ schema, needle, tr, onCopy }: {
    schema: unknown;
    needle: string;
    tr: Translate;
    onCopy: (text: string) => void;
}) {
    if (schema === undefined || schema === null) {
        return null;
    }
    const rows = schemaRows(schema);
    if (!rows) {
        return <JsonTree value={schema} label={tr('Parameters')} needle={needle} tr={tr} onCopy={onCopy} />;
    }
    if (rows.length === 0) {
        return <p className="tt-ri-dim">{tr('No parameters')}</p>;
    }
    return (
        <ul className="tt-ri-params" aria-label={tr('Parameters')}>
            {rows.map((row, index) => (
                <li key={index} className={`tt-ri-param${row.nested ? ' tt-ri-param-nested' : ''}`}>
                    <span className="tt-ri-key">{row.name}</span>
                    {row.type && <span className="tt-ri-dim">{row.type}</span>}
                    {row.required && <span className="tt-ri-tag">{tr('Required')}</span>}
                    {row.description && <span className="tt-ri-param-text">{row.description}</span>}
                </li>
            ))}
        </ul>
    );
}
