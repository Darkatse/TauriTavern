import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, expect, test, rstest as vi } from '@rstest/core';
import { AssistantApp } from './AssistantApp';
import { Transcript } from './Transcript';
import { createAssistantProfile } from './profile';
import { installAssistantDrawer } from './drawer';
import type { AssistantSnapshot } from './controller';
import type { AssistantActions, AssistantController } from './host';

afterEach(() => { cleanup(); document.body.replaceChildren(); });
function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(done => { resolve = done; });
    return { promise, resolve };
}
function harness() {
    const profile = createAssistantProfile();
    profile.model = { mode: 'connectionRef', connectionRef: 'test', modelId: 'test' };
    let snapshot: AssistantSnapshot = {
        initialized: true, profile, profileSaved: true, busy: false, error: null,
        sessionId: 'session', messages: [], nextBeforeSeq: null, run: null, events: [], responses: [],
    };
    const listeners = new Set<() => void>();
    const send = deferred<TauriTavernAgentSessionRunHandle>();
    const controller: AssistantController = {
        getSnapshot: () => snapshot, subscribe: listener => { listeners.add(listener); return () => { listeners.delete(listener); }; },
        initialize: () => Promise.resolve(), saveProfile: () => Promise.resolve(), send: vi.fn(() => send.promise),
        cancel: () => Promise.resolve(), refresh: () => Promise.resolve(), loadOlder: () => Promise.resolve(), dispose: () => {},
    };
    const actions: AssistantActions = {
        skill: {} as TauriTavernSkillApi,
        contentWidthPercent: 100, saveContentWidth: () => Promise.resolve(),
        copy: () => Promise.resolve(), openLink: () => Promise.resolve(), markdown: text => text,
        isMobile: () => true, shouldSendOnEnter: () => true,
        loadOptions: () => Promise.resolve({ models: [], presets: ['Default'], tools: [], diagnostics: [], skills: [] }),
        readResult: () => Promise.reject(new Error('No external result')), openConnections: () => {},
    };
    return { controller, actions, send, get snapshot() { return snapshot; }, publish(patch: Partial<AssistantSnapshot>) {
        snapshot = { ...snapshot, ...patch }; listeners.forEach(listener => listener());
    } };
}
test('IME and modified Enter never send to character chat; switching formatting retains a draft, and sending clears only after acceptance', async () => {
    document.body.innerHTML = '<div id="advanced-formatting-button"><div class="drawer-toggle"><div class="drawer-icon"></div></div><div id="AdvancedFormatting" class="openDrawer"></div></div>';
    const drawer = installAssistantDrawer();
    const h = harness();
    const globalKey = vi.fn();
    document.addEventListener('keydown', globalKey);
    render(<AssistantApp controller={h.controller} actions={h.actions} drawer={drawer} />, { container: drawer.mount });
    let input = screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Message the app assistant' });
    fireEvent.change(input, { target: { value: '检查设置' } });
    fireEvent.keyDown(input, { key: 'Enter', isComposing: true });
    fireEvent.keyDown(input, { key: 'Enter', keyCode: 229 });
    fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true });
    fireEvent.keyDown(input, { key: 'Enter', altKey: true });
    expect(h.controller.send).not.toHaveBeenCalled();
    expect(globalKey).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Advanced formatting' }));
    fireEvent.click(screen.getByRole('button', { name: 'Back to assistant' }));
    input = screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Message the app assistant' });
    expect(input.value).toBe('检查设置');
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(h.controller.send).toHaveBeenCalledTimes(1);
    expect(input.value).toBe('检查设置');
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(h.controller.send).toHaveBeenCalledTimes(1);
    await act(async () => { h.send.resolve({ runId: 'run', sessionId: 'session', status: 'calling_model' }); await h.send.promise; });
    expect(screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Message the app assistant' }).value).toBe('');
    document.removeEventListener('keydown', globalKey); drawer.dispose();
});

