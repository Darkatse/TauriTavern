// @ts-check

import { translate } from '../../i18n.js';
import { PARAM_HINTS } from './catalog.js';

/**
 * Question mark with a one-line explanation: hover shows it, a tap or click
 * (touch screens have no hover) repeats it as a toast.
 * @param {string} key
 * @returns {HTMLElement | null}
 */
function createHint(key) {
    const source = PARAM_HINTS[key];
    if (!source) return null;
    const text = translate(source);
    const hint = document.createElement('button');
    hint.type = 'button';
    hint.className = 'tt-gp-hint';
    hint.title = hint.ariaLabel = text;
    hint.innerHTML = '<i class="fa-solid fa-circle-question" aria-hidden="true"></i>';
    hint.addEventListener('click', event => {
        // Inside a label or drawer header the click would also toggle it.
        event.preventDefault();
        event.stopPropagation();
        toastr.info(text);
    });
    return hint;
}

/** Upstream help icons that already carry an explanation as their tooltip. */
const UPSTREAM_HINT_SELECTOR = '.fa-circle-info[title], .fa-circle-question[title], .note-link-span[title]';
/** Upstream question marks that only link to docs; the hint replaces them so each setting has one. */
const UPSTREAM_DOC_LINK_SELECTOR = 'a.fa-circle-question:not([title]), a.note-link-span:not([title])';

/**
 * Put the hint right after the label text: inside a checkbox label so it stays
 * on the label's line, otherwise next to the label. Skipped where upstream
 * already explains the setting in its own tooltip.
 * @param {Element | null} label
 * @param {string} key
 * @returns {Element | null} The element explaining the setting (ours or upstream's), if any
 */
export function placeHint(label, key) {
    if (!label) return null;
    const title = label.closest('.range-block-title, .inline-drawer-header') ?? label;
    const upstream = title.querySelector(UPSTREAM_HINT_SELECTOR);
    if (upstream) return upstream;
    const hint = createHint(key);
    if (!hint) return null;
    title.querySelector(UPSTREAM_DOC_LINK_SELECTOR)?.classList.add('tt-hidden');
    if (label instanceof HTMLLabelElement) label.append(hint); else label.after(hint);
    return hint;
}

/**
 * Once the hint explains a block, its own long descriptions are redundant.
 * Descriptions of nested sub-settings (e.g. the tool-call recursion limit)
 * stay, since the hint does not cover them.
 * @param {HTMLElement} block
 */
export function hideOwnDescriptions(block) {
    for (const description of block.querySelectorAll('.toggle-description')) {
        const owner = description.parentElement?.closest('.range-block');
        if (!owner || owner === block || !block.contains(owner)) description.classList.add('tt-hidden');
    }
}
