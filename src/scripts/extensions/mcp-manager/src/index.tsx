import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import {
    confirmActivate,
    confirmRemove,
    errorText,
    openAddServerDialog,
    promptServerName,
    requireMcpApi,
    tr,
    waitForHostReady,
} from './host';
import { McpManagerApp, type McpManagerActions } from './McpManagerApp';
import { ensureExaRecommendation } from './recommendation';
import { openTestCallDialog } from './test-call-dialog';

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
    const listed = await api.servers.list();
    const store = window.__TAURITAVERN__?.api?.extension?.store;
    const recommendation = store
        ? await ensureExaRecommendation(listed, api.servers.create, store)
        : { initial: listed, error: new Error('TauriTavern extension store is unavailable') };
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
        refresh: api.servers.refresh,
        setPermission: api.tools.setPermission,
        openTestCall: servers => openTestCallDialog({
            servers,
            discover: api.servers.discover,
            refresh: api.servers.refresh,
            testCall: api.tools.testCall,
        }),
        confirmActivate,
        confirmRemove,
    };

    createRoot(ensureContainer()).render(
        <StrictMode>
            <McpManagerApp
                initial={recommendation.initial}
                initialError={recommendation.error ? errorText(recommendation.error, tr('unknownError')) : ''}
                actions={actions}
                tr={tr}
            />
        </StrictMode>,
    );
}

await mountMcpManager();
