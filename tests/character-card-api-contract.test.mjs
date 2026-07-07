import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function withNavigatorUserAgent(userAgent, callback) {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
    Object.defineProperty(globalThis, 'navigator', {
        value: { userAgent },
        configurable: true,
    });

    try {
        return await callback();
    } finally {
        if (descriptor) {
            Object.defineProperty(globalThis, 'navigator', descriptor);
        } else {
            delete globalThis.navigator;
        }
    }
}

function streamFromBytes(bytes) {
    return new ReadableStream({
        start(controller) {
            controller.enqueue(bytes);
            controller.close();
        },
    });
}

async function installHarness({
    safeInvoke,
    createReadableFileStream,
    removeTemporaryFile,
} = {}) {
    const calls = [];
    const cleanups = [];
    globalThis.window = {
        __TAURITAVERN__: { api: {} },
    };

    const { installCharacterCardsApi } = await import(pathToFileURL(path.join(REPO_ROOT, 'src/tauri/main/api/character-cards.js')));
    installCharacterCardsApi({
        safeInvoke: safeInvoke ?? (async (command, args) => {
            calls.push({ command, args });
            return '/tmp/Alice.json';
        }),
        createReadableFileStream: createReadableFileStream ?? (async () => streamFromBytes(new TextEncoder().encode('{"name":"Alice"}'))),
        removeTemporaryFile: removeTemporaryFile ?? (async (filePath) => cleanups.push(filePath)),
    });

    return {
        calls,
        cleanups,
        characterCards: globalThis.window.__TAURITAVERN__.api.characterCards,
    };
}

test('api.characterCards picks desktop files through the host dialog', async () => {
    const { calls, characterCards } = await installHarness();

    assert.equal(characterCards.isNativePickerAvailable(), true);
    const files = await characterCards.pickFiles({ title: 'Replace Character Card' });

    assert.equal(calls[0].command, 'plugin:dialog|open');
    assert.equal(calls[0].args.options.title, 'Replace Character Card');
    assert.deepEqual(calls[0].args.options.filters, [
        { name: 'Character Card', extensions: ['json', 'png'] },
    ]);
    assert.equal(files.length, 1);
    assert.equal(files[0].name, 'Alice.json');
    assert.equal(files[0].type, 'application/json');
    assert.equal(await files[0].text(), '{"name":"Alice"}');
});

test('api.characterCards preserves raw desktop path percent signs', async () => {
    const { characterCards } = await installHarness({
        safeInvoke: async () => '/tmp/Alice%20.json',
    });

    const files = await characterCards.pickFiles();

    assert.equal(files.length, 1);
    assert.equal(files[0].name, 'Alice%20.json');
    assert.equal(files[0].type, 'application/json');
});

test('api.characterCards decodes file URL path names', async () => {
    const { characterCards } = await installHarness({
        safeInvoke: async () => pathToFileURL('/tmp/Alice Smith.json').href,
    });

    const files = await characterCards.pickFiles();

    assert.equal(files.length, 1);
    assert.equal(files[0].name, 'Alice Smith.json');
    assert.equal(files[0].type, 'application/json');
});

test('api.characterCards fails fast for unsupported native picker file types', async () => {
    const { characterCards } = await installHarness({
        safeInvoke: async () => '/tmp/Alice.yaml',
    });

    await assert.rejects(
        () => characterCards.pickFiles(),
        /Unsupported character card file type: Alice\.yaml/,
    );
});

test('api.characterCards stages iOS picked files through the native command', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)', async () => {
        const calls = [];
        const cleanups = [];

        const { characterCards } = await installHarness({
            safeInvoke: async (command, args) => {
                calls.push({ command, args });
                return {
                    cancelled: false,
                    filePath: '/cache/tauritavern-character-import-staging/picked.png',
                    fileName: 'Alice.png',
                };
            },
            createReadableFileStream: async () => streamFromBytes(new Uint8Array([137, 80, 78, 71])),
            removeTemporaryFile: async (filePath) => cleanups.push(filePath),
        });

        assert.equal(characterCards.isNativePickerAvailable(), true);
        const files = await characterCards.pickFiles();

        assert.equal(calls[0].command, 'ios_pick_character_card');
        assert.equal(files.length, 1);
        assert.equal(files[0].name, 'Alice.png');
        assert.equal(files[0].type, 'image/png');
        assert.deepEqual(new Uint8Array(await files[0].arrayBuffer()), new Uint8Array([137, 80, 78, 71]));
        assert.deepEqual(cleanups, ['/cache/tauritavern-character-import-staging/picked.png']);
    });
});

test('api.characterCards does not fail iOS picked files when staging cleanup fails', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)', async () => {
        const warnings = [];
        const originalWarn = console.warn;
        console.warn = (...args) => warnings.push(args);

        try {
            const { characterCards } = await installHarness({
                safeInvoke: async () => ({
                    cancelled: false,
                    filePath: '/cache/tauritavern-character-import-staging/picked.json',
                    fileName: 'Alice.json',
                }),
                removeTemporaryFile: async () => {
                    throw new Error('cleanup failed');
                },
            });

            const files = await characterCards.pickFiles();

            assert.equal(files.length, 1);
            assert.equal(files[0].name, 'Alice.json');
            assert.equal(await files[0].text(), '{"name":"Alice"}');
            assert.equal(warnings.length, 1);
        } finally {
            console.warn = originalWarn;
        }
    });
});

test('api.characterCards leaves Android on the WebView file input path', async () => {
    await withNavigatorUserAgent('Mozilla/5.0 (Linux; Android 15)', async () => {
        const calls = [];
        const { characterCards } = await installHarness({
            safeInvoke: async (command, args) => {
                calls.push({ command, args });
                return '/tmp/Alice.json';
            },
        });

        assert.equal(characterCards.isNativePickerAvailable(), false);
        assert.equal(await characterCards.pickFiles(), null);
        assert.deepEqual(calls, []);
    });
});
