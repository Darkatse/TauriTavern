import { DEFAULT_PROFILE_ID } from '../constants.js';
import { confirmAction, errorText, requireAgentApi, requireSkillApi } from '../host-api.js';
import { translateAgentSystem as tr, translateSkillInstallAction } from '../i18n.js';
import { loadSettings, subscribeSettings } from '../settings-store.js';
import { downloadBlobWithRuntime } from '../../../../file-export.js';
import { subscribeAgentProfilesChanged } from '../../../../tauritavern/agent/agent-profile-events.js';
import { buildSkillFileTree } from './file-tree.js';
import { SkillFileTreeNode } from './file-tree-node.js';
import { SkillFileViewer } from './file-viewer.js';
import {
    syncSkillDeletePortability,
    syncSkillInstallPortability,
    syncSkillMovePortability,
    syncSkillWritePortability,
} from './embedded-skill-sync.js';
import { buildSkillScopeSections, skillScopeKey, skillScopeLabel } from './scope.js';

const SKILL_FILE_VIEW_MAX_CHARS = 80000;
const SKILL_ARCHIVE_CONTENT_TYPE = 'application/zip';
const HOST_SCOPE_EVENT_KEYS = Object.freeze([
    'CHAT_CHANGED',
    'CHAT_LOADED',
    'CHARACTER_EDITED',
    'CHARACTER_DELETED',
    'CHARACTER_RENAMED',
    'PRESET_CHANGED',
    'PRESET_DELETED',
    'PRESET_RENAMED',
    'MAIN_API_CHANGED',
]);

function emptyImportDraft() {
    return {
        input: null,
        preview: null,
        conflictStrategy: 'skip',
        loading: false,
        sectionId: '',
    };
}

function emptyScopeDialog() {
    return {
        mode: '',
        selectedSectionId: '',
        sourceSectionId: '',
        skill: null,
    };
}

function sortSkills(skills) {
    return [...skills].sort((left, right) => {
        const leftName = String(left.displayName || left.name || '');
        const rightName = String(right.displayName || right.name || '');
        return leftName.localeCompare(rightName, undefined, { sensitivity: 'base' });
    });
}

function base64ToBlob(contentBase64, contentType) {
    const binary = atob(contentBase64);
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) {
        bytes[index] = binary.charCodeAt(index);
    }
    return new Blob([bytes], { type: contentType });
}

function includesText(value, query) {
    return String(value || '').toLowerCase().includes(query);
}

