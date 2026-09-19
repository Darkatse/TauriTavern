import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';
import userEvent from '@testing-library/user-event';

import {
    McpManagerApp,
    type McpManagerActions,
    type McpManagerInitial,
} from './McpManagerApp';
import { tr } from './host';
import { installPopupHost, TestPopup, uninstallPopupHost } from './popup-stub';
import { ensureExaRecommendation } from './recommendation';
import { openAddServerDialog, openEditServerDialog } from './server-dialog';

const SERVER_ID = '11111111-1111-4111-8111-111111111111';

function server(state: TauriTavernMcpServerState = 'paused'): TauriTavernMcpServer {
    return {
        id: SERVER_ID,
        displayName: 'Local tools',
        endpoint: 'http://127.0.0.1:3000/mcp',
        headers: {},
        protocolVersion: 'auto',
        state,
        toolPermissions: {},
        toolDescriptionOverrides: {},
    };
}

function discovery(): TauriTavernMcpDiscoveryResult {
    return {
        registrationId: SERVER_ID,
        protocolVersion: '2026-07-28',
        serverName: 'Local MCP',
        tools: [{
            id: `mcp/${SERVER_ID}:search`,
            nativeName: 'search',
            title: 'Search files',
            description: 'Search local files by name.',
            inputSchema: { type: 'object', properties: { query: { type: 'string' } }, required: ['query'] },
            annotations: {},
            permission: 'off',
        }],
        diagnostics: [],
        staleTools: [],
    };
}

function unexpected(name: string): Promise<never> {
    return Promise.reject(new Error(`Unexpected MCP Manager action: ${name}`));
}

function actions(overrides: Partial<McpManagerActions> = {}): McpManagerActions {
    return {
        addServer: () => unexpected('addServer'),
        editServer: () => unexpected('editServer'),
        setState: () => unexpected('setState'),
        remove: () => unexpected('remove'),
        discover: () => unexpected('discover'),
        refresh: () => unexpected('refresh'),
        setPermission: () => unexpected('setPermission'),
        setDescriptionOverride: () => unexpected('setDescriptionOverride'),
        openToolDialog: () => unexpected('openToolDialog'),
        openTestCall: () => unexpected('openTestCall'),
        confirmActivate: () => Promise.resolve(true),
        confirmRemove: () => Promise.resolve(true),
        ...overrides,
    };
}

function initial(servers: TauriTavernMcpServer[] = []): McpManagerInitial {
    return { servers, storageIssues: [] };
}

