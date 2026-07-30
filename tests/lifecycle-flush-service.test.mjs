import test from 'node:test';
import assert from 'node:assert/strict';

import {
    createLifecycleFlushService,
    TAURI_GRACEFUL_EXIT_EVENT,
} from '../src/tauri/main/services/lifecycle/lifecycle-flush-service.js';

class FakeEventTarget {
    constructor() {
        this.listeners = new Map();
    }

    addEventListener(type, listener) {
        const listeners = this.listeners.get(type) ?? new Set();
        listeners.add(listener);
        this.listeners.set(type, listeners);
    }

    removeEventListener(type, listener) {
        this.listeners.get(type)?.delete(listener);
    }

    dispatch(type) {
        for (const listener of this.listeners.get(type) ?? []) {
            listener({ type });
        }
    }

    listenerCount(type) {
        return this.listeners.get(type)?.size ?? 0;
    }
}

test('host lifecycle listeners are installed once and fan out to registered flushers', async () => {
    const windowObject = new FakeEventTarget();
    const documentObject = new FakeEventTarget();
    documentObject.visibilityState = 'visible';
    const calls = [];
    const service = createLifecycleFlushService({ windowObject, documentObject, logger: { error() {} } });

    service.register('session', reason => calls.push(`session:${reason}`));
    service.register('invokes', reason => calls.push(`invokes:${reason}`));
    service.install();
    service.install();

    assert.equal(windowObject.listenerCount('pagehide'), 1);
    assert.equal(windowObject.listenerCount('beforeunload'), 1);
    assert.equal(documentObject.listenerCount('visibilitychange'), 1);

    windowObject.dispatch('pagehide');
    await service.waitForIdle();
    assert.deepEqual(calls, ['session:pagehide', 'invokes:pagehide']);

    documentObject.visibilityState = 'hidden';
    documentObject.dispatch('visibilitychange');
    await service.waitForIdle();
    assert.deepEqual(calls.slice(-2), ['session:visibilitychange:hidden', 'invokes:visibilitychange:hidden']);
});

test('install and uninstall are idempotent across repeated cycles', () => {
    const windowObject = new FakeEventTarget();
    const documentObject = new FakeEventTarget();
    const service = createLifecycleFlushService({ windowObject, documentObject, logger: { error() {} } });

    service.install();
    service.install();
    service.uninstall();
    service.uninstall();

    assert.equal(windowObject.listenerCount('pagehide'), 0);
    assert.equal(windowObject.listenerCount('beforeunload'), 0);
    assert.equal(documentObject.listenerCount('visibilitychange'), 0);

    service.install();
    assert.equal(windowObject.listenerCount('pagehide'), 1);
    service.uninstall();
    assert.equal(windowObject.listenerCount('pagehide'), 0);
});

test('one failing lifecycle flusher does not block the remaining handlers', async () => {
    const windowObject = new FakeEventTarget();
    const documentObject = new FakeEventTarget();
    const errors = [];
    const calls = [];
    const service = createLifecycleFlushService({
        windowObject,
        documentObject,
        logger: { error: (...args) => errors.push(args) },
    });

    service.register('broken', () => {
        throw new Error('broken');
    });
    service.register('healthy', reason => calls.push(reason));

    await assert.rejects(service.flush('test'), /broken/);

    assert.deepEqual(calls, ['test']);
    assert.equal(errors.length, 1);
});

test('native exit waits for a successful flush before destroying the window', async () => {
    const windowObject = new FakeEventTarget();
    const documentObject = new FakeEventTarget();
    let nativeExitHandler;
    let destroyed = false;
    let releaseFlush;
    const flushBlock = new Promise(resolve => {
        releaseFlush = resolve;
    });

    windowObject.__TAURI__ = {
        event: {
            listen: async (event, handler) => {
                assert.equal(event, TAURI_GRACEFUL_EXIT_EVENT);
                nativeExitHandler = handler;
                return () => {};
            },
        },
        window: {
            getCurrentWindow: () => ({
                destroy: async () => {
                    destroyed = true;
                },
            }),
        },
    };

    const service = createLifecycleFlushService({ windowObject, documentObject, logger: { error() {} } });
    service.register('settings', () => flushBlock);
    service.install();
    await Promise.resolve();

    const exitTask = nativeExitHandler();
    await Promise.resolve();
    assert.equal(destroyed, false);

    releaseFlush();
    await exitTask;
    assert.equal(destroyed, true);
});

