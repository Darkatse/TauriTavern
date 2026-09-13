import assert from 'node:assert/strict';
import test from 'node:test';
import {
    acceptColdChatPayload, discardColdChatPayload, hydrateMessageSwipes, loadColdChatPayload,
    releaseCurrentSwipeSource,
} from '../src/scripts/tauri/chat/cold-swipes.js';

const original = {
    mes: 'current body', swipe_id: 1, swipes: ['older', 'active'],
    swipe_info: [{ extra: { state: 'old' } }, { extra: { state: 'current' } }],
    variables: [{ score: 0 }, { score: 1 }], extra: { content: 'live' },
};

function cold(sourceId = 0) {
    return { ...structuredClone(original), swipes: [null, 'active'], swipe_info: [null, { extra: { state: 'current' } }], tt_swipe_cold: { sourceId, record: 3 } };
}

function installHost(invoke) {
    const previous = globalThis.window;
    globalThis.window = { __TAURI__: { core: { invoke } } };
    return () => { globalThis.window = previous; };
}

function byteStream(text) {
    const bytes = new TextEncoder().encode(text);
    return [bytes.subarray(0, 7), bytes.subarray(7), new Uint8Array(0)];
}

test('hydration fills nulls in place and preserves edits, appended slots, selection and JSR variables, and ignores a late duplicate read', async () => {
    const gates = [Promise.withResolvers(), Promise.withResolvers()];
    const streams = new Map();
    const closed = [];
    let reads = 0;
    const restore = installHost(async (command, args) => {
        if (command === 'open_cold_swipe_record') {
            assert.deepEqual(args, {sourceId:0,record:3});
            const index = reads++;
            await gates[index].promise;
            streams.set(index + 10, byteStream(JSON.stringify(original)));
            return index + 10;
        }
        if (command === 'read_chat_bytes') return streams.get(args.rid).shift();
        if (command === 'plugin:resources|close') { closed.push(args.rid); return; }
        throw new Error(command);
    });
    try {
        const message = cold();
        const swipes = message.swipes;
        const info = message.swipe_info;
        const first = hydrateMessageSwipes(message);
        const second = hydrateMessageSwipes(message);
        message.mes = 'edited body';
        message.swipes[1] = 'edited active';
        message.swipe_info[1] = { extra: { edited: true } };
        message.variables[1].score = 9;
        message.swipe_info[0] = { extra: { historicalEdit: true } };
        message.swipes.push('appended one', 'appended two');
        message.swipe_info.push({}, { extra: { appended: true } });
        message.swipe_id = 3;
        gates[0].resolve();
        await first;
        assert.equal(message.swipes, swipes);
        assert.equal(message.swipe_info, info);
        assert.deepEqual(message.swipes, ['older', 'edited active', 'appended one', 'appended two']);
        assert.deepEqual(message.swipe_info, [{ extra: { historicalEdit: true } }, { extra: { edited: true } }, {}, { extra: { appended: true } }]);
        assert.equal(message.swipe_id, 3);
        assert.deepEqual(message.swipe_info[1], { extra: { edited: true } });
        assert.equal(message.variables[1].score, 9);
        assert.equal(message.mes, 'edited body');
        assert.equal(message.tt_swipe_cold, undefined);
        message.swipes[0] = 'edited after hydration';
        gates[1].resolve();
        await second;
        assert.equal(message.swipes[0], 'edited after hydration');
        assert.deepEqual(closed, [10, 11]);
    } finally { restore(); }
});

test('current and candidate sources close independently on acceptance, stale load and parse failure', async () => {
    let source = 0;
    const streams = new Map();
    const closed = [];
    const restore = installHost(async (command, args) => {
        if (command === 'open_cold_chat') {
            source++;
            streams.set(source + 100, byteStream(source === 3 ? '{invalid' : JSON.stringify({chat_metadata:{}}) + '\n' + JSON.stringify(cold(source))));
            return { sourceId:source, readerId:source + 100 };
        }
        if (command === 'read_chat_bytes') return streams.get(args.rid).shift();
        if (command === 'plugin:resources|close') { closed.push(args.rid); return; }
        throw new Error(command);
    });
    try {
        const active = await loadColdChatPayload({kind:'group',chatId:'active'}, false);
        active.shift();
        acceptColdChatPayload(active);
        discardColdChatPayload(active);
        assert.deepEqual(closed, [101]);
        const stale = await loadColdChatPayload({kind:'group',chatId:'stale'}, false);
        discardColdChatPayload(stale);
        assert.deepEqual(closed, [101, 102, 2]);
        await assert.rejects(loadColdChatPayload({kind:'group',chatId:'invalid'}, false), /Invalid JSONL/);
        assert.ok(closed.includes(3));
        assert.ok(!closed.includes(1));
        releaseCurrentSwipeSource();
        assert.equal(closed.at(-1), 1);
    } finally { restore(); }
});

test('hydration rejects shortened arrays without partially filling either array', async () => {
    let stream;
    const restore = installHost(async (command) => {
        if (command === 'open_cold_swipe_record') { stream = byteStream(JSON.stringify(original)); return 10; }
        if (command === 'read_chat_bytes') return stream.shift();
        if (command === 'plugin:resources|close') return;
        throw new Error(command);
    });
    try {
        const message = cold();
        message.swipe_info.pop();
        const before = structuredClone(message);
        await assert.rejects(hydrateMessageSwipes(message), /shortened/);
        assert.deepEqual(message, before);
    } finally { restore(); }
});
