import assert from 'node:assert/strict';
import test from 'node:test';

import { prepareWorldInfoEntries } from '../src/scripts/world-info-entry-prepare.js';

function legacyPrepare(entries, parseDecorators, getStringHash) {
    return entries.map((entry) => {
        const [decorators, content] = parseDecorators(entry.content || '');
        return { ...entry, decorators, content };
    }).map((entry) => {
        const hash = getStringHash(JSON.stringify(entry));
        return { ...entry, hash };
    });
}

test('world info entry preparation preserves legacy objects and hash inputs', () => {
    const entries = [
        { uid: 1, world: 'global', content: '@@@depth 2\nalpha', enabled: true, order: 10 },
        { uid: '2', world: 'chat', content: '', probability: 75, nested: { value: true } },
        { uid: 3, world: 'persona', content: null, hash: 123, extra: ['a', 'b'] },
    ];
    const parseDecorators = (content) => {
        const lines = String(content).split('\n');
        const decorators = lines.filter(line => line.startsWith('@@@'));
        return [decorators, lines.filter(line => !line.startsWith('@@@')).join('\n')];
    };
    const legacyHashInputs = [];
    const optimizedHashInputs = [];
    const hash = value => [...value].reduce((total, char) => (total * 31 + char.charCodeAt(0)) | 0, 0);

    const legacy = legacyPrepare(entries, parseDecorators, (value) => {
        legacyHashInputs.push(value);
        return hash(value);
    });
    const optimized = prepareWorldInfoEntries(entries, parseDecorators, (value) => {
        optimizedHashInputs.push(value);
        return hash(value);
    });

    assert.deepEqual(optimizedHashInputs, legacyHashInputs);
    assert.deepEqual(optimized, legacy);
    assert.deepEqual(entries[0], { uid: 1, world: 'global', content: '@@@depth 2\nalpha', enabled: true, order: 10 });
});
