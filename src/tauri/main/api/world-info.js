// @ts-check

import { openWorldInfoEntry as openWorldInfoEntryAdapter } from '../adapters/st/world-info.js';
import {
    getLastWorldInfoActivation,
    installWorldInfoActivationFeed,
    subscribeWorldInfoActivations,
} from '../services/world-info/activation-feed.js';

/**
 * @param {any} ref
 */
function normalizeWorldInfoEntryRef(ref) {
    if (!ref || typeof ref !== 'object') {
        throw new Error('WorldInfo entry ref must be an object');
    }

    const world = String(ref.world || '').trim();
    if (!world) {
        throw new Error('world is required');
    }

    const rawUid = ref.uid;
    if (rawUid === null || rawUid === undefined) {
        throw new Error('uid is required');
    }

    const uid = typeof rawUid === 'number' ? rawUid : String(rawUid).trim();
    if (uid === '') {
        throw new Error('uid is required');
    }

    return { world, uid };
}

function createWorldInfoApi() {
    installWorldInfoActivationFeed();

    return {
        async getLastActivation() {
            return getLastWorldInfoActivation();
        },
        async subscribeActivations(handler) {
            return subscribeWorldInfoActivations(handler);
        },
        async openEntry(ref) {
            const normalized = normalizeWorldInfoEntryRef(ref);
            return {
                opened: await openWorldInfoEntryAdapter(normalized),
            };
        },
    };
}

export function installWorldInfoApi() {
    const hostWindow = /** @type {any} */ (window);
    const hostAbi = hostWindow.__TAURITAVERN__;
    if (!hostAbi || typeof hostAbi !== 'object') {
        throw new Error('Host ABI __TAURITAVERN__ is missing');
    }

    if (!hostAbi.api || typeof hostAbi.api !== 'object') {
        hostAbi.api = {};
    }

    hostAbi.api.worldInfo = createWorldInfoApi();
}
