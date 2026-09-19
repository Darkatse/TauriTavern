import assert from 'node:assert/strict';
import test from 'node:test';

import { reciprocalRankFusion } from '../src/scripts/extensions/vectors/rank-fusion.js';

test('reciprocal rank fusion boosts agreement without counting duplicate chunks twice', () => {
    assert.deepEqual(
        reciprocalRankFusion([[11, 11, 22], [22, 33]], 3),
        [22, 11, 33],
    );
});

test('reciprocal rank fusion keeps the first ranking as the deterministic tie breaker', () => {
    assert.deepEqual(reciprocalRankFusion([[11, 22], [33, 44]], 2), [11, 33]);
    assert.deepEqual(reciprocalRankFusion([[11]], 0), []);
});
