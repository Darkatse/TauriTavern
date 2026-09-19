import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
let invokedCommand;
let invokedArgs;

global.window = {
    __TAURI_RUNNING__: true,
    __TAURI__: {
        core: {
            invoke: async (command, args) => {
                invokedCommand = command;
                invokedArgs = args;
            },
        },
    },
};

const compatPath = path.join(
    REPO_ROOT,
    'src/tauri/main/compat/mobile/mobile-runtime-compat.js',
);
const { installMobileRuntimeCompat } = await import(pathToFileURL(compatPath).href);

function createTargetWindow(userAgent, clipboard) {
    return {
        navigator: { userAgent, clipboard },
        Array,
        String,
        Object,
        requestIdleCallback() {},
        cancelIdleCallback() {},
    };
}

function setRuntimeUserAgent(userAgent) {
    Object.defineProperty(globalThis, 'navigator', {
        value: { userAgent },
        configurable: true,
    });
}

test('Android Web Clipboard writes through the native writer without replacing other methods', async () => {
    setRuntimeUserAgent('Mozilla/5.0 (Linux; Android 15)');
    const readText = async () => 'existing';
    const clipboard = { readText, writeText: async () => assert.fail('used Web Clipboard') };
    const targetWindow = createTargetWindow('Mozilla/5.0 (Linux; Android 15)', clipboard);

    installMobileRuntimeCompat(targetWindow);
    await targetWindow.navigator.clipboard.writeText(42);

    assert.equal(targetWindow.navigator.clipboard.readText, readText);
    assert.equal(invokedCommand, 'plugin:clipboard-manager|write_text');
    assert.deepEqual(invokedArgs, { text: '42' });
});

test('non-Android Web Clipboard is left unchanged', () => {
    setRuntimeUserAgent('Mozilla/5.0 (iPhone)');
    const writeText = async () => {};
    const targetWindow = createTargetWindow('Mozilla/5.0 (iPhone)', { writeText });

    installMobileRuntimeCompat(targetWindow);

    assert.equal(targetWindow.navigator.clipboard.writeText, writeText);
});
