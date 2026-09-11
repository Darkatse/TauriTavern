import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

function chatPayload(fileName) {
    return [
        { user_name: 'User', character_name: 'Review', chat_metadata: { integrity: fileName } },
        { name: 'User', is_user: true, is_system: false, mes: fileName, send_date: '2026-01-01T12:00:00Z', extra: {} },
    ].map(entry => JSON.stringify(entry)).join('\n');
}

test('persisted chat navigation', async (context) => {
    const { window, getModule, load, startHost } = createBrowserRuntime();
    window.happyDOM.settings.timer.maxIntervalIterations = 1;
    window.happyDOM.settings.timer.maxIntervalTime = 0;
    const errors = [];
    window.toastr.error = (...args) => errors.push(args);
    window.toastr.warning = () => {};
    const payloads = new Map();
    const handles = new Map();
    const loads = [];
    const writes = [];
    const changed = [];
    let nextHandle = 1;
    let listReads = 0;
    let storedChat;
    let entityKind;

    const hostInvoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (command, args) => {
        switch (command) {
            case 'get_chat_payload_path':
            case 'get_group_chat_path': {
                const fileName = args.fileName ?? args.id;
                loads.push({ fileName, allowNotFound: args.allowNotFound });
                if (!payloads.has(fileName)) {
                    if (args.allowNotFound) return null;
                    throw new Error(`Chat not found: ${fileName}`);
                }
                return fileName;
            }
            case 'plugin:fs|open': {
                const rid = nextHandle++;
                handles.set(rid, new TextEncoder().encode(payloads.get(args.path)));
                return rid;
            }
            case 'plugin:fs|read': {
                const content = handles.get(args.rid);
                const bytes = new window.Uint8Array(content.length + 8);
                bytes.set(content);
                new window.DataView(bytes.buffer).setBigUint64(content.length, BigInt(content.length));
                handles.set(args.rid, new Uint8Array());
                return bytes;
            }
            case 'plugin:resources|close':
                handles.delete(args.rid);
                return;
            default: return hostInvoke(command, args);
        }
    };

    try {
        await startHost();
        const main = (await load('script.js')).namespace;
        const groupChats = getModule('scripts/group-chats.js').namespace;
        const welcome = getModule('scripts/welcome-screen.js').namespace;
        main.reloadMarkdownProcessor();
        getModule('tauri/main/services/chat-surface/chat-virtualization-state.js').namespace
            .initializeChatVirtualization({ chat_virtualization_enabled: false });
        await getModule('scripts/system-messages.js').namespace.initSystemMessages();
        main.eventSource.on(main.event_types.CHAT_CHANGED, id => changed.push(id));

        window.fetch = async (url, options) => {
            const body = options?.body ? JSON.parse(options.body) : undefined;
            switch (url) {
                case '/api/settings/get': return window.Response.json({ result: 'file not find' });
                case '/api/settings/save':
                case '/api/settings/patch': return window.Response.json({
                    hash_algorithm: 'tt-user-settings-stable-sha256-v1', settings_hash: '0'.repeat(64),
                });
                case '/api/chats/recent': return window.Response.json([{
                    avatar: entityKind === 'character' ? 'Review.png' : '',
                    group: entityKind === 'group' ? 'review-group' : '',
                    file_name: 'target-chat.jsonl', chat_items: 1,
                    last_mes: '2026-01-01T12:00:00Z', mes: 'Target chat',
                }]);
                case '/api/characters/chats':
                case '/api/chats/search':
                    listReads += 1;
                    return window.Response.json([...payloads.keys()].map(fileName => ({
                        file_name: `${fileName}.jsonl`, last_mes: '2026-01-01T12:00:00Z',
                    })));
                case '/api/characters/merge-attributes':
                    storedChat = body.chat;
                    writes.push(storedChat);
                    return window.Response.json({ ok: true });
                case '/api/groups/edit':
                    storedChat = body.chat_id;
                    writes.push(storedChat);
                    return window.Response.json({ ok: true });
                default: throw new Error(`Unexpected request: ${url}`);
            }
        };
        await main.getSettings();

        async function reset(kind) {
            await main.clearChat({ clearData: true });
            main.setCharacterId(undefined);
            groupChats.resetSelectedGroup();
            main.setActiveCharacter();
            main.setActiveGroup();
            entityKind = kind;
            storedChat = 'default-chat';
            payloads.clear();
            for (const fileName of ['default-chat', 'target-chat', 'later-chat']) {
                payloads.set(fileName, chatPayload(fileName));
            }
            main.characters.splice(0, main.characters.length, {
                name: 'Review', avatar: 'Review.png', chat: storedChat, shallow: false,
                description: '', personality: '', first_mes: '', scenario: '', mes_example: '', creatorcomment: '',
                create_date: '2026-01-01T12:00:00Z', json_data: '',
                data: { extensions: {}, tags: [], creator_notes: '', creator: '' },
            });
            groupChats.applyGroupsSnapshot(kind === 'group' ? [{
                id: 'review-group', name: 'Review Group', chat_id: storedChat,
                chats: [...payloads.keys()], members: [], disabled_members: [],
            }] : []);
            loads.length = writes.length = changed.length = errors.length = 0;
            listReads = 0;
        }

        async function openExplicit(fileName) {
            if (entityKind === 'character') {
                await main.selectCharacterById(0, { chatFile: fileName });
            } else {
                await groupChats.openGroupById('review-group', { chatId: fileName });
            }
        }

        async function clickRecent() {
            await welcome.openWelcomeScreen();
            const recent = window.document.querySelector('.recentChat[data-file="target-chat"]');
            assert.ok(recent, 'Welcome must render the persisted target');
            recent.click();
            await window.happyDOM.waitUntilComplete();
            assert.equal(handles.size, 0, 'Chat reads must close their file handles');
        }

        for (const kind of ['character', 'group']) {
            await context.test(`${kind}: recent opens only its target and supports a subsequent same-entity switch`, async () => {
                await reset(kind);
                await clickRecent();
                assert.deepEqual(loads, [{ fileName: 'target-chat', allowNotFound: false }]);
                assert.equal(listReads, 0);
                assert.deepEqual(changed, ['target-chat']);
                assert.deepEqual(Array.from(main.chat, message => message.mes), ['target-chat']);
                assert.equal(storedChat, 'target-chat');
                assert.deepEqual(errors, []);

                await openExplicit('later-chat');
                assert.deepEqual(loads.map(entry => entry.fileName), ['target-chat', 'later-chat']);
                assert.deepEqual(changed, ['target-chat', 'later-chat']);
                assert.equal(storedChat, 'later-chat');
                assert.deepEqual(Array.from(main.chat, message => message.mes), ['later-chat']);
            });

            await context.test(`${kind}: failed recent preserves the remembered chat and can be retried`, async () => {
                await reset(kind);
                payloads.set('target-chat', '{invalid-jsonl');
                if (kind === 'group') {
                    groupChats.groups[0].members.push('Missing.png');
                }
                await clickRecent();
                assert.equal(storedChat, 'default-chat');
                assert.deepEqual(writes, []);
                assert.deepEqual(changed, []);
                assert.ok(errors.length > 0);

                payloads.set('target-chat', chatPayload('target-chat'));
                await openExplicit('target-chat');
                assert.deepEqual(loads.map(entry => entry.fileName), ['target-chat', 'target-chat']);
                assert.deepEqual(changed, ['target-chat']);
                assert.equal(storedChat, 'target-chat');
                assert.deepEqual(Array.from(main.chat, message => message.mes), ['target-chat']);
            });

            await context.test(`${kind}: recent does not reclaim a later selection from CHAT_CHANGED`, async () => {
                await reset(kind);
                const redirect = async (chatId) => {
                    if (chatId === 'target-chat') {
                        if (kind === 'group') {
                            await groupChats.openGroupChat('review-group', 'later-chat');
                        } else {
                            await openExplicit('later-chat');
                        }
                    }
                };
                main.eventSource.on(main.event_types.CHAT_CHANGED, redirect);
                try {
                    await clickRecent();
                    assert.deepEqual(loads.map(entry => entry.fileName), ['target-chat', 'later-chat']);
                    assert.deepEqual(changed, ['target-chat', 'later-chat']);
                    assert.deepEqual(writes, ['later-chat']);
                    assert.equal(main.getCurrentChatId(), 'later-chat');
                    assert.equal(storedChat, 'later-chat');
                    assert.deepEqual(Array.from(main.chat, message => message.mes), ['later-chat']);
                    assert.deepEqual(errors, []);
                } finally {
                    main.eventSource.removeListener(main.event_types.CHAT_CHANGED, redirect);
                }
            });
        }
    } finally {
        await window.happyDOM.close();
    }
});
