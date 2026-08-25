import { expect, test } from '@rstest/core';
import { waitFor } from '@testing-library/react';

import { createSkillManagerController } from './SkillManagerController';
import { SKILL_HOST_EVENT_KEYS, type SkillHostEventKey, type SkillManagerDeps } from './SkillManagerContract';
import { deleteSkillMutation } from './SkillManagerMutations';

const globalScope: TauriTavernSkillScope = { kind: 'global' };

function skill(name: string, scope: TauriTavernSkillScope = globalScope, hash = `${name}-hash`): TauriTavernSkillIndexEntry {
    return {
        scope,
        name,
        description: '',
        tags: [],
        installedHash: hash,
        fileCount: 1,
        totalBytes: 1,
        hasScripts: false,
        hasBinary: false,
        installedAt: '2026-01-01T00:00:00Z',
    };
}

function preview(name: string): TauriTavernSkillImportPreview {
    return { skill: skill(name), files: [], conflict: { kind: 'new' }, warnings: [], source: null };
}

function createSkillApi(overrides: Partial<TauriTavernSkillApi> = {}): TauriTavernSkillApi {
    const read: TauriTavernSkillReadResult = {
        name: 'SKILL.md', path: 'SKILL.md', content: 'body', chars: 4, words: 1,
        totalChars: 4, totalWords: 1, totalLines: 1, startLine: 1, endLine: 1,
        lineTruncated: false, bytes: 4, sha256: 'sha', truncated: false, resourceRef: 'skill://file',
    };
    return {
        list: () => Promise.resolve([]),
        listFiles: () => Promise.resolve([]),
        pickImportArchive: () => Promise.resolve(null),
        pickImportArchives: () => Promise.resolve(null),
        pickImportDirectories: () => Promise.resolve(null),
        discardPickedImport: () => Promise.resolve(),
        downloadImport: () => Promise.reject(new Error('not configured')),
        previewImport: () => Promise.reject(new Error('not configured')),
        installImport: () => Promise.reject(new Error('not configured')),
        readFile: () => Promise.resolve(read),
        writeFile: () => Promise.resolve(read),
        export: () => Promise.resolve({ fileName: 'skill.zip', contentBase64: '', sha256: 'sha' }),
        delete: () => Promise.resolve(),
        move: request => Promise.resolve({ scope: request.toScope, name: request.name, action: 'installed' }),
        retargetScope: () => Promise.resolve({}),
        ...overrides,
    };
}

function deferred<T>() {
    let resolver: ((value: T) => void) | undefined;
    const promise = new Promise<T>((resolve) => { resolver = resolve; });
    return { promise, resolve: (value: T) => resolver?.(value) };
}

