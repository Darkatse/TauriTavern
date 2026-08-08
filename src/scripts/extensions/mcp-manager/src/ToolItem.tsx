import type { McpMessageKey, McpTranslator } from './host';

const PERMISSIONS = ['off', 'ask', 'allow'] as const satisfies readonly TauriTavernMcpToolPermission[];

const PERMISSION_LABELS: Record<TauriTavernMcpToolPermission, McpMessageKey> = {
    off: 'permissionOff',
    ask: 'permissionAsk',
    allow: 'permissionAllow',
};

type SchemaParam = {
    name: string;
    required: boolean;
    description: string;
};

/**
 * Reads the human-facing surface of a JSON Schema 2020-12 object schema:
 * property names, whether they are required, and their one-line descriptions.
 * Non-object shapes simply yield no params; the raw schema stays one click away.
 */
function extractSchemaParams(inputSchema: Record<string, unknown>): SchemaParam[] {
    const { properties, required } = inputSchema;
    if (typeof properties !== 'object' || properties === null || Array.isArray(properties)) {
        return [];
    }
    const requiredNames = new Set(
        Array.isArray(required) ? required.filter((name): name is string => typeof name === 'string') : [],
    );
    return Object.entries(properties as Record<string, unknown>).map(([name, schema]) => {
        const description = (
            typeof schema === 'object'
            && schema !== null
            && 'description' in schema
            && typeof schema.description === 'string'
        ) ? schema.description : '';
        return { name, required: requiredNames.has(name), description };
    });
}

function toolDetails(tool: TauriTavernMcpTool): string {
    return JSON.stringify({
        inputSchema: tool.inputSchema,
        outputSchema: tool.outputSchema,
        annotations: tool.annotations,
    }, null, 2);
}

type PermissionGroupProps = {
    tool: TauriTavernMcpTool;
    busy: boolean;
    tr: McpTranslator;
    onSetPermission: (permission: TauriTavernMcpToolPermission) => void;
};

function PermissionGroup({ tool, busy, tr, onSetPermission }: PermissionGroupProps) {
    return (
        <div className="tt-mcp-perm" role="radiogroup" aria-label={tr('permissionFor', { name: tool.nativeName })}>
            {PERMISSIONS.map(permission => (
                <label key={permission} className={tool.permission === permission ? 'is-selected' : ''}>
                    <input
                        type="radio"
                        name={`tt-mcp-perm-${tool.id}`}
                        value={permission}
                        checked={tool.permission === permission}
                        disabled={busy}
                        onChange={() => onSetPermission(permission)}
                    />
                    <span>{tr(PERMISSION_LABELS[permission])}</span>
                </label>
            ))}
        </div>
    );
}

type ToolItemProps = {
    tool: TauriTavernMcpTool;
    busy: boolean;
    tr: McpTranslator;
    onSetPermission: (permission: TauriTavernMcpToolPermission) => void;
};

export function ToolItem({ tool, busy, tr, onSetPermission }: ToolItemProps) {
    const params = extractSchemaParams(tool.inputSchema);
    const hasNativeAlias = tool.title !== undefined && tool.title !== tool.nativeName;

    return (
        <li className="tt-mcp-tool">
            <div className="tt-mcp-tool-title">
                <b>{tool.title ?? tool.nativeName}</b>
                {hasNativeAlias && <code>{tool.nativeName}</code>}
            </div>
            <PermissionGroup tool={tool} busy={busy} tr={tr} onSetPermission={onSetPermission} />
            <div className="tt-mcp-tool-body">
                {tool.description && <p>{tool.description}</p>}
                {params.length > 0 && (
                    <div className="tt-mcp-params">
                        {params.map(param => (
                            <code
                                key={param.name}
                                className={param.required ? 'is-required' : ''}
                                title={param.description || undefined}
                            >
                                {param.required ? `${param.name}*` : param.name}
                            </code>
                        ))}
                    </div>
                )}
                <details className="tt-mcp-details">
                    <summary>{tr('schemaDetails')}</summary>
                    <pre>{toolDetails(tool)}</pre>
                </details>
            </div>
        </li>
    );
}
