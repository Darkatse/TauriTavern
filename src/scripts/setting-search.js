import { t } from './i18n.js';
import { event_types, eventSource } from './events.js';
import { isScreenReaderAssistanceEnabled } from './a11y/screen-reader.js';

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

const GENERATED_STREAMING_DESCRIPTION_IDS = [
    'streaming_textgenerationwebui',
    'streaming_kobold',
    'streaming_novel',
];
const SCREEN_READER_DESCRIPTION_ID_ATTRIBUTE = 'data-tt-sra-description-id';

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

function setDescribedByToken(control, descriptionId, enabled) {
    const tokens = new Set((control.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(Boolean));
    if (enabled) {
        tokens.add(descriptionId);
    } else {
        tokens.delete(descriptionId);
    }

    if (tokens.size > 0) {
        control.setAttribute('aria-describedby', Array.from(tokens).join(' '));
    } else {
        control.removeAttribute('aria-describedby');
    }
}

function getStreamingDescriptionText() {
    return `${t`Display the response bit by bit as it is generated.`} ${t`When this is off, responses will be displayed all at once when they are complete.`}`;
}

function syncGeneratedStreamingDescription(controlId, enabled) {
    const control = requireElementById(controlId);
    if (!(control instanceof HTMLInputElement) || control.type !== 'checkbox') {
        throw new Error(`Streaming description target must be an input[type="checkbox"]: #${controlId}`);
    }

    const descriptionId = `${controlId}_screen_reader_description`;
    let description = document.getElementById(descriptionId);

    if (enabled) {
        if (!description) {
            description = document.createElement('span');
            description.id = descriptionId;
            description.classList.add('sr-only');
            control.insertAdjacentElement('afterend', description);
        }
        description.textContent = getStreamingDescriptionText();
        setDescribedByToken(control, descriptionId, true);
        return;
    }

    setDescribedByToken(control, descriptionId, false);
    description?.remove();
}

function syncExistingStreamingDescription(controlId, enabled) {
    const control = requireElementById(controlId);
    if (!(control instanceof HTMLInputElement) || control.type !== 'checkbox') {
        throw new Error(`Streaming description target must be an input[type="checkbox"]: #${controlId}`);
    }

    const description = control.closest('.range-block')?.querySelector('.toggle-description');
    if (!(description instanceof HTMLElement)) {
        throw new Error(`Streaming description is missing for #${controlId}`);
    }

    if (enabled && !description.id) {
        description.id = `${controlId}_description`;
        description.setAttribute(SCREEN_READER_DESCRIPTION_ID_ATTRIBUTE, '');
    }

    if (description.id) {
        setDescribedByToken(control, description.id, enabled);
    }

    if (!enabled && description.hasAttribute(SCREEN_READER_DESCRIPTION_ID_ATTRIBUTE)) {
        description.removeAttribute('id');
        description.removeAttribute(SCREEN_READER_DESCRIPTION_ID_ATTRIBUTE);
    }
}

function syncScreenReaderSettingDescriptions() {
    const enabled = isScreenReaderAssistanceEnabled();
    syncExistingStreamingDescription('stream_toggle', enabled);
    GENERATED_STREAMING_DESCRIPTION_IDS.forEach(id => syncGeneratedStreamingDescription(id, enabled));
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

    syncScreenReaderSettingDescriptions();
    eventSource.on(event_types.SCREEN_READER_ASSISTANCE_CHANGED, syncScreenReaderSettingDescriptions);
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
