import { encodeIpcByteBody } from './ipc-byte-body.js';
import { BYTE_HEADERS } from '../../kernel/byte-protocol.js';

/**
 * Thrown when a newer save takes this commit over. It travels the existing failure path
 * (which aborts the staging session) and is recognized by code above the commit boundary;
 * it never means the write failed.
 */
function commitSupersededError() {
    return Object.assign(new Error('Commit superseded by a newer save'), { code: 'commit_superseded' });
}

/**
 * Own a bounded byte commit from begin through finish/abort.
 * Callers capture their immutable snapshot before entering this asynchronous boundary.
 *
 * `shouldCancel` is polled at frame boundaries; when it reports true the commit is aborted
 * through the failure path, leaving the target untouched.
 */
export async function commitBytes({ begin, frames, append, finish, abort, shouldCancel }) {
    const session = await begin();
    const sessionId = String(session?.sessionId || '').trim();
    if (!sessionId) throw new Error('Host did not return a commit session id');

    let offset = 0;
    try {
        const maxFrameBytes = Number(session.maxFrameBytes);
        if (!Number.isSafeInteger(maxFrameBytes) || maxFrameBytes < 4) {
            throw new Error('Host returned an invalid commit frame limit');
        }
        for await (const frame of frames(maxFrameBytes)) {
            if (shouldCancel?.()) throw commitSupersededError();
            const { body, headers: encodingHeaders } = encodeIpcByteBody(frame);
            const headers = { ...encodingHeaders, [BYTE_HEADERS.sessionId]: sessionId, [BYTE_HEADERS.offset]: String(offset) };
            const accepted = Number(await append(body, { headers }));
            if (accepted !== offset + frame.byteLength) {
                throw new Error(`Host commit returned unexpected offset ${accepted}`);
            }
            offset = accepted;
        }
        // A cancellation that lands during the last frame still voids this commit.
        if (shouldCancel?.()) throw commitSupersededError();
    } catch (error) {
        try {
            await abort(sessionId);
        } catch (abortError) {
            throw new AggregateError([error, abortError], error.message);
        }
        throw error;
    }

    // Finish consumes the host session on success and failure alike.
    return await finish(sessionId, offset);
}
