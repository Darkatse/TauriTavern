import { isAndroidRuntime } from '../../../../scripts/util/mobile-runtime.js';

const CONTROLLER_KEY = '__TAURITAVERN_ANDROID_IME_LAYOUT_HOST__';
const LIFT_ATTR = 'data-tt-android-ime-lift';
const SPACER_ATTR = 'data-tt-android-ime-spacer';

function requireHTMLElement(id) {
    const element = document.getElementById(id);
    if (!(element instanceof HTMLElement)) {
        throw new Error(`[TauriTavern] #${id} unavailable while installing Android IME layout host.`);
    }

    return element;
}

function moveNodeIntoLiftRoot(node, liftRoot) {
    if (node === liftRoot) {
        return;
    }

    if (node instanceof HTMLElement && node.hasAttribute(SPACER_ATTR)) {
        return;
    }

    liftRoot.append(node);
}

export function installAndroidImeLayoutHost() {
    if (!isAndroidRuntime()) {
        return null;
    }

    const existingController = window[CONTROLLER_KEY];
    if (existingController) {
        return existingController;
    }

    if (typeof MutationObserver !== 'function') {
        throw new Error('[TauriTavern] MutationObserver unavailable while installing Android IME layout host.');
    }

    const formShell = requireHTMLElement('form_sheld');

    const liftRoot = document.createElement('div');
    liftRoot.setAttribute(LIFT_ATTR, '');

    const spacer = document.createElement('div');
    spacer.setAttribute(SPACER_ATTR, '');
    spacer.setAttribute('aria-hidden', 'true');

    // Keep Android IME layout on host-private children so theme CSS can keep styling
    // #form_sheld without overriding the actual keyboard offset contract.
    while (formShell.firstChild) {
        moveNodeIntoLiftRoot(formShell.firstChild, liftRoot);
    }

    formShell.append(liftRoot, spacer);

    const observer = new MutationObserver((records) => {
        for (const record of records) {
            for (const addedNode of record.addedNodes) {
                moveNodeIntoLiftRoot(addedNode, liftRoot);
            }
        }
    });

    observer.observe(formShell, { childList: true });

    const controller = {
        dispose() {
            observer.disconnect();
            delete window[CONTROLLER_KEY];
        },
    };

    window[CONTROLLER_KEY] = controller;
    return controller;
}
