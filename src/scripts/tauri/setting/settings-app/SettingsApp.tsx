import { StrictMode, useState, useSyncExternalStore } from 'react';
import { createRoot } from 'react-dom/client';

import {
    CHAT_BACKUP_STORAGE_UNIT_BYTES,
    CHAT_BACKUP_STORAGE_UNITS,
    isZeroLimit,
    validateSettingsBoundary,
    type ChatBackupStorageUnit,
    type SettingsActions,
    type SettingsBackgroundOption,
    type SettingsCapabilities,
    type SettingsDataRootState,
    type SettingsHandle,
    type SettingsOption,
    type SettingsTranslate,
} from './SettingsContract';
import { SettingsAppearanceSection } from './SettingsAppearanceSection';
import {
    ActionButton,
    SelectField,
    SettingRow,
    SettingsSection,
    ToggleSwitch,
} from './SettingsComponents';
import { createSettingsController, type SettingsController } from './SettingsController';
import { SettingsSystemSection } from './SettingsSystemSection';
import { translateOptions, zstdCompressionHint } from './SettingsText';

const PANEL_RUNTIME_OPTIONS = [
    { value: 'compat', labelKey: 'Compact (Recommended)' },
    { value: 'aggressive', labelKey: 'Aggressive (More DOM Parking)' },
    { value: 'off', labelKey: 'Off (Legacy)' },
];

const EMBEDDED_RUNTIME_OPTIONS = [
    { value: 'auto', labelKey: 'Auto (Recommended)' },
    { value: 'compat', labelKey: 'Balanced' },
    { value: 'mobile-safe', labelKey: 'Power Saver' },
    { value: 'off', labelKey: 'Off (Legacy)' },
];

const PROMPT_CACHE_OPTIONS = [
    { value: 'off', labelKey: 'Off' },
    { value: '5m', labelKey: '5m (Default TTL)' },
    { value: '1h', labelKey: '1h (Extended)' },
];

type SettingsAppProps = {
    controller: SettingsController;
    capabilities: SettingsCapabilities;
    initialDataRoot: SettingsDataRootState | null;
    themeOptions: SettingsOption[];
    backgroundOptions: SettingsBackgroundOption[];
    currentBackground: string;
    actions: SettingsActions;
    tr: SettingsTranslate;
};