function createWorld(api: TauriTavernSkillApi, profiles: TauriTavernAgentProfileSummary[] = [
    { id: 'default-writer', displayName: 'Writer', directRunnable: true },
]) {
    const hostListeners: Array<{ name: string; listener: () => void }> = [];
    const removedListeners: Array<{ name: string; listener: () => void }> = [];
    const eventTypes = Object.fromEntries([
        'CHAT_CHANGED', 'CHAT_LOADED', 'CHARACTER_EDITED', 'CHARACTER_DELETED', 'CHARACTER_RENAMED',
        'PRESET_CHANGED', 'PRESET_DELETED', 'PRESET_RENAMED', 'MAIN_API_CHANGED',
    ].map(key => [key, key.toLowerCase()])) as Record<SkillHostEventKey, string>;
    let presetName = 'Preset A';
    let contextReads = 0;
    let settingsListener: ((settings: { editingProfileId?: string }) => void) | null = null;
    let profilesListener: (() => void) | null = null;
    const state = {
        hostListeners,
        removedListeners,
        errors: [] as unknown[],
        successes: [] as string[],
        toastErrors: [] as string[],
        discards: 0,
        get contextReads() { return contextReads; },
        setPresetName(value: string) { presetName = value; },
        emitHost(index = 0) { hostListeners[index]?.listener(); },
        emitSettings(settings: { editingProfileId?: string }) { settingsListener?.(settings); },
        emitProfiles() { profilesListener?.(); },
    };
    const deps: SkillManagerDeps = {
        loadSettings: () => Promise.resolve({ editingProfileId: 'default-writer' }),
        subscribeSettings: (listener) => { settingsListener = listener; return () => { settingsListener = null; }; },
        listProfiles: () => Promise.resolve(profiles),
        subscribeProfilesChanged: (listener) => { profilesListener = listener; return () => { profilesListener = null; }; },
        getHostContext: () => {
            contextReads += 1;
            return {
                mainApi: 'openai',
                getPresetManager: () => ({ getSelectedPreset: () => presetName, getSelectedPresetName: () => presetName }),
                characterId: null,
                characters: [],
                eventTypes,
                eventSource: {
                    on: (name, listener) => hostListeners.push({ name, listener }),
                    removeListener: (name, listener) => removedListeners.push({ name, listener }),
                },
            };
        },
        getSkillApi: () => ({ ...api, discardPickedImport: async (input) => { state.discards += 1; await api.discardPickedImport(input); } }),
        confirmAction: () => Promise.resolve(true),
        downloadExport: () => Promise.resolve({ mode: 'browser' }),
        syncInstallPortability: () => Promise.resolve(),
        syncMovePortability: () => Promise.resolve(),
        syncWritePortability: () => Promise.resolve(),
        syncDeletePortability: () => Promise.resolve(),
        supportsDirectoryImport: true,
        errorText: error => error instanceof Error ? error.message : typeof error === 'string' ? error : 'Unknown error',
        reportError: error => state.errors.push(error),
        logError: () => undefined,
        toastSuccess: message => state.successes.push(message),
        toastError: message => state.toastErrors.push(message),
        translateInstallAction: action => action,
        tr: key => key,
    };
    return { deps, state };
}

test('subscribes to nine Host events, rereads context, and rejects stale same-scope refreshes', async () => {
    const pending: Array<ReturnType<typeof deferred<TauriTavernSkillIndexEntry[]>>> = [];
    let holdGlobal = false;
    const api = createSkillApi({
        list: ({ scope } = {}) => {
            if (holdGlobal && scope?.kind === 'global') {
                const request = deferred<TauriTavernSkillIndexEntry[]>();
                pending.push(request);
                return request.promise;
            }
            return Promise.resolve([]);
        },
    });
    const { deps, state } = createWorld(api);
    const controller = createSkillManagerController(deps);
    await controller.init();
    expect(state.hostListeners).toHaveLength(9);
    expect(state.hostListeners.map(item => item.name)).toEqual(SKILL_HOST_EVENT_KEYS.map(key => key.toLowerCase()));
    expect(new Set(state.hostListeners.map(item => item.listener)).size).toBe(1);

    const contextReads = state.contextReads;
    state.setPresetName('Preset B');
    state.emitHost();
    await waitFor(() => expect(controller.getSnapshot().sections.find(item => item.id === 'preset')?.subtitle).toBe('openai / Preset B'));
    expect(state.contextReads).toBeGreaterThan(contextReads);
    const profileEventReads = state.contextReads;
    state.emitProfiles();
    await waitFor(() => expect(state.contextReads).toBeGreaterThan(profileEventReads));

    holdGlobal = true;
    controller.refreshAll();
    controller.refreshAll();
    await waitFor(() => expect(pending).toHaveLength(2));
    pending[1]?.resolve([skill('new')]);
    await waitFor(() => expect(controller.getSnapshot().sections.find(item => item.id === 'global')?.skills[0]?.name).toBe('new'));
    pending[0]?.resolve([skill('old')]);
    await Promise.resolve();
    expect(controller.getSnapshot().sections.find(item => item.id === 'global')?.skills[0]?.name).toBe('new');

    controller.dispose();
    expect(state.removedListeners).toHaveLength(9);
    expect(state.removedListeners.map(item => item.listener)).toEqual(state.hostListeners.map(item => item.listener));
});

