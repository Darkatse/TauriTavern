import { act, fireEvent, within } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';

import { mountTauriTavernDevLogsApp } from './DevLogsApp';
import type { DevLogsActions, DevLogsHandle, DevLogsMountOptions, LiveLogEntry } from './DevLogsContract';

declare global {
    // The mount creates its React root directly, so act() needs the opt-in.
    var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const tr = (key: string) => key;
const handles: DevLogsHandle[] = [];

afterEach(() => {
    for (const handle of handles.splice(0)) act(() => handle.unmount());
});

function createActions(overrides: Partial<DevLogsActions> = {}) {
    const copied: string[] = [];
    const errors: Error[] = [];
    const actions: DevLogsActions = {
        copyText: (text) => { copied.push(text); return Promise.resolve(); },
        reportError: (error) => {
            if (!(error instanceof Error)) throw new TypeError('expected an Error');
            errors.push(error);
        },
        ...overrides,
    };
    return { actions, copied, errors };
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

    const client: DevLogsMountOptions['client'] = {
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

function createLiveOptions(harness: LiveHarness, overrides: Partial<DevLogsMountOptions> = {}): DevLogsMountOptions {
    const base: DevLogsMountOptions = {
        title: 'Frontend Logs', initialEntries: [], consoleCaptureEnabled: false,
        showConsoleCapture: false, trimEntriesInPlace: null, client: harness.client, actions: createActions().actions, tr,
    };
    return { ...base, ...overrides };
}

function mountApp(options: DevLogsMountOptions) {
    const container = document.createElement('div');
    document.body.append(container);
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

function rows(container: HTMLElement): HTMLElement[] {
    return Array.from(container.querySelectorAll<HTMLElement>('.tt-dev-log-row'));
}

test('mount enforces the public boundary and unmounts the root', () => {
    const harness = createLiveHarness();
    expect(() => mountTauriTavernDevLogsApp(null, createLiveOptions(harness)))
        .toThrow('TauriTavern dev logs mount element is required');
    const badOptions = (patch: object): unknown => ({ ...createLiveOptions(harness), ...patch });
    const mountAt = (options: unknown) => () => mountTauriTavernDevLogsApp(document.createElement('div'), options);
    expect(mountAt(badOptions({ tr: undefined }))).toThrow('TauriTavern dev logs translator is required');
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
    expect(messages.some(args => args[0] === 'TauriTavern host subscription teardown failed')).toBe(true);
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
