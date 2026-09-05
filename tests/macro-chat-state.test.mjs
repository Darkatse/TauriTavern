import test from 'node:test';
import assert from 'node:assert/strict';
import { findLastMessageId, getLastSwipeId, getCurrentSwipeId } from '../src/scripts/macros/chat-state.js';

test('chat macros distinguish absent swipes from completed and pending swipes', () => {
    const user = { is_user: true, mes: 'Hello' };
    const cases = [
        { chat: [], expected: [null, null, null] },
        { chat: [{ mes: 'Greeting' }, user], expected: [1, null, null] },
        { chat: [user, { swipes: ['One', 'Two'], swipe_id: 0 }], expected: [1, 2, 1] },
        { chat: [user, { swipes: ['One', 'Two'], swipe_id: 2 }], expected: [0, 2, 3] },
    ];

    for (const { chat, expected } of cases) {
        assert.deepEqual([
            findLastMessageId(chat),
            getLastSwipeId(chat),
            getCurrentSwipeId(chat),
        ], expected);
    }

    const chat = [user, { swipes: ['One'], swipe_id: 1 }];
    assert.equal(findLastMessageId(chat, { excludePendingSwipe: false }), 1);
    assert.equal(findLastMessageId(chat, { filter: message => !message.is_user }), null);
    assert.equal(findLastMessageId(chat, {
        excludePendingSwipe: false,
        filter: message => !message.is_user,
    }), 1);
});
