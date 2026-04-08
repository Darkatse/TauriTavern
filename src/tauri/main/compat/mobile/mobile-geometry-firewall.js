const STYLE_ID = 'tt-mobile-geometry-firewall';

// NOTE: This layer intentionally contains only "core geometry" rules.
// Themes keep skinning freedom (colors, borders, shadows), but must not own
// mobile safe-area/viewport geometry for first-party shells.
//
// Why this file (and why it lives in the host layer):
// - iOS (WKWebView) runs edge-to-edge with `viewport-fit=cover`, so safe-area must be
//   explicitly consumed by first-party shells.
// - SillyTavern's upstream CSS uses a width breakpoint for "mobile styles". On iPad
//   landscape (and Stage Manager wide windows), that breakpoint is commonly not met,
//   so the app falls back to desktop layout while still being a mobile runtime.
// - We keep the desktop layout on iPad, but still apply the safe-area top offset via
//   the existing `--tt-inset-*` contract. This keeps behavior predictable, minimal,
//   and easy to maintain when syncing upstream CSS.
const FIREWALL_CSS = `
/* [TauriTavern] Mobile geometry firewall (host-last) */
@media screen and (max-width: 1000px) {
  /* Viewport root contract (mobile):
   * - Ensure documentElement has a non-zero, stable layout size.
   * - Avoid root transforms that would turn <html> into a fixed containing block.
   *
   * This is a prerequisite for third-party fixed overlays (fullscreen dialogs,
   * runtime hosts, etc.) to behave consistently across WebViews.
   */
  html,
  body {
    height: var(--tt-base-viewport-height, var(--doc-height, 100vh)) !important;
    min-height: var(--tt-base-viewport-height, var(--doc-height, 100vh)) !important;
  }

  html {
    -webkit-transform: none !important;
    transform: none !important;
    -webkit-perspective: none !important;
    perspective: none !important;
    -webkit-backface-visibility: hidden !important;
    backface-visibility: hidden !important;
  }

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

  body #completion_prompt_manager_popup {
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

  /* Scroll reachability contract (mobile):
   * Fixed shells commonly scroll inside panels/drawers. Ensure the last interactive
   * row can be scrolled above the iOS bottom safe-area instead of being covered
   * by the home indicator region.
   */
  body .drawer-content.openDrawer::after,
  body #character_popup::after,
  body #right-nav-panel > .scrollableInner::after,
  body #completion_prompt_manager_popup::after {
    content: '' !important;
    display: block !important;
    height: max(var(--tt-inset-bottom), 0px) !important;
    flex: 0 0 auto !important;
    pointer-events: none !important;
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

  body [data-tt-mobile-surface="viewport-host"][data-tt-mobile-surface][data-tt-mobile-surface] {
    top: 0 !important;
    left: 0 !important;
    right: 0 !important;
    bottom: 0 !important;
    width: 100vw !important;
    width: 100dvw !important;
    height: var(--tt-base-viewport-height, var(--doc-height, 100vh)) !important;
    height: var(--tt-base-viewport-height, var(--doc-height, 100dvh)) !important;
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

/* iPad wide screens (desktop layout + safe-area contract).
 *
 * When the viewport width exceeds 1000px, upstream "mobile" CSS does not apply.
 * On iOS we still run edge-to-edge, so first-party fixed/absolute shells must
 * consume --tt-inset-top to avoid falling under the status bar.
 *
 * This block intentionally keeps the desktop layout (centered --sheldWidth, no
 * 100vw mobile takeover). Only the safe-area top offset + corresponding height
 * adjustments are enforced.
 */
@media screen and (min-width: 1001px) {
  /* Keep a single source of truth for the top safe-area offset in this mode.
   * This is host-private plumbing, not public ABI.
   */
  body {
    --tt-safe-top: max(var(--tt-inset-top), 0px);
  }

  html,
  body {
    height: var(--tt-base-viewport-height, var(--doc-height, 100vh)) !important;
    min-height: var(--tt-base-viewport-height, var(--doc-height, 100vh)) !important;
  }

  html {
    -webkit-transform: none !important;
    transform: none !important;
    -webkit-perspective: none !important;
    perspective: none !important;
    -webkit-backface-visibility: hidden !important;
    backface-visibility: hidden !important;
  }

  body #top-bar {
    top: var(--tt-safe-top) !important;
  }

  body #top-settings-holder {
    margin-top: var(--tt-safe-top) !important;
  }

  body #sheld {
    top: calc(var(--topBarBlockSize) + var(--tt-safe-top)) !important;
    height: calc(var(--tt-base-viewport-height, var(--doc-height, 100vh)) - var(--topBarBlockSize) - var(--tt-safe-top) - 1px) !important;
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height, 100vh)) - var(--topBarBlockSize) - var(--tt-safe-top) - 1px) !important;
  }

  body #character_popup {
    top: calc(var(--topBarBlockSize) + var(--tt-safe-top)) !important;
    height: calc(var(--tt-base-viewport-height, var(--doc-height, 100vh)) - var(--topBarBlockSize) - var(--tt-safe-top)) !important;
    min-height: calc(var(--tt-base-viewport-height, var(--doc-height, 100vh)) - var(--topBarBlockSize) - var(--tt-safe-top)) !important;
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height, 100vh)) - var(--topBarBlockSize) - var(--tt-safe-top)) !important;
  }

  body #completion_prompt_manager_popup {
    top: calc(var(--topBarBlockSize) + var(--tt-safe-top)) !important;
    height: calc(100% - var(--topBarBlockSize) - var(--tt-safe-top)) !important;
  }

  body #top-settings-holder > .drawer > .drawer-content:not(.fillLeft):not(.fillRight) {
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height, 100vh)) - var(--topBarBlockSize) - var(--bottomFormBlockSize) - var(--tt-safe-top)) !important;
  }

  body .fillLeft,
  body .fillRight {
    top: var(--tt-safe-top) !important;
    max-height: calc(var(--tt-base-viewport-height, var(--doc-height, 100vh)) - var(--topBarBlockSize) - var(--tt-safe-top)) !important;
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
