const DATASET_META = {
    'settings.core': { label: 'Core settings' },
    'secrets.api_keys': { label: 'API keys', tone: 'sensitive' },
    'chat.character.history': { label: 'Character chats' },
    'chat.group.metadata': { label: 'Group metadata' },
    'chat.group.history': { label: 'Group chats' },
    'character.cards': { label: 'Character cards' },
    'character.avatars': { label: 'User avatars' },
    'world.info': { label: 'World info' },
    'preset.openai': { label: 'OpenAI presets' },
    'preset.novelai': { label: 'NovelAI presets' },
    'preset.textgen': { label: 'TextGen presets' },
    'preset.kobold': { label: 'KoboldAI presets' },
    'prompt.instruct': { label: 'Instruct prompts' },
    'prompt.context': { label: 'Context prompts' },
    'prompt.sysprompt': { label: 'System prompts' },
    'prompt.reasoning': { label: 'Reasoning prompts' },
    'quick.replies': { label: 'Quick replies' },
    'ui.themes': { label: 'Themes' },
    'ui.moving': { label: 'Moving UI' },
    'media.backgrounds': { label: 'Backgrounds' },
    'media.assets': { label: 'Assets' },
    'media.thumbnails': { label: 'Thumbnails', tone: 'large' },
    'media.user_images': { label: 'User images' },
    'user.files': { label: 'User files' },
    'user.workflows': { label: 'Workflows' },
    'vectors': { label: 'Vectors', tone: 'large' },
    'backups': { label: 'Backups', tone: 'large' },
    'extensions.local': { label: 'Local extensions' },
    'extensions.third_party': { label: 'Third-party extensions' },
    'extensions.sources': { label: 'Extension sources' },
    'extensions.store': { label: 'Extension store' },
    'agent.profiles': { label: 'Agent profiles' },
    'agent.llm_connections': { label: 'Agent LLM connections' },
    'agent.skills': { label: 'Agent skills' },
    'agent.persistent_state': { label: 'Agent persistent state' },
    'agent.run_journal': { label: 'Agent run journal' },
    'agent.run_context': { label: 'Agent run context', tone: 'large' },
    'agent.run_workspace_projection': { label: 'Agent workspace projection', tone: 'large' },
    'agent.run_tool_io': { label: 'Agent tool I/O', tone: 'large' },
    'agent.workspace_outputs': { label: 'Agent outputs', tone: 'large' },
    'agent.workspace_scratch': { label: 'Agent scratch', tone: 'large' },
    'agent.tasks': { label: 'Agent tasks', tone: 'large' },
    'agent.model_responses': { label: 'Agent model responses', tone: 'sensitive' },
    'agent.checkpoints': { label: 'Agent checkpoints', tone: 'large' },
};

const DATASET_GROUPS = [
    {
        id: 'core',
        label: 'Core',
        icon: 'fa-sliders',
        datasetIds: [
            'settings.core',
            'chat.character.history',
            'chat.group.metadata',
            'chat.group.history',
            'character.cards',
            'character.avatars',
            'world.info',
            'quick.replies',
        ],
    },
    {
        id: 'presets',
        label: 'Presets & prompts',
        icon: 'fa-wand-magic-sparkles',
        datasetIds: [
            'preset.openai',
            'preset.novelai',
            'preset.textgen',
            'preset.kobold',
            'prompt.instruct',
            'prompt.context',
            'prompt.sysprompt',
            'prompt.reasoning',
        ],
    },
    {
        id: 'media',
        label: 'Media & files',
        icon: 'fa-folder-open',
        datasetIds: [
            'ui.themes',
            'ui.moving',
            'media.backgrounds',
            'media.assets',
            'media.user_images',
            'user.files',
            'user.workflows',
        ],
    },
    {
        id: 'extensions',
        label: 'Extensions',
        icon: 'fa-puzzle-piece',
        datasetIds: [
            'extensions.local',
            'extensions.third_party',
            'extensions.sources',
            'extensions.store',
        ],
    },
    {
        id: 'agent',
        label: 'Agent continuity',
        icon: 'fa-brain',
        datasetIds: [
            'agent.profiles',
            'agent.llm_connections',
            'agent.skills',
            'agent.persistent_state',
            'agent.run_journal',
        ],
    },
    {
        id: 'heavy',
        label: 'Sensitive & large',
        icon: 'fa-vault',
        datasetIds: [
            'secrets.api_keys',
            'media.thumbnails',
            'vectors',
            'backups',
            'agent.run_context',
            'agent.run_workspace_projection',
            'agent.run_tool_io',
            'agent.workspace_outputs',
            'agent.workspace_scratch',
            'agent.tasks',
            'agent.model_responses',
            'agent.checkpoints',
        ],
    },
];

const CHAT_ONLY_DATASETS = [
    'chat.character.history',
    'chat.group.metadata',
    'chat.group.history',
    'character.cards',
    'character.avatars',
    'world.info',
];

const AGENT_CORE_DATASETS = [
    'agent.profiles',
    'agent.llm_connections',
    'agent.skills',
    'agent.persistent_state',
    'agent.run_journal',
];

function uniqueSupported(ids, supported) {
    const result = [];
    const seen = new Set();
    for (const id of ids || []) {
        if (!supported.has(id) || seen.has(id)) {
            continue;
        }
        seen.add(id);
        result.push(id);
    }
    return result;
}

function createGroups(catalog) {
    const supported = new Set(catalog.supportedDatasetIds);
    const used = new Set();
    const groups = DATASET_GROUPS.map((group) => {
        const ids = uniqueSupported(group.datasetIds, supported);
        ids.forEach((id) => used.add(id));
        return { ...group, datasetIds: ids };
    }).filter((group) => group.datasetIds.length > 0);

    const rest = catalog.supportedDatasetIds.filter((id) => !used.has(id));
    if (rest.length > 0) {
        groups.push({
            id: 'other',
            label: 'Other',
            icon: 'fa-ellipsis',
            datasetIds: rest,
        });
    }

    return groups;
}

