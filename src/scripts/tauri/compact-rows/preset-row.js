// @ts-check

import { translate } from '../../i18n.js';
import { adoptOverflowButtons, createOverflowMenuButton, ensureCompactRowsStyle } from './overflow-menu.js';

/**
 * @param {string} selector
 * @returns {HTMLElement}
 */
function requireElement(selector) {
    const element = document.querySelector(selector);
    if (!(element instanceof HTMLElement)) {
        throw new Error(`Chat Completion preset row: ${selector} not found`);
    }
    return element;
}

/**
 * Folds the Chat Completion preset block into one row: "Preset | select | save | extension
 * buttons | ⋯". The header's parts and the less frequent buttons are hidden, not removed:
 * their ids and handlers stay the upstream contract (openai.js, preset-manager.js), and the
 * menu simply clicks them. Anything else an extension adds, to the row or the header, stays
 * visible unless it opts into the menu with `data-tt-overflow` (see `adoptOverflowButtons`).
 */
export function installCompactPresetRow() {
    ensureCompactRowsStyle();
    const block = requireElement('#openai_api-presets');
    const select = requireElement('#settings_preset_openai');
    const save = requireElement('#update_oai_preset');
    const row = /** @type {HTMLElement} */ (select.parentElement);
    const buttonBar = save.parentElement;
    if (!(buttonBar instanceof HTMLElement) || buttonBar.parentElement !== row) {
        throw new Error('Chat Completion preset row: button bar not found');
    }
    const bind = /** @type {HTMLInputElement} */ (requireElement('#bind_preset_to_connection'));
    const actions = {
        saveAs: requireElement('#new_oai_preset'),
        rename: requireElement('[data-preset-manager-rename="openai"]'),
        importPreset: requireElement('#import_oai_preset'),
        exportPreset: requireElement('#export_oai_preset'),
        deletePreset: requireElement('#delete_oai_preset'),
    };
    const folded = [
        requireElement('#openai_api-presets .standoutHeader > strong'),
        requireElement('label[for="bind_preset_to_connection"]'),
        ...Object.values(actions),
    ];

    block.classList.add('tt-compact-presets');
    row.classList.add('tt-sel-row');
    // One gap for label, select and buttons, the same as in the model row above.
    row.classList.remove('flexNoGap');
    const label = document.createElement('span');
    label.className = 'tt-sel-label';
    label.textContent = translate('Preset');
    label.title = translate('Chat Completion Presets', 'openaipresets');
    row.prepend(label);
    for (const element of folded) element.classList.add('tt-hidden');
    const extensionItems = adoptOverflowButtons(row);
    const withEllipsis = (/** @type {string} */ text) => `${translate(text)}…`;

    buttonBar.append(createOverflowMenuButton({
        title: translate('More'),
        items: () => [
            { label: withEllipsis('Save as'), icon: 'file-circle-plus', onSelect: () => actions.saveAs.click() },
            { label: withEllipsis('Rename'), icon: 'pencil', onSelect: () => actions.rename.click() },
            { label: withEllipsis('Import'), icon: 'file-import', separatorBefore: true, onSelect: () => actions.importPreset.click() },
            { label: withEllipsis('Export'), icon: 'file-export', onSelect: () => actions.exportPreset.click() },
            ...extensionItems(),
            {
                label: translate('Bind preset to connection'),
                hint: translate('Bind presets to API connections'),
                icon: 'link',
                separatorBefore: true,
                checked: () => bind.checked,
                // The Connection Manager disables the checkbox, with the reason as its title,
                // while the selected model owns the connection.
                disabledReason: () => (bind.disabled ? bind.title || translate('A selected model decides the connection') : null),
                onSelect: () => {
                    bind.checked = !bind.checked;
                    bind.dispatchEvent(new Event('input', { bubbles: true }));
                },
            },
            { label: translate('Delete'), icon: 'trash-can', danger: true, separatorBefore: true, onSelect: () => actions.deletePreset.click() },
        ],
    }));
}
