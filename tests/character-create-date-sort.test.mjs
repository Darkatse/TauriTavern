import test from 'node:test';
import assert from 'node:assert/strict';

import { compareCreateDateKeysAscending } from '../src/scripts/util/compare-create-date.js';

test('compareCreateDateKeysAscending orders by primaryMs (oldest first)', () => {
    const older = { primaryMs: 1_000, fallbackMs: 9_999, avatar: 'a.png', name: 'A' };
    const newer = { primaryMs: 2_000, fallbackMs: 1, avatar: 'b.png', name: 'B' };

    assert.ok(compareCreateDateKeysAscending(older, newer) < 0);
    assert.ok(compareCreateDateKeysAscending(newer, older) > 0);
});

test('compareCreateDateKeysAscending breaks ties by fallbackMs', () => {
    const a = { primaryMs: 1_000, fallbackMs: 1_001, avatar: 'a.png', name: 'A' };
    const b = { primaryMs: 1_000, fallbackMs: 1_002, avatar: 'b.png', name: 'B' };

    assert.ok(compareCreateDateKeysAscending(a, b) < 0);
    assert.ok(compareCreateDateKeysAscending(b, a) > 0);
});

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

test('compareCreateDateKeysAscending returns 0 for identical keys', () => {
    const a = { primaryMs: 1_000, fallbackMs: 2_000, avatar: 'a.png', name: 'A' };
    const b = { primaryMs: 1_000, fallbackMs: 2_000, avatar: 'a.png', name: 'A' };

    assert.equal(compareCreateDateKeysAscending(a, b), 0);
});
