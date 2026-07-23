import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    return import(`${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`);
}

async function withChatApi(stContext, safeInvoke, run) {
    const previousWindow = globalThis.window;
    globalThis.window = {
        __TAURITAVERN__: {},
        SillyTavern: { getContext: () => stContext },
    };

    try {
        const { installChatApi } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/api/chat.js'),
        );
        installChatApi({ safeInvoke });
        await run(globalThis.window.__TAURITAVERN__.api.chat);
    } finally {
        if (previousWindow === undefined) {
            delete globalThis.window;
        } else {
            globalThis.window = previousWindow;
        }
    }
}

test('windowInfo exposes full-history semantics without a summary invoke', async () => {
    const context = {
        chat: [{ mes: 'first' }, { mes: 'second' }],
        chatId: 'Story.jsonl',
        groupId: null,
        characters: [{ name: 'Alice', avatar: 'Alice.png' }],
        characterId: 0,
    };

    await withChatApi(context, async () => {
        throw new Error('windowInfo must not invoke the backend');
    }, async (api) => {
        assert.deepEqual(await api.current.windowInfo(), {
            mode: 'off',
            chatKind: 'character',
            chatRef: {
                kind: 'character',
                characterId: 'Alice',
                fileName: 'Story',
            },
            totalCount: 2,
            windowStartIndex: 0,
            windowLength: 2,
        });
    });
});

test('history API retains character and group tail/before/beforePages command routing', async () => {
    const calls = [];
    const safeInvoke = async (command, args) => {
        calls.push({ command, args });
        if (command.endsWith('_summary')) {
            return { message_count: 4 };
        }
        if (command.endsWith('_tail')) {
            return {
                lines: ['{"mes":"two"}', '{"mes":"three"}'],
                cursor: { offset: 20, size: 40, modifiedMillis: 1 },
                hasMoreBefore: true,
            };
        }
        if (command.endsWith('_before_pages')) {
            return [{
                lines: ['{"mes":"one"}'],
                cursor: { offset: 10, size: 40, modifiedMillis: 1 },
                hasMoreBefore: true,
            }, {
                lines: ['{"mes":"zero"}'],
                cursor: { offset: 0, size: 40, modifiedMillis: 1 },
                hasMoreBefore: false,
            }];
        }
        if (command.endsWith('_before')) {
            return {
                lines: ['{"mes":"zero"}', '{"mes":"one"}'],
                cursor: { offset: 10, size: 40, modifiedMillis: 1 },
                hasMoreBefore: false,
            };
        }
        throw new Error(`Unexpected command: ${command}`);
    };

    await withChatApi({ chat: [], chatId: 'unused', groupId: null, characters: [], characterId: 0 }, safeInvoke, async (api) => {
        const character = api.open({ kind: 'character', characterId: 'Alice', fileName: 'Story' });
        const characterTail = await character.history.tail({ limit: 2 });
        assert.equal(characterTail.startIndex, 2);
        assert.deepEqual(characterTail.messages.map(message => message.mes), ['two', 'three']);

        const characterBefore = await character.history.before(characterTail, { limit: 2 });
        assert.equal(characterBefore.startIndex, 0);
        assert.deepEqual(characterBefore.messages.map(message => message.mes), ['zero', 'one']);
        const characterPages = await character.history.beforePages(characterTail, { limit: 1, pages: 2 });
        assert.deepEqual(characterPages.map(page => page.startIndex), [1, 0]);

        const group = api.open({ kind: 'group', chatId: 'Party' });
        const groupTail = await group.history.tail({ limit: 2 });
        assert.equal(groupTail.startIndex, 2);
        const groupBefore = await group.history.before(groupTail, { limit: 2 });
        assert.equal(groupBefore.startIndex, 0);
        const groupPages = await group.history.beforePages(groupTail, { limit: 1, pages: 2 });
        assert.deepEqual(groupPages.map(page => page.startIndex), [1, 0]);
    });

    assert.deepEqual(calls.map(call => call.command), [
        'get_character_chat_summary',
        'get_chat_payload_tail',
        'get_chat_payload_before',
        'get_chat_payload_before_pages',
        'get_group_chat_summary',
        'get_group_chat_payload_tail',
        'get_group_chat_payload_before',
        'get_group_chat_payload_before_pages',
    ]);
});
