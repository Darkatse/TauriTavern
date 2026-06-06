import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

function withWindow(value, fn) {
    const previousWindow = globalThis.window;
    globalThis.window = value;

    try {
        return fn();
    } finally {
        if (previousWindow === undefined) {
            delete globalThis.window;
        } else {
            globalThis.window = previousWindow;
        }
    }
}

test('active chat ref resolves character identity from exact avatar filename', async () => {
    const { getActiveChatSnapshot } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/adapters/st/active-chat-ref.js'),
    );

    const snapshot = withWindow({
        SillyTavern: {
            getContext: () => ({
                chat: [{ mes: 'hello' }],
                chatId: 'Story.jsonl',
                groupId: null,
                characters: [{ name: 'Alice', avatar: 'Alice#1.png' }],
                characterId: 0,
            }),
        },
    }, () => getActiveChatSnapshot());

    assert.deepEqual(snapshot, {
        ref: {
            kind: 'character',
            characterId: 'Alice#1',
            fileName: 'Story',
        },
        windowLength: 1,
    });
});

test('active chat ref rejects URL-like active character avatars', async () => {
    const { getActiveChatSnapshot } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/adapters/st/active-chat-ref.js'),
    );

    assert.throws(
        () => withWindow({
            SillyTavern: {
                getContext: () => ({
                    chat: [],
                    chatId: 'Story.jsonl',
                    groupId: null,
                    characters: [{ name: 'Alice', avatar: 'thumbnail?file=Alice.png' }],
                    characterId: 0,
                }),
            },
        }, () => getActiveChatSnapshot()),
        /Bad request: invalid avatar/,
    );
});
