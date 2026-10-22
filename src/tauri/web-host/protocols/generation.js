// @ts-check

/**
 * @typedef {{ events: any[], status: string }} StreamBatch
 * @typedef {{ reader: ReadableStreamDefaultReader<MessageEvent<string>>, seq: number, cursor: number,
 *   status: string, batch: Promise<StreamBatch> | undefined }} Session
 */

/** @param {ReturnType<import('../backend.js').createBackend>} backend @returns {import('../install.js').CommandHandlers} */
export function createGenerationCommands({ json, request }) {
    /** @type {Map<string, Session>} */
    const sessions = new Map();
    /** @param {string} id */
    function get(id) {
        const value = sessions.get(id);
        if (!value) throw { NotFound: `Unknown generation stream: ${id}` };
        return value;
    }
    /** @param {Session} session @returns {Promise<StreamBatch>} */
    async function pull(session) {
        if (session.status !== 'running') return { events: [], status: session.status };
        try {
            const { value, done } = await session.reader.read();
            // close() may have run while the reader was blocked.
            if (/** @type {string} */ (session.status) === 'cancelled') return { events: [], status: 'cancelled' };
            const seq = ++session.seq;
            if (done || value.data === '[DONE]') {
                session.status = 'done';
                return { events: [{ type: 'done', seq }], status: 'done' };
            }
            return { events: [{ type: 'chunk', seq, data: value.data }], status: 'running' };
        } catch (error) {
            session.status = 'error';
            return { events: [{ type: 'error', seq: ++session.seq, message: String(error) }], status: 'error' };
        }
    }
    /** @param {string} id */
    async function close(id) {
        const session = sessions.get(id);
        if (!session) return;
        sessions.delete(id);
        session.status = 'cancelled';
        await session.reader.cancel();
    }
    return {
        // Session chat writes are the record; there is no native history archive or OS notifier.
        chat_history_generation_started: () => undefined,
        chat_history_generation_finished: () => undefined,
        get_notification_permission_state: () => 'denied',
        get_chat_completions_status: ({ dto }) => json('/api/backends/chat-completions/status', dto),
        generate_chat_completion: ({ dto }) => json('/api/backends/chat-completions/generate', dto),
        start_chat_completion_stream: async ({ streamId, dto }) => {
            // Imported on first use: the upstream parser pulls in power-user.js, an app module that must load after init.js.
            const { default: EventSourceStream } = await import('../../../scripts/sse-stream.js');
            if (sessions.has(streamId)) throw { Conflict: `Generation stream already exists: ${streamId}` };
            const { body } = request('/api/backends/chat-completions/generate', JSON.stringify(dto));
            if (body === null || typeof body === 'string') throw new Error('Streaming generation requires a ReadableStream body');
            const reader = (/** @type {ReadableStream<BufferSource>} */ (body)).pipeThrough(new EventSourceStream()).getReader();
            sessions.set(streamId, { reader, seq: 0, cursor: -1, status: 'running', batch: undefined });
        },
        read_chat_completion_stream: ({ streamId, afterSeq }) => {
            const session = get(streamId);
            // Retain the unacknowledged read, including an in-flight pull, for this cursor.
            if (afterSeq === session.cursor) return session.batch;
            if (afterSeq !== session.seq) throw { BadRequest: `Invalid generation cursor: ${afterSeq}` };
            session.cursor = afterSeq;
            return session.batch = pull(session);
        },
        close_chat_completion_stream: ({ streamId }) => close(streamId),
        // Non-streaming backend requests finish synchronously, so none remain to cancel.
        cancel_chat_completion_generation: () => undefined,
    };
}
