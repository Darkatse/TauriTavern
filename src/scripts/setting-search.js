import { t } from './i18n.js';

/**
 * Search for settings that match the search string and highlight them.
 */
async function searchSettings() {
    removeHighlighting(); // Remove previous highlights
    const searchString = String($('#settingsSearch').val());
    const searchableText = $('#user-settings-block-content'); // Get the HTML block
    if (searchString.trim() !== '') {
        highlightMatchingElements(searchableText[0], searchString); // Highlight matching elements
    }

    const settingsSearchStatus = document.getElementById('settingsSearchStatus');
    if (!(settingsSearchStatus instanceof HTMLElement)) {
        throw new Error('Settings search status live region is missing');
    }

    const highlightedCount = document.querySelectorAll('#user-settings-block-content .highlighted').length;
    settingsSearchStatus.textContent = highlightedCount === 1 ? t`1 matching setting.` : t`${highlightedCount} matching settings.`;
}

const SETTINGS_RANGE_SELECTOR = [
    '#amount_gen',
    '#max_context',
    '#chat_width_slider',
    '#font_scale',
    '#blur_strength',
    '#shadow_width',
].join(', ');

/**
 * @param {string} id
 * @returns {HTMLElement}
 */
function requireElementById(id) {
    const element = document.getElementById(id);
    if (!(element instanceof HTMLElement)) {
        throw new Error(`Required settings control is missing: #${id}`);
    }
    return element;
}

/**
 * @param {HTMLInputElement} range
 */
function syncRangeValueText(range) {
    range.setAttribute('aria-valuetext', String(range.value));
}

/**
 * @param {string} sliderId
 * @param {string} counterId
 * @param {string} label
 */
function syncRangePair(sliderId, counterId, label) {
    const slider = requireElementById(sliderId);
    const counter = requireElementById(counterId);
    if (!(slider instanceof HTMLInputElement) || slider.type !== 'range') {
        throw new Error(`Settings range slider must be an input[type="range"]: #${sliderId}`);
    }
    if (!(counter instanceof HTMLInputElement)) {
        throw new Error(`Settings range counter must be an input: #${counterId}`);
    }

    slider.setAttribute('aria-label', label);
    counter.setAttribute('aria-label', t`${label} value`);
    syncRangeValueText(slider);
}

function initSettingsFormAccessibility() {
    syncRangePair('amount_gen', 'amount_gen_counter', t`Response tokens`);
    syncRangePair('max_context', 'max_context_counter', t`Context tokens`);
    syncRangePair('chat_width_slider', 'chat_width_slider_counter', t`Chat width`);
    syncRangePair('font_scale', 'font_scale_counter', t`Font scale`);
    syncRangePair('blur_strength', 'blur_strength_counter', t`Blur strength`);
    syncRangePair('shadow_width', 'shadow_width_counter', t`Text shadow width`);

    $(document).on('input change', SETTINGS_RANGE_SELECTOR, function () {
        if (!(this instanceof HTMLInputElement) || this.type !== 'range') {
            throw new Error('Settings range event target must be an input[type="range"]');
        }
        syncRangeValueText(this);
    });
}

/**
 * Check if the element is a child of a header element
 * @param {HTMLElement | Text | Document | Comment} element Settings block HTML element
 * @returns {boolean} True if the element is a child of a header element, false otherwise
 */
function isParentHeader(element) {
    return $(element).closest('h4, h3').length > 0;
}

/**
 * Recursively highlight elements that match the search string
 * @param {HTMLElement | Text | Document | Comment} element Settings block HTML element
 * @param {string} searchString Search string
 */
function highlightMatchingElements(element, searchString) {
    $(element).contents().each(function () {
        const isTextNode = this.nodeType === Node.TEXT_NODE;
        const isElementNode = this.nodeType === Node.ELEMENT_NODE;

        if (isTextNode && this.nodeValue.trim() !== '' && !isParentHeader(this)) {
            const parentElement = $(this).parent();
            const elementText = this.nodeValue;

            if (elementText.toLowerCase().includes(searchString.toLowerCase())) {
                parentElement.addClass('highlighted'); // Add CSS class to highlight matched elements
            }
        } else if (isElementNode && !$(this).is('h4')) {
            highlightMatchingElements(this, searchString);
        }
    });
}

/**
 * Remove highlighting from previously highlighted elements.
 */
function removeHighlighting() {
    $('.highlighted').removeClass('highlighted');  // Remove CSS class from previously highlighted elements
}

export function initSettingsSearch() {
    initSettingsFormAccessibility();
    $('#settingsSearch').on('input change', searchSettings);
}
