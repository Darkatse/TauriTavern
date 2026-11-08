import { act, fireEvent, render, waitFor, within } from '@testing-library/react';
import { afterAll, beforeEach, expect, rs, test } from '@rstest/core';
import { StrictMode, useState } from 'react';
import { EditorView } from '@codemirror/view';

import { createFrontend } from '../../../../../../tests/helpers/frontend.mjs';
import { SkillManager } from './SkillManager';
import { CodeMirrorTextarea } from '../CodeMirrorTextarea';
import * as hostApi from '../host-api';
import {
    emptySkillImportDraft,
    type SkillManagerController,
    type SkillManagerSnapshot,
} from './SkillManagerContract';
import { buildSkillFileTree, SkillFileViewer } from './SkillManagerFiles';
import { ensureSkillManagerContainer } from './settings-entry';

type EditorRuntime = typeof import('../../../../tauri/codemirror-editor.js');

// The host editor runtime runs in the application; the editors it mounts must live in the application's document.
const frontend = await createFrontend();
afterAll(frontend.close);
await frontend.startHost();
await frontend.load('script.js');
const editorRuntime = await frontend.load('scripts/tauri/codemirror-editor.js') as EditorRuntime;

function inApplication() {
    const { document } = frontend.window;
    return { container: document.body.appendChild(document.createElement('div')) };
}

beforeEach(() => {
    editorRuntime.initializeCodeMirrorEditor({ codemirror_editor_enabled: true });
    rs.spyOn(hostApi, 'loadCodeMirrorEditor').mockResolvedValue(editorRuntime);
});

const tr = (key: string): string => key;

/** CodeMirror renders in the shadow root of its wrapper. */
function editorRoot(container: HTMLElement): ShadowRoot {
    const root = container.querySelector('.tt-codemirror-editor')?.shadowRoot;
    if (!root) throw new Error('expected editor');
    return root;
}

const inEditor = (container: HTMLElement) => within(editorRoot(container) as unknown as HTMLElement);

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
        importBusy: false,
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

test('Skill preview and edits share CodeMirror history and submit the current draft', async () => {
    const save = rs.fn((content: string) => Promise.resolve({ ...readFile(), content }));
    const result = render(<StrictMode><SkillFileViewer file={readFile()} onSave={save} onClose={() => undefined} tr={tr} /></StrictMode>, inApplication());
    await waitFor(() => expect(editorRoot(result.container).querySelectorAll('.cm-editor').length).toBe(1));
    expect(editorRoot(result.container).querySelector('.cm-content')?.getAttribute('contenteditable')).toBe('false');
    fireEvent.click(inEditor(result.container).getByRole('button', { name: 'Find and replace' }));
    fireEvent.click(inEditor(result.container).getByRole('button', { name: 'Close' }));
    expect(editorRoot(result.container).activeElement).toBe(editorRoot(result.container).querySelector('.cm-content'));
    fireEvent.click(result.getByRole('button', { name: 'edit' }));
    const element = editorRoot(result.container).querySelector<HTMLElement>('.cm-editor');
    if (!element) throw new Error('expected editor');
    const editor = EditorView.findFromDOM(element);
    if (!editor) throw new Error('expected CodeMirror view');
    act(() => editor.dispatch({ changes: { from: 4, insert: ' edited' } }));
    fireEvent.click(inEditor(result.container).getByRole('button', { name: 'Undo' }));
    expect(editor.state.doc.toString()).toBe('body');
    fireEvent.click(inEditor(result.container).getByRole('button', { name: 'Redo' }));
    expect(editor.state.doc.toString()).toBe('body edited');
    fireEvent.click(result.getByRole('button', { name: 'save' }));
    await waitFor(() => expect(save).toHaveBeenCalledWith('body edited'));
    await waitFor(() => expect(result.getByRole('button', { name: 'edit' })).toBeTruthy());

    result.rerender(<SkillFileViewer file={{ ...readFile(), truncated: true }} onSave={save} onClose={() => undefined} tr={tr} />);
    await waitFor(() => expect(editorRoot(result.container).querySelector('.cm-content')?.getAttribute('contenteditable')).toBe('false'));
    expect(result.queryByRole('button', { name: 'edit' })).toBeNull();
});

