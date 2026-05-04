import { DEFAULT_PROFILE_ID, KNOWN_TOOLS, WORKSPACE_ROOTS } from './constants.js';
import { errorText, prettyJson, requireAgentApi, requireSkillApi } from './host-api.js';
import { translateAgentSystem as tr, translateSkillInstallAction } from './i18n.js';
import { defaultProfile, normalizeProfileForSave, normalizeProfileId, profileForEdit } from './profile-model.js';
import { loadSettings, patchSettings } from './settings-store.js';
import { buildSkillFileTree } from './skill-file-tree.js';
import { openSkillFileViewer } from './skill-file-viewer.js';

const SKILL_FILE_VIEW_MAX_CHARS = 80000;

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
                skills: [],
                selectedSkillName: '',
                skillFiles: [],
                loadingSkillFiles: false,
                expandedSkillFolders: {},
                skillImportKind: 'archiveFile',
                skillImportPath: '',
                skillImportConflictStrategy: 'skip',
                skillImportPreview: null,
                tabs: [
                    { id: 'profiles', labelKey: 'profiles' },
                    { id: 'skills', labelKey: 'skills' },
                ],
                toolNames: KNOWN_TOOLS,
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
            selectedSkill() {
                return this.skills.find((skill) => skill.name === this.selectedSkillName) || null;
            },
            selectedSkillFileTree() {
                if (!this.selectedSkill) {
                    return [];
                }

                return buildSkillFileTree(this.skillFiles);
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
                    await Promise.all([this.refreshProfiles(), this.refreshSkills()]);
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
            closePanel() {
                requestClose();
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
            async selectProfile(profileId) {
                const id = profileId || DEFAULT_PROFILE_ID;
                const result = await requireAgentApi().profiles.load({ profileId: id });
                if (!result?.profile) {
                    throw new Error(tr('agentProfileNotFound', { id }));
                }
                this.selectedProfileId = id;
                this.draft = profileForEdit(result.profile);
                this.refreshDraftJson();
            },
            refreshDraftJson() {
                this.draftJson = prettyJson(normalizeProfileForSave(this.draft));
            },
            applyDraftJson() {
                const parsed = JSON.parse(this.draftJson);
                this.draft = profileForEdit(parsed);
                this.selectedProfileId = parsed.id;
            },
            newProfile() {
                const id = this.nextProfileId('agent-profile');
                this.selectedProfileId = id;
                this.draft = profileForEdit(defaultProfile(id));
                this.refreshDraftJson();
            },
            copyProfile() {
                const id = this.nextProfileId(`${this.draft.id}-copy`);
                const copy = normalizeProfileForSave(this.draft);
                copy.id = id;
                copy.displayName = tr('copyDisplayName', { name: copy.displayName });
                this.selectedProfileId = id;
                this.draft = profileForEdit(copy);
                this.refreshDraftJson();
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
            async deleteProfile() {
                if (this.isBuiltinProfile) {
                    throw new Error(tr('agentProfileBuiltInDelete'));
                }
                const id = this.draft.id;
                if (!window.confirm(tr('deleteAgentProfileConfirm', { id }))) {
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
                    return;
                }
                if (!this.skills.some((skill) => skill.name === this.selectedSkillName)) {
                    this.selectedSkillName = this.skills[0].name;
                }
                await this.loadSelectedSkillFiles();
            },
            async selectSkill(skillName) {
                this.selectedSkillName = skillName;
                this.expandedSkillFolders = {};
                await this.loadSelectedSkillFiles();
            },
            async loadSelectedSkillFiles() {
                if (!this.selectedSkillName) {
                    this.skillFiles = [];
                    return;
                }
                this.skillFiles = [];
                this.loadingSkillFiles = true;
                try {
                    this.skillFiles = await requireSkillApi().listFiles({ name: this.selectedSkillName });
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    this.loadingSkillFiles = false;
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
            async previewSkillImport() {
                const input = {
                    kind: this.skillImportKind,
                    path: this.skillImportPath.trim(),
                };
                this.skillImportPreview = await requireSkillApi().previewImport(input);
            },
            async installSkillImport() {
                const input = {
                    kind: this.skillImportKind,
                    path: this.skillImportPath.trim(),
                };
                const request = { input };
                if (this.skillImportPreview?.conflict?.kind === 'different') {
                    request.conflictStrategy = this.skillImportConflictStrategy;
                }
                const result = await requireSkillApi().installImport(request);
                this.skills = await requireSkillApi().list();
                this.selectedSkillName = result.name;
                this.expandedSkillFolders = {};
                await this.loadSelectedSkillFiles();
                this.toast(tr('skillInstallToast', {
                    action: translateSkillInstallAction(result.action),
                    name: result.name,
                }));
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
            },
            prettyJson(value) {
                return prettyJson(value);
            },
            reportError(error) {
                const message = errorText(error);
                this.error = message;
                console.error('[AgentSystem]', error);
                window.toastr?.error?.(message);
            },
            toast(message) {
                window.toastr?.success?.(message);
            },
        },
        template: `
            <div class="ttas-root ttas-panel-root">
                <header class="ttas-titlebar">
                    <div>
                        <div class="ttas-eyebrow">{{ tr('tauriTavernAgent') }}</div>
                        <h3>{{ tr('agentSystem') }}</h3>
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
                            {{ tr(tab.labelKey) }}
                        </button>
                    </nav>

                    <section v-if="activeTab === 'profiles'" class="ttas-panel">
                        <div class="ttas-profile-layout">
                            <aside class="ttas-list">
                                <button v-for="profile in profiles" :key="profile.id" type="button" :class="{ active: selectedProfileId === profile.id }" @click="selectProfile(profile.id)">
                                    <strong>{{ profile.displayName }}</strong>
                                    <span>{{ profile.id }}</span>
                                </button>
                            </aside>
                            <div class="ttas-editor">
                                <div class="ttas-toolbar">
                                    <button type="button" class="menu_button menu_button_icon" @click="newProfile">
                                        <i class="fa-solid fa-plus"></i>
                                        <span>{{ tr('new') }}</span>
                                    </button>
                                    <button type="button" class="menu_button menu_button_icon" @click="copyProfile">
                                        <i class="fa-solid fa-copy"></i>
                                        <span>{{ tr('copy') }}</span>
                                    </button>
                                    <button type="button" class="menu_button menu_button_icon" @click="saveProfile" :disabled="saving || isBuiltinProfile">
                                        <i class="fa-solid fa-floppy-disk"></i>
                                        <span>{{ tr('save') }}</span>
                                    </button>
                                    <button type="button" class="menu_button menu_button_icon" @click="deleteProfile" :disabled="isBuiltinProfile">
                                        <i class="fa-solid fa-trash-can"></i>
                                        <span>{{ tr('delete') }}</span>
                                    </button>
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

                                <label class="ttas-field">
                                    <span>{{ tr('agentSystemPrompt') }}</span>
                                    <textarea class="text_pole textarea_compact" rows="12" v-model="draft.instructions.agentSystemPrompt" :disabled="isBuiltinProfile"></textarea>
                                </label>

                                <div class="ttas-section">
                                    <h4>{{ tr('tools') }}</h4>
                                    <div class="ttas-check-grid">
                                        <label v-for="tool in toolNames" :key="tool" class="checkbox_label">
                                            <input type="checkbox" :value="tool" v-model="draft.tools.allow" :disabled="isBuiltinProfile" />
                                            <span>{{ tool }}</span>
                                        </label>
                                    </div>
                                </div>

                                <div class="ttas-section">
                                    <h4>{{ tr('skills') }}</h4>
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
                                    <h4>{{ tr('workspaceRoots') }}</h4>
                                    <div class="ttas-root-grid">
                                        <div v-for="root in workspaceRoots" :key="root" class="ttas-root-row">
                                            <strong>{{ root }}</strong>
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
                                    <h4>{{ tr('outputArtifact') }}</h4>
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

                                <div class="ttas-section">
                                    <div class="ttas-pane-header">
                                        <h4>{{ tr('advancedJson') }}</h4>
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

                    <section v-if="activeTab === 'skills'" class="ttas-panel">
                        <div class="ttas-toolbar">
                            <button type="button" class="menu_button menu_button_icon" @click="refreshSkills">
                                <i class="fa-solid fa-rotate"></i>
                                <span>{{ tr('refresh') }}</span>
                            </button>
                            <button type="button" class="menu_button menu_button_icon" @click="exportSelectedSkill" :disabled="!selectedSkillName">
                                <i class="fa-solid fa-file-export"></i>
                                <span>{{ tr('export') }}</span>
                            </button>
                        </div>

                        <div class="ttas-grid">
                            <aside class="ttas-list">
                                <button v-for="skill in skills" :key="skill.name" type="button" :class="{ active: selectedSkillName === skill.name }" @click="selectSkill(skill.name)">
                                    <strong>{{ skill.displayName || skill.name }}</strong>
                                    <span>{{ skill.name }}</span>
                                </button>
                                <p v-if="skills.length === 0" class="ttas-empty">{{ tr('noSkillsInstalled') }}</p>
                            </aside>

                            <div class="ttas-pane">
                                <div v-if="selectedSkill" class="ttas-skill-meta">
                                    <h4>{{ selectedSkill.displayName || selectedSkill.name }}</h4>
                                    <p>{{ selectedSkill.description }}</p>
                                    <div class="ttas-tags">
                                        <span>{{ tr('fileCount', { count: selectedSkill.fileCount }) }}</span>
                                        <span>{{ tr('byteCount', { count: selectedSkill.totalBytes }) }}</span>
                                        <span v-if="selectedSkill.hasScripts">{{ tr('scriptsIncluded') }}</span>
                                        <span v-if="selectedSkill.hasBinary">{{ tr('binaryFiles') }}</span>
                                    </div>
                                </div>
                                <div v-if="loadingSkillFiles" class="ttas-loading">{{ tr('loadingSkillFiles') }}</div>
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
                                <p v-else class="ttas-empty">{{ tr('noFilesFoundForSkill') }}</p>
                            </div>
                        </div>

                        <div class="ttas-section">
                            <h4>{{ tr('installSkillFromPath') }}</h4>
                            <div class="ttas-toolbar">
                                <select v-model="skillImportKind">
                                    <option value="archiveFile">{{ tr('archiveFile') }}</option>
                                    <option value="directory">{{ tr('directory') }}</option>
                                </select>
                                <input class="text_pole" v-model="skillImportPath" />
                                <select v-model="skillImportConflictStrategy">
                                    <option value="skip">{{ tr('skipConflict') }}</option>
                                    <option value="replace">{{ tr('replaceConflict') }}</option>
                                </select>
                                <button type="button" class="menu_button" @click="previewSkillImport">{{ tr('preview') }}</button>
                                <button type="button" class="menu_button" @click="installSkillImport">{{ tr('install') }}</button>
                            </div>
                            <pre v-if="skillImportPreview" class="ttas-json">{{ prettyJson(skillImportPreview) }}</pre>
                        </div>
                    </section>
                </div>
            </div>
        `,
    };
}
