// @ts-check

import { CHAT_SURFACE_PROTOCOL_VERSION } from '../services/chat-surface/participant-registry.js';
import {
    getChatSurfaceParticipantRegistry,
    getChatSurfaceContentPreparation,
    isManagedChatSurfaceOwnershipRequired,
} from '../services/chat-surface/runtime.js';

/** @param {TauriTavernHostApi} api */
export function installChatSurfaceApi(api) {
    const registry = getChatSurfaceParticipantRegistry();
    api.chatSurface = Object.freeze({
        protocolVersion: CHAT_SURFACE_PROTOCOL_VERSION,
        isManagedOwnershipRequired: isManagedChatSurfaceOwnershipRequired,
        registerParticipant: registry.register,
        registerContentProcessor: /** @param {any} definition */ definition => getChatSurfaceContentPreparation().register(definition),
    });
}
