import { invoke } from '../../../tauri-bridge.js';
import { encodeBytesToBase64 } from '../../../tauri/main/binary-utils.js';
import { isAndroidRuntime } from './platform.js';
import { payloadToJsonlByteChunks } from './jsonl.js';

function positiveSafeInteger(value, label) {
    const number = Number(value);
    if (!Number.isSafeInteger(number) || number <= 0) {
        throw new Error(`${label} must be a positive safe integer`);
    }
    return number;
}

export async function commitChatPayload({ target, payload, force, commitReason }) {
    let sessionId = '';
    const normalizedCommitReason = commitReason ?? 'mutation';

    try {
        const begin = await invoke('begin_chat_commit', { target, force });
        sessionId = String(begin?.sessionId || '').trim();
        if (!sessionId) {
            throw new Error('Host chat commit did not return a session id');
        }

        const maxFrameBytes = positiveSafeInteger(begin?.maxFrameBytes, 'Host chat commit frame limit');
        const android = isAndroidRuntime();
        let offset = 0;

        for (const frame of payloadToJsonlByteChunks(payload, { maxChunkBytes: maxFrameBytes })) {
            const headers = {
                'session-id': sessionId,
                offset: String(offset),
            };
            const nextOffset = Number(await (android
                ? invoke('append_chat_commit_chunk', { data: encodeBytesToBase64(frame) }, {
                    headers: {
                        ...headers,
                        'chunk-encoding': 'base64',
                    },
                })
                : invoke('append_chat_commit_chunk', frame, { headers })));
            const expectedNextOffset = offset + frame.byteLength;
            if (nextOffset !== expectedNextOffset) {
                throw new Error(`Host chat commit returned unexpected offset ${nextOffset}`);
            }
            offset = nextOffset;
        }

        const finished = await invoke('finish_chat_commit', {
            sessionId,
            expectedSize: offset,
            commitReason: normalizedCommitReason,
        });
        sessionId = '';

        if (Number(finished?.size) !== offset) {
            throw new Error(`Host chat commit returned unexpected size ${finished?.size}`);
        }
    } catch (error) {
        if (sessionId) {
            try {
                await invoke('abort_chat_commit', { sessionId });
            } catch (abortError) {
                throw new AggregateError(
                    [error, abortError],
                    String(error?.message || error || 'Chat commit failed'),
                );
            }
        }
        throw error;
    }
}
