import { getRequestHeaders } from '../script.js';
import { eventSource, event_types } from './events.js';
import { debounce_timeout } from './constants.js';
import { debounce, cancelDebounce } from './utils.js';
import { StructuredCloneMap } from './util/StructuredCloneMap.js';
import { registerLifecycleFlushHandler } from '../tauri/main/services/lifecycle/lifecycle-flush-service.js';

/** Working documents, including edits awaiting persistence. API reads still read disk. */
export const worldInfoCache = new StructuredCloneMap({ cloneOnGet: true, cloneOnSet: false });
const reads = new Map();
const dirty = new Map();
let revision = 0;
// Serialize document writes and deletes.
let writes = Promise.resolve();

function enqueueWrite(task) {
    const pending = writes.then(task);
    // One failed operation must not prevent a later save or retry.
    writes = pending.catch(() => {});
    return pending;
}

async function requireSuccess(response, name, operation) {
    if (response.ok) return;
    let message = await response.text();
    try {
        const error = JSON.parse(message);
        const detail = error?.error ?? error?.message;
        if (typeof detail === 'string') message = detail;
    } catch {
        // Command failures may have a plain-text body.
    }
    throw new Error(`World info ${operation} failed for "${name}" (${response.status}): ${message}`);
}

export async function loadWorldInfo(name) {
    name = String(name ?? '');
    if (name === '') return;
    if (worldInfoCache.has(name)) return worldInfoCache.get(name);
    if (reads.has(name)) return reads.get(name);

    const pending = (async () => {
        const response = await fetch('/api/worldinfo/get', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify({ name }),
            cache: 'no-cache',
        });
        await requireSuccess(response, name, 'read');
        const data = await response.json();
        if (reads.get(name) === pending) worldInfoCache.set(name, data);
        return data;
    })();
    reads.set(name, pending);
    try {
        return await pending;
    } finally {
        if (reads.get(name) === pending) reads.delete(name);
    }
}

/** Preload one book at a time. */
export async function prefetchWorldInfos(names) {
    for (const name of names) {
        if (name !== '' && !worldInfoCache.has(name)) await loadWorldInfo(name);
    }
}

async function persist(name, entry) {
    // Capture once, when this operation starts, before transport can yield.
    const body = JSON.stringify({ name, data: entry.data });
    const response = await fetch('/api/worldinfo/edit', {
        method: 'POST',
        headers: getRequestHeaders(),
        body,
    });
    await requireSuccess(response, name, 'save');
    if (dirty.get(name)?.revision === entry.revision) dirty.delete(name);
    return entry.data;
}

const saveDebounced = debounce(() => {
    flushWorldInfoSaves().catch(reportSaveError);
}, debounce_timeout.relaxed);

function reportSaveError(error) {
    console.error(error);
    toastr.error(error.message);
}

/** Flush all edits, or just the books needed by a read/generation operation. */
export async function flushWorldInfoSaves(reason = 'worldinfo_flush', names = null) {
    if (names === null) cancelDebounce(saveDebounced);
    const failures = [];
    for (const name of names === null ? [...dirty.keys()] : names) {
        try {
            const data = await enqueueWrite(() => {
                const entry = dirty.get(name);
                return entry ? persist(name, entry) : undefined;
            });
            // Observers can save again; do not hold the write queue while awaiting them.
            if (data !== undefined) await eventSource.emit(event_types.WORLDINFO_UPDATED, name, data);
        } catch (error) {
            failures.push(error);
        }
    }
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1) {
        throw new AggregateError(failures, `World info saves failed (${reason})`);
    }
}

export async function saveWorldInfo(name, data, immediately = false) {
    name = String(name ?? '');
    if (name === '' || !data) return;
    reads.delete(name);
    worldInfoCache.set(name, data);
    dirty.set(name, { data, revision: ++revision });
    if (immediately) {
        try {
            await flushWorldInfoSaves('worldinfo_immediate_save', [name]);
        } catch (error) {
            reportSaveError(error);
            throw error;
        }
    } else {
        saveDebounced();
    }
}

export function deleteWorldInfoDocument(name) {
    return enqueueWrite(async () => {
        const response = await fetch('/api/worldinfo/delete', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify({ name }),
        });
        if (!response.ok) return false;
        dirty.delete(name);
        reads.delete(name);
        worldInfoCache.delete(name);
        return true;
    });
}

// Iframe listeners read world info from disk through /api, so flush pending edits first.
for (const event of [
    event_types.CHAT_CHANGED,
    event_types.CHAT_LOADED,
    event_types.GENERATION_STARTED,
    event_types.GENERATE_BEFORE_COMBINE_PROMPTS,
]) {
    eventSource.makeFirst(event, () => flushWorldInfoSaves(`event:${event}`));
}

registerLifecycleFlushHandler('world-info', reason => {
    if (!dirty.size) return Promise.resolve();
    return flushWorldInfoSaves(reason).catch(reportSaveError);
});
