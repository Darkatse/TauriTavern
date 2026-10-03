// @ts-check

import { main_api, saveSettingsDebounced } from '../../../script.js';
import { eventSource, event_types } from '../../events.js';
import { t, translate } from '../../i18n.js';
import { oai_settings, settingsToUpdate } from '../../openai.js';
import { ensureCompactRowsStyle } from '../compact-rows/overflow-menu.js';
import { DRAWERS, FALLBACK_VALUES, PANEL_SCOPE, PINNED_PRESET_KEYS, PAYLOAD_KEYS, SOURCE_SPECIFIC_MAX_SOURCES } from './catalog.js';
import { hideOwnDescriptions, placeHint } from './hints.js';
import { parseParams, serializeParams } from './json-view.js';
import { getOmittedParams, setParamOmitted } from './omission.js';
import { getEffectiveReasoningEffort } from './reasoning-effort-options.js';

const INACTIVE_CLASS = 'tt-gp-inactive';
const STYLE_ID = 'tauritavern-generation-params-style';
const HIDDEN_BLOCKS_KEY = 'tt:generationParams:hiddenBlocks';
const COLLAPSED_KEY = 'tt:generationParams:collapsed';

/**
 * `key` is the payload key for `request`, the `oai_settings` field for
 * `toggle`, and the preset key for `local` and `fallback`. `scope` is `source` when the
 * block is a provider-specific feature (see `SOURCE_SPECIFIC_MAX_SOURCES`),
 * otherwise `common`.
 * @typedef {{ kind: 'request' | 'toggle' | 'local' | 'fallback', scope: 'common' | 'source', key: string, settingsKey: string }} GenerationParam
 * @typedef {{ param: GenerationParam, block: HTMLElement, control: HTMLElement }} Entry
 */

