import { encodeBytesToBase64 } from '../../binary-utils.js';
import { isAndroidRuntime } from '../../../../scripts/util/mobile-runtime.js';

/**
 * Own a bounded byte commit from begin through finish/abort.
 * Callers capture their text snapshot before entering this asynchronous boundary.
 */
export async function commitBytes({ begin, frames, append, finish, abort }) {
    const session = await begin();
    const sessionId = String(session?.sessionId || '').trim();
    if (!sessionId) throw new Error('Host did not return a commit session id');

    let offset = 0;
    try {
        const maxFrameBytes = Number(session.maxFrameBytes);
        if (!Number.isSafeInteger(maxFrameBytes) || maxFrameBytes < 4) {
            throw new Error('Host returned an invalid commit frame limit');
        }
        const android = isAndroidRuntime();
        for (const frame of frames(maxFrameBytes)) {
            const headers = { 'session-id': sessionId, offset: String(offset) };
            if (android) headers['chunk-encoding'] = 'base64';
            const body = android ? { data: encodeBytesToBase64(frame) } : frame;
            const accepted = Number(await append(body, { headers }));
            if (accepted !== offset + frame.byteLength) {
                throw new Error(`Host commit returned unexpected offset ${accepted}`);
            }
            offset = accepted;
        }
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
