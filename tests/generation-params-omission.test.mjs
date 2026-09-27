import assert from 'node:assert/strict';
import test from 'node:test';
import { OMITTED_MAX_TOKENS_BUDGET } from '../src/scripts/tauri/generation-params/catalog.js';
import {
    applyParamOmissions,
    getEffectiveGenerationSettings,
    setParamOmitted,
} from '../src/scripts/tauri/generation-params/omission.js';

const payload = () => ({ messages: [], model: 'm', temperature: 1, top_k: 0, seed: 7 });

test('omitted keys are removed from the payload and restore cleanly', () => {
    const settings = { extensions: { other: { keep: true } } };
    assert.deepEqual(applyParamOmissions(payload(), settings), payload());

    setParamOmitted(settings, 'top_k', true);
    setParamOmitted(settings, 'seed', true);
    assert.deepEqual(applyParamOmissions(payload(), settings), { messages: [], model: 'm', temperature: 1 });

    setParamOmitted(settings, 'top_k', false);
    setParamOmitted(settings, 'seed', false);
    assert.deepEqual(settings.extensions.other, { keep: true });
    assert.deepEqual(applyParamOmissions(payload(), settings), payload());
});

test('foreign keys in the preset can never strip structural payload fields', () => {
    const settings = { extensions: { tauritavern: { omit_params: ['messages', 'model', 'temperature', 42] } } };
    assert.deepEqual(applyParamOmissions(payload(), settings), { messages: [], model: 'm', top_k: 0, seed: 7 });
});

test('removed settings stop affecting generation without overwriting the preset', () => {
    const settings = { n: 4, assistant_prefill: 'Prefill', assistant_impersonation: 'Impersonate', reasoning_effort: 'high' };
    for (const key of ['n', 'assistant_prefill', 'reasoning_effort']) setParamOmitted(settings, key, true);
    const effective = getEffectiveGenerationSettings(settings);
    assert.equal(effective.n, 1);
    assert.equal(effective.assistant_prefill, '');
    assert.equal(effective.assistant_impersonation, '');
    assert.equal(effective.reasoning_effort, 'auto');
    assert.equal(settings.n, 4);
    assert.equal(settings.assistant_prefill, 'Prefill');
    setParamOmitted(settings, 'n', false);
    assert.equal(getEffectiveGenerationSettings(settings).n, 4);
});

test('an omitted max_tokens leaves the request under every name and keeps a budget reserve', () => {
    const settings = { openai_max_tokens: 65535 };
    setParamOmitted(settings, 'max_tokens', true);
    assert.deepEqual(applyParamOmissions({ messages: [], max_completion_tokens: 65535 }, settings), { messages: [] });
    assert.equal(getEffectiveGenerationSettings(settings).openai_max_tokens, OMITTED_MAX_TOKENS_BUDGET);
    assert.equal(settings.openai_max_tokens, 65535);
});
