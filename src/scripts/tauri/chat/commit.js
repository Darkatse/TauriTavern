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

// [CHAT-COMMIT-PROGRESS-20261018] The single place save progress is measured.
// Character/group x payload/metadata/metadataExtension all funnel through commitChatRecords and
// the host's chunked commit protocol, so counting here covers every chat write. The queue-level
// `tt-record-save` stays as-is: that one answers "how many writes are still queued" (the kill-safe
// signal), this one answers "where is the write that is running right now".
// The payload carries only facts (kind/target/bytes); rendering copy belongs to the consumer.
const COMMIT_PROGRESS_EVENT = 'tt-chat-commit';
const RECORD_SEPARATOR = '\n';

const MEASURE_SCRATCH_BYTES = 64 * 1024;
const MEASURE_CHUNK_CHARS = Math.floor(MEASURE_SCRATCH_BYTES / 3);
const textEncoder = new TextEncoder();
const measureScratch = new Uint8Array(MEASURE_SCRATCH_BYTES);

let nextCommitId = 1;

/**
 * @type {Map<number, {
 *   id: number, kind: string, targetKind: string,
 *   status: 'writing' | 'finishing', written: number, total: number, startedAt: number,
 * }>}
 */
const activeCommits = new Map();

/**
 * UTF-8 length of one record. Encodes into a fixed scratch buffer so a large payload never needs
 * a second copy of its own bytes; the only cost is one extra encode pass over the records.
 */
function utf8ByteLength(text) {
    let bytes = 0;
    let offset = 0;
    while (offset < text.length) {
        const end = Math.min(text.length, offset + MEASURE_CHUNK_CHARS);
        const { read, written } = textEncoder.encodeInto(text.slice(offset, end), measureScratch);
        if (read === 0) break; // Defensive: never spin on an encoder that refuses to advance.
        bytes += written;
        offset += read;
    }
    return bytes;
}

/** Mirrors textFragmentsToByteChunks: one separator between records, none after the last. */
function measureRecords(records) {
    let bytes = 0;
    for (const record of records) bytes += utf8ByteLength(record);
    if (records.length > 1) bytes += (records.length - 1) * RECORD_SEPARATOR.length;
    return bytes;
}

function snapshotCommit(commit) {
    return {
        id: commit.id,
        kind: commit.kind,
        targetKind: commit.targetKind,
        status: commit.status,
        written: commit.written,
        total: commit.total,
        percent: commit.total > 0 ? Math.min(100, Math.round(commit.written / commit.total * 100)) : 0,
        startedAt: commit.startedAt,
    };
}

function emitCommitProgress() {
    // Contract tests run against a stub window that only carries __TAURI__.
    if (typeof window === 'undefined' || typeof window.dispatchEvent !== 'function') return;
    if (typeof CustomEvent !== 'function') return;
    const commits = [...activeCommits.values()].map(snapshotCommit);
    try {
        window.dispatchEvent(new CustomEvent(COMMIT_PROGRESS_EVENT, {
            detail: {
                pending: commits.length,
                // Mobile renders one line: keep the earliest commit so the label never flips
                // between concurrent commits (queue bypasses can overlap).
                primary: commits[0] ?? null,
                commits,
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
    const commit = {
        id: nextCommitId++,
        kind: operation.kind,
        targetKind: target.kind,
        status: 'writing',
        written: 0,
        total: measureRecords(records),
        startedAt: Date.now(),
    };
    activeCommits.set(commit.id, commit);
    emitCommitProgress();

    try {
        return await commitBytes({
            begin: () => invokeChatCommit('begin_chat_commit', { target, operation }),
            frames: maxChunkBytes => jsonlRecordsToByteChunks(records, { maxChunkBytes }),
            append: (body, options) => invokeChatCommit('append_chat_commit_chunk', body, options),
            onProgress: offset => {
                commit.written = offset;
                emitCommitProgress();
            },
            finish: (sessionId, expectedSize) => {
                // Bytes are all in the stage file here; the host still has to publish (rename,
                // fsync, cache invalidation). Report that tail instead of leaving the bar short.
                commit.written = expectedSize;
                commit.status = 'finishing';
                emitCommitProgress();
                return invokeChatCommit('finish_chat_commit', {
                    sessionId,
                    expectedSize,
                    commitReason,
                });
            },
            abort: sessionId => invokeChatCommit('abort_chat_commit', { sessionId }),
        });
    } finally {
        activeCommits.delete(commit.id);
        emitCommitProgress();
    }
}
