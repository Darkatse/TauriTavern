import { expect, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

const { window, load, startHost } = await createFrontend();

await startHost();
const { eventSource, event_types } = await load('script.js');
const tokenizers = await load('scripts/tokenizers.js');
await tokenizers.initTokenizers();

// Buckets outlive the page, so every run counts its own texts.
const nonce = window.crypto.randomUUID();
let fallback = false;
let counted = [];
const ajax = window.jQuery.ajax;
window.jQuery.ajax = options => {
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
