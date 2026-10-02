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
    await commitBytes({
        begin: () => invokeChatCommit('begin_chat_commit', { target, operation }),
        frames: maxChunkBytes => jsonlRecordsToByteChunks(records, { maxChunkBytes }),
        append: (body, options) => invokeChatCommit('append_chat_commit_chunk', body, options),
        finish: (sessionId, expectedSize) => invokeChatCommit('finish_chat_commit', {
            sessionId,
            expectedSize,
            commitReason,
        }),
        abort: sessionId => invokeChatCommit('abort_chat_commit', { sessionId }),
    });
}
