import { invoke } from '../../../tauri-bridge.js';
import { createReadableFileStreamService } from '../../../tauri/main/services/files/readable-file-stream-service.js';
import { jsonlStreamToPayload } from './jsonl.js';

const { createChatByteStream } = createReadableFileStreamService({ invoke });
const pendingSources = new WeakMap();
let currentSource;
let enabled = false;

export function initializeColdSwipes(settings) {
    enabled = settings.cold_swipes_enabled === true;
}

export function coldSwipesEnabled() {
    return enabled;
}

function closeSource(sourceId) {
    if (sourceId === undefined) return;
    // Cleanup cannot invalidate a successfully loaded chat. Reload also closes page resources natively.
    void invoke('plugin:resources|close', { rid: sourceId }).catch(error => {
        console.warn('Failed to close cold swipe source', error);
    });
}

export function releaseCurrentSwipeSource() {
    const previous = currentSource;
    currentSource = undefined;
    closeSource(previous);
}

export function acceptColdChatPayload(payload) {
    const next = pendingSources.get(payload);
    pendingSources.delete(payload);
    const previous = currentSource;
    currentSource = next;
    if (previous !== next) closeSource(previous);
}

export function discardColdChatPayload(payload) {
    const source = pendingSources.get(payload);
    pendingSources.delete(payload);
    closeSource(source);
}

export async function loadColdChatPayload(target, allowNotFound) {
    const opened = await invoke('open_cold_chat', { target, allowNotFound });
    if (!opened) return [];
    try {
        const payload = await jsonlStreamToPayload(createChatByteStream(opened.readerId));
        if (payload.some(message => message.tt_swipe_cold)) {
            pendingSources.set(payload, opened.sourceId);
        } else {
            closeSource(opened.sourceId);
        }
        return payload;
    } catch (error) {
        closeSource(opened.sourceId);
        throw error;
    }
}

export async function readColdSwipeRecord(reference) {
    const rid = await invoke('open_cold_swipe_record', reference);
    const records = await jsonlStreamToPayload(createChatByteStream(rid));
    if (records.length !== 1) throw new Error('Cold swipe source did not return one message');
    return records[0];
}

/** Fill unloaded slots in place; live values and appended slots remain authoritative. */
export async function hydrateMessageSwipes(message) {
    if (!message?.tt_swipe_cold) return;
    const reference = { sourceId: message.tt_swipe_cold.sourceId, record: message.tt_swipe_cold.record };
    const original = await readColdSwipeRecord({ sourceId: reference.sourceId, record: reference.record });
    const current = message.tt_swipe_cold;
    if (!current) return;
    const keys = ['swipes', 'swipe_info'];
    for (const key of keys) {
        if (!Array.isArray(message[key]) || message[key].length < original[key].length) {
            throw new Error('Cold swipe array shortened; load swipes before deleting slots');
        }
    }
    for (const key of keys) {
        for (let i = 0; i < original[key].length; i++) {
            message[key][i] ??= original[key][i];
        }
    }
    delete message.tt_swipe_cold;
}

/** The capture runs synchronously alongside JSON serialization; one commit has one opened source. */
export function coldSourceForPayload(payload) {
    let sourceId;
    for (const message of payload) {
        const reference = message?.tt_swipe_cold;
        if (!reference) continue;
        if (!Number.isInteger(reference.sourceId) || reference.sourceId < 0) {
            throw new Error('Cold swipe message has no source');
        }
        if (sourceId !== undefined && sourceId !== reference.sourceId) {
            throw new Error('Cannot save a chat with mixed cold swipe sources');
        }
        sourceId = reference.sourceId;
    }
    return sourceId;
}
