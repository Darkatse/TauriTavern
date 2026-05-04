import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { createAgentSystemPanelRoot } from './AgentSystemPanelApp.js';
import { translateAgentSystem as tr } from './i18n.js';

let activePanel = null;

export function openAgentSystemPanel() {
    if (activePanel?.dialog?.open) {
        activePanel.dialog.focus();
        return;
    }
    if (typeof HTMLDialogElement === 'undefined') {
        throw new Error(tr('agentSystemElementUnsupported'));
    }

    const dialog = document.createElement('dialog');
    if (typeof dialog.showModal !== 'function') {
        throw new Error(tr('agentSystemDialogUnsupported'));
    }
    dialog.className = 'ttas-dialog';
    const mount = document.createElement('div');
    mount.className = 'ttas-popup-mount';
    dialog.appendChild(mount);
    document.body.appendChild(dialog);

    let app = null;
    const requestClose = () => {
        dialog.close();
    };

    const cleanup = () => {
        app?.unmount();
        dialog.remove();
        if (activePanel?.dialog === dialog) {
            activePanel = null;
        }
    };
    dialog.addEventListener('close', cleanup, { once: true });
    dialog.addEventListener('cancel', (event) => {
        event.preventDefault();
        dialog.close();
    });

    app = createApp(createAgentSystemPanelRoot({ requestClose }));
    app.mount(mount);
    activePanel = { dialog, app };

    try {
        dialog.showModal();
    } catch (error) {
        cleanup();
        throw error;
    }
}
