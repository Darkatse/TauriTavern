import test from 'node:test';
import assert from 'node:assert/strict';
import { applyOutputPatches } from '../src/scripts/tauritavern/legacy-output-revision.js';

const patch = (old_string, new_string) => ({
    function: { name: 'apply_patch', arguments: JSON.stringify({ old_string, new_string }) },
});

test('output patches apply in order and preserve literal replacement text', () => {
    assert.equal(applyOutputPatches('Hello {{char}}.\r\nAn abrupt ending.', [
        patch('An abrupt ending.', 'A quiet ending.'),
        { function: { name: 'apply_patch', arguments: { old_string: 'quiet', new_string: '$& $$ {{user}}' } } },
        patch('Hello ', ''),
    ]), '{{char}}.\r\nA $& $$ {{user}} ending.');
});

test('output patches reject an unusable batch without returning a partial revision', () => {
    const text = 'A beginning. The end. The end.';
    for (const invalid of [
        patch('missing', 'replacement'),
        patch('The end.', 'replacement'),
        patch('', 'replacement'),
        patch('A new beginning.', null),
        { function: { name: 'another_tool', arguments: '{}' } },
        { function: { name: 'apply_patch', arguments: '{' } },
    ]) {
        assert.throws(() => applyOutputPatches(text, [patch('A beginning.', 'A new beginning.'), invalid]));
    }
    assert.throws(() => applyOutputPatches(text, []), /did not return an apply_patch/);
});
