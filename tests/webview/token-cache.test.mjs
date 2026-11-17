import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

const { window, load, startHost } = await createFrontend();

await startHost();
const { eventSource, event_types, changeMainAPI, setOnlineStatus } = await load('script.js');
const { power_user } = await load('scripts/power-user.js');
const textgen = await load('scripts/textgen-settings.js');
const tokenizers = await load('scripts/tokenizers.js');
await tokenizers.initTokenizers();

// Buckets outlive the page, so every run counts its own texts.
const nonce = window.crypto.randomUUID();
let fallback = false;
let counted = [];
const serverCounts = { 'model-a': 11, 'model-b': 29 };
let serverModels = [];
const ajax = window.jQuery.ajax;
window.jQuery.ajax = options => {
    if (options.url?.startsWith('/api/tokenizers/remote/textgenerationwebui/encode')) {
        const { text, model } = JSON.parse(options.data);
        if (!text.startsWith(nonce)) {
            return ajax(options);
        }
        serverModels.push(model);
        queueMicrotask(() => options.success({ count: serverCounts[model], ids: [], fallback: false }));
        return;
    }
    if (!options.url?.startsWith('/api/tokenizers/openai/count-batch')) {
        return ajax(options);
    }
    const messages = JSON.parse(options.data);
    counted.push(...messages.map(message => message.content.slice(nonce.length + 1)));
    return Promise.resolve({ token_counts: messages.map(() => 5), fallback });
};

/** Counts the texts in the chat in use, and returns the texts the backend counted. */
async function count(...texts) {
    counted = [];
    await tokenizers.countOpenAIMessageTokensBatchAsync(texts.map(text => ({ role: 'user', content: `${nonce} ${text}` })));
    return counted;
}

const leaveChat = () => eventSource.emit(event_types.CHAT_CHANGED, 'another chat');

test('a chat left keeps the counts it used', async () => {
    expect(await count('one', 'two')).toEqual(['one', 'two']);
    await leaveChat();
    expect(await count('one')).toEqual([]);
    expect(await count('three')).toEqual(['three']);
    await leaveChat();
    expect(await count('one', 'two', 'three')).toEqual(['two']);
});

test('fallback counts are counted again', async () => {
    fallback = true;
    expect(await count('four')).toEqual(['four']);
    expect(await count('four')).toEqual(['four']);
    fallback = false;
    expect(await count('four')).toEqual(['four']);
    expect(await count('four')).toEqual([]);
});

test('the API tokenizer counts with the model the server serves, however it is selected', async () => {
    const { main_api: previousMainApi } = await load('script.js');
    const settings = textgen.textgenerationwebui_settings;
    const previous = { tokenizer: power_user.tokenizer, type: settings.type, model: settings.llamacpp_model };
    try {
        changeMainAPI('textgenerationwebui');
        settings.type = textgen.textgen_types.LLAMACPP;
        setOnlineStatus('Connected');
        for (const selection of [tokenizers.tokenizers.API_CURRENT, tokenizers.tokenizers.BEST_MATCH]) {
            power_user.tokenizer = selection;
            serverModels = [];
            const text = `${nonce} api ${selection}`;
            settings.llamacpp_model = 'model-a';
            expect(await tokenizers.getTokenCountAsync(text)).toBe(11);
            settings.llamacpp_model = 'model-b';
            expect(await tokenizers.getTokenCountAsync(text)).toBe(29);
            expect(await tokenizers.getTokenCountAsync(text)).toBe(29);
            expect(serverModels).toEqual(['model-a', 'model-b']);
        }
    } finally {
        power_user.tokenizer = previous.tokenizer;
        settings.type = previous.type;
        settings.llamacpp_model = previous.model;
        changeMainAPI(previousMainApi);
    }
});
