// @ts-check

/**
 * Consume one Rust-owned chat completion session in sequence.
 *
 * @param {{
 *   safeInvoke: import('../../context/types.js').TauriInvokeFn;
 *   streamId: string;
 *   onEvent: (event: unknown) => void;
 *   isClosed: () => boolean;
 * }} deps
 * @returns {Promise<'done' | 'cancelled' | 'closed'>}
 */
export async function consumeChatCompletionStream({ safeInvoke, streamId, onEvent, isClosed }) {
    let afterSeq = 0;

    while (!isClosed()) {
        const result = await safeInvoke('read_chat_completion_stream', { streamId, afterSeq });
        const events = Array.isArray(result?.events) ? result.events : [];

        for (const event of events) {
            const seq = Number(event?.seq);
            if (!Number.isSafeInteger(seq) || seq !== afterSeq + 1) {
                throw new Error(`Invalid chat completion stream sequence: expected ${afterSeq + 1}, got ${seq}`);
            }

            afterSeq = seq;
            onEvent(event);
            if (isClosed()) {
                return 'closed';
            }
        }

        const status = String(result?.status || '');
        if (events.length === 0 && status !== 'running') {
            if (status === 'done' || status === 'cancelled') {
                return status;
            }

            throw new Error(`Chat completion stream ended without a terminal event (${status || 'unknown'})`);
        }
    }

    return 'closed';
}
