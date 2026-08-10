import { useState } from 'react';

import { errorText, type McpTranslator } from './host';
import { ServerRow } from './ServerRow';

export type McpManagerInitial = Awaited<ReturnType<TauriTavernMcpApi['servers']['list']>>;

export type McpManagerActions = {
    /** Opens the Add-server dialog; resolves the created server, or null when cancelled. */
    addServer: () => Promise<TauriTavernMcpServer | null>;
    /** Prompts for a new name and renames; resolves the updated server, or null when unchanged. */
    renameServer: (server: TauriTavernMcpServer) => Promise<TauriTavernMcpServer | null>;
    setState: TauriTavernMcpApi['servers']['setState'];
    remove: TauriTavernMcpApi['servers']['remove'];
    discover: TauriTavernMcpApi['servers']['discover'];
    refresh: TauriTavernMcpApi['servers']['refresh'];
    setPermission: TauriTavernMcpApi['tools']['setPermission'];
    /** Opens the unified test-call console with the current server list. */
    openTestCall: (servers: TauriTavernMcpServer[]) => Promise<void>;
    confirmActivate: (server: TauriTavernMcpServer) => Promise<boolean>;
    confirmRemove: (server: TauriTavernMcpServer) => Promise<boolean>;
};

type McpManagerAppProps = {
    initial: McpManagerInitial;
    initialError?: string;
    actions: McpManagerActions;
    tr: McpTranslator;
};

type ServerStatus = {
    activity: 'discover' | 'mutate' | undefined;
    error: string;
};

