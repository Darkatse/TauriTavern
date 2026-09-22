import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const APP_ORIGIN = 'http://localhost:8000';
const LONG_PRESS_DELAY_MS = 500;

const state = {
    hostInvokes: [],
    nativeSaves: [],
    consoleErrors: [],
    contentTypeBySource: new Map(),
    failingSources: new Set(),
};

function resetState() {
    state.hostInvokes.length = 0;
    state.nativeSaves.length = 0;
    state.consoleErrors.length = 0;
    state.contentTypeBySource.clear();
    state.failingSources.clear();
}

function setUserAgent(userAgent) {
    Object.defineProperty(globalThis, 'navigator', {
        value: { userAgent, language: 'en' },
        configurable: true,
    });
}

function flushAsyncWork() {
    // The save path is a chain of already-resolved promises; macrotask boundaries flush all of them
    // without depending on timing.
    return new Promise((resolve) => setImmediate(resolve))
        .then(() => new Promise((resolve) => setImmediate(resolve)))
        .then(() => new Promise((resolve) => setImmediate(resolve)));
}

globalThis.localStorage = { getItem: () => null };
globalThis.MutationObserver = class {
    observe() {}
    disconnect() {}
};

globalThis.console.error = (...args) => {
    state.consoleErrors.push(args.map(String).join(' '));
};

globalThis.fetch = async (url) => {
    const source = String(url);
    if (state.failingSources.has(source)) {
        throw new TypeError('Failed to fetch');
    }

    const type = state.contentTypeBySource.get(source) || 'image/png';
    return { ok: true, status: 200, blob: async () => new Blob([new Uint8Array([1, 2, 3])], { type }) };
};

globalThis.window = {
    location: { origin: APP_ORIGIN, href: `${APP_ORIGIN}/` },
    TauriTavernAndroidPublicDownloadBridge: {
        supportsDirectPublicDownloads: () => true,
        saveFileToDownloads: (sourcePath, displayName, mimeType) => {
            state.nativeSaves.push({ sourcePath, displayName, mimeType });
            return JSON.stringify({
                saved_path: `/storage/emulated/0/Download/${displayName}`,
                uri: 'content://downloads/1',
                display_name: displayName,
            });
        },
    },
    __TAURI__: {
        path: {
            join: async (...parts) => parts.join('/'),
            appCacheDir: async () => '/cache',
        },
        core: {
            invoke: async (command, args) => {
                if (command === 'download_remote_image') {
                    state.hostInvokes.push(args);
                    return { data: [4, 5, 6, 7], mimeType: 'image/webp', fileName: 'host-name.webp' };
                }

                return undefined;
            },
        },
    },
};

const modulePath = path.join(
    REPO_ROOT,
    'src/tauri/main/compat/mobile/mobile-image-long-press-save.js',
);
const { installMobileImageLongPressSave } = await import(pathToFileURL(modulePath).href);

function createDocument() {
    const listeners = new Map();
    return {
        addEventListener(type, handler) {
            if (!listeners.has(type)) {
                listeners.set(type, []);
            }
            listeners.get(type).push(handler);
        },
        dispatch(type, event) {
            for (const handler of listeners.get(type) || []) {
                handler(event);
            }
        },
        listenerCount(type) {
            return (listeners.get(type) || []).length;
        },
    };
}

function createImage(source) {
    const element = {
        nodeType: 1,
        tagName: 'IMG',
        currentSrc: source,
        src: '',
        isConnected: true,
        closest: (selector) => (selector === 'img' ? element : null),
        contains: (node) => node === element,
    };
    return element;
}

function touchEvent(target, { x = 10, y = 10, count = 1 } = {}) {
    return {
        target,
        touches: Array.from({ length: count }, () => ({ clientX: x, clientY: y })),
    };
}

async function startLongPress(targetDocument, target, timers) {
    targetDocument.dispatch('touchstart', touchEvent(target));
    timers.tick(LONG_PRESS_DELAY_MS);
    await flushAsyncWork();
}

test('a long press saves an image the page can read, naming it after the payload type', async (t) => {
    resetState();
    setUserAgent('Mozilla/5.0 (Linux; Android 15)');
    t.mock.timers.enable({ apis: ['setTimeout'] });

    const document = createDocument();
    const source = `${APP_ORIGIN}/user/images/abc`;
    state.contentTypeBySource.set(source, 'image/jpeg');
    installMobileImageLongPressSave({ document });

    await startLongPress(document, createImage(source), t.mock.timers);

    assert.deepEqual(state.hostInvokes, [], 'a readable image must not reach the host');
    assert.equal(state.nativeSaves.length, 1);
    assert.match(state.nativeSaves[0].displayName, /^image-\d+\.jpg$/);
    assert.equal(state.nativeSaves[0].mimeType, 'image/jpeg');
});

