// @ts-check

import { installDomEmbeddedRuntimeAdapter } from './dom-runtime-adapter.js';
import { createJsSlashRunnerRuntimeAdapter } from './js-slash-runner-runtime-adapter.js';
import { createLittleWhiteBoxRuntimeAdapter } from './littlewhitebox-runtime-adapter.js';

/**
 * @typedef {import('../../services/embedded-runtime/embedded-runtime-manager.js').createEmbeddedRuntimeManager} createEmbeddedRuntimeManager
 * @typedef {ReturnType<createEmbeddedRuntimeManager>} EmbeddedRuntimeManager
 */

/**
 * Installs embedded runtime detectors for chat-scoped render extensions.
 *
 * @param {{ manager: EmbeddedRuntimeManager }} options
 */
export function installChatEmbeddedRuntimeAdapters({ manager }) {
    if (!manager) {
        throw new Error('installChatEmbeddedRuntimeAdapters requires manager');
    }

    const chat = document.querySelector('#chat');
    if (!(chat instanceof HTMLElement)) {
        throw new Error('installChatEmbeddedRuntimeAdapters: #chat not found');
    }

    return installDomEmbeddedRuntimeAdapter({
        manager,
        root: chat,
        adapters: [
            createJsSlashRunnerRuntimeAdapter(),
            createLittleWhiteBoxRuntimeAdapter(),
        ],
    });
}

