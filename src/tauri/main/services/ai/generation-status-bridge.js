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
    /** @type {boolean | null} */
    let nativeCompletionSupported = null;

    function supportsLiveUpdates() {
        if (liveUpdatesSupported === null) {
            liveUpdatesSupported = bridge.get('supportsLiveUpdates') === true;
        }

        return liveUpdatesSupported;
    }

    function supportsProgressUpdates() {
        return supportsLiveUpdates() && bridge.has('onGenerationProgress');
    }

    function handlesCompletion() {
        if (nativeCompletionSupported === null) {
            nativeCompletionSupported = bridge.has('supportsNativeCompletion')
                && bridge.get('supportsNativeCompletion') === true;
        }

        return nativeCompletionSupported;
    }

    return {
        supportsProgressUpdates,
        /** @param {number} outputTokens */
        reportProgress(outputTokens) {
            if (!supportsProgressUpdates()) {
                return false;
            }

            return bridge.call('onGenerationProgress', outputTokens);
        },
        handlesCompletion,
    };
}
