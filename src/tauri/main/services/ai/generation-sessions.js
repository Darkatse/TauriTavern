// @ts-check

import { createAndroidGenerationBridge } from '../../adapters/android/android-generation-bridge.js';
import { confirmAiNotificationPermissionRationale } from '../../adapters/st/ai-notification-permission-rationale-popup.js';
import { estimateTokenCount } from '../../brokers/token-count-broker.js';
import { createAbortError, isAbortError } from '../../kernel/abort-error.js';
import { createRandomId } from '../../kernel/random-id.js';
import { jsonResponse } from '../../http-utils.js';
import { isCommandCancelled } from '../../../../scripts/util/command-error-utils.js';
import { createSystemNotificationService } from '../notifications/system-notification-service.js';
import {
    buildLegacyErrorPayload,
    formatAssistantErrorText,
    getUserFacingErrorMessage,
} from './ai-error-presenter.js';
import { createGenerationLifecycleService } from './generation-lifecycle-service.js';
import {
    getGenerationNotificationTexts,
    normalizeFailureNotificationBody,
    shouldNotifyCompletion,
} from './generation-notifications.js';
import { createGenerationStatusBridge } from './generation-status-bridge.js';
import { consumeGenerationStream } from './generation-stream-consumer.js';

/**
 * @typedef {import('../../context/types.js').TauriInvokeCommand} TauriInvokeCommand
 * @typedef {import('./generation-stream-consumer.js').GenerationStreamEvent} GenerationStreamEvent
 *
 * @typedef {{
 *   finish: (params?: {
 *     success?: boolean | undefined;
 *     errorMessage?: string | undefined;
 *     notifyFailure?: boolean | undefined;
 *   }) => void;
 *   reportStreamChunk: (data: string) => void;
 * }} GenerationLifecycle
 */

const STREAM_FRAME_INTERVAL_MS = 10;
const STREAM_RESPONSE_HEADERS = Object.freeze({
    'Content-Type': 'text/event-stream; charset=utf-8',
    'Cache-Control': 'no-cache',
    Connection: 'keep-alive',
});
const ANDROID_GENERATION_BRIDGE_NAME = 'TauriTavernAndroidAiBridge';
const ANDROID_LIVE_UPDATE_TOKEN_THROTTLE_MS = 4000;
const ANDROID_LIVE_UPDATE_TOKEN_MIN_CHARS_DELTA = 160;

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
 * Shallow-copies a plain object payload and normalizes non-objects to `{}`.
 * @param {unknown} value
 * @returns {Record<string, unknown>}
 */
export function copyGenerationPayload(value) {
    return value && typeof value === 'object' && !Array.isArray(value)
        ? { .../** @type {Record<string, unknown>} */ (value) }
        : {};
}

/**
 * A quiet generation, such as a summary, runs for the page itself and fails with an error response
 * instead of an assistant reply.
 * @param {Record<string, unknown>} payload
 * @returns {boolean}
 */
function isQuietRequest(payload) {
    return String(payload.type || '').trim().toLowerCase() === 'quiet';
}

/**
 * @param {unknown} error
 * @returns {boolean}
 */
export function isRequestCancelled(error) {
    return isAbortError(error) || isCommandCancelled(error);
}

/**
 * Serves one already-built error chunk as a finished SSE stream.
 * @param {unknown} chunk
 * @returns {Response}
 */
