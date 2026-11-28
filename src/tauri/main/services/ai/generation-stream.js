// @ts-check

import { createAbortError } from '../../kernel/abort-error.js';
import { createRandomId } from '../../kernel/random-id.js';
import { formatAssistantErrorText, getUserFacingErrorMessage } from './ai-error-presenter.js';

/**
 * @typedef {import('../../context/types.js').TauriInvokeFn} TauriInvokeFn
 *
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
 *
 * @typedef {{
 *   finish: (params?: {
 *     success?: boolean | undefined;
 *     errorMessage?: string | undefined;
 *     notifyFailure?: boolean | undefined;
 *   }) => void;
 *   reportStreamChunk: (data: string) => void;
 * }} GenerationStreamLifecycle
 */

const STREAM_FRAME_INTERVAL_MS = 10;
const STREAM_RESPONSE_HEADERS = Object.freeze({
    'Content-Type': 'text/event-stream; charset=utf-8',
    'Cache-Control': 'no-cache',
    Connection: 'keep-alive',
});

/**
 * @param {string} text
 * @returns {string}
 */
function encodeSseDataFrame(text) {
    if (!text.includes('\n') && !text.includes('\r')) {
        return `data: ${text}\n\n`;
    }

    const lines = text.split(/\r\n|\r|\n/g);
    const framed = lines.map((line) => `data: ${line}\n`).join('');
    return `${framed}\n`;
}

/**
 * Serves one already-built error chunk as a finished SSE stream.
 * @param {unknown} chunk
 * @returns {Response}
 */
export function createErrorStreamResponse(chunk) {
    const encoder = new TextEncoder();
    const readable = new ReadableStream({
        start(controller) {
            controller.enqueue(encoder.encode(encodeSseDataFrame(JSON.stringify(chunk))));
            controller.enqueue(encoder.encode(encodeSseDataFrame('[DONE]')));
            controller.close();
        },
    });

    return new Response(readable, {
        status: 200,
        headers: STREAM_RESPONSE_HEADERS,
    });
}

/**
 * Consume one Rust-owned generation stream session in sequence.
 *
 * @param {{
 *   safeInvoke: TauriInvokeFn;
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

/**
 * Starts a Rust stream session and serves it as SSE. `start(streamId)` must resolve before a
 * streaming `Response` is returned so pre-stream errors and cancellations flow through `generate`'s
 * error path. `errorChunk(assistantText)` wraps a formatted failure for that family, while a
 * `quiet` stream fails the readable body directly.
 *
 * @param {{
 *   safeInvoke: TauriInvokeFn;
 *   start: (streamId: string) => Promise<unknown>;
 *   errorChunk: (assistantText: string) => unknown;
 *   quiet: boolean;
 *   signal?: AbortSignal | null | undefined;
 *   lifecycle: GenerationStreamLifecycle;
 * }} params
 * @returns {Promise<Response>}
 */
