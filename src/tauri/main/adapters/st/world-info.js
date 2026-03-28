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
