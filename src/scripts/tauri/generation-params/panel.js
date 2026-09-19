// @ts-check

import { saveSettingsDebounced } from '../../../script.js';
import { eventSource, event_types } from '../../events.js';
import { translate } from '../../i18n.js';
import { oai_settings, settingsToUpdate } from '../../openai.js';
import { DRAWERS, EXCLUDED_PRESET_KEYS, PANEL_SCOPE, PAYLOAD_KEYS, SOURCE_SPECIFIC_MAX_SOURCES } from './catalog.js';
import { parseParams, serializeParams } from './json-view.js';
import { getOmittedParams, setParamOmitted } from './omission.js';

const INACTIVE_CLASS = 'tt-gp-inactive';
const STYLE_ID = 'tauritavern-generation-params-style';
const HIDDEN_BLOCKS_KEY = 'tt:generationParams:hiddenBlocks';

/**
 * `key` is the payload key for `request`, the `oai_settings` field for
 * `toggle`, and a stable local id for `local`. `scope` is `source` when the
 * block is a provider-specific feature (see `SOURCE_SPECIFIC_MAX_SOURCES`),
 * otherwise `common`.
 * @typedef {{ kind: 'request' | 'toggle' | 'local', scope: 'common' | 'source', key: string, settingsKey: string }} GenerationParam
 * @typedef {{ param: GenerationParam, block: HTMLElement, control: HTMLElement }} Entry
 */

function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const link = document.createElement('link');
    link.id = STYLE_ID;
    link.rel = 'stylesheet';
    link.href = new URL('./panel.css', import.meta.url).href;
    document.head.append(link);
}

/** Hidden `local` blocks are a device preference, not preset data. Loaded once; written through on change. */
const hiddenBlocks = (() => {
    try {
        const raw = JSON.parse(localStorage.getItem(HIDDEN_BLOCKS_KEY) ?? '[]');
        return new Set(Array.isArray(raw) ? raw.filter(key => typeof key === 'string') : []);
    } catch {
        return new Set();
    }
})();

/** @param {string} key @param {boolean} hidden */
function setBlockHidden(key, hidden) {
    if (hidden) hiddenBlocks.add(key); else hiddenBlocks.delete(key);
    localStorage.setItem(HIDDEN_BLOCKS_KEY, JSON.stringify([...hiddenBlocks]));
}

/**
 * Discover managed blocks from upstream's own preset table. Upstream toggles
 * `[data-source]` blocks with jQuery `.toggle()` when the source changes; that
 * inline `display` stays the single source of truth for "supported here".
 * Our active state is layered on top via a class. jQuery's show path writes
 * `display:block` when it finds an element hidden by computed style, so
 * `sync()` normalises any inline value other than `none` back to `''`.
 * @returns {Entry[]}
 */
function resolveEntries() {
    /** @type {Entry[]} */
    const found = [];
    for (const [presetKey, [selector, settingsKey, isCheckbox]] of Object.entries(settingsToUpdate)) {
        if (!selector || EXCLUDED_PRESET_KEYS.has(presetKey)) continue;
        const control = document.querySelector(selector);
        if (!(control instanceof HTMLElement) || !control.closest(PANEL_SCOPE)) continue;
        const block = control.closest('[data-source], .range-block, .inline-drawer');
        if (!(block instanceof HTMLElement)) continue;
        const kind = isCheckbox ? 'toggle' : presetKey in PAYLOAD_KEYS ? 'request' : 'local';
        const key = kind === 'request' ? PAYLOAD_KEYS[presetKey] : kind === 'toggle' ? settingsKey : presetKey;
        found.push({ param: { kind, scope: scopeOf(block), key, settingsKey }, block, control });
    }
    for (const { key, controlId } of DRAWERS) {
        const control = document.getElementById(controlId);
        const block = control?.closest('.inline-drawer');
        if (control instanceof HTMLElement && block instanceof HTMLElement) {
            found.push({ param: { kind: 'local', scope: 'common', key, settingsKey: key }, block, control });
        }
    }

    // A control nested inside another managed block (e.g. image quality under
    // media inlining) is part of that block, not a separate parameter.
    /** @type {Entry[]} */
    const kept = [];
    for (const entry of found) {
        if (kept.some(other => other.block.contains(entry.block))) continue;
        for (let i = kept.length - 1; i >= 0; i--) {
            if (entry.block.contains(kept[i].block)) kept.splice(i, 1);
        }
        kept.push(entry);
    }
    return kept.sort((a, b) => (a.block.compareDocumentPosition(b.block) & Node.DOCUMENT_POSITION_FOLLOWING) ? -1 : 1);
}