test('dispose blocks late initialization from installing subscriptions', async () => {
    const settings = deferred<{ editingProfileId?: string }>();
    const { deps, state } = createWorld(createSkillApi());
    deps.loadSettings = () => settings.promise;
    const controller = createSkillManagerController(deps);
    const initialization = controller.init();
    controller.dispose();
    settings.resolve({ editingProfileId: 'default-writer' });
    await initialization;
    expect(state.hostListeners).toHaveLength(0);
});

test('a failed final scope sync leaves initialization retryable and subscriptions truthful', async () => {
    let failPresetB = true;
    const api = createSkillApi({
        list: ({ scope } = {}) => scope?.kind === 'preset' && scope.name === 'Preset B' && failPresetB
            ? Promise.reject(new Error('preset scope unavailable'))
            : Promise.resolve([]),
    });
    const { deps, state } = createWorld(api);
    deps.subscribeSettings = () => {
        state.setPresetName('Preset B');
        return () => undefined;
    };
    const controller = createSkillManagerController(deps);

    await expect(controller.init()).rejects.toThrow('preset scope unavailable');
    expect(controller.getSnapshot().initialized).toBe(false);
    expect(state.removedListeners).toHaveLength(9);

    failPresetB = false;
    controller.refreshAll();
    await waitFor(() => expect(controller.getSnapshot().initialized).toBe(true));
    expect(controller.getSnapshot().error).toBe('');
    expect(state.hostListeners).toHaveLength(18);
});

test('supports archive, directory, manual, and download imports with staged-input cleanup', async () => {
    const archive = { kind: 'archiveFile' as const, path: '/tmp/archive.zip' };
    const directory = { kind: 'directory' as const, path: '/tmp/directory' };
    const seen: TauriTavernSkillImportInput[] = [];
    const downloads: string[] = [];
    const api = createSkillApi({
        pickImportArchives: () => Promise.resolve([archive]),
        pickImportDirectories: () => Promise.resolve([directory]),
        downloadImport: ({ url }) => { downloads.push(url); return Promise.resolve({ kind: 'inlineFiles', files: [{ path: 'SKILL.md', content: 'downloaded' }], source: { kind: 'url' } }); },
        previewImport: ({ input }) => { seen.push(input); return Promise.resolve(preview(`skill-${seen.length}`)); },
    });
    const { deps, state } = createWorld(api);
    const controller = createSkillManagerController(deps);
    await controller.init();

    controller.openImportScopeDialog('archive');
    controller.confirmScopeDialog();
    await waitFor(() => expect(seen).toHaveLength(1));
    controller.clearImportDraft();
    await waitFor(() => expect(state.discards).toBe(1));

    controller.openImportScopeDialog('archive');
    controller.setScopeImportKind('directory');
    controller.confirmScopeDialog();
    await waitFor(() => expect(seen).toHaveLength(2));
    controller.clearImportDraft();
    await waitFor(() => expect(state.discards).toBe(2));

    controller.openImportScopeDialog('manual');
    controller.confirmScopeDialog();
    await waitFor(() => expect(controller.getSnapshot().sourceDialog.mode).toBe('manual'));
    controller.setSourceContent('---\nname: manual\n---');
    controller.confirmSourceDialog();
    await waitFor(() => expect(seen).toHaveLength(3));
    controller.clearImportDraft();
    await waitFor(() => expect(state.discards).toBe(3));

    controller.openImportScopeDialog('download');
    controller.confirmScopeDialog();
    await waitFor(() => expect(controller.getSnapshot().sourceDialog.mode).toBe('download'));
    controller.setSourceUrl(' https://example.test/SKILL.md ');
    controller.confirmSourceDialog();
    await waitFor(() => expect(seen).toHaveLength(4));
    expect(downloads).toEqual(['https://example.test/SKILL.md']);
    expect(seen.map(input => input.kind)).toEqual(['archiveFile', 'directory', 'inlineFiles', 'inlineFiles']);

    controller.dispose();
    await waitFor(() => expect(state.discards).toBe(4));
});