export function createSkillManagerPanelRoot() {
    return {
        components: {
            SkillFileTreeNode,
            SkillFileViewer,
        },
        data() {
            return {
                initialized: false,
                loading: false,
                error: '',
                profiles: [],
                selectedProfileId: DEFAULT_PROFILE_ID,
                sections: [],
                importDraft: emptyImportDraft(),
                scopeDialog: emptyScopeDialog(),
                searchQuery: '',
                preview: null,
                previewRequestId: 0,
                fileViewer: null,
                fileViewerRequestId: 0,
                hostScopeEventDisposers: [],
                extensionEventDisposers: [],
            };
        },
        computed: {
            availableScopeSections() {
                return this.sections.filter((section) => section.available);
            },
            currentImportSection() {
                return this.findSection(this.importDraft.sectionId);
            },
            normalizedSearchQuery() {
                return String(this.searchQuery || '').trim().toLowerCase();
            },
            previewFileTree() {
                if (!this.preview) {
                    return [];
                }
                return buildSkillFileTree(this.preview.files);
            },
            scopeDialogTargets() {
                if (this.scopeDialog.mode === 'move') {
                    return this.availableScopeSections.filter((section) => section.id !== this.scopeDialog.sourceSectionId);
                }
                return this.availableScopeSections;
            },
        },
        async mounted() {
            await this.initialize();
        },
        unmounted() {
            this.unsubscribeHostScopeEvents();
            this.unsubscribeExtensionEvents();
            if (this.importDraft.input) {
                this.handleAsyncEvent(() => this.clearImportDraft());
            }
        },
        methods: {
            async initialize() {
                this.loading = true;
                try {
                    const settings = await loadSettings();
                    await this.refreshProfiles();
                    this.selectedProfileId = String(settings.editingProfileId || this.selectedProfileId || DEFAULT_PROFILE_ID);
                    if (!this.profiles.some((profile) => profile.id === this.selectedProfileId)) {
                        this.selectedProfileId = this.profiles[0]?.id || DEFAULT_PROFILE_ID;
                    }
                    this.rebuildSections();
                    await this.refreshAllSections();
                    this.subscribeHostScopeEvents();
                    this.subscribeExtensionEvents();
                    this.initialized = true;
                    await this.syncHostScopes();
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    this.loading = false;
                }
            },
            tr(key, params) {
                return tr(key, params);
            },
            reportError(error) {
                const message = errorText(error);
                this.error = message;
                console.error('[AgentSystem:SkillManager]', error);
                toastr.error(message);
            },
            toast(message) {
                toastr.success(message);
            },
            requireDialog(dialog, messageKey) {
                if (
                    typeof HTMLDialogElement === 'undefined'
                    || !(dialog instanceof HTMLDialogElement)
                    || typeof dialog.showModal !== 'function'
                ) {
                    throw new Error(tr(messageKey));
                }
                return dialog;
            },
            async showDialog(refName, messageKey) {
                await this.$nextTick();
                const dialog = this.requireDialog(this.$refs[refName], messageKey);
                if (!dialog.open) {
                    dialog.showModal();
                }
            },
            requireHostContext() {
                const context = window.SillyTavern?.getContext?.();
                if (!context) {
                    throw new Error(tr('sillyTavernContextUnavailable'));
                }
                return context;
            },
            handleAsyncEvent(operation) {
                void (async () => {
                    try {
                        await operation();
                    } catch (error) {
                        if (this.error !== errorText(error)) {
                            this.reportError(error);
                        }
                        queueMicrotask(() => {
                            throw error;
                        });
                    }
                })();
            },
            subscribeHostScopeEvents() {
                if (this.hostScopeEventDisposers.length > 0) {
                    return;
                }

                const context = this.requireHostContext();
                const eventSource = context.eventSource;
                const eventTypes = context.eventTypes;
                if (!eventSource || !eventTypes) {
                    throw new Error(tr('sillyTavernContextUnavailable'));
                }

                const onHostScopeChanged = () => this.handleAsyncEvent(() => this.syncHostScopes());
                this.hostScopeEventDisposers = HOST_SCOPE_EVENT_KEYS.map((eventKey) => {
                    const eventName = eventTypes[eventKey];
                    if (!eventName) {
                        throw new Error(`SillyTavern event type is unavailable: ${eventKey}`);
                    }
                    eventSource.on(eventName, onHostScopeChanged);
                    return () => eventSource.removeListener(eventName, onHostScopeChanged);
                });
            },
            unsubscribeHostScopeEvents() {
                for (const dispose of this.hostScopeEventDisposers) {
                    dispose();
                }
                this.hostScopeEventDisposers = [];
            },
            subscribeExtensionEvents() {
                if (this.extensionEventDisposers.length > 0) {
                    return;
                }

                this.extensionEventDisposers = [
                    subscribeSettings((settings) => {
                        this.handleAsyncEvent(() => this.syncSelectedProfileFromSettings(settings));
                    }),
                    subscribeAgentProfilesChanged(() => {
                        this.handleAsyncEvent(() => this.syncProfilesChanged());
                    }),
                ];
            },
            unsubscribeExtensionEvents() {
                for (const dispose of this.extensionEventDisposers) {
                    dispose();
                }
                this.extensionEventDisposers = [];
            },
            async refreshProfiles() {
                const result = await requireAgentApi().profiles.list();
                this.profiles = Array.isArray(result?.profiles) ? result.profiles : [];
                if (!this.profiles.some((profile) => profile.id === this.selectedProfileId)) {
                    this.selectedProfileId = this.profiles[0]?.id || DEFAULT_PROFILE_ID;
                }
            },
            sectionScopeKey(section) {
                return section?.available ? skillScopeKey(section.scope) : '';
            },
            rebuildSections() {
                const previous = new Map(this.sections.map((section) => [section.id, section]));
                const changedSectionIds = [];
                this.sections = buildSkillScopeSections({
                    selectedProfileId: this.selectedProfileId,
                    profiles: this.profiles,
                }).map((section) => {
                    const previousSection = previous.get(section.id);
                    const previousScopeKey = this.sectionScopeKey(previousSection);
                    const nextScopeKey = this.sectionScopeKey(section);
                    const scopeUnchanged = Boolean(previousSection) && previousScopeKey === nextScopeKey;
                    if (!scopeUnchanged) {
                        changedSectionIds.push(section.id);
                    }
                    return {
                        ...section,
                        skills: scopeUnchanged ? previousSection.skills : [],
                        loading: scopeUnchanged ? previousSection.loading : false,
                    };
                });
                return changedSectionIds;
            },
            async refreshChangedSections(changedSectionIds) {
                const sectionIds = [...new Set(changedSectionIds)];
                if (sectionIds.length === 0) {
                    return;
                }

                if (this.preview && sectionIds.includes(this.preview.sectionId)) {
                    this.closePreview();
                }
                if (this.importDraft.sectionId && sectionIds.includes(this.importDraft.sectionId)) {
                    await this.clearImportDraft();
                }
                if (this.scopeDialog.mode) {
                    this.closeScopeDialog();
                }

                await Promise.all(sectionIds.map((sectionId) => this.refreshSection(sectionId)));
            },
            async refreshAll() {
                this.closePreview();
                await this.refreshProfiles();
                this.rebuildSections();
                await this.refreshAllSections();
            },
            async syncHostScopes() {
                if (!this.initialized) {
                    return;
                }

                const changedSectionIds = this.rebuildSections();
                await this.refreshChangedSections(changedSectionIds);
            },
            async syncSelectedProfileFromSettings(settings) {
                if (!this.initialized) {
                    return;
                }

                const nextProfileId = String(settings?.editingProfileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
                if (nextProfileId === this.selectedProfileId) {
                    return;
                }

                this.selectedProfileId = nextProfileId;
                await this.refreshProfiles();
                const changedSectionIds = new Set(this.rebuildSections());
                changedSectionIds.add('profile');
                await this.refreshChangedSections([...changedSectionIds]);
            },
            async syncProfilesChanged() {
                if (!this.initialized) {
                    return;
                }

                await this.refreshProfiles();
                const changedSectionIds = new Set(this.rebuildSections());
                changedSectionIds.add('profile');
                await this.refreshChangedSections([...changedSectionIds]);
            },
            async refreshAllSections() {
                await Promise.all(this.sections.map((section) => this.refreshSection(section.id)));
            },
            findSection(sectionId) {
                return this.sections.find((section) => section.id === sectionId) || null;
            },
            async refreshSection(sectionId) {
                const section = this.findSection(sectionId);
                if (!section) {
                    throw new Error(tr('skillScopeNotFound', { id: sectionId }));
                }
                if (!section.available) {
                    section.skills = [];
                    section.loading = false;
                    return;
                }

                const requestScopeKey = this.sectionScopeKey(section);
                section.loading = true;
                try {
                    const skills = sortSkills(await requireSkillApi().list({ scope: section.scope }));
                    const currentSection = this.findSection(sectionId);
                    if (!currentSection || this.sectionScopeKey(currentSection) !== requestScopeKey) {
                        return;
                    }

                    currentSection.skills = skills;
                    if (this.preview?.sectionId === currentSection.id && !currentSection.skills.some((skill) => skill.name === this.preview.skill.name)) {
                        this.closePreview();
                    }
                } catch (error) {
                    this.reportError(error);
                    throw error;
                } finally {
                    const currentSection = this.findSection(sectionId);
                    if (currentSection && this.sectionScopeKey(currentSection) === requestScopeKey) {
                        currentSection.loading = false;
                    }
                }
            },
            async selectProfile(profileId) {
                this.selectedProfileId = String(profileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
                const changedSectionIds = new Set(this.rebuildSections());
                changedSectionIds.add('profile');
                await this.refreshChangedSections([...changedSectionIds]);
            },
            sectionCountLabel(section) {
                return tr('skillCount', { count: section.skills.length });
            },
            sectionUnavailableText(section) {
                return tr(section.unavailableKey || 'skillScopeUnavailable');
            },
            visibleSkills(section) {
                const query = this.normalizedSearchQuery;
                if (!query) {
                    return section.skills;
                }
                return section.skills.filter((skill) => (
                    includesText(skill.displayName, query)
                    || includesText(skill.name, query)
                    || includesText(skill.description, query)
                    || includesText(skill.version, query)
                    || includesText(skill.sourceKind, query)
                ));
            },
            skillRowKey(section, skill) {
                return `${section.id}:${skill.name}:${skill.installedHash || ''}`;
            },
            skillTitle(skill) {
                return skill.displayName || skill.name;
            },
            skillSubtitle(skill) {
                const description = String(skill.description || '').trim();
                if (description) {
                    return description;
                }
                if (skill.displayName && skill.displayName !== skill.name) {
                    return skill.name;
                }
                return tr('defaultDescription');
            },
            scopeDialogTitle() {
                return this.scopeDialog.mode === 'move' ? tr('selectMoveScope') : tr('selectImportScope');
            },
            scopeDialogConfirmLabel() {
                return this.scopeDialog.mode === 'move' ? tr('moveToScope') : tr('import');
            },
            resetScopeDialog() {
                this.scopeDialog = emptyScopeDialog();
            },
            closeScopeDialog() {
                const dialog = this.$refs.scopeDialog;
                if (typeof HTMLDialogElement !== 'undefined' && dialog instanceof HTMLDialogElement && dialog.open) {
                    dialog.close();
                    return;
                }
                this.resetScopeDialog();
            },
            async openImportScopeDialog() {
                try {
                    const target = this.availableScopeSections[0];
                    if (!target) {
                        throw new Error(tr('skillScopeUnavailable'));
                    }
                    this.scopeDialog = {
                        ...emptyScopeDialog(),
                        mode: 'import',
                        selectedSectionId: target.id,
                    };
                    await this.showDialog('scopeDialog', 'skillScopeDialogUnsupported');
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            async openMoveScopeDialog(section, skill) {
                try {
                    const targets = this.availableScopeSections.filter((target) => target.id !== section.id);
                    if (targets.length === 0) {
                        throw new Error(tr('skillMoveTargetUnavailable'));
                    }
                    this.scopeDialog = {
                        ...emptyScopeDialog(),
                        mode: 'move',
                        selectedSectionId: targets[0].id,
                        sourceSectionId: section.id,
                        skill,
                    };
                    await this.showDialog('scopeDialog', 'skillScopeDialogUnsupported');
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            async confirmScopeDialog() {
                const request = { ...this.scopeDialog };
                const target = this.findSection(request.selectedSectionId);
                if (!target?.available) {
                    const error = new Error(tr('skillScopeNotFound', { id: request.selectedSectionId }));
                    this.reportError(error);
                    throw error;
                }

                this.closeScopeDialog();
                if (request.mode === 'import') {
                    await this.pickAndPreviewImport(target);
                    return;
                }

                const source = this.findSection(request.sourceSectionId);
                if (!source?.available || !request.skill) {
                    const error = new Error(tr('skillScopeNotFound', { id: request.sourceSectionId }));
                    this.reportError(error);
                    throw error;
                }
                await this.moveSkill(source, request.skill, target);
            },
            setImportDraft(patch) {
                this.importDraft = {
                    ...this.importDraft,
                    ...patch,
                };
            },
            importPreviewSkill() {
                return this.importDraft.preview?.skill || null;
            },
            importWarnings() {
                const warnings = this.importDraft.preview?.warnings;
                return Array.isArray(warnings) ? warnings : [];
            },
            importHasConflict() {
                return this.importDraft.preview?.conflict?.kind === 'different';
            },
            importConflictText() {
                const kind = this.importDraft.preview?.conflict?.kind;
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
            async clearImportDraft() {
                if (this.importDraft.input) {
                    await requireSkillApi().discardPickedImport(this.importDraft.input);
                }
                this.importDraft = emptyImportDraft();
            },
            async pickAndPreviewImport(section) {
                if (!section.available) {
                    throw new Error(this.sectionUnavailableText(section));
                }
                try {
                    await this.clearImportDraft();
                    const input = await requireSkillApi().pickImportArchive();
                    if (!input) {
                        return;
                    }
                    this.importDraft = {
                        ...emptyImportDraft(),
                        input,
                        loading: true,
                        sectionId: section.id,
                    };
                    const preview = await requireSkillApi().previewImport({
                        input,
                        targetScope: section.scope,
                    });
                    this.setImportDraft({ preview, loading: false });
                } catch (error) {
                    this.importDraft = emptyImportDraft();
                    this.reportError(error);
                    throw error;
                }
            },
            async installImport() {
                const section = this.currentImportSection;
                const draft = this.importDraft;
                if (!section?.available) {
                    throw new Error(tr('skillScopeNotFound', { id: draft.sectionId }));
                }
                if (!draft.input || !draft.preview) {
                    throw new Error(tr('previewSkillImportFirst'));
                }

                try {
                    const request = {
                        input: draft.input,
                        targetScope: section.scope,
                    };
                    if (this.importHasConflict()) {
                        request.conflictStrategy = draft.conflictStrategy;
                    }
                    const result = await requireSkillApi().installImport(request);
                    await syncSkillInstallPortability(result);
                    this.importDraft = emptyImportDraft();
                    await this.refreshSection(section.id);
                    this.toast(tr('skillInstallToast', {
                        action: translateSkillInstallAction(result.action),
                        name: result.name,
                    }));
                } catch (error) {
                    this.importDraft = emptyImportDraft();
                    this.reportError(error);
                    throw error;
                }
            },
            async openSkillPreview(section, skill) {
                if (!section.available) {
                    throw new Error(this.sectionUnavailableText(section));
                }
                const requestId = ++this.previewRequestId;
                this.preview = {
                    requestId,
                    sectionId: section.id,
                    scope: section.scope,
                    scopeLabel: skillScopeLabel(section.scope),
                    skill,
                    files: [],
                    loading: true,
                    expandedFolders: {},
                };
                try {
                    await this.showDialog('previewDialog', 'skillPreviewDialogUnsupported');
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
                await this.loadPreviewFiles(requestId);
            },
            async loadPreviewFiles(requestId = this.preview?.requestId) {
                if (!this.preview) {
                    return;
                }
                const current = this.preview;
                current.loading = true;
                try {
                    const files = await requireSkillApi().listFiles({
                        scope: current.scope,
                        name: current.skill.name,
                    });
                    if (!this.preview || this.preview.requestId !== requestId) {
                        return;
                    }
                    this.preview.files = files;
                } catch (error) {
                    if (this.preview?.requestId === requestId) {
                        this.reportError(error);
                    }
                    throw error;
                } finally {
                    if (this.preview?.requestId === requestId) {
                        this.preview.loading = false;
                    }
                }
            },
            closePreview() {
                this.closeFileViewer();
                const dialog = this.$refs.previewDialog;
                if (typeof HTMLDialogElement !== 'undefined' && dialog instanceof HTMLDialogElement && dialog.open) {
                    dialog.close();
                    return;
                }
                this.preview = null;
            },
            onPreviewClosed() {
                this.closeFileViewer();
                this.preview = null;
            },
            onPreviewCancel() {
                if (this.fileViewer) {
                    this.closeFileViewer();
                    return;
                }
                this.closePreview();
            },
            previewFolderKey(node) {
                return `${this.preview?.scopeLabel || ''}:${this.preview?.skill?.name || ''}:${node.path}`;
            },
            isPreviewFolderOpen(node) {
                return this.preview?.expandedFolders?.[this.previewFolderKey(node)] === true;
            },
            togglePreviewFolder(node) {
                if (!this.preview) {
                    return;
                }
                const key = this.previewFolderKey(node);
                this.preview.expandedFolders = {
                    ...this.preview.expandedFolders,
                    [key]: !this.preview.expandedFolders[key],
                };
            },
            async openPreviewFile(node) {
                if (!this.preview) {
                    throw new Error(tr('selectSkillFirst'));
                }
                if (node.file.kind === 'binary') {
                    const error = new Error(tr('cannotDisplayBinarySkillFile', { path: node.path }));
                    this.reportError(error);
                    throw error;
                }

                const viewerId = this.fileViewerRequestId + 1;
                this.fileViewerRequestId = viewerId;
                try {
                    const preview = this.preview;
                    const result = await requireSkillApi().readFile({
                        scope: preview.scope,
                        name: preview.skill.name,
                        path: node.path,
                        maxChars: SKILL_FILE_VIEW_MAX_CHARS,
                    });
                    if (!this.preview || this.preview.requestId !== preview.requestId || this.fileViewerRequestId !== viewerId) {
                        return;
                    }
                    this.fileViewer = {
                        viewerId,
                        file: {
                            ...result,
                            onSave: async ({ content }) => {
                                const saved = await this.saveSkillFile(preview, result, content);
                                await Promise.all([
                                    this.refreshSection(preview.sectionId),
                                    this.loadPreviewFiles(preview.requestId),
                                ]);
                                return saved;
                            },
                        },
                    };
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            closeFileViewer() {
                this.fileViewerRequestId += 1;
                this.fileViewer = null;
            },
            async saveSkillFile(preview, file, content) {
                const api = requireSkillApi();
                if (typeof api.writeFile !== 'function') {
                    throw new Error(tr('hostSkillWriteApiUnavailable'));
                }
                const result = await api.writeFile({
                    scope: preview.scope,
                    name: preview.skill.name,
                    path: file.path,
                    content,
                    expectedSha256: file.sha256,
                });
                await syncSkillWritePortability({
                    scope: preview.scope,
                    name: preview.skill.name,
                });
                return result;
            },
            async moveSkill(section, skill, target) {
                if (!target?.available) {
                    throw new Error(tr('skillScopeNotFound', { id: target?.id || '' }));
                }

                const targetSkill = target.skills.find((item) => item.name === skill.name);
                const request = {
                    name: skill.name,
                    fromScope: section.scope,
                    toScope: target.scope,
                };
                if (targetSkill && targetSkill.installedHash !== skill.installedHash) {
                    const confirmed = await confirmAction(tr('replaceSkillOnMoveConfirm', {
                        name: skill.name,
                        scope: skillScopeLabel(target.scope),
                    }));
                    if (!confirmed) {
                        return;
                    }
                    request.conflictStrategy = 'replace';
                }

                try {
                    const result = await requireSkillApi().move(request);
                    await syncSkillMovePortability(request, result);
                    await Promise.all([
                        this.refreshSection(section.id),
                        this.refreshSection(target.id),
                    ]);
                    if (this.preview?.sectionId === section.id && this.preview.skill.name === skill.name) {
                        this.closePreview();
                    }
                    this.toast(tr('skillMovedToast', {
                        action: translateSkillInstallAction(result.action),
                        name: result.name,
                        scope: skillScopeLabel(target.scope),
                    }));
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            async exportSkill(section, skill) {
                try {
                    const payload = await requireSkillApi().export({
                        scope: section.scope,
                        name: skill.name,
                    });
                    const blob = base64ToBlob(payload.contentBase64, SKILL_ARCHIVE_CONTENT_TYPE);
                    const downloadResult = await downloadBlobWithRuntime(blob, payload.fileName, {
                        fallbackName: `${skill.name}.zip`,
                    });
                    if (downloadResult?.mode !== 'ios-native-share' || downloadResult.completed === true) {
                        this.toast(tr('exportedSkill', { name: skill.name }));
                    }
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
            async deleteSkill(section, skill) {
                const confirmed = await confirmAction(tr('deleteScopedSkillConfirm', {
                    name: skill.name,
                    scope: skillScopeLabel(section.scope),
                }));
                if (!confirmed) {
                    return;
                }

                try {
                    await requireSkillApi().delete({
                        scope: section.scope,
                        name: skill.name,
                    });
                    await syncSkillDeletePortability({
                        scope: section.scope,
                        name: skill.name,
                    });
                    if (this.preview?.sectionId === section.id && this.preview.skill.name === skill.name) {
                        this.closePreview();
                    }
                    await this.refreshSection(section.id);
                    this.toast(tr('deletedSkill', { name: skill.name }));
                } catch (error) {
                    this.reportError(error);
                    throw error;
                }
            },
        },
        template: `
            <div class="ttas-root ttas-panel-root ttas-skill-manager-root ttas-skill-manager-inline">
                <div v-if="loading && !initialized" class="ttas-loading">{{ tr('loadingSkillExtension') }}</div>
                <div v-else class="ttas-panel-body ttas-skill-manager-body">
                    <div v-if="error" class="ttas-error">
                        <i class="fa-solid fa-triangle-exclamation"></i>
                        <pre>{{ error }}</pre>
                    </div>

                    <div class="ttas-skill-manager-tools">
                        <div class="ttas-skill-toolbar-row">
                            <label class="ttas-field">
                                <span>{{ tr('selectProfile') }}</span>
                                <select :value="selectedProfileId" @change="selectProfile($event.target.value)">
                                    <option v-for="profile in profiles" :key="profile.id" :value="profile.id">{{ profile.displayName || profile.id }}</option>
                                </select>
                            </label>
                            <div class="ttas-skill-toolbar-actions">
                                <button type="button" class="menu_button menu_button_icon ttas-primary-button" :disabled="importDraft.loading" :title="tr('import')" :aria-label="tr('import')" @click="openImportScopeDialog">
                                    <i class="fa-solid fa-file-import"></i>
                                </button>
                                <button type="button" class="menu_button menu_button_icon" :title="tr('refresh')" :aria-label="tr('refresh')" @click="refreshAll">
                                    <i class="fa-solid fa-rotate"></i>
                                </button>
                            </div>
                        </div>
                        <label class="ttas-skill-search">
                            <i class="fa-solid fa-magnifying-glass"></i>
                            <input v-model="searchQuery" class="text_pole" type="search" :placeholder="tr('searchSkills')" />
                        </label>
                    </div>

                    <div v-if="importDraft.loading || importDraft.preview" class="ttas-skill-import-inline ttas-skill-import-global">
                        <div class="ttas-skill-import-inline-main">
                            <i class="fa-solid" :class="importDraft.loading ? 'fa-spinner fa-spin' : 'fa-box-archive'"></i>
                            <div>
                                <strong v-if="importDraft.preview">{{ importPreviewSkill().displayName || importPreviewSkill().name }}</strong>
                                <strong v-else>{{ tr('importSkillArchive') }}</strong>
                                <small v-if="currentImportSection">{{ tr('importTargetScope') }}: {{ tr(currentImportSection.labelKey) }} / {{ importConflictText() || tr('loadingSkillFiles') }}</small>
                            </div>
                        </div>
                        <select
                            v-if="importDraft.preview && importHasConflict()"
                            :value="importDraft.conflictStrategy"
                            @change="setImportDraft({ conflictStrategy: $event.target.value })"
                        >
                            <option value="skip">{{ tr('skipConflict') }}</option>
                            <option value="replace">{{ tr('replaceConflict') }}</option>
                        </select>
                        <button v-if="importDraft.preview" type="button" class="menu_button menu_button_icon ttas-primary-button" @click="installImport">
                            <i class="fa-solid fa-check"></i>
                            <span>{{ tr('install') }}</span>
                        </button>
                        <button type="button" class="menu_button menu_button_icon" @click="clearImportDraft">
                            <i class="fa-solid fa-xmark"></i>
                            <span>{{ tr('cancel') }}</span>
                        </button>
                        <ul v-if="importWarnings().length > 0" class="ttas-skill-import-warnings">
                            <li v-for="warning in importWarnings()" :key="warning">{{ warning }}</li>
                        </ul>
                    </div>

                    <div class="ttas-skill-section-list">
                        <section
                            v-for="section in sections"
                            :key="section.id"
                            class="ttas-skill-section"
                            :class="{ 'is-unavailable': !section.available }"
                        >
                            <header class="ttas-skill-section-head">
                                <div class="ttas-skill-section-title">
                                    <i class="fa-solid" :class="section.icon"></i>
                                    <div>
                                        <h4>{{ tr(section.labelKey) }}</h4>
                                        <small>{{ section.subtitle }}</small>
                                    </div>
                                </div>
                                <span class="ttas-skill-count-pill">{{ sectionCountLabel(section) }}</span>
                            </header>

                            <div v-if="!section.available" class="ttas-skill-empty">
                                <i class="fa-solid fa-circle-info"></i>
                                <span>{{ sectionUnavailableText(section) }}</span>
                            </div>

                            <div v-else class="ttas-skill-section-body">
                                <div v-if="section.loading" class="ttas-file-loading ttas-skill-section-loading">
                                    <span>{{ tr('loadingSkillFiles') }}</span>
                                    <div class="ttas-file-loading-lines"><i></i><i></i><i></i></div>
                                </div>

                                <div v-else-if="section.skills.length === 0" class="ttas-skill-empty">
                                    <i class="fa-solid fa-inbox"></i>
                                    <span>{{ tr('noSkillsInstalled') }}</span>
                                </div>

                                <div v-else-if="visibleSkills(section).length === 0" class="ttas-skill-empty">
                                    <i class="fa-solid fa-magnifying-glass"></i>
                                    <span>{{ tr('noSkillsMatch') }}</span>
                                </div>

                                <ul v-else class="ttas-skill-list">
                                    <li v-for="skill in visibleSkills(section)" :key="skillRowKey(section, skill)" class="ttas-skill-list-item">
                                        <article class="ttas-skill-row">
                                            <div class="ttas-skill-row-main">
                                                <i class="fa-solid fa-book-open"></i>
                                                <span class="ttas-skill-row-copy">
                                                    <strong>{{ skillTitle(skill) }}</strong>
                                                    <span>{{ skillSubtitle(skill) }}</span>
                                                </span>
                                            </div>
                                            <div class="ttas-skill-row-actions">
                                                <button type="button" class="menu_button menu_button_icon" :title="tr('viewSkill')" :aria-label="tr('viewSkill')" @click="openSkillPreview(section, skill)">
                                                    <i class="fa-solid fa-eye"></i>
                                                </button>
                                                <button type="button" class="menu_button menu_button_icon" :title="tr('moveToScope')" :aria-label="tr('moveToScope')" @click="openMoveScopeDialog(section, skill)">
                                                    <i class="fa-solid fa-arrow-right-arrow-left"></i>
                                                </button>
                                                <button type="button" class="menu_button menu_button_icon" :title="tr('export')" :aria-label="tr('export')" @click="exportSkill(section, skill)">
                                                    <i class="fa-solid fa-file-export"></i>
                                                </button>
                                                <button type="button" class="menu_button menu_button_icon ttas-danger-button" :title="tr('delete')" :aria-label="tr('delete')" @click="deleteSkill(section, skill)">
                                                    <i class="fa-solid fa-trash-can"></i>
                                                </button>
                                            </div>
                                        </article>
                                    </li>
                                </ul>
                            </div>
                        </section>
                    </div>
                </div>

                <dialog ref="scopeDialog" class="ttas-scope-dialog" data-tt-mobile-surface="fullscreen-window" @cancel.prevent="closeScopeDialog" @close="resetScopeDialog">
                    <div class="ttas-root ttas-scope-picker">
                        <header class="ttas-scope-picker-head">
                            <div>
                                <strong>{{ scopeDialogTitle() }}</strong>
                                <small v-if="scopeDialog.mode === 'move' && scopeDialog.skill">{{ skillTitle(scopeDialog.skill) }}</small>
                            </div>
                            <button type="button" class="menu_button menu_button_icon ttas-close-button" :title="tr('close')" :aria-label="tr('close')" @click="closeScopeDialog">
                                <i class="fa-solid fa-xmark"></i>
                            </button>
                        </header>
                        <div class="ttas-scope-list">
                            <label
                                v-for="target in scopeDialogTargets"
                                :key="target.id"
                                class="ttas-scope-option"
                                :class="{ active: scopeDialog.selectedSectionId === target.id }"
                            >
                                <input v-model="scopeDialog.selectedSectionId" type="radio" :value="target.id" />
                                <i class="fa-solid" :class="target.icon"></i>
                                <span>
                                    <strong>{{ tr(target.labelKey) }}</strong>
                                    <small>{{ target.subtitle }}</small>
                                </span>
                            </label>
                        </div>
                        <footer class="ttas-scope-picker-actions">
                            <button type="button" class="menu_button menu_button_icon" @click="closeScopeDialog">
                                <i class="fa-solid fa-xmark"></i>
                                <span>{{ tr('cancel') }}</span>
                            </button>
                            <button type="button" class="menu_button menu_button_icon ttas-primary-button" :disabled="!scopeDialog.selectedSectionId" @click="confirmScopeDialog">
                                <i class="fa-solid fa-check"></i>
                                <span>{{ scopeDialogConfirmLabel() }}</span>
                            </button>
                        </footer>
                    </div>
                </dialog>

                <dialog ref="previewDialog" class="ttas-file-dialog ttas-skill-preview-dialog" data-tt-mobile-surface="fullscreen-window" @cancel.prevent="onPreviewCancel" @close="onPreviewClosed">
                    <div v-if="preview" class="ttas-root ttas-file-viewer ttas-skill-preview-viewer">
                        <header class="ttas-titlebar ttas-file-viewer-titlebar">
                            <div>
                                <div class="ttas-eyebrow">{{ preview.scopeLabel }}</div>
                                <h3>{{ skillTitle(preview.skill) }}</h3>
                            </div>
                            <button type="button" class="menu_button menu_button_icon ttas-close-button" :title="tr('close')" :aria-label="tr('close')" @click="closePreview">
                                <i class="fa-solid fa-xmark"></i>
                            </button>
                        </header>
                        <div class="ttas-file-viewport" :class="{ loading: preview.loading }">
                            <div v-if="preview.loading" class="ttas-file-loading">
                                <span>{{ tr('loadingSkillFiles') }}</span>
                                <div class="ttas-file-loading-lines"><i></i><i></i><i></i></div>
                            </div>
                            <div v-else-if="previewFileTree.length === 0" class="ttas-empty ttas-file-empty">{{ tr('noFilesFoundForSkill') }}</div>
                            <ul v-else class="ttas-file-tree ttas-file-tree-root">
                                <SkillFileTreeNode
                                    v-for="node in previewFileTree"
                                    :key="node.path"
                                    :node="node"
                                    :depth="0"
                                    :is-folder-open="isPreviewFolderOpen"
                                    @toggle-folder="togglePreviewFolder"
                                    @open-file="openPreviewFile"
                                />
                            </ul>
                        </div>
                        <div v-if="fileViewer" class="ttas-file-overlay" @mousedown.self="closeFileViewer">
                            <div class="ttas-file-overlay-panel">
                                <SkillFileViewer
                                    :key="fileViewer.viewerId"
                                    :file="fileViewer.file"
                                    @close="closeFileViewer"
                                />
                            </div>
                        </div>
                    </div>
                </dialog>
            </div>
        `,
    };
}