/** @param {HTMLElement} block */
function scopeOf(block) {
    if (block.getAttribute('data-source-mode') === 'except') return 'common';
    const sources = block.getAttribute('data-source')?.split(',').filter(Boolean) ?? [];
    return sources.length > 0 && sources.length <= SOURCE_SPECIFIC_MAX_SOURCES ? 'source' : 'common';
}

/** Display name of the active source, from upstream's own dropdown (already localized). */
function sourceLabel() {
    const option = document.querySelector('#chat_completion_source option:checked');
    return option?.textContent?.trim() || String(oai_settings.chat_completion_source);
}

/** Upstream element carrying the block's label. */
function labelElementOf(/** @type {Entry} */ { block, control }) {
    return /** @type {HTMLInputElement} */ (control).labels?.[0]
        ?? document.getElementById(`${control.id}_text`)
        ?? block.querySelector('.range-block-title, .inline-drawer-header b, .inline-drawer-header');
}

/** Localized label straight from upstream markup, so no parallel i18n table. */
function labelOf(/** @type {Entry} */ entry) {
    return labelElementOf(entry)?.textContent?.replace(/\s+/g, ' ').trim() || entry.param.key;
}

/** @param {Entry} entry */
const isSupported = entry => entry.block.style.display !== 'none';

/** @param {Entry} entry */
function isActive({ param }) {
    switch (param.kind) {
        case 'toggle': return Boolean(/** @type {Record<string, unknown>} */ (oai_settings)[param.settingsKey]);
        case 'local': return !hiddenBlocks.has(param.key);
        default: return !getOmittedParams(oai_settings).includes(param.key);
    }
}

/**
 * Request params flip the omission list; toggles drive the upstream checkbox
 * so upstream handlers keep owning `oai_settings` and persistence; local
 * blocks only record a visibility preference.
 * @param {Entry} entry
 * @param {boolean} active
 */
function setActive({ param, control }, active) {
    switch (param.kind) {
        case 'toggle': {
            if (!(control instanceof HTMLInputElement) || control.checked === active) return;
            control.checked = active;
            control.dispatchEvent(new Event('input', { bubbles: true }));
            control.dispatchEvent(new Event('change', { bubbles: true }));
            return;
        }
        case 'local':
            setBlockHidden(param.key, !active);
            return;
        default:
            if (setParamOmitted(oai_settings, param.key, !active)) {
                saveSettingsDebounced();
            }
    }
}

const REMOVE_TITLE = { request: 'Remove from request', toggle: 'Turn off and hide', local: 'Hide section' };

/** @param {Entry} entry */
function readValue({ param }) {
    return /** @type {Record<string, unknown>} */ (oai_settings)[param.settingsKey];
}

/**
 * Write through the upstream control so its own `input` handler owns
 * `oai_settings`, counters and persistence, exactly like preset loading.
 * @param {Entry} entry
 * @param {unknown} value
 */
function writeValue({ control }, value) {
    if (!(control instanceof HTMLInputElement || control instanceof HTMLSelectElement || control instanceof HTMLTextAreaElement)) return;
    if (control.value === String(value)) return;
    control.value = String(value);
    control.dispatchEvent(new Event('input', { bubbles: true }));
    control.dispatchEvent(new Event('change', { bubbles: true }));
}

/**
 * @param {Entry} entry
 * @returns {import('./json-view.js').FieldType}
 */
function fieldTypeOf({ control }) {
    if (control instanceof HTMLSelectElement) {
        return { type: 'string', options: [...control.options].map(option => option.value) };
    }
    if (control instanceof HTMLTextAreaElement) {
        return { type: 'text' };
    }
    if (control instanceof HTMLInputElement && control.type !== 'checkbox') {
        const bound = (/** @type {string} */ raw) => (raw === '' ? undefined : Number(raw));
        return { type: 'number', min: bound(control.min), max: bound(control.max) };
    }
    return { type: 'boolean' };
}

