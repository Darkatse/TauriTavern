import { act, fireEvent, within } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';

import { mountTauriTavernRequestInspectorApp } from './RequestInspectorApp';
import type {
    LogIndexEntry, LogView, PromptSource, RequestInspectorActions, RequestInspectorHandle,
    RequestInspectorMountOptions, RequestPreview,
} from './RequestInspectorContract';

declare global {
    // The mount creates its React root directly, so act() needs the opt-in.
    var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const tr = (key: string) => key;
const handles: RequestInspectorHandle[] = [];

afterEach(() => {
    for (const handle of handles.splice(0)) act(() => handle.unmount());
});

const text = (value: string) => ({ type: 'text' as const, text: value, cacheBreakpoint: false });

function indexEntry(id: number): LogIndexEntry {
    return {
        id, timestampMs: 1750000000000 + id * 1000, level: 'INFO', ok: true,
        source: `source-${id}`, model: `model-${id}`, endpoint: 'https://api.example.com/v1/chat',
        durationMs: 100 + id, stream: true,
    };
}

function logView(id: number): LogView {
    return {
        ...indexEntry(id),
        responseRawKind: 'json',
        view: {
            request: { kind: 'chat', system: [], messages: [{ role: 'user', parts: [text(`request-${id}`)] }], tools: [], parameters: [] },
        },
    };
}

function createHarness(preview: RequestPreview | null = null) {
    let handler: ((entry: LogIndexEntry) => void) | null = null;
    let keepError: Error | null = null;
    const gatedViews = new Map<number, () => void>();
    let indexResolver: ((entries: LogIndexEntry[]) => void) | null = null;
    let indexRejecter: ((error: Error) => void) | null = null;
    const errors: Error[] = [];

    const actions: RequestInspectorActions = {
        copyText: () => Promise.resolve(),
        openTextViewer: () => Promise.resolve(),
        reportError: (error) => {
            if (!(error instanceof Error)) throw new TypeError('expected an Error');
            errors.push(error);
        },
        errorText: error => String(error),
    };
    const client: RequestInspectorMountOptions['client'] = {
        index: () => new Promise((resolve, reject) => {
            indexResolver = resolve;
            indexRejecter = reject;
        }),
        getLogView: id => new Promise((resolve) => {
            if (gatedViews.has(id)) {
                gatedViews.set(id, () => resolve(logView(id)));
                return;
            }
            resolve(logView(id));
        }),
        getLogText: id => Promise.resolve({ requestReadable: `request-${id}`, responseReadable: `response-${id}` }),
        getRaw: id => Promise.resolve({ id, requestRaw: '{}', responseRaw: '{}', responseRawKind: 'json' }),
        subscribeIndex: (next) => {
            handler = next;
            return Promise.resolve(() => {
                if (handler === next) handler = null;
            });
        },
        setKeep: () => keepError ? Promise.reject(keepError) : Promise.resolve(),
        preview: () => preview ? Promise.resolve(preview) : Promise.reject(new Error('no preview')),
    };

    return {
        client,
        actions,
        errors,
        emit: (entry: LogIndexEntry) => handler?.(entry),
        gateView: (id: number) => void gatedViews.set(id, () => {}),
        releaseView: (id: number) => {
            gatedViews.get(id)?.();
            gatedViews.delete(id);
        },
        setKeepError: (error: Error | null) => { keepError = error; },
        resolveIndex: (entries: LogIndexEntry[]) => indexResolver?.(entries),
        rejectIndex: (error: Error) => indexRejecter?.(error),
    };
}

function mountApp(harness: ReturnType<typeof createHarness>, ids: number[], select: 'preview' | 'latest' = 'latest') {
    const container = document.createElement('div');
    document.body.append(container);
    act(() => {
        handles.push(mountTauriTavernRequestInspectorApp(container, {
            keep: 5, entries: ids.map(indexEntry), select, client: harness.client, actions: harness.actions, tr,
        }));
    });
    return container;
}

async function flushAct(action?: () => void): Promise<void> {
    await act(async () => {
        action?.();
        await Promise.resolve();
    });
}

function logSelect(container: HTMLElement): HTMLSelectElement {
    return within(container).getByRole<HTMLSelectElement>('combobox', { name: 'Sent requests' });
}

function logIds(container: HTMLElement): number[] {
    return Array.from(logSelect(container).options, option => Number(option.value)).filter(Boolean);
}

test('the log selection follows the latest, stays on a pinned entry, and falls back when it is trimmed', async () => {
    const harness = createHarness();
    const container = mountApp(harness, [1, 2, 3, 4, 5]);
    await flushAct();

    await flushAct(() => harness.emit(indexEntry(6)));
    expect(logSelect(container).value).toBe('6');
    expect(logIds(container)).toHaveLength(5);
    expect(container.textContent).toContain('source-6');

    fireEvent.change(logSelect(container), { target: { value: '5' } });
    await flushAct();
    await flushAct(() => harness.emit(indexEntry(7)));
    expect(logSelect(container).value).toBe('5');
    expect(container.textContent).toContain('source-5');

    await flushAct(() => {
        harness.emit(indexEntry(8));
        harness.emit(indexEntry(9));
        harness.emit(indexEntry(10));
    });
    expect(logSelect(container).value).toBe('6');
    expect(logIds(container)).toHaveLength(5);
    expect(container.textContent).toContain('source-6');
});

test('a view that loads late does not replace the current selection', async () => {
    const harness = createHarness();
    harness.gateView(4);
    harness.gateView(5);
    const container = mountApp(harness, [4, 5]);
    await flushAct();

    // Selection starts at the newest, 5; move to 4 while 5 is still loading.
    fireEvent.change(logSelect(container), { target: { value: '4' } });
    await flushAct(() => harness.releaseView(5));
    expect(container.textContent).toContain('Loading...');
    expect(container.textContent).not.toContain('request-5');

    await flushAct(() => harness.releaseView(4));
    expect(container.textContent).toContain('request-4');
});

test('opening a row keeps the keyboard focus on it', async () => {
    const container = mountApp(createHarness(), [1]);
    await flushAct();

    const row = within(container).getByRole('button', { name: /request-1/u });
    row.focus();
    fireEvent.click(row);
    expect(row.getAttribute('aria-expanded')).toBe('true');
    expect(document.activeElement).toBe(row);
});

test('keep applies in two phases: a failure commits nothing, a failed reload keeps the new keep', async () => {
    const harness = createHarness();
    const container = mountApp(harness, [1, 2, 3, 4, 5]);
    const view = within(container);
    await flushAct();
    fireEvent.click(view.getByRole('button', { name: 'More' }));
    const keepInput = view.getByRole<HTMLInputElement>('spinbutton', { name: 'Sent requests kept' });

    fireEvent.change(keepInput, { target: { value: '3.5' } });
    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    expect(harness.errors[0]?.message).toContain('positive whole number');
    expect(logIds(container)).toHaveLength(5);

    harness.setKeepError(new Error('persist keep failed'));
    fireEvent.change(keepInput, { target: { value: '3' } });
    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    await flushAct();
    expect(harness.errors).toHaveLength(2);
    expect(logIds(container)).toHaveLength(5);
    expect(keepInput.value).toBe('3');

    // A successful setKeep trims at once; a failing index reload reports but
    // never reverts the persisted keep.
    harness.setKeepError(null);
    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    await flushAct();
    expect(logIds(container)).toEqual([5, 4, 3]);
    expect(logSelect(container).value).toBe('5');
    await flushAct(() => harness.rejectIndex(new Error('index reload failed')));
    expect(harness.errors).toHaveLength(3);
    expect(logIds(container)).toEqual([5, 4, 3]);
});

test('a keep reload trusts the host snapshot, merges raced events and drops a late duplicate', async () => {
    const harness = createHarness();
    const container = mountApp(harness, [1, 2, 3, 4, 5]);
    const view = within(container);
    await flushAct();
    fireEvent.click(view.getByRole('button', { name: 'More' }));

    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    await flushAct();
    await flushAct(() => harness.resolveIndex([2, 3, 4, 5, 6].map(indexEntry)));
    expect(logIds(container)).toEqual([6, 5, 4, 3, 2]);
    expect(logSelect(container).value).toBe('6');

    // The matching event may arrive after the snapshot response.
    await flushAct(() => harness.emit(indexEntry(6)));
    expect(logIds(container)).toEqual([6, 5, 4, 3, 2]);

    // An event arriving while the next snapshot is pending is appended once.
    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    await flushAct();
    await flushAct(() => harness.emit(indexEntry(7)));
    await flushAct(() => harness.resolveIndex([2, 3, 4, 5, 6].map(indexEntry)));
    expect(logIds(container)).toEqual([7, 6, 5, 4, 3]);
    expect(logSelect(container).value).toBe('7');
});

test('a preview is outlined by source: merged system rows, folded chat, tool turns by name', async () => {
    const chat = (id: number): PromptSource => ({
        role: id % 2 ? 'assistant' : 'user',
        tokens: 5,
        pieces: [{ type: 'chat', messageId: id, text: `line ${id}`, tokens: 5 }],
    });
    const promptSources: PromptSource[] = [
        { role: 'system', tokens: 10, pieces: [{ type: 'prompt', identifier: 'main', text: 'You are a narrator.', tokens: 10 }] },
        { role: 'system', tokens: 20, pieces: [{ type: 'prompt', identifier: 'worldInfoBefore', text: 'Keep the lamp lit.', tokens: 20 }] },
        ...Array.from({ length: 9 }, (_, id) => chat(id)),
        { role: 'assistant', tokens: 7, toolCallIds: ['call_1'], pieces: [{ type: 'chat', messageId: 9, text: '', tokens: 7 }] },
        { role: 'tool', tokens: 3, toolCallId: 'call_1', pieces: [{ type: 'chat', text: '{"hp":62}', tokens: 3 }] },
    ];
    const preview: RequestPreview = {
        source: 'custom',
        model: 'model-x',
        endpoint: 'https://api.example.com/v1/chat/completions',
        requestRaw: '{}',
        requestReadable: '',
        view: {
            request: {
                kind: 'chat',
                system: [],
                messages: [
                    { role: 'system', parts: [text('You are a narrator.\nKeep the lamp lit.')] },
                    ...Array.from({ length: 9 }, (_, id) => ({ role: id % 2 ? 'assistant' : 'user', parts: [text(`line ${id}`)] })),
                    { role: 'assistant', parts: [{ type: 'toolCall' as const, id: 'call_1', name: 'read_state', arguments: { target: 'keeper' } }] },
                    { role: 'tool', parts: [{ type: 'toolResult' as const, callId: 'call_1', parts: [text('{"hp":62}')] }] },
                ],
                tools: [],
                parameters: [],
            },
        },
        promptSources,
        toolSources: [],
        promptNames: { main: 'Main Prompt', worldInfoBefore: 'World Info (before)' },
        chatHistory: { included: 10, candidates: 13 },
        assemblyError: null,
        mcpDiagnostics: [],
    };
    const harness = createHarness(preview);
    const container = mountApp(harness, [1], 'preview');
    const view = within(container);
    await flushAct();

    const system = view.getByRole('button', { name: /Main Prompt/u }).closest('section');
    if (!system) throw new Error('system message not found');
    expect(system.textContent).toContain('Merged message · 2');
    expect(within(system).getByRole('button', { name: /World Info \(before\).*Keep the lamp lit\./u })).toBeTruthy();

    // Eleven conversation messages in a row: all but the last two fold into one line.
    const fold = view.getByRole('button', { name: /Chat #0–#8/u });
    expect(fold.textContent).toContain('9 messages · 3 more left out');
    expect(view.queryByText('line 3')).toBeNull();
    expect(view.getByRole('button', { name: /Chat #9.*read_state\(target: "keeper"\)/u })).toBeTruthy();
    expect(view.getByRole('button', { name: /read_state result.*\{"hp":62\}/u })).toBeTruthy();

    // A request sent meanwhile does not take the preview away.
    await flushAct(() => harness.emit(indexEntry(2)));
    expect(view.getByRole('button', { name: 'Preview request' }).getAttribute('aria-pressed')).toBe('true');
});
