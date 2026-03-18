// @ts-check

import { createEmbeddedRuntimeManager } from './embedded-runtime-manager.js';
import { resolveEmbeddedRuntimeProfile } from './embedded-runtime-profiles.js';

const GLOBAL_KEY = '__TAURITAVERN_EMBEDDED_RUNTIME__';

/**
 * @param {{ profileName: string }} options
 */
export function createEmbeddedRuntimeService({ profileName }) {
    const profile = resolveEmbeddedRuntimeProfile(profileName);
    const root = document.querySelector('#chat');
    if (!(root instanceof HTMLElement)) {
        throw new Error('EmbeddedRuntimeService: #chat root not found');
    }
    const manager = createEmbeddedRuntimeManager({
        profile,
        now: () => globalThis.performance?.now?.() ?? Date.now(),
        root,
    });

    /** @type {any} */ (globalThis)[GLOBAL_KEY] = manager;

    return {
        profile,
        manager,
    };
}
