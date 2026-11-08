import { HOSTS } from './host-identity.mjs';
import { createWebHostFixture } from './web-host-fixture.mjs';

// The test server serves src/ at the origin root, as the native host serves it to the WebView. Frontend code is
// loaded from there, never bundled into the test.

/**
 * @param {string} path relative to src/
 * @param {Window} [realm]
 * @returns {Promise<unknown>}
 */
export function importFrontend(path, realm = window) {
    return realm.eval(`import(${JSON.stringify(`/${path}`)})`);
}

export async function frontendText(path) {
    const response = await fetch(`/${path}`);
    if (!response.ok) throw new Error(`No frontend file at ${path}: ${response.status}`);
    return response.text();
}

// Assertions depend on the order timers fire in, not their length, so delays are divided rather than capped: a cap
// would put a 200 ms debounce and a 1 s save on one deadline and stop the debounce from coalescing two saves.
const TIMER_SCALE = 10;

function scaleTimers(window) {
    // The frame's own code, so errors thrown by timer callbacks reach the frame like any others.
    const pending = new window.Function('scale', `
        const pending = new Set();
        const schedule = setTimeout, cancel = clearTimeout;
        window.setTimeout = (callback, delay = 0, ...args) => {
            const id = schedule(() => {
                pending.delete(id);
                callback(...args);
            }, Math.ceil(delay / scale));
            pending.add(id);
            return id;
        };
        window.clearTimeout = id => {
            pending.delete(id);
            cancel(id);
        };
        return pending;
    `)(TIMER_SCALE);
    return async function waitUntilComplete() {
        do await new Promise(resolve => setTimeout(resolve, 0));
        while (pending.size);
    };
}

export function createPaginationWaiter(window) {
    const $ = window.jQuery;
    const pagination = $.fn.pagination;
    let rendered;
    $.fn.pagination = function (options, ...args) {
        if (options && typeof options === 'object') {
            const callback = options.callback;
            options.callback = (...args) => {
                const pending = callback(...args);
                Promise.resolve(pending).then(rendered.resolve, rendered.reject);
                return pending;
            };
        }
        return pagination.call(this, options, ...args);
    };
    Object.assign($.fn.pagination, pagination);
    return async action => {
        rendered = Promise.withResolvers();
        await action();
        await rendered.promise;
    };
}

export function captureBrowserErrors(window) {
    const errors = [];
    const failed = Promise.withResolvers();
    const record = error => { errors.push(error); failed.reject(error); };
    window.addEventListener('error', event => record(event.error ?? event.message));
    window.addEventListener('unhandledrejection', event => record(event.reason));
    window.toastr.error = (...args) => record(new Error(args.join(' ')));
    return { errors, failed: failed.promise };
}

// The fixture is the backend: it lives in the test page and outlives application frames, as the native host
// outlives a WebView reload.
export async function createFrontend({ identity = HOSTS.windows, fixture } = {}) {
    const frame = document.createElement('iframe');
    frame.src = '/__test/frontend.html';
    const loaded = new Promise(resolve => frame.addEventListener('load', resolve, { once: true }));
    // Outside <body>: the tests of a file may share one application, and each test empties the body.
    document.documentElement.append(frame);
    await loaded;
    const window = /** @type {Window} */ (frame.contentWindow);
    // index.html sets <base href="/">; jQuery UI Tabs loads hash tabs as remote documents unless the location matches.
    window.history.replaceState(null, '', '/');
    const waitUntilComplete = scaleTimers(window);
    const load = path => importFrontend(path, window);

    if (!fixture) {
        const response = await fetch('/__test/frontend-content.json');
        const { settings, theme, extensions } = await response.json();
        fixture = createWebHostFixture(settings, theme, [...extensions, { name: 'third-party/web-host-fixture', type: 'local' }]);
    }
    const { installWebHost } = await load('tauri/web-host/install.js');
    const host = installWebHost({ identity, backend: fixture.backend, store: fixture.store });

    async function startHost() {
        await load('lib.js');
        await load('tauri-main.js');
        await window.__TAURITAVERN_MAIN_READY__;
    }

    async function initializeChatRendering() {
        const main = await load('script.js');
        main.reloadMarkdownProcessor();
        (await load('tauri/main/services/chat-surface/chat-virtualization-state.js'))
            .initializeChatVirtualization({ chat_virtualization_enabled: false });
        await (await load('scripts/system-messages.js')).initSystemMessages();
        return main;
    }

    return { window, host, fixture, load, startHost, initializeChatRendering, waitUntilComplete, close: () => frame.remove() };
}
