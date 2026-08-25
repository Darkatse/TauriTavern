import { act, cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';

import { SkillManager } from './SkillManager';
import {
    emptySkillImportDraft,
    type SkillManagerController,
    type SkillManagerSnapshot,
} from './SkillManagerContract';
import { buildSkillFileTree } from './SkillManagerFiles';
import { ensureSkillManagerContainer } from './settings-entry';

const tr = (key: string): string => key;

function skill(name: string): TauriTavernSkillIndexEntry {
    return {
        scope: { kind: 'global' }, name, description: '', tags: [], installedHash: 'hash',
        fileCount: 1, totalBytes: 1, hasScripts: false, hasBinary: false,
        installedAt: '2026-01-01T00:00:00Z',
    };
}

function readFile(path = 'SKILL.md'): TauriTavernSkillReadResult {
    return {
        name: path, path, content: 'body', chars: 4, words: 1, totalChars: 4, totalWords: 1,
        totalLines: 1, startLine: 1, endLine: 1, lineTruncated: false, bytes: 4,
        sha256: 'sha', truncated: false, resourceRef: `skill://${path}`,
    };
}

function createViewController() {
    const file: TauriTavernSkillFileRef = {
        path: 'SKILL.md', kind: 'text', mediaType: 'text/markdown', sizeBytes: 4, sha256: 'sha',
    };
    let snapshot: SkillManagerSnapshot = {
        initialized: true,
        loading: false,
        error: '',
        profiles: [],
        selectedProfileId: 'default-writer',
        sections: [{
            id: 'global', icon: 'fa-globe', labelKey: 'skillScopeGlobal', available: true,
            subtitle: 'Global', scope: { kind: 'global' }, skills: [skill('writer')], loading: false,
        }],
        importDraft: emptySkillImportDraft(0),
        scopeDialog: { mode: 'import', importKind: 'archive', selectedSectionId: 'global' },
        sourceDialog: { mode: '' },
        searchQuery: '',
        preview: {
            id: 1, sectionId: 'global', scope: { kind: 'global' }, scopeLabel: 'Global',
            skill: skill('writer'), files: [file], loading: false, expandedFolders: {},
        },
        fileViewer: { id: 1, file: readFile() },
        supportsDirectoryImport: true,
    };
    const listeners = new Set<() => void>();
    const update = (patch: Partial<SkillManagerSnapshot>) => {
        snapshot = { ...snapshot, ...patch };
        listeners.forEach(listener => listener());
    };
    const noop = () => undefined;
    const controller: SkillManagerController = {
        getSnapshot: () => snapshot,
        subscribe: (listener) => { listeners.add(listener); return () => listeners.delete(listener); },
        init: () => Promise.resolve(),
        dispose: noop,
        setSearchQuery: noop,
        selectProfile: noop,
        refreshAll: noop,
        openImportScopeDialog: noop,
        openMoveScopeDialog: noop,
        setScopeDialogTarget: noop,
        setScopeImportKind: noop,
        closeScopeDialog: () => update({ scopeDialog: { mode: '' } }),
        confirmScopeDialog: noop,
        setSourceContent: noop,
        setSourceUrl: noop,
        closeSourceDialog: () => update({ sourceDialog: { mode: '' } }),
        confirmSourceDialog: noop,
        setImportConflict: noop,
        clearImportDraft: noop,
        installImports: () => Promise.resolve(),
        openSkillPreview: noop,
        closePreview: () => update({ preview: null, fileViewer: null }),
        previewClosed: () => update({ preview: null, fileViewer: null }),
        previewCancelled: () => snapshot.fileViewer ? update({ fileViewer: null }) : update({ preview: null }),
        togglePreviewFolder: noop,
        openPreviewFile: noop,
        closeFileViewer: () => update({ fileViewer: null }),
        saveOpenFile: content => Promise.resolve({ ...readFile(), content }),
        exportSkill: noop,
        deleteSkill: noop,
        dialogShowFailed: noop,
    };
    return {
        controller,
        getSnapshot: () => snapshot,
        reopenViewer: () => update({ fileViewer: { id: 2, file: readFile() } }),
    };
}

afterEach(() => {
    cleanup();
    document.body.replaceChildren();
});

test('dialog cancel is prevented and preview overlay closes only on self', async () => {
    const showModal = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'showModal');
    const close = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close');
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
        configurable: true,
        value(this: HTMLDialogElement) { this.open = true; },
    });
    Object.defineProperty(HTMLDialogElement.prototype, 'close', {
        configurable: true,
        value(this: HTMLDialogElement) { this.open = false; this.dispatchEvent(new Event('close')); },
    });
    const view = createViewController();
    try {
        render(<SkillManager controller={view.controller} tr={tr} />);
        const scopeDialog = document.querySelector<HTMLDialogElement>('dialog.ttas-scope-dialog:not(.ttas-skill-source-dialog)');
        const previewDialog = document.querySelector<HTMLDialogElement>('dialog.ttas-skill-preview-dialog');
        const overlay = document.querySelector<HTMLElement>('.ttas-file-overlay');
        const panel = document.querySelector<HTMLElement>('.ttas-file-overlay-panel');
        if (!scopeDialog || !previewDialog || !overlay || !panel) throw new Error('expected Skill dialogs');

        const scopeCancel = new Event('cancel', { cancelable: true });
        await act(() => scopeDialog.dispatchEvent(scopeCancel));
        expect(scopeCancel.defaultPrevented).toBe(true);
        expect(view.getSnapshot().scopeDialog.mode).toBe('');

        fireEvent.mouseDown(panel);
        expect(view.getSnapshot().fileViewer).not.toBeNull();
        fireEvent.mouseDown(overlay);
        expect(view.getSnapshot().fileViewer).toBeNull();

        act(view.reopenViewer);
        const firstPreviewCancel = new Event('cancel', { cancelable: true });
        await act(() => previewDialog.dispatchEvent(firstPreviewCancel));
        expect(firstPreviewCancel.defaultPrevented).toBe(true);
        expect(view.getSnapshot().preview).not.toBeNull();
        expect(view.getSnapshot().fileViewer).toBeNull();

        const secondPreviewCancel = new Event('cancel', { cancelable: true });
        await act(() => previewDialog.dispatchEvent(secondPreviewCancel));
        expect(secondPreviewCancel.defaultPrevented).toBe(true);
        expect(view.getSnapshot().preview).toBeNull();
    } finally {
        if (showModal) Object.defineProperty(HTMLDialogElement.prototype, 'showModal', showModal);
        else Reflect.deleteProperty(HTMLDialogElement.prototype, 'showModal');
        if (close) Object.defineProperty(HTMLDialogElement.prototype, 'close', close);
        else Reflect.deleteProperty(HTMLDialogElement.prototype, 'close');
    }
});

