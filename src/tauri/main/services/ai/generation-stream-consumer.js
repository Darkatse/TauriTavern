// @ts-check

/**
 * @typedef {{ type: 'chunk'; seq: number; data: string }
 *   | { type: 'done'; seq: number }
 *   | { type: 'error'; seq: number; message: string; details?: unknown }} GenerationStreamEvent
 *
 * @typedef {'running' | 'done' | 'error' | 'cancelled'} GenerationStreamStatus
 *
 * @typedef {{
 *   events: GenerationStreamEvent[];
 *   status: GenerationStreamStatus;
 * }} GenerationStreamReadResult
 */

/**
 * Consume one Rust-owned generation stream session in sequence.
 *
 * @param {{
 *   safeInvoke: import('../../context/types.js').TauriInvokeFn;
 *   streamId: string;
 *   onEvent: (event: GenerationStreamEvent) => void;
 *   isClosed: () => boolean;
 * }} deps
 * @returns {Promise<'done' | 'cancelled' | 'closed'>}
 */
export async function consumeGenerationStream({ safeInvoke, streamId, onEvent, isClosed }) {
    let afterSeq = 0;

    while (!isClosed()) {
        /** @type {GenerationStreamReadResult} */
        let result;
        try {
            result = await safeInvoke('read_generation_stream', { streamId, afterSeq });
        } catch {
            if (isClosed()) {
                return 'closed';
            }
            result = await safeInvoke('read_generation_stream', { streamId, afterSeq });
        }

        if (!result || !Array.isArray(result.events)) {
            throw new Error('Invalid generation stream read result');
        }

        for (const event of result.events) {
            const seq = Number(event?.seq);
            if (!Number.isSafeInteger(seq) || seq !== afterSeq + 1) {
                throw new Error(`Invalid generation stream sequence: expected ${afterSeq + 1}, got ${seq}`);
            }

            afterSeq = seq;
            onEvent(event);
            if (isClosed()) {
                return 'closed';
            }
        }

        const status = result.status;
        if (result.events.length === 0 && status !== 'running') {
            if (status === 'done' || status === 'cancelled') {
                return status;
            }

            throw new Error(`Generation stream ended without a terminal event (${status || 'unknown'})`);
        }
    }

    return 'closed';
}
