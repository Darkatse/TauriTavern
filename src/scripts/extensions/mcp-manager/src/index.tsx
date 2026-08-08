import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import {
    confirmActivate,
    confirmRemove,
    openAddServerDialog,
    promptServerName,
    requireMcpApi,
    tr,
    waitForHostReady,
} from './host';
import { McpManagerApp, type McpManagerActions } from './McpManagerApp';

const CONTAINER_ID = 'mcp_manager_container';

function ensureContainer(): HTMLElement {
    const existing = document.getElementById(CONTAINER_ID);
    if (existing instanceof HTMLElement) {
        return existing;
    }

    const column = document.getElementById('extensions_settings2');
    if (!(column instanceof HTMLElement)) {
        throw new Error('SillyTavern extension settings column is unavailable');
    }

    const container = document.createElement('div');
    container.id = CONTAINER_ID;
    container.className = 'extension_container';

    const anchor = document.getElementById('skill_manager_container')
        ?? document.getElementById('agent_system_container');
    if (anchor?.parentElement === column) {
        anchor.insertAdjacentElement('afterend', container);
    } else {
        column.prepend(container);
    }
    return container;
}

async function mountMcpManager(): Promise<void> {
    await waitForHostReady();
    const api = requireMcpApi();
    const initial = await api.servers.list();
    const actions: McpManagerActions = {
        addServer: () => openAddServerDialog(api.servers.create),
        renameServer: async (server) => {
            const displayName = await promptServerName(server);
            if (!displayName) {
                return null;
            }
            return api.servers.rename({ registrationId: server.id, displayName });
        },
        setState: api.servers.setState,
        remove: api.servers.remove,
        discover: api.servers.discover,
        setPermission: api.tools.setPermission,
        confirmActivate,
        confirmRemove,
    };

    createRoot(ensureContainer()).render(
        <StrictMode>
            <McpManagerApp initial={initial} actions={actions} tr={tr} />
        </StrictMode>,
    );
}

await mountMcpManager();