test('file tree validates paths and sorts folders before files', () => {
    const tree = buildSkillFileTree([
        { path: 'z.md', kind: 'text', mediaType: 'text/markdown', sizeBytes: 1, sha256: 'z' },
        { path: 'folder/b.md', kind: 'binary', mediaType: 'application/octet-stream', sizeBytes: 1, sha256: 'b' },
        { path: 'a.md', kind: 'text', mediaType: 'text/markdown', sizeBytes: 1, sha256: 'a' },
    ], tr);
    expect(tree.map(node => node.name)).toEqual(['folder', 'a.md', 'z.md']);
    expect(() => buildSkillFileTree([
        { path: '../escape.md', kind: 'text', mediaType: 'text/markdown', sizeBytes: 1, sha256: 'x' },
    ], tr)).toThrow('invalidSkillFilePath');
});

test('settings entry stays directly after Agent so MCP can anchor after Skill', () => {
    document.body.innerHTML = `
        <div id="rm_extensions_block">
            <div id="extensions_settings2">
                <div id="agent_system_container" class="extension_container"></div>
                <div id="hypebot_container" class="extension_container"></div>
            </div>
        </div>
    `;
    const skillContainer = ensureSkillManagerContainer();
    const agentContainer = document.getElementById('agent_system_container');
    expect(skillContainer.parentElement?.id).toBe('extensions_settings2');
    expect(agentContainer?.nextElementSibling).toBe(skillContainer);

    const mcpContainer = document.createElement('div');
    mcpContainer.id = 'mcp_manager_container';
    skillContainer.insertAdjacentElement('afterend', mcpContainer);
    expect([...skillContainer.parentElement?.children ?? []].map(element => element.id)).toEqual([
        'agent_system_container', 'skill_manager_container', 'mcp_manager_container', 'hypebot_container',
    ]);
    expect(ensureSkillManagerContainer()).toBe(skillContainer);
});
