import type { McpTranslator } from './host';
import { ToolItem } from './ToolItem';

function discoveryIdentity(discovery: TauriTavernMcpDiscoveryResult, tr: McpTranslator): string {
    const implementation = [discovery.serverName, discovery.serverVersion].filter(Boolean).join(' ');
    return implementation
        ? tr('discoveryIdentity', { implementation, protocol: discovery.protocolVersion })
        : `MCP ${discovery.protocolVersion}`;
}

type ServerRowProps = {
    server: TauriTavernMcpServer;
    discovery: TauriTavernMcpDiscoveryResult | undefined;
    expanded: boolean;
    busy: boolean;
    discovering: boolean;
    error: string;
    tr: McpTranslator;
    onToggleExpand: () => void;
    onToggleState: () => void;
    onRename: () => void;
    onRemove: () => void;
    onDiscover: () => void;
    onSetPermission: (tool: TauriTavernMcpTool, permission: TauriTavernMcpToolPermission) => void;
    onClearStale: (nativeName: string) => void;
};

export function ServerRow({
    server,
    discovery,
    expanded,
    busy,
    discovering,
    error,
    tr,
    onToggleExpand,
    onToggleState,
    onRename,
    onRemove,
    onDiscover,
    onSetPermission,
    onClearStale,
}: ServerRowProps) {
    const active = server.state === 'active';

    return (
        <article className={`tt-mcp-server${active ? ' is-active' : ''}${expanded ? ' is-expanded' : ''}`}>
            <div className="tt-mcp-server-row">
                <span className="tt-mcp-lamp" aria-hidden="true" />
                <div className="tt-mcp-identity">
                    <b className="tt-mcp-name" title={server.displayName}>{server.displayName}</b>
                    <code className="tt-mcp-endpoint" title={server.endpoint}>{server.endpoint}</code>
                </div>
                <button
                    type="button"
                    className={`tt-mcp-state${active ? ' is-active' : ''}`}
                    aria-pressed={active}
                    disabled={busy}
                    onClick={onToggleState}
                >
                    <span className="tt-mcp-state-dot" aria-hidden="true" />
                    {tr(active ? 'active' : 'paused')}
                </button>
                <div className="tt-mcp-row-actions">
                    <button
                        type="button"
                        className="tt-mcp-icon-btn"
                        title={tr('rename')}
                        aria-label={tr('rename')}
                        disabled={busy}
                        onClick={onRename}
                    >
                        <i className="fa-solid fa-pen" aria-hidden="true" />
                    </button>
                    <button
                        type="button"
                        className="tt-mcp-icon-btn is-danger"
                        title={tr('remove')}
                        aria-label={tr('remove')}
                        disabled={busy}
                        onClick={onRemove}
                    >
                        <i className="fa-solid fa-trash-can" aria-hidden="true" />
                    </button>
                    <button
                        type="button"
                        className="tt-mcp-icon-btn tt-mcp-chevron"
                        title={tr('toggleTools')}
                        aria-label={tr('toggleTools')}
                        aria-expanded={expanded}
                        onClick={onToggleExpand}
                    >
                        <i className="fa-solid fa-chevron-down" aria-hidden="true" />
                    </button>
                </div>
            </div>

            {error && !expanded && <p className="tt-mcp-error" role="alert">{error}</p>}

            {expanded && (
                <div className="tt-mcp-console">
                    {error && <p className="tt-mcp-error" role="alert">{error}</p>}
                    {!discovery && !active && (
                        <p className="tt-mcp-note">
                            <i className="fa-solid fa-pause" aria-hidden="true" />
                            {tr('pausedHint')}
                        </p>
                    )}

                    {!discovery && active && discovering && (
                        <p className="tt-mcp-note">
                            <i className="fa-solid fa-circle-notch fa-spin" aria-hidden="true" />
                            {tr('discovering')}
                        </p>
                    )}

                    {!discovery && active && !discovering && (
                        <button type="button" className="menu_button menu_button_icon" disabled={busy} onClick={onDiscover}>
                            <i className="fa-solid fa-arrows-rotate" aria-hidden="true" />
                            <span>{error ? tr('retry') : tr('discoverTools')}</span>
                        </button>
                    )}

                    {discovery && (
                        <>
                            <div className="tt-mcp-console-head">
                                <span className="tt-mcp-console-meta">{discoveryIdentity(discovery, tr)}</span>
                                <span className="tt-mcp-console-count">{tr('toolCount', { count: discovery.tools.length })}</span>
                                <button
                                    type="button"
                                    className="tt-mcp-icon-btn"
                                    title={tr('refreshTools')}
                                    aria-label={tr('refreshTools')}
                                    disabled={!active || busy}
                                    onClick={onDiscover}
                                >
                                    <i className={`fa-solid ${discovering ? 'fa-circle-notch fa-spin' : 'fa-arrows-rotate'}`} aria-hidden="true" />
                                </button>
                            </div>

                            {discovery.diagnostics.length > 0 && (
                                <details className="tt-mcp-details">
                                    <summary>{tr('diagnostics')} · {discovery.diagnostics.length}</summary>
                                    <ul>
                                        {discovery.diagnostics.map(diagnostic => (
                                            <li key={`${diagnostic.code}:${diagnostic.nativeName ?? ''}:${diagnostic.message}`}>
                                                <b>{diagnostic.nativeName ?? diagnostic.code}</b>
                                                <span>{diagnostic.message}</span>
                                            </li>
                                        ))}
                                    </ul>
                                </details>
                            )}

                            <ul className="tt-mcp-tools">
                                {discovery.tools.map(tool => (
                                    <ToolItem
                                        key={tool.id}
                                        tool={tool}
                                        busy={busy}
                                        tr={tr}
                                        onSetPermission={permission => onSetPermission(tool, permission)}
                                    />
                                ))}
                            </ul>

                            {discovery.staleTools.length > 0 && (
                                <div className="tt-mcp-stale">
                                    <b>{tr('configuredToolsMissing')}</b>
                                    <ul>
                                        {discovery.staleTools.map(tool => (
                                            <li key={tool.nativeName}>
                                                <code>{tool.nativeName}</code>
                                                <span>{tr(tool.permission === 'allow' ? 'permissionAllow' : 'permissionAsk')}</span>
                                                <button
                                                    type="button"
                                                    className="menu_button"
                                                    disabled={busy}
                                                    onClick={() => onClearStale(tool.nativeName)}
                                                >
                                                    {tr('setOff')}
                                                </button>
                                            </li>
                                        ))}
                                    </ul>
                                </div>
                            )}
                        </>
                    )}
                </div>
            )}
        </article>
    );
}
