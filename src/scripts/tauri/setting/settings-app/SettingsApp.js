import {
    ActionButton,
    SelectField,
    SettingRow,
    SettingsSection,
    ToggleSwitch,
    WallpaperField,
} from './components.js';
import { formatBytes } from '../format-bytes.js';

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

const CHAT_BACKUP_STORAGE_UNIT_BYTES = {
    MiB: 1024 * 1024,
    GiB: 1024 * 1024 * 1024,
};
const CHAT_BACKUP_STORAGE_UNIT_OPTIONS = Object.keys(CHAT_BACKUP_STORAGE_UNIT_BYTES);

function cloneOptions(options) {
    return options.map((option) => ({
        value: String(option.value || ''),
        label: String(option.label || option.value || ''),
        thumbnailUrl: String(option.thumbnailUrl || ''),
        isAnimated: Boolean(option.isAnimated),
    }));
}

function translateOptions(options, tr) {
    return options.map((option) => ({
        value: option.value,
        label: tr(option.labelKey),
    }));
}

function cloneDraft(values, themeOptions, backgroundOptions, currentBackground) {
    const fallbackTheme = themeOptions[0]?.value || '';
    const normalizedCurrentBackground = String(currentBackground || '');
    const fallbackBackground = backgroundOptions.some((option) => option.value === normalizedCurrentBackground)
        ? normalizedCurrentBackground
        : backgroundOptions[0]?.value || '';
    const maxTotalBytes = values.chatBackups.maxTotalBytes;
    const maxTotalUnit = maxTotalBytes >= CHAT_BACKUP_STORAGE_UNIT_BYTES.GiB ? 'GiB' : 'MiB';
    const maxTotalValue = maxTotalBytes > 0
        ? maxTotalBytes / CHAT_BACKUP_STORAGE_UNIT_BYTES[maxTotalUnit]
        : maxTotalBytes;

    return {
        panelRuntimeProfile: values.panelRuntimeProfile,
        embeddedRuntimeProfile: values.embeddedRuntimeProfile,
        chatVirtualizationEnabled: values.chatVirtualizationEnabled,
        chatBackups: {
            automaticEnabled: values.chatBackups.automaticEnabled,
            zstdCompressionEnabled: values.chatBackups.zstdCompressionEnabled,
            maxFilesPerPrefix: values.chatBackups.maxFilesPerPrefix,
            maxTotalFiles: values.chatBackups.maxTotalFiles,
            maxTotalValue,
            maxTotalUnit,
        },
        closeToTrayOnClose: values.closeToTrayOnClose,
        requestProxy: {
            enabled: values.requestProxy.enabled,
            url: values.requestProxy.url,
            bypass: values.requestProxy.bypass.join('\n'),
        },
        allowKeysExposure: values.allowKeysExposure,
        avatarPersonaOriginalImagesEnabled: values.avatarPersonaOriginalImagesEnabled,
        nativeRegexBackendEnabled: values.nativeRegexBackendEnabled,
        dynamicTheme: {
            themeEnabled: values.dynamicTheme.themeEnabled,
            dayTheme: values.dynamicTheme.dayTheme || fallbackTheme,
            nightTheme: values.dynamicTheme.nightTheme || fallbackTheme,
            wallpaperEnabled: values.dynamicTheme.wallpaperEnabled,
            dayWallpaper: values.dynamicTheme.dayWallpaper || (values.dynamicTheme.wallpaperEnabled ? fallbackBackground : ''),
            nightWallpaper: values.dynamicTheme.nightWallpaper || (values.dynamicTheme.wallpaperEnabled ? fallbackBackground : ''),
        },
        promptCacheTtl: values.promptCacheTtl,
    };
}

function cloneDataRoot(dataRoot) {
    if (!dataRoot) {
        return null;
    }

    return { ...dataRoot };
}

