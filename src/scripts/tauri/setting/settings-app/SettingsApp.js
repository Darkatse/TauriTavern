import {
    ActionButton,
    SelectField,
    SettingRow,
    SettingsSection,
    ToggleSwitch,
} from './components.js';

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

const CHAT_HISTORY_OPTIONS = [
    { value: 'windowed', labelKey: 'Windowed (Recommended)' },
    { value: 'off', labelKey: 'Off (Upstream full history)' },
];

const PROMPT_CACHE_OPTIONS = [
    { value: 'off', labelKey: 'Off' },
    { value: '5m', labelKey: '5m (Default TTL)' },
    { value: '1h', labelKey: '1h (Extended)' },
];

function cloneOptions(options) {
    return options.map((option) => ({
        value: String(option.value || '').trim(),
        label: String(option.label || option.value || '').trim(),
    }));
}

function translateOptions(options, tr) {
    return options.map((option) => ({
        value: option.value,
        label: tr(option.labelKey),
    }));
}

function cloneDraft(values, themeOptions) {
    const fallbackTheme = themeOptions[0]?.value || '';

    return {
        panelRuntimeProfile: values.panelRuntimeProfile,
        embeddedRuntimeProfile: values.embeddedRuntimeProfile,
        chatHistoryMode: values.chatHistoryMode,
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
            enabled: values.dynamicTheme.enabled,
            dayTheme: values.dynamicTheme.dayTheme || fallbackTheme,
            nightTheme: values.dynamicTheme.nightTheme || fallbackTheme,
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
    const capabilities = { ...viewModel.capabilities };
    const initialDraft = cloneDraft(viewModel.values, themeOptions);
    const initialDataRoot = cloneDataRoot(viewModel.dataRoot);

    return {
        name: 'TauriTavernSettingsApp',
        components: {
            ActionButton,
            SelectField,
            SettingRow,
            SettingsSection,
            ToggleSwitch,
        },
        data() {
            return {
                capabilities,
                themeOptions,
                draft: initialDraft,
                dataRoot: initialDataRoot,
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
            chatHistoryOptions() {
                return translateOptions(CHAT_HISTORY_OPTIONS, this.tr);
            },
            promptCacheOptions() {
                return translateOptions(PROMPT_CACHE_OPTIONS, this.tr);
            },
            requestProxySummary() {
                return this.tr(this.details.requestProxy ? 'Click to collapse' : 'Click to expand');
            },
            dynamicThemeSummary() {
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
            setDynamicThemeEnabled(enabled) {
                this.draft.dynamicTheme.enabled = enabled;
                if (!enabled) {
                    return;
                }
                this.details.dynamicTheme = true;
                this.$nextTick(() => this.$refs.dynamicThemeDay?.$el?.focus?.());
            },
            getDraft() {
                return {
                    panelRuntimeProfile: this.draft.panelRuntimeProfile,
                    embeddedRuntimeProfile: this.draft.embeddedRuntimeProfile,
                    chatHistoryMode: this.draft.chatHistoryMode,
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
                        <SelectField v-model="draft.embeddedRuntimeProfile" :options="embeddedRuntimeOptions" />
                    </SettingRow>

                    <SettingRow
                        :label="tr('Chat History')"
                        help-topic="chatHistory"
                        :help-title="tr('Learn more')"
                        @help="showHelp"
                    >
                        <SelectField v-model="draft.chatHistoryMode" :options="chatHistoryOptions" />
                    </SettingRow>

                    <SettingRow :label="tr('Rust Regex Backend')">
                        <ToggleSwitch v-model="draft.nativeRegexBackendEnabled" />
                    </SettingRow>

                    <small class="tt-settings-section-note">{{ tr('Requires reload to apply.') }}</small>
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
                            <span>{{ tr('Dynamic Theme') }}</span>
                            <span class="tt-settings-summary-meta">
                                <small>{{ dynamicThemeSummary }}</small>
                                <i class="fa-solid fa-chevron-down" aria-hidden="true"></i>
                            </span>
                        </summary>
                        <div class="tt-settings-disclosure-body">
                            <SettingRow
                                :label="tr('Enable Dynamic Theme')"
                                help-topic="dynamicTheme"
                                :help-title="tr('Learn more')"
                                @help="showHelp"
                            >
                                <ToggleSwitch
                                    :model-value="draft.dynamicTheme.enabled"
                                    @update:model-value="setDynamicThemeEnabled"
                                />
                            </SettingRow>
                            <SettingRow :label="tr('Day Theme')">
                                <SelectField
                                    ref="dynamicThemeDay"
                                    v-model="draft.dynamicTheme.dayTheme"
                                    :options="themeOptionsWithStored(draft.dynamicTheme.dayTheme)"
                                    :disabled="!draft.dynamicTheme.enabled"
                                />
                            </SettingRow>
                            <SettingRow :label="tr('Night Theme')">
                                <SelectField
                                    v-model="draft.dynamicTheme.nightTheme"
                                    :options="themeOptionsWithStored(draft.dynamicTheme.nightTheme)"
                                    :disabled="!draft.dynamicTheme.enabled"
                                />
                            </SettingRow>
                            <small class="tt-settings-section-note">{{ tr('Dynamic Theme hint') }}</small>
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
