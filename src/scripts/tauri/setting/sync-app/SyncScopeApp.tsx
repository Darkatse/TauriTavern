import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import type { SyncDatasetSelection, SyncScopeDatasetCatalog } from './SyncContract';

export type { SyncDatasetSelection, SyncScopeDatasetCatalog };

/**
 * Sync content scope dialog island. `setting-panel/sync-popup.js` owns the
 * confirm popup and persists the result; this root only edits a mount-local
 * dataset selection.
 *
 * The popup removes its dialog before resolving, then Save reads
 * `getSelection()` — so the selection lives in a mount-local variable that
 * every user action updates synchronously, never behind a pending React
 * commit. React only projects that variable into DOM.
 */

type DatasetTone = 'sensitive' | 'large';

type DatasetMeta = {
    label: string;
    tone?: DatasetTone;
};

type DatasetGroup = {
    id: string;
    label: string;
    icon: string;
    datasetIds: string[];
};

type SyncScopePreset = 'default' | 'chat' | 'agent' | 'full';

export type SyncScopeOptions = {
    catalog: SyncScopeDatasetCatalog;
    selection?: SyncDatasetSelection | null | undefined;
    tr: (key: string) => string;
};

export type SyncScopeHandle = {
    getSelection(): SyncDatasetSelection;
    unmount(): void;
};

