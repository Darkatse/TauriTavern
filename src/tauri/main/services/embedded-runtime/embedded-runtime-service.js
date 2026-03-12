// @ts-check

import { createEmbeddedRuntimeManager } from './embedded-runtime-manager.js';
import { resolveEmbeddedRuntimeProfile } from './embedded-runtime-profiles.js';

const GLOBAL_KEY = '__TAURITAVERN_EMBEDDED_RUNTIME__';

export function createEmbeddedRuntimeService() {
    const profile = resolveEmbeddedRuntimeProfile();
    const manager = createEmbeddedRuntimeManager({
        profile,
        now: () => globalThis.performance?.now?.() ?? Date.now(),
    });

    /** @type {any} */ (globalThis)[GLOBAL_KEY] = manager;

    return {
        profile,
        manager,
    };
}
