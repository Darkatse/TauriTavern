// @ts-check

import { cancelInlineDrawerMotion, setInlineDrawerContentOpen } from './tauri/perf/inline-drawer-motion.js';

const STATE_CHANGE = 'tt-drawer-state-change';
/** @typedef {{ toggle: HTMLElement; icon: HTMLElement | null; control: HTMLElement }} TopLevelParts */
/** @typedef {{ content: HTMLElement; icon: HTMLElement; control: HTMLElement }} InlineParts */
/** @type {WeakMap<HTMLElement, TopLevelParts>} */
const topLevelParts = new WeakMap();
/** @type {WeakMap<HTMLElement, HTMLElement>} */
const panelByToggle = new WeakMap();
/** @type {WeakMap<HTMLElement, boolean>} */
const projectedTopLevelState = new WeakMap();
/** @type {WeakMap<HTMLElement, boolean>} */
const projectedInlineState = new WeakMap();
/** @type {WeakMap<HTMLElement, HTMLElement>} */
const inlineDrawerByIcon = new WeakMap();
/** @type {MutationObserver | undefined} */
let stateObserver;
/** @type {Document | undefined} */
let initializedDocument;
let nextContentId = 0;

/** @param {HTMLElement} element */
function isInstance(element) {
    return element.isConnected && !element.closest('.template_element, template');
}

/** @param {HTMLElement} content */
function contentId(content) {
    // Templates and clones acquire their associations only once they are instances.
    if (!content.id || content.ownerDocument.getElementById(content.id) !== content) {
        let id;
        do { id = `tt-drawer-content-${++nextContentId}`; }
        while (content.ownerDocument.getElementById(id));
        content.id = id;
    }
    return content.id;
}

/** @param {HTMLElement | null} control @param {HTMLElement} content @param {boolean} open */
function projectDisclosure(control, content, open) {
    if (!control || !isInstance(content)) return;
    control.setAttribute('aria-controls', contentId(content));
    control.setAttribute('aria-expanded', String(open));
}

/** @param {HTMLElement} panel */
export function isTopLevelDrawerOpen(panel) {
    return panel.classList.contains('openDrawer');
}

/** @param {HTMLElement} panel */
function projectTopLevelDrawer(panel) {
    const open = isTopLevelDrawerOpen(panel);
    const parts = topLevelParts.get(panel);
    if (parts) {
        parts.icon?.classList.toggle('openIcon', open);
        parts.icon?.classList.toggle('closedIcon', !open);
        projectDisclosure(parts.control, panel, open);
    }
    const previous = projectedTopLevelState.get(panel);
    projectedTopLevelState.set(panel, open);
    if (previous !== undefined && previous !== open) {
        panel.dispatchEvent(new CustomEvent(STATE_CHANGE));
    }
}

/** @param {HTMLElement} drawer */
function initializeTopLevelDrawer(drawer) {
    if (!isInstance(drawer)) return;
    const toggle = drawer.querySelector(':scope > .drawer-toggle');
    if (!(toggle instanceof HTMLElement)) return;
    const panel = panelByToggle.get(toggle) ?? drawer.querySelector(':scope > .drawer-content');
    if (!(panel instanceof HTMLElement)) return;
    const icon = toggle.querySelector('.drawer-icon');
    const control = toggle.matches('button') ? toggle : icon instanceof HTMLElement ? icon : toggle;
    topLevelParts.set(panel, { toggle, icon: icon instanceof HTMLElement ? icon : null, control });
    panelByToggle.set(toggle, panel);
    if (!control.hasAttribute('aria-label') && !control.hasAttribute('aria-labelledby') && !control.title && toggle.title) {
        control.title = toggle.title;
    }
    projectTopLevelDrawer(panel);
    stateObserver?.observe(panel, { attributes: true, attributeFilter: ['class'] });
}

/** @param {HTMLElement | null | undefined} toggle @returns {HTMLElement | null} */
export function getTopLevelDrawerPanel(toggle) {
    if (!(toggle instanceof HTMLElement)) return null;
    initDrawers();
    const drawer = toggle.closest('.drawer');
    if (drawer instanceof HTMLElement) initializeTopLevelDrawer(drawer);
    return panelByToggle.get(toggle) ?? null;
}

/** @param {HTMLElement} panel @param {boolean} open */
export function setTopLevelDrawerOpen(panel, open) {
    initDrawers();
    panel.classList.toggle('openDrawer', open);
    panel.classList.toggle('closedDrawer', !open);
    // Existing extensions close core panels with an inline display override.
    if (open && panel.style.display === 'none') panel.style.removeProperty('display');
    projectTopLevelDrawer(panel);
}

/** @param {HTMLElement} panel @param {() => void} listener */
export function subscribeDrawerState(panel, listener) {
    initDrawers();
    projectTopLevelDrawer(panel);
    stateObserver?.observe(panel, { attributes: true, attributeFilter: ['class'] });
    panel.addEventListener(STATE_CHANGE, listener);
    return () => panel.removeEventListener(STATE_CHANGE, listener);
}