const DATASET_META: Record<string, DatasetMeta> = {
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

const DATASET_GROUPS: DatasetGroup[] = [
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

const PRESETS: ReadonlyArray<{ name: SyncScopePreset; label: string; icon: string }> = [
    { name: 'default', label: 'Recommended', icon: 'fa-star' },
    { name: 'chat', label: 'Chats', icon: 'fa-comments' },
    { name: 'agent', label: 'Agent', icon: 'fa-brain' },
    { name: 'full', label: 'Full', icon: 'fa-layer-group' },
];

function uniqueSupported(ids: readonly string[] | undefined, supported: ReadonlySet<string>): string[] {
    const result: string[] = [];
    const seen = new Set<string>();
    for (const id of ids ?? []) {
        if (!supported.has(id) || seen.has(id)) {
            continue;
        }
        seen.add(id);
        result.push(id);
    }
    return result;
}

function createGroups(catalog: SyncScopeDatasetCatalog): DatasetGroup[] {
    const supported = new Set(catalog.supportedDatasetIds);
    const used = new Set<string>();
    const groups = DATASET_GROUPS.map((group) => {
        const ids = uniqueSupported(group.datasetIds, supported);
        for (const id of ids) {
            used.add(id);
        }
        return { ...group, datasetIds: ids };
    }).filter(group => group.datasetIds.length > 0);

    const rest = catalog.supportedDatasetIds.filter(id => !used.has(id));
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

function normalizeInitialSelection(
    selection: SyncDatasetSelection | null | undefined,
    catalog: SyncScopeDatasetCatalog,
): string[] {
    const supported = new Set(catalog.supportedDatasetIds);
    const ids = uniqueSupported(selection?.dataset_ids ?? catalog.defaultDatasetIds, supported);
    return ids.length > 0 ? ids : uniqueSupported(catalog.defaultDatasetIds, supported);
}

type SyncScopeViewProps = {
    catalog: SyncScopeDatasetCatalog;
    groups: DatasetGroup[];
    selectedIds: string[];
    tr: (key: string) => string;
    onPreset: (name: SyncScopePreset) => void;
    onToggleDataset: (id: string) => void;
    onToggleGroup: (group: DatasetGroup) => void;
};

function SyncScopeView({
    catalog,
    groups,
    selectedIds,
    tr,
    onPreset,
    onToggleDataset,
    onToggleGroup,
}: SyncScopeViewProps) {
    const selectedSet = new Set(selectedIds);
    const hasSensitiveSelection = selectedIds.some(id => DATASET_META[id]?.tone === 'sensitive');

    return (
        <div className="tt-sync-scope-dialog">
            <div className="tt-sync-scope-presets">
                {PRESETS.map(preset => (
                    <button
                        key={preset.name}
                        type="button"
                        className="menu_button margin0"
                        onClick={() => onPreset(preset.name)}
                    >
                        <i className={`fa-solid ${preset.icon}`} aria-hidden="true"></i>
                        <span>{tr(preset.label)}</span>
                    </button>
                ))}
            </div>

            <div className="tt-sync-scope-summary">
                <b>{selectedIds.length} / {catalog.supportedDatasetIds.length}</b>
                <span>{tr('datasets selected')}</span>
                {hasSensitiveSelection && <code>{tr('Sensitive')}</code>}
            </div>

            <div className="tt-sync-scope-groups">
                {groups.map((group) => {
                    const groupSelectedCount = group.datasetIds.filter(id => selectedSet.has(id)).length;
                    const groupChecked = groupSelectedCount === group.datasetIds.length;
                    return (
                        <section key={group.id} className="tt-sync-scope-group">
                            <button
                                type="button"
                                className="tt-sync-scope-group-header"
                                onClick={() => onToggleGroup(group)}
                            >
                                <i className={`fa-solid ${group.icon}`} aria-hidden="true"></i>
                                <b>{tr(group.label)}</b>
                                <span>{groupSelectedCount} / {group.datasetIds.length}</span>
                                <i
                                    className={`fa-solid ${groupChecked ? 'fa-square-check' : 'fa-square'}`}
                                    aria-hidden="true"
                                >
                                </i>
                            </button>
                            {group.datasetIds.map((id) => {
                                const meta = DATASET_META[id];
                                return (
                                    <label key={id} className="tt-sync-scope-item">
                                        <input
                                            type="checkbox"
                                            checked={selectedSet.has(id)}
                                            onChange={() => onToggleDataset(id)}
                                        />
                                        <span>
                                            <b>{tr(meta?.label ?? id)}</b>
                                            <small>{id}</small>
                                        </span>
                                        {meta?.tone && (
                                            <code>{tr(meta.tone === 'sensitive' ? 'Sensitive' : 'Large')}</code>
                                        )}
                                    </label>
                                );
                            })}
                        </section>
                    );
                })}
            </div>
        </div>
    );
}

export function mountTauriTavernSyncScopeApp(
    mount: unknown,
    options: Partial<SyncScopeOptions> | undefined,
): SyncScopeHandle {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern Sync scope mount element is required');
    }
    if (!options || typeof options.tr !== 'function') {
        throw new Error('TauriTavern Sync translator is required');
    }
    if (!options.catalog) {
        throw new Error('TauriTavern Sync scope catalog is required');
    }
    const { catalog, selection, tr } = options;

    const groups = createGroups(catalog);
    let selectedIds = normalizeInitialSelection(selection, catalog);
    const root = createRoot(mount);

    function commit(next: string[]): void {
        selectedIds = next;
        render();
    }

    function render(): void {
        root.render(
            <StrictMode>
                <SyncScopeView
                    catalog={catalog}
                    groups={groups}
                    selectedIds={selectedIds}
                    tr={tr}
                    onPreset={applyPreset}
                    onToggleDataset={toggleDataset}
                    onToggleGroup={toggleGroup}
                />
            </StrictMode>,
        );
    }

    function applyPreset(name: SyncScopePreset): void {
        const supported = new Set(catalog.supportedDatasetIds);
        const presets: Record<SyncScopePreset, readonly string[]> = {
            default: catalog.defaultDatasetIds,
            chat: CHAT_ONLY_DATASETS,
            agent: AGENT_CORE_DATASETS,
            full: catalog.supportedDatasetIds,
        };
        const next = uniqueSupported(presets[name], supported);
        if (next.length > 0) {
            commit(next);
        }
    }

    function toggleDataset(id: string): void {
        if (selectedIds.includes(id)) {
            // The last selected dataset stays: syncing nothing is never valid.
            if (selectedIds.length === 1) {
                return;
            }
            commit(selectedIds.filter(item => item !== id));
            return;
        }
        commit([...selectedIds, id]);
    }

    function toggleGroup(group: DatasetGroup): void {
        const selectedCount = group.datasetIds.filter(id => selectedIds.includes(id)).length;
        if (selectedCount === group.datasetIds.length) {
            const groupSet = new Set(group.datasetIds);
            const next = selectedIds.filter(id => !groupSet.has(id));
            if (next.length > 0) {
                commit(next);
            }
            return;
        }

        const next = [...selectedIds];
        const seen = new Set(next);
        for (const id of group.datasetIds) {
            if (!seen.has(id)) {
                next.push(id);
            }
        }
        commit(next);
    }

    render();

    return {
        getSelection() {
            return {
                policy_version: catalog.policyVersion,
                dataset_ids: [...selectedIds],
            };
        },
        unmount() {
            root.unmount();
        },
    };
}
