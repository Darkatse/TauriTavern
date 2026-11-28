// @ts-check

/**
 * @param {{
 *   enabled: boolean;
 *   chunkText: (data: string) => string;
 *   statusBridge: {
 *     supportsProgressUpdates: () => boolean;
 *     reportProgress: (outputTokens: number) => boolean;
 *   };
 *   shouldReport: () => boolean;
 *   estimateTokenCount: (text: string) => number;
 *   progressThrottleMs: number;
 *   progressMinCharsDelta: number;
 * }} deps
 */
function createStreamProgressReporter({
    enabled,
    chunkText,
    statusBridge,
    shouldReport,
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
            if (!shouldReport()) {
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
 *     prepareForGeneration: () => Promise<boolean>;
 *     notifyIfAllowed: (params: { title: string; body: string }) => Promise<void>;
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
    function buildCompletionNotification({
        success,
        errorMessage,
        notifyFailure,
    }) {
        if (!success && !notifyFailure) {
            return null;
        }

        const texts = getNotificationTexts();
        if (success) {
            return {
                title: texts.successTitle,
                body: texts.successBody,
            };
        }

        return {
            title: texts.failureTitle,
            body: normalizeFailureNotificationBody(errorMessage) || texts.failureBody,
        };
    }

    return {
        /**
         * `chunkText` reads the output text of one stream event in the caller's response format.
         * @param {{ quiet: boolean; chunkText: (data: string) => string }} params
         */
        createLifecycle({ quiet, chunkText }) {
            let active = false;
            let canReportProgress = false;
            const progressReporter = createStreamProgressReporter({
                enabled: !quiet,
                chunkText,
                statusBridge,
                shouldReport: () => canReportProgress && shouldNotifyCompletion(),
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
                        void notificationService.prepareForGeneration()
                            .then((allowed) => {
                                canReportProgress = allowed;
                            })
                            .catch((error) => {
                                console.error('Failed to prepare notification permission:', error);
                                canReportProgress = false;
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
                        const notification = buildCompletionNotification({
                            success,
                            errorMessage,
                            notifyFailure,
                        });
                        if (notification) {
                            void notificationService.notifyIfAllowed(notification)
                                .catch((error) => {
                                    console.error('Failed to show generation completion notification:', error);
                                });
                        }
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
