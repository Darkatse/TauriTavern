import { invoke } from '../../../tauri-bridge.js';
import { commitBytes } from '../../../tauri/main/services/files/byte-commit.js';
import { jsonlRecordsToByteChunks, serializeChatPayload } from './jsonl.js';
import { coldSourceForPayload } from './cold-swipes.js';

const INTEGRITY = 'integrity';

/**
 * Host commands reject with serde-tagged `CommandError` values: `{ Variant: payload }` plain objects
 * without `message` or `stack`. Every rejection leaves this module as an Error carrying the original
 * value as `cause`; the integrity conflict is the only one callers branch on, through `code`.
 */
function toChatCommitError(error) {
    if (error instanceof Error) {
        return error;
    }
    if (error?.BadRequest === INTEGRITY) {
        return Object.assign(new Error(INTEGRITY, { cause: error }), { code: INTEGRITY });
    }
    return new Error(typeof error === 'string' ? error : JSON.stringify(error), { cause: error });
}

function invokeChatCommit(...args) {
    return invoke(...args).catch((error) => {
        throw toChatCommitError(error);
    });
}

// [CHAT-COMMIT-PROGRESS-20261018] Every chat write funnels through commitChatRecords, so measuring
// here covers character/group x payload/metadata/metadataExtension. The queue-level
// `tt-record-save` keeps its meaning: how many writes are still queued.
const COMMIT_PROGRESS_EVENT = 'tt-chat-commit';
const MEASURE_CHUNK_CHARS = Math.floor(64 * 1024 / 3);
const textEncoder = new TextEncoder();
const measureScratch = new Uint8Array(MEASURE_CHUNK_CHARS * 3);

/** In-flight commits in start order; a single-line UI shows the first one. */
const activeCommits = new Set();

/**
 * UTF-8 length of one record, encoded into a fixed scratch buffer so a large payload never needs a
 * second copy of its own bytes. The only cost is one extra encode pass over the records.
 */
function utf8ByteLength(text) {
    let bytes = 0;
    let offset = 0;
    while (offset < text.length) {
        const end = Math.min(text.length, offset + MEASURE_CHUNK_CHARS);
        const { read, written } = textEncoder.encodeInto(text.slice(offset, end), measureScratch);
        if (read === 0) break; // Never spin on an encoder that refuses to advance.
        bytes += written;
        offset += read;
    }
    return bytes;
}

/** Mirrors textFragmentsToByteChunks: one '\n' between records, none after the last. */
function measureRecords(records) {
    let bytes = Math.max(0, records.length - 1);
    for (const record of records) bytes += utf8ByteLength(record);
    return bytes;
}

function emitCommitProgress() {
    // Contract tests install a stub window with no dispatchEvent; a browser always has one.
    if (typeof window?.dispatchEvent !== 'function' || typeof CustomEvent !== 'function') return;
    const first = activeCommits.values().next().value;
    try {
        window.dispatchEvent(new CustomEvent(COMMIT_PROGRESS_EVENT, {
            // Emit a copy: listeners must see the value at dispatch time, not a live object.
            detail: {
                pending: activeCommits.size,
                primary: first ? { kind: first.kind, percent: first.percent } : null,
            },
        }));
    } catch {
        // A broken progress listener must never affect the save itself.
    }
}

export async function commitChatMetadata({ target, chatMetadata }) {
    return commitChatRecords({
        target,
        operation: { kind: 'metadata' },
        records: [serializeJson(chatMetadata)],
        commitReason: 'mutation',
    });
}

export async function commitChatMetadataExtension({ target, namespace, value }) {
    return commitChatRecords({
        target,
        operation: { kind: 'metadataExtension', namespace },
        records: [serializeJson(value)],
        commitReason: 'mutation',
    });
}

function serializeJson(value) {
    const text = JSON.stringify(value);
    if (text === undefined) throw new Error('Chat commit value must serialize to JSON');
    return text;
}

export async function commitChatPayload({ target, payload, force, commitReason }) {
    const records = serializeChatPayload(payload);
    const coldSourceId = coldSourceForPayload(payload);
    return commitChatRecords({
        target,
        operation: { kind: 'payload', force, ...(coldSourceId === undefined ? {} : { coldSourceId }) },
        records,
        commitReason,
    });
}

// Callers capture all payload values as JSON text before this first await.
async function commitChatRecords({ target, operation, records, commitReason }) {
    const total = measureRecords(records);
    const commit = { kind: operation.kind, percent: 0 };
    activeCommits.add(commit);
    emitCommitProgress();

    const report = offset => {
        const percent = total > 0 ? Math.min(100, Math.round(offset / total * 100)) : 0;
        if (percent === commit.percent) return;
        commit.percent = percent;
        emitCommitProgress();
    };

    try {
        return await commitBytes({
            begin: () => invokeChatCommit('begin_chat_commit', { target, operation }),
            frames: maxChunkBytes => jsonlRecordsToByteChunks(records, { maxChunkBytes }),
            append: (body, options) => invokeChatCommit('append_chat_commit_chunk', body, options),
            onProgress: report,
            finish: (sessionId, expectedSize) => {
                report(expectedSize);
                return invokeChatCommit('finish_chat_commit', {
                    sessionId,
                    expectedSize,
                    commitReason,
                });
            },
            abort: sessionId => invokeChatCommit('abort_chat_commit', { sessionId }),
        });
    } finally {
        activeCommits.delete(commit);
        emitCommitProgress();
    }
}
