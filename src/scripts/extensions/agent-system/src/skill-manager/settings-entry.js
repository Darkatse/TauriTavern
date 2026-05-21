import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { translateAgentSystem as tr } from '../i18n.js';
import { createSkillManagerPanelRoot } from './panel-app.js';

const EXTENSIONS_BLOCK_ID = 'rm_extensions_block';
const AGENT_SYSTEM_CONTAINER_ID = 'agent_system_container';
const SKILL_MANAGER_CONTAINER_ID = 'skill_manager_container';
const SKILL_MANAGER_MOUNT_ID = 'skill_manager_settings_mount';

function requireExtensionsBlock() {
    const block = document.getElementById(EXTENSIONS_BLOCK_ID);
    if (!(block instanceof HTMLElement)) {
        throw new Error(tr('extensionsBlockNotFound'));
    }
    return block;
}

function ensureSkillManagerContainer() {
    const existing = document.getElementById(SKILL_MANAGER_CONTAINER_ID);
    if (existing instanceof HTMLElement) {
        return existing;
    }

    const block = requireExtensionsBlock();
    const agentSystemContainer = block.querySelector(`#${AGENT_SYSTEM_CONTAINER_ID}`);
    if (!(agentSystemContainer instanceof HTMLElement)) {
        throw new Error(tr('mountContainerNotFound'));
    }

    const container = document.createElement('div');
    container.id = SKILL_MANAGER_CONTAINER_ID;
    container.className = 'extension_container';
    agentSystemContainer.insertAdjacentElement('afterend', container);
    return container;
}

function createSkillManagerSettingsApp() {
    const SkillManagerPanel = createSkillManagerPanelRoot();

    return createApp({
        components: {
            SkillManagerPanel,
        },
        methods: {
            tr(key, params) {
                return tr(key, params);
            },
        },
        template: `
            <div id="skill_manager_settings" class="ttas-root ttas-skill-manager-settings">
                <div class="inline-drawer">
                    <div class="inline-drawer-toggle inline-drawer-header">
                        <b>{{ tr('skillManager') }}</b>
                        <div class="inline-drawer-icon fa-solid fa-circle-chevron-down down"></div>
                    </div>
                    <div class="inline-drawer-content">
                        <SkillManagerPanel />
                    </div>
                </div>
            </div>
        `,
    });
}

export function mountSkillManagerSettingsPanel() {
    if (document.getElementById(SKILL_MANAGER_MOUNT_ID)) {
        return;
    }

    const container = ensureSkillManagerContainer();
    const mount = document.createElement('div');
    mount.id = SKILL_MANAGER_MOUNT_ID;
    container.appendChild(mount);
    createSkillManagerSettingsApp().mount(mount);
}
