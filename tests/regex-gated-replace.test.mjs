import test from 'node:test';
import assert from 'node:assert/strict';
import { gatedReplace } from '../src/scripts/tauri/regex/gated-replace.js';
import { requirementsOf } from '../src/scripts/tauri/regex/pattern-requirements.js';

/** Echoes every replacer argument so a divergence in captures, offsets or groups shows up. */
const echo = (...args) => JSON.stringify(args);

test('requirementsOf derives the literals every match of an alternative must contain', () => {
    const requirements = source => requirementsOf(source).map(alternative => alternative.requirements);

    assert.deepEqual(requirements('<think>[\\s\\S]*?(?:</think>|</thinking>)'), [[['<think>'], ['</think>', '</thinking>']]]);
    assert.deepEqual(requirements('ab+c|(?<=<a>)x(?=</a>)|.*'), [[['ab'], ['bc']], [['x'], ['</a>']], []]);
    // `\u{…}` is a code point with `u` and literal text without; surrogate halves repeat differently.
    assert.deepEqual(requirements('\\u{1F600}x😀+'), [[['x']]]);
    assert.equal(requirementsOf('(?i:a)'), null);
});

test('gatedReplace matches String.prototype.replace', () => {
    const cases = [
        // Dead alternatives are neutralised without disturbing capture numbering.
        ['(<a>)|(<b>)|(<c>)', 'g', '<b> <c> <b>'],
        ['(<a>)|(<b>)', '', 'x <b>'],
        ['(?<tag><\\w+>)|(?<end></\\w+>)', 'g', '<b>x</b>'],
        // Nothing can match.
        ['[\\s\\S]*?</guide>|<guide>[\\s\\S]*?</guide>', 'gi', 'no guide here'.repeat(20)],
        // The global search stops after the last required literal.
        ['([\\s\\S]*)</think>', 'g', `a</think>b</think>${'c'.repeat(200)}`],
        // Overlapping occurrences count: `aa` at 1 after a match ending at 1.
        ['a(?=aa)|aa', 'g', 'aaa'],
        ['A(?=aa)|aa', 'gi', 'aAa'],
        // Empty matches advance by code point under `u`.
        ['x*', 'gu', '😀a😀'],
        ['x*', 'g', '😀a😀'],
        // Case folding follows the flags: `ſ` folds to `s` only with `u`.
        ['<ſ>', 'giu', '<s> <S>'],
        ['<ſ>', 'gi', '<s> <ſ>'],
        // Annex B: without `u`, `\u{2}` is "uu".
        ['\\u{2}x', 'g', 'uux ux'],
    ];

    for (const [source, flags, text] of cases) {
        assert.equal(
            gatedReplace(text, new RegExp(source, flags), echo),
            text.replace(new RegExp(source, flags), echo),
            `/${source}/${flags}`,
        );
    }
});
