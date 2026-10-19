import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import vm from 'node:vm';
import { Window } from 'happy-dom';
import { HOSTS } from '../helpers/host-identity.mjs';
import { createWebHostFixture } from './web-host-fixture.mjs';

const root = fileURLToPath(new URL('../../src/', import.meta.url));
const defaultSettings = JSON.parse(readFileSync(new URL('../../default/content/settings.json', import.meta.url), 'utf8'));
const defaultTheme = JSON.parse(readFileSync(new URL('../../default/content/themes/Dark Lite.json', import.meta.url), 'utf8'));

// These tests drive the application through happyDOM.waitUntilComplete(), which drains its
// pending timers in real time: one navigation run schedules ~57 s of nominal delay, mostly
// 1 s save debounces and 200 ms utility delays.
//
// What the assertions actually depend on is the order these timers fire in, not their length,
// so every delay is divided rather than capped. happy-dom's own timer.maxTimeout caps instead,
// which flattens a 200 ms debounce and a 1 s save onto the same deadline and stops the debounce
// from coalescing two saves; dividing keeps them 5:1 apart at any factor. The suite passes
// unchanged from 5 up to 400, where the shortest window is down to 1 ms, so the work inside
// these windows is sub-millisecond and 10 keeps ~20 ms of room for a much slower machine.
// Past 10 the remaining gain is under 0.1 s, which is not worth spending that margin on.
const TIMER_SCALE = 10;

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

export async function createBrowserRuntime({ identity = HOSTS.windows, fixture = createWebHostFixture(defaultSettings, defaultTheme) } = {}) {
    const window = new Window({
        url: 'http://localhost/',
        settings: {
            disableCSSFileLoading: true, disableJavaScriptFileLoading: true,
            // Application lifetime intervals must not keep waitUntilComplete() running forever.
            timer: { maxIntervalIterations: 1 },
        },
    });
    // happy-dom #2182: Node's getter must work on subclasses, as it does in browsers.
    // DOMPurify reads this getter directly to avoid DOM clobbering.
    const nodeName = Object.getOwnPropertyDescriptor(window.Node.prototype, 'nodeName');
    Object.defineProperty(window.Node.prototype, 'nodeName', {
        ...nodeName,
        get() {
            for (let proto = Object.getPrototypeOf(this); proto !== window.Node.prototype; proto = Object.getPrototypeOf(proto)) {
                const getter = Object.getOwnPropertyDescriptor(proto, 'nodeName')?.get;
                if (getter) return getter.call(this);
            }
            return nodeName.get.call(this);
        },
    });
    // Elements must stringify as in browsers, or Popper serializes the document during startup.
    delete window.Element.prototype.toString;
    window.document.write(readFileSync(path.join(root, 'index.html'), 'utf8')
        .replace(/<script\b[^>]*>[\s\S]*?<\/script>/g, ''));
    // CSS loading is disabled; the input editor still needs resolved text metrics.
    window.document.getElementById('send_textarea').style.cssText = 'line-height:20px;font-size:16px;padding:0px';
    for (const file of ['jquery-3.5.1.min.js', 'jquery-ui.min.js', 'jquery.transit.min.js',
        'jquery-cookie-1.4.1.min.js', 'cropper.min.js', 'jquery-cropper.min.js',
        'toastr.min.js', 'select2.min.js', 'pagination.js']) {
        vm.runInContext(readFileSync(path.join(root, 'lib', file), 'utf8'), window, { filename: file });
    }
    // Exercise actual module evaluation without starting chat/settings IO through DOM-ready callbacks.
    window.jQuery.holdReady(true);
    window.structuredClone = structuredClone;
    // happy-dom's TransformStream is a Node Transform, not the Web Streams constructor.
    window.TransformStream = TransformStream;
    // happy-dom exposes option elements but omits the browser's Option constructor.
    window.Option = function (text = '', value = text, defaultSelected = false, selected = false) {
        const option = window.document.createElement('option');
        option.text = text;
        option.value = value;
        option.defaultSelected = defaultSelected;
        option.selected = selected;
        return option;
    };
    const scheduleTimeout = window.setTimeout.bind(window);
    window.setTimeout = (callback, delay = 0, ...args) =>
        scheduleTimeout(callback, Math.ceil(delay / TIMER_SCALE), ...args);
    window.fetch = async input => {
        const url = new URL(input, window.location.href);
        const file = url.protocol === 'file:' ? fileURLToPath(url) : path.join(root, decodeURIComponent(url.pathname));
        try { return new window.Response(readFileSync(file)); }
        catch (error) {
            if (error.code === 'ENOENT') return new window.Response('Not found', { status: 404 });
            throw error;
        }
    };
    const modules = new Map();
    let imports = Promise.resolve();
    const resolve = (specifier, owner) => specifier.startsWith('/')
        ? path.join(root, specifier) : fileURLToPath(new URL(specifier, owner.identifier));
    const link = (specifier, owner) => getModule(resolve(specifier, owner));

    function getModule(file) {
        file = path.resolve(root, file);
        if (modules.has(file)) return modules.get(file);
        const module = new vm.SourceTextModule(readFileSync(file, 'utf8'), {
            context: window,
            identifier: pathToFileURL(file).href,
            initializeImportMeta(meta) { meta.url = pathToFileURL(file).href; },
            importModuleDynamically(specifier, owner) {
                // Force Host-ready imports to finish before the bootloader starts script.js.
                const pending = imports.then(() => load(resolve(specifier, owner)));
                imports = pending.catch(() => {});
                return pending;
            },
        });
        modules.set(file, module);
        return module;
    }

    async function load(file) {
        const module = getModule(file);
        if (module.status === 'unlinked') await module.link(link);
        await module.evaluate();
        return module;
    }

    async function startHost() {
        await load('lib.js');
        await load('tauri-main.js');
        await window.__TAURITAVERN_MAIN_READY__;
        let completed;
        while (completed !== imports) {
            completed = imports;
            await completed;
        }
    }

    async function initializeChatRendering() {
        const main = (await load('script.js')).namespace;
        main.reloadMarkdownProcessor();
        getModule('tauri/main/services/chat-surface/chat-virtualization-state.js').namespace
            .initializeChatVirtualization({ chat_virtualization_enabled: false });
        await getModule('scripts/system-messages.js').namespace.initSystemMessages();
        return main;
    }

    const webHost = await load('tauri/web-host/install.js');
    const host = webHost.namespace.installWebHost({ identity, backend: fixture.backend, store: fixture.store });
    return { window, host, fixture, getModule, load, startHost, initializeChatRendering };
}
