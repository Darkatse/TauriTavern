// @ts-check

import { createAndroidGenerationBridge } from '../../adapters/android/android-generation-bridge.js';
import {
    confirmAiNotificationPermissionRationale,
    showAiNotificationsOffNotice,
} from '../../adapters/st/ai-notification-permission-rationale-popup.js';
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
import { createErrorStreamResponse, serveGenerationStream } from './generation-stream.js';

/**
 * @typedef {import('../../context/types.js').TauriInvokeCommand} TauriInvokeCommand
 */

const ANDROID_GENERATION_BRIDGE_NAME = 'TauriTavernAndroidAiBridge';
const ANDROID_LIVE_UPDATE_TOKEN_THROTTLE_MS = 4000;
const ANDROID_LIVE_UPDATE_TOKEN_MIN_CHARS_DELTA = 160;

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
            showNotificationsOffNotice: showAiNotificationsOffNotice,
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
     * Serves a generate route; a family without `nonStream` always streams. A failure becomes an
     * assistant reply, except for a quiet request: that answers `502 { error: { message } }` (or
     * `nonStream.quietErrorResponse`) before a stream starts, and fails the stream once streaming.
     * @param {{
     *   payload: Record<string, unknown>;
     *   signal?: AbortSignal | null | undefined;
     *   chunkText: (data: string) => string;
     *   startStream: (streamId: string) => Promise<unknown>;
     *   errorChunk: (assistantText: string) => unknown;
     *   nonStream?: {
     *     run: () => Promise<unknown>;
     *     errorCompletion: (assistantText: string) => unknown;
     *     quietErrorResponse?: ((error: unknown, errorMessage: string) => Response) | undefined;
     *   } | undefined;
     *   label: string;
     * }} params
     */
    async function generate({
        payload,
        signal,
        chunkText,
        startStream,
        errorChunk,
        nonStream,
        label,
    }) {
        const nonStreamForm = payload.stream ? undefined : nonStream;
        const quiet = isQuietRequest(payload);
        const lifecycle = lifecycles.createLifecycle({ quiet, chunkText });
        lifecycle.begin();

        try {
            if (!nonStreamForm) {
                return await serveGenerationStream({
                    safeInvoke: context.safeInvoke,
                    start: startStream,
                    errorChunk,
                    quiet,
                    signal,
                    lifecycle,
                });
            }

            const completion = await nonStreamForm.run();
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
                return nonStreamForm?.quietErrorResponse
                    ? nonStreamForm.quietErrorResponse(error, errorMessage)
                    : jsonResponse(buildLegacyErrorPayload(error), 502);
            }

            const assistantText = formatAssistantErrorText(errorMessage);
            return nonStreamForm
                ? jsonResponse(nonStreamForm.errorCompletion(assistantText))
                : createErrorStreamResponse(errorChunk(assistantText));
        }
    }

    return { generate, request };
}
