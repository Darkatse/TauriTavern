// @ts-check

import { getChatSurfaceParticipantRegistry } from './runtime.js';
import { CHAT_SURFACE_PROTOCOL_VERSION } from './participant-registry.js';

/**
 * Verifies capabilities only after the extension activation hook has returned.
 * Registry details stay internal; the public participant API remains narrow.
 *
 * @param {Array<{ extensionName: string; participantId: string }>} requirements
 */
export function assertRequiredChatSurfaceParticipants(requirements) {
    if (!Array.isArray(requirements)) {
        throw new TypeError('ChatSurface capability requirements must be an array');
    }
    const registry = getChatSurfaceParticipantRegistry();
    for (const requirement of requirements) {
        if (
            !requirement
            || typeof requirement.extensionName !== 'string'
            || typeof requirement.participantId !== 'string'
        ) {
            throw new TypeError('ChatSurface renderer capability is malformed');
        }
        if (!registry.has(requirement.participantId)) {
            throw new Error(
                `Bounded ChatSurface requires extension "${requirement.extensionName}" `
                + `to register protocol v${CHAT_SURFACE_PROTOCOL_VERSION} participant "${requirement.participantId}"`,
            );
        }
    }
}
