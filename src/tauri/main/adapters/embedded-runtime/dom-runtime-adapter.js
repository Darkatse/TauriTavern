// @ts-check

/**
 * @typedef {import('../../services/embedded-runtime/embedded-runtime-manager.js').createEmbeddedRuntimeManager} createEmbeddedRuntimeManager
 * @typedef {ReturnType<createEmbeddedRuntimeManager>} EmbeddedRuntimeManager
 */

/**
 * @typedef {object} DomEmbeddedRuntimeAdapter
 * @property {string} hostSelector
 * @property {(manager: EmbeddedRuntimeManager, host: HTMLElement) => void} registerHost
 */

const PLACEHOLDER_SELECTOR = '.tt-runtime-placeholder';
const SLOT_ID_SELECTOR = '[data-tt-runtime-slot-id]';

/**
 * @param {EmbeddedRuntimeManager} manager
 * @param {Node} root
 * @param {DomEmbeddedRuntimeAdapter[]} adapters
 */
function scanForHosts(manager, root, adapters) {
    if (!(root instanceof Element)) {
        return;
    }

    for (const adapter of adapters) {
        const nearest = root.closest(adapter.hostSelector);
        if (nearest instanceof HTMLElement) {
            adapter.registerHost(manager, nearest);
        }

        const hosts = root.querySelectorAll(adapter.hostSelector);
        for (const host of hosts) {
            adapter.registerHost(manager, /** @type {HTMLElement} */ (host));
        }
    }
}

/**
 * @param {EmbeddedRuntimeManager} manager
 * @param {Node} root
 */
function unregisterSlotsInSubtree(manager, root) {
    if (!(root instanceof Element)) {
        return;
    }

    /** @param {Element} el */
    const unregister = (el) => {
        if (!(el instanceof HTMLElement)) {
            return;
        }
        const id = String(el.dataset.ttRuntimeSlotId || '').trim();
        if (!id) {
            return;
        }
        manager.unregister(id);
        delete el.dataset.ttRuntimeSlotId;
    };

    if (root.matches(SLOT_ID_SELECTOR)) {
        unregister(root);
    }

    for (const el of root.querySelectorAll(SLOT_ID_SELECTOR)) {
        unregister(el);
    }
}

/**
 * Installs a DOM-driven adapter that discovers embedded runtimes under `root`,
 * registers them via detector adapters, and keeps the manager in sync as nodes
 * are added/removed.
 *
 * @param {object} options
 * @param {EmbeddedRuntimeManager} options.manager
 * @param {HTMLElement} options.root
 * @param {DomEmbeddedRuntimeAdapter[]} options.adapters
 */
export function installDomEmbeddedRuntimeAdapter({ manager, root, adapters }) {
    if (!manager) {
        throw new Error('installDomEmbeddedRuntimeAdapter requires manager');
    }
    if (!(root instanceof HTMLElement)) {
        throw new Error('installDomEmbeddedRuntimeAdapter requires root HTMLElement');
    }
    if (!Array.isArray(adapters) || adapters.length === 0) {
        throw new Error('installDomEmbeddedRuntimeAdapter requires non-empty adapters');
    }

    scanForHosts(manager, root, adapters);

    /** @param {Event} event */
    const onClick = (event) => {
        const target = event.target;
        if (!(target instanceof Element)) {
            return;
        }

        const placeholder = target.closest(PLACEHOLDER_SELECTOR);
        if (!placeholder) {
            return;
        }

        const host = placeholder.closest(SLOT_ID_SELECTOR);
        const id = host instanceof HTMLElement ? String(host.dataset.ttRuntimeSlotId || '').trim() : '';
        if (!id) {
            return;
        }

        manager.touch(id);
    };

    const observer = new MutationObserver((records) => {
        for (const record of records) {
            for (const addedNode of record.addedNodes) {
                scanForHosts(manager, addedNode, adapters);
            }
            for (const removedNode of record.removedNodes) {
                unregisterSlotsInSubtree(manager, removedNode);
            }
        }
    });

    observer.observe(root, { childList: true, subtree: true });
    root.addEventListener('click', onClick, true);

    return {
        dispose: () => {
            observer.disconnect();
            root.removeEventListener('click', onClick, true);
        },
    };
}

