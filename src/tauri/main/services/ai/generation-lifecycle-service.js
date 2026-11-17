// @ts-check

/**
 * @typedef {import('../notifications/system-notification-service.js').NotificationPermissionState} NotificationPermissionState
 */

/**
 * @param {{
 *   enabled: boolean;
 *   chunkText: (data: string) => string;
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
    chunkText,
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
                const text = chunkText(data);
                if (!text) {
                    return;
                }

                outputChunks.push(text);
                outputChars += text.length;

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
 *     supportsProgressUpdates: () => boolean;
 *     reportProgress: (outputTokens: number) => boolean;
 *     handlesCompletion: () => boolean;
 *   };
 *   shouldNotifyCompletion: () => boolean;
 *   getNotificationTexts: () => {
 *     successTitle: string;
 *     successBody: string;
 *     failureTitle: string;
 *     failureBody: string;
 *   };
 *   normalizeFailureNotificationBody: (errorMessage: string) => string;
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
    estimateTokenCount,
    progressThrottleMs,
    progressMinCharsDelta,
}) {
    let activeCount = 0;

    /**
     * @param {{
     *   success: boolean;
     *   errorMessage: string;
     *   notifyFailure: boolean;
     * }} params
     */
    async function showCompletionNotification({
        success,
        errorMessage,
        notifyFailure,
    }) {
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
         * `chunkText` reads the output text of one stream event in the caller's response format.
         * @param {{ quiet: boolean; chunkText: (data: string) => string }} params
         */
        createLifecycle({ quiet, chunkText }) {
            let active = false;
            /** @type {NotificationPermissionState | null} */
            let preparedPermissionState = null;
            const progressReporter = createStreamProgressReporter({
                enabled: !quiet,
                chunkText,
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
                        void notificationService.preparePermission()
                            .then((state) => {
                                preparedPermissionState = state;
                            })
                            .catch((error) => {
                                console.error('Failed to prepare notification permission:', error);
                                preparedPermissionState = 'prompt';
                            });
                    }
                },

                /**
                 * @param {{
                 *   success?: boolean | undefined;
                 *   errorMessage?: string | undefined;
                 *   notifyFailure?: boolean | undefined;
                 * }} [params]
                 */
                finish({ success = false, errorMessage = '', notifyFailure = true } = {}) {
                    if (!active) {
                        return;
                    }

                    active = false;
                    activeCount -= 1;

                    if (activeCount !== 0) {
                        return;
                    }

                    const shouldNotify = !quiet && shouldNotifyCompletion();
                    const handledByNative = statusBridge.handlesCompletion();

                    if (!handledByNative && shouldNotify) {
                        void (async () => {
                            try {
                                const permissionState = preparedPermissionState
                                    ?? await notificationService.getPermissionState();
                                preparedPermissionState = permissionState;

                                if (permissionState === 'granted') {
                                    await showCompletionNotification({
                                        success,
                                        errorMessage,
                                        notifyFailure,
                                    });
                                }
                            } catch (error) {
                                console.error('Failed to show generation completion notification:', error);
                            }
                        })();
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
