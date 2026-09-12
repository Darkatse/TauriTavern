import { invoke } from '../../../tauri-bridge.js';
import { encodeBytesToBase64 } from '../../../tauri/main/binary-utils.js';
import { isAndroidRuntime } from '../../util/mobile-runtime.js';
import { jsonlRecordsToByteChunks, serializeChatPayload } from './jsonl.js';

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

function positiveSafeInteger(value, label) {
    const number = Number(value);
    if (!Number.isSafeInteger(number) || number <= 0) {
        throw new Error(`${label} must be a positive safe integer`);
    }
    return number;
}

export async function commitChatPayload({ target, payload, force, commitReason }) {
    const records = serializeChatPayload(payload);
    const begin = await invokeChatCommit('begin_chat_commit', { target, force });
    const sessionId = String(begin?.sessionId || '').trim();
    if (!sessionId) {
        throw new Error('Host chat commit did not return a session id');
    }

    let offset = 0;
    try {
        const maxFrameBytes = positiveSafeInteger(begin?.maxFrameBytes, 'Host chat commit frame limit');
        const android = isAndroidRuntime();

        for (const frame of jsonlRecordsToByteChunks(records, { maxChunkBytes: maxFrameBytes })) {
            const headers = {
                'session-id': sessionId,
                offset: String(offset),
            };
            const nextOffset = Number(await (android
                ? invokeChatCommit('append_chat_commit_chunk', { data: encodeBytesToBase64(frame) }, {
                    headers: {
                        ...headers,
                        'chunk-encoding': 'base64',
                    },
                })
                : invokeChatCommit('append_chat_commit_chunk', frame, { headers })));
            if (nextOffset !== offset + frame.byteLength) {
                throw new Error(`Host chat commit returned unexpected offset ${nextOffset}`);
            }
            offset = nextOffset;
        }
    } catch (error) {
        try {
            await invokeChatCommit('abort_chat_commit', { sessionId });
        } catch (abortError) {
            throw new AggregateError([error, abortError], error.message);
        }
        throw error;
    }

    // finish consumes the session on the host whether it succeeds or fails; nothing is left to abort.
    const finished = await invokeChatCommit('finish_chat_commit', {
        sessionId,
        expectedSize: offset,
        commitReason,
    });
    if (Number(finished?.size) !== offset) {
        throw new Error(`Host chat commit returned unexpected size ${finished?.size}`);
    }
}
