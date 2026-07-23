import test from 'node:test';
import assert from 'node:assert/strict';

import {
    CHAT_SURFACE_PROTOCOL_VERSION,
    createChatSurfaceParticipantRegistry,
    normalizeChatSurfaceParticipant,
} from '../src/tauri/main/services/chat-surface/participant-registry.js';

test('ChatSurface registry freezes startup participants and forwards explicit faults', () => {
    assert.equal(CHAT_SURFACE_PROTOCOL_VERSION, 1);
    for (const definition of [
        { id: 'missing-version', didMount() {} },
        { id: 'old', protocolVersion: 0, didMount() {} },
        { id: 'empty', protocolVersion: 1 },
        { id: 'old-hook', protocolVersion: 1, claimRuntimes() {} },
    ]) {
        assert.throws(() => normalizeChatSurfaceParticipant(definition));
    }

    const registry = createChatSurfaceParticipantRegistry();
    const registration = registry.register({
        id: 'extension/example',
        protocolVersion: 1,
        prepareContent() {},
    });
    assert.deepEqual(Object.keys(registration), ['fault']);
    assert.equal(registry.has('extension/example'), true);
    assert.throws(() => registry.register({
        id: 'extension/example',
        protocolVersion: 1,
        didMount() {},
    }), /already registered/);

    const faults = [];
    const participants = registry.freeze(error => faults.push(error));
    assert.deepEqual(participants.map(participant => participant.id), ['extension/example']);
    assert.ok(Object.isFrozen(participants));
    assert.throws(() => registry.register({
        id: 'late',
        protocolVersion: 1,
        didMount() {},
    }), /before the first projection/);

    registration.fault(new Error('renderer failed'));
    registration.fault(new Error('ignored second fault'));
    assert.equal(faults.length, 1);
    assert.match(faults[0].message, /extension\/example is faulted/);
    assert.throws(() => registry.freeze(() => {}), /extension\/example is faulted/);
});
