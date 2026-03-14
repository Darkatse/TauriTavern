// @ts-check

import { PanelRuntimeKind } from '../../services/panel-runtime/panel-runtime-kinds.js';

/**
 * @typedef {import('../../services/embedded-runtime/embedded-runtime-manager.js').createEmbeddedRuntimeManager} createEmbeddedRuntimeManager
 * @typedef {ReturnType<createEmbeddedRuntimeManager>} EmbeddedRuntimeManager
 */

/**
 * @param {HTMLElement} drawerContent
 */
function isDrawerOpen(drawerContent) {
    return drawerContent.classList.contains('openDrawer') && !drawerContent.classList.contains('closedDrawer');
}

const COMPAT_ALWAYS_CONNECTED_EXTENSION_CONTAINER_IDS = new Set([
    'regex_container',
    'qr_container',
]);

/**
 * @param {string} id
 * @returns {HTMLElement}
 */
function mustGetElementById(id) {
    const el = document.getElementById(id);
    if (!(el instanceof HTMLElement)) {
        throw new Error(`PanelRuntime: #${id} not found`);
    }
    return el;
}

/**
 * @param {HTMLElement} host
 * @param {DocumentFragment} frag
 */
function parkChildrenInto(host, frag) {
    while (host.firstChild) {
        frag.appendChild(host.firstChild);
    }
}

/**
 * @param {HTMLElement} host
 * @param {DocumentFragment} frag
 */
function restoreChildren(host, frag) {
    if (!frag.firstChild) {
        return;
    }
    host.appendChild(frag);
}

/**
 * Phase 1: Park extension settings DOM under `#rm_extensions_block` while the
 * drawer is closed, keeping the mount containers connected for compatibility.
 *
 * @param {{ manager: EmbeddedRuntimeManager }} options
 */
export function installExtensionsSubtreeGates({ manager }) {
    if (!manager) {
        throw new Error('installExtensionsSubtreeGates requires manager');
    }

    const host = mustGetElementById('rm_extensions_block');
    const alwaysConnectedContainerIds = manager.profile === 'compat'
        ? COMPAT_ALWAYS_CONNECTED_EXTENSION_CONTAINER_IDS
        : null;

    const drawer = host.closest('.drawer');
    if (!(drawer instanceof HTMLElement)) {
        throw new Error('PanelRuntime(extensions): .drawer ancestor not found');
    }

    const toggle = drawer.querySelector('.drawer-toggle');
    if (!(toggle instanceof HTMLElement)) {
        throw new Error('PanelRuntime(extensions): .drawer-toggle not found');
    }

    const containers = Array.from(host.querySelectorAll('.extension_container')).filter((el) => el instanceof HTMLElement);
    /** @type {Map<HTMLElement, DocumentFragment>} */
    const parkedByContainer = new Map();
    for (const container of containers) {
        if (alwaysConnectedContainerIds && alwaysConnectedContainerIds.has(container.id)) {
            continue;
        }
        parkedByContainer.set(container, document.createDocumentFragment());
    }

    const slotId = 'tt:gate:rm_extensions_block:extension_containers';

    manager.register({
        id: slotId,
        kind: PanelRuntimeKind.SubtreeGate,
        element: host,
        visibilityMode: 'manual',
        initialVisible: isDrawerOpen(host),
        hydrate: () => {
            for (const [container, frag] of parkedByContainer.entries()) {
                restoreChildren(container, frag);
            }
        },
        dehydrate: () => {
            for (const [container, frag] of parkedByContainer.entries()) {
                parkChildrenInto(container, frag);
            }
        },
    });

    const onToggleClickCapture = () => {
        manager.setVisible(slotId, true);
        manager.reconcile();
    };
    toggle.addEventListener('click', onToggleClickCapture, true);

    const classObserver = new MutationObserver(() => {
        const open = isDrawerOpen(host);
        manager.setVisible(slotId, open);
        if (open) {
            manager.reconcile();
        }
    });

    classObserver.observe(host, { attributes: true, attributeFilter: ['class'] });

    manager.reconcile();

    return {
        slotId,
        dispose: () => {
            classObserver.disconnect();
            toggle.removeEventListener('click', onToggleClickCapture, true);
        },
    };
}
