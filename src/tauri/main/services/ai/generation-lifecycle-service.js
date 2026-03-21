// @ts-check

/**
 * @typedef {import('../notifications/system-notification-service.js').NotificationPermissionState} NotificationPermissionState
 */

/**
 * @param {unknown} value
 * @returns {string}
 */
function extractOpenAiChunkText(value) {
    const parsed = JSON.parse(String(value || ''));
    const delta = parsed?.choices?.[0]?.delta;
    return typeof delta?.content === 'string' ? delta.content : '';
}

/**
 * @param {{
 *   enabled: boolean;
 *   statusBridge: {
 *     supportsProgressUpdates: () => boolean;
 *     reportProgress: (outputTokens: number) => boolean;
 *   };
 *   shouldNotifyCompletion: () => boolean;
 *   estimateTokenCount: (text: string) => number;
 *   progressThrottleMs: number;
 *   progressMinCharsDelta: number;
 * }} deps
 */
function createStreamProgressReporter({
    enabled,
    statusBridge,
    shouldNotifyCompletion,
    estimateTokenCount,
    progressThrottleMs,
    progressMinCharsDelta,
}) {
    if (!enabled || !statusBridge.supportsProgressUpdates()) {
        return {
            reportStreamChunk() {
                // no-op
            },
        };
    }

    /** @type {string[]} */
    const outputChunks = [];
    let outputChars = 0;
    let lastTokenCount = 0;
    let lastTokenCharCount = 0;
    let lastTokenReportAt = 0;

    return {
        /** @param {string} data */
        reportStreamChunk(data) {
            if (!shouldNotifyCompletion()) {
                return;
            }

            try {
                const chunkText = extractOpenAiChunkText(data);
                if (!chunkText) {
                    return;
                }

                outputChunks.push(chunkText);
                outputChars += chunkText.length;

                const now = Date.now();
                const shouldCompute = now - lastTokenReportAt >= progressThrottleMs
                    && outputChars - lastTokenCharCount >= progressMinCharsDelta;

                if (!shouldCompute) {
                    return;
                }

                lastTokenReportAt = now;
                const charCountAtRequest = outputChars;
                const textSnapshot = outputChunks.join('');
                outputChunks.length = 0;
                outputChunks.push(textSnapshot);

                const normalized = estimateTokenCount(textSnapshot);
                lastTokenCharCount = charCountAtRequest;

                if (normalized === lastTokenCount) {
                    return;
                }

                lastTokenCount = normalized;
                statusBridge.reportProgress(normalized);
            } catch {
                // Ignore non-JSON chunks (for example keep-alives).
            }
        },
    };
}

/**
 * @param {{
 *   notificationService: {
 *     getPermissionState: () => Promise<NotificationPermissionState>;
 *     preparePermission: () => Promise<NotificationPermissionState>;
 *     show: (params: { title: string; body: string }) => Promise<void>;
 *   };
 *   statusBridge: {
 *     start: () => void;
 *     supportsProgressUpdates: () => boolean;
 *     reportProgress: (outputTokens: number) => boolean;
 *     finish: (params: { success: boolean; statusCode: number; showCompletionNotification: boolean }) => boolean;
 *     stop: () => void;
 *   };
 *   shouldNotifyCompletion: () => boolean;
 *   getNotificationTexts: () => {
 *     successTitle: string;
 *     successBody: string;
 *     failureTitle: string;
 *     failureBody: string;
 *   };
 *   normalizeFailureNotificationBody: (errorMessage: string) => string;
 *   extractFailureStatusCode: (errorMessage: string) => number;
 *   estimateTokenCount: (text: string) => number;
 *   progressThrottleMs: number;
 *   progressMinCharsDelta: number;
 * }} deps
 */
export function createGenerationLifecycleService({
    notificationService,
    statusBridge,
    shouldNotifyCompletion,
    getNotificationTexts,
    normalizeFailureNotificationBody,
    extractFailureStatusCode,
    estimateTokenCount,
    progressThrottleMs,
    progressMinCharsDelta,
}) {
    let activeCount = 0;

    /**
     * @param {Promise<NotificationPermissionState> | null} permissionPreparation
     * @returns {Promise<NotificationPermissionState>}
     */
    async function resolvePermissionState(permissionPreparation) {
        if (permissionPreparation) {
            return permissionPreparation;
        }

        return notificationService.getPermissionState();
    }

    /**
     * @param {{
     *   success: boolean;
     *   errorMessage: string;
     *   notifyFailure: boolean;
     *   permissionPreparation: Promise<NotificationPermissionState> | null;
     * }} params
     */
    async function showCompletionNotification({
        success,
        errorMessage,
        notifyFailure,
        permissionPreparation,
    }) {
        const permissionState = await resolvePermissionState(permissionPreparation);
        if (permissionState !== 'granted') {
            return;
        }

        const texts = getNotificationTexts();

        if (success) {
            await notificationService.show({
                title: texts.successTitle,
                body: texts.successBody,
            });
            return;
        }

        if (!notifyFailure) {
            return;
        }

        await notificationService.show({
            title: texts.failureTitle,
            body: normalizeFailureNotificationBody(errorMessage) || texts.failureBody,
        });
    }

    return {
        /**
         * @param {{ quiet: boolean }} params
         */
        createLifecycle({ quiet }) {
            let active = false;
            /** @type {Promise<NotificationPermissionState> | null} */
            let permissionPreparation = null;
            const progressReporter = createStreamProgressReporter({
                enabled: !quiet,
                statusBridge,
                shouldNotifyCompletion,
                estimateTokenCount,
                progressThrottleMs,
                progressMinCharsDelta,
            });

            return {
                begin() {
                    if (active) {
                        return;
                    }

                    active = true;
                    activeCount += 1;

                    if (!quiet) {
                        permissionPreparation = notificationService.preparePermission().catch((error) => {
                            console.error('Failed to prepare notification permission:', error);
                            return 'prompt';
                        });
                    }

                    if (activeCount === 1) {
                        statusBridge.start();
                    }
                },

                /**
                 * @param {{
                 *   success?: boolean;
                 *   errorMessage?: string;
                 *   notifyFailure?: boolean;
                 * }} [params]
                 */
                async finish({ success = false, errorMessage = '', notifyFailure = true } = {}) {
                    if (!active) {
                        return;
                    }

                    active = false;
                    activeCount = Math.max(0, activeCount - 1);

                    if (activeCount !== 0) {
                        return;
                    }

                    const shouldNotify = !quiet && shouldNotifyCompletion();
                    const handledByNative = statusBridge.finish({
                        success: Boolean(success),
                        statusCode: notifyFailure ? extractFailureStatusCode(errorMessage) : 0,
                        showCompletionNotification: Boolean(shouldNotify && (success || notifyFailure)),
                    });

                    if (!handledByNative && shouldNotify) {
                        try {
                            await showCompletionNotification({
                                success: Boolean(success),
                                errorMessage,
                                notifyFailure: Boolean(notifyFailure),
                                permissionPreparation,
                            });
                        } catch (error) {
                            console.error('Failed to show generation completion notification:', error);
                        }
                    }

                    if (!handledByNative) {
                        statusBridge.stop();
                    }
                },

                /** @param {string} data */
                reportStreamChunk(data) {
                    progressReporter.reportStreamChunk(data);
                },
            };
        },
    };
}
