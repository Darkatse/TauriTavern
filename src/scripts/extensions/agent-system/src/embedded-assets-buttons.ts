import { AGENT_TOGGLE_ICON } from './agent-icon';
import { reportAgentSystemError, requireSillyTavernContext } from './host-api';
import { translateAgentSystem as tr } from './i18n';
import { openEmbeddedAssetsPanel } from './embedded-assets-popup';

const PRESET_BUTTONS = Object.freeze([
    { apiId: 'kobold', selectId: 'settings_preset' },
    { apiId: 'novel', selectId: 'settings_preset_novel' },
    { apiId: 'openai', selectId: 'settings_preset_openai' },
    { apiId: 'textgenerationwebui', selectId: 'settings_preset_textgenerationwebui' },
]);

type EmbeddedAssetEventName =
    | 'CHARACTER_EDITOR_OPENED'
    | 'CHARACTER_EDITED'
    | 'CHARACTER_DELETED'
    | 'CHAT_CHANGED';
type EmbeddedAssetsHostContext = {
    characterId?: string | number | null;
    characters?: unknown[] | Record<string, unknown>;
    eventTypes?: Partial<Record<EmbeddedAssetEventName, string>>;
    eventSource?: {
        on: (eventName: string, listener: () => void) => void;
    };
};

function createEmbedButton(input: { id: string; targetKind: string; title: string }): HTMLButtonElement {
    const { id, targetKind, title } = input;
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

function findPresetButtonBar(select: HTMLSelectElement): HTMLElement | null {
    const row = select.parentElement;
    if (!(row instanceof HTMLElement)) {
        return null;
    }
    const bar = Array.from(row.children).find((child) => (
        child instanceof HTMLElement
        && child !== select
        && child.classList.contains('flex-container')
    ));
    return bar instanceof HTMLElement ? bar : null;
}

function mountPresetEmbedButtons(): void {
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
                reportAgentSystemError(error);
                throw error;
            }
        });

        bar.appendChild(button);
    }
}

function isCharacterEditMode(): boolean {
    const form = document.getElementById('form_create');
    if (!(form instanceof HTMLElement)) {
        throw new Error(tr('characterFormNotFound'));
    }
    const context = requireSillyTavernContext() as EmbeddedAssetsHostContext;
    return form.getAttribute('actiontype') === 'editcharacter'
        && Boolean(selectedCharacter(context));
}

function mountCharacterEmbedButton(): void {
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
            reportAgentSystemError(error);
            throw error;
        }
    });

    const anchor = document.getElementById('char_connections_button');
    if (anchor?.parentElement === bar) {
        anchor.after(button);
    } else {
        bar.insertBefore(button, document.getElementById('export_button'));
    }

    const sync = (): void => {
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

    const context = requireSillyTavernContext() as EmbeddedAssetsHostContext;
    const { eventSource, eventTypes } = requireEmbeddedAssetEvents(context);
    eventSource.on(eventTypes.CHARACTER_EDITOR_OPENED, sync);
    eventSource.on(eventTypes.CHARACTER_EDITED, sync);
    eventSource.on(eventTypes.CHARACTER_DELETED, sync);
    eventSource.on(eventTypes.CHAT_CHANGED, sync);
    sync();
}

export function mountEmbeddedAssetButtons(): void {
    mountPresetEmbedButtons();
    mountCharacterEmbedButton();
}

function selectedCharacter(context: EmbeddedAssetsHostContext): unknown {
    const id = context.characterId;
    if (id == null) return undefined;
    return Array.isArray(context.characters)
        ? context.characters[Number(id)]
        : context.characters?.[String(id)];
}

function requireEmbeddedAssetEvents(context: EmbeddedAssetsHostContext): {
    eventSource: NonNullable<EmbeddedAssetsHostContext['eventSource']>;
    eventTypes: Record<EmbeddedAssetEventName, string>;
} {
    const eventSource = context.eventSource;
    const eventTypes = context.eventTypes;
    const editorOpened = eventTypes?.CHARACTER_EDITOR_OPENED;
    const edited = eventTypes?.CHARACTER_EDITED;
    const deleted = eventTypes?.CHARACTER_DELETED;
    const chatChanged = eventTypes?.CHAT_CHANGED;
    if (typeof eventSource?.on !== 'function'
        || typeof editorOpened !== 'string'
        || typeof edited !== 'string'
        || typeof deleted !== 'string'
        || typeof chatChanged !== 'string') {
        throw new Error('agent.embedded_asset_events_unavailable: SillyTavern character event contract is unavailable');
    }
    return {
        eventSource,
        eventTypes: {
            CHARACTER_EDITOR_OPENED: editorOpened,
            CHARACTER_EDITED: edited,
            CHARACTER_DELETED: deleted,
            CHAT_CHANGED: chatChanged,
        },
    };
}