afterEach(() => {
    cleanup();
    uninstallPopupHost();
});
test('adds a server through the dialog action and lists it paused', async () => {
    const created = server('paused');
    const user = userEvent.setup();
    render(
        <McpManagerApp
            initial={initial()}
            tr={tr}
            actions={actions({ addServer: () => Promise.resolve(created) })}
        />,
    );

    await user.click(screen.getByRole('button', { name: 'Add server' }));

    expect(await screen.findByText('Local tools')).toBeTruthy();
    expect(screen.getByText('http://127.0.0.1:3000/mcp')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Paused' })).toBeTruthy();
});

test('creates the Exa recommendation once and does not restore it after deletion', async () => {
    let handled = false;
    const creates: Array<{ displayName: string; endpoint: string }> = [];
    const store = {
        tryGetJson: () => Promise.resolve({ found: handled }),
        setJson: () => {
            handled = true;
            return Promise.resolve();
        },
    };
    const create = (input: { displayName: string; endpoint: string }) => {
        creates.push(input);
        return Promise.resolve({ ...server(), ...input });
    };

    const first = await ensureExaRecommendation(initial(), create, store);
    expect(first.error).toBeUndefined();
    expect(first.initial.servers).toEqual([{ ...server(), ...creates[0] }]);
    expect(creates).toEqual([{
        displayName: 'Exa Search',
        endpoint: 'https://mcp.exa.ai/mcp',
    }]);

    const afterDeletion = await ensureExaRecommendation(initial(), create, store);
    expect(afterDeletion.error).toBeUndefined();
    expect(afterDeletion.initial.servers).toEqual([]);
    expect(creates).toHaveLength(1);
});

test('discovers tools on expand and persists an explicit permission choice', async () => {
    const permissionCalls: Array<{
        registrationId: string;
        nativeName: string;
        permission: TauriTavernMcpToolPermission;
    }> = [];
    let discoverCalls = 0;
    let refreshCalls = 0;
    const activeServer = server('active');
    const user = userEvent.setup();
    render(
        <McpManagerApp
            initial={initial([activeServer])}
            tr={tr}
            actions={actions({
                discover: () => {
                    discoverCalls += 1;
                    return Promise.resolve(discovery());
                },
                refresh: () => {
                    refreshCalls += 1;
                    return Promise.resolve(discovery());
                },
                setPermission: input => {
                    permissionCalls.push(input);
                    return Promise.resolve({
                        ...activeServer,
                        toolPermissions: { [input.nativeName]: 'allow' },
                    });
                },
            })}
        />,
    );

    await user.click(screen.getByRole('button', { name: 'Show or hide tools' }));
    expect(await screen.findByText('Search files')).toBeTruthy();
    expect(discoverCalls).toBe(1);

    await user.click(screen.getByRole('button', { name: 'Refresh tools' }));
    await waitFor(() => expect(refreshCalls).toBe(1));
    expect(discoverCalls).toBe(1);

    await user.click(screen.getByRole('radio', { name: 'Allow' }));

    await waitFor(() => expect(permissionCalls).toEqual([{
        registrationId: SERVER_ID,
        nativeName: 'search',
        permission: 'allow',
    }]));
    expect(screen.getByRole<HTMLInputElement>('radio', { name: 'Allow' }).checked).toBe(true);
});

test('opens the description editor from a tool row and applies the saved override', async () => {
    const calls: Parameters<TauriTavernMcpApi['tools']['setDescriptionOverride']>[0][] = [];
    const dialogInput: { current: Parameters<McpManagerActions['openToolDialog']>[0] | undefined } = {
        current: undefined,
    };
    const current: TauriTavernMcpServer = {
        ...server('active'),
        toolDescriptionOverrides: {
            search: { properties: { query: 'Search terms.' } },
        },
    };
    const user = userEvent.setup();
    render(
        <McpManagerApp
            initial={initial([current])}
            tr={tr}
            actions={actions({
                discover: () => Promise.resolve(discovery()),
                setDescriptionOverride: input => {
                    calls.push(input);
                    const toolDescriptionOverrides = { ...current.toolDescriptionOverrides };
                    if (input.override) {
                        toolDescriptionOverrides[input.nativeName] = input.override;
                    } else {
                        delete toolDescriptionOverrides[input.nativeName];
                    }
                    return Promise.resolve({ ...current, toolDescriptionOverrides });
                },
                openToolDialog: input => {
                    dialogInput.current = input;
                    return Promise.resolve();
                },
            })}
        />,
    );

    await user.click(screen.getByRole('button', { name: 'Show or hide tools' }));
    await user.click(await screen.findByRole('button', { name: 'Edit description' }));

    const dialog = dialogInput.current;
    if (!dialog) {
        throw new Error('Description dialog was not opened');
    }
    expect(dialog.tool.nativeName).toBe('search');
    expect(dialog.override).toEqual({ properties: { query: 'Search terms.' } });
    // Without custom text the row shows the server description and no marker.
    expect(screen.getByText('Search local files by name.')).toBeTruthy();
    expect(screen.queryByText('Custom')).toBeNull();

    await act(async () => {
        await dialog.save({
            description: 'Use only for local filename searches.',
            properties: { query: 'Search terms.' },
        });
    });
    expect(calls).toEqual([{
        registrationId: SERVER_ID,
        nativeName: 'search',
        override: {
            description: 'Use only for local filename searches.',
            properties: { query: 'Search terms.' },
        },
    }]);
    // The row now reflects the effective description and marks it as custom.
    expect(await screen.findByText('Use only for local filename searches.')).toBeTruthy();
    expect(screen.getByText('Custom')).toBeTruthy();
    expect(screen.queryByText('Search local files by name.')).toBeNull();
});

test('shows a tool editor opening failure on its server', async () => {
    const user = userEvent.setup();
    render(<McpManagerApp
        initial={initial([server('active')])} tr={tr}
        actions={actions({
            discover: () => Promise.resolve(discovery()),
            openToolDialog: () => Promise.reject(new Error('Popup API is unavailable')),
        })}
    />);
    await user.click(screen.getByRole('button', { name: 'Show or hide tools' }));
    await user.click(await screen.findByRole('button', { name: 'Edit description' }));

    expect((await screen.findByRole('alert')).textContent).toBe('Popup API is unavailable');
});

test('does not discover while paused and explains the state instead', async () => {
    let discoverCalls = 0;
    const user = userEvent.setup();
    render(
        <McpManagerApp
            initial={initial([server('paused')])}
            tr={tr}
            actions={actions({
                discover: () => {
                    discoverCalls += 1;
                    return Promise.resolve(discovery());
                },
            })}
        />,
    );

    await user.click(screen.getByRole('button', { name: 'Show or hide tools' }));

    expect(await screen.findByText(/Paused — activate to discover/)).toBeTruthy();
    expect(discoverCalls).toBe(0);
});

test('keeps a discovery error while an unrelated action is cancelled', async () => {
    let resolveEdit!: (value: TauriTavernMcpServer | null) => void;
    const editResult = new Promise<TauriTavernMcpServer | null>(resolve => {
        resolveEdit = resolve;
    });
    const user = userEvent.setup();
    render(
        <McpManagerApp
            initial={initial([server('active')])}
            tr={tr}
            actions={actions({
                discover: () => Promise.reject(new Error('discovery failed')),
                editServer: () => editResult,
            })}
        />,
    );

    await user.click(screen.getByRole('button', { name: 'Show or hide tools' }));
    expect((await screen.findByRole('alert')).textContent).toBe('discovery failed');

    await user.click(screen.getByRole('button', { name: 'Edit' }));
    expect(screen.queryByText('Discovering tools…')).toBeNull();
    expect(screen.getByRole('alert').textContent).toBe('discovery failed');

    await act(async () => {
        resolveEdit(null);
        await editResult;
    });
    expect(screen.getByRole('alert').textContent).toBe('discovery failed');
});

test('uses explicit refresh when retrying a failed catalog load', async () => {
    let refreshes = 0;
    const user = userEvent.setup();
    render(
        <McpManagerApp
            initial={initial([server('active')])}
            tr={tr}
            actions={actions({
                discover: () => Promise.reject(new Error('stored catalog is invalid')),
                refresh: () => {
                    refreshes += 1;
                    return Promise.resolve(discovery());
                },
            })}
        />,
    );

    await user.click(screen.getByRole('button', { name: 'Show or hide tools' }));
    expect((await screen.findByRole('alert')).textContent).toBe('stored catalog is invalid');
    await user.click(screen.getByRole('button', { name: 'Retry' }));

    expect(await screen.findByText('Search files')).toBeTruthy();
    expect(refreshes).toBe(1);
});

test('opens the unified test console from the toolbar with the current servers', async () => {
    const openedWith: TauriTavernMcpServer[][] = [];
    const activeServer = server('active');
    const user = userEvent.setup();
    render(
        <McpManagerApp
            initial={initial([activeServer])}
            tr={tr}
            actions={actions({
                openTestCall: servers => {
                    openedWith.push(servers);
                    return Promise.resolve();
                },
            })}
        />,
    );

    await user.click(screen.getByRole('button', { name: 'Test call' }));

    expect(openedWith).toEqual([[activeServer]]);
});

test('add-server popup validates in place, preserves failures, and returns the created server', async () => {
    installPopupHost();
    const drafts: Parameters<TauriTavernMcpApi['servers']['create']>[0][] = [];
    const opened = openAddServerDialog(draft => {
        drafts.push(draft);
        if (drafts.length === 1) {
            return Promise.reject(new Error('endpoint host is not allowed'));
        }
        return Promise.resolve(server());
    });
    const popup = TestPopup.current;
    if (!popup) {
        throw new Error('Add-server popup was not created');
    }

    const name = screen.getByLabelText<HTMLInputElement>('Name');
    const endpoint = screen.getByLabelText<HTMLInputElement>('Endpoint');
    await waitFor(() => expect(document.activeElement).toBe(name));

    expect(await popup.close(1)).toBe(false);
    expect(screen.getByRole('alert').textContent).toBe('Enter a name.');
    expect(drafts).toEqual([]);

    const user = userEvent.setup();
    await user.type(name, '  Local tools  ');
    await user.click(screen.getByRole('button', { name: 'Add header' }));
    await user.type(screen.getByLabelText('Header name'), 'x-api-key');
    await user.type(screen.getByLabelText('Header value'), 'secret');
    await user.type(endpoint, 'file:///etc/passwd');
    expect(await popup.close(1)).toBe(false);
    expect(screen.getByRole('alert').textContent).toBe('Enter a valid http:// or https:// URL.');
    expect(drafts).toEqual([]);

    await user.clear(endpoint);
    await user.type(endpoint, ' http://127.0.0.1:3000/mcp ');
    expect(await popup.close(1)).toBe(false);
    expect(screen.getByRole('alert').textContent).toBe('endpoint host is not allowed');
    expect(name.value).toBe('  Local tools  ');
    expect(endpoint.value).toBe(' http://127.0.0.1:3000/mcp ');

    expect(await popup.close(1)).toBe(true);
    expect(await opened).toEqual(server());
    expect(drafts).toEqual([
        {
            displayName: 'Local tools',
            endpoint: 'http://127.0.0.1:3000/mcp',
            headers: { 'x-api-key': 'secret' },
            protocolVersion: 'auto',
        },
        {
            displayName: 'Local tools',
            endpoint: 'http://127.0.0.1:3000/mcp',
            headers: { 'x-api-key': 'secret' },
            protocolVersion: 'auto',
        },
    ]);
});

test('imports one server from the direct MCP JSON form', async () => {
    installPopupHost();
    const drafts: Parameters<TauriTavernMcpApi['servers']['create']>[0][] = [];
    const opened = openAddServerDialog(draft => {
        drafts.push(draft);
        return Promise.resolve({
            ...server(),
            displayName: draft.displayName,
            endpoint: draft.endpoint,
        });
    });
    const popup = TestPopup.current;
    if (!popup) {
        throw new Error('Add-server popup was not created');
    }

    const user = userEvent.setup();
    await user.click(screen.getByRole('radio', { name: 'JSON' }));
    screen.getByLabelText<HTMLTextAreaElement>('MCP JSON').value = JSON.stringify({
        exa: {
            url: 'https://mcp.exa.ai/mcp',
            headers: { 'x-api-key': 'YOUR_EXA_API_KEY' },
        },
    });

    expect(await popup.close(1)).toBe(true);
    expect((await opened)?.displayName).toBe('exa');
    expect(drafts).toEqual([{
        displayName: 'exa',
        endpoint: 'https://mcp.exa.ai/mcp',
        headers: { 'x-api-key': 'YOUR_EXA_API_KEY' },
        protocolVersion: 'auto',
    }]);
});

test('edits a saved endpoint, headers, and protocol version', async () => {
    installPopupHost();
    const existing = {
        ...server(),
        headers: { ' X-API-Key ': 'old-secret', 'x-api-key': 'second-secret' },
        protocolVersion: '2025-11-25' as const,
    };
    const updates: Parameters<TauriTavernMcpApi['servers']['update']>[0][] = [];
    const opened = openEditServerDialog(existing, input => {
        updates.push(input);
        return Promise.resolve({
            ...existing,
            displayName: input.displayName,
            endpoint: input.endpoint,
            headers: input.headers,
            protocolVersion: input.protocolVersion,
        });
    });
    const popup = TestPopup.current;
    if (!popup) {
        throw new Error('Edit-server popup was not created');
    }

    const user = userEvent.setup();
    const endpoint = screen.getByLabelText<HTMLInputElement>('Endpoint');
    await user.clear(endpoint);
    await user.type(endpoint, 'https://user:pass@example.com/mcp?tenant=updated');
    const headerValue = screen.getAllByLabelText<HTMLInputElement>('Header value')[0];
    if (!headerValue) {
        throw new Error('Header value input was not created');
    }
    await user.clear(headerValue);
    await user.type(headerValue, 'new-secret');
    await user.selectOptions(screen.getByLabelText('Protocol version'), '2025-06-18');

    expect(await popup.close(1)).toBe(true);
    expect((await opened)?.protocolVersion).toBe('2025-06-18');
    expect((await opened)?.endpoint).toBe('https://user:pass@example.com/mcp?tenant=updated');
    expect(updates).toEqual([{
        registrationId: SERVER_ID,
        displayName: 'Local tools',
        endpoint: 'https://user:pass@example.com/mcp?tenant=updated',
        headers: { ' X-API-Key ': 'new-secret', 'x-api-key': 'second-secret' },
        protocolVersion: '2025-06-18',
    }]);
});
