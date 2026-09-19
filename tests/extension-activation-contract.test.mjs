import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';

test('required renderer activation preserves the original hook error while optional activation remains recoverable', async () => {
    const failure = new Error('Duplicate discriminator value "undefined"');
    const name = 'third-party/JS-Slash-Runner';
    const resourceUrl = (extension, path) => `/scripts/extensions/${extension}/${path}`;
    const manifest = { js: 'index.js', hooks: { activate: 'activate' } };
    let hookCalls = 0;
    const noop = () => {};
    const jquery = {
        append() { return this; },
        css() { return this; },
        val() { return this; },
        prop() { return this; },
        on() { return this; },
        get: noop,
        toggleClass: noop,
    };
    const dependencies = {
        '../lib.js': { getHljs: noop, Popper: { createPopper: noop } },
        '../script.js': {
            CLIENT_VERSION: 'SillyTavern:1.18.0:TauriTavern',
            eventSource: { emit: noop },
            event_types: { EXTENSIONS_FIRST_LOAD: 'extensions_first_load' },
        },
        './templates.js': { renderTemplateAsync: noop },
        './utils.js': {
            delay: noop,
            equalsIgnoreCaseAndAccents: (left, right) => left.toLowerCase() === right.toLowerCase(),
        },
        './i18n.js': { t: String.raw },
        './extensions/runtime/third-party-runtime.js': { createThirdPartyStylesheetResolver: () => ({}) },
        './extensions/runtime/asset-loader.js': {
            createExtensionAssetLoader: () => ({ addExtensionScript: noop, addExtensionStyle: noop }),
        },
        './extensions/runtime/resource-paths.js': {
            getExtensionResourceUrl: resourceUrl,
            isThirdPartyExtension: extension => extension.startsWith('third-party/'),
        },
        './extensions/runtime/tauri-ready.js': { waitForTauriMainReady: noop },
        [resourceUrl(name, manifest.js)]: {
            activate() {
                hookCalls++;
                throw failure;
            },
        },
    };

    // Load the complete frontend module with its browser and host imports stubbed.
    const source = await readFile(new URL('../src/scripts/extensions.js', import.meta.url), 'utf8');
    const { outputText } = ts.transpileModule(source, {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    });
    const extensions = {};
    runInNewContext(outputText, {
        exports: extensions,
        require: specifier => dependencies[specifier] ?? {},
        console: { debug: noop, log: noop },
        setInterval: noop,
        document: { body: {} },
        $: () => jquery,
        fetch: async url => {
            if (url === '/api/extensions/discover') {
                return { ok: true, json: async () => [{ name, type: 'local' }] };
            }
            assert.equal(url, resourceUrl(name, 'manifest.json'));
            return { ok: true, json: async () => manifest };
        },
    });

    await extensions.startOfflineExtensionsDiscovery();
    await assert.rejects(extensions.activateRequiredChatSurfaceExtensions(), error => error === failure);
    assert.equal(hookCalls, 1);

    await assert.doesNotReject(extensions.activateDeferredThirdPartyExtensions());
    assert.equal(hookCalls, 2);
});
