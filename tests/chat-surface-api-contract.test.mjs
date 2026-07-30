import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { CHAT_SURFACE_PROTOCOL_VERSION } from '../src/tauri/main/services/chat-surface/participant-registry.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('ChatSurface exposes only the three raw Project API members', async () => {
    assert.equal(CHAT_SURFACE_PROTOCOL_VERSION, 1);
    const source = await readFile(path.join(REPO_ROOT, 'src/tauri/main/api/chat-surface.js'), 'utf8');
    assert.match(source, /protocolVersion:\s*CHAT_SURFACE_PROTOCOL_VERSION/);
    assert.match(source, /isManagedOwnershipRequired:\s*isManagedChatSurfaceOwnershipRequired/);
    assert.match(source, /registerParticipant:\s*registry\.register/);
    assert.doesNotMatch(source, /project|virtual|range|revision|mountKey|chatEpoch/);
});
