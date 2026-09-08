import assert from 'node:assert/strict';
import test from 'node:test';
import {
    applyParamOmissions,
    getOmittedParams,
    setParamOmitted,
} from '../src/scripts/tauri/generation-params/omission.js';

const payload = () => ({ messages: [], model: 'm', temperature: 1, top_k: 0, seed: 7 });

test('presets without the field send every parameter unchanged', () => {
    for (const settings of [{}, { extensions: {} }, { extensions: { tauritavern: {} } }, null]) {
        assert.deepEqual(getOmittedParams(settings), []);
        assert.deepEqual(applyParamOmissions(payload(), settings), payload());
    }
});

test('omitted keys are removed from the payload and restore cleanly', () => {
    const settings = { extensions: { other: { keep: true } } };

    assert.equal(setParamOmitted(settings, 'top_k', true), true);
    assert.equal(setParamOmitted(settings, 'top_k', true), false);
    assert.equal(setParamOmitted(settings, 'seed', true), true);
    assert.deepEqual(settings.extensions.tauritavern.omit_params, ['top_k', 'seed']);
    assert.deepEqual(applyParamOmissions(payload(), settings), { messages: [], model: 'm', temperature: 1 });

    assert.equal(setParamOmitted(settings, 'top_k', false), true);
    assert.equal(setParamOmitted(settings, 'seed', false), true);
    assert.deepEqual(settings.extensions, { other: { keep: true } });
    assert.deepEqual(applyParamOmissions(payload(), settings), payload());
});

test('foreign keys in the preset can never strip structural payload fields', () => {
    const settings = { extensions: { tauritavern: { omit_params: ['messages', 'model', 'temperature', 42] } } };
    assert.deepEqual(getOmittedParams(settings), ['temperature']);
    assert.deepEqual(applyParamOmissions(payload(), settings), { messages: [], model: 'm', top_k: 0, seed: 7 });
    assert.throws(() => setParamOmitted(settings, 'messages', true), /Unknown request parameter/);
    // Toggles are value-driven, never omission-driven.
    assert.throws(() => setParamOmitted(settings, 'function_calling', true), /Unknown request parameter/);
});
