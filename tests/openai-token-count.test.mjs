import assert from 'node:assert/strict';
import test from 'node:test';

import {
    getOpenAIConversationTokenCount,
    getOpenAITextTokenCount,
    hasReachedOpenAITextTokenLimit,
} from '../src/scripts/util/openai-token-count.js';

test('OpenAI token count helpers preserve text and conversation offsets', () => {
    assert.equal(getOpenAITextTokenCount(8), 7);
    assert.equal(getOpenAIConversationTokenCount([8, 13], true), 20);
    assert.equal(getOpenAIConversationTokenCount([8, 13]), 18);
});

test('OpenAI token limit uses the caller-visible text count', () => {
    assert.equal(hasReachedOpenAITextTokenLimit(13, 12), true);
    assert.equal(hasReachedOpenAITextTokenLimit(12, 12), false);
    assert.equal(hasReachedOpenAITextTokenLimit(13, undefined), false);
});
