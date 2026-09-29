// @ts-check

/** @type {Map<string, Set<() => void>>} */
const hooks = new Map();

/**
 * Registers work that must run right after Panel Runtime reattaches a parked panel.
 *
 * Writes aimed at a parked subtree cannot reach it, so the owner of that state
 * catches up here. Hooks run synchronously inside the restore, before the code
 * that opened the drawer touches the restored DOM.
 *
 * @param {string} panelId Id of the drawer content element, e.g. `WorldInfo`
 * @param {() => void} hook
 * @returns {() => void} Unregisters the hook
 */
export function registerPanelRestoreHook(panelId, hook) {
    if (!panelId || typeof hook !== 'function') {
        throw new TypeError('Panel restore hooks require a panel id and function');
    }

    let panelHooks = hooks.get(panelId);
    if (!panelHooks) {
        panelHooks = new Set();
        hooks.set(panelId, panelHooks);
    }
    panelHooks.add(hook);

    return () => {
        panelHooks.delete(hook);
    };
}

/**
 * @param {string} panelId
 */
export function runPanelRestoreHooks(panelId) {
    for (const hook of hooks.get(panelId) ?? []) {
        try {
            hook();
        } catch (error) {
            // One failing owner must not leave the drawer half restored.
            console.error(`PanelParking(${panelId}): restore hook failed:`, error);
        }
    }
}