test('a cross-origin image the page cannot read falls back to the host and keeps its name and MIME', async (t) => {
    resetState();
    setUserAgent('Mozilla/5.0 (Linux; Android 15)');
    t.mock.timers.enable({ apis: ['setTimeout'] });

    const document = createDocument();
    const source = 'https://cdn.example.com/pic';
    state.failingSources.add(source);
    installMobileImageLongPressSave({ document });

    await startLongPress(document, createImage(source), t.mock.timers);

    assert.deepEqual(state.hostInvokes, [{ url: source }]);
    assert.equal(state.nativeSaves.length, 1);
    assert.equal(state.nativeSaves[0].displayName, 'host-name.webp');
    assert.equal(state.nativeSaves[0].mimeType, 'image/webp');
});

test('a same-origin failure is not proxied through the host and is reported', async (t) => {
    resetState();
    setUserAgent('Mozilla/5.0 (Linux; Android 15)');
    t.mock.timers.enable({ apis: ['setTimeout'] });

    const document = createDocument();
    const source = `${APP_ORIGIN}/user/images/missing.png`;
    state.failingSources.add(source);
    installMobileImageLongPressSave({ document });

    await startLongPress(document, createImage(source), t.mock.timers);

    assert.deepEqual(state.hostInvokes, [], 'the host has no route for the app\'s own URLs');
    assert.deepEqual(state.nativeSaves, []);
    assert.ok(
        state.consoleErrors.some((message) => message.includes('Image source is not readable')),
        'the failure must be reported',
    );
});

test('the tap ending a long press is swallowed exactly once', async (t) => {
    resetState();
    setUserAgent('Mozilla/5.0 (Linux; Android 15)');
    t.mock.timers.enable({ apis: ['setTimeout'] });

    const document = createDocument();
    const target = createImage(`${APP_ORIGIN}/user/images/abc`);
    installMobileImageLongPressSave({ document });

    await startLongPress(document, target, t.mock.timers);

    let prevented = 0;
    const clickEvent = {
        target,
        preventDefault() { prevented += 1; },
        stopImmediatePropagation() {},
    };

    document.dispatch('click', clickEvent);
    assert.equal(prevented, 1, 'the trailing tap must not reach the image click handlers');
    document.dispatch('click', clickEvent);
    assert.equal(prevented, 1, 'later taps must keep working');
});

test('dragging, lifting and multi-touch all cancel the gesture', async (t) => {
    resetState();
    setUserAgent('Mozilla/5.0 (Linux; Android 15)');
    t.mock.timers.enable({ apis: ['setTimeout'] });

    const document = createDocument();
    installMobileImageLongPressSave({ document });

    const target = createImage(`${APP_ORIGIN}/user/images/abc`);

    document.dispatch('touchstart', touchEvent(target));
    document.dispatch('touchmove', touchEvent(target, { x: 80 }));
    t.mock.timers.tick(LONG_PRESS_DELAY_MS);
    await flushAsyncWork();

    document.dispatch('touchstart', touchEvent(target));
    document.dispatch('touchend', {});
    t.mock.timers.tick(LONG_PRESS_DELAY_MS);
    await flushAsyncWork();

    document.dispatch('touchstart', touchEvent(target, { count: 2 }));
    t.mock.timers.tick(LONG_PRESS_DELAY_MS);
    await flushAsyncWork();

    assert.deepEqual(state.nativeSaves, []);
    assert.deepEqual(state.hostInvokes, []);
});

test('runtimes that already own an image menu are left untouched, and install is idempotent', () => {
    resetState();

    // Only Android installs the gesture: every other runtime already owns an image menu (desktop
    // WebView context menu, iOS system callout). iOS is asserted because the module is invoked on
    // it too: the mobile compat installer runs on every mobile runtime.
    for (const userAgent of [
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
        'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)',
        'Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X)',
    ]) {
        setUserAgent(userAgent);
        const document = createDocument();
        installMobileImageLongPressSave({ document });
        assert.equal(document.listenerCount('touchstart'), 0, userAgent);
    }

    setUserAgent('Mozilla/5.0 (Linux; Android 15)');
    const androidDocument = createDocument();
    installMobileImageLongPressSave({ document: androidDocument });
    installMobileImageLongPressSave({ document: androidDocument });
    assert.equal(androidDocument.listenerCount('touchstart'), 1);
    assert.equal(androidDocument.listenerCount('click'), 1);
});

test('an empty host payload fails loudly instead of saving nothing', async (t) => {
    resetState();
    setUserAgent('Mozilla/5.0 (Linux; Android 15)');
    t.mock.timers.enable({ apis: ['setTimeout'] });

    const document = createDocument();
    const source = 'https://cdn.example.com/empty';
    state.failingSources.add(source);
    const previousInvoke = globalThis.window.__TAURI__.core.invoke;
    globalThis.window.__TAURI__.core.invoke = async (command) => {
        if (command === 'download_remote_image') {
            return { data: [], mimeType: 'image/png', fileName: 'x.png' };
        }

        return undefined;
    };

    try {
        installMobileImageLongPressSave({ document });
        await startLongPress(document, createImage(source), t.mock.timers);
    } finally {
        globalThis.window.__TAURI__.core.invoke = previousInvoke;
    }

    assert.deepEqual(state.nativeSaves, []);
    assert.ok(
        state.consoleErrors.some((message) => message.includes('Host returned an empty image payload')),
        'an empty payload must not produce a file',
    );
});
