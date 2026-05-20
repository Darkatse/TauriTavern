import { DEFAULT_PROFILE_ID, KNOWN_TOOLS, WORKSPACE_ROOTS } from './constants.js';
import { confirmAction, errorText, prettyJson, requireAgentApi, requireSkillApi } from './host-api.js';
import { translateAgentSystem as tr, translateSkillInstallAction } from './i18n.js';
import { defaultProfile, normalizeProfileForSave, normalizeProfileId, profileForEdit } from './profile-model.js';
import { loadSettings, patchSettings } from './settings-store.js';
import { buildSkillFileTree } from './skill-file-tree.js';
import { openSkillFileViewer } from './skill-file-viewer.js';
import { downloadBlobWithRuntime } from '../../../file-export.js';
import { normalizeAgentSystemPrompt } from '../../../tauritavern/agent/agent-system-prompt.js';

const SKILL_FILE_VIEW_MAX_CHARS = 80000;
const PROFILE_EXPORT_CONTENT_TYPE = 'application/json';

const TOOL_GROUPS = Object.freeze([
    {
        id: 'context',
        labelKey: 'contextTools',
        icon: 'fa-comments',
        tools: ['chat.search', 'chat.read_messages', 'worldinfo.read_activated'],
    },
    {
        id: 'skills',
        labelKey: 'skillTools',
        icon: 'fa-book-open',
        tools: ['skill.list', 'skill.search', 'skill.read'],
    },
    {
        id: 'workspace-read',
        labelKey: 'workspaceReadTools',
        icon: 'fa-folder-tree',
        tools: ['workspace.list_files', 'workspace.search_files', 'workspace.read_file'],
    },
    {
        id: 'workspace-write',
        labelKey: 'workspaceWriteTools',
        icon: 'fa-pen-to-square',
        tools: ['workspace.write_file', 'workspace.apply_patch'],
    },
    {
        id: 'control',
        labelKey: 'controlTools',
        icon: 'fa-flag-checkered',
        tools: ['workspace.commit', 'workspace.finish'],
    },
]);

const WORKSPACE_ROOT_ICONS = Object.freeze({
    output: 'fa-message',
    scratch: 'fa-note-sticky',
    plan: 'fa-list-check',
    summaries: 'fa-layer-group',
    persist: 'fa-database',
});

function normalizeResolvedAgentSystemPrompt(result) {
    return normalizeAgentSystemPrompt(result?.agentSystemPrompt);
}

const SkillFileTreeNode = {
    name: 'SkillFileTreeNode',
    props: {
        depth: {
            type: Number,
            required: true,
        },
        isFolderOpen: {
            type: Function,
            required: true,
        },
        node: {
            type: Object,
            required: true,
        },
    },
    emits: ['toggle-folder', 'open-file'],
    methods: {
        rowPadding(depth) {
            return `${8 + depth * 16}px`;
        },
        tr(key, params) {
            return tr(key, params);
        },
    },
    template: `
        <li class="ttas-file-tree-item" :class="'ttas-file-tree-' + node.type">
            <button
                v-if="node.type === 'folder'"
                type="button"
                class="ttas-file-row"
                :style="{ paddingLeft: rowPadding(depth) }"
                :aria-expanded="isFolderOpen(node)"
                @click="$emit('toggle-folder', node)"
            >
                <i class="fa-solid" :class="isFolderOpen(node) ? 'fa-folder-open' : 'fa-folder'"></i>
                <span>{{ node.name }}</span>
                <small>{{ node.children.length }}</small>
            </button>
            <button
                v-else
                type="button"
                class="ttas-file-row"
                :style="{ paddingLeft: rowPadding(depth) }"
                @click="$emit('open-file', node)"
            >
                <i class="fa-solid" :class="node.file.kind === 'binary' ? 'fa-file' : 'fa-file-lines'"></i>
                <span>{{ node.name }}</span>
                <small>{{ tr('byteCount', { count: node.file.sizeBytes }) }}</small>
            </button>
            <ul v-if="node.type === 'folder' && isFolderOpen(node)" class="ttas-file-tree">
                <SkillFileTreeNode
                    v-for="child in node.children"
                    :key="child.path"
                    :node="child"
                    :depth="depth + 1"
                    :is-folder-open="isFolderOpen"
                    @toggle-folder="$emit('toggle-folder', $event)"
                    @open-file="$emit('open-file', $event)"
                />
            </ul>
        </li>
    `,
};

