import test from 'node:test';
import assert from 'node:assert/strict';

import { compareCreateDateKeysAscending } from '../src/scripts/util/compare-create-date.js';

test('compareCreateDateKeysAscending falls back to fallbackMs when primaryMs is missing', () => {
    const hasPrimary = { primaryMs: 1_000, fallbackMs: 2_000, avatar: 'a.png', name: 'A' };
    const noPrimary = { primaryMs: null, fallbackMs: 1_500, avatar: 'b.png', name: 'B' };

    assert.ok(compareCreateDateKeysAscending(hasPrimary, noPrimary) < 0);
    assert.ok(compareCreateDateKeysAscending(noPrimary, hasPrimary) > 0);
});

test('compareCreateDateKeysAscending is deterministic via avatar/name tie-breakers', () => {
    const sameTimeA = { primaryMs: 1_000, fallbackMs: 1_000, avatar: 'a.png', name: 'B' };
    const sameTimeB = { primaryMs: 1_000, fallbackMs: 1_000, avatar: 'b.png', name: 'A' };

    assert.ok(compareCreateDateKeysAscending(sameTimeA, sameTimeB) < 0);
    assert.ok(compareCreateDateKeysAscending(sameTimeB, sameTimeA) > 0);

    const sameAvatarA = { primaryMs: 1_000, fallbackMs: 1_000, avatar: 'a.png', name: 'A' };
    const sameAvatarB = { primaryMs: 1_000, fallbackMs: 1_000, avatar: 'a.png', name: 'B' };
    assert.ok(compareCreateDateKeysAscending(sameAvatarA, sameAvatarB) < 0);
});