function createErrorStreamResponse(chunk) {
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
 * Generations run in Rust for every API family, and every family's generate route is served by
 * `generate`. Create one per route registry, so one lifecycle counts every running generation.
 *
 * @param {{ safeInvoke: (command: TauriInvokeCommand, args?: any) => Promise<any> }} context
 */
export function createGenerationSessions(context) {
    const androidGenerationBridge = createAndroidGenerationBridge({ bridgeName: ANDROID_GENERATION_BRIDGE_NAME });
    const generationStatusBridge = createGenerationStatusBridge({ bridge: androidGenerationBridge });
    const lifecycles = createGenerationLifecycleService({
        notificationService: createSystemNotificationService({
            safeInvoke: context.safeInvoke,
            confirmPermissionRationale: confirmAiNotificationPermissionRationale,
        }),
        statusBridge: generationStatusBridge,
        shouldNotifyCompletion,
        getNotificationTexts: getGenerationNotificationTexts,
        normalizeFailureNotificationBody,
        estimateTokenCount,
        progressThrottleMs: ANDROID_LIVE_UPDATE_TOKEN_THROTTLE_MS,
        progressMinCharsDelta: ANDROID_LIVE_UPDATE_TOKEN_MIN_CHARS_DELTA,
    });

    /**
     * Runs a non-stream generation command; aborting `signal` cancels it in Rust.
     * @param {{
     *   command: TauriInvokeCommand;
     *   args: Record<string, unknown>;
     *   signal?: AbortSignal | null | undefined;
     * }} params
     */
    async function request({ command, args, signal }) {
        if (signal?.aborted) {
            throw createAbortError();
        }

        const requestId = createRandomId();
        let abortRequested = false;
        /** @type {(() => void) | null} */
        let abortHandler = null;
        if (signal) {
            abortHandler = () => {
                abortRequested = true;
                void context.safeInvoke('cancel_generation', { requestId })
                    .catch((error) => {
                        console.debug('Failed to cancel generation:', error);
                    });
            };
            signal.addEventListener('abort', abortHandler, { once: true });
        }

        try {
            const result = await context.safeInvoke(command, { ...args, requestId });

            if (abortRequested || signal?.aborted) {
                throw createAbortError();
            }

            return result;
        } catch (error) {
            if (abortRequested || signal?.aborted) {
                throw createAbortError();
            }

            throw error;
        } finally {
            if (signal && abortHandler) {
                signal.removeEventListener('abort', abortHandler);
            }
        }
    }

    /**
     * Starts a Rust stream session and serves it as SSE. `start(streamId)` must resolve before a
     * streaming `Response` is returned so pre-stream errors and cancellations flow through
     * `generate`'s error path. `errorChunk(assistantText)` wraps a formatted failure for that family.
     *
     * @param {{
     *   start: (streamId: string) => Promise<unknown>;
     *   errorChunk: (assistantText: string) => unknown;
     *   signal?: AbortSignal | null | undefined;
     *   lifecycle: GenerationLifecycle;
     * }} params
     * @returns {Promise<Response>}
     */
    async function streamResponse({ start, errorChunk, signal, lifecycle }) {
        if (signal?.aborted) {
            throw createAbortError();
        }

        const streamId = createRandomId();
        const closeSession = async () => {
            try {
                await context.safeInvoke('close_generation_stream', { streamId });
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
         *   failureMessage?: string;
         * }} [options]
         */
        const closeStream = async ({
            cancelUpstream = false,
            appendDone = false,
            errorPayload = null,
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
                    controllerRef.close();
                } catch {
                    // stream already closed
                }
            }

            if (signal && abortHandler) {
                signal.removeEventListener('abort', abortHandler);
                abortHandler = null;
            }

            await closeSession();

            const isSuccessfulCompletion = sawDone && !cancelUpstream && !errorPayload;
            const shouldNotifyFailure = !isSuccessfulCompletion && !cancelUpstream && Boolean(failureMessage || errorPayload);
            lifecycle.finish({
                success: isSuccessfulCompletion,
                errorMessage: failureMessage,
                notifyFailure: shouldNotifyFailure,
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
                    const failureMessage = getUserFacingErrorMessage(event);
                    void closeStream({
                        appendDone: true,
                        errorPayload: errorChunk(formatAssistantErrorText(failureMessage)),
                        failureMessage,
                    });
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
                    safeInvoke: context.safeInvoke,
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

                const failureMessage = getUserFacingErrorMessage(error);
                await closeStream({
                    appendDone: true,
                    errorPayload: errorChunk(formatAssistantErrorText(failureMessage)),
                    failureMessage,
                });
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

    /**
     * Serves a generate route. The family supplies how to start its stream and run its request,
     * how to read output text from its stream events, and its error shapes. A failure becomes an
     * assistant reply, except for a quiet request: that answers `502 { error: { message } }`, or
     * `quietErrorResponse` when a non-stream one fails.
     * @param {{
     *   payload: Record<string, unknown>;
     *   signal?: AbortSignal | null | undefined;
     *   chunkText: (data: string) => string;
     *   startStream: (streamId: string) => Promise<unknown>;
     *   runRequest: () => Promise<unknown>;
     *   errorChunk: (assistantText: string) => unknown;
     *   errorCompletion: (assistantText: string) => unknown;
     *   quietErrorResponse?: ((error: unknown, errorMessage: string) => Response) | undefined;
     *   label: string;
     * }} params
     */
    async function generate({
        payload,
        signal,
        chunkText,
        startStream,
        runRequest,
        errorChunk,
        errorCompletion,
        quietErrorResponse,
        label,
    }) {
        const wantsStream = Boolean(payload.stream);
        const quiet = isQuietRequest(payload);
        const lifecycle = lifecycles.createLifecycle({ quiet, chunkText });
        lifecycle.begin();

        try {
            if (wantsStream) {
                return await streamResponse({ start: startStream, errorChunk, signal, lifecycle });
            }

            const completion = await runRequest();
            lifecycle.finish({ success: true });
            return jsonResponse(completion ?? {});
        } catch (error) {
            const aborted = isRequestCancelled(error);
            const errorMessage = aborted ? '' : getUserFacingErrorMessage(error);

            lifecycle.finish({ success: false, errorMessage, notifyFailure: !aborted });

            if (aborted) {
                throw createAbortError();
            }

            console.error(`${label} generation failed:`, error);
            if (quiet) {
                return !wantsStream && quietErrorResponse
                    ? quietErrorResponse(error, errorMessage)
                    : jsonResponse(buildLegacyErrorPayload(error), 502);
            }

            const assistantText = formatAssistantErrorText(errorMessage);
            return wantsStream
                ? createErrorStreamResponse(errorChunk(assistantText))
                : jsonResponse(errorCompletion(assistantText));
        }
    }

    return { generate, request };
}
