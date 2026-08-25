import { DEFAULT_PROFILE_ID } from '../constants';
import { skillScopeLabel } from '../skill-scope';
import { installSkillImports, manualSkillImportInput, previewSkillImports, skillImportItemLabel } from './SkillImportOperation';
import {
    deleteSkillMutation,
    exportSkillArchive,
    moveSkillMutation,
    writeSkillFile,
} from './SkillManagerMutations';
import { createSkillManagerSections } from './SkillManagerSections';
import type {
    SkillImportDraft,
    SkillImportItem,
    SkillManagerController,
    SkillManagerDeps,
    SkillManagerSnapshot,
    SkillPreview,
    SkillSection,
    SkillSectionId,
} from './SkillManagerContract';
import { SKILL_HOST_EVENT_KEYS, emptySkillImportDraft } from './SkillManagerContract';
export function createSkillManagerController(deps: SkillManagerDeps): SkillManagerController {
    let sequence = 0;
    let snapshot: SkillManagerSnapshot = {
        initialized: false,
        loading: false,
        error: '',
        profiles: [],
        selectedProfileId: DEFAULT_PROFILE_ID,
        sections: [],
        importDraft: emptySkillImportDraft(sequence),
        scopeDialog: { mode: '' },
        sourceDialog: { mode: '' },
        searchQuery: '',
        preview: null,
        fileViewer: null,
        supportsDirectoryImport: deps.supportsDirectoryImport,
    };
    const listeners = new Set<() => void>();
    const unsubscribes: Array<() => void> = [];
    let previewFilesEpoch = 0;
    let fileViewerEpoch = 0;
    let disposed = false;
    let initPromise: Promise<void> | null = null;

    function commit(patch: Partial<SkillManagerSnapshot>): void {
        if (disposed) return;
        snapshot = { ...snapshot, ...patch };
        listeners.forEach(listener => listener());
    }

    function report(error: unknown): void {
        if (disposed) return;
        commit({ error: deps.errorText(error) });
        deps.reportError(error);
    }

    async function execute<T>(operation: () => Promise<T>): Promise<T> {
        try {
            return await operation();
        } catch (error) {
            if (snapshot.error !== deps.errorText(error)) report(error);
            throw error;
        }
    }

    function fire(operation: () => Promise<unknown>): void {
        void execute(operation).catch(error => queueMicrotask(() => { throw error; }));
    }

    async function refreshChanged(ids: readonly SkillSectionId[]): Promise<void> {
        const changed = [...new Set(ids)];
        if (changed.length === 0) return;
        if (snapshot.preview && changed.includes(snapshot.preview.sectionId)) closePreview();
        if (snapshot.importDraft.sectionId && changed.includes(snapshot.importDraft.sectionId)) {
            try {
                await finishDraft();
            } catch (error) {
                report(error);
            }
        }
        if (snapshot.scopeDialog.mode) commit({ scopeDialog: { mode: '' } });
        if (snapshot.sourceDialog.mode) commit({ sourceDialog: { mode: '' } });
        await Promise.all(changed.map(refreshSection));
    }

    async function refreshCommittedSections(ids: readonly SkillSectionId[]): Promise<void> {
        await Promise.allSettled([...new Set(ids)].map(id => execute(() => refreshSection(id))));
    }

    async function refreshProfiles(): Promise<void> {
        const profiles = await deps.listProfiles();
        if (disposed) return;
        const selectedProfileId = profiles.some(profile => profile.id === snapshot.selectedProfileId)
            ? snapshot.selectedProfileId
            : profiles[0]?.id ?? DEFAULT_PROFILE_ID;
        commit({ profiles, selectedProfileId });
    }

    async function syncHostScopes(): Promise<void> {
        if (!disposed && snapshot.initialized) await refreshChanged(rebuildSections());
    }

    async function syncSelectedProfile(settings: { editingProfileId?: string }): Promise<void> {
        if (disposed || !snapshot.initialized) return;
        const next = String(settings.editingProfileId || DEFAULT_PROFILE_ID).trim() || DEFAULT_PROFILE_ID;
        if (next === snapshot.selectedProfileId) return;
        commit({ selectedProfileId: next });
        await refreshProfiles();
        if (disposed) return;
        await refreshChanged([...rebuildSections(), 'profile']);
    }

    async function syncProfiles(): Promise<void> {
        if (disposed || !snapshot.initialized) return;
        await refreshProfiles();
        if (disposed) return;
        await refreshChanged([...rebuildSections(), 'profile']);
    }

    function subscribeExternalEvents(): void {
        const context = deps.getHostContext();
        const { eventSource, eventTypes } = context;
        if (!eventSource || !eventTypes) throw new Error(deps.tr('sillyTavernContextUnavailable'));
        const pending: Array<() => void> = [];
        const onScopeChanged = () => fire(syncHostScopes);
        try {
            for (const key of SKILL_HOST_EVENT_KEYS) {
                const eventName = eventTypes[key];
                if (!eventName) throw new Error(`SillyTavern event type is unavailable: ${key}`);
                eventSource.on(eventName, onScopeChanged);
                pending.push(() => eventSource.removeListener(eventName, onScopeChanged));
            }
            pending.push(deps.subscribeSettings(settings => fire(() => syncSelectedProfile(settings))));
            pending.push(deps.subscribeProfilesChanged(() => fire(syncProfiles)));
            unsubscribes.push(...pending);
        } catch (error) {
            pending.reverse().forEach(unsubscribe => unsubscribe());
            throw error;
        }
    }

    async function init(): Promise<void> {
        if (initPromise !== null) return initPromise;
        const initialization = execute(async () => {
            commit({ loading: true, error: '' });
            try {
                const settings = await deps.loadSettings();
                if (disposed) return;
                commit({ selectedProfileId: String(settings.editingProfileId || DEFAULT_PROFILE_ID) });
                await refreshProfiles();
                rebuildSections();
                await Promise.all(snapshot.sections.map(item => refreshSection(item.id)));
                if (disposed) return;
                subscribeExternalEvents();
                commit({ initialized: true });
                await syncHostScopes();
            } catch (error) {
                unsubscribes.splice(0).reverse().forEach(unsubscribe => unsubscribe());
                commit({ initialized: false });
                throw error;
            } finally {
                commit({ loading: false });
            }
        });
        initPromise = initialization;
        try {
            await initialization;
        } catch (error) {
            if (initPromise === initialization) initPromise = null;
            throw error;
        }
    }

    async function clearDraft(expectedId = snapshot.importDraft.id): Promise<void> {
        if (snapshot.importDraft.id !== expectedId) return;
        const hasItems = snapshot.importDraft.items.length > 0;
        if (hasItems) await deps.getSkillApi().discardPickedImport();
        if (snapshot.importDraft.id === expectedId) commit({ importDraft: emptySkillImportDraft(++sequence) });
    }

    async function finishDraft(expectedId = snapshot.importDraft.id): Promise<void> {
        if (snapshot.importDraft.id !== expectedId) return;
        const hasItems = snapshot.importDraft.items.length > 0;
        commit({ importDraft: emptySkillImportDraft(++sequence) });
        if (hasItems) await deps.getSkillApi().discardPickedImport();
    }

    function patchImportItem(draftId: number, index: number, patch: Partial<SkillImportItem>): void {
        if (snapshot.importDraft.id !== draftId) return;
        commit({ importDraft: {
            ...snapshot.importDraft,
            items: snapshot.importDraft.items.map((item, itemIndex) => itemIndex === index ? { ...item, ...patch } : item),
        } });
    }

    async function previewInputs(target: SkillSection, inputs: readonly TauriTavernSkillImportInput[]): Promise<void> {
        if (!target.scope) throw new Error(deps.tr('skillScopeNotFound', { id: target.id }));
        const draft: SkillImportDraft = {
            id: ++sequence,
            sectionId: target.id,
            installing: false,
            items: inputs.map(input => ({ input, preview: null, error: '', conflictStrategy: 'skip' })),
        };
        commit({ importDraft: draft });
        try {
            await previewSkillImports({
                items: draft.items,
                targetScope: target.scope,
                preview: request => deps.getSkillApi().previewImport(request),
                isActive: () => !disposed && snapshot.importDraft.id === draft.id,
                onPreview: (index, preview) => patchImportItem(draft.id, index, { preview }),
                onError: (index, error) => {
                    patchImportItem(draft.id, index, { error: deps.errorText(error) });
                    deps.logError('Failed to preview Skill import', error);
                },
            });
        } catch (error) {
            try {
                await finishDraft(draft.id);
            } catch (cleanupError) {
                report(cleanupError);
            }
            throw error;
        }
    }

    async function pickAndPreview(target: SkillSection, kind: 'archive' | 'directory'): Promise<void> {
        const api = deps.getSkillApi();
        const inputs = kind === 'directory' ? await api.pickImportDirectories() : await api.pickImportArchives();
        if (inputs) await previewInputs(target, inputs);
    }

    async function confirmSource(): Promise<void> {
        const request = snapshot.sourceDialog;
        if (!request.mode) return;
        const target = availableSection(request.sectionId);
        commit({ sourceDialog: { ...request, loading: true } });
        try {
            const input = request.mode === 'download'
                ? await deps.getSkillApi().downloadImport({ url: request.url })
                : manualSkillImportInput(request.content, deps.tr);
            if (snapshot.sourceDialog.mode && snapshot.sourceDialog.id === request.id) {
                commit({ sourceDialog: { mode: '' } });
                await previewInputs(target, [input]);
            }
        } catch (error) {
            if (snapshot.sourceDialog.mode && snapshot.sourceDialog.id === request.id) {
                commit({ sourceDialog: { ...request, loading: false } });
            }
            throw error;
        }
    }

    async function confirmScope(): Promise<void> {
        const request = snapshot.scopeDialog;
        if (!request.mode) return;
        const target = availableSection(request.selectedSectionId);
        if (request.mode === 'import') {
            await clearDraft();
            commit({ scopeDialog: { mode: '' } });
            if (request.importKind === 'manual' || request.importKind === 'download') {
                commit({ sourceDialog: { id: ++sequence, mode: request.importKind, sectionId: target.id, content: '', url: '', loading: false } });
            } else {
                await pickAndPreview(target, request.importKind);
            }
            return;
        }
        commit({ scopeDialog: { mode: '' } });
        await moveSkill(availableSection(request.sourceSectionId), request.skill, target);
    }

    async function installDraft(): Promise<void> {
        const draft = snapshot.importDraft;
        const target = availableSection(draft.sectionId);
        const items = draft.items.filter(item => item.preview && !item.error);
        if (items.length === 0) throw new Error(deps.tr('previewSkillImportFirst'));
        commit({ importDraft: { ...draft, installing: true } });
        let results: TauriTavernSkillInstallResult[];
        let hostCommitted = false;
        try {
            results = await installSkillImports({
                items,
                targetScope: target.scope,
                install: request => deps.getSkillApi().installImport(request),
                onInstalled: () => { hostCommitted = true; },
                syncPortability: deps.syncInstallPortability,
                onError: (item, error) => {
                    deps.logError('Failed to install Skill import', error);
                    deps.toastError(deps.tr('skillImportItemFailed', { name: skillImportItemLabel(item, deps.tr), error: deps.errorText(error) }));
                },
            });
        } catch (error) {
            if (!hostCommitted && snapshot.importDraft.id === draft.id) commit({ importDraft: { ...draft, installing: false } });
            if (!hostCommitted) throw error;
            try {
                await finishDraft(draft.id);
            } catch (cleanupError) {
                report(cleanupError);
            }
            await refreshCommittedSections([target.id]);
            throw error;
        }
        let cleanupError: unknown = null;
        try {
            await finishDraft(draft.id);
        } catch (error) {
            cleanupError = error;
        }
        await refreshCommittedSections([target.id]);
        if (draft.items.length === 1) {
            const result = results[0];
            if (!result) throw new Error('Skill install returned no result');
            deps.toastSuccess(deps.tr('skillInstallToast', { action: deps.translateInstallAction(result.action), name: result.name }));
        } else {
            if (results.length) deps.toastSuccess(deps.tr('skillBatchInstallToast', { count: results.length, total: draft.items.length }));
            const failures = draft.items.length - results.length;
            if (failures) deps.toastError(deps.tr('skillBatchInstallFailed', { count: failures }));
        }
        if (cleanupError) throw cleanupError;
    }

    async function loadPreviewFiles(previewId: number): Promise<void> {
        const current = snapshot.preview;
        if (!current || current.id !== previewId) return;
        const epoch = ++previewFilesEpoch;
        commit({ preview: { ...current, loading: true } });
        try {
            const files = await deps.getSkillApi().listFiles({ scope: current.scope, name: current.skill.name });
            if (!disposed && snapshot.preview?.id === previewId && previewFilesEpoch === epoch) commit({ preview: { ...snapshot.preview, files } });
        } catch (error) {
            if (!disposed && snapshot.preview?.id === previewId && previewFilesEpoch === epoch) throw error;
        } finally {
            if (!disposed && snapshot.preview?.id === previewId && previewFilesEpoch === epoch) commit({ preview: { ...snapshot.preview, loading: false } });
        }
    }

    function closeFileViewer(): void {
        fileViewerEpoch += 1;
        commit({ fileViewer: null });
    }

    function closePreview(): void {
        previewFilesEpoch += 1;
        closeFileViewer();
        commit({ preview: null });
    }

    const { availableSection, rebuildSections, refreshSection } = createSkillManagerSections({
        deps,
        getSnapshot: () => snapshot,
        commit,
        isDisposed: () => disposed,
        closePreview,
    });

    async function openFile(file: TauriTavernSkillFileRef): Promise<void> {
        const preview = snapshot.preview;
        if (!preview) throw new Error(deps.tr('selectSkillFirst'));
        if (file.kind === 'binary') throw new Error(deps.tr('cannotDisplayBinarySkillFile', { path: file.path }));
        const id = ++fileViewerEpoch;
        try {
            const result = await deps.getSkillApi().readFile({ scope: preview.scope, name: preview.skill.name, path: file.path });
            if (!disposed && snapshot.preview?.id === preview.id && fileViewerEpoch === id) commit({ fileViewer: { id, file: result } });
        } catch (error) {
            if (!disposed && snapshot.preview?.id === preview.id && fileViewerEpoch === id) throw error;
        }
    }

    async function saveOpenFile(content: string): Promise<TauriTavernSkillReadResult> {
        return execute(async () => {
            const preview = snapshot.preview;
            const viewer = snapshot.fileViewer;
            if (!preview || !viewer) throw new Error(deps.tr('selectSkillFirst'));
            const result = await writeSkillFile(deps, preview, viewer.file, content, {
                onCommitted: file => {
                    if (snapshot.fileViewer?.id === viewer.id) commit({ fileViewer: { ...viewer, file } });
                },
                reconcile: () => Promise.allSettled([
                    execute(() => refreshSection(preview.sectionId)),
                    execute(() => loadPreviewFiles(preview.id)),
                ]),
            });
            deps.toastSuccess(deps.tr('savedSkillFile', { path: result.path }));
            return result;
        });
    }

    async function moveSkill(source: SkillSection & { scope: TauriTavernSkillScope }, skill: TauriTavernSkillIndexEntry, target: SkillSection & { scope: TauriTavernSkillScope }): Promise<void> {
        const moved = await moveSkillMutation(deps, source, skill, target, {
            onCommitted: () => {
                if (snapshot.preview?.sectionId === source.id && snapshot.preview.skill.name === skill.name) closePreview();
            },
            reconcile: () => refreshCommittedSections([source.id, target.id]),
        });
        if (!moved) return;
        deps.toastSuccess(deps.tr('skillMovedToast', { action: deps.translateInstallAction(moved.action), name: moved.name, scope: skillScopeLabel(target.scope) }));
    }

    async function exportSkill(sectionId: SkillSectionId, skill: TauriTavernSkillIndexEntry): Promise<void> {
        const source = availableSection(sectionId);
        if (await exportSkillArchive(deps, source.scope, skill)) {
            deps.toastSuccess(deps.tr('exportedSkill', { name: skill.name }));
        }
    }

    async function deleteSkill(sectionId: SkillSectionId, skill: TauriTavernSkillIndexEntry): Promise<void> {
        const source = availableSection(sectionId);
        if (!await deleteSkillMutation(deps, source.scope, skill, {
            onCommitted: () => {
                if (snapshot.preview?.sectionId === source.id && snapshot.preview.skill.name === skill.name) closePreview();
            },
            reconcile: () => refreshCommittedSections([source.id]),
        })) return;
        deps.toastSuccess(deps.tr('deletedSkill', { name: skill.name }));
    }

    return {
        getSnapshot: () => snapshot,
        subscribe(listener) { listeners.add(listener); return () => listeners.delete(listener); },
        init,
        dispose() {
            if (disposed) return;
            const discard = snapshot.importDraft.items.length > 0;
            disposed = true;
            unsubscribes.splice(0).reverse().forEach(unsubscribe => unsubscribe());
            listeners.clear();
            if (discard) void deps.getSkillApi().discardPickedImport().catch(error => { deps.reportError(error); queueMicrotask(() => { throw error; }); });
        },
        setSearchQuery: value => commit({ searchQuery: value }),
        selectProfile(profileId) {
            fire(async () => {
                commit({ selectedProfileId: profileId.trim() || DEFAULT_PROFILE_ID });
                await refreshChanged([...rebuildSections(), 'profile']);
            });
        },
        refreshAll: () => fire(async () => {
            if (!snapshot.initialized) {
                await init();
                return;
            }
            commit({ error: '' });
            closePreview();
            await refreshProfiles();
            rebuildSections();
            await Promise.all(snapshot.sections.map(item => refreshSection(item.id)));
        }),
        openImportScopeDialog(kind) {
            const target = snapshot.sections.find(item => item.available);
            if (!target) { fire(() => Promise.reject(new Error(deps.tr('skillScopeUnavailable')))); return; }
            commit({ scopeDialog: { mode: 'import', importKind: kind, selectedSectionId: target.id } });
        },
        openMoveScopeDialog(sectionId, skill) {
            const target = snapshot.sections.find(item => item.available && item.id !== sectionId);
            if (!target) { fire(() => Promise.reject(new Error(deps.tr('skillMoveTargetUnavailable')))); return; }
            commit({ scopeDialog: { mode: 'move', sourceSectionId: sectionId, selectedSectionId: target.id, skill } });
        },
        setScopeDialogTarget(id) { if (snapshot.scopeDialog.mode) commit({ scopeDialog: { ...snapshot.scopeDialog, selectedSectionId: id } }); },
        setScopeImportKind(kind) { if (snapshot.scopeDialog.mode === 'import') commit({ scopeDialog: { ...snapshot.scopeDialog, importKind: kind } }); },
        closeScopeDialog: () => commit({ scopeDialog: { mode: '' } }),
        confirmScopeDialog: () => fire(confirmScope),
        setSourceContent(value) { if (snapshot.sourceDialog.mode) commit({ sourceDialog: { ...snapshot.sourceDialog, content: value } }); },
        setSourceUrl(value) { if (snapshot.sourceDialog.mode) commit({ sourceDialog: { ...snapshot.sourceDialog, url: value.trim() } }); },
        closeSourceDialog: () => commit({ sourceDialog: { mode: '' } }),
        confirmSourceDialog: () => fire(confirmSource),
        setImportConflict(index, strategy) { patchImportItem(snapshot.importDraft.id, index, { conflictStrategy: strategy }); },
        clearImportDraft: () => fire(clearDraft),
        installImports: () => execute(installDraft),
        openSkillPreview(sectionId, skill) {
            fire(async () => {
                const source = availableSection(sectionId);
                const preview: SkillPreview = { id: ++sequence, sectionId, scope: source.scope, scopeLabel: skillScopeLabel(source.scope), skill, files: [], loading: true, expandedFolders: {} };
                commit({ preview });
                await loadPreviewFiles(preview.id);
            });
        },
        closePreview,
        previewClosed: closePreview,
        previewCancelled() { if (snapshot.fileViewer) closeFileViewer(); else closePreview(); },
        togglePreviewFolder(path) {
            const preview = snapshot.preview;
            if (!preview) return;
            const key = `${preview.scopeLabel}:${preview.skill.name}:${path}`;
            commit({ preview: { ...preview, expandedFolders: { ...preview.expandedFolders, [key]: !preview.expandedFolders[key] } } });
        },
        openPreviewFile: file => fire(() => openFile(file)),
        closeFileViewer,
        saveOpenFile,
        exportSkill: (sectionId, skill) => fire(() => exportSkill(sectionId, skill)),
        deleteSkill: (sectionId, skill) => fire(() => deleteSkill(sectionId, skill)),
        dialogShowFailed(kind, error) {
            if (kind === 'scope') commit({ scopeDialog: { mode: '' } });
            if (kind === 'source') commit({ sourceDialog: { mode: '' } });
            if (kind === 'preview') closePreview();
            report(error);
            queueMicrotask(() => { throw error; });
        },
    };
}
