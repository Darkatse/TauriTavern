// @ts-check

import { getDefaultChatSurfaceParticipantRegistry } from './participant-registry.js';
import { isChatVirtualizationEnabled } from './chat-virtualization-state.js';

/** @type {any} */
let controller = null;
/** @type {ReturnType<import('./content-preparation.js').createContentPreparation>} */
let contentPreparation;

export function getChatSurfaceParticipantRegistry() {
    return getDefaultChatSurfaceParticipantRegistry();
}

export function isManagedChatSurfaceOwnershipRequired() {
    return isChatVirtualizationEnabled();
}

/** @param {any} nextController @param {typeof contentPreparation} preparation */
export function installChatSurfaceController(nextController, preparation) {
    if (!nextController || typeof nextController !== 'object') {
        throw new TypeError('ChatSurface controller is required');
    }
    if (controller && controller !== nextController) {
        throw new Error('ChatSurface controller is already installed');
    }
    controller = nextController;
    contentPreparation = preparation;
    return controller;
}

export function getInstalledChatSurfaceController() {
    return controller;
}

export function getChatSurfaceContentPreparation() {
    return contentPreparation;
}
