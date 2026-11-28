// The host draws the top-level drawer buttons in a system bar region; the page keeps the drawers.
import { getTopLevelDrawerPanel, isTopLevelDrawerOpen, subscribeDrawerState } from './drawers.js';
import { subscribeWindowSnapshot } from './util/window-layout.js';

// Extensions add drawers, and `/panels` hides the holder.
const HOLDER_OBSERVATION = { childList: true, attributes: true, attributeFilter: ['style', 'class'] };
// Labels and marks have many writers. No aria-* here: drawers.js rewrites it on every lookup.
const TOGGLE_OBSERVATION = { subtree: true, attributes: true, attributeFilter: ['class', 'title', 'data-tt-assistant-activity'] };

/** @type {Map<string, HTMLElement>} */
const controls = new Map();
/** @type {WeakSet<HTMLElement>} */
const trackedToggles = new WeakSet();
/** @type {WeakSet<HTMLElement>} */
const trackedPanels = new WeakSet();
const observer = new MutationObserver(refresh);
let invoke = null;
let active = false;
let scheduled = false;
let published = null;

export function installDrawerBar(context) {
    invoke = context.safeInvoke;
    subscribeWindowSnapshot((snapshot) => {
        const edge = snapshot.drawerBar?.edge;
        active = Boolean(edge);
        if (active) {
            document.documentElement.dataset.ttDrawerBar = edge;
            observer.observe(document.getElementById('top-settings-holder'), HOLDER_OBSERVATION);
            refresh();
        } else {
            delete document.documentElement.dataset.ttDrawerBar;
        }
    });
    window.addEventListener('tt-drawer-bar-activate', (event) => {
        // Like the toggles they stand for, native buttons are inert behind a modal.
        if (document.querySelector(':modal')) return;
        const control = controls.get(event.detail.id);
        if (!control) {
            console.warn('[TauriTavern] Drawer bar activated an unknown drawer:', event.detail.id);
            return;
        }
        // Icon handlers persist panel state; the click then bubbles to the toggle.
        control.click();
    });
}

function refresh() {
    if (!active || scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
        scheduled = false;
        publish().catch(error => console.warn('[TauriTavern] Drawer bar:', error));
    });
}

async function publish() {
    controls.clear();
    const items = [];
    // Every toggle the stylesheet hides gets a native button, including drawers without a panel.
    for (const drawer of document.querySelectorAll('#top-settings-holder > .drawer')) {
        const toggle = drawer.querySelector(':scope > .drawer-toggle');
        if (!toggle || !drawer.checkVisibility()) continue;
        const panel = getTopLevelDrawerPanel(toggle);
        if (!trackedToggles.has(toggle)) {
            trackedToggles.add(toggle);
            observer.observe(toggle, TOGGLE_OBSERVATION);
        }
        if (panel && !trackedPanels.has(panel)) {
            trackedPanels.add(panel);
            subscribeDrawerState(panel, refresh);
        }
        const icon = toggle.querySelector('.drawer-icon');
        const id = drawer.id || `drawer-${items.length}`;
        controls.set(id, icon ?? toggle);
        items.push({
            id,
            title: icon?.title || toggle.title,
            open: panel ? isTopLevelDrawerOpen(panel) : false,
            mark: markOf(icon),
        });
    }
    const key = JSON.stringify(items);
    if (key === published) return;
    published = key;
    try {
        await invoke('set_drawer_bar_items', { items });
    } catch (error) {
        // A failed publication may retry, without invalidating a newer one.
        if (published === key) published = null;
        throw error;
    }
}

/** The page marks drawers with the API status glow and the assistant's activity badge. */
function markOf(icon) {
    if (icon?.classList.contains('redOverlayGlow')) return 'alert';
    return icon?.dataset.ttAssistantActivity || null;
}
