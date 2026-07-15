import test from 'node:test';
import assert from 'node:assert/strict';

import { createLifecycleFlushService } from '../src/tauri/main/services/lifecycle/lifecycle-flush-service.js';

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

    await service.flush('test');

    assert.deepEqual(calls, ['test']);
    assert.equal(errors.length, 1);
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