function SettingsApp({
    controller,
    capabilities,
    initialDataRoot,
    themeOptions,
    backgroundOptions,
    currentBackground,
    actions,
    tr,
}: SettingsAppProps) {
    const { draft, chatBackupStorageStats } = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const [appearanceOpen, setAppearanceOpen] = useState(false);

    const chatBackupHistoryDisabled = isZeroLimit(draft.chatBackups.maxFilesPerPrefix)
        || isZeroLimit(draft.chatBackups.maxTotalFiles)
        || isZeroLimit(draft.chatBackups.maxTotalValue);
    const zstdHint = zstdCompressionHint(tr, draft.chatBackups.zstdCompressionEnabled, chatBackupStorageStats);

    const showHelp = (topic: string) => {
        void actions.showHelp(topic);
    };

    function setChatBackupStorageUnit(value: string): void {
        const nextUnit: ChatBackupStorageUnit = value === 'GiB' ? 'GiB' : 'MiB';
        const { maxTotalUnit: currentUnit, maxTotalValue } = draft.chatBackups;
        const currentValue = Number(maxTotalValue);
        controller.patchChatBackups({
            maxTotalValue: nextUnit !== currentUnit && currentValue > 0
                ? String(currentValue * CHAT_BACKUP_STORAGE_UNIT_BYTES[currentUnit] / CHAT_BACKUP_STORAGE_UNIT_BYTES[nextUnit])
                : maxTotalValue,
            maxTotalUnit: nextUnit,
        });
    }

    return (
        <div className="tt-settings-root">
            <header className="tt-settings-header">
                <div>
                    <b>{tr('TauriTavern Settings')}</b>
                </div>
            </header>

            {capabilities.supportsCloseToTrayOnClose && (
                <SettingsSection title={tr('Interface')} icon="fa-window-minimize">
                    <SettingRow
                        label={tr('Minimize to tray on close (Windows)')}
                        helpTopic="closeToTray"
                        helpTitle={tr('Learn more')}
                        onHelp={showHelp}
                    >
                        <ToggleSwitch
                            checked={draft.closeToTrayOnClose}
                            ariaLabel={tr('Minimize to tray on close (Windows)')}
                            onChange={checked => controller.updateDraft('closeToTrayOnClose', checked)}
                        />
                    </SettingRow>
                </SettingsSection>
            )}

            <SettingsSection title={tr('Performance')} icon="fa-gauge-high">
                <SettingRow
                    label={tr('Panel Runtime')}
                    helpTopic="panelRuntime"
                    helpTitle={tr('Learn more')}
                    onHelp={showHelp}
                >
                    <SelectField
                        value={draft.panelRuntimeProfile}
                        options={translateOptions(PANEL_RUNTIME_OPTIONS, tr)}
                        ariaLabel={tr('Panel Runtime')}
                        onChange={value => controller.updateDraft('panelRuntimeProfile', value)}
                    />
                </SettingRow>

                <SettingRow
                    label={tr('Embedded Runtime')}
                    helpTopic="embeddedRuntime"
                    helpTitle={tr('Learn more')}
                    onHelp={showHelp}
                >
                    <SelectField
                        value={draft.embeddedRuntimeProfile}
                        options={translateOptions(EMBEDDED_RUNTIME_OPTIONS, tr)}
                        disabled={draft.chatVirtualizationEnabled}
                        ariaLabel={tr('Embedded Runtime')}
                        onChange={value => controller.updateDraft('embeddedRuntimeProfile', value)}
                    />
                </SettingRow>

                <SettingRow
                    label={tr('Chat DOM Virtualization')}
                    helpTopic="chatVirtualization"
                    helpTitle={tr('Learn more')}
                    onHelp={showHelp}
                >
                    <ToggleSwitch
                        checked={draft.chatVirtualizationEnabled}
                        ariaLabel={tr('Chat DOM Virtualization')}
                        onChange={checked => controller.updateDraft('chatVirtualizationEnabled', checked)}
                    />
                </SettingRow>

                <SettingRow
                    label={tr('CodeMirror Editor')}
                    helpTopic="codeMirrorEditor"
                    helpTitle={tr('Learn more')}
                    onHelp={showHelp}
                >
                    <ToggleSwitch
                        checked={draft.codeMirrorEditorEnabled}
                        ariaLabel={tr('CodeMirror Editor')}
                        onChange={checked => controller.updateDraft('codeMirrorEditorEnabled', checked)}
                    />
                </SettingRow>

                <SettingRow label={tr('Rust Regex Backend')}>
                    <ToggleSwitch
                        checked={draft.nativeRegexBackendEnabled}
                        ariaLabel={tr('Rust Regex Backend')}
                        onChange={checked => controller.updateDraft('nativeRegexBackendEnabled', checked)}
                    />
                </SettingRow>

                <small className="tt-settings-section-note">{tr('Requires reload to apply.')}</small>
            </SettingsSection>

            <SettingsSection title={tr('Chat Backups')} icon="fa-clock-rotate-left">
                <SettingRow
                    label={tr('Automatic Chat Backups')}
                    hint={tr('Create a backup automatically when an eligible chat save completes.')}
                >
                    <ToggleSwitch
                        checked={draft.chatBackups.automaticEnabled}
                        ariaLabel={tr('Automatic Chat Backups')}
                        onChange={checked => controller.patchChatBackups({ automaticEnabled: checked })}
                    />
                </SettingRow>

                <SettingRow
                    label={tr('zstd Compression')}
                    helpTopic="zstdCompression"
                    helpTitle={tr('Learn more')}
                    onHelp={showHelp}
                    hint={(
                        <>
                            {zstdHint.summary}
                            {zstdHint.saved && <br />}
                            {zstdHint.before}
                            {zstdHint.saved && <strong className="tt-settings-hint-accent">{zstdHint.saved}</strong>}
                            {zstdHint.after}
                        </>
                    )}
                >
                    <ToggleSwitch
                        checked={draft.chatBackups.zstdCompressionEnabled}
                        ariaLabel={tr('zstd Compression')}
                        onChange={checked => controller.patchChatBackups({ zstdCompressionEnabled: checked })}
                    />
                </SettingRow>

                <SettingRow
                    label={tr('Backups per character or group')}
                    hint={tr('Maximum backups sharing the same character or group name.')}
                >
                    <input
                        className="text_pole tt-settings-input"
                        type="number"
                        min="-1"
                        step="1"
                        value={draft.chatBackups.maxFilesPerPrefix}
                        aria-label={tr('Backups per character or group')}
                        onChange={event => controller.patchChatBackups({ maxFilesPerPrefix: event.target.value })}
                    />
                </SettingRow>

                <SettingRow label={tr('Total backup files')}>
                    <input
                        className="text_pole tt-settings-input"
                        type="number"
                        min="-1"
                        step="1"
                        value={draft.chatBackups.maxTotalFiles}
                        aria-label={tr('Total backup files')}
                        onChange={event => controller.patchChatBackups({ maxTotalFiles: event.target.value })}
                    />
                </SettingRow>

                <SettingRow label={tr('Backup storage limit')}>
                    <div className="tt-settings-number-with-unit">
                        <input
                            className="text_pole tt-settings-input"
                            type="number"
                            min="-1"
                            step="any"
                            value={draft.chatBackups.maxTotalValue}
                            aria-label={tr('Backup storage limit')}
                            onChange={event => controller.patchChatBackups({ maxTotalValue: event.target.value })}
                        />
                        <select
                            className="text_pole tt-settings-select"
                            value={draft.chatBackups.maxTotalUnit}
                            aria-label={tr('Storage unit')}
                            onChange={event => setChatBackupStorageUnit(event.target.value)}
                        >
                            {CHAT_BACKUP_STORAGE_UNITS.map(unit => (
                                <option key={unit} value={unit}>{unit}</option>
                            ))}
                        </select>
                    </div>
                </SettingRow>

                <small className="tt-settings-section-note">
                    {tr('Use -1 for unlimited. Oldest backups are removed first when a limit is exceeded.')}
                </small>
                {chatBackupHistoryDisabled && (
                    <small className="tt-settings-warning">
                        {tr('A limit of 0 disables chat backup history and deletes all existing chat backups in the background.')}
                    </small>
                )}
            </SettingsSection>

            <SettingsSystemSection
                capabilities={capabilities}
                proxy={draft.requestProxy}
                initialDataRoot={initialDataRoot}
                chooseDataRoot={actions.chooseDataRoot}
                tr={tr}
                onPatchProxy={patch => controller.patchRequestProxy(patch)}
            />

            <SettingsSection title={tr('Models')} icon="fa-brain">
                <SettingRow
                    label={tr('Claude Prompt Cache')}
                    helpTopic="claudePromptCache"
                    helpTitle={tr('Learn more')}
                    onHelp={showHelp}
                >
                    <SelectField
                        value={draft.promptCacheTtl}
                        options={translateOptions(PROMPT_CACHE_OPTIONS, tr)}
                        ariaLabel={tr('Claude Prompt Cache')}
                        onChange={value => controller.updateDraft('promptCacheTtl', value)}
                    />
                </SettingRow>
            </SettingsSection>

            <SettingsSection title={tr('Misc')} icon="fa-shapes">
                <SettingRow
                    label={tr('Allow Keys Exposure')}
                    helpTopic="allowKeysExposure"
                    helpTitle={tr('When enabled, API keys can be viewed/copied inside the app. Takes effect after restart.')}
                    onHelp={showHelp}
                >
                    <ToggleSwitch
                        checked={draft.allowKeysExposure}
                        ariaLabel={tr('Allow Keys Exposure')}
                        onChange={checked => controller.updateDraft('allowKeysExposure', checked)}
                    />
                </SettingRow>

                <SettingRow
                    label={tr('Enable Character/User Avatar Original Images')}
                    helpTopic="avatarPersonaOriginalImages"
                    helpTitle={tr('When enabled, character/user avatars load full-size images. Takes effect after reload.')}
                    onHelp={showHelp}
                >
                    <ToggleSwitch
                        checked={draft.avatarPersonaOriginalImagesEnabled}
                        ariaLabel={tr('Enable Character/User Avatar Original Images')}
                        onChange={checked => controller.updateDraft('avatarPersonaOriginalImagesEnabled', checked)}
                    />
                </SettingRow>

                <SettingsAppearanceSection
                    theme={draft.dynamicTheme}
                    open={appearanceOpen}
                    themeOptions={themeOptions}
                    backgroundOptions={backgroundOptions}
                    currentBackground={currentBackground}
                    chooseWallpaper={actions.chooseWallpaper}
                    tr={tr}
                    onOpenChange={setAppearanceOpen}
                    onPatch={patch => controller.patchDynamicTheme(patch)}
                    onShowHelp={showHelp}
                />
            </SettingsSection>

            <SettingsSection title={tr('Development')} icon="fa-code">
                <div className="tt-settings-action-grid">
                    <ActionButton label={tr('Manage Quick Access')} icon="fa-magic-wand-sparkles" onClick={() => void actions.manageQuickAccess()} />
                    <ActionButton label={tr('Reload Frontend')} icon="fa-arrows-rotate" onClick={() => void actions.reloadFrontend()} />
                    <ActionButton label={tr('Frontend Logs')} icon="fa-terminal" onClick={() => void actions.openFrontendLogs()} />
                    <ActionButton label={tr('Backend Logs')} icon="fa-server" onClick={() => void actions.openBackendLogs()} />
                    <ActionButton label={tr('LLM API Logs')} icon="fa-file-lines" onClick={() => void actions.openLlmApiLogs()} />
                </div>
            </SettingsSection>

            {capabilities.lanSyncAllowed && (
                <SettingsSection title={tr('Sync')} icon="fa-rotate">
                    <div className="tt-settings-action-grid">
                        <ActionButton label={tr('Open Panel')} icon="fa-up-right-from-square" onClick={() => void actions.openSync()} />
                    </div>
                </SettingsSection>
            )}
        </div>
    );
}

export function mountTauriTavernSettingsApp(
    mount: unknown,
    options: unknown,
): SettingsHandle {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern settings mount element is required');
    }
    validateSettingsBoundary(options);

    const controller = createSettingsController({
        values: options.viewModel.values,
        chatBackupStorageStats: options.viewModel.chatBackupStorageStats ?? null,
    });
    const root = createRoot(mount);
    let mounted = true;
    root.render(
        <StrictMode>
            <SettingsApp
                controller={controller}
                capabilities={options.viewModel.capabilities}
                initialDataRoot={options.viewModel.dataRoot}
                themeOptions={options.themeOptions ?? []}
                backgroundOptions={options.backgroundOptions ?? []}
                currentBackground={options.currentBackground ?? ''}
                actions={options.actions}
                tr={options.tr}
            />
        </StrictMode>,
    );

    return {
        getDraft: () => controller.getDraft(),
        setChatBackupStorageStats: stats => {
            // The popup may already be gone when the stats request resolves.
            if (mounted) {
                controller.setChatBackupStorageStats(stats);
            }
        },
        unmount: () => {
            mounted = false;
            root.unmount();
        },
    };
}
