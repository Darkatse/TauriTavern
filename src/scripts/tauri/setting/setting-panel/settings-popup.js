import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../../popup.js';
import { translate } from '../../../i18n.js';
import {
    openDialog,
    setDataRoot,
    updateTauriTavernSettings,
} from '../../../../tauri-bridge.js';
import { runTaskOrPopup, showErrorPopup } from './popup-utils.js';
import {
    loadChatBackupStorageStats,
    loadTauriTavernSettingsViewModel,
} from './settings-view-model.js';
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
    chatVirtualization: {
        title: 'Chat DOM Virtualization',
        lines: [
            'Chat DOM virtualization fully solves the performance problems caused by rendering many messages in very long chats.',
            'When enabled, only messages near the viewport and the newest message stay mounted. Turn it off to use upstream SillyTavern chat rendering. Renderer extensions must support ChatSurface.',
        ],
    },
    zstdCompression: {
        title: 'zstd Compression',
        lines: [
            'zstd compression applies to all chat backups.',
            'When this setting changes, existing backups are converted in the background.',
            'TauriTavern can read zstd backups directly. SillyTavern does not support this backup format.',
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
        title: 'Dynamic Theme & Wallpaper',
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

function getBackgroundThumbnailUrl(filename, isAnimated) {
    if (filename.toLowerCase().endsWith('.mp4')) {
        return '/img/No-Image-Placeholder.svg';
    }

    const url = typeof window.__TAURITAVERN_THUMBNAIL__ === 'function'
        ? window.__TAURITAVERN_THUMBNAIL__('bg', filename)
        : `/thumbnail?type=bg&file=${encodeURIComponent(filename)}`;
    return isAnimated ? `${url}&static=true` : url;
}

async function readBackgroundOptions() {
    const {
        background_settings,
        refreshSystemBackgroundEntries,
    } = await import('../../../backgrounds.js');

    const backgroundEntries = await refreshSystemBackgroundEntries();

    return {
        currentBackground: String(background_settings.name || ''),
        backgroundOptions: backgroundEntries.map((entry) => ({
            value: entry.filename,
            label: entry.filename,
            thumbnailUrl: getBackgroundThumbnailUrl(entry.filename, entry.isAnimated),
            isAnimated: entry.isAnimated,
        })),
    };
}

function createPopupColumn() {
    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '10px';
    return root;
}

function setWallpaperPickerSelection(root, selectedValue) {
    for (const item of root.querySelectorAll('.tt-wallpaper-picker-item')) {
        item.classList.toggle('is-selected', item.getAttribute('data-value') === selectedValue);
    }
}

function createWallpaperPickerItem(option, selected, onSelect) {
    const item = document.createElement('button');
    item.type = 'button';
    item.className = 'tt-wallpaper-picker-item';
    item.dataset.value = option.value;
    item.title = option.label;

    const thumbnail = document.createElement('span');
    thumbnail.className = 'tt-wallpaper-picker-thumb';
    thumbnail.style.backgroundImage = `url("${option.thumbnailUrl}")`;

    const label = document.createElement('span');
    label.className = 'tt-wallpaper-picker-label';
    label.textContent = option.label;

    item.append(thumbnail, label);
    item.classList.toggle('is-selected', selected === option.value);
    item.addEventListener('click', () => onSelect(option.value));

    return item;
}

async function chooseWallpaper(backgroundOptions, request = {}) {
    if (backgroundOptions.length === 0) {
        await callGenericPopup(translate('No backgrounds available'), POPUP_TYPE.TEXT, '', {
            okButton: translate('OK'),
            allowVerticalScrolling: true,
            wide: false,
            large: false,
        });
        return null;
    }

    const requestedValue = String(request?.currentValue || '');
    let selected = backgroundOptions.some((option) => option.value === requestedValue)
        ? requestedValue
        : backgroundOptions[0].value;

    const picker = document.createElement('div');
    picker.className = 'tt-wallpaper-picker';

    for (const option of backgroundOptions) {
        picker.appendChild(createWallpaperPickerItem(option, selected, (nextValue) => {
            selected = nextValue;
            setWallpaperPickerSelection(picker, selected);
        }));
    }

    const result = await callGenericPopup(picker, POPUP_TYPE.CONFIRM, '', {
        okButton: translate('Select'),
        cancelButton: translate('Cancel'),
        allowVerticalScrolling: true,
        wide: true,
        large: false,
    });

    return result === POPUP_RESULT.AFFIRMATIVE ? selected : null;
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

async function showChatVirtualizationCompatibility() {
    const content = createPopupColumn();
    const title = document.createElement('b');
    title.textContent = translate('ChatSurface renderer requirements');
    const explanation = document.createElement('div');
    explanation.textContent = translate(
        'Chat DOM Virtualization requires renderer extensions that support ChatSurface. Use the official versions listed below or later:',
    );
    content.append(title, explanation);

    for (const [labelKey, repository, href] of [
        ['JS-Slash-Runner 4.9.1 or later:', 'N0VI028/JS-Slash-Runner', 'https://github.com/N0VI028/JS-Slash-Runner'],
        ['LittleWhiteBox 3.0.4 or later:', 'RT15548/LittleWhiteBox', 'https://github.com/RT15548/LittleWhiteBox'],
    ]) {
        const line = document.createElement('div');
        const link = document.createElement('a');
        link.href = href;
        link.target = '_blank';
        link.rel = 'noopener noreferrer';
        link.textContent = repository;
        line.append(`${translate(labelKey)} `, link);
        content.appendChild(line);
    }

    const repositoryNotice = document.createElement('div');
    repositoryNotice.textContent = translate(
        'If an extension was installed from another repository, reinstall it from the official repository to receive upstream updates.',
    );
    content.appendChild(repositoryNotice);

    await callGenericPopup(content, POPUP_TYPE.TEXT, '', {
        okButton: translate('Close'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });
}

async function confirmChatBackupHistoryPurge() {
    const content = createPopupColumn();
    const title = document.createElement('b');
    title.textContent = translate('Disable chat backup history?');
    const explanation = document.createElement('div');
    explanation.textContent = translate(
        'Setting any chat backup limit to 0 deletes all existing chat backups in the background. This cannot be undone.',
    );
    content.append(title, explanation);

    const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
        okButton: translate('Disable and delete backups'),
        cancelButton: translate('Cancel'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });

    return result === POPUP_RESULT.AFFIRMATIVE;
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

function createSettingsActions(backgroundOptions) {
    return {
        chooseDataRoot: () => runTaskOrPopup(chooseDataRoot),
        chooseWallpaper: (request) => runTaskOrPopup(() => chooseWallpaper(backgroundOptions, request)),
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

    const [viewModel, bundle, backgroundModel] = await Promise.all([
        loadTauriTavernSettingsViewModel(),
        importSettingsBundle(),
        readBackgroundOptions(),
    ]);

    const mount = document.createElement('div');
    const appHandle = bundle.mountTauriTavernSettingsApp(mount, {
        viewModel,
        themeOptions: readThemeOptions(),
        backgroundOptions: backgroundModel.backgroundOptions,
        currentBackground: backgroundModel.currentBackground,
        actions: createSettingsActions(backgroundModel.backgroundOptions),
        tr: translate,
    });
    let pendingUpdate = null;

    try {
        const popupPromise = callTauriTavernPanelPopup(mount, POPUP_TYPE.CONFIRM, '', {
            okButton: translate('Save'),
            cancelButton: translate('Close'),
            allowVerticalScrolling: true,
            wider: true,
            wide: false,
            large: false,
            onClosing: async (popup) => {
                if (popup.result !== POPUP_RESULT.AFFIRMATIVE) {
                    return true;
                }

                try {
                    pendingUpdate = buildTauriTavernSettingsUpdate(viewModel.values, appHandle.getDraft());
                } catch (error) {
                    await showErrorPopup(error);
                    return false;
                }

                if (
                    pendingUpdate.requiresChatBackupPurgeConfirmation
                    && !await confirmChatBackupHistoryPurge()
                ) {
                    return false;
                }

                if (
                    pendingUpdate.changes.chatVirtualizationEnabled
                    && pendingUpdate.next.chatVirtualizationEnabled
                ) {
                    await showChatVirtualizationCompatibility();
                }

                return true;
            },
        });
        if (viewModel.values.chatBackups.zstdCompressionEnabled) {
            void loadChatBackupStorageStats()
                .then(appHandle.setChatBackupStorageStats)
                .catch(error => console.warn('Chat backup storage stats are unavailable:', error));
        }

        const result = await popupPromise;

        if (result !== POPUP_RESULT.AFFIRMATIVE) {
            return;
        }

        const update = pendingUpdate
            ?? buildTauriTavernSettingsUpdate(viewModel.values, appHandle.getDraft());
        if (!update.hasChanges) {
            return;
        }

        const updatedSettings = await updateTauriTavernSettings(update.patch);
        applyTauriTavernSettingsUpdateEffects(update, updatedSettings);
    } finally {
        appHandle.unmount();
    }
}
