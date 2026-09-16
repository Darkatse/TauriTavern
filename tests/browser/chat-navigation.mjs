import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

function chatPayload(fileName) {
    return [
        { user_name: 'User', character_name: 'Review', chat_metadata: { integrity: fileName } },
        { name: 'User', is_user: true, is_system: false, mes: fileName, send_date: '2026-01-01T12:00:00Z', extra: {} },
    ].map(entry => JSON.stringify(entry)).join('\n');
}

test('chat persistence and navigation', async (context) => {
    const { window, getModule, load, startHost } = createBrowserRuntime();
    window.happyDOM.settings.timer.maxIntervalIterations = 1;
    window.happyDOM.settings.timer.maxIntervalTime = 0;
    const errors = [];
    window.toastr.error = (...args) => errors.push(args);
    window.toastr.warning = () => {};
    const payloads = new Map();
    const handles = new Map();
    const coldPayloads = new Map();
    const swipeSources = new Map();
    const swipeReads = [];
    let swipeReadError;
    let swipeReadGate;
    let onPrune;
    const loads = [];
    const writes = [];
    const changed = [];
    const commits = [];
    const sessions = new Map();
    let nextHandle = 1;
    let listReads = 0;
    let storedChat;
    let entityKind;
    let fullSaveError;

    function readHeader(fileName) {
        return JSON.parse(payloads.get(fileName).split('\n')[0]);
    }

    function checkIntegrity(fileName, metadata, force = false) {
        const existing = payloads.has(fileName) && readHeader(fileName).chat_metadata?.integrity;
        if (!force && existing && existing !== metadata?.integrity) throw { BadRequest: 'integrity' };
    }

    const hostInvoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (command, args, options) => {
        switch (command) {
            case 'open_cold_chat': {
                const fileName = args.target.fileName ?? args.target.chatId;
                loads.push({ fileName, allowNotFound: args.allowNotFound });
                if (!payloads.has(fileName)) {
                    if (args.allowNotFound) return null;
                    throw new Error(`Chat not found: ${fileName}`);
                }
                const original = payloads.get(fileName).split('\n').map(line => JSON.parse(line));
                const sourceId = nextHandle++;
                swipeSources.set(sourceId, original);
                const projected = structuredClone(coldPayloads.get(fileName) ?? original);
                for (const message of projected) {
                    if (message.tt_swipe_cold) message.tt_swipe_cold.sourceId = sourceId;
                }
                const readerId = nextHandle++;
                handles.set(readerId, new window.TextEncoder().encode(projected.map(message => JSON.stringify(message)).join('\n')));
                return { sourceId, readerId };
            }
            case 'open_cold_swipe_record': {
                if (swipeReadError) throw swipeReadError;
                const record = swipeSources.get(args.sourceId)[args.record];
                swipeReads.push(args.record);
                if (swipeReadGate) {
                    swipeReadGate.entered.resolve();
                    await swipeReadGate.release.promise;
                }
                const rid = nextHandle++;
                handles.set(rid, new window.TextEncoder().encode(JSON.stringify(record)));
                return rid;
            }
            case 'read_chat_bytes': {
                const content = handles.get(args.rid);
                const chunk = content.subarray(0, 64);
                handles.set(args.rid, content.subarray(chunk.length));
                return new window.Uint8Array(chunk);
            }
            case 'get_character_chat_metadata': return readHeader(args.fileName).chat_metadata;
            case 'prune_agent_chat_persistent_states': onPrune?.(args.dto); return;
            case 'commit_chat_metadata': {
                const fileName = args.target.fileName ?? args.target.chatId;
                if (!payloads.has(fileName)) throw { NotFound: 'Chat missing' };
                checkIntegrity(fileName, args.chatMetadata);
                const header = readHeader(fileName);
                header.chat_metadata = args.chatMetadata;
                const original = payloads.get(fileName);
                const newline = original.indexOf('\n');
                payloads.set(fileName, JSON.stringify(header) + '\n' + (newline < 0 ? '' : original.slice(newline + 1)));
                commits.push({ kind: 'metadata', fileName });
                return;
            }
            case 'begin_chat_commit': {
                const sessionId = String(nextHandle++);
                sessions.set(sessionId, { ...args, source: swipeSources.get(args.coldSourceId), frames: [] });
                return { sessionId, maxFrameBytes: 1024 * 1024 };
            }
            case 'append_chat_commit_chunk': {
                const session = sessions.get(options.headers['session-id']);
                session.frames.push(Buffer.from(args));
                return session.frames.reduce((sum, frame) => sum + frame.length, 0);
            }
            case 'finish_chat_commit': {
                if (fullSaveError) throw fullSaveError;
                const session = sessions.get(args.sessionId);
                const fileName = session.target.fileName ?? session.target.chatId;
                let jsonl = Buffer.concat(session.frames).toString();
                if (session.source) {
                    jsonl = jsonl.split('\n').filter(Boolean).map(line => {
                        const message = JSON.parse(line);
                        if (message.tt_swipe_cold) {
                            const original = session.source[message.tt_swipe_cold.record];
                            for (const key of ['swipes', 'swipe_info']) {
                                message[key] = message[key].map((value, i) => value ?? original[key][i]);
                            }
                            delete message.tt_swipe_cold;
                        }
                        return JSON.stringify(message);
                    }).join('\n');
                }
                checkIntegrity(fileName, JSON.parse(jsonl.split('\n')[0]).chat_metadata, session.force);
                payloads.set(fileName, jsonl);
                commits.push({ kind: 'full', fileName, force: session.force, reason: args.commitReason });
                sessions.delete(args.sessionId);
                return { acceptedSize: args.expectedSize, size: Buffer.byteLength(jsonl) };
            }
            case 'abort_chat_commit': sessions.delete(args.sessionId); return;
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
            case 'plugin:fs|fstat': return { size: handles.get(args.rid).length };
            case 'plugin:fs|read': {
                const content = handles.get(args.rid);
                const chunk = content.subarray(0, args.len);
                const bytes = new window.Uint8Array(args.len + 8);
                bytes.set(chunk);
                new window.DataView(bytes.buffer).setBigUint64(args.len, BigInt(chunk.length));
                handles.set(args.rid, content.subarray(chunk.length));
                return bytes;
            }
            case 'plugin:resources|close':
                swipeSources.delete(args.rid);
                handles.delete(args.rid);
                return;
            default: return hostInvoke(command, args, options);
        }
    };

    try {
        await startHost();
        const main = (await load('script.js')).namespace;
        const groupChats = getModule('scripts/group-chats.js').namespace;
        const welcome = getModule('scripts/welcome-screen.js').namespace;
        const coldSwipes = getModule('scripts/tauri/chat/cold-swipes.js').namespace;
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
        // These chat fixtures skip settings bootstrap; establish its normal save baseline.
        getModule('scripts/tauri/setting/settings-delta-save.js').namespace.captureSettingsSaveBaseline({}, {
            hash_algorithm: 'tt-user-settings-stable-sha256-v1', settings_hash: '0'.repeat(64),
        });
        main.setAnimationDuration(0);

        async function reset(kind) {
            coldSwipes.initializeColdSwipes({ cold_swipes_enabled:false });
            coldPayloads.clear();
            swipeReads.length = 0;
            swipeReadError = swipeReadGate = onPrune = undefined;
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
            commits.length = 0;
            fullSaveError = undefined;
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

            await context.test(`${kind}: cold deletion stays synchronous, appended swipes save, and historical rollback hydrates on demand`, async () => {
                await reset(kind);
                const original = [
                    { chat_metadata: { integrity: 'target-chat' } },
                    { name:'Review', mes:'active', swipe_id:1, swipes:['older','active'], swipe_info:[{extra:{}},{extra:{}}], variables:[{score:0},{score:1}], extra:{} },
                    { name:'User', mes:'tail', is_user:true, extra:{} },
                ];
                const projected = structuredClone(original);
                projected[1].swipes = [null,'active'];
                projected[1].swipe_info[0] = null;
                projected[1].tt_swipe_cold = { sourceId:0,record:1 };
                payloads.set('target-chat', original.map(message => JSON.stringify(message)).join('\n'));
                coldPayloads.set('target-chat', projected);
                coldSwipes.initializeColdSwipes({ cold_swipes_enabled:true });
                await openExplicit('target-chat');
                const message = main.chat[0];
                assert.equal(main.ensureSwipes(message), false);
                assert.equal(message.swipes[0], null);
                assert.equal(message.variables[message.swipe_id].score, 1);
                message.variables[message.swipe_id].score = 9;

                swipeReadError = new Error('source read failed');
                const deletion = main.deleteMessage(1);
                assert.equal(main.chat.length, 1);
                await deletion;
                message.swipes.push('appended');
                message.swipe_info.push({extra:{}});
                message.swipe_id = 2;
                await main.flushDebouncedChatSave();
                assert.deepEqual(swipeReads, []);
                assert.deepEqual(Array.from(message.swipes), [null,'active','appended']);
                assert.ok(message.tt_swipe_cold);
                const saved = JSON.parse(payloads.get('target-chat').split('\n')[1]);
                assert.deepEqual(saved.swipes, ['older','active','appended']);
                assert.equal(saved.swipe_id, 2);
                assert.equal(saved.variables[1].score, 9);
                assert.equal(saved.tt_swipe_cold, undefined);
                assert.deepEqual(errors, []);

                // Slot deletion needs a read; failure leaves the message intact and retryable.
                await assert.rejects(main.deleteSwipe(2, 0), /source read failed/);
                assert.equal(message.swipes.length, 3);
                swipeReadError = undefined;
                errors.length = 0;
                await main.deleteSwipe(2, 0);
                assert.deepEqual(Array.from(message.swipes), ['older','active']);
                assert.equal(message.tt_swipe_cold, undefined);
                assert.deepEqual(swipeReads, [1]);

                // The existing rollback delegates to the same deleteSwipe boundary on a historical message.
                payloads.set('target-chat', original.map(message => JSON.stringify(message)).join('\n'));
                await main.reloadCurrentChat();
                const historical = main.chat[0];
                historical.extra = {tauritavern:{agent:{runId:'cold-run',rollback:{strategy:'deleteSwipe',swipeId:2}}}};
                historical.swipes.push('agent reply');
                historical.swipe_info.push({extra:historical.extra});
                historical.swipe_id = 2;
                historical.mes = 'agent reply';
                const rollback = (await load('scripts/tauritavern/agent/agent-run-message-rollback.js')).namespace;
                const result = await rollback.rollbackAgentRunDriftMessages({ runId:'cold-run', targets:[{messageId:'0'}], script:main });
                assert.equal(result.swipesRemoved, 1);
                assert.equal(main.chat.length, 2);
                assert.equal(historical.mes, 'active');
                assert.deepEqual(Array.from(historical.swipes), ['older','active']);
                assert.equal(historical.tt_swipe_cold, undefined);

                if (kind === 'character') {
                    // Ancillary cleanup can outlive deletion and navigation, but must retain its original target.
                    original[1].swipe_info[0].extra = {tauritavern:{agent:{persistStateId:'cold-state'}}};
                    payloads.set('target-chat', original.map(message => JSON.stringify(message)).join('\n'));
                    await main.reloadCurrentChat();
                    swipeReadGate = {entered:Promise.withResolvers(),release:Promise.withResolvers()};
                    const pruned = Promise.withResolvers();
                    onPrune = pruned.resolve;
                    const deleted = main.deleteMessage(0);
                    assert.equal(main.chat.length, 1);
                    await swipeReadGate.entered.promise;
                    await deleted;
                    await openExplicit('later-chat');
                    swipeReadGate.release.resolve();
                    const request = await pruned.promise;
                    assert.equal(request.chatRef.fileName, 'target-chat');
                    assert.deepEqual(Array.from(request.candidateStateIds), ['cold-state']);
                    assert.equal(JSON.parse(payloads.get('target-chat').split('\n')[1]).mes, 'tail');
                    swipeReadGate = undefined;
                }

                payloads.set('target-chat', original.map(message => JSON.stringify(message)).join('\n'));
                await openExplicit('target-chat');
                swipeReadGate = {entered:Promise.withResolvers(),release:Promise.withResolvers()};
                const pending = main.deleteSwipe(0, 0);
                await swipeReadGate.entered.promise;
                await openExplicit('later-chat');
                swipeReadGate.release.reject(new Error('source read failed after navigation'));
                await pending;
                assert.equal(main.getCurrentChatId(), 'later-chat');
                assert.equal(main.chat.length, 1);
                assert.equal(main.chat[0].mes, 'later-chat');
                assert.deepEqual(errors, []);
                assert.equal(swipeSources.size, 0);
                assert.equal(handles.size, 0);
            });

            await context.test(`${kind}: metadata saves preserve the body and pending message deletion`, async () => {
                await reset(kind);
                await openExplicit('target-chat');
                const original = payloads.get('target-chat');
                const entityWrites = writes.length;
                main.chat_metadata.variables = { score: '1' };
                main.chat_metadata.lastInContextMessageId = 42;
                main.chat[0].mes = 'not saved by metadata';
                await main.saveMetadata();
                const updated = payloads.get('target-chat');
                assert.equal(updated.slice(updated.indexOf('\n')), original.slice(original.indexOf('\n')));
                assert.equal(readHeader('target-chat').chat_metadata.variables.score, '1');
                assert.equal(readHeader('target-chat').chat_metadata.lastInContextMessageId, undefined);
                assert.equal(main.chat_metadata.lastInContextMessageId, 42);
                assert.equal(writes.length, entityWrites);
                assert.deepEqual(commits.map(commit => commit.kind), ['metadata']);

                await main.deleteMessage(0);
                main.chat_metadata.variables.score = '2';
                await main.saveMetadata();
                assert.equal(payloads.get('target-chat').split('\n').length, 2);
                const pending = main.flushDebouncedChatSave();
                assert.ok(pending, 'metadata must not cancel the pending full save');
                await pending;
                assert.equal(payloads.get('target-chat').split('\n').length, 1);
                assert.equal(readHeader('target-chat').chat_metadata.variables.score, '2');
                assert.deepEqual(commits.map(commit => commit.kind), ['metadata', 'metadata', 'full']);
                assert.deepEqual(errors, []);
            });

            await context.test(`${kind}: metadata conflict recovery owns the save queue until complete`, async () => {
                await reset(kind);
                await openExplicit('target-chat');
                payloads.set('target-chat', chatPayload('external'));
                main.chat[0].mes = 'local body';
                const { Popup } = getModule('scripts/popup.js').namespace;
                const originalInput = Popup.show.input;
                const entered = Promise.withResolvers();
                const answer = Promise.withResolvers();
                Popup.show.input = () => { entered.resolve(); return answer.promise; };
                try {
                    const pending = main.saveMetadata();
                    await entered.promise;
                    assert.equal(main.isChatSaving, true);
                    let nextEntered = false;
                    const next = main.enqueueChatSave(async () => { nextEntered = true; });
                    await Promise.resolve();
                    assert.equal(nextEntered, false);
                    answer.resolve('OVERWRITE');
                    await Promise.all([pending, next]);
                    assert.equal(main.isChatSaving, false);
                    assert.equal(nextEntered, true);
                    assert.deepEqual(commits, [{ kind: 'full', fileName: 'target-chat', force: true, reason: 'mutation' }]);
                    assert.equal(readHeader('target-chat').chat_metadata.integrity, 'target-chat');
                    assert.equal(JSON.parse(payloads.get('target-chat').split('\n')[1]).mes, 'local body');
                } finally {
                    Popup.show.input = originalInput;
                }
            });

            await context.test(`${kind}: declining a metadata overwrite reloads and failures stay failures`, async () => {
                await reset(kind);
                await openExplicit('target-chat');
                const external = chatPayload('external');
                payloads.set('target-chat', external);
                const { Popup } = getModule('scripts/popup.js').namespace;
                const originalInput = Popup.show.input;
                const originalReload = window.location.reload;
                let reloads = 0;
                Popup.show.input = async () => '';
                window.location.reload = () => { reloads += 1; };
                try {
                    await main.saveMetadata();
                    assert.equal(reloads, 1);
                    assert.equal(payloads.get('target-chat'), external);
                    assert.deepEqual(commits, []);

                    Popup.show.input = async () => 'OVERWRITE';
                    fullSaveError = { InternalServerError: 'publish failed' };
                    await assert.rejects(() => main.saveMetadata(), error => error.cause === fullSaveError && error.code === undefined);
                    assert.equal(payloads.get('target-chat'), external);
                    assert.deepEqual(commits, []);
                    assert.equal(main.isChatSaving, false);

                    payloads.delete('target-chat');
                    Popup.show.input = () => { throw new Error('NotFound must not prompt for overwrite'); };
                    await assert.rejects(() => main.saveMetadata(), error => error.cause.NotFound === 'Chat missing' && error.code === undefined);
                    assert.equal(payloads.has('target-chat'), false);
                } finally {
                    Popup.show.input = originalInput;
                    window.location.reload = originalReload;
                }
            });
        }

        for (const greeting of ['', 'Hello']) {
            await context.test(`new group establishes metadata before greeting hooks (${greeting || 'empty'})`, async () => {
                await reset('group');
                payloads.clear();
                groupChats.groups[0].members = ['Review.png'];
                main.characters[0].first_mes = greeting;
                const onGreeting = async () => {
                    const identity = main.chat_metadata.integrity;
                    assert.ok(identity);
                    assert.equal(readHeader('default-chat').chat_metadata.integrity, identity);
                    main.chat_metadata.variables = { fromGreeting: 'saved' };
                    await main.saveMetadata();
                };
                main.eventSource.on(main.event_types.CHARACTER_FIRST_MESSAGE_SELECTED, onGreeting);
                try {
                    await groupChats.openGroupById('review-group');
                    assert.equal(readHeader('default-chat').chat_metadata.variables.fromGreeting, 'saved');
                    assert.equal(main.chat_metadata.variables.fromGreeting, 'saved');
                    assert.equal(payloads.get('default-chat').split('\n').length, greeting ? 2 : 1);
                    assert.deepEqual(commits.map(commit => commit.kind), ['full', 'metadata', 'full']);
                    assert.ok(commits.filter(commit => commit.kind === 'full').every(commit => commit.reason === 'maintenance'));
                    assert.deepEqual(errors, []);
                } finally {
                    main.eventSource.removeListener(main.event_types.CHARACTER_FIRST_MESSAGE_SELECTED, onGreeting);
                }
            });
        }
    } finally {
        await window.happyDOM.close();
    }
});
