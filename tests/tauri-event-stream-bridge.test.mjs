import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

async function createBridge() {
    const { createTauriEventStreamBridge } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/services/dev-logging/tauri-event-stream-bridge.js'),
    );

    /** @type {{ type: 'invoke', enabled: boolean } | { type: 'listen' } | { type: 'unlisten' }[]} */
    const calls = [];
    /** @type {((event: { payload: unknown }) => void) | null} */
    let eventHandler = null;
    /** @type {Promise<void> | null} */
    let disableGate = null;
    /** @type {Error | null} */
    let listenFailure = null;

    const bridge = createTauriEventStreamBridge({
        safeInvoke: async (_command, args) => {
            calls.push({ type: 'invoke', enabled: Boolean(args?.enabled) });
            if (args?.enabled === false && disableGate) {
                const gate = disableGate;
                disableGate = null;
                await gate;
            }
        },
        enableCommand: 'cmd',
        eventName: 'evt',
        listen: async (_eventName, handler) => {
            if (listenFailure) {
                const error = listenFailure;
                listenFailure = null;
                throw error;
            }
            calls.push({ type: 'listen' });
            eventHandler = handler;
            return () => {
                calls.push({ type: 'unlisten' });
                eventHandler = null;
            };
        },
    });

    return {
        bridge,
        calls,
        emit: (payload) => eventHandler?.({ payload }),
        setDisableGate: (gate) => {
            disableGate = gate;
        },
        failNextListen: (error) => {
            listenFailure = error;
        },
    };
}

const enableCalls = (calls) => calls.filter((call) => call.type === 'invoke' && call.enabled).length;
const disableCalls = (calls) => calls.filter((call) => call.type === 'invoke' && !call.enabled).length;

test('a rejected start removes the ghost subscriber instead of dispatching to it later', async () => {
    const { bridge, calls, emit, failNextListen } = await createBridge();

    const ghostCalls = [];
    failNextListen(new Error('listen unavailable'));
    await assert.rejects(bridge.subscribe((entry) => ghostCalls.push(entry)), /listen unavailable/);

    const liveCalls = [];
    const dispose = await bridge.subscribe((entry) => liveCalls.push(entry));
    emit('entry');

    assert.deepEqual(ghostCalls, []);
    assert.deepEqual(liveCalls, ['entry']);
    // The failed start ran its compensating disable; the successful start re-enabled once.
    assert.deepEqual(calls.map((call) => call.type === 'invoke' ? `invoke:${call.enabled}` : call.type), [
        'invoke:true',
        'invoke:false',
        'invoke:true',
        'listen',
    ]);
    await dispose();
});

test('multiple subscribers share one stream and only the last unsubscribe stops it', async () => {
    const { bridge, calls, emit } = await createBridge();

    const first = [];
    const second = [];
    const disposeFirst = await bridge.subscribe((entry) => first.push(entry));
    const disposeSecond = await bridge.subscribe((entry) => second.push(entry));
    assert.equal(enableCalls(calls), 1);

    emit('a');
    assert.deepEqual(first, ['a']);
    assert.deepEqual(second, ['a']);

    await disposeFirst();
    assert.equal(disableCalls(calls), 0);
    emit('b');
    assert.deepEqual(first, ['a']);
    assert.deepEqual(second, ['a', 'b']);

    await disposeSecond();
    assert.equal(disableCalls(calls), 1);
    assert.equal(calls.at(-1)?.type, 'invoke');
});

test('a subscribe racing the last unsubscribe cannot overtake the in-flight disable', async () => {
    const { bridge, calls, emit, setDisableGate } = await createBridge();

    const disposeFirst = await bridge.subscribe(() => {});

    /** @type {() => void} */
    let releaseDisable = () => {};
    setDisableGate(new Promise((resolve) => {
        releaseDisable = resolve;
    }));

    const stopPromise = disposeFirst();
    const secondCalls = [];
    const subscribePromise = bridge.subscribe((entry) => secondCalls.push(entry));

    // The disable is still gated; the re-enable must be queued behind it.
    await Promise.resolve();
    assert.equal(calls.filter((call) => call.type === 'invoke').length, 2);

    releaseDisable();
    await stopPromise;
    const disposeSecond = await subscribePromise;

    const sequence = calls.map((call) => call.type === 'invoke' ? `invoke:${call.enabled}` : call.type);
    const lastDisable = sequence.lastIndexOf('invoke:false');
    const lastEnable = sequence.lastIndexOf('invoke:true');
    assert.ok(lastDisable > -1 && lastEnable > lastDisable, `disable must land before re-enable: ${sequence}`);

    emit('after-race');
    assert.deepEqual(secondCalls, ['after-race']);
    await disposeSecond();
});

test('disposers are idempotent and resolve with the in-flight stop', async () => {
    const { bridge, calls } = await createBridge();

    const dispose = await bridge.subscribe(() => {});
    const firstStop = dispose();
    const secondStop = dispose();
    await Promise.all([firstStop, secondStop, dispose()]);

    assert.equal(disableCalls(calls), 1);
    assert.equal(calls.filter((call) => call.type === 'unlisten').length, 1);
});
