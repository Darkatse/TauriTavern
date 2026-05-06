import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { createEmbeddedAssetsPanelRoot } from './embedded-assets-panel.js';
import { translateAgentSystem as tr } from './i18n.js';

let activePanel = null;

export function openEmbeddedAssetsPanel(target) {
    if (activePanel?.dialog?.open) {
        activePanel.dialog.focus();
        return;
    }
    if (typeof HTMLDialogElement === 'undefined') {
        throw new Error(tr('agentAssetsElementUnsupported'));
    }

    const dialog = document.createElement('dialog');
    if (typeof dialog.showModal !== 'function') {
        throw new Error(tr('agentAssetsDialogUnsupported'));
    }
    dialog.className = 'ttas-dialog ttas-embed-dialog';
    dialog.setAttribute('data-tt-mobile-surface', 'fullscreen-window');

    const mount = document.createElement('div');
    mount.className = 'ttas-popup-mount ttas-embed-popup-mount';
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

    app = createApp(createEmbeddedAssetsPanelRoot({ target, requestClose }));
    app.mount(mount);
    activePanel = { dialog, app };

    try {
        dialog.showModal();
    } catch (error) {
        cleanup();
        throw error;
    }
}