test('reasoning stays expanded when a saved reply replaces its live preview', () => {
    const h = harness();
    const response: TauriTavernAgentRunLiveResponse = { invocationId: 'root', invocationExitPolicy: 'reply_allowed', round: 1, attempt: 1, text: 'Answer', reasoning: 'Evidence', toolIds: [] };
    h.publish({ run: { runId: 'run', active: true, status: 'calling_model' }, responses: [response] });
    const view = render(<Transcript snapshot={h.snapshot} controller={h.controller} actions={h.actions} visible />);
    fireEvent.click(screen.getByRole('button', { name: 'Thought process' }));
    h.publish({ responses: [], messages: [{ seq: 2, runId: 'run', createdAt: '', origin: { invocationId: 'root', round: 1 },
        message: { role: 'assistant', providerMetadata: null, parts: [{ type: 'reasoning', text: 'Evidence', provider_metadata: null }, { type: 'text', text: 'Answer' }] } }] });
    view.rerender(<Transcript snapshot={h.snapshot} controller={h.controller} actions={h.actions} visible />);
    expect(screen.getByRole('button', { name: 'Thought process' }).getAttribute('aria-expanded')).toBe('true');
});

test('stop remains pending until a terminal update and preserves the next draft', async () => {
    document.body.innerHTML = '<div id="advanced-formatting-button"><div class="drawer-toggle"><div class="drawer-icon"></div></div><div id="AdvancedFormatting" class="openDrawer"></div></div>';
    const drawer = installAssistantDrawer();
    const h = harness();
    h.publish({ run: { runId: 'run', active: true, status: 'calling_model' } });
    render(<AssistantApp controller={h.controller} actions={h.actions} drawer={drawer} />, { container: drawer.mount });
    fireEvent.change(screen.getByRole('textbox', { name: 'Message the app assistant' }), { target: { value: 'Next message' } });
    fireEvent.click(screen.getByRole('button', { name: 'Stop this run' }));
    await waitFor(() => expect(screen.getByRole('status').textContent).toBe('Stopping…'));
    expect(screen.getByRole('button', { name: 'Stop this run' }).hasAttribute('disabled')).toBe(true);
    expect(screen.queryByRole('button', { name: 'Send message' })).toBeNull();
    act(() => h.publish({ run: { runId: 'run', active: false, status: 'cancelled' } }));
    expect(screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Message the app assistant' }).value).toBe('Next message');
    expect(screen.getByRole('button', { name: 'Send message' }).hasAttribute('disabled')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
    act(() => h.publish({ run: { runId: 'next-run', active: true, status: 'calling_model' }, busy: true }));
    expect(screen.getByRole('button', { name: 'Stop this run' }).hasAttribute('disabled')).toBe(false);
    drawer.dispose();
});

test('reused provider call IDs keep each round’s result with its own call', () => {
    const h = harness();
    const entry = (seq: number, round: number, part: TauriTavernAgentModelContentPart): TauriTavernAgentSessionMessage => ({
        seq, runId: 'run', createdAt: '', origin: { invocationId: 'root', round },
        message: { role: part.type === 'toolResult' ? 'tool' : 'assistant', providerMetadata: null, parts: [part] },
    });
    const call = { type: 'toolCall' as const, call: { callId: 'tool_call_0', toolId: 'extension/in-app-agent:app.evaluate', arguments: { code: 'return 1' }, providerMetadata: {} } };
    const result = (content: string) => ({ type: 'toolResult' as const, result: { callId: 'tool_call_0', toolId: call.call.toolId, content, structured: null, isError: false, resourceRefs: [] } });
    h.publish({ messages: [entry(1, 1, call), entry(2, 1, result('first result')), entry(3, 2, call), entry(4, 2, result('second result'))] });
    render(<Transcript snapshot={h.snapshot} controller={h.controller} actions={h.actions} visible />);
    const [first, second] = screen.getAllByRole('article');
    if (!first || !second) throw new Error('Expected both model rounds');
    fireEvent.click(within(first).getByRole('button', { name: /Run page script/ }));
    fireEvent.click(within(second).getByRole('button', { name: /Run page script/ }));
    expect(within(first).getByText('first result')).toBeTruthy();
    expect(within(first).queryByText('second result')).toBeNull();
    expect(within(second).getByText('second result')).toBeTruthy();
});

test('paging preserves the visible anchor through pending updates and never follows an older user message', async () => {
    const h = harness();
    const page = deferred<void>();
    h.controller.loadOlder = () => page.promise;
    const entry = (seq: number, role: 'user' | 'assistant'): TauriTavernAgentSessionMessage => ({
        seq, runId: 'run', createdAt: '', message: { role, providerMetadata: null, parts: [{ type: 'text', text: `Message ${seq}` }] },
    });
    h.publish({ messages: [entry(10, 'assistant')], nextBeforeSeq: 10 });
    const view = render(<Transcript snapshot={h.snapshot} controller={h.controller} actions={h.actions} visible />);
    const root = screen.getByRole('region', { name: 'Conversation' });
    const row = view.container.querySelector<HTMLElement>('[data-message-seq="10"]');
    if (!row) throw new Error('Expected the current page');
    let rowContentOffset = 160;
    root.getBoundingClientRect = () => DOMRect.fromRect({ y: 100, height: 300 });
    row.getBoundingClientRect = () => DOMRect.fromRect({ y: 100 + rowContentOffset - root.scrollTop, height: 80 });
    Object.defineProperty(root, 'scrollHeight', { value: 1000 });
    root.scrollTop = 120;
    const visibleOffset = () => row.getBoundingClientRect().top - root.getBoundingClientRect().top;
    const before = visibleOffset();
    fireEvent.click(screen.getByRole('button', { name: 'Load earlier messages' }));
    h.publish({ busy: true, responses: [] });
    view.rerender(<Transcript snapshot={h.snapshot} controller={h.controller} actions={h.actions} visible />);
    expect(visibleOffset()).toBe(before);
    rowContentOffset += 200;
    h.publish({ busy: false, messages: [entry(1, 'user'), ...h.snapshot.messages] });
    view.rerender(<Transcript snapshot={h.snapshot} controller={h.controller} actions={h.actions} visible />);
    await act(async () => { page.resolve(); await page.promise; });
    expect(visibleOffset()).toBe(before);
    h.publish({ responses: [] });
    view.rerender(<Transcript snapshot={h.snapshot} controller={h.controller} actions={h.actions} visible />);
    expect(visibleOffset()).toBe(before);
});

test('cancelling settings releases an unconfirmed Skill import and saving waits for its selection', async () => {
    document.body.innerHTML = '<div id="advanced-formatting-button"><div class="drawer-toggle"><div class="drawer-icon"></div></div><div id="AdvancedFormatting" class="openDrawer"></div></div>';
    const drawer = installAssistantDrawer();
    const h = harness();
    h.publish({ profileSaved: false });
    let importing = false;
    const input: TauriTavernSkillImportInput = { kind: 'inlineFiles', files: [] };
    const preview: TauriTavernSkillImportPreview = {
        skill: { scope: { kind: 'profile', profileId: h.snapshot.profile.id }, name: 'test-skill', description: 'Test import',
            tags: [], installedHash: 'hash', fileCount: 1, totalBytes: 12, hasScripts: false, hasBinary: false, installedAt: '' },
        files: [], conflict: { kind: 'new' }, warnings: [], source: null,
    };
    h.actions.skill = { acquireImport: () => {
        if (importing) throw new Error('skill.import_busy');
        importing = true;
        return () => { importing = false; return Promise.resolve(); };
    }, pickImportArchives: () => Promise.resolve([input]),
        discoverImports: () => Promise.resolve([input]), previewImport: () => Promise.resolve(preview) } as unknown as TauriTavernSkillApi;
    const view = render(<AssistantApp controller={h.controller} actions={h.actions} drawer={drawer} />, { container: drawer.mount });
    fireEvent.click(screen.getByRole('button', { name: 'Assistant settings' }));
    await screen.findByRole('button', { name: /Skill/ });
    fireEvent.click(screen.getByRole('button', { name: /Skill/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Import ZIP' }));
    await screen.findByText('test-skill');
    expect(screen.getByRole('button', { name: 'Save settings' }).hasAttribute('disabled')).toBe(true);
    const footer = view.container.querySelector<HTMLElement>('.ttia-settings-footer');
    if (!footer) throw new Error('Expected settings actions');
    fireEvent.click(within(footer).getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Assistant settings' }));
    expect(screen.getByRole('button', { name: 'Save settings' }).hasAttribute('disabled')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: /Skill/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Import ZIP' }));
    await screen.findByText('test-skill');
    expect(screen.queryByRole('alert')).toBeNull();
    drawer.dispose();
});