test('native exit keeps the window open when persistence fails', async () => {
    const windowObject = new FakeEventTarget();
    const documentObject = new FakeEventTarget();
    const errors = [];
    let nativeExitHandler;
    let destroyed = false;

    windowObject.__TAURI__ = {
        event: {
            listen: async (_event, handler) => {
                nativeExitHandler = handler;
                return () => {};
            },
        },
        window: {
            getCurrentWindow: () => ({
                destroy: async () => {
                    destroyed = true;
                },
            }),
        },
    };

    const service = createLifecycleFlushService({
        windowObject,
        documentObject,
        logger: { error: (...args) => errors.push(args) },
    });
    service.register('settings', () => Promise.reject(new Error('disk full')));
    service.install();
    await Promise.resolve();

    await nativeExitHandler();

    assert.equal(destroyed, false);
    assert.match(String(errors.at(-1)?.[0]), /keeping the app open/);
});

test('concurrent flush calls reuse the in-flight promise and run handlers once', async () => {
    const service = createLifecycleFlushService({
        windowObject: new FakeEventTarget(),
        documentObject: new FakeEventTarget(),
        logger: { error() {} },
    });
    const calls = [];
    let resolveHandler;
    const handlerBlock = new Promise(resolve => {
        resolveHandler = resolve;
    });

    service.register('session', async reason => {
        calls.push(reason);
        await handlerBlock;
    });

    const firstFlush = service.flush('first');
    const concurrentFlush = service.flush('second');
    assert.strictEqual(firstFlush, concurrentFlush);

    await Promise.resolve();
    assert.deepEqual(calls, ['first']);

    resolveHandler();
    await firstFlush;

    const nextFlush = service.flush('third');
    assert.notStrictEqual(firstFlush, nextFlush);
    await nextFlush;
    assert.deepEqual(calls, ['first', 'third']);
});

test('waitForIdle resolves after the active lifecycle flush completes', async () => {
    const service = createLifecycleFlushService({
        windowObject: new FakeEventTarget(),
        documentObject: new FakeEventTarget(),
        logger: { error() {} },
    });
    const calls = [];

    service.register('async-handler', async reason => {
        calls.push(`start:${reason}`);
        await Promise.resolve();
        calls.push(`end:${reason}`);
    });

    service.flush('test');
    await service.waitForIdle();

    assert.deepEqual(calls, ['start:test', 'end:test']);
});

test('higher priority lifecycle flushers run after pending state producers', async () => {
    const service = createLifecycleFlushService({
        windowObject: new FakeEventTarget(),
        documentObject: new FakeEventTarget(),
        logger: { error() {} },
    });
    const calls = [];

    service.register('invoke-broker', () => calls.push('invoke'), { priority: 100 });
    service.register('session-state', async () => {
        calls.push('session:start');
        await Promise.resolve();
        calls.push('session:end');
    });

    await service.flush('test');

    assert.deepEqual(calls, ['session:start', 'session:end', 'invoke']);
});

test('re-registering a lifecycle flusher replaces the previous implementation', async () => {
    const service = createLifecycleFlushService({
        windowObject: new FakeEventTarget(),
        documentObject: new FakeEventTarget(),
        logger: { error() {} },
    });
    const calls = [];

    service.register('session', () => calls.push('old'));
    const unregister = service.register('session', () => calls.push('new'));
    await service.flush('test');
    unregister();
    await service.flush('test');

    assert.deepEqual(calls, ['new']);
});
