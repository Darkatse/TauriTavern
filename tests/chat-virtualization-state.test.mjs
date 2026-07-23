import test from 'node:test';
import assert from 'node:assert/strict';

const MODULE_URL = new URL(
    '../src/tauri/main/services/chat-surface/chat-virtualization-state.js',
    import.meta.url,
);

test('chat virtualization defaults at the settings boundary and freezes for the page lifetime', async () => {
    const state = await import(`${MODULE_URL.href}?disabled`);

    assert.throws(() => state.isChatVirtualizationEnabled(), /not initialized/);
    assert.equal(state.initializeChatVirtualization({ chat_virtualization_enabled: false }), false);
    assert.equal(state.isChatVirtualizationEnabled(), false);
    assert.equal(state.initializeChatVirtualization({ chat_virtualization_enabled: false }), false);
    assert.throws(
        () => state.initializeChatVirtualization({ chat_virtualization_enabled: true }),
        /changed during this page lifetime/,
    );
});

test('chat virtualization accepts only the canonical boolean setting', async () => {
    const state = await import(`${MODULE_URL.href}?enabled`);

    assert.throws(() => state.initializeChatVirtualization({}), /must be a boolean/);
    assert.equal(state.initializeChatVirtualization({ chat_virtualization_enabled: true }), true);
    assert.equal(state.isChatVirtualizationEnabled(), true);
});
