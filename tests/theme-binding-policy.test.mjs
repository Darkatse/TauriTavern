import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { resolveThemeBinding } from '../src/scripts/theme-binding-policy.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('theme binding resolves chat, entity, then fallback and reports missing references', () => {
    const candidates = [
        { scope: 'chat', name: 'Missing chat theme' },
        { scope: 'character', name: 'Character theme' },
        { scope: 'fallback', name: 'Fallback theme' },
    ];

    assert.deepEqual(resolveThemeBinding(candidates, ['Character theme', 'Fallback theme']), {
        selected: candidates[1],
        missing: [candidates[0]],
    });
    assert.deepEqual(resolveThemeBinding(candidates, ['Fallback theme']), {
        selected: candidates[2],
        missing: candidates.slice(0, 2),
    });
    assert.deepEqual(resolveThemeBinding(candidates, []), {
        selected: null,
        missing: candidates,
    });
});
