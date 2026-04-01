const STYLE_ID = 'tt-mobile-geometry-firewall';

// NOTE: This layer intentionally contains only "core geometry" rules.
// Themes keep skinning freedom (colors, borders, shadows), but must not own
// mobile safe-area/viewport geometry for first-party shells.
const FIREWALL_CSS = `
/* [TauriTavern] Mobile geometry firewall (host-last) */
@media screen and (max-width: 1000px) {
  body #top-settings-holder,
  body #top-bar {
    position: fixed !important;
    top: max(var(--tt-inset-top), 0px) !important;
    margin-top: 0 !important;
    left: 0 !important;
    right: 0 !important;
    width: 100vw !important;
    width: 100dvw !important;
    padding-right: max(var(--tt-inset-right), 0px) !important;
    padding-left: max(var(--tt-inset-left), 0px) !important;
  }

  body #top-settings-holder > .drawer > .drawer-content:not(.fillLeft):not(.fillRight) {
    position: absolute !important;
    top: var(--topBarBlockSize) !important;
    left: 0 !important;
    right: 0 !important;
    width: auto !important;
    max-width: none !important;
    margin-top: 0 !important;
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height)) - var(--topBarBlockSize) - max(var(--tt-inset-top), 0px)) !important;
  }

  body #sheld,
  body #character_popup {
    top: calc(var(--topBarBlockSize) + max(var(--tt-inset-top), 0px)) !important;
    height: calc(var(--tt-base-viewport-height, var(--doc-height)) - var(--topBarBlockSize) - max(var(--tt-inset-top), 0px)) !important;
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height)) - var(--topBarBlockSize) - max(var(--tt-inset-top), 0px)) !important;
  }

  body #form_sheld {
    position: relative !important;
    left: auto !important;
    right: auto !important;
    bottom: auto !important;
    transform: none !important;
    padding-right: max(var(--tt-inset-right), 0px) !important;
    padding-left: max(var(--tt-inset-left), 0px) !important;
    padding-bottom: var(--tt-bottom-inset) !important;
  }

  /* NOTE: Repeat attribute selectors to beat typical framework-scoped CSS (e.g. Vue scoped + !important). */
  body [data-tt-mobile-surface="edge-window"][data-tt-mobile-surface][data-tt-mobile-surface] {
    top: max(var(--tt-inset-top), var(--tt-original-top, 0px)) !important;
  }

  body [data-tt-mobile-surface="fullscreen-window"][data-tt-mobile-surface][data-tt-mobile-surface] {
    top: max(var(--tt-inset-top), 0px) !important;
    left: max(var(--tt-inset-left), 0px) !important;
    right: max(var(--tt-inset-right), 0px) !important;
    bottom: max(var(--tt-viewport-bottom-inset, var(--tt-inset-bottom)), 0px) !important;
    width: auto !important;
    height: auto !important;
    max-width: none !important;
    max-height: none !important;
  }

  body [data-tt-ime-surface="fixed-shell"][data-tt-ime-active] {
    --tt-bottom-inset: max(var(--tt-inset-bottom), 0px);
    --tt-viewport-bottom-inset-local: max(var(--tt-bottom-inset), var(--tt-ime-bottom));
    --tt-keyboard-offset: max(calc(var(--tt-viewport-bottom-inset-local) - var(--tt-bottom-inset)), 0px);
    scroll-padding-bottom: var(--tt-keyboard-offset) !important;
  }

  body #character_popup[data-tt-ime-surface="fixed-shell"][data-tt-ime-active] {
    height: calc(var(--tt-base-viewport-height, var(--doc-height)) - var(--topBarBlockSize) - max(var(--tt-inset-top), 0px) - var(--tt-keyboard-offset)) !important;
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height)) - var(--topBarBlockSize) - max(var(--tt-inset-top), 0px) - var(--tt-keyboard-offset)) !important;
  }

  body .drawer-content[data-tt-ime-surface="fixed-shell"][data-tt-ime-active] {
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height)) - var(--topBarBlockSize) - max(var(--tt-inset-top), 0px) - var(--tt-keyboard-offset)) !important;
  }

  body #top-settings-holder > .drawer > .drawer-content[data-tt-ime-surface="fixed-shell"][data-tt-ime-active]:not(.fillLeft):not(.fillRight) {
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height)) - var(--topBarBlockSize) - max(var(--tt-inset-top), 0px) - var(--tt-keyboard-offset)) !important;
  }

  body [data-tt-mobile-surface="fullscreen-window"][data-tt-mobile-surface][data-tt-mobile-surface][data-tt-ime-surface="fixed-shell"][data-tt-ime-active] {
    bottom: max(var(--tt-viewport-bottom-inset-local), 0px) !important;
  }
}
`.trim();

function requireHead() {
    const { head } = document;
    if (!(head instanceof HTMLHeadElement)) {
        throw new Error('[TauriTavern] document.head unavailable while installing mobile geometry firewall.');
    }
    return head;
}

function requireStyleElement() {
    const existing = document.getElementById(STYLE_ID);
    if (!existing) {
        const style = document.createElement('style');
        style.id = STYLE_ID;
        style.type = 'text/css';
        return style;
    }

    if (!(existing instanceof HTMLStyleElement)) {
        throw new Error(`[TauriTavern] #${STYLE_ID} is not a <style> element.`);
    }

    return existing;
}

export function installMobileGeometryFirewall() {
    if (typeof MutationObserver !== 'function') {
        throw new Error('[TauriTavern] MutationObserver unavailable while installing mobile geometry firewall.');
    }

    const head = requireHead();
    const style = requireStyleElement();
    style.textContent = FIREWALL_CSS;

    const ensureLast = () => {
        if (!style.isConnected || head.lastElementChild !== style) {
            head.appendChild(style);
        }
    };

    ensureLast();

    const observer = new MutationObserver(ensureLast);
    observer.observe(head, { childList: true });

    const controller = {
        dispose() {
            observer.disconnect();
            if (style.isConnected) {
                style.remove();
            }
        },
        ensureLast,
    };

    return controller;
}
