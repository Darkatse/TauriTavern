// @ts-check

import { createEmbeddedRuntimeManager } from '../embedded-runtime/embedded-runtime-manager.js';
import { resolvePanelRuntimeProfile } from './panel-runtime-profiles.js';

const GLOBAL_KEY = '__TAURITAVERN_PANEL_RUNTIME__';

/**
 * @param {{ profileName: string }} options
 */
export function createPanelRuntimeService({ profileName }) {
    const profile = resolvePanelRuntimeProfile(profileName);
    const manager = createEmbeddedRuntimeManager({
        profile,
        now: () => globalThis.performance?.now?.() ?? Date.now(),
        root: null,
    });

    /** @type {any} */ (globalThis)[GLOBAL_KEY] = manager;

    return {
        profile,
        manager,
    };
}
