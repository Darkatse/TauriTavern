// @ts-check

import { getActiveIosPolicyCapabilities } from '../../tauritavern/ios-policy.js';

const STORAGE_KEY = 'tauritavern:extensions_menu_shortcuts';
const CONTAINER_ID = 'tauritavern_extensions_menu_shortcuts';
const DEFAULT_SHORTCUT_IDS = ['sync'];

const SHORTCUTS = [
    {
        id: 'sync',
        label: 'Sync Panel',
        icon: 'fa-rotate',
        available: () => getActiveIosPolicyCapabilities()?.sync?.lan !== false,
        open: async () => {
            const { openSyncPopup } = await import('./setting-panel/sync-popup.js');
            await openSyncPopup();
        },
    },
    {
        id: 'frontend-logs',
        label: 'Frontend Logs',
        icon: 'fa-terminal',
        open: async () => {
            const { openFrontendLogsPanel } = await import('./dev-logs.js');
            await openFrontendLogsPanel();
        },
    },
    {
        id: 'backend-logs',
        label: 'Backend Logs',
        icon: 'fa-server',
        open: async () => {
            const { openBackendLogsPanel } = await import('./dev-logs.js');
            await openBackendLogsPanel();
        },
    },
    {
        id: 'llm-api-logs',
        label: 'LLM API Logs',
        icon: 'fa-file-lines',
        open: async () => {
            const { openLlmApiLogsPanel } = await import('./dev-logs.js');
            await openLlmApiLogsPanel();
        },
    },
    {
        id: 'reload-frontend',
        label: 'Reload Frontend',
        icon: 'fa-arrows-rotate',
        open: async () => window.location.reload(),
    },
];

const SHORTCUT_IDS = new Set(SHORTCUTS.map(shortcut => shortcut.id));

/** @param {unknown} value */
function normalizeShortcutIds(value) {
    if (!Array.isArray(value)) {
        throw new Error('Stored quick access menu must be an array');
    }

    const selected = new Set(value);
    for (const id of selected) {
        if (typeof id !== 'string' || !SHORTCUT_IDS.has(id)) {
            throw new Error(`Stored quick access menu contains an unsupported shortcut: ${String(id)}`);
        }
    }

    return SHORTCUTS.filter(shortcut => selected.has(shortcut.id)).map(shortcut => shortcut.id);
}

export function getExtensionMenuShortcutIds() {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw === null ? [...DEFAULT_SHORTCUT_IDS] : normalizeShortcutIds(JSON.parse(raw));
}

/** @param {unknown} value */
export function setExtensionMenuShortcutIds(value) {
    const ids = normalizeShortcutIds(value);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(ids));
    return ids;
}

/** @param {(typeof SHORTCUTS)[number]} shortcut @param {(key: string) => string} translate */
function createMenuItem(shortcut, translate) {
    const item = document.createElement('div');
    item.id = `tauritavern_shortcut_${shortcut.id}`;
    item.className = 'list-group-item flex-container flexGap5';
    item.setAttribute('role', 'button');

    const icon = document.createElement('div');
    icon.className = `fa-fw fa-solid ${shortcut.icon} extensionsMenuExtensionButton`;

    const label = document.createElement('span');
    label.textContent = translate(shortcut.label);

    item.append(icon, label);
    return item;
}

/** @param {string[]} [ids] */
export async function renderExtensionMenuShortcuts(ids = getExtensionMenuShortcutIds()) {
    const selected = new Set(normalizeShortcutIds(ids));
    const [extensions, keyboard, i18n, popup] = await Promise.all([
        import('../../extensions.js'),
        import('../../keyboard.js'),
        import('../../i18n.js'),
        import('./setting-panel/popup-utils.js'),
    ]);
    await extensions.ensureExtensionsUiReady();

    const menu = document.getElementById('extensionsMenu');
    if (!(menu instanceof HTMLElement)) {
        throw new Error('TauriTavern quick access menu: extensions menu is unavailable');
    }

    let container = document.getElementById(CONTAINER_ID);
    if (!(container instanceof HTMLElement)) {
        container = document.createElement('div');
        container.id = CONTAINER_ID;
        container.className = 'extension_container';
        menu.prepend(container);
    }

    const items = SHORTCUTS
        .filter(shortcut => selected.has(shortcut.id) && (shortcut.available?.() ?? true))
        .map(shortcut => {
            const item = createMenuItem(shortcut, i18n.translate);
            item.addEventListener('click', () => popup.runOrPopup(shortcut.open));
            keyboard.makeKeyboardInteractable(item);
            return item;
        });

    container.replaceChildren(...items);
    extensions.showHideExtensionsMenu();
}

export async function openExtensionMenuShortcutsManager() {
    const selected = new Set(getExtensionMenuShortcutIds());
    const [{ POPUP_RESULT, POPUP_TYPE }, { translate }, { createTauriTavernPanelPopup }] = await Promise.all([
        import('../../popup.js'),
        import('../../i18n.js'),
        import('./panel-popup.js'),
    ]);

    const content = document.createElement('div');
    content.className = 'flex-container flexFlowColumn';
    const title = document.createElement('h3');
    title.className = 'margin0';
    title.textContent = translate('Quick Access Menu');
    const hint = document.createElement('small');
    hint.textContent = translate('Choose which TauriTavern tools appear in the wand menu beside the message box.');
    content.append(title, hint);

    const popup = createTauriTavernPanelPopup(content, POPUP_TYPE.CONFIRM, '', {
        okButton: translate('Save'),
        cancelButton: translate('Cancel'),
        leftAlign: true,
        customInputs: SHORTCUTS.map(shortcut => ({
            id: `tauritavern_shortcut_toggle_${shortcut.id}`,
            label: translate(shortcut.label),
            defaultState: selected.has(shortcut.id),
            tooltip: shortcut.available?.() === false
                ? translate('LAN Sync is disabled by the active iOS policy.')
                : undefined,
        })),
    });

    if (await popup.show() !== POPUP_RESULT.AFFIRMATIVE) {
        return;
    }

    const ids = SHORTCUTS
        .filter(shortcut => popup.inputResults?.get(`tauritavern_shortcut_toggle_${shortcut.id}`) === true)
        .map(shortcut => shortcut.id);
    setExtensionMenuShortcutIds(ids);
    await renderExtensionMenuShortcuts(ids);
}
