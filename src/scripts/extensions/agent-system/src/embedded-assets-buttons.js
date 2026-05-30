import { AGENT_TOGGLE_ICON } from './agent-icon.js';
import { errorText } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { openEmbeddedAssetsPanel } from './embedded-assets-popup.js';

const PRESET_BUTTONS = Object.freeze([
    { apiId: 'kobold', selectId: 'settings_preset' },
    { apiId: 'novel', selectId: 'settings_preset_novel' },
    { apiId: 'openai', selectId: 'settings_preset_openai' },
    { apiId: 'textgenerationwebui', selectId: 'settings_preset_textgenerationwebui' },
]);

function reportInteractionError(error) {
    console.error('[AgentSystem]', error);
    window.toastr?.error?.(errorText(error));
}

function createEmbedButton({ id, targetKind, title }) {
    const button = document.createElement('button');
    button.id = id;
    button.type = 'button';
    button.className = 'menu_button menu_button_icon ttas-agent-embed-button';
    button.dataset.ttasEmbedTarget = targetKind;
    button.title = title;
    button.setAttribute('aria-label', title);
    button.innerHTML = AGENT_TOGGLE_ICON;
    return button;
}

function requireSillyTavernContext() {
    const context = window.SillyTavern?.getContext?.();
    if (!context) {
        throw new Error(tr('sillyTavernContextUnavailable'));
    }
    return context;
}

function findPresetButtonBar(select) {
    const row = select.parentElement;
    if (!(row instanceof HTMLElement)) {
        return null;
    }
    return Array.from(row.children).find((child) => (
        child instanceof HTMLElement
        && child !== select
        && child.classList.contains('flex-container')
    )) || null;
}

function mountPresetEmbedButtons() {
    for (const { apiId, selectId } of PRESET_BUTTONS) {
        const select = document.getElementById(selectId);
        if (!(select instanceof HTMLSelectElement)) {
            throw new Error(tr('presetSelectNotFound', { id: selectId }));
        }

        const buttonId = `ttas_agent_embed_preset_${apiId}`;
        if (document.getElementById(buttonId)) {
            continue;
        }

        const bar = findPresetButtonBar(select);
        if (!(bar instanceof HTMLElement)) {
            throw new Error(tr('presetButtonBarNotFound', { apiId }));
        }

        const button = createEmbedButton({
            id: buttonId,
            targetKind: 'preset',
            title: tr('openAgentAssets'),
        });
        button.dataset.ttasPresetApi = apiId;
        button.addEventListener('click', (event) => {
            event.preventDefault();
            try {
                openEmbeddedAssetsPanel({ kind: 'preset', apiId });
            } catch (error) {
                reportInteractionError(error);
                throw error;
            }
        });

        bar.appendChild(button);
    }
}

function isCharacterEditMode() {
    const form = document.getElementById('form_create');
    if (!(form instanceof HTMLElement)) {
        throw new Error(tr('characterFormNotFound'));
    }
    const context = requireSillyTavernContext();
    return form.getAttribute('actiontype') === 'editcharacter'
        && Boolean(context.characters?.[context.characterId]);
}

function mountCharacterEmbedButton() {
    const bar = document.querySelector('#avatar_controls .form_create_bottom_buttons_block');
    if (!(bar instanceof HTMLElement)) {
        throw new Error(tr('characterButtonBarNotFound'));
    }

    if (document.getElementById('ttas_character_agent_embed_button')) {
        return;
    }

    const button = createEmbedButton({
        id: 'ttas_character_agent_embed_button',
        targetKind: 'character',
        title: tr('openAgentAssets'),
    });
    button.addEventListener('click', (event) => {
        event.preventDefault();
        try {
            openEmbeddedAssetsPanel({ kind: 'character' });
        } catch (error) {
            reportInteractionError(error);
            throw error;
        }
    });

    const anchor = document.getElementById('char_connections_button');
    if (anchor?.parentElement === bar) {
        anchor.after(button);
    } else {
        bar.insertBefore(button, document.getElementById('export_button'));
    }

    const sync = () => {
        const visible = isCharacterEditMode();
        button.classList.toggle('displayNone', !visible);
        button.disabled = !visible;
    };

    const form = document.getElementById('form_create');
    if (!(form instanceof HTMLElement)) {
        throw new Error(tr('characterFormNotFound'));
    }
    new MutationObserver(sync).observe(form, {
        attributes: true,
        attributeFilter: ['actiontype'],
    });

    const context = requireSillyTavernContext();
    const events = context.eventTypes;
    context.eventSource.on(events.CHARACTER_EDITOR_OPENED, sync);
    context.eventSource.on(events.CHARACTER_EDITED, sync);
    context.eventSource.on(events.CHARACTER_DELETED, sync);
    context.eventSource.on(events.CHAT_CHANGED, sync);
    sync();
}

export async function mountEmbeddedAssetButtons() {
    mountPresetEmbedButtons();
    mountCharacterEmbedButton();
}