function normalizeInitialSelection(selection, catalog) {
    const supported = new Set(catalog.supportedDatasetIds);
    const ids = uniqueSupported(selection?.dataset_ids || catalog.defaultDatasetIds, supported);
    return ids.length > 0 ? ids : uniqueSupported(catalog.defaultDatasetIds, supported);
}

export function createTauriTavernSyncScopeApp(options) {
    const {
        catalog,
        selection,
        tr,
    } = options || {};

    if (typeof tr !== 'function') {
        throw new Error('TauriTavern Sync translator is required');
    }

    return {
        name: 'TauriTavernSyncScopeApp',
        data() {
            return {
                selectedIds: normalizeInitialSelection(selection, catalog),
            };
        },
        computed: {
            groups() {
                return createGroups(catalog);
            },
            selectedSet() {
                return new Set(this.selectedIds);
            },
            selectedCount() {
                return this.selectedIds.length;
            },
            totalCount() {
                return catalog.supportedDatasetIds.length;
            },
            hasSensitiveSelection() {
                return this.selectedIds.some((id) => DATASET_META[id]?.tone === 'sensitive');
            },
        },
        methods: {
            tr(key) {
                return tr(key);
            },
            datasetLabel(id) {
                return DATASET_META[id]?.label || id;
            },
            datasetTone(id) {
                return DATASET_META[id]?.tone || '';
            },
            groupSelectedCount(group) {
                return group.datasetIds.filter((id) => this.selectedSet.has(id)).length;
            },
            groupChecked(group) {
                return this.groupSelectedCount(group) === group.datasetIds.length;
            },
            setPreset(name) {
                const supported = new Set(catalog.supportedDatasetIds);
                const presets = {
                    default: catalog.defaultDatasetIds,
                    chat: CHAT_ONLY_DATASETS,
                    agent: AGENT_CORE_DATASETS,
                    full: catalog.supportedDatasetIds,
                };
                const next = uniqueSupported(presets[name], supported);
                if (next.length > 0) {
                    this.selectedIds = next;
                }
            },
            toggleDataset(id) {
                if (this.selectedSet.has(id)) {
                    if (this.selectedIds.length === 1) {
                        return;
                    }
                    this.selectedIds = this.selectedIds.filter((item) => item !== id);
                    return;
                }

                this.selectedIds = [...this.selectedIds, id];
            },
            toggleGroup(group) {
                if (this.groupChecked(group)) {
                    const groupSet = new Set(group.datasetIds);
                    const next = this.selectedIds.filter((id) => !groupSet.has(id));
                    if (next.length > 0) {
                        this.selectedIds = next;
                    }
                    return;
                }

                const next = [...this.selectedIds];
                const seen = new Set(next);
                for (const id of group.datasetIds) {
                    if (!seen.has(id)) {
                        next.push(id);
                    }
                }
                this.selectedIds = next;
            },
            getSelection() {
                return {
                    policy_version: catalog.policyVersion,
                    dataset_ids: [...this.selectedIds],
                };
            },
        },
        template: `
            <div class="tt-sync-scope-dialog">
                <div class="tt-sync-scope-presets">
                    <button type="button" class="menu_button margin0" @click="setPreset('default')">
                        <i class="fa-solid fa-star" aria-hidden="true"></i>
                        <span>{{ tr('Recommended') }}</span>
                    </button>
                    <button type="button" class="menu_button margin0" @click="setPreset('chat')">
                        <i class="fa-solid fa-comments" aria-hidden="true"></i>
                        <span>{{ tr('Chats') }}</span>
                    </button>
                    <button type="button" class="menu_button margin0" @click="setPreset('agent')">
                        <i class="fa-solid fa-brain" aria-hidden="true"></i>
                        <span>{{ tr('Agent') }}</span>
                    </button>
                    <button type="button" class="menu_button margin0" @click="setPreset('full')">
                        <i class="fa-solid fa-layer-group" aria-hidden="true"></i>
                        <span>{{ tr('Full') }}</span>
                    </button>
                </div>

                <div class="tt-sync-scope-summary">
                    <b>{{ selectedCount }} / {{ totalCount }}</b>
                    <span>{{ tr('datasets selected') }}</span>
                    <code v-if="hasSensitiveSelection">{{ tr('Sensitive') }}</code>
                </div>

                <div class="tt-sync-scope-groups">
                    <section v-for="group in groups" :key="group.id" class="tt-sync-scope-group">
                        <button type="button" class="tt-sync-scope-group-header" @click="toggleGroup(group)">
                            <i class="fa-solid" :class="group.icon" aria-hidden="true"></i>
                            <b>{{ tr(group.label) }}</b>
                            <span>{{ groupSelectedCount(group) }} / {{ group.datasetIds.length }}</span>
                            <i class="fa-solid" :class="groupChecked(group) ? 'fa-square-check' : 'fa-square'" aria-hidden="true"></i>
                        </button>
                        <label v-for="id in group.datasetIds" :key="id" class="tt-sync-scope-item">
                            <input
                                type="checkbox"
                                :checked="selectedSet.has(id)"
                                @change="toggleDataset(id)"
                            />
                            <span>
                                <b>{{ tr(datasetLabel(id)) }}</b>
                                <small>{{ id }}</small>
                            </span>
                            <code v-if="datasetTone(id)">{{ tr(datasetTone(id) === 'sensitive' ? 'Sensitive' : 'Large') }}</code>
                        </label>
                    </section>
                </div>
            </div>
        `,
    };
}
