import test from 'node:test';
import assert from 'node:assert/strict';

import { assertRequiredChatSurfaceParticipants } from '../src/tauri/main/services/chat-surface/capability-gate.js';
import { getChatSurfaceParticipantRegistry } from '../src/tauri/main/services/chat-surface/runtime.js';

test('bounded capability gate requires the exact protocol-v1 participant identity', () => {
    const requirement = [{
        extensionName: 'ExampleRenderer',
        participantId: 'example/message-runtime',
    }];
    assert.throws(
        () => assertRequiredChatSurfaceParticipants(requirement),
        /requires extension "ExampleRenderer".*protocol v1 participant "example\/message-runtime"/,
    );

    getChatSurfaceParticipantRegistry().register({
        id: 'example/message-runtime',
        protocolVersion: 1,
        prepareContent() {},
    });
    assertRequiredChatSurfaceParticipants(requirement);
});
