// @ts-check

/**
 * @typedef {{
 *   call: (methodName: string, ...args: any[]) => boolean;
 *   get: (methodName: string, ...args: any[]) => any;
 *   has: (methodName: string) => boolean;
 * }} NativeGenerationBridge
 */

/**
 * @param {{
 *   bridge: NativeGenerationBridge;
 * }} deps
 */
export function createGenerationStatusBridge({ bridge }) {
    /** @type {boolean | null} */
    let liveUpdatesSupported = null;

    function supportsLiveUpdates() {
        if (liveUpdatesSupported === null) {
            liveUpdatesSupported = bridge.get('supportsLiveUpdates') === true;
        }

        return liveUpdatesSupported;
    }

    function supportsProgressUpdates() {
        return supportsLiveUpdates() && bridge.has('onGenerationProgress');
    }

    function supportsNativeCompletion() {
        return supportsLiveUpdates() && bridge.has('onGenerationFinish');
    }

    /**
     * @param {{
     *   success: boolean;
     *   statusCode: number;
     *   showCompletionNotification: boolean;
     * }} params
     */
    function finish({ success, statusCode, showCompletionNotification }) {
        if (!supportsNativeCompletion()) {
            return false;
        }

        return bridge.call(
            'onGenerationFinish',
            JSON.stringify({
                success: Boolean(success),
                status_code: Number.isInteger(statusCode) ? statusCode : 0,
                show_completion_notification: Boolean(showCompletionNotification),
            }),
        );
    }

    return {
        start() {
            bridge.call('onGenerationStart');
        },
        supportsProgressUpdates,
        /** @param {number} outputTokens */
        reportProgress(outputTokens) {
            if (!supportsProgressUpdates()) {
                return false;
            }

            return bridge.call('onGenerationProgress', outputTokens);
        },
        finish,
        stop() {
            bridge.call('onGenerationStop');
        },
    };
}
