import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../../popup.js';
import { translate } from '../../../i18n.js';
import {
    openDialog,
    setDataRoot,
    updateTauriTavernSettings,
} from '../../../../tauri-bridge.js';
import { runTaskOrPopup } from './popup-utils.js';
import { loadTauriTavernSettingsViewModel } from './settings-view-model.js';
import { buildTauriTavernSettingsUpdate } from './settings-patch.js';
import { applyTauriTavernSettingsUpdateEffects } from './settings-effects.js';
import { callTauriTavernPanelPopup } from '../panel-popup.js';

const SETTINGS_STYLE_ID = 'tauritavern-settings-style';

const HELP_TOPICS = {
    panelRuntime: {
        title: 'Panel Runtime',
        lines: [
            'Panel Runtime help: compact',
            'Panel Runtime help: aggressive',
            'Panel Runtime help: off',
        ],
    },
    embeddedRuntime: {
        title: 'Embedded Runtime',
        lines: [
            'Embedded Runtime help: off',
            'Embedded Runtime help: auto',
            'Embedded Runtime help: balanced',
            'Embedded Runtime help: saver',
        ],
    },
    chatHistory: {
        title: 'Chat History',
        lines: [
            'Chat History help: windowed',
            'Chat History help: off',
        ],
    },
    closeToTray: {
        title: 'Minimize to tray on close (Windows)',
        lines: [
            'Minimize to tray help: on',
            'Minimize to tray help: off',
            'Minimize to tray help: exit',
        ],
    },
    claudePromptCache: {
        title: 'Claude Prompt Cache',
        lines: [
            'Prompt Cache help: scope',
            'Prompt Cache help: breakpoint',
        ],
    },
    allowKeysExposure: {
        title: 'Allow Keys Exposure',
        lines: [
            'When enabled, API keys can be viewed/copied inside the app. Takes effect after restart.',
        ],
    },
    avatarPersonaOriginalImages: {
        title: 'Enable Character/User Avatar Original Images',
        lines: [
            'Character/User avatar originals help: on',
            'Character/User avatar originals help: off',
        ],
    },
    dynamicTheme: {
        title: 'Dynamic Theme',
        lines: [
            'Dynamic Theme help: mapping',
            'Dynamic Theme help: behavior',
        ],
    },
};

function ensureSettingsStyle() {
    if (document.getElementById(SETTINGS_STYLE_ID)) {
        return;
    }

    const link = document.createElement('link');
    link.id = SETTINGS_STYLE_ID;
    link.rel = 'stylesheet';
    link.href = new URL('./settings-app.css', import.meta.url).href;
    document.head.appendChild(link);
}

async function importSettingsBundle() {
    return import(new URL('../dist/settings.bundle.js', import.meta.url).href);
}

function readThemeOptions() {
    const upstreamThemeSelect = document.getElementById('themes');
    if (!(upstreamThemeSelect instanceof HTMLSelectElement)) {
        throw new Error('TauriTavern settings: TauriTavern theme selector not found');
    }

    return Array.from(upstreamThemeSelect.options).map((option) => ({
        value: option.value,
        label: option.textContent || option.value,
    }));
}

function createPopupColumn() {
    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '10px';
    return root;
}

async function showHelpTopic(topicId) {
    const topic = HELP_TOPICS[topicId];
    if (!topic) {
        throw new Error(`Unknown TauriTavern settings help topic: ${topicId}`);
    }

    const content = createPopupColumn();
    const title = document.createElement('b');
    title.textContent = translate(topic.title);
    content.appendChild(title);

    for (const lineKey of topic.lines) {
        const line = document.createElement('div');
        line.textContent = translate(lineKey);
        content.appendChild(line);
    }

    await callGenericPopup(content, POPUP_TYPE.TEXT, '', {
        okButton: translate('Close'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });
}

async function chooseDataRoot() {
    const picked = await openDialog({
        directory: true,
        multiple: false,
        title: translate('Select Data Directory'),
    });

    const selected = Array.isArray(picked) ? picked[0] : picked;
    const normalized = String(selected || '').trim();
    if (!normalized) {
        return null;
    }

    const confirm = createPopupColumn();
    const title = document.createElement('b');
    title.textContent = translate('Change Data Directory');
    const hint = document.createElement('div');
    hint.textContent = translate('The app will migrate data on next startup. Restart is required.');
    const pathPreview = document.createElement('pre');
    pathPreview.style.margin = '0';
    pathPreview.style.whiteSpace = 'pre-wrap';
    pathPreview.textContent = normalized;
    confirm.append(title, hint, pathPreview);

    const confirmation = await callGenericPopup(confirm, POPUP_TYPE.CONFIRM, '', {
        okButton: translate('Confirm'),
        cancelButton: translate('Cancel'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });
    if (confirmation !== POPUP_RESULT.AFFIRMATIVE) {
        return null;
    }

    await setDataRoot(normalized);

    await callGenericPopup(translate('Data directory saved. Restart to apply.'), POPUP_TYPE.TEXT, '', {
        okButton: translate('OK'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });

    return normalized;
}

function createSettingsActions() {
    return {
        chooseDataRoot: () => runTaskOrPopup(chooseDataRoot),
        showHelp: (topicId) => runTaskOrPopup(() => showHelpTopic(topicId)),
        reloadFrontend: () => runTaskOrPopup(async () => {
            window.location.reload();
        }),
        openFrontendLogs: () => runTaskOrPopup(async () => {
            const { openFrontendLogsPanel } = await import('../dev-logs.js');
            await openFrontendLogsPanel();
        }),
        openBackendLogs: () => runTaskOrPopup(async () => {
            const { openBackendLogsPanel } = await import('../dev-logs.js');
            await openBackendLogsPanel();
        }),
        openLlmApiLogs: () => runTaskOrPopup(async () => {
            const { openLlmApiLogsPanel } = await import('../dev-logs.js');
            await openLlmApiLogsPanel();
        }),
        openSync: () => runTaskOrPopup(async () => {
            const { openSyncPopup } = await import('./sync-popup.js');
            await openSyncPopup();
        }),
    };
}

export async function openTauriTavernSettingsPopup() {
    ensureSettingsStyle();

    const [viewModel, bundle] = await Promise.all([
        loadTauriTavernSettingsViewModel(),
        importSettingsBundle(),
    ]);

    const mount = document.createElement('div');
    const appHandle = bundle.mountTauriTavernSettingsApp(mount, {
        viewModel,
        themeOptions: readThemeOptions(),
        actions: createSettingsActions(),
        tr: translate,
    });

    try {
        const result = await callTauriTavernPanelPopup(mount, POPUP_TYPE.CONFIRM, '', {
            okButton: translate('Save'),
            cancelButton: translate('Close'),
            allowVerticalScrolling: true,
            wider: true,
            wide: false,
            large: false,
        });

        if (result !== POPUP_RESULT.AFFIRMATIVE) {
            return;
        }

        const update = buildTauriTavernSettingsUpdate(viewModel.values, appHandle.getDraft());
        if (!update.hasChanges) {
            return;
        }

        const updatedSettings = await updateTauriTavernSettings(update.patch);
        applyTauriTavernSettingsUpdateEffects(update, updatedSettings);
    } finally {
        appHandle.unmount();
    }
}
