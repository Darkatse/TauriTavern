import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import vm from 'node:vm';
import { Window } from 'happy-dom';

const root = fileURLToPath(new URL('../../src/', import.meta.url));

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

export function createBrowserRuntime() {
    const window = new Window({
        url: 'http://localhost/',
        settings: { disableCSSFileLoading: true, disableJavaScriptFileLoading: true },
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
    window.document.write(readFileSync(path.join(root, 'index.html'), 'utf8')
        .replace(/<script\b[^>]*>[\s\S]*?<\/script>/g, ''));
    for (const file of ['jquery-3.5.1.min.js', 'jquery-ui.min.js', 'jquery.transit.min.js',
        'jquery-cookie-1.4.1.min.js', 'cropper.min.js', 'jquery-cropper.min.js',
        'toastr.min.js', 'select2.min.js', 'pagination.js']) {
        vm.runInContext(readFileSync(path.join(root, 'lib', file), 'utf8'), window, { filename: file });
    }
    // Exercise actual module evaluation without starting chat/settings IO through DOM-ready callbacks.
    window.jQuery.holdReady(true);
    window.structuredClone = structuredClone;
    const scheduleTimeout = window.setTimeout.bind(window);
    window.setTimeout = (callback, delay = 0, ...args) =>
        scheduleTimeout(callback, Math.ceil(delay / TIMER_SCALE), ...args);
    window.fetch = async () => { throw new Error('Unexpected network request'); };
    window.localStorage.setItem('tt:embeddedRuntimeProfile', 'off');
    const listeners = new Map();
    window.__TAURI__ = {
        core: {
            async invoke(command, args) {
                switch (command) {
                    case 'is_ready': return true;
                    case 'backend_error_bridge_ready': return [];
                    case 'wait_for_backend_ready': return;
                    case 'get_tauritavern_settings': return {
                        panel_runtime_profile: 'off', embedded_runtime_profile: 'off',
                        dynamic_theme: { enabled: false, wallpaper_enabled: false },
                    };
                    case 'read_frontend_template': return readFileSync(path.join(root, 'scripts/templates', args.name), 'utf8');
                    default: throw new Error(`Unexpected IPC command: ${command}`);
                }
            },
            convertFileSrc: file => `http://localhost/${file}`,
        },
        event: {
            async listen(name, callback) {
                listeners.set(name, callback);
                return () => listeners.delete(name);
            },
        },
        window: { getCurrentWindow: () => ({}) },
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

    return { window, listeners, getModule, load, startHost };
}
