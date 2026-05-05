import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { errorText, waitForHostReady } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { mountChatInputAgentToggle } from './chat-input-toggle.js';
import { mountAgentRunTimelinePanel } from './run-timeline-panel.js';
import { openAgentSystemPanel } from './panel-popup.js';
import { loadSettings, patchSettings, subscribeSettings } from './settings-store.js';

function createAgentSystemEntryApp() {
    return createApp({
        data() {
            return {
                loading: false,
                unsubscribeSettings: null,
                settings: {
                    agentModeEnabled: false,
                },
            };
        },
        async mounted() {
            this.loading = true;
            try {
                this.settings = await loadSettings();
                this.unsubscribeSettings = subscribeSettings((settings) => {
                    this.settings = settings;
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
        },
        methods: {
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
    const container = document.getElementById('agent_system_container');
    if (!(container instanceof HTMLElement)) {
        throw new Error(tr('mountContainerNotFound'));
    }

    const mount = document.createElement('div');
    mount.id = 'agent_system_mount';
    container.appendChild(mount);
    createAgentSystemEntryApp().mount(mount);
    await mountChatInputAgentToggle();
    await mountAgentRunTimelinePanel();
}

void mountAgentSystem();
