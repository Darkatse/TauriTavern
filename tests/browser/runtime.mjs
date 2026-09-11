import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import vm from 'node:vm';
import { Window } from 'happy-dom';

const root = fileURLToPath(new URL('../../src/', import.meta.url));

export function createBrowserRuntime() {
    const window = new Window({
        url: 'http://localhost/',
        settings: { disableCSSFileLoading: true, disableJavaScriptFileLoading: true },
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
