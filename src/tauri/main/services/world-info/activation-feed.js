// @ts-check

import { subscribeFinalWorldInfoScans } from '../../adapters/st/world-info.js';

/** @type {Set<(batch: any) => void>} */
const subscribers = new Set();

/** @type {any | null} */
let lastActivation = null;
let installed = false;

/**
 * @param {unknown} position
 */
function normalizePosition(position) {
    switch (Number(position)) {
        case 0:
            return 'before';
        case 1:
            return 'after';
        case 2:
            return 'an_top';
        case 3:
            return 'an_bottom';
        case 4:
            return 'depth';
        case 5:
            return 'em_top';
        case 6:
            return 'em_bottom';
        case 7:
            return 'outlet';
        default:
            return undefined;
    }
}

/**
 * @param {any} entry
 */
function normalizeDisplayName(entry) {
    const comment = String(entry?.comment || '').trim();
    if (comment) {
        return comment;
    }

    if (Array.isArray(entry?.key)) {
        const key = entry.key.find((/** @type {any} */ value) => String(value || '').trim());
        if (key !== undefined) {
            return String(key).trim();
        }
    }

    return String(entry?.uid ?? '').trim();
}

/**
 * @param {any} entry
 */
function normalizeEntry(entry) {
    const position = normalizePosition(entry?.position);
    return {
        world: String(entry?.world || '').trim(),
        uid: typeof entry?.uid === 'number' ? entry.uid : String(entry?.uid ?? '').trim(),
        displayName: normalizeDisplayName(entry),
        constant: Boolean(entry?.constant),
        ...(position ? { position } : {}),
    };
}

/**
 * @param {any} payload
 */
function normalizeActivationBatch(payload) {
    const entries = Array.from(payload?.activated?.entries?.values?.() ?? []).map(normalizeEntry);
    return {
        timestampMs: Date.now(),
        trigger: String(payload?.trigger || 'normal').trim() || 'normal',
        entries,
    };
}

/**
 * @param {any} batch
 */
function publish(batch) {
    lastActivation = batch;
    for (const handler of subscribers) {
        handler(batch);
    }
}

export function installWorldInfoActivationFeed() {
    if (installed) {
        return;
    }

    installed = true;
    subscribeFinalWorldInfoScans((payload) => {
        publish(normalizeActivationBatch(payload));
    });
}

export function getLastWorldInfoActivation() {
    return lastActivation;
}

/**
 * @param {(batch: any) => void} handler
 */
export function subscribeWorldInfoActivations(handler) {
    if (typeof handler !== 'function') {
        throw new Error('handler must be a function');
    }

    installWorldInfoActivationFeed();
    subscribers.add(handler);
    return () => subscribers.delete(handler);
}
