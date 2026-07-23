// @ts-check

import { getDefaultChatSurfaceParticipantRegistry } from './participant-registry.js';
import { isChatVirtualizationEnabled } from './chat-virtualization-state.js';

/** @type {any} */
let controller = null;

export function getChatSurfaceParticipantRegistry() {
    return getDefaultChatSurfaceParticipantRegistry();
}

export function isManagedChatSurfaceOwnershipRequired() {
    return isChatVirtualizationEnabled();
}

/** @param {any} nextController */
export function installChatSurfaceController(nextController) {
    if (!nextController || typeof nextController !== 'object') {
        throw new TypeError('ChatSurface controller is required');
    }
    if (controller && controller !== nextController) {
        throw new Error('ChatSurface controller is already installed');
    }
    controller = nextController;
    return controller;
}

export function getInstalledChatSurfaceController() {
    return controller;
}