test('installs a batch in order without letting one item failure stop later items', async () => {
    const inputs = ['one', 'bad', 'install-fail', 'last']
        .map(name => ({ kind: 'archiveFile' as const, path: `/tmp/${name}.zip` }));
    const installs: Parameters<TauriTavernSkillApi['installImport']>[0][] = [];
    const api = createSkillApi({
        pickImportArchives: () => Promise.resolve(inputs),
        previewImport: ({ input }) => {
            const name = 'path' in input ? input.path.split('/').at(-1)?.replace('.zip', '') ?? '' : '';
            return name === 'bad'
                ? Promise.reject(new Error('invalid archive'))
                : Promise.resolve({ ...preview(name), conflict: { kind: name === 'last' ? 'different' : 'new' } });
        },
        installImport: (request) => {
            installs.push(request);
            const name = 'path' in request.input ? request.input.path.split('/').at(-1)?.replace('.zip', '') ?? '' : '';
            return name === 'install-fail'
                ? Promise.reject(new Error('install failed'))
                : Promise.resolve({ scope: globalScope, name, action: 'installed' });
        },
    });
    const { deps, state } = createWorld(api);
    const controller = createSkillManagerController(deps);
    await controller.init();
    controller.openImportScopeDialog('archive');
    controller.confirmScopeDialog();
    await waitFor(() => expect(controller.getSnapshot().importDraft.items.every(item => item.preview || item.error)).toBe(true));
    expect(controller.getSnapshot().importDraft.items[1]?.error).toBe('invalid archive');
    controller.setImportConflict(3, 'replace');
    await controller.installImports();

    expect(installs.map(request => 'path' in request.input ? request.input.path : '')).toEqual([
        '/tmp/one.zip', '/tmp/install-fail.zip', '/tmp/last.zip',
    ]);
    expect(installs[2]?.conflictStrategy).toBe('replace');
    expect(state.discards).toBe(1);
    expect(state.successes).toContain('skillBatchInstallToast');
    expect(state.toastErrors).toEqual(['skillImportItemFailed', 'skillBatchInstallFailed']);
});

test('the install commit boundary keeps retries honest and reconciles committed Host state', async () => {
    let installAttempts = 0;
    let installed = false;
    const api = createSkillApi({
        list: ({ scope } = {}) => Promise.resolve(scope?.kind === 'global' && installed ? [skill('retry')] : []),
        pickImportArchives: () => Promise.resolve([{ kind: 'archiveFile', path: '/tmp/retry.zip' }]),
        previewImport: () => Promise.resolve(preview('retry')),
        installImport: () => {
            installAttempts += 1;
            if (installAttempts === 1) return Promise.reject(new Error('install unavailable'));
            installed = true;
            return Promise.resolve({ scope: globalScope, name: 'retry', action: 'installed' });
        },
    });
    const { deps, state } = createWorld(api);
    deps.syncInstallPortability = () => Promise.reject(new Error('portable sync failed'));
    const controller = createSkillManagerController(deps);
    await controller.init();
    controller.openImportScopeDialog('archive');
    controller.confirmScopeDialog();
    await waitFor(() => expect(controller.getSnapshot().importDraft.items[0]?.preview?.skill.name).toBe('retry'));

    await expect(controller.installImports()).rejects.toThrow('install unavailable');
    expect(controller.getSnapshot().importDraft).toMatchObject({ installing: false });
    expect(controller.getSnapshot().importDraft.items).toHaveLength(1);

    await expect(controller.installImports()).rejects.toThrow('portable sync failed');
    expect(controller.getSnapshot().importDraft.items).toHaveLength(0);
    expect(controller.getSnapshot().sections.find(section => section.id === 'global')?.skills[0]?.name).toBe('retry');
    expect(state.errors.map(error => error instanceof Error ? error.message : '')).toEqual([
        'install unavailable',
        'portable sync failed',
    ]);
});