/** @param {HTMLElement} drawer @returns {InlineParts | null} */
function getInlineParts(drawer) {
    const content = drawer.querySelector(':scope > .inline-drawer-content');
    if (!(content instanceof HTMLElement)) return null;
    const toggle = Array.from(drawer.querySelectorAll('.inline-drawer-toggle'))
        .find(element => element.closest('.inline-drawer') === drawer);
    const icon = toggle?.matches('.inline-drawer-icon') ? toggle : toggle?.querySelector('.inline-drawer-icon');
    if (!(toggle instanceof HTMLElement) || !(icon instanceof HTMLElement)) return null;
    return {
        content,
        icon,
        control: toggle.matches('button') ? toggle : icon,
    };
}

/** @param {InlineParts} parts */
function readInlineState(parts) {
    return parts.icon.classList.contains('up');
}

/** @param {HTMLElement} drawer */
export function isInlineDrawerOpen(drawer) {
    const parts = getInlineParts(drawer);
    return parts ? readInlineState(parts) : false;
}

/** @param {HTMLElement} drawer @param {InlineParts} parts */
function projectInlineDrawer(drawer, parts) {
    const open = readInlineState(parts);
    if (projectedInlineState.has(drawer) && projectedInlineState.get(drawer) !== open) {
        cancelInlineDrawerMotion(parts.content);
    }
    projectDisclosure(parts.control, parts.content, open);
    projectedInlineState.set(drawer, open);
}

/** @param {HTMLElement} drawer */
function initializeInlineDrawer(drawer) {
    if (!isInstance(drawer)) return;
    const parts = getInlineParts(drawer);
    if (!parts) return;
    projectInlineDrawer(drawer, parts);
    inlineDrawerByIcon.set(parts.icon, drawer);
    stateObserver?.observe(parts.icon, { attributes: true, attributeFilter: ['class'] });
}

/** @param {HTMLElement} drawer @param {InlineParts} parts @param {boolean} open */
function commitInlineState(drawer, parts, open) {
    parts.icon.classList.toggle('up', open);
    parts.icon.classList.toggle('down', !open);
    parts.icon.classList.toggle('fa-circle-chevron-up', open);
    parts.icon.classList.toggle('fa-circle-chevron-down', !open);
    projectDisclosure(parts.control, parts.content, open);
    projectedInlineState.set(drawer, open);
}

/**
 * Animated user activation notifies lazy editors before changing display.
 * @param {HTMLElement} drawer
 * @param {number} durationMs
 */
export function toggleInlineDrawer(drawer, durationMs) {
    initDrawers();
    initializeInlineDrawer(drawer);
    const parts = getInlineParts(drawer);
    if (!parts) return false;
    const open = !readInlineState(parts);
    cancelInlineDrawerMotion(parts.content);
    commitInlineState(drawer, parts, open);
    drawer.dispatchEvent(new CustomEvent('inline-drawer-toggle', { bubbles: true, detail: { open } }));
    const currentOpen = isInlineDrawerOpen(drawer);
    if (currentOpen !== open) return currentOpen;
    const motion = setInlineDrawerContentOpen(parts.content, open, { durationMs });
    void motion.then(finished => {
        if (finished && isInlineDrawerOpen(drawer) === open) {
            drawer.dispatchEvent(new CustomEvent('inline-drawer-motion-complete', { bubbles: true, detail: { open } }));
        }
    });
    return open;
}

/**
 * The public instant path historically notifies after display, without detail.
 * @param {HTMLElement} drawer
 * @param {boolean} open
 */
export function setInlineDrawerOpen(drawer, open) {
    initDrawers();
    initializeInlineDrawer(drawer);
    const parts = getInlineParts(drawer);
    if (!parts) return;
    commitInlineState(drawer, parts, open);
    void setInlineDrawerContentOpen(parts.content, open, { durationMs: 0 });
    drawer.dispatchEvent(new CustomEvent('inline-drawer-toggle', { bubbles: true }));
}

/** @param {Element} element */
function initializeDrawer(element) {
    if (!(element instanceof HTMLElement)) return;
    if (element.matches('.drawer')) initializeTopLevelDrawer(element);
    if (element.matches('.inline-drawer')) initializeInlineDrawer(element);
}

/** @param {Element} root */
function initializeSubtree(root) {
    initializeDrawer(root);
    root.querySelectorAll('.drawer, .inline-drawer').forEach(initializeDrawer);
}

/** Discover instances; observe only their state-bearing elements, including moved panels. */
export function initDrawers() {
    if (initializedDocument === document || !document.body) return;
    initializedDocument = document;
    stateObserver = new MutationObserver(mutations => {
        for (const { target } of mutations) {
            if (!(target instanceof HTMLElement)) continue;
            if (projectedTopLevelState.has(target)) projectTopLevelDrawer(target);
            const drawer = inlineDrawerByIcon.get(target);
            if (!drawer) continue;
            const parts = getInlineParts(drawer);
            if (!parts) continue;
            projectInlineDrawer(drawer, parts);
        }
    });
    initializeSubtree(document.body);
    const discoveryObserver = new MutationObserver(mutations => {
        for (const { addedNodes } of mutations) {
            for (const node of addedNodes) {
                if (!(node instanceof Element)) continue;
                const drawer = node.parentElement?.closest('.drawer, .inline-drawer');
                if (drawer) initializeDrawer(drawer);
                initializeSubtree(node);
            }
        }
    });
    discoveryObserver.observe(document.body, { childList: true, subtree: true });
}