test('controlled Skill text survives cancelled mounts, permission changes, and the disabled editor preference', async () => {
    function Source({ readOnly = false, disabled = false }) {
        const [value, setValue] = useState('draft');
        return <CodeMirrorTextarea value={value} onChange={setValue} readOnly={readOnly} disabled={disabled} label="SKILL.md" className="text_pole" />;
    }
    let release!: (runtime: EditorRuntime) => void;
    const pending = new Promise<EditorRuntime>(resolve => { release = resolve; });
    const loadRuntime = rs.spyOn(hostApi, 'loadCodeMirrorEditor').mockReturnValueOnce(pending).mockResolvedValue(editorRuntime);
    const first = render(<StrictMode><Source /></StrictMode>, inApplication());
    await waitFor(() => expect(loadRuntime).toHaveBeenCalled());
    first.unmount();
    const current = render(<StrictMode><Source /></StrictMode>, inApplication());
    current.container.querySelector('textarea')?.focus();
    current.container.querySelector('textarea')?.setSelectionRange(3, 3);
    await waitFor(() => expect(editorRoot(current.container).querySelectorAll('.cm-editor').length).toBe(1));
    expect(editorRoot(current.container).activeElement).toBe(editorRoot(current.container).querySelector('.cm-content'));
    await act(async () => { release(editorRuntime); await pending; });
    const element = editorRoot(current.container).querySelector<HTMLElement>('.cm-editor');
    if (!element) throw new Error('expected editor');
    const editor = EditorView.findFromDOM(element);
    if (!editor) throw new Error('expected CodeMirror view');
    expect(editor.state.selection.main.from).toBe(3);
    expect(editor.state.selection.main.to).toBe(3);
    act(() => editor.dispatch({ changes: { from: 5, insert: ' updated' } }));
    expect(current.container.querySelector('textarea')?.value).toBe('draft updated');
    fireEvent.click(inEditor(current.container).getByRole('button', { name: 'Find and replace' }));
    fireEvent.input(inEditor(current.container).getByRole('textbox', { name: 'Find' }), { target: { value: 'updated' } });
    const selection = editor.state.selection.main;
    current.rerender(<StrictMode><Source readOnly /></StrictMode>);
    expect(editorRoot(current.container).querySelector('.cm-content')?.getAttribute('aria-readonly')).toBe('true');
    expect(inEditor(current.container).queryByRole('button', { name: 'Toggle replace' })).toBeNull();
    expect(editorRoot(current.container).querySelector('input[name=replace]')).toBeNull();
    current.rerender(<StrictMode><Source disabled /></StrictMode>);
    expect(editorRoot(current.container).querySelector('.cm-content')?.getAttribute('contenteditable')).toBe('false');
    current.rerender(<StrictMode><Source /></StrictMode>);
    expect(editorRoot(current.container).querySelector('.cm-content')?.getAttribute('contenteditable')).toBe('true');
    expect(editorRoot(current.container).querySelector('input[name=search]')?.getAttribute('aria-invalid')).toBe('false');
    expect(inEditor(current.container).getByRole<HTMLInputElement>('textbox', { name: 'Find' }).value).toBe('updated');
    expect(editor.state.selection.main.eq(selection)).toBe(true);
    fireEvent.click(inEditor(current.container).getByRole('button', { name: 'Toggle replace' }));
    expect(inEditor(current.container).getByRole('textbox', { name: 'Replace' })).toBeTruthy();
    fireEvent.click(inEditor(current.container).getByRole('button', { name: 'Undo' }));
    expect(editor.state.doc.toString()).toBe('draft');
    current.unmount();

    editorRuntime.initializeCodeMirrorEditor({ codemirror_editor_enabled: false });
    const plain = render(<Source />, inApplication());
    await act(async () => { await Promise.resolve(); });
    fireEvent.change(plain.getByRole('textbox', { name: 'SKILL.md' }), { target: { value: 'plain draft' } });
    expect((plain.getByRole('textbox', { name: 'SKILL.md' }) as HTMLTextAreaElement).value).toBe('plain draft');
    expect(plain.container.querySelector('.tt-codemirror-editor')).toBeNull();
});

test('dialog cancel is prevented and preview overlay closes only on self', () => {
    const view = createViewController();
    const { container } = render(<SkillManager controller={view.controller} tr={tr} />, inApplication());
    const scopeDialog = container.querySelector<HTMLDialogElement>('dialog.ttas-scope-dialog:not(.ttas-skill-source-dialog)');
    const previewDialog = container.querySelector<HTMLDialogElement>('dialog.ttas-skill-preview-dialog');
    const overlay = container.querySelector<HTMLElement>('.ttas-file-overlay');
    const panel = container.querySelector<HTMLElement>('.ttas-file-overlay-panel');
    if (!scopeDialog || !previewDialog || !overlay || !panel) throw new Error('expected Skill dialogs');

    const scopeCancel = new Event('cancel', { cancelable: true });
    act(() => { scopeDialog.dispatchEvent(scopeCancel); });
    expect(scopeCancel.defaultPrevented).toBe(true);
    expect(view.getSnapshot().scopeDialog.mode).toBe('');

    fireEvent.mouseDown(panel);
    expect(view.getSnapshot().fileViewer).not.toBeNull();
    fireEvent.mouseDown(overlay);
    expect(view.getSnapshot().fileViewer).toBeNull();

    act(view.reopenViewer);
    const firstPreviewCancel = new Event('cancel', { cancelable: true });
    act(() => { previewDialog.dispatchEvent(firstPreviewCancel); });
    expect(firstPreviewCancel.defaultPrevented).toBe(true);
    expect(view.getSnapshot().preview).not.toBeNull();
    expect(view.getSnapshot().fileViewer).toBeNull();

    const secondPreviewCancel = new Event('cancel', { cancelable: true });
    act(() => { previewDialog.dispatchEvent(secondPreviewCancel); });
    expect(secondPreviewCancel.defaultPrevented).toBe(true);
    expect(view.getSnapshot().preview).toBeNull();
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

test('settings entry stays directly after Agent and reuses its container', () => {
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

    expect(ensureSkillManagerContainer()).toBe(skillContainer);
});