export function createAgentSystemPanelRoot({ requestClose }) {
    return {
        components: {
            SkillFileTreeNode,
        },
        data() {
            return {
                initialized: false,
                loading: false,
                saving: false,
                error: '',
                settings: {},
                profiles: [],
                selectedProfileId: DEFAULT_PROFILE_ID,
                draft: profileForEdit(defaultProfile()),
                draftJson: prettyJson(defaultProfile()),
                resolvedAgentSystemPrompt: '',
                skills: [],
                selectedSkillName: '',
                skillFiles: [],
                skillFilesRequestId: 0,
                loadingSkillFiles: false,
                expandedSkillFolders: {},
                skillImportInput: null,
                skillImportConflictStrategy: 'skip',
                skillImportPreview: null,
                tabs: [
                    { id: 'profiles', labelKey: 'profiles', icon: 'fa-id-card-clip' },
                    { id: 'skills', labelKey: 'skills', icon: 'fa-book-bookmark' },
                ],
                toolSpecs: [],
                toolNames: [...KNOWN_TOOLS],
                selectedToolName: KNOWN_TOOLS[0],
                workspaceRoots: WORKSPACE_ROOTS,
            };
        },
        computed: {
            activeTab() {
                return this.settings.activeTab;
            },
            isBuiltinProfile() {
                return this.draft.id === DEFAULT_PROFILE_ID;
            },
            agentSystemPromptEditorValue() {
                if (this.isBuiltinProfile) {
                    return this.resolvedAgentSystemPrompt;
                }
                return this.draft.instructions.agentSystemPrompt ?? '';
            },
            agentSystemPromptPlaceholder() {
                if (this.isBuiltinProfile || String(this.draft.instructions.agentSystemPrompt || '').trim()) {
                    return '';
                }
                return this.resolvedAgentSystemPrompt;
            },
            profileStats() {
                const allowedTools = new Set(Array.isArray(this.draft?.tools?.allow) ? this.draft.tools.allow : []);
                const enabledToolCount = this.toolNames.filter((tool) => allowedTools.has(tool)).length;
                const visibleRootCount = Array.isArray(this.draft?.workspace?.visibleRoots)
                    ? this.draft.workspace.visibleRoots.length
                    : 0;
                const writableRootCount = Array.isArray(this.draft?.workspace?.writableRoots)
                    ? this.draft.workspace.writableRoots.length
                    : 0;
                return [
                    {
                        icon: 'fa-layer-group',
                        label: tr('presentation'),
                        value: tr(this.draft.run.presentation || 'foreground'),
                    },
                    {
                        icon: 'fa-repeat',
                        label: tr('maxRounds'),
                        value: this.draft.tools.maxRounds,
                    },
                    {
                        icon: 'fa-screwdriver-wrench',
                        label: tr('tools'),
                        value: `${enabledToolCount}/${this.toolNames.length}`,
                    },
                    {
                        icon: 'fa-folder-tree',
                        label: tr('workspaceRoots'),
                        value: `${writableRootCount}/${visibleRootCount}`,
                    },
                ];
            },
            toolGroupsWithTools() {
                const groupedTools = new Set();
                const groups = TOOL_GROUPS
                    .map((group) => {
                        const tools = group.tools.filter((tool) => this.toolNames.includes(tool));
                        tools.forEach((tool) => groupedTools.add(tool));
                        return { ...group, tools };
                    })
                    .filter((group) => group.tools.length > 0);
                const extraTools = this.toolNames.filter((tool) => !groupedTools.has(tool));
                if (extraTools.length > 0) {
                    groups.push({
                        id: 'other',
                        labelKey: 'otherTools',
                        icon: 'fa-ellipsis',
                        tools: extraTools,
                    });
                }
                return groups;
            },
            toolSpecsByName() {
                return Object.fromEntries(this.toolSpecs.map((spec) => [spec.name, spec]));
            },
            selectedToolSpec() {
                return this.toolSpecsByName[this.selectedToolName] || null;
            },
            selectedToolEnabled() {
                return Array.isArray(this.draft?.tools?.allow) && this.draft.tools.allow.includes(this.selectedToolName);
            },
            selectedToolProperties() {
                const properties = this.selectedToolSpec?.inputSchema?.properties || {};
                const required = new Set(Array.isArray(this.selectedToolSpec?.inputSchema?.required)
                    ? this.selectedToolSpec.inputSchema.required
                    : []);
                return Object.entries(properties).map(([name, schema]) => ({
                    name,
                    schema,
                    required: required.has(name),
                    type: this.schemaType(schema),
                    description: String(schema?.description || ''),
                }));
            },
            selectedSkill() {
                return this.skills.find((skill) => skill.name === this.selectedSkillName) || null;
            },
            selectedSkillTags() {
                if (!this.selectedSkill) {
                    return [];
                }
                const tags = [
                    tr('fileCount', { count: this.selectedSkill.fileCount }),
                    tr('byteCount', { count: this.selectedSkill.totalBytes }),
                ];
                if (this.selectedSkill.version) {
                    tags.push(this.selectedSkill.version);
                }
                if (this.selectedSkill.sourceKind) {
                    tags.push(this.selectedSkill.sourceKind);
                }
                if (this.selectedSkill.hasScripts) {
                    tags.push(tr('scriptsIncluded'));
                }
                if (this.selectedSkill.hasBinary) {
                    tags.push(tr('binaryFiles'));
                }
                return tags;
            },
            selectedSkillFileTree() {
                if (!this.selectedSkill) {
                    return [];
                }

                return buildSkillFileTree(this.skillFiles);
            },
            skillImportPreviewSkill() {
                return this.skillImportPreview?.skill || null;
            },
            skillImportPath() {
                return this.skillImportInput?.path || '';
            },
            skillImportHasConflict() {
                return this.skillImportPreview?.conflict?.kind === 'different';
            },
            skillImportConflictText() {
                const kind = this.skillImportPreview?.conflict?.kind;
                if (kind === 'new') {
                    return tr('conflictNew');
                }
                if (kind === 'same') {
                    return tr('conflictSame');
                }
                if (kind === 'different') {
                    return tr('conflictDifferent');
                }
                return '';
            },
            skillImportWarnings() {
                return Array.isArray(this.skillImportPreview?.warnings) ? this.skillImportPreview.warnings : [];
            },
        },
        async mounted() {
            await this.initialize();
        },
        methods: {
            async initialize() {
                this.loading = true;
                try {
                    this.settings = await loadSettings();
                    if (!this.tabs.some((tab) => tab.id === this.settings.activeTab)) {
                        this.settings = await patchSettings(this.settings, { activeTab: 'profiles' });
                    }
                    await Promise.all([this.refreshToolSpecs(), this.refreshProfiles(), this.refreshSkills()]);
                    this.selectedProfileId = this.settings.selectedProfileId || DEFAULT_PROFILE_ID;
                    await this.selectProfile(this.selectedProfileId);
                    this.initialized = true;
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    this.loading = false;
                }
            },
            async closePanel() {
                try {
                    await this.clearSkillImportDraft();
                    requestClose();
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            tr(key, params) {
                return tr(key, params);
            },
            async saveSettingsPatch(patch) {
                this.settings = await patchSettings(this.settings, patch);
            },
            async setTab(tab) {
                await this.saveSettingsPatch({ activeTab: tab });
            },
            async setDefaultProfile(profileId) {
                this.selectedProfileId = profileId;
                await this.saveSettingsPatch({ selectedProfileId: profileId });
                await this.selectProfile(profileId);
            },
            async refreshProfiles() {
                const result = await requireAgentApi().profiles.list();
                this.profiles = Array.isArray(result?.profiles) ? result.profiles : [];
                if (!this.profiles.some((profile) => profile.id === this.settings.selectedProfileId)) {
                    this.settings.selectedProfileId = DEFAULT_PROFILE_ID;
                }
            },
            async refreshToolSpecs() {
                const api = requireAgentApi().tools;
                if (!api?.list) {
                    throw new Error(tr('hostAgentToolApiUnavailable'));
                }
                const result = await api.list();
                this.toolSpecs = result.tools;
                this.toolNames = this.toolSpecs.map((tool) => tool.name);
                if (!this.toolNames.includes(this.selectedToolName)) {
                    this.selectedToolName = this.toolNames[0];
                }
            },
            async selectProfile(profileId) {
                const id = profileId || DEFAULT_PROFILE_ID;
                const profilesApi = requireAgentApi().profiles;
                const [result, promptResult] = await Promise.all([
                    profilesApi.load({ profileId: id }),
                    profilesApi.resolveSystemPrompt({ profileId: id }),
                ]);
                if (!result?.profile) {
                    throw new Error(tr('agentProfileNotFound', { id }));
                }
                this.selectedProfileId = id;
                this.draft = profileForEdit(result.profile);
                this.resolvedAgentSystemPrompt = normalizeResolvedAgentSystemPrompt(promptResult);
                this.refreshDraftJson();
            },
            refreshDraftJson() {
                this.draftJson = prettyJson(normalizeProfileForSave(this.draft));
            },
            applyDraftJson() {
                const parsed = JSON.parse(this.draftJson);
                this.draft = profileForEdit(parsed);
                this.selectedProfileId = parsed.id;
                this.resolvedAgentSystemPrompt = '';
            },
            newProfile() {
                const id = this.nextProfileId('agent-profile');
                this.selectedProfileId = id;
                this.draft = profileForEdit(defaultProfile(id));
                this.resolvedAgentSystemPrompt = '';
                this.refreshDraftJson();
            },
            copyProfile() {
                const id = this.nextProfileId(`${this.draft.id}-copy`);
                const copy = normalizeProfileForSave(this.draft);
                copy.id = id;
                copy.displayName = tr('copyDisplayName', { name: copy.displayName });
                this.selectedProfileId = id;
                this.draft = profileForEdit(copy);
                this.resolvedAgentSystemPrompt = '';
                this.refreshDraftJson();
            },
            setAgentSystemPromptDraft(event) {
                if (this.isBuiltinProfile) {
                    return;
                }
                this.draft.instructions.agentSystemPrompt = event.target.value;
            },
            nextProfileId(base) {
                const normalized = normalizeProfileId(base) || 'agent-profile';
                const ids = new Set(this.profiles.map((profile) => profile.id));
                if (!ids.has(normalized)) {
                    return normalized;
                }
                for (let index = 2; index < 1000; index += 1) {
                    const candidate = `${normalized}-${index}`;
                    if (!ids.has(candidate)) {
                        return candidate;
                    }
                }
                throw new Error(tr('unableToAllocateProfileId'));
            },
            syncWritableRoots() {
                const visible = new Set(this.draft.workspace.visibleRoots);
                this.draft.workspace.writableRoots = this.draft.workspace.writableRoots.filter((root) => visible.has(root));
            },
            enabledToolCount(tools) {
                const allow = new Set(Array.isArray(this.draft.tools.allow) ? this.draft.tools.allow : []);
                return tools.filter((tool) => allow.has(tool)).length;
            },
            selectTool(toolName) {
                this.selectedToolName = toolName;
            },
            toolSpec(toolName) {
                return this.toolSpecsByName[toolName];
            },
            toolTitle(toolName) {
                return this.toolSpec(toolName)?.title || toolName;
            },
            toolModelName(toolName) {
                return this.toolSpec(toolName)?.modelName || toolName.replace(/\./g, '_');
            },
            toolSource(toolName) {
                return this.toolSpec(toolName)?.source || '';
            },
            schemaType(schema) {
                const type = schema?.type;
                if (Array.isArray(type)) {
                    return type.join(' | ');
                }
                return String(type || tr('value'));
            },
            toolBadges(toolName) {
                const spec = this.toolSpec(toolName);
                const annotations = spec?.annotations || {};
                const badges = [];
                if (annotations.readOnly) {
                    badges.push({ key: 'read', label: tr('readOnlyTool') });
                }
                if (annotations.mutating) {
                    badges.push({ key: 'write', label: tr('mutatingTool') });
                }
                if (annotations.control) {
                    badges.push({ key: 'control', label: tr('controlTool') });
                }
                if (this.toolHasDescriptionOverride(toolName)) {
                    badges.push({ key: 'custom', label: tr('customizedTool') });
                }
                return badges;
            },
            toolHasDescriptionOverride(toolName) {
                const override = this.draft?.tools?.toolDescriptions?.[toolName];
                return Boolean(override?.description || Object.keys(override?.properties || {}).length > 0);
            },
            getToolDescriptionOverride(toolName) {
                return this.draft.tools.toolDescriptions?.[toolName]?.description || '';
            },
            getToolPropertyDescriptionOverride(toolName, property) {
                return this.draft.tools.toolDescriptions?.[toolName]?.properties?.[property] || '';
            },
            setToolDescriptionOverride(toolName, value) {
                this.updateToolDescriptionOverride(toolName, (override) => {
                    const description = String(value || '');
                    if (description.trim()) {
                        override.description = description;
                    } else {
                        delete override.description;
                    }
                });
            },
            setToolPropertyDescriptionOverride(toolName, property, value) {
                this.updateToolDescriptionOverride(toolName, (override) => {
                    const description = String(value || '');
                    const properties = { ...(override.properties || {}) };
                    if (description.trim()) {
                        properties[property] = description;
                    } else {
                        delete properties[property];
                    }
                    if (Object.keys(properties).length > 0) {
                        override.properties = properties;
                    } else {
                        delete override.properties;
                    }
                });
            },
            updateToolDescriptionOverride(toolName, mutate) {
                const toolDescriptions = { ...(this.draft.tools.toolDescriptions || {}) };
                const override = { ...(toolDescriptions[toolName] || {}) };
                mutate(override);
                if (!override.description && !override.properties) {
                    delete toolDescriptions[toolName];
                } else {
                    toolDescriptions[toolName] = override;
                }
                this.draft.tools.toolDescriptions = toolDescriptions;
            },
            resetToolDescriptionOverride(toolName) {
                const toolDescriptions = { ...(this.draft.tools.toolDescriptions || {}) };
                delete toolDescriptions[toolName];
                this.draft.tools.toolDescriptions = toolDescriptions;
            },
            resetToolPropertyDescriptionOverride(toolName, property) {
                this.updateToolDescriptionOverride(toolName, (override) => {
                    const properties = { ...(override.properties || {}) };
                    delete properties[property];
                    if (Object.keys(properties).length > 0) {
                        override.properties = properties;
                    } else {
                        delete override.properties;
                    }
                });
            },
            async toggleToolAllowed(toolName, event) {
                const enabled = event.target.checked;
                const allow = new Set(this.draft.tools.allow);
                if (enabled) {
                    allow.add(toolName);
                } else {
                    if (this.toolHasDescriptionOverride(toolName)) {
                        if (!await confirmAction(tr('removeToolDescriptionOnDisableConfirm', { tool: toolName }))) {
                            event.target.checked = true;
                            return;
                        }
                        this.resetToolDescriptionOverride(toolName);
                    }
                    allow.delete(toolName);
                }
                this.draft.tools.allow = this.toolNames.filter((tool) => allow.has(tool));
            },
            workspaceRootIcon(root) {
                return WORKSPACE_ROOT_ICONS[root] || 'fa-folder';
            },
            async saveProfile() {
                if (this.isBuiltinProfile) {
                    throw new Error(tr('agentProfileBuiltInEdit'));
                }
                this.saving = true;
                try {
                    const profile = normalizeProfileForSave(this.draft);
                    await requireAgentApi().profiles.save({ profile });
                    await this.refreshProfiles();
                    await this.setDefaultProfile(profile.id);
                    this.draft = profileForEdit(profile);
                    this.refreshDraftJson();
                    this.toast(tr('agentProfileSaved'));
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    this.saving = false;
                }
            },
            profileDraftHasUnsavedChanges(savedProfile) {
                return prettyJson(normalizeProfileForSave(this.draft)) !== prettyJson(savedProfile);
            },
            async exportSelectedProfile() {
                const profileId = this.selectedProfileId || DEFAULT_PROFILE_ID;
                const result = await requireAgentApi().profiles.load({ profileId });
                const profile = result?.profile;
                if (!profile) {
                    throw new Error(tr('agentProfileNotFound', { id: profileId }));
                }
                if (profileId !== DEFAULT_PROFILE_ID && this.profileDraftHasUnsavedChanges(profile)) {
                    throw new Error(tr('agentProfileExportSaveFirst'));
                }

                const blob = new Blob([`${prettyJson(profile)}\n`], { type: PROFILE_EXPORT_CONTENT_TYPE });
                const downloadResult = await downloadBlobWithRuntime(blob, `${profile.id}.agent-profile.json`, {
                    fallbackName: 'agent-profile.json',
                });
                if (downloadResult?.mode !== 'ios-native-share' || downloadResult.completed === true) {
                    this.toast(tr('exportedProfile', { id: profile.id }));
                }
            },
            async deleteProfile() {
                if (this.isBuiltinProfile) {
                    throw new Error(tr('agentProfileBuiltInDelete'));
                }
                const id = this.draft.id;
                if (!await confirmAction(tr('deleteAgentProfileConfirm', { id }))) {
                    return;
                }
                await requireAgentApi().profiles.delete({ profileId: id });
                await this.refreshProfiles();
                await this.setDefaultProfile(DEFAULT_PROFILE_ID);
                this.toast(tr('deletedProfile', { id }));
            },
            async refreshSkills() {
                this.skills = await requireSkillApi().list();
                if (this.skills.length === 0) {
                    this.selectedSkillName = '';
                    this.skillFiles = [];
                    this.skillFilesRequestId += 1;
                    this.loadingSkillFiles = false;
                    this.expandedSkillFolders = {};
                    return;
                }
                if (!this.skills.some((skill) => skill.name === this.selectedSkillName)) {
                    this.selectedSkillName = this.skills[0].name;
                    this.expandedSkillFolders = {};
                }
                await this.loadSelectedSkillFiles();
            },
            async selectSkill(skillName) {
                if (!skillName || skillName === this.selectedSkillName) {
                    return;
                }
                this.selectedSkillName = skillName;
                this.expandedSkillFolders = {};
                await this.loadSelectedSkillFiles();
            },
            async loadSelectedSkillFiles() {
                const skillName = this.selectedSkillName;
                const requestId = ++this.skillFilesRequestId;
                if (!skillName) {
                    this.skillFiles = [];
                    this.loadingSkillFiles = false;
                    return;
                }
                this.skillFiles = [];
                this.loadingSkillFiles = true;
                try {
                    const files = await requireSkillApi().listFiles({ name: skillName });
                    if (requestId !== this.skillFilesRequestId || skillName !== this.selectedSkillName) {
                        return;
                    }
                    this.skillFiles = files;
                } catch (error) {
                    if (requestId !== this.skillFilesRequestId || skillName !== this.selectedSkillName) {
                        return;
                    }
                    this.reportError(error);
                    throw error;
                } finally {
                    if (requestId === this.skillFilesRequestId && skillName === this.selectedSkillName) {
                        this.loadingSkillFiles = false;
                    }
                }
            },
            folderKey(node) {
                return `${this.selectedSkillName}:${node.path}`;
            },
            isFolderOpen(node) {
                return this.expandedSkillFolders[this.folderKey(node)] === true;
            },
            toggleSkillFolder(node) {
                const key = this.folderKey(node);
                this.expandedSkillFolders = {
                    ...this.expandedSkillFolders,
                    [key]: !this.expandedSkillFolders[key],
                };
            },
            async openSkillFile(node) {
                if (!this.selectedSkillName) {
                    throw new Error(tr('selectSkillFirst'));
                }
                if (node.file.kind === 'binary') {
                    const error = new Error(tr('cannotDisplayBinarySkillFile', { path: node.path }));
                    this.reportError(error);
                    throw error;
                }

                try {
                    const result = await requireSkillApi().readFile({
                        name: this.selectedSkillName,
                        path: node.path,
                        maxChars: SKILL_FILE_VIEW_MAX_CHARS,
                    });
                    openSkillFileViewer(result);
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            async clearSkillImportDraft() {
                await requireSkillApi().discardPickedImport?.(this.skillImportInput);
                this.skillImportInput = null;
                this.skillImportPreview = null;
            },
            async pickAndPreviewSkillImport() {
                try {
                    await this.clearSkillImportDraft();
                    const input = await requireSkillApi().pickImportArchive();
                    if (!input) {
                        return;
                    }

                    this.skillImportInput = input;
                    this.skillImportConflictStrategy = 'skip';
                    this.skillImportPreview = await requireSkillApi().previewImport(input);
                } catch (error) {
                    this.skillImportInput = null;
                    this.skillImportPreview = null;
                    this.reportError(error);
                    throw error;
                }
            },
            async installSkillImport() {
                if (!this.skillImportInput || !this.skillImportPreview) {
                    throw new Error(tr('previewSkillImportFirst'));
                }

                try {
                    const request = { input: this.skillImportInput };
                    if (this.skillImportHasConflict) {
                        request.conflictStrategy = this.skillImportConflictStrategy;
                    }
                    const result = await requireSkillApi().installImport(request);
                    this.skills = await requireSkillApi().list();
                    this.selectedSkillName = result.name;
                    this.expandedSkillFolders = {};
                    this.skillImportInput = null;
                    this.skillImportPreview = null;
                    await this.loadSelectedSkillFiles();
                    this.toast(tr('skillInstallToast', {
                        action: translateSkillInstallAction(result.action),
                        name: result.name,
                    }));
                } catch (error) {
                    this.skillImportInput = null;
                    this.skillImportPreview = null;
                    this.reportError(error);
                    throw error;
                }
            },
            async exportSelectedSkill() {
                if (!this.selectedSkillName) {
                    throw new Error(tr('selectSkillFirst'));
                }
                const payload = await requireSkillApi().exportSkill({ name: this.selectedSkillName });
                const anchor = document.createElement('a');
                anchor.href = `data:application/zip;base64,${payload.contentBase64}`;
                anchor.download = payload.fileName;
                document.body.appendChild(anchor);
                anchor.click();
                anchor.remove();
                this.toast(tr('exportedSkill', { name: this.selectedSkillName }));
            },
            async deleteSelectedSkill() {
                if (!this.selectedSkillName) {
                    throw new Error(tr('selectSkillFirst'));
                }
                const name = this.selectedSkillName;
                if (!await confirmAction(tr('deleteSkillConfirm', { name }))) {
                    return;
                }

                await this.clearSkillImportDraft();
                await requireSkillApi().deleteSkill({ name });
                this.expandedSkillFolders = {};
                this.skillFiles = [];
                await this.refreshSkills();
                this.toast(tr('deletedSkill', { name }));
            },
            prettyJson(value) {
                return prettyJson(value);
            },
            reportError(error) {
                const message = errorText(error);
                this.error = message;
                console.error('[AgentSystem]', error);
                toastr.error(message);
            },
            toast(message) {
                toastr.success(message);
            },
        },
        template: `
            <div class="ttas-root ttas-panel-root">
                <header class="ttas-titlebar">
                    <div class="ttas-titlebar-main">
                        <div class="ttas-title-icon" aria-hidden="true">
                            <i class="fa-solid fa-atom"></i>
                        </div>
                        <div class="ttas-title-copy">
                            <div class="ttas-eyebrow">{{ tr('tauriTavernAgent') }}</div>
                            <h3>{{ tr('agentSystem') }}</h3>
                        </div>
                    </div>
                    <button type="button" class="menu_button menu_button_icon ttas-close-button" :title="tr('close')" @click="closePanel">
                        <i class="fa-solid fa-xmark"></i>
                    </button>
                </header>

                <div v-if="loading && !initialized" class="ttas-loading">{{ tr('loadingAgentSystem') }}</div>
                <div v-else class="ttas-panel-body">
                    <div v-if="error" class="ttas-error">
                        <i class="fa-solid fa-triangle-exclamation"></i>
                        <pre>{{ error }}</pre>
                    </div>

                    <nav class="ttas-tabs">
                        <button v-for="tab in tabs" :key="tab.id" type="button" class="menu_button" :class="{ active: activeTab === tab.id }" @click="setTab(tab.id)">
                            <i class="fa-solid" :class="tab.icon"></i>
                            <span>{{ tr(tab.labelKey) }}</span>
                        </button>
                    </nav>

                    <transition name="ttas-panel-fade" mode="out-in">
                    <section v-if="activeTab === 'profiles'" key="profiles" class="ttas-panel">
                        <div class="ttas-profile-layout">
                            <aside class="ttas-list ttas-side-list">
                                <div class="ttas-list-header">
                                    <h4>{{ tr('profiles') }}</h4>
                                    <span>{{ tr('profileCount', { count: profiles.length }) }}</span>
                                </div>
                                <button v-for="profile in profiles" :key="profile.id" type="button" :class="{ active: selectedProfileId === profile.id }" @click="selectProfile(profile.id)">
                                    <strong>{{ profile.displayName }}</strong>
                                    <span>{{ profile.id }}</span>
                                    <small v-if="profile.description">{{ profile.description }}</small>
                                </button>
                            </aside>
                            <div class="ttas-editor">
                                <label class="ttas-mobile-select ttas-field">
                                    <span>{{ tr('profiles') }}</span>
                                    <select :value="selectedProfileId" @change="selectProfile($event.target.value)">
                                        <option v-for="profile in profiles" :key="profile.id" :value="profile.id">{{ profile.displayName }}</option>
                                    </select>
                                </label>

                                <div class="ttas-editor-hero">
                                    <div class="ttas-hero-copy">
                                        <div class="ttas-eyebrow">{{ tr('profileSummary') }}</div>
                                        <h4>{{ draft.displayName || draft.id }}</h4>
                                        <p>{{ draft.id }}</p>
                                    </div>
                                    <div class="ttas-editor-actions">
                                        <button type="button" class="menu_button menu_button_icon" @click="newProfile">
                                            <i class="fa-solid fa-plus"></i>
                                            <span>{{ tr('new') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon" @click="copyProfile">
                                            <i class="fa-solid fa-copy"></i>
                                            <span>{{ tr('copy') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon" @click="exportSelectedProfile" :disabled="saving">
                                            <i class="fa-solid fa-file-export"></i>
                                            <span>{{ tr('export') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon ttas-primary-button" @click="saveProfile" :disabled="saving || isBuiltinProfile">
                                            <i class="fa-solid" :class="saving ? 'fa-spinner fa-spin' : 'fa-floppy-disk'"></i>
                                            <span>{{ tr('save') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon ttas-danger-button" @click="deleteProfile" :disabled="isBuiltinProfile">
                                            <i class="fa-solid fa-trash-can"></i>
                                            <span>{{ tr('delete') }}</span>
                                        </button>
                                    </div>
                                    <div class="ttas-stat-grid">
                                        <div v-for="stat in profileStats" :key="stat.label" class="ttas-stat">
                                            <i class="fa-solid" :class="stat.icon"></i>
                                            <span>{{ stat.label }}</span>
                                            <strong>{{ stat.value }}</strong>
                                        </div>
                                    </div>
                                </div>

                                <div class="ttas-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-fingerprint"></i>
                                        <h4>{{ tr('identity') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('profileId') }}</span>
                                            <input class="text_pole" v-model="draft.id" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('displayName') }}</span>
                                            <input class="text_pole" v-model="draft.displayName" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field ttas-span-2">
                                            <span>{{ tr('description') }}</span>
                                            <input class="text_pole" v-model="draft.description" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-gauge-high"></i>
                                        <h4>{{ tr('runPolicy') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('presentation') }}</span>
                                            <select v-model="draft.run.presentation" :disabled="isBuiltinProfile">
                                                <option value="foreground">{{ tr('foreground') }}</option>
                                                <option value="background">{{ tr('background') }}</option>
                                            </select>
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('planMode') }}</span>
                                            <select v-model="draft.plan.mode" :disabled="isBuiltinProfile">
                                                <option value="none">{{ tr('none') }}</option>
                                            </select>
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('maxRounds') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.tools.maxRounds" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('maxToolCalls') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.tools.maxCallsPerRun" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('modelRetries') }}</span>
                                            <input class="text_pole" type="number" min="0" v-model.number="draft.run.modelRetry.maxRetries" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('retryIntervalMs') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.run.modelRetry.intervalMs" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-layer-group"></i>
                                        <h4>{{ tr('initialContext') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('initialChatHistoryMessages') }}</span>
                                            <input class="text_pole" type="number" step="1" v-model.number="draft.context.initialChatHistoryMessages" :disabled="isBuiltinProfile" />
                                            <small class="ttas-field-hint">
                                                <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
                                                <span>{{ tr('initialChatHistoryMessagesHint') }}</span>
                                            </small>
                                        </label>
                                        <label class="checkbox_label ttas-field">
                                            <span>{{ tr('includeActivatedWorldInfo') }}</span>
                                            <input type="checkbox" v-model="draft.context.includeActivatedWorldInfo" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-terminal"></i>
                                        <h4>{{ tr('prompt') }}</h4>
                                    </div>
                                    <label class="ttas-field">
                                        <span>{{ tr('agentSystemPrompt') }}</span>
                                        <textarea class="text_pole textarea_compact ttas-system-prompt-textarea" rows="12" :value="agentSystemPromptEditorValue" :placeholder="agentSystemPromptPlaceholder" :disabled="isBuiltinProfile" @input="setAgentSystemPromptDraft"></textarea>
                                    </label>
                                </div>

                                <div class="ttas-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-screwdriver-wrench"></i>
                                        <h4>{{ tr('capabilityMatrix') }}</h4>
                                    </div>
                                    <div class="ttas-tool-workbench">
                                        <div class="ttas-tool-groups">
                                            <div v-for="group in toolGroupsWithTools" :key="group.id" class="ttas-tool-group">
                                                <header>
                                                    <i class="fa-solid" :class="group.icon"></i>
                                                    <strong>{{ tr(group.labelKey) }}</strong>
                                                    <span>{{ enabledToolCount(group.tools) }}/{{ group.tools.length }}</span>
                                                </header>
                                                <div class="ttas-tool-list">
                                                    <div
                                                        v-for="tool in group.tools"
                                                        :key="tool"
                                                        class="ttas-tool-row"
                                                        :class="{ active: selectedToolName === tool, enabled: draft.tools.allow.includes(tool), customized: toolHasDescriptionOverride(tool) }"
                                                    >
                                                        <input
                                                            type="checkbox"
                                                            :checked="draft.tools.allow.includes(tool)"
                                                            :disabled="isBuiltinProfile"
                                                            @change="toggleToolAllowed(tool, $event)"
                                                        />
                                                        <button type="button" class="ttas-tool-select" @click="selectTool(tool)">
                                                            <strong>{{ toolTitle(tool) }}</strong>
                                                            <span>{{ tool }}</span>
                                                        </button>
                                                        <i v-if="toolHasDescriptionOverride(tool)" class="fa-solid fa-pen-nib ttas-tool-custom-marker" :title="tr('customizedTool')"></i>
                                                    </div>
                                                </div>
                                            </div>
                                        </div>

                                        <aside v-if="selectedToolSpec" class="ttas-tool-editor-panel">
                                            <header class="ttas-tool-editor-header">
                                                <div>
                                                    <div class="ttas-eyebrow">{{ selectedToolName }}</div>
                                                    <h5>{{ selectedToolSpec.title }}</h5>
                                                </div>
                                                <button
                                                    type="button"
                                                    class="menu_button menu_button_icon"
                                                    :disabled="isBuiltinProfile || !toolHasDescriptionOverride(selectedToolName)"
                                                    @click="resetToolDescriptionOverride(selectedToolName)"
                                                >
                                                    <i class="fa-solid fa-rotate-left"></i>
                                                    <span>{{ tr('reset') }}</span>
                                                </button>
                                            </header>

                                            <div class="ttas-tool-badge-row">
                                                <span class="ttas-tool-model-name">{{ toolModelName(selectedToolName) }}</span>
                                                <span v-if="toolSource(selectedToolName)">{{ toolSource(selectedToolName) }}</span>
                                                <span v-for="badge in toolBadges(selectedToolName)" :key="badge.key" :class="'ttas-tool-badge-' + badge.key">{{ badge.label }}</span>
                                                <span v-if="!selectedToolEnabled" class="ttas-tool-badge-disabled">{{ tr('disabledTool') }}</span>
                                            </div>

                                            <div class="ttas-tool-default-description">
                                                <span>{{ tr('defaultDescription') }}</span>
                                                <p>{{ selectedToolSpec.description }}</p>
                                            </div>

                                            <label class="ttas-field">
                                                <span>{{ tr('customToolDescription') }}</span>
                                                <textarea
                                                    class="text_pole textarea_compact ttas-tool-description-textarea"
                                                    rows="5"
                                                    :value="getToolDescriptionOverride(selectedToolName)"
                                                    :placeholder="selectedToolSpec.description"
                                                    :disabled="isBuiltinProfile || !selectedToolEnabled"
                                                    @input="setToolDescriptionOverride(selectedToolName, $event.target.value)"
                                                ></textarea>
                                            </label>

                                            <div class="ttas-tool-property-list">
                                                <div class="ttas-tool-property-title">
                                                    <i class="fa-solid fa-sliders"></i>
                                                    <strong>{{ tr('toolParameters') }}</strong>
                                                </div>
                                                <div v-if="selectedToolProperties.length === 0" class="ttas-empty">{{ tr('noToolParameters') }}</div>
                                                <div v-for="property in selectedToolProperties" :key="property.name" class="ttas-tool-property-row">
                                                    <div class="ttas-tool-property-meta">
                                                        <code>{{ property.name }}</code>
                                                        <span>{{ property.type }}</span>
                                                        <em v-if="property.required">{{ tr('required') }}</em>
                                                    </div>
                                                    <p v-if="property.description">{{ property.description }}</p>
                                                    <div class="ttas-tool-property-edit">
                                                        <textarea
                                                            class="text_pole textarea_compact"
                                                            rows="3"
                                                            :value="getToolPropertyDescriptionOverride(selectedToolName, property.name)"
                                                            :placeholder="property.description"
                                                            :disabled="isBuiltinProfile || !selectedToolEnabled"
                                                            @input="setToolPropertyDescriptionOverride(selectedToolName, property.name, $event.target.value)"
                                                        ></textarea>
                                                        <button
                                                            type="button"
                                                            class="menu_button menu_button_icon"
                                                            :disabled="isBuiltinProfile || !getToolPropertyDescriptionOverride(selectedToolName, property.name)"
                                                            @click="resetToolPropertyDescriptionOverride(selectedToolName, property.name)"
                                                        >
                                                            <i class="fa-solid fa-rotate-left"></i>
                                                            <span>{{ tr('reset') }}</span>
                                                        </button>
                                                    </div>
                                                </div>
                                            </div>
                                        </aside>
                                    </div>
                                </div>

                                <div class="ttas-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-book"></i>
                                        <h4>{{ tr('skillAccess') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('visibleSkills') }}</span>
                                            <input class="text_pole" v-model="draft.skills.visibleCsv" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('deniedSkills') }}</span>
                                            <input class="text_pole" v-model="draft.skills.denyCsv" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('maxCharsPerCall') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.skills.maxReadCharsPerCall" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('maxCharsPerRun') }}</span>
                                            <input class="text_pole" type="number" min="1" v-model.number="draft.skills.maxReadCharsPerRun" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-folder-tree"></i>
                                        <h4>{{ tr('workspaceAccess') }}</h4>
                                    </div>
                                    <div class="ttas-root-grid">
                                        <div v-for="root in workspaceRoots" :key="root" class="ttas-root-row">
                                            <div class="ttas-root-name">
                                                <i class="fa-solid" :class="workspaceRootIcon(root)"></i>
                                                <strong>{{ root }}</strong>
                                            </div>
                                            <label class="checkbox_label">
                                                <input type="checkbox" :value="root" v-model="draft.workspace.visibleRoots" @change="syncWritableRoots" :disabled="isBuiltinProfile" />
                                                <span>{{ tr('visible') }}</span>
                                            </label>
                                            <label class="checkbox_label">
                                                <input type="checkbox" :value="root" v-model="draft.workspace.writableRoots" :disabled="isBuiltinProfile || !draft.workspace.visibleRoots.includes(root)" />
                                                <span>{{ tr('writable') }}</span>
                                            </label>
                                        </div>
                                    </div>
                                </div>

                                <div class="ttas-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-file-lines"></i>
                                        <h4>{{ tr('outputArtifact') }}</h4>
                                    </div>
                                    <div class="ttas-form-grid">
                                        <label class="ttas-field">
                                            <span>{{ tr('messageBodyPath') }}</span>
                                            <input class="text_pole" v-model="draft.output.artifacts[0].path" :disabled="isBuiltinProfile" />
                                        </label>
                                        <label class="ttas-field">
                                            <span>{{ tr('kind') }}</span>
                                            <input class="text_pole" v-model="draft.output.artifacts[0].kind" :disabled="isBuiltinProfile" />
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section ttas-json-section">
                                    <div class="ttas-pane-header">
                                        <div class="ttas-section-title">
                                            <i class="fa-solid fa-code"></i>
                                            <h4>{{ tr('advancedJson') }}</h4>
                                        </div>
                                        <div class="ttas-toolbar">
                                            <button type="button" class="menu_button" @click="refreshDraftJson">{{ tr('refreshJson') }}</button>
                                            <button type="button" class="menu_button" @click="applyDraftJson" :disabled="isBuiltinProfile">{{ tr('applyJson') }}</button>
                                        </div>
                                    </div>
                                    <textarea class="text_pole ttas-json" v-model="draftJson" :readonly="isBuiltinProfile"></textarea>
                                </div>
                            </div>
                        </div>
                    </section>

                    <section v-else-if="activeTab === 'skills'" key="skills" class="ttas-panel">
                        <div class="ttas-grid">
                            <aside class="ttas-list ttas-side-list">
                                <div class="ttas-list-header">
                                    <h4>{{ tr('skills') }}</h4>
                                    <span>{{ tr('skillCount', { count: skills.length }) }}</span>
                                </div>
                                <button v-for="skill in skills" :key="skill.name" type="button" :class="{ active: selectedSkillName === skill.name }" @click="selectSkill(skill.name)">
                                    <strong>{{ skill.displayName || skill.name }}</strong>
                                    <span>{{ skill.name }}</span>
                                    <small>{{ skill.description }}</small>
                                </button>
                                <p v-if="skills.length === 0" class="ttas-empty">{{ tr('noSkillsInstalled') }}</p>
                            </aside>

                            <div class="ttas-pane ttas-skill-pane">
                                <label class="ttas-mobile-select ttas-field">
                                    <span>{{ tr('skills') }}</span>
                                    <select :value="selectedSkillName" @change="selectSkill($event.target.value)" :disabled="skills.length === 0">
                                        <option v-for="skill in skills" :key="skill.name" :value="skill.name">{{ skill.displayName || skill.name }}</option>
                                    </select>
                                </label>

                                <div class="ttas-skill-hero">
                                    <div v-if="selectedSkill" class="ttas-skill-meta">
                                        <div class="ttas-eyebrow">{{ selectedSkill.name }}</div>
                                        <h4>{{ selectedSkill.displayName || selectedSkill.name }}</h4>
                                        <p>{{ selectedSkill.description }}</p>
                                        <div class="ttas-tags">
                                            <span v-for="tag in selectedSkillTags" :key="tag">{{ tag }}</span>
                                        </div>
                                    </div>
                                    <div class="ttas-editor-actions">
                                        <button type="button" class="menu_button menu_button_icon" @click="refreshSkills">
                                            <i class="fa-solid fa-rotate"></i>
                                            <span>{{ tr('refresh') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon" @click="exportSelectedSkill" :disabled="!selectedSkillName">
                                            <i class="fa-solid fa-file-export"></i>
                                            <span>{{ tr('export') }}</span>
                                        </button>
                                        <button type="button" class="menu_button menu_button_icon ttas-danger-button" @click="deleteSelectedSkill" :disabled="!selectedSkillName">
                                            <i class="fa-solid fa-trash-can"></i>
                                            <span>{{ tr('delete') }}</span>
                                        </button>
                                    </div>
                                </div>

                                <div class="ttas-section ttas-files-section">
                                    <div class="ttas-section-title">
                                        <i class="fa-solid fa-folder-open"></i>
                                        <h4>{{ tr('files') }}</h4>
                                    </div>
                                    <div class="ttas-file-viewport" :class="{ loading: loadingSkillFiles }">
                                        <div v-if="loadingSkillFiles" class="ttas-file-loading" role="status" aria-live="polite">
                                            <span>{{ tr('loadingSkillFiles') }}</span>
                                            <div class="ttas-file-loading-lines" aria-hidden="true">
                                                <i v-for="index in 5" :key="index"></i>
                                            </div>
                                        </div>
                                        <ul v-else-if="selectedSkillFileTree.length > 0" class="ttas-file-tree ttas-file-tree-root">
                                            <SkillFileTreeNode
                                                v-for="node in selectedSkillFileTree"
                                                :key="node.path"
                                                :node="node"
                                                :depth="0"
                                                :is-folder-open="isFolderOpen"
                                                @toggle-folder="toggleSkillFolder"
                                                @open-file="openSkillFile"
                                            />
                                        </ul>
                                        <p v-else class="ttas-empty ttas-file-empty">{{ tr('noFilesFoundForSkill') }}</p>
                                    </div>
                                </div>
                            </div>
                        </div>

                        <div class="ttas-section">
                            <div class="ttas-section-title">
                                <i class="fa-solid fa-box-archive"></i>
                                <h4>{{ tr('importSkillArchive') }}</h4>
                            </div>
                            <div class="ttas-toolbar">
                                <button type="button" class="menu_button menu_button_icon" @click="pickAndPreviewSkillImport">
                                    <i class="fa-solid fa-file-import"></i>
                                    <span>{{ tr('import') }}</span>
                                </button>
                                <button type="button" class="menu_button menu_button_icon" @click="installSkillImport" :disabled="!skillImportPreview">
                                    <i class="fa-solid fa-box-archive"></i>
                                    <span>{{ tr('install') }}</span>
                                </button>
                                <select v-if="skillImportHasConflict" v-model="skillImportConflictStrategy">
                                    <option value="skip">{{ tr('skipConflict') }}</option>
                                    <option value="replace">{{ tr('replaceConflict') }}</option>
                                </select>
                            </div>
                            <div v-if="skillImportPath" class="ttas-import-path">
                                <i class="fa-solid fa-file-zipper"></i>
                                <span>{{ skillImportPath }}</span>
                            </div>
                            <div v-if="skillImportPreview" class="ttas-import-preview">
                                <div class="ttas-import-summary">
                                    <div>
                                        <span>{{ tr('name') }}</span>
                                        <strong>{{ skillImportPreviewSkill.name }}</strong>
                                    </div>
                                    <div>
                                        <span>{{ tr('conflict') }}</span>
                                        <strong>{{ skillImportConflictText }}</strong>
                                    </div>
                                    <div>
                                        <span>{{ tr('files') }}</span>
                                        <strong>{{ tr('fileCount', { count: skillImportPreviewSkill.fileCount }) }}</strong>
                                    </div>
                                    <div>
                                        <span>{{ tr('size') }}</span>
                                        <strong>{{ tr('byteCount', { count: skillImportPreviewSkill.totalBytes }) }}</strong>
                                    </div>
                                </div>
                                <div v-if="skillImportWarnings.length > 0" class="ttas-warning-list">
                                    <strong>{{ tr('importWarnings', { count: skillImportWarnings.length }) }}</strong>
                                    <ul>
                                        <li v-for="warning in skillImportWarnings" :key="warning">{{ warning }}</li>
                                    </ul>
                                </div>
                                <details class="ttas-details">
                                    <summary>{{ tr('importDetails') }}</summary>
                                    <pre class="ttas-json">{{ prettyJson(skillImportPreview) }}</pre>
                                </details>
                            </div>
                        </div>
                    </section>
                    </transition>
                </div>
            </div>
        `,
    };
}
