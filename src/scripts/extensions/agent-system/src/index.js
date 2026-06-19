import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { errorText, requireAgentApi, waitForHostReady } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { mountChatInputAgentToggle } from './chat-input-toggle.js';
import { mountEmbeddedAssetButtons } from './embedded-assets-buttons.js';
import { mountAgentRunTimelinePanel } from './run-timeline-panel.js';
import { mountSkillManagerSettingsPanel } from './skill-manager/settings-entry.js';
import { openAgentSystemPanel } from './panel-popup.js';
import { loadSettings, patchSettings, subscribeSettings } from './settings-store.js';
import { startModelTargetLlmConnectionSync, syncSavedModelTargetLlmConnections } from './model-target-connection.js';
import { subscribeAgentProfilesChanged } from '../../../tauritavern/agent/agent-profile-events.js';
import { DEFAULT_AGENT_PROFILE_ID } from '../../../tauritavern/agent/agent-system-settings.js';

function createAgentSystemEntryApp() {
    return createApp({
        data() {
            return {
                loading: false,
                unsubscribeSettings: null,
                unsubscribeProfiles: null,
                profiles: [],
                settings: {
                    agentModeEnabled: false,
                    chatInputToggleHidden: false,
                    activeProfileId: DEFAULT_AGENT_PROFILE_ID,
                },
            };
        },
        computed: {
            activeProfileOptions() {
                return this.profiles.filter((profile) => profile.directRunnable !== false);
            },
            activeProfileId() {
                return this.settings.activeProfileId || DEFAULT_AGENT_PROFILE_ID;
            },
        },
        async mounted() {
            this.loading = true;
            try {
                this.settings = await loadSettings();
                await this.refreshProfiles();
                await this.ensureActiveProfileSelectable();
                this.unsubscribeSettings = subscribeSettings((settings) => {
                    this.settings = settings;
                });
                this.unsubscribeProfiles = subscribeAgentProfilesChanged(() => {
                    this.handleAsyncEvent(async () => {
                        await this.refreshProfiles();
                        await this.ensureActiveProfileSelectable();
                    });
                });
            } catch (error) {
                this.reportError(error);
                throw error;
            } finally {
                this.loading = false;
            }
        },
        unmounted() {
            this.unsubscribeSettings?.();
            this.unsubscribeProfiles?.();
        },
        methods: {
            handleAsyncEvent(operation) {
                void (async () => {
                    try {
                        await operation();
                    } catch (error) {
                        this.reportError(error);
                        queueMicrotask(() => {
                            throw error;
                        });
                    }
                })();
            },
            async refreshProfiles() {
                const result = await requireAgentApi().profiles.list();
                this.profiles = Array.isArray(result?.profiles) ? result.profiles : [];
            },
            async ensureActiveProfileSelectable() {
                if (this.activeProfileOptions.some((profile) => profile.id === this.activeProfileId)) {
                    return;
                }
                const previousProfileId = this.activeProfileId;
                await this.setActiveProfile(DEFAULT_AGENT_PROFILE_ID);
                if (previousProfileId !== DEFAULT_AGENT_PROFILE_ID) {
                    this.warn(tr('activeProfileResetToDefault'));
                }
            },
            async toggleAgentMode() {
                try {
                    this.settings = await patchSettings(this.settings, {
                        agentModeEnabled: !this.settings.agentModeEnabled,
                    });
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            async toggleChatInputToggleVisibility() {
                try {
                    this.settings = await patchSettings(this.settings, {
                        chatInputToggleHidden: !this.settings.chatInputToggleHidden,
                    });
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            async setActiveProfile(profileId) {
                const id = String(profileId || '').trim();
                const profile = this.profiles.find((item) => item.id === id);
                if (!profile) {
                    throw new Error(tr('agentProfileNotFound', { id }));
                }
                if (profile.directRunnable === false) {
                    throw new Error(tr('agentProfileNotDirectRunnable', { id }));
                }
                this.settings = await patchSettings(this.settings, {
                    activeProfileId: id,
                });
            },
            openPanel() {
                openAgentSystemPanel().catch((error) => {
                    this.reportError(error);
                    throw error;
                });
            },
            tr(key, params) {
                return tr(key, params);
            },
            reportError(error) {
                const message = errorText(error);
                console.error('[AgentSystem]', error);
                window.toastr?.error?.(message);
            },
            warn(message) {
                window.toastr?.warning?.(message);
            },
        },
        template: `
            <div id="agent_system_settings" class="ttas-root">
                <div class="inline-drawer">
                    <div class="inline-drawer-toggle inline-drawer-header">
                        <b>{{ tr('agentSystem') }}</b>
                        <div class="inline-drawer-icon fa-solid fa-circle-chevron-down down"></div>
                    </div>
                    <div class="inline-drawer-content">
                        <div class="ttas-entry">
                            <button type="button" class="menu_button menu_button_icon" :class="{ active: settings.agentModeEnabled }" :disabled="loading" @click="toggleAgentMode">
                                <i class="fa-solid" :class="settings.agentModeEnabled ? 'fa-toggle-on' : 'fa-toggle-off'"></i>
                                <span>{{ settings.agentModeEnabled ? tr('agentModeOn') : tr('agentModeOff') }}</span>
                            </button>
                            <button type="button" class="menu_button menu_button_icon" :class="{ active: settings.chatInputToggleHidden }" :aria-pressed="String(settings.chatInputToggleHidden)" :disabled="loading" @click="toggleChatInputToggleVisibility">
                                <i class="fa-solid" :class="settings.chatInputToggleHidden ? 'fa-eye' : 'fa-eye-slash'"></i>
                                <span>{{ settings.chatInputToggleHidden ? tr('showChatInputToggle') : tr('hideChatInputToggle') }}</span>
                            </button>
                            <label class="ttas-field ttas-entry-active-profile">
                                <span>{{ tr('activeProfile') }}</span>
                                <select :value="activeProfileId" :disabled="loading || activeProfileOptions.length === 0" @change="setActiveProfile($event.target.value)">
                                    <option v-for="profile in activeProfileOptions" :key="profile.id" :value="profile.id">{{ profile.displayName || profile.id }}</option>
                                </select>
                            </label>
                            <button type="button" class="menu_button menu_button_icon" @click="openPanel">
                                <i class="fa-solid fa-up-right-from-square"></i>
                                <span>{{ tr('openAgentSystem') }}</span>
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        `,
    });
}

async function mountAgentSystem() {
    await waitForHostReady();
    startModelTargetLlmConnectionSync();
    await syncSavedModelTargetLlmConnections();

    const container = document.getElementById('agent_system_container');
    if (!(container instanceof HTMLElement)) {
        throw new Error(tr('mountContainerNotFound'));
    }

    const mount = document.createElement('div');
    mount.id = 'agent_system_mount';
    container.appendChild(mount);
    createAgentSystemEntryApp().mount(mount);
    mountSkillManagerSettingsPanel();
    await mountChatInputAgentToggle();
    await mountEmbeddedAssetButtons();
    await mountAgentRunTimelinePanel();
}

void mountAgentSystem();