export function installGenerationParamsPanel() {
    const entries = resolveEntries();
    const anchor = entries[0]?.block;
    if (!anchor?.parentElement) {
        throw new Error('Generation parameter blocks not found; cannot install panel');
    }
    ensureStyle();

    const bar = document.createElement('div');
    bar.className = 'tt-gp-bar';
    bar.innerHTML = `
        <div class="tt-gp-title">${translate('Request Parameter Management')}</div>
        <div class="tt-gp-actions">
            <button type="button" class="menu_button tt-gp-add" aria-expanded="false">
                <i class="fa-solid fa-plus" aria-hidden="true"></i><span>${translate('Add parameter')}</span>
            </button>
            <button type="button" class="menu_button tt-gp-mode" title="${translate('Edit as JSON')}" aria-pressed="false">
                <i class="fa-solid fa-code" aria-hidden="true"></i>
            </button>
        </div>
        <div class="tt-gp-picker" hidden></div>
        <div class="tt-gp-json" hidden>
            <small class="tt-gp-json-hint">${translate('Keys listed here are sent with these values; keys omitted are removed from the request. Only known parameters are accepted.')}</small>
            <textarea class="text_pole tt-gp-json-text" rows="12" spellcheck="false"></textarea>
            <small class="tt-gp-json-error" hidden></small>
            <div class="tt-gp-json-actions">
                <button type="button" class="menu_button tt-gp-json-apply">${translate('Apply')}</button>
                <button type="button" class="menu_button tt-gp-json-reset">${translate('Reset')}</button>
            </div>
        </div>`;
    anchor.parentElement.insertBefore(bar, anchor);
    const q = (/** @type {string} */ selector) => /** @type {HTMLElement} */ (bar.querySelector(selector));
    const addButton = /** @type {HTMLButtonElement} */ (q('.tt-gp-add'));
    const modeButton = /** @type {HTMLButtonElement} */ (q('.tt-gp-mode'));
    const picker = q('.tt-gp-picker');
    const json = q('.tt-gp-json');
    const jsonText = /** @type {HTMLTextAreaElement} */ (q('.tt-gp-json-text'));
    const jsonError = q('.tt-gp-json-error');
    const jsonHint = q('.tt-gp-json-hint');
    const defaultJsonHint = jsonHint.textContent;

    // Local blocks are display preferences, not request configuration; JSON covers the rest.
    const jsonEntries = entries.filter(entry => entry.param.kind !== 'local');
    let jsonMode = false;
    let jsonSnapshot = '';

    const addable = () => entries.filter(entry => isSupported(entry) && !isActive(entry));

    function renderPicker() {
        const groups = [
            { scope: 'common', title: translate('General') },
            { scope: 'source', title: sourceLabel() },
        ];
        picker.replaceChildren(...groups.flatMap(({ scope, title }) => {
            const chips = addable().filter(entry => entry.param.scope === scope).map(entry => {
                const chip = document.createElement('button');
                chip.type = 'button';
                chip.className = 'tt-gp-chip';
                chip.textContent = labelOf(entry);
                chip.addEventListener('click', () => {
                    setActive(entry, true);
                    sync();
                });
                return chip;
            });
            if (!chips.length) return [];
            const group = document.createElement('div');
            group.className = 'tt-gp-group';
            group.dataset.ttScope = scope;
            const heading = document.createElement('div');
            heading.className = 'tt-gp-group-title';
            heading.textContent = title;
            group.append(heading, ...chips);
            return [group];
        }));
    }

    function sync() {
        for (const entry of entries) {
            const { style } = entry.block;
            if (style.display && style.display !== 'none') style.display = ''; // jQuery show() artefact
            entry.block.classList.toggle(INACTIVE_CLASS, !isActive(entry) || (jsonMode && entry.param.kind !== 'local'));
        }
        const empty = addable().length === 0;
        addButton.disabled = empty;
        if (empty) setPickerOpen(false);
        else if (!picker.hidden) renderPicker();
    }

    function renderJson() {
        jsonText.value = serializeParams(jsonEntries
            .filter(isSupported)
            .map(entry => ({ key: entry.param.key, active: isActive(entry), value: readValue(entry) })));
        jsonSnapshot = jsonText.value;
        jsonHint.textContent = defaultJsonHint;
        showJsonError([]);
    }

    /** @param {import('./json-view.js').ParseError[]} errors */
    function showJsonError(errors) {
        jsonError.hidden = errors.length === 0;
        jsonError.textContent = errors.map(error => {
            switch (error.kind) {
                case 'syntax': return `${translate('Invalid JSON format')}: ${error.detail}`;
                case 'unknown': return `${translate('Unknown or unsupported parameter for the current API format')}: ${error.key}`;
                case 'invalid': return `${translate('Invalid value')}: ${error.key}`;
            }
        }).join('\n');
    }

    /** @returns {boolean} whether the text was applied */
    function applyJson() {
        const supportedEntries = jsonEntries.filter(isSupported);
        const schema = new Map(supportedEntries.map(entry => [entry.param.key, fieldTypeOf(entry)]));
        const { values, errors } = parseParams(jsonText.value, schema);
        showJsonError(errors);
        if (errors.length) return false;
        for (const entry of supportedEntries) {
            const value = values.get(entry.param.key);
            const active = entry.param.kind === 'toggle' ? value === true : value !== undefined;
            setActive(entry, active);
            if (active && entry.param.kind === 'request') writeValue(entry, value);
        }
        sync();
        renderJson();
        return true;
    }

    /** @param {boolean} on */
    function setJsonMode(on) {
        if (!on && !applyJson()) return; // stay in JSON mode until the text is valid
        jsonMode = on;
        json.hidden = !on;
        modeButton.setAttribute('aria-pressed', String(on));
        modeButton.title = translate(on ? 'Back to form' : 'Edit as JSON');
        addButton.hidden = on;
        if (on) { setPickerOpen(false); renderJson(); }
        sync();
    }

    modeButton.addEventListener('click', () => setJsonMode(!jsonMode));
    q('.tt-gp-json-apply').addEventListener('click', applyJson);
    q('.tt-gp-json-reset').addEventListener('click', renderJson);
    jsonText.addEventListener('keydown', event => {
        if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') applyJson();
    });

    /** @param {boolean} open */
    function setPickerOpen(open) {
        picker.hidden = !open;
        addButton.setAttribute('aria-expanded', String(open));
        if (open) renderPicker();
    }

    addButton.addEventListener('click', () => setPickerOpen(picker.hidden));
    document.addEventListener('click', event => {
        if (!picker.hidden && event.target instanceof Node && !bar.contains(event.target)) {
            setPickerOpen(false);
        }
    });

    for (const entry of entries) {
        entry.block.classList.add('tt-gp-block');
        // Stable hooks for CSS, tests and future first-party panels.
        entry.block.dataset.ttParam = entry.param.key;
        entry.block.dataset.ttKind = entry.param.kind;
        entry.block.dataset.ttScope = entry.param.scope;
        // Sliders collapse to their number box: one compact row per parameter.
        // Upstream keeps syncing number → slider via `data-for`, so the hidden
        // range input remains the value owner.
        if (entry.control instanceof HTMLInputElement && entry.control.type === 'range') {
            entry.block.classList.add('tt-gp-compact');
        }
        const remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'tt-gp-remove';
        remove.title = remove.ariaLabel = translate(REMOVE_TITLE[entry.param.kind]);
        remove.innerHTML = '<i class="fa-solid fa-xmark" aria-hidden="true"></i>';
        remove.addEventListener('click', event => {
            // Inside a drawer header the click would also toggle the drawer.
            if (entry.block.classList.contains('inline-drawer')) event.stopPropagation();
            setActive(entry, false);
            sync();
        });
        // Localization replaces label contents; keep the button outside that node.
        const label = labelElementOf(entry);
        if (label) label.after(remove); else entry.block.append(remove);
        if (entry.param.kind === 'toggle') {
            $(entry.control).on('input change', sync);
        }
    }

    for (const eventName of [
        event_types.SETTINGS_LOADED_AFTER,
        event_types.OAI_PRESET_CHANGED_AFTER,
        event_types.CHATCOMPLETION_SOURCE_CHANGED,
    ]) {
        eventSource.on(eventName, () => {
            sync();
            if (jsonMode) {
                if (jsonText.value === jsonSnapshot) renderJson();
                else jsonHint.textContent = translate('Settings changed. Your draft is kept. Apply to use it here, or Reset to reload.');
            }
        });
    }
    sync();
}