test('an obsolete preview cannot repopulate a draft cleared by a scope change', async () => {
    const pending = deferred<TauriTavernSkillImportPreview>();
    const api = createSkillApi({
        pickImportArchives: () => Promise.resolve([{ kind: 'archiveFile', path: '/tmp/pending.zip' }]),
        previewImport: () => pending.promise,
    });
    const profiles = [
        { id: 'default-writer', displayName: 'Writer', directRunnable: true },
        { id: 'second', displayName: 'Second', directRunnable: true },
    ];
    const { deps, state } = createWorld(api, profiles);
    const controller = createSkillManagerController(deps);
    await controller.init();
    controller.openImportScopeDialog('archive');
    controller.setScopeDialogTarget('profile');
    controller.confirmScopeDialog();
    await waitFor(() => expect(controller.getSnapshot().importDraft.items).toHaveLength(1));
    state.emitSettings({ editingProfileId: 'second' });
    await waitFor(() => expect(controller.getSnapshot().importDraft.items).toHaveLength(0));
    pending.resolve(preview('late'));
    await Promise.resolve();
    expect(controller.getSnapshot().importDraft.items).toHaveLength(0);
    expect(state.discards).toBe(1);
});

test('rejects stale preview/file loads and saves with optimistic sha before portable sync', async () => {
    const files: [TauriTavernSkillFileRef, TauriTavernSkillFileRef] = [
        { path: 'one.md', kind: 'text', mediaType: 'text/markdown', sizeBytes: 3, sha256: 'sha-one' },
        { path: 'two.md', kind: 'text', mediaType: 'text/markdown', sizeBytes: 3, sha256: 'sha-two' },
    ];
    const fileLists: Array<ReturnType<typeof deferred<TauriTavernSkillFileRef[]>>> = [];
    const reads: Array<{ path: string; request: ReturnType<typeof deferred<TauriTavernSkillReadResult>> }> = [];
    const writes: Parameters<TauriTavernSkillApi['writeFile']>[0][] = [];
    const events: string[] = [];
    let writeFailure = '';
    const readResult = (path: string, sha256: string, content = path): TauriTavernSkillReadResult => ({
        name: path, path, content, chars: content.length, words: 1, totalChars: content.length,
        totalWords: 1, totalLines: 1, startLine: 1, endLine: 1, lineTruncated: false,
        bytes: content.length, sha256, truncated: false, resourceRef: `skill://${path}`,
    });
    const api = createSkillApi({
        list: ({ scope } = {}) => Promise.resolve(scope?.kind === 'global' ? [skill('alpha'), skill('beta')] : []),
        listFiles: () => {
            if (fileLists.length >= 2) return Promise.resolve(files);
            const request = deferred<TauriTavernSkillFileRef[]>();
            fileLists.push(request);
            return request.promise;
        },
        readFile: ({ path }) => {
            const request = deferred<TauriTavernSkillReadResult>();
            reads.push({ path, request });
            return request.promise;
        },
        writeFile: (request) => {
            events.push('host-write');
            writes.push(request);
            return writeFailure === 'sha'
                ? Promise.reject(new Error('sha256 conflict'))
                : Promise.resolve(readResult(request.path, `sha-${request.content}`, request.content));
        },
    });
    const { deps, state } = createWorld(api);
    deps.syncWritePortability = () => {
        events.push('portable-write');
        return writeFailure === 'portable' ? Promise.reject(new Error('portable sync failed')) : Promise.resolve();
    };
    const controller = createSkillManagerController(deps);
    await controller.init();

    controller.openSkillPreview('global', skill('alpha'));
    controller.openSkillPreview('global', skill('beta'));
    await waitFor(() => expect(fileLists).toHaveLength(2));
    fileLists[1]?.resolve(files);
    await waitFor(() => expect(controller.getSnapshot().preview?.skill.name).toBe('beta'));
    fileLists[0]?.resolve([files[0]]);
    await Promise.resolve();
    expect(controller.getSnapshot().preview?.files).toEqual(files);

    controller.openPreviewFile(files[0]);
    controller.openPreviewFile(files[1]);
    await waitFor(() => expect(reads).toHaveLength(2));
    reads[1]?.request.resolve(readResult('two.md', 'sha-two'));
    await waitFor(() => expect(controller.getSnapshot().fileViewer?.file.path).toBe('two.md'));
    reads[0]?.request.resolve(readResult('one.md', 'sha-one'));
    await Promise.resolve();
    expect(controller.getSnapshot().fileViewer?.file.path).toBe('two.md');

    await controller.saveOpenFile('changed');
    expect(writes[0]?.expectedSha256).toBe('sha-two');
    expect(events).toEqual(['host-write', 'portable-write']);

    writeFailure = 'sha';
    await expect(controller.saveOpenFile('conflict')).rejects.toThrow('sha256 conflict');
    writeFailure = 'portable';
    await expect(controller.saveOpenFile('portable')).rejects.toThrow('portable sync failed');
    expect(controller.getSnapshot().fileViewer?.file).toMatchObject({ content: 'portable', sha256: 'sha-portable' });
    expect(events.slice(-2)).toEqual(['host-write', 'portable-write']);
    expect(state.errors.map(error => error instanceof Error ? error.message : '')).toEqual(['sha256 conflict', 'portable sync failed']);

    writeFailure = '';
    await controller.saveOpenFile('retry');
    expect(writes.at(-1)?.expectedSha256).toBe('sha-portable');
});

