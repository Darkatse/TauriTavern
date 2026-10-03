// @ts-check

import { Popper } from '../../../lib.js';
import { SURFACE, applySurface } from '../../tauritavern/layout-kit.js';

const STYLE_ID = 'tauritavern-compact-rows-style';

/**
 * Loads the shared compact-row and overflow-menu styles once. They also define
 * `.tt-hidden`, so any module using that class calls this first.
 */
export function ensureCompactRowsStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const link = document.createElement('link');
    link.id = STYLE_ID;
    link.rel = 'stylesheet';
    link.href = new URL('./compact-rows.css', import.meta.url).href;
    document.head.append(link);
}

/**
 * @typedef {object} OverflowMenuItem
 * @property {string} label Visible text (already translated)
 * @property {string | Element} icon Font Awesome icon name without the `fa-` prefix, or an icon element to copy
 * @property {() => void} onSelect Runs after the menu closes
 * @property {boolean} [danger] Destructive action, drawn in the warning color
 * @property {boolean} [separatorBefore] Draws a rule above the item
 * @property {() => boolean} [checked] Makes the item a checkbox reflecting this state
 * @property {string} [hint] Tooltip with a longer explanation
 */

/** @type {(() => void) | null} Closes the menu that is open, if any; one menu at a time. */
let closeOpenMenu = null;

/**
 * Lets extensions move their buttons from a compact row into its "⋯" menu: a button
 * marked `data-tt-overflow` inside `row` is hidden there (CSS, so buttons added later
 * are covered) and returned as a menu item. The item's label is the button's
 * `aria-label` or `title`, its icon a copy of the button's own `<i>`, `<svg>` or
 * `<img>`, and choosing it clicks the button. Disabled buttons are left out.
 * @param {HTMLElement} row
 * @returns {() => OverflowMenuItem[]} Reads the marked buttons, in document order; call it on every open
 */
export function adoptOverflowButtons(row) {
    row.classList.add('tt-overflow-host');
    return () => [...row.querySelectorAll('[data-tt-overflow]')].flatMap(button => {
        if (!(button instanceof HTMLElement) || button.matches(':disabled')) return [];
        const label = button.getAttribute('aria-label') || button.title;
        if (!label) {
            console.error('TauriTavern: a [data-tt-overflow] button needs an aria-label or title to be listed in the ⋯ menu', button);
            return [];
        }
        return [{ label, icon: button.querySelector('i, svg, img') ?? 'puzzle-piece', onSelect: () => button.click() }];
    });
}

/**
 * A "⋯" button whose menu holds a row's less frequent actions. Tap or click to open;
 * an outside press, Escape, Tab, choosing an item, or scrolling / resizing that moves
 * the button closes it, so touch screens (no hover) behave like mouse. Scrolling
 * elsewhere, such as the chat while a reply streams, leaves it open. The menu lives
 * on <body> so drawers cannot clip it.
 * @param {object} options
 * @param {string} options.title Button tooltip and accessible name
 * @param {() => OverflowMenuItem[]} options.items Read on every open, so states stay current
 * @returns {HTMLButtonElement}
 */
export function createOverflowMenuButton({ title, items }) {
    ensureCompactRowsStyle();
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'menu_button menu_button_icon tt-row-button tt-overflow-button';
    button.title = button.ariaLabel = title;
    button.setAttribute('aria-haspopup', 'menu');
    button.setAttribute('aria-expanded', 'false');
    button.innerHTML = '<i class="fa-fw fa-solid fa-ellipsis" aria-hidden="true"></i>';

    button.addEventListener('click', (event) => {
        event.stopPropagation();
        const wasOpen = button.getAttribute('aria-expanded') === 'true';
        closeOpenMenu?.();
        if (!wasOpen) open(button, items());
    });
    return button;
}

/**
 * @param {string | Element} icon
 * @returns {HTMLElement}
 */
function renderIcon(icon) {
    const slot = document.createElement('span');
    slot.className = 'tt-overflow-icon';
    slot.setAttribute('aria-hidden', 'true');
    if (typeof icon === 'string') {
        const glyph = document.createElement('i');
        glyph.className = `fa-fw fa-solid fa-${icon}`;
        slot.append(glyph);
    } else {
        const copy = /** @type {Element} */ (icon.cloneNode(true));
        for (const node of [copy, ...copy.querySelectorAll('[id]')]) node.removeAttribute('id');
        slot.append(copy);
    }
    return slot;
}

/**
 * Elements whose scrolling moves `element`; the window is handled separately.
 * @param {Element} element
 * @returns {Element[]}
 */
function scrollAncestorsOf(element) {
    const found = [];
    for (let node = element.parentElement; node; node = node.parentElement) {
        const { overflowX, overflowY } = getComputedStyle(node);
        if (/auto|scroll|overlay/.test(`${overflowX} ${overflowY}`)) found.push(node);
    }
    return found;
}

