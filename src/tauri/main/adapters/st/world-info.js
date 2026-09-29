// @ts-check

import { eventSource, event_types } from '../../../../scripts/events.js';

/**
 * @param {(payload: any) => void} handler
 */
export function subscribeFinalWorldInfoScans(handler) {
    if (typeof handler !== 'function') {
        throw new Error('handler must be a function');
    }

    const listener = /** @param {any} payload */ (payload) => {
        if (!payload?.isFinal || payload?.isDryRun) {
            return;
        }

        handler(payload);
    };

    eventSource.on(event_types.WORLDINFO_SCAN_DONE, listener);
    return () => eventSource.removeListener(event_types.WORLDINFO_SCAN_DONE, listener);
}

/**
 * @param {{ world: string; uid: string | number }} ref
 */
export async function openWorldInfoEntry(ref) {
    const { openWorldInfoEntry } = await import('../../../../scripts/world-info.js');
    if (typeof openWorldInfoEntry !== 'function') {
        throw new Error('world-info openWorldInfoEntry() is unavailable');
    }

    return openWorldInfoEntry(ref.world, ref.uid);
}

/**
 * Brings the World Info selects up to date after Panel Runtime reattached them.
 */
export async function syncWorldInfoListOptions() {
    const { syncWorldInfoListOptions } = await import('../../../../scripts/world-info.js');
    if (typeof syncWorldInfoListOptions !== 'function') {
        throw new Error('world-info syncWorldInfoListOptions() is unavailable');
    }

    // The drawer may have closed again while the import settled; its next hydrate syncs instead.
    if (!document.getElementById('world_info')) {
        return;
    }

    syncWorldInfoListOptions();
}