test('moves with explicit replacement and syncs every Host mutation before its portable copy', async () => {
    const sourceSkill = skill('shared', globalScope, 'source-hash');
    const presetScope: TauriTavernSkillScope = { kind: 'preset', apiId: 'openai', name: 'Preset A' };
    const events: string[] = [];
    const moves: Parameters<TauriTavernSkillApi['move']>[0][] = [];
    const api = createSkillApi({
        list: ({ scope } = {}) => {
            if (scope?.kind === 'global') return Promise.resolve([sourceSkill]);
            if (scope?.kind === 'preset') return Promise.resolve([skill('shared', presetScope, 'target-hash')]);
            return Promise.resolve([]);
        },
        move: (request) => { events.push('host-move'); moves.push(request); return Promise.resolve({ scope: request.toScope, name: request.name, action: 'replaced' }); },
        delete: () => { events.push('host-delete'); return Promise.resolve(); },
    });
    const { deps } = createWorld(api);
    deps.syncMovePortability = () => { events.push('portable-move'); return Promise.resolve(); };
    deps.syncDeletePortability = () => { events.push('portable-delete'); return Promise.resolve(); };
    const controller = createSkillManagerController(deps);
    await controller.init();

    controller.openMoveScopeDialog('global', sourceSkill);
    controller.confirmScopeDialog();
    await waitFor(() => expect(moves).toHaveLength(1));
    expect(moves[0]?.conflictStrategy).toBe('replace');
    controller.deleteSkill('global', sourceSkill);
    await waitFor(() => expect(events).toContain('portable-delete'));
    expect(events.filter(event => event.includes('move') || event.includes('delete'))).toEqual([
        'host-move', 'portable-move', 'host-delete', 'portable-delete',
    ]);
});

test('a committed delete reconciles before its portable sync failure propagates', async () => {
    const events: string[] = [];
    const api = createSkillApi({
        delete: () => { events.push('host-delete'); return Promise.resolve(); },
    });
    const { deps } = createWorld(api);
    deps.syncDeletePortability = () => { events.push('portable-delete'); return Promise.reject(new Error('portable sync failed')); };

    await expect(deleteSkillMutation(deps, globalScope, skill('deleted'), {
        onCommitted: () => events.push('committed'),
        reconcile: () => { events.push('reconcile'); return Promise.resolve(); },
    })).rejects.toThrow('portable sync failed');

    expect(events).toEqual(['host-delete', 'committed', 'portable-delete', 'reconcile']);
});