/**
 * @param {HTMLButtonElement} button
 * @param {OverflowMenuItem[]} items
 */
function open(button, items) {
    const menu = document.createElement('div');
    menu.className = 'list-group tt-overflow-menu';
    menu.setAttribute('role', 'menu');

    let close = () => {};
    for (const item of items) {
        if (item.separatorBefore) {
            const rule = document.createElement('div');
            rule.className = 'tt-overflow-rule';
            rule.setAttribute('role', 'separator');
            menu.append(rule);
        }
        const checked = item.checked ? item.checked() : null;
        const entry = document.createElement('div');
        entry.className = 'list-group-item tt-overflow-item';
        entry.tabIndex = -1;
        entry.classList.toggle('tt-overflow-danger', Boolean(item.danger));
        entry.setAttribute('role', checked === null ? 'menuitem' : 'menuitemcheckbox');
        if (checked !== null) entry.setAttribute('aria-checked', String(checked));
        if (item.hint) entry.title = item.hint;
        const text = document.createElement('span');
        text.textContent = item.label;
        entry.append(renderIcon(checked === null ? item.icon : checked ? 'square-check' : 'square'), text);
        entry.addEventListener('click', (event) => {
            event.stopPropagation();
            close();
            item.onSelect();
        });
        menu.append(entry);
    }
    // The menu sits on <body>, outside the drawer it belongs to: keep SillyTavern's
    // "press outside closes drawers" handler off it, and keep the mobile geometry
    // firewall from treating it as a top-edge window (Popper places it).
    menu.addEventListener('mousedown', event => event.stopPropagation());
    menu.addEventListener('touchstart', event => event.stopPropagation(), { passive: true });
    applySurface(menu, SURFACE.None);
    document.body.append(menu);

    const popper = Popper.createPopper(button, menu, {
        placement: 'bottom-end',
        strategy: 'fixed',
        modifiers: [{ name: 'flip', options: { fallbackPlacements: ['top-end'] } }],
    });
    const entries = () => /** @type {HTMLElement[]} */ ([...menu.querySelectorAll('.tt-overflow-item')]);
    /** @type {(Element | Window)[]} */
    const scrollTargets = [...scrollAncestorsOf(button), window];

    // `mousedown` too: the Android back button dispatches a synthetic one on <html>.
    const onPointerDown = (/** @type {Event} */ event) => {
        if (!menu.contains(/** @type {Node} */ (event.target)) && !button.contains(/** @type {Node} */ (event.target))) close();
    };
    const onKeyDown = (/** @type {KeyboardEvent} */ event) => {
        // Like a native menu, Tab also closes it and continues from the ⋯ button.
        if (event.key === 'Escape' || (event.key === 'Tab' && menu.contains(document.activeElement))) {
            close();
            button.focus();
        } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
            event.preventDefault();
            const list = entries();
            const index = list.indexOf(/** @type {HTMLElement} */ (document.activeElement));
            const step = event.key === 'ArrowDown' ? 1 : -1;
            list[(index + step + list.length) % list.length]?.focus();
        } else if (event.key === 'Enter' && menu.contains(document.activeElement)) {
            event.preventDefault();
            /** @type {HTMLElement} */ (document.activeElement).click();
        }
    };
    const onFocusOut = (/** @type {FocusEvent} */ event) => {
        const next = event.relatedTarget;
        if (next instanceof Node && !menu.contains(next) && !button.contains(next)) close();
    };
    const onMoved = () => close();

    let closed = false;
    close = () => {
        if (closed) return;
        closed = true;
        document.removeEventListener('pointerdown', onPointerDown, true);
        document.removeEventListener('mousedown', onPointerDown, true);
        document.removeEventListener('keydown', onKeyDown, true);
        menu.removeEventListener('focusout', onFocusOut);
        for (const target of scrollTargets) target.removeEventListener('scroll', onMoved);
        window.removeEventListener('resize', onMoved);
        popper.destroy();
        menu.remove();
        button.setAttribute('aria-expanded', 'false');
        if (closeOpenMenu === close) closeOpenMenu = null;
    };
    closeOpenMenu = close;

    document.addEventListener('pointerdown', onPointerDown, true);
    document.addEventListener('mousedown', onPointerDown, true);
    document.addEventListener('keydown', onKeyDown, true);
    menu.addEventListener('focusout', onFocusOut);
    for (const target of scrollTargets) target.addEventListener('scroll', onMoved, { passive: true });
    window.addEventListener('resize', onMoved);
    button.setAttribute('aria-expanded', 'true');
    entries()[0]?.focus({ preventScroll: true });
}
