import assert from 'node:assert/strict';
import test from 'node:test';
import {
    applyParamOmissions,
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
