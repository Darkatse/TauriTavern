import { isAndroidRuntime } from '../../../../scripts/util/mobile-runtime.js';

const SURFACE_ATTR = 'data-tt-ime-surface';
const ACTIVE_ATTR = 'data-tt-ime-active';

const SURFACE_KIND = /** @type {const} */ ({
    Composer: 'composer',
    FixedShell: 'fixed-shell',
    Dialog: 'dialog',
});

const FIXED_SHELL_ROOT_SELECTOR = [
    '#character_popup',
    '#completion_prompt_manager_popup',
    '#top-settings-holder > .drawer > .drawer-content.openDrawer:not(.fillLeft):not(.fillRight)',
    '.drawer-content.openDrawer',
    '#floatingPrompt',
    '#cfgConfig',
    '#logprobsViewer',
    '#movingDivs > div',
].join(', ');

let installed = false;

function requireInsetsBridge() {
    const bridge = window.__TAURITAVERN_INSETS__;
    if (!bridge || typeof bridge.setImeTarget !== 'function') {
        throw new Error('[TauriTavern] Android insets bridge unavailable while routing IME target.');
    }
    return bridge;
}

function requireShed() {
    const sheld = document.getElementById('sheld');
    if (!(sheld instanceof HTMLElement)) {
        throw new Error('[TauriTavern] #sheld unavailable while routing IME target.');
    }
    return sheld;
}

function isEditable(element) {
    if (!(element instanceof HTMLElement)) {
        return false;
    }

    if (element instanceof HTMLTextAreaElement) {
        return true;
    }

    if (element instanceof HTMLInputElement) {
        return true;
    }

    return element.isContentEditable;
}

function resolveImeSurfaceRoot(editable) {
    const sheld = requireShed();

    if (sheld.contains(editable)) {
        return { root: sheld, kind: SURFACE_KIND.Composer };
    }

    const dialog = editable.closest('dialog.popup[open]');
    if (dialog instanceof HTMLElement) {
        return { root: dialog, kind: SURFACE_KIND.Dialog };
    }

    const dialoguePopup = editable.closest('#dialogue_popup');
    if (dialoguePopup instanceof HTMLElement) {
        return { root: dialoguePopup, kind: SURFACE_KIND.Dialog };
    }

    const overlaySurface = editable.closest('[data-tt-mobile-surface]');
    if (overlaySurface instanceof HTMLElement) {
        return { root: overlaySurface, kind: SURFACE_KIND.FixedShell };
    }

    const fixedShellRoot = editable.closest(FIXED_SHELL_ROOT_SELECTOR);
    if (fixedShellRoot instanceof HTMLElement) {
        return { root: fixedShellRoot, kind: SURFACE_KIND.FixedShell };
    }

    return { root: sheld, kind: SURFACE_KIND.Composer };
}

function setActiveSurface(previous, next, kind) {
    if (previous === next) {
        if (previous && previous.getAttribute(SURFACE_ATTR) !== kind) {
            previous.setAttribute(SURFACE_ATTR, kind);
        }
        return;
    }

    if (previous) {
        previous.removeAttribute(ACTIVE_ATTR);
        previous.removeAttribute(SURFACE_ATTR);
    }

    if (next) {
        next.setAttribute(ACTIVE_ATTR, '');
        next.setAttribute(SURFACE_ATTR, kind);
    }
}

export function installMobileImeSurfaceController() {
    if (!isAndroidRuntime()) {
        return null;
    }

    if (installed) {
        return null;
    }
    installed = true;

    let activeSurface = null;
    let activeKind = SURFACE_KIND.Composer;
    let desiredImeTarget = null;

    const applyRouting = (editableOrNull) => {
        if (!editableOrNull) {
            setActiveSurface(activeSurface, null, activeKind);
            activeSurface = null;
            activeKind = SURFACE_KIND.Composer;

            if (desiredImeTarget !== null) {
                desiredImeTarget = null;
                requireInsetsBridge().setImeTarget(null);
            }
            return;
        }

        const { root, kind } = resolveImeSurfaceRoot(editableOrNull);
        setActiveSurface(activeSurface, root, kind);
        activeSurface = root;
        activeKind = kind;

        const nextImeTarget = root.id === 'sheld' ? null : root;
        if (desiredImeTarget !== nextImeTarget) {
            desiredImeTarget = nextImeTarget;
            requireInsetsBridge().setImeTarget(nextImeTarget);
        }
    };

    const onFocusIn = (event) => {
        const target = event.target;
        if (!isEditable(target)) {
            return;
        }
        applyRouting(/** @type {HTMLElement} */ (target));
    };

    const onFocusOut = () => {
        Promise.resolve().then(() => {
            const nextActive = document.activeElement;
            if (isEditable(nextActive)) {
                return;
            }
            applyRouting(null);
        });
    };

    document.addEventListener('focusin', onFocusIn, true);
    document.addEventListener('focusout', onFocusOut, true);

    return {
        dispose() {
            document.removeEventListener('focusin', onFocusIn, true);
            document.removeEventListener('focusout', onFocusOut, true);
        },
    };
}
