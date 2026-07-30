import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const BRIDGE_PATH = path.join(REPO_ROOT, 'src/tauri/main/bootstrap/backend-error-bridge.js');

async function importFreshBridge() {
    const url = `${pathToFileURL(BRIDGE_PATH).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function installFakeWindow({ invoke, consumerReady = false }) {
    const dispatched = [];
    let listener = null;
    const calls = [];

    global.CustomEvent = class CustomEvent {
        constructor(type, options = {}) {
            this.type = type;
            this.detail = options.detail;
        }
    };

    global.window = {
        __TAURI__: {
            event: {
                async listen(eventName, callback) {
                    calls.push(['listen', eventName]);
                    listener = callback;
                    return () => {};
                },
            },
            core: {
                async invoke(command) {
                    calls.push(['invoke', command]);
                    return invoke(command);
                },
            },
        },
        __TAURITAVERN_BACKEND_ERROR_CONSUMER_READY__: consumerReady,
        dispatchEvent(event) {
            dispatched.push(event);
        },
    };

    return {
        calls,
        dispatched,
        get listener() {
            return listener;
        },
        window: global.window,
    };
}

function cleanupGlobals() {
    delete global.window;
    delete global.CustomEvent;
}

test('backend error bridge registers listener before draining pending errors', async () => {
    const fake = installFakeWindow({
        invoke(command) {
            assert.equal(command, 'backend_error_bridge_ready');
            return [' first ', { message: 'second' }, '', { message: '   ' }];
        },
    });

    try {
        const { installBackendErrorBridge } = await importFreshBridge();
        await installBackendErrorBridge();

        assert.equal(typeof fake.listener, 'function');
        assert.deepEqual(fake.calls, [
            ['listen', 'tauritavern-backend-error'],
            ['invoke', 'backend_error_bridge_ready'],
        ]);
        assert.deepEqual(fake.window.__TAURITAVERN_BACKEND_ERROR_QUEUE__, [
            { message: 'first' },
            { message: 'second' },
        ]);
    } finally {
        cleanupGlobals();
    }
});

test('backend error bridge forwards runtime events after consumer is ready', async () => {
    const fake = installFakeWindow({
        consumerReady: true,
        invoke() {
            return [];
        },
    });

    try {
        const { installBackendErrorBridge } = await importFreshBridge();
        await installBackendErrorBridge();

        fake.listener({ payload: ' later ' });

        assert.equal(fake.dispatched.length, 1);
        assert.equal(fake.dispatched[0].type, 'tauritavern:backend-error');
        assert.deepEqual(fake.dispatched[0].detail, { message: 'later' });
    } finally {
        cleanupGlobals();
    }
});

test('backend error bridge fails fast when ready command fails', async () => {
    const fake = installFakeWindow({
        invoke() {
            throw new Error('ready failed');
        },
    });

    try {
        const { installBackendErrorBridge } = await importFreshBridge();
        await assert.rejects(installBackendErrorBridge(), /ready failed/);
        assert.equal(typeof fake.listener, 'function');
    } finally {
        cleanupGlobals();
    }
});