export async function serveGenerationStream({
    safeInvoke,
    start,
    errorChunk,
    quiet,
    signal,
    lifecycle,
}) {
    if (signal?.aborted) {
        throw createAbortError();
    }

    const streamId = createRandomId();
    const closeSession = async () => {
        try {
            await safeInvoke('close_generation_stream', { streamId });
        } catch (error) {
            console.debug('Failed to close generation stream:', error);
        }
    };

    try {
        await start(streamId);
    } catch (error) {
        if (signal?.aborted) {
            throw createAbortError();
        }
        throw error;
    }

    if (signal?.aborted) {
        await closeSession();
        throw createAbortError();
    }

    const encoder = new TextEncoder();
    let isClosed = false;
    let sawDone = false;
    /** @type {ReturnType<typeof setTimeout> | null} */
    let flushTimer = null;
    /** @type {(() => void) | null} */
    let abortHandler = null;
    /** @type {ReadableStreamDefaultController<Uint8Array> | null} */
    let controllerRef = null;
    /** @type {string[]} */
    const pendingFrames = [];

    const flushFrames = () => {
        if (!controllerRef || pendingFrames.length === 0) {
            return;
        }

        const framed = pendingFrames.map((data) => encodeSseDataFrame(data)).join('');
        pendingFrames.length = 0;
        controllerRef.enqueue(encoder.encode(framed));
    };

    const scheduleFlush = () => {
        if (flushTimer !== null || isClosed) {
            return;
        }

        flushTimer = setTimeout(() => {
            flushTimer = null;
            flushFrames();
        }, STREAM_FRAME_INTERVAL_MS);
    };

    /**
     * @param {{
     *   cancelUpstream?: boolean;
     *   appendDone?: boolean;
     *   errorPayload?: unknown;
     *   streamError?: Error | null;
     *   failureMessage?: string;
     * }} [options]
     */
    const closeStream = async ({
        cancelUpstream = false,
        appendDone = false,
        errorPayload = null,
        streamError = null,
        failureMessage = '',
    } = {}) => {
        if (isClosed) {
            return;
        }

        isClosed = true;

        if (flushTimer !== null) {
            clearTimeout(flushTimer);
            flushTimer = null;
        }

        if (errorPayload) {
            pendingFrames.push(JSON.stringify(errorPayload));
        }

        if (appendDone && !sawDone) {
            sawDone = true;
            pendingFrames.push('[DONE]');
        }

        flushFrames();

        if (controllerRef) {
            try {
                if (streamError) {
                    controllerRef.error(streamError);
                } else {
                    controllerRef.close();
                }
            } catch {
                // stream already closed
            }
        }

        if (signal && abortHandler) {
            signal.removeEventListener('abort', abortHandler);
            abortHandler = null;
        }

        await closeSession();

        const isSuccessfulCompletion = sawDone && !cancelUpstream && !errorPayload && !streamError;
        const shouldNotifyFailure = !isSuccessfulCompletion
            && !cancelUpstream
            && Boolean(failureMessage || errorPayload || streamError);
        lifecycle.finish({
            success: isSuccessfulCompletion,
            errorMessage: failureMessage,
            notifyFailure: shouldNotifyFailure,
        });
    };

    /** @param {unknown} error */
    const failStream = (error) => {
        const failureMessage = getUserFacingErrorMessage(error);
        if (quiet) {
            return closeStream({
                streamError: new Error(failureMessage),
                failureMessage,
            });
        }

        return closeStream({
            appendDone: true,
            errorPayload: errorChunk(formatAssistantErrorText(failureMessage)),
            failureMessage,
        });
    };

    /** @param {GenerationStreamEvent} event */
    const onStreamEvent = (event) => {
        if (isClosed) {
            return;
        }

        switch (event.type) {
            case 'chunk': {
                if (!event.data) {
                    return;
                }

                pendingFrames.push(event.data);

                if (event.data === '[DONE]') {
                    sawDone = true;
                    flushFrames();
                    return;
                }

                lifecycle.reportStreamChunk(event.data);
                scheduleFlush();
                return;
            }
            case 'error': {
                void failStream(event);
                return;
            }
            case 'done': {
                void closeStream({ appendDone: true });
                return;
            }
            default: {
                /** @type {never} */
                const unexpected = event;
                throw new Error(`Unexpected generation stream event: ${JSON.stringify(unexpected)}`);
            }
        }
    };

    const pumpStream = async () => {
        try {
            const status = await consumeGenerationStream({
                safeInvoke,
                streamId,
                onEvent: onStreamEvent,
                isClosed: () => isClosed,
            });
            if (status === 'done') {
                await closeStream({ appendDone: true });
            } else if (status === 'cancelled') {
                await closeStream({ cancelUpstream: true });
            }
        } catch (error) {
            if (isClosed) {
                return;
            }

            await failStream(error);
        }
    };

    const readable = new ReadableStream({
        start(controller) {
            controllerRef = controller;

            if (signal) {
                abortHandler = () => {
                    void closeStream({ cancelUpstream: true });
                };

                if (signal.aborted) {
                    abortHandler();
                    return;
                }

                signal.addEventListener('abort', abortHandler, { once: true });
            }

            void pumpStream();
        },
        async cancel() {
            await closeStream({ cancelUpstream: true });
        },
    });

    return new Response(readable, {
        status: 200,
        headers: STREAM_RESPONSE_HEADERS,
    });
}