function requireAction(actions, name) {
    const action = actions?.[name];
    if (typeof action !== 'function') {
        throw new Error(`TauriTavern settings action is unavailable: ${name}`);
    }

    return action;
}

export function createTauriTavernSettingsApp(options) {
    const { viewModel, actions, tr } = options || {};
    if (!viewModel?.capabilities || !viewModel?.values) {
        throw new Error('TauriTavern settings view model is required');
    }
    if (typeof tr !== 'function') {
        throw new Error('TauriTavern settings translator is required');
    }

    const themeOptions = cloneOptions(options.themeOptions || []);
    const backgroundOptions = cloneOptions(options.backgroundOptions || []);
    const currentBackground = String(options.currentBackground || '');
    const capabilities = { ...viewModel.capabilities };
    const initialDraft = cloneDraft(viewModel.values, themeOptions, backgroundOptions, currentBackground);
    const initialDataRoot = cloneDataRoot(viewModel.dataRoot);
    const chatBackupStorageStats = viewModel.chatBackupStorageStats ?? null;

    return {
        name: 'TauriTavernSettingsApp',
        components: {
            ActionButton,
            SelectField,
            SettingRow,
            SettingsSection,
            ToggleSwitch,
            WallpaperField,
        },
        data() {
            return {
                capabilities,
                themeOptions,
                backgroundOptions,
                draft: initialDraft,
                dataRoot: initialDataRoot,
                chatBackupStorageStats,
                details: {
                    dataRoot: false,
                    requestProxy: initialDraft.requestProxy.enabled,
                    dynamicTheme: false,
                },
                busy: {
                    dataRoot: false,
                },
            };
        },
        computed: {
            systemVisible() {
                return this.capabilities.supportsDataRootSelection || this.capabilities.requestProxyAllowed;
            },
            panelRuntimeOptions() {
                return translateOptions(PANEL_RUNTIME_OPTIONS, this.tr);
            },
            embeddedRuntimeOptions() {
                return translateOptions(EMBEDDED_RUNTIME_OPTIONS, this.tr);
            },
            chatBackupStorageUnitOptions() {
                return CHAT_BACKUP_STORAGE_UNIT_OPTIONS;
            },
            chatBackupHistoryDisabled() {
                return this.draft.chatBackups.maxFilesPerPrefix === 0
                    || this.draft.chatBackups.maxTotalFiles === 0
                    || this.draft.chatBackups.maxTotalValue === 0;
            },
            zstdCompressionHint() {
                const base = this.tr(
                    'Saves substantial space, but SillyTavern cannot read this format.',
                );
                const originalBytes = this.chatBackupStorageStats?.originalBytes ?? 0;
                const storedBytes = this.chatBackupStorageStats?.storedBytes ?? 0;
                if (
                    !this.draft.chatBackups.zstdCompressionEnabled
                    || originalBytes <= storedBytes
                ) {
                    return { summary: base, before: '', saved: '', after: '' };
                }

                const ratio = Math.round(storedBytes / originalBytes * 100);
                const [before, after] = this.tr(
                    'Compressed backups currently use about {ratio}% of their original size and have saved about {saved}.',
                )
                    .replace('{ratio}', String(ratio))
                    .split('{saved}');
                return {
                    summary: base,
                    before,
                    saved: formatBytes(originalBytes - storedBytes),
                    after,
                };
            },
            promptCacheOptions() {
                return translateOptions(PROMPT_CACHE_OPTIONS, this.tr);
            },
            requestProxySummary() {
                return this.tr(this.details.requestProxy ? 'Click to collapse' : 'Click to expand');
            },
            dynamicAppearanceSummary() {
                return this.tr(this.details.dynamicTheme ? 'Click to collapse' : 'Click to expand');
            },
            dataRootSummary() {
                if (!this.dataRoot) {
                    return '';
                }
                if (this.dataRoot.migrationError) {
                    return this.tr('Data directory migration failed:');
                }
                if (this.dataRoot.migrationPending) {
                    return this.tr('Data directory migration is pending.');
                }
                return this.dataRoot.currentDataRoot;
            },
            dataRootStatus() {
                if (!this.dataRoot) {
                    return '';
                }
                if (this.dataRoot.migrationError) {
                    return `${this.tr('Data directory migration failed:')} ${this.dataRoot.migrationError}`;
                }
                if (this.dataRoot.migrationPending) {
                    const configuredLine = this.dataRoot.configuredDataRoot
                        ? `${this.tr('Configured data directory:')} ${this.dataRoot.configuredDataRoot}`
                        : '';
                    const pendingLine = this.tr('Data directory migration is pending.');
                    return configuredLine ? `${configuredLine}\n${pendingLine}` : pendingLine;
                }
                if (this.dataRoot.configuredDataRoot && this.dataRoot.configuredDataRoot !== this.dataRoot.currentDataRoot) {
                    return `${this.tr('Configured data directory:')} ${this.dataRoot.configuredDataRoot}`;
                }
                return '';
            },
        },
        methods: {
            tr(key) {
                return tr(key);
            },
            themeOptionsWithStored(storedValue) {
                const normalized = String(storedValue || '').trim();
                if (!normalized || this.themeOptions.some((option) => option.value === normalized)) {
                    return this.themeOptions;
                }

                return [
                    ...this.themeOptions,
                    { value: normalized, label: normalized },
                ];
            },
            backgroundOptionsWithStored(storedValue) {
                const normalized = String(storedValue || '');
                if (!normalized || this.backgroundOptions.some((option) => option.value === normalized)) {
                    return this.backgroundOptions;
                }

                return [
                    ...this.backgroundOptions,
                    {
                        value: normalized,
                        label: normalized,
                        thumbnailUrl: '',
                        isAnimated: false,
                    },
                ];
            },
            backgroundOption(storedValue) {
                const normalized = String(storedValue || '');
                return this.backgroundOptionsWithStored(normalized)
                    .find((option) => option.value === normalized) || null;
            },
            ensureWallpaperDefaults() {
                const fallback = this.backgroundOptions[0]?.value || '';
                if (!this.draft.dynamicTheme.dayWallpaper) {
                    this.draft.dynamicTheme.dayWallpaper = fallback;
                }
                if (!this.draft.dynamicTheme.nightWallpaper) {
                    this.draft.dynamicTheme.nightWallpaper = fallback;
                }
            },
            showHelp(topicId) {
                void requireAction(actions, 'showHelp')(topicId);
            },
            runAction(name) {
                void requireAction(actions, name)();
            },
            async chooseDataRoot() {
                this.busy.dataRoot = true;
                try {
                    const selected = await requireAction(actions, 'chooseDataRoot')();
                    if (!selected || !this.dataRoot) {
                        return;
                    }

                    this.dataRoot = {
                        ...this.dataRoot,
                        configuredDataRoot: selected,
                        migrationPending: true,
                        migrationError: '',
                    };
                } finally {
                    this.busy.dataRoot = false;
                }
            },
            setRequestProxyEnabled(enabled) {
                this.draft.requestProxy.enabled = enabled;
                if (!enabled) {
                    return;
                }
                this.details.requestProxy = true;
                this.$nextTick(() => this.$refs.requestProxyUrl?.focus?.());
            },
            setThemeSwitchingEnabled(enabled) {
                this.draft.dynamicTheme.themeEnabled = enabled;
                if (!enabled) {
                    return;
                }
                this.details.dynamicTheme = true;
                this.$nextTick(() => this.$refs.dynamicThemeDay?.$el?.focus?.());
            },
            setWallpaperSwitchingEnabled(enabled) {
                this.draft.dynamicTheme.wallpaperEnabled = enabled;
                if (!enabled) {
                    return;
                }

                this.ensureWallpaperDefaults();
                this.details.dynamicTheme = true;
            },
            setChatBackupStorageUnit(unit) {
                if (!Object.hasOwn(CHAT_BACKUP_STORAGE_UNIT_BYTES, unit)) {
                    return;
                }

                const currentUnit = this.draft.chatBackups.maxTotalUnit;
                const currentValue = Number(this.draft.chatBackups.maxTotalValue);
                if (unit !== currentUnit && currentValue > 0) {
                    this.draft.chatBackups.maxTotalValue = currentValue
                        * CHAT_BACKUP_STORAGE_UNIT_BYTES[currentUnit]
                        / CHAT_BACKUP_STORAGE_UNIT_BYTES[unit];
                }
                this.draft.chatBackups.maxTotalUnit = unit;
            },
            async chooseWallpaper(targetKey) {
                const selected = await requireAction(actions, 'chooseWallpaper')({
                    currentValue: this.draft.dynamicTheme[targetKey],
                });
                if (!selected) {
                    return;
                }

                this.draft.dynamicTheme[targetKey] = selected;
            },
            getDraft() {
                return {
                    panelRuntimeProfile: this.draft.panelRuntimeProfile,
                    embeddedRuntimeProfile: this.draft.embeddedRuntimeProfile,
                    chatVirtualizationEnabled: this.draft.chatVirtualizationEnabled,
                    chatBackups: { ...this.draft.chatBackups },
                    closeToTrayOnClose: this.draft.closeToTrayOnClose,
                    requestProxy: { ...this.draft.requestProxy },
                    allowKeysExposure: this.draft.allowKeysExposure,
                    avatarPersonaOriginalImagesEnabled: this.draft.avatarPersonaOriginalImagesEnabled,
                    nativeRegexBackendEnabled: this.draft.nativeRegexBackendEnabled,
                    dynamicTheme: { ...this.draft.dynamicTheme },
                    promptCacheTtl: this.draft.promptCacheTtl,
                };
            },
        },
        template: `
            <div class="tt-settings-root">
                <header class="tt-settings-header">
                    <div>
                        <b>{{ tr('TauriTavern Settings') }}</b>
                    </div>
                </header>

                <SettingsSection
                    v-if="capabilities.supportsCloseToTrayOnClose"
                    :title="tr('Interface')"
                    icon="fa-window-minimize"
                >
                    <SettingRow
                        :label="tr('Minimize to tray on close (Windows)')"
                        help-topic="closeToTray"
                        :help-title="tr('Learn more')"
                        @help="showHelp"
                    >
                        <ToggleSwitch v-model="draft.closeToTrayOnClose" />
                    </SettingRow>
                </SettingsSection>

                <SettingsSection :title="tr('Performance')" icon="fa-gauge-high">
                    <SettingRow
                        :label="tr('Panel Runtime')"
                        help-topic="panelRuntime"
                        :help-title="tr('Learn more')"
                        @help="showHelp"
                    >
                        <SelectField v-model="draft.panelRuntimeProfile" :options="panelRuntimeOptions" />
                    </SettingRow>

                    <SettingRow
                        :label="tr('Embedded Runtime')"
                        help-topic="embeddedRuntime"
                        :help-title="tr('Learn more')"
                        @help="showHelp"
                    >
                        <SelectField
                            v-model="draft.embeddedRuntimeProfile"
                            :options="embeddedRuntimeOptions"
                            :disabled="draft.chatVirtualizationEnabled"
                        />
                    </SettingRow>

                    <SettingRow
                        :label="tr('Chat DOM Virtualization')"
                        help-topic="chatVirtualization"
                        :help-title="tr('Learn more')"
                        @help="showHelp"
                    >
                        <ToggleSwitch v-model="draft.chatVirtualizationEnabled" />
                    </SettingRow>

                    <SettingRow :label="tr('Rust Regex Backend')">
                        <ToggleSwitch v-model="draft.nativeRegexBackendEnabled" />
                    </SettingRow>

                    <small class="tt-settings-section-note">{{ tr('Requires reload to apply.') }}</small>
                </SettingsSection>

                <SettingsSection :title="tr('Chat Backups')" icon="fa-clock-rotate-left">
                    <SettingRow
                        :label="tr('Automatic Chat Backups')"
                        :hint="tr('Create a backup automatically when an eligible chat save completes.')"
                    >
                        <ToggleSwitch v-model="draft.chatBackups.automaticEnabled" />
                    </SettingRow>

                    <SettingRow
                        :label="tr('zstd Compression')"
                        help-topic="zstdCompression"
                        :help-title="tr('Learn more')"
                        @help="showHelp"
                    >
                        <template #hint>
                            {{ zstdCompressionHint.summary }}<br v-if="zstdCompressionHint.saved" />{{ zstdCompressionHint.before }}<strong
                                v-if="zstdCompressionHint.saved"
                                class="tt-settings-hint-accent"
                            >{{ zstdCompressionHint.saved }}</strong>{{ zstdCompressionHint.after }}
                        </template>
                        <ToggleSwitch v-model="draft.chatBackups.zstdCompressionEnabled" />
                    </SettingRow>

                    <SettingRow
                        :label="tr('Backups per character or group')"
                        :hint="tr('Maximum backups sharing the same character or group name.')"
                    >
                        <input
                            v-model.number="draft.chatBackups.maxFilesPerPrefix"
                            class="text_pole tt-settings-input"
                            type="number"
                            min="-1"
                            step="1"
                        />
                    </SettingRow>

                    <SettingRow :label="tr('Total backup files')">
                        <input
                            v-model.number="draft.chatBackups.maxTotalFiles"
                            class="text_pole tt-settings-input"
                            type="number"
                            min="-1"
                            step="1"
                        />
                    </SettingRow>

                    <SettingRow :label="tr('Backup storage limit')">
                        <div class="tt-settings-number-with-unit">
                            <input
                                v-model.number="draft.chatBackups.maxTotalValue"
                                class="text_pole tt-settings-input"
                                type="number"
                                min="-1"
                                step="any"
                            />
                            <select
                                class="text_pole tt-settings-select"
                                :value="draft.chatBackups.maxTotalUnit"
                                :aria-label="tr('Storage unit')"
                                @change="setChatBackupStorageUnit($event.target.value)"
                            >
                                <option
                                    v-for="unit in chatBackupStorageUnitOptions"
                                    :key="unit"
                                    :value="unit"
                                >
                                    {{ unit }}
                                </option>
                            </select>
                        </div>
                    </SettingRow>

                    <small class="tt-settings-section-note">
                        {{ tr('Use -1 for unlimited. Oldest backups are removed first when a limit is exceeded.') }}
                    </small>
                    <small
                        v-if="chatBackupHistoryDisabled"
                        class="tt-settings-warning"
                    >
                        {{ tr('A limit of 0 disables chat backup history and deletes all existing chat backups in the background.') }}
                    </small>
                </SettingsSection>

                <SettingsSection v-if="systemVisible" :title="tr('System')" icon="fa-sliders">
                    <details
                        v-if="capabilities.supportsDataRootSelection && dataRoot"
                        class="tt-settings-disclosure"
                        :open="details.dataRoot"
                        @toggle="details.dataRoot = $event.currentTarget.open"
                    >
                        <summary>
                            <span>{{ tr('Data Directory') }}</span>
                            <span class="tt-settings-summary-meta">
                                <small>{{ dataRootSummary }}</small>
                                <i class="fa-solid fa-chevron-down" aria-hidden="true"></i>
                            </span>
                        </summary>
                        <div class="tt-settings-disclosure-body">
                            <SettingRow :label="tr('Data Directory')">
                                <div class="tt-settings-path-row">
                                    <input class="text_pole" type="text" readonly :value="dataRoot.currentDataRoot" />
                                    <ActionButton
                                        :label="tr('Choose...')"
                                        icon="fa-folder-open"
                                        :disabled="busy.dataRoot"
                                        @click="chooseDataRoot"
                                    />
                                </div>
                            </SettingRow>
                            <small v-if="dataRootStatus" class="tt-settings-status">{{ dataRootStatus }}</small>
                            <small class="tt-settings-section-note">{{ tr('Data Directory hint') }}</small>
                        </div>
                    </details>

                    <details
                        v-if="capabilities.requestProxyAllowed"
                        class="tt-settings-disclosure"
                        :open="details.requestProxy"
                        @toggle="details.requestProxy = $event.currentTarget.open"
                    >
                        <summary>
                            <span>{{ tr('Request Proxy (Advanced)') }}</span>
                            <span class="tt-settings-summary-meta">
                                <small>{{ requestProxySummary }}</small>
                                <i class="fa-solid fa-chevron-down" aria-hidden="true"></i>
                            </span>
                        </summary>
                        <div class="tt-settings-disclosure-body">
                            <SettingRow :label="tr('Enable Request Proxy')">
                                <ToggleSwitch
                                    :model-value="draft.requestProxy.enabled"
                                    @update:model-value="setRequestProxyEnabled"
                                />
                            </SettingRow>
                            <SettingRow :label="tr('Request Proxy URL')">
                                <input
                                    ref="requestProxyUrl"
                                    v-model="draft.requestProxy.url"
                                    class="text_pole tt-settings-input"
                                    type="text"
                                    :disabled="!draft.requestProxy.enabled"
                                    placeholder="http://127.0.0.1:7890"
                                />
                            </SettingRow>
                            <div class="tt-settings-stack">
                                <span>{{ tr('Bypass (one per line)') }}</span>
                                <textarea
                                    v-model="draft.requestProxy.bypass"
                                    rows="6"
                                    :disabled="!draft.requestProxy.enabled"
                                    placeholder="localhost&#10;127.0.0.1&#10;10.0.0.0/8"
                                ></textarea>
                                <small class="tt-settings-section-note">{{ tr('Matching hosts will connect directly (no proxy).') }}</small>
                            </div>
                            <small class="tt-settings-section-note">{{ tr('Applies to all backend requests.') }}</small>
                        </div>
                    </details>
                </SettingsSection>

                <SettingsSection :title="tr('Models')" icon="fa-brain">
                    <SettingRow
                        :label="tr('Claude Prompt Cache')"
                        help-topic="claudePromptCache"
                        :help-title="tr('Learn more')"
                        @help="showHelp"
                    >
                        <SelectField v-model="draft.promptCacheTtl" :options="promptCacheOptions" />
                    </SettingRow>
                </SettingsSection>

                <SettingsSection :title="tr('Misc')" icon="fa-shapes">
                    <SettingRow
                        :label="tr('Allow Keys Exposure')"
                        help-topic="allowKeysExposure"
                        :help-title="tr('When enabled, API keys can be viewed/copied inside the app. Takes effect after restart.')"
                        @help="showHelp"
                    >
                        <ToggleSwitch v-model="draft.allowKeysExposure" />
                    </SettingRow>

                    <SettingRow
                        :label="tr('Enable Character/User Avatar Original Images')"
                        help-topic="avatarPersonaOriginalImages"
                        :help-title="tr('When enabled, character/user avatars load full-size images. Takes effect after reload.')"
                        @help="showHelp"
                    >
                        <ToggleSwitch v-model="draft.avatarPersonaOriginalImagesEnabled" />
                    </SettingRow>

                    <details
                        class="tt-settings-disclosure"
                        :open="details.dynamicTheme"
                        @toggle="details.dynamicTheme = $event.currentTarget.open"
                    >
                        <summary>
                            <span>{{ tr('Dynamic Theme & Wallpaper') }}</span>
                            <span class="tt-settings-summary-meta">
                                <small>{{ dynamicAppearanceSummary }}</small>
                                <i class="fa-solid fa-chevron-down" aria-hidden="true"></i>
                            </span>
                        </summary>
                        <div class="tt-settings-disclosure-body">
                            <SettingRow
                                :label="tr('Enable Theme Switching')"
                                help-topic="dynamicTheme"
                                :help-title="tr('Learn more')"
                                @help="showHelp"
                            >
                                <ToggleSwitch
                                    :model-value="draft.dynamicTheme.themeEnabled"
                                    @update:model-value="setThemeSwitchingEnabled"
                                />
                            </SettingRow>
                            <SettingRow :label="tr('Day Theme')">
                                <SelectField
                                    ref="dynamicThemeDay"
                                    v-model="draft.dynamicTheme.dayTheme"
                                    :options="themeOptionsWithStored(draft.dynamicTheme.dayTheme)"
                                    :disabled="!draft.dynamicTheme.themeEnabled"
                                />
                            </SettingRow>
                            <SettingRow :label="tr('Night Theme')">
                                <SelectField
                                    v-model="draft.dynamicTheme.nightTheme"
                                    :options="themeOptionsWithStored(draft.dynamicTheme.nightTheme)"
                                    :disabled="!draft.dynamicTheme.themeEnabled"
                                />
                            </SettingRow>
                            <SettingRow :label="tr('Enable Wallpaper Switching')">
                                <ToggleSwitch
                                    :model-value="draft.dynamicTheme.wallpaperEnabled"
                                    @update:model-value="setWallpaperSwitchingEnabled"
                                />
                            </SettingRow>
                            <SettingRow :label="tr('Day Wallpaper')">
                                <WallpaperField
                                    :option="backgroundOption(draft.dynamicTheme.dayWallpaper)"
                                    :value="draft.dynamicTheme.dayWallpaper"
                                    :placeholder="tr('Choose Wallpaper')"
                                    :disabled="!draft.dynamicTheme.wallpaperEnabled"
                                    @choose="chooseWallpaper('dayWallpaper')"
                                />
                            </SettingRow>
                            <SettingRow :label="tr('Night Wallpaper')">
                                <WallpaperField
                                    :option="backgroundOption(draft.dynamicTheme.nightWallpaper)"
                                    :value="draft.dynamicTheme.nightWallpaper"
                                    :placeholder="tr('Choose Wallpaper')"
                                    :disabled="!draft.dynamicTheme.wallpaperEnabled"
                                    @choose="chooseWallpaper('nightWallpaper')"
                                />
                            </SettingRow>
                            <small class="tt-settings-section-note">{{ tr('Dynamic Theme & Wallpaper hint') }}</small>
                        </div>
                    </details>
                </SettingsSection>

                <SettingsSection :title="tr('Development')" icon="fa-code">
                    <div class="tt-settings-action-grid">
                        <ActionButton :label="tr('Reload Frontend')" icon="fa-arrows-rotate" @click="runAction('reloadFrontend')" />
                        <ActionButton :label="tr('Frontend Logs')" icon="fa-terminal" @click="runAction('openFrontendLogs')" />
                        <ActionButton :label="tr('Backend Logs')" icon="fa-server" @click="runAction('openBackendLogs')" />
                        <ActionButton :label="tr('LLM API Logs')" icon="fa-file-lines" @click="runAction('openLlmApiLogs')" />
                    </div>
                </SettingsSection>

                <SettingsSection v-if="capabilities.lanSyncAllowed" :title="tr('Sync')" icon="fa-rotate">
                    <div class="tt-settings-action-grid">
                        <ActionButton :label="tr('Open Panel')" icon="fa-up-right-from-square" @click="runAction('openSync')" />
                    </div>
                </SettingsSection>
            </div>
        `,
    };
}