export function McpManagerApp({ initial, initialError = '', actions, tr }: McpManagerAppProps) {
    const [servers, setServers] = useState(initial.servers);
    const [discoveries, setDiscoveries] = useState<Record<string, TauriTavernMcpDiscoveryResult>>({});
    const [expanded, setExpanded] = useState<Record<string, boolean>>({});
    const [statuses, setStatuses] = useState<Record<string, ServerStatus>>({});
    const [adding, setAdding] = useState(false);
    const [testing, setTesting] = useState(false);
    const [panelError, setPanelError] = useState(initialError);

    const sortedServers = [...servers].sort((left, right) => (
        left.displayName.localeCompare(right.displayName) || left.id.localeCompare(right.id)
    ));

    function updateStatus(id: string, patch: Partial<ServerStatus>): void {
        setStatuses(current => ({
            ...current,
            [id]: { activity: undefined, error: '', ...current[id], ...patch },
        }));
    }

    function replaceServer(updated: TauriTavernMcpServer): void {
        setServers(current => current.map(server => (server.id === updated.id ? updated : server)));
    }

    async function runServerAction(
        id: string,
        activity: NonNullable<ServerStatus['activity']>,
        operation: () => Promise<boolean>,
    ): Promise<void> {
        updateStatus(id, activity === 'discover' ? { activity, error: '' } : { activity });
        try {
            if (await operation()) {
                updateStatus(id, { error: '' });
            }
        } catch (error) {
            updateStatus(id, { error: errorText(error, tr('unknownError')) });
        } finally {
            updateStatus(id, { activity: undefined });
        }
    }

    async function addServer(): Promise<void> {
        setPanelError('');
        setAdding(true);
        try {
            const server = await actions.addServer();
            if (server) {
                setServers(current => [...current, server]);
            }
        } catch (error) {
            setPanelError(errorText(error, tr('unknownError')));
        } finally {
            setAdding(false);
        }
    }

    async function renameServer(server: TauriTavernMcpServer): Promise<void> {
        await runServerAction(server.id, 'mutate', async () => {
            const updated = await actions.renameServer(server);
            if (!updated) {
                return false;
            }
            replaceServer(updated);
            return true;
        });
    }

    async function toggleServer(server: TauriTavernMcpServer): Promise<void> {
        const nextState: TauriTavernMcpServerState = server.state === 'active' ? 'paused' : 'active';
        await runServerAction(server.id, 'mutate', async () => {
            if (nextState === 'active' && !await actions.confirmActivate(server)) {
                return false;
            }
            replaceServer(await actions.setState({ registrationId: server.id, state: nextState }));
            return true;
        });
    }

    async function removeServer(server: TauriTavernMcpServer): Promise<void> {
        await runServerAction(server.id, 'mutate', async () => {
            if (!await actions.confirmRemove(server)) {
                return false;
            }
            await actions.remove(server.id);
            setServers(current => current.filter(item => item.id !== server.id));
            setDiscoveries(current => {
                const next = { ...current };
                delete next[server.id];
                return next;
            });
            return true;
        });
    }

    async function loadTools(
        server: TauriTavernMcpServer,
        load: McpManagerActions['discover'],
    ): Promise<void> {
        await runServerAction(server.id, 'discover', async () => {
            const discovery = await load(server.id);
            setDiscoveries(current => ({ ...current, [server.id]: discovery }));
            return true;
        });
    }

    function toggleExpand(server: TauriTavernMcpServer): void {
        const next = !expanded[server.id];
        setExpanded(current => ({ ...current, [server.id]: next }));
        // Expanding an active server means "show me its tools" — discover right away.
        // A previous failure stays visible instead of refiring on every toggle.
        const status = statuses[server.id];
        if (next && server.state === 'active' && !discoveries[server.id] && !status?.error && !status?.activity) {
            void loadTools(server, actions.discover);
        }
    }

    async function setPermission(
        server: TauriTavernMcpServer,
        tool: TauriTavernMcpTool,
        permission: TauriTavernMcpToolPermission,
    ): Promise<void> {
        await runServerAction(server.id, 'mutate', async () => {
            replaceServer(await actions.setPermission({
                registrationId: server.id,
                nativeName: tool.nativeName,
                permission,
            }));
            setDiscoveries(current => {
                const discovery = current[server.id];
                if (!discovery) {
                    return current;
                }
                return {
                    ...current,
                    [server.id]: {
                        ...discovery,
                        tools: discovery.tools.map(item => (
                            item.id === tool.id ? { ...item, permission } : item
                        )),
                    },
                };
            });
            return true;
        });
    }

    async function clearStalePermission(server: TauriTavernMcpServer, nativeName: string): Promise<void> {
        await runServerAction(server.id, 'mutate', async () => {
            replaceServer(await actions.setPermission({
                registrationId: server.id,
                nativeName,
                permission: 'off',
            }));
            setDiscoveries(current => {
                const discovery = current[server.id];
                if (!discovery) {
                    return current;
                }
                return {
                    ...current,
                    [server.id]: {
                        ...discovery,
                        staleTools: discovery.staleTools.filter(tool => tool.nativeName !== nativeName),
                    },
                };
            });
            return true;
        });
    }

    async function openTestConsole(): Promise<void> {
        setPanelError('');
        setTesting(true);
        try {
            await actions.openTestCall(servers);
        } catch (error) {
            setPanelError(errorText(error, tr('unknownError')));
        } finally {
            setTesting(false);
        }
    }

    return (
        <section id="mcp_manager_settings" className="tt-mcp-root">
            <div className="inline-drawer">
                <div className="inline-drawer-toggle inline-drawer-header tt-mcp-drawer-header">
                    <span className="tt-mcp-title">
                        <i className="fa-solid fa-plug-circle-bolt" aria-hidden="true" />
                        <b>{tr('mcp')}</b>
                    </span>
                    <div className="inline-drawer-icon fa-solid fa-circle-chevron-down down" />
                </div>

                <div className="inline-drawer-content">
                    {initial.storageIssues.length > 0 && (
                        <section className="tt-mcp-alert" role="status">
                            <b>
                                <i className="fa-solid fa-triangle-exclamation" aria-hidden="true" />
                                {tr('storageIssues')}
                            </b>
                            <ul>
                                {initial.storageIssues.map(issue => (
                                    <li key={issue.fileName}>
                                        <code>{issue.fileName}</code>
                                        <span>{issue.message}</span>
                                    </li>
                                ))}
                            </ul>
                        </section>
                    )}

                    {panelError && <p className="tt-mcp-error" role="alert">{panelError}</p>}

                    {servers.length === 0 ? (
                        <div className="tt-mcp-empty">
                            <i className="fa-solid fa-plug-circle-bolt" aria-hidden="true" />
                            <b>{tr('emptyTitle')}</b>
                            <span>{tr('emptyHint')}</span>
                            <button
                                type="button"
                                className="menu_button menu_button_icon"
                                disabled={adding}
                                onClick={() => void addServer()}
                            >
                                <i className={`fa-solid ${adding ? 'fa-circle-notch fa-spin' : 'fa-plus'}`} aria-hidden="true" />
                                <span>{tr('addServer')}</span>
                            </button>
                        </div>
                    ) : (
                        <>
                            <div className="tt-mcp-toolbar">
                                <span className="tt-mcp-count">{tr('serverCount', { count: servers.length })}</span>
                                <div className="tt-mcp-toolbar-actions">
                                    <button
                                        type="button"
                                        className="menu_button"
                                        disabled={testing}
                                        onClick={() => void openTestConsole()}
                                    >
                                        {tr('testCall')}
                                    </button>
                                    <button
                                        type="button"
                                        className="menu_button menu_button_icon"
                                        disabled={adding}
                                        onClick={() => void addServer()}
                                    >
                                        <i className={`fa-solid ${adding ? 'fa-circle-notch fa-spin' : 'fa-plus'}`} aria-hidden="true" />
                                        <span>{tr('addServer')}</span>
                                    </button>
                                </div>
                            </div>
                            <div className="tt-mcp-servers">
                                {sortedServers.map(server => {
                                    const status = statuses[server.id];
                                    return (
                                        <ServerRow
                                            key={server.id}
                                            server={server}
                                            discovery={discoveries[server.id]}
                                            expanded={expanded[server.id] === true}
                                            busy={status?.activity !== undefined}
                                            discovering={status?.activity === 'discover'}
                                            error={status?.error ?? ''}
                                            tr={tr}
                                            onToggleExpand={() => toggleExpand(server)}
                                            onToggleState={() => void toggleServer(server)}
                                            onRename={() => void renameServer(server)}
                                            onRemove={() => void removeServer(server)}
                                            onDiscover={() => void loadTools(server, actions.discover)}
                                            onRefresh={() => void loadTools(server, actions.refresh)}
                                            onSetPermission={(tool, permission) => void setPermission(server, tool, permission)}
                                            onClearStale={nativeName => void clearStalePermission(server, nativeName)}
                                        />
                                    );
                                })}
                            </div>
                        </>
                    )}
                </div>
            </div>
        </section>
    );
}
