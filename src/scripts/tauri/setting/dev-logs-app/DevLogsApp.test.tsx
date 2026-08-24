import { act, fireEvent, within } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';

import { mountTauriTavernDevLogsApp } from './DevLogsApp';
import type {
    DevLogsActions, DevLogsHandle, DevLogsMountOptions, LlmApiLogIndexEntry, LlmApiLogPreview,
    LlmApiLogRaw, LlmApiLogsPanelOptions, LiveLogEntry, LiveLogPanelOptions,
} from './DevLogsContract';

declare global {
    // The mount creates its React root directly, so act() needs the opt-in.
    var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const tr = (key: string) => key;
const handles: DevLogsHandle[] = [];
const containers: HTMLElement[] = [];

afterEach(() => {
    for (const handle of handles.splice(0)) act(() => handle.unmount());
    for (const container of containers.splice(0)) container.remove();
});

function createActions(overrides: Partial<DevLogsActions> = {}) {
    const copied: string[] = [];
    const viewed: Array<{ title: string; text: string; wrap: 'soft' | 'off' }> = [];
    const errors: Error[] = [];
    const actions: DevLogsActions = {
        copyText: (text) => { copied.push(text); return Promise.resolve(); },
        openTextViewer: (options) => { viewed.push(options); return Promise.resolve(); },
        reportError: (error) => { errors.push(error as Error); },
        ...overrides,
    };
    return { actions, copied, viewed, errors };
}

function liveEntry(id: number, overrides: Partial<LiveLogEntry> = {}): LiveLogEntry {
    return { id, timestampMs: 1750000000000 + id * 1000, level: 'info', message: `msg-${id}`, ...overrides };
}

function createLiveHarness(options: { failSetConsoleCapture?: boolean; failTeardown?: boolean } = {}) {
    let handler: ((entry: LiveLogEntry) => void) | null = null;
    let calls = 0;
    let active = 0;
    let delayed = false;
    let failure: Error | null = null;
    let resolveDelayed: (() => void) | null = null;

    const client: LiveLogPanelOptions['client'] = {
        subscribe: (next) => {
            calls += 1;
            if (failure) return Promise.reject(failure);
            const register = () => {
                handler = next;
                active += 1;
                return () => {
                    active -= 1;
                    if (handler === next) handler = null;
                    if (options.failTeardown) return Promise.reject(new Error('stream stop failed'));
                };
            };
            if (delayed) {
                delayed = false;
                return new Promise((resolve) => {
                    resolveDelayed = () => resolve(register());
                });
            }
            return Promise.resolve(register());
        },
        setConsoleCaptureEnabled: options.failSetConsoleCapture
            ? () => Promise.reject(new Error('persist failed'))
            : () => Promise.resolve(),
    };

    return {
        client,
        emit: (entry: LiveLogEntry) => handler?.(entry),
        subscribeCalls: () => calls,
        activeSubscriptions: () => active,
        delaySubscribe: () => { delayed = true; },
        resolveDelayedSubscribe: () => resolveDelayed?.(),
        failSubscribe: (error: Error) => { failure = error; },
    };
}

type LiveHarness = ReturnType<typeof createLiveHarness>;

function createLiveOptions(harness: LiveHarness, overrides: Partial<LiveLogPanelOptions> = {}): LiveLogPanelOptions {
    const base = {
        kind: 'live' as const, title: 'Frontend Logs', initialEntries: [], consoleCaptureEnabled: false,
        showConsoleCapture: false, trimEntriesInPlace: null, client: harness.client, actions: createActions().actions, tr,
    };
    return { ...base, ...overrides };
}

function mountApp(options: DevLogsMountOptions) {
    const container = document.createElement('div');
    document.body.append(container);
    containers.push(container);
    let handle!: DevLogsHandle;
    act(() => {
        handle = mountTauriTavernDevLogsApp(container, options);
    });
    handles.push(handle);
    return { container, handle };
}

function close(handle: DevLogsHandle): void {
    act(() => handle.unmount());
    handles.splice(handles.indexOf(handle), 1);
}

async function flushAct(action?: () => void): Promise<void> {
    await act(async () => {
        action?.();
        await Promise.resolve();
    });
}

function indexEntry(id: number, overrides: Partial<LlmApiLogIndexEntry> = {}): LlmApiLogIndexEntry {
    const base = {
        id, timestampMs: 1750000000000 + id * 1000, level: 'INFO' as const, ok: true,
        source: `source-${id}`, model: `model-${id}`, endpoint: 'https://api.example.com/v1/chat',
        durationMs: 100 + id, stream: true,
    };
    return { ...base, ...overrides };
}

function previewOf(id: number): LlmApiLogPreview {
    return {
        ...indexEntry(id), errorMessage: null, responseRawKind: 'json',
        requestReadable: `request-${id}`, responseReadable: `response-${id}`,
    };
}

function createLlmHarness() {
    let handler: ((entry: LlmApiLogIndexEntry) => void) | null = null;
    let keepError: Error | null = null;
    const gatedPreviews = new Map<number, () => void>();
    let indexResolver: ((entries: LlmApiLogIndexEntry[]) => void) | null = null;
    let indexRejecter: ((error: Error) => void) | null = null;

    const client: LlmApiLogsPanelOptions['client'] = {
        index: () => new Promise((resolve, reject) => {
            indexResolver = resolve;
            indexRejecter = reject;
        }),
        getPreview: (id) => new Promise((resolve) => {
            if (gatedPreviews.has(id)) {
                gatedPreviews.set(id, () => resolve(previewOf(id)));
                return;
            }
            resolve(previewOf(id));
        }),
        getRaw: (id) => Promise.resolve<LlmApiLogRaw>({
            id,
            requestRaw: `raw-request-${id}`,
            responseRaw: `raw-response-${id}`,
            responseRawKind: 'json',
        }),
        subscribeIndex: (next) => {
            handler = next;
            return Promise.resolve(() => {
                if (handler === next) handler = null;
            });
        },
        setKeep: () => keepError ? Promise.reject(keepError) : Promise.resolve(),
    };

    return {
        client,
        emit: (entry: LlmApiLogIndexEntry) => handler?.(entry),
        gatePreview: (id: number) => void gatedPreviews.set(id, () => {}),
        releasePreview: (id: number) => {
            gatedPreviews.get(id)?.();
            gatedPreviews.delete(id);
        },
        setKeepError: (error: Error | null) => { keepError = error; },
        resolveIndex: (entries: LlmApiLogIndexEntry[]) => indexResolver?.(entries),
        rejectIndex: (error: Error) => indexRejecter?.(error),
    };
}

type LlmHarness = ReturnType<typeof createLlmHarness>;

function createLlmOptions(harness: LlmHarness, overrides: Partial<LlmApiLogsPanelOptions> = {}): LlmApiLogsPanelOptions {
    const base = {
        kind: 'llm-api' as const, initialKeep: 5, initialIndexEntries: [], initialPreview: null,
        client: harness.client, actions: createActions().actions, tr,
    };
    return { ...base, ...overrides };
}

function rows(container: HTMLElement): HTMLElement[] {
    return Array.from(container.querySelectorAll<HTMLElement>('.tt-dev-log-row'));
}

function entrySelect(container: HTMLElement): HTMLSelectElement {
    return within(container).getByRole<HTMLSelectElement>('combobox', { name: 'Log entry' });
}

test('mount enforces the public boundary and unmounts the root', () => {
    const harness = createLiveHarness();
    expect(() => mountTauriTavernDevLogsApp(null, createLiveOptions(harness)))
        .toThrow('TauriTavern dev logs mount element is required');
    const badOptions = (patch: object) => ({ ...createLiveOptions(harness), ...patch }) as never;
    const mountAt = (options: never) => () => mountTauriTavernDevLogsApp(document.createElement('div'), options);
    expect(mountAt(badOptions({ tr: undefined }))).toThrow('TauriTavern dev logs translator is required');
    expect(mountAt(badOptions({ kind: 'unknown' }))).toThrow('Unsupported TauriTavern dev logs panel: unknown');
    expect(mountAt(badOptions({ client: {} }))).toThrow('TauriTavern dev logs client method is unavailable: subscribe');

    const { container, handle } = mountApp(createLiveOptions(harness));
    expect(container.innerHTML).not.toBe('');
    close(handle);
    expect(container.innerHTML).toBe('');
});

test('subscription lifecycle: StrictMode keeps one active subscription, late resolve disposes, setup failure reports', async () => {
    const harness = createLiveHarness();
    const { handle } = mountApp(createLiveOptions(harness));
    await flushAct();
    // StrictMode ran the effect twice; the racing first subscription disposed itself.
    expect(harness.subscribeCalls()).toBe(2);
    expect(harness.activeSubscriptions()).toBe(1);

    close(handle);
    expect(harness.activeSubscriptions()).toBe(0);

    // A subscribe that resolves only after unmount disposes itself immediately.
    const delayedHarness = createLiveHarness();
    delayedHarness.delaySubscribe();
    const delayed = mountApp(createLiveOptions(delayedHarness));
    await flushAct();
    close(delayed.handle);
    await flushAct(() => delayedHarness.resolveDelayedSubscribe());
    expect(delayedHarness.activeSubscriptions()).toBe(0);

    // A rejected subscribe surfaces through reportError.
    const failingHarness = createLiveHarness();
    failingHarness.failSubscribe(new Error('stream unavailable'));
    const failingSpies = createActions();
    mountApp(createLiveOptions(failingHarness, { actions: failingSpies.actions }));
    await flushAct();
    expect(failingSpies.errors[0]?.message).toContain('stream unavailable');

    const messages: unknown[][] = [];
    const originalConsoleError = console.error;
    console.error = (...args: unknown[]) => { messages.push(args); };
    try {
        const teardownHarness = createLiveHarness({ failTeardown: true });
        const teardownMount = mountApp(createLiveOptions(teardownHarness));
        await flushAct();
        close(teardownMount.handle);
        await flushAct();
    } finally {
        console.error = originalConsoleError;
    }
    expect(messages.some(args => args[0] === 'TauriTavern dev log subscription teardown failed')).toBe(true);
});

test('live panel windows the tail, filters levels, pauses, clears and copies', async () => {
    const harness = createLiveHarness();
    const initial = Array.from({ length: 350 }, (_, i) => liveEntry(i + 1, {
        level: i % 7 === 0 ? 'error' : 'info',
    }));
    const spies = createActions();
    const { container } = mountApp(createLiveOptions(harness, {
        initialEntries: initial,
        actions: spies.actions,
        showConsoleCapture: true,
        trimEntriesInPlace: entries => {
            if (entries.length > 351) entries.splice(0, entries.length - 351);
        },
    }));
    const view = within(container);
    const status = () => container.querySelector('.tt-dev-log-status small')?.textContent;

    expect(rows(container)).toHaveLength(300);
    expect(rows(container)[0]?.textContent).toContain('msg-51');
    expect(status()).toBe('Showing 300/350');

    fireEvent.click(view.getByRole('button', { name: 'Show older' }));
    expect(rows(container)).toHaveLength(350);
    expect(view.queryByRole('button', { name: 'Show older' })).toBeNull();

    fireEvent.click(view.getByRole('button', { name: 'ERROR' }));
    expect(view.getByRole('button', { name: 'ERROR' }).className).toContain('active');
    expect(rows(container)).toHaveLength(50);
    expect(status()).toBe('Showing 50/50');
    fireEvent.click(view.getByRole('button', { name: 'ALL' }));
    expect(rows(container)).toHaveLength(350);

    fireEvent.click(view.getByRole('checkbox', { name: 'Pause' }));
    await flushAct(() => harness.emit(liveEntry(351)));
    expect(rows(container)).toHaveLength(350);
    expect(status()).toBe('Showing 350/350 · +1 new · Paused');
    fireEvent.click(view.getByRole('checkbox', { name: 'Pause' }));
    expect(rows(container)).toHaveLength(351);
    expect(status()).toBe('Showing 351/351');

    await flushAct(() => harness.emit(liveEntry(352)));
    expect(rows(container)).toHaveLength(351);
    expect(rows(container)[0]?.textContent).toContain('msg-2');
    expect(status()).toBe('Showing 351/351');

    fireEvent.click(view.getByRole('button', { name: 'Copy' }));
    const copied = spies.copied[0]?.split('\n') ?? [];
    expect(copied).toHaveLength(351);
    expect(copied[0]).toContain('msg-2');
    expect(copied[350]).toContain('msg-352');

    fireEvent.click(view.getByRole('button', { name: 'Clear' }));
    expect(rows(container)).toHaveLength(0);
    await flushAct(() => harness.emit(liveEntry(353)));
    expect(rows(container)).toHaveLength(1);
    expect(status()).toBe('Showing 1/1');
});

test('console capture toggle rolls back and reports when persistence fails', async () => {
    const harness = createLiveHarness({ failSetConsoleCapture: true });
    const spies = createActions();
    const { container } = mountApp(createLiveOptions(harness, {
        actions: spies.actions,
        showConsoleCapture: true,
        consoleCaptureEnabled: true,
    }));
    const toggle = within(container).getByRole<HTMLInputElement>('checkbox', { name: 'Capture full console logs' });
    expect(toggle.checked).toBe(true);

    fireEvent.click(toggle);
    expect(toggle.checked).toBe(false);
    await flushAct();
    expect(toggle.checked).toBe(true);
    expect(spies.errors[0]?.message).toContain('persist failed');
});

test('llm panel navigates by stable id: follow latest, historical pin and trim fallback', async () => {
    const harness = createLlmHarness();
    const { container } = mountApp(createLlmOptions(harness, {
        initialIndexEntries: [1, 2, 3, 4, 5].map(id => indexEntry(id)),
        initialPreview: previewOf(5),
    }));
    const view = within(container);
    await flushAct();
    const meta = () => container.querySelector('.tt-dev-log-meta')?.textContent;

    await flushAct(() => harness.emit(indexEntry(6)));
    expect(entrySelect(container).value).toBe('6');
    expect(entrySelect(container).options).toHaveLength(5);
    expect(meta()).toContain('source-6');

    fireEvent.click(view.getByRole('button', { name: 'Prev' }));
    await flushAct();
    expect(entrySelect(container).value).toBe('5');
    await flushAct(() => harness.emit(indexEntry(7)));
    expect(entrySelect(container).value).toBe('5');
    expect(meta()).toContain('source-5');

    await flushAct(() => {
        harness.emit(indexEntry(8));
        harness.emit(indexEntry(9));
        harness.emit(indexEntry(10));
    });
    expect(entrySelect(container).value).toBe('6');
    expect(entrySelect(container).options).toHaveLength(5);
    expect(meta()).toContain('source-6');
});

test('llm preview resolves stale loads against the current selection', async () => {
    const harness = createLlmHarness();
    harness.gatePreview(4);
    harness.gatePreview(5);
    const { container } = mountApp(createLlmOptions(harness, {
        initialIndexEntries: [4, 5].map(id => indexEntry(id)),
    }));
    const view = within(container);
    await flushAct();

    // Selection starts at tail id 5; navigate to 4 while 5 is still loading.
    fireEvent.click(view.getByRole('button', { name: 'Prev' }));
    expect(entrySelect(container).value).toBe('4');

    // The tail load resolves late and must not overwrite the pinned selection.
    await flushAct(() => harness.releasePreview(5));
    const sections = container.querySelectorAll<HTMLTextAreaElement>('.tt-dev-log-text-section textarea');
    expect(sections[0]?.value).toBe('Loading...');

    await flushAct(() => harness.releasePreview(4));
    expect(sections[0]?.value).toBe('request-4');
    expect(sections[1]?.value).toBe('response-4');
    expect(container.querySelector('.tt-dev-log-meta')?.textContent).toContain('source-4');
});

test('llm keep applies in two phases: failure commits nothing, reload failure keeps the new keep', async () => {
    const harness = createLlmHarness();
    const spies = createActions();
    const { container } = mountApp(createLlmOptions(harness, {
        initialIndexEntries: [1, 2, 3, 4, 5].map(id => indexEntry(id)),
        initialPreview: previewOf(5),
        actions: spies.actions,
    }));
    const view = within(container);
    await flushAct();
    const keepInput = view.getByRole<HTMLInputElement>('spinbutton', { name: 'LLM API keep' });

    fireEvent.change(keepInput, { target: { value: '3.5' } });
    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    expect(spies.errors[0]?.message).toContain('positive number');
    expect(entrySelect(container).options).toHaveLength(5);

    harness.setKeepError(new Error('persist keep failed'));
    fireEvent.change(keepInput, { target: { value: '3' } });
    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    await flushAct();
    expect(spies.errors).toHaveLength(2);
    expect(entrySelect(container).options).toHaveLength(5);
    expect(keepInput.value).toBe('3');

    // Successful setKeep trims locally at once; a failing index reload reports
    // but never reverts the persisted keep.
    harness.setKeepError(null);
    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    await flushAct();
    expect(entrySelect(container).options).toHaveLength(3);
    expect(container.querySelector('.tt-dev-log-meta')?.textContent).toContain('source-5');
    await flushAct(() => harness.rejectIndex(new Error('index reload failed')));
    expect(spies.errors).toHaveLength(3);
    expect(entrySelect(container).options).toHaveLength(3);
});

test('llm keep reload trusts the host snapshot, merges raced events and dedupes late delivery', async () => {
    const harness = createLlmHarness();
    const { container } = mountApp(createLlmOptions(harness, {
        initialIndexEntries: [1, 2, 3, 4, 5].map(id => indexEntry(id)),
        initialPreview: previewOf(5),
    }));
    const view = within(container);
    await flushAct();
    const optionIds = () => Array.from(entrySelect(container).options, option => Number(option.value));

    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    await flushAct();
    await flushAct(() => harness.resolveIndex([2, 3, 4, 5, 6].map(id => indexEntry(id))));
    expect(optionIds()).toEqual([6, 5, 4, 3, 2]);
    expect(entrySelect(container).value).toBe('6');

    // The matching event may arrive after the snapshot response.
    await flushAct(() => harness.emit(indexEntry(6)));
    expect(optionIds()).toEqual([6, 5, 4, 3, 2]);

    // An event arriving while the next snapshot is pending is appended once.
    fireEvent.click(view.getByRole('button', { name: 'Apply' }));
    await flushAct();
    await flushAct(() => harness.emit(indexEntry(7)));
    await flushAct(() => harness.resolveIndex([2, 3, 4, 5, 6].map(id => indexEntry(id))));
    expect(optionIds()).toEqual([7, 6, 5, 4, 3]);
    expect(entrySelect(container).value).toBe('7');
});

test('llm text viewer carries readable and raw title, text and wrap contracts', async () => {
    const harness = createLlmHarness();
    const spies = createActions();
    const { container } = mountApp(createLlmOptions(harness, {
        initialIndexEntries: [7].map(id => indexEntry(id)),
        initialPreview: previewOf(7),
        actions: spies.actions,
    }));
    await flushAct();

    const sections = container.querySelectorAll<HTMLElement>('.tt-dev-log-text-section');
    fireEvent.click(within(sections.item(0)).getByRole('button', { name: 'Request body' }));
    await flushAct();
    expect(spies.viewed).toEqual([{ title: 'Request body', text: 'request-7', wrap: 'soft' }]);

    const details = container.querySelector<HTMLDetailsElement>('details.tt-dev-log-raw');
    if (!details) throw new Error('raw disclosure not found');
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await flushAct();

    const rawSections = details.querySelectorAll<HTMLElement>('.tt-dev-log-text-section');
    fireEvent.click(within(rawSections.item(0)).getByRole('button', { name: 'Raw JSON/SSE - Request body' }));
    await flushAct();
    expect(spies.viewed[1]).toEqual({ title: 'Raw JSON/SSE - Request body', text: 'raw-request-7', wrap: 'off' });
});