function ensureStyle() {
    ensureCompactRowsStyle(); // defines the shared `.tt-hidden`
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
        if (!selector || PINNED_PRESET_KEYS.includes(presetKey)) continue;
        const control = document.querySelector(selector);
        if (!(control instanceof HTMLElement) || !control.closest(PANEL_SCOPE)) continue;
        const block = control.closest('[data-source], .range-block, .inline-drawer');
        if (!(block instanceof HTMLElement)) continue;
        const kind = isCheckbox ? 'toggle'
            : presetKey in PAYLOAD_KEYS ? 'request'
                : presetKey in FALLBACK_VALUES ? 'fallback' : 'local';
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

/**
 * @param {string} presetKey `settingsToUpdate` key of an upstream control the panel relies on
 * @returns {HTMLElement}
 */
function requireControl(presetKey) {
    const selector = settingsToUpdate[presetKey]?.[0];
    const control = selector ? document.querySelector(selector) : null;
    if (!(control instanceof HTMLElement)) {
        throw new Error(`Generation parameter control not found: ${presetKey}`);
    }
    return control;
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
        // A drawer's own title sits in its header; titles inside its body belong to sub-settings.
        ?? (block.classList.contains('inline-drawer')
            ? block.querySelector(':scope > .inline-drawer-header b') ?? block.querySelector(':scope > .inline-drawer-header')
            : block.querySelector('.range-block-title'));
}

/** Localized label straight from upstream markup, so no parallel i18n table. */
function labelOf(/** @type {Entry} */ entry) {
    return labelElementOf(entry)?.textContent?.replace(/\s+/g, ' ').trim() || entry.param.key;
}

/** @param {Entry} entry */
const isSupported = entry => entry.block.style.display !== 'none';

/** Fallback blocks added back this session; otherwise they show only when their value differs from the fallback. */
const revealedFallbacks = new Set();

/** @param {Entry} entry */
function isActive({ param }) {
    switch (param.kind) {
        case 'toggle': return Boolean(/** @type {Record<string, unknown>} */ (oai_settings)[param.settingsKey]);
        case 'local': return !hiddenBlocks.has(param.key);
        case 'fallback': return revealedFallbacks.has(param.key)
            || Number(/** @type {Record<string, unknown>} */ (oai_settings)[param.settingsKey]) !== FALLBACK_VALUES[param.key];
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
function setActive(entry, active) {
    const { param, control } = entry;
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
        case 'fallback':
            if (active) {
                revealedFallbacks.add(param.key);
            } else {
                revealedFallbacks.delete(param.key);
                writeValue(entry, FALLBACK_VALUES[param.key]);
            }
            return;
        default:
            if (setParamOmitted(oai_settings, param.key, !active)) {
                saveSettingsDebounced();
            }
    }
}

const REMOVE_TITLE = { request: 'Remove from request', toggle: 'Turn off and hide', local: 'Hide section', fallback: 'Reset to default and hide' };

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

/**
 * Upstream's "click slider numbers to input manually" tip. This panel turns the Chat
 * Completion sliders into number boxes, so the tip only applies to the other APIs.
 */
function installSliderTipToggle() {
    const tip = document.getElementById('clickSlidersTips');
    if (!tip) {
        throw new Error('#clickSlidersTips not found; cannot hide it for Chat Completion');
    }
    const sync = () => tip.classList.toggle('tt-hidden', main_api === 'openai');
    eventSource.on(event_types.MAIN_API_CHANGED, sync);
    eventSource.on(event_types.SETTINGS_LOADED_AFTER, sync);
    sync();
}

/**
 * One muted line under the pinned Reasoning Effort select when the current connection
 * cannot use the preset's stored value at all: the select shows Auto (what is sent), and
 * the note says why. The stored value is kept, so it applies again on a connection that has it.
 * @param {HTMLElement} select
 * @returns {() => void} Updates the note from the current settings
 */
function installEffortNote(select) {
    const note = document.createElement('small');
    note.className = 'tt-gp-effort-note';
    note.hidden = true;
    select.after(note);
    return () => {
        const stored = oai_settings.reasoning_effort;
        const unsupported = typeof stored === 'string' && stored !== '' && stored !== 'auto'
            && getEffectiveReasoningEffort(oai_settings) === 'auto';
        note.hidden = !unsupported;
        note.textContent = unsupported ? t`Preset value "${stored}" isn't supported here; sent as Auto.` : '';
    };
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
        <div class="tt-gp-title inline-drawer-header" role="button" tabindex="0" aria-expanded="true">
            <b>${translate('Request Parameter Management')}</b>
            <i class="fa-solid inline-drawer-icon" aria-hidden="true"></i>
        </div>
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
    // Pinned settings sit above the section, so folding it never hides them.
    for (const presetKey of PINNED_PRESET_KEYS) {
        const control = requireControl(presetKey);
        const block = control.closest('[data-source], .range-block');
        if (!(block instanceof HTMLElement)) {
            throw new Error(`Pinned generation parameter block not found: ${presetKey}`);
        }
        bar.before(block);
        if (placeHint(labelElementOf(/** @type {Entry} */ ({ block, control })), presetKey)) hideOwnDescriptions(block);
    }
    // Reasoning effort is no longer removable; a preset that removed it meant "not sent", which is Auto.
    const normalizePinned = () => {
        if (setParamOmitted(oai_settings, PAYLOAD_KEYS.reasoning_effort, false)) {
            oai_settings.reasoning_effort = 'auto';
            $(settingsToUpdate.reasoning_effort[0]).val('auto');
            saveSettingsDebounced();
        }
    };
    normalizePinned();
    const effortSelect = requireControl('reasoning_effort');
    const syncEffortNote = installEffortNote(effortSelect);
    // jQuery, so `.trigger('input')` counts too. The OpenCode API format changes the
    // vocabulary without an event of its own.
    $([effortSelect, requireControl('opencode_api_format')]).on('input', syncEffortNote);
    installSliderTipToggle();
    const q = (/** @type {string} */ selector) => /** @type {HTMLElement} */ (bar.querySelector(selector));
    const addButton = /** @type {HTMLButtonElement} */ (q('.tt-gp-add'));
    const modeButton = /** @type {HTMLButtonElement} */ (q('.tt-gp-mode'));
    const picker = q('.tt-gp-picker');
    const json = q('.tt-gp-json');
    const jsonText = /** @type {HTMLTextAreaElement} */ (q('.tt-gp-json-text'));
    const jsonError = q('.tt-gp-json-error');
    const jsonHint = q('.tt-gp-json-hint');
    const title = q('.tt-gp-title');
    const chevron = q('.tt-gp-title .inline-drawer-icon');
    const defaultJsonHint = jsonHint.textContent;
    picker.id = 'tt-gp-picker';
    addButton.setAttribute('aria-controls', picker.id);

    // Local blocks are display preferences, not request configuration; JSON covers the rest.
    const jsonEntries = entries.filter(entry => entry.param.kind !== 'local');
    let jsonMode = false;
    // Folding the section is a device preference, like hidden local blocks.
    let collapsed = localStorage.getItem(COLLAPSED_KEY) === 'true';
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
                    setPickerOpen(false);
                    const target = entry.control.matches('input[type="range"]')
                        ? /** @type {HTMLElement} */ (entry.block.querySelector(`input[type="number"][data-for="${CSS.escape(entry.control.id)}"]`))
                        : entry.control;
                    const drawerToggle = entry.block.matches('.inline-drawer')
                        ? /** @type {HTMLElement} */ (entry.block.querySelector('.inline-drawer-icon'))
                        : null;
                    (drawerToggle ?? target).focus();
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
            entry.block.classList.toggle(INACTIVE_CLASS, collapsed || !isActive(entry) || (jsonMode && entry.param.kind !== 'local'));
        }
        bar.classList.toggle('tt-gp-collapsed', collapsed);
        title.setAttribute('aria-expanded', String(!collapsed));
        chevron.classList.toggle('fa-circle-chevron-up', !collapsed);
        chevron.classList.toggle('fa-circle-chevron-down', collapsed);
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
            if (active && (entry.param.kind === 'request' || entry.param.kind === 'fallback')) writeValue(entry, value);
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

    function toggleCollapsed() {
        collapsed = !collapsed;
        localStorage.setItem(COLLAPSED_KEY, String(collapsed));
        if (collapsed) setPickerOpen(false);
        sync();
    }
    title.addEventListener('click', toggleCollapsed);
    title.addEventListener('keydown', event => {
        if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            toggleCollapsed();
        }
    });

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
    picker.addEventListener('keydown', event => {
        if (event.key !== 'Escape') return;
        event.preventDefault();
        event.stopPropagation();
        setPickerOpen(false);
        addButton.focus();
    });
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
        const remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'tt-gp-remove';
        remove.title = remove.ariaLabel = `${translate(REMOVE_TITLE[entry.param.kind])}: ${labelOf(entry)}`;
        remove.innerHTML = '<i class="fa-solid fa-xmark" aria-hidden="true"></i>';
        remove.addEventListener('click', event => {
            // Inside a drawer header the click would also toggle the drawer.
            if (entry.block.classList.contains('inline-drawer')) event.stopPropagation();
            setActive(entry, false);
            sync();
            addButton.focus();
        });
        // Localization replaces label contents; keep the buttons outside that node.
        const label = labelElementOf(entry);
        // × follows the explanation icon (or sits inside a toggle's label, a
        // full-width row once its checkbox is hidden) to stay on the label's line.
        const hint = placeHint(label, entry.param.key);
        if (hint) hint.after(remove);
        else if (entry.param.kind === 'toggle' && label instanceof HTMLLabelElement) label.append(remove);
        else if (label) label.after(remove);
        else entry.block.append(remove);
        if (hint) hideOwnDescriptions(entry.block);
        // Sliders collapse to their number box: one compact row per parameter.
        // Upstream keeps syncing number → slider via `data-for`, so the hidden
        // range input remains the value owner.
        // Plain number boxes (max response length, swipes) join them unless a
        // description needs the block's full width.
        if (entry.control instanceof HTMLInputElement && (entry.control.type === 'range'
            || (entry.control.type === 'number' && !entry.block.querySelector('.toggle-description:not(.tt-hidden)')))) {
            entry.block.classList.add('tt-gp-compact');
        }
        if (entry.param.kind === 'toggle') {
            // Shown means on and × turns it off, so the checkbox itself is
            // redundant; its label must not toggle it either.
            entry.control.classList.add('tt-hidden');
            if (entry.control instanceof HTMLInputElement) {
                entry.control.labels?.[0]?.addEventListener('click', event => event.preventDefault());
            }
            $(entry.control).on('input change', sync);
        }
    }

    for (const eventName of [
        event_types.SETTINGS_LOADED_AFTER,
        event_types.OAI_PRESET_CHANGED_AFTER,
        event_types.CHATCOMPLETION_SOURCE_CHANGED,
    ]) {
        eventSource.on(eventName, () => {
            normalizePinned();
            sync();
            syncEffortNote();
            if (jsonMode) {
                if (jsonText.value === jsonSnapshot) renderJson();
                else jsonHint.textContent = translate('Settings changed. Your draft is kept. Apply to use it here, or Reset to reload.');
            }
        });
    }
    sync();
    syncEffortNote();
}
